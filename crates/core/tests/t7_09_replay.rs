//! T-7-09f：把用户真实案例固化为离线回放（不触网、不改判定语义）。
//!
//! 覆盖：地区 JPN 但 Gemini 超时 / TCP 正常 + UDP 大面积超时（本地 QoS）/
//! IPv6 慢回落 / 自动选节点路线不一致 / 保护域审计。

use parallaxe_core::contracts::{
    Confidence, LocalPolicy, Mode, Path, Report, ResultRecord, RiskSignal, ServiceObserved, State,
    TransportLayerResult,
};
use parallaxe_core::forensics::{
    attach_route_consistency, classify_dual_stack, detect_local_qos, evaluate_contract,
    guard_pause_from_report, route_consistency, BindingRecord, ContractOutcome, ContractResponse,
    DualStackObservation, DualStackState, ObservationPoint, ProbeProtocol, RegionSignalSource,
    RouteConsistency, ServiceContract,
};

fn result(target: &str, state: State) -> ResultRecord {
    ResultRecord {
        destination: Default::default(),
        target: target.to_string(),
        state,
        reason: None,
        confidence: Confidence::Low,
        source: None,
        path: Path::system("path-system"),
        duration_ms: 500,
        request_count: 1,
        evidence: Vec::new(),
        region_layers: parallaxe_core::verdict::all_layers_null(),
        observed: Vec::new(),
        conflicts: Vec::new(),
        agreement: Vec::new(),
        transport: TransportLayerResult {
            attempts: Vec::new(),
            path_diagnostic: None,
        },
        service_observed: ServiceObserved {
            surface: "fixture".to_string(),
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
    }
}

fn report_with(results: Vec<ResultRecord>) -> Report {
    let mut report = Report::new("0.1.0", "2026-11-23T00:00:00Z", "run-replay", Mode::Online);
    report.results = results;
    report
}

#[test]
fn case_region_jpn_but_gemini_timeout_is_not_collapsed() {
    let contract = ServiceContract {
        target_id: "google.gemini".to_string(),
        allowed_final_hosts: vec!["gemini.google.com".to_string()],
        max_redirects: 1,
        expected_status: vec![200],
        allowed_content_types: vec!["text/html".to_string()],
        required_markers: vec!["window.wiz_global_data".to_string()],
        challenge_markers: vec!["unusual traffic".to_string()],
        login_markers: vec!["sign in".to_string()],
        region_source: RegionSignalSource::YouTubeGl,
        max_body_bytes: 4_000_000,
        contract_version: 2,
        verified_at: "2026-11-23".to_string(),
    };
    // 地区信号正常（JPN）但响应结构不完整：结论只保留“地区已观察”，不压缩成正常。
    let response = ContractResponse {
        final_host: "gemini.google.com".to_string(),
        redirects: 0,
        status: 200,
        content_type: Some("text/html".to_string()),
        body: "<html>region is JPN</html>".to_string(),
        region: Some("JPN".to_string()),
        body_truncated: false,
    };
    assert_eq!(
        evaluate_contract(&contract, &response),
        ContractOutcome::StructureMismatch {
            missing: "window.wiz_global_data".to_string()
        }
    );
    // 同时请求超时：报告里两条证据分别存在，没有单一“正常/不可用”。
    let mut record = result("google.gemini", State::ReachableOnly);
    record.service_observed.observed_country = Some("JPN".to_string());
    record.reason = Some("TRANSPORT_TIMEOUT".to_string());
    let report = report_with(vec![record]);
    assert_eq!(report.results[0].state, State::ReachableOnly);
    assert_eq!(
        report.results[0]
            .service_observed
            .observed_country
            .as_deref(),
        Some("JPN")
    );
    assert_eq!(
        report.results[0].reason.as_deref(),
        Some("TRANSPORT_TIMEOUT")
    );
}

#[test]
fn case_tcp_ok_udp_timeouts_pauses_udp_and_marks_local_qos() {
    let mut points = vec![ObservationPoint {
        protocol: ProbeProtocol::TcpHttp,
        node_label: "TCP-CONTROL".to_string(),
        airport: "control".to_string(),
        success: true,
        timed_out: false,
        first_byte_ms: Some(90),
    }];
    for index in 0..6 {
        points.push(ObservationPoint {
            protocol: ProbeProtocol::UdpQuic,
            node_label: format!("udp-{index}"),
            airport: if index < 3 {
                "snow".to_string()
            } else {
                "cyber".to_string()
            },
            success: false,
            timed_out: true,
            first_byte_ms: None,
        });
    }
    let finding = detect_local_qos(&points);
    assert!(finding.suspect_local_qos);
    assert_eq!(finding.paused_protocols, vec![ProbeProtocol::UdpQuic]);
    // 只暂停，不重发：未测节点不出现在观测点里，也没有故障标记。
    assert_eq!(points.len(), 7);
    let mut record = result("control.tcp", State::Ok);
    record.risk_signal.markers = finding.markers.clone();
    let report = report_with(vec![record]);
    assert_eq!(
        guard_pause_from_report(&report).as_deref(),
        Some("LOCAL_QOS_SUSPECT, MULTI_AIRPORT_UDP_FAILURE")
    );
}

#[test]
fn case_ipv6_slow_then_v4_fallback_keeps_first_round() {
    let slow = DualStackObservation {
        v6_attempted: true,
        v6_success: true,
        v6_first_byte_ms: Some(9_500),
        v6_budget_ms: 8_000,
        ..Default::default()
    };
    assert_eq!(classify_dual_stack(&slow), DualStackState::Ipv6Slow);
    // 主动指定地址族复测（v4 成功）不覆盖首轮的 v6 观测。
    let retest = DualStackObservation {
        v4_attempted: true,
        v4_success: true,
        explicit_family_retest: true,
        ..Default::default()
    };
    assert_eq!(classify_dual_stack(&retest), DualStackState::Ipv4Only);
    assert_eq!(classify_dual_stack(&slow), DualStackState::Ipv6Slow);
}

#[test]
fn case_auto_selected_node_switches_marks_route_inconsistent() {
    let base = BindingRecord {
        node_label: "JP-01".to_string(),
        node_id: "node-jp-01".to_string(),
        remote: "jp01.example:443".to_string(),
        protocol: ProbeProtocol::TcpHttp,
        egress_ip: Some("203.0.113.50".to_string()),
        family: None,
        started_at: "t0".to_string(),
        ended_at: Some("t1".to_string()),
    };
    let mut switched = base.clone();
    switched.node_id = "node-jp-02".to_string();
    switched.egress_ip = Some("203.0.113.51".to_string());
    assert_eq!(
        route_consistency(&[base.clone(), switched.clone()]),
        RouteConsistency::Inconsistent { field: "node_id" }
    );
    let mut results = vec![
        result("google.gemini", State::Ok),
        result("google.books", State::Ok),
    ];
    attach_route_consistency(&mut results, &[base, switched]);
    let report = report_with(results);
    assert!(guard_pause_from_report(&report)
        .unwrap_or_default()
        .contains("ROUTE_INCONSISTENT"));
    // 整组不得合并：两份结果都保留，各自带风险标记。
    assert_eq!(report.results.len(), 2);
    assert!(report.results.iter().all(|item| item
        .risk_signal
        .markers
        .contains(&"ROUTE_INCONSISTENT".to_string())));
}

#[test]
fn case_protection_audit_blocks_export_even_with_region_signal() {
    let registry = parallaxe_core::targets::Registry::embedded().unwrap();
    let mut plan = parallaxe_core::GuardPlan {
        plan_id: "plan-protect".to_string(),
        run_id: "run".to_string(),
        selected_by: "USER".to_string(),
        operation: "PREVIEW".to_string(),
        candidates: vec!["google.accounts".to_string()],
        affected_domains: vec!["accounts.google.com".to_string()],
        affected_services: Vec::new(),
        affected_shared_paths: vec!["accounts.google.com (PROTECTED)".to_string()],
        impact_closure: Vec::new(),
        reasons: vec!["PROTECTED_HOST".to_string()],
        primary_reason: Some("PROTECTED_HOST".to_string()),
        safe_to_apply: false,
        closure_complete: true,
        applied: false,
        rollback: Vec::new(),
        limitations: Vec::new(),
        effect_scope: "LOCAL_TRAFFIC_ONLY".to_string(),
        remote_account_impact: "NOT_PROVABLE".to_string(),
        shared_egress_guarantee: "UNPROVEN".to_string(),
        source_run_id: None,
    };
    let refusal =
        parallaxe_core::guard_export::export_rules(parallaxe_core::guard_export::ExportRequest {
            registry: &registry,
            plan: &plan,
            format: parallaxe_core::guard_export::RuleFormat::DomainList,
            mode: parallaxe_core::guard_export::ExportMode::Provider,
            generated_at: "2026-11-23T00:00:00Z".to_string(),
            forensic_pause: None,
        })
        .unwrap_err();
    assert_eq!(refusal.code, "EXPORT_CONFLICT");
    // 地区信号不能绕过保护名单：即使把计划改成区域正常也不导出。
    plan.reasons = vec!["NO_SAFE_BLOCK".to_string()];
    let refusal =
        parallaxe_core::guard_export::export_rules(parallaxe_core::guard_export::ExportRequest {
            registry: &registry,
            plan: &plan,
            format: parallaxe_core::guard_export::RuleFormat::DomainList,
            mode: parallaxe_core::guard_export::ExportMode::Provider,
            generated_at: "2026-11-23T00:00:00Z".to_string(),
            forensic_pause: None,
        })
        .unwrap_err();
    assert_eq!(
        refusal.code, "EXPORT_NO_FQDN",
        "保护目标被排除后没有可导出 FQDN；绝不放宽保护名单"
    );
}
