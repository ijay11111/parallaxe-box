//! `proxy`：代理模式、本机端口探测与 PAC 最小子集求值。
//!
//! 边界：只做**只读**路径判断。不改系统代理、不改路由/DNS、不读凭据、不扫网段；
//! 探测只连接用户环境变量或内置常见端口列表，全部有界超时。
//!
//! 五种模式（GUI/CLI 一致）：`AUTO`（默认）/ `NONE` / `HTTP` / `SOCKS` / `PAC`。
//! 本构建不含 SOCKS 传输，`SOCKS` 会在产生任何网络 I/O 前被明确拒绝，而不是静默直连。

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use crate::targets::ConfigError;

/// 用户可选的代理模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyMode {
    /// 按环境变量与内置常见端口探测当前可用代理；没有可用代理时直连。
    #[default]
    Auto,
    /// 强制直连，忽略所有代理设置。
    None,
    /// 用户显式提供的 HTTP(S) 代理。
    Http,
    /// SOCKS 代理（本构建不支持传输，仅保留选择与明确提示）。
    Socks,
    /// PAC 网址：按 `FindProxyForURL` 规则逐目标分流。
    Pac,
}

impl ProxyMode {
    /// 解析 CLI/配置里的模式字符串（大小写不敏感）。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(ProxyMode::Auto),
            "none" | "direct" => Some(ProxyMode::None),
            "http" | "https" => Some(ProxyMode::Http),
            "socks" | "socks5" | "socks5h" => Some(ProxyMode::Socks),
            "pac" => Some(ProxyMode::Pac),
            _ => None,
        }
    }

    /// 报告与 CLI 使用的稳定机器码。
    pub fn as_str(self) -> &'static str {
        match self {
            ProxyMode::Auto => "AUTO",
            ProxyMode::None => "NONE",
            ProxyMode::Http => "HTTP",
            ProxyMode::Socks => "SOCKS",
            ProxyMode::Pac => "PAC",
        }
    }
}

/// 内置常见本地代理端口（Clash/mihomo、v2rayN、Shadowsocks、Privoxy 等默认值）。
/// 只做本机回环连接，不扫网段；用户可用 `PARALLAXE_PROXY_PORTS` 追加。
pub const COMMON_PROXY_PORTS: &[u16] = &[
    7890, 7891, 7892, 7893, 7897, 7898, 7899, 1080, 1081, 1086, 1087, 10808, 10809, 10810, 8118,
    8888, 8889, 20171, 20172, 60487, 4780, 6152, 8080,
];

/// 探测到的候选代理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyCandidate {
    /// 归一化后的代理 URI，例如 `http://127.0.0.1:7890`。
    pub uri: String,
    /// 来源：`env:HTTPS_PROXY` / `port:7890` / `env:PARALLAXE_PROXY_PORTS`。
    pub source: String,
    /// TCP 端口是否响应（只表示端口开着，不代表能到目标站）。
    pub port_open: bool,
}

/// 一次运行前的网络路径准备结果。
#[derive(Debug, Clone, Default)]
pub struct NetworkPrep {
    /// 传输层默认代理；`None` 表示直连。
    pub default_proxy: Option<String>,
    /// 每个 host 的路径（`Some(proxy)` 表示经代理，`None` 表示直连）。
    pub routes: HashMap<String, Option<String>>,
    /// 写入 `path.proxy_mode` 的值：`DIRECT_SELECTED` / `PROXY_SELECTED` / `HTTP` / `PAC` 等。
    pub path_label: String,
    /// 探测摘要，写入 `path.reason`，供 UI/报告解释。
    pub note: String,
    /// 候选端口与响应情况。
    pub candidates: Vec<ProxyCandidate>,
}

fn field_error(message: impl Into<String>) -> ConfigError {
    ConfigError::field("--proxy", message)
}

/// 归一化代理 URI：只接受 http/https、无凭据、无路径。
pub fn normalize_proxy_uri(value: &str) -> Result<String, ConfigError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(field_error("代理地址为空"));
    }
    if trimmed.contains('@') {
        return Err(field_error("拒绝带凭据的代理 URI"));
    }
    if trimmed.contains(['?', '#', '\\']) || trimmed.chars().any(char::is_whitespace) {
        return Err(field_error("代理 URI 不接受查询或空白"));
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let parsed = url::Url::parse(&candidate).map_err(|_| field_error("代理 URI 无法解析"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(field_error("代理 URI 只接受 http/https"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(field_error("拒绝带凭据的代理 URI"));
    }
    if !matches!(parsed.path(), "" | "/") {
        return Err(field_error("代理 URI 不接受路径"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| field_error("代理 URI 缺少主机"))?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| field_error("代理 URI 缺少端口"))?;
    Ok(format!("{}://{host}:{port}", parsed.scheme()))
}

#[cfg(not(target_arch = "wasm32"))]
fn proxy_host_port(uri: &str) -> Option<(String, u16)> {
    let parsed = url::Url::parse(uri).ok()?;
    let host = parsed.host_str()?.to_string();
    let port = parsed.port_or_known_default()?;
    Some((host, port))
}

/// 从环境变量、Windows 系统代理设置与内置端口表收集候选（去重、保序）。
pub fn collect_candidates() -> Vec<ProxyCandidate> {
    let mut list: Vec<ProxyCandidate> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |value: &str, source: String| {
        if let Ok(uri) = normalize_proxy_uri(value) {
            if seen.insert(uri.clone()) {
                list.push(ProxyCandidate {
                    uri,
                    source,
                    port_open: false,
                });
            }
        }
    };
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = std::env::var(key) {
            push(&value, format!("env:{key}"));
        }
    }
    for value in system_proxy_values() {
        push(&value, "system:ProxyServer".to_string());
    }
    let mut extra_ports: Vec<u16> = Vec::new();
    if let Ok(extra) = std::env::var("PARALLAXE_PROXY_PORTS") {
        for item in extra.split(',') {
            if let Ok(port) = item.trim().parse::<u16>() {
                if port != 0 {
                    extra_ports.push(port);
                }
            }
        }
    }
    for port in extra_ports
        .into_iter()
        .chain(COMMON_PROXY_PORTS.iter().copied())
    {
        let uri = format!("http://127.0.0.1:{port}");
        if seen.insert(uri.clone()) {
            list.push(ProxyCandidate {
                uri,
                source: format!("port:{port}"),
                port_open: false,
            });
        }
    }
    list
}

/// 只读 Windows 系统代理（WinINET）设置。
///
/// 用固定参数查询 `reg.exe`，不执行任何来自 PAC/代理输入的字符串；失败时静默返回空，
/// 不影响端口探测与直连回退。其他平台没有等价的统一设置，返回空。
#[cfg(windows)]
fn system_proxy_values() -> Vec<String> {
    use std::process::Command;
    let reg = std::env::var("SystemRoot")
        .map(|root| format!("{root}\\System32\\reg.exe"))
        .unwrap_or_else(|_| "reg.exe".to_string());
    let query = |name: &str| -> Option<String> {
        let output = Command::new(&reg)
            .args([
                "query",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
                "/v",
                name,
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let Some(field) = parts.next() else { continue };
            if field != name {
                continue;
            }
            let values: Vec<&str> = parts.collect();
            return values.last().map(|value| (*value).to_string());
        }
        None
    };
    let enabled = query("ProxyEnable").unwrap_or_default();
    if !enabled.contains("0x1") && !enabled.eq_ignore_ascii_case("1") {
        return Vec::new();
    }
    let Some(server) = query("ProxyServer") else {
        return Vec::new();
    };
    let mut values = Vec::new();
    if server.contains('=') {
        for part in server.split(';') {
            let Some((scheme, address)) = part.split_once('=') else {
                continue;
            };
            if matches!(
                scheme.trim().to_ascii_lowercase().as_str(),
                "http" | "https"
            ) {
                values.push(address.trim().to_string());
            }
        }
    } else if !server.trim().is_empty() {
        values.push(server.trim().to_string());
    }
    values
}

#[cfg(not(windows))]
fn system_proxy_values() -> Vec<String> {
    Vec::new()
}

#[cfg(target_arch = "wasm32")]
fn empty_prep(label: &str, note: String) -> NetworkPrep {
    NetworkPrep {
        default_proxy: None,
        routes: HashMap::new(),
        path_label: label.to_string(),
        note,
        candidates: Vec::new(),
    }
}

/// 准备网络路径：校验模式、探测端口、按目标 host 选择代理/直连。
///
/// `hosts` 是本次实际会访问的目标 host（已去重）。不修改任何系统设置。
#[cfg(not(target_arch = "wasm32"))]
pub async fn prepare(
    mode: ProxyMode,
    explicit_proxy: Option<&str>,
    pac_url: Option<&str>,
    hosts: &[String],
) -> Result<NetworkPrep, ConfigError> {
    let mode = if mode == ProxyMode::Auto && explicit_proxy.is_some() {
        // 向后兼容：旧 `--proxy` 等价于显式 HTTP 代理。
        ProxyMode::Http
    } else {
        mode
    };
    match mode {
        ProxyMode::None => Ok(direct_prep("用户选择不代理（强制直连）")),
        ProxyMode::Socks => Err(ConfigError::field(
            "--proxy-mode",
            "当前构建不支持 SOCKS 传输；请改用 HTTP 代理、PAC 或直连（不会静默直连）",
        )),
        ProxyMode::Http => {
            let raw = explicit_proxy.ok_or_else(|| {
                ConfigError::field("--proxy-mode", "HTTP 模式需要 --proxy http://host:port")
            })?;
            let uri = normalize_proxy_uri(raw)?;
            let mut routes = HashMap::new();
            for host in hosts {
                routes.insert(host.to_ascii_lowercase(), Some(uri.clone()));
            }
            Ok(NetworkPrep {
                default_proxy: Some(uri.clone()),
                routes,
                path_label: "HTTP".to_string(),
                note: format!("用户指定 HTTP 代理 {uri}（远端可达性不在本机验证）"),
                candidates: Vec::new(),
            })
        }
        ProxyMode::Pac => {
            let url = pac_url.ok_or_else(|| {
                ConfigError::field(
                    "--proxy-mode",
                    "PAC 模式需要 --pac-url http(s)://…/proxy.pac",
                )
            })?;
            match fetch_pac(url).await {
                Ok(script) => {
                    let mut routes = HashMap::new();
                    let mut via_proxy = 0usize;
                    let mut direct = 0usize;
                    let mut socks = 0usize;
                    let mut default_proxy = None;
                    for host in hosts {
                        let url_for_host = format!("https://{host}/");
                        let decision = evaluate_pac(&script, &url_for_host, host);
                        let key = host.to_ascii_lowercase();
                        match decision {
                            PacDecision::Direct => {
                                direct += 1;
                                routes.insert(key, None);
                            }
                            PacDecision::Proxy(proxy) => {
                                via_proxy += 1;
                                if default_proxy.is_none() {
                                    default_proxy = Some(proxy.clone());
                                }
                                routes.insert(key, Some(proxy));
                            }
                            PacDecision::Socks(proxy) => {
                                socks += 1;
                                routes.insert(key, None);
                                default_proxy.get_or_insert(proxy);
                            }
                        }
                    }
                    let mut note = format!(
                        "PAC {url}：按 FindProxyForURL 分流，{} 项经代理 / {} 项直连",
                        via_proxy, direct
                    );
                    if socks > 0 {
                        note.push_str(&format!(
                            "；{socks} 项返回 SOCKS（当前构建不支持，已回退直连）"
                        ));
                    }
                    note.push_str("；PAC 仅使用最小语法子集，未覆盖的调用不猜测");
                    Ok(NetworkPrep {
                        default_proxy,
                        routes,
                        path_label: "PAC".to_string(),
                        note,
                        candidates: Vec::new(),
                    })
                }
                Err(error) => {
                    // 明确回退，不静默：报告与 UI 都能看到原因。
                    let mut prep = auto_prepare(hosts).await;
                    prep.path_label = "PAC".to_string();
                    prep.note = format!("PAC 获取失败（{error}），已回退自动选择；{}", prep.note);
                    Ok(prep)
                }
            }
        }
        ProxyMode::Auto => Ok(auto_prepare(hosts).await),
    }
}

/// wasm 端没有本机端口探测能力：保持直连并明确注明，不伪装探测结果。
#[cfg(target_arch = "wasm32")]
pub async fn prepare(
    _mode: ProxyMode,
    _explicit_proxy: Option<&str>,
    _pac_url: Option<&str>,
    _hosts: &[String],
) -> Result<NetworkPrep, ConfigError> {
    Ok(empty_prep(
        "DIRECT_SELECTED",
        "wasm 端不支持本机代理探测：本次直连".to_string(),
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn direct_prep(note: &str) -> NetworkPrep {
    NetworkPrep {
        default_proxy: None,
        routes: HashMap::new(),
        path_label: "DIRECT_SELECTED".to_string(),
        note: note.to_string(),
        candidates: Vec::new(),
    }
}

/// 自动选择：端口探测 → 首个可用候选 → 逐 host 验证 → 不可达的回退直连。
#[cfg(not(target_arch = "wasm32"))]
async fn auto_prepare(hosts: &[String]) -> NetworkPrep {
    auto_prepare_with(collect_candidates(), hosts).await
}

/// 可注入候选的自动选择（测试与其他前端复用；不在函数内读环境变量）。
#[cfg(not(target_arch = "wasm32"))]
pub async fn auto_prepare_with(
    mut candidates: Vec<ProxyCandidate>,
    hosts: &[String],
) -> NetworkPrep {
    if candidates.is_empty() {
        return direct_prep("自动：未发现环境代理或常见代理端口");
    }
    for candidate in candidates.iter_mut() {
        candidate.port_open = port_open(&candidate.uri).await;
    }
    let open: Vec<String> = candidates
        .iter()
        .filter(|candidate| candidate.port_open)
        .map(|candidate| candidate.uri.clone())
        .collect();
    let open_summary = if open.is_empty() {
        "无端口响应".to_string()
    } else {
        open.join(", ")
    };
    if open.is_empty() {
        let mut prep = direct_prep(&format!(
            "自动：探测 {} 个候选，{}；未发现可用代理 → 直连",
            candidates.len(),
            open_summary
        ));
        prep.candidates = candidates;
        return prep;
    }
    // 先用第一个 host 找出一个真正能建立 CONNECT 隧道的代理，避免对每个目标
    // 都重复试全部候选。
    let probe_host = hosts
        .first()
        .cloned()
        .unwrap_or_else(|| "www.google.com".to_string());
    let mut selected: Option<String> = None;
    for uri in open.iter().take(3) {
        if http_connect_ok(uri, &probe_host).await {
            selected = Some(uri.clone());
            break;
        }
    }
    let Some(selected) = selected else {
        let mut prep = direct_prep(&format!(
            "自动：端口响应 [{}]，但均无法建立到 {} 的 CONNECT 通道 → 直连",
            open_summary, probe_host
        ));
        prep.candidates = candidates;
        return prep;
    };
    // 逐 host 验证；失败的目标如实回退直连，而不是把整次运行判成“整体受限”。
    let mut routes = HashMap::new();
    let mut via_proxy = 0usize;
    let mut direct = 0usize;
    let mut proxy_failed = 0usize;
    let mut direct_unverified = 0usize;
    for host in hosts {
        let key = host.to_ascii_lowercase();
        if http_connect_ok(&selected, host).await {
            routes.insert(key, Some(selected.clone()));
            via_proxy += 1;
        } else {
            proxy_failed += 1;
            if tcp_reachable(host, 443).await {
                routes.insert(key, None);
                direct += 1;
            } else {
                routes.insert(key, None);
                direct += 1;
                direct_unverified += 1;
            }
        }
    }
    let label = if via_proxy == 0 {
        "DIRECT_SELECTED"
    } else {
        "PROXY_SELECTED"
    };
    let mut note = format!(
        "自动：探测 {} 个候选，端口响应 [{}]，选中 {selected}；{via_proxy} 个域名经代理",
        candidates.len(),
        open_summary
    );
    if proxy_failed > 0 {
        note.push_str(&format!(
            "，{proxy_failed} 个域名代理不可达回退直连（{direct} 个直连，其中 {direct_unverified} 个未验证到 443）"
        ));
    }
    note.push_str("；未修改系统代理/路由/DNS");
    NetworkPrep {
        default_proxy: (via_proxy > 0).then_some(selected),
        routes,
        path_label: label.to_string(),
        note,
        candidates,
    }
}

/// 端口是否接受 TCP 连接（短超时，仅回环候选）。
#[cfg(not(target_arch = "wasm32"))]
async fn port_open(uri: &str) -> bool {
    let Some((host, port)) = proxy_host_port(uri) else {
        return false;
    };
    matches!(
        tokio::time::timeout(
            Duration::from_millis(350),
            tokio::net::TcpStream::connect((host.as_str(), port))
        )
        .await,
        Ok(Ok(_))
    )
}

/// 通过 HTTP 代理建立到 `host:443` 的 CONNECT 隧道；只握手，不发送目标请求。
#[cfg(not(target_arch = "wasm32"))]
pub async fn http_connect_ok(proxy_uri: &str, host: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Some((proxy_host, proxy_port)) = proxy_host_port(proxy_uri) else {
        return false;
    };
    let Ok(Ok(mut stream)) = tokio::time::timeout(
        Duration::from_millis(900),
        tokio::net::TcpStream::connect((proxy_host.as_str(), proxy_port)),
    )
    .await
    else {
        return false;
    };
    let request = format!(
        "CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\nProxy-Connection: keep-alive\r\n\r\n"
    );
    if tokio::time::timeout(
        Duration::from_millis(600),
        stream.write_all(request.as_bytes()),
    )
    .await
    .is_err()
    {
        return false;
    }
    let mut buffer = [0u8; 128];
    let Ok(Ok(read)) =
        tokio::time::timeout(Duration::from_millis(1_200), stream.read(&mut buffer)).await
    else {
        return false;
    };
    let text = String::from_utf8_lossy(&buffer[..read]);
    text.lines()
        .next()
        .map(|line| line.contains(" 200"))
        .unwrap_or(false)
}

/// 直连 443 是否可建立 TCP（不发送 HTTP）。
#[cfg(not(target_arch = "wasm32"))]
async fn tcp_reachable(host: &str, port: u16) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_millis(900),
            tokio::net::TcpStream::connect((host, port))
        )
        .await,
        Ok(Ok(_))
    )
}

/// 获取 PAC 文本：只读 GET、空 Cookie、无代理、5 秒超时、最多 512 KiB。
#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_pac(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|_| "PAC 地址无法解析".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("PAC 地址只接受 http/https".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("PAC 地址不得包含凭据".to_string());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::limited(3))
        .timeout(Duration::from_secs(5))
        .user_agent("Parallaxe-Research/0 (pac-read-only)")
        .build()
        .map_err(|_| "PAC 客户端构建失败".to_string())?;
    let response = client
        .get(parsed)
        .header(reqwest::header::COOKIE, "")
        .send()
        .await
        .map_err(|_| "PAC 获取超时或失败".to_string())?;
    if !response.status().is_success() {
        return Err(format!("PAC 返回 HTTP {}", response.status().as_u16()));
    }
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "PAC 读取失败".to_string())?
    {
        if body.len() + chunk.len() > 512 * 1024 {
            return Err("PAC 超过 512 KiB 上限".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| "PAC 不是 UTF-8 文本".to_string())
}

// ---------------------------------------------------------------------------
// PAC 最小子集：if/else、var、return、常见字符串/网络函数。
// ---------------------------------------------------------------------------

/// PAC 求值结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacDecision {
    Direct,
    Proxy(String),
    Socks(String),
}

/// PAC 语法限制（写入报告/UI，避免假确定）。
pub const PAC_LIMITATIONS: &str = "PAC 最小子集：支持 if/else、var、return、PROXY/DIRECT、isPlainHostName/dnsDomainIs/localHostOrDomainIs/shExpMatch/isInNet(仅 IP 字面量)/dnsResolve(仅 IP 字面量)/myIpAddress；需要真实 DNS 解析、重复调用或自定义调用的 PAC 不支持，相关条件按 false 处理";

/// 对单个 URL/host 求值 `FindProxyForURL`。
pub fn evaluate_pac(script: &str, url: &str, host: &str) -> PacDecision {
    let Some(body) = extract_function_body(script, "FindProxyForURL") else {
        return PacDecision::Direct;
    };
    let tokens = lex(&body);
    let mut parser = Parser { tokens, pos: 0 };
    let statements = parser.parse_block_top();
    let mut interpreter = Interpreter {
        url: url.to_string(),
        host: host.to_string(),
        vars: HashMap::new(),
    };
    match interpreter.exec(&statements) {
        Some(value) => parse_directive(&value),
        None => PacDecision::Direct,
    }
}

fn extract_function_body(script: &str, name: &str) -> Option<String> {
    let cleaned = strip_comments(script);
    let index = cleaned.find(name)?;
    let open = cleaned[index..].find('{')? + index;
    let mut depth = 0usize;
    let bytes = cleaned.as_bytes();
    let mut in_string: Option<u8> = None;
    let mut index = open;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote) = in_string {
            if byte == quote && (index == 0 || bytes[index - 1] != b'\\') {
                in_string = None;
            }
        } else {
            match byte {
                b'"' | b'\'' => in_string = Some(byte),
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(cleaned[open + 1..index].to_string());
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn strip_comments(script: &str) -> String {
    let mut out = String::with_capacity(script.len());
    let bytes = script.as_bytes();
    let mut index = 0;
    let mut in_string: Option<u8> = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote) = in_string {
            out.push(byte as char);
            if byte == quote && (index == 0 || bytes[index - 1] != b'\\') {
                in_string = None;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            in_string = Some(byte);
            out.push(byte as char);
            index += 1;
            continue;
        }
        if byte == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'/' {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'*' {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        out.push(byte as char);
        index += 1;
    }
    out
}

fn parse_directive(raw: &str) -> PacDecision {
    for part in raw.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut words = part.split_whitespace();
        let kind = words.next().unwrap_or_default().to_ascii_uppercase();
        match kind.as_str() {
            "DIRECT" => return PacDecision::Direct,
            "PROXY" | "HTTP" | "HTTPS" => {
                return match words.next() {
                    Some(address) => PacDecision::Proxy(format!("http://{address}")),
                    None => PacDecision::Direct,
                };
            }
            "SOCKS" | "SOCKS4" | "SOCKS5" => {
                return match words.next() {
                    Some(address) => PacDecision::Socks(format!("socks5://{address}")),
                    None => PacDecision::Direct,
                };
            }
            _ => continue,
        }
    }
    PacDecision::Direct
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Num(f64),
    Sym(String),
}

fn lex(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        if ch == '"' || ch == '\'' {
            let quote = ch;
            index += 1;
            let mut value = String::new();
            while index < chars.len() {
                let c = chars[index];
                if c == '\\' && index + 1 < chars.len() {
                    let next = chars[index + 1];
                    value.push(match next {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        other => other,
                    });
                    index += 2;
                    continue;
                }
                if c == quote {
                    index += 1;
                    break;
                }
                value.push(c);
                index += 1;
            }
            tokens.push(Token::Str(value));
            continue;
        }
        if ch.is_ascii_digit() {
            let start = index;
            while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.') {
                index += 1;
            }
            let value: String = chars[start..index].iter().collect();
            tokens.push(Token::Num(value.parse().unwrap_or(0.0)));
            continue;
        }
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let start = index;
            while index < chars.len()
                && (chars[index].is_alphanumeric() || chars[index] == '_' || chars[index] == '$')
            {
                index += 1;
            }
            let value: String = chars[start..index].iter().collect();
            tokens.push(Token::Ident(value));
            continue;
        }
        let two: String = chars[index..(index + 2).min(chars.len())].iter().collect();
        if ["&&", "||", "==", "!=", "<=", ">="].contains(&two.as_str()) {
            tokens.push(Token::Sym(two));
            index += 2;
            continue;
        }
        tokens.push(Token::Sym(ch.to_string()));
        index += 1;
    }
    tokens
}

#[derive(Debug, Clone)]
enum Expr {
    Str(String),
    Num(f64),
    Bool(bool),
    Var(String),
    Call(String, Vec<Expr>),
    Not(Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
enum Stmt {
    Return(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Var(String, Expr),
    Expr(Expr),
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn eat_sym(&mut self, value: &str) -> bool {
        if matches!(self.peek(), Some(Token::Sym(sym)) if sym == value) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_ident(&mut self, value: &str) -> bool {
        if matches!(self.peek(), Some(Token::Ident(id)) if id == value) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_block_top(&mut self) -> Vec<Stmt> {
        self.parse_block(false)
    }

    fn parse_block(&mut self, braced: bool) -> Vec<Stmt> {
        let mut statements = Vec::new();
        loop {
            while self.eat_sym(";") {}
            match self.peek() {
                None => break,
                Some(Token::Sym(sym)) if sym == "}" => {
                    if braced {
                        self.pos += 1;
                    }
                    break;
                }
                _ => {}
            }
            let Some(statement) = self.parse_statement() else {
                // 无法识别的声明：跳到分号或结束括号，保持最小子集的如实降级。
                while let Some(token) = self.next() {
                    if matches!(&token, Token::Sym(sym) if sym == ";" || sym == "}") {
                        if matches!(&token, Token::Sym(sym) if sym == "}") {
                            self.pos -= 1;
                        }
                        break;
                    }
                }
                continue;
            };
            statements.push(statement);
        }
        statements
    }

    fn parse_statement(&mut self) -> Option<Stmt> {
        if self.eat_ident("if") {
            if !self.eat_sym("(") {
                return None;
            }
            let condition = self.parse_expr();
            if !self.eat_sym(")") {
                return None;
            }
            let then_branch = if self.eat_sym("{") {
                self.parse_block(true)
            } else {
                self.parse_statement().into_iter().collect()
            };
            while self.eat_sym(";") {}
            let else_branch = if self.eat_ident("else") {
                if self.eat_ident("if") {
                    // 把 `else if` 还原成嵌套 if。
                    self.pos -= 1;
                    self.parse_statement().into_iter().collect()
                } else if self.eat_sym("{") {
                    self.parse_block(true)
                } else {
                    self.parse_statement().into_iter().collect()
                }
            } else {
                Vec::new()
            };
            return Some(Stmt::If(condition, then_branch, else_branch));
        }
        if self.eat_ident("return") {
            let expr = self.parse_expr();
            while self.eat_sym(";") {}
            return Some(Stmt::Return(expr));
        }
        if self.eat_ident("var") || self.eat_ident("let") || self.eat_ident("const") {
            let name = match self.next() {
                Some(Token::Ident(name)) => name,
                _ => return None,
            };
            if !self.eat_sym("=") {
                return None;
            }
            let expr = self.parse_expr();
            while self.eat_sym(";") {}
            return Some(Stmt::Var(name, expr));
        }
        let expr = self.parse_expr();
        while self.eat_sym(";") {}
        Some(Stmt::Expr(expr))
    }

    fn parse_expr(&mut self) -> Expr {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Expr {
        let mut left = self.parse_and();
        while self.eat_sym("||") {
            let right = self.parse_and();
            left = Expr::Binary("||".into(), Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_and(&mut self) -> Expr {
        let mut left = self.parse_equality();
        while self.eat_sym("&&") {
            let right = self.parse_equality();
            left = Expr::Binary("&&".into(), Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_equality(&mut self) -> Expr {
        let mut left = self.parse_additive();
        loop {
            let op = match self.peek() {
                Some(Token::Sym(sym))
                    if ["==", "!=", "<", ">", "<=", ">="].contains(&sym.as_str()) =>
                {
                    sym.clone()
                }
                _ => break,
            };
            self.pos += 1;
            let right = self.parse_additive();
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_additive(&mut self) -> Expr {
        let mut left = self.parse_unary();
        loop {
            let op = match self.peek() {
                Some(Token::Sym(sym)) if sym == "+" || sym == "-" => sym.clone(),
                _ => break,
            };
            self.pos += 1;
            let right = self.parse_unary();
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_unary(&mut self) -> Expr {
        if self.eat_sym("!") {
            return Expr::Not(Box::new(self.parse_unary()));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Expr {
        match self.next() {
            Some(Token::Str(value)) => Expr::Str(value),
            Some(Token::Num(value)) => Expr::Num(value),
            Some(Token::Ident(name)) => match name.as_str() {
                "true" => Expr::Bool(true),
                "false" => Expr::Bool(false),
                _ => {
                    if self.eat_sym("(") {
                        let mut args = Vec::new();
                        if !self.eat_sym(")") {
                            loop {
                                args.push(self.parse_expr());
                                if self.eat_sym(",") {
                                    continue;
                                }
                                self.eat_sym(")");
                                break;
                            }
                        }
                        Expr::Call(name, args)
                    } else {
                        Expr::Var(name)
                    }
                }
            },
            Some(Token::Sym(sym)) if sym == "(" => {
                let expr = self.parse_expr();
                self.eat_sym(")");
                expr
            }
            _ => Expr::Str(String::new()),
        }
    }
}

#[derive(Debug, Clone)]
enum Value {
    Str(String),
    Num(f64),
    Bool(bool),
}

impl Value {
    fn as_str(&self) -> String {
        match self {
            Value::Str(value) => value.clone(),
            Value::Num(value) => value.to_string(),
            Value::Bool(value) => value.to_string(),
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            Value::Str(value) => !value.is_empty(),
            Value::Num(value) => *value != 0.0,
            Value::Bool(value) => *value,
        }
    }

    fn as_num(&self) -> f64 {
        match self {
            Value::Num(value) => *value,
            Value::Str(value) => value.parse().unwrap_or(0.0),
            Value::Bool(value) => {
                if *value {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

struct Interpreter {
    url: String,
    host: String,
    vars: HashMap<String, Value>,
}

impl Interpreter {
    fn exec(&mut self, statements: &[Stmt]) -> Option<String> {
        for statement in statements {
            match statement {
                Stmt::Return(expr) => return Some(self.eval(expr).as_str()),
                Stmt::Var(name, expr) => {
                    let value = self.eval(expr);
                    self.vars.insert(name.clone(), value);
                }
                Stmt::Expr(expr) => {
                    let _ = self.eval(expr);
                }
                Stmt::If(condition, then_branch, else_branch) => {
                    let branch = if self.eval(condition).as_bool() {
                        then_branch
                    } else {
                        else_branch
                    };
                    if let Some(value) = self.exec(branch) {
                        return Some(value);
                    }
                }
            }
        }
        None
    }

    fn eval(&self, expr: &Expr) -> Value {
        match expr {
            Expr::Str(value) => Value::Str(value.clone()),
            Expr::Num(value) => Value::Num(*value),
            Expr::Bool(value) => Value::Bool(*value),
            Expr::Var(name) => {
                // PAC 里常见的两个隐式参数可以直接当变量使用。
                if name == "url" {
                    return Value::Str(self.url.clone());
                }
                if name == "host" {
                    return Value::Str(self.host.clone());
                }
                self.vars
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| Value::Str(String::new()))
            }
            Expr::Not(inner) => Value::Bool(!self.eval(inner).as_bool()),
            Expr::Binary(op, left, right) => match op.as_str() {
                "&&" => Value::Bool(self.eval(left).as_bool() && self.eval(right).as_bool()),
                "||" => Value::Bool(self.eval(left).as_bool() || self.eval(right).as_bool()),
                "==" => Value::Bool(self.eval(left).as_str() == self.eval(right).as_str()),
                "!=" => Value::Bool(self.eval(left).as_str() != self.eval(right).as_str()),
                "<" => Value::Bool(self.eval(left).as_num() < self.eval(right).as_num()),
                ">" => Value::Bool(self.eval(left).as_num() > self.eval(right).as_num()),
                "<=" => Value::Bool(self.eval(left).as_num() <= self.eval(right).as_num()),
                ">=" => Value::Bool(self.eval(left).as_num() >= self.eval(right).as_num()),
                "+" => {
                    let left = self.eval(left);
                    let right = self.eval(right);
                    if matches!((&left, &right), (Value::Num(_), Value::Num(_))) {
                        Value::Num(left.as_num() + right.as_num())
                    } else {
                        Value::Str(format!("{}{}", left.as_str(), right.as_str()))
                    }
                }
                "-" => Value::Num(self.eval(left).as_num() - self.eval(right).as_num()),
                _ => Value::Bool(false),
            },
            Expr::Call(name, args) => self.call(name, args),
        }
    }

    fn call(&self, name: &str, args: &[Expr]) -> Value {
        let values: Vec<Value> = args.iter().map(|arg| self.eval(arg)).collect();
        let arg_str =
            |index: usize| -> String { values.get(index).map(Value::as_str).unwrap_or_default() };
        match name {
            "isPlainHostName" => Value::Bool(!self.host.contains('.')),
            "dnsDomainIs" => Value::Bool(self.host.ends_with(&arg_str(1))),
            "localHostOrDomainIs" => {
                let host = arg_str(0);
                let hostdom = arg_str(1);
                Value::Bool(host == hostdom || hostdom.ends_with(&format!(".{host}")))
            }
            "shExpMatch" => Value::Bool(shell_glob_match(&arg_str(0), &arg_str(1))),
            "isInNet" => Value::Bool(is_in_net(&self.host, &arg_str(1), &arg_str(2))),
            "dnsResolve" => {
                let host = arg_str(0);
                Value::Str(if host.parse::<std::net::IpAddr>().is_ok() {
                    host
                } else {
                    String::new()
                })
            }
            "myIpAddress" => Value::Str("127.0.0.1".to_string()),
            "isResolvable" => Value::Bool(false),
            "weekdayRange" | "timeRange" | "dateRange" => Value::Bool(false),
            "alert" => Value::Bool(true),
            _ => Value::Bool(false),
        }
    }
}

fn shell_glob_match(text: &str, pattern: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    let mut dp = vec![vec![false; text.len() + 1]; pattern.len() + 1];
    dp[0][0] = true;
    for p in 1..=pattern.len() {
        if pattern[p - 1] == '*' {
            dp[p][0] = dp[p - 1][0];
        }
        for t in 1..=text.len() {
            dp[p][t] = match pattern[p - 1] {
                '*' => dp[p - 1][t] || dp[p][t - 1],
                '?' => dp[p - 1][t - 1],
                literal => literal == text[t - 1] && dp[p - 1][t - 1],
            };
        }
    }
    dp[pattern.len()][text.len()]
}

fn is_in_net(host: &str, pattern: &str, mask: &str) -> bool {
    use std::net::IpAddr;
    let (Ok(host), Ok(pattern), Ok(mask)) = (
        host.parse::<IpAddr>(),
        pattern.parse::<IpAddr>(),
        mask.parse::<IpAddr>(),
    ) else {
        // 主机名为非字面量时需要真实 DNS；最小子集不猜测。
        return false;
    };
    match (host, pattern, mask) {
        (IpAddr::V4(host), IpAddr::V4(pattern), IpAddr::V4(mask)) => {
            let host = u32::from(host);
            let pattern = u32::from(pattern);
            let mask = u32::from(mask);
            host & mask == pattern & mask
        }
        _ => false,
    }
}

/// 供 UI/文档复用的模式说明。
pub fn mode_description(mode: ProxyMode) -> &'static str {
    match mode {
        ProxyMode::Auto => "自动选择：探测本机环境与常见端口，选可达路径（默认）",
        ProxyMode::None => "不代理：强制直连，忽略所有代理",
        ProxyMode::Http => "HTTP 代理：http://host:port（无凭据）",
        ProxyMode::Socks => "SOCKS 代理：当前构建不支持传输，选择后会被明确拒绝",
        ProxyMode::Pac => "PAC 网址：按 FindProxyForURL 规则逐目标分流",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pac_basic_rules() {
        let script = r#"
            function FindProxyForURL(url, host) {
                if (shExpMatch(host, "*.google.com")) return "PROXY 127.0.0.1:7890; DIRECT";
                if (dnsDomainIs(host, ".youtube.com")) return "PROXY 127.0.0.1:7891";
                return "DIRECT";
            }
        "#;
        assert_eq!(
            evaluate_pac(script, "https://books.google.com/", "books.google.com"),
            PacDecision::Proxy("http://127.0.0.1:7890".into())
        );
        assert_eq!(
            evaluate_pac(script, "https://www.youtube.com/", "www.youtube.com"),
            PacDecision::Proxy("http://127.0.0.1:7891".into())
        );
        assert_eq!(
            evaluate_pac(script, "https://example.org/", "example.org"),
            PacDecision::Direct
        );
    }

    #[test]
    fn pac_is_in_net_and_else() {
        let script = r#"
            function FindProxyForURL(url, host) {
                if (isInNet(host, "10.0.0.0", "255.0.0.0")) {
                    return "DIRECT";
                } else if (isPlainHostName(host)) {
                    return "DIRECT";
                } else {
                    return "PROXY proxy.local:8080";
                }
            }
        "#;
        assert_eq!(
            evaluate_pac(script, "https://intranet/", "intranet"),
            PacDecision::Direct
        );
        assert_eq!(
            evaluate_pac(script, "http://10.1.2.3/", "10.1.2.3"),
            PacDecision::Direct
        );
        assert_eq!(
            evaluate_pac(script, "https://example.org/", "example.org"),
            PacDecision::Proxy("http://proxy.local:8080".into())
        );
    }

    #[test]
    fn pac_socks_is_reported_unhonoured() {
        let script =
            r#"function FindProxyForURL(url, host) { return "SOCKS5 127.0.0.1:1080; DIRECT"; }"#;
        assert_eq!(
            evaluate_pac(script, "https://example.org/", "example.org"),
            PacDecision::Socks("socks5://127.0.0.1:1080".into())
        );
    }

    #[test]
    fn proxy_uri_normalization_rejects_credentials() {
        assert_eq!(
            normalize_proxy_uri("127.0.0.1:7890").unwrap(),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy_uri("http://127.0.0.1:7890").unwrap(),
            "http://127.0.0.1:7890"
        );
        assert!(normalize_proxy_uri("http://user:pass@127.0.0.1:7890").is_err());
        assert!(normalize_proxy_uri("socks5://127.0.0.1:1080").is_err());
        assert!(normalize_proxy_uri("http://127.0.0.1:7890/path").is_err());
    }
}
