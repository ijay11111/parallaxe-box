//! 报告 schema 与三格式投影：字段集合、枚举、七层顺序、CSV/HTML 安全。

use parallaxe_core::contracts::{Mode, Report, State};
use parallaxe_core::reports;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::Registry;

fn keys(value: &serde_json::Value) -> Vec<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn report_matches_schema_field_sets() {
    let registry = Registry::embedded().expect("registry");
    let options = CheckOptions {
        fixture: Some("gemini-positive".to_string()),
        run_id: Some("run-schema".to_string()),
        ..Default::default()
    };
    let output = runtime::run_check(&registry, &options, None, false, None)
        .await
        .expect("fixture run");
    let report = serde_json::to_value(&output.report).expect("serialize");
    let mut top = keys(&report);
    top.sort();
    let expected_top = [
        "coverage",
        "exit_code",
        "generated_at",
        "mode",
        "network_request_count",
        "plans",
        "results",
        "run_id",
        "schema",
        "simulated_request_count",
        "termination",
        "tool",
        "version",
        "wall_duration_ms",
    ];
    assert_eq!(top, expected_top);
    assert_eq!(report["schema"], "parallaxe.report/v1");
    assert_eq!(report["tool"], "parallaxe-box");
    assert_eq!(report["mode"], "FIXTURE");
    assert_eq!(report["network_request_count"], 0);
    assert_eq!(report["simulated_request_count"], 1);
    assert_eq!(report["exit_code"], 0);

    let result = &report["results"][0];
    let required = [
        "target",
        "state",
        "reason",
        "confidence",
        "source",
        "path",
        "duration_ms",
        "request_count",
        "evidence",
        "region_layers",
        "observed",
        "conflicts",
        "agreement",
        "transport",
        "service_observed",
        "risk_signal",
        "local_policy",
    ];
    for key in required {
        assert!(result.get(key).is_some(), "result 缺少 {key}");
    }
    let allowed: Vec<&str> = required
        .iter()
        .copied()
        .chain(
            [
                "expectation",
                "origin",
                "consent_version",
                "imported_at",
                "rule_digest",
                "eligibility_notes",
            ]
            .into_iter(),
        )
        .collect();
    for key in keys(result) {
        assert!(allowed.contains(&key.as_str()), "result 多余字段 {key}");
    }
    assert_eq!(result["state"], "OK");
    assert_eq!(result["path"]["requested_family"], "AUTO");
    assert_eq!(result["path"]["bind_status"], "NOT_REQUESTED");
    let layers = result["region_layers"].as_array().expect("layers");
    assert_eq!(layers.len(), 7);
    let names: Vec<&str> = layers
        .iter()
        .map(|layer| layer["layer"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        vec![
            "EGRESS_NETWORK",
            "SERVICE_POLICY",
            "ACCOUNT",
            "PAYMENTS",
            "PREFERENCE",
            "DEVICE",
            "CLOUD"
        ]
    );
    assert_eq!(layers[2]["value"], serde_json::Value::Null);
    assert_eq!(layers[2]["reason"], "NOT_OBSERVABLE_ANONYMOUSLY");

    let evidence = &result["evidence"][0];
    let evidence_keys = [
        "id",
        "target_id",
        "sample_id",
        "rule_id",
        "rule_version",
        "observed_at",
        "layer",
        "claim",
        "value",
        "source_type",
        "source_ref",
        "confidence",
        "path_id",
        "origin",
        "reason",
        "fixture",
        "freshness",
    ];
    for key in evidence_keys {
        assert!(evidence.get(key).is_some(), "evidence 缺少 {key}");
    }
    assert_eq!(evidence["fixture"], true);

    for event in &output.events {
        let value = serde_json::to_value(event).expect("event");
        assert_eq!(value["schema"], "parallaxe.event/v1");
        assert!(value["seq"].as_u64().unwrap_or(0) >= 1);
        assert_eq!(value["is_fixture"], true);
    }
}

#[tokio::test]
async fn empty_admission_report_is_well_formed() {
    let registry = Registry::embedded().expect("registry");
    let options = CheckOptions {
        profile: Some("google-all".to_string()),
        run_id: Some("run-empty".to_string()),
        ..Default::default()
    };
    let output = runtime::run_check(&registry, &options, None, false, None)
        .await
        .expect("check");
    let report = &output.report;
    assert!(report.results.is_empty());
    assert_eq!(report.coverage.selected_count, 107);
    assert_eq!(report.coverage.uncovered.len(), 135);
    assert!(report
        .coverage
        .uncovered
        .iter()
        .filter(|item| item.selected)
        .all(|item| item.reason != "NOT_SELECTED"));
    assert_eq!(report.network_request_count, 0);
    assert_eq!(report.exit_code, 3);
    assert_eq!(report.termination, parallaxe_core::Termination::Complete);
    // 覆盖集合互斥且覆盖全部目录 ID。
    let mut ids: Vec<&str> = report
        .coverage
        .uncovered
        .iter()
        .map(|item| item.target.as_str())
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 135);
}

#[test]
fn csv_is_formula_safe_and_html_is_offline() {
    let mut report = Report::new("0.1.0", "2026-11-21T00:00:00Z", "run-format", Mode::Online);
    report.exit_code = 0;
    let mut record = parallaxe_core::contracts::ResultRecord {
        target: "=cmd|<script>alert(1)</script>".to_string(),
        state: State::ReachableOnly,
        reason: Some("NO_REGION_SIGNAL".to_string()),
        confidence: parallaxe_core::Confidence::Low,
        source: Some(parallaxe_core::Source::DirectAnonymousObservation),
        path: parallaxe_core::Path::system("path-format"),
        duration_ms: 1,
        request_count: 0,
        evidence: Vec::new(),
        region_layers: parallaxe_core::verdict::all_layers_null(),
        observed: Vec::new(),
        conflicts: Vec::new(),
        agreement: Vec::new(),
        transport: parallaxe_core::TransportLayerResult {
            attempts: Vec::new(),
            path_diagnostic: Some("SYSTEM_PATH".to_string()),
        },
        service_observed: parallaxe_core::ServiceObserved {
            surface: "<script>alert(1)</script>".to_string(),
            observed_country: None,
            evidence_ids: Vec::new(),
            reason: None,
        },
        risk_signal: parallaxe_core::RiskSignal {
            markers: Vec::new(),
            evidence_ids: Vec::new(),
            reason: None,
        },
        local_policy: parallaxe_core::LocalPolicy {
            protected: false,
            guard_eligible: false,
            plan_id: None,
            reason: None,
        },
        expectation: None,
        origin: None,
        consent_version: None,
        imported_at: None,
        rule_digest: None,
        eligibility_notes: Vec::new(),
    };
    record.region_layers[0].value = Some("US".to_string());
    report.results.push(record);
    report.exit_code = 0;
    let csv = reports::to_csv(&report);
    assert!(csv.starts_with("target,state,reason"));
    assert!(csv.contains("\"'=cmd"), "CSV 必须防公式注入：{csv}");
    let html = reports::to_html(&report);
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(!html.contains("<script src"));
    assert!(!html.contains("fonts.googleapis.com"));
    assert!(!html.contains("cdn.jsdelivr.net"));
    assert!(html.contains("&lt;script&gt;"), "不可信文本必须转义");
    assert!(html.contains("本次未覆盖"));
    assert!(reports::to_jsonl(&[]).is_empty());
    assert_eq!(reports::usage_error_code(), 1);
}

#[test]
fn empty_selection_is_not_vacuous_success() {
    let mut report = Report::new("test", "2026-11-21T00:00:00Z", "empty", Mode::Online);
    assert_eq!(reports::aggregate_report(&report), 3);
    report.termination = parallaxe_core::Termination::Failed;
    assert_eq!(reports::aggregate_report(&report), 1);
}
