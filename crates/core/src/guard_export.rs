//! `guard_export`：T-7-07 规则导入导出（ADR-006 D-1 / 四层策略主方案）。
//!
//! 只把**已通过 Guard 闸门**的候选 FQDN 渲染成静态规则文件，不监听端口、不写系统、
//! 不创建临时订阅服务，也不替用户替换任何配置。两种模式：
//!
//! * [`ExportMode::Provider`]（默认、推荐）：独立 rule-provider + 一次性 Merge/Script
//!   片段，`RULE-SET,...,REJECT` 加到用户规则最前，不改用户节点与原有分流；
//! * [`ExportMode::Full`]（第二备选）：生成完整规则，供用户自行整包替换；导出强警示，
//!   软件不替代执行、不主动推荐。
//!
//! 两种模式都受同一套闸门约束：保护名单 / 共享 CDN / 依赖闭包 / 未知一律不放行。
//! 导出清单只含 FQDN，绝不含 IP / ASN / Cookie / 账号字段。

use std::collections::BTreeSet;
use std::net::IpAddr;

use serde::Serialize;

use crate::contracts::{reason, GuardPlan};
use crate::targets::Registry;

/// 导出清单 schema。
pub const EXPORT_SCHEMA: &str = "parallaxe.guard-export/v1";
/// 整包替换的强警示（导出界面与文档必须原样展示）。
pub const FULL_MODE_WARNING: &str = "整包替换会覆盖你原有的分流规则，可能导致其他流量异常；请先备份你的配置。仅在用户主动选择时使用，本工具不替你替换配置。";

const REASON_SHARED_CDN: &str = "GUARD_DEFERRED_SHARED_CDN";
const REASON_DEPENDENCY_CONFLICT: &str = "BLOCK_DEFERRED_DEPENDENCY_CONFLICT";

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFormat {
    /// mihomo / Clash Meta（rule-provider + Merge/Script 片段）。
    Clash,
    /// sing-box（本地 rule_set 源文件 + route 片段）。
    SingBox,
    /// 纯 FQDN 域名清单（DNS / hosts / 其他工具自行导入）。
    DomainList,
}

impl RuleFormat {
    pub const ALL: [RuleFormat; 3] = [
        RuleFormat::Clash,
        RuleFormat::SingBox,
        RuleFormat::DomainList,
    ];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "clash" | "mihomo" => Some(RuleFormat::Clash),
            "singbox" | "sing-box" => Some(RuleFormat::SingBox),
            "domainlist" | "domain-list" | "domains" => Some(RuleFormat::DomainList),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RuleFormat::Clash => "clash",
            RuleFormat::SingBox => "singbox",
            RuleFormat::DomainList => "domainlist",
        }
    }
}

/// 导出模式：不污染式 / 整包替换。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    Provider,
    Full,
}

impl ExportMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "provider" | "merge" => Some(ExportMode::Provider),
            "full" | "replace" => Some(ExportMode::Full),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ExportMode::Provider => "provider",
            ExportMode::Full => "full",
        }
    }
}

/// 一个导出文件。
#[derive(Debug, Clone, Serialize)]
pub struct ExportArtifact {
    pub name: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// 导出清单（JSON 元数据；文件由调用方落盘或复制到剪贴板）。
#[derive(Debug, Clone, Serialize)]
pub struct ExportBundle {
    pub schema: String,
    pub plan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    pub format: String,
    pub mode: String,
    pub generated_at: String,
    pub domains: Vec<String>,
    pub artifacts: Vec<ExportArtifact>,
    pub warnings: Vec<String>,
    pub applied: bool,
    pub note: String,
}

/// 拒绝原因：稳定机器码 + 人读说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRefusal {
    pub code: &'static str,
    pub detail: String,
}

impl ExportRefusal {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        ExportRefusal {
            code,
            detail: detail.into(),
        }
    }
}

/// 导出请求。
pub struct ExportRequest<'a> {
    pub registry: &'a Registry,
    pub plan: &'a GuardPlan,
    pub format: RuleFormat,
    pub mode: ExportMode,
    pub generated_at: String,
    /// 路线变化 / 本地 QoS / 主要信号过期时为 `Some(说明)`：暂停自动规则生成。
    pub forensic_pause: Option<String>,
}

/// 校验并渲染导出。
pub fn export_rules(request: ExportRequest<'_>) -> Result<ExportBundle, ExportRefusal> {
    let plan = request.plan;
    if let Some(pause) = &request.forensic_pause {
        return Err(ExportRefusal::new(
            "EXPORT_PAUSED_FORENSIC",
            format!("路线变化 / 本地 QoS / 信号过期，暂停自动规则生成：{pause}"),
        ));
    }
    if !plan.closure_complete {
        return Err(ExportRefusal::new(
            "EXPORT_CLOSURE_INCOMPLETE",
            "影响闭包不完整：缺结果、缺边、依赖过期、预算超限或来源中断，未知一律不放行。",
        ));
    }
    if plan.candidates.is_empty() {
        return Err(ExportRefusal::new(
            "EXPORT_NO_CANDIDATES",
            "没有通过判定与可信度门槛的候选目标。",
        ));
    }
    for blocked in [
        REASON_SHARED_CDN,
        REASON_DEPENDENCY_CONFLICT,
        reason::PROTECTED_HOST,
        reason::BUDGET_EXHAUSTED,
    ] {
        if plan.reasons.iter().any(|item| item == blocked) {
            return Err(ExportRefusal::new(
                "EXPORT_CONFLICT",
                format!("存在未解除的闸门原因 {blocked}：保护域、共享 CDN、依赖冲突或预算异常时不得导出。"),
            ));
        }
    }

    // 只导出候选目标自身的精确 FQDN；闭包里的关联主机不进入阻断清单。
    let mut domains = BTreeSet::new();
    let mut omitted = Vec::new();
    for id in &plan.candidates {
        let Some(target) = request.registry.get(id) else {
            omitted.push(format!("{id}:UNKNOWN_TARGET"));
            continue;
        };
        let host = target.host().trim_end_matches('.').to_ascii_lowercase();
        if target.raw.protected {
            omitted.push(format!("{host}:PROTECTED_HOST"));
            continue;
        }
        if !valid_fqdn(&host) {
            omitted.push(format!("{host}:NOT_FQDN"));
            continue;
        }
        if !plan.affected_domains.iter().any(|item| item == &host) {
            omitted.push(format!("{host}:NOT_IN_CLOSURE"));
            continue;
        }
        domains.insert(host);
    }
    if domains.is_empty() {
        return Err(ExportRefusal::new(
            "EXPORT_NO_FQDN",
            format!(
                "候选里没有可导出的精确 FQDN（已省略：{}）。",
                omitted.join(", ")
            ),
        ));
    }
    let domains: Vec<String> = domains.into_iter().collect();

    let mut warnings = vec![
        "导出的是静态规则文件：软件不替你导入、不替用户替换配置，也不宣称已生效。".to_string(),
        "只用判定过的精确 FQDN；不含 IP / ASN / Cookie / 账号字段，禁止按 CDN IP 泛阻断。"
            .to_string(),
        "远端账号影响与共享出口保证仍无法由本工具证明。".to_string(),
    ];
    if plan
        .reasons
        .iter()
        .any(|item| item == reason::NO_SAFE_BLOCK)
    {
        warnings.push(
            "本计划仍标记 NO_SAFE_BLOCK（目录没有专用路径证据）：导出仅供你自行判断与手工应用，不构成自动屏蔽建议。"
                .to_string(),
        );
    }

    let mut artifacts = Vec::new();
    match (request.format, request.mode) {
        (RuleFormat::Clash, ExportMode::Provider) => {
            artifacts.push(ExportArtifact {
                name: "parallaxe-block.yaml".to_string(),
                content: clash_provider(&domains),
                warning: None,
            });
            artifacts.push(ExportArtifact {
                name: "parallaxe-block-merge.yaml".to_string(),
                content: clash_merge(&domains),
                warning: Some(
                    "provider 文件必须放在 mihomo/Clash 客户端目录内（SAFE_PATHS 限制）；prepend-rules 在部分 Clash Verge 版本会被静默忽略，请改用 Script 兜底并核对 runtime config 第一条是 RULE-SET,parallaxe-block,REJECT。"
                        .to_string(),
                ),
            });
            artifacts.push(ExportArtifact {
                name: "parallaxe-block-script.js".to_string(),
                content: clash_script(&domains),
                warning: None,
            });
        }
        (RuleFormat::Clash, ExportMode::Full) => {
            warnings.push(FULL_MODE_WARNING.to_string());
            artifacts.push(ExportArtifact {
                name: "parallaxe-full.yaml".to_string(),
                content: clash_full(&domains),
                warning: Some(FULL_MODE_WARNING.to_string()),
            });
        }
        (RuleFormat::SingBox, ExportMode::Provider) => {
            artifacts.push(ExportArtifact {
                name: "parallaxe-block.json".to_string(),
                content: singbox_rule_set(&domains),
                warning: None,
            });
            artifacts.push(ExportArtifact {
                name: "parallaxe-block-route.json".to_string(),
                content: singbox_route_snippet(),
                warning: Some(
                    "把 rule_set 的 path 指向同目录 parallaxe-block.json；只新增一条 reject 规则，不改你的节点与出站。"
                        .to_string(),
                ),
            });
        }
        (RuleFormat::SingBox, ExportMode::Full) => {
            warnings.push(FULL_MODE_WARNING.to_string());
            artifacts.push(ExportArtifact {
                name: "parallaxe-full.json".to_string(),
                content: singbox_full(&domains),
                warning: Some(FULL_MODE_WARNING.to_string()),
            });
        }
        (RuleFormat::DomainList, _) => {
            warnings.push(
                "纯域名清单不含任何代理配置；请自行导入你的 DNS / hosts / 过滤工具，并注意不要把它转换成 IP 段阻断。"
                    .to_string(),
            );
            artifacts.push(ExportArtifact {
                name: "parallaxe-block-domains.txt".to_string(),
                content: domain_list(&domains),
                warning: None,
            });
        }
    }
    if !omitted.is_empty() {
        warnings.push(format!("已省略不可导出项：{}", omitted.join(", ")));
    }

    Ok(ExportBundle {
        schema: EXPORT_SCHEMA.to_string(),
        plan_id: plan.plan_id.clone(),
        source_run_id: plan.source_run_id.clone(),
        format: request.format.as_str().to_string(),
        mode: request.mode.as_str().to_string(),
        generated_at: request.generated_at,
        domains,
        artifacts,
        warnings,
        applied: false,
        note: "导出未写入系统：没有回环服务、没有临时订阅端口，也没有改动你的代理配置。"
            .to_string(),
    })
}

/// FQDN 校验：拒绝 IP、协议前缀、通配符与非法标签。
pub fn valid_fqdn(host: &str) -> bool {
    if host.is_empty()
        || host.len() > 253
        || host.contains("://")
        || host.contains('/')
        || host.contains('*')
        || host.contains(' ')
        || host.parse::<IpAddr>().is_ok()
    {
        return false;
    }
    let host = host.trim_end_matches('.');
    if !host.contains('.') || host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    })
}

fn clash_provider(domains: &[String]) -> String {
    let mut out = String::from(
        "# Parallaxe box rule-provider（不污染式；behavior: domain，精确 FQDN）\n# 只放本文件到你的 mihomo/Clash 客户端目录，并在主配置中引用。\npayload:\n",
    );
    for domain in domains {
        out.push_str(&format!("  - {domain}\n"));
    }
    out
}

fn clash_merge(domains: &[String]) -> String {
    let mut out = String::from(
        "# Parallaxe box 一次性 Merge 片段：只新增独立 rule-provider，不改你的节点与原有分流。\n# 1) parallaxe-block.yaml 必须放在客户端目录内（mihomo SAFE_PATHS 限制）。\n# 2) prepend-rules 在部分 Clash Verge 版本会被静默忽略；若未生效，请改用 parallaxe-block-script.js。\n# 3) 应用后打开生成的 runtime config，确认第一条规则是 RULE-SET,parallaxe-block,REJECT。\nrule-providers:\n  parallaxe-block:\n    type: file\n    behavior: domain\n    format: yaml\n    path: ./parallaxe-block.yaml\nprepend-rules:\n  - RULE-SET,parallaxe-block,REJECT\n# 本片段导出的精确 FQDN（仅供核对）：\n",
    );
    for domain in domains {
        out.push_str(&format!("#   - {domain}\n"));
    }
    out
}

fn clash_script(domains: &[String]) -> String {
    let mut out = String::from(
        "// Parallaxe box Script 兜底：用于 prepend-rules 被静默忽略的 Clash Verge 版本。\n// 只新增独立 rule-provider，并把 REJECT 放到规则最前；不改你的节点与原有分流。\nfunction main(config) {\n  config[\"rule-providers\"] = config[\"rule-providers\"] || {};\n  config[\"rule-providers\"][\"parallaxe-block\"] = {\n    type: \"file\",\n    behavior: \"domain\",\n    format: \"yaml\",\n    path: \"./parallaxe-block.yaml\",\n  };\n  config.rules = config.rules || [];\n  const reject = \"RULE-SET,parallaxe-block,REJECT\";\n  config.rules = [reject].concat(config.rules.filter((rule) => rule !== reject));\n  return config;\n}\n// 精确 FQDN：\n",
    );
    for domain in domains {
        out.push_str(&format!("//   {domain}\n"));
    }
    out
}

fn clash_full(domains: &[String]) -> String {
    let mut out = String::from(
        "# ⚠️ 整包替换：会覆盖你原有的分流规则。请先备份。此文件由用户主动选择生成，软件不替你替换。\n# 只含判定过的精确 FQDN；末尾 MATCH,DIRECT 不接管你未列出的流量。\nmixed-port: 7890\nmode: rule\nrules:\n",
    );
    for domain in domains {
        out.push_str(&format!("  - DOMAIN,{domain},REJECT\n"));
    }
    out.push_str("  - MATCH,DIRECT\n");
    out
}

fn singbox_rule_set(domains: &[String]) -> String {
    let body = serde_json::json!({
        "version": 1,
        "rules": [{
            "domain": domains,
        }],
    });
    let mut text = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}

fn singbox_route_snippet() -> String {
    let body = serde_json::json!({
        "route": {
            "rule_set": [{
                "type": "local",
                "tag": "parallaxe-block",
                "format": "source",
                "path": "./parallaxe-block.json",
            }],
            "rules": [{
                "rule_set": "parallaxe-block",
                "action": "reject",
            }],
        },
    });
    let mut text = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}

fn singbox_full(domains: &[String]) -> String {
    let body = serde_json::json!({
        "log": {"level": "warn"},
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": 7890,
        }],
        "outbounds": [{
            "type": "direct",
            "tag": "direct",
        }],
        "route": {
            "rules": [{
                "domain": domains,
                "action": "reject",
            }],
            "final": "direct",
        },
    });
    let mut text = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}

fn domain_list(domains: &[String]) -> String {
    let mut out = String::new();
    for domain in domains {
        out.push_str(domain);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::GuardPlan;

    fn plan(candidates: Vec<&str>, domains: Vec<&str>, reasons: Vec<&str>) -> GuardPlan {
        GuardPlan {
            plan_id: "plan-test".to_string(),
            run_id: "run-test".to_string(),
            selected_by: "USER".to_string(),
            operation: "PREVIEW".to_string(),
            candidates: candidates.into_iter().map(str::to_string).collect(),
            affected_domains: domains.into_iter().map(str::to_string).collect(),
            affected_services: Vec::new(),
            affected_shared_paths: Vec::new(),
            impact_closure: Vec::new(),
            reasons: reasons.into_iter().map(str::to_string).collect(),
            primary_reason: None,
            safe_to_apply: false,
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

    fn request<'a>(
        registry: &'a Registry,
        plan: &'a GuardPlan,
        format: RuleFormat,
        mode: ExportMode,
    ) -> ExportRequest<'a> {
        ExportRequest {
            registry,
            plan,
            format,
            mode,
            generated_at: "2026-11-23T00:00:00Z".to_string(),
            forensic_pause: None,
        }
    }

    #[test]
    fn provider_export_only_contains_exact_fqdn() {
        let registry = Registry::embedded().unwrap();
        let plan = plan(vec!["google.gemini"], vec!["gemini.google.com"], vec![]);
        let bundle = export_rules(request(
            &registry,
            &plan,
            RuleFormat::Clash,
            ExportMode::Provider,
        ))
        .expect("export");
        assert_eq!(bundle.domains, vec!["gemini.google.com"]);
        assert_eq!(bundle.artifacts.len(), 3);
        let provider = &bundle.artifacts[0].content;
        assert!(provider.contains("payload:"));
        assert!(provider.contains("  - gemini.google.com"));
        assert!(!provider.contains("1.1.1.1"));
        let merge = &bundle.artifacts[1].content;
        assert!(merge.contains("RULE-SET,parallaxe-block,REJECT"));
        assert!(merge.contains("SAFE_PATHS"));
        let script = &bundle.artifacts[2].content;
        assert!(script.contains("config.rules.unshift") || script.contains("[reject].concat"));
        assert!(!bundle.applied);
        assert!(bundle
            .warnings
            .iter()
            .any(|warning| warning.contains("静态规则文件")));
    }

    #[test]
    fn full_mode_is_explicitly_warned() {
        let registry = Registry::embedded().unwrap();
        let plan = plan(vec!["google.gemini"], vec!["gemini.google.com"], vec![]);
        let bundle = export_rules(request(
            &registry,
            &plan,
            RuleFormat::Clash,
            ExportMode::Full,
        ))
        .expect("export");
        assert_eq!(bundle.artifacts.len(), 1);
        assert!(bundle
            .warnings
            .iter()
            .any(|warning| warning.contains("覆盖你原有的分流规则")));
        assert!(bundle.artifacts[0].content.contains("⚠️ 整包替换"));
        assert!(bundle.artifacts[0].content.contains("MATCH,DIRECT"));
    }

    #[test]
    fn singbox_and_domainlist_exports_are_plain_fqdn_lists() {
        let registry = Registry::embedded().unwrap();
        let plan = plan(vec!["google.gemini"], vec!["gemini.google.com"], vec![]);
        let singbox = export_rules(request(
            &registry,
            &plan,
            RuleFormat::SingBox,
            ExportMode::Provider,
        ))
        .expect("export");
        assert_eq!(singbox.artifacts.len(), 2);
        let parsed: serde_json::Value =
            serde_json::from_str(&singbox.artifacts[0].content).expect("valid json");
        assert_eq!(parsed["rules"][0]["domain"][0], "gemini.google.com");
        let snippet: serde_json::Value =
            serde_json::from_str(&singbox.artifacts[1].content).expect("valid json");
        assert_eq!(snippet["route"]["rules"][0]["action"], "reject");
        let list = export_rules(request(
            &registry,
            &plan,
            RuleFormat::DomainList,
            ExportMode::Provider,
        ))
        .expect("export");
        assert_eq!(list.artifacts[0].content, "gemini.google.com\n");
    }

    #[test]
    fn gates_refuse_conflicts_unknown_and_forensic_pause() {
        let registry = Registry::embedded().unwrap();
        let conflict = plan(
            vec!["google.gemini"],
            vec!["gemini.google.com"],
            vec!["GUARD_DEFERRED_SHARED_CDN"],
        );
        assert_eq!(
            export_rules(request(
                &registry,
                &conflict,
                RuleFormat::Clash,
                ExportMode::Provider
            ))
            .unwrap_err()
            .code,
            "EXPORT_CONFLICT"
        );
        let unknown = plan(vec!["not.in.catalog"], vec!["nope.example"], vec![]);
        assert_eq!(
            export_rules(request(
                &registry,
                &unknown,
                RuleFormat::DomainList,
                ExportMode::Provider
            ))
            .unwrap_err()
            .code,
            "EXPORT_NO_FQDN"
        );
        let normal = plan(vec!["google.gemini"], vec!["gemini.google.com"], vec![]);
        let mut req = request(&registry, &normal, RuleFormat::Clash, ExportMode::Provider);
        req.forensic_pause = Some("ROUTE_INCONSISTENT".to_string());
        assert_eq!(
            export_rules(req).unwrap_err().code,
            "EXPORT_PAUSED_FORENSIC"
        );

        let mut interrupted = normal.clone();
        interrupted.closure_complete = false;
        assert_eq!(
            export_rules(request(
                &registry,
                &interrupted,
                RuleFormat::Clash,
                ExportMode::Provider
            ))
            .unwrap_err()
            .code,
            "EXPORT_CLOSURE_INCOMPLETE"
        );
    }

    #[test]
    fn fqdn_validation_rejects_ip_and_non_fqdn() {
        assert!(valid_fqdn("gemini.google.com"));
        assert!(valid_fqdn("a-b.example.co.uk"));
        assert!(!valid_fqdn("1.2.3.4"));
        assert!(!valid_fqdn("2001:db8::1"));
        assert!(!valid_fqdn("https://gemini.google.com"));
        assert!(!valid_fqdn("*.google.com"));
        assert!(!valid_fqdn("localhost"));
        assert!(!valid_fqdn("-bad.example.com"));
        assert!(!valid_fqdn("bad-.example.com"));
        assert!(!valid_fqdn("example"));
    }
}
