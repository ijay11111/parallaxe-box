//! 阶段 7 演示：用**合成的安全计划**展示规则导出与系统阻断命令渲染。
//!
//! 注意：真实目录没有“专用路径”证据，`guard plan` 恒 `safe_to_apply=false`；
//! 本示例只验证导出器/命令模型本身，不代表真实目标已获准阻断。

use std::net::IpAddr;

use parallaxe_core::contracts::GuardPlan;
use parallaxe_core::guard_export::{export_rules, ExportMode, ExportRequest, RuleFormat};
use parallaxe_core::sysblock::{build_plan, Platform, ResolvedHost};
use parallaxe_core::targets::Registry;

fn plan() -> GuardPlan {
    GuardPlan {
        plan_id: "plan-demo".to_string(),
        run_id: "run-demo".to_string(),
        selected_by: "USER".to_string(),
        operation: "PREVIEW".to_string(),
        candidates: vec!["google.gemini".to_string()],
        affected_domains: vec!["gemini.google.com".to_string()],
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
        source_run_id: Some("run-source-demo".to_string()),
    }
}

fn main() {
    let registry = Registry::embedded().expect("embedded registry");
    let plan = plan();
    for (format, mode) in [
        (RuleFormat::Clash, ExportMode::Provider),
        (RuleFormat::Clash, ExportMode::Full),
        (RuleFormat::SingBox, ExportMode::Provider),
        (RuleFormat::DomainList, ExportMode::Provider),
    ] {
        let bundle = export_rules(ExportRequest {
            registry: &registry,
            plan: &plan,
            format,
            mode,
            generated_at: "2026-11-23T00:00:00Z".to_string(),
            forensic_pause: None,
        })
        .expect("safe plan exports");
        println!("== {} / {} ==", bundle.format, bundle.mode);
        println!("domains: {}", bundle.domains.join(", "));
        for warning in &bundle.warnings {
            println!("warning: {warning}");
        }
        for artifact in &bundle.artifacts {
            println!("-- {} --", artifact.name);
            println!("{}", artifact.content);
        }
        println!();
    }

    let resolved = vec![ResolvedHost {
        fqdn: "gemini.google.com".to_string(),
        addresses: vec![
            "203.0.113.10".parse::<IpAddr>().unwrap(),
            "2001:db8:1::10".parse::<IpAddr>().unwrap(),
        ],
    }];
    for platform in [Platform::Windows, Platform::Linux] {
        let sys = build_plan(
            &registry,
            &plan,
            &resolved,
            platform,
            "2026-11-23T00:00:00Z",
            "demo",
        )
        .expect("safe system plan");
        println!("== sysblock {} ({}) ==", sys.platform, sys.engine);
        for entry in &sys.entries {
            println!("{} -> {} ({})", entry.fqdn, entry.ip, entry.family);
        }
        for command in &sys.apply_commands {
            println!("apply: {command}");
        }
        for command in &sys.rollback_commands {
            println!("rollback: {command}");
        }
        println!();
    }
}
