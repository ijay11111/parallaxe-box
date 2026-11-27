//! 源码构建专属的发行包模式入口与强制确认弹窗。
//!
//! 正式发行构建由 core 的编译期 feature 恒定为发行包模式，本模块不绘制任何入口。

use egui::{vec2, Align, Align2, Layout, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};

use parallaxe_core::distribution::DISTRIBUTION_RELEASE_BUILD;
use parallaxe_core::i18n::Locale;
use parallaxe_core::targets::Registry;

use crate::icons::{icon, IconKind};
use crate::theme::{
    btn_filled, btn_outlined, card_frame, render_admonition, render_pill, Palette, FONT_BODY,
    FONT_META, FONT_TITLE, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3,
};
use crate::views::UiAction;

/// 「关于软件」中的源码模式开关。Release 构建完全不调用本函数。
pub fn render_source_mode_card(
    ui: &mut Ui,
    registry: &Registry,
    locale: Locale,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    if DISTRIBUTION_RELEASE_BUILD {
        return;
    }
    let active = registry.distribution_mode();
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            icon(
                ui,
                if active { IconKind::Lock } else { IconKind::Info },
                18.0,
                if active { p.ok_fg } else { p.warning_fg },
            );
            ui.label(
                RichText::new(if locale == Locale::EnUs {
                    "Distribution package mode"
                } else {
                    "发行包模式"
                })
                .size(FONT_TITLE)
                .strong()
                .color(p.text_primary),
            );
            render_pill(
                ui,
                if active {
                    if locale == Locale::EnUs { "ON" } else { "已开启" }
                } else {
                    if locale == Locale::EnUs { "OFF" } else { "已关闭" }
                },
                if active { p.ok_fg } else { p.text_secondary },
                if active { p.ok_bg } else { p.card_alt },
                if active { p.ok_fg } else { p.border },
            );
        });
        ui.add_space(SPACE_1);
        ui.add(
            egui::Label::new(
                RichText::new(if locale == Locale::EnUs {
                    "Source builds default to zero requests. Enabling this testing mode requires explicit confirmation and lifts the built-in target gate. AI autonomous debugging remains a separate switch."
                } else {
                    "源码构建默认零发包。开启此测试模式会解除内置目标门禁，必须经过测试者确认；AI 自主调试是独立开关，不会随此操作联动。"
                })
                .size(FONT_META)
                .color(p.text_secondary),
            )
            .wrap(),
        );
        ui.add_space(SPACE_2);
        ui.horizontal_wrapped(|ui| {
            if active {
                if btn_outlined(
                    ui,
                    p,
                    if locale == Locale::EnUs {
                        "Return to zero-request mode"
                    } else {
                        "关闭发行包模式（恢复零发包）"
                    },
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::DisableDistributionMode);
                }
            } else if btn_filled(
                ui,
                p,
                if locale == Locale::EnUs {
                    "Switch to distribution package mode…"
                } else {
                    "切换到发行包模式…"
                },
                true,
            )
            .clicked()
            {
                actions.push(UiAction::RequestDistributionModeEnable);
            }
        });
    });
}

/// 强制确认弹窗：不确认时保持零发包，不允许静默提权。
pub fn render_confirm_modal(
    ctx: &egui::Context,
    open: &mut bool,
    locale: Locale,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    if !*open {
        return;
    }
    if DISTRIBUTION_RELEASE_BUILD {
        *open = false;
        return;
    }
    let screen = ctx.screen_rect();
    let safe = crate::layout::SafeAreaInsets::for_platform();
    let safe_top = safe.top + 12.0;
    let safe_bottom = safe.bottom + 12.0;
    let offset_y = (safe_top - safe_bottom) * 0.5;
    let width = (screen.width() - 32.0).clamp(280.0, 560.0);
    let max_h = (screen.height() - safe_top - safe_bottom).max(240.0);
    let mut keep_open = *open;
    let mut confirm = false;
    let mut cancel = false;

    egui::Area::new(egui::Id::new("distribution_mode_confirm_modal"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, offset_y])
        .constrain(true)
        .show(ctx, |ui| {
            ui.painter().rect_filled(
                screen,
                Rounding::ZERO,
                egui::Color32::from_black_alpha(120),
            );
            egui::Frame::window(&ctx.style())
                .fill(p.card)
                .stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(RADIUS_MD))
                .inner_margin(Margin::same(SPACE_3))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.set_max_height(max_h);
                    ui.horizontal_wrapped(|ui| {
                        icon(ui, IconKind::Info, 18.0, p.warning_fg);
                        ui.label(
                            RichText::new(if locale == Locale::EnUs {
                                "Confirm distribution package mode"
                            } else {
                                "确认切换到发行包模式"
                            })
                            .size(FONT_TITLE)
                            .strong()
                            .color(p.text_primary),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .small_button(RichText::new("×").size(FONT_TITLE))
                                .clicked()
                            {
                                cancel = true;
                            }
                        });
                    });
                    ui.add_space(SPACE_2);
                    ScrollArea::vertical()
                        .id_salt("distribution_mode_confirm_scroll")
                        .max_height((max_h - 150.0).max(120.0))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(if locale == Locale::EnUs {
                                            "This lifts the zero-request gate and allows real checks against all 107 built-in Google targets. Checks still enforce a 3.5-second interval, batching, TCP-first probing, and immediate stop on HTTP 403/429. Do you confirm that you are the tester and accept responsibility?"
                                        } else {
                                            "此操作将解除零发包门禁，允许对内置 Google 全量 107 项目标真实发包。检测仍遵守 3.5 秒间隔、分批、优先 TCP，遇到 403/429 立即停止。你确认自己是测试者并承担相应责任吗？"
                                        })
                                        .size(FONT_BODY)
                                        .color(p.warning_fg),
                                    )
                                    .wrap(),
                                );
                            });
                            ui.add_space(SPACE_2);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(if locale == Locale::EnUs {
                                        "AI autonomous debugging is independent. This confirmation does not enable it; when it is off, AI still needs per-run F3 confirmation."
                                    } else {
                                        "AI 自主调试与本开关完全独立。本次确认不会自动打开 AI 自主；AI 自主关闭时，AI 仍需逐次 F3 确认。"
                                    })
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        });
                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                        if btn_filled(
                            ui,
                            p,
                            if locale == Locale::EnUs {
                                "Confirm and enable"
                            } else {
                                "确认并开启"
                            },
                            true,
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                        if btn_outlined(
                            ui,
                            p,
                            if locale == Locale::EnUs {
                                "Cancel (keep zero requests)"
                            } else {
                                "取消（保持零发包）"
                            },
                            true,
                        )
                        .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
        });

    if confirm {
        actions.push(UiAction::ConfirmDistributionModeEnable);
        keep_open = false;
    }
    if cancel {
        keep_open = false;
    }
    *open = keep_open;
}
