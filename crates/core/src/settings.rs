//! Explicit, opt-in settings persistence. No file is discovered automatically.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub density: String,
    pub profile: String,
    pub rule_format: String,
    pub min_interval_ms: u64,
    pub inter_target_interval_ms: u64,
    pub batch_size: u32,
    pub batch_cooldown_ms: u64,
    /// 源码构建中经用户确认后启用的发行包模式；正式发行构建恒为开。
    pub distribution_mode: bool,
    pub plugins: std::collections::BTreeMap<String, String>,
    pub plugin_directories: std::collections::BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "zh-CN".into(),
            theme: "dark".into(),
            density: "normal".into(),
            profile: "simple".into(),
            rule_format: "clash".into(),
            min_interval_ms: 4_000,
            inter_target_interval_ms: 0,
            batch_size: 5,
            batch_cooldown_ms: 45000,
            distribution_mode: false,
            plugins: Default::default(),
            plugin_directories: Default::default(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        let limits = crate::targets::HARD_LIMITS;
        if !["zh-CN", "en-US"].contains(&self.language.as_str())
            || !["dark", "light"].contains(&self.theme.as_str())
            || !["normal", "compact"].contains(&self.density.as_str())
            || !["clash", "singbox", "domainlist"].contains(&self.rule_format.as_str())
            || self.profile.is_empty()
            || self.profile.len() > 128
            || self.min_interval_ms > limits.max_min_interval_ms
            || self.inter_target_interval_ms
                > crate::targets::HARD_LIMITS.max_inter_target_interval_ms
            || self.batch_size == 0
            || self.batch_size > limits.max_batch_size
            || self.batch_cooldown_ms > limits.max_batch_cooldown_ms
            || self.plugins.len() > 128
            || self.plugin_directories.len() > 128
            || self.plugin_directories.iter().any(|(id, path)| {
                !self.plugins.contains_key(id) || path.is_empty() || path.len() > 4096
            })
            || self.plugins.iter().any(|(id, hash)| {
                id.is_empty()
                    || id.len() > 128
                    || hash.len() != 64
                    || !hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err("invalid settings".into());
        }
        Ok(())
    }
}

// Complete records survive interrupted writes. The lock covers read-modify-write.
#[cfg(not(target_arch = "wasm32"))]
pub fn update<F>(path: &std::path::Path, change: F) -> Result<Settings, String>
where
    F: FnOnce(&mut Settings) -> Result<(), String>,
{
    use fs2::FileExt;
    use std::io::{Read, Seek, SeekFrom, Write};
    let existed = path.exists();
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        // 追加而不是覆盖：锁内先读完历史记录，再在末尾追加一条完整记录。
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.lock_exclusive().map_err(|e| e.to_string())?;
    let mut wrote = false;
    let result = (|| {
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("settings log too large".into());
        }
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err("incomplete settings record".into());
        }
        let mut settings = parse_log(&bytes)?;
        change(&mut settings)?;
        settings.validate()?;
        let mut record = serde_json::to_vec(&settings).map_err(|e| e.to_string())?;
        record.push(b'\n');
        if bytes.len() + record.len() > 8 * 1024 * 1024 {
            return Err("settings log too large".into());
        }
        file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        file.write_all(&record)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        wrote = true;
        Ok(settings)
    })();
    let _ = FileExt::unlock(&file);
    drop(file);
    if result.is_err() && !existed && !wrote {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Truncates a partially written trailing record back to the last complete valid record.
#[cfg(not(target_arch = "wasm32"))]
pub fn repair(path: &std::path::Path) -> Result<Settings, String> {
    use fs2::FileExt;
    use std::io::Read;
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(e.to_string()),
    };
    file.lock_exclusive().map_err(|e| e.to_string())?;
    let result = (|| {
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("settings log too large".into());
        }
        if bytes.is_empty() {
            return Ok(Settings::default());
        }
        let Some(last_nl) = bytes.iter().rposition(|b| *b == b'\n') else {
            return Err("no complete settings record to recover".into());
        };
        let complete_len = last_nl + 1;
        let recovered = parse_log(&bytes[..complete_len])?;
        if bytes.len() > complete_len {
            file.set_len(complete_len as u64)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Ok(recovered)
    })();
    let _ = FileExt::unlock(&file);
    result
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load(path: &std::path::Path) -> Result<Settings, String> {
    use fs2::FileExt;
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(e.to_string()),
    };
    FileExt::lock_shared(&file).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let read = std::io::Read::by_ref(&mut file)
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes);
    let _ = FileExt::unlock(&file);
    read.map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("settings log too large".into());
    }
    parse_log(&bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_log(bytes: &[u8]) -> Result<Settings, String> {
    let complete = match bytes.iter().rposition(|b| *b == b'\n') {
        Some(end) => &bytes[..end],
        None if bytes.is_empty() => return Ok(Settings::default()),
        None => return Err("incomplete settings record".into()),
    };
    let line = complete
        .rsplit(|b| *b == b'\n')
        .next()
        .ok_or("missing settings record")?;
    let settings: Settings = serde_json::from_slice(line).map_err(|e| e.to_string())?;
    settings.validate()?;
    Ok(settings)
}
