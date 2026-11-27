//! 用合成响应走真实 reader 与 verdict，不解析域名，不给在线传输发许可。
use parallaxe_core::{
    contracts::*,
    fixtures,
    network::PathDiagnostic,
    probe::FetchOutcome,
    signals::{self, Sample},
    state::{FreshnessStore, MemoryStore, NullStore},
    targets::Registry,
    verdict::{self, VerdictInput},
};
use time::OffsetDateTime;

fn result(reg: &Registry, id: &str, sample: &Sample, store: &dyn FreshnessStore) -> ResultRecord {
    let target = reg.get(id).unwrap();
    let path = Path::system("test-path");
    let evaluation = signals::evaluate(
        reg,
        target,
        sample,
        &path.path_id,
        "2026-11-21T00:00:00Z",
        false,
    );
    let outcome = FetchOutcome {
        destination: Default::default(),
        status: sample.status,
        content_type: sample.content_type.clone(),
        location: sample.location.clone(),
        body: sample.body.clone(),
        complete: sample.complete,
        truncated: sample.truncated,
        attempts: vec![],
        request_count: u64::from(sample.status.is_some()),
        error: None,
        observed_family: sample.observed_family,
    };
    verdict::decide(VerdictInput {
        registry: reg,
        target,
        evaluation: &evaluation,
        outcome: &outcome,
        path: &path,
        path_diagnostic: PathDiagnostic::System,
        fixture: false,
        now: OffsetDateTime::UNIX_EPOCH,
        store,
        expectation: None,
        capability_unsupported: None,
        cancelled: false,
        global_timeout: false,
    })
}

#[test]
fn repeated_first_country_cannot_hide_conflict_or_fill_a_layer() {
    let reg = Registry::embedded().unwrap();
    let mut sample = fixtures::find("gemini-positive").unwrap().sample;
    sample.body = ["USA", "USA", "USA", "USA", "JPN", "CHN", "CAN", "DEU"]
        .map(|s| format!(",2,1,200,\"{s}\""))
        .join("")
        .into_bytes();
    let r = result(&reg, "google.gemini", &sample, &NullStore::default());
    assert_eq!(r.state, State::Conflict);
    assert_eq!(r.agreement[0].votes.len(), 4);
    assert!(r.agreement[0].tie && r.agreement[0].winner.is_none());
    assert!(r.region_layers.iter().all(|l| l.value.is_none()));
    assert!(r.service_observed.observed_country.is_none());
}
#[test]
fn response_contract_gates_content_and_missing_counters() {
    let reg = Registry::embedded().unwrap();
    for (status, mime) in [
        (Some(500), "text/html"),
        (Some(200), "text/css"),
        (Some(200), "application/json"),
        (None, "text/html"),
    ] {
        let mut sample = fixtures::find("gemini-positive").unwrap().sample;
        sample.status = status;
        sample.content_type = Some(mime.into());
        let r = result(&reg, "google.gemini", &sample, &NullStore::default());
        assert!(r.evidence.iter().all(|e| e.rule_id != "rule.inline3"));
        assert!(r.service_observed.observed_country.is_none());
        if status.is_none() {
            assert!(
                result(&reg, "google.http204", &sample, &NullStore::default())
                    .evidence
                    .is_empty()
            );
        }
    }
}
#[test]
fn edition_stops_at_login_and_uses_gl_not_language() {
    let reg = Registry::embedded().unwrap();
    let store = NullStore::default();
    let mut sample = fixtures::find("news-edition").unwrap().sample;
    sample.location = Some("/?hl=en-US&gl=JP&ignored=synthetic".into());
    let r = result(&reg, "google.news", &sample, &store);
    assert_eq!(r.region_layers[4].value.as_deref(), Some("JP"));
    assert!(r.service_observed.observed_country.is_none());
    sample.location = Some("//ACCOUNTS.GOOGLE.COM.?gl=JP".into());
    let r = result(&reg, "google.news", &sample, &store);
    assert_eq!(r.reason.as_deref(), Some(reason::REDIRECT_TO_LOGIN));
    assert!(r.region_layers.iter().all(|l| l.value.is_none()));
}
#[test]
fn only_matching_family_and_target_can_accumulate_stale() {
    let reg = Registry::embedded().unwrap();
    let store = MemoryStore::default();
    let mut sample = fixtures::find("gemini-positive").unwrap().sample;
    let hit = result(&reg, "google.gemini", &sample, &store);
    assert_eq!(hit.confidence, Confidence::Low);
    assert!(!hit.service_observed.evidence_ids.is_empty());
    sample.body = b"<html><title>mock</title></html>".to_vec();
    for (id, family) in [
        ("google.gemini", Some(ObservedFamily::V6)),
        ("google.books", Some(ObservedFamily::V4)),
        ("google.gemini", None),
    ] {
        sample.observed_family = family;
        for _ in 0..3 {
            assert_ne!(result(&reg, id, &sample, &store).state, State::SignalStale);
        }
    }
    sample.observed_family = Some(ObservedFamily::V4);
    for n in 1..=3 {
        assert_eq!(
            result(&reg, "google.gemini", &sample, &store).state == State::SignalStale,
            n == 3
        );
    }
}
#[test]
fn scalar_readers_export_only_allowed_values() {
    let reg = Registry::embedded().unwrap();
    let store = NullStore::default();
    let mut sample = fixtures::find("gemini-positive").unwrap().sample;
    sample.content_type = Some("text/plain".into());
    sample.body = b"ip=synthetic-sentinel\nloc=JP\ncolo=NRT\nhttp=not-a-protocol\n".to_vec();
    let r = result(&reg, "control.cf-trace", &sample, &store);
    assert_eq!(r.service_observed.observed_country.as_deref(), Some("JP"));
    assert!(r
        .evidence
        .iter()
        .any(|e| e.claim == "trace.ip_present" && e.value == true));
    let text = serde_json::to_string(&r).unwrap();
    assert!(!text.contains("synthetic-sentinel") && !text.contains("not-a-protocol"));
    sample.content_type = Some("application/json".into());
    sample.body =
        br#"{"country":"US","asn":64512,"colo":{"private":"sentinel"},"extra":"sentinel"}"#
            .to_vec();
    let r = result(&reg, "control.cf-meta", &sample, &store);
    assert_eq!(r.service_observed.observed_country.as_deref(), Some("US"));
    assert!(!serde_json::to_string(&r).unwrap().contains("sentinel"));
    sample.body = b"not-json".to_vec();
    assert!(result(&reg, "control.cf-meta", &sample, &store)
        .evidence
        .iter()
        .all(|e| e.rule_id != "rule.meta"));
}
