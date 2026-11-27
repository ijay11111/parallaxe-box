//! `parallaxe agent --stdio`：一行一个 UTF-8 JSON 对象，stdout 只出协议对象。
//!
//! 九个操作全部投影共享 core；无 shell、无任意 URL/代理控制、无 apply。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use parallaxe_core::ai_bridge::{
    AiScope, BridgeControl, BridgePaths, BridgeRequest, BridgeResult, BridgeStatus, DecisionKind,
    ResultStatus, MAX_BATCH_SIZE, MAX_TARGETS, MIN_INTERVAL_MS,
};
use parallaxe_core::contracts::{Event, Mode, Report};
use parallaxe_core::guard;
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::reports;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::selftest;
use parallaxe_core::targets::{ConfigError, Registry};

const OPERATIONS: [&str; 13] = [
    "list_targets",
    "load_profile",
    "check",
    "explain",
    "selftest",
    "guard_plan",
    "export_report",
    "cancel",
    "isolate",
    "ai_status",
    "ai_request_check",
    "ai_request_status",
    "ai_autonomous",
];

/// 协议版本：v1 的 9 个只读操作不变；v2 追加任务 A 的 4 个 AI 授权/取回操作。
const PROTOCOL_VERSION: u8 = 2;

struct RunRecord {
    report: Report,
    /// 已完成运行的事件快照；阶段 2 的 export 只用报告，保留以便 GUI/重放。
    #[allow(dead_code)]
    events: Vec<Event>,
}

struct Session {
    runs: HashMap<String, RunRecord>,
    /// 活动运行（本阶段运行同步完成，保留字段以表达 max_active_runs=1 的语义）。
    active: Option<String>,
    /// AI 授权桥（文件级、无监听端口；路径由 `PARALLAXE_AI_BRIDGE_DIR` 或系统临时目录决定）。
    bridge: BridgePaths,
    /// 本会话内由 agent 提交、尚未取回结果的请求。
    ai_pending: HashMap<String, BridgeRequest>,
    /// agent 自主关闭过自主检测后，即使 GUI 状态文件暂时滞后也不再按自主模式提交。
    ai_revoked: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            runs: HashMap::new(),
            active: None,
            bridge: BridgePaths::resolve(),
            ai_pending: HashMap::new(),
            ai_revoked: false,
        }
    }
}

impl Session {
    fn next_run_id(&mut self, prefix: &str) -> String {
        // 会话内追加序号，即使时钟同刻或回拨也不覆盖旧运行。
        format!(
            "{prefix}-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            self.runs.len()
        )
    }

    fn store(&mut self, run_id: String, report: Report, events: Vec<Event>) -> String {
        self.runs
            .insert(run_id.clone(), RunRecord { report, events });
        run_id
    }
}

/// 会话级 locale（0=zh-CN，1=en-US）；仅影响可选 `message_localized`。
static AGENT_LOCALE: AtomicU8 = AtomicU8::new(0);

fn agent_locale() -> Locale {
    if AGENT_LOCALE.load(Ordering::Relaxed) == 1 {
        Locale::EnUs
    } else {
        Locale::ZhCn
    }
}

fn agent_error_key(code: &str) -> &'static str {
    match code {
        "INVALID_ARGUMENT" => "agent.error.invalid_argument",
        "UNSUPPORTED_SCHEMA" => "agent.error.unsupported_schema",
        "UNKNOWN_RUN" => "agent.error.unknown_run",
        "BUSY" => "agent.error.busy",
        "INTERNAL" => "agent.error.internal",
        "POLICY_DENIED" => "agent.error.policy_denied",
        "BRIDGE_UNAVAILABLE" => "agent.error.bridge_unavailable",
        "AUTONOMOUS_DISABLED" => "agent.error.autonomous_disabled",
        _ => "agent.error.generic",
    }
}

fn localized_agent_message(code: &str, message: &str) -> String {
    let locale = agent_locale();
    if locale == Locale::ZhCn {
        return message.to_string();
    }
    // 常见静态消息给出真正的英文；未知动态消息保留原文并由英文模板包裹。
    let translated = match message {
        "无法解析的 JSON 行" => "Unparsable JSON line",
        "缺少字符串 id" => "Missing string id",
        "只接受 parallaxe.agent/v1" => "Only parallaxe.agent/v1 is accepted",
        "缺少 op" => "Missing op",
        "缺少 profile" => "Missing profile",
        "未知 profile" => "Unknown profile",
        "缺少 run_id" => "Missing run_id",
        "缺少 target" => "Missing target",
        "未知运行 ID" => "Unknown run ID",
        "目标不在该运行内" => "Target is not part of this run",
        "缺少 format" => "Missing format",
        "format 只支持 json/html/csv/md" => "format only supports json/html/csv/md",
        "缺少 source_run_id" => "Missing source_run_id",
        "未知目标 ID" => "Unknown target ID",
        "action 只支持 plan/explain" => "action only supports plan/explain",
        "已有活动运行" => "A run is already active",
        "参数类型、范围或选择器组合不合法" => {
            "Invalid argument type, range or selector combination"
        }
        "agent 单条请求不能追加 state_dir/proxy/interface" => {
            "A single agent request cannot add state_dir/proxy/interface"
        }
        "未检测到 GUI 授权桥" => "No GUI authorization bridge detected",
        "自主检测只能由用户在 GUI 设置中打开" => {
            "Autonomous detection can only be enabled by the user in the GUI settings"
        }
        "授权决定与请求范围不匹配" => {
            "The authorization decision does not match the request scope"
        }
        "请求忙" => "The request is busy",
        "请求被拒绝" => "Request denied",
        _ => "",
    };
    if !translated.is_empty() {
        return translated.to_string();
    }
    translate(locale, agent_error_key(code), &[message])
}

fn error_response(id: Option<&str>, code: &str, message: &str) -> Value {
    json!({
        "id": id,
        "ok": false,
        "error": {
            "code": code,
            "message_safe": message,
            "message_localized": localized_agent_message(code, message),
        },
    })
}

fn ok_response(id: &str, data: Value) -> Value {
    json!({"id": id, "ok": true, "data": data})
}

fn event_notification(id: &str, event: &Event) -> Value {
    json!({"id": id, "record": event})
}

fn string_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(|text| text.to_string())
}

fn string_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|text| text.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn handle_list_targets(registry: &Registry, args: &Value) -> Value {
    let group = string_arg(args, "group");
    let selection = match &group {
        Some(group) => registry.select(None, std::slice::from_ref(group), &[], false),
        None => registry.select(None, &[], &[], true),
    };
    let selection = match selection {
        Ok(selection) => selection,
        Err(error) => {
            return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": error.to_string()}})
        }
    };
    let selected: std::collections::HashSet<&str> =
        selection.ids.iter().map(String::as_str).collect();
    let now = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
    let items: Vec<Value> = registry
        .entries()
        .iter()
        .filter(|entry| selected.contains(entry.id()))
        .map(|entry| {
            let admission = registry.admission(entry, false, &now);
            json!({
                "id": entry.raw.id,
                "group": entry.raw.group,
                "protected": entry.raw.protected,
                "admission_status": admission.status,
                "reason": admission.reason,
            })
        })
        .collect();
    json!({
        "protocol_version": PROTOCOL_VERSION,
        "operations": OPERATIONS,
        "max_active_runs": 1,
        "targets": items,
    })
}

fn handle_load_profile(registry: &Registry, args: &Value) -> Value {
    let Some(profile_id) = string_arg(args, "profile") else {
        return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "缺少 profile"}});
    };
    let Some(profile) = registry.profile(&profile_id) else {
        return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "未知 profile"}});
    };
    let selection = match registry.select(Some(&profile_id), &[], &[], false) {
        Ok(selection) => selection,
        Err(error) => {
            return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": error.to_string()}})
        }
    };
    json!({
        "profile": profile.id,
        "selected_ids": selection.ids,
        "budget": {
            "run_timeout_ms": profile.run_timeout_ms,
            "request_timeout_ms": profile.request_timeout_ms,
            "max_concurrency": profile.max_concurrency,
            "per_host": profile.per_host,
        }
    })
}

fn check_options(args: &Value) -> Result<CheckOptions, &'static str> {
    if args.get("state_dir").is_some()
        || args.get("proxy").is_some()
        || args.get("proxy_mode").is_some()
        || args.get("pac_url").is_some()
        || args.get("interface").is_some()
    {
        return Err("POLICY_DENIED");
    }
    let object = args.as_object().ok_or("INVALID_ARGUMENT")?;
    for (key, value) in object {
        let valid = match key.as_str() {
            "group" | "target" => {
                value.is_string()
                    || value
                        .as_array()
                        .is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_string))
            }
            "profile" | "fixture" | "expect" => value.is_string(),
            "full" | "ipv4" | "fast" | "plan" | "confirm_budget" | "node_sweep" => {
                value.is_boolean()
            }
            "timeout_ms"
            | "max_concurrency"
            | "min_interval_ms"
            | "inter_target_interval_ms"
            | "batch_size"
            | "batch_cooldown_ms"
            | "node_retest" => value.is_u64(),
            _ => false,
        };
        if !valid {
            return Err("INVALID_ARGUMENT");
        }
    }
    let selector = |key| {
        string_arg(args, key)
            .map(|s| vec![s])
            .unwrap_or_else(|| string_array(args, key))
    };
    let groups = selector("group");
    let targets = selector("target");
    let profile = string_arg(args, "profile");
    let fixture = string_arg(args, "fixture");
    if profile.is_some() && (!groups.is_empty() || !targets.is_empty() || fixture.is_some()) {
        return Err("INVALID_ARGUMENT");
    }
    Ok(CheckOptions {
        profile,
        groups,
        targets,
        full: args
            .get("full")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        expect: string_arg(args, "expect"),
        fixture,
        state_dir: None,
        proxy: None,
        proxy_mode: parallaxe_core::ProxyMode::Auto,
        pac_url: None,
        proxy_path: None,
        proxy_note: None,
        interface: None,
        ipv4: args
            .get("ipv4")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        fast: args
            .get("fast")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        timeout_ms: args.get("timeout_ms").and_then(|value| value.as_u64()),
        max_concurrency: args
            .get("max_concurrency")
            .and_then(|value| value.as_u64())
            .map(u32::try_from)
            .transpose()
            .map_err(|_| "INVALID_ARGUMENT")?,
        min_interval_ms: args.get("min_interval_ms").and_then(|value| value.as_u64()),
        inter_target_interval_ms: args
            .get("inter_target_interval_ms")
            .and_then(|value| value.as_u64()),
        batch_size: args
            .get("batch_size")
            .and_then(|value| value.as_u64())
            .map(u32::try_from)
            .transpose()
            .map_err(|_| "INVALID_ARGUMENT")?,
        batch_cooldown_ms: args
            .get("batch_cooldown_ms")
            .and_then(|value| value.as_u64()),
        node_retest: args
            .get("node_retest")
            .and_then(|value| value.as_u64())
            .map(u32::try_from)
            .transpose()
            .map_err(|_| "INVALID_ARGUMENT")?,
        plan: args
            .get("plan")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        confirm_budget: args
            .get("confirm_budget")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        node_sweep: args
            .get("node_sweep")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        qos_control: None,
        run_id: None,
    })
}

fn handle_explain(session: &Session, args: &Value) -> Value {
    let Some(run_id) = string_arg(args, "run_id") else {
        return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "缺少 run_id"}});
    };
    let Some(target) = string_arg(args, "target") else {
        return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "缺少 target"}});
    };
    let Some(record) = session.runs.get(&run_id) else {
        return json!({"error": {"code": "UNKNOWN_RUN", "message_safe": "未知运行 ID"}});
    };
    if let Some(result) = record
        .report
        .results
        .iter()
        .find(|result| result.target == target)
    {
        return json!({"result": result});
    }
    if let Some(item) = record
        .report
        .coverage
        .uncovered
        .iter()
        .find(|item| item.target == target)
    {
        return json!({"uncovered": item});
    }
    json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "目标不在该运行内"}})
}

fn handle_export(session: &Session, args: &Value) -> Value {
    let Some(run_id) = string_arg(args, "run_id") else {
        return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "缺少 run_id"}});
    };
    let Some(format) = string_arg(args, "format") else {
        return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "缺少 format"}});
    };
    let Some(record) = session.runs.get(&run_id) else {
        return json!({"error": {"code": "UNKNOWN_RUN", "message_safe": "未知运行 ID"}});
    };
    let content = match format.as_str() {
        "json" => reports::to_json(&record.report),
        "html" => reports::to_html(&record.report),
        "csv" => reports::to_csv(&record.report),
        "md" => reports::to_markdown(&record.report, parallaxe_core::Locale::ZhCn),
        _ => {
            return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "format 只支持 json/html/csv/md"}})
        }
    };
    json!({"format": format, "content": content})
}

fn handle_guard_plan(
    registry: &Registry,
    session: &mut Session,
    args: &Value,
) -> (Value, Vec<Event>) {
    let Some(source_run_id) = string_arg(args, "source_run_id") else {
        return (
            json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "缺少 source_run_id"}}),
            Vec::new(),
        );
    };
    let targets = string_array(args, "targets");
    if targets.iter().any(|id| registry.get(id).is_none()) {
        return (
            json!({"error":{"code":"INVALID_ARGUMENT","message_safe":"未知目标 ID"}}),
            Vec::new(),
        );
    }
    let Some(source_results) = session
        .runs
        .get(&source_run_id)
        .map(|record| record.report.results.clone())
    else {
        return (
            json!({"error": {"code": "UNKNOWN_RUN", "message_safe": "未知运行 ID"}}),
            Vec::new(),
        );
    };
    let run_id = session.next_run_id("run-guard");
    let now = time::OffsetDateTime::now_utc();
    let source_interrupted = session
        .runs
        .get(&source_run_id)
        .is_some_and(|record| record.report.termination != parallaxe_core::Termination::Complete);
    let preview = guard::plan_preview(
        registry,
        guard::GuardRequest {
            run_id: &run_id,
            source_run_id: Some(&source_run_id),
            targets: &targets,
            results: &source_results,
            recorded_at: parallaxe_core::state::format_time(now),
            as_of: now.date(),
            source_interrupted,
        },
    );
    let plan = preview.plan;
    let mut log = parallaxe_core::events::EventLog::new(&run_id, false, now);
    log.run_started(
        "ONLINE",
        Some("guard-preview"),
        "parallaxe.assets/v1",
        &targets,
    );
    log.guard_plan_ready(&plan);
    let mut report = Report::new(
        parallaxe_core::VERSION,
        parallaxe_core::state::format_time(now),
        &run_id,
        Mode::Online,
    );
    report.plans = vec![plan.clone()];
    report.termination = parallaxe_core::Termination::Complete;
    report.exit_code = reports::aggregate_report(&report);
    log.run_finished(&report);
    let events = log.into_events();
    session.store(run_id.clone(), report, events.clone());
    (json!({"run_id": run_id}), events)
}

fn handle_isolate(args: &Value) -> Value {
    let action = string_arg(args, "action").unwrap_or_else(|| "explain".to_string());
    if action != "plan" && action != "explain" {
        return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "action 只支持 plan/explain"}});
    }
    json!({
        "status": "NO_EXISTING_PATH",
        "paths": [],
        "limitations": [
            "只解释已有路径，不创建、切换或验证新代理",
            "独立路径必须由用户自行提供并证明"
        ],
        "action": action,
        "protect": string_array(args, "protect"),
        "defer": string_array(args, "defer"),
        "selected_by": "USER",
    })
}

fn ai_error(code: &str, message: &str) -> Value {
    json!({"error": {"code": code, "message_safe": message}})
}

fn ai_args_are_strict(args: &Value, allowed: &[&str]) -> bool {
    match args.as_object() {
        Some(object) => object.keys().all(|key| allowed.contains(&key.as_str())),
        None => false,
    }
}

fn parse_rfc3339(text: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()
}

/// 状态文件超过 120 秒未刷新即视为宿主已退出（防止残留“自主已开”的误判）。
fn ai_status_is_live(status: &BridgeStatus) -> bool {
    parse_rfc3339(&status.updated_at).is_some_and(|updated| {
        time::OffsetDateTime::now_utc() - updated < time::Duration::seconds(120)
    })
}

fn ai_qos_doc() -> Value {
    json!({
        "min_interval_ms": MIN_INTERVAL_MS,
        "batch_size_max": MAX_BATCH_SIZE,
        "max_targets": MAX_TARGETS,
        "preferred_transport": "TCP_FIRST",
        "stop_on": ["403", "429"],
    })
}

/// 任务 A：读取 AI 自主调试开关与宿主能力（只读，不发包）。
fn handle_ai_status(session: &Session, args: &Value) -> Value {
    if !ai_args_are_strict(args, &[]) {
        return ai_error("INVALID_ARGUMENT", "ai_status 不接受任何参数");
    }
    let status = session.bridge.read_status().filter(ai_status_is_live);
    let Some(status) = status else {
        return json!({
            "available": false,
            "host": Value::Null,
            "autonomous_enabled": false,
            "autonomous_revoked_by_agent": session.ai_revoked,
            "session_only": true,
            "imported_targets": [],
            "capabilities": [],
            "pending_requests": session.ai_pending.len(),
            "qos": ai_qos_doc(),
            "protocol_version": PROTOCOL_VERSION,
            "notice": "未检测到 GUI 授权桥；默认零发包。请先打开 Parallaxe box（桌面 GUI）再重试。",
        });
    };
    json!({
        "available": true,
        "host": status.host,
        "autonomous_enabled": status.autonomous_enabled,
        "autonomous_revoked_by_agent": session.ai_revoked,
        "session_only": status.session_only,
        "imported_targets": status.imported_targets,
        "capabilities": status.capabilities,
        "pending_requests": session.ai_pending.len(),
        "qos": ai_qos_doc(),
        "protocol_version": PROTOCOL_VERSION,
    })
}

/// 任务 A/B：提交一次性检测请求。agent 自己不发包，由 GUI 在用户确认（A）
/// 或用户已打开自主检测（B）后执行，并把脱敏报告回传。
fn handle_ai_request_check(session: &mut Session, args: &Value) -> Value {
    if !ai_args_are_strict(args, &["profile", "targets", "purpose"]) {
        return ai_error(
            "INVALID_ARGUMENT",
            "ai_request_check 只接受 profile/targets/purpose",
        );
    }
    let Some(status) = session.bridge.read_status().filter(ai_status_is_live) else {
        return ai_error("BRIDGE_UNAVAILABLE", "未检测到 GUI 授权桥");
    };
    let profile = string_arg(args, "profile");
    let mut targets = string_array(args, "targets");
    if let Some(single) = args.get("targets").and_then(Value::as_str) {
        targets = vec![single.to_string()];
    }
    let purpose =
        string_arg(args, "purpose").unwrap_or_else(|| "外部 AI 请求的一次性首跳观测".to_string());
    let scope = match profile {
        Some(profile) => AiScope::for_profile(profile, purpose),
        None => AiScope::for_targets(targets, purpose),
    };
    let request = match BridgeRequest::new(scope, MIN_INTERVAL_MS, 5) {
        Ok(request) => request,
        Err(message) => return ai_error("INVALID_ARGUMENT", &message),
    };
    if let Err(error) = session.bridge.write_request(&request) {
        return ai_error("INTERNAL", &error.to_string());
    }
    let autonomous_mode = status.autonomous_enabled && !session.ai_revoked;
    session
        .ai_pending
        .insert(request.request_id.clone(), request.clone());
    json!({
        "request_id": request.request_id,
        "status": "PENDING",
        "mode": if autonomous_mode { "AUTONOMOUS" } else { "USER_CONFIRMATION" },
        "scope": request.scope.label(),
        "expires_at": request.expires_at,
        "min_interval_ms": request.min_interval_ms,
        "batch_size": request.batch_size,
        "protocol_version": PROTOCOL_VERSION,
        "notice": if autonomous_mode {
            "自主检测已开启：GUI 将在 QoS 红线内自动执行并回传脱敏报告；随后用 ai_request_status 取回。"
        } else {
            "请在 GUI 的「AI 检测一次性授权」弹窗点击确认；未确认前保持零发包。"
        },
    })
}

/// 任务 A/B：等待并取回授权结果；COMPLETED 时把报告装载进本会话，
/// 之后 `explain` / `export_report` 与普通运行完全同构。
async fn handle_ai_request_status(session: &mut Session, args: &Value) -> Value {
    if !ai_args_are_strict(args, &["request_id", "wait_ms"]) {
        return ai_error(
            "INVALID_ARGUMENT",
            "ai_request_status 只接受 request_id/wait_ms",
        );
    }
    let Some(request_id) = string_arg(args, "request_id") else {
        return ai_error("INVALID_ARGUMENT", "缺少 request_id");
    };
    let wait_ms = args
        .get("wait_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(30_000);
    let Some(request) = session.ai_pending.get(&request_id).cloned() else {
        return ai_error(
            "INVALID_ARGUMENT",
            "request_id 不在本会话内（请先调用 ai_request_check）",
        );
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
    loop {
        if let Some(result) = session.bridge.read_result(&request_id) {
            return ai_result_data(session, &request, result);
        }
        if let Some(decision) = session.bridge.read_decision(&request_id) {
            if decision.nonce != request.nonce || decision.scope_digest != request.scope_digest() {
                return ai_error("POLICY_DENIED", "授权决定与请求范围不匹配");
            }
            match decision.decision {
                DecisionKind::Denied => {
                    return json!({
                        "request_id": request_id,
                        "status": "DENIED",
                        "autonomous": false,
                        "network_request_count": 0,
                        "protocol_version": PROTOCOL_VERSION,
                    })
                }
                DecisionKind::Approved => {
                    let expired = parse_rfc3339(&decision.expires_at)
                        .is_some_and(|end| time::OffsetDateTime::now_utc() >= end);
                    if expired {
                        return json!({
                            "request_id": request_id,
                            "status": "EXPIRED",
                            "autonomous": decision.autonomous,
                            "notice": "授权已过期，未执行任何发包；请重新提交请求。",
                            "protocol_version": PROTOCOL_VERSION,
                        });
                    }
                    if std::time::Instant::now() >= deadline {
                        return json!({
                            "request_id": request_id,
                            "status": "APPROVED",
                            "autonomous": decision.autonomous,
                            "network_request_count": 0,
                            "notice": "用户已授权，GUI 正在执行；稍后再次查询即可取回报告。",
                            "protocol_version": PROTOCOL_VERSION,
                        });
                    }
                }
            }
        } else if parse_rfc3339(&request.expires_at)
            .is_some_and(|end| time::OffsetDateTime::now_utc() >= end)
        {
            return json!({
                "request_id": request_id,
                "status": "EXPIRED",
                "autonomous": false,
                "network_request_count": 0,
                "notice": "用户在有效期内未确认，已按零发包过期。",
                "protocol_version": PROTOCOL_VERSION,
            });
        }
        if std::time::Instant::now() >= deadline {
            return json!({
                "request_id": request_id,
                "status": "PENDING",
                "autonomous": false,
                "network_request_count": 0,
                "notice": "仍在等待用户确认；未确认前保持零发包。",
                "protocol_version": PROTOCOL_VERSION,
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

fn ai_result_data(session: &mut Session, request: &BridgeRequest, result: BridgeResult) -> Value {
    match result.status {
        ResultStatus::Completed => {
            let Some(report) = result.report else {
                return ai_error("INTERNAL", "结果缺少报告");
            };
            let run_id = report.run_id.clone();
            session.store(run_id.clone(), report, Vec::new());
            json!({
                "request_id": request.request_id,
                "status": "COMPLETED",
                "run_id": run_id,
                "origin": result.origin,
                "autonomous": result.autonomous,
                "network_request_count": result.network_request_count,
                "explain_ready": true,
                "export_ready": true,
                "protocol_version": PROTOCOL_VERSION,
            })
        }
        ResultStatus::Denied => json!({
            "request_id": request.request_id,
            "status": "DENIED",
            "autonomous": false,
            "network_request_count": 0,
            "reason": result.denied_reason,
            "protocol_version": PROTOCOL_VERSION,
        }),
        ResultStatus::Failed => json!({
            "request_id": request.request_id,
            "status": "FAILED",
            "autonomous": result.autonomous,
            "network_request_count": 0,
            "error": result.error,
            "protocol_version": PROTOCOL_VERSION,
        }),
    }
}

/// 任务 B：agent 只能**关闭**自主检测（打开必须由用户在 GUI 设置里完成）。
fn handle_ai_autonomous(session: &mut Session, args: &Value) -> Value {
    if !ai_args_are_strict(args, &["enabled", "reason"]) {
        return ai_error("INVALID_ARGUMENT", "ai_autonomous 只接受 enabled/reason");
    }
    let Some(enabled) = args.get("enabled").and_then(Value::as_bool) else {
        return ai_error("INVALID_ARGUMENT", "缺少布尔 enabled");
    };
    if enabled {
        return ai_error("POLICY_DENIED", "自主检测只能由用户在 GUI 设置中打开");
    }
    let reason = string_arg(args, "reason");
    let control = BridgeControl::disable_autonomous(reason.clone());
    if let Err(error) = session.bridge.write_control(&control) {
        return ai_error("INTERNAL", &error.to_string());
    }
    // 即时收回：最多等待 2 秒确认 GUI 已消费控制指令并把状态翻回关闭。
    let mut confirmed = false;
    for _ in 0..20 {
        let off = session
            .bridge
            .read_status()
            .filter(ai_status_is_live)
            .is_some_and(|status| !status.autonomous_enabled);
        if off {
            confirmed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    session.ai_revoked = true;
    json!({
        "status": "REVOKED",
        "autonomous": false,
        "confirmed_by_host": confirmed,
        "revoked_by": "AGENT",
        "session_only": true,
        "reason": reason,
        "protocol_version": PROTOCOL_VERSION,
        "notice": if confirmed {
            "GUI 已确认关闭自主检测；后续请求必须重新由用户确认。"
        } else {
            "关闭指令已提交；GUI 下次轮询即收回权限，后续请求不会再按自主模式提交。"
        },
    })
}

/// 运行 NDJSON 会话，返回进程退出码。
pub async fn run_stdio(registry: Registry, locale: Locale) -> u8 {
    AGENT_LOCALE.store(
        match locale {
            Locale::ZhCn => 0,
            Locale::EnUs => 1,
        },
        Ordering::Relaxed,
    );
    match serve(
        registry,
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await
    {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

async fn serve<R: tokio::io::AsyncBufRead + Unpin, W: AsyncWriteExt + Unpin>(
    registry: Registry,
    input: R,
    mut stdout: W,
) -> std::io::Result<()> {
    let mut reader = input.lines();
    let mut session = Session::default();

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Result<Value, _> = serde_json::from_str(&line);
        let value = match parsed {
            Ok(value) => value,
            Err(_) => {
                let response = error_response(None, "INVALID_ARGUMENT", "无法解析的 JSON 行");
                write_line(&mut stdout, &response).await?;
                continue;
            }
        };
        let id = value
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::to_string);
        let Some(id) = id else {
            let response = error_response(None, "INVALID_ARGUMENT", "缺少字符串 id");
            write_line(&mut stdout, &response).await?;
            continue;
        };
        if value.get("schema").and_then(|schema| schema.as_str())
            != Some(parallaxe_core::contracts::AGENT_SCHEMA)
        {
            let response =
                error_response(Some(&id), "UNSUPPORTED_SCHEMA", "只接受 parallaxe.agent/v1");
            write_line(&mut stdout, &response).await?;
            continue;
        }
        let Some(op) = value.get("op").and_then(|op| op.as_str()) else {
            let response = error_response(Some(&id), "INVALID_ARGUMENT", "缺少 op");
            write_line(&mut stdout, &response).await?;
            continue;
        };
        let args = value.get("args").cloned().unwrap_or_else(|| json!({}));

        match op {
            "list_targets" => {
                let data = handle_list_targets(&registry, &args);
                emit_data(&mut stdout, &id, data).await?;
            }
            "load_profile" => {
                let data = handle_load_profile(&registry, &args);
                let ok = data.get("error").is_none();
                let response = if ok {
                    ok_response(&id, data)
                } else {
                    error_response(
                        Some(&id),
                        data["error"]["code"].as_str().unwrap_or("INVALID_ARGUMENT"),
                        data["error"]["message_safe"]
                            .as_str()
                            .unwrap_or("请求被拒绝"),
                    )
                };
                write_line(&mut stdout, &response).await?;
            }
            "check" => {
                if session.active.is_some() {
                    write_line(
                        &mut stdout,
                        &error_response(Some(&id), "BUSY", "已有活动运行"),
                    )
                    .await?;
                    continue;
                }
                let mut options = match check_options(&args) {
                    Ok(options) => options,
                    Err(code) => {
                        let message = if code == "POLICY_DENIED" {
                            "agent 单条请求不能追加 state_dir/proxy/interface"
                        } else {
                            "参数类型、范围或选择器组合不合法"
                        };
                        write_line(&mut stdout, &error_response(Some(&id), code, message)).await?;
                        continue;
                    }
                };
                options.run_id = Some(session.next_run_id("run-check"));
                let run_id = options.run_id.clone().unwrap_or_default();
                session.active = Some(run_id.clone());
                let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
                let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
                let check = runtime::run_check_streaming(
                    &registry,
                    &options,
                    None,
                    false,
                    None,
                    cancel_rx,
                    Some(event_tx),
                );
                tokio::pin!(check);
                let mut ack_sent = false;
                let outcome = loop {
                    tokio::select! {
                        result = &mut check => break result,
                        Some(event) = event_rx.recv() => {
                            // 首个事件到达即回执 run_id，客户端随后可在运行中发送 cancel。
                            if !ack_sent {
                                write_line(
                                    &mut stdout,
                                    &ok_response(&id, json!({"run_id": run_id.clone()})),
                                )
                                .await?;
                                ack_sent = true;
                            }
                            write_line(&mut stdout, &event_notification(&id, &event)).await?;
                        }
                        line = reader.next_line() => {
                            let Ok(Some(line)) = line else {
                                // stdin 断开：请求取消，让运行尽快收束。
                                let _ = cancel_tx.send(true);
                                continue;
                            };
                            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                                continue;
                            };
                            let canceling = value.get("schema").and_then(Value::as_str)
                                == Some(parallaxe_core::contracts::AGENT_SCHEMA)
                                && value.get("op").and_then(Value::as_str) == Some("cancel")
                                && value
                                    .get("args")
                                    .and_then(|args| args.get("run_id"))
                                    .and_then(Value::as_str)
                                    == Some(run_id.as_str());
                            let request_id = value
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("null")
                                .to_string();
                            if canceling {
                                let _ = cancel_tx.send(true);
                                write_line(
                                    &mut stdout,
                                    &ok_response(&request_id, json!({"status": "REQUESTED"})),
                                )
                                .await?;
                            } else {
                                write_line(
                                    &mut stdout,
                                    &error_response(Some(&request_id), "BUSY", "已有活动运行"),
                                )
                                .await?;
                            }
                        }
                    }
                };
                session.active = None;
                match outcome {
                    Ok(output) => {
                        if !ack_sent {
                            write_line(
                                &mut stdout,
                                &ok_response(&id, json!({"run_id": run_id.clone()})),
                            )
                            .await?;
                        }
                        // 运行结束后把通道里剩余的事件排空，避免丢事件。
                        while let Ok(event) = event_rx.try_recv() {
                            write_line(&mut stdout, &event_notification(&id, &event)).await?;
                        }
                        let actual_run_id = output.report.run_id.clone();
                        session.store(actual_run_id, output.report, output.events);
                    }
                    Err(error) => {
                        if ack_sent {
                            // 已回执后才失败：追加作用域错误回执（含同一 id）。
                            write_line(
                                &mut stdout,
                                &error_response(Some(&id), "INTERNAL", &safe_error(&error)),
                            )
                            .await?;
                        } else {
                            write_line(
                                &mut stdout,
                                &error_response(
                                    Some(&id),
                                    match error {
                                        ConfigError::Field { ref location, .. }
                                            if location != "check" =>
                                        {
                                            "INVALID_ARGUMENT"
                                        }
                                        _ => "INTERNAL",
                                    },
                                    &safe_error(&error),
                                ),
                            )
                            .await?;
                        }
                    }
                }
            }
            "explain" => {
                let data = handle_explain(&session, &args);
                emit_data(&mut stdout, &id, data).await?;
            }
            "selftest" => {
                match selftest::run(
                    &registry,
                    string_arg(&args, "suite").as_deref(),
                    Some(session.next_run_id("run-selftest")),
                )
                .await
                {
                    Ok(outcome) => {
                        let run_id = outcome.report.run_id.clone();
                        let passed = outcome.passed;
                        let failed = outcome.failed;
                        session.store(run_id.clone(), outcome.report, outcome.events.clone());
                        write_line(
                            &mut stdout,
                            &ok_response(
                                &id,
                                json!({"run_id": run_id, "passed": passed, "failed": failed}),
                            ),
                        )
                        .await?;
                        for event in &outcome.events {
                            write_line(&mut stdout, &event_notification(&id, event)).await?;
                        }
                    }
                    Err(error) => {
                        write_line(
                            &mut stdout,
                            &error_response(Some(&id), "INVALID_ARGUMENT", &safe_error(&error)),
                        )
                        .await?;
                    }
                }
            }
            "guard_plan" => {
                let (data, events) = handle_guard_plan(&registry, &mut session, &args);
                if data.get("error").is_some() {
                    write_line(
                        &mut stdout,
                        &error_response(
                            Some(&id),
                            data["error"]["code"].as_str().unwrap_or("INVALID_ARGUMENT"),
                            data["error"]["message_safe"]
                                .as_str()
                                .unwrap_or("请求被拒绝"),
                        ),
                    )
                    .await?;
                } else {
                    write_line(&mut stdout, &ok_response(&id, data)).await?;
                    for event in &events {
                        write_line(&mut stdout, &event_notification(&id, event)).await?;
                    }
                }
            }
            "export_report" => {
                let data = handle_export(&session, &args);
                emit_data(&mut stdout, &id, data).await?;
            }
            "cancel" => {
                let Some(run_id) = string_arg(&args, "run_id") else {
                    write_line(
                        &mut stdout,
                        &error_response(Some(&id), "INVALID_ARGUMENT", "缺少 run_id"),
                    )
                    .await?;
                    continue;
                };
                if session.active.as_deref() == Some(run_id.as_str()) {
                    session.active = None;
                    write_line(
                        &mut stdout,
                        &ok_response(&id, json!({"status": "REQUESTED"})),
                    )
                    .await?;
                } else if session.runs.contains_key(&run_id) {
                    write_line(
                        &mut stdout,
                        &ok_response(&id, json!({"status": "ALREADY_TERMINAL"})),
                    )
                    .await?;
                } else {
                    write_line(
                        &mut stdout,
                        &error_response(Some(&id), "UNKNOWN_RUN", "未知运行 ID"),
                    )
                    .await?;
                }
            }
            "isolate" => {
                let data = handle_isolate(&args);
                emit_data(&mut stdout, &id, data).await?;
            }
            "ai_status" => {
                let data = handle_ai_status(&session, &args);
                emit_data(&mut stdout, &id, data).await?;
            }
            "ai_request_check" => {
                let data = handle_ai_request_check(&mut session, &args);
                emit_data(&mut stdout, &id, data).await?;
            }
            "ai_request_status" => {
                let data = handle_ai_request_status(&mut session, &args).await;
                emit_data(&mut stdout, &id, data).await?;
            }
            "ai_autonomous" => {
                let data = handle_ai_autonomous(&mut session, &args);
                emit_data(&mut stdout, &id, data).await?;
            }
            _ => {
                write_line(
                    &mut stdout,
                    &error_response(Some(&id), "INVALID_ARGUMENT", "未知动作"),
                )
                .await?;
            }
        }
    }
    // stdin 断开：不启动后台服务。
    stdout.flush().await
}

async fn emit_data<W: AsyncWriteExt + Unpin>(
    stdout: &mut W,
    id: &str,
    data: Value,
) -> std::io::Result<()> {
    if data.get("error").is_some() {
        let response = error_response(
            Some(id),
            data["error"]["code"].as_str().unwrap_or("INVALID_ARGUMENT"),
            data["error"]["message_safe"]
                .as_str()
                .unwrap_or("请求被拒绝"),
        );
        write_line(stdout, &response).await
    } else {
        write_line(stdout, &ok_response(id, data)).await
    }
}

async fn write_line<W: AsyncWriteExt + Unpin>(
    stdout: &mut W,
    value: &Value,
) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value)?;
    text.push('\n');
    stdout.write_all(text.as_bytes()).await?;
    stdout.flush().await
}

fn safe_error(error: &ConfigError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn broken_output_stops_reader_without_waiting_for_eof() {
        let (mut peer, input) = tokio::io::duplex(64);
        let (output, dead) = tokio::io::duplex(64);
        drop(dead);
        peer.write_all(b"{}\n").await.unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            serve(Registry::embedded().unwrap(), BufReader::new(input), output),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
    }
}
