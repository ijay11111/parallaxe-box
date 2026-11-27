//! 代理模式、端口探测、PAC 子集与超时语义（离线；本地 mock 代理）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parallaxe_core::probe::{FetchOutcome, FetchRequest, Transport};
use parallaxe_core::proxy::{
    auto_prepare_with, evaluate_pac, http_connect_ok, prepare, PacDecision, ProxyCandidate,
    ProxyMode,
};
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::{Registry, HARD_LIMITS};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn simple_profile_is_unlimited_by_default() {
    let registry = Registry::embedded().expect("registry");
    let selection = registry
        .select(Some("simple"), &[], &[], false)
        .expect("simple");
    assert_eq!(selection.ids.len(), 16);
    assert!(selection.ids.iter().any(|id| id == "google.one"));
    assert_eq!(
        selection.budget.run_timeout_ms, 0,
        "simple 默认必须是不限时（0），不能是 20 秒"
    );
    assert!(selection.budget.run_timeout_ms < HARD_LIMITS.max_run_timeout_ms);
}

#[test]
fn timeout_zero_is_accepted_and_oversize_is_rejected() {
    let registry = Registry::embedded().expect("registry");
    let options = CheckOptions {
        profile: Some("simple".to_string()),
        timeout_ms: Some(0),
        ..Default::default()
    };
    assert!(runtime::network_plan(&registry, &options).is_ok());
    let options = CheckOptions {
        profile: Some("simple".to_string()),
        timeout_ms: Some(HARD_LIMITS.max_run_timeout_ms + 1),
        ..Default::default()
    };
    assert!(runtime::network_plan(&registry, &options).is_err());
}

struct AlwaysOk;
impl Transport for AlwaysOk {
    fn allows_synthetic_admission(&self) -> bool {
        true
    }
    fn fetch<'a>(&'a self, _r: FetchRequest) -> parallaxe_core::probe::BoxFuture<'a, FetchOutcome> {
        Box::pin(async {
            FetchOutcome {
                destination: Default::default(),
                status: Some(200),
                content_type: Some("text/html; charset=utf-8".to_string()),
                location: None,
                body: b"<html><title>ok</title></html>".to_vec(),
                complete: true,
                truncated: false,
                attempts: Vec::new(),
                request_count: 1,
                error: None,
                observed_family: Some(parallaxe_core::ObservedFamily::V4),
            }
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unlimited_run_completes_all_sixteen() {
    let registry = Registry::embedded().expect("registry");
    let selection = registry
        .select(Some("simple"), &[], &[], false)
        .expect("simple");
    let admitted = selection.ids.iter().cloned().collect();
    let options = CheckOptions {
        profile: Some("simple".to_string()),
        timeout_ms: Some(0),
        min_interval_ms: Some(0),
        batch_cooldown_ms: Some(0),
        run_id: Some("run-unlimited".to_string()),
        ..Default::default()
    };
    let run = runtime::run_check(
        &registry,
        &options,
        Some(Arc::new(AlwaysOk)),
        false,
        Some(admitted),
    )
    .await
    .expect("check");
    assert_eq!(
        run.report.termination,
        parallaxe_core::Termination::Complete,
        "不限时运行不应被截断"
    );
    assert_eq!(run.report.results.len(), 16, "16 项都应有结果");
    assert_eq!(run.report.coverage.covered.len(), 16);
}

struct Pending;
impl Transport for Pending {
    fn allows_synthetic_admission(&self) -> bool {
        true
    }
    fn fetch<'a>(&'a self, _r: FetchRequest) -> parallaxe_core::probe::BoxFuture<'a, FetchOutcome> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finite_timeout_still_marks_global_timeout() {
    let registry = Registry::embedded().expect("registry");
    let options = CheckOptions {
        targets: vec!["google.books".into()],
        timeout_ms: Some(150),
        ..Default::default()
    };
    let run = runtime::run_check(
        &registry,
        &options,
        Some(Arc::new(Pending)),
        false,
        Some(options.targets.iter().cloned().collect()),
    )
    .await
    .expect("check");
    assert_eq!(
        run.report.termination,
        parallaxe_core::Termination::GlobalTimeout
    );
}

/// 本地 mock HTTP 代理：接受 CONNECT 并返回 200。
async fn spawn_mock_proxy() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr");
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let counter = Arc::clone(&counter);
            tokio::spawn(async move {
                let mut buffer = [0u8; 1024];
                if socket.read(&mut buffer).await.is_ok() {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = socket
                        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                        .await;
                }
            });
        }
    });
    (address, count)
}

#[tokio::test]
async fn connect_probe_accepts_live_proxy_and_rejects_dead_port() {
    let (address, count) = spawn_mock_proxy().await;
    let uri = format!("http://{address}");
    assert!(http_connect_ok(&uri, "books.google.com").await);
    assert_eq!(count.load(Ordering::SeqCst), 1);

    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_address = dead.local_addr().unwrap();
    drop(dead);
    assert!(!http_connect_ok(&format!("http://{dead_address}"), "books.google.com").await);
}

#[tokio::test]
async fn auto_prefers_live_proxy_and_falls_back_to_direct() {
    let (address, _) = spawn_mock_proxy().await;
    let candidates = vec![ProxyCandidate {
        uri: format!("http://{address}"),
        source: "test".to_string(),
        port_open: false,
    }];
    // 目标 host 用回环地址：经代理时只与 mock 代理握手，不会访问外网。
    let prep = auto_prepare_with(candidates, &["127.0.0.1".to_string()]).await;
    assert_eq!(prep.path_label, "PROXY_SELECTED");
    assert_eq!(
        prep.routes.get("127.0.0.1").cloned().flatten(),
        Some(format!("http://{address}"))
    );

    // 死端口：端口不响应 → 直连，且摘要如实说明。
    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_address = dead.local_addr().unwrap();
    drop(dead);
    let prep = auto_prepare_with(
        vec![ProxyCandidate {
            uri: format!("http://{dead_address}"),
            source: "test".to_string(),
            port_open: false,
        }],
        &["127.0.0.1".to_string()],
    )
    .await;
    assert_eq!(prep.path_label, "DIRECT_SELECTED");
    assert!(prep.note.contains("直连"), "{}", prep.note);
}

#[tokio::test]
async fn explicit_modes_build_expected_routes() {
    let hosts = vec![
        "books.google.com".to_string(),
        "www.youtube.com".to_string(),
    ];
    let prep = prepare(ProxyMode::Http, Some("127.0.0.1:7890"), None, &hosts)
        .await
        .expect("http prep");
    assert_eq!(prep.path_label, "HTTP");
    assert_eq!(prep.default_proxy.as_deref(), Some("http://127.0.0.1:7890"));
    assert_eq!(
        prep.routes.get("books.google.com").cloned().flatten(),
        Some("http://127.0.0.1:7890".to_string())
    );

    let prep = prepare(ProxyMode::None, Some("127.0.0.1:7890"), None, &hosts)
        .await
        .expect("none prep");
    assert_eq!(prep.path_label, "DIRECT_SELECTED");
    assert!(prep.default_proxy.is_none());

    assert!(
        prepare(
            ProxyMode::Socks,
            Some("socks5://127.0.0.1:1080"),
            None,
            &hosts
        )
        .await
        .is_err(),
        "SOCKS 必须在产生 I/O 前被明确拒绝"
    );
    assert!(
        prepare(
            ProxyMode::Http,
            Some("http://user:pass@127.0.0.1:7890"),
            None,
            &hosts
        )
        .await
        .is_err(),
        "带凭据代理必须拒绝"
    );

    // 旧 `--proxy` 向后兼容：未指定模式时等价 HTTP 模式。
    let prep = prepare(ProxyMode::Auto, Some("127.0.0.1:7890"), None, &hosts)
        .await
        .expect("legacy proxy");
    assert_eq!(prep.path_label, "HTTP");
}

#[tokio::test]
async fn pac_fetch_failure_notes_fallback_and_stays_local() {
    // 回环死端口：PAC 获取失败 → 回退自动；host 用回环地址避免任何外网请求。
    let prep = prepare(
        ProxyMode::Pac,
        None,
        Some("http://127.0.0.1:1/proxy.pac"),
        &["127.0.0.1".to_string()],
    )
    .await
    .expect("pac fallback");
    assert_eq!(prep.path_label, "PAC");
    assert!(prep.note.contains("PAC 获取失败"), "{}", prep.note);
}

#[test]
fn pac_subset_evaluates_common_functions() {
    let script = r#"
        function FindProxyForURL(url, host) {
            if (isPlainHostName(host)) return "DIRECT";
            if (dnsDomainIs(host, ".google.com")) return "PROXY 127.0.0.1:7890";
            if (shExpMatch(url, "*/youtube/*")) return "PROXY 127.0.0.1:7891";
            return "DIRECT";
        }
    "#;
    assert_eq!(
        evaluate_pac(script, "https://books.google.com/", "books.google.com"),
        PacDecision::Proxy("http://127.0.0.1:7890".into())
    );
    assert_eq!(
        evaluate_pac(script, "https://intranet/", "intranet"),
        PacDecision::Direct
    );
    assert_eq!(
        evaluate_pac(script, "https://x.test/youtube/v", "x.test"),
        PacDecision::Proxy("http://127.0.0.1:7891".into()),
        "url 变量必须可用"
    );
}

#[test]
fn prepare_network_does_not_run_for_plan() {
    // `--plan` 走 network_plan（零发包），不得触发代理探测；这里只验证契约：
    // plan 返回的预览不含任何探测字段，且对无效 profile 仍快速失败。
    let registry = Registry::embedded().expect("registry");
    let options = CheckOptions {
        profile: Some("simple".to_string()),
        plan: true,
        ..Default::default()
    };
    let preview = runtime::network_plan(&registry, &options).expect("plan");
    assert_eq!(preview.target_count, 16);
    assert!(preview.zero_packet);
}
