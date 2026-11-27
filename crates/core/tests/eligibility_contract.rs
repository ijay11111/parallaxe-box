//! 资格说明契约：12 族覆盖、只读静态、不参与判定、JSON 可选字段。

use parallaxe_core::contracts::{Mode, Report};
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::Registry;
use parallaxe_core::EligibilityCategory;

const GATED_FAMILIES: [&str; 12] = [
    "ai",
    "identity",
    "workspace",
    "education",
    "developer",
    "cloud",
    "media",
    "store",
    "content",
    "search",
    "api",
    "static",
];

#[test]
fn catalog_covers_all_twelve_service_families() {
    let registry = Registry::embedded().expect("registry");
    let covered = registry.eligibility_families();
    for family in GATED_FAMILIES {
        assert!(covered.contains(family), "缺少服务族 {family}");
    }
    assert!(registry.eligibility.len() >= 18);
}

#[test]
fn every_google_target_has_at_least_one_official_note() {
    let registry = Registry::embedded().expect("registry");
    for target in registry
        .entries()
        .iter()
        .filter(|target| target.id().starts_with("google."))
    {
        let notes = registry.eligibility_notes_for(target.id());
        assert!(
            !notes.is_empty(),
            "Google 目标 {} 没有任何资格说明",
            target.id()
        );
        for note in notes {
            assert_eq!(note.category, EligibilityCategory::EntitlementGate);
            assert!(!note.verified, "verified 必须为 false");
            assert!(
                note.source_url.starts_with("https://"),
                "source_url 必须是 https：{}",
                note.source_url
            );
            assert!(note.summary_key.starts_with("eligibility."));
        }
    }
}

#[test]
fn control_targets_do_not_get_google_entitlement_notes() {
    let registry = Registry::embedded().expect("registry");
    assert!(registry.eligibility_notes_for("control.chatgpt").is_empty());
}

#[tokio::test]
async fn report_carries_notes_without_changing_verdict() {
    let registry = Registry::embedded().expect("registry");
    let options = CheckOptions {
        fixture: Some("gemini-positive".to_string()),
        ..Default::default()
    };
    let output = runtime::run_check(&registry, &options, None, false, None)
        .await
        .expect("fixture run");
    let report = &output.report;
    assert_eq!(report.mode, Mode::Fixture);
    assert_eq!(report.network_request_count, 0);
    assert_eq!(report.exit_code, 0);
    let result = report.results.first().expect("one result");
    assert_eq!(result.state.as_str(), "OK");
    assert!(!result.eligibility_notes.is_empty(), "gemini 应带资格说明");
    let json = serde_json::to_value(result).expect("serialize");
    let notes = json["eligibility_notes"].as_array().expect("notes array");
    assert!(!notes.is_empty());
    assert_eq!(notes[0]["category"], "ENTITLEMENT_GATE");
    assert_eq!(notes[0]["verified"], false);
    // 机器判定字段不受影响。
    assert_eq!(json["state"], "OK");
    assert_eq!(json["reason"], serde_json::Value::Null);
}

#[test]
fn empty_notes_are_omitted_for_backward_compatibility() {
    let mut report = Report::new("0.1.0", "2026-11-22T00:00:00Z", "run-note", Mode::Fixture);
    let mut result = parallaxe_core::contracts::ResultRecord {
        destination: Default::default(),
        target: "synthetic".to_string(),
        state: parallaxe_core::State::Unknown,
        reason: None,
        confidence: parallaxe_core::Confidence::None,
        source: None,
        path: parallaxe_core::Path::system("path-note"),
        duration_ms: 0,
        request_count: 0,
        evidence: Vec::new(),
        region_layers: parallaxe_core::verdict::all_layers_null(),
        observed: Vec::new(),
        conflicts: Vec::new(),
        agreement: Vec::new(),
        transport: parallaxe_core::TransportLayerResult {
            attempts: Vec::new(),
            path_diagnostic: None,
        },
        service_observed: parallaxe_core::ServiceObserved {
            surface: "synthetic".to_string(),
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
    result.region_layers[0].value = Some("US".to_string());
    report.results.push(result);
    let mut json = serde_json::to_value(&report).expect("serialize");
    assert!(json["results"][0].get("eligibility_notes").is_none());
    // 旧报告（没有该字段）仍可反序列化。
    let parsed: Report = serde_json::from_value(json.clone()).expect("旧报告反序列化");
    assert!(parsed.results[0].eligibility_notes.is_empty());
    // 人为塞回旧字段后再删掉，模拟跨版本报告。
    json["results"][0]["eligibility_notes"] = serde_json::json!([]);
    let parsed: Report = serde_json::from_value(json).expect("空数组反序列化");
    assert!(parsed.results[0].eligibility_notes.is_empty());
}
