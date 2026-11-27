//! A bounded, declarative plugin protocol. It has no filesystem or network host calls.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "parallaxe.plugin/v1";
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub license: String,
    pub source: String,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub operations: Vec<Operation>,
    #[serde(default)]
    pub pages: Vec<Page>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ReportCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Resolver { input: String, output: String },
    Hook { event: HookEvent, text: String },
    CoverageBadge { prefix: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    Start,
    Complete,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Component {
    Heading {
        text: String,
    },
    Badge {
        text: String,
    },
    Callout {
        text: String,
    },
    Markdown {
        text: String,
    },
    Code {
        text: String,
    },
    Progress {
        label: String,
        value: u8,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Form {
        label: String,
    },
    Button {
        label: String,
    },
}

fn valid_text(s: &str, max_chars: usize) -> bool {
    !s.trim().is_empty()
        && s.chars().count() <= max_chars
        && s.len() <= max_chars.saturating_mul(4)
        && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
}
fn check_text(context: &str, field: &str, s: &str, max_chars: usize) -> Result<(), String> {
    if valid_text(s, max_chars) {
        Ok(())
    } else {
        Err(format!(
            "{context}: field '{field}' is empty, contains control characters, or exceeds {max_chars} characters"
        ))
    }
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err("manifest too large".into());
        }
        let manifest: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        let source = url::Url::parse(&self.source).map_err(|_| "invalid source URL")?;
        if self.schema != SCHEMA {
            return Err(format!(
                "invalid plugin declaration: schema must be '{SCHEMA}'"
            ));
        }
        if !valid_id(&self.id) {
            return Err("invalid plugin declaration: field 'id' must be 1..=128 lowercase ASCII letters, digits, '-' or '.'".into());
        }
        check_text("invalid plugin declaration", "name", &self.name, 128)?;
        check_text("invalid plugin declaration", "version", &self.version, 64)?;
        check_text("invalid plugin declaration", "author", &self.author, 256)?;
        check_text("invalid plugin declaration", "license", &self.license, 128)?;
        if !matches!(source.scheme(), "http" | "https")
            || source.host_str().is_none()
            || !source.username().is_empty()
            || source.password().is_some()
        {
            return Err(
                "invalid plugin declaration: field 'source' must be an http/https URL without credentials"
                    .into(),
            );
        }
        if self.permissions.len() > 1 {
            return Err("invalid plugin declaration: at most 1 permission is allowed".into());
        }
        if self.operations.len() > 64 {
            return Err("invalid plugin declaration: operations exceed limit (64)".into());
        }
        if self.pages.len() > 16 {
            return Err("invalid plugin declaration: pages exceed limit (16)".into());
        }
        for (idx, operation) in self.operations.iter().enumerate() {
            match operation {
                Operation::Resolver { input, output } => {
                    check_text(&format!("invalid operation[{idx}]"), "input", input, 512)?;
                    check_text(&format!("invalid operation[{idx}]"), "output", output, 512)?;
                }
                Operation::Hook { text, .. } => {
                    check_text(&format!("invalid operation[{idx}]"), "text", text, 512)?;
                }
                Operation::CoverageBadge { prefix } => {
                    check_text(&format!("invalid operation[{idx}]"), "prefix", prefix, 512)?;
                    if !self.permissions.contains(&Permission::ReportCounts) {
                        return Err(format!(
                            "invalid operation[{idx}] or missing permission: coverage_badge requires 'report_counts'"
                        ));
                    }
                }
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for (page_idx, page) in self.pages.iter().enumerate() {
            if !valid_id(&page.id) {
                return Err(format!("invalid page[{page_idx}]: invalid page id"));
            }
            if !ids.insert(&page.id) {
                return Err(format!(
                    "invalid page[{page_idx}]: duplicate page id '{}'",
                    page.id
                ));
            }
            check_text(
                &format!("invalid page[{page_idx}]"),
                "title",
                &page.title,
                128,
            )?;
            if page.components.len() > 64 {
                return Err(format!(
                    "invalid page[{page_idx}]: components exceed limit (64)"
                ));
            }
            for (comp_idx, component) in page.components.iter().enumerate() {
                let ctx = format!("invalid page component[{page_idx}][{comp_idx}]");
                match component {
                    Component::Heading { text }
                    | Component::Badge { text }
                    | Component::Callout { text }
                    | Component::Markdown { text }
                    | Component::Code { text } => {
                        check_text(&ctx, "text", text, 4096)?;
                    }
                    Component::Progress { label, value } => {
                        check_text(&ctx, "label", label, 256)?;
                        if *value > 100 {
                            return Err(format!("{ctx}: field 'value' must be 0..=100"));
                        }
                    }
                    Component::Form { label } | Component::Button { label } => {
                        check_text(&ctx, "label", label, 256)?;
                    }
                    Component::Table { headers, rows } => {
                        if headers.is_empty() || headers.len() > 16 {
                            return Err(format!("{ctx}: headers must contain 1..=16 columns"));
                        }
                        if rows.len() > 64 {
                            return Err(format!("{ctx}: rows exceed limit (64)"));
                        }
                        for (h_idx, value) in headers.iter().enumerate() {
                            check_text(&ctx, &format!("headers[{h_idx}]"), value, 256)?;
                        }
                        for (r_idx, row) in rows.iter().enumerate() {
                            if row.len() != headers.len() {
                                return Err(format!(
                                    "{ctx}: rows[{r_idx}] width {} does not match headers {}",
                                    row.len(),
                                    headers.len()
                                ));
                            }
                            for (c_idx, value) in row.iter().enumerate() {
                                check_text(&ctx, &format!("rows[{r_idx}][{c_idx}]"), value, 256)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Validates and normalizes a 64-hex SHA-256 digest to lowercase.
pub fn normalize_sha256(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.len() != 64 || !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid sha256 digest format (expected 64 hex characters)".into());
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// Callers pass the digest explicitly approved for this exact manifest.
pub fn verified(bytes: &[u8], approved_digest: &str) -> Result<Manifest, String> {
    let normalized = normalize_sha256(approved_digest)?;
    if digest(bytes) != normalized {
        return Err("plugin content changed; approval required".into());
    }
    Manifest::parse(bytes)
}

#[derive(Debug, Default)]
pub struct Runtime {
    failures: u8,
    disabled: bool,
}

impl Runtime {
    pub fn invoke(
        &mut self,
        bytes: &[u8],
        approved_digest: &str,
        input: &str,
        event: Option<HookEvent>,
        counts: Option<(usize, usize)>,
    ) -> Result<Vec<String>, String> {
        if self.disabled {
            return Err("plugin circuit open".into());
        }
        let result = (|| {
            if input.len() > 4096 {
                return Err("input budget exceeded".into());
            }
            let manifest = verified(bytes, approved_digest)?;
            let mut output = Vec::new();
            for op in manifest.operations {
                match op {
                    Operation::Resolver {
                        input: expected,
                        output: value,
                    } if input == expected => output.push(value),
                    Operation::Hook {
                        event: expected,
                        text,
                    } if event == Some(expected) => output.push(text),
                    Operation::CoverageBadge { prefix } => {
                        if let Some((covered, selected)) = counts {
                            output.push(format!("{prefix}{covered}/{selected}"));
                        }
                    }
                    _ => {}
                }
            }
            Ok(output)
        })();
        if result.is_err() {
            self.failures = self.failures.saturating_add(1);
            self.disabled = self.failures >= 3;
        } else {
            self.failures = 0;
        }
        result
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_directory(dir: &std::path::Path) -> Result<(Vec<u8>, Manifest), String> {
    use std::io::Read;
    let path = dir.join("manifest.json");
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_MANIFEST_BYTES as u64 {
        return Err("invalid manifest file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let manifest = Manifest::parse(&bytes)?;
    Ok((bytes, manifest))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_enabled(
    settings: &crate::settings::Settings,
    id: &str,
) -> Result<(Vec<u8>, Manifest), String> {
    let expected = settings.plugins.get(id).ok_or("plugin is disabled")?;
    let directory = settings
        .plugin_directories
        .get(id)
        .ok_or("plugin directory missing")?;
    let (bytes, manifest) = read_directory(std::path::Path::new(directory))?;
    if manifest.id != id {
        return Err("plugin id changed".into());
    }
    verified(&bytes, expected)?;
    Ok((bytes, manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"schema":"parallaxe.plugin/v1","id":"example","name":"Example","version":"1","author":"Example","license":"MIT","source":"https://example.com/","permissions":["report_counts"],"operations":[{"type":"coverage_badge","prefix":"Coverage "}]}"#;

    #[test]
    fn permissions_digest_and_circuit_breaker() {
        let bytes = SAMPLE.as_bytes();
        let hash = digest(bytes);
        let mut runtime = Runtime::default();
        assert_eq!(
            runtime
                .invoke(bytes, &hash, "", None, Some((2, 3)))
                .unwrap(),
            vec!["Coverage 2/3"]
        );
        let missing_permission = SAMPLE.replace("[\"report_counts\"]", "[]");
        assert!(Manifest::parse(missing_permission.as_bytes()).is_err());
        assert!(Manifest::parse(SAMPLE.replace("report_counts", "network").as_bytes()).is_err());
        for _ in 0..3 {
            assert!(runtime.invoke(bytes, "wrong", "", None, None).is_err());
        }
        assert!(runtime.invoke(bytes, &hash, "", None, None).is_err());
    }

    #[test]
    fn char_limits_field_diagnostics_and_case_insensitive_sha256() {
        // 60 CJK chars (180 UTF-8 bytes) must pass the 128-char name limit.
        let long_cjk_name = "测".repeat(60);
        let manifest_json = format!(
            r#"{{"schema":"parallaxe.plugin/v1","id":"cjk-sample","name":"{long_cjk_name}","version":"1.0","author":"Example","license":"MIT","source":"https://example.com/"}}"#
        );
        let bytes = manifest_json.as_bytes();
        let upper_hash = digest(bytes).to_ascii_uppercase();
        assert!(verified(bytes, &upper_hash).is_ok());

        let format_err = verified(bytes, "not-a-hex-digest").unwrap_err();
        assert!(format_err.contains("invalid sha256 digest format"));

        let wrong_hash = "0".repeat(64);
        let mismatch_err = verified(bytes, &wrong_hash).unwrap_err();
        assert!(mismatch_err.contains("plugin content changed"));

        let too_long = "a".repeat(129);
        let bad_name = format!(
            r#"{{"schema":"parallaxe.plugin/v1","id":"bad-name","name":"{too_long}","version":"1.0","author":"Example","license":"MIT","source":"https://example.com/"}}"#
        );
        let err = Manifest::parse(bad_name.as_bytes()).unwrap_err();
        assert!(err.contains("name"), "expected field name in error: {err}");
    }
}
