//! `sysblock`：T-7-08 Windows / Linux 应用级系统阻断的**计划与命令模型**。
//!
//! 本模块审查候选域名并构建预览。显式解析入口通过系统 DNS 获取地址，防火墙写入由 CLI 执行。
//! 红线：
//! * 只阻断经 Guard 判定、非保护名单、非共享 CDN 的精确 FQDN 所解析出的 IP；
//! * 禁止按 IP / CDN IP / ASN 泛阻断，也拒绝把 IP 字面量当作 FQDN；
//! * 启用需要用户确认，撤销通过 CLI 核验本次运行的已知规则。
//!
//! 由于当前目录没有“专用路径”证据，`GuardPlan::safe_to_apply` 通常为 false，
//! 因此本模块会诚实地返回 `SYSBLOCK_NOT_SAFE`，而不是替用户放宽闸门。

use std::collections::BTreeSet;
use std::net::IpAddr;

use serde::Serialize;

use crate::contracts::GuardPlan;
use crate::guard_export::valid_fqdn;
use crate::targets::Registry;

/// 测试版统一关闭系统写入入口，独立于平台及计划是否满足安全条件。
pub const SYSTEM_BLOCK_ENABLED: bool = false;

/// 系统阻断计划 schema。
pub const SYSBLOCK_SCHEMA: &str = "parallaxe.sysblock/v1";

/// 支持的平台：仅 Windows / Linux（macOS/Android/iOS 不做系统阻断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
}

impl Platform {
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "windows") {
            Some(Platform::Windows)
        } else if cfg!(target_os = "linux") {
            Some(Platform::Linux)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Linux => "linux",
        }
    }

    fn engine(self) -> &'static str {
        match self {
            Platform::Windows => "WINDOWS_NETSECURITY",
            Platform::Linux => "NFTABLES",
        }
    }
}

/// DNS 解析结果（由调用方显式提供或显式解析）。
#[derive(Debug, Clone)]
pub struct ResolvedHost {
    pub fqdn: String,
    pub addresses: Vec<IpAddr>,
}

/// 单个阻断项：精确 FQDN → 精确 IP。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SysBlockEntry {
    pub fqdn: String,
    pub ip: String,
    pub family: &'static str,
}

/// 系统阻断计划：含影响范围、执行/回滚命令与审计字段。
#[derive(Debug, Clone, Serialize)]
pub struct SysBlockPlan {
    pub schema: String,
    pub rule_tag: String,
    pub platform: String,
    pub engine: String,
    pub created_at: String,
    pub source_plan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    pub entries: Vec<SysBlockEntry>,
    pub requires_admin: bool,
    pub warnings: Vec<String>,
    pub apply_commands: Vec<String>,
    pub rollback_commands: Vec<String>,
    pub effect_scope: String,
    pub zero_residue: bool,
}

/// 拒绝原因（稳定机器码）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysBlockError {
    pub code: &'static str,
    pub detail: String,
}

impl SysBlockError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        SysBlockError {
            code,
            detail: detail.into(),
        }
    }
}

/// 校验规则标签：只允许小写字母、数字与连字符，避免命令注入。
pub fn valid_rule_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 32
        && tag
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn routable(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast())
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|ip| !routable(ip.into())))
        }
    }
}

/// 在任何 DNS 操作前验证计划，返回允许解析的候选域名。
pub fn candidate_hosts(
    registry: &Registry,
    plan: &GuardPlan,
    rule_tag: &str,
) -> Result<BTreeSet<String>, SysBlockError> {
    if !valid_rule_tag(rule_tag) {
        return Err(SysBlockError::new(
            "SYSBLOCK_INVALID_TAG",
            "规则标签只允许小写字母、数字与连字符（≤32）",
        ));
    }
    if !plan.safe_to_apply {
        return Err(SysBlockError::new(
            "SYSBLOCK_NOT_SAFE",
            "Guard 计划未通过安全闸门（safe_to_apply=false）：不写系统。",
        ));
    }
    if !plan.closure_complete || !plan.reasons.is_empty() || !plan.affected_shared_paths.is_empty()
    {
        return Err(SysBlockError::new(
            "SYSBLOCK_CLOSURE_INCOMPLETE",
            "影响闭包不完整或仍有未解除的闸门原因：未知一律不放行。",
        ));
    }
    if plan.candidates.is_empty() {
        return Err(SysBlockError::new(
            "SYSBLOCK_NO_CANDIDATES",
            "没有经过判定可阻断的候选目标。",
        ));
    }
    let mut candidate_hosts: BTreeSet<String> = BTreeSet::new();
    for id in &plan.candidates {
        if id.is_empty()
            || id.starts_with('-')
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return Err(SysBlockError::new(
                "SYSBLOCK_UNKNOWN_CANDIDATE",
                "候选 ID 无效",
            ));
        }
        let Some(target) = registry.get(id) else {
            return Err(SysBlockError::new(
                "SYSBLOCK_UNKNOWN_CANDIDATE",
                format!("候选目标 {id} 不在目录中"),
            ));
        };
        if target.raw.protected {
            return Err(SysBlockError::new(
                "SYSBLOCK_PROTECTED",
                format!("候选 {} 属于保护名单：拒绝阻断", target.host()),
            ));
        }
        let host = target.host().trim_end_matches('.').to_ascii_lowercase();
        if !valid_fqdn(&host) || !plan.affected_domains.contains(&host) {
            return Err(SysBlockError::new(
                "SYSBLOCK_HOST_OUTSIDE_CLOSURE",
                "候选域名未经影响闭包确认。",
            ));
        }
        candidate_hosts.insert(host);
    }
    Ok(candidate_hosts)
}

/// 构建系统阻断计划。
///
/// 必须满足：`safe_to_apply && closure_complete && reasons.is_empty()`，候选非空，
/// 且每条解析记录都对应候选 FQDN；任何不符都 fail-closed。
pub fn build_plan(
    registry: &Registry,
    plan: &GuardPlan,
    resolved: &[ResolvedHost],
    platform: Platform,
    created_at: impl Into<String>,
    rule_tag: &str,
) -> Result<SysBlockPlan, SysBlockError> {
    let candidate_hosts = candidate_hosts(registry, plan, rule_tag)?;
    if resolved.is_empty() {
        return Err(SysBlockError::new(
            "SYSBLOCK_NO_RESOLVED",
            "没有可用的解析结果：需要显式解析或提供 host=ip；不允许凭空猜 IP。",
        ));
    }
    let mut entries: BTreeSet<(String, String, &'static str)> = BTreeSet::new();
    for host in resolved {
        let fqdn = host.fqdn.trim_end_matches('.').to_ascii_lowercase();
        if fqdn.parse::<IpAddr>().is_ok() {
            return Err(SysBlockError::new(
                "SYSBLOCK_IP_LITERAL",
                "拒绝把 IP 字面量当作阻断对象；必须来自判定过的 FQDN。",
            ));
        }
        if !valid_fqdn(&fqdn) {
            return Err(SysBlockError::new(
                "SYSBLOCK_NOT_FQDN",
                format!("{fqdn} 不是合法 FQDN"),
            ));
        }
        if !candidate_hosts.contains(&fqdn) {
            return Err(SysBlockError::new(
                "SYSBLOCK_HOST_NOT_CANDIDATE",
                format!("{fqdn} 不在本计划的候选 FQDN 内：拒绝扩大阻断范围"),
            ));
        }
        for address in &host.addresses {
            if !routable(*address) {
                return Err(SysBlockError::new(
                    "SYSBLOCK_NON_ROUTABLE",
                    format!("{fqdn} 解析到非公网地址 {address}：拒绝阻断"),
                ));
            }
            let family = if address.is_ipv6() { "V6" } else { "V4" };
            entries.insert((fqdn.clone(), address.to_string(), family));
        }
    }
    if entries.is_empty() {
        return Err(SysBlockError::new(
            "SYSBLOCK_NO_RESOLVED",
            "候选 FQDN 没有解析出任何可用地址。",
        ));
    }
    let entries: Vec<SysBlockEntry> = entries
        .into_iter()
        .map(|(fqdn, ip, family)| SysBlockEntry { fqdn, ip, family })
        .collect();
    if entries
        .iter()
        .map(|e| &e.fqdn)
        .collect::<BTreeSet<_>>()
        .len()
        != candidate_hosts.len()
    {
        return Err(SysBlockError::new(
            "SYSBLOCK_NO_RESOLVED",
            "每个已审查候选都需要解析地址。",
        ));
    }
    // CLI re-reads the source report and resolves again in the administrator's terminal.
    let targets = plan
        .candidates
        .iter()
        .map(|id| format!("--target {id}"))
        .collect::<Vec<_>>()
        .join(" ");
    let apply_commands = vec![format!("parallaxe guard block {targets} --source-report report.json --resolve --apply --confirm --rule-tag {rule_tag}")];
    let rollback_commands = unblock_commands(platform, rule_tag)?;
    Ok(SysBlockPlan {
        schema: SYSBLOCK_SCHEMA.to_string(),
        rule_tag: format!("parallaxe-block-{rule_tag}"),
        platform: platform.as_str().to_string(),
        engine: platform.engine().to_string(),
        created_at: created_at.into(),
        source_plan_id: plan.plan_id.clone(),
        source_run_id: plan.source_run_id.clone(),
        entries,
        requires_admin: true,
        warnings: vec![
            "本操作需要管理员 / root 权限；只阻断上述精确 FQDN 解析出的公网 IP。".to_string(),
            "系统层拦的是不走代理的流量（全局模式直连）；走代理的流量由规则层 REJECT 处理，两者互补。".to_string(),
            "不按 IP 段 / CDN IP / ASN 泛阻断；CDN IP 轮换后需重新解析并重新生成计划。".to_string(),
            "请先将本次源报告保存为 report.json。撤销只核实本运行已知规则。".to_string(),
        ],
        apply_commands,
        rollback_commands,
        effect_scope: "LOCAL_OS_EGRESS_ONLY".to_string(),
        zero_residue: false,
    })
}

/// 只公开带所有权核验的 CLI 入口。
pub fn unblock_commands(_platform: Platform, tag: &str) -> Result<Vec<String>, SysBlockError> {
    if !valid_rule_tag(tag) {
        return Err(SysBlockError::new("SYSBLOCK_INVALID_TAG", "规则标签无效"));
    }
    Ok(vec![format!(
        "parallaxe guard unblock --rule-tag {tag} --apply"
    )])
}

/// 审查先于 resolver，解析失败或部分结果均不构造计划。
pub fn resolve_plan_with(
    registry: &Registry,
    plan: &GuardPlan,
    platform: Platform,
    now: String,
    tag: &str,
    mut resolve: impl FnMut(&str) -> Result<Vec<IpAddr>, String>,
) -> Result<SysBlockPlan, SysBlockError> {
    let mut resolved = Vec::new();
    for fqdn in candidate_hosts(registry, plan, tag)? {
        let addresses =
            resolve(&fqdn).map_err(|msg| SysBlockError::new("SYSBLOCK_RESOLVE_FAILED", msg))?;
        resolved.push(ResolvedHost { fqdn, addresses });
    }
    build_plan(registry, plan, &resolved, platform, now, tag)
}

pub fn resolve_plan(
    registry: &Registry,
    plan: &GuardPlan,
    platform: Platform,
    now: String,
    tag: &str,
) -> Result<SysBlockPlan, SysBlockError> {
    use std::net::ToSocketAddrs;
    resolve_plan_with(registry, plan, platform, now, tag, |host| {
        (host, 443)
            .to_socket_addrs()
            .map(|iter| iter.map(|a| a.ip()).collect())
            .map_err(|_| format!("无法解析 {host}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn safe_plan() -> GuardPlan {
        GuardPlan {
            plan_id: "plan-safe".to_string(),
            run_id: "run-safe".to_string(),
            selected_by: "USER".to_string(),
            operation: "PREVIEW".to_string(),
            candidates: vec!["google.gemini".to_string()],
            affected_domains: vec!["support.google.com".to_string()],
            affected_services: Vec::new(),
            affected_shared_paths: Vec::new(),
            impact_closure: Vec::new(),
            reasons: Vec::new(),
            primary_reason: None,
            safe_to_apply: true,
            closure_complete: true,
            applied: false,
            rollback: Vec::new(),
            limitations: Vec::new(),
            effect_scope: "LOCAL_TRAFFIC_ONLY".to_string(),
            remote_account_impact: "NOT_PROVABLE".to_string(),
            shared_egress_guarantee: "UNPROVEN".to_string(),
            source_run_id: Some("run-source".to_string()),
        }
    }

    fn resolved() -> Vec<ResolvedHost> {
        vec![ResolvedHost {
            fqdn: "support.google.com".to_string(),
            addresses: vec![
                "203.0.113.10".parse().unwrap(),
                "2001:db8:1::10".parse().unwrap(),
            ],
        }]
    }

    #[test]
    fn builds_exact_fqdn_plan_with_reversible_commands() {
        let registry = Registry::embedded().unwrap();
        let plan = build_plan(
            &registry,
            &safe_plan(),
            &resolved(),
            Platform::Windows,
            "2026-11-23T00:00:00Z",
            "test",
        )
        .expect("plan");
        assert_eq!(plan.entries.len(), 2);
        assert!(plan
            .entries
            .iter()
            .all(|entry| entry.fqdn == "support.google.com"));
        assert_eq!(plan.apply_commands.len(), 1);
        assert!(plan.apply_commands[0].contains("guard block --target google.gemini"));
        assert_eq!(
            plan.rollback_commands,
            ["parallaxe guard unblock --rule-tag test --apply"]
        );
        assert!(!plan.zero_residue);
        assert!(plan.requires_admin);
    }

    #[test]
    fn gui_resolver_is_gated_and_produces_complete_dry_run() {
        let registry = Registry::embedded().unwrap();
        let mut plan = safe_plan();
        let make = |plan: &GuardPlan| {
            resolve_plan_with(
                &registry,
                plan,
                Platform::Linux,
                "test".into(),
                "gui",
                |host| {
                    assert_eq!(host, "support.google.com");
                    Ok(resolved()[0].addresses.clone())
                },
            )
        };
        assert_eq!(make(&plan).unwrap().entries.len(), 2);
        plan.safe_to_apply = false;
        assert_eq!(
            resolve_plan_with(
                &registry,
                &plan,
                Platform::Linux,
                "test".into(),
                "gui",
                |_| panic!("DNS before review")
            )
            .unwrap_err()
            .code,
            "SYSBLOCK_NOT_SAFE"
        );
    }

    #[test]
    fn dns_preflight_rejects_incomplete_or_shared_scope() {
        let registry = Registry::embedded().unwrap();
        let mut plan = safe_plan();
        plan.affected_domains.clear();
        assert_eq!(
            candidate_hosts(&registry, &plan, "test").unwrap_err().code,
            "SYSBLOCK_HOST_OUTSIDE_CLOSURE"
        );
        let mut plan = safe_plan();
        plan.affected_shared_paths.push("shared dependency".into());
        assert_eq!(
            candidate_hosts(&registry, &plan, "test").unwrap_err().code,
            "SYSBLOCK_CLOSURE_INCOMPLETE"
        );
    }

    #[test]
    fn unsafe_plan_and_widening_are_refused() {
        let registry = Registry::embedded().unwrap();
        let mut unsafe_plan = safe_plan();
        unsafe_plan.safe_to_apply = false;
        assert_eq!(
            build_plan(
                &registry,
                &unsafe_plan,
                &resolved(),
                Platform::Linux,
                "t",
                "test"
            )
            .unwrap_err()
            .code,
            "SYSBLOCK_NOT_SAFE"
        );
        let mut conflicted = safe_plan();
        conflicted.reasons = vec!["NO_SAFE_BLOCK".to_string()];
        assert_eq!(
            build_plan(
                &registry,
                &conflicted,
                &resolved(),
                Platform::Linux,
                "t",
                "test"
            )
            .unwrap_err()
            .code,
            "SYSBLOCK_CLOSURE_INCOMPLETE"
        );
        let outside = vec![ResolvedHost {
            fqdn: "accounts.google.com".to_string(),
            addresses: vec!["203.0.113.11".parse().unwrap()],
        }];
        assert_eq!(
            build_plan(
                &registry,
                &safe_plan(),
                &outside,
                Platform::Linux,
                "t",
                "test"
            )
            .unwrap_err()
            .code,
            "SYSBLOCK_HOST_NOT_CANDIDATE"
        );
    }

    #[test]
    fn ip_literals_private_addresses_and_bad_tags_are_refused() {
        let registry = Registry::embedded().unwrap();
        let literal = vec![ResolvedHost {
            fqdn: "203.0.113.10".to_string(),
            addresses: vec!["203.0.113.10".parse().unwrap()],
        }];
        assert_eq!(
            build_plan(
                &registry,
                &safe_plan(),
                &literal,
                Platform::Linux,
                "t",
                "test"
            )
            .unwrap_err()
            .code,
            "SYSBLOCK_IP_LITERAL"
        );
        let private = vec![ResolvedHost {
            fqdn: "support.google.com".to_string(),
            addresses: vec!["192.168.1.1".parse().unwrap()],
        }];
        assert_eq!(
            build_plan(
                &registry,
                &safe_plan(),
                &private,
                Platform::Linux,
                "t",
                "test"
            )
            .unwrap_err()
            .code,
            "SYSBLOCK_NON_ROUTABLE"
        );
        let mut injected = safe_plan();
        injected.plan_id = "x".to_string();
        assert_eq!(
            build_plan(
                &registry,
                &injected,
                &resolved(),
                Platform::Linux,
                "t",
                "bad; rm -rf /"
            )
            .unwrap_err()
            .code,
            "SYSBLOCK_INVALID_TAG"
        );
        assert!(unblock_commands(Platform::Linux, "bad tag").is_err());
        assert_eq!(
            unblock_commands(Platform::Windows, "x1").unwrap(),
            vec!["parallaxe guard unblock --rule-tag x1 --apply"]
        );
    }
}
