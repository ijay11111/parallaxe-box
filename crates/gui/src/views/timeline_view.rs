//! 事件时间线、本地运行历史（重测不覆盖）与同源核心导出器。

use std::path::PathBuf;

use egui::{Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::contracts::EventName;
use parallaxe_core::i18n::{translate, Locale};

use crate::commands::ExportFormat;
use crate::state::AppState;
use crate::theme::{
    btn_outlined, btn_tonal, card_frame, render_pill, subtle_well_frame, Palette, ThemeMode,
    FONT_BODY, FONT_META, FONT_TITLE, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
};
use crate::views::UiAction;

fn human_event_name(event: EventName, locale: Locale) -> &'static str {
    if locale == Locale::EnUs {
        match event {
            EventName::RunStarted => "Started",
            EventName::PreflightReady => "Preflight",
            EventName::TargetStarted => "Checking",
            EventName::EvidenceObserved => "Evidence",
            EventName::TargetFinished => "Completed",
            EventName::CoverageUpdated => "Coverage",
            EventName::GuardPlanReady => "Guard Preview",
            EventName::Error => "Error",
            EventName::RunFinished => "Finished",
            EventName::RunCancelled => "Cancelled",
        }
    } else {
        match event {
            EventName::RunStarted => "开始检测",
            EventName::PreflightReady => "预检就绪",
            EventName::TargetStarted => "开始服务",
            EventName::EvidenceObserved => "记录证据",
            EventName::TargetFinished => "完成服务",
            EventName::CoverageUpdated => "更新覆盖",
            EventName::GuardPlanReady => "生成预演",
            EventName::Error => "发生错误",
            EventName::RunFinished => "检测结束",
            EventName::RunCancelled => "检测取消",
        }
    }
}

fn plain_timeline_summary(raw: &str) -> String {
    raw.replace("模式=FIXTURE", "模式：离线演示")
        .replace("模式=ONLINE", "模式：在线只读")
        .replace("termination=COMPLETE", "状态：已完成")
        .replace("termination=CANCELLED", "状态：已取消")
        .replace("termination=GLOBAL_TIMEOUT", "状态：全局超时")
        .replace("termination=FAILED", "状态：失败")
        .replace("termination=PARTIAL", "状态：部分完成")
        .replace("CONFLICTING_EVIDENCE", "证据存在冲突")
        .replace("REGION_RESTRICTED_FIXTURE", "演示地区受限")
        .replace("POLICY_BLOCKED_FIXTURE", "演示策略阻断")
        .replace("REGION_RESTRICTED", "服务返回明确地区限制")
        .replace("POLICY_BLOCKED", "服务返回明确策略阻断")
        .replace("ADDRESS_MISMATCH", "连接落点与解析不一致")
        .replace("UNEXPECTED_REGION", "连接地区与预期不符")
        .replace("REDIRECT_TO_LOGIN", "跳转登录页")
        .replace("CAPTCHA_CHALLENGE", "遇到验证码")
        .replace("LOCAL_PATH_ERROR", "连接路径异常")
        .replace("LOCAL_QOS_SUSPECT", "疑似网络限速")
        .replace("MULTI_AIRPORT_UDP_FAILURE", "多节点 UDP 异常")
        .replace("REACHABLE_ONLY", "仅可达")
        .replace("SIGNAL_STALE", "规则失效")
        .replace("UNSUPPORTED", "暂不支持")
        .replace("RESTRICTED", "受限")
        .replace("BLOCKED", "被阻断")
        .replace("CONFLICT", "信号冲突")
        .replace("UNKNOWN", "未知")
        .replace("NO_SAFE_BLOCK", "暂不可自动屏蔽")
        .replace("safe_to_apply=false", "不可自动应用")
        .replace("safe_to_apply=true", "可人工评估")
        .replace("applied=false", "未改动系统")
        .replace("path-user-offline-preview", "离线预演路径")
        .replace("path-selftest", "自检路径")
        .replace("path-interface", "本机网卡路径")
        .replace("path-proxy", "代理路径")
        .replace("path-fixture", "内置示例路径")
        .replace("path-system", "系统默认路径")
        .replace("claim=", "结论：")
        .replace("value=", "值：")
}

/// 渲染事件时间线、历史运行切换与报告导出区。
#[allow(clippy::too_many_arguments)]
pub fn render(
    ui: &mut Ui,
    state: &AppState,
    theme: ThemeMode,
    export_path_input: &mut String,
    export_preview: Option<(ExportFormat, &str)>,
    export_status: Option<&str>,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);
    let locale = state.current_locale();

    // 1. 历史运行切换条（每次重测独立保存，永不覆盖旧证据）
    if !state.run_order.is_empty() {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(translate(
                    locale,
                    "gui.timeline.history",
                    &[&state.run_order.len().to_string()],
                ))
                .size(FONT_TITLE)
                .strong()
                .color(p.text_primary),
            );
            ui.add_space(SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                for (idx, run_id) in state.run_order.iter().rev().enumerate() {
                    let is_active = state.active_run_id.as_deref() == Some(run_id.as_str());
                    let ordinal = (state.run_order.len() - idx).to_string();
                    let label = if let Some(r) = state.runs.get(run_id) {
                        let status_tag = match r.exit_code() {
                            Some(c) if show_tech => format!("exit={c}"),
                            Some(0) => translate(locale, "gui.timeline.status.ok", &[]),
                            Some(2) => translate(locale, "gui.timeline.status.anomalies", &[]),
                            Some(_) => translate(locale, "gui.timeline.status.attention", &[]),
                            None => translate(locale, "gui.timeline.status.running", &[]),
                        };
                        if show_tech {
                            format!("{run_id} [{status_tag}]")
                        } else {
                            translate(locale, "gui.timeline.run_status", &[&ordinal, &status_tag])
                        }
                    } else {
                        translate(locale, "gui.timeline.run", &[&ordinal])
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
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(translate(locale, "gui.export.title", &[]))
                    .size(FONT_TITLE)
                    .strong()
                    .color(p.text_primary),
            );
            if matches!(
                run.report.as_ref().and_then(|r| r.scope),
                Some(parallaxe_core::contracts::ObservationScope::AndroidSimulatedUser)
            ) {
                render_pill(
                    ui,
                    &translate(locale, "gui.scope.android.badge", &[]),
                    p.info_fg,
                    p.info_bg,
                    p.info_fg,
                );
            }
            if run.report.as_ref().and_then(|r| r.origin.as_deref())
                == Some(parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS)
            {
                let auto_lbl = if show_tech {
                    format!(
                        "{} ({})",
                        translate(locale, "gui.ai.badge.autonomous_run", &[]),
                        parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS
                    )
                } else {
                    translate(locale, "gui.ai.badge.autonomous_run", &[])
                };
                let pill = render_pill(ui, &auto_lbl, p.warning_fg, p.warning_bg, p.warning_fg);
                if show_tech {
                    pill.on_hover_text(format!(
                        "origin={}",
                        parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS
                    ));
                }
            }
        });
        ui.add_space(SPACE_2);
        if run.report.is_none() {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.export.not_terminal", &[]))
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_2);
                for fmt in [
                    ExportFormat::Markdown,
                    ExportFormat::Text,
                    ExportFormat::Json,
                    ExportFormat::Jsonl,
                    ExportFormat::Html,
                    ExportFormat::Csv,
                ] {
                    let lbl = translate(locale, "gui.export.preview_btn", &[fmt.label()]);
                    if btn_tonal(ui, p, &lbl, true).clicked() {
                        *export_path_input = format!("parallaxe-report.{}", fmt.extension());
                        actions.push(UiAction::ExportPreview(fmt));
                    }
                }
            });
            ui.add_space(SPACE_3);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_2);
                ui.label(
                    RichText::new(translate(locale, "gui.export.save_label", &[]))
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                ui.add(egui::TextEdit::singleline(export_path_input).desired_width(240.0));
                for fmt in [
                    ExportFormat::Markdown,
                    ExportFormat::Text,
                    ExportFormat::Json,
                    ExportFormat::Jsonl,
                    ExportFormat::Html,
                    ExportFormat::Csv,
                ] {
                    let save_lbl = translate(locale, "gui.export.save_btn", &[fmt.extension()]);
                    if btn_outlined(ui, p, &save_lbl, true).clicked() {
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
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_2;
                    if btn_outlined(ui, p, &translate(locale, "gui.export.copy_btn", &[]), true)
                        .clicked()
                    {
                        ui.ctx().copy_text(content.to_string());
                    }
                    ui.label(
                        RichText::new(format!("{} · {} B", fmt.label(), content.len()))
                            .monospace()
                            .size(FONT_META)
                            .color(p.text_muted),
                    );
                });
                ui.add_space(SPACE_1);
                egui::CollapsingHeader::new(format!(
                    "导出预览（{}，{} 字节）",
                    fmt.label(),
                    content.len()
                ))
                .default_open(true)
                .show(ui, |ui| {
                    let snippet: String = content.chars().take(6000).collect();
                    ScrollArea::vertical()
                        .id_salt("export_preview_scroll")
                        .max_height(220.0)
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
                                    let ev_text = if show_tech {
                                        entry.event.as_str()
                                    } else {
                                        human_event_name(entry.event, locale)
                                    };
                                    let ev_label = ui.add(
                                        egui::Label::new(
                                            RichText::new(ev_text).size(FONT_META).color(p.accent),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                    if show_tech {
                                        ev_label.on_hover_text(entry.event.as_str());
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&entry.ts)
                                                .monospace()
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                    let disp_summary = if show_tech {
                                        entry.summary.clone()
                                    } else {
                                        plain_timeline_summary(&entry.summary)
                                    };
                                    if let Some(target) = &entry.target {
                                        if ui.link(&disp_summary).clicked() {
                                            actions.push(UiAction::SelectTarget(target.clone()));
                                        }
                                    } else {
                                        ui.label(RichText::new(&disp_summary).size(FONT_META));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
    });
}
