//! `guard`：阶段 4 的只读影响闭包、拒绝性预览与审计。
//!
//! 本模块**不写盘、没有 apply 能力**：`applied` 恒为 false，规则渲染在 P0 恒返回
//! `None`。闭包只使用本地依赖事实，绝不在线枚举新主机，也不按 IP / CDN IP / ASN 阻断。
//!
//! 旧入口 [`preview_plan`] 保留阶段 2 的占位语义（GUI 阶段 3 的演示场景仍在消费它）；
//! 阶段 4 的真实计算走 [`plan_preview`]，它需要目录与依赖图，并额外返回审计记录。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use time::Date;

use crate::contracts::{reason, ClosureNode, ClosureRole, GuardPlan, ResultRecord, State};
use crate::targets::{DependencyEdge, Registry, Target};

/// 审计记录 schema。
pub const GUARD_AUDIT_SCHEMA: &str = "parallaxe.guard-audit/v1";
/// 依赖事实的最大可接受年龄（天）。超过即视为过期，闭包不完整。
pub const DEPENDENCY_MAX_AGE_DAYS: i64 = 365;
/// 闭包节点上限（有穷队列的资源预算）。
pub const MAX_CLOSURE_NODES: usize = 10_000;
/// 闭包扫描边上限。
pub const MAX_EDGES_SCANNED: usize = 100_000;

const REASON_SHARED_CDN: &str = "GUARD_DEFERRED_SHARED_CDN";
const REASON_DEPENDENCY_CONFLICT: &str = "BLOCK_DEFERRED_DEPENDENCY_CONFLICT";

/// 一次只读预览请求。所有字段均来自已完成的运行或静态目录。
#[derive(Debug, Clone)]
pub struct GuardRequest<'a> {
    pub run_id: &'a str,
    pub source_run_id: Option<&'a str>,
    /// 用户显式选择的目标 ID（`selected_by=USER`）。
    pub targets: &'a [String],
    /// 来源运行的结果（同一 TOML / 同一路径）。
    pub results: &'a [ResultRecord],
    /// 审计记录时间（调用方提供，core 不自行取时）。
    pub recorded_at: String,
    /// 计算闭包时用于判断依赖事实是否过期的基准日。
    pub as_of: Date,
    /// 来源运行被取消 / 中断 / 部分完成时为 true。
    pub source_interrupted: bool,
}

/// 预览结果：计划 + 审计。
#[derive(Debug, Clone)]
pub struct GuardPreview {
    pub plan: GuardPlan,
    pub audit: GuardAudit,
}

/// 选择审计：请求、准入候选与被拒项。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GuardSelectionAudit {
    pub requested: Vec<String>,
    pub candidates: Vec<String>,
    pub rejected: Vec<GuardRejection>,
}

/// 单个被拒目标。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GuardRejection {
    pub target: String,
    pub reason: String,
}

/// 预览前后的范围摘要。两侧都只描述内存事实，`rules_written` 恒为 0。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GuardScopeSummary {
    pub affected_services: Vec<String>,
    pub affected_domains: Vec<String>,
    pub affected_shared_paths: Vec<String>,
    pub closure_nodes: usize,
    pub rules_written: u64,
}

/// 回滚状态：预览未写入，因此不需要回滚，也未执行回滚。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GuardRollbackAudit {
    pub required: bool,
    pub performed: bool,
    pub status: String,
    pub note: String,
}

/// 一次只读预览的审计留痕。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GuardAudit {
    pub schema: String,
    pub recorded_at: String,
    pub plan_id: String,
    pub run_id: String,
    pub source_run_id: Option<String>,
    pub operation: String,
    pub selected_by: String,
    pub selection: GuardSelectionAudit,
    pub before: GuardScopeSummary,
    pub after: GuardScopeSummary,
    pub closure_complete: bool,
    pub safe_to_apply: bool,
    pub applied: bool,
    pub result: String,
    pub effect_scope: String,
    pub rollback: GuardRollbackAudit,
}

/// Guard 候选准入（阶段 4 正式规则）：异常状态 + 未被保护 + 高可信 + 非陈旧。
pub fn guard_eligible(result: &ResultRecord) -> bool {
    matches!(result.state, State::Restricted | State::Blocked)
        && result.confidence.meets_guard_floor()
        && result.local_policy.guard_eligible
        && !result.local_policy.protected
        && result
            .evidence
            .iter()
            .all(|item| item.freshness.status != crate::contracts::FreshnessStatus::Stale)
}

/// 阶段 2 兼容入口：不计算闭包，只返回拒绝性纯数据计划。
///
/// 阶段 3 GUI 的演示场景仍使用该函数；阶段 4 的真实计算请用 [`plan_preview`]。
pub fn preview_plan(
    run_id: &str,
    source_run_id: Option<&str>,
    targets: &[String],
    results: &[ResultRecord],
    closure_complete: bool,
) -> GuardPlan {
    let mut reasons: Vec<String> = Vec::new();
    let mut candidates = Vec::new();
    for target in targets {
        match results.iter().find(|result| &result.target == target) {
            Some(result) if guard_eligible(result) => candidates.push(target.clone()),
            Some(_) => reasons.push(format!("{}:{}", target, reason::NOT_GUARD_ELIGIBLE)),
            None => reasons.push(format!("{}:{}", target, reason::INCONCLUSIVE)),
        }
    }
    if !closure_complete || candidates.is_empty() {
        reasons.push(reason::NO_SAFE_BLOCK.to_string());
    }
    GuardPlan {
        plan_id: format!("plan-{run_id}"),
        run_id: run_id.to_string(),
        selected_by: "USER".to_string(),
        operation: "PREVIEW".to_string(),
        candidates,
        affected_domains: Vec::new(),
        affected_services: Vec::new(),
        affected_shared_paths: Vec::new(),
        impact_closure: Vec::<ClosureNode>::new(),
        reasons,
        primary_reason: Some(reason::NO_SAFE_BLOCK.to_string()),
        safe_to_apply: false,
        closure_complete,
        applied: false,
        rollback: vec!["预览未写入任何规则，无需回滚".to_string()],
        limitations: standard_limitations(),
        effect_scope: "LOCAL_TRAFFIC_ONLY".to_string(),
        remote_account_impact: "NOT_PROVABLE".to_string(),
        shared_egress_guarantee: "UNPROVEN".to_string(),
        source_run_id: source_run_id.map(|text| text.to_string()),
    }
}

/// 阶段 4 只读预览：计算影响闭包、三个否决原因优先级与审计记录。
///
/// * 全程只读：`applied=false`、`rules_written=0`，不返回任何可应用片段。
/// * 缺结果、缺边、依赖过期、预算超限或来源中断 → `closure_complete=false` → 否决。
/// * 原因优先级：`GUARD_DEFERRED_SHARED_CDN` > `BLOCK_DEFERRED_DEPENDENCY_CONFLICT`
///   > `NO_SAFE_BLOCK`。仅在没有前两者时才追加 `NO_SAFE_BLOCK`。
pub fn plan_preview(registry: &Registry, request: GuardRequest<'_>) -> GuardPreview {
    let mut closure_complete = !request.source_interrupted;
    let mut reasons: BTreeSet<String> = BTreeSet::new();
    let mut rejected: Vec<GuardRejection> = Vec::new();
    let mut candidates: Vec<String> = Vec::new();

    let mut requested: Vec<String> = request.targets.to_vec();
    requested.sort();
    requested.dedup();

    let mut results_by_target: BTreeMap<&str, &ResultRecord> = BTreeMap::new();
    for result in request.results {
        results_by_target
            .entry(result.target.as_str())
            .or_insert(result);
    }

    let mut walk = ClosureWalk::default();
    for id in &requested {
        let rejection = match (registry.get(id), results_by_target.get(id.as_str())) {
            (Some(target), Some(result))
                if target.raw.protected || result.local_policy.protected =>
            {
                Some(reason::PROTECTED_HOST)
            }
            (Some(_), Some(result)) if !guard_eligible(result) => Some(reason::NOT_GUARD_ELIGIBLE),
            (Some(target), Some(_)) if !target.host().is_empty() => {
                candidates.push(id.clone());
                walk.push(ClosureNode {
                    node: format!("host:{}", target.host()),
                    via: None,
                    kind: "CANDIDATE".into(),
                    source_ref: id.clone(),
                    role: ClosureRole::Affected,
                });
                None
            }
            _ => {
                closure_complete = false;
                Some(reason::INCONCLUSIVE)
            }
        };
        if let Some(reason) = rejection {
            rejected.push(GuardRejection {
                target: id.clone(),
                reason: reason.into(),
            });
        }
    }

    // 同主机的其他服务即使未被选中，也属于反向 ENTRY 闭包。
    let mut owners: BTreeMap<String, Vec<&Target>> = BTreeMap::new();
    let mut adjacent: BTreeMap<&str, Vec<&DependencyEdge>> = BTreeMap::new();
    if !walk.queue.is_empty() {
        for target in registry.entries() {
            owners.entry(target.host()).or_default().push(target);
        }
        for edge in &registry.dependencies.edges {
            adjacent.entry(&edge.src).or_default().push(edge);
            if edge.src != edge.dst {
                adjacent.entry(&edge.dst).or_default().push(edge);
            }
        }
    }
    let mut edges_scanned = 0usize;
    while let Some((node, role)) = walk.queue.pop_front() {
        let edges = adjacent
            .get(node.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let entries = node
            .strip_prefix("host:")
            .and_then(|host| owners.get(host))
            .map(Vec::as_slice)
            .unwrap_or_default();
        edges_scanned += edges.len() + entries.len() + usize::from(node.starts_with("svc:"));
        if walk.exhausted || edges_scanned > MAX_EDGES_SCANNED {
            walk.exhausted = true;
            break;
        }
        if let Some(host) = node.strip_prefix("host:") {
            let cdn = is_shared_cdn(registry, host);
            let protected =
                host_is_protected(registry, host) || entries.iter().any(|t| t.raw.protected);
            if cdn {
                reasons.insert(REASON_SHARED_CDN.to_string());
            }
            if protected || entries.len() > 1 {
                reasons.insert(REASON_DEPENDENCY_CONFLICT.to_string());
            }
            let classification = if cdn {
                Some("SHARED_CDN")
            } else if protected {
                Some("PROTECTED_HOST")
            } else if entries.len() > 1 {
                Some("SHARED_HOST")
            } else {
                None
            };
            if let (Some(kind), Some(entry)) = (
                classification,
                walk.nodes.get_mut(&(node.clone(), role_key(role))),
            ) {
                entry.kind = kind.into();
            }
            for target in entries {
                walk.push(ClosureNode {
                    node: format!("svc:{}", target.id()),
                    via: Some(node.clone()),
                    kind: "ENTRY".into(),
                    source_ref: target.id().into(),
                    role,
                });
            }
        } else if let Some(target) = node.strip_prefix("svc:").and_then(|id| registry.get(id)) {
            // 通过显式依赖到达的上游服务也有自己的入口主机。
            let host_node = format!("host:{}", target.host());
            if !walk
                .nodes
                .contains_key(&(host_node.clone(), role_key(ClosureRole::Affected)))
            {
                walk.push(ClosureNode {
                    node: host_node,
                    via: Some(node.clone()),
                    kind: "ENTRY".into(),
                    source_ref: target.id().into(),
                    role: ClosureRole::Associated,
                });
            }
        }
        for edge in edges {
            if !edge_fresh(edge, request.as_of) {
                closure_complete = false;
            }
            if is_critical_kind(&edge.kind) {
                reasons.insert(REASON_DEPENDENCY_CONFLICT.to_string());
            }
            if edge.dst == node {
                // 反向依赖：依赖当前节点的服务/上游功能会被一起阻断。
                walk.push(ClosureNode {
                    node: edge.src.clone(),
                    via: Some(node.clone()),
                    kind: edge.kind.clone(),
                    source_ref: edge.source.clone(),
                    role,
                });
            }
            if edge.src == node {
                // 正向边：当前节点声明依赖的关键链/后端只作“关联检查”。
                walk.push(ClosureNode {
                    node: edge.dst.clone(),
                    via: Some(node.clone()),
                    kind: edge.kind.clone(),
                    source_ref: edge.source.clone(),
                    role: ClosureRole::Associated,
                });
            }
        }
    }
    if walk.exhausted {
        closure_complete = false;
        reasons.insert(reason::BUDGET_EXHAUSTED.to_string());
    }
    if walk.nodes.is_empty() {
        closure_complete = false;
    }

    let mut reason_list: Vec<String> = reasons.into_iter().collect();
    // 只在前两类否决都不成立时才追加 NO_SAFE_BLOCK：本版目录没有任何
    // “专用路径”证据，因此所有候选都停在③。
    if !reason_list
        .iter()
        .any(|r| r == REASON_SHARED_CDN || r == REASON_DEPENDENCY_CONFLICT)
    {
        reason_list.push(reason::NO_SAFE_BLOCK.to_string());
    }
    reason_list.sort_by_key(|item| reason_rank(item));
    let safe_to_apply = closure_complete && !candidates.is_empty() && reason_list.is_empty();

    // BTreeMap 已按 (节点,角色) 排序，同一名称的双角色条目相邻。
    let closure_nodes: Vec<_> = walk.nodes.into_values().collect();
    let mut affected_domains = Vec::new();
    let mut affected_services = Vec::new();
    for node in &closure_nodes {
        if let Some(host) = node.node.strip_prefix("host:") {
            affected_domains.push(host.to_string());
        } else if let Some(service) = node.node.strip_prefix("svc:") {
            affected_services.push(service.to_string());
        }
    }
    affected_domains.dedup();
    affected_services.dedup();

    let mut shared_paths: Vec<String> = Vec::new();
    for node in &closure_nodes {
        let Some(host) = node.node.strip_prefix("host:") else {
            if is_critical_kind(&node.kind) {
                shared_paths.push(format!(
                    "{} → {} ({})",
                    node.via.as_deref().unwrap_or("?"),
                    node.node,
                    node.kind
                ));
            }
            continue;
        };
        match node.kind.as_str() {
            "SHARED_HOST" => shared_paths.push(format!("{host} (SHARED_HOST)")),
            "SHARED_CDN" => shared_paths.push(format!("{host} (SHARED_CDN)")),
            "PROTECTED_HOST" => shared_paths.push(format!("{host} (PROTECTED)")),
            kind if is_critical_kind(kind) => shared_paths.push(format!(
                "{} → {} ({kind})",
                node.via.as_deref().unwrap_or("?"),
                node.node
            )),
            _ => {}
        }
    }
    shared_paths.sort();
    shared_paths.dedup();

    let primary_reason = reason_list.first().cloned();
    let plan_id = format!("plan-{}", request.run_id);

    let before = GuardScopeSummary {
        affected_services: vec![],
        affected_domains: vec![],
        affected_shared_paths: vec![],
        closure_nodes: 0,
        rules_written: 0,
    };
    let after = GuardScopeSummary {
        affected_services: affected_services.clone(),
        affected_domains: affected_domains.clone(),
        affected_shared_paths: shared_paths.clone(),
        closure_nodes: closure_nodes.len(),
        rules_written: 0,
    };

    let plan = GuardPlan {
        plan_id: plan_id.clone(),
        run_id: request.run_id.to_string(),
        selected_by: "USER".to_string(),
        operation: "PREVIEW".to_string(),
        candidates: candidates.clone(),
        affected_domains,
        affected_services,
        affected_shared_paths: shared_paths,
        impact_closure: closure_nodes,
        reasons: reason_list.clone(),
        primary_reason,
        safe_to_apply,
        closure_complete,
        applied: false,
        rollback: vec!["预览未写入任何规则，无需回滚".to_string()],
        limitations: standard_limitations(),
        effect_scope: "LOCAL_TRAFFIC_ONLY".to_string(),
        remote_account_impact: "NOT_PROVABLE".to_string(),
        shared_egress_guarantee: "UNPROVEN".to_string(),
        source_run_id: request.source_run_id.map(|text| text.to_string()),
    };

    let audit = GuardAudit {
        schema: GUARD_AUDIT_SCHEMA.to_string(),
        recorded_at: request.recorded_at,
        plan_id,
        run_id: request.run_id.to_string(),
        source_run_id: request.source_run_id.map(|text| text.to_string()),
        operation: "PREVIEW".to_string(),
        selected_by: "USER".to_string(),
        selection: GuardSelectionAudit {
            requested,
            candidates,
            rejected,
        },
        before,
        after,
        closure_complete,
        safe_to_apply,
        applied: false,
        result: if safe_to_apply {
            "PREVIEW_READY_NOT_APPLIED".to_string()
        } else {
            "PREVIEW_REJECTED".to_string()
        },
        effect_scope: "LOCAL_TRAFFIC_ONLY".to_string(),
        rollback: GuardRollbackAudit {
            required: false,
            performed: false,
            status: "NOT_REQUIRED".to_string(),
            note: "只读预览未写入任何规则；本版本没有 apply 能力，也没有回滚动作".to_string(),
        },
    };

    GuardPreview { plan, audit }
}

/// P0 规则渲染：始终 `None`。
///
/// 目录没有任何“专用路径”证据，`safe_to_apply` 恒为 false；即便未来它为 true，
/// 本版本也明确没有写入器与 apply 能力（06 文档 §5）。
pub fn render_rule_preview(_plan: &GuardPlan) -> Option<String> {
    None
}

fn standard_limitations() -> Vec<String> {
    vec![
        "只读预览：未写入任何规则，本版本没有 apply 能力".to_string(),
        "本工具只减少本机流量，不能改变远端账号判定或共享出口画像".to_string(),
        "共享 CDN、登录、付款、下载链默认保护，不能作为阻断候选".to_string(),
        "闭包只基于本地依赖事实，缺边、过期或中断即否决，不开展在线枚举".to_string(),
        "禁止按 IP / CDN IP / ASN 生成阻断".to_string(),
    ]
}

#[derive(Default)]
struct ClosureWalk {
    nodes: BTreeMap<(String, u8), ClosureNode>,
    queue: VecDeque<(String, ClosureRole)>,
    exhausted: bool,
}

impl ClosureWalk {
    fn push(&mut self, node: ClosureNode) {
        let key = (node.node.clone(), role_key(node.role));
        if self.nodes.contains_key(&key) {
            return;
        }
        if self.nodes.len() >= MAX_CLOSURE_NODES {
            self.exhausted = true;
            return;
        }
        self.queue.push_back((node.node.clone(), node.role));
        self.nodes.insert(key, node);
    }
}

fn role_key(role: ClosureRole) -> u8 {
    match role {
        ClosureRole::Affected => 0,
        ClosureRole::Associated => 1,
    }
}

fn reason_rank(reason_text: &str) -> u8 {
    match reason_text {
        REASON_SHARED_CDN => 0,
        REASON_DEPENDENCY_CONFLICT => 1,
        "NO_SAFE_BLOCK" => 2,
        _ => 3,
    }
}

fn is_critical_kind(kind: &str) -> bool {
    matches!(
        kind,
        "LOGIN"
            | "OAUTH"
            | "PAYMENT"
            | "DOWNLOAD"
            | "UPDATE"
            | "SHARED_BACKEND"
            | "REQUIRES"
            | "REDIRECT"
    )
}

fn edge_fresh(edge: &DependencyEdge, as_of: Date) -> bool {
    let Some(raw) = edge.verified_at.date else {
        return false;
    };
    let Ok(month) = time::Month::try_from(raw.month) else {
        return false;
    };
    let Ok(date) = Date::from_calendar_date(raw.year.into(), month, raw.day) else {
        return false;
    };
    let age = (as_of - date).whole_days();
    (0..=DEPENDENCY_MAX_AGE_DAYS).contains(&age)
}

fn is_shared_cdn(registry: &Registry, host: &str) -> bool {
    let cdn = &registry.dependencies.classifications;
    cdn.cdn_exact.iter().any(|item| host == item)
        || cdn
            .cdn_suffixes
            .iter()
            .any(|suffix| host_under(host, suffix))
}

fn host_is_protected(registry: &Registry, host: &str) -> bool {
    let seeds = &registry.dependencies.seeds;
    seeds.exact.iter().any(|item| host == item)
        || seeds.suffixes.iter().any(|suffix| host_under(host, suffix))
        || seeds
            .protected_families
            .iter()
            .any(|family| family_matches(family, host))
}

fn host_under(host: &str, suffix: &str) -> bool {
    host == suffix
        || host
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn valid_dns_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// 保护谓词（06 文档 §1）。只用于**保护**，不生成任何通配阻断。
fn family_matches(family: &str, host: &str) -> bool {
    match family {
        "google-account-country" => host
            .strip_prefix("accounts.google.")
            .is_some_and(valid_dns_suffix),
        "mtalk-label" => host
            .strip_suffix(".google.com")
            .is_some_and(|label| !label.contains('.') && label.starts_with("mtalk")),
        "mtalk-alt-1-8" => host.strip_suffix(".google.com").is_some_and(|label| {
            !label.contains('.') && (1..=8).any(|index| label == format!("alt{index}-mtalk"))
        }),
        "clients-1-6" => host.strip_suffix(".google.com").is_some_and(|label| {
            !label.contains('.') && (1..=6).any(|index| label == format!("clients{index}"))
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::Registry;

    #[test]
    fn shared_cdn_and_protection_predicates_and_reason_priority() {
        let registry = Registry::embedded().unwrap();
        assert!(is_shared_cdn(&registry, "dl.google.com"));
        assert!(is_shared_cdn(&registry, "gstatic.com"));
        assert!(is_shared_cdn(&registry, "fonts.gstatic.com"));
        assert!(!is_shared_cdn(&registry, "gemini.google.com"));
        assert!(host_is_protected(&registry, "accounts.google.com"));
        assert!(host_is_protected(&registry, "mtalk.google.com"));
        assert!(!host_is_protected(&registry, "gemini.google.com"));
        assert!(reason_rank(REASON_SHARED_CDN) < reason_rank(REASON_DEPENDENCY_CONFLICT));
        assert!(reason_rank(REASON_DEPENDENCY_CONFLICT) < reason_rank("NO_SAFE_BLOCK"));
    }

    #[test]
    fn protected_families_are_conservative() {
        assert!(family_matches(
            "google-account-country",
            "accounts.google.com"
        ));
        assert!(family_matches(
            "google-account-country",
            "accounts.google.co.uk"
        ));
        // 保护是保守方向：宁可多保护，也不放过可疑别名。
        assert!(family_matches(
            "google-account-country",
            "accounts.google.com.evil"
        ));
        assert!(!family_matches("google-account-country", "accounts.google"));
        assert!(!family_matches(
            "google-account-country",
            "accounts.google.evil!"
        ));
        assert!(family_matches("mtalk-label", "mtalk.google.com"));
        assert!(family_matches("mtalk-alt-1-8", "alt8-mtalk.google.com"));
        assert!(!family_matches("mtalk-alt-1-8", "alt9-mtalk.google.com"));
        assert!(family_matches("clients-1-6", "clients6.google.com"));
        assert!(!family_matches("clients-1-6", "clients7.google.com"));
        assert!(!family_matches("mtalk-label", "xmtalk.google.com"));
    }
}
