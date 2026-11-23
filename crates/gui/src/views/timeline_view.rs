//! 事件时间线、本地运行历史（重测不覆盖）与同源核心导出器。

use std::path::PathBuf;

use egui::{Margin, RichText, Rounding, ScrollArea, Stroke, Ui};

use crate::commands::ExportFormat;
use crate::state::AppState;
use crate::theme::{
    btn_outlined, btn_tonal, card_frame, render_pill, subtle_well_frame, Palette, ThemeMode,
    FONT_BODY, FONT_META, FONT_TITLE, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
};
use crate::views::UiAction;

/// 渲染事件时间线、历史运行切换与报告导出区。
#[allow(clippy::too_many_arguments)]
pub fn render(
    ui: &mut Ui,
    state: &AppState,
    theme: ThemeMode,
    export_path_input: &mut String,
    export_preview: Option<(ExportFormat, &str)>,
    export_status: Option<&str>,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);

    // 1. 历史运行切换条（每次重测独立保存，永不覆盖旧证据）
    if !state.run_order.is_empty() {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(format!("本地会话运行历史 ({})", state.run_order.len()))
                    .size(FONT_TITLE)
                    .strong()
                    .color(p.text_primary),
            );
            ui.add_space(SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                for run_id in state.run_order.iter().rev() {
                    let is_active = state.active_run_id.as_deref() == Some(run_id.as_str());
                    let label = if let Some(r) = state.runs.get(run_id) {
                        format!(
                            "{} [{}]",
                            run_id,
                            r.exit_code()
                                .map(|c| format!("exit={c}"))
                                .unwrap_or_else(|| "运行中".to_string())
                        )
                    } else {
                        run_id.clone()
                    };
                    if ui.selectable_label(is_active, label).clicked() {
                        actions.push(UiAction::SelectRun(run_id.clone()));
                    }
                }
            });
        });
        ui.add_space(SPACE_4);
    }

    let Some(run) = state.active_run() else {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("尚无事件流记录。")
                    .size(FONT_BODY)
                    .color(p.text_secondary),
            );
        });
        return;
    };

    // 2. 同源导出卡片（仅终态报告可导出，直接调用 parallaxe_core::reports）
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("报告导出（与 CLI/agent 共用 parallaxe_core::reports 导出器）")
                .size(FONT_TITLE)
                .strong()
                .color(p.text_primary),
        );
        ui.add_space(SPACE_2);
        if run.report.is_none() {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(
                        "运行尚未收到终结事件（RunFinished / RunCancelled），终结后方可导出。",
                    )
                    .size(FONT_META)
                    .color(p.text_secondary),
                );
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                for fmt in [
                    ExportFormat::Json,
                    ExportFormat::Jsonl,
                    ExportFormat::Html,
                    ExportFormat::Csv,
                    ExportFormat::Markdown,
                    ExportFormat::Text,
                ] {
                    if btn_tonal(ui, p, &format!("预览 {}", fmt.label()), true).clicked() {
                        actions.push(UiAction::ExportPreview(fmt));
                    }
                }
            });
            ui.add_space(SPACE_3);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                ui.label(
                    RichText::new("保存到本地路径：")
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                ui.add(egui::TextEdit::singleline(export_path_input).desired_width(240.0));
                for fmt in [
                    ExportFormat::Json,
                    ExportFormat::Jsonl,
                    ExportFormat::Html,
                    ExportFormat::Csv,
                    ExportFormat::Markdown,
                    ExportFormat::Text,
                ] {
                    if btn_outlined(ui, p, &format!("保存 .{}", fmt.extension()), true).clicked()
                    {
                        let trimmed = export_path_input.trim();
                        if !trimmed.is_empty() {
                            actions.push(UiAction::ExportToFile(fmt, PathBuf::from(trimmed)));
                        }
                    }
                }
            });
            if let Some(status) = export_status {
                ui.add_space(SPACE_2);
                ui.label(RichText::new(status).size(FONT_META).color(p.info_fg));
            }
            if let Some((fmt, content)) = export_preview {
                ui.add_space(SPACE_2);
                egui::CollapsingHeader::new(format!(
                    "导出预览（{}，{} 字节）",
                    fmt.label(),
                    content.len()
                ))
                .default_open(true)
                .show(ui, |ui| {
                    let snippet: String = content.chars().take(4000).collect();
                    ScrollArea::vertical()
                        .id_salt("export_preview_scroll")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut snippet.clone())
                                    .code_editor()
                                    .desired_width(f32::INFINITY),
                            );
                        });
                });
            }
        }
    });

    ui.add_space(SPACE_4);

    // 3. 严格按 seq 排列的单次运行事件时间线
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_2;
            ui.label(
                RichText::new("事件时间线")
                    .size(FONT_TITLE)
                    .strong()
                    .color(p.text_primary),
            );
            render_pill(
                ui,
                &format!("共 {} 条", run.timeline.len()),
                p.accent,
                p.accent_soft,
                p.accent,
            );
            render_pill(
                ui,
                &format!("去重 {}", run.ignored_duplicate_events),
                p.text_secondary,
                p.card_alt,
                p.border,
            );
            render_pill(
                ui,
                &format!("终结后忽略 {}", run.ignored_late_events),
                p.text_secondary,
                p.card_alt,
                p.border,
            );
            if run.seq_gap_detected {
                render_pill(
                    ui,
                    "[!] 存在序号缺口",
                    p.danger_fg,
                    p.danger_bg,
                    p.danger_fg,
                );
            }
        });
        ui.add_space(SPACE_3);

        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .inner_margin(Margin::same(SPACE_2))
            .show(ui, |ui| {
                ScrollArea::both()
                    .id_salt("timeline_events_scroll")
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("timeline_grid")
                            .num_columns(4)
                            .striped(true)
                            .spacing([SPACE_3, SPACE_2])
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("seq")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new("事件")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new("时间 (UTC)")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new("内容摘要")
                                        .size(FONT_META)
                                        .strong()
                                        .color(p.text_secondary),
                                );
                                ui.end_row();
                                for entry in &run.timeline {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!("#{}", entry.seq))
                                                .monospace()
                                                .size(FONT_META),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(entry.event.as_str())
                                                .monospace()
                                                .size(FONT_META)
                                                .color(p.accent),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&entry.ts)
                                                .monospace()
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                    if let Some(target) = &entry.target {
                                        if ui.link(&entry.summary).clicked() {
                                            actions.push(UiAction::SelectTarget(target.clone()));
                                        }
                                    } else {
                                        ui.label(RichText::new(&entry.summary).size(FONT_META));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
    });
}
