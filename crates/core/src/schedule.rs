//! `schedule`：T-7-09a 统一调度器与运行前风险预览。
//!
//! 调度键不是只有目标 host，而是 `(host, 节点, 协议, 会话类型, 共享基础设施组, 本地实例)`。
//! 实际发送时刻由调度器统一分配，任务内不再自行 `sleep`，避免并发任务同时醒来。
//! 生成计划与风险预览本身**零发包**。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::forensics::ProbeProtocol;
use crate::probe::FetchRequest;
use crate::targets::{Budget, HARD_LIMITS};

/// 多维调度键。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScheduleKey {
    pub host: String,
    pub node: String,
    pub protocol: ProbeProtocol,
    pub session: String,
    pub shared_group: String,
    pub instance: String,
}

impl ScheduleKey {
    /// 当前路径模式下未知节点/共享组：使用显式占位，不猜具体节点。
    pub fn current_path(host: &str, protocol: ProbeProtocol) -> Self {
        ScheduleKey {
            host: host.to_ascii_lowercase(),
            node: "CURRENT_PATH".to_string(),
            protocol,
            session: protocol.as_str().to_string(),
            shared_group: "UNKNOWN".to_string(),
            instance: "LOCAL".to_string(),
        }
    }
}

/// 调度槽：请求索引与相对发送偏移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendSlot {
    pub index: usize,
    pub offset_ms: u64,
    pub key: ScheduleKey,
}

fn effective_interval(budget: &Budget) -> u64 {
    budget.min_interval_ms.min(HARD_LIMITS.max_min_interval_ms)
}

fn effective_batch_size(budget: &Budget) -> u32 {
    budget.batch_size.clamp(1, HARD_LIMITS.max_batch_size)
}

fn effective_inter_target_interval(budget: &Budget) -> u64 {
    budget
        .inter_target_interval_ms
        .min(HARD_LIMITS.max_inter_target_interval_ms)
}

fn effective_cooldown(budget: &Budget) -> u64 {
    budget
        .batch_cooldown_ms
        .min(HARD_LIMITS.max_batch_cooldown_ms)
}

/// 为一批已准入请求分配统一发送时刻。
///
/// * 同一 host / 节点 / 协议 / 会话：第 n 个偏移 `n * min_interval_ms`；
/// * UDP/QUIC 每满 `batch_size` 再追加一次 `batch_cooldown_ms`；
/// * 不同 host 之间：每个新目标首次请求按 `inter_target_interval_ms` 逐目标错开
///   （0 时保持原有"互不延迟"行为，可由用户调节至上限 10s）。
pub fn plan_slots(requests: &[FetchRequest], budget: &Budget) -> Vec<SendSlot> {
    let interval = effective_interval(budget);
    let inter_target = effective_inter_target_interval(budget);
    let batch_size = effective_batch_size(budget);
    let cooldown = effective_cooldown(budget);
    let mut seen: BTreeMap<ScheduleKey, u32> = BTreeMap::new();
    // 跨目标间隔：每个新 host 的首次请求按出现顺序错开 inter_target。
    // 同一 host 的后续请求不重复叠加，避免双重延迟（其间隔由 min_interval 负责）。
    let mut host_order: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_host_rank: u32 = 0;
    let mut slots = Vec::with_capacity(requests.len());
    for (index, request) in requests.iter().enumerate() {
        let key = ScheduleKey::current_path(&request.host, request.protocol);
        let is_new_host = !host_order.contains_key(&request.host);
        let host_rank = if is_new_host {
            let rank = next_host_rank;
            host_order.insert(request.host.clone(), rank);
            next_host_rank += 1;
            rank
        } else {
            *host_order.get(&request.host).unwrap_or(&0)
        };
        let ordinal = seen.entry(key.clone()).or_insert(0);
        let base = u64::from(*ordinal) * interval
            + if is_new_host {
                u64::from(host_rank) * inter_target
            } else {
                0
            };
        let cooldown_batches = if key.protocol == ProbeProtocol::UdpQuic {
            u64::from(*ordinal) / u64::from(batch_size)
        } else {
            0
        };
        slots.push(SendSlot {
            index,
            offset_ms: base + cooldown_batches * cooldown,
            key,
        });
        *ordinal += 1;
    }
    slots
}

/// 风险预览的单个计划目标（此处只描述，不发起请求）。
#[derive(Debug, Clone)]
pub struct PlannedTarget {
    pub target_id: String,
    pub host: String,
    pub protocol: ProbeProtocol,
}

/// 运行前风险预览（T-7-09a）：目标/节点/协议分布、预计新连接与会话、并发与耗时。
#[derive(Debug, Clone, Serialize)]
pub struct NetworkRiskPreview {
    pub schema: String,
    pub profile: Option<String>,
    pub scope: String,
    pub scan_mode: String,
    pub requires_confirmation: bool,
    pub target_count: usize,
    pub node_count: usize,
    pub tcp_http_targets: usize,
    pub udp_quic_targets: usize,
    pub dns_queries: usize,
    pub estimated_new_connections: usize,
    pub estimated_udp_quic_sessions: usize,
    pub max_concurrency: u32,
    pub tcp_concurrency: u32,
    pub udp_quic_concurrency: u32,
    pub min_interval_ms: u64,
    /// 跨目标（不同 host）检验之间的间隔（用户可调）。
    pub inter_target_interval_ms: u64,
    pub batch_size: u32,
    pub batch_cooldown_ms: u64,
    pub node_retest: u32,
    pub estimated_wall_ms: u64,
    pub high_risk_protocols: Vec<String>,
    pub zero_packet: bool,
    pub note: String,
}

/// 由计划目标与预算生成风险预览；不触发任何网络 I/O。
pub fn risk_preview(
    profile: Option<&str>,
    scope: &str,
    planned: &[PlannedTarget],
    budget: &Budget,
    node_sweep: bool,
) -> NetworkRiskPreview {
    let tcp_http_targets = planned
        .iter()
        .filter(|item| item.protocol == ProbeProtocol::TcpHttp)
        .count();
    let udp_quic_targets = planned.len().saturating_sub(tcp_http_targets);
    let mut per_host: BTreeMap<&str, u32> = BTreeMap::new();
    for item in planned {
        *per_host.entry(item.host.as_str()).or_insert(0) += 1;
    }
    let interval = effective_interval(budget);
    let batch_size = effective_batch_size(budget);
    let cooldown = effective_cooldown(budget);
    let mut estimated_wall_ms = budget
        .request_timeout_ms
        .min(HARD_LIMITS.max_request_timeout_ms);
    for count in per_host.values() {
        let extra = u64::from(count.saturating_sub(1)) * interval;
        let udp_batches = if udp_quic_targets > 0 {
            u64::from(*count) / u64::from(batch_size)
        } else {
            0
        };
        estimated_wall_ms =
            estimated_wall_ms.max(budget.request_timeout_ms + extra + udp_batches * cooldown);
    }
    let mut high_risk_protocols = Vec::new();
    if udp_quic_targets > 0 {
        high_risk_protocols.push("UDP_QUIC".to_string());
    }
    NetworkRiskPreview {
        schema: "parallaxe.network-plan/v1".to_string(),
        profile: profile.map(str::to_string),
        scope: scope.to_string(),
        scan_mode: if node_sweep {
            "NODE_SWEEP".to_string()
        } else {
            match profile {
                Some("simple") => "SIMPLE".to_string(),
                Some("google-all") => "GOOGLE_ALL".to_string(),
                Some("full") => "CONTROLLED_FULL".to_string(),
                _ => "CUSTOM".to_string(),
            }
        },
        requires_confirmation: planned.len() > 30,
        target_count: planned.len(),
        node_count: 0,
        tcp_http_targets,
        udp_quic_targets,
        // 当前 HTTP-only 传输不发独立 DNS 探测；解析随请求发生，不单列。
        dns_queries: 0,
        estimated_new_connections: planned.len(),
        estimated_udp_quic_sessions: 0,
        max_concurrency: budget.max_concurrency.min(HARD_LIMITS.max_concurrency),
        tcp_concurrency: budget
            .tcp_concurrency
            .min(HARD_LIMITS.max_tcp_concurrency)
            .min(budget.max_concurrency),
        udp_quic_concurrency: budget
            .udp_quic_concurrency
            .min(HARD_LIMITS.max_udp_quic_concurrency),
        min_interval_ms: interval,
        inter_target_interval_ms: effective_inter_target_interval(budget),
        batch_size,
        batch_cooldown_ms: cooldown,
        node_retest: budget.node_retest.min(HARD_LIMITS.max_node_retest),
        estimated_wall_ms,
        high_risk_protocols,
        zero_packet: true,
        note: "本预览只描述计划与预算，不发起任何网络请求；节点信息来自当前路径，不做自动选择/故障转移。"
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::AttemptFamily;

    fn request(host: &str, path: &str) -> FetchRequest {
        FetchRequest {
            target_id: format!("{host}{path}"),
            url: format!("https://{host}{path}"),
            host: host.to_string(),
            method: "GET".to_string(),
            family: AttemptFamily::Auto,
            protocol: ProbeProtocol::TcpHttp,
            timeout_ms: 2000,
            max_body_bytes: 1024,
            max_redirects: 0,
        }
    }

    fn budget() -> Budget {
        Budget {
            min_interval_ms: 100,
            batch_size: 2,
            batch_cooldown_ms: 1000,
            request_timeout_ms: 2000,
            ..Budget::default()
        }
    }

    #[test]
    fn same_host_is_spaced_but_distinct_hosts_are_not() {
        let requests = vec![
            request("a.example", "/1"),
            request("a.example", "/2"),
            request("a.example", "/3"),
            request("b.example", "/1"),
        ];
        let slots = plan_slots(&requests, &budget());
        assert_eq!(slots[0].offset_ms, 0);
        assert_eq!(slots[1].offset_ms, 100);
        assert_eq!(slots[2].offset_ms, 200);
        assert_eq!(slots[3].offset_ms, 0, "不同 host 不互相延迟");
        assert_eq!(slots[0].key.host, "a.example");
    }

    #[test]
    fn inter_target_interval_spaces_distinct_hosts_and_caps_at_hard_limit() {
        let requests = vec![
            request("books.google.com", "/1"),
            request("books.google.com", "/2"),
            request("one.google.com", "/1"),
            request("gemini.google.com", "/1"),
        ];
        let mut b = budget();
        b.inter_target_interval_ms = 500;
        let slots = plan_slots(&requests, &b);
        // 第一个 host rank=0 → 0；同 host 第二次由 min_interval 负责(100)
        assert_eq!(slots[0].offset_ms, 0);
        assert_eq!(slots[1].offset_ms, 100);
        // 第二个新 host rank=1 → 1*500；第三个 rank=2 → 2*500
        assert_eq!(slots[2].offset_ms, 500);
        assert_eq!(slots[3].offset_ms, 1_000);

        // 超过硬上限时封顶到 10000
        let mut capped = budget();
        capped.inter_target_interval_ms = HARD_LIMITS.max_inter_target_interval_ms + 5_000;
        let capped_slots = plan_slots(&requests, &capped);
        assert_eq!(
            capped_slots[2].offset_ms,
            HARD_LIMITS.max_inter_target_interval_ms
        );
        assert_eq!(
            capped_slots[3].offset_ms,
            2 * HARD_LIMITS.max_inter_target_interval_ms
        );

        // 默认 0 时不同 host 不额外延迟（向后兼容）
        let default_slots = plan_slots(&requests, &budget());
        assert_eq!(default_slots[2].offset_ms, 0);
        assert_eq!(default_slots[3].offset_ms, 0);
    }

    #[test]
    fn udp_batches_get_cooldown_and_are_not_sent_concurrently() {
        let mut requests = Vec::new();
        for index in 0..5 {
            let mut item = request("q.example", &format!("/{index}"));
            item.protocol = ProbeProtocol::UdpQuic;
            requests.push(item);
        }
        let slots = plan_slots(&requests, &budget());
        let offsets: Vec<u64> = slots.iter().map(|slot| slot.offset_ms).collect();
        assert_eq!(offsets, vec![0, 100, 1200, 1300, 2400]);
    }

    #[test]
    fn risk_preview_is_zero_packet_and_counts_protocols() {
        let planned = vec![
            PlannedTarget {
                target_id: "a".to_string(),
                host: "a.example".to_string(),
                protocol: ProbeProtocol::TcpHttp,
            },
            PlannedTarget {
                target_id: "b".to_string(),
                host: "a.example".to_string(),
                protocol: ProbeProtocol::TcpHttp,
            },
            PlannedTarget {
                target_id: "c".to_string(),
                host: "q.example".to_string(),
                protocol: ProbeProtocol::UdpQuic,
            },
        ];
        let preview = risk_preview(Some("simple"), "CURRENT_PATH", &planned, &budget(), false);
        assert!(preview.zero_packet);
        assert_eq!(preview.target_count, 3);
        assert_eq!(preview.tcp_http_targets, 2);
        assert_eq!(preview.udp_quic_targets, 1);
        assert_eq!(preview.estimated_new_connections, 3);
        assert_eq!(preview.estimated_udp_quic_sessions, 0);
        assert!(preview.estimated_wall_ms >= 2000);
        assert_eq!(preview.high_risk_protocols, vec!["UDP_QUIC".to_string()]);
        assert_eq!(preview.node_count, 0);
        assert_eq!(preview.scan_mode, "SIMPLE");
        assert!(!preview.requires_confirmation);
    }
}
