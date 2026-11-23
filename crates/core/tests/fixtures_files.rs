//! 文件形式的最小回归集：`tests/fixtures/<target>/<case>/{meta.json,response.http}`。

use std::fs;
use std::path::{Path, PathBuf};

use parallaxe_core::contracts::{Attempt, AttemptFamily, ObservedFamily, Path as ProbePath, State};
use parallaxe_core::probe::FetchOutcome;
use parallaxe_core::signals::{self, Sample};
use parallaxe_core::state::NullStore;
use parallaxe_core::targets::Registry;
use parallaxe_core::verdict::{decide, VerdictInput};
use serde_json::Value;
use time::OffsetDateTime;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
}

fn meta_files(root: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                meta_files(&path, out);
            } else if path.file_name().and_then(|name| name.to_str()) == Some("meta.json") {
                out.push(path);
            }
        }
    }
}

#[test]
fn fixture_files_follow_contract_and_match_expectations() {
    let registry = Registry::embedded().expect("registry");
    let root = fixture_root();
    let mut files = Vec::new();
    meta_files(&root, &mut files);
    assert!(!files.is_empty(), "缺少 tests/fixtures 案例");
    for meta_path in files {
        let directory = meta_path.parent().expect("case dir");
        let meta: Value = serde_json::from_str(&fs::read_to_string(&meta_path).expect("meta"))
            .expect("meta json");
        assert_eq!(meta["schema"], "parallaxe.fixture/v1");
        assert_eq!(meta["mode"], "FIXTURE");
        let target_id = meta["target_id"].as_str().expect("target_id");
        let target = registry.get(target_id).unwrap_or_else(|| {
            panic!("fixture 目标不存在：{target_id}");
        });
        assert_eq!(
            meta["expected"]["network_request_count"].as_u64(),
            Some(0),
            "fixture 不得声称真实网络请求"
        );
        let rule_versions = meta["rule_versions"].as_object().expect("rule_versions");
        for (rule_id, version) in rule_versions {
            let rule = registry
                .rule(rule_id)
                .unwrap_or_else(|| panic!("未知规则 {rule_id}"));
            assert_eq!(version.as_u64(), Some(rule.version));
            assert!(target.raw.signals.contains(rule_id));
        }
        let response = fs::read_to_string(directory.join("response.http")).expect("response");
        let sample: Sample = signals::parse_http_text(&response).expect("response.http 解析");
        assert_eq!(
            meta["body_bytes"].as_u64(),
            Some(sample.body.len() as u64),
            "{} 的 body_bytes 与净化体不一致",
            meta_path.display()
        );
        let status = meta["status"].as_u64().map(|value| value as u16);
        assert_eq!(status, sample.status);
        let truncated = meta["truncated"].as_bool().unwrap_or(false);
        let complete = meta["complete"].as_bool().unwrap_or(true);
        assert_eq!(truncated, !complete);
        let mut sample = sample;
        sample.complete = complete;
        sample.truncated = truncated;
        sample.observed_family = Some(ObservedFamily::V4);

        let probe_path = ProbePath::system("path-fixture");
        let evaluation = signals::evaluate(
            &registry,
            target,
            &sample,
            &probe_path.path_id,
            "2026-11-21T00:00:00Z",
            true,
        );
        let outcome = FetchOutcome {
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
            request_count: meta["expected"]["request_count"].as_u64().unwrap_or(1),
            error: None,
            observed_family: Some(ObservedFamily::V4),
        };
        let store = NullStore::default();
        let result = decide(VerdictInput {
            registry: &registry,
            target,
            evaluation: &evaluation,
            outcome: &outcome,
            path: &probe_path,
            path_diagnostic: parallaxe_core::network::PathDiagnostic::System,
            fixture: true,
            now: OffsetDateTime::now_utc(),
            store: &store,
            expectation: None,
            capability_unsupported: None,
            cancelled: false,
            global_timeout: false,
        });
        let expected_state: State =
            serde_json::from_value(meta["expected"]["state"].clone()).expect("expected.state");
        assert_eq!(
            result.state,
            expected_state,
            "{} 状态不符（reason={:?}）",
            meta_path.display(),
            result.reason
        );
        let expected_reason = meta["expected"]["reason"].as_str();
        assert_eq!(
            result.reason.as_deref(),
            expected_reason,
            "{} reason 不符",
            meta_path.display()
        );
        assert_eq!(result.request_count, outcome.request_count);
        assert_eq!(result.region_layers.len(), 7);
    }
}

#[test]
fn fixture_response_parser_handles_crlf_and_empty_body() {
    let text = "HTTP/1.1 204 SYNTHETIC\r\nContent-Type: text/plain\r\nContent-Length: 0\r\n\r\n";
    let sample = signals::parse_http_text(text).expect("parse");
    assert_eq!(sample.status, Some(204));
    assert!(sample.body.is_empty());
    assert_eq!(sample.content_type.as_deref(), Some("text/plain"));
}
