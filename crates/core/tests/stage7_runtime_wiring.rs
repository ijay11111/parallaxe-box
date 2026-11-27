//! Offline end-to-end: the real admission, batch queue, verdict and terminal report.
use parallaxe_core::{
    contracts::{Mode, ObservationScope, Report, State},
    forensics::ProbeProtocol,
    probe::{
        BoxFuture, FetchOutcome, FetchRequest, QosControl, Transport, TransportError,
        TransportErrorKind,
    },
    reports,
    runtime::{run_check, CheckOptions},
    targets::Registry,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
struct Replay {
    udp: AtomicUsize,
    body: &'static str,
}
impl Transport for Replay {
    fn allows_synthetic_admission(&self) -> bool {
        true
    }
    fn binding(&self, id: &str) -> (ProbeProtocol, Option<String>) {
        if id == "google.gemini" {
            (ProbeProtocol::TcpHttp, None)
        } else {
            (ProbeProtocol::UdpQuic, Some(id.into()))
        }
    }
    fn fetch<'a>(&'a self, r: FetchRequest) -> BoxFuture<'a, FetchOutcome> {
        Box::pin(async move {
            if r.protocol == ProbeProtocol::UdpQuic {
                let n = self.udp.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                if n < 2 {
                    return FetchOutcome::failure(
                        TransportError {
                            kind: TransportErrorKind::Timeout,
                            stage: "HTTP",
                        },
                        vec![],
                        1,
                    );
                }
            }
            FetchOutcome {
                destination: Default::default(),
                status: Some(200),
                content_type: Some("text/html".into()),
                location: None,
                body: self.body.as_bytes().to_vec(),
                complete: true,
                truncated: false,
                attempts: vec![],
                request_count: 1,
                error: None,
                observed_family: None,
            }
        })
    }
}
#[tokio::test]
async fn running_queue_pauses_then_resumes_without_redial_or_state_override() {
    let reg = Registry::embedded().unwrap();
    let control = QosControl::default();
    let mut pause = control.subscribe();
    let fake = Arc::new(Replay {
        udp: AtomicUsize::new(0),
        body: ",2,1,200,\"USA\"",
    });
    let transport = fake.clone();
    let options = CheckOptions {
        targets: [
            "google.gemini",
            "google.drive",
            "google.news",
            "google.play",
        ]
        .map(String::from)
        .to_vec(),
        qos_control: Some(control.clone()),
        ..Default::default()
    };
    let admitted = options.targets.iter().cloned().collect();
    let job = tokio::spawn(async move {
        run_check(&reg, &options, Some(transport), false, Some(admitted))
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), pause.wait_for(|v| *v))
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    eprintln!(
        "paused=true udp_calls={} job_finished={}",
        fake.udp.load(Ordering::SeqCst),
        job.is_finished()
    );
    assert_eq!(fake.udp.load(Ordering::SeqCst), 2);
    assert!(!job.is_finished());
    assert!(control.resume());
    assert!(!control.resume());
    let out = job.await.unwrap();
    assert_eq!(fake.udp.load(Ordering::SeqCst), 3);
    assert_eq!(out.report.network_request_count, 0);
    assert!(out.report.results.iter().any(|r| r
        .risk_signal
        .markers
        .iter()
        .any(|m| m == "LOCAL_QOS_SUSPECT")));
    let r = out
        .report
        .results
        .iter()
        .find(|r| r.target == "google.gemini")
        .unwrap();
    eprintln!("after_resume udp_calls={} network_request_count={} terminal_markers={:?} gemini_state={:?} contract_markers={:?}", fake.udp.load(Ordering::SeqCst), out.report.network_request_count, out.report.results.iter().flat_map(|r| &r.risk_signal.markers).collect::<Vec<_>>(), r.state, r.risk_signal.markers);
    assert_eq!(r.state, State::Ok);
    assert!(r
        .risk_signal
        .markers
        .iter()
        .any(|m| m == "SERVICE_CONTRACT_STRUCTURE_MISMATCH"));
    assert!(serde_json::to_string(&out.events.last())
        .unwrap()
        .contains("LOCAL_QOS_SUSPECT"));
}
#[test]
fn android_scope_survives_report_and_human_exports() {
    use parallaxe_core::i18n::Locale;
    let mut r = Report::new("v", "time", "id", Mode::Online);
    r.scope = ObservationScope::for_platform("android");
    let doc = reports::to_json(&r);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&doc).unwrap()["scope"],
        "ANDROID_SIMULATED_USER"
    );
    eprintln!(
        "report.scope={}",
        serde_json::from_str::<serde_json::Value>(&doc).unwrap()["scope"]
    );
    let r: Report = serde_json::from_str(&doc).unwrap();
    for text in [
        reports::to_markdown(&r, Locale::ZhCn),
        reports::to_text(&r, Locale::ZhCn),
        reports::to_html(&r),
    ] {
        assert!(text.contains("ANDROID_SIMULATED_USER"));
        assert!(text.contains("非系统级观测"));
    }
    assert_eq!(ObservationScope::for_platform("linux"), None);
}

#[test]
fn explicit_proxy_binding_is_required_for_upstream_protocol_metadata() {
    use parallaxe_core::probe::HttpTransport;
    assert!(HttpTransport::new(None, None)
        .unwrap()
        .with_confirmed_proxy_binding(ProbeProtocol::UdpQuic, None)
        .is_err());
    let proxy = HttpTransport::new(Some("http://127.0.0.1:8080".into()), None)
        .unwrap()
        .with_confirmed_proxy_binding(ProbeProtocol::UdpQuic, Some("confirmed-provider".into()))
        .unwrap();
    assert_eq!(
        proxy.binding("google.news"),
        (ProbeProtocol::UdpQuic, Some("confirmed-provider".into()))
    );
}

#[tokio::test]
async fn wasm_offline_error_maps_to_non_network_unsupported_reason() {
    struct WasmOfflineStub;
    impl Transport for WasmOfflineStub {
        fn allows_synthetic_admission(&self) -> bool {
            true
        }
        fn fetch<'a>(&'a self, r: FetchRequest) -> BoxFuture<'a, FetchOutcome> {
            Box::pin(async move {
                FetchOutcome::failure(
                    TransportError {
                        kind: TransportErrorKind::WasmOffline,
                        stage: "WASM_OFFLINE",
                    },
                    vec![parallaxe_core::contracts::Attempt {
                        family: r.family,
                        phase: "WASM_OFFLINE".to_string(),
                        connect_ms: None,
                        first_byte_ms: None,
                        http_status: None,
                        fallback: false,
                        request_sent: false,
                    }],
                    0,
                )
            })
        }
    }

    assert_eq!(
        TransportErrorKind::WasmOffline.reason(),
        "SCOPE_WASM_OFFLINE"
    );
    let reg = Registry::embedded().unwrap();
    let options = CheckOptions {
        targets: vec!["google.gemini".to_string()],
        ..Default::default()
    };
    let admitted = options.targets.iter().cloned().collect();
    let out = run_check(
        &reg,
        &options,
        Some(Arc::new(WasmOfflineStub)),
        false,
        Some(admitted),
    )
    .await
    .unwrap();
    let rec = out
        .report
        .results
        .iter()
        .find(|r| r.target == "google.gemini")
        .expect("result for google.gemini");
    assert_eq!(rec.state, State::Unsupported);
    assert_ne!(rec.state, State::LocalPathError);
    assert_eq!(rec.reason.as_deref(), Some("SCOPE_WASM_OFFLINE"));
    assert_ne!(rec.reason.as_deref(), Some("TRANSPORT_OTHER"));
    assert_eq!(out.report.exit_code, 3);
    assert_eq!(out.report.network_request_count, 0);
}
