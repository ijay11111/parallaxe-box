use parallaxe_core::{
    destination::{AddressBook, Comparison, Destination},
    fixtures, linkcheck, reports,
    runtime::{self, CheckOptions},
    signals, Locale, Registry, State,
};

#[tokio::test]
async fn socket_observations_are_risks_and_disclose_only_on_request() {
    let registry = Registry::embedded().unwrap();
    for (id, proxied) in [("address-mismatch", false), ("address-proxy", true)] {
        let mut report = runtime::run_check(
            &registry,
            &CheckOptions {
                fixture: Some(id.into()),
                ..Default::default()
            },
            None,
            false,
            None,
        )
        .await
        .unwrap()
        .report;
        assert_eq!(report.network_request_count, 0);
        let r = &report.results[0];
        let d = &r.destination;
        assert_eq!(r.state, State::Ok);
        assert_eq!(d.peer_country.as_deref(), Some("CN"));
        assert_eq!(d.peer_asn, Some(64497));
        for marker in ["ADDRESS_MISMATCH", "UNEXPECTED_REGION"] {
            assert_eq!(r.risk_signal.markers.iter().any(|s| s == marker), !proxied);
        }
        if proxied {
            assert_eq!(d.region_consistency, Comparison::ProxyNotComparable);
        } else {
            assert!(d.resolved_countries.contains("US"));
        }
        for show in [false, true, false] {
            report.results[0].destination.set_show_ip(show);
            for text in [
                reports::to_json(&report),
                reports::to_csv(&report),
                reports::to_markdown(&report, Locale::EnUs),
                reports::to_html(&report),
            ] {
                assert_eq!(text.contains("203.0.113.9"), show);
                assert_eq!(text.contains("192.0.2.8"), show && !proxied);
                for token in [
                    "peer_role",
                    "peer_address",
                    "peer_asn",
                    "region_consistency",
                    "64497",
                ] {
                    assert!(text.contains(token), "missing {token}");
                }
            }
            let link = linkcheck::analyze_with_ip(&registry, &report, "test".into(), show);
            assert_eq!(
                link.rows[0].connection_edge.peer_address.as_deref(),
                show.then_some("203.0.113.9")
            );
            assert!(link.rows[0].http_egress.observed_country.is_none());
        }
    }
}

#[test]
fn compares_all_candidates_and_longest_prefix_for_both_families() {
    let book = AddressBook::parse(
        "2001:db8::/32,US,64496\n2001:db8:1::/48,CN,64497\n192.0.2.0/24,GB,64498",
    )
    .unwrap();
    let a = "2001:db8:1::1".parse().unwrap();
    let b = "192.0.2.3".parse().unwrap();
    let d = Destination::observe(vec![b, a, b], Some(a), false, &book, Some("CN"));
    assert_eq!(d.resolved_count, 2);
    assert_eq!(d.peer_asn, Some(64497));
    assert_eq!(d.address_consistency, Comparison::Match);
    assert_eq!(d.region_consistency, Comparison::Match);
    let missing = Destination::observe(vec![a], None, false, &book, Some("US"));
    assert_eq!(missing.markers().count(), 0);
    assert!(!missing.peer_observed);
    assert_eq!(
        missing.peer_role,
        parallaxe_core::destination::PeerRole::NotObserved
    );
    // IPv4-mapped IPv6 socket peer canonicalizes to IPv4 so it matches DNS + CIDR without false mismatch.
    let mapped_peer: std::net::IpAddr = "::ffff:192.0.2.3".parse().unwrap();
    let canonical = Destination::observe(vec![b], Some(mapped_peer), false, &book, Some("GB"));
    assert_eq!(canonical.address_consistency, Comparison::Match);
    assert_eq!(canonical.region_consistency, Comparison::Match);
    assert_eq!(canonical.peer_country.as_deref(), Some("GB"));
    assert_eq!(canonical.peer_asn, Some(64498));
    assert_eq!(canonical.markers().count(), 0);
    assert!(AddressBook::parse("192.0.2.0/24,US,1\n192.0.2.1/24,US,2").is_err());
}

#[tokio::test]
async fn autonomous_report_origin_survives_all_export_formats() {
    let registry = Registry::embedded().unwrap();
    let mut report = runtime::run_check(
        &registry,
        &CheckOptions {
            fixture: Some("address-mismatch".into()),
            ..Default::default()
        },
        None,
        false,
        None,
    )
    .await
    .unwrap()
    .report;
    report.origin = Some(parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS.into());
    report.autonomous_mode = Some(true);
    for text in [
        reports::to_json(&report),
        reports::to_csv(&report),
        reports::to_markdown(&report, Locale::ZhCn),
        reports::to_text(&report, Locale::EnUs),
        reports::to_html(&report),
    ] {
        assert!(
            text.contains("AGENT_AUTONOMOUS"),
            "export missing AGENT_AUTONOMOUS audit tag"
        );
    }
    let html = reports::to_html(&report);
    assert!(
        html.contains("class=\"warn\""),
        "HTML export should visually highlight address mismatch cells"
    );
}

#[tokio::test]
async fn explicit_product_decision_has_its_own_evidence_and_confidence() {
    let registry = Registry::embedded().unwrap();
    for (id, state) in [
        ("product-blocked", State::Blocked),
        ("product-restricted", State::Restricted),
    ] {
        let output = runtime::run_check(
            &registry,
            &CheckOptions {
                fixture: Some(id.into()),
                ..Default::default()
            },
            None,
            false,
            None,
        )
        .await
        .unwrap();
        let r = &output.report.results[0];
        assert_eq!(r.state, state);
        assert_eq!(output.report.exit_code, 2);
        assert_eq!(output.report.network_request_count, 0);
        let proof = r
            .evidence
            .iter()
            .find(|e| e.claim == "service.policy_decision")
            .unwrap();
        assert_eq!(proof.confidence, r.confidence);
        assert_eq!(proof.value.as_str(), r.reason.as_deref());
        assert!(proof.fixture);
        assert_eq!(r.local_policy.guard_eligible, !r.local_policy.protected);
        assert!(r.service_observed.observed_country.is_none());
    }
}

#[test]
fn quotes_other_products_partial_responses_and_stop_pages_are_not_policy_evidence() {
    let reg = Registry::embedded().unwrap();
    let case = fixtures::find("product-restricted").unwrap();
    let check = |id: &str, s: &signals::Sample| {
        signals::evaluate(&reg, reg.get(id).unwrap(), s, "path", "now", true)
    };
    let has_policy = |e: signals::Evaluation| {
        e.evidence
            .iter()
            .any(|e| e.claim == "service.policy_decision")
    };
    assert!(!has_policy(check("google.drive", &case.sample)));
    for status in [200, 302, 401, 403, 429, 503] {
        let mut sample = case.sample.clone();
        sample.status = Some(status);
        let e = check(case.target_id, &sample);
        if matches!(status, 401 | 403 | 429) {
            assert!(e.stop.is_some());
        }
        assert!(!has_policy(e));
    }
    let mut sample = case.sample.clone();
    sample.truncated = true;
    assert!(!has_policy(check(case.target_id, &sample)));
    sample = case.sample;
    sample.complete = false;
    assert!(!has_policy(check(case.target_id, &sample)));
    let mut html = fixtures::find("gemini-positive").unwrap().sample;
    html.body =
        b"<html><body><main><h1>Gemini is not available in your country.</h1></main></body></html>"
            .to_vec();
    assert!(has_policy(check("google.gemini", &html)));
    html.body
        .splice(6..6, b"<script>example</script>".iter().copied());
    assert!(!has_policy(check("google.gemini", &html)));
}

#[test]
fn product_contract_for_an_import_uses_the_full_url() {
    use parallaxe_core::targets::UserTargetImportInput;
    let mut registry = Registry::embedded().unwrap();
    let sample = fixtures::find("product-restricted").unwrap().sample;
    for (id, url, expected) in [
        (
            "user.product",
            "https://generativelanguage.googleapis.com/v1/models",
            true,
        ),
        (
            "user.other",
            "https://generativelanguage.googleapis.com/other",
            false,
        ),
    ] {
        registry
            .import_user_target(
                UserTargetImportInput {
                    id: Some(id.into()),
                    url: url.into(),
                    surface: Some("public".into()),
                    signals: vec!["rule.http".into()],
                    allow_insecure_http: false,
                    user_group: None,
                },
                true,
            )
            .unwrap();
        let ev = signals::evaluate(
            &registry,
            registry.get(id).unwrap(),
            &sample,
            "direct",
            "now",
            true,
        );
        assert_eq!(
            ev.evidence
                .iter()
                .any(|e| e.claim == "service.policy_decision"),
            expected
        );
    }
}
