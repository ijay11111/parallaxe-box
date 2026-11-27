//! 「本次覆盖」与「本次未覆盖」两栏视图（严格消费 `Coverage` 完整快照）。
//!
//! 表格化对齐改造（参考 the_grid + gpui-kit DataTable）：
//! - 两栏卡片间距 16px，内边距 16px；
//! - 内置固定列宽表格 + 横向滚动防折行，等宽数字呈现请求计数，`NOT_SELECTED`/`NO_ADMISSION` 等状态不挤碎。

use egui::{Margin, RichText, Rounding, ScrollArea, Stroke, Ui};

use crate::layout::ViewportTier;
use crate::state::AppState;
use crate::theme::{
    card_frame, render_pill, spaced_columns, subtle_well_frame, Palette, ThemeMode, FONT_BODY,
    FONT_META, FONT_TITLE, RADIUS_MD, SPACE_2, SPACE_3, SPACE_4,
};
use crate::views::UiAction;

/// 渲染两栏覆盖视图；保证每个目标归属清晰，不把未覆盖当作已通过。
pub fn render(
    ui: &mut Ui,
    state: &AppState,
    theme: ThemeMode,
    tier: ViewportTier,
    only_selected_uncovered: &mut bool,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);
    let Some(run) = state.active_run() else {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("尚无覆盖快照；启动检测或离线演示后将在此列出本次覆盖与未覆盖两栏。")
                    .size(FONT_BODY)
                    .color(p.text_secondary),
            );
        });
        return;
    };
    let Some(cov) = &run.coverage else {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(format!(
                    "尚未收到覆盖快照（当前预检选中 {} / 获准 {} / 跳过 {}）。",
                    run.selected_ids.len(),
                    run.admitted_ids.len(),
                    run.skipped.len()
                ))
                .size(FONT_BODY)
                .color(p.text_secondary),
            );
        });
        return;
    };

    let selected_uncovered_count = cov.uncovered.iter().filter(|u| u.selected).count();

    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_2;
            ui.label(
                RichText::new("覆盖两栏快照")
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
            render_pill(
                ui,
                &format!("目录 {}", cov.catalog_size),
                p.text_secondary,
                p.card_alt,
                p.border,
            );
            render_pill(
                ui,
                &format!("选中 {}", cov.selected_count),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            );
            render_pill(
                ui,
                &format!("已覆盖 {}", cov.covered.len()),
                p.accent,
                p.accent_soft,
                p.accent,
            );
            render_pill(
                ui,
                &format!("未覆盖 {} (已选未测 {})", cov.uncovered.len(), selected_uncovered_count),
                p.warning_fg,
                p.warning_bg,
                p.warning_fg,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(only_selected_uncovered, "未覆盖栏仅看「本次已选」项");
            });
        });
        ui.add_space(SPACE_2);
        ui.label(
            RichText::new(
                "口径说明：首个传输尝试即计入「本次覆盖」（失败也算已尝试，但无地区信号≠地区已判定）；未选、未获准、预算耗尽或取消未启动均归「本次未覆盖」。",
            )
            .size(FONT_META)
            .color(p.text_secondary),
        );
    });

    ui.add_space(SPACE_4);

    if tier == ViewportTier::Narrow {
        render_covered_card(ui, cov, p, actions);
        ui.add_space(SPACE_4);
        render_uncovered_card(ui, cov, p, *only_selected_uncovered, actions);
    } else {
        spaced_columns(ui, 2, SPACE_4, |col_ui, idx| {
            if idx == 0 {
                render_covered_card(col_ui, cov, p, actions);
            } else {
                render_uncovered_card(col_ui, cov, p, *only_selected_uncovered, actions);
            }
        });
    }
}

fn render_covered_card(
    ui: &mut Ui,
    cov: &parallaxe_core::contracts::Coverage,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("本次覆盖 ({})", cov.covered.len()))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.accent),
            );
        });
        ui.add_space(SPACE_2);
        if cov.covered.is_empty() {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("本次未产生任何传输尝试（0 项已覆盖）。")
                        .size(FONT_BODY)
                        .color(p.text_secondary),
                );
            });
            return;
        }
        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .inner_margin(Margin::same(SPACE_2))
            .show(ui, |ui| {
                ScrollArea::both()
                    .id_salt("covered_scroll")
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("covered_table_grid")
                            .num_columns(3)
                            .striped(true)
                            .spacing([SPACE_3, SPACE_2])
                            .min_col_width(72.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("目标 ID")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new("请求数")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new("传输状态")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.end_row();
                                for item in &cov.covered {
                                    if ui.link(&item.target).clicked() {
                                        actions.push(UiAction::SelectTarget(item.target.clone()));
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!("{}", item.request_count))
                                                .monospace()
                                                .size(FONT_META),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                    if item.http_received {
                                        render_pill(
                                            ui,
                                            "已收 HTTP",
                                            p.info_fg,
                                            p.info_bg,
                                            p.info_fg,
                                        );
                                    } else {
                                        render_pill(
                                            ui,
                                            "未收 HTTP（传输前失败）",
                                            p.warning_fg,
                                            p.warning_bg,
                                            p.warning_fg,
                                        );
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
    });
}

fn render_uncovered_card(
    ui: &mut Ui,
    cov: &parallaxe_core::contracts::Coverage,
    p: Palette,
    only_selected: bool,
    actions: &mut Vec<UiAction>,
) {
    let items: Vec<_> = cov
        .uncovered
        .iter()
        .filter(|item| !only_selected || item.selected)
        .collect();
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(format!(
                "本次未覆盖 ({} / 总计 {})",
                items.len(),
                cov.uncovered.len()
            ))
            .strong()
            .size(FONT_TITLE)
            .color(p.warning_fg),
        );
        ui.add_space(SPACE_2);
        if items.is_empty() {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("当前筛选条件下无未覆盖项。")
                        .size(FONT_BODY)
                        .color(p.text_secondary),
                );
            });
            return;
        }
        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .inner_margin(Margin::same(SPACE_2))
            .show(ui, |ui| {
                ScrollArea::both()
                    .id_salt("uncovered_scroll")
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("uncovered_table_grid")
                            .num_columns(3)
                            .striped(true)
                            .spacing([SPACE_3, SPACE_2])
                            .min_col_width(68.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("目标 ID")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new("范围")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new("未覆盖原因")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.end_row();
                                for item in items {
                                    if ui.link(&item.target).clicked() {
                                        actions.push(UiAction::SelectTarget(item.target.clone()));
                                    }
                                    if item.selected {
                                        render_pill(
                                            ui,
                                            "已选未测",
                                            p.warning_fg,
                                            p.warning_bg,
                                            p.warning_fg,
                                        );
                                    } else {
                                        render_pill(
                                            ui,
                                            "未选",
                                            p.text_secondary,
                                            p.card_alt,
                                            p.border,
                                        );
                                    }
                                    use parallaxe_core::contracts::reason;
                                    let label = match item.reason.as_str() {
                                        reason::NO_ADMISSION | reason::ALLOW_ONLINE_FALSE => {
                                            "未获联网许可"
                                        }
                                        reason::NOT_SELECTED => "不在本次范围",
                                        reason::SAFE_DEFAULT_FALSE => "默认不自动检测",
                                        reason::GLOBAL_TIMEOUT => "检测已超时",
                                        reason::CANCELLED => "检测已取消",
                                        reason::BUDGET_EXHAUSTED => "已到检测预算上限",
                                        reason::ADMISSION_GRANT_EXPIRED => "联网许可已过期",
                                        _ => "未执行，详见原因码",
                                    };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(label)
                                                .size(FONT_META)
                                                .color(p.text_primary),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
            });
    });
}
