//! `targets`：注册目录、信号规则、profile 与依赖数据的加载与校验。
//!
//! 不发送任何请求；外部覆盖层不能提权（不得含 `admission`，不得放宽预算或保护）。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path as FsPath, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml::value::Datetime;

use crate::contracts::{reason, Confidence, Layer, Source};
use crate::eligibility::{EligibilityCatalog, EligibilityNote};
use crate::i18n::{translate, Locale};

/// 内嵌默认资产（单文件运行所需）。
pub const EMBEDDED_TARGETS: &str = include_str!("../../../assets/targets.toml");
/// 内嵌信号规则。
pub const EMBEDDED_RULES: &str = include_str!("../../../assets/signal-rules.toml");
/// 内嵌 profiles。
pub const EMBEDDED_PROFILES: &str = include_str!("../../../assets/profiles.toml");
/// 内嵌依赖图。
pub const EMBEDDED_DEPENDENCIES: &str = include_str!("../../../assets/dependencies.toml");
/// 内嵌优先级示例（不当作真实用户状态）。
pub const EMBEDDED_PRIORITIES_EXAMPLE: &str =
    include_str!("../../../assets/priorities.example.toml");
/// 内嵌静态资格说明（不参与判定）。
pub const EMBEDDED_ELIGIBILITY: &str = include_str!("../../../assets/eligibility-notes.toml");

/// 12 个 Google family + 对照表新增的 3 个 family。
pub const FAMILIES: [&str; 15] = [
    "identity",
    "search",
    "content",
    "workspace",
    "media",
    "store",
    "ai",
    "education",
    "cloud",
    "developer",
    "static",
    "api",
    "cdn",
    "domestic",
    "social",
];

/// 硬预算上限（profiles 只能在代码硬限制内生效）。
#[derive(Debug, Clone, Copy)]
pub struct HardLimits {
    pub max_concurrency: u32,
    pub per_host: u32,
    pub logical_gets_per_target: u32,
    pub extra_attempts_per_target: u32,
    pub max_redirects: u32,
    pub max_body_bytes: u64,
    pub v6_fallback_after_ms: u64,
    pub max_run_timeout_ms: u64,
    pub max_request_timeout_ms: u64,
    /// 同 host / 同节点 / 同协议的发送间隔上限（网络检测；用户裁定可到 10s）。
    pub max_min_interval_ms: u64,
    /// 跨目标（不同 host）检验之间的间隔上限，与同目标上限一致（10s）。
    pub max_inter_target_interval_ms: u64,
    /// 单个小批次的节点/请求数上限。
    pub max_batch_size: u32,
    /// UDP/QUIC 全局并发上限（低噪声硬限）。
    pub max_udp_quic_concurrency: u32,
    /// TCP 全局并发上限。
    pub max_tcp_concurrency: u32,
    /// 批次冷却上限。
    pub max_batch_cooldown_ms: u64,
    /// 单节点复测次数上限。
    pub max_node_retest: u32,
}

/// 代码内的唯一真值。
pub const HARD_LIMITS: HardLimits = HardLimits {
    max_concurrency: 6,
    per_host: 2,
    logical_gets_per_target: 1,
    extra_attempts_per_target: 1,
    // 上限 2：全局默认仍为 0（不跟跳）；仅单个目标显式声明时才允许有限跳转。
    max_redirects: 2,
    max_body_bytes: 16_777_216,
    v6_fallback_after_ms: 8000,
    max_run_timeout_ms: 120_000,
    // 30s：基础 10s + 最多 2 跳各 10s；非跳转目标仍用 profile 的 10s 基础预算。
    max_request_timeout_ms: 30_000,
    max_min_interval_ms: 10000,
    max_inter_target_interval_ms: 10000,
    max_batch_size: 15,
    max_udp_quic_concurrency: 1,
    max_tcp_concurrency: 4,
    max_batch_cooldown_ms: 60_000,
    max_node_retest: 1,
};

/// 加载错误：一律在产生网络 I/O 前返回，退出码 1。
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("配置解析失败：{0}")]
    Parse(String),
    #[error("字段位置 {location}：{message}")]
    Field { location: String, message: String },
    #[error("读取目录失败：{0}")]
    Io(String),
}

impl ConfigError {
    /// 稳定机器码；JSON/agent 可用它而不依赖本地化文字。
    pub fn code(&self) -> &'static str {
        match self {
            ConfigError::Parse(_) => "CONFIG_PARSE",
            ConfigError::Field { .. } => "CONFIG_FIELD",
            ConfigError::Io(_) => "CONFIG_IO",
        }
    }

    /// 按语言包渲染；`Display` 仍保持中文默认，兼容既有调用方。
    pub fn localized(&self, locale: Locale) -> String {
        match self {
            ConfigError::Parse(detail) => translate(locale, "error.parse", &[detail]),
            ConfigError::Field { location, message } => {
                translate(locale, "error.field", &[location, message])
            }
            ConfigError::Io(detail) => translate(locale, "error.io", &[detail]),
        }
    }

    /// 构造带字段位置的配置错误（供 crate 内各模块统一使用）。
    pub fn field(location: impl Into<String>, message: impl Into<String>) -> Self {
        ConfigError::Field {
            location: location.into(),
            message: message.into(),
        }
    }
}

/// 目录默认值（所有行继承，目标不能放宽）。
#[derive(Debug, Clone, Deserialize)]
pub struct Defaults {
    pub method: String,
    pub precondition: String,
    pub request_budget: u32,
    pub max_body_bytes: u64,
    pub max_redirects: u32,
    pub stale_misses: u32,
    pub source_type: Source,
    pub verified_at: Datetime,
}

/// TOML 行结构（targets 与 controls 共用）。
#[derive(Debug, Clone, Deserialize)]
pub struct RawTarget {
    pub id: String,
    pub family: String,
    pub group: String,
    pub matrix_group: String,
    pub url: String,
    pub surface: String,
    pub signals: Vec<String>,
    pub confidence: Confidence,
    pub confidence_basis: String,
    pub failure: String,
    pub protected: bool,
    pub verification: String,
    pub allow_online: bool,
    pub safe_default: bool,
    #[serde(default)]
    pub observed_redirect_host: Option<String>,
    /// 本目标允许的最大跳转次数；不填则用 [defaults] 的全局值。
    /// 只跟随同域跳转，且只读取匿名公开页的地区展示字段；不登录、无 Cookie。
    #[serde(default)]
    pub max_redirects: Option<u32>,
}

/// 运行期目标：注册行 + 合并后的默认值。
#[derive(Debug, Clone)]
pub struct Target {
    pub raw: RawTarget,
    pub defaults: Defaults,
}

impl Target {
    pub fn id(&self) -> &str {
        &self.raw.id
    }

    /// 注册 URL 的主机（小写）。
    pub fn host(&self) -> String {
        parse_https_url(&self.raw.url)
            .or_else(|_| parse_user_http_or_https_url(&self.raw.url, true).map(|(u, _)| u))
            .map(|u| u.host)
            .unwrap_or_default()
    }

    /// 本目标生效的最大跳转次数：注册行优先，否则用全局默认值。
    pub fn max_redirects(&self) -> u32 {
        self.raw
            .max_redirects
            .unwrap_or(self.defaults.max_redirects)
    }

    /// 注册 URL 的路径（含前导斜杠，不含查询）。
    pub fn path(&self) -> String {
        parse_https_url(&self.raw.url)
            .or_else(|_| parse_user_http_or_https_url(&self.raw.url, true).map(|(u, _)| u))
            .map(|u| u.path)
            .unwrap_or_default()
    }
}

/// 解析后的 HTTPS URL（仅注册表使用，不接受用户输入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsUrl {
    pub host: String,
    pub path: String,
    pub query: Option<String>,
}

/// 解析并规范化注册 URL；拒绝 userinfo、fragment、非 https 与空主机。
pub fn parse_https_url(input: &str) -> Result<HttpsUrl, String> {
    let rest = input
        .strip_prefix("https://")
        .ok_or_else(|| "只接受 https:// 地址".to_string())?;
    if rest.contains('#') {
        return Err("不接受 fragment".to_string());
    }
    if rest.contains('@') {
        return Err("不接受 userinfo/凭据".to_string());
    }
    let (authority, remainder) = match rest.find(['/', '?']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err("主机为空".to_string());
    }
    if authority.contains(':') {
        return Err("不接受显式端口".to_string());
    }
    let authority_lower = authority.to_ascii_lowercase();
    if !authority_lower
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("主机含非法字符".to_string());
    }
    let (path, query) = match remainder.find('?') {
        Some(index) => (
            &remainder[..index],
            Some(remainder[index + 1..].to_string()),
        ),
        None => (remainder, None),
    };
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    };
    Ok(HttpsUrl {
        host: authority_lower,
        path,
        query,
    })
}

/// 许可记录：仅受信任发行快照可提供，外部层禁止。
#[derive(Debug, Clone, Deserialize)]
pub struct Grant {
    pub target_id: String,
    pub method: String,
    pub exact_url: String,
    pub surface: String,
    pub purpose: String,
    pub policy_ref: String,
    pub reviewed_by: String,
    pub reviewed_at: Datetime,
    pub valid_until: Datetime,
    #[serde(default)]
    pub rule_versions: BTreeMap<String, u64>,
}

/// 准入状态机结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub admitted: bool,
    pub status: &'static str,
    pub reason: Option<&'static str>,
}

impl Admission {
    fn allow() -> Self {
        Admission {
            admitted: true,
            status: "ADMITTED",
            reason: None,
        }
    }

    fn deny(status: &'static str, reason: &'static str) -> Self {
        Admission {
            admitted: false,
            status,
            reason: Some(reason),
        }
    }
}

/// targets.toml 顶层。
#[derive(Debug, Clone, Deserialize)]
pub struct TargetsFile {
    pub schema_version: u32,
    pub defaults: Defaults,
    #[serde(default)]
    pub admission: AdmissionTable,
    pub targets: Vec<RawTarget>,
    #[serde(default)]
    pub controls: Vec<RawTarget>,
}

/// `[admission]` 表。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AdmissionTable {
    #[serde(default)]
    pub grants: Vec<Grant>,
}

/// 规则作用域（合取）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuleScope {
    pub target_id: String,
    pub host: String,
    pub path: String,
    pub surface: String,
}

/// 提取器描述。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Extractor {
    pub kind: String,
    pub pattern: String,
    pub capture: String,
}

/// 单条信号规则。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub id: String,
    pub version: u64,
    pub enabled: bool,
    pub layer: Layer,
    pub channel: String,
    pub credibility: Confidence,
    pub source_grade: Source,
    pub notes: String,
    pub negative_case: String,
    pub extract: Extractor,
    pub applies_to: Vec<RuleScope>,
}

/// `[detection]`：安全停步策略（不是国家证据）。
#[derive(Debug, Clone, Deserialize)]
pub struct Detection {
    pub login_hosts: Vec<String>,
    pub login_context_hosts: Vec<String>,
    pub login_path_prefixes: Vec<String>,
    pub consent_hosts: Vec<String>,
    pub consent_markers: Vec<String>,
    pub captcha_path_prefixes: Vec<String>,
    pub challenge_markers: Vec<String>,
    pub rate_limit_status: Vec<u16>,
    pub auth_or_denied_status: Vec<u16>,
    pub allowed_html_types: Vec<String>,
    pub allowed_json_types: Vec<String>,
    pub min_text_bytes: u64,
    pub max_body_bytes: u64,
    pub require_complete_for_content: bool,
    pub source: String,
}

/// signal-rules.toml 顶层。
#[derive(Debug, Clone, Deserialize)]
pub struct RulesFile {
    pub schema_version: u32,
    pub detection: Detection,
    pub rules: Vec<Rule>,
}

/// profile 定义。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    #[serde(default)]
    pub groups: Option<Vec<String>>,
    #[serde(default)]
    pub namespace: Option<String>,
    pub run_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub max_concurrency: u32,
    pub per_host: u32,
}

/// profiles.toml 顶层。
#[derive(Debug, Clone, Deserialize)]
pub struct ProfilesFile {
    pub schema_version: u32,
    pub limits: ProfileLimits,
    pub profiles: Vec<Profile>,
}

/// `[limits]`：必须与代码硬限一致，不能抬高。
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileLimits {
    pub max_concurrency: u32,
    pub per_host: u32,
    pub logical_gets_per_target: u32,
    pub extra_attempts_per_target: u32,
    pub max_redirects: u32,
    pub max_body_bytes: u64,
    pub v6_fallback_after_ms: u64,
    #[serde(default)]
    pub min_interval_ms: u64,
    #[serde(default)]
    pub inter_target_interval_ms: u64,
    #[serde(default)]
    pub batch_size: u32,
    #[serde(default)]
    pub udp_quic_concurrency: u32,
    #[serde(default)]
    pub tcp_concurrency: u32,
    #[serde(default)]
    pub batch_cooldown_ms: u64,
    #[serde(default)]
    pub node_retest: u32,
}

/// 依赖边。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DependencyEdge {
    pub src: String,
    pub dst: String,
    pub kind: String,
    pub source_type: Source,
    pub source: String,
    pub verified_at: Datetime,
}

/// 依赖种子。
#[derive(Debug, Clone, Deserialize)]
pub struct DependencySeeds {
    pub source: String,
    pub exact: Vec<String>,
    pub suffixes: Vec<String>,
    pub protected_families: Vec<String>,
}

/// CDN 归因分类。
#[derive(Debug, Clone, Deserialize)]
pub struct DependencyClassifications {
    pub cdn_exact: Vec<String>,
    pub cdn_suffixes: Vec<String>,
}

/// dependencies.toml 顶层。
#[derive(Debug, Clone, Deserialize)]
pub struct DependenciesFile {
    pub schema_version: u32,
    pub default_scope: String,
    pub seeds: DependencySeeds,
    pub classifications: DependencyClassifications,
    pub edges: Vec<DependencyEdge>,
}

/// priorities 示例文件。
#[derive(Debug, Clone, Deserialize)]
pub struct PrioritiesFile {
    pub schema_version: u32,
    pub example_only: bool,
    #[serde(default)]
    pub services: Vec<PriorityEntry>,
}

/// 用户优先级条目（示例文件不加载为用户状态）。
#[derive(Debug, Clone, Deserialize)]
pub struct PriorityEntry {
    pub id: String,
    pub criticality: String,
    pub protect_from_guard: bool,
    pub route_policy: String,
    pub selected_by: String,
}

/// 加载路径覆盖（仅显式本地目录）。
#[derive(Debug, Clone, Default)]
pub struct LoadPaths {
    pub targets_dir: Option<PathBuf>,
    pub profiles_dir: Option<PathBuf>,
    pub config: Option<PathBuf>,
}

/// 不可变目录快照（支持受控的用户态运行时导入与一次性会话级授权）。
#[derive(Debug, Clone)]
pub struct Registry {
    entries: Vec<Target>,
    by_id: HashMap<String, usize>,
    grants: Vec<Grant>,
    pub rules: RulesFile,
    pub profiles: ProfilesFile,
    pub dependencies: DependenciesFile,
    pub priorities_example: PrioritiesFile,
    /// 静态资格说明（不参与判定，不读取账号状态）。
    pub eligibility: EligibilityCatalog,
    base_target_count: usize,
    external_target_ids: HashSet<String>,
    builtin_rule_ids: HashSet<String>,
    user_targets: BTreeMap<String, UserTargetMeta>,
    user_rules: BTreeMap<String, UserRuleMeta>,
    /// 永久授权的"更换内置目标"用户目标（仅这些精确 URL/规则可通过 admission 直接放行）。
    authorized_user_targets: BTreeMap<String, UserTargetAuthorization>,
    active_session_grant: std::sync::Arc<std::sync::Mutex<Option<SessionGrant>>>,
    /// 当前运行是否处于发行包模式。正式发行构建恒为 true；源码构建默认 false。
    distribution_mode: bool,
}

fn parse_targets(text: &str, location: &str) -> Result<TargetsFile, ConfigError> {
    toml::from_str::<TargetsFile>(text).map_err(|error| {
        ConfigError::Parse(format!("{location}: {}", first_line(&error.to_string())))
    })
}

fn parse_rules(text: &str) -> Result<RulesFile, ConfigError> {
    toml::from_str::<RulesFile>(text).map_err(|error| {
        ConfigError::Parse(format!(
            "signal-rules.toml: {}",
            first_line(&error.to_string())
        ))
    })
}

fn parse_profiles(text: &str) -> Result<ProfilesFile, ConfigError> {
    toml::from_str::<ProfilesFile>(text).map_err(|error| {
        ConfigError::Parse(format!("profiles.toml: {}", first_line(&error.to_string())))
    })
}

fn parse_dependencies(text: &str) -> Result<DependenciesFile, ConfigError> {
    toml::from_str::<DependenciesFile>(text).map_err(|error| {
        ConfigError::Parse(format!(
            "dependencies.toml: {}",
            first_line(&error.to_string())
        ))
    })
}

fn parse_priorities(text: &str) -> Result<PrioritiesFile, ConfigError> {
    toml::from_str::<PrioritiesFile>(text).map_err(|error| {
        ConfigError::Parse(format!(
            "priorities.example.toml: {}",
            first_line(&error.to_string())
        ))
    })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}

impl Registry {
    /// 加载内嵌资产并校验；不做任何文件 I/O。
    pub fn embedded() -> Result<Self, ConfigError> {
        Self::from_parts(
            parse_targets(EMBEDDED_TARGETS, "targets.toml")?,
            parse_rules(EMBEDDED_RULES)?,
            parse_profiles(EMBEDDED_PROFILES)?,
            parse_dependencies(EMBEDDED_DEPENDENCIES)?,
            parse_priorities(EMBEDDED_PRIORITIES_EXAMPLE)?,
            EligibilityCatalog::parse(EMBEDDED_ELIGIBILITY)?,
        )
    }

    /// 加载内嵌资产并叠加显式目录覆盖。
    pub fn load(paths: &LoadPaths) -> Result<Self, ConfigError> {
        let mut targets = parse_targets(EMBEDDED_TARGETS, "targets.toml")?;
        let mut profiles = parse_profiles(EMBEDDED_PROFILES)?;

        let mut external_target_ids = HashSet::new();
        if let Some(dir) = &paths.targets_dir {
            for file in toml_files(dir)? {
                let text = read_text(&file)?;
                let layer = parse_targets(&text, &file.display().to_string())?;
                if !layer.admission.grants.is_empty() {
                    return Err(ConfigError::field(
                        file.display().to_string(),
                        "外部层不得包含 admission 许可",
                    ));
                }
                merge_targets(&mut targets, layer, &file, &mut external_target_ids)?;
            }
        }
        if let Some(dir) = &paths.profiles_dir {
            for file in toml_files(dir)? {
                let layer = parse_profiles(&read_text(&file)?)?;
                merge_profiles(&mut profiles, layer, &file)?;
            }
        }
        if let Some(file) = &paths.config {
            // 配置只允许显式引用真实优先级文件；示例文件不能冒充用户选择。
            let text = read_text(file)?;
            let priorities = parse_priorities(&text)?;
            if priorities.example_only {
                return Err(ConfigError::field(
                    file.display().to_string(),
                    "示例优先级不能作为用户配置加载",
                ));
            }
        }

        let mut registry = Self::from_parts(
            targets,
            parse_rules(EMBEDDED_RULES)?,
            profiles,
            parse_dependencies(EMBEDDED_DEPENDENCIES)?,
            parse_priorities(EMBEDDED_PRIORITIES_EXAMPLE)?,
            EligibilityCatalog::parse(EMBEDDED_ELIGIBILITY)?,
        )?;
        registry.external_target_ids = external_target_ids;
        Ok(registry)
    }

    fn from_parts(
        targets: TargetsFile,
        rules: RulesFile,
        profiles: ProfilesFile,
        dependencies: DependenciesFile,
        priorities_example: PrioritiesFile,
        eligibility: EligibilityCatalog,
    ) -> Result<Self, ConfigError> {
        validate_limits(&profiles)?;
        validate_targets(&targets, &rules)?;
        validate_rules(&rules, &targets)?;
        validate_profiles(&profiles, &targets)?;
        validate_dependencies(&dependencies, &targets)?;

        let mut entries = Vec::with_capacity(targets.targets.len() + targets.controls.len());
        for raw in targets.targets.into_iter().chain(targets.controls) {
            entries.push(Target {
                raw,
                defaults: targets.defaults.clone(),
            });
        }
        let mut by_id = HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            by_id.insert(entry.id().to_string(), index);
        }
        let base_target_count = entries.len();
        let builtin_rule_ids = rules.rules.iter().map(|r| r.id.clone()).collect();
        Ok(Registry {
            entries,
            by_id,
            grants: targets.admission.grants,
            rules,
            profiles,
            dependencies,
            priorities_example,
            eligibility,
            base_target_count,
            external_target_ids: HashSet::new(),
            builtin_rule_ids,
            user_targets: BTreeMap::new(),
            user_rules: BTreeMap::new(),
            authorized_user_targets: BTreeMap::new(),
            active_session_grant: std::sync::Arc::new(std::sync::Mutex::new(None)),
            distribution_mode: crate::distribution::DISTRIBUTION_RELEASE_BUILD,
        })
    }

    /// 目录顺序的全部条目。
    pub fn entries(&self) -> &[Target] {
        &self.entries
    }

    /// 目录条目数（135）。
    pub fn catalog_size(&self) -> usize {
        self.entries.len()
    }

    /// 当前是否处于发行包模式。
    pub fn distribution_mode(&self) -> bool {
        self.distribution_mode
    }

    /// 切换源码构建的发行包模式。
    ///
    /// 正式发行构建由编译期 feature 恒定为开启，任何运行时调用都不能关闭；
    /// 源码构建只有在用户完成「关于软件」确认框后才应传入 `true`。
    pub fn set_distribution_mode(&mut self, enabled: bool) {
        self.distribution_mode = crate::distribution::DISTRIBUTION_RELEASE_BUILD || enabled;
    }

    /// 是否为内嵌目录中的内置目标（不含外部覆盖层新增目标与运行时导入的用户目标）。
    pub fn is_embedded_builtin_target(&self, target_id: &str) -> bool {
        self.by_id.get(target_id).is_some_and(|index| {
            *index < self.base_target_count
                && !self.user_targets.contains_key(target_id)
                && !self.external_target_ids.contains(target_id)
        })
    }

    /// 是否为发行包模式可放行的内嵌 Google 目标（严格限定 107 项，不含 control.*）。
    pub fn is_embedded_google_target(&self, target_id: &str) -> bool {
        target_id.starts_with("google.") && self.is_embedded_builtin_target(target_id)
    }

    /// 按 ID 查询。
    pub fn get(&self, id: &str) -> Option<&Target> {
        self.by_id
            .get(id)
            .and_then(|index| self.entries.get(*index))
    }

    /// 某目标适用的静态资格说明（family + 显式 target 命中）。
    /// 只读、不参与判定，也不读取账号状态。
    pub fn eligibility_notes_for(&self, target_id: &str) -> Vec<EligibilityNote> {
        let family = self.get(target_id).map(|target| target.raw.family.as_str());
        self.eligibility.matching(target_id, family)
    }

    /// 资格说明覆盖的服务族（测试用）。
    pub fn eligibility_families(&self) -> std::collections::BTreeSet<String> {
        self.eligibility.covered_families()
    }

    /// 全部规则（含禁用负例，供索引核对）。
    pub fn rules(&self) -> &[Rule] {
        &self.rules.rules
    }

    /// 规则按 ID。
    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.rules.rules.iter().find(|rule| rule.id == id)
    }

    /// 某目标可用（启用且作用域匹配）的规则。
    pub fn rules_for(&self, target: &Target) -> Vec<&Rule> {
        let host = target.host();
        let path = target.path();
        self.rules
            .rules
            .iter()
            .filter(|rule| rule.enabled && target.raw.signals.contains(&rule.id))
            .filter(|rule| {
                rule.applies_to.iter().any(|scope| {
                    scope.target_id == target.raw.id
                        && scope.host == host
                        && scope.path == path
                        && scope.surface == target.raw.surface
                })
            })
            .collect()
    }

    /// 命名 profile。
    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles
            .profiles
            .iter()
            .find(|profile| profile.id == id)
    }

    /// 准入判定：选中 ∧ 许可 ∧ allow_online ∧（隐式默认时 safe_default）∧ 规则版本匹配。
    /// 发行包模式只对内嵌内置目录解除 `allow_online` / 静态 grant 门禁；普通导入目标仍走 F3。
    pub fn admission(&self, target: &Target, implicit_default: bool, now: &str) -> Admission {
        if implicit_default && !target.raw.safe_default {
            return Admission::deny("DENIED_SAFE_DEFAULT_FALSE", reason::SAFE_DEFAULT_FALSE);
        }
        if self.distribution_mode && self.is_embedded_google_target(target.id()) {
            return Admission::allow();
        }
        if self.authorized_user_targets.contains_key(target.id()) && !self.distribution_mode {
            return Admission::deny("DENIED_DISTRIBUTION_MODE_OFF", reason::NO_ADMISSION);
        }
        if !target.raw.allow_online {
            return Admission::deny("DENIED_ALLOW_ONLINE_FALSE", reason::ALLOW_ONLINE_FALSE);
        }
        // 永久授权的"更换内置目标"用户目标：仅对精确 URL/表面 + 规则版本一致的目标放行。
        // 该分支只可能由用户在默认规则包专属确认框里显式授权后写入，不会影响内置目标。
        if let Some(auth) = self.authorized_user_targets.get(&target.raw.id) {
            if !self.user_targets.contains_key(&target.raw.id) {
                return Admission::deny("DENIED_NO_GRANT", reason::NO_ADMISSION);
            }
            if auth.exact_url != target.raw.url || auth.surface != target.raw.surface {
                return Admission::deny("DENIED_GRANT_SURFACE_MISMATCH", reason::ADMISSION_SURFACE);
            }
            let active_rules = self.rules_for(target);
            if active_rules.is_empty() {
                return Admission::deny("DENIED_NO_GRANT", reason::NO_ADMISSION);
            }
            for rule in active_rules {
                if auth.rule_versions.get(&rule.id).copied() != Some(rule.version) {
                    return Admission::deny(
                        "DENIED_GRANT_RULE_VERSION_MISMATCH",
                        reason::ADMISSION_RULE_VERSION,
                    );
                }
            }
            return Admission::allow();
        }
        let Some(grant) = self
            .grants
            .iter()
            .find(|grant| grant.target_id == target.raw.id)
        else {
            return Admission::deny("DENIED_NO_GRANT", reason::NO_ADMISSION);
        };
        if grant.exact_url != target.raw.url
            || grant.surface != target.raw.surface
            || grant.method != target.defaults.method
        {
            return Admission::deny("DENIED_GRANT_SURFACE_MISMATCH", reason::ADMISSION_SURFACE);
        }
        let today = now.get(..10).unwrap_or(now);
        let reviewed = grant.reviewed_at.to_string();
        let valid = grant.valid_until.to_string();
        if today < reviewed.as_str() || today > valid.as_str() {
            return Admission::deny("DENIED_GRANT_EXPIRED", reason::ADMISSION_GRANT_EXPIRED);
        }
        for rule in self.rules_for(target) {
            let granted = grant.rule_versions.get(&rule.id).copied();
            if granted != Some(rule.version) {
                return Admission::deny(
                    "DENIED_GRANT_RULE_VERSION_MISMATCH",
                    reason::ADMISSION_RULE_VERSION,
                );
            }
        }
        Admission::allow()
    }

    /// 展开选集；结果按目录顺序稳定去重。
    pub fn select(
        &self,
        profile: Option<&str>,
        groups: &[String],
        target_ids: &[String],
        full: bool,
    ) -> Result<Selection, ConfigError> {
        if usize::from(full)
            + usize::from(profile.is_some())
            + usize::from(!groups.is_empty())
            + usize::from(!target_ids.is_empty())
            > 1
        {
            return Err(ConfigError::field("selection", "选择器互斥"));
        }
        let profile = if full { Some("full") } else { profile };
        if let Some(name) = profile {
            let item = self
                .profile(name)
                .ok_or_else(|| ConfigError::field(format!("--profile {name}"), "未知 profile"))?;
            let namespace = item.namespace.as_deref();
            let ids: HashSet<&str> = item
                .ids
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(String::as_str)
                .collect();
            let group_set: HashSet<&str> = item
                .groups
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(String::as_str)
                .collect();
            let predicate = |entry: &Target| -> bool {
                match namespace {
                    Some("google") => entry.raw.id.starts_with("google."),
                    Some("all") => !self.user_targets.contains_key(entry.id()),
                    Some(_) => false,
                    None => {
                        if !ids.is_empty() {
                            ids.contains(entry.id())
                        } else {
                            group_set.contains(entry.raw.group.as_str())
                        }
                    }
                }
            };
            let mut selection = self.selection_from(predicate, Some(item.id.clone()), false);
            selection.budget = Budget {
                run_timeout_ms: item.run_timeout_ms.min(HARD_LIMITS.max_run_timeout_ms),
                request_timeout_ms: item
                    .request_timeout_ms
                    .min(HARD_LIMITS.max_request_timeout_ms),
                max_concurrency: item.max_concurrency.min(HARD_LIMITS.max_concurrency),
                per_host: item.per_host.min(HARD_LIMITS.per_host),
                max_redirects: HARD_LIMITS.max_redirects,
                max_body_bytes: HARD_LIMITS.max_body_bytes,
                v6_fallback_after_ms: HARD_LIMITS.v6_fallback_after_ms,
                ..Budget::default()
            };
            return Ok(selection);
        }
        if !groups.is_empty() || !target_ids.is_empty() {
            for group in groups {
                if !self.entries.iter().any(|entry| &entry.raw.group == group) {
                    return Err(ConfigError::field(format!("--group {group}"), "未知 group"));
                }
            }
            for id in target_ids {
                if !self.by_id.contains_key(id) {
                    return Err(ConfigError::field(format!("--target {id}"), "未知目标 ID"));
                }
            }
            let group_set: HashSet<&str> = groups.iter().map(String::as_str).collect();
            let id_set: HashSet<&str> = target_ids.iter().map(String::as_str).collect();
            let predicate = |entry: &Target| -> bool {
                group_set.contains(entry.raw.group.as_str()) || id_set.contains(entry.id())
            };
            return Ok(self.selection_from(predicate, None, false));
        }
        let mut selection = self.select(Some("simple"), &[], &[], false)?;
        selection.implicit_default = true;
        Ok(selection)
    }

    fn selection_from(
        &self,
        predicate: impl Fn(&Target) -> bool,
        profile: Option<String>,
        implicit_default: bool,
    ) -> Selection {
        let selected: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| predicate(entry))
            .map(|(index, _)| index)
            .collect();
        Selection {
            profile,
            ids: selected
                .iter()
                .map(|index| self.entries[*index].id().to_string())
                .collect(),
            implicit_default,
            budget: Budget::default(),
        }
    }

    /// 依据 `--expect` 构造期望。
    pub fn build_expectation(spec: Option<&str>) -> Result<Option<ExpectationSpec>, ConfigError> {
        let Some(spec) = spec else { return Ok(None) };
        let Some((key, value)) = spec.split_once('=') else {
            return Err(ConfigError::field("--expect", "格式应为 KEY=VALUE"));
        };
        match key {
            "state" => Ok(Some(ExpectationSpec::State(value.to_string()))),
            "service_country" => Ok(Some(ExpectationSpec::ServiceCountry(value.to_string()))),
            "family" => Ok(Some(ExpectationSpec::Family(value.to_string()))),
            other => Err(ConfigError::field(
                format!("--expect {other}"),
                "v1 只比较 state/service_country/family",
            )),
        }
    }
}

/// 选集与预算。
#[derive(Debug, Clone)]
pub struct Selection {
    pub profile: Option<String>,
    pub ids: Vec<String>,
    pub implicit_default: bool,
    pub budget: Budget,
}

/// 运行预算（不得超过硬限）。
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub run_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub max_concurrency: u32,
    pub per_host: u32,
    pub max_redirects: u32,
    pub max_body_bytes: u64,
    pub v6_fallback_after_ms: u64,
    /// 同一 host / 节点 / 协议 / 会话类型的发送间隔（起始 3–5 秒）。
    pub min_interval_ms: u64,
    /// 跨目标（不同 host）检验之间的间隔（0=不额外延迟；可调，上限 10s）。
    pub inter_target_interval_ms: u64,
    /// 小批次大小（≤15；UDP/QUIC 建议 3–5）。
    pub batch_size: u32,
    /// UDP/QUIC 全局并发（起始 1）。
    pub udp_quic_concurrency: u32,
    /// TCP 全局并发（起始 2–4）。
    pub tcp_concurrency: u32,
    /// 批次冷却（起始 30–60 秒）。
    pub batch_cooldown_ms: u64,
    /// 单节点默认复测次数（最多 1）。
    pub node_retest: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            run_timeout_ms: HARD_LIMITS.max_run_timeout_ms,
            request_timeout_ms: HARD_LIMITS.max_request_timeout_ms,
            max_concurrency: HARD_LIMITS.max_concurrency,
            per_host: HARD_LIMITS.per_host,
            max_redirects: HARD_LIMITS.max_redirects,
            max_body_bytes: HARD_LIMITS.max_body_bytes,
            v6_fallback_after_ms: HARD_LIMITS.v6_fallback_after_ms,
            // 低噪声安全默认：宁可慢，也不因过密发包污染本地网络。
            min_interval_ms: 3_500,
            inter_target_interval_ms: 0,
            batch_size: 5,
            udp_quic_concurrency: 1,
            tcp_concurrency: 3,
            batch_cooldown_ms: 45_000,
            node_retest: 1,
        }
    }
}

/// `--expect` 的解析结果。
#[derive(Debug, Clone)]
pub enum ExpectationSpec {
    State(String),
    ServiceCountry(String),
    Family(String),
}

fn validate_limits(profiles: &ProfilesFile) -> Result<(), ConfigError> {
    if profiles.schema_version != 1 {
        return Err(ConfigError::field(
            "profiles.toml",
            "schema_version 必须为 1",
        ));
    }
    let limits = &profiles.limits;
    if limits.max_concurrency > HARD_LIMITS.max_concurrency
        || limits.per_host > HARD_LIMITS.per_host
        || limits.logical_gets_per_target > HARD_LIMITS.logical_gets_per_target
        || limits.extra_attempts_per_target > HARD_LIMITS.extra_attempts_per_target
        || limits.max_redirects > HARD_LIMITS.max_redirects
        || limits.max_body_bytes > HARD_LIMITS.max_body_bytes
        || limits.v6_fallback_after_ms > HARD_LIMITS.v6_fallback_after_ms
        || limits.min_interval_ms > HARD_LIMITS.max_min_interval_ms
        || limits.batch_size > HARD_LIMITS.max_batch_size
        || limits.udp_quic_concurrency > HARD_LIMITS.max_udp_quic_concurrency
        || limits.tcp_concurrency > HARD_LIMITS.max_tcp_concurrency
        || limits.batch_cooldown_ms > HARD_LIMITS.max_batch_cooldown_ms
        || limits.node_retest > HARD_LIMITS.max_node_retest
    {
        return Err(ConfigError::field(
            "profiles.toml [limits]",
            "不得放宽代码硬限制",
        ));
    }
    Ok(())
}

fn validate_targets(file: &TargetsFile, rules: &RulesFile) -> Result<(), ConfigError> {
    if file.schema_version != 1 {
        return Err(ConfigError::field(
            "targets.toml",
            "schema_version 必须为 1",
        ));
    }
    if file.defaults.method != "GET" {
        return Err(ConfigError::field(
            "targets.toml defaults.method",
            "仅允许 GET",
        ));
    }
    if file.defaults.precondition != "ANONYMOUS_ONLY" {
        return Err(ConfigError::field(
            "targets.toml defaults.precondition",
            "仅允许 ANONYMOUS_ONLY",
        ));
    }
    if file.defaults.request_budget > HARD_LIMITS.logical_gets_per_target {
        return Err(ConfigError::field(
            "targets.toml defaults.request_budget",
            "超过硬限 1",
        ));
    }
    if file.defaults.max_body_bytes > HARD_LIMITS.max_body_bytes {
        return Err(ConfigError::field(
            "targets.toml defaults.max_body_bytes",
            "超过 16 MiB",
        ));
    }
    if file.defaults.max_redirects > 0 {
        return Err(ConfigError::field(
            "targets.toml defaults.max_redirects",
            "不支持自动跟跳",
        ));
    }
    let rule_ids: HashSet<&str> = rules.rules.iter().map(|rule| rule.id.as_str()).collect();
    let enabled: HashSet<&str> = rules
        .rules
        .iter()
        .filter(|rule| rule.enabled)
        .map(|rule| rule.id.as_str())
        .collect();
    let mut seen = HashSet::new();
    for raw in file.targets.iter().chain(file.controls.iter()) {
        let location = format!("targets.toml [[{}]]", raw.id);
        if raw.id.trim().is_empty() {
            return Err(ConfigError::field("targets.toml", "ID 不得为空"));
        }
        if !seen.insert(raw.id.as_str()) {
            return Err(ConfigError::field(location, "ID 重复"));
        }
        if !FAMILIES.contains(&raw.family.as_str()) {
            return Err(ConfigError::field(location, "family 非法"));
        }
        if !matches!(
            raw.matrix_group.as_str(),
            "A" | "B" | "C" | "D" | "E" | "CONTROL"
        ) {
            return Err(ConfigError::field(location, "matrix_group 非法"));
        }
        if raw.group.trim().is_empty() || raw.surface.trim().is_empty() {
            return Err(ConfigError::field(location, "group/surface 不得为空"));
        }
        let url = parse_https_url(&raw.url)
            .map_err(|message| ConfigError::field(location.clone(), message))?;
        if let Some(query) = url.query.as_deref() {
            // 只接受注册表中已固定的常量查询；不接受任意查询串。
            let allowed = (raw.id == "google.play-detail"
                && query == "id=com.google.android.youtube")
                || (raw.id == "google.gmail" && query == "id=com.google.android.gm");
            if !allowed {
                return Err(ConfigError::field(location.clone(), "未登记的查询串"));
            }
        }
        if raw.safe_default && !raw.allow_online {
            return Err(ConfigError::field(
                location.clone(),
                "safe_default=true 但 allow_online=false 属非法组合",
            ));
        }
        if raw.signals.is_empty() {
            return Err(ConfigError::field(location.clone(), "signals 不得为空"));
        }
        for signal in &raw.signals {
            if !rule_ids.contains(signal.as_str()) {
                return Err(ConfigError::field(
                    location.clone(),
                    format!("引用未知规则 {signal}"),
                ));
            }
            if !enabled.contains(signal.as_str()) {
                return Err(ConfigError::field(
                    location.clone(),
                    format!("不得引用禁用规则 {signal}"),
                ));
            }
        }
    }
    Ok(())
}

fn validate_rules(rules: &RulesFile, targets: &TargetsFile) -> Result<(), ConfigError> {
    if rules.schema_version != 1 {
        return Err(ConfigError::field(
            "signal-rules.toml",
            "schema_version 必须为 1",
        ));
    }
    let mut seen = HashSet::new();
    let targets_by_id: HashMap<&str, &RawTarget> = targets
        .targets
        .iter()
        .chain(targets.controls.iter())
        .map(|raw| (raw.id.as_str(), raw))
        .collect();
    let kinds = [
        "status",
        "header",
        "redirect_host",
        "redirect_param",
        "regex",
        "text_label",
        "body_length",
        "text_kv",
        "json",
    ];
    for rule in &rules.rules {
        if !seen.insert(rule.id.as_str()) {
            return Err(ConfigError::field(
                format!("signal-rules.toml {}", rule.id),
                "规则 ID 重复",
            ));
        }
        if rule.version == 0 {
            return Err(ConfigError::field(
                format!("signal-rules.toml {}", rule.id),
                "version 必须为正整数",
            ));
        }
        if rule.channel != "SERVICE" && rule.channel != "TRANSPORT" {
            return Err(ConfigError::field(
                format!("signal-rules.toml {}", rule.id),
                "channel 必须为 SERVICE 或 TRANSPORT",
            ));
        }
        if !kinds.contains(&rule.extract.kind.as_str()) {
            return Err(ConfigError::field(
                format!("signal-rules.toml {}", rule.id),
                "未知 extract.kind",
            ));
        }
        for scope in &rule.applies_to {
            if !rule.enabled {
                // 禁用负例只作回归索引，不参与双向一致性与准入。
                continue;
            }
            let Some(raw) = targets_by_id.get(scope.target_id.as_str()) else {
                return Err(ConfigError::field(
                    format!("signal-rules.toml {}", rule.id),
                    format!("作用域指向未知目标 {}", scope.target_id),
                ));
            };
            let url = parse_https_url(&raw.url)
                .map_err(|message| ConfigError::field(rule.id.clone(), message))?;
            if scope.host != url.host || scope.path != url.path || scope.surface != raw.surface {
                return Err(ConfigError::field(
                    format!("signal-rules.toml {}", rule.id),
                    "作用域 host/path/surface 与注册行不一致",
                ));
            }
            if !raw.signals.iter().any(|signal| signal == &rule.id) {
                return Err(ConfigError::field(
                    format!("signal-rules.toml {}", rule.id),
                    "作用域目标未在 signals 引用该规则",
                ));
            }
        }
    }
    // 双向一致：目标的每个 signal 都必须有对应作用域。
    for raw in targets.targets.iter().chain(targets.controls.iter()) {
        for signal in &raw.signals {
            let rule = rules
                .rules
                .iter()
                .find(|rule| &rule.id == signal)
                .ok_or_else(|| ConfigError::field("targets.toml", format!("未知规则 {signal}")))?;
            if !rule
                .applies_to
                .iter()
                .any(|scope| scope.target_id == raw.id)
            {
                return Err(ConfigError::field(
                    "signal-rules.toml",
                    format!("规则 {signal} 缺少目标 {} 的作用域", raw.id),
                ));
            }
        }
    }
    Ok(())
}

fn validate_profiles(profiles: &ProfilesFile, targets: &TargetsFile) -> Result<(), ConfigError> {
    let ids: HashSet<&str> = targets
        .targets
        .iter()
        .chain(targets.controls.iter())
        .map(|raw| raw.id.as_str())
        .collect();
    let groups: HashSet<&str> = targets
        .targets
        .iter()
        .chain(targets.controls.iter())
        .map(|raw| raw.group.as_str())
        .collect();
    let mut seen = HashSet::new();
    for profile in &profiles.profiles {
        if !seen.insert(profile.id.as_str()) {
            return Err(ConfigError::field(
                format!("profiles.toml {}", profile.id),
                "profile ID 重复",
            ));
        }
        let count = usize::from(profile.ids.is_some())
            + usize::from(profile.groups.is_some())
            + usize::from(profile.namespace.is_some());
        if count != 1 {
            return Err(ConfigError::field(
                format!("profiles.toml {}", profile.id),
                "ids/groups/namespace 必须恰有一个",
            ));
        }
        if let Some(ids_list) = &profile.ids {
            for id in ids_list {
                if !ids.contains(id.as_str()) {
                    return Err(ConfigError::field(
                        format!("profiles.toml {}", profile.id),
                        format!("引用未知目标 {id}"),
                    ));
                }
            }
        }
        if let Some(group_list) = &profile.groups {
            for group in group_list {
                if !groups.contains(group.as_str()) {
                    return Err(ConfigError::field(
                        format!("profiles.toml {}", profile.id),
                        format!("引用未知 group {group}"),
                    ));
                }
            }
        }
        if let Some(namespace) = &profile.namespace {
            if namespace != "google" && namespace != "all" {
                return Err(ConfigError::field(
                    format!("profiles.toml {}", profile.id),
                    "namespace 必须为 google 或 all",
                ));
            }
        }
        // run_timeout_ms = 0 合法：表示"不限时"（只受单请求超时与 QoS 约束）。
        if profile.run_timeout_ms > HARD_LIMITS.max_run_timeout_ms
            || profile.request_timeout_ms == 0
            || profile.request_timeout_ms > HARD_LIMITS.max_request_timeout_ms
        {
            return Err(ConfigError::field(
                format!("profiles.toml {}", profile.id),
                "运行/请求超时超限",
            ));
        }
        if profile.max_concurrency == 0
            || profile.max_concurrency > HARD_LIMITS.max_concurrency
            || profile.per_host == 0
            || profile.per_host > HARD_LIMITS.per_host
            || profile.per_host > profile.max_concurrency
        {
            return Err(ConfigError::field(
                format!("profiles.toml {}", profile.id),
                "并发/单主机上限非法",
            ));
        }
    }
    Ok(())
}

fn validate_dependencies(
    deps: &DependenciesFile,
    targets: &TargetsFile,
) -> Result<(), ConfigError> {
    if deps.schema_version != 1 {
        return Err(ConfigError::field(
            "dependencies.toml",
            "schema_version 必须为 1",
        ));
    }
    if deps.default_scope != "UNKNOWN" {
        return Err(ConfigError::field(
            "dependencies.toml default_scope",
            "v1 必须为 UNKNOWN",
        ));
    }
    let hosts: HashSet<String> = targets
        .targets
        .iter()
        .chain(targets.controls.iter())
        .filter_map(|raw| parse_https_url(&raw.url).ok().map(|url| url.host))
        .chain(
            targets
                .targets
                .iter()
                .filter_map(|raw| raw.observed_redirect_host.clone()),
        )
        .collect();
    for edge in &deps.edges {
        let src_host = edge.src.strip_prefix("host:").map(str::to_string);
        let dst_host = edge.dst.strip_prefix("host:").map(str::to_string);
        if let Some(host) = src_host {
            if !host.contains('.') {
                return Err(ConfigError::field(
                    "dependencies.toml edges",
                    "节点必须为规范 FQDN",
                ));
            }
        }
        if let Some(host) = dst_host {
            if !host.contains('.') {
                return Err(ConfigError::field(
                    "dependencies.toml edges",
                    "节点必须为规范 FQDN",
                ));
            }
            // 目标可以是目录外主机（共享 CDN 种子），但必须可解析为已知域或种子。
            let known = hosts.contains(&host)
                || deps.seeds.exact.iter().any(|seed| seed == &host)
                || deps
                    .seeds
                    .suffixes
                    .iter()
                    .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")));
            if !known {
                return Err(ConfigError::field(
                    "dependencies.toml edges",
                    format!("依赖节点 {host} 不在目录或种子中"),
                ));
            }
        }
    }
    Ok(())
}

impl DependenciesFile {
    /// 与某目标主机相关的显式入边与出边（只读解释）。
    pub fn explain_for_host(&self, host: &str) -> Vec<&DependencyEdge> {
        self.edges
            .iter()
            .filter(|edge| edge.src == format!("host:{host}") || edge.dst == format!("host:{host}"))
            .collect()
    }
}

pub(crate) fn check_entry(path: &FsPath, directory: bool, field: &str) -> Result<(), ConfigError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(ConfigError::field(field, "无法检查路径")),
    };
    #[cfg(windows)]
    let link = std::os::windows::fs::MetadataExt::file_attributes(&meta) & 0x400 != 0;
    #[cfg(not(windows))]
    let link = meta.file_type().is_symlink();
    if link || (directory && !meta.is_dir()) || (!directory && !meta.is_file()) {
        return Err(ConfigError::field(
            field,
            "拒绝链接、重解析点或异常路径类型",
        ));
    }
    Ok(())
}

pub(crate) fn checked_local_path(
    dir: &FsPath,
    directory: bool,
    field: &str,
) -> Result<PathBuf, ConfigError> {
    let invalid = |message| ConfigError::field(field, message);
    let dir = std::env::current_dir()
        .map_err(|_| invalid("工作目录不可用"))?
        .join(dir);
    if dir
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid("拒绝父目录跳转"));
    }
    #[cfg(windows)]
    if !matches!(dir.components().next(), Some(std::path::Component::Prefix(p)) if matches!(p.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return Err(invalid("拒绝网络共享或设备路径"));
    }
    for parent in dir.ancestors() {
        check_entry(parent, parent != dir || directory, field)?;
    }
    Ok(dir)
}

fn toml_files(dir: &FsPath) -> Result<Vec<PathBuf>, ConfigError> {
    let dir = checked_local_path(dir, true, "配置目录")?;
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|_| ConfigError::Io("配置目录不可读".into()))?
    {
        let path = entry
            .map_err(|_| ConfigError::Io("无法枚举配置目录".into()))?
            .path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn read_text(path: &FsPath) -> Result<String, ConfigError> {
    let path = checked_local_path(path, false, "配置文件")?;
    std::fs::read_to_string(path).map_err(|_| ConfigError::Io("配置文件不可读".into()))
}

fn merge_targets(
    base: &mut TargetsFile,
    layer: TargetsFile,
    file: &FsPath,
    external_ids: &mut HashSet<String>,
) -> Result<(), ConfigError> {
    let mut known: HashMap<String, RawTarget> = base
        .targets
        .iter()
        .chain(base.controls.iter())
        .cloned()
        .map(|raw| (raw.id.clone(), raw))
        .collect();
    let mut layer_ids = HashSet::new();
    for raw in layer.targets.into_iter().chain(layer.controls) {
        if !layer_ids.insert(raw.id.clone()) {
            return Err(ConfigError::field("targets-dir", "同层目标 ID 重复"));
        }
        if let Some(existing) = known.get(&raw.id) {
            if existing.url != raw.url
                || existing.surface != raw.surface
                || existing.signals != raw.signals
            {
                return Err(ConfigError::field(
                    file.display().to_string(),
                    format!("覆盖 {} 时改变了 URL/表面/规则（请用新 ID）", raw.id),
                ));
            }
            if existing.protected && !raw.protected {
                return Err(ConfigError::field(
                    file.display().to_string(),
                    format!("覆盖 {} 时移除了保护", raw.id),
                ));
            }
            if !existing.allow_online && raw.allow_online {
                return Err(ConfigError::field(
                    file.display().to_string(),
                    format!("覆盖 {} 时放宽了 allow_online", raw.id),
                ));
            }
            if !existing.safe_default && raw.safe_default {
                return Err(ConfigError::field(
                    file.display().to_string(),
                    format!("覆盖 {} 时放宽了 safe_default", raw.id),
                ));
            }
            // 覆盖行替换原行；上面已保证不可提权。
            if let Some(slot) = base.targets.iter_mut().find(|item| item.id == raw.id) {
                *slot = raw.clone();
            } else if let Some(slot) = base.controls.iter_mut().find(|item| item.id == raw.id) {
                *slot = raw.clone();
            }
        } else {
            if raw.allow_online || raw.safe_default {
                return Err(ConfigError::field(
                    file.display().to_string(),
                    "外部新增目标不得声明在线许可或默认许可",
                ));
            }
            if raw.id.starts_with("control.") {
                base.controls.push(raw.clone());
            } else {
                base.targets.push(raw.clone());
            }
            external_ids.insert(raw.id.clone());
            known.insert(raw.id.clone(), raw);
        }
    }
    Ok(())
}

fn merge_profiles(
    base: &mut ProfilesFile,
    layer: ProfilesFile,
    file: &FsPath,
) -> Result<(), ConfigError> {
    let mut layer_ids = HashSet::new();
    for profile in layer.profiles {
        if !layer_ids.insert(profile.id.clone()) {
            return Err(ConfigError::field("profiles-dir", "同层 profile ID 重复"));
        }
        if let Some(existing) = base
            .profiles
            .iter_mut()
            .find(|candidate| candidate.id == profile.id)
        {
            if profile.run_timeout_ms > existing.run_timeout_ms
                || profile.request_timeout_ms > existing.request_timeout_ms
                || profile.max_concurrency > existing.max_concurrency
                || profile.per_host > existing.per_host
            {
                return Err(ConfigError::field(
                    file.display().to_string(),
                    format!("覆盖 profile {} 时放宽了预算", profile.id),
                ));
            }
            *existing = profile;
        } else {
            base.profiles.push(profile);
        }
    }
    Ok(())
}

/// 面向用户界面的人话检查方式分类（底层严格对应 9 种白名单 reader kind）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InspectionCategory {
    /// IP 出口检查（Cloudflare trace 键值提取：`text_kv`）
    IpEgressTrace,
    /// HTTP 状态检查（首跳 HTTP 状态码：`status`）
    HttpStatus,
    /// 响应头检查（受控响应头如 Content-Type：`header`）
    ResponseHeader,
    /// 跳转域名检查（301/302 Location 主机：`redirect_host`）
    RedirectHost,
    /// 跳转版次参数检查（Location 中 hl/gl/ceid：`redirect_param`）
    RedirectParam,
    /// 正文正则提取（有界正则匹配国家码/标志：`regex`）
    BodyRegex,
    /// 正文特征标签检查（固定公开文本标签：`text_label`）
    TextLabel,
    /// 响应字节长度检查（完整响应字节数：`body_length`）
    BodyLength,
    /// JSON 字段提取（JSON 顶层白名单标量 country/colo/asn：`json`）
    JsonField,
}

impl InspectionCategory {
    pub const ALL: [InspectionCategory; 9] = [
        InspectionCategory::IpEgressTrace,
        InspectionCategory::HttpStatus,
        InspectionCategory::ResponseHeader,
        InspectionCategory::RedirectHost,
        InspectionCategory::RedirectParam,
        InspectionCategory::BodyRegex,
        InspectionCategory::TextLabel,
        InspectionCategory::BodyLength,
        InspectionCategory::JsonField,
    ];

    /// 人话分类名称。
    pub fn label(self) -> &'static str {
        match self {
            InspectionCategory::IpEgressTrace => "IP 出口检查 (Cloudflare trace 键值)",
            InspectionCategory::HttpStatus => "HTTP 状态码检查",
            InspectionCategory::ResponseHeader => "响应头检查 (Content-Type)",
            InspectionCategory::RedirectHost => "跳转域名检查 (Location Host)",
            InspectionCategory::RedirectParam => "跳转版次参数检查 (hl / gl / ceid)",
            InspectionCategory::BodyRegex => "正文正则提取 (国家码/区域特征)",
            InspectionCategory::TextLabel => "正文特征标签检查",
            InspectionCategory::BodyLength => "响应字节长度检查",
            InspectionCategory::JsonField => "JSON 字段提取 (country / colo / asn)",
        }
    }

    /// 紧凑下拉框短标签。
    pub fn short_label(self) -> &'static str {
        match self {
            InspectionCategory::IpEgressTrace => "IP 出口检查",
            InspectionCategory::HttpStatus => "HTTP 状态码",
            InspectionCategory::ResponseHeader => "响应头检查",
            InspectionCategory::RedirectHost => "跳转域名",
            InspectionCategory::RedirectParam => "跳转版次参数",
            InspectionCategory::BodyRegex => "正文正则提取",
            InspectionCategory::TextLabel => "正文特征标签",
            InspectionCategory::BodyLength => "响应字节长度",
            InspectionCategory::JsonField => "JSON 字段提取",
        }
    }

    /// 底层对应的 9 种白名单 reader kind。
    pub fn reader_kind(self) -> &'static str {
        match self {
            InspectionCategory::IpEgressTrace => "text_kv",
            InspectionCategory::HttpStatus => "status",
            InspectionCategory::ResponseHeader => "header",
            InspectionCategory::RedirectHost => "redirect_host",
            InspectionCategory::RedirectParam => "redirect_param",
            InspectionCategory::BodyRegex => "regex",
            InspectionCategory::TextLabel => "text_label",
            InspectionCategory::BodyLength => "body_length",
            InspectionCategory::JsonField => "json",
        }
    }

    /// 对应内置可直接选用的默认规则 ID。
    pub fn default_builtin_rule_id(self) -> &'static str {
        match self {
            InspectionCategory::IpEgressTrace => "rule.trace",
            InspectionCategory::HttpStatus => "rule.http",
            InspectionCategory::ResponseHeader => "rule.ct",
            InspectionCategory::RedirectHost => "rule.location-host",
            InspectionCategory::RedirectParam => "rule.edition",
            InspectionCategory::BodyRegex => "rule.inline3",
            InspectionCategory::TextLabel => "rule.search-from-ip",
            InspectionCategory::BodyLength => "rule.body-bytes",
            InspectionCategory::JsonField => "rule.meta-json",
        }
    }

    /// 默认提取 pattern 与 capture 示例。
    pub fn default_pattern_and_capture(self) -> (&'static str, &'static str) {
        match self {
            InspectionCategory::IpEgressTrace => ("loc,colo,http,tls,ip_present", "value"),
            InspectionCategory::HttpStatus => ("status", "value"),
            InspectionCategory::ResponseHeader => ("content-type", "value"),
            InspectionCategory::RedirectHost => ("location.host", "value"),
            InspectionCategory::RedirectParam => ("hl,gl,ceid", "value"),
            InspectionCategory::BodyRegex => ("\"GL\":\"([A-Z]{2})\"", "1"),
            InspectionCategory::TextLabel => ("From your internet address", "value"),
            InspectionCategory::BodyLength => ("body_bytes", "value"),
            InspectionCategory::JsonField => ("country,colo,asn", "value"),
        }
    }

    /// 能证明什么与不能证明什么（面向用户直白说明）。
    pub fn proof_boundary(self) -> (&'static str, &'static str) {
        match self {
            InspectionCategory::IpEgressTrace => (
                "能证明该 trace 表面观察到的出口国家码 (loc) 与机房代号 (colo)",
                "不代表 Google 或其他目标服务对该出口的业务判定",
            ),
            InspectionCategory::HttpStatus => (
                "能证明首跳 HTTP 状态码（如 200/204/302/403）可达或被拒",
                "HTTP 200 不等于服务地区可用，绝不自动推导国家",
            ),
            InspectionCategory::ResponseHeader => (
                "能证明响应头的 MIME 类型是否符合预期（HTML/JSON/纯文本）",
                "不含地区信号，不能单独证明服务可用国家",
            ),
            InspectionCategory::RedirectHost => (
                "能证明 301/302 首跳 Location 指向的目标主机（如跳登录或同意页）",
                "不自动跟跳，也不代表登录后的账号归属地",
            ),
            InspectionCategory::RedirectParam => (
                "能证明公开跳转 URL 中的版次参数（hl/gl/ceid）",
                "仅反映公开页面默认版次，不代表账号地区或付费资格",
            ),
            InspectionCategory::BodyRegex => (
                "能从完整 HTML 响应中按有界正则提取内联地区码或区域标志",
                "受页面改版影响，未命中只记缺失原因，不猜测地区",
            ),
            InspectionCategory::TextLabel => (
                "能证明页面正文包含指定公开提示标签",
                "仅证明标签存在，不单独推导国家码",
            ),
            InspectionCategory::BodyLength => (
                "能证明完整响应的字节长度（<= 16 MiB）",
                "长度本身不代表内容正常或地区受支持",
            ),
            InspectionCategory::JsonField => (
                "能从 JSON 响应提取顶层 country / colo / asn 白名单标量",
                "仅代表该 JSON 端点的观测值，不外推至其他服务",
            ),
        }
    }

    pub fn from_reader_kind(kind: &str) -> Option<Self> {
        match kind {
            "text_kv" => Some(InspectionCategory::IpEgressTrace),
            "status" => Some(InspectionCategory::HttpStatus),
            "header" => Some(InspectionCategory::ResponseHeader),
            "redirect_host" => Some(InspectionCategory::RedirectHost),
            "redirect_param" => Some(InspectionCategory::RedirectParam),
            "regex" => Some(InspectionCategory::BodyRegex),
            "text_label" => Some(InspectionCategory::TextLabel),
            "body_length" => Some(InspectionCategory::BodyLength),
            "json" => Some(InspectionCategory::JsonField),
            _ => None,
        }
    }
}

/// 用户自定义导入的默认展示分组名（仅影响展示与筛选，不影响判定与许可闸门）。
pub const DEFAULT_USER_GROUP: &str = "我的导入";

fn default_user_group() -> String {
    DEFAULT_USER_GROUP.to_string()
}

/// 校验并归一化用户自定义分组名称（独立于内置 `google` / `control` 命名空间）。
pub fn normalize_user_group(raw: Option<&str>) -> Result<String, ConfigError> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_USER_GROUP.to_string());
    };
    if trimmed.chars().count() > 48 {
        return Err(ConfigError::field(
            "user_group",
            "自定义分组名称长度不得超过 48 个字符",
        ));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(ConfigError::field(
            "user_group",
            "自定义分组名称不得包含控制字符",
        ));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower == "google"
        || lower.starts_with("google.")
        || lower == "control"
        || lower.starts_with("control.")
    {
        return Err(ConfigError::field(
            "user_group",
            "自定义分组必须独立于内置 google / control 命名空间",
        ));
    }
    Ok(trimmed.to_string())
}

/// 用户导入目标的审计与留痕元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserTargetMeta {
    pub id: String,
    pub url: String,
    pub surface: String,
    pub signals: Vec<String>,
    /// 用户自主归类分组（仅用于展示与筛选，默认"我的导入"，绝不影响判定或准入）。
    #[serde(default = "default_user_group")]
    pub user_group: String,
    pub origin: String,
    pub consent_version: String,
    pub imported_at: String,
    pub rule_digest: String,
    pub allow_insecure_http: bool,
    pub http_warning: Option<String>,
    /// 若本目标用于"更换"某个内置目标，记录被更换的内置目标 ID（仅审计与展示，不改变判定）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_builtin_id: Option<String>,
    /// 被更换的内置目标是否为受保护目标（批量更换需二次明确确认）。
    #[serde(default)]
    pub replaces_protected: bool,
}

/// 用户导入检查方式（规则）的审计与留痕元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRuleMeta {
    pub id: String,
    pub version: u64,
    pub kind: String,
    pub pattern: String,
    pub capture: String,
    pub origin: String,
    pub consent_version: String,
    pub imported_at: String,
    pub rule_digest: String,
}

/// 用户导入单个目标的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTargetImportInput {
    pub id: Option<String>,
    pub url: String,
    pub surface: Option<String>,
    pub signals: Vec<String>,
    pub allow_insecure_http: bool,
    #[serde(default)]
    pub user_group: Option<String>,
}

/// 用户对某个"更换内置目标"的用户目标所持有的显式在线授权（永久范围）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserTargetAuthorization {
    pub target_id: String,
    pub exact_url: String,
    pub surface: String,
    #[serde(default)]
    pub rule_versions: BTreeMap<String, u64>,
    pub rule_digest: String,
    pub consent_version: String,
    pub granted_at: String,
    /// 授权来源（当前仅发行包默认规则包）。
    pub source: String,
}

/// 永久授权写入用户状态目录时使用的单条目标快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedDefaultBundleTarget {
    pub id: String,
    pub url: String,
    pub surface: String,
    pub signals: Vec<String>,
    pub user_group: String,
    #[serde(default)]
    pub allow_insecure_http: bool,
    pub replaced_builtin_id: String,
    #[serde(default)]
    pub rule_versions: BTreeMap<String, u64>,
}

/// 默认规则包永久授权文件（`parallaxe.default-bundle-consent/v1`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultBundleConsentFile {
    pub schema: String,
    pub consent_version: String,
    pub granted_at: String,
    /// 当前仅支持 `PERMANENT`（`SESSION` 不落盘）。
    pub mode: String,
    #[serde(default)]
    pub targets: Vec<PersistedDefaultBundleTarget>,
}

/// 默认规则包授权范围的单条精确边界（ID、URL、表面、规则摘要、规则版本）。
pub type DefaultBundleScopeEntry = (String, String, String, String, BTreeMap<String, u64>);

/// 恢复用户状态目录时的结果摘要。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserStateRestoreReport {
    pub restored_targets: usize,
    pub authorized_targets: usize,
    pub skipped_targets: usize,
}

/// 一次性会话级在线许可（仅对单次 run 的精确用户导入目标与规则有效）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGrant {
    pub grant_id: String,
    pub consent_version: String,
    pub confirmed_at: String,
    pub prompt_summary: String,
    pub targets: BTreeMap<String, SessionTargetGrant>,
}

/// 一次性会话许可中的单目标绑定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTargetGrant {
    pub target_id: String,
    pub exact_url: String,
    pub surface: String,
    pub rule_versions: BTreeMap<String, u64>,
    pub rule_digest: String,
}

/// 解析并校验用户导入 URL（默认仅 https://；显式允许 http:// 时返回安全警告；拒绝 userinfo、fragment、端口及本机/私有地址）。
pub fn parse_user_http_or_https_url(
    input: &str,
    allow_insecure_http: bool,
) -> Result<(HttpsUrl, Option<String>), String> {
    let trimmed = input.trim();
    let (rest, warning) = if let Some(r) = trimmed.strip_prefix("https://") {
        (r, None)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        if !allow_insecure_http {
            return Err(
                "默认仅接受 https:// 地址；若确需导入 http:// 请显式勾选允许非加密 HTTP 并知悉风险"
                    .to_string(),
            );
        }
        (
            r,
            Some("警告：已显式允许非加密 http:// 地址，传输内容不具备 TLS 保护".to_string()),
        )
    } else {
        return Err("只接受 https://（或显式允许的 http://）完整地址".to_string());
    };

    if rest.contains('#') {
        return Err("不接受含 fragment (#) 的地址".to_string());
    }
    if rest.contains('@') {
        return Err("不接受含 userinfo/凭据 (@) 的地址".to_string());
    }
    if rest.chars().any(char::is_whitespace) {
        return Err("地址不得包含空白字符".to_string());
    }
    let (authority, remainder) = match rest.find(['/', '?']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err("主机名不得为空".to_string());
    }
    if authority.contains(':') {
        return Err("不接受显式端口".to_string());
    }
    let host = authority.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || !host.contains('.')
        || host.starts_with('.')
        || host.starts_with('-')
        || host.ends_with('-')
        || host.contains("..")
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("主机名必须为合法的公网 FQDN 域名".to_string());
    }
    if crate::probe::is_private_destination(&host)
        || host.ends_with(".internal")
        || host.ends_with(".lan")
        || host.ends_with(".home.arpa")
    {
        return Err("拒绝本机、回环、局域网或私有地址".to_string());
    }
    let (path, query) = match remainder.find('?') {
        Some(index) => (
            &remainder[..index],
            Some(remainder[index + 1..].to_string()),
        ),
        None => (remainder, None),
    };
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    };
    Ok((HttpsUrl { host, path, query }, warning))
}

/// 校验用户导入的正则表达式是否有界且安全（防 ReDoS 与越界回溯）。
pub fn validate_safe_regex(pattern: &str) -> Result<(), String> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Err("正则表达式不得为空".to_string());
    }
    if trimmed.len() > 256 {
        return Err("正则表达式长度超过 256 字符上限".to_string());
    }
    if trimmed.contains("name=\"region\"") {
        return Err("禁止启用已废弃的 legacy region 负例模式".to_string());
    }
    for forbidden in ["(?=", "(?!", "(?<=", "(?<!"] {
        if trimmed.contains(forbidden) {
            return Err(format!("拒绝包含环视断言 ({forbidden}) 的高风险正则"));
        }
    }
    // 检查反向引用 \1..\9
    let chars: Vec<char> = trimmed.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            if let Some(&next) = chars.get(i + 1) {
                if next.is_ascii_digit() && next != '0' {
                    return Err("拒绝包含反向引用的正则表达式".to_string());
                }
            }
            i += 2;
            continue;
        }
        i += 1;
    }

    // 检查花括号量词上限与无限上界 {n,} / {n,m}
    let mut idx = 0;
    while idx < chars.len() {
        if chars[idx] == '\\' {
            idx += 2;
            continue;
        }
        if chars[idx] == '{' {
            if let Some(end_offset) = chars[idx + 1..].iter().position(|&c| c == '}') {
                let content: String = chars[idx + 1..idx + 1 + end_offset].iter().collect();
                if let Some((min_s, max_s)) = content.split_once(',') {
                    let min_v = min_s.trim().parse::<u32>().map_err(|_| "量词下界非法")?;
                    if max_s.trim().is_empty() {
                        return Err("拒绝无上界花括号量词 {n,}（请指定有界上限 <= 64）".to_string());
                    }
                    let max_v = max_s.trim().parse::<u32>().map_err(|_| "量词上界非法")?;
                    if min_v > 64 || max_v > 64 || min_v > max_v {
                        return Err("花括号量词重复次数不得超过 64".to_string());
                    }
                } else {
                    let exact = content.trim().parse::<u32>().map_err(|_| "量词数值非法")?;
                    if exact > 64 {
                        return Err("花括号量词重复次数不得超过 64".to_string());
                    }
                }
                idx += end_offset + 2;
                continue;
            }
        }
        idx += 1;
    }

    // 检查括号分组嵌套量词 (ReDoS 核心模式，如 (a+)+, (.*)+, (.+)*, (a|aa)+)
    // 记录每一层分组内部是否含重复量词或分支
    #[derive(Clone, Copy)]
    struct GroupInfo {
        has_quantifier: bool,
        has_alternation: bool,
    }
    let mut stack: Vec<GroupInfo> = Vec::new();
    let mut in_char_class = false;
    let mut pos = 0;
    while pos < chars.len() {
        let ch = chars[pos];
        if ch == '\\' {
            pos += 2;
            continue;
        }
        if ch == '[' && !in_char_class {
            in_char_class = true;
            pos += 1;
            continue;
        }
        if ch == ']' && in_char_class {
            in_char_class = false;
            pos += 1;
            continue;
        }
        if in_char_class {
            pos += 1;
            continue;
        }
        match ch {
            '(' => {
                if stack.len() >= 4 {
                    return Err("拒绝分组嵌套深度超过 4 层的复杂正则".to_string());
                }
                stack.push(GroupInfo {
                    has_quantifier: false,
                    has_alternation: false,
                });
            }
            ')' => {
                if let Some(group) = stack.pop() {
                    let next_ch = chars.get(pos + 1).copied();
                    let followed_by_quant = matches!(next_ch, Some('+' | '*' | '?' | '{'));
                    if followed_by_quant && (group.has_quantifier || group.has_alternation) {
                        return Err(
                            "拒绝危险正则：分组内包含量词或分支且外层再次叠加量词（防 ReDoS 回溯）"
                                .to_string(),
                        );
                    }
                    if let Some(parent) = stack.last_mut() {
                        if group.has_quantifier || followed_by_quant {
                            parent.has_quantifier = true;
                        }
                        if group.has_alternation {
                            parent.has_alternation = true;
                        }
                    }
                }
            }
            '+' | '*' | '{' => {
                if let Some(top) = stack.last_mut() {
                    top.has_quantifier = true;
                }
            }
            '|' => {
                if let Some(top) = stack.last_mut() {
                    top.has_alternation = true;
                }
            }
            _ => {}
        }
        pos += 1;
    }

    if trimmed.contains(".*.*") || trimmed.contains(".+.+") {
        return Err("拒绝相邻无界通配符 .*.* 或 .+.+（防二次回溯）".to_string());
    }

    regex::RegexBuilder::new(trimmed)
        .size_limit(32 * 1024)
        .dfa_size_limit(32 * 1024)
        .build()
        .map_err(|e| format!("正则表达式编译失败或状态机超限：{e}"))?;
    Ok(())
}

/// 计算一组规则的稳定 SHA-256 摘要前缀（用于报告 `rule_digest` 审计留痕）。
pub fn compute_rules_digest(rules: &[&Rule]) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&Rule> = rules.to_vec();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut hasher = Sha256::new();
    for r in sorted {
        hasher.update(
            format!(
                "{}|{}|{}|{}|{};",
                r.id, r.version, r.extract.kind, r.extract.pattern, r.extract.capture
            )
            .as_bytes(),
        );
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(23);
    hex.push_str("sha256:");
    for b in &digest[..8] {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

#[derive(Deserialize)]
struct UserRulesImportToml {
    #[serde(default)]
    rules: Vec<Rule>,
}

impl Registry {
    /// 内置目录条数（固定 135）。
    pub fn builtin_catalog_size(&self) -> usize {
        self.base_target_count - self.external_target_ids.len()
    }

    pub(crate) fn is_embedded_target(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
            && !self.external_target_ids.contains(id)
            && !self.is_user_imported(id)
    }

    /// 判断某目标是否为用户运行时导入。
    pub fn is_user_imported(&self, target_id: &str) -> bool {
        self.user_targets.contains_key(target_id)
    }

    /// 判断某规则是否为用户运行时导入。
    pub fn is_user_rule(&self, rule_id: &str) -> bool {
        self.user_rules.contains_key(rule_id)
    }

    /// 获取全部已导入的用户目标元数据（按 ID 稳定排序）。
    pub fn user_targets(&self) -> Vec<&UserTargetMeta> {
        self.user_targets.values().collect()
    }

    /// 获取全部已导入的用户检查方式元数据（按 ID 稳定排序）。
    pub fn user_rules(&self) -> Vec<&UserRuleMeta> {
        self.user_rules.values().collect()
    }

    /// 查询单个用户目标的审计元数据。
    pub fn user_target_meta(&self, target_id: &str) -> Option<&UserTargetMeta> {
        self.user_targets.get(target_id)
    }

    /// 返回当前已登记的用户自定义分组名清单（按名称稳定排序去重）。
    pub fn user_groups(&self) -> Vec<String> {
        let groups: std::collections::BTreeSet<String> = self
            .user_targets
            .values()
            .map(|m| m.user_group.clone())
            .collect();
        groups.into_iter().collect()
    }

    /// 查询指定用户目标所属的自定义分组名（内置目标返回 `None`）。
    pub fn user_group_of(&self, target_id: &str) -> Option<&str> {
        self.user_targets
            .get(target_id)
            .map(|m| m.user_group.as_str())
    }

    /// 若目标或所绑定规则包含用户导入项，返回 `(origin, consent_version, imported_at, rule_digest)`。
    pub fn user_audit_for(&self, target: &Target) -> Option<(String, String, String, String)> {
        if let Some(meta) = self.user_targets.get(&target.raw.id) {
            return Some((
                meta.origin.clone(),
                meta.consent_version.clone(),
                meta.imported_at.clone(),
                meta.rule_digest.clone(),
            ));
        }
        let user_rule = target
            .raw
            .signals
            .iter()
            .find_map(|sig| self.user_rules.get(sig));
        user_rule.map(|rm| {
            (
                rm.origin.clone(),
                rm.consent_version.clone(),
                rm.imported_at.clone(),
                rm.rule_digest.clone(),
            )
        })
    }

    /// F2：导入单条自定义检查方式（规则），强制要求用户已主动勾选责任声明。
    pub fn import_user_rule(
        &mut self,
        mut rule: Rule,
        consent_accepted: bool,
    ) -> Result<UserRuleMeta, ConfigError> {
        if !consent_accepted {
            return Err(ConfigError::field(
                "consent",
                "必须先阅读并勾选同意《自定义检查方式导入责任声明》",
            ));
        }
        let id = rule.id.trim().to_string();
        if id.is_empty() {
            return Err(ConfigError::field("rule.id", "检查方式 ID 不得为空"));
        }
        if id == "rule.legacy-region" || self.builtin_rule_ids.contains(&id) {
            return Err(ConfigError::field(
                "rule.id",
                format!("不得覆盖或冒充内置规则 {id}"),
            ));
        }
        if self.user_rules.contains_key(&id) || self.rule(&id).is_some() {
            return Err(ConfigError::field("rule.id", format!("规则 ID 重复：{id}")));
        }
        if !id.starts_with("rule.")
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(ConfigError::field(
                "rule.id",
                "规则 ID 必须以 rule. 开头且仅含字母、数字、点、横线或下划线",
            ));
        }
        if rule.version == 0 {
            rule.version = 1;
        }
        if !rule.enabled {
            return Err(ConfigError::field(
                "rule.enabled",
                "导入的自定义检查方式必须处于启用状态 (enabled=true)",
            ));
        }
        if rule.channel != "SERVICE" && rule.channel != "TRANSPORT" {
            return Err(ConfigError::field(
                "rule.channel",
                "channel 必须为 SERVICE 或 TRANSPORT",
            ));
        }
        let allowed_kinds = [
            "status",
            "header",
            "redirect_host",
            "redirect_param",
            "regex",
            "text_label",
            "body_length",
            "text_kv",
            "json",
        ];
        if !allowed_kinds.contains(&rule.extract.kind.as_str()) {
            return Err(ConfigError::field(
                "rule.extract.kind",
                format!(
                    "不支持的检查方式 kind '{}'（仅支持 9 种白名单 reader）",
                    rule.extract.kind
                ),
            ));
        }
        if rule.extract.pattern.trim().is_empty() || rule.extract.pattern.len() > 256 {
            return Err(ConfigError::field(
                "rule.extract.pattern",
                "extract.pattern 不得为空且长度不得超过 256",
            ));
        }
        if rule.extract.pattern.contains("name=\"region\"") {
            return Err(ConfigError::field(
                "rule.extract.pattern",
                "禁止启用已废弃的 legacy region 负例特征",
            ));
        }
        if rule.extract.kind == "regex" {
            validate_safe_regex(&rule.extract.pattern)
                .map_err(|msg| ConfigError::field("rule.extract.pattern", msg))?;
        }
        if !matches!(rule.extract.capture.as_str(), "value" | "1" | "0") {
            return Err(ConfigError::field(
                "rule.extract.capture",
                "capture 仅支持 value、1 或 0",
            ));
        }
        // F2 红线：导入的规则不得自动启用到任何内置目标！
        for scope in &rule.applies_to {
            if !self.user_targets.contains_key(&scope.target_id) {
                return Err(ConfigError::field(
                    "rule.applies_to",
                    format!(
                        "自定义规则不得绑定到内置或未导入的目标 '{}'（严禁提升或篡改内置目标）",
                        scope.target_id
                    ),
                ));
            }
        }
        rule.id = id.clone();
        let now_str = crate::state::format_time(time::OffsetDateTime::now_utc());
        let digest = compute_rules_digest(&[&rule]);
        let meta = UserRuleMeta {
            id: id.clone(),
            version: rule.version,
            kind: rule.extract.kind.clone(),
            pattern: rule.extract.pattern.clone(),
            capture: rule.extract.capture.clone(),
            origin: crate::contracts::ORIGIN_USER_IMPORTED.to_string(),
            consent_version: crate::contracts::LIABILITY_CONSENT_VERSION.to_string(),
            imported_at: now_str,
            rule_digest: digest,
        };
        self.rules.rules.push(rule);
        self.user_rules.insert(id, meta.clone());
        Ok(meta)
    }

    /// F2：从 TOML 文本批量导入自定义检查方式（与 `assets/signal-rules.toml` 的 `[[rules]]` 格式一致）。
    pub fn import_user_rules_toml(
        &mut self,
        toml_text: &str,
        consent_accepted: bool,
    ) -> Result<Vec<UserRuleMeta>, ConfigError> {
        if !consent_accepted {
            return Err(ConfigError::field(
                "consent",
                "必须先阅读并勾选同意《自定义检查方式导入责任声明》",
            ));
        }
        let raw_val: toml::Value = toml::from_str(toml_text).map_err(|e| {
            ConfigError::Parse(format!(
                "规则 TOML 解析失败：{}",
                first_line(&e.to_string())
            ))
        })?;
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
                        format!("导入规则严禁包含许可或越权字段 '{forbidden}'"),
                    ));
                }
            }
            if let Some(rules_arr) = table.get("rules").and_then(|v| v.as_array()) {
                for item in rules_arr {
                    if let Some(rt) = item.as_table() {
                        for forbidden in ["admission", "grants", "allow_online", "safe_default"] {
                            if rt.contains_key(forbidden) {
                                return Err(ConfigError::field(
                                    forbidden,
                                    format!("规则条目严禁包含许可字段 '{forbidden}'"),
                                ));
                            }
                        }
                    }
                }
            }
        }
        let parsed: Vec<Rule> = if raw_val.get("rules").is_some() {
            let file: UserRulesImportToml = toml::from_str(toml_text).map_err(|e| {
                ConfigError::Parse(format!(
                    "[[rules]] 字段格式错误：{}",
                    first_line(&e.to_string())
                ))
            })?;
            file.rules
        } else {
            let single: Rule = toml::from_str(toml_text).map_err(|e| {
                ConfigError::Parse(format!("单条规则格式错误：{}", first_line(&e.to_string())))
            })?;
            vec![single]
        };
        if parsed.is_empty() {
            return Err(ConfigError::field("rules", "未找到任何 [[rules]] 定义"));
        }
        let mut out = Vec::with_capacity(parsed.len());
        for rule in parsed {
            out.push(self.import_user_rule(rule, consent_accepted)?);
        }
        Ok(out)
    }

    /// F1：导入单个用户自定义网址并绑定所选检查方式，强制要求勾选责任声明；默认 `allow_online=false`、`safe_default=false`。
    /// 重复网址默认硬拒绝；批量"更换"路径请调用 `import_user_target_replacing`。
    pub fn import_user_target(
        &mut self,
        input: UserTargetImportInput,
        consent_accepted: bool,
    ) -> Result<UserTargetMeta, ConfigError> {
        self.import_user_target_replacing(input, consent_accepted, false)
    }

    /// F1：导入单个用户自定义网址；`replace_existing=true` 时允许"更换"同 URL 的内置/用户目标。
    pub fn import_user_target_replacing(
        &mut self,
        input: UserTargetImportInput,
        consent_accepted: bool,
        replace_existing: bool,
    ) -> Result<UserTargetMeta, ConfigError> {
        if !consent_accepted {
            return Err(ConfigError::field(
                "consent",
                "必须先阅读并勾选同意《自定义网址导入责任声明》",
            ));
        }
        let (parsed_url, http_warning) =
            parse_user_http_or_https_url(&input.url, input.allow_insecure_http)
                .map_err(|msg| ConfigError::field("url", msg))?;
        let scheme = if input.url.trim().starts_with("http://") {
            "http://"
        } else {
            "https://"
        };
        let normalized_url = match &parsed_url.query {
            Some(q) if !q.is_empty() => {
                format!("{scheme}{}{}?{q}", parsed_url.host, parsed_url.path)
            }
            _ => format!("{scheme}{}{}", parsed_url.host, parsed_url.path),
        };

        // 重复 URL 不再硬拒绝：记录候选，等 ID/检查方式解析后由 `replace_existing` 决定是否更换。
        let duplicate_idx = self
            .entries
            .iter()
            .position(|e| e.raw.url == normalized_url);

        // 生成或校验目标 ID（强制 `user.` 命名空间，防止冒充内置 `google.*` 或 `control.*`）
        let id = match input.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(custom) => {
                if custom.starts_with("google.") || custom.starts_with("control.") {
                    return Err(ConfigError::field(
                        "id",
                        "用户导入目标不得使用内置 google. 或 control. 前缀",
                    ));
                }
                let normalized_id = if custom.starts_with("user.") {
                    custom.to_string()
                } else {
                    format!("user.{custom}")
                };
                if !normalized_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                {
                    return Err(ConfigError::field(
                        "id",
                        "目标 ID 只能包含 ASCII 字母、数字、点、横线或下划线",
                    ));
                }
                normalized_id
            }
            None => {
                let slug: String = parsed_url
                    .host
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                    .collect();
                let mut seq = self.user_targets.len() + 1;
                loop {
                    let candidate = format!("user.{slug}-{seq}");
                    if !self.by_id.contains_key(&candidate) {
                        break candidate;
                    }
                    seq += 1;
                }
            }
        };

        let mut replaced_builtin_id: Option<String> = None;
        let mut replaces_protected = false;
        if let Some(idx) = duplicate_idx {
            let dup_id = self.entries[idx].raw.id.clone();
            let dup_is_user = self.user_targets.contains_key(&dup_id);
            if dup_is_user {
                let idempotent = dup_id == id
                    && self.user_targets.get(&id).is_some_and(|meta| {
                        let mut current = meta.signals.clone();
                        let mut incoming: Vec<String> =
                            input.signals.iter().map(|s| s.trim().to_string()).collect();
                        current.sort();
                        current.dedup();
                        incoming.sort();
                        incoming.dedup();
                        meta.url == normalized_url && current == incoming
                    });
                if idempotent {
                    if let Some(meta) = self.user_targets.get(&id) {
                        return Ok(meta.clone());
                    }
                }
                if !replace_existing {
                    return Err(ConfigError::field(
                        "url",
                        format!("网址已由用户目标 {dup_id} 导入；如需更换请显式确认"),
                    ));
                }
                // 用新条目替换旧用户目标，避免同一 URL 出现两个用户目标。
                self.remove_user_target_entry(&dup_id);
            } else if replace_existing {
                let dup = &self.entries[idx];
                replaced_builtin_id = Some(dup_id);
                replaces_protected = dup.raw.protected;
            } else {
                return Err(ConfigError::field(
                    "url",
                    format!("网址已存在于目录中：{normalized_url}"),
                ));
            }
        }

        if self.by_id.contains_key(&id) {
            return Err(ConfigError::field("id", format!("目标 ID 已存在：{id}")));
        }

        if input.signals.is_empty() {
            return Err(ConfigError::field(
                "signals",
                "请至少为该网址选择一种检查方式",
            ));
        }
        let mut dedup_signals = Vec::new();
        for sig in &input.signals {
            let sig = sig.trim();
            let Some(rule) = self.rule(sig) else {
                return Err(ConfigError::field(
                    "signals",
                    format!("未知检查方式 ID：{sig}"),
                ));
            };
            if !rule.enabled {
                return Err(ConfigError::field(
                    "signals",
                    format!("不得绑定已禁用的负例规则：{sig}"),
                ));
            }
            if !dedup_signals.contains(&sig.to_string()) {
                dedup_signals.push(sig.to_string());
            }
        }

        let user_group = normalize_user_group(input.user_group.as_deref())?;

        let surface = input
            .surface
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("用户自定义导入表面")
            .to_string();

        // 在运行时规则表的对应规则上追加该用户目标的精确作用域（不修改内置目标）
        for sig in &dedup_signals {
            if let Some(rule) = self.rules.rules.iter_mut().find(|r| &r.id == sig) {
                let scope_exists = rule.applies_to.iter().any(|s| s.target_id == id);
                if !scope_exists {
                    rule.applies_to.push(RuleScope {
                        target_id: id.clone(),
                        host: parsed_url.host.clone(),
                        path: parsed_url.path.clone(),
                        surface: surface.clone(),
                    });
                }
            }
        }

        let bound_rules: Vec<&Rule> = dedup_signals
            .iter()
            .filter_map(|sig| self.rule(sig))
            .collect();
        let rule_digest = compute_rules_digest(&bound_rules);
        let now_str = crate::state::format_time(time::OffsetDateTime::now_utc());

        let defaults = self
            .entries
            .first()
            .map(|e| e.defaults.clone())
            .ok_or_else(|| ConfigError::field("registry", "缺少默认目录基线"))?;

        let raw = RawTarget {
            id: id.clone(),
            family: "user".to_string(),
            group: "user.custom".to_string(),
            matrix_group: "CONTROL".to_string(),
            url: normalized_url.clone(),
            surface: surface.clone(),
            signals: dedup_signals.clone(),
            confidence: Confidence::Low,
            confidence_basis: "用户自定义导入检查（USER_IMPORTED）".to_string(),
            failure: "用户自选规则未命中或网络不可达时返回 UNKNOWN/REACHABLE_ONLY".to_string(),
            protected: true,
            verification: "NO_HTTP".to_string(),
            allow_online: false,
            safe_default: false,
            observed_redirect_host: None,
            max_redirects: None,
        };

        let idx = self.entries.len();
        self.entries.push(Target { raw, defaults });
        self.by_id.insert(id.clone(), idx);

        let meta = UserTargetMeta {
            id: id.clone(),
            url: normalized_url,
            surface,
            signals: dedup_signals,
            user_group,
            origin: crate::contracts::ORIGIN_USER_IMPORTED.to_string(),
            consent_version: crate::contracts::LIABILITY_CONSENT_VERSION.to_string(),
            imported_at: now_str,
            rule_digest,
            allow_insecure_http: input.allow_insecure_http,
            http_warning,
            replaced_builtin_id: replaced_builtin_id.clone(),
            replaces_protected,
        };
        self.user_targets.insert(id, meta.clone());
        Ok(meta)
    }

    /// 移除一个用户目标（含其规则作用域），并重建 ID 索引。
    fn remove_user_target_entry(&mut self, id: &str) {
        if let Some(idx) = self.by_id.remove(id) {
            if idx < self.entries.len() {
                self.entries.remove(idx);
            }
        }
        self.user_targets.remove(id);
        self.authorized_user_targets.remove(id);
        for rule in &mut self.rules.rules {
            rule.applies_to.retain(|scope| scope.target_id != id);
        }
        self.by_id = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.id().to_string(), index))
            .collect();
    }

    /// F1：批量文本导入网址（每行一个 URL，或 `自定义ID | URL`，忽略空行与 `#` 注释）。
    pub fn import_user_targets_batch(
        &mut self,
        batch_text: &str,
        surface: Option<String>,
        signals: &[String],
        allow_insecure_http: bool,
        consent_accepted: bool,
    ) -> Result<Vec<UserTargetMeta>, ConfigError> {
        if !consent_accepted {
            return Err(ConfigError::field(
                "consent",
                "必须先阅读并勾选同意《自定义网址导入责任声明》",
            ));
        }
        let lines: Vec<&str> = batch_text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        if lines.is_empty() {
            return Err(ConfigError::field("batch", "批量输入内容为空"));
        }
        // 先在克隆快照上完整预校验，保证原子性（任一行非法则整批拒绝，不留半截脏数据）
        let mut staging = self.clone();
        let mut metas = Vec::with_capacity(lines.len());
        for (line_no, line) in lines.into_iter().enumerate() {
            let (custom_id, raw_url) = if let Some((left, right)) = line.split_once('|') {
                (Some(left.trim().to_string()), right.trim().to_string())
            } else {
                (None, line.to_string())
            };
            let meta = staging
                .import_user_target(
                    UserTargetImportInput {
                        id: custom_id,
                        url: raw_url,
                        surface: surface.clone(),
                        signals: signals.to_vec(),
                        allow_insecure_http,
                        user_group: None,
                    },
                    consent_accepted,
                )
                .map_err(|err| {
                    ConfigError::field(format!("第 {} 行", line_no + 1), err.to_string())
                })?;
            metas.push(meta);
        }
        *self = staging;
        Ok(metas)
    }

    /// F3：生成运行前显式确认提示语。
    /// 源码零发包模式仍只允许用户导入目标；发行包模式允许对用户逐次选择的内置目标确认。
    pub fn build_session_confirmation_prompt(
        &self,
        target_ids: &[String],
    ) -> Result<String, ConfigError> {
        if target_ids.is_empty() {
            return Err(ConfigError::field(
                "session_grant",
                "请先选择至少一个检测目标",
            ));
        }
        let mut includes_builtin = false;
        for id in target_ids {
            if self.user_targets.contains_key(id) {
                continue;
            }
            if self.distribution_mode && self.is_embedded_google_target(id) {
                includes_builtin = true;
                continue;
            }
            return Err(ConfigError::field(
                "session_grant",
                format!("目标 '{id}' 不是可授权目标；零发包模式下严禁对内置目标注入会话许可"),
            ));
        }
        let n = target_ids.len();
        if includes_builtin {
            Ok(format!(
                "您即将对 {n} 个已选择的内置目标发起匿名只读检测，这是您自身行为，产生后果本软件不负责。确认后仅在本次运行注入一次性、可撤销的本地会话许可（不持久化、不扩散到同域其它路径）。"
            ))
        } else {
            Ok(format!(
                "您即将对 {n} 个用户导入的网址发起 {n} 次匿名只读 GET，这是您自身行为，产生后果本软件不负责。确认后仅在本次运行注入一次性、可撤销的本地会话许可（不持久化、不扩散到同域其它路径、不提升内置目标）。"
            ))
        }
    }

    /// F3：在用户显式确认后注入一次性、仅本次运行有效的本地会话许可（session-scoped grant）。
    pub fn authorize_session_once(
        &self,
        target_ids: &[String],
        user_confirmed: bool,
    ) -> Result<SessionGrant, ConfigError> {
        if !user_confirmed {
            return Err(ConfigError::field(
                "session_grant",
                "未获用户运行前显式确认，拒绝注入一次性联网许可",
            ));
        }
        let prompt_summary = self.build_session_confirmation_prompt(target_ids)?;
        let now = time::OffsetDateTime::now_utc();
        let confirmed_at = crate::state::format_time(now);
        let grant_id = format!("session-grant-{}", now.unix_timestamp_nanos());
        let mut targets = BTreeMap::new();
        for id in target_ids {
            let target = self
                .get(id)
                .ok_or_else(|| ConfigError::field("session_grant", format!("未知目标 {id}")))?;
            let meta = self.user_targets.get(id);
            if meta.is_none() && !(self.distribution_mode && self.is_embedded_google_target(id)) {
                return Err(ConfigError::field(
                    "session_grant",
                    "仅允许授权用户导入目标或发行包模式下的内置目标",
                ));
            }
            let rules = self.rules_for(target);
            if rules.is_empty() {
                return Err(ConfigError::field(
                    "session_grant",
                    format!("目标 {id} 未绑定有效检查规则"),
                ));
            }
            let mut rule_versions = BTreeMap::new();
            for r in &rules {
                rule_versions.insert(r.id.clone(), r.version);
            }
            targets.insert(
                id.clone(),
                SessionTargetGrant {
                    target_id: id.clone(),
                    exact_url: target.raw.url.clone(),
                    surface: target.raw.surface.clone(),
                    rule_versions,
                    rule_digest: meta
                        .map(|meta| meta.rule_digest.clone())
                        .unwrap_or_else(|| "builtin".to_string()),
                },
            );
        }
        let grant = SessionGrant {
            grant_id,
            consent_version: crate::contracts::LIABILITY_CONSENT_VERSION.to_string(),
            confirmed_at,
            prompt_summary,
            targets,
        };
        if let Ok(mut slot) = self.active_session_grant.lock() {
            *slot = Some(grant.clone());
        }
        Ok(grant)
    }

    /// 是否为"更换内置目标"的用户目标（发行包默认规则包路径）。
    pub fn is_default_bundle_replacement(&self, target_id: &str) -> bool {
        self.user_targets
            .get(target_id)
            .is_some_and(|meta| meta.replaced_builtin_id.is_some())
    }

    /// 返回被更换的内置目标 ID（非"更换"目标返回 `None`）。
    pub fn replaced_builtin_id_of(&self, target_id: &str) -> Option<&str> {
        self.user_targets
            .get(target_id)
            .and_then(|meta| meta.replaced_builtin_id.as_deref())
    }

    /// 当前持有永久在线授权的"更换内置目标"用户目标 ID（稳定排序）。
    pub fn authorized_user_target_ids(&self) -> Vec<String> {
        self.authorized_user_targets.keys().cloned().collect()
    }

    /// 某"更换内置目标"用户目标是否持有永久在线授权。
    pub fn is_user_target_authorized(&self, target_id: &str) -> bool {
        self.authorized_user_targets.contains_key(target_id)
    }

    /// 校验并返回"更换内置目标"的精确授权边界（拒绝把授权扩散到普通导入目标）。
    fn default_bundle_scope(
        &self,
        target_ids: &[String],
    ) -> Result<Vec<DefaultBundleScopeEntry>, ConfigError> {
        if !self.distribution_mode {
            return Err(ConfigError::field(
                "default_bundle",
                "请先在设置→关于软件中确认开启发行包模式，再启用默认规则包",
            ));
        }
        if target_ids.is_empty() {
            return Err(ConfigError::field(
                "default_bundle",
                "请先选择至少一个发行包授权版目标",
            ));
        }
        let mut scoped = Vec::with_capacity(target_ids.len());
        let mut seen = HashSet::new();
        for id in target_ids {
            if !seen.insert(id.clone()) {
                continue;
            }
            let meta = self.user_targets.get(id).ok_or_else(|| {
                ConfigError::field(
                    "default_bundle",
                    format!("目标 {id} 不是用户导入目标，拒绝发行包授权"),
                )
            })?;
            if self.replaced_builtin_id_of(id).is_none() {
                return Err(ConfigError::field(
                    "default_bundle",
                    format!("目标 {id} 不是更换内置目标，拒绝发行包授权扩散"),
                ));
            }
            let target = self
                .get(id)
                .ok_or_else(|| ConfigError::field("default_bundle", format!("未知目标 {id}")))?;
            let rules = self.rules_for(target);
            if rules.is_empty() {
                return Err(ConfigError::field(
                    "default_bundle",
                    format!("目标 {id} 未绑定有效检查规则"),
                ));
            }
            let mut rule_versions = BTreeMap::new();
            for rule in rules {
                rule_versions.insert(rule.id.clone(), rule.version);
            }
            scoped.push((
                id.clone(),
                meta.url.clone(),
                meta.surface.clone(),
                meta.rule_digest.clone(),
                rule_versions,
            ));
        }
        Ok(scoped)
    }

    /// 默认规则包专属确认框选择「仅本次」：注入一次性会话许可（不落盘、退出即失效）。
    pub fn authorize_default_bundle_session(
        &self,
        target_ids: &[String],
        user_confirmed: bool,
    ) -> Result<SessionGrant, ConfigError> {
        self.default_bundle_scope(target_ids)?;
        self.authorize_session_once(target_ids, user_confirmed)
    }

    /// 默认规则包专属确认框选择「永久」：写入内存授权（由调用方持久化到用户状态目录）。
    pub fn authorize_default_bundle_permanent(
        &mut self,
        target_ids: &[String],
        granted_at: &str,
    ) -> Result<Vec<UserTargetAuthorization>, ConfigError> {
        let scoped = self.default_bundle_scope(target_ids)?;
        let mut out = Vec::with_capacity(scoped.len());
        for (id, url, surface, rule_digest, rule_versions) in scoped {
            if let Some(idx) = self.by_id.get(&id).copied() {
                if let Some(entry) = self.entries.get_mut(idx) {
                    entry.raw.allow_online = true;
                }
            }
            let auth = UserTargetAuthorization {
                target_id: id.clone(),
                exact_url: url,
                surface,
                rule_versions,
                rule_digest,
                consent_version: crate::contracts::LIABILITY_CONSENT_VERSION.to_string(),
                granted_at: granted_at.to_string(),
                source: "BUNDLED_DEFAULT_BUNDLE".to_string(),
            };
            self.authorized_user_targets.insert(id, auth.clone());
            out.push(auth);
        }
        Ok(out)
    }

    /// 撤销全部默认规则包永久授权（恢复零发包基线；不删除普通用户导入目标）。
    pub fn revoke_default_bundle_authorization(&mut self) {
        let ids: Vec<String> = self.authorized_user_targets.keys().cloned().collect();
        for id in ids {
            if let Some(idx) = self.by_id.get(&id).copied() {
                if let Some(entry) = self.entries.get_mut(idx) {
                    entry.raw.allow_online = false;
                }
            }
        }
        self.authorized_user_targets.clear();
        self.revoke_session_grant();
    }

    /// 把当前永久授权写入用户状态目录（严禁写回 `assets/`）。
    pub fn save_default_bundle_consent(&self, state_dir: &FsPath) -> Result<PathBuf, ConfigError> {
        let canonical_or_raw = state_dir.to_string_lossy().replace('\\', "/");
        if canonical_or_raw.contains("assets") {
            return Err(ConfigError::field(
                "state_dir",
                "严禁将默认规则包授权写入内置 assets/ 目录",
            ));
        }
        std::fs::create_dir_all(state_dir)
            .map_err(|e| ConfigError::Io(format!("无法创建用户状态目录：{e}")))?;
        let now = crate::state::format_time(time::OffsetDateTime::now_utc());
        let mut targets = Vec::with_capacity(self.authorized_user_targets.len());
        for (id, auth) in &self.authorized_user_targets {
            let meta = self.user_targets.get(id).ok_or_else(|| {
                ConfigError::field("default_bundle", format!("授权目标 {id} 缺少元数据"))
            })?;
            let replaced = meta.replaced_builtin_id.clone().ok_or_else(|| {
                ConfigError::field(
                    "default_bundle",
                    format!("授权目标 {id} 不是更换内置目标，拒绝落盘"),
                )
            })?;
            targets.push(PersistedDefaultBundleTarget {
                id: id.clone(),
                url: auth.exact_url.clone(),
                surface: auth.surface.clone(),
                signals: meta.signals.clone(),
                user_group: meta.user_group.clone(),
                allow_insecure_http: meta.allow_insecure_http,
                replaced_builtin_id: replaced,
                rule_versions: auth.rule_versions.clone(),
            });
        }
        targets.sort_by(|a, b| a.id.cmp(&b.id));
        let file = DefaultBundleConsentFile {
            schema: "parallaxe.default-bundle-consent/v1".to_string(),
            consent_version: crate::contracts::LIABILITY_CONSENT_VERSION.to_string(),
            granted_at: now,
            mode: "PERMANENT".to_string(),
            targets,
        };
        let text = serde_json::to_string_pretty(&file)
            .map_err(|e| ConfigError::Parse(format!("序列化默认规则包授权失败：{e}")))?;
        let path = state_dir.join("default-bundle-consent.json");
        std::fs::write(&path, text)
            .map_err(|e| ConfigError::Io(format!("写入默认规则包授权失败：{e}")))?;
        Ok(path)
    }

    /// 启动时从用户状态目录恢复默认规则包授权：
    /// - 先按持久快照重新导入"更换内置目标"；
    /// - 仅当规则版本仍与授权时一致，才恢复 `allow_online=true`；
    /// - 任一条目校验失败只跳过该条，不静默扩大授权范围。
    pub fn restore_default_bundle_state(
        &mut self,
        state_dir: &FsPath,
    ) -> Result<UserStateRestoreReport, ConfigError> {
        if !self.distribution_mode {
            return Ok(UserStateRestoreReport::default());
        }
        let path = state_dir.join("default-bundle-consent.json");
        if !path.exists() {
            return Ok(UserStateRestoreReport::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| ConfigError::Io(format!("读取默认规则包授权失败：{e}")))?;
        let file: DefaultBundleConsentFile = serde_json::from_str(&text)
            .map_err(|e| ConfigError::Parse(format!("默认规则包授权文件损坏：{e}")))?;
        if file.schema != "parallaxe.default-bundle-consent/v1" {
            return Err(ConfigError::field(
                "schema",
                format!("不支持的默认规则包授权 schema '{}'", file.schema),
            ));
        }
        if file.mode != "PERMANENT" {
            return Err(ConfigError::field("mode", "仅 PERMANENT 授权允许落盘恢复"));
        }
        let mut report = UserStateRestoreReport::default();
        for item in &file.targets {
            // 只允许恢复"确实更换了同名同 URL 内置目标"的条目。
            let builtin_ok = self
                .entries
                .iter()
                .take(self.base_target_count)
                .any(|entry| entry.raw.id == item.replaced_builtin_id && entry.raw.url == item.url);
            if !builtin_ok {
                report.skipped_targets += 1;
                continue;
            }
            let input = UserTargetImportInput {
                id: Some(item.id.clone()),
                url: item.url.clone(),
                surface: Some(item.surface.clone()),
                signals: item.signals.clone(),
                allow_insecure_http: item.allow_insecure_http,
                user_group: Some(item.user_group.clone()),
            };
            match self.import_user_target_replacing(input, true, true) {
                Ok(_) => {
                    report.restored_targets += 1;
                    let current: BTreeMap<String, u64> = self
                        .get(&item.id)
                        .map(|target| {
                            self.rules_for(target)
                                .into_iter()
                                .map(|rule| (rule.id.clone(), rule.version))
                                .collect()
                        })
                        .unwrap_or_default();
                    if current == item.rule_versions {
                        if let Some(idx) = self.by_id.get(&item.id).copied() {
                            if let Some(entry) = self.entries.get_mut(idx) {
                                entry.raw.allow_online = true;
                            }
                        }
                        let meta = self.user_targets.get(&item.id).cloned();
                        if let Some(meta) = meta {
                            self.authorized_user_targets.insert(
                                item.id.clone(),
                                UserTargetAuthorization {
                                    target_id: item.id.clone(),
                                    exact_url: meta.url.clone(),
                                    surface: meta.surface.clone(),
                                    rule_versions: item.rule_versions.clone(),
                                    rule_digest: meta.rule_digest.clone(),
                                    consent_version: file.consent_version.clone(),
                                    granted_at: file.granted_at.clone(),
                                    source: "BUNDLED_DEFAULT_BUNDLE".to_string(),
                                },
                            );
                            report.authorized_targets += 1;
                        }
                    } else {
                        report.skipped_targets += 1;
                    }
                }
                Err(_) => report.skipped_targets += 1,
            }
        }
        Ok(report)
    }

    /// 撤销并删除用户状态目录中的默认规则包授权，恢复零发包基线。
    pub fn revoke_default_bundle_state(&mut self, state_dir: &FsPath) -> Result<(), ConfigError> {
        self.revoke_default_bundle_authorization();
        let path = state_dir.join("default-bundle-consent.json");
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| ConfigError::Io(format!("删除默认规则包授权失败：{e}")))?;
        }
        Ok(())
    }

    /// F3：原子消费一次性会话许可（一旦被单次 run 取出，后续 run 自动回归零许可）。
    pub fn take_session_grant(&self) -> Option<SessionGrant> {
        self.active_session_grant
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    /// 查看当前是否有待消费的一次性会话许可快照。
    pub fn active_session_grant_snapshot(&self) -> Option<SessionGrant> {
        self.active_session_grant
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }

    /// F3：立即撤销当前尚未消费的一次性会话许可。
    pub fn revoke_session_grant(&self) {
        if let Ok(mut slot) = self.active_session_grant.lock() {
            *slot = None;
        }
    }

    /// 校验某个目标在给定的一次性会话许可下是否获准单次联网。
    pub fn check_session_grant_admission(
        &self,
        grant: &SessionGrant,
        target: &Target,
        implicit_default: bool,
        is_full_profile: bool,
    ) -> Admission {
        if implicit_default || is_full_profile {
            return Admission::deny("DENIED_SAFE_DEFAULT_FALSE", reason::SAFE_DEFAULT_FALSE);
        }
        if !self.is_user_imported(target.id())
            && !(self.distribution_mode && self.is_embedded_google_target(target.id()))
        {
            return Admission::deny("DENIED_NO_GRANT", reason::NO_ADMISSION);
        }
        let Some(entry) = grant.targets.get(target.id()) else {
            return Admission::deny("DENIED_NO_GRANT", reason::NO_ADMISSION);
        };
        if entry.exact_url != target.raw.url || entry.surface != target.raw.surface {
            return Admission::deny("DENIED_GRANT_SURFACE_MISMATCH", reason::ADMISSION_SURFACE);
        }
        let active_rules = self.rules_for(target);
        if active_rules.is_empty() {
            return Admission::deny("DENIED_NO_GRANT", reason::NO_ADMISSION);
        }
        for rule in active_rules {
            if entry.rule_versions.get(&rule.id).copied() != Some(rule.version) {
                return Admission::deny(
                    "DENIED_GRANT_RULE_VERSION_MISMATCH",
                    reason::ADMISSION_RULE_VERSION,
                );
            }
        }
        Admission::allow()
    }

    /// F3：撤销一次性许可并清除全部用户导入的目标与检查方式，恢复至纯内置目录状态。
    pub fn clear_user_imports(&mut self) {
        self.revoke_session_grant();
        self.authorized_user_targets.clear();
        let user_target_ids: HashSet<String> = self.user_targets.keys().cloned().collect();
        let user_rule_ids: HashSet<String> = self.user_rules.keys().cloned().collect();
        self.entries.truncate(self.base_target_count);
        self.by_id.retain(|id, _| !user_target_ids.contains(id));
        self.rules
            .rules
            .retain(|rule| !user_rule_ids.contains(&rule.id));
        for rule in &mut self.rules.rules {
            rule.applies_to
                .retain(|scope| !user_target_ids.contains(&scope.target_id));
        }
        self.user_targets.clear();
        self.user_rules.clear();
    }

    /// F2：将用户导入的检查方式保存到显式指定的用户态目录（严禁写回 `assets/`）。
    pub fn save_user_rules_to_state_dir(&self, state_dir: &FsPath) -> Result<PathBuf, ConfigError> {
        let canonical_or_raw = state_dir.to_string_lossy().replace('\\', "/");
        if canonical_or_raw.contains("assets") {
            return Err(ConfigError::field(
                "state_dir",
                "严禁将用户导入规则写入内置 assets/ 目录",
            ));
        }
        std::fs::create_dir_all(state_dir)
            .map_err(|e| ConfigError::Io(format!("无法创建用户态目录：{e}")))?;
        let user_rules: Vec<&Rule> = self
            .rules
            .rules
            .iter()
            .filter(|r| self.user_rules.contains_key(&r.id))
            .collect();
        let path = state_dir.join("user-imported-rules.toml");
        let doc = serde_json::json!({
            "origin": crate::contracts::ORIGIN_USER_IMPORTED,
            "consent_version": crate::contracts::LIABILITY_CONSENT_VERSION,
            "rules": user_rules,
        });
        let toml_str = toml::to_string_pretty(&doc)
            .map_err(|e| ConfigError::Parse(format!("序列化用户规则失败：{e}")))?;
        std::fs::write(&path, toml_str)
            .map_err(|e| ConfigError::Io(format!("写入用户规则文件失败：{e}")))?;
        Ok(path)
    }
}
