//! `network`：只读系统路径观测与地址族/回落诊断。
//!
//! 不修改任何系统设置，不读浏览器或订阅；TUN 只作为观测事实，不作为代理归因。

use crate::contracts::{Attempt, AttemptFamily, BindStatus};

/// 路径诊断值：三种 v6 结论与一般情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathDiagnostic {
    Ipv6Slow,
    Ipv6Broken,
    Ipv6OnlyEgress,
    ProxyRemoteUnverified,
    System,
}

impl PathDiagnostic {
    /// 写入 `transport.path_diagnostic` 的文本。
    pub fn as_str(self) -> &'static str {
        match self {
            PathDiagnostic::Ipv6Slow => "IPv6_SLOW",
            PathDiagnostic::Ipv6Broken => "IPv6_BROKEN",
            PathDiagnostic::Ipv6OnlyEgress => "IPV6_ONLY_EGRESS",
            PathDiagnostic::ProxyRemoteUnverified => "PROXY_REMOTE_UNVERIFIED",
            PathDiagnostic::System => "SYSTEM_PATH",
        }
    }
}

/// 只读的本地路径观察。
#[derive(Debug, Clone)]
pub struct LocalPathObservation {
    /// 是否检测到 TUN/TAP 类接口；`None` 表示无法判定，不猜。
    pub via_tun: Option<bool>,
    /// 第一个匹配的接口别名，仅本地展示。
    pub interface_alias: Option<String>,
    /// 绑定状态：未请求绑定时为 NOT_REQUESTED。
    pub bind_status: BindStatus,
}

/// 枚举本机接口，判断是否存在 TUN/TAP 类接管（只读）。
#[cfg(not(target_arch = "wasm32"))]
pub fn observe_local_path() -> LocalPathObservation {
    let mut via_tun = None;
    let mut alias = None;
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for iface in interfaces {
            let name = iface.name.to_ascii_lowercase();
            let looks_tun = name.contains("tun")
                || name.contains("tap")
                || name.contains("wireguard")
                || name.contains("nexpath");
            if looks_tun {
                via_tun = Some(true);
                alias = Some(iface.name);
                break;
            }
        }
    }
    LocalPathObservation {
        via_tun,
        interface_alias: alias,
        bind_status: BindStatus::NotRequested,
    }
}

/// 判断给定接口别名是否存在（只读）。
#[cfg(not(target_arch = "wasm32"))]
pub fn interface_exists(reference: &str) -> bool {
    if_addrs::get_if_addrs()
        .map(|list| {
            list.iter().any(|iface| {
                iface.name.eq_ignore_ascii_case(reference) || iface.ip().to_string() == reference
            })
        })
        .unwrap_or(false)
}

/// wasm 端没有本机接口枚举：保持未知，不猜 TUN。
#[cfg(target_arch = "wasm32")]
pub fn observe_local_path() -> LocalPathObservation {
    LocalPathObservation {
        via_tun: None,
        interface_alias: None,
        bind_status: BindStatus::NotRequested,
    }
}

/// wasm 端无法验证本机接口。
#[cfg(target_arch = "wasm32")]
pub fn interface_exists(_reference: &str) -> bool {
    false
}

/// 从尝试账本推导 v6 三态（不参与地区判定）。
///
/// * `IPv6_SLOW`：v6 成功但首包超过门槛；
/// * `IPv6_BROKEN`：v6 失败而 v4 成功；
/// * `IPV6_ONLY_EGRESS`：v4 失败而 v6 成功。
pub fn diagnose_attempts(attempts: &[Attempt], v6_budget_ms: u64) -> PathDiagnostic {
    let success = |attempt: &Attempt| attempt.http_status.is_some();
    let v6: Vec<&Attempt> = attempts
        .iter()
        .filter(|attempt| attempt.family == AttemptFamily::V6)
        .collect();
    let v4: Vec<&Attempt> = attempts
        .iter()
        .filter(|attempt| attempt.family == AttemptFamily::V4)
        .collect();
    let v6_success = v6.iter().any(|attempt| success(attempt));
    let v4_success = v4.iter().any(|attempt| success(attempt));
    if v6_success && !v4.is_empty() && !v4_success {
        return PathDiagnostic::Ipv6OnlyEgress;
    }
    if v6_success
        && v6
            .iter()
            .filter_map(|attempt| attempt.first_byte_ms)
            .max()
            .map(|ms| ms > v6_budget_ms)
            .unwrap_or(false)
    {
        return PathDiagnostic::Ipv6Slow;
    }
    if !v6.is_empty() && !v6_success && v4_success {
        return PathDiagnostic::Ipv6Broken;
    }
    PathDiagnostic::System
}
