//! 阶段 4 · OPT-1 流式取消与逐事件回传。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use parallaxe_core::contracts::{
    reason, Attempt, AttemptFamily, ObservedFamily, State, Termination,
};
use parallaxe_core::probe::{BoxFuture, FetchOutcome, FetchRequest, Transport};
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::Registry;
use tokio::sync::{mpsc, watch};

struct SlowTransport {
    delay: Duration,
}

impl Transport for SlowTransport {
    fn allows_synthetic_admission(&self) -> bool {
        true
    }

    fn fetch<'a>(&'a self, request: FetchRequest) -> BoxFuture<'a, FetchOutcome> {
        let delay = self.delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            FetchOutcome {
                status: Some(200),
                content_type: Some("text/html".to_string()),
                location: None,
                body: Vec::new(),
                complete: true,
                truncated: false,
                attempts: vec![Attempt {
                    family: AttemptFamily::V4,
                    phase: "HTTP_RESPONSE".to_string(),
                    connect_ms: Some(1),
                    first_byte_ms: Some(2),
                    http_status: Some(200),
                    fallback: false,
                    request_sent: true,
                }],
                request_count: if request.method == "GET" { 1 } else { 0 },
                error: None,
                observed_family: Some(ObservedFamily::V4),
            }
        })
    }
}

fn event_names(events: &[parallaxe_core::contracts::Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| event.event.as_str().to_string())
        .collect()
}

#[tokio::test]
async fn cancel_stops_pending_targets_and_emits_run_cancelled() {
    let registry = Registry::embedded().unwrap();
    let options = CheckOptions {
        targets: vec!["google.drive".to_string(), "google.gemini".to_string()],
        run_id: Some("run-cancel-test".to_string()),
        ..Default::default()
    };
    let admitted: HashSet<String> = options.targets.iter().cloned().collect();
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        runtime::run_check_streaming(
            &registry,
            &options,
            Some(Arc::new(SlowTransport {
                delay: Duration::from_millis(400),
            })),
            false,
            Some(admitted),
            cancel_rx,
            Some(event_tx),
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(40)).await;
    cancel_tx.send(true).unwrap();
    let output = handle.await.unwrap().unwrap();

    assert_eq!(output.report.termination, Termination::Cancelled);
    assert_eq!(output.report.network_request_count, 0);
    assert_eq!(output.report.exit_code, 3);
    assert!(!output.report.results.is_empty());
    for result in &output.report.results {
        assert_eq!(result.state, State::Unknown);
        assert_eq!(result.reason.as_deref(), Some(reason::CANCELLED));
    }
    let names = event_names(&output.events);
    assert_eq!(names.first().map(String::as_str), Some("RunStarted"));
    assert_eq!(names.last().map(String::as_str), Some("RunCancelled"));
    assert!(!names.iter().any(|name| name == "RunFinished"));
    for (index, event) in output.events.iter().enumerate() {
        assert_eq!(event.seq, index as u64 + 1);
    }

    let mut streamed = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        streamed.push(event);
    }
    assert_eq!(streamed.len(), output.events.len());
    assert_eq!(
        serde_json::to_value(&streamed).unwrap(),
        serde_json::to_value(&output.events).unwrap()
    );
}

#[tokio::test]
async fn streaming_matches_the_non_streaming_wrapper_on_fixtures() {
    let registry = Registry::embedded().unwrap();
    let options = CheckOptions {
        fixture: Some("gemini-positive".to_string()),
        run_id: Some("run-stream-fixture".to_string()),
        ..Default::default()
    };
    let baseline = runtime::run_check(&registry, &options, None, false, None)
        .await
        .unwrap();
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let streamed = runtime::run_check_streaming(
        &registry,
        &options,
        None,
        false,
        None,
        cancel_rx,
        Some(event_tx),
    )
    .await
    .unwrap();
    assert_eq!(baseline.events.len(), streamed.events.len());
    for (left, right) in baseline.events.iter().zip(streamed.events.iter()) {
        assert_eq!(left.event, right.event);
        assert_eq!(left.seq, right.seq);
        assert_eq!(left.run_id, right.run_id);
    }
    let mut pushed = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        pushed.push(event);
    }
    assert_eq!(pushed.len(), streamed.events.len());
    assert_eq!(streamed.report.network_request_count, 0);
}

#[tokio::test]
async fn uncancelled_streaming_run_finishes_normally() {
    let registry = Registry::embedded().unwrap();
    let options = CheckOptions {
        targets: vec!["google.gemini".to_string()],
        run_id: Some("run-stream-complete".to_string()),
        ..Default::default()
    };
    let admitted: HashSet<String> = options.targets.iter().cloned().collect();
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let output = runtime::run_check_streaming(
        &registry,
        &options,
        Some(Arc::new(SlowTransport {
            delay: Duration::from_millis(10),
        })),
        false,
        Some(admitted),
        cancel_rx,
        Some(event_tx),
    )
    .await
    .unwrap();
    assert_eq!(output.report.termination, Termination::Complete);
    assert_eq!(output.report.results.len(), 1);
    assert_eq!(
        event_names(&output.events).last().map(String::as_str),
        Some("RunFinished")
    );
    let mut streamed = 0;
    while event_rx.try_recv().is_ok() {
        streamed += 1;
    }
    assert_eq!(streamed, output.events.len());
}
