//! `linkcheck`：阶段 4 的只读链路体检投影。
//!
//! 四列固定分开：resolver / 实际连接边缘 / HTTP 出口 / 地址族。未知留空（`null`），
//! QUIC 与 DNS 泄漏未测就写 `UNTESTED`；**不扫端口**、不切节点、不改系统代理/DNS/hosts。
//!
//! 默认零发包：本模块只投影已有运行结果。只有调用方显式要求时，才会对**内置目录主机**
//! 做一次系统 DNS 查询（`tokio::net::lookup_host`），并把次数记入 `dns_query_count`。

use std::collections::{BTreeMap, HashMap};

use crate::contracts::{Attempt, Layer, ObservedFamily, Report, ResultRecord};
use crate::network;
use crate::targets::{ConfigError, Registry};

/// 链路报告 schema。
pub const LINK_SCHEMA: &str = "parallaxe.link/v1";

/// 一条目标的链路体检行。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkRow {
    pub target: String,
    pub host: String,
    pub resolver: LinkResolverCell,
    pub connection_edge: LinkConnectionCell,
    pub http_egress: LinkEgressCell,
    pub address_family: LinkFamilyCell,
}

/// resolver 列。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkResolverCell {
    pub status: String,
    pub v4: u64,
    pub v6: u64,
    /// 默认留空；只有显式 `--show-ip` 才填入。
    pub addresses: Vec<String>,
    pub reason: Option<String>,
}

/// 实际连接边缘列。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkConnectionCell {
    pub status: String,
    pub observed_family: Option<String>,
    pub attempts: Vec<LinkAttemptCell>,
    /// 当前传输层不暴露对端地址；未知就保持 null。
    pub peer_address: Option<String>,
    pub reason: Option<String>,
}

/// 单次尝试的脱敏账本。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkAttemptCell {
    pub family: String,
    pub phase: String,
    pub http_status: Option<u16>,
    pub connect_ms: Option<u64>,
    pub first_byte_ms: Option<u64>,
    pub fallback: bool,
    pub request_sent: bool,
}

/// HTTP 出口列。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkEgressCell {
    pub status: String,
    pub observed_country: Option<String>,
    pub source: Option<String>,
    pub layer: Option<String>,
    pub reason: Option<String>,
}

/// 地址族列。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkFamilyCell {
    pub v4: String,
    pub v6: String,
    pub fallback: bool,
    pub diagnostic: Option<String>,
}

/// 能力清单：明确写出已实现、未测与禁止项。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkCapabilities {
    pub route_get: String,
    pub dns_compare: String,
    pub dns_leak: String,
    pub quic: String,
    pub http_egress: String,
    pub connection_edge: String,
    pub port_scan: String,
}

/// 本机只读路径观察。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkLocalPath {
    pub via_tun: Option<bool>,
    pub interface_alias: Option<String>,
    pub note: String,
}

/// 完整链路体检报告。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkReport {
    pub schema: String,
    pub generated_at: String,
    pub source: String,
    pub source_run_id: Option<String>,
    pub rows: Vec<LinkRow>,
    pub capabilities: LinkCapabilities,
    pub local_path: LinkLocalPath,
    pub dns_query_count: u64,
    pub http_request_count: u64,
    pub limitations: Vec<String>,
}

fn empty_resolver() -> LinkResolverCell {
    LinkResolverCell {
        status: "NOT_OBSERVED".to_string(),
        v4: 0,
        v6: 0,
        addresses: Vec::new(),
        reason: Some("RESOLVER_ANSWER_NOT_CAPTURED".to_string()),
    }
}

fn empty_connection() -> LinkConnectionCell {
    LinkConnectionCell {
        status: "NOT_OBSERVED".to_string(),
        observed_family: None,
        attempts: Vec::new(),
        peer_address: None,
        reason: Some("NO_PRIOR_ATTEMPT".to_string()),
    }
}

fn empty_egress(reason: &str) -> LinkEgressCell {
    LinkEgressCell {
        status: "NOT_OBSERVED".to_string(),
        observed_country: None,
        source: None,
        layer: None,
        reason: Some(reason.to_string()),
    }
}

fn empty_family() -> LinkFamilyCell {
    LinkFamilyCell {
        v4: "NOT_OBSERVED".to_string(),
        v6: "NOT_OBSERVED".to_string(),
        fallback: false,
        diagnostic: None,
    }
}

fn empty_row(registry: &Registry, id: &str, reason: &str) -> LinkRow {
    LinkRow {
        target: id.to_string(),
        host: registry
            .get(id)
            .map(|target| target.host())
            .unwrap_or_default(),
        resolver: empty_resolver(),
        connection_edge: empty_connection(),
        http_egress: empty_egress(reason),
        address_family: empty_family(),
    }
}

fn family_name(attempt: &Attempt) -> String {
    match attempt.family {
        crate::contracts::AttemptFamily::V4 => "V4",
        crate::contracts::AttemptFamily::V6 => "V6",
        crate::contracts::AttemptFamily::Auto => "AUTO",
        crate::contracts::AttemptFamily::Unknown => "UNKNOWN",
    }
    .to_string()
}

fn attempt_cell(attempt: &Attempt) -> LinkAttemptCell {
    LinkAttemptCell {
        family: family_name(attempt),
        phase: attempt.phase.clone(),
        http_status: attempt.http_status,
        connect_ms: attempt.connect_ms,
        first_byte_ms: attempt.first_byte_ms,
        fallback: attempt.fallback,
        request_sent: attempt.request_sent,
    }
}

fn row_from_result(registry: &Registry, result: &ResultRecord) -> LinkRow {
    let host = registry
        .get(&result.target)
        .map(|target| target.host())
        .unwrap_or_default();
    let attempts: Vec<LinkAttemptCell> =
        result.transport.attempts.iter().map(attempt_cell).collect();
    let connection_edge = if attempts.is_empty() {
        empty_connection()
    } else {
        let observed_family = result.path.observed_family.map(|family| match family {
            ObservedFamily::V4 => "V4".to_string(),
            ObservedFamily::V6 => "V6".to_string(),
        });
        LinkConnectionCell {
            status: "OBSERVED".to_string(),
            observed_family,
            attempts,
            peer_address: None,
            reason: None,
        }
    };

    let egress_layer = result
        .region_layers
        .iter()
        .find(|layer| layer.layer == Layer::EgressNetwork);
    let http_egress = if let Some(country) = egress_layer.and_then(|layer| layer.value.as_ref()) {
        let witness = result.evidence.iter().find(|item| {
            item.layer.region_layer() == Some(Layer::EgressNetwork)
                && crate::signals::country_claim(item).as_ref() == Some(country)
        });
        LinkEgressCell {
            status: "OBSERVED".into(),
            observed_country: Some(country.clone()),
            source: witness.map(|evidence| evidence.source_ref.clone()),
            layer: Some("EGRESS_NETWORK".into()),
            reason: None,
        }
    } else {
        empty_egress(
            egress_layer
                .and_then(|layer| layer.reason.as_deref())
                .unwrap_or("NO_EGRESS_EVIDENCE"),
        )
    };

    let mut family = empty_family();
    let mut saw_v4 = false;
    let mut saw_v6 = false;
    let mut v4_ok = false;
    let mut v6_ok = false;
    for attempt in &result.transport.attempts {
        match attempt.family {
            crate::contracts::AttemptFamily::V4 => {
                saw_v4 = true;
                v4_ok |= attempt.http_status.is_some();
            }
            crate::contracts::AttemptFamily::V6 => {
                saw_v6 = true;
                v6_ok |= attempt.http_status.is_some();
            }
            _ => {}
        }
        family.fallback |= attempt.fallback;
    }
    family.v4 = if !saw_v4 {
        "NOT_OBSERVED".to_string()
    } else if v4_ok {
        "OBSERVED_OK".to_string()
    } else {
        "OBSERVED_FAIL".to_string()
    };
    family.v6 = if !saw_v6 {
        "NOT_OBSERVED".to_string()
    } else if v6_ok {
        "OBSERVED_OK".to_string()
    } else {
        "OBSERVED_FAIL".to_string()
    };
    family.diagnostic = result.transport.path_diagnostic.clone();

    LinkRow {
        target: result.target.clone(),
        host,
        resolver: empty_resolver(),
        connection_edge,
        http_egress,
        address_family: family,
    }
}

/// 从一次已完成运行投影链路体检；不发起任何请求。
pub fn analyze(registry: &Registry, report: &Report, generated_at: String) -> LinkReport {
    let mut rows = BTreeMap::new();
    for result in &report.results {
        rows.entry(&result.target)
            .or_insert_with(|| row_from_result(registry, result));
    }
    for item in report
        .coverage
        .uncovered
        .iter()
        .filter(|item| item.selected)
    {
        rows.entry(&item.target)
            .or_insert_with(|| empty_row(registry, &item.target, &item.reason));
    }
    let path = network::observe_local_path();
    LinkReport {
        schema: LINK_SCHEMA.to_string(),
        generated_at,
        source: match report.mode {
            crate::contracts::Mode::Fixture => "FIXTURE".to_string(),
            crate::contracts::Mode::Online => "PRIOR_RUN".to_string(),
        },
        source_run_id: Some(report.run_id.clone()),
        rows: rows.into_values().collect(),
        capabilities: capabilities(),
        local_path: LinkLocalPath {
            via_tun: path.via_tun,
            interface_alias: path.interface_alias,
            note: "只观察接口存在性，不证明本次请求经过该接口；不修改系统路径".to_string(),
        },
        dns_query_count: 0,
        http_request_count: report.network_request_count,
        limitations: limitations(),
    }
}

/// 空选择快照：只为用户显式选择的目录目标生成“未观察”行，零发包。
pub fn analyze_selection(
    registry: &Registry,
    target_ids: &[String],
    generated_at: String,
) -> LinkReport {
    let path = network::observe_local_path();
    let mut ids: Vec<_> = target_ids.iter().collect();
    ids.sort_unstable();
    ids.dedup();
    let rows = ids
        .into_iter()
        .map(|id| empty_row(registry, id, crate::contracts::reason::NO_ADMISSION))
        .collect();
    LinkReport {
        schema: LINK_SCHEMA.to_string(),
        generated_at,
        source: "SELECTION_ONLY".to_string(),
        source_run_id: None,
        rows,
        capabilities: capabilities(),
        local_path: LinkLocalPath {
            via_tun: path.via_tun,
            interface_alias: path.interface_alias,
            note: "只观察接口存在性，不证明本次请求经过该接口；不修改系统路径".to_string(),
        },
        dns_query_count: 0,
        http_request_count: 0,
        limitations: limitations(),
    }
}

/// 空来源报告：没有任何已观察结果。
pub fn empty(generated_at: String) -> LinkReport {
    let path = network::observe_local_path();
    LinkReport {
        schema: LINK_SCHEMA.to_string(),
        generated_at,
        source: "NONE".to_string(),
        source_run_id: None,
        rows: Vec::new(),
        capabilities: capabilities(),
        local_path: LinkLocalPath {
            via_tun: path.via_tun,
            interface_alias: path.interface_alias,
            note: "只观察接口存在性，不证明本次请求经过该接口；不修改系统路径".to_string(),
        },
        dns_query_count: 0,
        http_request_count: 0,
        limitations: limitations(),
    }
}

fn capabilities() -> LinkCapabilities {
    LinkCapabilities {
        route_get: "LOCAL_INTERFACE_OBSERVATION_ONLY".to_string(),
        dns_compare: "UNSUPPORTED_CAPABILITY".to_string(),
        dns_leak: "UNTESTED".to_string(),
        quic: "UNTESTED".to_string(),
        http_egress: "PRIOR_RUN_ONLY".to_string(),
        connection_edge: "PRIOR_RUN_ONLY".to_string(),
        port_scan: "PROHIBITED".to_string(),
    }
}

fn limitations() -> Vec<String> {
    vec![
        "只读诊断：不切节点、不改系统代理/DNS/hosts/防火墙".to_string(),
        "禁止端口扫描；QUIC 与 DNS 泄漏在本版本未测，明确标为 UNTESTED".to_string(),
        "resolver 对比需要第二个可信 resolver，本版本不启用，因此标为 UNSUPPORTED_CAPABILITY"
            .to_string(),
        "连接边缘与 HTTP 出口只来自既有运行结果，未知字段留空".to_string(),
        "本工具不能证明远端账号、共享出口或独立路径的归属".to_string(),
    ]
}

/// 对内置目录主机做一次显式、可计数的系统 DNS 查询。
///
/// 用户导入目标一律拒绝（需要一次性会话许可，且本命令不消费许可）。查询失败只记录
/// 原因，不重试、不切换到其他 resolver、不发起 HTTP。
pub async fn probe_resolver(
    registry: &Registry,
    report: &mut LinkReport,
    show_ip: bool,
    timeout_ms: u64,
) -> Result<(), ConfigError> {
    if report.source == "FIXTURE" {
        return Err(ConfigError::field(
            "--resolve",
            "离线 fixture 不执行系统 DNS 查询",
        ));
    }
    let timeout = std::time::Duration::from_millis(timeout_ms.max(1));
    probe_resolver_with(registry, report, |host| {
        resolve_host(host, timeout, show_ip)
    })
    .await;
    Ok(())
}

async fn probe_resolver_with<F, Fut>(registry: &Registry, report: &mut LinkReport, mut lookup: F)
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = LinkResolverCell>,
{
    let mut answers = HashMap::<String, LinkResolverCell>::new();
    for row in &mut report.rows {
        let host = match registry.get(&row.target) {
            None => Err("UNKNOWN_TARGET"),
            Some(target) if registry.user_audit_for(target).is_some() => {
                Err("USER_TARGET_REQUIRES_SESSION_GRANT")
            }
            Some(_) if !registry.is_embedded_target(&row.target) => Err("NOT_BUILTIN_TARGET"),
            Some(target) => {
                let host = target.host();
                if host.is_empty() {
                    Err("EMPTY_HOST")
                } else if host != row.host {
                    Err("TARGET_HOST_MISMATCH")
                } else {
                    Ok(host)
                }
            }
        };
        match host {
            Err(reason) => {
                row.resolver = empty_resolver();
                row.resolver.reason = Some(reason.into());
            }
            Ok(host) => {
                if let Some(answer) = answers.get(&host) {
                    row.resolver = answer.clone();
                } else {
                    report.dns_query_count += 1;
                    row.resolver = lookup(host.clone()).await;
                    answers.insert(host, row.resolver.clone());
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn resolve_host(
    host: String,
    timeout: std::time::Duration,
    show_ip: bool,
) -> LinkResolverCell {
    let mut cell = empty_resolver();
    match tokio::time::timeout(timeout, tokio::net::lookup_host((host.as_str(), 443))).await {
        Ok(Ok(addresses)) => {
            let mut v4 = 0u64;
            let mut v6 = 0u64;
            let mut rendered = Vec::new();
            for address in addresses {
                if address.is_ipv4() {
                    v4 += 1;
                } else {
                    v6 += 1;
                }
                if show_ip {
                    rendered.push(address.ip().to_string());
                }
            }
            rendered.sort();
            rendered.dedup();
            cell.status = if v4 + v6 > 0 {
                "RESOLVED".to_string()
            } else {
                "EMPTY_ANSWER".to_string()
            };
            cell.v4 = v4;
            cell.v6 = v6;
            cell.addresses = rendered;
            cell.reason = None;
        }
        Ok(Err(_)) => {
            cell.status = "RESOLVE_FAILED".to_string();
            cell.reason = Some("TRANSPORT_RESOLVE".to_string());
        }
        Err(_) => {
            cell.status = "RESOLVE_TIMEOUT".to_string();
            cell.reason = Some("TRANSPORT_TIMEOUT".to_string());
        }
    }
    cell
}

/// wasm 端没有系统 DNS 查询能力：降级为离线解释，不改判定语义。
#[cfg(target_arch = "wasm32")]
async fn resolve_host(
    _host: String,
    _timeout: std::time::Duration,
    _show_ip: bool,
) -> LinkResolverCell {
    let mut cell = empty_resolver();
    cell.status = "UNTESTED".to_string();
    cell.reason = Some("WASM_OFFLINE".to_string());
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolver_checks_catalog_host_before_sharing_even_failed_answers() {
        let registry = Registry::embedded().unwrap();
        let ids = ["google.flights", "google.travel", "google.search"].map(String::from);
        let mut report = analyze_selection(&registry, &ids, String::new());
        let mismatched = report
            .rows
            .iter_mut()
            .find(|r| r.target == "google.search")
            .unwrap();
        mismatched.host = "fixture.invalid".into();
        mismatched.resolver.addresses.push("old-address".into());
        let mut queried = Vec::new();
        probe_resolver_with(&registry, &mut report, |host| {
            queried.push(host);
            std::future::ready(LinkResolverCell {
                status: "RESOLVE_TIMEOUT".into(),
                ..empty_resolver()
            })
        })
        .await;
        assert_eq!(queried, ["www.google.com"]);
        assert_eq!(report.dns_query_count, 1);
        for row in report.rows {
            if row.target == "google.search" {
                assert_eq!(row.resolver.reason.as_deref(), Some("TARGET_HOST_MISMATCH"));
                assert!(row.resolver.addresses.is_empty());
            } else {
                assert_eq!(row.resolver.status, "RESOLVE_TIMEOUT");
            }
        }
    }
}
