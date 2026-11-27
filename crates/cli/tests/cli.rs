//! CLI 端到端：只读命令、默认空许可、离线 selftest、输出格式与退出码。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::Value;

fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_parallaxe")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(exe())
        .args(args)
        .output()
        .expect("运行 parallaxe");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn build_info_reports_compile_time_distribution_marker() {
    let (code, stdout, stderr) = run(&["build-info", "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let value: Value = serde_json::from_str(&stdout).expect("纯 JSON");
    assert_eq!(value["schema"], "parallaxe.build-info/v1");
    assert_eq!(
        value["distribution_release_build"],
        cfg!(feature = "distribution-release")
    );
    assert_eq!(
        value["marker"],
        if cfg!(feature = "distribution-release") {
            "parallaxe.distribution-release/v1"
        } else {
            "parallaxe.source-build/v1"
        }
    );
}

#[test]
fn targets_list_is_read_only_and_lists_107_google_targets() {
    let (code, stdout, stderr) = run(&["targets", "list", "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let value: Value = serde_json::from_str(&stdout).expect("纯 JSON");
    assert_eq!(value["schema"], "parallaxe.targets/v1");
    assert_eq!(value["catalog_size"], 135);
    assert_eq!(value["selected_count"], 107);
    assert_eq!(
        value["targets"].as_array().map(|items| items.len()),
        Some(107)
    );
    assert!(value["targets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["allow_online"] == false));
}

#[test]
fn empty_admission_check_selects_107_sends_nothing_and_exits_3() {
    let (code, stdout, stderr) = run(&["check", "--headless", "--profile", "google-all", "--json"]);
    assert_eq!(code, 3, "stderr={stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("stdout 只有 JSON");
    assert_eq!(report["schema"], "parallaxe.report/v1");
    assert_eq!(report["coverage"]["selected_count"], 107);
    assert_eq!(report["network_request_count"], 0);
    assert_eq!(
        report["results"].as_array().map(|items| items.len()),
        Some(0)
    );
    assert_eq!(report["exit_code"], 3);
    assert_eq!(report["termination"], "COMPLETE");
    let uncovered = report["coverage"]["uncovered"].as_array().unwrap();
    assert_eq!(uncovered.len(), 135);
    assert!(uncovered
        .iter()
        .filter(|item| item["selected"] == true)
        .all(|item| item["reason"] != "NOT_SELECTED"));
}

#[test]
fn simple_profile_selects_16_and_also_exits_3() {
    let (code, stdout, _) = run(&["check", "--headless", "--profile", "simple", "--json"]);
    assert_eq!(code, 3);
    let report: Value = serde_json::from_str(&stdout).unwrap();
    // simple 现含 google.one，共 16 项；源码构建默认零发包。
    assert_eq!(report["coverage"]["selected_count"], 16);
    assert_eq!(report["network_request_count"], 0);
}

#[test]
fn selftest_offline_passes() {
    let (code, stdout, stderr) = run(&["selftest", "--offline", "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema"], "parallaxe.selftest/v1");
    assert_eq!(value["failed"], 0);
    assert!(value["passed"].as_u64().unwrap_or(0) >= 20);
    let names: Vec<&str> = value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["name"].as_str().unwrap_or_default())
        .collect();
    assert!(names.contains(&"rules:legacy-selector-negative"));
    assert!(names.contains(&"exit:blocked-unknown-conflict"));
    assert!(names.contains(&"stale:default-off"));
    assert!(names.contains(&"admission:empty-grants"));
}

#[test]
fn fixture_mode_is_offline_and_reports_country() {
    let (code, stdout, _) = run(&[
        "check",
        "--headless",
        "--fixture",
        "gemini-positive",
        "--json",
    ]);
    assert_eq!(code, 0);
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["mode"], "FIXTURE");
    assert_eq!(report["network_request_count"], 0);
    assert_eq!(report["simulated_request_count"], 1);
    assert_eq!(report["results"][0]["state"], "OK");
    assert_eq!(
        report["results"][0]["service_observed"]["observed_country"],
        "US"
    );
    assert_eq!(report["results"][0]["evidence"][0]["fixture"], true);
}

#[test]
fn expectation_unsettled_is_uncertain() {
    let (code, stdout, _) = run(&[
        "check",
        "--headless",
        "--fixture",
        "news-edition",
        "--expect",
        "service_country=US",
        "--json",
    ]);
    assert_eq!(code, 3, "版次不能冒充出口国家");
    let report: Value = serde_json::from_str(&stdout).unwrap();
    let expectation = &report["results"][0]["expectation"];
    assert_eq!(expectation["status"], "NOT_OBSERVABLE");
}

#[test]
fn known_expectation_difference_is_only_a_hint() {
    let (code, stdout, _) = run(&[
        "check",
        "--headless",
        "--fixture",
        "gemini-positive",
        "--expect",
        "service_country=CN",
        "--json",
    ]);
    assert_eq!(code, 0, "已知不同只作提示，不单独升级退出码");
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["results"][0]["expectation"]["status"], "MISMATCH");
}

#[test]
fn selftest_shortcut_and_help_exit_zero() {
    let (code, stdout, _) = run(&["--selftest"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("离线自检"));
    let (code, stdout, _) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("parallaxe"));
    // 无参数：GUI 构建默认进入图形界面（双击 exe 的体验）；headless 构建仍报用法错误。
    // GUI 在无显示环境可能立即退出，但退出码应为 0（成功进入启动路径）。
    let (code, _, _) = run(&[]);
    if cfg!(feature = "gui") {
        assert_eq!(code, 0, "GUI 构建无参数应默认进入 GUI");
    } else {
        assert_eq!(code, 1, "headless 构建缺少子命令属于用法错误");
    }
}

#[test]
fn usage_errors_exit_1() {
    let (code, _, stderr) = run(&["check", "--headless", "--profile", "nope", "--json"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("错误"));
    let (code, _, _) = run(&[
        "check",
        "--headless",
        "--profile",
        "simple",
        "--json",
        "--csv",
    ]);
    assert_eq!(code, 1);
    let (code, _, _) = run(&["check", "--headless", "--target", "nope.target", "--json"]);
    assert_eq!(code, 1);
    let (code, _, stderr) = run(&["explain", "--target", "nope.target", "--json"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("错误"));
}

#[test]
fn explain_dependencies_priorities_and_isolate_are_read_only() {
    let (code, stdout, _) = run(&["explain", "--target", "google.gemini", "--json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["target"]["id"], "google.gemini");
    assert_eq!(
        value["target"]["admission_status"],
        "DENIED_ALLOW_ONLINE_FALSE"
    );

    let (code, stdout, _) = run(&[
        "dependencies",
        "explain",
        "--target",
        "google.play",
        "--json",
    ]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert!(value["edges"].is_array());

    let (code, stdout, _) = run(&["priorities", "list", "--json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        value["services"].as_array().map(|items| items.len()),
        Some(0)
    );

    let (code, stdout, _) = run(&["isolate", "plan", "--protect", "google.gemini", "--json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["status"], "NO_EXISTING_PATH");
    assert_eq!(value["paths"].as_array().map(|items| items.len()), Some(0));
}

#[test]
fn guard_preview_is_never_applicable() {
    let (code, stdout, _) = run(&[
        "guard",
        "plan",
        "--dry-run",
        "--target",
        "google.gemini",
        "--json",
    ]);
    assert_eq!(code, 3);
    let plan: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(plan["safe_to_apply"], false);
    assert_eq!(plan["closure_complete"], false);
    assert_eq!(plan["applied"], false);
    assert_eq!(plan["operation"], "PREVIEW");
    assert_eq!(plan["effect_scope"], "LOCAL_TRAFFIC_ONLY");
}

#[test]
fn jsonl_and_html_and_csv_formats_are_single_purpose() {
    let (code, stdout, _) = run(&[
        "check",
        "--headless",
        "--fixture",
        "gemini-positive",
        "--jsonl",
    ]);
    assert_eq!(code, 0);
    let mut count = 0;
    for line in stdout.lines() {
        let value: Value = serde_json::from_str(line).expect("每行一个 JSON");
        assert_eq!(value["schema"], "parallaxe.event/v1");
        count += 1;
    }
    assert!(count >= 5);
    assert!(!stdout.contains('\u{1b}'), "结构化 stdout 不得含 ANSI");

    let (code, stdout, _) = run(&[
        "check",
        "--headless",
        "--fixture",
        "gemini-positive",
        "--html",
    ]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("<!DOCTYPE html>"));
    assert!(!stdout.contains("fonts.googleapis.com"));
    assert!(!stdout.contains("cdn.jsdelivr.net"));

    let (code, stdout, _) = run(&[
        "check",
        "--headless",
        "--fixture",
        "gemini-positive",
        "--csv",
    ]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("target,state,reason"));
}

struct Agent {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl Agent {
    fn start() -> Self {
        Self::start_with(&[])
    }

    fn start_with(prefix: &[&str]) -> Self {
        let mut command = Command::new(exe());
        command.args(prefix).args(["agent", "--stdio"]);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("启动 agent");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        Agent {
            child,
            stdin,
            reader: BufReader::new(stdout),
        }
    }

    fn request(
        &mut self,
        id: &str,
        op: &str,
        args: Value,
        notifications: usize,
    ) -> (Value, Vec<Value>) {
        let line = serde_json::json!({
            "schema": "parallaxe.agent/v1",
            "id": id,
            "op": op,
            "args": args,
        });
        writeln!(self.stdin, "{line}").expect("写请求");
        self.stdin.flush().expect("flush");
        self.read_response(id, notifications)
    }

    fn raw(&mut self, line: &str) -> Value {
        writeln!(self.stdin, "{line}").expect("写坏行");
        self.stdin.flush().expect("flush");
        let mut text = String::new();
        self.reader.read_line(&mut text).expect("读响应");
        serde_json::from_str(text.trim()).expect("错误回包必须是 JSON")
    }

    fn read_response(&mut self, id: &str, notifications: usize) -> (Value, Vec<Value>) {
        let mut response = None;
        let mut events = Vec::new();
        let mut read_lines = 0;
        while response.is_none() || events.len() < notifications {
            let mut line = String::new();
            let read = self.reader.read_line(&mut line).expect("读响应");
            assert!(read > 0, "agent 提前退出");
            read_lines += 1;
            assert!(read_lines < 200, "响应行数异常");
            let value: Value = serde_json::from_str(line.trim()).expect("NDJSON 每行合法");
            if let Some(record) = value.get("record") {
                assert_eq!(record["schema"], "parallaxe.event/v1", "事件必须带 schema");
                events.push(record.clone());
            } else {
                assert_eq!(value["id"], id);
                response = Some(value);
            }
        }
        (response.unwrap(), events)
    }

    fn finish(self) -> (i32, String) {
        drop(self.stdin);
        let output = self.child.wait_with_output().expect("等待 agent");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}

#[test]
fn agent_supports_protocol_operations_over_ndjson() {
    let mut agent = Agent::start();
    let bad = agent.raw("{not json");
    assert_eq!(bad["id"], Value::Null, "坏行必须用 id:null 回包");
    assert_eq!(bad["error"]["code"], "INVALID_ARGUMENT");
    let bad = agent.raw(r#"{"schema":"other/v1","id":"s1","op":"list_targets","args":{}}"#);
    assert_eq!(bad["id"], "s1");
    assert_eq!(bad["error"]["code"], "UNSUPPORTED_SCHEMA");
    let (response, events) = agent.request("1", "list_targets", serde_json::json!({}), 0);
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["protocol_version"], 2);
    assert_eq!(response["data"]["max_active_runs"], 1);
    assert_eq!(
        response["data"]["operations"]
            .as_array()
            .map(|items| items.len()),
        Some(13)
    );
    assert!(events.is_empty());

    let (response, _) = agent.request(
        "2",
        "load_profile",
        serde_json::json!({"profile": "simple"}),
        0,
    );
    assert_eq!(response["ok"], true);
    assert_eq!(
        response["data"]["selected_ids"]
            .as_array()
            .map(|items| items.len()),
        Some(16)
    );

    let (response, events) = agent.request(
        "3",
        "check",
        serde_json::json!({
            "profile": "google-all",
            "min_interval_ms": 4000,
            "batch_size": 5,
            "batch_cooldown_ms": 1000,
            "node_retest": 0,
            "plan": false,
            "confirm_budget": false,
            "node_sweep": false
        }),
        4,
    );
    assert_eq!(response["ok"], true);
    let run_id = response["data"]["run_id"].as_str().unwrap().to_string();
    let names: Vec<&str> = events
        .iter()
        .map(|event| event["event"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        vec![
            "RunStarted",
            "PreflightReady",
            "CoverageUpdated",
            "RunFinished"
        ]
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], index as u64 + 1, "事件序号必须连续");
        assert_eq!(event["run_id"], run_id);
    }
    assert_eq!(events[3]["payload"]["report"]["exit_code"], 3);
    assert_eq!(events[3]["payload"]["report"]["network_request_count"], 0);

    let (response, _) = agent.request(
        "4",
        "explain",
        serde_json::json!({"run_id": run_id, "target": "google.gemini"}),
        0,
    );
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["uncovered"]["selected"], true);

    let (response, _) = agent.request(
        "5",
        "export_report",
        serde_json::json!({"run_id": run_id, "format": "json"}),
        0,
    );
    assert_eq!(response["ok"], true);
    let content: Value = serde_json::from_str(response["data"]["content"].as_str().unwrap())
        .expect("导出 JSON 可解析");
    assert_eq!(content["schema"], "parallaxe.report/v1");

    let (response, events) = agent.request(
        "6",
        "guard_plan",
        serde_json::json!({"source_run_id": run_id, "targets": ["google.gemini"]}),
        3,
    );
    assert_eq!(response["ok"], true);
    let names: Vec<&str> = events
        .iter()
        .map(|event| event["event"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(names, vec!["RunStarted", "GuardPlanReady", "RunFinished"]);
    assert_eq!(events[2]["payload"]["report"]["exit_code"], 3);

    let (response, _) = agent.request("7", "selftest", serde_json::json!({}), 8);
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["failed"], 0);

    let (response, _) = agent.request("8", "cancel", serde_json::json!({"run_id": run_id}), 0);
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["status"], "ALREADY_TERMINAL");

    let (response, _) = agent.request(
        "9",
        "isolate",
        serde_json::json!({"action": "plan", "protect": ["google.gemini"], "defer": []}),
        0,
    );
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["status"], "NO_EXISTING_PATH");

    let (response, _) = agent.request("10", "nope", serde_json::json!({}), 0);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "INVALID_ARGUMENT");

    let (response, _) = agent.request(
        "11",
        "check",
        serde_json::json!({"profile": "google-all", "proxy": "http://127.0.0.1:7897"}),
        0,
    );
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "POLICY_DENIED");

    let (code, stderr) = agent.finish();
    assert_eq!(code, 0);
    assert!(stderr.is_empty(), "stdout 之外不应有日志：{stderr}");
}

#[test]
fn selectors_and_agent_errors_fail_closed() {
    assert_eq!(
        run(&["check", "--full", "--target", "unknown", "--json"]).0,
        1
    );
    let (code, text, _) = run(&["check", "--headless", "--json"]);
    assert_eq!(code, 3);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["coverage"]["selected_count"],
        16
    );
    let mut agent = Agent::start();
    for (op, args) in [
        (
            "check",
            serde_json::json!({"full":true,"max_concurrency":4294967297u64}),
        ),
        ("check", serde_json::json!({"full":"true"})),
        ("list_targets", serde_json::json!({"group":"unknown"})),
    ] {
        assert_eq!(
            agent.request("e", op, args, 0).0["error"]["code"],
            "INVALID_ARGUMENT"
        );
    }
    assert_eq!(agent.finish().0, 0);
}

#[test]
fn selftests_do_not_overwrite_run_ids_or_diverge_from_events() {
    let mut agent = Agent::start();
    let mut ids = Vec::new();
    for id in ["first", "second"] {
        let (response, events) = agent.request(id, "selftest", serde_json::json!({}), 8);
        let run_id = response["data"]["run_id"].as_str().unwrap().to_owned();
        assert!(events.iter().all(|e| e["run_id"] == run_id));
        let terminal = &events.last().unwrap()["payload"]["report"];
        let (export, _) = agent.request(
            "get",
            "export_report",
            serde_json::json!({"run_id":run_id,"format":"json"}),
            0,
        );
        assert_eq!(
            serde_json::from_str::<Value>(export["data"]["content"].as_str().unwrap()).unwrap(),
            *terminal
        );
        ids.push(run_id);
    }
    assert_ne!(ids[0], ids[1]);
    assert_eq!(agent.finish().0, 0);
}

#[test]
fn default_binary_creates_no_files_in_clean_working_directory() {
    let dir = std::env::temp_dir().join(format!(
        "parallaxe-empty-cwd-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let out = Command::new(exe())
        .args(["check", "--headless", "--profile", "google-all", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn link_explain_is_read_only_and_marks_untested_capabilities() {
    let (code, stdout, stderr) = run(&["link", "explain", "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema"], "parallaxe.link/v1");
    assert_eq!(value["capabilities"]["quic"], "UNTESTED");
    assert_eq!(value["capabilities"]["dns_leak"], "UNTESTED");
    assert_eq!(value["capabilities"]["port_scan"], "PROHIBITED");
    assert_eq!(value["dns_query_count"], 0);
    assert_eq!(value["http_request_count"], 0);
    assert_eq!(value["rows"].as_array().map(|items| items.len()), Some(16));
    assert!(value["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["connection_edge"]["status"] == "NOT_OBSERVED"));
}

#[test]
fn link_explain_projects_fixture_without_sending_packets() {
    let (code, stdout, _) = run(&["link", "explain", "--fixture", "drive-login", "--json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["source"], "FIXTURE");
    assert_eq!(value["http_request_count"], 0);
    let rows = value["rows"].as_array().unwrap();
    let row = rows
        .iter()
        .find(|row| row["target"] == "google.drive")
        .unwrap();
    assert_eq!(row["connection_edge"]["status"], "OBSERVED");
    assert_eq!(row["address_family"]["v4"], "OBSERVED_OK");
    assert_eq!(row["resolver"]["status"], "NOT_OBSERVED");
}

#[test]
fn guard_audit_and_rollback_preview_never_apply() {
    let (code, stdout, _) = run(&[
        "guard",
        "plan",
        "--dry-run",
        "--target",
        "google.gemini",
        "--json",
    ]);
    assert_eq!(code, 3);
    let plan: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(plan["audit"]["schema"], "parallaxe.guard-audit/v1");
    assert_eq!(plan["audit"]["after"]["rules_written"], 0);
    assert_eq!(plan["audit"]["applied"], false);
    assert_eq!(plan["audit"]["result"], "PREVIEW_REJECTED");
    assert_eq!(plan["audit"]["rollback"]["status"], "NOT_REQUIRED");
    assert!(!stdout.contains("REJECT,Proxy"));

    let (code, stdout, _) = run(&["guard", "rollback", "--json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["status"], "NOTHING_APPLIED");
    assert_eq!(value["writes_performed"], 0);
    assert_eq!(value["applied"], false);
}

#[test]
fn guard_source_report_rejects_unreadable_or_invalid_files() {
    let bad = std::env::temp_dir().join(format!(
        "parallaxe-source-report-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&bad, "{}").unwrap();
    let (code, _, stderr) = run(&[
        "guard",
        "plan",
        "--dry-run",
        "--target",
        "google.gemini",
        "--source-report",
        bad.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("错误"));
    std::fs::remove_file(&bad).unwrap();
}

#[test]
fn human_output_is_localized_but_json_contract_is_not() {
    let (code, stdout, stderr) = run(&[
        "--locale",
        "en-US",
        "check",
        "--headless",
        "--fixture",
        "gemini-positive",
    ]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(
        stdout.contains("Target") && stdout.contains("Scope boundary"),
        "{stdout}"
    );
}

#[test]
fn agent_localized_error_keeps_machine_message_safe() {
    let mut agent = Agent::start_with(&["--locale", "en-US"]);
    let bad = agent.raw("{not json");
    assert_eq!(bad["ok"], false);
    assert_eq!(bad["error"]["code"], "INVALID_ARGUMENT");
    // 机器可读字段保持原样，只有新增的 message_localized 随 locale 变化。
    assert_eq!(bad["error"]["message_safe"], "无法解析的 JSON 行");
    assert_eq!(bad["error"]["message_localized"], "Unparsable JSON line");
    let (code, stderr) = agent.finish();
    assert_eq!(code, 0, "stderr={stderr}");
}

fn strip_volatile(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in [
                "generated_at",
                "run_id",
                "wall_duration_ms",
                "duration_ms",
                "observed_at",
                "last_hit_at",
            ] {
                map.remove(key);
            }
            for child in map.values_mut() {
                strip_volatile(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                strip_volatile(child);
            }
        }
        _ => {}
    }
}

#[test]
fn json_machine_fields_are_identical_across_locales() {
    let (zh_code, zh_stdout, zh_stderr) = run(&[
        "--locale",
        "zh-CN",
        "check",
        "--headless",
        "--fixture",
        "gemini-positive",
        "--json",
    ]);
    let (en_code, en_stdout, en_stderr) = run(&[
        "--locale",
        "en-US",
        "check",
        "--headless",
        "--fixture",
        "gemini-positive",
        "--json",
    ]);
    assert_eq!(zh_code, 0, "stderr={zh_stderr}");
    assert_eq!(en_code, 0, "stderr={en_stderr}");
    let mut zh: Value = serde_json::from_str(&zh_stdout).expect("zh JSON");
    let mut en: Value = serde_json::from_str(&en_stdout).expect("en JSON");
    strip_volatile(&mut zh);
    strip_volatile(&mut en);
    assert_eq!(zh, en, "JSON 机器字段不得随 locale 变化");
    assert!(zh["results"][0]["eligibility_notes"].is_array());
}

#[test]
fn network_plan_preview_is_zero_packet_and_reports_budget() {
    let (code, stdout, stderr) = run(&["check", "--plan", "--profile", "google-all", "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let value: Value = serde_json::from_str(&stdout).expect("纯 JSON");
    assert_eq!(value["schema"], "parallaxe.network-plan/v1");
    assert_eq!(value["target_count"], 107);
    assert_eq!(value["zero_packet"], true);
    assert_eq!(value["estimated_udp_quic_sessions"], 0);
    assert_eq!(value["min_interval_ms"], 3500);
    assert_eq!(value["batch_size"], 5);
    assert_eq!(value["node_count"], 0);
    assert!(value["estimated_wall_ms"].as_u64().unwrap() >= 12000);
}

#[test]
fn inter_target_interval_is_user_settable_and_capped() {
    // 用户可设跨目标间隔（不同服务之间），零发包预览可见。
    let (code, stdout, stderr) = run(&[
        "check",
        "--plan",
        "--profile",
        "google-all",
        "--inter-target-interval-ms",
        "4000",
        "--json",
    ]);
    assert_eq!(code, 0, "stderr={stderr}");
    let value: Value = serde_json::from_str(&stdout).expect("纯 JSON");
    assert_eq!(value["inter_target_interval_ms"], 4_000);
    assert_eq!(value["zero_packet"], true);

    // 超出上限（10000）必须拒绝。
    let (code2, _out2, _err2) = run(&[
        "check",
        "--plan",
        "--profile",
        "google-all",
        "--inter-target-interval-ms",
        "10001",
        "--json",
    ]);
    assert_ne!(code2, 0, "超限应被拒绝");
}

#[test]
fn md_and_txt_formats_work_with_locale() {
    let (code, stdout, stderr) = run(&[
        "--locale",
        "zh-CN",
        "check",
        "--headless",
        "--profile",
        "simple",
        "--format",
        "md",
    ]);
    assert_eq!(code, 3, "stderr={stderr}");
    assert!(stdout.starts_with("# "), "缺 Markdown 标题：{stdout}");
    assert!(stdout.contains("| 目标 | 状态 |"));
    assert!(stdout.contains("## 覆盖与未覆盖"));

    let (code, stdout, stderr) = run(&[
        "--locale",
        "en-US",
        "check",
        "--headless",
        "--profile",
        "simple",
        "--format",
        "txt",
    ]);
    assert_eq!(code, 3, "stderr={stderr}");
    assert!(stdout.contains("Target"));
    assert!(!stdout.contains('\r'));
}

#[test]
fn guard_export_refuses_when_no_candidate_and_requires_ack_for_full_mode() {
    let dir = std::env::temp_dir().join(format!("parallaxe-export-{}", std::process::id()));
    let dir_text = dir.to_string_lossy().to_string();
    let (code, stdout, stderr) = run(&[
        "guard",
        "export",
        "--target",
        "google.gemini",
        "--out-dir",
        &dir_text,
        "--json",
    ]);
    assert_eq!(code, 3, "stderr={stderr}");
    let value: Value = serde_json::from_str(&stdout).expect("拒绝 JSON");
    assert_eq!(value["status"], "REFUSED");
    assert!(
        value["code"]
            .as_str()
            .is_some_and(|code| code.starts_with("EXPORT_")),
        "必须是稳定导出机器码：{value}"
    );
    assert!(!dir.exists(), "被拒绝时不得创建输出目录");

    let (code, _, stderr) = run(&[
        "guard",
        "export",
        "--target",
        "google.gemini",
        "--out-dir",
        &dir_text,
        "--mode",
        "full",
    ]);
    assert_eq!(code, 1, "整包替换必须二次确认：{stderr}");
    assert!(stderr.contains("--acknowledge-overwrite"));
}

#[test]
fn beta_disables_system_writes_before_inputs_or_dns() {
    let sandbox = tempfile::tempdir().unwrap();
    for args in [
        vec![
            "guard",
            "block",
            "--resolve",
            "--apply",
            "--confirm",
            "--source-report",
            "missing.json",
            "--audit-out",
            "audit.json",
            "--json",
        ],
        vec![
            "guard",
            "unblock",
            "--rule-tag",
            "test",
            "--apply",
            "--json",
        ],
    ] {
        let output = Command::new(exe())
            .args(args)
            .current_dir(sandbox.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
        let doc: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(doc["code"], "SYSBLOCK_BETA_DISABLED");
        assert_eq!(doc["applied"], false);
    }
    assert_eq!(sandbox.path().read_dir().unwrap().count(), 0);
    let (_, help, _) = run(&["guard", "--help"]);
    assert!(help.contains("export") && help.contains("status"));
    assert!(!help
        .lines()
        .any(|line| matches!(line.split_whitespace().next(), Some("block" | "unblock"))));
}

#[test]
fn export_requires_explicit_folder_and_does_not_default_to_cwd() {
    let sandbox = tempfile::tempdir().unwrap();
    for extra in [&[][..], &["--out", "legacy"][..]] {
        let output = Command::new(exe())
            .args(["guard", "export"])
            .args(extra)
            .current_dir(sandbox.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        if extra.is_empty() {
            assert!(String::from_utf8_lossy(&output.stderr).contains("请显式指定导出文件夹"));
        }
        assert_eq!(sandbox.path().read_dir().unwrap().count(), 0);
    }
}

#[test]
fn node_sweep_requires_explicit_proxy_and_reports_scan_mode() {
    let (code, _, stderr) = run(&["check", "--plan", "--profile", "simple", "--node-sweep"]);
    assert_eq!(code, 1, "节点筛选必须显式代理：{stderr}");
    assert!(stderr.contains("--proxy"));

    let (code, stdout, stderr) = run(&["check", "--plan", "--profile", "simple", "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["scan_mode"], "SIMPLE");
    assert_eq!(value["requires_confirmation"], false);
}
