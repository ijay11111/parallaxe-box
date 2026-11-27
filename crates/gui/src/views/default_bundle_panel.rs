//! 发行包预置默认规则包入口与专属授权确认框。
//!
//! 设计要点（与交接文档 1.2 一致）：
//! - 规则包编译期嵌入（用户无需自己找 zip / 手动导入）；
//! - 批量勾选 + 一次总确认；受保护目标需二次明确勾选；
//! - 授权范围二选一：「仅本次」（会话许可，退出失效）或「永久」
//!   （写入用户状态目录，重启有效，设置页可一键撤销）；
//! - 不勾 / 取消 = 不更换、不打开在线检测，保持零发包基线。

use std::collections::BTreeSet;

use egui::{vec2, Align, Align2, Layout, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::i18n::Locale;
use parallaxe_core::import_bundle::{
    bundled_default_rules_bundle, BundlePreviewReport, ImportBundle,
};
use parallaxe_core::targets::Registry;

use crate::icons::{icon, IconKind};
use crate::theme::{
    btn_filled, btn_outlined, card_frame, render_admonition, render_pill, Palette, FONT_BODY,
    FONT_META, FONT_TITLE, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3,
};
use crate::views::UiAction;

/// 授权生效范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundledAuthMode {
    Session,
    Permanent,
}

impl BundledAuthMode {
    fn label(self, locale: Locale) -> &'static str {
        match (self, locale) {
            (BundledAuthMode::Session, Locale::EnUs) => "This session only",
            (BundledAuthMode::Permanent, Locale::EnUs) => "Permanent (revocable)",
            (BundledAuthMode::Session, _) => "仅本次生效",
            (BundledAuthMode::Permanent, _) => "永久生效（可随时撤销）",
        }
    }

    fn hint(self, locale: Locale) -> &'static str {
        match (self, locale) {
            (BundledAuthMode::Session, Locale::EnUs) => {
                "Closing the app ends the authorization and restores the zero-request baseline."
            }
            (BundledAuthMode::Permanent, Locale::EnUs) => {
                "Saved to your local user data folder; you can revoke it anytime in Settings."
            }
            (BundledAuthMode::Session, _) => "关闭软件后授权失效，恢复默认零发包。",
            (BundledAuthMode::Permanent, _) => {
                "保存到本机用户状态目录，重启后仍有效；可随时在设置页一键撤销。"
            }
        }
    }
}

/// 默认规则包入口状态。
pub struct DefaultBundleState {
    pub bundle: Option<ImportBundle>,
    pub load_error: Option<String>,
    pub modal_open: bool,
    /// 勾选的预览行（1-based，与 `ImportBundle.targets` 顺序一致）。
    pub selected: BTreeSet<usize>,
    pub protected_confirmed: bool,
    pub mode: BundledAuthMode,
    pub feedback: Option<Result<String, String>>,
    pub select_all: bool,
}

impl Default for DefaultBundleState {
    fn default() -> Self {
        match bundled_default_rules_bundle() {
            Ok(bundle) => {
                let selected = (1..=bundle.targets.len()).collect();
                Self {
                    bundle: Some(bundle),
                    load_error: None,
                    modal_open: false,
                    selected,
                    protected_confirmed: false,
                    mode: BundledAuthMode::Session,
                    feedback: None,
                    select_all: true,
                }
            }
            Err(err) => Self {
                bundle: None,
                load_error: Some(err.to_string()),
                modal_open: false,
                selected: BTreeSet::new(),
                protected_confirmed: false,
                mode: BundledAuthMode::Session,
                feedback: None,
                select_all: false,
            },
        }
    }
}

impl DefaultBundleState {
    pub fn item_count(&self) -> usize {
        self.bundle.as_ref().map(|b| b.targets.len()).unwrap_or(0)
    }

    /// 重置为「全选 + 未确认 + 仅本次」的安全默认。
    pub fn reset_for_open(&mut self) {
        self.modal_open = true;
        self.selected = (1..=self.item_count()).collect();
        self.protected_confirmed = false;
        self.mode = BundledAuthMode::Session;
        self.select_all = true;
        self.feedback = None;
    }
}

fn preview_of(registry: &Registry, state: &DefaultBundleState) -> Option<BundlePreviewReport> {
    state
        .bundle
        .as_ref()
        .map(|bundle| registry.preview_import_bundle(bundle))
}

fn selected_counts(preview: &BundlePreviewReport, selected: &BTreeSet<usize>) -> (usize, usize) {
    let mut total = 0;
    let mut protected = 0;
    for row in &preview.targets {
        if selected.contains(&row.index) {
            total += 1;
            if row.duplicate_protected {
                protected += 1;
            }
        }
    }
    (total, protected)
}

/// 检查卡内的紧凑「我的授权版」入口（不额外占用垂直空间）。
pub fn render_scope_entry(
    ui: &mut Ui,
    registry: &Registry,
    state: &mut DefaultBundleState,
    locale: Locale,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    if state.bundle.is_none() {
        return;
    }
    let permanent = registry.authorized_user_target_ids().len();
    let session = registry
        .active_session_grant_snapshot()
        .map(|grant| grant.targets.len())
        .unwrap_or(0);
    let scope = if permanent > 0 { permanent } else { session };
    let label = if locale == Locale::EnUs {
        format!("Authorized edition · {} items", state.item_count())
    } else {
        format!("我的授权版 · {} 项", state.item_count())
    };
    let active = scope > 0;
    let fill = if active { p.accent_soft } else { p.card_alt };
    let stroke = if active {
        Stroke::new(1.0_f32, p.accent)
    } else {
        Stroke::new(1.0_f32, p.border)
    };
    let fg = if active { p.accent } else { p.text_primary };
    if ui
        .add(
            egui::Button::new(RichText::new(label).size(FONT_META).strong().color(fg))
                .fill(fill)
                .stroke(stroke)
                .rounding(Rounding::same(RADIUS_MD))
                .min_size(vec2(0.0, 30.0)),
        )
        .on_hover_text(if locale == Locale::EnUs {
            if registry.distribution_mode() {
                "Installer-provided rules pack. Choose this session or permanent scope for your authorized edition."
            } else {
                "Installer-provided rules pack. Confirm once to enable online checks for these targets."
            }
        } else if registry.distribution_mode() {
            "发行包已预置的默认规则包；请选择授权版目标的仅本次或永久生效范围。"
        } else {
            "发行包已预置的默认规则包；确认一次即可为这批目标开启在线检测。"
        })
        .clicked()
    {
        if scope > 0 {
            actions.push(UiAction::RunAuthorizedBundledTargets);
        } else {
            state.reset_for_open();
        }
    }
    if active
        && ui
            .small_button(RichText::new(if locale == Locale::EnUs {
                "Manage"
            } else {
                "管理"
            }))
            .clicked()
    {
        state.reset_for_open();
    }
    if let Some(Ok(msg)) = &state.feedback {
        ui.label(
            RichText::new(if locale == Locale::EnUs {
                "Authorized"
            } else {
                "已授权"
            })
            .size(FONT_META)
            .color(p.ok_fg),
        )
        .on_hover_text(msg);
    }
    if let Some(Err(msg)) = &state.feedback {
        ui.label(
            RichText::new(if locale == Locale::EnUs {
                "Authorization failed"
            } else {
                "授权失败"
            })
            .size(FONT_META)
            .color(p.danger_fg),
        )
        .on_hover_text(msg);
    }
}

/// 总览页的「我的授权版」入口卡（始终可见，零操作拿到目标）。
pub fn render_entry_card(
    ui: &mut Ui,
    registry: &Registry,
    state: &mut DefaultBundleState,
    locale: Locale,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let Some(bundle) = state.bundle.as_ref() else {
        return;
    };
    let preview = registry.preview_import_bundle(bundle);
    let replaced = preview
        .targets
        .iter()
        .filter(|row| registry.is_default_bundle_replacement(&row.planned_id))
        .count();
    let permanent_count = registry.authorized_user_target_ids().len();
    let session_count = registry
        .active_session_grant_snapshot()
        .map(|grant| grant.targets.len())
        .unwrap_or(0);
    let scope_count = if permanent_count > 0 {
        permanent_count
    } else {
        session_count
    };

    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            icon(ui, IconKind::Lock, 18.0, p.accent);
            let title = ui.label(
                RichText::new(if locale == Locale::EnUs {
                    "Authorized edition targets"
                } else {
                    "我的授权版"
                })
                .size(FONT_TITLE)
                .strong()
                .color(p.text_primary),
            );
            title.on_hover_text(if locale == Locale::EnUs {
                if registry.distribution_mode() {
                    "The installer already includes this rules pack. Choose which authorized editions to use and whether the scope is session-only or permanent."
                } else {
                    "The installer already includes this rules pack. Confirm once to replace the built-in targets with your authorized editions; nothing is sent before you confirm."
                }
            } else if registry.distribution_mode() {
                "发行包已预置这份默认规则包，无需自己找 zip 或手动导入；请选择授权版目标与仅本次/永久生效范围。"
            } else {
                "发行包已预置这份默认规则包，无需自己找 zip 或手动导入；核对一次并确认更换后即可在线检测，确认前保持零发包。"
            });
            render_pill(
                ui,
                &format!("{} 项", state.item_count()),
                p.accent,
                p.accent_soft,
                p.accent,
            );
            if permanent_count > 0 {
                render_pill(
                    ui,
                    if locale == Locale::EnUs {
                        "Permanent"
                    } else {
                        "永久授权"
                    },
                    p.ok_fg,
                    p.ok_bg,
                    p.ok_fg,
                );
            } else if session_count > 0 {
                render_pill(
                    ui,
                    if locale == Locale::EnUs {
                        "This session"
                    } else {
                        "仅本次"
                    },
                    p.info_fg,
                    p.info_bg,
                    p.info_fg,
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                if scope_count > 0
                    && btn_outlined(
                        ui,
                        p,
                        if locale == Locale::EnUs {
                            "Manage"
                        } else {
                            "管理授权"
                        },
                        true,
                    )
                    .clicked()
                {
                    state.reset_for_open();
                }
                if btn_filled(
                    ui,
                    p,
                    if scope_count > 0 {
                        if locale == Locale::EnUs {
                            "Run authorized targets"
                        } else {
                            "开始检测（我的授权版）"
                        }
                    } else if registry.distribution_mode() {
                        if locale == Locale::EnUs {
                            "Choose scope"
                        } else {
                            "选择生效范围"
                        }
                    } else if replaced > 0 {
                        if locale == Locale::EnUs {
                            "Re-authorize"
                        } else {
                            "重新授权"
                        }
                    } else if locale == Locale::EnUs {
                        "Review and authorize"
                    } else {
                        "查看并授权"
                    },
                    true,
                )
                .clicked()
                {
                    if scope_count > 0 {
                        actions.push(UiAction::RunAuthorizedBundledTargets);
                    } else {
                        state.reset_for_open();
                    }
                }
            });
        });
        if let Some(Ok(msg)) = &state.feedback {
            ui.label(RichText::new(msg).size(FONT_META).color(p.ok_fg));
        }
        if let Some(Err(msg)) = &state.feedback {
            ui.label(RichText::new(msg).size(FONT_META).color(p.danger_fg));
        }
    });
}

/// 发行包模式只保留授权范围选择；内置门禁已由构建标记或开关 A 显式解除。
fn render_scope_only_modal(
    ctx: &egui::Context,
    state: &mut DefaultBundleState,
    locale: Locale,
    p: Palette,
    permanent_available: bool,
    actions: &mut Vec<UiAction>,
) {
    let screen = ctx.screen_rect();
    let safe = crate::layout::SafeAreaInsets::for_platform();
    let safe_top = safe.top + 12.0;
    let safe_bottom = safe.bottom + 12.0;
    let offset_y = (safe_top - safe_bottom) * 0.5;
    let width = (screen.width() - 32.0).clamp(280.0, 520.0);
    let max_h = (screen.height() - safe_top - safe_bottom).max(240.0);
    let mut cancel = false;
    let mut chosen: Option<BundledAuthMode> = None;

    egui::Area::new(egui::Id::new("default_bundle_scope_modal"))
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
                        icon(ui, IconKind::Lock, 18.0, p.accent);
                        ui.label(
                            RichText::new(if locale == Locale::EnUs {
                                "Choose authorized edition scope"
                            } else {
                                "选择授权版生效范围"
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
                    render_admonition(ui, p.info_bg, p.info_fg, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(if locale == Locale::EnUs {
                                    format!(
                                        "Distribution package mode is already active. Choose how long the {} authorized-edition targets should remain enabled. This step only sets the scope; it does not repeat the built-in gate confirmation.",
                                        state.item_count()
                                    )
                                } else {
                                    format!(
                                        "发行包模式已经开启。请选择这 {} 项授权版目标的生效范围；此处只设置范围，不再重复内置门禁确认。",
                                        state.item_count()
                                    )
                                })
                                .size(FONT_META)
                                .color(p.info_fg),
                            )
                            .wrap(),
                        );
                    });
                    ui.add_space(SPACE_2);
                    for mode in [BundledAuthMode::Session, BundledAuthMode::Permanent] {
                        let enabled = mode != BundledAuthMode::Permanent || permanent_available;
                        let button = egui::Button::new(
                            RichText::new(mode.label(locale)).size(FONT_BODY).strong(),
                        )
                        .fill(p.accent_soft)
                        .stroke(Stroke::new(1.0_f32, p.accent))
                        .rounding(Rounding::same(RADIUS_MD))
                        .min_size(vec2(0.0, 44.0));
                        if ui.add_enabled(enabled, button).clicked() {
                            chosen = Some(mode);
                        }
                        ui.label(
                            RichText::new(mode.hint(locale))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        ui.add_space(SPACE_1);
                    }
                    if !permanent_available {
                        ui.label(
                            RichText::new(if locale == Locale::EnUs {
                                "Permanent scope is unavailable on this platform."
                            } else {
                                "当前平台不支持永久落盘，只能选择「仅本次」。"
                            })
                            .size(FONT_META)
                            .color(p.warning_fg),
                        );
                    }
                    ui.add_space(SPACE_2);
                    if btn_outlined(
                        ui,
                        p,
                        if locale == Locale::EnUs { "Cancel" } else { "取消" },
                        true,
                    )
                    .clicked()
                    {
                        cancel = true;
                    }
                });
        });

    if let Some(mode) = chosen {
        actions.push(UiAction::ConfirmDefaultBundleAuth {
            permanent: mode == BundledAuthMode::Permanent,
            indices: (1..=state.item_count()).collect(),
        });
        state.modal_open = false;
    } else if cancel {
        state.modal_open = false;
    }
}

/// 专属确认弹窗：源码零发包模式为批量更换 + 受保护二次确认；发行包模式仅选择生效范围。
pub fn render_modal(
    ctx: &egui::Context,
    registry: &Registry,
    state: &mut DefaultBundleState,
    locale: Locale,
    p: Palette,
    permanent_available: bool,
    actions: &mut Vec<UiAction>,
) {
    if !state.modal_open || state.bundle.is_none() {
        return;
    }
    if registry.distribution_mode() {
        render_scope_only_modal(ctx, state, locale, p, permanent_available, actions);
        return;
    }
    let Some(preview) = preview_of(registry, state) else {
        return;
    };
    let (selected_total, selected_protected) = selected_counts(&preview, &state.selected);
    let duplicate_total = preview
        .targets
        .iter()
        .filter(|row| row.duplicate_of.is_some())
        .count();
    let protected_total = preview
        .targets
        .iter()
        .filter(|row| row.duplicate_protected)
        .count();
    let distribution_mode = registry.distribution_mode();

    let screen = ctx.screen_rect();
    let width = (screen.width() - 32.0).clamp(280.0, 720.0);
    let max_h = (screen.height() - 48.0).max(240.0);
    let mut open = state.modal_open;
    let mut confirm = false;
    let mut cancel = false;

    egui::Area::new(egui::Id::new("default_bundle_auth_modal"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .constrain(true)
        .show(ctx, |ui| {
            ui.painter().rect_filled(
                ctx.screen_rect(),
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
                        icon(ui, IconKind::Lock, 18.0, p.accent);
                        ui.label(
                            RichText::new(if locale == Locale::EnUs {
                                if distribution_mode {
                                    "Enable your authorized edition"
                                } else {
                                    "Confirm replacing built-in targets"
                                }
                            } else if distribution_mode {
                                "启用我的授权版"
                            } else {
                                "确认更换默认规则包"
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
                    render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
                        ui.spacing_mut().item_spacing.y = SPACE_1;
                        ui.add(
                            egui::Label::new(
                                RichText::new(if locale == Locale::EnUs {
                                    if distribution_mode {
                                        "Distribution package mode is already active. Choose the targets to replace with your authorized editions and select session-only or permanent scope. Protected targets still require a second explicit confirmation."
                                    } else {
                                        "You are replacing built-in targets with your authorized editions. After confirmation these targets open online checks and can be checked and exported. Nothing is sent before you confirm."
                                    }
                                } else if distribution_mode {
                                    "发行包模式已经开启。此处仅选择要更换为「用户授权版」的目标，并选择仅本次或永久生效范围；受保护目标仍需二次明确确认。"
                                } else {
                                    "你正在把内置目标更换为「用户授权版」。确认后这些目标将打开在线检测，可直接检测并导出报告；确认前不会发出任何请求。"
                                })
                                .size(FONT_META)
                                .color(p.warning_fg),
                            )
                            .wrap(),
                        );
                        let totals = if locale == Locale::EnUs {
                            format!(
                                "Total {} · duplicates {} · protected {}",
                                preview.targets.len(),
                                duplicate_total,
                                protected_total
                            )
                        } else {
                            format!(
                                "共 {} 项 · 与内置重复 {} 项 · 受保护 {} 项",
                                preview.targets.len(),
                                duplicate_total,
                                protected_total
                            )
                        };
                        ui.label(
                            RichText::new(totals)
                                .size(FONT_META)
                            .color(p.text_primary),
                        );
                    });
                    ui.add_space(SPACE_2);

                    // 操作反馈（成功/失败）都留在弹窗内，避免错误被静默吞掉。
                    if let Some(Ok(msg)) = &state.feedback {
                        ui.label(RichText::new(msg).size(FONT_META).color(p.ok_fg));
                        ui.add_space(SPACE_1);
                    }
                    if let Some(Err(msg)) = &state.feedback {
                        ui.label(RichText::new(msg).size(FONT_META).color(p.danger_fg));
                        ui.add_space(SPACE_1);
                    }

                    // 生效范围
                    ui.label(
                        RichText::new(if locale == Locale::EnUs {
                            "Authorization scope"
                        } else {
                            "授权生效范围"
                        })
                        .size(FONT_BODY)
                        .strong()
                        .color(p.text_primary),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for mode in [BundledAuthMode::Session, BundledAuthMode::Permanent] {
                            let enabled = mode != BundledAuthMode::Permanent || permanent_available;
                            let mut active = state.mode == mode;
                            if ui
                                .add_enabled(
                                    enabled,
                                    egui::Button::new(
                                        RichText::new(mode.label(locale)).size(FONT_META),
                                    )
                                    .fill(if active {
                                        p.accent_soft
                                    } else {
                                        egui::Color32::TRANSPARENT
                                    })
                                    .stroke(Stroke::new(
                                        1.0_f32,
                                        if active { p.accent } else { p.border },
                                    ))
                                    .rounding(Rounding::same(RADIUS_MD)),
                                )
                                .clicked()
                            {
                                state.mode = mode;
                                active = true;
                            }
                            if active {
                                ui.label(
                                    RichText::new(mode.hint(locale))
                                        .size(FONT_META)
                                        .color(p.text_secondary),
                                );
                            }
                        }
                        if !permanent_available {
                            ui.label(
                                RichText::new(if locale == Locale::EnUs {
                                    "Permanent authorization is unavailable on this platform; only this session can be used."
                                } else {
                                    "当前平台不支持永久落盘，只能选择「仅本次」。"
                                })
                                .size(FONT_META)
                                .color(p.warning_fg),
                            );
                        }
                    });

                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        let mut all = state.select_all;
                        if ui
                            .checkbox(
                                &mut all,
                                RichText::new(if locale == Locale::EnUs {
                                    "Select all targets"
                                } else {
                                    "全选目标"
                                })
                                .size(FONT_META)
                                .strong(),
                            )
                            .changed()
                        {
                            state.select_all = all;
                            if all {
                                state.selected = (1..=state.item_count()).collect();
                            } else {
                                state.selected.clear();
                                state.protected_confirmed = false;
                            }
                        }
                        let selected_label = if locale == Locale::EnUs {
                            format!(
                                "Selected {} / {}",
                                selected_total,
                                preview.targets.len()
                            )
                        } else {
                            format!(
                                "已选 {} / {}",
                                selected_total,
                                preview.targets.len()
                            )
                        };
                        ui.label(
                            RichText::new(selected_label)
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                    });
                    ui.add_space(SPACE_1);
                    let list_h = (max_h - 330.0).clamp(120.0, 320.0);
                    egui::Frame::none()
                        .fill(p.card_alt)
                        .stroke(Stroke::new(1.0_f32, p.border_subtle))
                        .rounding(Rounding::same(RADIUS_MD))
                        .inner_margin(Margin::same(SPACE_2))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ScrollArea::vertical()
                                .id_salt("default_bundle_target_list")
                                .max_height(list_h)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    let mut last_group: Option<String> = None;
                                    for row in &preview.targets {
                                        if last_group.as_deref() != Some(row.user_group.as_str()) {
                                            if last_group.is_some() {
                                                ui.add_space(SPACE_1);
                                            }
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} · {}",
                                                    row.user_group,
                                                    preview
                                                        .targets
                                                        .iter()
                                                        .filter(|r| {
                                                            r.user_group == row.user_group
                                                        })
                                                        .count()
                                                ))
                                                .size(FONT_META)
                                                .strong()
                                                .color(p.accent),
                                            );
                                            last_group = Some(row.user_group.clone());
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing =
                                                vec2(SPACE_1, SPACE_1);
                                            let mut checked = state.selected.contains(&row.index);
                                            if ui.checkbox(&mut checked, "").changed() {
                                                if checked {
                                                    state.selected.insert(row.index);
                                                } else {
                                                    state.selected.remove(&row.index);
                                                }
                                                state.select_all =
                                                    state.selected.len() == preview.targets.len();
                                            }
                                            // 交接文档 1.3：预览显示完整 user.* ID（默认完整，不做前缀隐藏）。
                                            ui.label(
                                                RichText::new(&row.planned_id)
                                                    .size(FONT_META)
                                                    .strong()
                                                    .color(p.text_primary),
                                            );
                                            if row.duplicate_protected {
                                                render_pill(
                                                    ui,
                                                    if locale == Locale::EnUs {
                                                        "Protected"
                                                    } else {
                                                        "受保护"
                                                    },
                                                    p.warning_fg,
                                                    p.warning_bg,
                                                    p.warning_fg,
                                                );
                                            }
                                            if row.duplicate_of.is_some() {
                                                render_pill(
                                                    ui,
                                                    if row.duplicate_is_user {
                                                        if locale == Locale::EnUs {
                                                            "Replaces imported"
                                                        } else {
                                                            "更换已导入"
                                                        }
                                                    } else if locale == Locale::EnUs {
                                                        "Replaces built-in"
                                                    } else {
                                                        "更换内置"
                                                    },
                                                    p.info_fg,
                                                    p.info_bg,
                                                    p.info_fg,
                                                );
                                            }
                                        });
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&row.url)
                                                    .size(FONT_META)
                                                    .color(p.text_secondary),
                                            )
                                            .wrap(),
                                        );
                                    }
                                });
                        });

                    ui.add_space(SPACE_2);
                    if selected_protected > 0 {
                        let msg = if locale == Locale::EnUs {
                            format!(
                                "I confirm replacing these {selected_protected} protected targets (secondary explicit confirmation)"
                            )
                        } else {
                            format!(
                                "我确认更换这 {selected_protected} 个受保护目标（二次明确确认）"
                            )
                        };
                        ui.checkbox(
                            &mut state.protected_confirmed,
                            RichText::new(msg)
                                .size(FONT_META)
                                .strong()
                                .color(p.warning_fg),
                        );
                    }

                    ui.add_space(SPACE_2);
                    let ready = selected_total > 0
                        && (selected_protected == 0 || state.protected_confirmed)
                        && (state.mode != BundledAuthMode::Permanent || permanent_available);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                        if btn_filled(
                            ui,
                            p,
                            if locale == Locale::EnUs {
                                if distribution_mode {
                                    "Confirm replacement & scope"
                                } else {
                                    "Confirm replacement & authorize"
                                }
                            } else if distribution_mode {
                                "确认更换并设置范围"
                            } else {
                                "确认更换并授权"
                            },
                            ready,
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                        if btn_outlined(
                            ui,
                            p,
                            if locale == Locale::EnUs {
                                "Cancel"
                            } else if distribution_mode {
                                "取消（不更换）"
                            } else {
                                "取消（不更换、零发包）"
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
        let indices: Vec<usize> = state.selected.iter().copied().collect();
        actions.push(UiAction::ConfirmDefaultBundleAuth {
            permanent: state.mode == BundledAuthMode::Permanent,
            indices,
        });
        open = false;
    }
    if cancel {
        open = false;
    }
    state.modal_open = open;
}

/// 设置页「撤销默认包授权」卡片。
pub fn render_revoke_card(
    ui: &mut Ui,
    registry: &Registry,
    locale: Locale,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let permanent = registry.authorized_user_target_ids().len();
    let session = registry
        .active_session_grant_snapshot()
        .map(|grant| grant.targets.len())
        .unwrap_or(0);
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            icon(ui, IconKind::Lock, 16.0, p.warning_fg);
            ui.label(
                RichText::new(if locale == Locale::EnUs {
                    "Authorized edition consent"
                } else {
                    "我的授权版 · 授权管理"
                })
                .size(FONT_BODY)
                .strong()
                .color(p.text_primary),
            );
            let status_label = if locale == Locale::EnUs {
                format!("permanent {permanent} · session {session}")
            } else {
                format!("永久 {permanent} 项 · 本次 {session} 项")
            };
            render_pill(
                ui,
                &status_label,
                p.text_secondary,
                p.card_alt,
                p.border,
            );
        });
        ui.add_space(SPACE_1);
        ui.add(
            egui::Label::new(
                RichText::new(if locale == Locale::EnUs {
                    if registry.distribution_mode() {
                        "Revoking removes the saved authorization for these authorized-edition targets. Built-in targets remain governed by distribution package mode."
                    } else {
                        "Revoking restores the zero-request baseline and removes the saved authorization file. Imported targets remain listed but cannot send requests."
                    }
                } else if registry.distribution_mode() {
                    "撤销后仅这批授权版目标恢复零发包，并删除本机授权文件；发行包模式下的内置目标仍按模式设置运行。"
                } else {
                    "撤销后恢复零发包基线，并删除本机保存的授权文件；已导入的目标仍会列出，但不会发出任何请求。"
                })
                .size(FONT_META)
                .color(p.text_secondary),
            )
            .wrap(),
        );
        ui.add_space(SPACE_2);
        if btn_outlined(
            ui,
            p,
            if locale == Locale::EnUs {
                "Revoke bundled authorization"
            } else {
                "撤销默认包授权"
            },
            permanent > 0 || session > 0,
        )
        .clicked()
        {
            actions.push(UiAction::RevokeDefaultBundleAuth);
        }
    });
}
