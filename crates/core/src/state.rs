//! 唯一可选的历史状态：`--state-dir` 下的新鲜度计数。
//!
//! 默认不发现、不创建、不读写任何目录；只有用户显式传入目录才启用 N=3。
//! 文件只保存不可逆摘要与非敏感时间，不存 URL 查询、接口名或地址。

use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(not(target_arch = "wasm32"))]
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

use crate::contracts::{Freshness, FreshnessStatus};
#[cfg(not(target_arch = "wasm32"))]
use crate::targets::ConfigError;

/// 状态文件 schema。
pub const STATE_SCHEMA: &str = "parallaxe.freshness/v1";
/// 计数饱和值。
pub const MISS_SATURATION: u64 = 3;
/// 展示窗口：超过 7 天只作历史。
pub const HISTORY_DAYS: i64 = 7;

/// 计数键的组成部分（全部来自注册表与本次运行，不含用户数据）。
#[derive(Debug, Clone)]
pub struct StaleKeyParts<'a> {
    pub target_id: &'a str,
    pub url: &'a str,
    pub surface: &'a str,
    pub rule_id: &'a str,
    pub rule_version: u64,
    pub family: &'a str,
    pub path_id: &'a str,
    pub shape: &'a str,
}

impl StaleKeyParts<'_> {
    /// 长度分隔后取 SHA-256，输出不可逆摘要。
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        for part in [
            self.target_id.to_string(),
            self.url.to_string(),
            self.surface.to_string(),
            self.rule_id.to_string(),
            self.rule_version.to_string(),
            self.family.to_string(),
            self.path_id.to_string(),
            self.shape.to_string(),
        ] {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        let digest = hasher.finalize();
        let mut text = String::with_capacity(digest.len() * 2);
        for byte in digest {
            text.push_str(&format!("{byte:02x}"));
        }
        text
    }
}

/// 状态条目（与架构 05 的字段对应）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub key_digest: String,
    pub rule_version: u64,
    pub last_hit_at: Option<String>,
    pub last_comparable_at: Option<String>,
    pub misses: u64,
    pub last_run_digest: Option<String>,
    pub quarantined: bool,
}

/// 状态文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub schema: String,
    pub entries: Vec<Entry>,
}

/// 新鲜度存储端口。
pub trait FreshnessStore: Send + Sync {
    /// 是否启用跨运行计数。
    fn enabled(&self) -> bool;
    /// 记录合格命中，返回 CURRENT。
    fn hit(&self, parts: &StaleKeyParts<'_>, now: OffsetDateTime) -> Freshness;
    /// 记录可比缺失；仅当启用且有历史命中时累加。
    fn miss(&self, parts: &StaleKeyParts<'_>, now: OffsetDateTime) -> Freshness;
    /// 存储错误（脱敏），供报告 reason 使用。
    fn error(&self) -> Option<String> {
        None
    }
}

/// 默认存储：不读不写，N=1，永不 stale。
#[derive(Debug, Default)]
pub struct NullStore {
    misses: Mutex<HashMap<String, u64>>,
}

impl FreshnessStore for NullStore {
    fn enabled(&self) -> bool {
        false
    }

    fn hit(&self, _parts: &StaleKeyParts<'_>, now: OffsetDateTime) -> Freshness {
        Freshness {
            status: FreshnessStatus::Current,
            last_hit_at: Some(format_time(now)),
            comparable_misses: 0,
        }
    }

    fn miss(&self, parts: &StaleKeyParts<'_>, _now: OffsetDateTime) -> Freshness {
        let key = parts.digest();
        let mut misses = self
            .misses
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let counter = misses.entry(key).or_insert(0);
        *counter = (*counter + 1).min(MISS_SATURATION);
        Freshness {
            status: FreshnessStatus::Missing,
            last_hit_at: None,
            comparable_misses: *counter,
        }
    }
}

/// 内存存储：供 selftest 注入虚拟状态目录。
#[derive(Debug, Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<String, Entry>>,
}

impl FreshnessStore for MemoryStore {
    fn enabled(&self) -> bool {
        true
    }

    fn hit(&self, parts: &StaleKeyParts<'_>, now: OffsetDateTime) -> Freshness {
        let key = parts.digest();
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        entries.insert(
            key,
            Entry {
                key_digest: parts.digest(),
                rule_version: parts.rule_version,
                last_hit_at: Some(format_time(now)),
                last_comparable_at: Some(format_time(now)),
                misses: 0,
                last_run_digest: None,
                quarantined: false,
            },
        );
        Freshness {
            status: FreshnessStatus::Current,
            last_hit_at: Some(format_time(now)),
            comparable_misses: 0,
        }
    }

    fn miss(&self, parts: &StaleKeyParts<'_>, now: OffsetDateTime) -> Freshness {
        let key = parts.digest();
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(entry) = entries.get_mut(&key) else {
            return Freshness {
                status: FreshnessStatus::Missing,
                last_hit_at: None,
                comparable_misses: 0,
            };
        };
        if entry.rule_version != parts.rule_version {
            *entry = Entry {
                key_digest: key.clone(),
                rule_version: parts.rule_version,
                last_hit_at: None,
                last_comparable_at: None,
                misses: 0,
                last_run_digest: None,
                quarantined: false,
            };
            return Freshness {
                status: FreshnessStatus::Missing,
                last_hit_at: None,
                comparable_misses: 0,
            };
        }
        if !within_window(entry.last_hit_at.as_deref(), now)
            || !within_window(entry.last_comparable_at.as_deref(), now)
        {
            entry.last_hit_at = None;
            entry.misses = 0;
            return Freshness {
                status: FreshnessStatus::Missing,
                last_hit_at: None,
                comparable_misses: 0,
            };
        }
        entry.misses = entry.misses.saturating_add(1).min(MISS_SATURATION);
        entry.last_comparable_at = Some(format_time(now));
        Freshness {
            status: if entry.misses >= MISS_SATURATION {
                FreshnessStatus::Stale
            } else {
                FreshnessStatus::Missing
            },
            last_hit_at: entry.last_hit_at.clone(),
            comparable_misses: entry.misses,
        }
    }
}

/// 目录存储：显式 `--state-dir` 才创建（wasm 无文件系统，不提供）。
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct DirStore {
    dir: PathBuf,
    // 在读取前取得且持有到运行结束；争用立即报错，不覆盖另一份快照。
    _lock: std::fs::File,
    state: Mutex<StateFile>,
    error: Mutex<Option<String>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DirStore {
    /// 打开或创建指定目录内的状态文件。
    pub fn open(dir: &Path) -> Result<Self, ConfigError> {
        let invalid = |message| ConfigError::field("--state-dir", message);
        let dir = crate::targets::checked_local_path(dir, true, "--state-dir")?;
        std::fs::create_dir_all(&dir).map_err(|_| invalid("目录不可创建"))?;
        let file = dir.join("freshness-v1.json");
        let lock_path = dir.join("freshness-v1.lock");
        crate::targets::check_entry(&lock_path, false, "--state-dir")?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|_| invalid("状态锁不可用"))?;
        lock.try_lock_exclusive()
            .map_err(|_| invalid("状态目录已被占用或不支持独占锁"))?;
        crate::targets::check_entry(&file, false, "--state-dir")?;
        let state = match std::fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<StateFile>(&text) {
                Ok(state) if state.schema == STATE_SCHEMA => state,
                Ok(_) => return Err(invalid("状态 schema 不识别")),
                Err(_) => return Err(invalid("状态文件损坏")),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StateFile {
                schema: STATE_SCHEMA.to_string(),
                entries: Vec::new(),
            },
            Err(_) => return Err(invalid("状态文件不可读")),
        };
        Ok(DirStore {
            dir,
            _lock: lock,
            state: Mutex::new(state),
            error: Mutex::new(None),
        })
    }

    fn file(&self) -> PathBuf {
        self.dir.join("freshness-v1.json")
    }

    fn persist(&self, state: &StateFile) -> Result<(), String> {
        let temp = self.dir.join("freshness-v1.json.tmp");
        // 不打开或截断预先存在的临时文件（包括硬链接）。
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|_| "状态临时文件冲突或不可写".to_string())?;
        let payload = serde_json::to_vec_pretty(state).map_err(|_| "状态序列化失败".to_string())?;
        file.write_all(&payload)
            .map_err(|_| "状态写入失败".to_string())?;
        file.sync_all().map_err(|_| "状态同步失败".to_string())?;
        drop(file);
        std::fs::rename(&temp, self.file()).map_err(|_| "状态原子替换失败".to_string())?;
        Ok(())
    }

    fn update<F>(&self, parts: &StaleKeyParts<'_>, now: OffsetDateTime, apply: F) -> Freshness
    where
        F: FnOnce(&mut Entry, &mut bool),
    {
        let key = parts.digest();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let index = state
            .entries
            .iter()
            .position(|entry| entry.key_digest == key);
        let mut entry = match index {
            Some(index) => state.entries[index].clone(),
            None => Entry {
                key_digest: key.clone(),
                rule_version: parts.rule_version,
                last_hit_at: None,
                last_comparable_at: None,
                misses: 0,
                last_run_digest: None,
                quarantined: false,
            },
        };
        if entry.rule_version != parts.rule_version {
            entry = Entry {
                key_digest: key.clone(),
                rule_version: parts.rule_version,
                last_hit_at: None,
                last_comparable_at: None,
                misses: 0,
                last_run_digest: None,
                quarantined: false,
            };
        }
        if !within_window(entry.last_hit_at.as_deref(), now)
            || !within_window(entry.last_comparable_at.as_deref(), now)
        {
            entry.last_hit_at = None;
            entry.misses = 0;
        }
        let mut stale = false;
        apply(&mut entry, &mut stale);
        if let Some(index) = index {
            state.entries[index] = entry.clone();
        } else {
            state.entries.push(entry.clone());
        }
        state
            .entries
            .retain(|item| within_window(item.last_hit_at.as_deref(), now) || item.misses > 0);
        if let Err(error) = self.persist(&state) {
            *self.error.lock().unwrap_or_else(|error| error.into_inner()) = Some(error);
            return Freshness::not_tracked();
        }
        Freshness {
            status: if stale {
                FreshnessStatus::Stale
            } else {
                FreshnessStatus::Missing
            },
            last_hit_at: entry.last_hit_at,
            comparable_misses: entry.misses,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FreshnessStore for DirStore {
    fn enabled(&self) -> bool {
        true
    }

    fn hit(&self, parts: &StaleKeyParts<'_>, now: OffsetDateTime) -> Freshness {
        let timestamp = format_time(now);
        let mut status = self.update(parts, now, move |entry, _| {
            entry.last_hit_at = Some(timestamp.clone());
            entry.last_comparable_at = Some(timestamp.clone());
            entry.misses = 0;
        });
        if status.status != FreshnessStatus::NotTracked {
            status.status = FreshnessStatus::Current;
        }
        status
    }

    fn miss(&self, parts: &StaleKeyParts<'_>, now: OffsetDateTime) -> Freshness {
        let timestamp = format_time(now);
        self.update(parts, now, move |entry, stale| {
            if entry.last_hit_at.is_none() {
                return;
            }
            entry.misses = entry.misses.saturating_add(1).min(MISS_SATURATION);
            entry.last_comparable_at = Some(timestamp.clone());
            if entry.misses >= MISS_SATURATION {
                *stale = true;
            }
        })
    }

    fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// 是否仍在 7 天窗口内（RFC 3339 UTC）。
pub fn within_window(last_hit: Option<&str>, now: OffsetDateTime) -> bool {
    let Some(text) = last_hit else {
        return false;
    };
    let Ok(parsed) = OffsetDateTime::parse(text, &Rfc3339) else {
        return false;
    };
    parsed <= now && now - parsed <= Duration::days(HISTORY_DAYS)
}

/// RFC 3339 UTC 文本。
pub fn format_time(now: OffsetDateTime) -> String {
    now.format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// 桌面端默认用户状态目录（永久授权唯一可写位置；绝不指向内置 `assets/`）。
///
/// - Windows：`%LOCALAPPDATA%\Parallaxe-box\user-state`
/// - macOS：`$HOME/Library/Application Support/Parallaxe-box/user-state`
/// - 其它：`$XDG_DATA_HOME/parallaxe-box/user-state` 或 `$HOME/.local/share/parallaxe-box/user-state`
///
/// 移动端与 wasm 返回 `None`：这两个平台不提供"永久生效"落盘能力，
/// GUI 会明确提示并把授权限制为「仅本次」。
pub fn default_user_state_dir() -> Option<std::path::PathBuf> {
    if cfg!(any(
        target_os = "android",
        target_os = "ios",
        target_arch = "wasm32"
    )) {
        return None;
    }
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| {
            std::path::PathBuf::from(h)
                .join("Library")
                .join("Application Support")
        })
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".local").join("share"))
            })
    }?;
    let app = if cfg!(target_os = "windows") || cfg!(target_os = "macos") {
        base.join("Parallaxe-box")
    } else {
        base.join("parallaxe-box")
    };
    Some(app.join("user-state"))
}
