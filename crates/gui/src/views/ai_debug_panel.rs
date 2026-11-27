//! 任务 A（T-10-00）：AI 启动检测的 GUI 授权与「AI 自主调试 → 自主检测」会话级控制面。
//!
//! 严格遵守 T-10-00 裁决红线：
//! - 路径 A（默认）：AI 发起检测请求 → 弹出人话「一次性检测授权」确认框 → 用户点击确认后才真实发包；
//! - 路径 B（opt-in）：设置页「AI 自主调试 → 自主检测」开关默认关闭，仅存本会话内存、绝不落盘；
//! - 打开后 AI 权限几乎等同用户（自主检测 + 自主导出脱敏报告，报告强制标注 `origin=AGENT_AUTONOMOUS`）；
//! - AI 可随时自主关闭该开关，关闭后权限即时收回并展示明确提示；
//! - 无论路径 A 还是路径 B，均强制遵守 QoS 规范（间隔 >= 3500 ms、分批、优先 TCP、遇 403/429 即停）。

use egui::{vec2, Align, Layout, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS;
use parallaxe_core::i18n::{translate, Locale};

use crate::icons::{icon, icon_tile, IconKind};
use crate::theme::{
    btn_danger_tonal, btn_filled, btn_outlined, btn_tonal, card_frame, render_admonition,
    render_modal_scrim, render_pill, subtle_well_frame, Palette, FONT_BODY, FONT_META, FONT_TITLE,
    RADIUS_LG, SPACE_1, SPACE_2, SPACE_3, SPACE_5,
};
use crate::views::UiAction;

/// AI 发起检测时的强制最小请求间隔（3.5 秒，遵守全员 QoS 红线）。
pub const AI_MIN_INTERVAL_MS: u64 = 3_500;
/// AI 发起检测时的单批上限（避免高频并发触发运营商限速）。
pub const AI_MAX_BATCH_SIZE: u32 = 10;

/// 路径 A：待用户一键确认的一次性 AI 检测请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiCheckRequest {
    pub request_id: String,
    pub requester: String,
    pub profile_id: String,
    pub scope_label: String,
    pub target_count: usize,
    pub min_interval_ms: u64,
    pub batch_size: u32,
    pub purpose: String,
    /// 若来自 agent 授权桥：对应的桥请求 ID（GUI 确认后写回授权决定）。
    pub bridge_request_id: Option<String>,
    /// 桥请求的显式目标 ID（仅用户已导入目标会真实发包）。
    pub targets: Vec<String>,
    /// 是否按「自主检测」路径执行（默认 false = 路径 A 人工确认）。
    pub autonomous: bool,
}

impl AiCheckRequest {
    /// 构造带 QoS 强制下限的默认请求（供路径 A 演示与 agent 桥接复用）。
    pub fn sample_for_profile(
        profile_id: &str,
        scope_label: String,
        target_count: usize,
        min_interval_ms: u64,
        batch_size: u32,
        locale: Locale,
    ) -> Self {
        Self {
            request_id: format!(
                "ai-req-{}",
                time::OffsetDateTime::now_utc().unix_timestamp()
            ),
            requester: translate(locale, "gui.ai.f3.default_requester", &[]),
            profile_id: profile_id.to_string(),
            scope_label,
            target_count,
            min_interval_ms: min_interval_ms.max(AI_MIN_INTERVAL_MS),
            batch_size: batch_size.clamp(1, AI_MAX_BATCH_SIZE),
            purpose: translate(locale, "gui.ai.f3.default_purpose", &[]),
            bridge_request_id: None,
            targets: Vec::new(),
            autonomous: false,
        }
    }
}

/// AI 自主调试会话级状态（**严禁序列化落盘**，关闭软件即自动恢复关闭）。
#[derive(Debug, Clone, Default)]
pub struct AiDebugState {
    /// 路径 B 开关：默认关闭，仅会话内存态。
    pub autonomous_enabled: bool,
    /// 最近一次是否由 AI 自主关闭（用于展示清晰收回反馈）。
    pub revoked_by_ai: bool,
    /// AI 自主关闭时附带的简短原因说明。
    pub last_notice: Option<String>,
    /// 本会话内由 AI 自主模式发起的运行计数。
    pub autonomous_runs: u32,
    /// 本会话内由 AI 自主模式导出的脱敏报告计数。
    pub autonomous_exports: u32,
    /// 路径 A：等待用户一键确认的一次性 F3 授权请求。
    pub pending_f3_request: Option<AiCheckRequest>,
}

impl AiDebugState {
    /// 用户在设置页手动切换「自主检测」开关。
    pub fn set_autonomous_by_user(&mut self, enabled: bool, locale: Locale) {
        self.autonomous_enabled = enabled;
        self.revoked_by_ai = false;
        self.last_notice = Some(if enabled {
            translate(locale, "gui.ai.notice.enabled_by_user", &[])
        } else {
            translate(locale, "gui.ai.notice.disabled_by_user", &[])
        });
    }

    /// AI 自主关闭「自主检测」开关，立即收回后续自主检测与导出权限。
    pub fn self_disable_by_ai(&mut self, reason: Option<&str>, locale: Locale) -> bool {
        let was_enabled = self.autonomous_enabled;
        self.autonomous_enabled = false;
        self.revoked_by_ai = true;
        let reason_text = reason
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| translate(locale, "gui.ai.notice.ai_self_disable_default", &[]));
        self.last_notice = Some(translate(
            locale,
            "gui.ai.notice.disabled_by_ai",
            &[&reason_text],
        ));
        was_enabled
    }

    /// 清除提示横幅。
    pub fn clear_notice(&mut self) {
        self.last_notice = None;
        self.revoked_by_ai = false;
    }
}

/// 在总览/检测页顶部展示轻量状态提示（仅当自主模式开启或刚被 AI 自主关闭时显示）。
pub fn render_session_banner(
    ui: &mut Ui,
    ai: &mut AiDebugState,
    p: Palette,
    locale: Locale,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    if ai.autonomous_enabled {
        render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                icon(ui, IconKind::Guard, 15.0, p.warning_fg);
                let pill_text = if show_tech {
                    format!(
                        "{} (origin={ORIGIN_AGENT_AUTONOMOUS})",
                        translate(locale, "gui.ai.banner.active_pill", &[])
                    )
                } else {
                    translate(locale, "gui.ai.banner.active_pill", &[])
                };
                let pill = render_pill(ui, &pill_text, p.warning_fg, p.card, p.warning_fg);
                if show_tech {
                    pill.on_hover_text(format!("origin={ORIGIN_AGENT_AUTONOMOUS} · session-only"));
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.ai.banner.active_desc", &[]))
                            .size(FONT_META)
                            .color(p.text_primary),
                    )
                    .wrap(),
                );
                if btn_danger_tonal(
                    ui,
                    p,
                    &translate(locale, "gui.ai.btn.disable_now", &[]),
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::SetAiAutonomous(false));
                }
            });
        });
        ui.add_space(SPACE_3);
    } else if ai.revoked_by_ai {
        if let Some(notice) = ai.last_notice.clone() {
            render_admonition(ui, p.info_bg, p.info_fg, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                    icon(ui, IconKind::Check, 15.0, p.ok_fg);
                    render_pill(
                        ui,
                        &translate(locale, "gui.ai.status.revoked_by_ai", &[]),
                        p.ok_fg,
                        p.ok_bg,
                        p.ok_fg,
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(notice).size(FONT_META).color(p.text_primary),
                        )
                        .wrap(),
                    );
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.guard.modal.close", &[]),
                        true,
                    )
                    .clicked()
                    {
                        ai.clear_notice();
                    }
                });
            });
            ui.add_space(SPACE_3);
        }
    }
}

/// 路径 B 的运行条件与实时状态（桌面端文件桥；移动端为 None）。
#[derive(Debug, Clone, Copy, Default)]
pub struct AiBridgeView<'a> {
    /// 已导入的自定义网址数量。
    pub imported_targets: usize,
    /// 当前构建/会话是否处于发行包模式。
    pub distribution_mode: bool,
    /// 文件桥目录（仅技术细节开启时展示）。
    pub bridge_dir: Option<&'a str>,
    /// 最近一次桥错误。
    pub last_error: Option<&'a str>,
}

/// 在设置页渲染「AI 自主调试」卡片（路径 A 预览入口 + 路径 B「自主检测」会话级开关 + AI 自关演示）。
pub fn render_settings_card(
    ui: &mut Ui,
    ai: &AiDebugState,
    p: Palette,
    locale: Locale,
    show_tech: bool,
    bridge: Option<AiBridgeView<'_>>,
    actions: &mut Vec<UiAction>,
) {
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            icon_tile(ui, IconKind::Guard, 24.0, p.accent, p.accent_soft);
            ui.label(
                RichText::new(translate(locale, "gui.ai.card.title", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
            let (status_lbl, fg, bg) = if ai.autonomous_enabled {
                (
                    translate(locale, "gui.ai.status.enabled", &[]),
                    p.warning_fg,
                    p.warning_bg,
                )
            } else if ai.revoked_by_ai {
                (
                    translate(locale, "gui.ai.status.revoked_by_ai", &[]),
                    p.ok_fg,
                    p.ok_bg,
                )
            } else {
                (
                    translate(locale, "gui.ai.status.disabled_default", &[]),
                    p.text_secondary,
                    p.card_alt,
                )
            };
            render_pill(ui, &status_lbl, fg, bg, fg);
            render_pill(
                ui,
                &translate(locale, "gui.ai.pill.session_only", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            );
        });

        ui.add_space(SPACE_1);
        ui.add(
            egui::Label::new(
                RichText::new(translate(locale, "gui.ai.card.subtitle", &[]))
                    .size(FONT_META)
                    .color(p.text_secondary),
            )
            .wrap(),
        );

        ui.add_space(SPACE_2);

        // 路径 B：自主检测会话开关（默认关闭，仅内存不落盘）
        subtle_well_frame(p)
            .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                    let mut checked = ai.autonomous_enabled;
                    let toggle_resp = ui.checkbox(
                        &mut checked,
                        RichText::new(translate(locale, "gui.ai.toggle.autonomous", &[]))
                            .strong()
                            .size(FONT_BODY)
                            .color(p.text_primary),
                    );
                    if toggle_resp.changed() {
                        actions.push(UiAction::SetAiAutonomous(checked));
                    }
                    if btn_tonal(
                        ui,
                        p,
                        &translate(locale, "gui.ai.btn.open_f3_preview", &[]),
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::OpenAiF3AuthModal);
                    }
                    if ai.autonomous_enabled {
                        if btn_outlined(
                            ui,
                            p,
                            &translate(locale, "gui.ai.btn.simulate_ai_close", &[]),
                            true,
                        )
                        .clicked()
                        {
                            actions.push(UiAction::AiSelfDisableAutonomous);
                        }
                        if btn_danger_tonal(
                            ui,
                            p,
                            &translate(locale, "gui.ai.btn.disable_now", &[]),
                            true,
                        )
                        .clicked()
                        {
                            actions.push(UiAction::SetAiAutonomous(false));
                        }
                    }
                });
                ui.add_space(SPACE_1);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{} {}",
                            translate(locale, "gui.ai.toggle.desc", &[]),
                            translate(locale, "gui.ai.toggle.qos_note", &[]),
                        ))
                        .size(FONT_META)
                        .color(p.text_secondary),
                    )
                    .wrap(),
                );
                if show_tech {
                    ui.add_space(SPACE_1);
                    ui.label(
                        RichText::new(format!(
                            "origin={ORIGIN_AGENT_AUTONOMOUS} · persistence=NONE (memory-only) · runs={} · exports={}",
                            ai.autonomous_runs, ai.autonomous_exports
                        ))
                        .monospace()
                        .size(FONT_META)
                        .color(p.text_muted),
                    );
                }
            });

        // 路径 B 的运行条件（用户视角：开关打开后为什么还没开始跑）。
        ui.add_space(SPACE_2);
        subtle_well_frame(p)
            .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.ai.bridge.conditions_title", &[]))
                        .strong()
                        .size(FONT_BODY)
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_1);
                let scope_condition = if bridge.is_some_and(|view| view.distribution_mode) {
                    "gui.ai.bridge.condition_distribution"
                } else {
                    "gui.ai.bridge.condition_imported"
                };
                for key in [
                    "gui.ai.bridge.condition_gui",
                    scope_condition,
                    "gui.ai.bridge.condition_agent",
                ] {
                    ui.add(
                        egui::Label::new(
                            RichText::new(translate(locale, key, &[]))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(SPACE_1);
                if let Some(view) = bridge {
                    let (key, args): (&str, Vec<String>) = if view.distribution_mode
                        && ai.autonomous_enabled
                    {
                        ("gui.ai.bridge.status_distribution_waiting", Vec::new())
                    } else if view.distribution_mode {
                        ("gui.ai.bridge.status_distribution_path_a", Vec::new())
                    } else if ai.autonomous_enabled && view.imported_targets == 0 {
                        (
                            "gui.ai.bridge.status_no_targets",
                            vec![view.imported_targets.to_string()],
                        )
                    } else if ai.autonomous_enabled {
                        (
                            "gui.ai.bridge.status_waiting",
                            vec![view.imported_targets.to_string()],
                        )
                    } else {
                        (
                            "gui.ai.bridge.status_path_a",
                            vec![view.imported_targets.to_string()],
                        )
                    };
                    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
                    let color = if !view.distribution_mode
                        && ai.autonomous_enabled
                        && view.imported_targets == 0
                    {
                        p.warning_fg
                    } else {
                        p.text_primary
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(translate(locale, key, &arg_refs))
                                .size(FONT_META)
                                .color(color),
                        )
                        .wrap(),
                    );
                    if let Some(error) = view.last_error {
                        ui.add(
                            egui::Label::new(
                                RichText::new(error).size(FONT_META).color(p.danger_fg),
                            )
                            .wrap(),
                        );
                    }
                    if show_tech {
                        if let Some(dir) = view.bridge_dir {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("bridge_dir={dir}"))
                                        .monospace()
                                        .size(FONT_META)
                                        .color(p.text_muted),
                                )
                                .wrap(),
                            );
                        }
                    }
                } else {
                    ui.add(
                        egui::Label::new(
                            RichText::new(translate(locale, "gui.ai.bridge.mobile_note", &[]))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        )
                        .wrap(),
                    );
                }
            });

        if let Some(notice) = ai.last_notice.as_deref() {
            ui.add_space(SPACE_1);
            let color = if ai.autonomous_enabled {
                p.warning_fg
            } else {
                p.ok_fg
            };
            ui.add(
                egui::Label::new(RichText::new(notice).size(FONT_META).color(color)).wrap(),
            );
        }
    });
}

/// 路径 A：AI 请求启动检测时的「一次性检测授权（F3）」人话确认弹窗。
pub fn render_f3_auth_modal(
    ctx: &egui::Context,
    ai: &AiDebugState,
    p: Palette,
    locale: Locale,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let Some(req) = ai.pending_f3_request.as_ref() else {
        return;
    };

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        actions.push(UiAction::RejectAiF3Auth);
        return;
    }

    render_modal_scrim(ctx, "ai_f3_auth_scrim", p);

    let screen = ctx.screen_rect();
    let safe = crate::layout::SafeAreaInsets::for_platform();
    let narrow = screen.width() < 520.0;
    let compact_h = screen.height() < 460.0;
    let h_margin = if screen.width() < 480.0 { 12.0 } else { 24.0 };
    let inner_margin = if screen.width() < 480.0 || compact_h {
        SPACE_3
    } else {
        SPACE_5
    };
    let modal_w = (screen.width() - h_margin * 2.0 - inner_margin * 2.0 - 4.0).clamp(220.0, 520.0);
    let pad_v = if compact_h { 8.0 } else { 12.0 };
    let safe_top = safe.top + pad_v;
    let safe_bottom = safe.bottom + pad_v;
    let offset_y = (safe_top - safe_bottom) * 0.5;
    let max_win_h =
        (screen.height() - safe_top - safe_bottom - inner_margin * 2.0 - 4.0).max(140.0);
    // 为顶部标题栏和底部固定操作栏预留空间，确保主操作按钮在桌面和手机竖/横屏下始终立即可见
    let footer_reserve = if narrow {
        172.0
    } else if compact_h {
        156.0
    } else {
        128.0
    };
    let max_body_h = (max_win_h - footer_reserve).max(68.0);

    let title = if show_tech {
        format!("{} (F3)", translate(locale, "gui.ai.f3.modal_title", &[]))
    } else {
        translate(locale, "gui.ai.f3.modal_title", &[])
    };

    egui::Area::new(egui::Id::new("ai_f3_authorization_modal"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, offset_y])
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::window(&ctx.style())
                .fill(p.card)
                .stroke(Stroke::new(1.5_f32, p.accent))
                .rounding(Rounding::same(RADIUS_LG))
                .inner_margin(Margin::same(inner_margin))
                .show(ui, |ui| {
                    ui.set_width(modal_w);
                    ui.set_max_width(modal_w);

                    // 标题栏
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            vec2((modal_w - 34.0).max(150.0), 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&title)
                                            .size(if narrow { FONT_BODY + 1.0 } else { FONT_TITLE })
                                            .strong()
                                            .color(p.text_primary),
                                    )
                                    .wrap(),
                                );
                            },
                        );
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("×").size(16.0).color(p.text_secondary),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                actions.push(UiAction::RejectAiF3Auth);
                            }
                        });
                    });

                    ui.add_space(SPACE_2);
                    ui.separator();
                    ui.add_space(SPACE_2);

                    ScrollArea::vertical()
                        .id_salt("ai_f3_auth_modal_scroll")
                        .max_height(max_body_h)
                        .min_scrolled_height(max_body_h.min(if compact_h { 96.0 } else { 180.0 }))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.set_width(modal_w);
                            ui.set_max_width(modal_w);

                            render_admonition(ui, p.info_bg, p.info_fg, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(translate(
                                            locale,
                                            "gui.ai.f3.modal_lead",
                                            &[],
                                        ))
                                        .size(FONT_BODY)
                                        .color(p.text_primary),
                                    )
                                    .wrap(),
                                );
                            });

                            ui.add_space(SPACE_3);

                            // 结构化人话核对清单
                            let count_s = req.target_count.to_string();
                            let interval_s =
                                req.min_interval_ms.max(AI_MIN_INTERVAL_MS).to_string();
                            let batch_s = req.batch_size.clamp(1, AI_MAX_BATCH_SIZE).to_string();
                            subtle_well_frame(p)
                                .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let rows = [
                                        (
                                            translate(locale, "gui.ai.f3.field.requester", &[]),
                                            req.requester.clone(),
                                        ),
                                        (
                                            translate(locale, "gui.ai.f3.field.scope", &[]),
                                            if req.scope_label.contains(&count_s) {
                                                req.scope_label.clone()
                                            } else {
                                                format!("{} ({count_s})", req.scope_label)
                                            },
                                        ),
                                        (
                                            translate(locale, "gui.ai.f3.field.pace", &[]),
                                            translate(
                                                locale,
                                                "gui.ai.f3.pace_value",
                                                &[&interval_s, &batch_s],
                                            ),
                                        ),
                                        (
                                            translate(locale, "gui.ai.f3.field.purpose", &[]),
                                            req.purpose.clone(),
                                        ),
                                    ];
                                    for (idx, (k, v)) in rows.into_iter().enumerate() {
                                        if idx > 0 {
                                            ui.add_space(SPACE_1);
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing = vec2(SPACE_2, 2.0);
                                            ui.label(
                                                RichText::new(format!("{k}："))
                                                    .size(FONT_META)
                                                    .strong()
                                                    .color(p.text_secondary),
                                            );
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(v)
                                                        .size(FONT_META)
                                                        .color(p.text_primary),
                                                )
                                                .wrap(),
                                            );
                                        });
                                    }
                                    if show_tech {
                                        ui.add_space(SPACE_1);
                                        ui.label(
                                            RichText::new(format!(
                                                "request_id={} · profile={}",
                                                req.request_id, req.profile_id
                                            ))
                                            .monospace()
                                            .size(FONT_META)
                                            .color(p.text_muted),
                                        );
                                    }
                                });

                            ui.add_space(SPACE_2);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(translate(
                                        locale,
                                        "gui.ai.f3.boundary_note",
                                        &[],
                                    ))
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        });

                    ui.add_space(SPACE_2);
                    ui.separator();
                    ui.add_space(SPACE_2);

                    // 固定底栏：唯一高强调主按钮（一键授权） + 拒绝按钮，永不被滚动区裁切
                    if narrow {
                        let w = ui.available_width();
                        if ui
                            .add_sized(
                                [w, 42.0],
                                egui::Button::new(
                                    RichText::new(translate(locale, "gui.ai.f3.btn_approve", &[]))
                                        .strong()
                                        .size(FONT_BODY)
                                        .color(p.accent_fg),
                                )
                                .fill(p.accent)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(8.0)),
                            )
                            .clicked()
                        {
                            actions.push(UiAction::ApproveAiF3Auth);
                        }
                        ui.add_space(SPACE_2);
                        if ui
                            .add_sized(
                                [w, 40.0],
                                egui::Button::new(
                                    RichText::new(translate(locale, "gui.ai.f3.btn_reject", &[]))
                                        .size(FONT_BODY)
                                        .color(p.text_primary),
                                )
                                .fill(p.card_alt)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .rounding(Rounding::same(8.0)),
                            )
                            .clicked()
                        {
                            actions.push(UiAction::RejectAiF3Auth);
                        }
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                            if btn_filled(
                                ui,
                                p,
                                &translate(locale, "gui.ai.f3.btn_approve", &[]),
                                true,
                            )
                            .clicked()
                            {
                                actions.push(UiAction::ApproveAiF3Auth);
                            }
                            if btn_outlined(
                                ui,
                                p,
                                &translate(locale, "gui.ai.f3.btn_reject", &[]),
                                true,
                            )
                            .clicked()
                            {
                                actions.push(UiAction::RejectAiF3Auth);
                            }
                        });
                    }
                });
        });
}
