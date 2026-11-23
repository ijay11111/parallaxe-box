//! 请求预算与匿名客户端：只用本地 mock server，不触真实网络。

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use parallaxe_core::probe::{run_batch, FetchRequest, HttpTransport, Transport};
use parallaxe_core::targets::{Budget, HARD_LIMITS};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

#[derive(Default)]
struct Stats {
    active: AtomicUsize,
    peak: AtomicUsize,
    requests: AtomicUsize,
    heads: AtomicUsize,
    cookie_empty: AtomicUsize,
    cookie_missing: AtomicUsize,
    authorization: AtomicUsize,
    paths: Mutex<HashMap<String, usize>>,
}

async fn spawn_server(stats: Arc<Stats>) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let stats = Arc::clone(&stats);
            tokio::spawn(async move {
                let active = stats.active.fetch_add(1, Ordering::SeqCst) + 1;
                let mut peak = stats.peak.load(Ordering::SeqCst);
                while peak < active {
                    match stats.peak.compare_exchange(
                        peak,
                        active,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => break,
                        Err(current) => peak = current,
                    }
                }
                let mut buffer = [0u8; 4096];
                let mut request = Vec::new();
                loop {
                    match socket.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(read) => {
                            request.extend_from_slice(&buffer[..read]);
                            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let mut lines = text.lines();
                let request_line = lines.next().unwrap_or_default();
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default();
                let path = parts.next().unwrap_or("/").to_string();
                if method == "HEAD" {
                    stats.heads.fetch_add(1, Ordering::SeqCst);
                }
                let mut cookie_seen = false;
                for line in text.lines() {
                    let lower = line.to_ascii_lowercase();
                    if let Some(value) = lower.strip_prefix("cookie:") {
                        cookie_seen = true;
                        if value.trim().is_empty() {
                            stats.cookie_empty.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    if lower.starts_with("authorization:")
                        || lower.starts_with("proxy-authorization:")
                    {
                        stats.authorization.fetch_add(1, Ordering::SeqCst);
                    }
                }
                if !cookie_seen {
                    stats.cookie_missing.fetch_add(1, Ordering::SeqCst);
                }
                stats.requests.fetch_add(1, Ordering::SeqCst);
                if let Ok(mut paths) = stats.paths.lock() {
                    *paths.entry(path.clone()).or_insert(0) += 1;
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
                let body = "<html><title>mock</title>,2,1,200,\"USA\"</html>";
                let (status, extra) = if path == "/redirect" {
                    ("302 Found", "Location: /not-requested\r\n")
                } else {
                    ("200 OK", "")
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\n{extra}Set-Cookie: fixture=synthetic\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
                stats.active.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    address
}

/// 测试专用：关闭低噪声间隔，避免本地 mock 测试被人为拖慢。
fn fast_budget() -> Budget {
    Budget {
        min_interval_ms: 0,
        batch_cooldown_ms: 0,
        ..Budget::default()
    }
}

fn request(address: std::net::SocketAddr, path: &str) -> FetchRequest {
    FetchRequest {
        target_id: format!("mock{path}"),
        url: format!("http://{address}{path}"),
        host: address.ip().to_string(),
        method: "GET".to_string(),
        family: parallaxe_core::AttemptFamily::Auto,
        protocol: parallaxe_core::forensics::ProbeProtocol::TcpHttp,
        timeout_ms: 2_000,
        max_body_bytes: 16_777_216,
        max_redirects: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn budget_and_anonymity_hold() {
    let stats = Arc::new(Stats::default());
    let address = spawn_server(Arc::clone(&stats)).await;
    let transport: Arc<dyn Transport> =
        Arc::new(HttpTransport::new(None, None).expect("transport"));
    let requests: Vec<FetchRequest> = (0..10)
        .map(|index| request(address, &format!("/p{index}")))
        .collect();
    let budget = Budget {
        max_concurrency: 6,
        per_host: 2,
        ..fast_budget()
    };
    let (_tx, rx) = watch::channel(false);
    let outcomes = run_batch(transport, requests, budget, rx).await;
    assert_eq!(outcomes.len(), 10, "每个目标都应有结果");
    assert!(
        stats.peak.load(Ordering::SeqCst) <= HARD_LIMITS.per_host as usize,
        "同主机并发峰值 {} 超过 2",
        stats.peak.load(Ordering::SeqCst)
    );
    assert_eq!(stats.heads.load(Ordering::SeqCst), 0, "不得发送 HEAD");
    assert_eq!(stats.requests.load(Ordering::SeqCst), 10);
    assert_eq!(
        stats.cookie_missing.load(Ordering::SeqCst),
        0,
        "每个请求都必须带显式空 Cookie 头"
    );
    assert_eq!(
        stats.cookie_empty.load(Ordering::SeqCst),
        10,
        "Cookie 必须为空"
    );
    assert_eq!(
        stats.authorization.load(Ordering::SeqCst),
        0,
        "不得发送 Authorization/Proxy-Authorization"
    );
    for (_, outcome) in &outcomes {
        assert_eq!(outcome.request_count, 1, "单目标一次逻辑 GET");
        assert!(outcome.complete, "样本应完整");
        assert!(
            String::from_utf8_lossy(&outcome.body).contains("USA"),
            "正文应被有界读取"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dedupe_sends_one_request() {
    let stats = Arc::new(Stats::default());
    let address = spawn_server(Arc::clone(&stats)).await;
    let transport: Arc<dyn Transport> =
        Arc::new(HttpTransport::new(None, None).expect("transport"));
    let duplicate = request(address, "/same");
    let mut alias = duplicate.clone();
    alias.target_id = "alias".into();
    let requests = vec![duplicate, alias];
    let (_tx, rx) = watch::channel(false);
    let outcomes = run_batch(transport, requests, fast_budget(), rx).await;
    assert_eq!(outcomes.len(), 2, "重复请求共享同一结果");
    assert_eq!(
        stats.requests.load(Ordering::SeqCst),
        1,
        "去重键相同只发一次"
    );
    assert_eq!(outcomes[0].1.request_count, 1);
    assert_eq!(outcomes[1].0, "alias");
    assert_eq!(outcomes[1].1.request_count, 0);
    assert_eq!(outcomes[1].1.body, outcomes[0].1.body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v6_failure_falls_back_once() {
    let stats = Arc::new(Stats::default());
    let address = spawn_server(Arc::clone(&stats)).await;
    let transport = HttpTransport::new(None, None).expect("transport");
    let mut v6 = request(address, "/fallback");
    v6.family = parallaxe_core::AttemptFamily::V6;
    let outcome = transport.fetch(v6).await;
    assert!(outcome.status.is_some(), "回落 v4 后应获得 HTTP");
    assert_eq!(outcome.request_count, 1, "仅回落那次真正送出 HTTP 请求");
    assert_eq!(outcome.attempts.len(), 2, "主尝试与回落各留一条账");
    assert!(outcome.attempts[1].fallback, "第二次尝试必须标记 fallback");
    assert_eq!(
        stats.requests.load(Ordering::SeqCst),
        1,
        "解析失败不产生 HTTP 请求"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn truncated_body_is_bounded() {
    let stats = Arc::new(Stats::default());
    let address = spawn_server(Arc::clone(&stats)).await;
    let transport = HttpTransport::new(None, None).expect("transport");
    let mut small = request(address, "/limit");
    small.max_body_bytes = 10;
    let outcome = transport.fetch(small).await;
    assert!(outcome.truncated);
    assert!(!outcome.complete);
    assert!(outcome.body.len() <= 10);
}

#[tokio::test]
async fn exact_limit_is_complete_and_redirect_is_terminal() {
    let stats = Arc::new(Stats::default());
    let addr = spawn_server(stats.clone()).await;
    let transport = HttpTransport::new(None, None).unwrap();
    let mut r = request(addr, "/redirect");
    r.max_redirects = 10; // 底层不能因调用者的宽松选项跟跳。
    r.max_body_bytes = b"<html><title>mock</title>,2,1,200,\"USA\"</html>".len() as u64;
    let outcome = transport.fetch(r).await;
    assert_eq!(outcome.status, Some(302));
    assert!(outcome.complete && !outcome.truncated);
    assert_eq!(stats.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn distinct_body_budgets_cannot_share_a_result() {
    let stats = Arc::new(Stats::default());
    let addr = spawn_server(stats.clone()).await;
    let (tx, rx) = watch::channel(false);
    let a = request(addr, "/budget");
    let mut b = a.clone();
    b.target_id = "small".into();
    b.max_body_bytes = 1;
    let items = run_batch(
        Arc::new(HttpTransport::new(None, None).unwrap()),
        vec![a, b],
        fast_budget(),
        rx,
    )
    .await;
    assert!(items[0].1.complete);
    assert_eq!(items[1].1.body.len(), 1);
    assert!(items[1].1.truncated);
    assert_eq!(stats.requests.load(Ordering::SeqCst), 2);
    drop(tx);
}

#[test]
fn direct_client_ignores_environment_proxy() {
    // 在独立子进程设置假代理，避免与并行测试竞争环境变量。
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "budget_and_anonymity_hold"])
        .env("HTTP_PROXY", "http://127.0.0.1:0")
        .env("ALL_PROXY", "http://127.0.0.1:0")
        .env("NO_PROXY", "")
        .env("http_proxy", "http://127.0.0.1:0")
        .env("all_proxy", "http://127.0.0.1:0")
        .env("no_proxy", "")
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("1 passed"));
}

#[test]
fn reserved_destinations_and_proxy_credentials_are_rejected() {
    for host in [
        "LOCALHOST.",
        "a.localhost.",
        "192.168.1.1",
        "[fe80::1]",
        "::ffff:127.0.0.1",
    ] {
        assert!(parallaxe_core::probe::is_private_destination(host));
    }
    for uri in [
        "http://",
        "https://@example.invalid",
        "http://proxy.invalid/path",
        "http://proxy.invalid:bad",
    ] {
        assert!(HttpTransport::new(Some(uri.into()), None).is_err());
    }
}

#[derive(Default)]
struct Gate {
    started: AtomicUsize,
    changed: tokio::sync::Notify,
}
impl Transport for Gate {
    fn fetch<'a>(
        &'a self,
        _: FetchRequest,
    ) -> parallaxe_core::probe::BoxFuture<'a, parallaxe_core::probe::FetchOutcome> {
        self.started.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_one();
        Box::pin(std::future::pending())
    }
}
#[tokio::test]
async fn cancellation_stops_queued_hosts_at_global_limit() {
    let gate = Arc::new(Gate::default());
    let (tx, rx) = watch::channel(false);
    let requests = (0..18)
        .map(|i| {
            let mut r = request("127.0.0.1:1".parse().unwrap(), &format!("/{i}"));
            r.host = format!("host-{}", i % 6);
            r
        })
        .collect();
    let batch = tokio::spawn(run_batch(gate.clone(), requests, fast_budget(), rx));
    tokio::time::timeout(Duration::from_secs(3), async {
        while gate.started.load(Ordering::SeqCst) < 6 {
            gate.changed.notified().await;
        }
    })
    .await
    .unwrap();
    tx.send(true).unwrap();
    assert!(batch.await.unwrap().is_empty());
    assert_eq!(gate.started.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn oversized_requested_budget_cannot_exceed_sixteen_mib() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let limit = HARD_LIMITS.max_body_bytes as usize;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    limit + 1
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let block = [0; 8192];
        for _ in 0..limit / block.len() {
            socket.write_all(&block).await.unwrap();
        }
        socket.write_all(&[0]).await.unwrap();
    });
    let mut r = request(addr, "/cap");
    r.max_body_bytes = u64::MAX;
    r.timeout_ms = 12000;
    let outcome = HttpTransport::new(None, None).unwrap().fetch(r).await;
    assert_eq!(outcome.body.len(), limit);
    assert!(outcome.truncated && !outcome.complete);
    server.await.unwrap();
}
