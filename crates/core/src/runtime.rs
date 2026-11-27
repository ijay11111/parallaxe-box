//! 运行协调器（core 门面）：选集 → 准入 → 探测 → 判定 → 报告/事件。
//!
//! 默认空许可：选中但未获准时发包 0、退出 3；离线 fixture 只替代输入。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use time::OffsetDateTime;

use crate::contracts::{
    reason, Attempt, AttemptFamily, BindStatus, Confidence, Coverage, Covered, Expectation,
    LocalPolicy, Mode, Path, Report, RequestedFamily, ResultRecord, RiskSignal, ServiceObserved,
    State, Termination, TransportLayerResult, Uncovered,
};
use crate::events::EventLog;
use crate::fixtures;
use crate::network::{self, PathDiagnostic};
use crate::probe::{self, FetchOutcome, FetchRequest, Transport};
use crate::reports;
use crate::signals::{self, Sample};
#[cfg(not(target_arch = "wasm32"))]
use crate::state::DirStore;
use crate::state::{FreshnessStore, NullStore};
use crate::targets::{ConfigError, ExpectationSpec, Registry, Target};
use crate::verdict::{self, VerdictInput};

/// `check` 的参数（已由 CLI 解析；这里不接受任意 URL/头）。
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    pub profile: Option<String>,
    pub groups: Vec<String>,
    pub targets: Vec<String>,
    pub full: bool,
    pub expect: Option<String>,
    pub fixture: Option<String>,
    pub state_dir: Option<PathBuf>,
    pub proxy: Option<String>,
    /// 代理模式（默认自动；未配置任何代理时也走自动探测）。
    pub proxy_mode: crate::proxy::ProxyMode,
    /// PAC 网址（仅 `proxy_mode = Pac` 使用）。
    pub pac_url: Option<String>,
    /// 前端在启动运行前写入的实际路径结论（`DIRECT_SELECTED`/`PROXY_SELECTED`/`HTTP`/`PAC`）。
    pub proxy_path: Option<String>,
    /// 前端写入的路径探测摘要，进入 `path.reason`。
    pub proxy_note: Option<String>,
    pub interface: Option<String>,
    pub ipv4: bool,
    pub fast: bool,
    pub timeout_ms: Option<u64>,
    pub max_concurrency: Option<u32>,
    /// 同 host / 节点 / 协议发送间隔（0–10000ms）。
    pub min_interval_ms: Option<u64>,
    /// 跨目标（不同 host）检验之间的间隔（0–10000ms）。
    pub inter_target_interval_ms: Option<u64>,
    /// 小批次大小（1–15）。
    pub batch_size: Option<u32>,
    /// 批次冷却（0–60000ms）。
    pub batch_cooldown_ms: Option<u64>,
    /// 单节点复测次数（0–1）。
    pub node_retest: Option<u32>,
    /// 只生成运行前风险预览（零发包）。
    pub plan: bool,
    /// 受控全量：新建连接超过安全预算（30）时必须显式二次确认。
    pub confirm_budget: bool,
    /// 节点筛选模式：每节点只做低噪声最小探针（需要 --proxy）。
    pub node_sweep: bool,
    /// Optional live pause/resume handle scoped to this run.
    pub qos_control: Option<probe::QosControl>,
    pub run_id: Option<String>,
}

/// 一次运行的结果。
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub report: Report,
    pub events: Vec<crate::contracts::Event>,
}

fn next_run_id(now: OffsetDateTime) -> String {
    let nanos = now.unix_timestamp_nanos();
    format!("run-{nanos}")
}

fn path_for(options: &CheckOptions) -> Result<(Path, Option<PathDiagnostic>), ConfigError> {
    let mut path = Path::system(if options.proxy_path.is_some() {
        "path-proxy-plan"
    } else if options.proxy.is_some() {
        "path-proxy"
    } else if options.interface.is_some() {
        "path-interface"
    } else {
        "path-system"
    });
    path.requested_family = if options.ipv4 {
        RequestedFamily::V4
    } else {
        RequestedFamily::Auto
    };
    // 前端已在产生网络 I/O 前完成模式选择/端口探测：如实记录实际选择与摘要。
    if let Some(label) = &options.proxy_path {
        path.proxy_mode = label.clone();
        path.reason = options.proxy_note.clone();
        if label.contains("PROXY") || label == "HTTP" {
            path.proxy_ref = Some("detected-proxy".to_string());
        }
    }
    // 枚举到接口不证明本次请求经过它；未验证字段保持 null。
    let mut diagnostic = None;
    if let Some(proxy) = &options.proxy {
        if proxy.contains('@') {
            return Err(ConfigError::field("--proxy", "拒绝带凭据的代理 URI"));
        }
        if options.proxy_path.is_none() {
            path.proxy_mode = "PROXY".to_string();
            path.proxy_ref = Some("explicit-proxy".to_string());
            path.reason = Some(reason::PROXY_REMOTE_UNVERIFIED.to_string());
        } else if options.proxy_path.as_deref() == Some("HTTP") {
            path.proxy_ref = Some("explicit-proxy".to_string());
        }
        path.bind_status = BindStatus::Unknown;
        diagnostic = Some(PathDiagnostic::ProxyRemoteUnverified);
    }
    if let Some(interface) = &options.interface {
        if !network::interface_exists(interface) {
            return Err(ConfigError::field("--interface", "接口不存在"));
        }
        path.interface_ref = Some("explicit-interface".to_string());
        path.proxy_mode = if options.proxy.is_some() {
            "PROXY_AND_INTERFACE".to_string()
        } else {
            "INTERFACE".to_string()
        };
        // G5 未验证绑定/远端族：拒绝静默直连。
        path.bind_status = BindStatus::Unsupported;
        path.reason = Some(reason::INTERFACE_UNVERIFIED.to_string());
        diagnostic = Some(PathDiagnostic::ProxyRemoteUnverified);
    }
    Ok((path, diagnostic))
}

fn budget_json(budget: &crate::targets::Budget) -> serde_json::Value {
    serde_json::json!({
        "run_timeout_ms": budget.run_timeout_ms,
        "request_timeout_ms": budget.request_timeout_ms,
        "max_concurrency": budget.max_concurrency,
        "per_host": budget.per_host,
        "max_redirects": budget.max_redirects,
        "max_body_bytes": budget.max_body_bytes,
        "v6_fallback_after_ms": budget.v6_fallback_after_ms,
        "min_interval_ms": budget.min_interval_ms,
        "inter_target_interval_ms": budget.inter_target_interval_ms,
        "batch_size": budget.batch_size,
        "udp_quic_concurrency": budget.udp_quic_concurrency,
        "tcp_concurrency": budget.tcp_concurrency,
        "batch_cooldown_ms": budget.batch_cooldown_ms,
        "node_retest": budget.node_retest,
    })
}

/// 应用 T-7-09a 的预算覆盖；任何越界都在产生网络 I/O 前返回错误。
fn apply_budget_overrides(
    budget: &mut crate::targets::Budget,
    options: &CheckOptions,
) -> Result<(), ConfigError> {
    if let Some(interval) = options.min_interval_ms {
        if interval > crate::targets::HARD_LIMITS.max_min_interval_ms {
            return Err(ConfigError::field("--min-interval-ms", "超出可设范围"));
        }
        budget.min_interval_ms = interval;
    }
    if let Some(interval) = options.inter_target_interval_ms {
        if interval > crate::targets::HARD_LIMITS.max_inter_target_interval_ms {
            return Err(ConfigError::field(
                "--inter-target-interval-ms",
                "超出可设范围",
            ));
        }
        budget.inter_target_interval_ms = interval;
    }
    if let Some(batch_size) = options.batch_size {
        if batch_size == 0 || batch_size > crate::targets::HARD_LIMITS.max_batch_size {
            return Err(ConfigError::field("--batch-size", "超出可设范围"));
        }
        budget.batch_size = batch_size;
    }
    if let Some(cooldown) = options.batch_cooldown_ms {
        if cooldown > crate::targets::HARD_LIMITS.max_batch_cooldown_ms {
            return Err(ConfigError::field("--batch-cooldown-ms", "超出可设范围"));
        }
        budget.batch_cooldown_ms = cooldown;
    }
    if let Some(retest) = options.node_retest {
        if retest > crate::targets::HARD_LIMITS.max_node_retest {
            return Err(ConfigError::field("--node-retest", "超出可设范围"));
        }
        budget.node_retest = retest;
    }
    Ok(())
}

/// 新建连接安全预算：超过即要求显式二次确认（受控全量模式）。
pub const SAFE_NEW_CONNECTION_BUDGET: usize = 30;

/// 运行前风险预览（零发包）：目标/节点/协议分布、预计连接/会话、并发与耗时。
pub fn network_plan(
    registry: &Registry,
    options: &CheckOptions,
) -> Result<crate::schedule::NetworkRiskPreview, ConfigError> {
    if options.node_sweep && options.proxy.is_none() {
        return Err(ConfigError::field(
            "--node-sweep",
            "节点筛选模式需要显式 --proxy：本版本不做自动节点枚举或自动切换",
        ));
    }
    let selection = registry.select(
        options.profile.as_deref(),
        &options.groups,
        &options.targets,
        options.full,
    )?;
    let mut budget = selection.budget;
    if options.fast {
        budget.run_timeout_ms = budget.run_timeout_ms.min(20_000);
        budget.max_concurrency = budget.max_concurrency.min(4);
    }
    if options.node_sweep {
        // 节点筛选：低噪声第一阶段的保守上限（每节点一个最小探针）。
        budget.min_interval_ms = budget.min_interval_ms.max(3_500);
        budget.batch_size = budget.batch_size.min(3);
        budget.node_retest = 0;
    }
    if let Some(timeout) = options.timeout_ms {
        if timeout > crate::targets::HARD_LIMITS.max_run_timeout_ms {
            return Err(ConfigError::field("--timeout", "超出可设范围"));
        }
        // timeout = 0 表示用户显式选择“不限时”；非零值覆盖 profile 预算，
        // 但仍先经 HARD_LIMITS 校验（simple 默认 0 不会被 min(0, N) 吞掉用户设置）。
        budget.run_timeout_ms = timeout;
    }
    if let Some(concurrency) = options.max_concurrency {
        if concurrency == 0 || concurrency > crate::targets::HARD_LIMITS.max_concurrency {
            return Err(ConfigError::field("--max-concurrency", "超出可设范围"));
        }
        budget.max_concurrency = budget.max_concurrency.min(concurrency);
    }
    apply_budget_overrides(&mut budget, options)?;
    let planned: Vec<crate::schedule::PlannedTarget> = selection
        .ids
        .iter()
        .filter_map(|id| registry.get(id))
        .map(|target| crate::schedule::PlannedTarget {
            target_id: target.raw.id.clone(),
            host: target.host(),
            protocol: crate::forensics::ProbeProtocol::TcpHttp,
        })
        .collect();
    Ok(crate::schedule::risk_preview(
        selection.profile.as_deref(),
        if options.proxy.is_some() || options.interface.is_some() {
            "CURRENT_PATH_WITH_EXPLICIT_PROXY_OR_INTERFACE"
        } else {
            "CURRENT_PATH"
        },
        &planned,
        &budget,
        options.node_sweep,
    ))
}

/// 运行前网络路径准备：展开选集 → 汇总 host → 探测/求值代理模式。
///
/// 会产生**有界 TCP 探测**（端口连通 + HTTP CONNECT 握手），不发送任何目标 HTTP
/// 请求；`--plan` 不应调用本函数（保持零发包）。
pub async fn prepare_network(
    registry: &Registry,
    options: &CheckOptions,
) -> Result<crate::proxy::NetworkPrep, ConfigError> {
    let selection = registry.select(
        options.profile.as_deref(),
        &options.groups,
        &options.targets,
        options.full,
    )?;
    let mut hosts: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for id in &selection.ids {
        if let Some(target) = registry.get(id) {
            let host = target.host().to_ascii_lowercase();
            if seen.insert(host.clone()) {
                hosts.push(host);
            }
        }
    }
    crate::proxy::prepare(
        options.proxy_mode,
        options.proxy.as_deref(),
        options.pac_url.as_deref(),
        &hosts,
    )
    .await
}

/// 目录顺序的覆盖快照：已覆盖 + 未覆盖（已选/未选），每个 ID 恰出现一次。
fn build_coverage(
    registry: &Registry,
    selected: &HashSet<String>,
    covered: &HashMap<String, Covered>,
    reasons: &HashMap<String, String>,
) -> Coverage {
    let mut covered_list = Vec::new();
    let mut uncovered = Vec::new();
    for entry in registry.entries() {
        let id = entry.id().to_string();
        if let Some(item) = covered.get(&id) {
            covered_list.push(item.clone());
        } else {
            uncovered.push(Uncovered {
                target: id.clone(),
                selected: selected.contains(&id),
                reason: reasons.get(&id).cloned().unwrap_or_else(|| {
                    if selected.contains(&id) {
                        reason::NO_ADMISSION.to_string()
                    } else {
                        reason::NOT_SELECTED.to_string()
                    }
                }),
            });
        }
    }
    Coverage {
        catalog_size: registry.catalog_size() as u64,
        selected_count: selected.len() as u64,
        covered: covered_list,
        uncovered,
    }
}

fn empty_result(
    target: &Target,
    path: &Path,
    state: State,
    reason_text: &str,
    diagnostic: &str,
) -> ResultRecord {
    ResultRecord {
        destination: Default::default(),
        target: target.raw.id.clone(),
        state,
        reason: Some(reason_text.to_string()),
        confidence: Confidence::None,
        source: None,
        path: path.clone(),
        duration_ms: 0,
        request_count: 0,
        evidence: Vec::new(),
        region_layers: crate::verdict::all_layers_null(),
        observed: Vec::new(),
        conflicts: Vec::new(),
        agreement: Vec::new(),
        transport: TransportLayerResult {
            attempts: Vec::new(),
            path_diagnostic: Some(diagnostic.to_string()),
        },
        service_observed: ServiceObserved {
            surface: target.raw.surface.clone(),
            observed_country: None,
            evidence_ids: Vec::new(),
            reason: Some(reason::NO_REGION_SIGNAL.to_string()),
        },
        risk_signal: RiskSignal {
            markers: Vec::new(),
            evidence_ids: Vec::new(),
            reason: None,
        },
        local_policy: LocalPolicy {
            protected: target.raw.protected,
            guard_eligible: false,
            plan_id: None,
            reason: Some(if target.raw.protected {
                reason::PROTECTED_HOST.to_string()
            } else {
                reason::NOT_GUARD_ELIGIBLE.to_string()
            }),
        },
        expectation: None,
        origin: None,
        consent_version: None,
        imported_at: None,
        rule_digest: None,
        eligibility_notes: Vec::new(),
    }
}

/// 若目标或所绑定的规则由用户导入，则在 `ResultRecord` 及所属 `Evidence` 上写入不可伪造的审计留痕。
pub fn attach_user_audit(registry: &Registry, target: &Target, result: &mut ResultRecord) {
    if let Some((origin, consent_version, imported_at, rule_digest)) =
        registry.user_audit_for(target)
    {
        for ev in &mut result.evidence {
            ev.origin = origin.clone();
        }
        result.origin = Some(origin);
        result.consent_version = Some(consent_version);
        result.imported_at = Some(imported_at);
        result.rule_digest = Some(rule_digest);
    }
}

fn path_diagnostic_for(outcome: &FetchOutcome) -> PathDiagnostic {
    network::diagnose_attempts(
        &outcome.attempts,
        crate::targets::HARD_LIMITS.v6_fallback_after_ms,
    )
}

/// 执行一次 `check`（兼容入口）。
///
/// 等价于 [`run_check_streaming`]，但内部自建一个永不取消的信号，并一次性返回全部事件。
/// 需要运行中取消或逐事件回传的调用方请直接用 `run_check_streaming`。
///
/// * `transport`：在线传输；空许可时不会被调用。
/// * `network`：该传输是否计真实网络流量（测试用合成传输传 false）。
/// * `admitted_override`：仅离线套件/测试注入的合成准入，正式 CLI 永远传 `None`。
pub async fn run_check(
    registry: &Registry,
    options: &CheckOptions,
    transport: Option<Arc<dyn Transport>>,
    network: bool,
    admitted_override: Option<HashSet<String>>,
) -> Result<RunOutput, ConfigError> {
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    run_check_streaming(
        registry,
        options,
        transport,
        network,
        admitted_override,
        cancel_rx,
        None,
    )
    .await
}

/// 执行一次 `check`，支持外部取消与逐事件回传（T-4-02 / OPT-1）。
///
/// * `cancel_rx`：外部取消信号。收到 `true` 后不再启动新目标，未完成目标记为
///   `CANCELLED`，终态用 `RunCancelled` 而不是 `RunFinished`。
/// * `event_tx`：可选的逐事件通道。事件在提交时立即推送副本；接收端关闭不影响运行。
///
/// 原 [`run_check`] 是本函数的零破坏包装：返回值与事件流均与阶段 2/3 完全一致。
pub async fn run_check_streaming(
    registry: &Registry,
    options: &CheckOptions,
    transport: Option<Arc<dyn Transport>>,
    network: bool,
    admitted_override: Option<HashSet<String>>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::contracts::Event>>,
) -> Result<RunOutput, ConfigError> {
    let started = Instant::now();
    let now = OffsetDateTime::now_utc();
    let run_id = options.run_id.clone().unwrap_or_else(|| next_run_id(now));

    if admitted_override.is_some()
        && (network
            || !transport
                .as_ref()
                .is_some_and(|t| t.allows_synthetic_admission()))
    {
        return Err(ConfigError::field("check", "合成准入只允许离线替身"));
    }
    if let Some(case_id) = &options.fixture {
        if options.profile.is_some()
            || options.full
            || !options.groups.is_empty()
            || !options.targets.is_empty()
        {
            return Err(ConfigError::field("--fixture", "与在线选择器互斥"));
        }
        if options.state_dir.is_some() {
            return Err(ConfigError::field(
                "--fixture",
                "拒绝真实状态目录；离线套件只注入虚拟状态目录",
            ));
        }
        if options.proxy.is_some() || options.interface.is_some() {
            return Err(ConfigError::field(
                "--fixture",
                "与 --proxy/--interface 互斥",
            ));
        }
        return run_fixture(
            registry,
            case_id,
            &run_id,
            options.expect.as_deref(),
            now,
            event_tx,
            || started.elapsed().as_millis() as u64,
        );
    }

    if options.node_sweep && options.proxy.is_none() {
        return Err(ConfigError::field(
            "--node-sweep",
            "节点筛选模式需要显式 --proxy：本版本不做自动节点枚举或自动切换",
        ));
    }
    let selection = registry.select(
        options.profile.as_deref(),
        &options.groups,
        &options.targets,
        options.full,
    )?;
    let mut budget = selection.budget;
    if options.fast {
        budget.run_timeout_ms = budget.run_timeout_ms.min(20_000);
        budget.max_concurrency = budget.max_concurrency.min(4);
    }
    if options.node_sweep {
        // 节点筛选：低噪声第一阶段的保守上限（每节点一个最小探针）。
        budget.min_interval_ms = budget.min_interval_ms.max(3_500);
        budget.batch_size = budget.batch_size.min(3);
        budget.node_retest = 0;
    }
    if let Some(timeout) = options.timeout_ms {
        if timeout > crate::targets::HARD_LIMITS.max_run_timeout_ms {
            return Err(ConfigError::field("--timeout", "超出可设范围"));
        }
        // timeout = 0 表示用户显式选择“不限时”；非零值覆盖 profile 预算，
        // 但仍先经 HARD_LIMITS 校验（simple 默认 0 不会被 min(0, N) 吞掉用户设置）。
        budget.run_timeout_ms = timeout;
    }
    if let Some(concurrency) = options.max_concurrency {
        if concurrency == 0 || concurrency > crate::targets::HARD_LIMITS.max_concurrency {
            return Err(ConfigError::field("--max-concurrency", "超出可设范围"));
        }
        budget.max_concurrency = budget.max_concurrency.min(concurrency);
    }
    budget.per_host = budget
        .per_host
        .min(budget.max_concurrency)
        .min(crate::targets::HARD_LIMITS.per_host);
    apply_budget_overrides(&mut budget, options)?;

    let (path, path_diagnostic) = path_for(options)?;
    let expectation = Registry::build_expectation(options.expect.as_deref())?;
    #[cfg(not(target_arch = "wasm32"))]
    let store: Box<dyn FreshnessStore> = match &options.state_dir {
        Some(dir) => Box::new(DirStore::open(dir)?),
        None => Box::new(NullStore::default()),
    };
    #[cfg(target_arch = "wasm32")]
    let store: Box<dyn FreshnessStore> = {
        if options.state_dir.is_some() {
            return Err(ConfigError::field(
                "--state-dir",
                "wasm 端没有可写的状态目录",
            ));
        }
        Box::new(NullStore::default())
    };

    let selected: HashSet<String> = selection.ids.iter().cloned().collect();
    let selected_ids: Vec<String> = selection.ids.clone();
    let mut log = EventLog::with_sink(&run_id, false, now, event_tx);
    log.run_started(
        "ONLINE",
        selection.profile.as_deref(),
        "parallaxe.assets/v1",
        &selected_ids,
    );

    let mut admitted_ids = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut reasons: HashMap<String, String> = HashMap::new();
    let admission_time = crate::state::format_time(now);
    // F3：原子取出一次性会话级许可（消费即失效，绝不延续至下次运行）
    let one_time_session_grant = registry.take_session_grant();
    let is_full_profile = options.full || options.profile.as_deref() == Some("full");
    for id in &selected_ids {
        let target = registry
            .get(id)
            .ok_or_else(|| ConfigError::field("selection", format!("未知目标 {id}")))?;
        let admission = if admitted_override
            .as_ref()
            .is_some_and(|ids| ids.contains(id))
        {
            crate::targets::Admission {
                admitted: true,
                status: "ADMITTED",
                reason: None,
            }
        } else {
            let base_adm = registry.admission(target, selection.implicit_default, &admission_time);
            if base_adm.admitted {
                base_adm
            } else if let Some(grant) = &one_time_session_grant {
                let sess_adm = registry.check_session_grant_admission(
                    grant,
                    target,
                    selection.implicit_default,
                    is_full_profile,
                );
                if sess_adm.admitted {
                    sess_adm
                } else {
                    base_adm
                }
            } else {
                base_adm
            }
        };
        if admission.admitted {
            admitted_ids.push(id.clone());
        } else {
            let reason_text = admission.reason.unwrap_or(reason::NO_ADMISSION).to_string();
            skipped.push((id.clone(), reason_text.clone()));
            reasons.insert(id.clone(), reason_text);
        }
    }
    // 接口绑定不可验证时：报告 UNSUPPORTED，不静默直连。
    let interface_unsupported = path.bind_status == BindStatus::Unsupported;
    if !interface_unsupported
        && admitted_ids.len() > SAFE_NEW_CONNECTION_BUDGET
        && !options.confirm_budget
    {
        return Err(ConfigError::field(
            "budget",
            format!(
                "本次将新建 {} 个连接，超过安全预算 {}；请先 --plan 预览并加 --confirm-budget 显式确认",
                admitted_ids.len(),
                SAFE_NEW_CONNECTION_BUDGET
            ),
        ));
    }
    if interface_unsupported {
        admitted_ids.clear();
        skipped.clear();
        for id in &selected_ids {
            skipped.push((id.clone(), reason::UNSUPPORTED_CAPABILITY.to_string()));
            reasons.insert(id.clone(), reason::UNSUPPORTED_CAPABILITY.to_string());
        }
    }
    log.preflight_ready(&admitted_ids, &skipped, budget_json(&budget), &path);

    let mut report = Report::new(
        crate::VERSION,
        crate::state::format_time(now),
        &run_id,
        Mode::Online,
    );
    report.wall_duration_ms = started.elapsed().as_millis() as u64;

    let mut results: Vec<ResultRecord> = Vec::new();
    let mut covered: HashMap<String, Covered> = HashMap::new();
    let mut network_request_count = 0u64;
    let mut termination = Termination::Complete;

    if interface_unsupported {
        for id in &selected_ids {
            if let Some(target) = registry.get(id) {
                let mut res = empty_result(
                    target,
                    &path,
                    State::Unsupported,
                    reason::UNSUPPORTED_CAPABILITY,
                    "UNSUPPORTED_CAPABILITY",
                );
                attach_user_audit(registry, target, &mut res);
                results.push(res);
            }
        }
    } else if !admitted_ids.is_empty() {
        let Some(transport) = transport else {
            return Err(ConfigError::field("check", "缺少传输端口"));
        };
        let requested_family = match path.requested_family {
            RequestedFamily::V4 => AttemptFamily::V4,
            RequestedFamily::V6 => AttemptFamily::V6,
            RequestedFamily::Auto => AttemptFamily::Auto,
        };
        let mut requests = Vec::new();
        for id in &admitted_ids {
            let target = registry
                .get(id)
                .ok_or_else(|| ConfigError::field("check", "准入目录不一致"))?;
            let request = FetchRequest {
                target_id: target.raw.id.clone(),
                url: target.raw.url.clone(),
                host: target.host(),
                method: target.defaults.method.clone(),
                family: requested_family,
                protocol: transport.binding(id).0,
                timeout_ms: budget.request_timeout_ms,
                max_body_bytes: target.defaults.max_body_bytes.min(budget.max_body_bytes),
                max_redirects: target
                    .max_redirects()
                    .min(crate::targets::HARD_LIMITS.max_redirects),
            };
            requests.push(request);
        }
        let cancel_requested = *cancel_rx.borrow();
        let batch = probe::run_batch_checked(
            transport,
            requests,
            budget,
            cancel_rx.clone(),
            options.qos_control.clone().unwrap_or_default(),
        )
        .await?;
        let timed_out = batch.timed_out;
        let stopped_for_http = batch.stopped_for_http;
        if cancel_requested || *cancel_rx.borrow() {
            termination = Termination::Cancelled;
        } else if timed_out {
            termination = Termination::GlobalTimeout;
        } else if stopped_for_http {
            termination = Termination::Partial;
        }
        let mut by_target: HashMap<_, _> = batch
            .items
            .into_iter()
            .map(|(id, out, markers)| (id, (out, markers)))
            .collect();
        for id in &admitted_ids {
            let target = registry
                .get(id)
                .ok_or_else(|| ConfigError::field("check", format!("未知目标 {id}")))?;
            let (outcome, qos_markers) = match by_target.remove(id) {
                Some(outcome) => outcome,
                None => {
                    let missing_reason = if stopped_for_http {
                        reason::QOS_HTTP_STOP
                    } else if termination == Termination::GlobalTimeout {
                        reason::GLOBAL_TIMEOUT
                    } else {
                        reason::CANCELLED
                    };
                    let mut result = empty_result(
                        target,
                        &path,
                        State::Unknown,
                        missing_reason,
                        missing_reason,
                    );
                    result.expectation = expectation
                        .as_ref()
                        .map(crate::verdict::expectation_for_empty);
                    attach_user_audit(registry, target, &mut result);
                    reasons.insert(id.clone(), missing_reason.to_string());
                    log.target_started(id, &path.path_id, 1);
                    log.target_finished(&result);
                    results.push(result);
                    continue;
                }
            };
            log.target_started(id, &path.path_id, 1);
            let sample = Sample::from_outcome(&outcome);
            let evaluation = signals::evaluate(
                registry,
                target,
                &sample,
                &path.path_id,
                &crate::state::format_time(now),
                false,
            );
            let diagnostic = path_diagnostic.unwrap_or_else(|| path_diagnostic_for(&outcome));
            let mut result = verdict::decide(VerdictInput {
                registry,
                target,
                evaluation: &evaluation,
                outcome: &outcome,
                path: &path,
                path_diagnostic: diagnostic,
                fixture: false,
                now,
                store: store.as_ref(),
                expectation: expectation.as_ref(),
                capability_unsupported: None,
                cancelled: false,
                global_timeout: false,
            });
            if let Some(marker) =
                crate::forensics::service_contract_marker(target, &sample, &result)
            {
                result.risk_signal.markers.push(marker);
            }
            result.risk_signal.markers.extend(qos_markers);
            attach_user_audit(registry, target, &mut result);
            for item in &result.evidence {
                log.evidence_observed(id, item);
            }
            network_request_count += if network { outcome.request_count } else { 0 };
            covered.insert(
                id.clone(),
                verdict::covered(&result, outcome.http_received()),
            );
            log.target_finished(&result);
            results.push(result);
        }
    }

    if let Some(error) = store.error() {
        return Err(ConfigError::field("--state-dir", error));
    }
    report.results = results;
    report.coverage = build_coverage(registry, &selected, &covered, &reasons);
    report.network_request_count = network_request_count;
    report.simulated_request_count = 0;
    report.termination = termination;
    report.wall_duration_ms = started.elapsed().as_millis() as u64;
    report.exit_code = reports::aggregate_report(&report);
    log.coverage_updated(&report.coverage);
    if termination == Termination::Cancelled {
        log.run_cancelled(&report);
    } else {
        log.run_finished(&report);
    }
    Ok(RunOutput {
        report,
        events: log.into_events(),
    })
}

/// 使用指定时刻回放合成样本。无需系统时钟、Tokio 运行时或网络权限。
pub fn replay_fixture(
    registry: &Registry,
    case_id: &str,
    run_id: &str,
    now: OffsetDateTime,
) -> Result<RunOutput, ConfigError> {
    run_fixture(registry, case_id, run_id, None, now, None, || 0)
}

fn run_fixture(
    registry: &Registry,
    case_id: &str,
    run_id: &str,
    expect: Option<&str>,
    now: OffsetDateTime,
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::contracts::Event>>,
    elapsed_ms: impl FnOnce() -> u64,
) -> Result<RunOutput, ConfigError> {
    let case = fixtures::find(case_id).ok_or_else(|| {
        ConfigError::field(
            format!("--fixture {case_id}"),
            format!("未登记案例；可用：{}", fixtures::case_ids().join(", ")),
        )
    })?;
    let target = registry
        .get(case.target_id)
        .ok_or_else(|| ConfigError::field("fixture", "目标不存在"))?;
    let store = NullStore::default();
    let expectation = Registry::build_expectation(expect)?;
    let mut path = Path::system("path-fixture");
    if case.id == "address-proxy" {
        path.proxy_mode = "EXPLICIT".into();
        path.proxy_ref = Some("proxy-fixture".into());
    }
    let mut log = EventLog::with_sink(run_id, true, now, event_tx);
    log.run_started(
        "FIXTURE",
        Some("fixture"),
        "parallaxe.fixtures/v1",
        std::slice::from_ref(&target.raw.id),
    );
    log.preflight_ready(
        std::slice::from_ref(&target.raw.id),
        &[],
        serde_json::json!({"simulated": true}),
        &path,
    );
    let evaluation = signals::evaluate(
        registry,
        target,
        &case.sample,
        &path.path_id,
        &crate::state::format_time(now),
        true,
    );
    let attempts = vec![Attempt {
        family: AttemptFamily::V4,
        phase: "FIXTURE".to_string(),
        connect_ms: Some(1),
        first_byte_ms: Some(2),
        http_status: case.sample.status,
        fallback: false,
        request_sent: case.sample.status.is_some(),
    }];
    let outcome = FetchOutcome {
        destination: case.destination(),
        status: case.sample.status,
        content_type: case.sample.content_type.clone(),
        location: case.sample.location.clone(),
        body: case.sample.body.clone(),
        complete: case.sample.complete,
        truncated: case.sample.truncated,
        attempts,
        request_count: case.expected_request_count,
        error: None,
        observed_family: case.sample.observed_family,
    };
    log.target_started(target.id(), &path.path_id, 1);
    let result = verdict::decide(VerdictInput {
        registry,
        target,
        evaluation: &evaluation,
        outcome: &outcome,
        path: &path,
        path_diagnostic: PathDiagnostic::System,
        fixture: true,
        now,
        store: &store,
        expectation: expectation.as_ref(),
        capability_unsupported: None,
        cancelled: false,
        global_timeout: false,
    });
    for item in &result.evidence {
        log.evidence_observed(target.id(), item);
    }
    log.target_finished(&result);
    let selected: HashSet<String> = [target.raw.id.clone()].into_iter().collect();
    let mut covered = HashMap::new();
    covered.insert(
        target.raw.id.clone(),
        verdict::covered(&result, case.sample.status.is_some()),
    );
    let mut report = Report::new(
        crate::VERSION,
        crate::state::format_time(now),
        run_id,
        Mode::Fixture,
    );
    report.results = vec![result];
    report.coverage = build_coverage(registry, &selected, &covered, &HashMap::new());
    report.simulated_request_count = case.expected_request_count;
    report.network_request_count = 0;
    report.termination = Termination::Complete;
    report.wall_duration_ms = elapsed_ms();
    report.exit_code = reports::aggregate_report(&report);
    log.coverage_updated(&report.coverage);
    log.run_finished(&report);
    Ok(RunOutput {
        report,
        events: log.into_events(),
    })
}

/// 只读解释：静态目录 + 绑定规则；不补探。
pub fn explain_target(registry: &Registry, id: &str) -> Result<serde_json::Value, ConfigError> {
    let target = registry
        .get(id)
        .ok_or_else(|| ConfigError::field(format!("--target {id}"), "未知目标 ID"))?;
    let now = crate::state::format_time(OffsetDateTime::now_utc());
    let admission = registry.admission(target, false, &now);
    let rules: Vec<serde_json::Value> = registry
        .rules_for(target)
        .iter()
        .map(|rule| {
            serde_json::json!({
                "id": rule.id,
                "version": rule.version,
                "kind": rule.extract.kind,
                "layer": rule.layer,
                "channel": rule.channel,
                "credibility": rule.credibility,
                "notes": rule.notes,
                "negative_case": rule.negative_case,
            })
        })
        .collect();
    let dependencies = registry
        .dependencies
        .explain_for_host(&target.host())
        .iter()
        .map(|edge| {
            serde_json::json!({
                "src": edge.src,
                "dst": edge.dst,
                "kind": edge.kind,
                "source": edge.source,
            })
        })
        .collect::<Vec<_>>();
    let mut target_obj = serde_json::json!({
        "id": target.raw.id,
        "family": target.raw.family,
        "group": target.raw.group,
        "matrix_group": target.raw.matrix_group,
        "url": target.raw.url,
        "surface": target.raw.surface,
        "protected": target.raw.protected,
        "verification": target.raw.verification,
        "confidence": target.raw.confidence,
        "confidence_basis": target.raw.confidence_basis,
        "failure": target.raw.failure,
        "allow_online": target.raw.allow_online,
        "safe_default": target.raw.safe_default,
        "admission_status": admission.status,
        "reason": admission.reason,
    });
    if let Some((origin, consent_version, imported_at, rule_digest)) =
        registry.user_audit_for(target)
    {
        if let Some(map) = target_obj.as_object_mut() {
            map.insert("origin".to_string(), serde_json::Value::String(origin));
            map.insert(
                "consent_version".to_string(),
                serde_json::Value::String(consent_version),
            );
            map.insert(
                "imported_at".to_string(),
                serde_json::Value::String(imported_at),
            );
            map.insert(
                "rule_digest".to_string(),
                serde_json::Value::String(rule_digest),
            );
        }
    }
    Ok(serde_json::json!({
        "schema": "parallaxe.explain/v1",
        "target": target_obj,
        "rules": rules,
        "dependencies": dependencies,
        "limitations": [
            "静态解释不含任何在线探测结果",
            "共享出口与远端账号影响不可由本工具证明"
        ]
    }))
}

/// 只读依赖解释。
pub fn explain_dependencies(
    registry: &Registry,
    id: &str,
) -> Result<serde_json::Value, ConfigError> {
    let target = registry
        .get(id)
        .ok_or_else(|| ConfigError::field(format!("--target {id}"), "未知目标 ID"))?;
    let host = target.host();
    let edges: Vec<serde_json::Value> = registry
        .dependencies
        .explain_for_host(&host)
        .iter()
        .map(|edge| {
            serde_json::json!({
                "src": edge.src,
                "dst": edge.dst,
                "kind": edge.kind,
                "source_type": edge.source_type,
                "source": edge.source,
                "verified_at": edge.verified_at.to_string(),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "schema": "parallaxe.dependencies/v1",
        "target": id,
        "host": host,
        "edges": edges,
        "limitations": [
            "命中或未命中都不能证明专属性或安全阻断",
            "影响闭包与规则生成属于阶段 4"
        ]
    }))
}

/// 供测试与 selftest 使用的诊断文本。
pub fn diagnostic_text(diagnostic: PathDiagnostic) -> &'static str {
    diagnostic.as_str()
}

/// 列出用户显式优先级（示例文件不当作真实状态）。
pub fn priorities_list(_registry: &Registry) -> serde_json::Value {
    serde_json::json!({
        "schema": "parallaxe.priorities/v1",
        "selected_by": "USER",
        "services": [],
        "notes": [
            "示例文件 priorities.example.toml 不当作真实用户状态",
            "本工具不擅自降低任何服务的优先级"
        ]
    })
}

/// 供测试构造预期。
pub fn expectation_from_spec(spec: &ExpectationSpec) -> Expectation {
    crate::verdict::expectation_for_empty(spec)
}

/// F3：对用户已导入的目标与规则执行离线合成预览（`network_request_count = 0`，不发起任何真实网络请求，完整保留 `origin=USER_IMPORTED` 等审计留痕）。
pub async fn run_user_import_offline_preview(
    registry: &Registry,
    target_ids: &[String],
    custom_sample: Option<Sample>,
    run_id: Option<String>,
) -> Result<RunOutput, ConfigError> {
    if target_ids.is_empty() {
        return Err(ConfigError::field(
            "offline_preview",
            "请先选择至少一个用户导入的目标进行离线预览",
        ));
    }
    let started = Instant::now();
    let now = OffsetDateTime::now_utc();
    let now_str = crate::state::format_time(now);
    let run_id =
        run_id.unwrap_or_else(|| format!("run-user-preview-{}", now.unix_timestamp_nanos()));
    let path = Path::system("path-user-offline-preview");
    let store = NullStore::default();

    let mut targets = Vec::with_capacity(target_ids.len());
    for id in target_ids {
        let target = registry
            .get(id)
            .ok_or_else(|| ConfigError::field("offline_preview", format!("未知目标 ID：{id}")))?;
        targets.push(target);
    }

    let mut log = EventLog::new(&run_id, true, now);
    log.run_started(
        "FIXTURE",
        Some("user-offline-preview"),
        "parallaxe.user-import/v1",
        target_ids,
    );
    log.preflight_ready(
        target_ids,
        &[],
        serde_json::json!({"simulated": true, "origin": crate::contracts::ORIGIN_USER_IMPORTED}),
        &path,
    );

    let mut results = Vec::with_capacity(targets.len());
    let mut covered = HashMap::new();
    for target in targets {
        let sample = match &custom_sample {
            Some(s) => s.clone(),
            None => synthesize_preview_sample_for_target(registry, target),
        };
        log.target_started(target.id(), &path.path_id, 1);
        let evaluation =
            signals::evaluate(registry, target, &sample, &path.path_id, &now_str, true);
        let outcome = FetchOutcome {
            destination: Default::default(),
            status: sample.status,
            content_type: sample.content_type.clone(),
            location: sample.location.clone(),
            body: sample.body.clone(),
            complete: sample.complete,
            truncated: sample.truncated,
            attempts: vec![Attempt {
                family: AttemptFamily::V4,
                phase: "FIXTURE".to_string(),
                connect_ms: Some(1),
                first_byte_ms: Some(2),
                http_status: sample.status,
                fallback: false,
                request_sent: sample.status.is_some(),
            }],
            request_count: 1,
            error: None,
            observed_family: sample.observed_family,
        };
        let mut result = verdict::decide(VerdictInput {
            registry,
            target,
            evaluation: &evaluation,
            outcome: &outcome,
            path: &path,
            path_diagnostic: PathDiagnostic::System,
            fixture: true,
            now,
            store: &store,
            expectation: None,
            capability_unsupported: None,
            cancelled: false,
            global_timeout: false,
        });
        attach_user_audit(registry, target, &mut result);
        for ev in &result.evidence {
            log.evidence_observed(target.id(), ev);
        }
        covered.insert(
            target.raw.id.clone(),
            verdict::covered(&result, sample.status.is_some()),
        );
        log.target_finished(&result);
        results.push(result);
    }

    let selected: HashSet<String> = target_ids.iter().cloned().collect();
    let mut report = Report::new(crate::VERSION, now_str, &run_id, Mode::Fixture);
    report.results = results;
    report.coverage = build_coverage(registry, &selected, &covered, &HashMap::new());
    report.simulated_request_count = target_ids.len() as u64;
    report.network_request_count = 0;
    report.termination = Termination::Complete;
    report.wall_duration_ms = started.elapsed().as_millis() as u64;
    report.exit_code = reports::aggregate_report(&report);
    log.coverage_updated(&report.coverage);
    log.run_finished(&report);
    Ok(RunOutput {
        report,
        events: log.into_events(),
    })
}

fn synthesize_preview_sample_for_target(registry: &Registry, target: &Target) -> Sample {
    let rules = registry.rules_for(target);
    let has_kind = |k: &str| rules.iter().any(|r| r.extract.kind == k);
    if has_kind("text_kv") {
        Sample {
            status: Some(200),
            content_type: Some("text/plain; charset=utf-8".to_string()),
            location: None,
            body: b"fl=29f1\nh=example.com\nip=203.0.113.10\nts=1700000000.000\nvisit_scheme=https\nuag=Parallaxe\ncolo=SJC\nsliver=none\nhttp=http/2\nloc=US\ntls=TLSv1.3\nsni=plaintext\nwarp=off\ngateway=off\nrbi=off\nkex=X25519\n".to_vec(),
            complete: true,
            truncated: false,
            observed_family: Some(crate::contracts::ObservedFamily::V4),
        }
    } else if has_kind("json") {
        Sample {
            status: Some(200),
            content_type: Some("application/json; charset=utf-8".to_string()),
            location: None,
            body: br#"{"country":"US","colo":"SJC","asn":13335}"#.to_vec(),
            complete: true,
            truncated: false,
            observed_family: Some(crate::contracts::ObservedFamily::V4),
        }
    } else if has_kind("redirect_host") || has_kind("redirect_param") {
        Sample {
            status: Some(302),
            content_type: Some("text/html; charset=utf-8".to_string()),
            location: Some("https://news.google.com/?hl=en-US&gl=US&ceid=US:en".to_string()),
            body: b"<html>Redirecting...</html>".to_vec(),
            complete: true,
            truncated: false,
            observed_family: Some(crate::contracts::ObservedFamily::V4),
        }
    } else {
        Sample {
            status: Some(200),
            content_type: Some("text/html; charset=utf-8".to_string()),
            location: None,
            body: b"<html><head><title>Preview</title></head><body><div>From your internet address</div><script>AF_initDataCallback({data:[,2,1,200,\"USA\"],\"GL\":\"US\"})</script><img src=\"regionflags/us.png\"/></body></html>".to_vec(),
            complete: true,
            truncated: false,
            observed_family: Some(crate::contracts::ObservedFamily::V4),
        }
    }
}
