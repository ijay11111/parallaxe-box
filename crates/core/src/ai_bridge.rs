//! AI 启动检测的本地授权桥（文件级、同用户、**不监听任何端口**）。
//!
//! 设计边界（T-10-00 裁决）：
//! - 路径 A（默认）：agent 只能提交**一次性检测请求**；用户在 GUI 的 F3 确认框里点确认后，
//!   由 GUI（持有一切导入与会话许可的进程）真实发包，并把**脱敏报告**回传给 agent；
//! - 路径 B（opt-in）：用户在 GUI 设置里打开「AI 自主调试 → 自主检测」后才生效，
//!   开关状态只存在于 GUI 会话内存，由本桥的 `status` 文件只读投影给 agent；
//! - agent 可写 `control` 文件请求关闭自主检测（权限即时收回），但**永远不能自己打开**；
//! - 所有文件位于当前用户临时目录下的固定子目录，单次有效、带 TTL、原子写入；
//! - 桥只传 ID / 判定 / 脱敏报告，不传任意 URL、Cookie、账号或磁盘路径。
//!
//! 本模块不做任何网络 I/O；真实发包发生在 GUI/测试宿主进程内。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

use crate::contracts::{Report, ORIGIN_AGENT_AUTONOMOUS};

/// 桥目录环境变量（测试与自动化可显式指定；产品默认使用系统临时目录）。
pub const BRIDGE_DIR_ENV: &str = "PARALLAXE_AI_BRIDGE_DIR";
/// 默认目录名（位于 `std::env::temp_dir()` 下）。
pub const BRIDGE_DIR_NAME: &str = "parallaxe-ai-bridge";

pub const REQUEST_SCHEMA: &str = "parallaxe.ai.request/v1";
pub const DECISION_SCHEMA: &str = "parallaxe.ai.decision/v1";
pub const RESULT_SCHEMA: &str = "parallaxe.ai.result/v1";
pub const STATUS_SCHEMA: &str = "parallaxe.ai.status/v1";
pub const CONTROL_SCHEMA: &str = "parallaxe.ai.control/v1";

/// AI 检测请求的强制最小间隔（3.5 秒，全员 QoS 红线）。
pub const MIN_INTERVAL_MS: u64 = 3_500;
/// AI 检测请求的单批上限。
pub const MAX_BATCH_SIZE: u32 = 10;
/// 单次请求最多目标数。
pub const MAX_TARGETS: usize = 20;
/// 请求等待用户确认的有效期。
pub const REQUEST_TTL_SECONDS: i64 = 120;
/// 授权决定的有效期（覆盖一次真实运行）。
pub const DECISION_TTL_SECONDS: i64 = 180;
/// 结果保留时长（便于 agent 取回导出）。
pub const RESULT_TTL_SECONDS: i64 = 3_600;

/// 检测范围：Profile 与显式目标 ID 二选一；**只接受目录 ID，不接受 URL**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiScope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub purpose: String,
}

impl AiScope {
    pub fn for_profile(profile: impl Into<String>, purpose: impl Into<String>) -> Self {
        Self {
            profile: Some(profile.into()),
            targets: Vec::new(),
            purpose: purpose.into(),
        }
    }

    pub fn for_targets<I, S>(targets: I, purpose: impl Into<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            profile: None,
            targets: targets.into_iter().map(Into::into).collect(),
            purpose: purpose.into(),
        }
    }

    /// 人类可读范围（不包含原始 URL）。
    pub fn label(&self) -> String {
        match &self.profile {
            Some(profile) => format!("profile={profile}"),
            None => format!("targets={}", self.targets.join(",")),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.profile.is_some() && !self.targets.is_empty() {
            return Err("profile 与 targets 只能二选一".into());
        }
        if self.profile.is_none() && self.targets.is_empty() {
            return Err("必须指定 profile 或 targets".into());
        }
        if let Some(profile) = &self.profile {
            if !valid_token(profile, 32) {
                return Err("profile 名称非法".into());
            }
        }
        if self.targets.len() > MAX_TARGETS {
            return Err(format!("单次最多 {MAX_TARGETS} 个目标"));
        }
        for target in &self.targets {
            if !valid_target_id(target) {
                return Err(format!("目标 ID 非法：{target}"));
            }
        }
        if !valid_purpose(&self.purpose) {
            return Err("purpose 含非法字符或过长（≤160，且不得包含 URL）".into());
        }
        Ok(())
    }
}

/// agent 提交的一次性检测请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub schema: String,
    pub request_id: String,
    /// 由 agent 生成、用于把授权决定绑定回本请求的随机串（非机密）。
    pub nonce: String,
    pub requester: String,
    pub created_at: String,
    pub expires_at: String,
    pub scope: AiScope,
    pub min_interval_ms: u64,
    pub batch_size: u32,
    pub preferred_transport: String,
}

impl BridgeRequest {
    pub fn new(scope: AiScope, min_interval_ms: u64, batch_size: u32) -> Result<Self, String> {
        scope.validate()?;
        let now = OffsetDateTime::now_utc();
        let nonce = new_nonce(now);
        let (min_interval_ms, batch_size) = qos_floor(min_interval_ms, batch_size);
        Ok(Self {
            schema: REQUEST_SCHEMA.to_string(),
            request_id: format!("ai-req-{}-{}", now.unix_timestamp_nanos(), &nonce[..8]),
            nonce,
            requester: "external-ai-agent".to_string(),
            created_at: format_time(now),
            expires_at: format_time(now + Duration::seconds(REQUEST_TTL_SECONDS)),
            scope,
            min_interval_ms,
            batch_size,
            preferred_transport: "TCP_FIRST".to_string(),
        })
    }

    pub fn scope_digest(&self) -> String {
        scope_digest(&self.scope, self.min_interval_ms, self.batch_size)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != REQUEST_SCHEMA {
            return Err("请求 schema 不匹配".into());
        }
        if !valid_request_id(&self.request_id) {
            return Err("request_id 非法".into());
        }
        if !valid_nonce(&self.nonce) {
            return Err("nonce 非法".into());
        }
        if !valid_token(&self.requester, 64) {
            return Err("requester 非法".into());
        }
        self.scope.validate()?;
        if self.min_interval_ms < MIN_INTERVAL_MS {
            return Err("min_interval_ms 低于 3500ms 红线".into());
        }
        if self.batch_size == 0 || self.batch_size > MAX_BATCH_SIZE {
            return Err("batch_size 越界".into());
        }
        if self.preferred_transport != "TCP_FIRST" {
            return Err("preferred_transport 只支持 TCP_FIRST".into());
        }
        parse_time(&self.created_at).ok_or("created_at 非法")?;
        parse_time(&self.expires_at).ok_or("expires_at 非法")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionKind {
    Approved,
    Denied,
}

/// 用户（GUI/宿主）对请求作出的决定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeDecision {
    pub schema: String,
    pub request_id: String,
    pub nonce: String,
    pub scope_digest: String,
    pub decision: DecisionKind,
    /// 恒为 `USER`：决定只能由用户侧进程写入。
    pub decided_by: String,
    pub user_confirmed: bool,
    pub decided_at: String,
    pub expires_at: String,
    pub autonomous: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl BridgeDecision {
    pub fn approve(request: &BridgeRequest, autonomous: bool) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            schema: DECISION_SCHEMA.to_string(),
            request_id: request.request_id.clone(),
            nonce: request.nonce.clone(),
            scope_digest: request.scope_digest(),
            decision: DecisionKind::Approved,
            decided_by: "USER".to_string(),
            user_confirmed: true,
            decided_at: format_time(now),
            expires_at: format_time(now + Duration::seconds(DECISION_TTL_SECONDS)),
            autonomous,
            origin: Some(if autonomous {
                ORIGIN_AGENT_AUTONOMOUS.to_string()
            } else {
                crate::contracts::ORIGIN_USER_IMPORTED.to_string()
            }),
        }
    }

    pub fn deny(request: &BridgeRequest) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            schema: DECISION_SCHEMA.to_string(),
            request_id: request.request_id.clone(),
            nonce: request.nonce.clone(),
            scope_digest: request.scope_digest(),
            decision: DecisionKind::Denied,
            decided_by: "USER".to_string(),
            user_confirmed: true,
            decided_at: format_time(now),
            expires_at: format_time(now + Duration::seconds(DECISION_TTL_SECONDS)),
            autonomous: false,
            origin: None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != DECISION_SCHEMA {
            return Err("决定 schema 不匹配".into());
        }
        if !valid_request_id(&self.request_id) {
            return Err("request_id 非法".into());
        }
        if !valid_nonce(&self.nonce) {
            return Err("nonce 非法".into());
        }
        if self.decided_by != "USER" || !self.user_confirmed {
            return Err("决定必须由用户确认".into());
        }
        parse_time(&self.decided_at).ok_or("decided_at 非法")?;
        parse_time(&self.expires_at).ok_or("expires_at 非法")?;
        Ok(())
    }
}

/// GUI 会话状态投影（自主检测开关**只存在于 GUI 内存**，本文件只是只读投影）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeStatus {
    pub schema: String,
    pub host: String,
    pub updated_at: String,
    pub autonomous_enabled: bool,
    /// 恒为 `true`：开关不落盘，关闭软件即恢复关闭。
    pub session_only: bool,
    /// 宿主当前可真实发包的用户导入目标 ID（不含 URL）。
    #[serde(default)]
    pub imported_targets: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl BridgeStatus {
    pub fn new(
        host: impl Into<String>,
        autonomous_enabled: bool,
        imported_targets: Vec<String>,
    ) -> Self {
        Self {
            schema: STATUS_SCHEMA.to_string(),
            host: host.into(),
            updated_at: format_time(OffsetDateTime::now_utc()),
            autonomous_enabled,
            session_only: true,
            imported_targets,
            capabilities: vec![
                "user_confirmed_one_shot".to_string(),
                "autonomous_detection".to_string(),
                "sanitized_export".to_string(),
            ],
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != STATUS_SCHEMA {
            return Err("状态 schema 不匹配".into());
        }
        if !valid_token(&self.host, 32) {
            return Err("host 非法".into());
        }
        if !self.session_only {
            return Err("自主检测开关必须是会话级（session_only=true）".into());
        }
        for target in &self.imported_targets {
            if !valid_target_id(target) {
                return Err("导入目标 ID 非法".into());
            }
        }
        Ok(())
    }
}

/// agent 请求关闭自主检测（唯一允许 agent 写入的控制指令）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeControl {
    pub schema: String,
    pub action: ControlAction,
    pub requested_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlAction {
    DisableAutonomous,
}

impl BridgeControl {
    pub fn disable_autonomous(reason: Option<String>) -> Self {
        Self {
            schema: CONTROL_SCHEMA.to_string(),
            action: ControlAction::DisableAutonomous,
            requested_at: format_time(OffsetDateTime::now_utc()),
            reason: reason.filter(|text| valid_purpose(text)),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_SCHEMA {
            return Err("控制指令 schema 不匹配".into());
        }
        parse_time(&self.requested_at).ok_or("requested_at 非法")?;
        if let Some(reason) = &self.reason {
            if !valid_purpose(reason) {
                return Err("reason 非法".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResultStatus {
    Completed,
    Denied,
    Failed,
}

/// 宿主回传的最终结果（报告一律脱敏，不写任意磁盘路径）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeResult {
    pub schema: String,
    pub request_id: String,
    pub status: ResultStatus,
    pub completed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub autonomous: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub network_request_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<Report>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denied_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl BridgeResult {
    pub fn completed(request: &BridgeRequest, report: Report, autonomous: bool) -> Self {
        let report = sanitize_report(report);
        Self {
            schema: RESULT_SCHEMA.to_string(),
            request_id: request.request_id.clone(),
            status: ResultStatus::Completed,
            completed_at: format_time(OffsetDateTime::now_utc()),
            run_id: Some(report.run_id.clone()),
            autonomous,
            origin: report.origin.clone(),
            network_request_count: report.network_request_count,
            report: Some(report),
            denied_reason: None,
            error: None,
        }
    }

    pub fn denied(request: &BridgeRequest, reason: impl Into<String>) -> Self {
        Self {
            schema: RESULT_SCHEMA.to_string(),
            request_id: request.request_id.clone(),
            status: ResultStatus::Denied,
            completed_at: format_time(OffsetDateTime::now_utc()),
            run_id: None,
            autonomous: false,
            origin: None,
            network_request_count: 0,
            report: None,
            denied_reason: Some(reason.into()),
            error: None,
        }
    }

    pub fn failed(request: &BridgeRequest, error: impl Into<String>) -> Self {
        Self {
            schema: RESULT_SCHEMA.to_string(),
            request_id: request.request_id.clone(),
            status: ResultStatus::Failed,
            completed_at: format_time(OffsetDateTime::now_utc()),
            run_id: None,
            autonomous: false,
            origin: None,
            network_request_count: 0,
            report: None,
            denied_reason: None,
            error: Some(error.into()),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != RESULT_SCHEMA {
            return Err("结果 schema 不匹配".into());
        }
        if !valid_request_id(&self.request_id) {
            return Err("request_id 非法".into());
        }
        parse_time(&self.completed_at).ok_or("completed_at 非法")?;
        Ok(())
    }
}

/// 桥目录布局与读写。所有写入均为「临时文件 + 原子改名」。
#[derive(Debug, Clone)]
pub struct BridgePaths {
    root: PathBuf,
}

impl Default for BridgePaths {
    fn default() -> Self {
        Self::resolve()
    }
}

impl BridgePaths {
    /// 优先使用 `PARALLAXE_AI_BRIDGE_DIR`，否则使用系统临时目录下的固定子目录。
    pub fn resolve() -> Self {
        match std::env::var_os(BRIDGE_DIR_ENV) {
            Some(dir) if !dir.is_empty() => Self {
                root: PathBuf::from(dir),
            },
            _ => Self {
                root: std::env::temp_dir().join(BRIDGE_DIR_NAME),
            },
        }
    }

    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn requests_dir(&self) -> PathBuf {
        self.root.join("requests")
    }

    fn decisions_dir(&self) -> PathBuf {
        self.root.join("decisions")
    }

    fn results_dir(&self) -> PathBuf {
        self.root.join("results")
    }

    fn controls_dir(&self) -> PathBuf {
        self.root.join("controls")
    }

    pub fn ensure(&self) -> io::Result<()> {
        for dir in [
            self.root.clone(),
            self.requests_dir(),
            self.decisions_dir(),
            self.results_dir(),
            self.controls_dir(),
        ] {
            fs::create_dir_all(&dir)?;
            restrict_permissions(&dir)?;
        }
        Ok(())
    }

    pub fn write_request(&self, request: &BridgeRequest) -> io::Result<()> {
        request.validate().map_err(invalid_input)?;
        self.ensure()?;
        write_json(
            &self
                .requests_dir()
                .join(format!("{}.json", request.request_id)),
            request,
        )
    }

    /// 读取仍未过期、格式合法的请求（按创建时间升序）。
    pub fn read_requests(&self) -> Vec<BridgeRequest> {
        let now = OffsetDateTime::now_utc();
        let mut requests: Vec<BridgeRequest> = read_dir_json(&self.requests_dir())
            .into_iter()
            .filter(|request: &BridgeRequest| request.validate().is_ok())
            .filter(|request| parse_time(&request.expires_at).is_some_and(|end| now < end))
            .collect();
        requests.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        requests
    }

    pub fn write_decision(&self, decision: &BridgeDecision) -> io::Result<()> {
        decision.validate().map_err(invalid_input)?;
        self.ensure()?;
        write_json(
            &self
                .decisions_dir()
                .join(format!("{}.json", decision.request_id)),
            decision,
        )
    }

    pub fn read_decision(&self, request_id: &str) -> Option<BridgeDecision> {
        if !valid_request_id(request_id) {
            return None;
        }
        let path = self.decisions_dir().join(format!("{request_id}.json"));
        let decision: BridgeDecision = read_json(&path)?;
        decision.validate().ok()?;
        Some(decision)
    }

    pub fn write_result(&self, result: &BridgeResult) -> io::Result<()> {
        result.validate().map_err(invalid_input)?;
        self.ensure()?;
        write_json(
            &self
                .results_dir()
                .join(format!("{}.json", result.request_id)),
            result,
        )
    }

    pub fn read_result(&self, request_id: &str) -> Option<BridgeResult> {
        if !valid_request_id(request_id) {
            return None;
        }
        let result: BridgeResult =
            read_json(&self.results_dir().join(format!("{request_id}.json")))?;
        result.validate().ok()?;
        Some(result)
    }

    pub fn write_status(&self, status: &BridgeStatus) -> io::Result<()> {
        status.validate().map_err(invalid_input)?;
        self.ensure()?;
        write_json(&self.root.join("status.json"), status)
    }

    pub fn read_status(&self) -> Option<BridgeStatus> {
        let status: BridgeStatus = read_json(&self.root.join("status.json"))?;
        status.validate().ok()?;
        Some(status)
    }

    pub fn write_control(&self, control: &BridgeControl) -> io::Result<()> {
        control.validate().map_err(invalid_input)?;
        self.ensure()?;
        let name = format!(
            "{}-{}.json",
            OffsetDateTime::now_utc().unix_timestamp_nanos(),
            &new_nonce(OffsetDateTime::now_utc())[..8]
        );
        write_json(&self.controls_dir().join(name), control)
    }

    /// 取出并删除所有控制指令（宿主消费后立即失效）。
    pub fn take_controls(&self) -> Vec<BridgeControl> {
        let dir = self.controls_dir();
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut controls = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Some(control) = read_json::<BridgeControl>(&path) {
                if control.validate().is_ok() {
                    controls.push(control);
                }
            }
            let _ = fs::remove_file(&path);
        }
        controls
    }

    /// 清理超过 TTL 的请求、决定与结果（宿主每次轮询可调用）。
    pub fn cleanup_expired(&self) {
        let now = OffsetDateTime::now_utc();
        cleanup_dir(&self.requests_dir(), now, |request: &BridgeRequest| {
            parse_time(&request.expires_at)
        });
        cleanup_dir(&self.decisions_dir(), now, |decision: &BridgeDecision| {
            parse_time(&decision.expires_at)
        });
        cleanup_dir(&self.results_dir(), now, |result: &BridgeResult| {
            parse_time(&result.completed_at).map(|at| at + Duration::seconds(RESULT_TTL_SECONDS))
        });
        // 已决定/已完成的请求文件在 10 分钟后删除，避免残留。
        cleanup_dir(&self.requests_dir(), now, |request: &BridgeRequest| {
            parse_time(&request.expires_at).map(|at| at + Duration::seconds(600))
        });
    }
}

/// 把 QoS 强制到红线以内：间隔 ≥3500ms、单批 ≤10。
pub fn qos_floor(min_interval_ms: u64, batch_size: u32) -> (u64, u32) {
    (
        min_interval_ms.max(MIN_INTERVAL_MS),
        batch_size.clamp(1, MAX_BATCH_SIZE),
    )
}

/// 范围的稳定摘要（绑定授权决定与请求，防止替换范围）。
pub fn scope_digest(scope: &AiScope, min_interval_ms: u64, batch_size: u32) -> String {
    let mut targets = scope.targets.clone();
    targets.sort();
    let canonical = format!(
        "{}|{}|{}|{}",
        scope.profile.as_deref().unwrap_or("-"),
        targets.join(","),
        min_interval_ms,
        batch_size
    );
    hex_sha256(canonical.as_bytes())
}

/// 报告脱敏：强制关闭原始 IP 展示，其余字段原样保留。
pub fn sanitize_report(mut report: Report) -> Report {
    for row in &mut report.results {
        row.destination.set_show_ip(false);
    }
    report
}

/// 生成 32 位十六进制 nonce（不用于机密性，只用于把决定绑回请求）。
pub fn new_nonce(now: OffsetDateTime) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let seed = format!(
        "{}-{}-{}-{:?}",
        now.unix_timestamp_nanos(),
        std::process::id(),
        counter,
        std::thread::current().id()
    );
    hex_sha256(seed.as_bytes())
}

pub fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn format_time(now: OffsetDateTime) -> String {
    now.format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn parse_time(text: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(text, &Rfc3339).ok()
}

fn valid_token(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid_target_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 80
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

fn valid_request_id(text: &str) -> bool {
    text.starts_with("ai-req-") && valid_token(text, 64)
}

fn valid_nonce(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit())
}

fn valid_purpose(text: &str) -> bool {
    text.chars().count() <= 160
        && !text.chars().any(|c| c.is_control())
        && !text.contains("://")
        && !text.contains('@')
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_atomic(path, &bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let suffix = format!(
        "tmp-{}-{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    );
    let tmp = path.with_extension(suffix);
    fs::write(&tmp, bytes)?;
    restrict_permissions(&tmp)?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn read_dir_json<T: for<'de> Deserialize<'de>>(dir: &Path) -> Vec<T> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter_map(|entry| read_json::<T>(&entry.path()))
        .collect()
}

fn cleanup_dir<T: for<'de> Deserialize<'de>>(
    dir: &Path,
    now: OffsetDateTime,
    deadline: impl Fn(&T) -> Option<OffsetDateTime>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let expired = read_json::<T>(&path)
            .and_then(|value| deadline(&value))
            .is_some_and(|end| now >= end);
        if expired {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> io::Result<()> {
    // Windows 上桥目录位于当前用户临时目录内；不额外改写 ACL（见 docs/agent-interface.md 限制说明）。
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::Mode;

    fn temp_root(name: &str) -> BridgePaths {
        let root = std::env::temp_dir().join(format!(
            "parallaxe-ai-bridge-test-{name}-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        BridgePaths::with_root(root)
    }

    #[test]
    fn qos_floor_enforces_red_lines() {
        assert_eq!(qos_floor(0, 0), (MIN_INTERVAL_MS, 1));
        assert_eq!(qos_floor(1_000, 99), (MIN_INTERVAL_MS, MAX_BATCH_SIZE));
        assert_eq!(qos_floor(9_000, 3), (9_000, 3));
    }

    #[test]
    fn request_roundtrip_is_bound_to_scope_and_ttl() {
        let paths = temp_root("roundtrip");
        let request = BridgeRequest::new(
            AiScope::for_targets(["example.com"], "用户导入网址首跳观测"),
            3_500,
            5,
        )
        .unwrap();
        paths.write_request(&request).unwrap();

        let read = paths.read_requests();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].scope.targets, vec!["example.com".to_string()]);
        assert_eq!(read[0].min_interval_ms, MIN_INTERVAL_MS);

        let decision = BridgeDecision::approve(&read[0], false);
        paths.write_decision(&decision).unwrap();
        let stored = paths.read_decision(&read[0].request_id).unwrap();
        assert_eq!(stored.scope_digest, read[0].scope_digest());
        assert!(stored.user_confirmed);
        assert_eq!(
            stored.origin.as_deref(),
            Some(crate::contracts::ORIGIN_USER_IMPORTED)
        );

        let mut report = Report::new(
            crate::VERSION,
            "2026-11-25T00:00:00Z",
            "run-bridge-test",
            Mode::Online,
        );
        report.network_request_count = 1;
        let result = BridgeResult::completed(&read[0], report, false);
        paths.write_result(&result).unwrap();
        let stored = paths.read_result(&read[0].request_id).unwrap();
        assert_eq!(stored.status, ResultStatus::Completed);
        assert_eq!(stored.network_request_count, 1);
        assert_eq!(stored.run_id.as_deref(), Some("run-bridge-test"));
        paths.cleanup_expired();
        assert!(paths.read_result(&read[0].request_id).is_some());
    }

    #[test]
    fn urls_and_control_characters_never_enter_the_scope() {
        for scope in [
            AiScope::for_targets(["https://example.com"], "x"),
            AiScope::for_targets(["example.com\nX"], "x"),
            AiScope::for_targets(["Example.com"], "x"),
        ] {
            assert!(BridgeRequest::new(scope, 3_500, 5).is_err());
        }
        assert!(
            BridgeRequest::new(AiScope::for_profile("simple", "参见 https://x"), 3_500, 5).is_err()
        );
        assert!(BridgeRequest::new(
            AiScope::for_profile("simple", "用户确认后观测首跳"),
            3_500,
            5
        )
        .is_ok());
    }

    #[test]
    fn status_is_session_only_and_carries_ids_not_urls() {
        let paths = temp_root("status");
        let status = BridgeStatus::new("GUI", true, vec!["example.com".to_string()]);
        paths.write_status(&status).unwrap();
        let read = paths.read_status().unwrap();
        assert!(read.autonomous_enabled);
        assert!(read.session_only);
        assert_eq!(read.imported_targets, vec!["example.com".to_string()]);

        let mut tampered = read.clone();
        tampered.session_only = false;
        assert!(paths.write_status(&tampered).is_err());
        assert!(read.updated_at.contains('T'));
    }

    #[test]
    fn self_disable_control_is_consumed_once() {
        let paths = temp_root("control");
        paths
            .write_control(&BridgeControl::disable_autonomous(Some(
                "批量测试结束，主动收回权限".to_string(),
            )))
            .unwrap();
        let first = paths.take_controls();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].action, ControlAction::DisableAutonomous);
        assert!(paths.take_controls().is_empty(), "控制指令必须一次性消费");
    }

    #[test]
    fn agent_cannot_write_a_decision_for_a_different_scope() {
        let paths = temp_root("digest");
        let request =
            BridgeRequest::new(AiScope::for_targets(["a.example"], "x"), 3_500, 5).unwrap();
        let mut decision = BridgeDecision::approve(&request, false);
        decision.scope_digest = scope_digest(&AiScope::for_targets(["b.example"], "x"), 3_500, 5);
        assert_ne!(decision.scope_digest, request.scope_digest());
        // 决定在写入层仍是合法结构；绑定校验由 agent 侧完成（见 agent lib 测试）。
        paths.write_decision(&decision).unwrap();
        assert!(paths.read_decision(&request.request_id).is_some());
    }
}
