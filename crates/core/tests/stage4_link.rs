//! 阶段 4 · 链路体检投影：四列分离、未知留空、未测标未测、禁端口扫描。

use parallaxe_core::contracts::{reason, Mode, Report};
use parallaxe_core::linkcheck;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::{Registry, UserTargetImportInput};

#[tokio::test]
async fn fixture_report_projects_four_columns_without_new_packets() {
    let registry = Registry::embedded().unwrap();
    let options = CheckOptions {
        fixture: Some("drive-login".to_string()),
        ..Default::default()
    };
    let output = runtime::run_check(&registry, &options, None, false, None)
        .await
        .unwrap();
    assert_eq!(output.report.network_request_count, 0);
    let link = linkcheck::analyze(
        &registry,
        &output.report,
        "2026-11-22T09:52:00Z".to_string(),
    );
    assert_eq!(link.schema, linkcheck::LINK_SCHEMA);
    assert_eq!(link.source, "FIXTURE");
    assert_eq!(link.dns_query_count, 0);
    assert_eq!(link.http_request_count, 0);
    let row = link
        .rows
        .iter()
        .find(|row| row.target == "google.drive")
        .expect("drive 行");
    assert_eq!(row.host, "support.google.com");
    assert_eq!(row.connection_edge.status, "OBSERVED");
    assert_eq!(row.connection_edge.attempts.len(), 1);
    assert!(row.connection_edge.peer_address.is_none());
    assert_eq!(row.address_family.v4, "OBSERVED_OK");
    assert_eq!(row.address_family.v6, "NOT_OBSERVED");
    assert_eq!(row.resolver.status, "NOT_OBSERVED");
    assert!(row.resolver.addresses.is_empty());
    assert_eq!(row.http_egress.status, "NOT_OBSERVED");
}

#[test]
fn selection_only_report_is_zero_packet_and_marks_unknown_columns() {
    let registry = Registry::embedded().unwrap();
    let link = linkcheck::analyze_selection(
        &registry,
        &["google.gemini".to_string(), "google.books".to_string()],
        "2026-11-22T09:52:00Z".to_string(),
    );
    assert_eq!(link.source, "SELECTION_ONLY");
    assert_eq!(link.rows.len(), 2);
    for row in &link.rows {
        assert_eq!(row.resolver.status, "NOT_OBSERVED");
        assert_eq!(row.connection_edge.status, "NOT_OBSERVED");
        assert_eq!(row.http_egress.status, "NOT_OBSERVED");
        assert_eq!(
            row.http_egress.reason.as_deref(),
            Some(reason::NO_ADMISSION)
        );
        assert_eq!(row.address_family.v4, "NOT_OBSERVED");
    }
    assert_eq!(link.capabilities.quic, "UNTESTED");
    assert_eq!(link.capabilities.dns_leak, "UNTESTED");
    assert_eq!(link.capabilities.port_scan, "PROHIBITED");
    assert_eq!(link.capabilities.dns_compare, "UNSUPPORTED_CAPABILITY");
    assert_eq!(link.dns_query_count, 0);
    assert_eq!(link.http_request_count, 0);
}

#[tokio::test]
async fn resolver_probe_refuses_user_imported_targets_without_querying() {
    let mut registry = Registry::embedded().unwrap();
    let meta = registry
        .import_user_target(
            UserTargetImportInput {
                id: Some("link-user".to_string()),
                url: "https://example.org/link-check".to_string(),
                surface: Some("stage4-link".to_string()),
                signals: vec!["rule.http".to_string()],
                allow_insecure_http: false,
                user_group: None,
            },
            true,
        )
        .unwrap();
    let mut link = linkcheck::analyze_selection(
        &registry,
        std::slice::from_ref(&meta.id),
        "2026-11-22T09:52:00Z".to_string(),
    );
    linkcheck::probe_resolver(&registry, &mut link, false, 100)
        .await
        .unwrap();
    assert_eq!(link.dns_query_count, 0);
    assert_eq!(
        link.rows[0].resolver.reason.as_deref(),
        Some("USER_TARGET_REQUIRES_SESSION_GRANT")
    );
}

#[test]
fn link_report_does_not_leak_addresses_by_default() {
    let registry = Registry::embedded().unwrap();
    let link = linkcheck::analyze_selection(
        &registry,
        &["google.gemini".to_string()],
        "2026-11-22T09:52:00Z".to_string(),
    );
    let json = serde_json::to_string(&link).unwrap();
    assert!(!json.contains("203.0.113.53"));
    assert!(json.contains("\"port_scan\":\"PROHIBITED\""));
    assert!(json.contains("\"quic\":\"UNTESTED\""));
    // 未知就是 null，不写“看起来像已观测”的空字符串。
    assert!(json.contains("\"observed_country\":null"));
}

#[tokio::test]
async fn empty_report_keeps_all_columns_visible() {
    let registry = Registry::embedded().unwrap();
    let report = Report::new(
        parallaxe_core::VERSION,
        "2026-11-22T09:52:00Z",
        "run-empty",
        Mode::Online,
    );
    let link = linkcheck::analyze(&registry, &report, "2026-11-22T09:52:00Z".to_string());
    assert!(link.rows.is_empty());
    assert_eq!(
        link.capabilities.route_get,
        "LOCAL_INTERFACE_OBSERVATION_ONLY"
    );
    assert!(link
        .limitations
        .iter()
        .any(|item| item.contains("端口扫描")));
}

#[tokio::test]
async fn service_policy_and_requested_family_do_not_fill_observation_columns() {
    use parallaxe_core::contracts::{AttemptFamily, ObservedFamily};
    let registry = Registry::embedded().unwrap();
    let options = CheckOptions {
        fixture: Some("gemini-positive".into()),
        ..Default::default()
    };
    let mut report = runtime::run_check(&registry, &options, None, false, None)
        .await
        .unwrap()
        .report;
    assert_eq!(
        report.results[0]
            .service_observed
            .observed_country
            .as_deref(),
        Some("US")
    );
    report.results[0].transport.attempts[0].family = AttemptFamily::Auto;
    for observed in [None, Some(ObservedFamily::V6)] {
        report.results[0].path.observed_family = observed;
        let link = linkcheck::analyze(&registry, &report, report.generated_at.clone());
        let row = &link.rows[0];
        assert_eq!(row.http_egress.status, "NOT_OBSERVED");
        assert!(row.http_egress.observed_country.is_none());
        assert_eq!(
            row.connection_edge.observed_family.as_deref(),
            observed.map(|_| "V6")
        );
        assert_eq!(row.connection_edge.attempts[0].family, "AUTO");
    }
}

#[tokio::test]
async fn uncovered_result_keeps_one_row_and_its_attempts() {
    use parallaxe_core::contracts::Uncovered;
    let registry = Registry::embedded().unwrap();
    let options = CheckOptions {
        fixture: Some("drive-login".into()),
        ..Default::default()
    };
    let mut report = runtime::run_check(&registry, &options, None, false, None)
        .await
        .unwrap()
        .report;
    report.coverage.covered.clear();
    report.coverage.uncovered.push(Uncovered {
        target: "google.drive".into(),
        selected: true,
        reason: "CANCELLED".into(),
    });
    let link = linkcheck::analyze(&registry, &report, report.generated_at.clone());
    assert_eq!(link.rows.len(), 1);
    assert_eq!(link.rows[0].connection_edge.attempts.len(), 1);
}

#[tokio::test]
async fn fixture_reports_reject_live_resolution() {
    let registry = Registry::embedded().unwrap();
    let report = Report::new(
        parallaxe_core::VERSION,
        "2026-11-22T09:52:00Z",
        "offline",
        Mode::Fixture,
    );
    let mut link = linkcheck::analyze(&registry, &report, report.generated_at.clone());
    assert!(linkcheck::probe_resolver(&registry, &mut link, false, 1)
        .await
        .is_err());
    assert_eq!(link.dns_query_count, 0);
}
