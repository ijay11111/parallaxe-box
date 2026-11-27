//! `forensics`：T-7-09 网络取证安全（调度预算之外的证据模型）。
//!
//! 这里只做**可解释的证据分类**，不新增 state / reason 码，也不改判定链路：
//! 节点绑定与路线一致性、本地 QoS 疑似、Google 返回契约、双栈分类、波次级 DNS 缓存。
//! 所有标记复用 `risk_signal.markers` / `Freshness` / `Confidence`，结论无法证明时保持
//! UNKNOWN / CONFLICT / SIGNAL_STALE。

use std::collections::BTreeMap;

use crate::contracts::{FreshnessStatus, ObservedFamily, Report, ResultRecord, State};

/// 取证元数据 schema（报告内嵌字段仍沿用 `parallaxe.report/v1`）。
pub const FORENSIC_SCHEMA: &str = "parallaxe.forensics/v1";

/// 探针协议：HTTP(S) 走 TCP/TLS；UDP/QUIC 仅作为独立会话类型建模。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProbeProtocol {
    TcpHttp,
    UdpQuic,
}

impl ProbeProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            ProbeProtocol::TcpHttp => "TCP_HTTP",
            ProbeProtocol::UdpQuic => "UDP_QUIC",
        }
    }
}

impl std::fmt::Display for ProbeProtocol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 单次请求的节点绑定记录（T-7-09b）。
///
/// 名称只作标签，实际出口由 Google 地区信号单独记录；两者不一致时报告必须提示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingRecord {
    pub node_label: String,
    pub node_id: String,
    pub remote: String,
    pub protocol: ProbeProtocol,
    pub egress_ip: Option<String>,
    pub family: Option<ObservedFamily>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// 路线一致性结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteConsistency {
    Consistent,
    /// 连续请求之间节点 / 远端 / 出口发生变化：整组结果不得合并。
    Inconsistent {
        field: &'static str,
    },
    /// 没有可比较的绑定信息，不能宣称一致。
    Unverifiable,
}

/// 比较一组请求的节点 ID、远端与出口 IP。
pub fn route_consistency(records: &[BindingRecord]) -> RouteConsistency {
    let mut iter = records.iter();
    let Some(first) = iter.next() else {
        return RouteConsistency::Unverifiable;
    };
    for item in iter {
        if item.node_id != first.node_id {
            return RouteConsistency::Inconsistent { field: "node_id" };
        }
        if item.remote != first.remote {
            return RouteConsistency::Inconsistent { field: "remote" };
        }
        // 只有当两侧都观测到出口时才比较；缺观测保持不可验证，不猜。
        match (&item.egress_ip, &first.egress_ip) {
            (Some(left), Some(right)) if left != right => {
                return RouteConsistency::Inconsistent { field: "egress_ip" }
            }
            (None, _) | (_, None) => return RouteConsistency::Unverifiable,
            _ => {}
        }
    }
    RouteConsistency::Consistent
}

impl RouteConsistency {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteConsistency::Consistent => "CONSISTENT",
            RouteConsistency::Inconsistent { field } => match *field {
                "node_id" => "INCONSISTENT_NODE",
                "remote" => "INCONSISTENT_REMOTE",
                _ => "INCONSISTENT_EGRESS",
            },
            RouteConsistency::Unverifiable => "UNVERIFIABLE",
        }
    }
}

/// 单节点单协议的观测点（不包含未探测节点）。
#[derive(Debug, Clone)]
pub struct ObservationPoint {
    pub protocol: ProbeProtocol,
    pub node_label: String,
    pub airport: String,
    pub success: bool,
    pub timed_out: bool,
    pub first_byte_ms: Option<u64>,
}

/// 本地 QoS 识别结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QosFinding {
    pub suspect_local_qos: bool,
    pub markers: Vec<String>,
    /// 需要暂停的协议（只暂停，不重发，也不把未测节点标为故障）。
    pub paused_protocols: Vec<ProbeProtocol>,
    pub detail: String,
}

/// 识别“TCP 正常 + UDP/QUIC 大面积超时 / 多机场同挂 / 首包整体上升”。
pub fn detect_local_qos(points: &[ObservationPoint]) -> QosFinding {
    detect_local_qos_against(points, None)
}

/// 带基线的版本：只有提供历史首包基线时才判断“首包整体上升”，避免自证。
pub fn detect_local_qos_against(
    points: &[ObservationPoint],
    baseline_first_byte_ms: Option<u64>,
) -> QosFinding {
    let tcp_ok = points
        .iter()
        .any(|point| point.protocol == ProbeProtocol::TcpHttp && point.success);
    let udp: Vec<&ObservationPoint> = points
        .iter()
        .filter(|point| point.protocol == ProbeProtocol::UdpQuic)
        .collect();
    let udp_timeouts = udp.iter().filter(|point| point.timed_out).count();
    let mut markers: Vec<String> = Vec::new();
    let mut paused = Vec::new();

    let consecutive_udp_timeouts = tcp_ok && udp_timeouts >= 2;
    if consecutive_udp_timeouts {
        markers.push("LOCAL_QOS_SUSPECT".to_string());
        paused.push(ProbeProtocol::UdpQuic);
    }
    let mut airports: Vec<&str> = udp
        .iter()
        .filter(|point| point.timed_out)
        .map(|point| point.airport.as_str())
        .filter(|name| !name.is_empty())
        .collect();
    airports.sort_unstable();
    airports.dedup();
    if tcp_ok && airports.len() >= 2 {
        markers.push("MULTI_AIRPORT_UDP_FAILURE".to_string());
        if !paused.contains(&ProbeProtocol::UdpQuic) {
            paused.push(ProbeProtocol::UdpQuic);
        }
    }

    // 首包整体上升：成功样本的中位数超过基线的 3 倍且至少增加 200ms。
    let mut first_bytes: Vec<u64> = points
        .iter()
        .filter(|point| point.success)
        .filter_map(|point| point.first_byte_ms)
        .collect();
    first_bytes.sort_unstable();
    if let (Some(baseline), Some(median)) = (
        baseline_first_byte_ms,
        first_bytes.get(first_bytes.len() / 2).copied(),
    ) {
        if median > baseline.saturating_mul(3) && median > baseline + 200 {
            markers.push("FIRST_BYTE_RISE".to_string());
        }
    }

    let suspect_local_qos = markers
        .iter()
        .any(|marker| marker == "LOCAL_QOS_SUSPECT" || marker == "MULTI_AIRPORT_UDP_FAILURE");
    let detail = if suspect_local_qos {
        "TCP 对照正常而 UDP/QUIC 连续或跨机场超时：疑似本地 QoS。暂停同协议后续探测，未测节点不标故障，冷却后由用户显式恢复。"
            .to_string()
    } else if markers.iter().any(|marker| marker == "FIRST_BYTE_RISE") {
        "首包时间整体上升：记录为质量证据，不改判定；建议冷却后再测。".to_string()
    } else {
        "未观察到本地 QoS 组合特征。".to_string()
    };
    QosFinding {
        suspect_local_qos,
        markers,
        paused_protocols: paused,
        detail,
    }
}

/// Google 目标返回契约（T-7-09c）。
#[derive(Debug, Clone)]
pub struct ServiceContract {
    pub target_id: String,
    pub allowed_final_hosts: Vec<String>,
    pub max_redirects: u8,
    pub expected_status: Vec<u16>,
    pub allowed_content_types: Vec<String>,
    pub required_markers: Vec<String>,
    pub challenge_markers: Vec<String>,
    pub login_markers: Vec<String>,
    pub region_source: RegionSignalSource,
    pub max_body_bytes: u64,
    pub contract_version: u32,
    pub verified_at: String,
}

/// 地区信号来源；来源可信度由来源本身决定，不按页面语言推断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionSignalSource {
    InlineCountryCode,
    YouTubeGl,
    NewsGlCeid,
    PlayRegionStructure,
    StructuredPolicy,
}

impl RegionSignalSource {
    pub fn as_str(self) -> &'static str {
        match self {
            RegionSignalSource::InlineCountryCode => "INLINE_COUNTRY_CODE",
            RegionSignalSource::YouTubeGl => "YOUTUBE_GL",
            RegionSignalSource::NewsGlCeid => "NEWS_GL_CEID",
            RegionSignalSource::PlayRegionStructure => "PLAY_REGION_STRUCTURE",
            RegionSignalSource::StructuredPolicy => "STRUCTURED_POLICY",
        }
    }
}

/// 明确禁止作为地区判据的内容（`判断-*` 文档）。
pub const FORBIDDEN_REGION_CRITERIA: [&str; 7] = [
    "PAGE_LANGUAGE",
    "DOMAIN_SUFFIX",
    "RESOLVED_IP_GEOIP",
    "SERVER_HEADER_ALONE",
    "HTTP_200_ALONE",
    "NODE_NAME_COUNTRY",
    "CURRENCY_FORMAT",
];

pub fn is_forbidden_criterion(name: &str) -> bool {
    FORBIDDEN_REGION_CRITERIA
        .iter()
        .any(|item| item.eq_ignore_ascii_case(name))
}

/// 一次契约核验所需的响应事实。
#[derive(Debug, Clone)]
pub struct ContractResponse {
    pub final_host: String,
    pub redirects: u8,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
    pub region: Option<String>,
    pub body_truncated: bool,
}

/// 契约核验结论：地区限制、限流、挑战、登录页与结构不符必须分开。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractOutcome {
    Matched { region: String },
    RegionRestricted,
    PolicyBlocked,
    NoRegionSignal,
    Challenge,
    RateLimited { status: u16 },
    LoginPage,
    LocalInjection,
    StructureMismatch { missing: String },
    RedirectOutOfRange,
    HostMismatch,
    StatusUnexpected,
    BodyTruncated,
}

impl ContractOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContractOutcome::Matched { .. } => "MATCHED",
            ContractOutcome::RegionRestricted => "REGION_RESTRICTED",
            ContractOutcome::PolicyBlocked => "POLICY_BLOCKED",
            ContractOutcome::NoRegionSignal => "NO_REGION_SIGNAL",
            ContractOutcome::Challenge => "CHALLENGE",
            ContractOutcome::RateLimited { .. } => "RATE_LIMITED",
            ContractOutcome::LoginPage => "LOGIN_PAGE",
            ContractOutcome::LocalInjection => "LOCAL_INJECTION",
            ContractOutcome::StructureMismatch { .. } => "STRUCTURE_MISMATCH",
            ContractOutcome::RedirectOutOfRange => "REDIRECT_OUT_OF_RANGE",
            ContractOutcome::HostMismatch => "HOST_MISMATCH",
            ContractOutcome::StatusUnexpected => "STATUS_UNEXPECTED",
            ContractOutcome::BodyTruncated => "BODY_TRUNCATED",
        }
    }
}

/// 按目标专属契约核验；200 不等于 OK，429/503/挑战页不自动映射为地区受限。
pub fn evaluate_contract(
    contract: &ServiceContract,
    response: &ContractResponse,
) -> ContractOutcome {
    let lower_body = response.body.to_ascii_lowercase();
    if contract
        .challenge_markers
        .iter()
        .any(|marker| lower_body.contains(&marker.to_ascii_lowercase()))
    {
        return ContractOutcome::Challenge;
    }
    if contract
        .login_markers
        .iter()
        .any(|marker| lower_body.contains(&marker.to_ascii_lowercase()))
    {
        return ContractOutcome::LoginPage;
    }
    if lower_body.contains("captive") || lower_body.contains("proxy authentication") {
        return ContractOutcome::LocalInjection;
    }
    if matches!(response.status, 429 | 503) {
        return ContractOutcome::RateLimited {
            status: response.status,
        };
    }
    if !contract.allowed_final_hosts.is_empty()
        && !contract
            .allowed_final_hosts
            .iter()
            .any(|host| host.eq_ignore_ascii_case(&response.final_host))
    {
        return ContractOutcome::HostMismatch;
    }
    if response.redirects > contract.max_redirects {
        return ContractOutcome::RedirectOutOfRange;
    }
    if let Some(policy) = product_policy(&contract.target_id, response) {
        return policy;
    }
    if !contract.expected_status.contains(&response.status) {
        return ContractOutcome::StatusUnexpected;
    }
    if response.content_type.is_none() && !contract.allowed_content_types.is_empty() {
        return ContractOutcome::StructureMismatch {
            missing: "CONTENT_TYPE".into(),
        };
    }
    if let Some(content_type) = &response.content_type {
        if !contract.allowed_content_types.is_empty()
            && !contract
                .allowed_content_types
                .iter()
                .any(|allowed| content_type.starts_with(allowed))
        {
            return ContractOutcome::StructureMismatch {
                missing: "CONTENT_TYPE".to_string(),
            };
        }
    }
    if response.body_truncated {
        return ContractOutcome::BodyTruncated;
    }
    if let Some(missing) = contract
        .required_markers
        .iter()
        .find(|marker| !lower_body.contains(&marker.to_ascii_lowercase()))
    {
        return ContractOutcome::StructureMismatch {
            missing: missing.clone(),
        };
    }
    match &response.region {
        Some(region) if !region.is_empty() => ContractOutcome::Matched {
            region: region.clone(),
        },
        _ => ContractOutcome::NoRegionSignal,
    }
}

/// A denial must describe this request on the product's own, bounded first response.
/// Match entire structured fields, never translation resources or text snippets in a page.
fn product_policy(target_id: &str, r: &ContractResponse) -> Option<ContractOutcome> {
    if r.body_truncated || r.redirects != 0 || r.body.len() > 32 * 1024 {
        return None;
    }
    let mime = r.content_type.as_deref()?.split(';').next()?.trim();
    if (target_id, r.final_host.as_str()) == ("google.gemini", "support.google.com")
        && r.status == 200
        && mime.eq_ignore_ascii_case("text/html")
    {
        // Standalone denial surface. Ignore HTML with scripts, templates, samples or other text.
        static DENIAL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let heading = DENIAL.get_or_init(||regex::Regex::new(r"(?i)^\s*(?:<!doctype html>\s*)?<html>\s*<body>\s*<main>\s*<h1>\s*Gemini (?:is not available|isn['’]t (?:currently )?supported) in your country\.?\s*</h1>\s*</main>\s*</body>\s*</html>\s*$").expect("constant regex"));
        return heading
            .is_match(&r.body)
            .then_some(ContractOutcome::RegionRestricted);
    }
    if !matches!(target_id, "google.gen-api" | "google.models")
        || r.final_host != "generativelanguage.googleapis.com"
        || !mime.eq_ignore_ascii_case("application/json")
    {
        return None;
    }
    let payload: serde_json::Value = serde_json::from_str(&r.body).ok()?;
    let error = payload.get("error")?;
    if error.get("code")?.as_u64()? != u64::from(r.status) {
        return None;
    }
    match (
        r.status,
        error.get("status")?.as_str()?,
        error.get("message")?.as_str()?,
    ) {
        (400, "FAILED_PRECONDITION", "User location is not supported for the API use.") => {
            Some(ContractOutcome::RegionRestricted)
        }
        (451, "PERMISSION_DENIED", "Unavailable For Legal Reasons") => {
            Some(ContractOutcome::PolicyBlocked)
        }
        _ => None,
    }
}

pub(crate) fn policy_product(url: &str) -> Option<&'static str> {
    match url {
        "https://generativelanguage.googleapis.com/v1beta/models"
        | "https://generativelanguage.googleapis.com/v1/models" => Some("google.models"),
        "https://generativelanguage.googleapis.com/" => Some("google.gen-api"),
        "https://support.google.com/gemini/" => Some("google.gemini"),
        _ => None,
    }
}

/// Evaluate the first-party policy contract before reducing evidence to a verdict.
/// An imported alias must match an exact canonical HTTPS URL.
pub(crate) fn policy_outcome(
    target: &crate::targets::Target,
    sample: &crate::signals::Sample,
) -> Option<ContractOutcome> {
    let product = policy_product(&target.raw.url)?;
    if !sample.complete
        || sample.truncated
        || sample.location.is_some()
        || sample.body.len() > 32 * 1024
    {
        return None;
    }
    let response = ContractResponse {
        final_host: target.host(),
        redirects: 0,
        status: sample.status?,
        content_type: sample.mime(),
        body: sample.text(),
        region: None,
        body_truncated: false,
    };
    product_policy(product, &response)
}

/// Only versioned catalog surfaces carry a service-specific contract. A Location header
/// is never a visited host: this transport returns the first response without redirects.
pub(crate) fn service_contract_marker(
    target: &crate::targets::Target,
    sample: &crate::signals::Sample,
    result: &ResultRecord,
) -> Option<String> {
    let status = sample.status?;
    if let Some(e) = crate::signals::policy_evidence(&result.evidence) {
        return Some(format!("SERVICE_CONTRACT_{}", e.value.as_str()?));
    }
    let (url, source, marker) = match target.id() {
        "google.gemini" => (
            "https://support.google.com/gemini/",
            RegionSignalSource::InlineCountryCode,
            "window.wiz_global_data",
        ),
        "google.youtube" => (
            "https://www.youtube.com/",
            RegionSignalSource::YouTubeGl,
            "\"gl\"",
        ),
        "google.news" => (
            "https://news.google.com/",
            RegionSignalSource::NewsGlCeid,
            "",
        ),
        "google.play" => (
            "https://play.google.com/store",
            RegionSignalSource::PlayRegionStructure,
            "",
        ),
        _ => return Some("SERVICE_CONTRACT_UNSPECIFIED".into()),
    };
    if target.raw.url != url {
        return Some("SERVICE_CONTRACT_UNSPECIFIED".into());
    }
    let news = source == RegionSignalSource::NewsGlCeid;
    let contract = ServiceContract {
        target_id: target.id().into(),
        allowed_final_hosts: vec![target.host()],
        max_redirects: 0,
        expected_status: if news {
            vec![301, 302, 303, 307, 308]
        } else {
            vec![200]
        },
        allowed_content_types: if news {
            vec![]
        } else {
            vec!["text/html".into()]
        },
        required_markers: if marker.is_empty() {
            vec![]
        } else {
            vec![marker.into()]
        },
        challenge_markers: vec!["unusual traffic".into()],
        login_markers: vec!["servicelogin".into()],
        region_source: source,
        max_body_bytes: target.defaults.max_body_bytes,
        contract_version: 1,
        verified_at: target.defaults.verified_at.to_string(),
    };
    let region = match source {
        RegionSignalSource::InlineCountryCode => result.service_observed.observed_country.clone(),
        _ => {
            let mut regions = result
                .evidence
                .iter()
                .filter(|e| {
                    matches!(
                        (source, e.rule_id.as_str(), e.claim.as_str()),
                        (RegionSignalSource::YouTubeGl, "rule.yt-gl", "edition.gl")
                            | (RegionSignalSource::NewsGlCeid, "rule.edition", "edition.gl")
                            | (
                                RegionSignalSource::PlayRegionStructure,
                                "rule.flag",
                                "store_region_flag"
                            )
                    )
                })
                .filter_map(|e| e.value.as_str());
            regions.next().and_then(|first| {
                regions
                    .all(|other| other == first)
                    .then(|| first.to_owned())
            })
        }
    };
    let response = ContractResponse {
        final_host: target.host(),
        redirects: 0,
        status,
        content_type: sample.mime(),
        body: sample.text(),
        region,
        body_truncated: sample.truncated || !sample.complete,
    };
    Some(format!(
        "SERVICE_CONTRACT_{}",
        evaluate_contract(&contract, &response).as_str()
    ))
}

/// 连续多次取不到目标预期信号时标记 SIGNAL_STALE（不默默判成正常）。
pub fn contract_signal_stale(consecutive_misses: u32, miss_threshold: u32) -> bool {
    consecutive_misses >= miss_threshold.max(1)
}

/// 双栈证据分类（T-7-09e）：各态分列，主动复测不覆盖首轮。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DualStackState {
    Ipv4Available,
    Ipv6Available,
    Ipv6Slow,
    Ipv4Only,
    Ipv6Only,
    Ipv6FailedV4Ok,
    DnsMismatch,
    Unknown,
}

impl DualStackState {
    pub fn as_str(self) -> &'static str {
        match self {
            DualStackState::Ipv4Available => "IPV4_AVAILABLE",
            DualStackState::Ipv6Available => "IPV6_AVAILABLE",
            DualStackState::Ipv6Slow => "IPV6_SLOW",
            DualStackState::Ipv4Only => "IPV4_ONLY_EGRESS",
            DualStackState::Ipv6Only => "IPV6_ONLY_EGRESS",
            DualStackState::Ipv6FailedV4Ok => "IPV6_BROKEN",
            DualStackState::DnsMismatch => "DNS_CONNECT_MISMATCH",
            DualStackState::Unknown => "UNKNOWN",
        }
    }
}

/// 双栈观测：首轮与主动指定地址族的复测分别保留。
#[derive(Debug, Clone, Default)]
pub struct DualStackObservation {
    pub v4_attempted: bool,
    pub v4_success: bool,
    pub v4_first_byte_ms: Option<u64>,
    pub v6_attempted: bool,
    pub v6_success: bool,
    pub v6_first_byte_ms: Option<u64>,
    pub v6_budget_ms: u64,
    pub dns_matches_connect: Option<bool>,
    pub explicit_family_retest: bool,
}

pub fn classify_dual_stack(observation: &DualStackObservation) -> DualStackState {
    if observation.dns_matches_connect == Some(false) {
        return DualStackState::DnsMismatch;
    }
    if observation.v6_attempted && observation.v6_success && !observation.v4_success {
        if observation
            .v6_first_byte_ms
            .map(|ms| ms > observation.v6_budget_ms)
            .unwrap_or(false)
        {
            return DualStackState::Ipv6Slow;
        }
        if observation.v4_attempted {
            return DualStackState::Ipv6Only;
        }
        return DualStackState::Ipv6Available;
    }
    if observation.v6_attempted && !observation.v6_success && observation.v4_success {
        return DualStackState::Ipv6FailedV4Ok;
    }
    if observation.v6_attempted && observation.v6_success {
        if observation
            .v6_first_byte_ms
            .map(|ms| ms > observation.v6_budget_ms)
            .unwrap_or(false)
        {
            return DualStackState::Ipv6Slow;
        }
        return DualStackState::Ipv6Available;
    }
    if observation.v4_attempted && observation.v4_success && !observation.v6_attempted {
        return DualStackState::Ipv4Only;
    }
    if observation.v4_success {
        return DualStackState::Ipv4Available;
    }
    DualStackState::Unknown
}

/// 波次级 DNS 缓存（T-7-09e）：同 run 复用、记解析器/地址集/命中、不跨 run。
#[derive(Debug, Clone, Default)]
pub struct DnsWaveCache {
    pub wave_id: String,
    entries: BTreeMap<String, DnsCacheEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsCacheEntry {
    pub resolver: String,
    pub addresses: Vec<String>,
    pub hits: u64,
}

impl DnsWaveCache {
    pub fn new(wave_id: impl Into<String>) -> Self {
        DnsWaveCache {
            wave_id: wave_id.into(),
            entries: BTreeMap::new(),
        }
    }

    pub fn lookup(&mut self, host: &str) -> Option<&DnsCacheEntry> {
        let entry = self.entries.get_mut(host)?;
        entry.hits += 1;
        Some(entry)
    }

    pub fn insert(&mut self, host: &str, resolver: &str, addresses: Vec<String>) {
        self.entries.insert(
            host.to_string(),
            DnsCacheEntry {
                resolver: resolver.to_string(),
                addresses,
                hits: 0,
            },
        );
    }

    /// 控制集异常或地址集自相矛盾时重新解析。
    pub fn invalidate(&mut self, host: &str) {
        self.entries.remove(host);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Guard 导出前的取证暂停检查：路线变化 / 本地 QoS / 主要信号过期 → 暂停自动规则生成。
pub fn guard_pause_from_report(report: &Report) -> Option<String> {
    let mut reasons: Vec<String> = Vec::new();
    let mut markers: Vec<&str> = report
        .results
        .iter()
        .flat_map(|result| result.risk_signal.markers.iter().map(String::as_str))
        .collect();
    markers.sort_unstable();
    markers.dedup();
    for marker in markers {
        if matches!(
            marker,
            "LOCAL_QOS_SUSPECT"
                | "MULTI_AIRPORT_UDP_FAILURE"
                | "ROUTE_INCONSISTENT"
                | "INCONSISTENT_NODE"
                | "INCONSISTENT_REMOTE"
                | "INCONSISTENT_EGRESS"
        ) {
            reasons.push(marker.to_string());
        }
    }
    let stale = report.results.iter().any(|result| {
        result.state == State::SignalStale
            || result
                .evidence
                .iter()
                .any(|evidence| evidence.freshness.status == FreshnessStatus::Stale)
    });
    if stale {
        reasons.push("SIGNAL_STALE".to_string());
    }
    if reasons.is_empty() {
        None
    } else {
        reasons.sort();
        reasons.dedup();
        Some(reasons.join(", "))
    }
}

/// 把节点绑定写入既有 Path 字段，并在路线不一致时追加风险标记（不新增 schema 字段）。
pub fn attach_route_consistency(results: &mut [ResultRecord], bindings: &[BindingRecord]) {
    match route_consistency(bindings) {
        RouteConsistency::Inconsistent { .. } => {
            for result in results {
                let marker = "ROUTE_INCONSISTENT".to_string();
                if !result.risk_signal.markers.contains(&marker) {
                    result.risk_signal.markers.push(marker);
                }
                result.risk_signal.reason =
                    Some("连续请求之间节点 / 远端 / 出口发生变化：整组结果不合并".to_string());
            }
        }
        RouteConsistency::Consistent | RouteConsistency::Unverifiable => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(
        protocol: ProbeProtocol,
        airport: &str,
        success: bool,
        timed_out: bool,
    ) -> ObservationPoint {
        ObservationPoint {
            protocol,
            node_label: format!("{airport}-node"),
            airport: airport.to_string(),
            success,
            timed_out,
            first_byte_ms: if success { Some(120) } else { None },
        }
    }

    #[test]
    fn route_consistency_detects_switch_and_missing_evidence() {
        let base = BindingRecord {
            node_label: "JP-1".to_string(),
            node_id: "node-1".to_string(),
            remote: "jp1.example:443".to_string(),
            protocol: ProbeProtocol::TcpHttp,
            egress_ip: Some("203.0.113.10".to_string()),
            family: Some(ObservedFamily::V4),
            started_at: "t0".to_string(),
            ended_at: Some("t1".to_string()),
        };
        assert_eq!(
            route_consistency(&[base.clone(), base.clone()]),
            RouteConsistency::Consistent
        );
        let mut switched = base.clone();
        switched.node_id = "node-2".to_string();
        assert_eq!(
            route_consistency(&[base.clone(), switched]),
            RouteConsistency::Inconsistent { field: "node_id" }
        );
        let mut no_egress = base.clone();
        no_egress.egress_ip = None;
        assert_eq!(
            route_consistency(&[base, no_egress]),
            RouteConsistency::Unverifiable
        );
        assert_eq!(route_consistency(&[]), RouteConsistency::Unverifiable);
    }

    #[test]
    fn tcp_ok_udp_timeouts_is_local_qos_and_pauses_only_udp() {
        let points = vec![
            point(ProbeProtocol::TcpHttp, "a", true, false),
            point(ProbeProtocol::UdpQuic, "a", false, true),
            point(ProbeProtocol::UdpQuic, "b", false, true),
        ];
        let finding = detect_local_qos(&points);
        assert!(finding.suspect_local_qos);
        assert!(finding.markers.contains(&"LOCAL_QOS_SUSPECT".to_string()));
        assert!(finding
            .markers
            .contains(&"MULTI_AIRPORT_UDP_FAILURE".to_string()));
        assert_eq!(finding.paused_protocols, vec![ProbeProtocol::UdpQuic]);
        // 未探测的节点不会出现在结果里，更不会被标成故障。
        assert!(finding.detail.contains("未测节点不标故障"));
    }

    #[test]
    fn single_udp_timeout_without_tcp_is_not_qos() {
        let points = vec![point(ProbeProtocol::UdpQuic, "a", false, true)];
        let finding = detect_local_qos(&points);
        assert!(!finding.suspect_local_qos);
        assert!(finding.paused_protocols.is_empty());
    }

    fn contract() -> ServiceContract {
        ServiceContract {
            target_id: "google.gemini".to_string(),
            allowed_final_hosts: vec!["gemini.google.com".to_string()],
            max_redirects: 1,
            expected_status: vec![200],
            allowed_content_types: vec!["text/html".to_string()],
            required_markers: vec!["window.wiz_global_data".to_string()],
            challenge_markers: vec!["unusual traffic".to_string()],
            login_markers: vec!["sign in".to_string()],
            region_source: RegionSignalSource::InlineCountryCode,
            max_body_bytes: 4_000_000,
            contract_version: 1,
            verified_at: "2026-11-23".to_string(),
        }
    }

    fn response(status: u16, body: &str) -> ContractResponse {
        ContractResponse {
            final_host: "gemini.google.com".to_string(),
            redirects: 0,
            status,
            content_type: Some("text/html; charset=utf-8".to_string()),
            body: body.to_string(),
            region: Some("JPN".to_string()),
            body_truncated: false,
        }
    }

    #[test]
    fn contract_separates_status_from_region_restriction() {
        let contract = contract();
        assert_eq!(
            evaluate_contract(&contract, &response(429, "window.wiz_global_data")),
            ContractOutcome::RateLimited { status: 429 }
        );
        assert_eq!(
            evaluate_contract(&contract, &response(503, "window.wiz_global_data")),
            ContractOutcome::RateLimited { status: 503 }
        );
        assert_eq!(
            evaluate_contract(&contract, &response(200, "unusual traffic challenge")),
            ContractOutcome::Challenge
        );
        // 200 但结构不符：不能算 OK。
        assert_eq!(
            evaluate_contract(&contract, &response(200, "<html>different</html>")),
            ContractOutcome::StructureMismatch {
                missing: "window.wiz_global_data".to_string()
            }
        );
        assert_eq!(
            evaluate_contract(&contract, &response(200, "window.wiz_global_data")),
            ContractOutcome::Matched {
                region: "JPN".to_string()
            }
        );
        let mut no_region = response(200, "window.wiz_global_data");
        no_region.region = None;
        assert_eq!(
            evaluate_contract(&contract, &no_region),
            ContractOutcome::NoRegionSignal
        );
        assert!(is_forbidden_criterion("PAGE_LANGUAGE"));
        assert!(is_forbidden_criterion("node_name_country"));
        assert!(!is_forbidden_criterion("INLINE_COUNTRY_CODE"));
        assert!(contract_signal_stale(3, 3));
    }

    #[test]
    fn dual_stack_states_and_dns_mismatch() {
        let mut observation = DualStackObservation {
            v6_attempted: true,
            v6_success: true,
            v6_first_byte_ms: Some(9000),
            v6_budget_ms: 8000,
            ..Default::default()
        };
        assert_eq!(classify_dual_stack(&observation), DualStackState::Ipv6Slow);
        observation.v6_first_byte_ms = Some(120);
        assert_eq!(
            classify_dual_stack(&observation),
            DualStackState::Ipv6Available
        );
        let broken = DualStackObservation {
            v6_attempted: true,
            v4_attempted: true,
            v4_success: true,
            ..Default::default()
        };
        assert_eq!(classify_dual_stack(&broken), DualStackState::Ipv6FailedV4Ok);
        let only_v6 = DualStackObservation {
            v4_attempted: true,
            v6_attempted: true,
            v6_success: true,
            ..Default::default()
        };
        assert_eq!(classify_dual_stack(&only_v6), DualStackState::Ipv6Only);
        let mismatch = DualStackObservation {
            dns_matches_connect: Some(false),
            ..Default::default()
        };
        assert_eq!(classify_dual_stack(&mismatch), DualStackState::DnsMismatch);
    }

    #[test]
    fn dns_wave_cache_reuses_within_wave_and_can_invalidate() {
        let mut wave = DnsWaveCache::new("run-1#wave-0");
        wave.insert(
            "gemini.google.com",
            "system",
            vec!["203.0.113.1".to_string()],
        );
        let first = wave.lookup("gemini.google.com").expect("hit");
        assert_eq!(first.hits, 1);
        assert_eq!(first.resolver, "system");
        wave.lookup("gemini.google.com");
        assert_eq!(wave.lookup("gemini.google.com").expect("hit").hits, 3);
        wave.invalidate("gemini.google.com");
        assert!(wave.lookup("gemini.google.com").is_none());
        // 新 run 新建缓存，不跨 run 复用。
        let mut next = DnsWaveCache::new("run-2#wave-0");
        assert!(next.lookup("gemini.google.com").is_none());
        assert!(next.is_empty());
        next.insert("a.example", "system", Vec::new());
        next.clear();
        assert!(next.is_empty());
    }
}
