//! `probe`：受预算约束的只读传输与匿名客户端工厂。
//!
//! 不提供裸 URL 接口；只接收已准入的一次性请求描述。默认空许可下本模块不会
//! 被在线运行调用；测试与 selftest 使用本地 mock / 合成传输。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Semaphore};

use tokio::time::Instant;

use crate::contracts::{Attempt, AttemptFamily};
use crate::targets::{Budget, HARD_LIMITS};

/// 盒装 future，便于以 `dyn Transport` 注入离线实现。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 一次传输请求（已绑定目标与运行）。
#[derive(Debug, Clone)]
pub struct FetchRequest {
    pub target_id: String,
    pub url: String,
    pub host: String,
    pub method: String,
    pub family: AttemptFamily,
    pub protocol: crate::forensics::ProbeProtocol,
    pub timeout_ms: u64,
    pub max_body_bytes: u64,
    pub max_redirects: u32,
}

impl FetchRequest {
    /// 去重键：传输+族+协议+方法+URL（不含请求体；本工具只发空体 GET）。
    pub fn dedupe_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            match self.family {
                AttemptFamily::V4 => "v4",
                AttemptFamily::V6 => "v6",
                AttemptFamily::Auto => "auto",
                AttemptFamily::Unknown => "unknown",
            },
            self.protocol.as_str(),
            self.method,
            self.url,
            self.max_redirects,
            self.timeout_ms,
            self.max_body_bytes
        )
    }
}

/// 传输错误类别（不含原始错误文本）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportErrorKind {
    Timeout,
    Resolve,
    Connect,
    Tls,
    Other,
}

impl TransportErrorKind {
    /// 对应 reason 码。
    pub fn reason(self) -> &'static str {
        match self {
            TransportErrorKind::Timeout => crate::contracts::reason::TRANSPORT_TIMEOUT,
            TransportErrorKind::Resolve => crate::contracts::reason::TRANSPORT_RESOLVE,
            TransportErrorKind::Connect => crate::contracts::reason::TRANSPORT_CONNECT,
            TransportErrorKind::Tls => crate::contracts::reason::TRANSPORT_TLS,
            TransportErrorKind::Other => crate::contracts::reason::TRANSPORT_OTHER,
        }
    }
}

/// 脱敏传输错误。
#[derive(Debug, Clone)]
pub struct TransportError {
    pub kind: TransportErrorKind,
    pub stage: &'static str,
}

/// 一次逻辑探测的样本（可能含主请求与一次受控回落）。
#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub location: Option<String>,
    pub body: Vec<u8>,
    pub complete: bool,
    pub truncated: bool,
    pub attempts: Vec<Attempt>,
    pub request_count: u64,
    pub error: Option<TransportError>,
    /// 主尝试实际地址族（由传输层在拿到对端后填写）。
    pub observed_family: Option<crate::contracts::ObservedFamily>,
}

impl FetchOutcome {
    /// 无 HTTP 的失败样本。
    pub fn failure(error: TransportError, attempts: Vec<Attempt>, request_count: u64) -> Self {
        FetchOutcome {
            status: None,
            content_type: None,
            location: None,
            body: Vec::new(),
            complete: false,
            truncated: false,
            attempts,
            request_count,
            error: Some(error),
            observed_family: None,
        }
    }

    /// 是否收到 HTTP 首跳。
    pub fn http_received(&self) -> bool {
        self.status.is_some()
    }
}

/// 传输端口：在线实现与离线实现分离。
pub trait Transport: Send + Sync {
    /// 仅无网络能力的测试替身可显式启用合成准入；真实传输保持默认拒绝。
    fn allows_synthetic_admission(&self) -> bool {
        false
    }
    /// 执行一次逻辑探测；实现方必须自行保证不跟跳、上限读取与空 Cookie。
    fn fetch<'a>(&'a self, request: FetchRequest) -> BoxFuture<'a, FetchOutcome>;
}

/// 匿名 HTTP 传输（reqwest + rustls；无 cookie store）。
#[cfg(not(target_arch = "wasm32"))]
pub struct HttpTransport {
    proxy: Option<String>,
    local_address: Option<std::net::IpAddr>,
    user_agent: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl HttpTransport {
    /// 新建匿名传输；`proxy` 只接受用户显式给定的无凭据 URI。
    pub fn new(
        proxy: Option<String>,
        local_address: Option<std::net::IpAddr>,
    ) -> Result<Self, String> {
        if let Some(uri) = &proxy {
            let rest = uri
                .strip_prefix("socks5h://")
                .or_else(|| uri.strip_prefix("socks5://"))
                .or_else(|| uri.strip_prefix("http://"))
                .or_else(|| uri.strip_prefix("https://"))
                .ok_or_else(|| "代理 URI 只接受 http/https/socks5/socks5h".to_string())?;
            if rest.contains('@') {
                return Err("不接受带凭据的代理 URI".to_string());
            }
            if rest.is_empty()
                || rest.contains(['/', '?', '#', '\\'])
                || uri.chars().any(char::is_whitespace)
                || reqwest::Url::parse(uri)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .is_none()
            {
                return Err("代理 URI 不接受路径或查询".to_string());
            }
        }
        Ok(HttpTransport {
            proxy,
            local_address,
            user_agent: "Parallaxe-Research/0 (signed-out; public page only)".to_string(),
        })
    }

    fn build_client(
        &self,
        request: &FetchRequest,
        peers: &[std::net::SocketAddr],
    ) -> Result<reqwest::Client, String> {
        // 编译特性不含 SOCKS；允许离线描述，但不可将其当 HTTP 代理或静默直连。
        if self
            .proxy
            .as_deref()
            .is_some_and(|uri| uri.starts_with("socks5"))
        {
            return Err("本构建不支持 SOCKS 传输".into());
        }
        let mut builder = reqwest::Client::builder()
            .user_agent(self.user_agent.clone())
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .http2_adaptive_window(true)
            .pool_max_idle_per_host(0);
        if let Some(proxy) = &self.proxy {
            let proxy = reqwest::Proxy::all(proxy).map_err(|_| "代理 URI 无法解析".to_string())?;
            builder = builder.proxy(proxy);
        }
        if let Some(address) = self.local_address {
            builder = builder.local_address(address);
        }
        if !peers.is_empty() {
            builder = builder.resolve_to_addrs(&request.host, peers);
        }
        builder
            .build()
            .map_err(|_| "匿名客户端构建失败".to_string())
    }

    async fn attempt(
        &self,
        request: &FetchRequest,
        family: AttemptFamily,
        fallback: bool,
        deadline: Instant,
    ) -> FetchOutcome {
        let started = Instant::now();
        let fail = |kind, stage: &'static str| {
            FetchOutcome::failure(
                TransportError { kind, stage },
                vec![Attempt {
                    family,
                    phase: stage.to_string(),
                    connect_ms: None,
                    first_byte_ms: None,
                    http_status: None,
                    fallback,
                    request_sent: false,
                }],
                0,
            )
        };
        let Ok(url) = reqwest::Url::parse(&request.url) else {
            return fail(TransportErrorKind::Other, "POLICY");
        };
        // HTTP 仅供字面回环地址的本机测试；发行目录始终是 HTTPS。
        let local_test = self.proxy.is_none()
            && url.scheme() == "http"
            && request
                .host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if request.method != "GET"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.host_str() != Some(&request.host)
            || (!local_test && (url.scheme() != "https" || is_private_destination(&request.host)))
        {
            return fail(TransportErrorKind::Other, "POLICY");
        }
        let mut peers = Vec::new();
        if self.proxy.is_none() {
            let Some(port) = url.port_or_known_default() else {
                return fail(TransportErrorKind::Other, "POLICY");
            };
            match tokio::time::timeout_at(
                deadline,
                tokio::net::lookup_host((request.host.as_str(), port)),
            )
            .await
            {
                Ok(Ok(resolved)) => {
                    for peer in resolved {
                        if !local_test && is_private_ip(peer.ip()) {
                            return fail(TransportErrorKind::Other, "POLICY");
                        }
                        if match family {
                            AttemptFamily::V4 => peer.is_ipv4(),
                            AttemptFamily::V6 => peer.is_ipv6(),
                            _ => true,
                        } {
                            peers.push(peer);
                        }
                    }
                }
                Ok(Err(_)) => return fail(TransportErrorKind::Resolve, "RESOLVE"),
                Err(_) => return fail(TransportErrorKind::Timeout, "RESOLVE"),
            }
            if peers.is_empty() {
                return fail(TransportErrorKind::Resolve, "RESOLVE");
            }
        }
        let client = match self.build_client(request, &peers) {
            Ok(client) => client,
            Err(_) => {
                return FetchOutcome::failure(
                    TransportError {
                        kind: TransportErrorKind::Other,
                        stage: "CLIENT",
                    },
                    vec![Attempt {
                        family,
                        phase: "CLIENT".to_string(),
                        connect_ms: None,
                        first_byte_ms: None,
                        http_status: None,
                        fallback,
                        request_sent: false,
                    }],
                    0,
                )
            }
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return fail(TransportErrorKind::Timeout, "HTTP");
        }
        let http_request = client
            .get(url)
            .timeout(remaining)
            .header(reqwest::header::COOKIE, "");
        let response = http_request.send().await;
        match response {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.to_string());
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.to_string());
                let mut body = Vec::new();
                let mut truncated = false;
                // 此时已经收到响应头，不将等待读体计入首包。
                let first_byte_ms = Some(started.elapsed().as_millis() as u64);
                let limit = request.max_body_bytes.min(HARD_LIMITS.max_body_bytes) as usize;
                loop {
                    match response.chunk().await {
                        Ok(Some(chunk)) => {
                            if chunk.len() > limit.saturating_sub(body.len()) {
                                let remaining = limit.saturating_sub(body.len());
                                body.extend_from_slice(&chunk[..remaining]);
                                truncated = true;
                                break;
                            }
                            body.extend_from_slice(&chunk);
                        }
                        Ok(None) => break,
                        Err(_) => {
                            truncated = true;
                            break;
                        }
                    }
                }
                let complete = !truncated;
                FetchOutcome {
                    status: Some(status),
                    content_type,
                    location,
                    body,
                    complete,
                    truncated,
                    attempts: vec![Attempt {
                        family,
                        phase: "HTTP_RESPONSE".to_string(),
                        connect_ms: None,
                        first_byte_ms,
                        http_status: Some(status),
                        fallback,
                        request_sent: true,
                    }],
                    request_count: 1,
                    error: None,
                    observed_family: response.remote_addr().filter(|_| self.proxy.is_none()).map(
                        |address| {
                            if address.is_ipv6() {
                                crate::contracts::ObservedFamily::V6
                            } else {
                                crate::contracts::ObservedFamily::V4
                            }
                        },
                    ),
                }
            }
            Err(error) => {
                let kind = if error.is_timeout() {
                    TransportErrorKind::Timeout
                } else if error.is_connect() {
                    TransportErrorKind::Connect
                } else {
                    TransportErrorKind::Other
                };
                FetchOutcome::failure(
                    TransportError {
                        kind,
                        stage: if tcp_failure(&error) { "TCP" } else { "HTTP" },
                    },
                    vec![Attempt {
                        family,
                        phase: "HTTP_REQUEST".to_string(),
                        connect_ms: None,
                        first_byte_ms: None,
                        http_status: None,
                        fallback,
                        request_sent: false,
                    }],
                    0,
                )
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Transport for HttpTransport {
    fn fetch<'a>(&'a self, request: FetchRequest) -> BoxFuture<'a, FetchOutcome> {
        Box::pin(async move {
            let deadline = Instant::now()
                + Duration::from_millis(request.timeout_ms.min(HARD_LIMITS.max_request_timeout_ms));
            let primary = self
                .attempt(&request, request.family, false, deadline)
                .await;
            // 无法证明代理后端地址族；TLS/策略拒绝等也不作为改族重试理由。
            if self.proxy.is_none()
                && request.family == AttemptFamily::V6
                && Instant::now() < deadline
                && primary.error.as_ref().is_some_and(|error| {
                    matches!(error.kind, TransportErrorKind::Resolve) || error.stage == "TCP"
                })
            {
                let mut fallback = self
                    .attempt(&request, AttemptFamily::V4, true, deadline)
                    .await;
                let mut attempts = primary.attempts;
                attempts.append(&mut fallback.attempts);
                fallback.attempts = attempts;
                fallback.request_count += primary.request_count;
                fallback
            } else {
                primary
            }
        })
    }
}

// 仅肯定的套接字连接失败允许换族；不解析可能含敏感细节的错误字符串。
#[cfg(not(target_arch = "wasm32"))]
fn tcp_failure(error: &reqwest::Error) -> bool {
    use std::error::Error;
    let mut cause = error.source();
    while let Some(inner) = cause {
        if inner.downcast_ref::<std::io::Error>().is_some_and(|e| {
            matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::AddrNotAvailable
            )
        }) {
            return true;
        }
        cause = inner.source();
    }
    false
}

/// wasm 端传输降级：浏览器 fetch 需要 GUI 侧的异步桥接；本 crate 只返回
/// 明确的离线解释失败，绝不改判定语义（避免把“未实现”伪装成网络错误）。
#[cfg(target_arch = "wasm32")]
pub struct WasmTransport;

#[cfg(target_arch = "wasm32")]
impl Transport for WasmTransport {
    fn fetch<'a>(&'a self, request: FetchRequest) -> BoxFuture<'a, FetchOutcome> {
        Box::pin(async move {
            FetchOutcome::failure(
                TransportError {
                    kind: TransportErrorKind::Other,
                    stage: "WASM_OFFLINE",
                },
                vec![Attempt {
                    family: request.family,
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

/// 并发执行一组已准入请求；全局 ≤6、单主机 ≤2（受 profile 进一步收紧）。
pub async fn run_batch(
    transport: Arc<dyn Transport>,
    requests: Vec<FetchRequest>,
    budget: Budget,
    cancel: watch::Receiver<bool>,
) -> Vec<(String, FetchOutcome)> {
    run_batch_checked(transport, requests, budget, cancel)
        .await
        .map(|(items, _)| items)
        .unwrap_or_default()
}

pub(crate) async fn run_batch_checked(
    transport: Arc<dyn Transport>,
    requests: Vec<FetchRequest>,
    budget: Budget,
    cancel: watch::Receiver<bool>,
) -> Result<(Vec<(String, FetchOutcome)>, bool), crate::targets::ConfigError> {
    let global_limit = budget.max_concurrency.min(HARD_LIMITS.max_concurrency);
    let per_host = budget.per_host.min(HARD_LIMITS.per_host).min(global_limit);
    if per_host == 0 {
        return Err(crate::targets::ConfigError::field(
            "budget",
            "并发上限不能为零",
        ));
    }
    let deadline = Instant::now()
        + Duration::from_millis(budget.run_timeout_ms.min(HARD_LIMITS.max_run_timeout_ms));
    let start = Instant::now();
    let slots = crate::schedule::plan_slots(&requests, &budget);
    let global = Arc::new(Semaphore::new(global_limit as usize));
    let mut hosts = HashMap::new();
    let mut order = Vec::new();
    let mut seen = HashMap::new();
    let mut tasks = tokio::task::JoinSet::new();
    for (position, request) in requests.into_iter().enumerate() {
        let offset_ms = slots
            .get(position)
            .map(|slot| slot.offset_ms)
            .unwrap_or_default();
        let key = request.dedupe_key();
        let next = seen.len();
        let index = *seen.entry(key).or_insert(next);
        let owner = index == next;
        order.push((request.target_id.clone(), index, owner));
        if !owner {
            continue;
        }
        let host = Arc::clone(
            hosts
                .entry(request.host.to_ascii_lowercase())
                .or_insert_with(|| Arc::new(Semaphore::new(per_host as usize))),
        );
        let global = Arc::clone(&global);
        let transport = Arc::clone(&transport);
        let mut cancel = cancel.clone();
        tasks.spawn(async move {
            // 发送时刻由统一调度器分配；任务本身不 sleep 决定间隔。
            let slot = start + Duration::from_millis(offset_ms);
            tokio::select! {
                biased;
                _ = cancel.wait_for(|flag| *flag) => (index, None, false),
                _ = tokio::time::sleep_until(deadline) => (index, None, true),
                outcome = async {
                    tokio::time::sleep_until(slot).await;
                    let _host = host.acquire().await.ok()?;
                    let _global = global.acquire().await.ok()?;
                    Some(transport.fetch(request).await)
                } => (index, outcome, false),
            }
        });
    }
    let mut completed = HashMap::new();
    let mut timed_out = false;
    while let Some(item) = tasks.join_next().await {
        let (index, outcome, expired) =
            item.map_err(|_| crate::targets::ConfigError::field("check", "传输任务异常结束"))?;
        timed_out |= expired;
        if let Some(outcome) = outcome {
            completed.insert(index, outcome);
        }
    }
    // 逆序分发：消费者保留同一样本，只由所有者记账；唯一请求直接移动大正文。
    let mut results = Vec::new();
    for (target, index, owner) in order.into_iter().rev() {
        let outcome = if owner {
            completed.remove(&index)
        } else {
            completed.get(&index).cloned().map(|mut outcome| {
                outcome.request_count = 0;
                outcome
            })
        };
        if let Some(outcome) = outcome {
            results.push((target, outcome));
        }
    }
    results.reverse();
    Ok((results, timed_out))
}

/// 判断 URL 是否为本机/私有目的地（准入层拒绝，不做 SSRF）。
pub fn is_private_destination(host: &str) -> bool {
    let lower = host.trim_end_matches('.').to_ascii_lowercase();
    lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower
            .trim_matches(['[', ']'])
            .parse()
            .is_ok_and(is_private_ip)
}

fn is_private_ip(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.octets()[0] == 0
                || v4.is_broadcast()
                || v4.is_multicast()
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|ip| is_private_ip(ip.into()))
        }
    }
}
