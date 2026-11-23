//! 处置中心：本机屏蔽影响预演、主动屏蔽任意服务（含当前正常服务，如 Google One）、
//! 链路体检四列只读视图与安全审计回滚说明。
//!
//! 设计原则：
//! - 全界面采用清晰大白话中文说明"每个处置方法是干什么的、解决什么问题"，
//!   彻底隐藏默认界面上的英文等式代码（技术字段收拢至默认折叠的「查看技术字段」区域）；
//! - 明确支持用户主动选择任意服务（包括当前访问正常的服务，例如 Google Books 受限、Google One 正常时主动屏蔽 Google One）
//!   进行静态依赖误伤预演并生成只读处置清单；
//! - 严守阶段 4/5 红线：全部数据原样消费自 `parallaxe_core::guard` / `linkcheck` / `targets`，
//!   绝不自行判定、绝不自动写入系统规则（`applied=false`、`rules_written=0`）。

use std::collections::BTreeSet;

use egui::{vec2, Align, Layout, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::contracts::State;
use parallaxe_core::guard::{self, GuardRequest};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::linkcheck::{self, LinkReport};
use parallaxe_core::targets::Registry;

use crate::state::AppState;
use crate::theme::{
    btn_outlined, btn_tonal, card_frame, kv_row, render_admonition, render_pill, spaced_columns,
    state_badge, subtle_well_frame, Palette, ThemeMode, FONT_BODY, FONT_META, FONT_TITLE,
    RADIUS_MD, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
};
use crate::views::service_matrix::service_name;
use crate::views::UiAction;

/// 处置中心交互状态（支持用户主动勾选任意服务进行屏蔽预演或生成处置清单）。
#[derive(Debug, Default)]
pub struct GuardViewState {
    /// 用户主动选中的拟屏蔽/拟预演目标（可包含当前正常服务，如 `google.one`）。
    pub selected_services: BTreeSet<String>,
    /// 全目录服务搜索关键字。
    pub service_search: String,
}

/// 渲染处置中心主视图。
#[allow(clippy::too_many_arguments)]
pub fn render(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    view: &mut GuardViewState,
    resolved_link_report: Option<&LinkReport>,
    show_ip: bool,
    theme: ThemeMode,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);

    // 1. 顶部定位说明 + 主动选择拟屏蔽服务（含正常服务，如 Google One）
    render_purpose_and_selector_card(ui, registry, state, view, theme, p, actions);
    ui.add_space(SPACE_4);

    // 2. 链路体检四列只读视图（全中文人话 + 可折叠技术字段）
    render_stage4_linkcheck_card(
        ui,
        registry,
        state,
        view,
        resolved_link_report,
        show_ip,
        p,
        actions,
    );
    ui.add_space(SPACE_4);

    // 3 & 4：底部双栏 16px 间距（「隔离预演与误伤分析」与「安全审计与只读回滚说明」）
    if ui.available_width() >= 860.0 {
        spaced_columns(ui, 2, SPACE_4, |col_ui, idx| {
            if idx == 0 {
                render_active_plans_section(col_ui, state, view, p, actions);
            } else {
                render_stage4_guard_audit_and_rollback_card(col_ui, registry, state, view, p);
            }
        });
    } else {
        render_active_plans_section(ui, state, view, p, actions);
        ui.add_space(SPACE_4);
        render_stage4_guard_audit_and_rollback_card(ui, registry, state, view, p);
    }
}

fn render_purpose_and_selector_card(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    view: &mut GuardViewState,
    theme: ThemeMode,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let locale = state.current_locale();
    let card_outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let card_inner_w = (card_outer_w - SPACE_4 * 2.0).max(200.0);
        ui.set_width(card_inner_w);
        ui.set_max_width(card_inner_w);

        let (source_run_id, effective_targets) = build_preview_targets(state, view);

        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(translate(locale, "gui.guard.title", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(translate(locale, "gui.guard.subtitle", &[]))
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
            });
        });

        ui.add_space(SPACE_2);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            render_pill(
                ui,
                &translate(locale, "gui.guard.pill.local_only", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            )
            .on_hover_text("LOCAL_TRAFFIC_ONLY");
            render_pill(
                ui,
                &translate(locale, "gui.guard.pill.no_remote", &[]),
                p.text_secondary,
                p.card_alt,
                p.border,
            )
            .on_hover_text("NOT_PROVABLE");
            render_pill(
                ui,
                &translate(locale, "gui.guard.pill.readonly", &[]),
                p.ok_fg,
                p.ok_bg,
                p.ok_fg,
            )
            .on_hover_text("applied=false · rules_written=0");

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                if btn_outlined(
                    ui,
                    p,
                    &translate(locale, "gui.guard.btn.load_demo", &[]),
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::RunScenario(
                        crate::commands::OfflineScenario::BooksAndSharedCdnGuard,
                    ));
                }
                let eff_cnt = effective_targets.len().to_string();
                let preview_label = translate(locale, "gui.guard.btn.preview", &[&eff_cnt]);
                if btn_tonal(ui, p, &preview_label, !effective_targets.is_empty()).clicked() {
                    actions.push(UiAction::RequestGuardPreview {
                        source_run_id: source_run_id.clone(),
                        targets: effective_targets.clone(),
                    });
                }
            });
        });

        ui.add_space(SPACE_3);
        // 主动选择拟屏蔽的服务（支持任意正常服务，例如 Google One！）
        subtle_well_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                ui.label(
                    RichText::new(translate(locale, "gui.guard.selector.label", &[]))
                        .strong()
                        .size(FONT_META)
                        .color(p.text_primary),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut view.service_search)
                        .desired_width(248.0)
                        .hint_text(translate(locale, "gui.guard.selector.search_hint", &[])),
                );
                if !view.selected_services.is_empty() {
                    if let Some(first_id) = view.selected_services.iter().next().cloned() {
                        let first_name = service_name(&first_id);
                        let btn_text = if view.selected_services.len() == 1 {
                            translate(locale, "gui.guard.selector.proactive_one", &[first_name])
                        } else {
                            let n_str = view.selected_services.len().to_string();
                            translate(
                                locale,
                                "gui.guard.selector.proactive_multi",
                                &[first_name, &n_str],
                            )
                        };
                        if btn_tonal(ui, p, &btn_text, true).clicked() {
                            actions.push(UiAction::OpenPlan(first_id));
                        }
                    }
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.guard.selector.clear", &[]),
                        true,
                    )
                    .clicked()
                    {
                        view.selected_services.clear();
                    }
                }
            });

            ui.add_space(SPACE_2);
            // 构造候选芯片列表：常用核心服务（显式包含 Google One / Google Books / Gemini / Play / Gmail / Drive）+ 当前运行结果 + 搜索匹配项
            let mut candidate_ids: Vec<String> = Vec::new();
            let search = view.service_search.trim().to_ascii_lowercase();
            if search.is_empty() {
                if let Some(run) = state.active_run() {
                    for r in run.ordered_results().into_iter().take(6) {
                        if !candidate_ids.contains(&r.target) {
                            candidate_ids.push(r.target.clone());
                        }
                    }
                }
                for preset in [
                    "google.one",
                    "google.books",
                    "google.gemini",
                    "google.play",
                    "google.gmail",
                    "google.drive",
                ] {
                    let s = preset.to_string();
                    if registry.get(preset).is_some() && !candidate_ids.contains(&s) {
                        candidate_ids.push(s);
                    }
                }
                for sel in &view.selected_services {
                    if !candidate_ids.contains(sel) {
                        candidate_ids.push(sel.clone());
                    }
                }
            } else {
                for entry in registry.entries() {
                    let id = entry.id();
                    let name_low = service_name(id).to_ascii_lowercase();
                    let host_low = entry.host().to_ascii_lowercase();
                    if id.to_ascii_lowercase().contains(&search)
                        || name_low.contains(&search)
                        || host_low.contains(&search)
                    {
                        candidate_ids.push(id.to_string());
                        if candidate_ids.len() >= 10 {
                            break;
                        }
                    }
                }
            }

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                for id in candidate_ids {
                    let checked = view.selected_services.contains(&id);
                    let run_state = state
                        .active_run()
                        .and_then(|r| r.result_for(&id))
                        .map(|r| r.state);
                    let state_suffix = match run_state {
                        Some(State::Ok) => translate(locale, "gui.guard.chip.ok_blockable", &[]),
                        Some(s) => state_badge(s, theme).label(locale),
                        None => {
                            if registry.get(&id).is_some_and(|t| t.raw.protected) {
                                translate(locale, "gui.guard.chip.protected_manual", &[])
                            } else {
                                translate(locale, "gui.guard.chip.blockable", &[])
                            }
                        }
                    };
                    let prefix = if checked {
                        translate(locale, "gui.guard.chip.selected", &[])
                    } else {
                        "+".to_string()
                    };
                    let chip_text = if locale == Locale::EnUs {
                        format!("{prefix} {} ({state_suffix})", service_name(&id))
                    } else {
                        format!("{prefix} {}（{state_suffix}）", service_name(&id))
                    };
                    let resp = ui.add(
                        egui::Button::new(RichText::new(chip_text).size(FONT_META).color(
                            if checked {
                                p.accent
                            } else {
                                p.text_primary
                            },
                        ))
                        .fill(if checked { p.accent_soft } else { p.card })
                        .stroke(Stroke::new(
                            1.0_f32,
                            if checked { p.accent } else { p.border },
                        ))
                        .rounding(Rounding::same(14.0))
                        .min_size(vec2(0.0, 26.0)),
                    );
                    if resp.clicked() {
                        if checked {
                            view.selected_services.remove(&id);
                        } else {
                            view.selected_services.insert(id);
                        }
                    }
                }
            });

            // 若用户主动勾选了服务（例如 Google One），即时用大白话展示其主机与静态依赖误伤分析
            if !view.selected_services.is_empty() {
                ui.add_space(SPACE_2);
                ui.separator();
                ui.add_space(SPACE_1);
                for id in &view.selected_services {
                    if let Some(target) = registry.get(id) {
                        let host = target.host();
                        let same_host_others: Vec<&str> = registry
                            .entries()
                            .iter()
                            .filter(|e| e.id() != id && e.host() == host)
                            .map(|e| service_name(e.id()))
                            .collect();
                        let dep_edges: Vec<String> = registry
                            .dependencies
                            .edges
                            .iter()
                            .filter(|edge| {
                                edge.src == format!("svc:{id}")
                                    || edge.dst == format!("svc:{id}")
                                    || edge.src == format!("host:{host}")
                                    || edge.dst == format!("host:{host}")
                            })
                            .map(|e| format!("{} → {}", e.src, e.dst))
                            .collect();
                        let impact_note = if !same_host_others.is_empty() {
                            format!(
                                "注意：与 {} 共用域名 {host}，屏蔽会连带阻断这些服务",
                                same_host_others.join("、")
                            )
                        } else if !dep_edges.is_empty() {
                            format!("静态目录登记了 {} 条关联依赖边，屏蔽前建议核对下方影响闭包", dep_edges.len())
                        } else {
                            format!("静态目录中 {host} 未发现与其他服务共用入口域名（仍需留意共享 CDN 资源）")
                        };
                        let protect_hint = if target.raw.protected {
                            "核心受保护服务（系统不自动屏蔽；如需主动屏蔽请点击上方「生成处置清单」手动配置）"
                        } else {
                            "非核心受保护服务（可预演隔离并生成处置清单）"
                        };
                        ui.label(
                            RichText::new(format!(
                                "• {}（{}）：{}；{}。",
                                service_name(id),
                                host,
                                protect_hint,
                                impact_note
                            ))
                            .size(FONT_META)
                            .color(p.text_secondary),
                        );
                    }
                }
            }
        });
    });
}

fn render_active_plans_section(
    ui: &mut Ui,
    state: &AppState,
    view: &GuardViewState,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    if let Some(run) = state.active_run() {
        if run.plans.is_empty() {
            render_empty_plan_card(ui, state, view, p, actions);
        } else {
            for (idx, plan) in run.plans.iter().enumerate() {
                if idx > 0 {
                    ui.add_space(SPACE_4);
                }
                render_guard_plan_card(ui, plan, state.current_locale(), p);
            }
        }
    } else {
        render_empty_plan_card(ui, state, view, p, actions);
    }
}

fn render_empty_plan_card(
    ui: &mut Ui,
    state: &AppState,
    view: &GuardViewState,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let locale = state.current_locale();
    let card_outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let card_inner_w = (card_outer_w - SPACE_4 * 2.0).max(200.0);
        ui.set_width(card_inner_w);
        ui.set_max_width(card_inner_w);
        ui.label(
            RichText::new(translate(locale, "gui.guard.plan.title", &[]))
                .strong()
                .size(FONT_TITLE)
                .color(p.text_primary),
        );
        ui.add_space(SPACE_2);
        ui.label(
            RichText::new(translate(locale, "gui.guard.plan.empty_hint", &[]))
                .size(FONT_BODY)
                .color(p.text_secondary),
        );
        ui.add_space(SPACE_3);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_2;
            let (source_run_id, targets) = build_preview_targets(state, view);
            let cnt = targets.len().to_string();
            let label = translate(locale, "gui.guard.btn.preview", &[&cnt]);
            if btn_tonal(ui, p, &label, !targets.is_empty()).clicked() {
                actions.push(UiAction::RequestGuardPreview {
                    source_run_id,
                    targets,
                });
            }
            if btn_outlined(
                ui,
                p,
                &translate(locale, "gui.guard.btn.load_demo", &[]),
                true,
            )
            .clicked()
            {
                actions.push(UiAction::RunScenario(
                    crate::commands::OfflineScenario::BooksAndSharedCdnGuard,
                ));
            }
        });
    });
}

fn humanize_veto_reason(code: &str) -> &'static str {
    match code {
        "GUARD_DEFERRED_SHARED_CDN" => "涉及共享 CDN（如静态资源域名），屏蔽会误伤其他正常服务",
        "BLOCK_DEFERRED_DEPENDENCY_CONFLICT" => "存在依赖冲突，直接屏蔽会连带影响受保护的关键服务",
        "NO_SAFE_BLOCK" => "当前没有可安全自动屏蔽的独占域名（已拦截以防误伤）",
        "PROTECTED_HOST" => "属于核心受保护服务，系统拒绝自动屏蔽（如需主动屏蔽请通过处置清单手动配置）",
        "NOT_GUARD_ELIGIBLE" => "该服务目前不是高可信受限/阻断状态，系统不自动建议屏蔽（如需主动屏蔽正常服务请使用处置清单）",
        "INCONCLUSIVE" => "本次尚未获取该服务的检测结果，系统暂不自动建议屏蔽（仍可生成手动处置清单）",
        _ => "按安全保守策略暂缓自动屏蔽，请参考影响清单手动评估",
    }
}

fn render_guard_plan_card(
    ui: &mut Ui,
    plan: &parallaxe_core::contracts::GuardPlan,
    locale: Locale,
    p: Palette,
) {
    let card_outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let card_inner_w = (card_outer_w - SPACE_4 * 2.0).max(200.0);
        ui.set_width(card_inner_w);
        ui.set_max_width(card_inner_w);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            ui.label(
                RichText::new(translate(locale, "gui.guard.plan.title", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
            render_pill(
                ui,
                &translate(locale, "gui.guard.plan.readonly_pill", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            );
            if plan.safe_to_apply {
                render_pill(
                    ui,
                    &translate(locale, "gui.guard.plan.safe_pill", &[]),
                    p.ok_fg,
                    p.ok_bg,
                    p.ok_fg,
                );
            } else {
                render_pill(
                    ui,
                    &translate(locale, "gui.guard.plan.blocked_pill", &[]),
                    p.warning_fg,
                    p.warning_bg,
                    p.warning_fg,
                );
            }
        });

        ui.add_space(SPACE_3);
        subtle_well_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let candidates = if plan.candidates.is_empty() {
                "无符合自动屏蔽条件的服务".to_string()
            } else {
                plan.candidates
                    .iter()
                    .map(|c| service_name(c))
                    .collect::<Vec<_>>()
                    .join("、")
            };
            kv_row(ui, p, "评估目标", 76.0, &candidates, false);
            ui.add_space(SPACE_1);
            kv_row(
                ui,
                p,
                "依赖完整度",
                76.0,
                if plan.closure_complete {
                    "依赖关系完整（已闭合检查）"
                } else {
                    "依赖信息不完整（按安全保守原则拦截）"
                },
                false,
            );
            ui.add_space(SPACE_1);
            kv_row(
                ui,
                p,
                "本机改动",
                76.0,
                if plan.applied {
                    "已应用"
                } else {
                    "未修改任何本机或代理规则"
                },
                false,
            );
        });

        if let Some(primary) = &plan.primary_reason {
            ui.add_space(SPACE_3);
            render_admonition(ui, p.danger_bg, p.danger_fg, |ui| {
                ui.label(
                    RichText::new(format!("拦截原因：{}", humanize_veto_reason(primary)))
                        .strong()
                        .size(FONT_BODY)
                        .color(p.danger_fg),
                );
                if !plan.affected_shared_paths.is_empty() {
                    ui.add_space(SPACE_1);
                    ui.label(
                        RichText::new(format!(
                            "可能被误伤的共享路径：{}",
                            plan.affected_shared_paths.join("；")
                        ))
                        .size(FONT_META)
                        .color(p.text_primary),
                    );
                }
            });
        }

        if !plan.impact_closure.is_empty() {
            ui.add_space(SPACE_3);
            ui.label(
                RichText::new(format!(
                    "若屏蔽该服务，可能连带影响以下 {} 个域名/服务：",
                    plan.impact_closure.len()
                ))
                .strong()
                .size(FONT_META)
                .color(p.text_secondary),
            );
            ui.add_space(SPACE_2);
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (idx, node) in plan.impact_closure.iter().enumerate() {
                    if idx > 0 {
                        ui.add_space(SPACE_1 + 2.0);
                    }
                    ui.horizontal_wrapped(|ui| {
                        let role_raw = format!("{:?}", node.role);
                        let (zh_role, fg, bg) = if role_raw.contains("Affected") {
                            ("直接受影响", p.warning_fg, p.warning_bg)
                        } else {
                            ("关联服务", p.info_fg, p.info_bg)
                        };
                        render_pill(ui, zh_role, fg, bg, fg);
                        ui.label(
                            RichText::new(&node.node)
                                .monospace()
                                .size(FONT_META)
                                .color(p.text_primary),
                        );
                        if let Some(via) = node.via.as_deref() {
                            ui.label(
                                RichText::new(format!("经由 {via}"))
                                    .size(FONT_META)
                                    .color(p.text_muted),
                            );
                        }
                    });
                }
            });
        }

        ui.add_space(SPACE_2);
        ui.collapsing("查看预演技术字段", |ui| {
            ui.monospace(format!(
                "plan_id={}\nop={} · safe_to_apply={} · applied={} · closure_complete={}\nprimary_reason={}",
                plan.plan_id,
                plan.operation,
                plan.safe_to_apply,
                plan.applied,
                plan.closure_complete,
                plan.primary_reason.as_deref().unwrap_or("none")
            ));
        });
    });
}

fn render_stage4_guard_audit_and_rollback_card(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    view: &GuardViewState,
    p: Palette,
) {
    let locale = state.current_locale();
    if view.selected_services.is_empty() {
        if let Some(audit) = state.active_run().and_then(|run| run.guard_audit.as_ref()) {
            render_audit(
                ui,
                p,
                locale,
                audit,
                &translate(locale, "gui.guard.audit.title", &[]),
            );
            return;
        }
    }
    let (source_run_id, targets) = build_preview_targets(state, view);
    let results: Vec<parallaxe_core::contracts::ResultRecord> = state
        .active_run()
        .map(|r| r.ordered_results().into_iter().cloned().collect())
        .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let source_interrupted = state.active_run().and_then(|r| r.termination())
        != Some(parallaxe_core::Termination::Complete);
    let preview = guard::plan_preview(
        registry,
        GuardRequest {
            run_id: &format!("draft-{source_run_id}"),
            source_run_id: Some(&source_run_id),
            targets: &targets,
            results: &results,
            recorded_at: parallaxe_core::state::format_time(now),
            as_of: now.date(),
            source_interrupted,
        },
    );
    render_audit(
        ui,
        p,
        locale,
        &preview.audit,
        &translate(locale, "gui.guard.audit.live_title", &[]),
    );
}

fn render_audit(ui: &mut Ui, p: Palette, locale: Locale, audit: &guard::GuardAudit, title: &str) {
    let outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let inner_w = (outer_w - SPACE_4 * 2.0).max(160.0);
        ui.set_width(inner_w);
        ui.set_max_width(inner_w);
        ui.label(
            RichText::new(title)
                .strong()
                .size(FONT_TITLE)
                .color(p.text_primary),
        );
        ui.add_space(SPACE_2);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            let rw_str = audit.after.rules_written.to_string();
            render_pill(
                ui,
                &translate(locale, "gui.guard.audit.rules_written", &[&rw_str]),
                p.ok_fg,
                p.ok_bg,
                p.ok_fg,
            );
            render_pill(
                ui,
                &translate(locale, "gui.guard.audit.no_rollback", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            );
        });

        ui.add_space(SPACE_3);
        subtle_well_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let result_zh = if audit.safe_to_apply {
                "预演通过（仍仅只读，不自动写入）"
            } else {
                "已拦截自动应用（仅作只读预演，防误伤）"
            };
            kv_row(ui, p, "审计结论", 76.0, result_zh, false);
            ui.add_space(SPACE_1);
            let cand = if audit.selection.candidates.is_empty() {
                "无".to_string()
            } else {
                audit
                    .selection
                    .candidates
                    .iter()
                    .map(|c| service_name(c))
                    .collect::<Vec<_>>()
                    .join("、")
            };
            kv_row(
                ui,
                p,
                "服务筛选",
                76.0,
                &format!(
                    "共评估 {} 项 · 符合自动条件 {} 项（{}）· 拦截 {} 项",
                    audit.selection.requested.len(),
                    audit.selection.candidates.len(),
                    cand,
                    audit.selection.rejected.len(),
                ),
                false,
            );
            if !audit.selection.rejected.is_empty() {
                ui.add_space(SPACE_1);
                let rej_summary: Vec<String> = audit
                    .selection
                    .rejected
                    .iter()
                    .take(3)
                    .map(|r| {
                        format!(
                            "{}：{}",
                            service_name(&r.target),
                            humanize_veto_reason(&r.reason)
                        )
                    })
                    .collect();
                kv_row(ui, p, "拦截说明", 76.0, &rej_summary.join("；"), false);
            }
        });

        ui.add_space(SPACE_3);
        render_admonition(ui, p.info_bg, p.info_fg, |ui| {
            ui.label(
                RichText::new(translate(locale, "gui.guard.audit.rollback_banner", &[]))
                    .strong()
                    .size(FONT_META)
                    .color(p.info_fg),
            );
            ui.add_space(SPACE_1);
            ui.label(
                RichText::new(&audit.rollback.note)
                    .size(FONT_META)
                    .color(p.text_primary),
            );
        });

        ui.add_space(SPACE_2);
        ui.collapsing(translate(locale, "gui.guard.audit.tech_fields", &[]), |ui| {
            ui.monospace(format!(
                "schema={}\nplan_id={}\nresult={} · safe_to_apply={} · applied={}\nrules_written={}/{} · rollback={} (required={}, performed={})",
                audit.schema,
                audit.plan_id,
                audit.result,
                audit.safe_to_apply,
                audit.applied,
                audit.before.rules_written,
                audit.after.rules_written,
                audit.rollback.status,
                audit.rollback.required,
                audit.rollback.performed
            ));
        });
    });
}

fn link_cell(ui: &mut Ui, w: f32, draw: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(w);
        draw(ui);
    });
}

fn humanize_link_status(status: &str, locale: Locale) -> (String, bool, bool) {
    let (key, is_ok, is_err) = match status {
        "OBSERVED" | "RESOLVED" => ("gui.link.status.observed", true, false),
        "OBSERVED_OK" => ("gui.link.status.ok", true, false),
        "RESOLVE_FAILED" => ("gui.link.status.resolve_failed", false, true),
        "FAILED" | "ERROR" => ("gui.link.status.failed", false, true),
        "NOT_OBSERVED" => ("gui.link.status.not_observed", false, false),
        "UNTESTED" | "SKIPPED" => ("gui.link.status.untested", false, false),
        "UNAVAILABLE" => ("gui.link.status.unavailable", false, true),
        _ => ("gui.link.status.pending", false, false),
    };
    (translate(locale, key, &[]), is_ok, is_err)
}

fn human_status_pill(
    ui: &mut Ui,
    prefix: Option<&str>,
    raw_status: &str,
    locale: Locale,
    p: Palette,
) {
    let (localized, is_ok, is_err) = humanize_link_status(raw_status, locale);
    let label = match prefix {
        Some(pre) => format!("{pre} {localized}"),
        None => localized,
    };
    let (fg, bg) = if is_ok {
        (p.info_fg, p.info_bg)
    } else if is_err {
        (p.danger_fg, p.danger_bg)
    } else {
        (p.text_secondary, p.card_alt)
    };
    render_pill(ui, &label, fg, bg, fg).on_hover_text(raw_status);
}

fn humanize_link_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("RESOLVER_ANSWER_NOT_CAPTURED") => "未捕获 DNS 解析记录",
        Some("TRANSPORT_RESOLVE") => "本机域名解析未返回结果",
        Some("NO_REGION_SIGNAL") => "页面未携带明确地区信号",
        Some("REGION_RESTRICTED_FIXTURE") => "演示：服务返回地区限制",
        Some("POLICY_BLOCKED_FIXTURE") => "演示：策略拒绝访问",
        Some(_) => "详见折叠技术字段",
        None => "无附加异常说明",
    }
}

fn humanize_layer_code(layer: Option<&str>) -> &'static str {
    match layer {
        Some("SERVICE_POLICY") => "服务策略层",
        Some("EGRESS_NETWORK") => "出口网络层",
        Some("PREFERENCE") => "公开版次层",
        Some("ACCOUNT") => "账号层（未采集）",
        Some(_) => "其他归因层",
        None => "未归因",
    }
}

#[allow(clippy::too_many_arguments)]
fn render_stage4_linkcheck_card(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    view: &GuardViewState,
    resolved_link_report: Option<&LinkReport>,
    show_ip: bool,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let computed_report: LinkReport;
    let link_report: &LinkReport = if let Some(resolved) = resolved_link_report
        .filter(|r| r.source_run_id.as_deref() == state.active_run_id.as_deref())
    {
        resolved
    } else if let Some(run) = state.active_run() {
        if view.selected_services.is_empty() {
            if let Some(rep) = &run.report {
                computed_report = linkcheck::analyze(registry, rep, rep.generated_at.clone());
                &computed_report
            } else {
                let (_, ids) = build_preview_targets(state, view);
                computed_report = linkcheck::analyze_selection(
                    registry,
                    &ids,
                    parallaxe_core::state::format_time(time::OffsetDateTime::now_utc()),
                );
                &computed_report
            }
        } else {
            let (_, ids) = build_preview_targets(state, view);
            computed_report = linkcheck::analyze_selection(
                registry,
                &ids,
                parallaxe_core::state::format_time(time::OffsetDateTime::now_utc()),
            );
            &computed_report
        }
    } else {
        let (_, ids) = build_preview_targets(state, view);
        computed_report = linkcheck::analyze_selection(
            registry,
            &ids,
            parallaxe_core::state::format_time(time::OffsetDateTime::now_utc()),
        );
        &computed_report
    };

    let locale = state.current_locale();
    let card_outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let card_inner_w = (card_outer_w - SPACE_4 * 2.0).max(200.0);
        ui.set_width(card_inner_w);
        ui.set_max_width(card_inner_w);

        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(translate(locale, "gui.link.title", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
            let resolve_targets: Vec<String> = link_report
                .rows
                .iter()
                .map(|r| r.target.clone())
                .collect();
            let offline = state.active_run().is_some_and(|run| run.is_fixture);
            let cnt = resolve_targets.len().to_string();
            let label = if offline {
                translate(locale, "gui.link.btn.offline", &[])
            } else {
                translate(locale, "gui.link.btn.resolve", &[&cnt])
            };
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if btn_outlined(ui, p, &label, !offline && !resolve_targets.is_empty()).clicked() {
                    actions.push(UiAction::RunLinkResolve(resolve_targets));
                }
            });
        });

        ui.add_space(2.0);
        ui.label(
            RichText::new(translate(locale, "gui.link.subtitle", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        );

        ui.add_space(SPACE_2);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.route", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            )
            .on_hover_text(format!("route_get={}", link_report.capabilities.route_get));
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.dns_compare", &[]),
                p.warning_fg,
                p.warning_bg,
                p.warning_fg,
            )
            .on_hover_text(format!("dns_compare={}", link_report.capabilities.dns_compare));
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.dns_leak", &[]),
                p.text_secondary,
                p.card_alt,
                p.border,
            )
            .on_hover_text(format!("dns_leak={}", link_report.capabilities.dns_leak));
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.quic", &[]),
                p.text_secondary,
                p.card_alt,
                p.border,
            )
            .on_hover_text(format!("quic={}", link_report.capabilities.quic));
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.port_scan", &[]),
                p.danger_fg,
                p.danger_bg,
                p.danger_fg,
            )
            .on_hover_text(format!("port_scan={}", link_report.capabilities.port_scan));
        });

        ui.add_space(SPACE_2);
        let tun_key = match link_report.local_path.via_tun {
            Some(true) => "gui.link.tun.yes",
            Some(false) => "gui.link.tun.no",
            None => "gui.link.tun.unknown",
        };
        let tun_desc = translate(locale, tun_key, &[]);
        let iface_fallback = translate(locale, "gui.link.iface_unknown", &[]);
        let iface_desc = link_report
            .local_path
            .interface_alias
            .as_deref()
            .unwrap_or(&iface_fallback);
        let dns_cnt = link_report.dns_query_count.to_string();
        let http_cnt = link_report.http_request_count.to_string();
        ui.label(
            RichText::new(translate(
                locale,
                "gui.link.local_overview",
                &[&tun_desc, iface_desc, &dns_cnt, &http_cnt],
            ))
            .size(FONT_META)
            .color(p.text_secondary),
        );

        ui.add_space(SPACE_3);

        // 固定列宽 + 横向滚动四列数据表（全中文标签）
        let table_outer_w = ui.available_width();
        let col_gap = SPACE_2;
        let col_widths = [140.0_f32, 188.0, 162.0, 188.0, 188.0];
        let total_w: f32 = col_widths.iter().sum::<f32>() + col_gap * 4.0 + SPACE_4;

        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .show(ui, |ui| {
                ui.set_max_width(table_outer_w);
                ScrollArea::horizontal()
                    .id_salt("linkcheck_four_col_hscroll")
                    .max_width(table_outer_w)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_width(total_w.max(table_outer_w - 2.0));
                        egui::Frame::none()
                            .fill(p.table_head_bg)
                            .inner_margin(Margin::symmetric(SPACE_2, SPACE_2))
                            .show(ui, |ui| {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = col_gap;
                                    let link_headers = [
                                        translate(locale, "gui.link.col.service", &[]),
                                        translate(locale, "gui.link.col.dns", &[]),
                                        translate(locale, "gui.link.col.edge", &[]),
                                        translate(locale, "gui.link.col.egress", &[]),
                                        translate(locale, "gui.link.col.dualstack", &[]),
                                    ];
                                    for (w, h) in col_widths.into_iter().zip(&link_headers) {
                                        link_cell(ui, w, |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(h)
                                                        .size(FONT_META)
                                                        .strong()
                                                        .color(p.text_secondary),
                                                )
                                                .wrap_mode(egui::TextWrapMode::Extend),
                                            );
                                        });
                                    }
                                });
                            });

                        for (idx, row) in link_report.rows.iter().enumerate() {
                            let is_user_target = registry
                                .user_targets()
                                .into_iter()
                                .any(|m| m.id == row.target);
                            let bg = if idx % 2 == 1 {
                                p.table_even_bg
                            } else {
                                p.card
                            };
                            let resp = egui::Frame::none()
                                .fill(bg)
                                .inner_margin(Margin::symmetric(SPACE_2, SPACE_2))
                                .show(ui, |ui| {
                                    ui.horizontal_top(|ui| {
                                        ui.spacing_mut().item_spacing.x = col_gap;

                                        // 列 0：目标服务
                                        link_cell(ui, col_widths[0], |ui| {
                                            ui.label(
                                                RichText::new(service_name(&row.target))
                                                    .strong()
                                                    .size(FONT_BODY)
                                                    .color(p.text_primary),
                                            )
                                            .on_hover_text(&row.target);
                                            ui.add_space(2.0);
                                            ui.label(
                                                RichText::new(&row.host)
                                                    .monospace()
                                                    .size(FONT_META)
                                                    .color(p.text_secondary),
                                            );
                                            ui.add_space(2.0);
                                            render_pill(
                                                ui,
                                                if is_user_target { "用户导入" } else { "内置目录" },
                                                if is_user_target {
                                                    p.warning_fg
                                                } else {
                                                    p.text_muted
                                                },
                                                p.card_alt,
                                                p.border_subtle,
                                            );
                                        });

                                        // 列 1：域名解析
                                        link_cell(ui, col_widths[1], |ui| {
                                            human_status_pill(
                                                ui,
                                                None,
                                                &row.resolver.status,
                                                locale,
                                                p,
                                            );
                                            ui.add_space(SPACE_1);
                                            ui.label(
                                                RichText::new(format!(
                                                    "IPv4 {} · IPv6 {}",
                                                    row.resolver.v4, row.resolver.v6
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                            );
                                            ui.label(
                                                RichText::new(humanize_link_reason(
                                                    row.resolver.reason.as_deref(),
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_muted),
                                            );
                                        });

                                        // 列 2：连接边缘
                                        link_cell(ui, col_widths[2], |ui| {
                                            human_status_pill(
                                                ui,
                                                None,
                                                &row.connection_edge.status,
                                                locale,
                                                p,
                                            );
                                            ui.add_space(SPACE_1);
                                            let fam = row
                                                .connection_edge
                                                .observed_family
                                                .as_deref()
                                                .unwrap_or("—");
                                            ui.label(
                                                RichText::new(format!(
                                                    "{fam} · {} tries",
                                                    row.connection_edge.attempts.len()
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                            );
                                            let peer = row
                                                .connection_edge
                                                .peer_address
                                                .as_deref()
                                                .unwrap_or("—");
                                            ui.label(
                                                RichText::new(peer)
                                                    .size(FONT_META)
                                                    .color(p.text_muted),
                                            );
                                        });

                                        // 列 3：网页出口
                                        link_cell(ui, col_widths[3], |ui| {
                                            human_status_pill(
                                                ui,
                                                None,
                                                &row.http_egress.status,
                                                locale,
                                                p,
                                            );
                                            ui.add_space(SPACE_1);
                                            let country = row
                                                .http_egress
                                                .observed_country
                                                .as_deref()
                                                .unwrap_or("—");
                                            ui.label(
                                                RichText::new(format!(
                                                    "{country} · {}",
                                                    humanize_layer_code(
                                                        row.http_egress.layer.as_deref()
                                                    )
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                            );
                                            ui.label(
                                                RichText::new(humanize_link_reason(
                                                    row.http_egress.reason.as_deref(),
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_muted),
                                            );
                                        });

                                        // 列 4：双栈状态
                                        link_cell(ui, col_widths[4], |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = SPACE_1;
                                                human_status_pill(
                                                    ui,
                                                    Some("IPv4"),
                                                    &row.address_family.v4,
                                                    locale,
                                                    p,
                                                );
                                            });
                                            ui.add_space(2.0);
                                            let (v6_lbl, _, _) = humanize_link_status(
                                                &row.address_family.v6,
                                                locale,
                                            );
                                            ui.label(
                                                RichText::new(format!("IPv6: {v6_lbl}"))
                                                    .size(FONT_META)
                                                    .color(p.text_secondary),
                                            );
                                            ui.label(
                                                RichText::new(if row.address_family.fallback {
                                                    "已触发双栈回退"
                                                } else {
                                                    "未触发回退"
                                                })
                                                .size(FONT_META)
                                                .color(p.text_muted),
                                            );
                                        });
                                    });
                                });
                            let rect = resp.response.rect;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.left(), rect.top()),
                                    egui::pos2(rect.right(), rect.top()),
                                ],
                                Stroke::new(1.0_f32, p.border_subtle),
                            );
                        }
                    });
            });

        ui.add_space(SPACE_2);
        ui.collapsing("查看链路技术字段（parallaxe.link/v1）", |ui| {
            ui.monospace(format!(
                "schema=parallaxe.link/v1 · show_ip={show_ip}\nroute_get={} · dns_compare={} · dns_leak={} · quic={} · port_scan={}\nvia_tun={:?} · interface_alias={:?}",
                link_report.capabilities.route_get,
                link_report.capabilities.dns_compare,
                link_report.capabilities.dns_leak,
                link_report.capabilities.quic,
                link_report.capabilities.port_scan,
                link_report.local_path.via_tun,
                link_report.local_path.interface_alias,
            ));
        });
    });
}

fn build_preview_targets(state: &AppState, view: &GuardViewState) -> (String, Vec<String>) {
    let source_run_id = state
        .active_run()
        .map(|r| r.run_id.clone())
        .unwrap_or_else(|| "run-manual-preview".to_string());
    if !view.selected_services.is_empty() {
        return (
            source_run_id,
            view.selected_services.iter().cloned().collect(),
        );
    }
    if let Some(run) = state.active_run() {
        let mut ids: Vec<String> = run
            .ordered_results()
            .into_iter()
            .map(|r| r.target.clone())
            .collect();
        if ids.is_empty() {
            ids = run.selected_ids.iter().take(6).cloned().collect();
        }
        if !ids.is_empty() {
            return (source_run_id, ids);
        }
    }
    (
        source_run_id,
        vec![
            "google.books".to_string(),
            "google.one".to_string(),
            "google.play".to_string(),
        ],
    )
}
