//! 统一导入包（`parallaxe.import/v1`）：把"自定义网址"与"自定义检查方式"合并为一体，
//! 支持手动表单、文本文件（txt / md / csv）与 ZIP 导入包三种入口共用同一解析、校验、预览与原子导入引擎。
//!
//! 安全与合规红线：
//! - 严禁改变任何核心判定逻辑、内置目录或内置 `assets/`；
//! - 所有导入目标默认 `allow_online=false`、`safe_default=false`、`origin=USER_IMPORTED`，未获 F3 一次性会话授权前严格零发包；
//! - 责任声明分层强制：含目标必须勾选网址声明，含规则必须额外勾选更严格的检查方式声明，未勾选则零写入拒绝；
//! - 原子导入：先在克隆快照中完整校验并执行导入，任一条目失败整体回滚；
//! - ZIP 严防路径穿越、符号链接、非法文件类型、单包超 10 MiB、解压总字节超 50 MiB（防 ZIP 炸弹）与脚本执行。

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use flate2::read::DeflateDecoder;
use serde::{Deserialize, Serialize};

use crate::contracts::{LIABILITY_CONSENT_VERSION, ORIGIN_USER_IMPORTED};
use crate::targets::{
    normalize_user_group, ConfigError, Registry, Rule, UserRuleMeta, UserTargetImportInput,
    UserTargetMeta, DEFAULT_USER_GROUP,
};

/// 统一导入包 schema 版本。
pub const IMPORT_BUNDLE_SCHEMA: &str = "parallaxe.import/v1";
/// 发行包内预置的默认规则包（编译期嵌入；用户无需自己找 zip）。
pub const EMBEDDED_DEFAULT_RULES_BUNDLE: &[u8] =
    include_bytes!("../../../assets/bundled/default-rules-v2.zip");
/// ZIP 单包大小上限（10 MiB）。
pub const MAX_ZIP_BYTES: u64 = 10 * 1024 * 1024;
/// ZIP 解压后总字节上限（50 MiB，含流式解压硬截断防炸弹）。
pub const MAX_ZIP_EXTRACTED_BYTES: u64 = 50 * 1024 * 1024;
/// 单个文本/JSON 文件读取上限（4 MiB）。
pub const MAX_TEXT_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// 导入来源类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ImportSource {
    Manual,
    TextFile,
    ZipBundle,
}

impl ImportSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportSource::Manual => "MANUAL",
            ImportSource::TextFile => "TEXT_FILE",
            ImportSource::ZipBundle => "ZIP_BUNDLE",
        }
    }

    pub fn zh_label(self) -> &'static str {
        match self {
            ImportSource::Manual => "手动表单",
            ImportSource::TextFile => "文本文件",
            ImportSource::ZipBundle => "ZIP 导入包",
        }
    }
}

/// 导入包内的单个目标条目（绑定专属 `signals` 为一等公民，支持可选用户自建分组）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportTargetEntry {
    #[serde(default)]
    pub id: Option<String>,
    pub url: String,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub signals: Vec<String>,
    #[serde(default)]
    pub allow_insecure_http: bool,
    /// 可选用户自主分组名（默认归入"我的导入"；独立于 google/control；仅影响展示与筛选）。
    #[serde(default, alias = "group")]
    pub user_group: Option<String>,
    /// 当网址与内置目录（或已有用户目标）重复时，是否允许"更换"。
    /// 默认 `false`（保持硬拒绝）；批量预览中由用户逐条确认后置 `true`。
    #[serde(default)]
    pub replace_existing: bool,
}

/// 导入包责任声明留痕记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportConsentRecord {
    pub target_notice_version: String,
    pub rule_notice_version: String,
    #[serde(default)]
    pub accepted_at: Option<String>,
    #[serde(default)]
    pub target_accepted: bool,
    #[serde(default)]
    pub rule_accepted: bool,
}

impl Default for ImportConsentRecord {
    fn default() -> Self {
        Self {
            target_notice_version: LIABILITY_CONSENT_VERSION.to_string(),
            rule_notice_version: LIABILITY_CONSENT_VERSION.to_string(),
            accepted_at: None,
            target_accepted: false,
            rule_accepted: false,
        }
    }
}

/// 统一导入包（`parallaxe.import/v1`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportBundle {
    pub schema: String,
    pub source: ImportSource,
    #[serde(default)]
    pub targets: Vec<ImportTargetEntry>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub readme: Option<String>,
    #[serde(default)]
    pub consent: ImportConsentRecord,
}

impl ImportBundle {
    pub fn new(source: ImportSource) -> Self {
        Self {
            schema: IMPORT_BUNDLE_SCHEMA.to_string(),
            source,
            targets: Vec::new(),
            rules: Vec::new(),
            readme: None,
            consent: ImportConsentRecord::default(),
        }
    }

    /// 本导入包是否包含待导入网址（需勾选网址责任声明）。
    pub fn requires_target_consent(&self) -> bool {
        !self.targets.is_empty()
    }

    /// 本导入包是否包含随包自定义检查方式（需额外勾选更严格的检查方式责任声明）。
    pub fn requires_rule_consent(&self) -> bool {
        !self.rules.is_empty()
    }
}

/// 预览表中的单条规则校验状态。
#[derive(Debug, Clone)]
pub struct BundleRulePreviewRow {
    pub id: String,
    pub kind: String,
    pub pattern: String,
    pub valid: bool,
    pub error: Option<String>,
}

/// 预览表中的单条目标校验与绑定状态（"谁用谁"一目了然）。
#[derive(Debug, Clone)]
pub struct BundleTargetPreviewRow {
    pub index: usize,
    pub planned_id: String,
    pub url: String,
    pub surface: String,
    pub user_group: String,
    /// `(rule_id, is_new_in_bundle)`
    pub bound_signals: Vec<(String, bool)>,
    pub valid: bool,
    pub error: Option<String>,
    /// 与目录中已有目标重复时，记录冲突目标 ID（内置或用户目标）。
    pub duplicate_of: Option<String>,
    /// 冲突目标是内置受保护目标（批量更换需二次明确确认）。
    pub duplicate_protected: bool,
    /// 冲突目标是此前导入的用户目标。
    pub duplicate_is_user: bool,
}

/// 统一导入前只读预览结果（不修改真实 `Registry`）。
#[derive(Debug, Clone)]
pub struct BundlePreviewReport {
    pub source: ImportSource,
    pub rules: Vec<BundleRulePreviewRow>,
    pub targets: Vec<BundleTargetPreviewRow>,
    pub requires_target_consent: bool,
    pub requires_rule_consent: bool,
    pub all_valid: bool,
    pub summary_error: Option<String>,
}

/// 原子导入完成后的审计报告。
#[derive(Debug, Clone, Serialize)]
pub struct BundleImportReport {
    pub schema: String,
    pub source: ImportSource,
    pub origin: String,
    pub consent_version: String,
    pub imported_rules: Vec<UserRuleMeta>,
    pub imported_targets: Vec<UserTargetMeta>,
}

/// 解析 `parallaxe.import/v1` JSON 为 `ImportBundle`（拒绝越权许可字段）。
pub fn parse_import_bundle_json(json_text: &str) -> Result<ImportBundle, ConfigError> {
    let raw: serde_json::Value = serde_json::from_str(json_text)
        .map_err(|e| ConfigError::Parse(format!("导入包 JSON 解析失败：{e}")))?;
    reject_forbidden_json_keys(&raw, "manifest")?;
    let mut bundle: ImportBundle = serde_json::from_value(raw)
        .map_err(|e| ConfigError::Parse(format!("导入包字段格式错误：{e}")))?;
    if bundle.schema.trim().is_empty() {
        bundle.schema = IMPORT_BUNDLE_SCHEMA.to_string();
    }
    if bundle.schema != IMPORT_BUNDLE_SCHEMA {
        return Err(ConfigError::field(
            "schema",
            format!(
                "不支持的导入包 schema '{}'（仅支持 {IMPORT_BUNDLE_SCHEMA}）",
                bundle.schema
            ),
        ));
    }
    // 强制重置 JSON 内自带的 consent 标记，必须由当前会话用户在界面主动勾选！
    bundle.consent.target_accepted = false;
    bundle.consent.rule_accepted = false;
    bundle.consent.accepted_at = None;
    Ok(bundle)
}

fn reject_forbidden_json_keys(val: &serde_json::Value, path: &str) -> Result<(), ConfigError> {
    const FORBIDDEN: [&str; 5] = [
        "admission",
        "grants",
        "allow_online",
        "safe_default",
        "limits",
    ];
    match val {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if FORBIDDEN.contains(&k.as_str()) {
                    return Err(ConfigError::field(
                        format!("{path}.{k}"),
                        format!("导入包严禁包含许可或越权字段 '{k}'"),
                    ));
                }
                reject_forbidden_json_keys(v, &format!("{path}.{k}"))?;
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                reject_forbidden_json_keys(item, &format!("{path}[{i}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 解析纯文本 / Markdown / CSV 导入内容为统一 `ImportBundle`。
///
/// 支持格式：
/// - 分组标题行：`[分组名]` 或 `分组：分组名` / `group: 分组名`（作用于后续未单独指定分组的条目）
/// - 竖线分隔（首选）：`URL [| 规则ID1,规则ID2] [| ID] [| 表面说明] [| 分组名]`
/// - `组名 | URL [| 规则ID1,规则ID2] [| 表面说明]`（当首列以 `@` 开头或含中文/空格时视为分组名）
/// - 兼容旧版顺序：`自定义ID | URL [| 规则ID1,规则ID2] [| 表面说明] [| 分组名]`
/// - Markdown 表格行：`| URL | 规则ID1,规则ID2 | ID | 表面说明 | 分组名 |`（自动跳过表头与 `|---|` 分隔行）
/// - Markdown 代码块围栏（`` ``` ``）与 `#` 注释行自动忽略
/// - CSV 行：`URL,规则ID1;规则ID2,ID,表面说明,分组名`（当不含 `|` 且首列为 `http(s)://` 时）
/// - 若某行省略规则，采用 `default_signals`；若默认也为空，则绑定 `rule.http`（仅连通性状态码检查）。
pub fn parse_import_text(
    text: &str,
    default_signals: &[String],
    allow_insecure_http: bool,
) -> Result<ImportBundle, ConfigError> {
    parse_import_text_with_default_group(text, default_signals, allow_insecure_http, None)
}

/// 同 `parse_import_text`，并可指定未显式声明分组时的默认用户分组名。
pub fn parse_import_text_with_default_group(
    text: &str,
    default_signals: &[String],
    allow_insecure_http: bool,
    default_group: Option<&str>,
) -> Result<ImportBundle, ConfigError> {
    let mut bundle = ImportBundle::new(ImportSource::TextFile);
    let fallback_signals: Vec<String> = if default_signals.is_empty() {
        vec!["rule.http".to_string()]
    } else {
        default_signals.to_vec()
    };
    let mut current_group: Option<String> = default_group
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    for (line_idx, raw_line) in text.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("```")
            || trimmed.starts_with("---")
            || trimmed.starts_with(">")
        {
            continue;
        }
        // 支持 `[分组名]`、`分组：分组名`、`group: 分组名` 独立分组指令行
        if let Some(section_group) = parse_group_header_line(trimmed) {
            current_group = Some(section_group);
            continue;
        }
        // 去掉 Markdown 表格首尾 `|` 并跳过分隔线（如 `| --- | --- |`）或纯表头行
        let cleaned = trimmed.trim_start_matches('|').trim_end_matches('|').trim();
        if cleaned.is_empty()
            || cleaned
                .chars()
                .all(|c| matches!(c, '-' | ':' | '|' | ' ' | '\t'))
        {
            continue;
        }
        let lower_first = cleaned
            .split(['|', ','])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if matches!(
            lower_first.as_str(),
            "url" | "网址" | "目标网址" | "target" | "目标" | "group" | "分组" | "组名"
        ) {
            continue;
        }

        let entry = parse_single_text_line(
            cleaned,
            &fallback_signals,
            allow_insecure_http,
            current_group.as_deref(),
            line_idx + 1,
        )?;
        bundle.targets.push(entry);
    }

    if bundle.targets.is_empty() {
        return Err(ConfigError::field(
            "text",
            "未解析到任何有效网址条目（格式：URL | 规则1,规则2 | 名称 | 说明 | 分组）",
        ));
    }
    Ok(bundle)
}

fn parse_group_header_line(trimmed: &str) -> Option<String> {
    if let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let name = inner.trim();
        if !name.is_empty() && !name.contains("http://") && !name.contains("https://") {
            return Some(name.to_string());
        }
    }
    for prefix in ["group:", "GROUP:", "分组:", "分组：", "组名:", "组名："] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn looks_like_ascii_target_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn parse_single_text_line(
    line: &str,
    fallback_signals: &[String],
    allow_insecure_http: bool,
    current_group: Option<&str>,
    line_no: usize,
) -> Result<ImportTargetEntry, ConfigError> {
    let parts: Vec<String> = if line.contains('|') {
        line.split('|').map(|s| s.trim().to_string()).collect()
    } else if line.contains(',') && (line.starts_with("https://") || line.starts_with("http://")) {
        line.split(',').map(|s| s.trim().to_string()).collect()
    } else {
        vec![line.to_string()]
    };

    let looks_like_url = |s: &str| {
        let low = s.to_ascii_lowercase();
        low.starts_with("https://") || low.starts_with("http://")
    };

    let (url, raw_rules, id, surface, explicit_group) = if looks_like_url(&parts[0]) {
        // 标准格式：URL [| rules] [| id] [| surface] [| user_group]
        let url = parts[0].clone();
        let raw_rules = parts.get(1).cloned().unwrap_or_default();
        let id = parts.get(2).cloned().filter(|s| !s.is_empty());
        let surface = parts.get(3).cloned().filter(|s| !s.is_empty());
        let group = parts.get(4).cloned().filter(|s| !s.is_empty());
        (url, raw_rules, id, surface, group)
    } else if parts.len() >= 2 && looks_like_url(&parts[1]) {
        // 首列在 URL 之前：支持 `@组名 | URL` 或中文/含空格 `组名 | URL`，兼容旧版 ASCII `ID | URL`
        let col0 = parts[0].as_str();
        let is_group_prefix =
            col0.starts_with('@') || (!col0.is_empty() && !looks_like_ascii_target_id(col0));
        if is_group_prefix {
            let group_name = col0.trim_start_matches('@').trim().to_string();
            let group = (!group_name.is_empty()).then_some(group_name);
            let url = parts[1].clone();
            let raw_rules = parts.get(2).cloned().unwrap_or_default();
            let surface = parts.get(3).cloned().filter(|s| !s.is_empty());
            let id = parts.get(4).cloned().filter(|s| !s.is_empty());
            (url, raw_rules, id, surface, group)
        } else {
            // 兼容旧版：ID | URL [| rules] [| surface] [| user_group]
            let id = Some(parts[0].clone()).filter(|s| !s.is_empty());
            let url = parts[1].clone();
            let raw_rules = parts.get(2).cloned().unwrap_or_default();
            let surface = parts.get(3).cloned().filter(|s| !s.is_empty());
            let group = parts.get(4).cloned().filter(|s| !s.is_empty());
            (url, raw_rules, id, surface, group)
        }
    } else {
        // 仍保留原样交给核心 URL 校验器报精确错误（供预览表标红展示）
        let url = parts[0].clone();
        let raw_rules = parts.get(1).cloned().unwrap_or_default();
        let id = parts.get(2).cloned().filter(|s| !s.is_empty());
        let surface = parts.get(3).cloned().filter(|s| !s.is_empty());
        let group = parts.get(4).cloned().filter(|s| !s.is_empty());
        if url.is_empty() {
            return Err(ConfigError::field(format!("第 {line_no} 行"), "网址列为空"));
        }
        (url, raw_rules, id, surface, group)
    };

    let signals: Vec<String> = if raw_rules.trim().is_empty() {
        fallback_signals.to_vec()
    } else {
        let parsed: Vec<String> = raw_rules
            .split([',', ';', ' '])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if parsed.is_empty() {
            fallback_signals.to_vec()
        } else {
            parsed
        }
    };

    let user_group = explicit_group.or_else(|| current_group.map(str::to_string));

    Ok(ImportTargetEntry {
        id,
        url,
        surface,
        signals,
        allow_insecure_http,
        user_group,
        replace_existing: false,
    })
}

/// 从本地文本文件（`.txt` / `.md` / `.csv` / `.json`）读取并解析为统一 `ImportBundle`。
pub fn read_import_text_file(
    path: &Path,
    default_signals: &[String],
    allow_insecure_http: bool,
) -> Result<ImportBundle, ConfigError> {
    let raw_str = path.to_string_lossy().replace('\\', "/");
    if raw_str.starts_with("//") {
        return Err(ConfigError::field(
            "path",
            "仅支持本机文件路径，拒绝网络共享路径",
        ));
    }
    let meta = std::fs::metadata(path)
        .map_err(|e| ConfigError::Io(format!("无法读取导入文件 {}：{e}", path.display())))?;
    if !meta.is_file() {
        return Err(ConfigError::field("path", "指定路径不是常规文件"));
    }
    if meta.len() > MAX_TEXT_FILE_BYTES {
        return Err(ConfigError::field(
            "path",
            format!(
                "文本文件超过 {} MiB 上限",
                MAX_TEXT_FILE_BYTES / (1024 * 1024)
            ),
        ));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(ext.as_str(), "txt" | "md" | "csv" | "json") {
        return Err(ConfigError::field(
            "path",
            format!("不支持的文件扩展名 '.{ext}'（仅支持 .txt / .md / .csv / .json）"),
        ));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::Io(format!("读取文本文件失败（需 UTF-8 编码）：{e}")))?;
    if ext == "json" {
        let mut bundle = parse_import_bundle_json(&content)?;
        bundle.source = ImportSource::TextFile;
        Ok(bundle)
    } else {
        parse_import_text(&content, default_signals, allow_insecure_http)
    }
}

/// 自动清理临时解压目录的 RAII 守卫。
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 从本地 `.zip` 导入包安全解压至临时目录并解析为统一 `ImportBundle`。
///
/// 安全检查：
/// 1. 拒绝网络共享路径与单包超 10 MiB；
/// 2. 拒绝路径穿越（`..`、绝对路径、盘符、反斜杠、空字节）与 Unix 符号链接条目；
/// 3. 仅允许白名单文件：`manifest.json`、`targets.txt`/`targets.md`/`targets.csv`/`targets.json`、`rules.toml`、`README.md`；
/// 4. 流式解压硬截断：解压后总字节严格 `<= 50 MiB`（防 ZIP 炸弹）并逐文件校验 CRC-32；
/// 5. 不执行任何脚本；`README.md` 仅作只读展示；临时目录退出函数自动删除。
pub fn read_import_zip(path: &Path) -> Result<ImportBundle, ConfigError> {
    let raw_str = path.to_string_lossy().replace('\\', "/");
    if raw_str.starts_with("//") {
        return Err(ConfigError::field(
            "zip",
            "仅支持本机 ZIP 文件路径，拒绝网络共享路径",
        ));
    }
    let meta = std::fs::metadata(path)
        .map_err(|e| ConfigError::Io(format!("无法读取 ZIP 文件 {}：{e}", path.display())))?;
    if !meta.is_file() {
        return Err(ConfigError::field("zip", "指定路径不是常规文件"));
    }
    if meta.len() > MAX_ZIP_BYTES {
        return Err(ConfigError::field(
            "zip",
            format!(
                "ZIP 导入包大小 {} 字节超过 {} MiB 安全上限",
                meta.len(),
                MAX_ZIP_BYTES / (1024 * 1024)
            ),
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| ConfigError::Io(format!("读取 ZIP 文件失败：{e}")))?;
    parse_import_zip_bytes(&bytes)
}

/// 解析内存中的 ZIP 导入包字节（并在临时目录中完成受限解包与解析，自动清理临时目录）。
pub fn parse_import_zip_bytes(bytes: &[u8]) -> Result<ImportBundle, ConfigError> {
    if bytes.len() as u64 > MAX_ZIP_BYTES {
        return Err(ConfigError::field(
            "zip",
            format!(
                "ZIP 导入包大小 {} 字节超过 {} MiB 安全上限",
                bytes.len(),
                MAX_ZIP_BYTES / (1024 * 1024)
            ),
        ));
    }

    let extracted = extract_zip_entries_safe(bytes)?;
    if extracted.is_empty() {
        return Err(ConfigError::field("zip", "ZIP 导入包内未包含任何有效文件"));
    }

    // 落地至独立临时目录校验后再读取（满足临时目录安全解压约定，函数结束自动清除）
    let temp_dir = std::env::temp_dir().join(format!(
        "parallaxe-import-{}-{}",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| ConfigError::Io(format!("无法创建临时解压目录：{e}")))?;
    let _guard = TempDirGuard(temp_dir.clone());

    let mut files_by_role: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (canonical_name, data) in extracted {
        let out_path = temp_dir.join(&canonical_name);
        std::fs::write(&out_path, &data)
            .map_err(|e| ConfigError::Io(format!("写入临时解包文件失败：{e}")))?;
        files_by_role.insert(canonical_name, out_path);
    }

    let mut bundle = if let Some(manifest_path) = files_by_role.get("manifest.json") {
        let text = std::fs::read_to_string(manifest_path)
            .map_err(|e| ConfigError::Parse(format!("读取 manifest.json 失败：{e}")))?;
        let mut b = parse_import_bundle_json(&text)?;
        b.source = ImportSource::ZipBundle;
        b
    } else {
        ImportBundle::new(ImportSource::ZipBundle)
    };

    // 可选 rules.toml：与 signal-rules.toml [[rules]] 同格式
    if let Some(rules_path) = files_by_role.get("rules.toml") {
        let text = std::fs::read_to_string(rules_path)
            .map_err(|e| ConfigError::Parse(format!("读取 rules.toml 失败：{e}")))?;
        let parsed_rules = parse_rules_toml_standalone(&text)?;
        bundle.rules.extend(parsed_rules);
    }

    // 可选 targets.json / targets.txt / targets.md / targets.csv
    // 同一导入包同时提供多种目标文件时，`targets.json` 为权威格式，
    // 不再把 txt/md/csv 的同一批目标重复追加（避免 107 条变 214 条）。
    let mut json_targets_loaded = false;
    if let Some(tjson_path) = files_by_role.get("targets.json") {
        let text = std::fs::read_to_string(tjson_path)
            .map_err(|e| ConfigError::Parse(format!("读取 targets.json 失败：{e}")))?;
        let raw: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| ConfigError::Parse(format!("targets.json 解析失败：{e}")))?;
        reject_forbidden_json_keys(&raw, "targets.json")?;
        let list: Vec<ImportTargetEntry> = if raw.is_array() {
            serde_json::from_value(raw)
                .map_err(|e| ConfigError::Parse(format!("targets.json 数组格式错误：{e}")))?
        } else if let Some(arr) = raw.get("targets") {
            serde_json::from_value(arr.clone())
                .map_err(|e| ConfigError::Parse(format!("targets.json targets 字段错误：{e}")))?
        } else {
            return Err(ConfigError::field(
                "targets.json",
                "targets.json 必须为目标数组或包含 targets 数组的对象",
            ));
        };
        bundle.targets.extend(list);
        json_targets_loaded = true;
    }

    if !json_targets_loaded {
        for text_key in ["targets.txt", "targets.md", "targets.csv"] {
            if let Some(tpath) = files_by_role.get(text_key) {
                let text = std::fs::read_to_string(tpath)
                    .map_err(|e| ConfigError::Parse(format!("读取 {text_key} 失败：{e}")))?;
                let sub = parse_import_text(&text, &[], false)?;
                bundle.targets.extend(sub.targets);
            }
        }
    }

    // 最终防御：同 ID 或同 URL 只保留首次出现（顺序稳定），绝不静默重复导入。
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();
    let mut seen_urls: BTreeSet<String> = BTreeSet::new();
    bundle.targets.retain(|entry| {
        let id_ok = entry
            .id
            .as_deref()
            .map(|id| seen_ids.insert(id.trim().to_string()))
            .unwrap_or(true);
        let url_ok = seen_urls.insert(entry.url.trim().to_string());
        id_ok && url_ok
    });

    // 可选 README.md（只读展示，不执行）
    if let Some(readme_path) = files_by_role.get("readme.md") {
        let text = std::fs::read_to_string(readme_path)
            .map_err(|e| ConfigError::Parse(format!("读取 README.md 失败：{e}")))?;
        bundle.readme = Some(text);
    }

    if bundle.targets.is_empty() && bundle.rules.is_empty() {
        return Err(ConfigError::field(
            "zip",
            "ZIP 导入包未包含任何目标（targets.* / manifest.json）或规则（rules.toml）",
        ));
    }
    Ok(bundle)
}

/// 解析发行包内预置的默认规则包（编译期嵌入，零外部文件依赖）。
pub fn bundled_default_rules_bundle() -> Result<ImportBundle, ConfigError> {
    parse_import_zip_bytes(EMBEDDED_DEFAULT_RULES_BUNDLE)
}

#[derive(Deserialize)]
struct StandaloneRulesToml {
    #[serde(default)]
    rules: Vec<Rule>,
}

/// 解析 `rules.toml` 为 `Vec<Rule>`（严禁越权字段）。
pub fn parse_rules_toml_standalone(toml_text: &str) -> Result<Vec<Rule>, ConfigError> {
    let raw_val: toml::Value = toml::from_str(toml_text)
        .map_err(|e| ConfigError::Parse(format!("rules.toml 解析失败：{e}")))?;
    if let Some(table) = raw_val.as_table() {
        for forbidden in [
            "admission",
            "grants",
            "allow_online",
            "safe_default",
            "limits",
        ] {
            if table.contains_key(forbidden) {
                return Err(ConfigError::field(
                    forbidden,
                    format!("rules.toml 严禁包含许可或越权字段 '{forbidden}'"),
                ));
            }
        }
        if let Some(arr) = table.get("rules").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(rt) = item.as_table() {
                    for forbidden in ["admission", "grants", "allow_online", "safe_default"] {
                        if rt.contains_key(forbidden) {
                            return Err(ConfigError::field(
                                forbidden,
                                format!("rules.toml 条目严禁包含许可字段 '{forbidden}'"),
                            ));
                        }
                    }
                }
            }
        }
    }
    let parsed: Vec<Rule> = if raw_val.get("rules").is_some() {
        let file: StandaloneRulesToml = toml::from_str(toml_text)
            .map_err(|e| ConfigError::Parse(format!("rules.toml [[rules]] 格式错误：{e}")))?;
        file.rules
    } else {
        let single: Rule = toml::from_str(toml_text)
            .map_err(|e| ConfigError::Parse(format!("rules.toml 单条规则格式错误：{e}")))?;
        vec![single]
    };
    Ok(parsed)
}

// ============================================================================
// 纯 Rust 安全 ZIP 解包器（支持 STORED=0 & DEFLATE=8，零外部 zip crate 依赖）
// ============================================================================

const EOCD_SIG: u32 = 0x0605_4b50;
const CDH_SIG: u32 = 0x0201_4b50;
const LFH_SIG: u32 = 0x0403_4b50;

fn read_u16_le(bytes: &[u8], off: usize) -> Result<u16, ConfigError> {
    let slice = bytes
        .get(off..off + 2)
        .ok_or_else(|| ConfigError::Parse("ZIP 结构截断（u16 越界）".to_string()))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(bytes: &[u8], off: usize) -> Result<u32, ConfigError> {
    let slice = bytes
        .get(off..off + 4)
        .ok_or_else(|| ConfigError::Parse("ZIP 结构截断（u32 越界）".to_string()))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn find_eocd(bytes: &[u8]) -> Result<usize, ConfigError> {
    if bytes.len() < 22 {
        return Err(ConfigError::Parse(
            "文件不是有效的 ZIP 归档（长度不足）".to_string(),
        ));
    }
    let min_pos = bytes.len().saturating_sub(22 + 65_535);
    for pos in (min_pos..=bytes.len() - 22).rev() {
        if read_u32_le(bytes, pos)? == EOCD_SIG {
            let comment_len = read_u16_le(bytes, pos + 20)? as usize;
            if pos + 22 + comment_len == bytes.len() {
                return Ok(pos);
            }
        }
    }
    Err(ConfigError::Parse(
        "未找到 ZIP 末尾中央目录记录 (EOCD)，文件可能损坏或非 ZIP 格式".to_string(),
    ))
}

fn validate_and_canonicalize_zip_entry_name(raw_name: &str) -> Result<Option<String>, ConfigError> {
    if raw_name.is_empty() || raw_name.contains('\0') {
        return Err(ConfigError::field(
            "zip.entry",
            "ZIP 条目名称为空或含空字节",
        ));
    }
    if raw_name.contains('\\') || raw_name.starts_with('/') || raw_name.contains(':') {
        return Err(ConfigError::field(
            "zip.entry",
            format!("拒绝含路径穿越/绝对路径/盘符的 ZIP 条目：{raw_name}"),
        ));
    }
    let segments: Vec<&str> = raw_name.split('/').collect();
    for seg in &segments {
        if *seg == ".." {
            return Err(ConfigError::field(
                "zip.entry",
                format!("拒绝含目录穿越 '..' 的 ZIP 条目：{raw_name}"),
            ));
        }
    }
    // 跳过目录条目与 macOS 元数据目录
    if raw_name.ends_with('/') {
        return Ok(None);
    }
    let non_empty: Vec<&str> = segments.into_iter().filter(|s| !s.is_empty()).collect();
    if non_empty.is_empty() || non_empty[0] == "__MACOSX" {
        return Ok(None);
    }
    // 仅允许根目录文件或最多 1 层顶级目录内的白名单文件
    if non_empty.len() > 2 {
        return Err(ConfigError::field(
            "zip.entry",
            format!("ZIP 目录层级过深（仅允许根目录或单层包目录）：{raw_name}"),
        ));
    }
    let basename = non_empty.last().copied().unwrap_or("");
    let lower = basename.to_ascii_lowercase();
    const ALLOWED_FILES: [&str; 7] = [
        "manifest.json",
        "targets.txt",
        "targets.md",
        "targets.csv",
        "targets.json",
        "rules.toml",
        "readme.md",
    ];
    if !ALLOWED_FILES.contains(&lower.as_str()) {
        return Err(ConfigError::field(
            "zip.entry",
            format!(
                "ZIP 包含不在白名单中的文件 '{raw_name}'（仅允许 manifest.json / targets.{{txt,md,csv,json}} / rules.toml / README.md）"
            ),
        ));
    }
    Ok(Some(lower))
}

fn extract_zip_entries_safe(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, ConfigError> {
    let eocd = find_eocd(bytes)?;
    let entry_count = read_u16_le(bytes, eocd + 10)? as usize;
    let cd_size = read_u32_le(bytes, eocd + 12)? as usize;
    let cd_offset = read_u32_le(bytes, eocd + 16)? as usize;
    if cd_offset.saturating_add(cd_size) > bytes.len() {
        return Err(ConfigError::Parse("ZIP 中央目录越界".to_string()));
    }
    if entry_count > 64 {
        return Err(ConfigError::field(
            "zip",
            format!("ZIP 条目数 ({entry_count}) 超过导入包安全上限 (64)"),
        ));
    }

    let mut out = BTreeMap::new();
    let mut total_extracted: u64 = 0;
    let mut cursor = cd_offset;

    for _ in 0..entry_count {
        if read_u32_le(bytes, cursor)? != CDH_SIG {
            return Err(ConfigError::Parse("ZIP 中央目录条目标记无效".to_string()));
        }
        let flags = read_u16_le(bytes, cursor + 8)?;
        if flags & 0x0001 != 0 {
            return Err(ConfigError::field(
                "zip",
                "不支持加密的 ZIP 导入包，请使用未加密标准 ZIP",
            ));
        }
        let method = read_u16_le(bytes, cursor + 10)?;
        let expected_crc = read_u32_le(bytes, cursor + 16)?;
        let comp_size = read_u32_le(bytes, cursor + 20)? as usize;
        let uncomp_size = read_u32_le(bytes, cursor + 24)? as u64;
        let name_len = read_u16_le(bytes, cursor + 28)? as usize;
        let extra_len = read_u16_le(bytes, cursor + 30)? as usize;
        let comment_len = read_u16_le(bytes, cursor + 32)? as usize;
        let ext_attrs = read_u32_le(bytes, cursor + 38)?;
        let lfh_offset = read_u32_le(bytes, cursor + 42)? as usize;

        let name_slice = bytes
            .get(cursor + 46..cursor + 46 + name_len)
            .ok_or_else(|| ConfigError::Parse("ZIP 条目名越界".to_string()))?;
        let raw_name = std::str::from_utf8(name_slice)
            .map_err(|_| ConfigError::field("zip.entry", "ZIP 条目名不是有效 UTF-8"))?;

        cursor = cursor
            .checked_add(46 + name_len + extra_len + comment_len)
            .ok_or_else(|| ConfigError::Parse("ZIP 偏移溢出".to_string()))?;

        // 拒绝 Unix 符号链接条目（mode 0o120000）
        let unix_mode = (ext_attrs >> 16) & 0o170000;
        if unix_mode == 0o120000 {
            return Err(ConfigError::field(
                "zip.entry",
                format!("拒绝包含符号链接的 ZIP 条目：{raw_name}"),
            ));
        }

        let Some(canonical_name) = validate_and_canonicalize_zip_entry_name(raw_name)? else {
            continue;
        };
        if out.contains_key(&canonical_name) {
            return Err(ConfigError::field(
                "zip.entry",
                format!("ZIP 包含重复的白名单文件：{canonical_name}"),
            ));
        }
        if total_extracted.saturating_add(uncomp_size) > MAX_ZIP_EXTRACTED_BYTES {
            return Err(ConfigError::field(
                "zip",
                format!(
                    "ZIP 解压声明总大小超过 {} MiB 安全上限",
                    MAX_ZIP_EXTRACTED_BYTES / (1024 * 1024)
                ),
            ));
        }

        // 读取本地文件头 (LFH)
        if read_u32_le(bytes, lfh_offset)? != LFH_SIG {
            return Err(ConfigError::Parse("ZIP 本地文件头标记无效".to_string()));
        }
        let lfh_name_len = read_u16_le(bytes, lfh_offset + 26)? as usize;
        let lfh_extra_len = read_u16_le(bytes, lfh_offset + 28)? as usize;
        let data_start = lfh_offset
            .checked_add(30 + lfh_name_len + lfh_extra_len)
            .ok_or_else(|| ConfigError::Parse("ZIP 数据偏移溢出".to_string()))?;
        let data_end = data_start
            .checked_add(comp_size)
            .ok_or_else(|| ConfigError::Parse("ZIP 数据长度溢出".to_string()))?;
        let comp_slice = bytes
            .get(data_start..data_end)
            .ok_or_else(|| ConfigError::Parse("ZIP 压缩数据越界".to_string()))?;

        let remaining_budget = MAX_ZIP_EXTRACTED_BYTES.saturating_sub(total_extracted);
        let decoded = match method {
            0 => {
                if comp_slice.len() as u64 > remaining_budget {
                    return Err(ConfigError::field(
                        "zip",
                        "ZIP 解压后总大小超过 50 MiB 上限",
                    ));
                }
                comp_slice.to_vec()
            }
            8 => {
                // 流式硬限读取（防谎报 uncompressed_size 的 ZIP 炸弹）
                let mut decoder = DeflateDecoder::new(Cursor::new(comp_slice));
                let mut limited = (&mut decoder).take(remaining_budget.saturating_add(1));
                let mut buf = Vec::new();
                limited
                    .read_to_end(&mut buf)
                    .map_err(|e| ConfigError::Parse(format!("ZIP DEFLATE 解压失败：{e}")))?;
                if buf.len() as u64 > remaining_budget {
                    return Err(ConfigError::field(
                        "zip",
                        "ZIP 实际解压字节数超过 50 MiB 安全上限（疑似 ZIP 炸弹已拒绝）",
                    ));
                }
                buf
            }
            other => {
                return Err(ConfigError::field(
                    "zip",
                    format!("不支持的 ZIP 压缩算法 ({other})，仅支持 STORED(0) 与 DEFLATE(8)"),
                ));
            }
        };

        let actual_crc = crc32fast::hash(&decoded);
        if actual_crc != expected_crc {
            return Err(ConfigError::Parse(format!(
                "ZIP 条目 '{raw_name}' CRC-32 校验失败"
            )));
        }

        total_extracted = total_extracted.saturating_add(decoded.len() as u64);
        out.insert(canonical_name, decoded);
    }

    Ok(out)
}

/// 构造最小标准 ZIP 字节（使用 STORED 方法，供测试与内置示例包使用）。
pub fn build_stored_zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let lfh_offset = out.len() as u32;
        let name_bytes = name.as_bytes();
        let crc = crc32fast::hash(data);
        let size = data.len() as u32;

        // Local file header
        out.extend_from_slice(&LFH_SIG.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // method = STORED
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);

        // Central directory header
        central.extend_from_slice(&CDH_SIG.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&0u16.to_le_bytes()); // method = STORED
        central.extend_from_slice(&0u16.to_le_bytes()); // time
        central.extend_from_slice(&0u16.to_le_bytes()); // date
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&lfh_offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);

    // End of central directory
    out.extend_from_slice(&EOCD_SIG.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

impl Registry {
    /// 只读预演导入包：在克隆快照中逐条校验规则与目标绑定，绝不修改真实目录。
    pub fn preview_import_bundle(&self, bundle: &ImportBundle) -> BundlePreviewReport {
        let mut staging = self.clone();
        let mut new_rule_ids: BTreeSet<String> = BTreeSet::new();
        let mut rule_rows = Vec::with_capacity(bundle.rules.len());
        let mut all_valid = true;

        for rule in &bundle.rules {
            match staging.import_user_rule(rule.clone(), true) {
                Ok(meta) => {
                    new_rule_ids.insert(meta.id.clone());
                    rule_rows.push(BundleRulePreviewRow {
                        id: meta.id,
                        kind: meta.kind,
                        pattern: meta.pattern,
                        valid: true,
                        error: None,
                    });
                }
                Err(err) => {
                    all_valid = false;
                    rule_rows.push(BundleRulePreviewRow {
                        id: rule.id.clone(),
                        kind: rule.extract.kind.clone(),
                        pattern: rule.extract.pattern.clone(),
                        valid: false,
                        error: Some(err.to_string()),
                    });
                }
            }
        }

        let mut target_rows = Vec::with_capacity(bundle.targets.len());
        for (idx, item) in bundle.targets.iter().enumerate() {
            let input = UserTargetImportInput {
                id: item.id.clone(),
                url: item.url.clone(),
                surface: item.surface.clone(),
                signals: item.signals.clone(),
                allow_insecure_http: item.allow_insecure_http,
                user_group: item.user_group.clone(),
            };
            let bound_signals: Vec<(String, bool)> = item
                .signals
                .iter()
                .map(|sig| (sig.clone(), new_rule_ids.contains(sig)))
                .collect();
            // 只读判定：同 URL 是否已在目录（内置或用户目标）中。
            let normalized = staging
                .entries()
                .iter()
                .find(|entry| entry.raw.url.trim() == item.url.trim())
                .map(|entry| entry.id().to_string());
            let duplicate_protected = normalized
                .as_deref()
                .is_some_and(|id| staging.get(id).is_some_and(|entry| entry.raw.protected));
            let duplicate_is_user = normalized
                .as_deref()
                .is_some_and(|id| staging.is_user_imported(id));
            let replace_for_preview = item.replace_existing || normalized.is_some();
            match staging.import_user_target_replacing(input, true, replace_for_preview) {
                Ok(meta) => {
                    target_rows.push(BundleTargetPreviewRow {
                        index: idx + 1,
                        planned_id: meta.id,
                        url: meta.url,
                        surface: meta.surface,
                        user_group: meta.user_group,
                        bound_signals,
                        valid: true,
                        error: None,
                        duplicate_of: normalized,
                        duplicate_protected,
                        duplicate_is_user,
                    });
                }
                Err(err) => {
                    all_valid = false;
                    let fallback_group = normalize_user_group(item.user_group.as_deref())
                        .unwrap_or_else(|_| {
                            item.user_group
                                .clone()
                                .filter(|s| !s.trim().is_empty())
                                .unwrap_or_else(|| DEFAULT_USER_GROUP.to_string())
                        });
                    target_rows.push(BundleTargetPreviewRow {
                        index: idx + 1,
                        planned_id: item.id.clone().unwrap_or_else(|| "(自动生成)".to_string()),
                        url: item.url.clone(),
                        surface: item
                            .surface
                            .clone()
                            .unwrap_or_else(|| "用户自定义导入表面".to_string()),
                        user_group: fallback_group,
                        bound_signals,
                        valid: false,
                        error: Some(err.to_string()),
                        duplicate_of: normalized,
                        duplicate_protected,
                        duplicate_is_user,
                    });
                }
            }
        }

        let summary_error = if bundle.targets.is_empty() && bundle.rules.is_empty() {
            all_valid = false;
            Some("导入包内尚未包含任何网址或检查方式".to_string())
        } else if !all_valid {
            Some(
                "存在未通过校验的条目，请先修正后再导入（导入为原子操作，任一项非法将整体回滚）"
                    .to_string(),
            )
        } else {
            None
        };

        BundlePreviewReport {
            source: bundle.source,
            rules: rule_rows,
            targets: target_rows,
            requires_target_consent: bundle.requires_target_consent(),
            requires_rule_consent: bundle.requires_rule_consent(),
            all_valid,
            summary_error,
        }
    }

    /// 原子导入统一导入包（`ImportBundle`）：
    /// - 含目标未勾 `target_consent` → 拒绝（零写入）；
    /// - 含规则未勾 `rule_consent` → 拒绝（零写入）；
    /// - 任一条目非法 → 克隆快照丢弃、原目录完全回滚；
    /// - 全部导入目标默认 `allow_online=false`、`safe_default=false`、`origin=USER_IMPORTED`，零网络请求。
    pub fn import_bundle(
        &mut self,
        bundle: &ImportBundle,
        target_consent: bool,
        rule_consent: bool,
    ) -> Result<BundleImportReport, ConfigError> {
        if bundle.schema != IMPORT_BUNDLE_SCHEMA {
            return Err(ConfigError::field(
                "schema",
                format!("不支持的导入包 schema '{}'", bundle.schema),
            ));
        }
        if bundle.targets.is_empty() && bundle.rules.is_empty() {
            return Err(ConfigError::field("bundle", "导入包为空，未包含目标或规则"));
        }
        if bundle.requires_target_consent() && !target_consent {
            return Err(ConfigError::field(
                "target_consent",
                "本导入包包含自定义网址，必须先阅读并勾选《导入自定义网址责任声明》",
            ));
        }
        if bundle.requires_rule_consent() && !rule_consent {
            return Err(ConfigError::field(
                "rule_consent",
                "本导入包包含自定义检查方式，必须额外阅读并勾选《导入自定义检查方式责任声明》",
            ));
        }

        // 在克隆快照上完整执行，任何错误均整体回滚
        let mut staging = self.clone();
        let mut imported_rules = Vec::with_capacity(bundle.rules.len());
        for (idx, rule) in bundle.rules.iter().enumerate() {
            let meta = staging
                .import_user_rule(rule.clone(), rule_consent)
                .map_err(|err| {
                    ConfigError::field(format!("bundle.rules[{idx}]"), err.to_string())
                })?;
            imported_rules.push(meta);
        }

        let mut imported_targets = Vec::with_capacity(bundle.targets.len());
        for (idx, entry) in bundle.targets.iter().enumerate() {
            let meta = staging
                .import_user_target_replacing(
                    UserTargetImportInput {
                        id: entry.id.clone(),
                        url: entry.url.clone(),
                        surface: entry.surface.clone(),
                        signals: entry.signals.clone(),
                        allow_insecure_http: entry.allow_insecure_http,
                        user_group: entry.user_group.clone(),
                    },
                    target_consent,
                    entry.replace_existing,
                )
                .map_err(|err| {
                    ConfigError::field(format!("bundle.targets[{idx}]"), err.to_string())
                })?;
            imported_targets.push(meta);
        }

        *self = staging;
        Ok(BundleImportReport {
            schema: IMPORT_BUNDLE_SCHEMA.to_string(),
            source: bundle.source,
            origin: ORIGIN_USER_IMPORTED.to_string(),
            consent_version: LIABILITY_CONSENT_VERSION.to_string(),
            imported_rules,
            imported_targets,
        })
    }
}
