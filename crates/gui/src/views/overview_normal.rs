//! 总览诊断仪表盘摘要卡与底部上下文卡：只展示全量计数与 core 已有的观测；空状态不是零异常。
//!
//! 视觉与布局原则：
//! - 4 张独立等宽指标卡（参考 shadcn-admin 统计卡），卡片间距 = 16px >= 内边距 = 16px；
//! - 等宽数字字体（monospace）对齐数值，仅异常/冲突非零时以语义色强调；
//! - 底部双栏上下文卡等宽对齐，间距 16px，窄屏自动切单列堆叠。

use crate::{
    icons::{icon, icon_tile, IconKind},
    layout::ActiveTab,
    state::RunView,
    theme::{
        card_frame, render_animated_bar, spaced_columns, Palette, FONT_BODY, FONT_DISPLAY,
        FONT_META, FONT_TITLE, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
    },
    views::{service_matrix::service_name, UiAction},
};
use egui::{RichText, Ui};
use parallaxe_core::i18n::{translate, Locale};

struct KpiSpec {
    title: String,
    value: Option<String>,
    hint: String,
    icon: IconKind,
    color: egui::Color32,
    ratio: Option<f32>,
}

pub fn metrics(ui: &mut Ui, p: Palette, run: Option<&RunView>, locale: Locale, max_columns: usize) {
    let measured = run.filter(|r| !r.result_order.is_empty());
    let counts = measured.map(RunView::state_counts);
    let covered = run.and_then(|r| r.coverage.as_ref());

    let anomaly_val = counts.map(|c| c.definite_anomalies().to_string());
    let conflict_val = measured.map(|r| r.total_conflict_targets().to_string());
    let unknown_val = measured.map(|r| r.total_unknown_targets().to_string());
    let coverage_val = covered.map(|c| format!("{} / {}", c.covered.len(), c.selected_count));
    let coverage_ratio = covered.map(|c| {
        if c.selected_count > 0 {
            (c.covered.len() as f32) / (c.selected_count as f32)
        } else {
            0.0
        }
    });

    let items = [
        KpiSpec {
            title: translate(locale, "gui.kpi.anomaly.title", &[]),
            value: anomaly_val,
            hint: translate(locale, "gui.kpi.anomaly.hint", &[]),
            icon: IconKind::Alert,
            color: p.danger_fg,
            ratio: None,
        },
        KpiSpec {
            title: translate(locale, "gui.kpi.conflict.title", &[]),
            value: conflict_val,
            hint: translate(locale, "gui.kpi.conflict.hint", &[]),
            icon: IconKind::Layers,
            color: p.warning_fg,
            ratio: None,
        },
        KpiSpec {
            title: translate(locale, "gui.kpi.unknown.title", &[]),
            value: unknown_val,
            hint: translate(locale, "gui.kpi.unknown.hint", &[]),
            icon: IconKind::Globe,
            color: p.info_fg,
            ratio: None,
        },
        KpiSpec {
            title: translate(locale, "gui.kpi.coverage.title", &[]),
            value: coverage_val,
            hint: translate(locale, "gui.kpi.coverage.hint", &[]),
            icon: IconKind::Coverage,
            color: p.accent,
            ratio: coverage_ratio,
        },
    ];

    let cols = if ui.available_width() < 680.0 { 2 } else { 4 }.min(max_columns.max(1));
    for (row_idx, chunk) in items.chunks(cols).enumerate() {
        if row_idx > 0 {
            ui.add_space(SPACE_4);
        }
        spaced_columns(ui, chunk.len(), SPACE_4, |col_ui, idx| {
            let item = &chunk[idx];
            card_frame(p).show(col_ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(84.0);
                ui.horizontal_wrapped(|ui| {
                    let ic_col = match item.value.as_deref() {
                        Some(v) if v != "0" && v != "0 / 0" => item.color,
                        _ => p.text_muted,
                    };
                    icon(ui, item.icon, 14.5, ic_col);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&item.title)
                                .size(FONT_META)
                                .color(p.text_secondary),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(SPACE_1);
                let display = item.value.as_deref().unwrap_or("—");
                let num_col = if display == "0" || display == "—" || display == "0 / 0" {
                    p.text_primary
                } else {
                    item.color
                };
                ui.label(
                    RichText::new(display)
                        .monospace()
                        .strong()
                        .size(FONT_DISPLAY)
                        .color(num_col),
                );
                ui.add_space(SPACE_1);
                if let Some(r) = item.ratio {
                    ui.add_space(4.0);
                    render_animated_bar(
                        ui,
                        ui.make_persistent_id("overview_kpi_cov_bar"),
                        r,
                        4.0,
                        p.accent,
                        p.border_subtle,
                    );
                    ui.add_space(4.0);
                } else {
                    ui.label(
                        RichText::new(&item.hint)
                            .size(FONT_META)
                            .color(p.text_muted),
                    );
                }
            });
        });
    }
}

pub fn context(
    ui: &mut Ui,
    p: Palette,
    run: Option<&RunView>,
    locale: Locale,
    actions: &mut Vec<UiAction>,
) {
    let draw_card = |ui: &mut Ui, i: usize, actions: &mut Vec<UiAction>| {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(112.0);
            ui.horizontal(|ui| {
                let (ic, title, fg, bg) = if i == 0 {
                    (
                        IconKind::Coverage,
                        translate(locale, "gui.context.coverage.title", &[]),
                        p.accent,
                        p.accent_soft,
                    )
                } else {
                    let has_risk = run.is_some_and(|r| {
                        r.plans
                            .iter()
                            .any(|pl| !pl.affected_shared_paths.is_empty())
                    });
                    let title = translate(locale, "gui.context.shared.title", &[]);
                    if has_risk {
                        (IconKind::Layers, title, p.warning_fg, p.warning_bg)
                    } else {
                        (IconKind::Layers, title, p.accent, p.accent_soft)
                    }
                };
                icon_tile(ui, ic, 28.0, fg, bg);
                ui.add_space(SPACE_1);
                ui.label(RichText::new(title).strong().size(FONT_TITLE));
            });
            ui.add_space(SPACE_3);
            if i == 0 {
                if let Some(c) = run.and_then(|r| r.coverage.as_ref()) {
                    let cov_str = c.covered.len().to_string();
                    let uncov_str = c
                        .uncovered
                        .iter()
                        .filter(|e| e.selected)
                        .count()
                        .to_string();
                    ui.label(
                        RichText::new(translate(
                            locale,
                            "gui.context.coverage.summary",
                            &[&cov_str, &uncov_str],
                        ))
                        .size(FONT_BODY),
                    );
                } else {
                    ui.label(
                        RichText::new(translate(locale, "gui.context.coverage.empty", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                }
                ui.add_space(SPACE_2);
                if ui
                    .link(translate(locale, "gui.context.coverage.link", &[]))
                    .clicked()
                {
                    actions.push(UiAction::SwitchTab(ActiveTab::Coverage));
                }
            } else {
                let risk = run.and_then(|r| {
                    r.plans
                        .iter()
                        .find(|pl| !pl.affected_shared_paths.is_empty())
                });
                if let Some(plan) = risk {
                    let sep = if locale == Locale::EnUs { ", " } else { "、" };
                    let services = plan
                        .affected_services
                        .iter()
                        .map(|s| service_name(s))
                        .collect::<Vec<_>>()
                        .join(sep);
                    ui.label(
                        RichText::new(translate(
                            locale,
                            "gui.context.shared.warning",
                            &[&services],
                        ))
                        .size(FONT_BODY)
                        .color(p.warning_fg),
                    );
                } else {
                    ui.label(
                        RichText::new(translate(locale, "gui.context.shared.empty", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                }
                ui.add_space(SPACE_2);
                if ui
                    .link(translate(locale, "gui.context.shared.link", &[]))
                    .clicked()
                {
                    actions.push(UiAction::SwitchTab(ActiveTab::Guard));
                }
            }
        });
    };

    if ui.available_width() < 720.0 {
        for i in 0..2 {
            if i > 0 {
                ui.add_space(SPACE_4);
            }
            draw_card(ui, i, actions);
        }
    } else {
        spaced_columns(ui, 2, SPACE_4, |col_ui, i| {
            draw_card(col_ui, i, actions);
        });
    }
}
