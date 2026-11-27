//! 任务 A/B 的 agent 端到端验收：AI 授权桥（A 一次性确认 / B 自主检测 / 自关即时收回）。
//!
//! - 默认用例完全离线：只验证协议、授权绑定、默认零发包与自关指令；
//! - `live_ai_bridge_sends_real_packets_for_a_and_b` 需显式 `--ignored` 运行（真实网络），
//!   用 `example.com` 作为用户导入目标，逐次授权并断言 `network_request_count > 0`。

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

use parallaxe_core::ai_bridge::{
    BridgeDecision, BridgePaths, BridgeRequest, BridgeResult, BridgeStatus,
};
use parallaxe_core::contracts::Report;
use parallaxe_core::targets::{LoadPaths, Registry};
use parallaxe_core::{runtime, CheckOptions};
use serde_json::{json, Value};

fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_parallaxe"))
}

struct Agent {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    root: PathBuf,
}

impl Agent {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!(
            "parallaxe-agent-bridge-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let mut child = Command::new(exe())
            .args(["agent", "--stdio"])
            .env("PARALLAXE_AI_BRIDGE_DIR", &root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动 agent");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
            root,
        }
    }

    fn paths(&self) -> BridgePaths {
        BridgePaths::with_root(&self.root)
    }

    fn request(&mut self, id: &str, op: &str, args: Value) -> Value {
        let line = json!({"schema": "parallaxe.agent/v1", "id": id, "op": op, "args": args});
        writeln!(self.stdin, "{line}").expect("写请求");
        self.stdin.flush().expect("flush");
        loop {
            let mut text = String::new();
            let read = self.reader.read_line(&mut text).expect("读响应");
            assert!(read > 0, "agent 提前退出");
            let value: Value = serde_json::from_str(text.trim()).expect("NDJSON 合法");
            if value.get("record").is_some() {
                continue;
            }
            assert_eq!(value["id"], id);
            return value;
        }
    }

    fn finish(mut self) -> i32 {
        drop(self.stdin);
        self.child.wait().expect("等待 agent").code().unwrap_or(-1)
    }
}

fn write_status(root: &Path, autonomous: bool, imported: &[&str]) {
    BridgePaths::with_root(root)
        .write_status(&BridgeStatus::new(
            "GUI",
            autonomous,
            imported.iter().map(|id| id.to_string()).collect(),
        ))
        .expect("写状态投影");
}

fn read_request(root: &Path, request_id: &str) -> BridgeRequest {
    BridgePaths::with_root(root)
        .read_requests()
        .into_iter()
        .find(|request| request.request_id == request_id)
        .expect("桥请求必须已写入")
}

#[test]
fn bridge_contract_defaults_off_and_self_disable_revokes() {
    let mut agent = Agent::start();
    let root = agent.root.clone();

    // 无 GUI 状态投影：默认零发包、不可提交。
    let status = agent.request("s0", "ai_status", json!({}));
    assert_eq!(status["data"]["available"], false);
    assert_eq!(status["data"]["autonomous_enabled"], false);
    let denied = agent.request(
        "s1",
        "ai_request_check",
        json!({"targets": ["example.com"]}),
    );
    assert_eq!(denied["error"]["code"], "BRIDGE_UNAVAILABLE");

    // 用户打开 GUI 后：A 路径默认走人工确认。
    write_status(&root, false, &["example.com"]);
    let pending = agent.request(
        "s2",
        "ai_request_check",
        json!({"targets": ["example.com"], "purpose": "只观测首跳"}),
    );
    assert_eq!(pending["data"]["status"], "PENDING");
    assert_eq!(pending["data"]["mode"], "USER_CONFIRMATION");
    assert_eq!(pending["data"]["min_interval_ms"], 3500);
    let request_id = pending["data"]["request_id"].as_str().unwrap().to_string();
    let request = read_request(&root, &request_id);
    assert_eq!(request.scope.targets, vec!["example.com".to_string()]);

    // 未确认前保持 PENDING，零发包。
    let waiting = agent.request(
        "s3",
        "ai_request_status",
        json!({"request_id": request_id, "wait_ms": 300}),
    );
    assert_eq!(waiting["data"]["status"], "PENDING");
    assert_eq!(waiting["data"]["network_request_count"], 0);

    // 用户拒绝：零发包。
    agent
        .paths()
        .write_decision(&BridgeDecision::deny(&request))
        .unwrap();
    let denied = agent.request("s4", "ai_request_status", json!({"request_id": request_id}));
    assert_eq!(denied["data"]["status"], "DENIED");

    // URL 不得进入 scope；agent 也不得自行打开自主检测。
    let bad = agent.request(
        "s5",
        "ai_request_check",
        json!({"targets": ["https://example.com/"]}),
    );
    assert_eq!(bad["error"]["code"], "INVALID_ARGUMENT");
    let enable = agent.request("s6", "ai_autonomous", json!({"enabled": true}));
    assert_eq!(enable["error"]["code"], "POLICY_DENIED");

    // 模拟用户已打开自主检测；agent 自关后必须立刻回到人工确认。
    write_status(&root, true, &["example.com"]);
    let auto = agent.request(
        "s7",
        "ai_request_check",
        json!({"targets": ["example.com"]}),
    );
    assert_eq!(auto["data"]["mode"], "AUTONOMOUS");

    let revoked = agent.request(
        "s8",
        "ai_autonomous",
        json!({"enabled": false, "reason": "批量测试结束"}),
    );
    assert_eq!(revoked["data"]["status"], "REVOKED");
    assert_eq!(revoked["data"]["autonomous"], false);

    // 宿主消费控制指令并翻回关闭（模拟 GUI 下一帧）。
    let controls = agent.paths().take_controls();
    assert_eq!(controls.len(), 1);
    write_status(&root, false, &["example.com"]);
    let after = agent.request(
        "s9",
        "ai_request_check",
        json!({"targets": ["example.com"]}),
    );
    assert_eq!(after["data"]["mode"], "USER_CONFIRMATION");

    assert_eq!(agent.finish(), 0);
}

#[test]
fn cli_ai_status_is_read_only_and_reports_bridge_state() {
    let root = std::env::temp_dir().join(format!(
        "parallaxe-cli-ai-status-{}-{}",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    write_status(&root, false, &["user.example-home"]);
    let output = Command::new(exe())
        .args(["ai", "status", "--json"])
        .env("PARALLAXE_AI_BRIDGE_DIR", &root)
        .output()
        .expect("运行 ai status");
    assert!(output.status.success());
    let doc: Value = serde_json::from_slice(&output.stdout).expect("JSON 输出");
    assert_eq!(doc["available"], true);
    assert_eq!(doc["autonomous_enabled"], false);
    assert_eq!(doc["session_only"], true);
    assert_eq!(doc["imported_targets"][0], "user.example-home");
    assert_eq!(doc["requests"].as_array().map(Vec::len), Some(0));
}

fn real_run(registry: &Registry, ids: &[String], label: &str) -> Report {
    registry
        .authorize_session_once(ids, true)
        .expect("一次性会话许可");
    let options = CheckOptions {
        targets: ids.to_vec(),
        min_interval_ms: Some(3500),
        batch_size: Some(5),
        confirm_budget: true,
        run_id: Some(format!("run-live-ai-bridge-{label}")),
        ..Default::default()
    };
    let transport: Arc<dyn parallaxe_core::probe::Transport> =
        Arc::new(parallaxe_core::probe::HttpTransport::new(None, None).expect("transport"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime
        .block_on(runtime::run_check(
            registry,
            &options,
            Some(transport),
            true,
            None,
        ))
        .expect("真实运行")
        .report
}

/// 真实发包证据（需网络与 `--ignored`）：A 一次性确认 → 真实首跳；
/// B 自主模式 → 再次真实首跳并标注 AGENT_AUTONOMOUS；自关后不再自动执行。
#[test]
#[ignore = "live network: run explicitly with --ignored --nocapture"]
fn live_ai_bridge_sends_real_packets_for_a_and_b() {
    let mut agent = Agent::start();
    let root = agent.root.clone();

    let mut registry = Registry::load(&LoadPaths {
        targets_dir: None,
        profiles_dir: None,
        config: None,
    })
    .expect("加载目录");
    let text =
        "验收导入 | https://example.com/ | rule.http,rule.trace | Example 首页 | example-home";
    let bundle = parallaxe_core::import_bundle::parse_import_text(
        text,
        &["rule.http".to_string(), "rule.trace".to_string()],
        false,
    )
    .expect("解析导入文本");
    let preview = registry.preview_import_bundle(&bundle);
    assert!(preview.all_valid, "导入预览必须全绿");
    let imported = registry.import_bundle(&bundle, true, true).expect("导入");
    let ids: Vec<String> = imported
        .imported_targets
        .iter()
        .map(|meta| meta.id.clone())
        .collect();
    assert_eq!(ids.len(), 1);
    let target_id = ids[0].clone();

    // 路径 A：用户未打开自主检测 → 请求等待人工确认。
    write_status(&root, false, &[target_id.as_str()]);
    let pending = agent.request(
        "a1",
        "ai_request_check",
        json!({"targets": [target_id.as_str()], "purpose": "用户确认后观测首跳"}),
    );
    assert_eq!(pending["data"]["mode"], "USER_CONFIRMATION");
    let request_id = pending["data"]["request_id"].as_str().unwrap().to_string();
    let request = read_request(&root, &request_id);
    BridgePaths::with_root(&root)
        .write_decision(&BridgeDecision::approve(&request, false))
        .expect("写授权决定");
    let mut report_a = real_run(&registry, &ids, "a");
    assert!(report_a.network_request_count > 0, "A 路径缺少真实发包证据");
    // 宿主（GUI）对用户导入目标补记来源，与生产 GUI 的桥回传一致。
    report_a.origin = Some(parallaxe_core::contracts::ORIGIN_USER_IMPORTED.to_string());
    BridgePaths::with_root(&root)
        .write_result(&BridgeResult::completed(&request, report_a.clone(), false))
        .expect("写回报告");

    let done_a = agent.request("a2", "ai_request_status", json!({"request_id": request_id}));
    assert_eq!(done_a["data"]["status"], "COMPLETED");
    let run_a = done_a["data"]["run_id"].as_str().unwrap().to_string();
    assert_eq!(
        done_a["data"]["network_request_count"].as_u64(),
        Some(report_a.network_request_count)
    );
    let explain = agent.request(
        "a3",
        "explain",
        json!({"run_id": run_a, "target": target_id.as_str()}),
    );
    assert_eq!(explain["ok"], true);
    assert_eq!(explain["data"]["result"]["target"], target_id.as_str());
    let export = agent.request(
        "a4",
        "export_report",
        json!({"run_id": run_a, "format": "json"}),
    );
    let content: Value = serde_json::from_str(export["data"]["content"].as_str().unwrap()).unwrap();
    assert!(content["network_request_count"].as_u64().unwrap() > 0);
    assert_eq!(
        content["origin"].as_str(),
        Some(parallaxe_core::contracts::ORIGIN_USER_IMPORTED)
    );
    println!(
        "[A] real packets={} run={} origin={}",
        report_a.network_request_count, run_a, content["origin"]
    );

    // 路径 B：用户已打开「自主检测」→ 请求按 AUTONOMOUS 提交，报告标注 AGENT_AUTONOMOUS。
    write_status(&root, true, &[target_id.as_str()]);
    let auto = agent.request(
        "b1",
        "ai_request_check",
        json!({"targets": [target_id.as_str()], "purpose": "自主模式下重复首跳"}),
    );
    assert_eq!(auto["data"]["mode"], "AUTONOMOUS");
    let request_b_id = auto["data"]["request_id"].as_str().unwrap().to_string();
    let request_b = read_request(&root, &request_b_id);
    BridgePaths::with_root(&root)
        .write_decision(&BridgeDecision::approve(&request_b, true))
        .expect("自主授权决定");
    let mut report_b = real_run(&registry, &ids, "b");
    assert!(report_b.network_request_count > 0);
    report_b.origin = Some(parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS.to_string());
    report_b.autonomous_mode = Some(true);
    BridgePaths::with_root(&root)
        .write_result(&BridgeResult::completed(&request_b, report_b.clone(), true))
        .expect("写回自主报告");
    let done_b = agent.request(
        "b2",
        "ai_request_status",
        json!({"request_id": request_b_id}),
    );
    assert_eq!(done_b["data"]["status"], "COMPLETED");
    assert_eq!(done_b["data"]["autonomous"], true);
    let run_b = done_b["data"]["run_id"].as_str().unwrap().to_string();
    let export_b = agent.request(
        "b3",
        "export_report",
        json!({"run_id": run_b, "format": "json"}),
    );
    let content_b: Value =
        serde_json::from_str(export_b["data"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        content_b["origin"].as_str(),
        Some(parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS)
    );
    assert_eq!(content_b["autonomous_mode"], true);
    assert!(content_b["network_request_count"].as_u64().unwrap() > 0);
    println!(
        "[B] real packets={} run={} origin={} autonomous_mode={}",
        report_b.network_request_count, run_b, content_b["origin"], content_b["autonomous_mode"]
    );

    // AI 自主关闭：权限即时收回，下一请求回到人工确认（且零发包）。
    let revoked = agent.request(
        "b4",
        "ai_autonomous",
        json!({"enabled": false, "reason": "批量验收结束，主动收回权限"}),
    );
    assert_eq!(revoked["data"]["status"], "REVOKED");
    let controls = agent.paths().take_controls();
    assert_eq!(controls.len(), 1);
    write_status(&root, false, &[target_id.as_str()]);
    let after = agent.request(
        "b5",
        "ai_request_check",
        json!({"targets": [target_id.as_str()]}),
    );
    assert_eq!(after["data"]["mode"], "USER_CONFIRMATION");
    let after_id = after["data"]["request_id"].as_str().unwrap().to_string();
    let still_pending = agent.request(
        "b6",
        "ai_request_status",
        json!({"request_id": after_id, "wait_ms": 200}),
    );
    assert_eq!(still_pending["data"]["status"], "PENDING");
    assert_eq!(still_pending["data"]["network_request_count"], 0);
    println!("[B] self-disable revoked: next request stays USER_CONFIRMATION / zero packets");

    assert_eq!(agent.finish(), 0);
}
