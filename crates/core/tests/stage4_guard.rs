//! 阶段 4 · Guard 影响闭包、否决优先级、审计与只读边界。

use parallaxe_core::contracts::{
    Confidence, LocalPolicy, Path, ResultRecord, RiskSignal, ServiceObserved, Source, State,
    TransportLayerResult,
};
use parallaxe_core::guard::{self, GuardRequest};
use parallaxe_core::targets::Registry;
use parallaxe_core::verdict;
use time::macros::date;

fn result_for(registry: &Registry, id: &str, state: State) -> ResultRecord {
    let target = registry.get(id).expect("目标存在");
    ResultRecord {
        destination: Default::default(),
        target: id.to_string(),
        state,
        reason: Some("SYNTHETIC_STAGE4".to_string()),
        confidence: Confidence::High,
        source: Some(Source::DirectAnonymousObservation),
        path: Path::system("path-stage4-test"),
        duration_ms: 1,
        request_count: 1,
        evidence: Vec::new(),
        region_layers: verdict::all_layers_null(),
        observed: Vec::new(),
        conflicts: Vec::new(),
        agreement: Vec::new(),
        transport: TransportLayerResult {
            attempts: Vec::new(),
            path_diagnostic: Some("SYSTEM_PATH".to_string()),
        },
        service_observed: ServiceObserved {
            surface: target.raw.surface.clone(),
            observed_country: None,
            evidence_ids: Vec::new(),
            reason: None,
        },
        risk_signal: RiskSignal {
            markers: Vec::new(),
            evidence_ids: Vec::new(),
            reason: None,
        },
        local_policy: LocalPolicy {
            protected: target.raw.protected,
            guard_eligible: !target.raw.protected,
            plan_id: None,
            reason: None,
        },
        expectation: None,
        origin: None,
        consent_version: None,
        imported_at: None,
        rule_digest: None,
        eligibility_notes: Vec::new(),
    }
}

fn preview_for(
    registry: &Registry,
    targets: &[String],
    results: &[ResultRecord],
    as_of: time::Date,
) -> guard::GuardPreview {
    guard::plan_preview(
        registry,
        GuardRequest {
            run_id: "run-guard-test",
            source_run_id: Some("run-source"),
            targets,
            results,
            recorded_at: "2026-11-22T09:52:00Z".to_string(),
            as_of,
            source_interrupted: false,
        },
    )
}

#[test]
fn dependency_conflict_is_vetoed_without_rule_fragment() {
    let registry = Registry::embedded().unwrap();
    let targets = vec!["google.notebook".to_string()];
    let results = vec![result_for(&registry, "google.notebook", State::Restricted)];
    let preview = preview_for(&registry, &targets, &results, date!(2026 - 11 - 22));
    let plan = &preview.plan;

    assert!(plan.closure_complete);
    assert!(!plan.safe_to_apply);
    assert!(!plan.applied);
    assert_eq!(
        plan.primary_reason.as_deref(),
        Some("BLOCK_DEFERRED_DEPENDENCY_CONFLICT")
    );
    assert!(!plan.reasons.iter().any(|item| item == "NO_SAFE_BLOCK"));
    assert!(plan.impact_closure.iter().any(|node| {
        node.node == "host:notebook.google.com"
            && node.role == parallaxe_core::contracts::ClosureRole::Associated
    }));
    assert!(plan
        .affected_shared_paths
        .iter()
        .any(|item| item.contains("notebook.google.com")));
    assert!(guard::render_rule_preview(plan).is_none());
    assert_eq!(preview.audit.after.rules_written, 0);
    assert!(!preview.audit.applied);
    assert_eq!(preview.audit.result, "PREVIEW_REJECTED");
    assert_eq!(preview.audit.rollback.status, "NOT_REQUIRED");
    assert!(!preview.audit.rollback.required);
    assert_eq!(preview.audit.schema, guard::GUARD_AUDIT_SCHEMA);
}

#[test]
fn protected_candidates_are_rejected_before_closure() {
    let registry = Registry::embedded().unwrap();
    let targets = vec!["google.drive".to_string()];
    let results = vec![result_for(&registry, "google.drive", State::Blocked)];
    let preview = preview_for(&registry, &targets, &results, date!(2026 - 11 - 22));
    assert!(preview.plan.candidates.is_empty());
    assert!(!preview.plan.closure_complete);
    assert_eq!(preview.audit.selection.rejected[0].reason, "PROTECTED_HOST");
}

#[test]
fn shared_host_stops_at_dependency_conflict() {
    let registry = Registry::embedded().unwrap();
    let targets = vec!["google.gemini".to_string()];
    let results = vec![result_for(&registry, "google.gemini", State::Blocked)];
    let preview = preview_for(&registry, &targets, &results, date!(2026 - 11 - 22));
    let plan = &preview.plan;

    assert!(plan.closure_complete);
    assert_eq!(
        plan.reasons,
        vec!["BLOCK_DEFERRED_DEPENDENCY_CONFLICT".to_string()]
    );
    assert_eq!(
        plan.primary_reason.as_deref(),
        Some("BLOCK_DEFERRED_DEPENDENCY_CONFLICT")
    );
    assert!(!plan.safe_to_apply);
    assert!(plan
        .impact_closure
        .iter()
        .any(|node| node.node == "host:support.google.com"));
}

#[test]
fn user_imported_targets_are_hard_protected_and_cannot_be_guard_candidates() {
    let mut registry = Registry::embedded().unwrap();
    let meta = registry
        .import_user_target(
            parallaxe_core::targets::UserTargetImportInput {
                id: Some("guard-cdn".to_string()),
                url: "https://gstatic.com/guard-cdn-test".to_string(),
                surface: Some("stage4-test".to_string()),
                signals: vec!["rule.http".to_string()],
                allow_insecure_http: false,
                user_group: None,
            },
            true,
        )
        .expect("导入测试目标");
    let targets = vec![meta.id.clone()];
    let results = vec![result_for(&registry, &meta.id, State::Blocked)];
    let preview = preview_for(&registry, &targets, &results, date!(2026 - 11 - 22));
    let plan = &preview.plan;

    assert!(plan.candidates.is_empty());
    assert!(!plan.safe_to_apply);
    assert!(!plan.closure_complete);
    assert_eq!(preview.audit.selection.rejected[0].reason, "PROTECTED_HOST");
}

#[test]
fn missing_or_interrupted_source_never_claims_complete_closure() {
    let registry = Registry::embedded().unwrap();
    let targets = vec!["google.gemini".to_string()];
    let preview = preview_for(&registry, &targets, &[], date!(2026 - 11 - 22));
    assert!(!preview.plan.closure_complete);
    assert!(!preview.plan.safe_to_apply);
    assert_eq!(preview.audit.selection.rejected.len(), 1);
    assert_eq!(preview.audit.selection.rejected[0].reason, "INCONCLUSIVE");

    let results = vec![result_for(&registry, "google.gemini", State::Restricted)];
    let interrupted = guard::plan_preview(
        &registry,
        GuardRequest {
            run_id: "run-guard-test",
            source_run_id: None,
            targets: &targets,
            results: &results,
            recorded_at: "2026-11-22T09:52:00Z".to_string(),
            as_of: date!(2026 - 11 - 22),
            source_interrupted: true,
        },
    );
    assert!(!interrupted.plan.closure_complete);
    assert!(!interrupted.plan.safe_to_apply);
}

#[test]
fn stale_dependency_facts_invalidate_the_closure() {
    let registry = Registry::embedded().unwrap();
    let targets = vec!["google.notebook".to_string()];
    let results = vec![result_for(&registry, "google.notebook", State::Restricted)];
    let preview = preview_for(&registry, &targets, &results, date!(2028 - 01 - 01));
    assert!(!preview.plan.closure_complete);
    assert!(!preview.plan.safe_to_apply);
}

#[test]
fn closure_output_is_deterministic_and_applied_is_always_false() {
    let registry = Registry::embedded().unwrap();
    let targets = vec!["google.notebook".to_string(), "google.gemini".to_string()];
    let results = vec![
        result_for(&registry, "google.notebook", State::Restricted),
        result_for(&registry, "google.gemini", State::Blocked),
    ];
    let first = preview_for(&registry, &targets, &results, date!(2026 - 11 - 22));
    let second = preview_for(&registry, &targets, &results, date!(2026 - 11 - 22));
    let a: Vec<(String, String)> = first
        .plan
        .impact_closure
        .iter()
        .map(|node| (node.node.clone(), node.kind.clone()))
        .collect();
    let b: Vec<(String, String)> = second
        .plan
        .impact_closure
        .iter()
        .map(|node| (node.node.clone(), node.kind.clone()))
        .collect();
    assert_eq!(a, b);
    assert_eq!(first.plan.reasons, second.plan.reasons);
    assert!(!first.plan.applied);
    assert!(!first.plan.safe_to_apply);
    assert!(first
        .plan
        .limitations
        .iter()
        .any(|item| item.contains("apply")));
}

#[test]
fn shared_entry_host_includes_unselected_service_and_its_dependencies() {
    let mut registry = Registry::embedded().unwrap();
    // Only the sibling depends on the CDN. It must still veto the selected target.
    let mut edge = registry.dependencies.edges[0].clone();
    edge.src = "svc:google.flow".into();
    edge.dst = "host:dl.google.com".into();
    registry.dependencies.edges.push(edge);
    let targets = ["google.labs".to_string()];
    let results = [result_for(&registry, &targets[0], State::Restricted)];
    let preview = preview_for(&registry, &targets, &results, date!(2026 - 11 - 22));
    assert_eq!(preview.plan.candidates, targets);
    assert!(preview
        .plan
        .impact_closure
        .iter()
        .any(|node| node.node == "svc:google.flow"
            && node.via.as_deref() == Some("host:labs.google")
            && node.role == parallaxe_core::contracts::ClosureRole::Affected));
    assert_eq!(
        preview.plan.reasons,
        [
            "GUARD_DEFERRED_SHARED_CDN",
            "BLOCK_DEFERRED_DEPENDENCY_CONFLICT"
        ]
    );
    assert_eq!(preview.audit.after.rules_written, 0);
}

#[test]
fn source_report_cannot_override_catalog_protection_or_confidence_floor() {
    let registry = Registry::embedded().unwrap();
    let mut protected = result_for(&registry, "google.drive", State::Restricted);
    protected.local_policy.protected = false;
    protected.local_policy.guard_eligible = true;
    let mut weak = result_for(&registry, "google.labs", State::Blocked);
    weak.confidence = Confidence::Low;
    let mut unknown = result_for(&registry, "google.labs", State::Blocked);
    unknown.target = "absent-from-catalog".into();
    let results = [protected, weak, unknown];
    let targets: Vec<_> = results.iter().map(|r| r.target.clone()).collect();
    let preview = preview_for(&registry, &targets, &results, date!(2026 - 11 - 22));
    assert!(preview.audit.selection.candidates.is_empty());
    assert_eq!(preview.audit.selection.rejected.len(), 3);
    assert!(!preview.plan.closure_complete);
}

#[test]
fn dependency_dates_exclude_tomorrow_and_include_today() {
    let registry = Registry::embedded().unwrap();
    let targets = ["google.notebook".to_string()];
    let results = [result_for(&registry, &targets[0], State::Restricted)];
    assert!(
        !preview_for(&registry, &targets, &results, date!(2026 - 11 - 20))
            .plan
            .closure_complete
    );
    assert!(
        preview_for(&registry, &targets, &results, date!(2026 - 11 - 21))
            .plan
            .closure_complete
    );
}
