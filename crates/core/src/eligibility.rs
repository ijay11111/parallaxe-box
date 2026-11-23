//! 静态资格说明（E-1）：只解释“访问可达 ≠ 订阅/完整功能可用”。
//!
//! 本模块不发起任何请求、不读取账号/Cookie/付款/家庭组状态，也不参与 verdict。
//! 数据来自 `assets/eligibility-notes.toml`，每条都带官方链接与 `verified=false`。

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::targets::ConfigError;

/// 资格说明类别；当前只有“订阅/功能前置闸门”。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EligibilityCategory {
    EntitlementGate,
}

/// 报告里的可选资格说明（`verified` 永远为 false）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EligibilityNote {
    pub category: EligibilityCategory,
    pub summary_key: String,
    pub source_url: String,
    pub verified: bool,
}

impl EligibilityNote {
    /// 按 locale 渲染一句可直接展示的说明；GUI 只读调用，不触发任何探测。
    pub fn summary(&self, locale: crate::i18n::Locale) -> String {
        crate::i18n::translate(locale, &self.summary_key, &[])
    }
}

#[derive(Debug, Clone)]
struct CatalogEntry {
    id: String,
    note: EligibilityNote,
    families: Vec<String>,
    targets: Vec<String>,
}

/// 只读的资格说明目录。
#[derive(Debug, Clone, Default)]
pub struct EligibilityCatalog {
    entries: Vec<CatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct RawFile {
    schema: String,
    #[serde(default)]
    notes: Vec<RawNote>,
}

#[derive(Debug, Deserialize)]
struct RawNote {
    id: String,
    category: EligibilityCategory,
    summary_key: String,
    source_url: String,
    #[serde(default)]
    families: Vec<String>,
    #[serde(default)]
    targets: Vec<String>,
}

impl EligibilityCatalog {
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let raw: RawFile = toml::from_str(text).map_err(|error| {
            ConfigError::Parse(format!(
                "eligibility-notes.toml: {}",
                error.to_string().lines().next().unwrap_or_default()
            ))
        })?;
        if raw.schema != "parallaxe.eligibility/v1" {
            return Err(ConfigError::field(
                "eligibility-notes.toml#schema",
                "只接受 parallaxe.eligibility/v1",
            ));
        }
        let mut entries = Vec::with_capacity(raw.notes.len());
        let mut seen = HashSet::new();
        for note in raw.notes {
            if note.id.trim().is_empty() {
                return Err(ConfigError::field(
                    "eligibility-notes.toml#id",
                    "id 不能为空",
                ));
            }
            if !seen.insert(note.id.clone()) {
                return Err(ConfigError::field(
                    format!("eligibility-notes.toml#{}", note.id),
                    "id 重复",
                ));
            }
            if !note.summary_key.starts_with("eligibility.") {
                return Err(ConfigError::field(
                    format!("eligibility-notes.toml#{}", note.id),
                    "summary_key 必须以 eligibility. 开头",
                ));
            }
            if !note.source_url.starts_with("https://") {
                return Err(ConfigError::field(
                    format!("eligibility-notes.toml#{}", note.id),
                    "source_url 必须是 https:// 官方链接",
                ));
            }
            if note.families.is_empty() && note.targets.is_empty() {
                return Err(ConfigError::field(
                    format!("eligibility-notes.toml#{}", note.id),
                    "families 与 targets 不能同时为空",
                ));
            }
            entries.push(CatalogEntry {
                id: note.id,
                note: EligibilityNote {
                    category: note.category,
                    summary_key: note.summary_key,
                    source_url: note.source_url,
                    // 本软件不验证账号状态，数据文件不能把它改成 true。
                    verified: false,
                },
                families: note.families,
                targets: note.targets,
            });
        }
        Ok(Self { entries })
    }

    /// 某目标适用的全部资格说明（按 families 或显式 targets 命中）。
    /// family 规则只作用于 Google 命名空间，避免把 Google One 闸门误套到 ChatGPT 等对照目标。
    pub fn matching(&self, target_id: &str, family: Option<&str>) -> Vec<EligibilityNote> {
        let google_target = target_id.starts_with("google.");
        self.entries
            .iter()
            .filter(|entry| {
                entry.targets.iter().any(|target| target == target_id)
                    || (google_target
                        && family.is_some_and(|family| {
                            entry.families.iter().any(|candidate| candidate == family)
                        }))
            })
            .map(|entry| entry.note.clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 目录覆盖的服务族（供测试断言 12 族全覆盖）。
    pub fn covered_families(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .flat_map(|entry| entry.families.iter().cloned())
            .collect()
    }

    /// 全部摘要 key（供测试核对 zh-CN/en-US 语言包完整）。
    pub fn summary_keys(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .map(|entry| entry.note.summary_key.clone())
            .collect()
    }

    pub fn ids(&self) -> BTreeSet<String> {
        self.entries.iter().map(|entry| entry.id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMBEDDED: &str = include_str!("../../../assets/eligibility-notes.toml");

    #[test]
    fn embedded_catalog_is_valid_and_verified_is_false() {
        let catalog = EligibilityCatalog::parse(EMBEDDED).expect("内置资格说明可解析");
        assert!(catalog.len() >= 18, "覆盖条目过少: {}", catalog.len());
        for note in catalog.matching("google.gemini", Some("ai")) {
            assert!(!note.verified);
            assert!(note.source_url.starts_with("https://"));
        }
    }

    #[test]
    fn duplicate_or_insecure_source_is_rejected() {
        let insecure = EMBEDDED.replace(
            "https://support.google.com/googleone/answer/9080668",
            "http://example.invalid/help",
        );
        assert!(EligibilityCatalog::parse(&insecure).is_err());
    }
}
