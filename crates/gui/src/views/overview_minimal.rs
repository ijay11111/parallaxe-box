//! 极简工作区（三卡极简结构：当前出口 / 重要服务 / 需要注意），严格消费同一批结果，不另算结论。
//!
//! 跨端一致性（参考 Dioxus 同一组件跨宽/窄屏自适应思路 + 参考图 -3 / -8）：
//! - 桌面三卡等宽等高，卡片间距 = 16px（与卡片内边距相等，永不粘连）；
//! - 窄屏 `< 840px` 自动复用同一组卡片纵向单列堆叠，信息与冲突/UNKNOWN 零丢失。

use crate::{
    icons::{icon_tile, IconKind},
    layout::{ActiveTab, ViewportTier},
    state::RunView,
    theme::{
        btn_tonal, card_frame, render_admonition, render_animated_bar, render_pill,
        render_state_badge, spaced_columns, subtle_well_frame, Palette, ThemeMode, FONT_BODY,
        FONT_DISPLAY, FONT_META, FONT_TITLE, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
    },
    views::{eligibility_badge_button, service_matrix::service_name, UiAction},
};
use egui::{Margin, RichText, Rounding, Stroke, Ui};
use parallaxe_core::i18n::{translate, Locale};

pub fn render(
    ui: &mut Ui,
    run: Option<&RunView>,
    theme: ThemeMode,
    locale: Locale,
    tier: ViewportTier,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);
    let draw = |ui: &mut Ui, i: usize, actions: &mut Vec<UiAction>| {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if tier != ViewportTier::Narrow {
                ui.set_min_height(268.0);
            }
            let (ic, title_key, sub_key) = [
                (
                    IconKind::Globe,
                    "gui.minimal.egress.title",
                    "gui.minimal.egress.sub",
                ),
                (
                    IconKind::Services,
                    "gui.minimal.services.title",
                    "gui.minimal.services.sub",
                ),
                (
                    IconKind::Alert,
                    "gui.minimal.attention.title",
                    "gui.minimal.attention.sub",
                ),
            ][i];

            ui.horizontal(|ui| {
                let (fg, bg) = if i == 2 {
                    (p.warning_fg, p.warning_bg)
                } else {
                    (p.accent, p.accent_soft)
                };
                icon_tile(ui, ic, 32.0, fg, bg);
                ui.add_space(SPACE_1);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(translate(locale, title_key, &[]))
                            .size(FONT_TITLE)
                            .strong()
                            .color(p.text_primary),
                    );
                    ui.label(
                        RichText::new(translate(locale, sub_key, &[]))
                            .size(FONT_META)
                            .color(p.text_secondary),
                    );
                });
            });
            ui.add_space(SPACE_3);
            ui.separator();
            ui.add_space(SPACE_3);

            match i {
                0 => {
                    ui.horizontal_wrapped(|ui| {
                        let mode_key = if run.is_some_and(|r| r.is_fixture) {
                            "gui.minimal.mode.fixture"
                        } else if run.is_some() {
                            "gui.minimal.mode.live"
                        } else {
                            "gui.minimal.mode.idle"
                        };
                        ui.label(
                            RichText::new(translate(locale, mode_key, &[]))
                                .size(FONT_DISPLAY)
                                .strong()
                                .color(p.text_primary),
                        );
                        if let Some(r) = run {
                            if let Some(code) = r.exit_code() {
                                let (fg, bg, border) = match code {
                                    0 => (p.ok_fg, p.ok_bg, p.ok_fg),
                                    2 => (p.danger_fg, p.danger_bg, p.danger_fg),
                                    _ => (p.warning_fg, p.warning_bg, p.warning_fg),
                                };
                                let pill_text = if show_tech {
                                    format!("exit={code}")
                                } else {
                                    match (code, locale) {
                                        (0, Locale::EnUs) => "All Clear".to_string(),
                                        (0, _) => "结果正常".to_string(),
                                        (2, Locale::EnUs) => "Anomalies Found".to_string(),
                                        (2, _) => "发现异常".to_string(),
                                        (_, Locale::EnUs) => "Needs Attention".to_string(),
                                        (_, _) => "待进一步确认".to_string(),
                                    }
                                };
                                let pill = render_pill(ui, &pill_text, fg, bg, border);
                                if show_tech {
                                    pill.on_hover_text(format!("exit={code}"));
                                }
                            }
                        }
                    });
                    ui.add_space(SPACE_3);
                    subtle_well_frame(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(translate(locale, "gui.minimal.egress.note", &[]))
                                .size(FONT_BODY)
                                .color(p.text_primary),
                        );
                        if let Some(cov) = run.and_then(|r| r.coverage.as_ref()) {
                            let ratio = if cov.selected_count > 0 {
                                (cov.covered.len() as f32) / (cov.selected_count as f32)
                            } else {
                                0.0
                            };
                            let cov_str = cov.covered.len().to_string();
                            let sel_str = cov.selected_count.to_string();
                            ui.add_space(SPACE_2);
                            ui.label(
                                RichText::new(translate(
                                    locale,
                                    "gui.minimal.coverage_ratio",
                                    &[&cov_str, &sel_str],
                                ))
                                .monospace()
                                .size(FONT_META)
                                .color(p.text_secondary),
                            );
                            ui.add_space(SPACE_1);
                            render_animated_bar(
                                ui,
                                ui.make_persistent_id("minimal_cov_bar"),
                                ratio,
                                4.0,
                                p.accent,
                                p.border,
                            );
                        }
                    });
                    ui.add_space(SPACE_3);
                    if ui
                        .link(translate(locale, "gui.minimal.link.coverage", &[]))
                        .clicked()
                    {
                        actions.push(UiAction::SwitchTab(ActiveTab::Coverage));
                    }
                }
                1 => {
                    if let Some(r) = run {
                        let mut results = r.ordered_results();
                        results.sort_by_key(|r| super::service_matrix::priority(r));
                        let unknown_reg = translate(locale, "gui.matrix.region_unknown", &[]);
                        for (idx, item) in results.into_iter().take(3).enumerate() {
                            if idx > 0 {
                                ui.add_space(SPACE_2);
                            }
                            let country = item
                                .service_observed
                                .observed_country
                                .as_deref()
                                .unwrap_or(&unknown_reg);
                            egui::Frame::none()
                                .fill(p.card_alt)
                                .stroke(Stroke::new(1.0_f32, p.border_subtle))
                                .rounding(Rounding::same(RADIUS_MD))
                                .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing =
                                            egui::vec2(SPACE_2, SPACE_1);
                                        ui.label(
                                            RichText::new(service_name(&item.target))
                                                .size(FONT_BODY)
                                                .strong()
                                                .color(p.text_primary),
                                        );
                                        ui.label(
                                            RichText::new(country)
                                                .monospace()
                                                .strong()
                                                .size(FONT_BODY)
                                                .color(if country == "CN" {
                                                    p.warning_fg
                                                } else {
                                                    p.text_secondary
                                                }),
                                        );
                                        render_state_badge(ui, item.state, theme, locale, true);
                                    });
                                    if !item.eligibility_notes.is_empty() {
                                        ui.add_space(SPACE_1);
                                        ui.push_id(&item.target, |ui| {
                                            if eligibility_badge_button(
                                                ui,
                                                p,
                                                locale,
                                                item.eligibility_notes.len(),
                                                false,
                                            )
                                            .clicked()
                                            {
                                                actions.push(UiAction::SelectTarget(
                                                    item.target.clone(),
                                                ));
                                            }
                                        });
                                    }
                                });
                        }
                        if r.result_order.is_empty() {
                            subtle_well_frame(p).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new(translate(
                                        locale,
                                        "gui.minimal.empty_no_results",
                                        &[],
                                    ))
                                    .size(FONT_BODY)
                                    .color(p.text_secondary),
                                );
                            });
                        }
                    } else {
                        subtle_well_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(translate(locale, "gui.minimal.empty_services", &[]))
                                    .size(FONT_BODY)
                                    .color(p.text_secondary),
                            );
                        });
                    }
                    ui.add_space(SPACE_3);
                    if ui
                        .link(translate(locale, "gui.minimal.link.services", &[]))
                        .clicked()
                    {
                        actions.push(UiAction::SwitchTab(ActiveTab::Services));
                    }
                }
                _ => {
                    if let Some(r) = run.filter(|r| !r.result_order.is_empty()) {
                        let c = r.state_counts();
                        let has_anomaly = c.definite_anomalies() > 0;
                        let conflicts = r.total_conflict_targets();
                        let unknowns = r.total_unknown_targets();
                        let (bg, stripe) = if has_anomaly {
                            (p.warning_bg, p.warning_fg)
                        } else if conflicts > 0 || unknowns > 0 {
                            (p.info_bg, p.info_fg)
                        } else {
                            (p.card_alt, p.border_hover)
                        };
                        let anom_str = c.definite_anomalies().to_string();
                        let conf_str = conflicts.to_string();
                        let unk_str = unknowns.to_string();
                        render_admonition(ui, bg, stripe, |ui| {
                            ui.label(
                                RichText::new(translate(
                                    locale,
                                    "gui.minimal.anomalies_count",
                                    &[&anom_str],
                                ))
                                .size(FONT_TITLE)
                                .strong()
                                .color(if has_anomaly {
                                    p.warning_fg
                                } else {
                                    p.text_primary
                                }),
                            );
                            ui.add_space(SPACE_1);
                            ui.label(
                                RichText::new(translate(
                                    locale,
                                    "gui.minimal.conflicts_unknowns",
                                    &[&conf_str, &unk_str],
                                ))
                                .monospace()
                                .size(FONT_META)
                                .color(p.text_secondary),
                            );
                        });
                        if let Some(top_item) = r
                            .ordered_results()
                            .into_iter()
                            .min_by_key(|item| super::service_matrix::priority(item))
                            .filter(|item| super::service_matrix::priority(item) < 5)
                        {
                            ui.add_space(SPACE_3);
                            let btn_label = translate(
                                locale,
                                "gui.minimal.open_plan",
                                &[service_name(&top_item.target)],
                            );
                            if btn_tonal(ui, p, &btn_label, true).clicked() {
                                actions.push(UiAction::OpenPlan(top_item.target.clone()));
                            }
                        }
                    } else if run.is_some_and(RunView::is_zero_admission_online) {
                        subtle_well_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(translate(locale, "gui.minimal.zero_admission", &[]))
                                    .size(FONT_BODY)
                                    .color(p.warning_fg),
                            );
                        });
                    } else {
                        subtle_well_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(translate(
                                    locale,
                                    "gui.minimal.empty_attention",
                                    &[],
                                ))
                                .size(FONT_BODY)
                                .color(p.text_secondary),
                            );
                        });
                    }
                }
            }
        });
    };

    if tier == ViewportTier::Narrow {
        for i in 0..3 {
            if i > 0 {
                ui.add_space(SPACE_4);
            }
            draw(ui, i, actions);
        }
        // 唯一主 CTA「开始检测」位于上方检测控制卡内；三卡下方仅保留次级跳转入口（>= 44px 触控高度）。
        ui.add_space(SPACE_4);
        let cta_w = ui.available_width();
        let detail = ui.add_sized(
            [cta_w, 44.0],
            egui::Button::new(
                RichText::new(translate(locale, "gui.minimal.link.services", &[]))
                    .size(FONT_BODY)
                    .color(p.text_primary),
            )
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border))
            .rounding(Rounding::same(RADIUS_MD)),
        );
        if detail.clicked() {
            actions.push(UiAction::SwitchTab(ActiveTab::Services));
        }
    } else {
        spaced_columns(ui, 3, SPACE_4, |col_ui, i| {
            draw(col_ui, i, actions);
        });
    }
}
