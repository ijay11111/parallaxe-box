//! `parallaxe agent --stdio`：一行一个 UTF-8 JSON 对象，stdout 只出协议对象。
//!
//! 九个操作全部投影共享 core；无 shell、无任意 URL/代理控制、无 apply。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use parallaxe_core::contracts::{Event, Mode, Report};
use parallaxe_core::guard;
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::reports;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::selftest;
use parallaxe_core::targets::{ConfigError, Registry};

const OPERATIONS: [&str; 9] = [
    "list_targets",
    "load_profile",
    "check",
    "explain",
    "selftest",
    "guard_plan",
    "export_report",
    "cancel",
    "isolate",
];

struct RunRecord {
    report: Report,
    /// 已完成运行的事件快照；阶段 2 的 export 只用报告，保留以便 GUI/重放。
    #[allow(dead_code)]
    events: Vec<Event>,
}

#[derive(Default)]
struct Session {
    runs: HashMap<String, RunRecord>,
    /// 活动运行（本阶段运行同步完成，保留字段以表达 max_active_runs=1 的语义）。
    active: Option<String>,
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
        "format 只支持 json/html/csv" => "format only supports json/html/csv",
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
        "protocol_version": 1,
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
            "full" | "ipv4" | "fast" => value.is_boolean(),
            "timeout_ms" | "max_concurrency" => value.is_u64(),
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
        _ => {
            return json!({"error": {"code": "INVALID_ARGUMENT", "message_safe": "format 只支持 json/html/csv"}})
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
