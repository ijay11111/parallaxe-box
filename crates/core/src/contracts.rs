//! 传输契约（v1）：报告、事件、覆盖、路径与四层结果的纯值类型。
//!
//! 字段名与 `docs/architecture/02-data-model.md` 的 JSON Schema 一一对应；
//! 本模块不含任何判定逻辑，也不依赖任何适配器。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::eligibility::EligibilityNote;

/// 报告 schema 常量。
pub const REPORT_SCHEMA: &str = "parallaxe.report/v1";
/// 事件信封 schema 常量。
pub const EVENT_SCHEMA: &str = "parallaxe.event/v1";
/// agent 请求 schema 常量。
pub const AGENT_SCHEMA: &str = "parallaxe.agent/v1";
/// 工具名常量。
pub const TOOL_NAME: &str = "parallaxe-box";

/// 顶层 state，恰好 9 个具名值（ADR-003）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    Ok,
    Restricted,
    Blocked,
    ReachableOnly,
    LocalPathError,
    Conflict,
    Unknown,
    SignalStale,
    Unsupported,
}

impl State {
    /// 是否属于「明确异常」，用于退出码 2 的归约。
    pub fn is_definite_anomaly(self) -> bool {
        matches!(
            self,
            State::Restricted | State::Blocked | State::LocalPathError
        )
    }

    /// 是否属于「不确定」，用于退出码 3 的归约。
    pub fn is_uncertain(self) -> bool {
        matches!(
            self,
            State::Unknown | State::Conflict | State::SignalStale | State::Unsupported
        )
    }

    /// 全部九个值，供 schema 测试与文档核对。
    pub const ALL: [State; 9] = [
        State::Ok,
        State::Restricted,
        State::Blocked,
        State::ReachableOnly,
        State::LocalPathError,
        State::Conflict,
        State::Unknown,
        State::SignalStale,
        State::Unsupported,
    ];

    /// 枚举文本（与 JSON 值一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            State::Ok => "OK",
            State::Restricted => "RESTRICTED",
            State::Blocked => "BLOCKED",
            State::ReachableOnly => "REACHABLE_ONLY",
            State::LocalPathError => "LOCAL_PATH_ERROR",
            State::Conflict => "CONFLICT",
            State::Unknown => "UNKNOWN",
            State::SignalStale => "SIGNAL_STALE",
            State::Unsupported => "UNSUPPORTED",
        }
    }
}

/// 证据可信度（针对某一 claim），不是来源等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Confidence {
    None,
    Low,
    Medium,
    High,
}

impl Confidence {
    /// 是否达到 Guard 候选的最低可信度（阶段 4 使用，本阶段只生成字段）。
    pub fn meets_guard_floor(self) -> bool {
        matches!(self, Confidence::High | Confidence::Medium)
    }
}

/// 来源类别；`Source` 不是可信度换算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Source {
    OfficialDoc,
    DirectAnonymousObservation,
    CommunityReport,
    LegacyCode,
    Inference,
}

/// 七种归因层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Layer {
    EgressNetwork,
    ServicePolicy,
    Account,
    Payments,
    Preference,
    Device,
    Cloud,
}

impl Layer {
    /// 固定顺序：报告中的 `region_layers` 必须按此顺序恰好 7 项。
    pub const ORDER: [Layer; 7] = [
        Layer::EgressNetwork,
        Layer::ServicePolicy,
        Layer::Account,
        Layer::Payments,
        Layer::Preference,
        Layer::Device,
        Layer::Cloud,
    ];
}

/// Evidence 的 layer 允许 `TRANSPORT` 伪层，不进入七层归因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayerRef {
    Layer(Layer),
    Transport(TransportLayer),
}

/// 传输伪层，序列化为字符串 `TRANSPORT`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransportLayer {
    #[serde(rename = "TRANSPORT")]
    Transport,
}

impl LayerRef {
    /// 是否参与地区归因。
    pub fn region_layer(self) -> Option<Layer> {
        match self {
            LayerRef::Layer(layer) => Some(layer),
            LayerRef::Transport(_) => None,
        }
    }
}

impl From<Layer> for LayerRef {
    fn from(value: Layer) -> Self {
        LayerRef::Layer(value)
    }
}

/// 路径请求的地址族意图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestedFamily {
    Auto,
    V4,
    V6,
}

/// 实际观察到的地址族。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservedFamily {
    V4,
    V6,
}

/// 路径绑定状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BindStatus {
    NotRequested,
    Verified,
    Unsupported,
    Unknown,
}

/// G5 路径描述：请求路径、实际绑定/对端、resolver 与出口观察位置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Path {
    pub path_id: String,
    pub requested_family: RequestedFamily,
    pub proxy_mode: String,
    pub interface_ref: Option<String>,
    pub proxy_ref: Option<String>,
    pub observed_interface: Option<String>,
    pub observed_family: Option<ObservedFamily>,
    pub resolver_path: Option<String>,
    pub connected_peer_ref: Option<String>,
    pub http_egress_ref: Option<String>,
    pub bind_status: BindStatus,
    pub via_tun: Option<bool>,
    pub reason: Option<String>,
}

impl Path {
    /// 默认系统选路路径。
    pub fn system(path_id: impl Into<String>) -> Self {
        Path {
            path_id: path_id.into(),
            requested_family: RequestedFamily::Auto,
            proxy_mode: "SYSTEM".to_string(),
            interface_ref: None,
            proxy_ref: None,
            observed_interface: None,
            observed_family: None,
            resolver_path: Some("SYSTEM_RESOLVER".to_string()),
            connected_peer_ref: None,
            http_egress_ref: None,
            bind_status: BindStatus::NotRequested,
            via_tun: None,
            reason: None,
        }
    }
}

/// 一次传输尝试（含 TLS 前失败）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub family: AttemptFamily,
    pub phase: String,
    pub connect_ms: Option<u64>,
    pub first_byte_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub fallback: bool,
    pub request_sent: bool,
}

/// 尝试记录的地址族：与 Path 的枚举不同，额外允许 `UNKNOWN`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttemptFamily {
    Auto,
    V4,
    V6,
    Unknown,
}

/// 新鲜度：由 core 产出，不由输入声明。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Freshness {
    pub status: FreshnessStatus,
    pub last_hit_at: Option<String>,
    pub comparable_misses: u64,
}

/// 新鲜度状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FreshnessStatus {
    Current,
    Missing,
    Stale,
    Historical,
    NotTracked,
}

impl Freshness {
    /// 未跟踪（传输证据、fixture 默认）。
    pub fn not_tracked() -> Self {
        Freshness {
            status: FreshnessStatus::NotTracked,
            last_hit_at: None,
            comparable_misses: 0,
        }
    }
}

/// 单条证据：只含白名单字段，不含原始报文。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub target_id: String,
    pub sample_id: String,
    pub rule_id: String,
    pub rule_version: u64,
    pub observed_at: String,
    pub layer: LayerRef,
    pub claim: String,
    pub value: Value,
    pub source_type: Source,
    pub source_ref: String,
    pub confidence: Confidence,
    pub path_id: String,
    pub origin: String,
    pub reason: Option<String>,
    pub fixture: bool,
    pub freshness: Freshness,
}

/// 七层中的一项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerValue {
    pub layer: Layer,
    pub value: Option<String>,
    pub reason: Option<String>,
}

/// 四层结果中的 transport。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportLayerResult {
    pub attempts: Vec<Attempt>,
    pub path_diagnostic: Option<String>,
}

/// 四层结果中的 service_observed。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceObserved {
    pub surface: String,
    pub observed_country: Option<String>,
    pub evidence_ids: Vec<String>,
    pub reason: Option<String>,
}

/// 四层结果中的 risk_signal。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskSignal {
    pub markers: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub reason: Option<String>,
}

/// 四层结果中的 local_policy。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPolicy {
    pub protected: bool,
    pub guard_eligible: bool,
    pub plan_id: Option<String>,
    pub reason: Option<String>,
}

/// 同层冲突。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub claim: String,
    pub evidence_ids: Vec<String>,
}

/// 分桶投票中的一票。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub value: String,
    pub sample_ids: Vec<String>,
}

/// 分桶投票结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agreement {
    pub bucket: String,
    pub votes: Vec<Vote>,
    pub winner: Option<String>,
    pub tie: bool,
}

/// `--expect` 的比较结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExpectationStatus {
    Match,
    Mismatch,
    NotObservable,
}

/// `--expect` 的比较对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expectation {
    pub key: String,
    pub expected: String,
    pub actual: Option<String>,
    pub status: ExpectationStatus,
}

/// 单目标结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultRecord {
    #[serde(default)]
    pub destination: crate::destination::Destination,
    pub target: String,
    pub state: State,
    pub reason: Option<String>,
    pub confidence: Confidence,
    pub source: Option<Source>,
    pub path: Path,
    pub duration_ms: u64,
    pub request_count: u64,
    pub evidence: Vec<Evidence>,
    pub region_layers: Vec<LayerValue>,
    pub observed: Vec<String>,
    pub conflicts: Vec<Conflict>,
    pub agreement: Vec<Agreement>,
    pub transport: TransportLayerResult,
    pub service_observed: ServiceObserved,
    pub risk_signal: RiskSignal,
    pub local_policy: LocalPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expectation: Option<Expectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_digest: Option<String>,
    /// 静态资格提示：不参与判定，仅解释“访问可达 ≠ 订阅/完整功能可用”。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub eligibility_notes: Vec<EligibilityNote>,
}

/// 覆盖集合中的已覆盖项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Covered {
    pub target: String,
    pub request_count: u64,
    pub http_received: bool,
}

/// 覆盖集合中的未覆盖项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Uncovered {
    pub target: String,
    pub selected: bool,
    pub reason: String,
}

/// 两栏覆盖快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    pub catalog_size: u64,
    pub selected_count: u64,
    pub covered: Vec<Covered>,
    pub uncovered: Vec<Uncovered>,
}

impl Coverage {
    /// 空覆盖：目录已知、未选任何项。
    pub fn empty(catalog_size: usize) -> Self {
        Coverage {
            catalog_size: catalog_size as u64,
            selected_count: 0,
            covered: Vec::new(),
            uncovered: Vec::new(),
        }
    }
}

/// 运行终止原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Termination {
    Complete,
    Cancelled,
    GlobalTimeout,
    Failed,
    Partial,
}

impl Termination {
    /// JSON 枚举值。
    pub fn as_str(self) -> &'static str {
        match self {
            Termination::Complete => "COMPLETE",
            Termination::Cancelled => "CANCELLED",
            Termination::GlobalTimeout => "GLOBAL_TIMEOUT",
            Termination::Failed => "FAILED",
            Termination::Partial => "PARTIAL",
        }
    }
}

/// 运行模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mode {
    Online,
    Fixture,
}

/// 影响闭包中的一个节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosureNode {
    pub node: String,
    pub via: Option<String>,
    pub kind: String,
    pub source_ref: String,
    pub role: ClosureRole,
}

/// 闭包节点角色：受影响 / 关联检查。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClosureRole {
    Affected,
    Associated,
}

/// Guard 预览计划（纯数据，阶段 4 才计算闭包；本阶段只生成拒绝性预览）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardPlan {
    pub plan_id: String,
    pub run_id: String,
    pub selected_by: String,
    pub operation: String,
    pub candidates: Vec<String>,
    pub affected_domains: Vec<String>,
    pub affected_services: Vec<String>,
    pub affected_shared_paths: Vec<String>,
    pub impact_closure: Vec<ClosureNode>,
    pub reasons: Vec<String>,
    pub primary_reason: Option<String>,
    pub safe_to_apply: bool,
    pub closure_complete: bool,
    pub applied: bool,
    pub rollback: Vec<String>,
    pub limitations: Vec<String>,
    pub effect_scope: String,
    pub remote_account_impact: String,
    pub shared_egress_guarantee: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
}

/// 报告封套（`parallaxe.report/v1`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    /// Observation boundary. Historical and desktop reports omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ObservationScope>,
    /// Optional run origin audit tag (e.g. `AGENT_AUTONOMOUS` for opt-in autonomous AI runs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Explicitly records when the session-only AI autonomous detection switch was active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autonomous_mode: Option<bool>,
    pub schema: String,
    pub tool: String,
    pub version: String,
    pub generated_at: String,
    pub run_id: String,
    pub mode: Mode,
    pub results: Vec<ResultRecord>,
    pub coverage: Coverage,
    pub plans: Vec<GuardPlan>,
    pub wall_duration_ms: u64,
    pub network_request_count: u64,
    pub simulated_request_count: u64,
    pub termination: Termination,
    pub exit_code: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservationScope {
    AndroidSimulatedUser,
}
impl ObservationScope {
    pub fn for_platform(os: &str) -> Option<Self> {
        (os == "android").then_some(Self::AndroidSimulatedUser)
    }
    pub fn description(self) -> &'static str {
        "ANDROID_SIMULATED_USER — App 自身匿名请求，非系统级观测 / anonymous app requests only"
    }
}

impl Report {
    /// 用给定字段构造报告，schema/tool 固定。
    pub fn new(
        version: impl Into<String>,
        generated_at: impl Into<String>,
        run_id: impl Into<String>,
        mode: Mode,
    ) -> Self {
        Report {
            scope: ObservationScope::for_platform(std::env::consts::OS),
            origin: None,
            autonomous_mode: None,
            schema: REPORT_SCHEMA.to_string(),
            tool: TOOL_NAME.to_string(),
            version: version.into(),
            generated_at: generated_at.into(),
            run_id: run_id.into(),
            mode,
            results: Vec::new(),
            coverage: Coverage::empty(0),
            plans: Vec::new(),
            wall_duration_ms: 0,
            network_request_count: 0,
            simulated_request_count: 0,
            termination: Termination::Complete,
            exit_code: 0,
        }
    }
}

/// 十种事件名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventName {
    RunStarted,
    PreflightReady,
    TargetStarted,
    EvidenceObserved,
    TargetFinished,
    CoverageUpdated,
    GuardPlanReady,
    RunFinished,
    RunCancelled,
    Error,
}

impl EventName {
    /// 事件名文本（与变体名一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            EventName::RunStarted => "RunStarted",
            EventName::PreflightReady => "PreflightReady",
            EventName::TargetStarted => "TargetStarted",
            EventName::EvidenceObserved => "EvidenceObserved",
            EventName::TargetFinished => "TargetFinished",
            EventName::CoverageUpdated => "CoverageUpdated",
            EventName::GuardPlanReady => "GuardPlanReady",
            EventName::RunFinished => "RunFinished",
            EventName::RunCancelled => "RunCancelled",
            EventName::Error => "Error",
        }
    }
}

/// 事件信封：`{schema,run_id,seq,ts,is_fixture,event,payload}`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub schema: String,
    pub run_id: String,
    pub seq: u64,
    pub ts: String,
    pub is_fixture: bool,
    pub event: EventName,
    pub payload: Value,
}

impl Event {
    /// 构造一条事件。
    pub fn new(
        run_id: impl Into<String>,
        seq: u64,
        ts: impl Into<String>,
        is_fixture: bool,
        event: EventName,
        payload: Value,
    ) -> Self {
        Event {
            schema: EVENT_SCHEMA.to_string(),
            run_id: run_id.into(),
            seq,
            ts: ts.into(),
            is_fixture,
            event,
            payload,
        }
    }
}

/// 匿名约束常量（写入报告/自检，防止被适配器改写）。
pub const ACCOUNT_MODE: &str = "PROHIBITED";
/// 会话模式常量。
pub const SESSION_MODE: &str = "ANONYMOUS_ONLY";
/// 凭据来源常量：必须为空。
pub const CREDENTIAL_SOURCES: &[&str] = &[];
/// 浏览器资料来源常量：必须为空。
pub const BROWSER_PROFILE_SOURCES: &[&str] = &[];
/// 用户导入来源审计常量。
pub const ORIGIN_USER_IMPORTED: &str = "USER_IMPORTED";
/// AI 自主调试模式来源审计常量（仅在用户手动打开会话级开关时标注）。
pub const ORIGIN_AGENT_AUTONOMOUS: &str = "AGENT_AUTONOMOUS";
/// 用户导入责任声明同意版本常量。
pub const LIABILITY_CONSENT_VERSION: &str = "liability-v1";
/// 导入网址强制责任声明原文（必须在界面显式展示并经用户主动勾选）。
pub const USER_TARGET_LIABILITY_NOTICE: &str = "本软件支持导入并检查自定义网址，但该检查由用户主动发起，属于用户自身行为。请在自有或获明确授权的网络与目标上使用，并遵守适用法律、目标服务条款与隐私政策。因使用本功能产生的任何后果（包括但不限于访问被拒、账号风控、带宽/费用、法律或合规争议），由用户自行承担，本软件作者与贡献者不承担责任。本软件不提供绕过付费、地区限制、风控或验证码的能力，也不会为任何违法或未经授权的使用负责。";
/// 导入检查方式强制责任声明原文（必须在界面显式展示并经用户主动勾选）。
pub const USER_RULE_LIABILITY_NOTICE: &str = "您正在导入的检查方式不是本软件内置、开发、测试或背书的功能，属于用户自行提供的定义。本软件仅按通用规则解析并执行该定义，不对该检查方式的正确性、安全性、合法性或可能造成的后果负责。由此产生的任何结果、损害或争议（包括但不限于对目标站的过量请求、隐私暴露、误判或合规问题），由用户自行承担全部责任，本软件作者与贡献者不承担责任。";

/// 常用 reason 码；schema 中 reason 为自由字符串，此处集中定义避免拼写漂移。
pub mod reason {
    pub const NO_ADMISSION: &str = "NO_ADMISSION";
    pub const NOT_SELECTED: &str = "NOT_SELECTED";
    pub const ADMISSION_GRANT_EXPIRED: &str = "GRANT_EXPIRED";
    pub const ADMISSION_RULE_VERSION: &str = "GRANT_RULE_VERSION_MISMATCH";
    pub const ADMISSION_SURFACE: &str = "GRANT_SURFACE_MISMATCH";
    pub const ALLOW_ONLINE_FALSE: &str = "ALLOW_ONLINE_FALSE";
    pub const SAFE_DEFAULT_FALSE: &str = "SAFE_DEFAULT_FALSE";
    pub const FIELD_MISSING: &str = "FIELD_MISSING";
    pub const NO_REGION_SIGNAL: &str = "NO_REGION_SIGNAL";
    pub const REDIRECT_TO_LOGIN: &str = "REDIRECT_TO_LOGIN";
    pub const CONSENT_REQUIRED: &str = "CONSENT_REQUIRED";
    pub const CAPTCHA_CHALLENGE: &str = "CAPTCHA_CHALLENGE";
    pub const DENIED_OR_AUTH: &str = "DENIED_OR_AUTH";
    pub const RATE_LIMITED: &str = "RATE_LIMITED";
    pub const QOS_HTTP_STOP: &str = "QOS_HTTP_403_429_STOP";
    pub const CHALLENGE_PAGE: &str = "CHALLENGE_PAGE";
    pub const TRUNCATED: &str = "TRUNCATED";
    pub const TRANSPORT_TIMEOUT: &str = "TRANSPORT_TIMEOUT";
    pub const TRANSPORT_CONNECT: &str = "TRANSPORT_CONNECT";
    pub const TRANSPORT_RESOLVE: &str = "TRANSPORT_RESOLVE";
    pub const TRANSPORT_TLS: &str = "TRANSPORT_TLS";
    pub const TRANSPORT_OTHER: &str = "TRANSPORT_OTHER";
    pub const UNEXPECTED_RESPONSE: &str = "UNEXPECTED_RESPONSE";
    pub const STALE_AFTER_MISSES: &str = "STALE_AFTER_MISSES";
    pub const CONFLICTING_EVIDENCE: &str = "CONFLICTING_EVIDENCE";
    pub const EDITION_ONLY: &str = "EDITION_ONLY_NO_EGRESS";
    pub const UNSUPPORTED_CAPABILITY: &str = "UNSUPPORTED_CAPABILITY";
    pub const INCONCLUSIVE: &str = "INCONCLUSIVE";
    pub const CANCELLED: &str = "CANCELLED";
    pub const GLOBAL_TIMEOUT: &str = "GLOBAL_TIMEOUT";
    pub const BUDGET_EXHAUSTED: &str = "BUDGET_EXHAUSTED";
    pub const GUARD_STAGE4_ONLY: &str = "GUARD_STAGE4_ONLY";
    pub const PROTECTED_HOST: &str = "PROTECTED_HOST";
    pub const NOT_GUARD_ELIGIBLE: &str = "NOT_GUARD_ELIGIBLE";
    pub const NO_SAFE_BLOCK: &str = "NO_SAFE_BLOCK";
    pub const UNMAPPED_COUNTRY: &str = "UNMAPPED_COUNTRY_CODE";
    pub const NOT_OBSERVABLE: &str = "NOT_OBSERVABLE_ANONYMOUSLY";
    pub const NOT_COLLECTED: &str = "NOT_COLLECTED";
    pub const PUBLIC_EDITION: &str = "PUBLIC_EDITION";
    pub const PROXY_REMOTE_UNVERIFIED: &str = "PROXY_REMOTE_UNVERIFIED";
    pub const INTERFACE_UNVERIFIED: &str = "INTERFACE_UNVERIFIED";
    pub const FIXTURE_ONLY: &str = "FIXTURE_ONLY";
    pub const STALE_DIR_UNWRITABLE: &str = "STALE_DIR_UNWRITABLE";
    pub const SCOPE_WASM_OFFLINE: &str = "SCOPE_WASM_OFFLINE";
    pub const REGION_RESTRICTED: &str = "REGION_RESTRICTED";
    pub const POLICY_BLOCKED: &str = "POLICY_BLOCKED";
}

/// 把三字母国家码归一到两字母，供 `--expect` 与投票比较；未知返回 None。
pub fn normalize_country(code: &str) -> Option<String> {
    let upper = code.trim().to_ascii_uppercase();
    let mapped = match upper.as_str() {
        "USA" => "US",
        "CHN" => "CN",
        "GBR" => "GB",
        "JPN" => "JP",
        "DEU" => "DE",
        "SGP" => "SG",
        "HKG" => "HK",
        "TWN" => "TW",
        "KOR" => "KR",
        "FRA" => "FR",
        "NLD" => "NL",
        "CAN" => "CA",
        "AUS" => "AU",
        "IND" => "IN",
        "RUS" => "RU",
        other => other,
    };
    if mapped.len() == 2 && mapped.chars().all(|c| c.is_ascii_uppercase()) {
        Some(mapped.to_string())
    } else {
        None
    }
}
