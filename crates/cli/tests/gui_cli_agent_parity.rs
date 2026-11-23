//! 真实三端进程一致性验收：CLI 二进制（`--json` / `--jsonl`）× agent `--stdio` 子进程 × GUI `AppState`。

#![cfg(feature = "gui")]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use parallaxe_core::contracts::{Event, Report};
use parallaxe_gui::state::AppState;
use serde_json::Value;

fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_parallaxe")
}

/// 抹平两次独立运行必然不同的 run_id / 时间戳 / 墙钟，保留全部业务判定字段。
fn normalize_report(mut report: Value) -> Value {
    report["run_id"] = Value::String("<RUN_ID>".to_string());
    report["generated_at"] = Value::String("<TS>".to_string());
    report["wall_duration_ms"] = Value::Number(0.into());
    if let Some(results) = report["results"].as_array_mut() {
        for r in results {
            if let Some(evs) = r["evidence"].as_array_mut() {
                for ev in evs {
                    ev["observed_at"] = Value::String("<TS>".to_string());
                    if ev["freshness"]["last_hit_at"].is_string() {
                        ev["freshness"]["last_hit_at"] = Value::String("<TS>".to_string());
                    }
                }
            }
        }
    }
    report
}

fn run_cli_json_and_jsonl(args: &[&str]) -> (i32, Report, Vec<Event>) {
    let mut json_args = args.to_vec();
    json_args.push("--json");
    let out_json = Command::new(exe())
        .args(&json_args)
        .output()
        .expect("CLI --json");
    let code = out_json.status.code().unwrap_or(-1);
    let report: Report =
        serde_json::from_slice(&out_json.stdout).expect("CLI --json 输出应可解析为 Report");

    let mut jsonl_args = args.to_vec();
    jsonl_args.push("--jsonl");
    let out_jsonl = Command::new(exe())
        .args(&jsonl_args)
        .output()
        .expect("CLI --jsonl");
    assert_eq!(out_jsonl.status.code(), Some(code));
    let events: Vec<Event> = String::from_utf8_lossy(&out_jsonl.stdout)
        .lines()
        .map(|l| serde_json::from_str::<Event>(l).expect("JSONL 行合法"))
        .collect();
    (code, report, events)
}

fn run_agent_check(args: Value) -> (Value, Vec<Event>) {
    let mut child = Command::new(exe())
        .args(["agent", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动 agent");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let req = serde_json::json!({
        "schema": "parallaxe.agent/v1",
        "id": "parity-1",
        "op": "check",
        "args": args,
    });
    writeln!(stdin, "{req}").unwrap();
    stdin.flush().unwrap();
    drop(stdin);

    let reader = BufReader::new(stdout);
    let mut events = Vec::new();
    let mut terminal_report = None;
    for line in reader.lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        let val: Value = serde_json::from_str(&line).unwrap();
        if let Some(rec) = val.get("record") {
            let ev: Event = serde_json::from_value(rec.clone()).unwrap();
            if let Some(rep) = rec["payload"].get("report") {
                terminal_report = Some(rep.clone());
            }
            events.push(ev);
        }
    }
    let status = child.wait().unwrap();
    assert!(status.success(), "agent 进程应正常退出");
    (terminal_report.expect("agent 应输出终结报告"), events)
}

#[test]
fn tri_interface_live_process_parity_drive_conflict_and_google_all() {
    for (cli_args, agent_args, expected_code) in [
        (
            vec!["check", "--headless", "--fixture", "drive-login"],
            serde_json::json!({"fixture": "drive-login"}),
            3,
        ),
        (
            vec!["check", "--headless", "--fixture", "conflict-tie"],
            serde_json::json!({"fixture": "conflict-tie"}),
            3,
        ),
        (
            vec!["check", "--headless", "--fixture", "gemini-positive"],
            serde_json::json!({"fixture": "gemini-positive"}),
            0,
        ),
        (
            vec!["check", "--headless", "--profile", "google-all"],
            serde_json::json!({"profile": "google-all"}),
            3,
        ),
    ] {
        let (code, cli_report, cli_events) = run_cli_json_and_jsonl(&cli_args);
        assert_eq!(code, expected_code);
        let (agent_report, agent_events) = run_agent_check(agent_args);

        let gui_from_cli = AppState::from_events(&cli_events);
        let gui_from_agent = AppState::from_events(&agent_events);

        let cli_norm = normalize_report(serde_json::to_value(&cli_report).unwrap());
        let agent_norm = normalize_report(agent_report);
        let gui_cli_norm = normalize_report(
            serde_json::to_value(gui_from_cli.active_run().unwrap().report.as_ref().unwrap())
                .unwrap(),
        );
        let gui_agent_norm = normalize_report(
            serde_json::to_value(
                gui_from_agent
                    .active_run()
                    .unwrap()
                    .report
                    .as_ref()
                    .unwrap(),
            )
            .unwrap(),
        );

        assert_eq!(
            gui_cli_norm, cli_norm,
            "GUI(来自CLI事件) 必须与 CLI --json 一致"
        );
        assert_eq!(
            gui_agent_norm, agent_norm,
            "GUI(来自agent事件) 必须与 agent 报告一致"
        );
        assert_eq!(cli_norm, agent_norm, "CLI 与 agent 报告必须逐字段一致");
    }
}
