//! 默认空许可的零调用证明：即使注入计数传输器，也不得发出任何请求。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parallaxe_core::probe::{
    FetchOutcome, FetchRequest, Transport, TransportError, TransportErrorKind,
};
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::Registry;

#[derive(Default)]
struct CountingTransport {
    calls: AtomicUsize,
}

impl Transport for CountingTransport {
    fn fetch<'a>(
        &'a self,
        _request: FetchRequest,
    ) -> parallaxe_core::probe::BoxFuture<'a, FetchOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            FetchOutcome::failure(
                TransportError {
                    kind: TransportErrorKind::Other,
                    stage: "SHOULD_NOT_BE_CALLED",
                },
                Vec::new(),
                1,
            )
        })
    }
}

#[tokio::test]
async fn empty_admission_never_calls_transport() {
    let registry = Registry::embedded().expect("registry");
    let transport = Arc::new(CountingTransport::default());
    let options = CheckOptions {
        profile: Some("google-all".to_string()),
        run_id: Some("run-zero-calls".to_string()),
        ..Default::default()
    };
    let output = runtime::run_check(
        &registry,
        &options,
        Some(Arc::clone(&transport) as Arc<dyn Transport>),
        true,
        None,
    )
    .await
    .expect("check");
    assert_eq!(
        transport.calls.load(Ordering::SeqCst),
        0,
        "空许可下必须是零调用"
    );
    assert_eq!(output.report.network_request_count, 0);
    assert_eq!(output.report.results.len(), 0);
    assert_eq!(output.report.exit_code, 3);
    // 三个前端入口都不得自行判国：事件里没有伪造的 TargetStarted/TargetFinished。
    assert!(!output
        .events
        .iter()
        .any(|event| event.event == parallaxe_core::EventName::TargetStarted));
}

#[tokio::test]
async fn synthetic_admission_requires_explicit_offline_port() {
    let registry = Registry::embedded().unwrap();
    let transport = Arc::new(CountingTransport::default());
    let options = CheckOptions {
        targets: vec!["google.gemini".into()],
        ..Default::default()
    };
    for network in [true, false] {
        assert!(runtime::run_check(
            &registry,
            &options,
            Some(transport.clone()),
            network,
            Some(options.targets.iter().cloned().collect())
        )
        .await
        .is_err());
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

struct PartialBatch;
impl Transport for PartialBatch {
    fn allows_synthetic_admission(&self) -> bool {
        true
    }
    fn fetch<'a>(&'a self, r: FetchRequest) -> parallaxe_core::probe::BoxFuture<'a, FetchOutcome> {
        Box::pin(async move {
            if r.target_id == "google.news" {
                return std::future::pending().await;
            }
            FetchOutcome::failure(
                TransportError {
                    kind: TransportErrorKind::Resolve,
                    stage: "RESOLVE",
                },
                Vec::new(),
                0,
            )
        })
    }
}
#[tokio::test]
async fn timeout_retains_completed_anomaly_and_prioritizes_exit_two() {
    let registry = Registry::embedded().unwrap();
    let options = CheckOptions {
        targets: vec!["google.news".into(), "google.youtube".into()],
        timeout_ms: Some(200),
        ..Default::default()
    };
    let run = runtime::run_check(
        &registry,
        &options,
        Some(Arc::new(PartialBatch)),
        false,
        Some(options.targets.iter().cloned().collect()),
    )
    .await
    .unwrap();
    assert_eq!(
        run.report.termination,
        parallaxe_core::Termination::GlobalTimeout
    );
    assert_eq!(run.report.exit_code, 2);
    assert_eq!(run.report.coverage.covered.len(), 1);
    assert!(run
        .report
        .coverage
        .uncovered
        .iter()
        .any(|u| u.target == "google.news" && u.reason == "GLOBAL_TIMEOUT"));
}
