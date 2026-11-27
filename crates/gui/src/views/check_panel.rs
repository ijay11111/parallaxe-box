//! 检测页控制台与可滚动目标分组列表（问题 1、问题 4、补充 A、补充 C）。
//!
//! 设计原则：
//! - 检查范围选择（简单检查 / 完整检查 / 其余 profile）直接位于检测首页（桌面端紧邻主 CTA，移动端在主 CTA 正上方）；
//! - 检测节奏（请求间隔 3500–5000ms、每批数量、网络环境、限速自动放慢）与设置页共享同一份 `SchedulePrefs`；
//! - 目标分组列表置于独立 `ScrollArea::vertical()` 内并严控最大高度（桌面 <= 420px，移动 <= 40% 视口高），
//!   支持搜索与渐进披露（常用组 / 全部组），用户自定义分组排在最前；
//! - 严格遵循 4/8 间距、单屏唯一主 CTA、窄屏 >= 44px 触控高度、零水平溢出。

use std::collections::{BTreeMap, BTreeSet};

use egui::{vec2, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::targets::{Registry, Target, FAMILIES, HARD_LIMITS};

use crate::icons::{icon, IconKind};
use crate::layout::{ActiveTab, ViewportTier};
use crate::schedule_ui::{
    IspHint, NetworkProfileKind, SchedulePrefs, TimeOfDayPolicy, GUI_MAX_INTERVAL_MS,
};
use crate::theme::{
    btn_outlined, btn_tonal, card_frame, family_label, render_admonition, render_pill,
    subtle_well_frame, DensityMode, Palette, ThemeMode, FONT_BODY, FONT_META, FONT_TITLE,
    RADIUS_MD, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
};
use crate::views::{service_matrix::service_name, UiAction};

/// 检测页可直接在首页快速切换的主推 profile（Google 16 简单检测 / Google 全服务）。
pub const PRIMARY_PROFILES: [&str; 2] = ["simple", "google-all"];

/// 其余内置 profile（收纳于「更多范围」下拉菜单）：仅「全部」。
pub const SECONDARY_PROFILES: [&str; 1] = ["full"];

/// 全部支持的内置 profile ID 列表（仅三个：简单 16 / Google 全服务 / 全部）。
pub const ALL_PROFILE_IDS: [&str; 3] = ["simple", "google-all", "full"];

/// 动态计算某个 profile 实际包含的目标数量（与核心 `Registry::select` 完全同源）。
pub fn profile_target_count(registry: &Registry, profile_id: &str) -> usize {
    registry
        .select(Some(profile_id), &[], &[], false)
        .map(|sel| sel.ids.len())
        .unwrap_or(0)
}

/// 生成用户友好的 profile 中文/英文展示文案（带真实条目数，不裸露 `simple`/`google-all` 机器 ID）。
pub fn profile_display_label(registry: &Registry, profile_id: &str, locale: Locale) -> String {
    let count = profile_target_count(registry, profile_id).to_string();
    let key = match profile_id {
        "simple" => "gui.check.profile.simple",
        "google-all" => "gui.check.profile.google_all",
        "full" => "gui.check.profile.full",
        _ => return format!("{profile_id} ({count})"),
    };
    translate(locale, key, &[&count])
}

/// 将 profile ID 转换为对应的检测执行意图。
pub fn profile_run_action(profile_id: &str) -> UiAction {
    if profile_id == "full" {
        UiAction::RunFull
    } else {
        UiAction::RunProfile(profile_id.to_string())
    }
}

/// 共享的检测节奏控制组件（首页检测面板与设置页共用同一套控件与 `SchedulePrefs`）。
///
/// 返回 `true` 表示用户修改了任意节奏参数，调用方应立即持久化设置。
#[allow(clippy::too_many_arguments)]
pub fn render_pace_controls(
    ui: &mut Ui,
    prefs: &mut SchedulePrefs,
    p: Palette,
    locale: Locale,
    tier: ViewportTier,
    show_extended: bool,
    show_tech: bool,
    id_prefix: &str,
) -> bool {
    let mut changed = false;
    let eff = prefs.effective();

    // 1. 请求间隔滑块（始终可直接拖动，无需先解锁）+ 推荐档位快捷按钮
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
        ui.label(
            RichText::new(translate(locale, "gui.schedule.manual_interval", &[]))
                .size(FONT_BODY)
                .strong()
                .color(p.text_primary),
        );
        let mut interval_val = eff.min_interval_ms;
        if ui
            .add(
                egui::Slider::new(&mut interval_val, 0..=GUI_MAX_INTERVAL_MS)
                    .step_by(100.0)
                    .suffix(" ms"),
            )
            .changed()
        {
            prefs.set_interval_ms(interval_val);
            changed = true;
        }
        for preset in [3_500_u64, 4_000_u64, 5_000_u64] {
            let selected = eff.min_interval_ms == preset;
            if ui
                .selectable_label(selected, format!("{preset} ms"))
                .clicked()
            {
                prefs.set_interval_ms(preset);
                changed = true;
            }
        }
    });
    ui.add_space(SPACE_1);
    ui.add(
        egui::Label::new(
            RichText::new(translate(locale, "gui.schedule.safe_interval_hint", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        )
        .wrap(),
    );

    // 低于 3000ms 非模态风控提示（绝不静默跳过）
    if prefs.is_low_interval() {
        ui.add_space(SPACE_2);
        render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(translate(locale, "gui.schedule.low_interval_warning", &[]))
                        .size(FONT_META)
                        .color(p.warning_fg),
                )
                .wrap(),
            );
        });
    }

    ui.add_space(SPACE_3);

    // 2. 每批数量 + 限速自动放慢开关（QoS 自适应）
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
        ui.label(
            RichText::new(translate(locale, "gui.schedule.batch_size", &[]))
                .size(FONT_BODY)
                .color(p.text_secondary),
        );
        let mut batch_val = eff.batch_size;
        if ui
            .add(egui::Slider::new(
                &mut batch_val,
                1..=HARD_LIMITS.max_batch_size,
            ))
            .changed()
        {
            prefs.set_batch_size(batch_val);
            changed = true;
        }
        if tier != ViewportTier::Narrow {
            ui.add_space(SPACE_2);
        }
        let mut qos_adaptive = prefs.qos_adaptive_enabled();
        if ui
            .checkbox(
                &mut qos_adaptive,
                translate(locale, "gui.schedule.qos_adaptive_toggle", &[]),
            )
            .changed()
        {
            prefs.set_qos_adaptive(qos_adaptive);
            changed = true;
        }
    });

    ui.add_space(SPACE_3);

    // 3. 网络环境选择（人话标签，不裸露 MOBILE / UNKNOWN 等机器码）
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
        ui.label(
            RichText::new(translate(locale, "gui.schedule.profile_label", &[]))
                .size(FONT_BODY)
                .color(p.text_secondary),
        );
        for kind in NetworkProfileKind::ALL {
            let label = if show_tech {
                format!("{} ({})", kind.label(locale), kind.code())
            } else {
                kind.label(locale)
            };
            if ui
                .selectable_label(prefs.profile_kind == kind, label)
                .clicked()
            {
                prefs.select_profile_kind(kind);
                changed = true;
            }
        }
    });

    if show_extended {
        ui.add_space(SPACE_3);
        // 4. 运营商提示（选填）+ 时段策略
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
            ui.label(
                RichText::new(translate(locale, "gui.schedule.isp_label", &[]))
                    .size(FONT_META)
                    .color(p.text_secondary),
            );
            egui::ComboBox::from_id_salt((id_prefix, "isp_hint"))
                .selected_text(prefs.isp_hint.label(locale))
                .show_ui(ui, |ui| {
                    for isp in IspHint::ALL {
                        if ui
                            .selectable_value(&mut prefs.isp_hint, isp, isp.label(locale))
                            .changed()
                        {
                            prefs.custom_interval_ms = None;
                            if !prefs.lock_interval {
                                prefs.manual_interval_ms = prefs.profile_baseline().0;
                            }
                            changed = true;
                        }
                    }
                });
            ui.add_space(SPACE_2);
            ui.label(
                RichText::new(translate(locale, "gui.schedule.tod_label", &[]))
                    .size(FONT_META)
                    .color(p.text_secondary),
            );
            for policy in TimeOfDayPolicy::ALL {
                let lbl = if show_tech {
                    format!("{} ({})", policy.label(locale), policy.code())
                } else {
                    policy.label(locale)
                };
                if ui
                    .selectable_label(prefs.time_of_day == policy, lbl)
                    .clicked()
                {
                    prefs.time_of_day = policy;
                    prefs.custom_interval_ms = None;
                    if !prefs.lock_interval {
                        prefs.manual_interval_ms = prefs.profile_baseline().0;
                    }
                    changed = true;
                }
            }
        });
        ui.add_space(SPACE_3);

        // 5. 跨目标额外等待间隔
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
            ui.label(
                RichText::new(translate(locale, "gui.schedule.inter_target_interval", &[]))
                    .size(FONT_META)
                    .color(p.text_secondary),
            );
            if ui
                .add(
                    egui::Slider::new(
                        &mut prefs.inter_target_interval_ms,
                        0..=HARD_LIMITS.max_inter_target_interval_ms,
                    )
                    .step_by(100.0)
                    .suffix(" ms"),
                )
                .changed()
            {
                changed = true;
            }
        });
        ui.add_space(SPACE_1);
        ui.add(
            egui::Label::new(
                RichText::new(translate(locale, "gui.schedule.inter_target_hint", &[]))
                    .size(FONT_META)
                    .color(p.text_muted),
            )
            .wrap(),
        );
    }

    if show_tech {
        ui.add_space(SPACE_2);
        ui.label(
            RichText::new(format!(
                "source={} · profile={} · tod={} · cooldown={}ms",
                eff.source_code,
                prefs.profile_kind.code(),
                prefs.time_of_day.code(),
                eff.batch_cooldown_ms
            ))
            .monospace()
            .size(FONT_META)
            .color(p.text_muted),
        );
    }

    changed
}

/// 首页检测控制卡输出事件。
pub struct CheckCardOutcome {
    pub schedule_changed: bool,
    pub profile_changed: bool,
}

/// 在总览/检测首页渲染一体化检查控制卡：
/// - 问题 1：检查范围选择器（简单检查 / 完整检查 / 更多范围）
/// - 问题 4：检测节奏摘要与展开面板（与设置页双向实时同步）
/// - 补充 A：独立可滚动目标分组列表（限高防撑屏、搜索与渐进披露、自定义分组置顶）
/// - 唯一主 CTA「开始检测」（桌面位于控制卡顶栏右侧，移动端位于分组列表正下方、底栏上方）
#[allow(clippy::too_many_arguments)]
pub fn render_check_card(
    ui: &mut Ui,
    registry: &Registry,
    default_bundle: &mut crate::views::default_bundle_panel::DefaultBundleState,
    selected_profile: &mut String,
    schedule_prefs: &mut SchedulePrefs,
    pace_panel_open: &mut bool,
    group_search: &mut String,
    show_all_groups: &mut bool,
    checked_targets: &mut BTreeSet<String>,
    pending: bool,
    has_run: bool,
    density: DensityMode,
    theme: ThemeMode,
    locale: Locale,
    tier: ViewportTier,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) -> CheckCardOutcome {
    let p = Palette::for_theme(theme);
    let mut outcome = CheckCardOutcome {
        schedule_changed: false,
        profile_changed: false,
    };
    let is_narrow = tier == ViewportTier::Narrow;
    let _ = pending;

    card_frame(p)
        .inner_margin(Margin::symmetric(SPACE_4, SPACE_3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // ── 区域 1：检查范围选择（问题 1）+ 桌面端唯一主 CTA ─────────────────────
            let draw_scope_controls =
                |ui: &mut Ui, selected_profile: &mut String, profile_changed: &mut bool| {
                    ui.label(
                        RichText::new(translate(locale, "gui.check.scope_label", &[]))
                            .size(FONT_TITLE)
                            .strong()
                            .color(p.text_primary),
                    );
                    for pid in PRIMARY_PROFILES {
                        let lbl = profile_display_label(registry, pid, locale);
                        let active = *selected_profile == pid;
                        if scope_chip(ui, p, &lbl, active, is_narrow).clicked() {
                            *selected_profile = pid.to_string();
                            *profile_changed = true;
                        }
                    }
                    let secondary_selected = SECONDARY_PROFILES
                        .iter()
                        .find(|&&id| *selected_profile == id)
                        .map(|&id| profile_display_label(registry, id, locale))
                        .unwrap_or_else(|| translate(locale, "gui.check.more_profiles", &[]));
                    egui::ComboBox::from_id_salt("check_page_more_profiles")
                        .selected_text(secondary_selected)
                        .show_ui(ui, |ui| {
                            for pid in ALL_PROFILE_IDS {
                                let lbl = profile_display_label(registry, pid, locale);
                                if ui.selectable_label(*selected_profile == pid, lbl).clicked() {
                                    *selected_profile = pid.to_string();
                                    *profile_changed = true;
                                }
                            }
                        });
                };
            if !is_narrow && ui.available_width() >= 760.0 {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_2;
                    draw_scope_controls(ui, selected_profile, &mut outcome.profile_changed);
                    crate::views::default_bundle_panel::render_scope_entry(
                        ui,
                        registry,
                        default_bundle,
                        locale,
                        p,
                        actions,
                    );
                });
            } else if is_narrow {
                // 移动端：顶行「检测范围 + 更多范围」，第二行两个主范围并排等宽（44px 触控高），紧凑对称不折三行
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_2;
                    ui.label(
                        RichText::new(translate(locale, "gui.check.scope_label", &[]))
                            .size(FONT_TITLE)
                            .strong()
                            .color(p.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let secondary_selected = SECONDARY_PROFILES
                            .iter()
                            .find(|&&id| *selected_profile == id)
                            .map(|&id| profile_display_label(registry, id, locale))
                            .unwrap_or_else(|| translate(locale, "gui.check.more_profiles", &[]));
                        egui::ComboBox::from_id_salt("check_page_more_profiles")
                            .selected_text(secondary_selected)
                            .show_ui(ui, |ui| {
                                for pid in ALL_PROFILE_IDS {
                                    let lbl = profile_display_label(registry, pid, locale);
                                    if ui.selectable_label(*selected_profile == pid, lbl).clicked()
                                    {
                                        *selected_profile = pid.to_string();
                                        outcome.profile_changed = true;
                                    }
                                }
                            });
                    });
                });
                ui.add_space(SPACE_2);
                crate::theme::spaced_columns(ui, 2, SPACE_2, |col_ui, idx| {
                    let pid = PRIMARY_PROFILES[idx];
                    let lbl = profile_display_label(registry, pid, locale);
                    let active = *selected_profile == pid;
                    let (fill, stroke, fg) = if active {
                        (p.accent_soft, Stroke::new(1.0_f32, p.accent), p.accent)
                    } else {
                        (p.card_alt, Stroke::new(1.0_f32, p.border), p.text_primary)
                    };
                    if col_ui
                        .add_sized(
                            [col_ui.available_width(), 44.0],
                            egui::Button::new(
                                RichText::new(&lbl).size(FONT_BODY).strong().color(fg),
                            )
                            .fill(fill)
                            .stroke(stroke)
                            .rounding(Rounding::same(RADIUS_MD))
                            .truncate(),
                        )
                        .on_hover_text(&lbl)
                        .clicked()
                    {
                        *selected_profile = pid.to_string();
                        outcome.profile_changed = true;
                    }
                });
                ui.add_space(SPACE_1);
                ui.horizontal_wrapped(|ui| {
                    crate::views::default_bundle_panel::render_scope_entry(
                        ui,
                        registry,
                        default_bundle,
                        locale,
                        p,
                        actions,
                    );
                });
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                    draw_scope_controls(ui, selected_profile, &mut outcome.profile_changed);
                    crate::views::default_bundle_panel::render_scope_entry(
                        ui,
                        registry,
                        default_bundle,
                        locale,
                        p,
                        actions,
                    );
                });
            }

            ui.add_space(SPACE_2);

            // ── 区域 2：检测节奏摘要条 + 展开调节面板（问题 4） ───────────────────────
            let eff = schedule_prefs.effective();
            let interval_str = eff.min_interval_ms.to_string();
            let batch_str = eff.batch_size.to_string();
            let net_str = schedule_prefs.profile_kind.label(locale);
            let pace_summary = translate(
                locale,
                "gui.check.pace_summary",
                &[&interval_str, &batch_str, &net_str],
            );

            let groups_always_open = !has_run && density == DensityMode::Normal;
            let groups_toggle_id = ui.make_persistent_id("check_groups_inline_open");
            let mut groups_open = groups_always_open
                || !group_search.is_empty()
                || !checked_targets.is_empty()
                || ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(groups_toggle_id).unwrap_or(false));
            let scope_cnt = profile_target_count(registry, selected_profile).to_string();

            subtle_well_frame(p)
                .inner_margin(Margin::symmetric(SPACE_3, SPACE_1))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                        icon(ui, IconKind::Timeline, 14.0, p.accent);
                        ui.label(
                            RichText::new(translate(locale, "gui.check.pace_label", &[]))
                                .size(FONT_META)
                                .strong()
                                .color(p.text_primary),
                        );
                        if schedule_prefs.is_low_interval() {
                            ui.label(
                                RichText::new("< 3000 ms")
                                    .size(FONT_META)
                                    .strong()
                                    .color(p.warning_fg),
                            );
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(&pace_summary)
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                        let toggle_key = if *pace_panel_open {
                            "gui.check.pace_close"
                        } else {
                            "gui.check.pace_open"
                        };
                        if btn_outlined(ui, p, &translate(locale, toggle_key, &[]), true).clicked()
                        {
                            *pace_panel_open = !*pace_panel_open;
                        }
                        if !groups_always_open {
                            let grp_btn =
                                translate(locale, "gui.check.groups.collapse_label", &[&scope_cnt]);
                            if btn_outlined(ui, p, &grp_btn, true).clicked() {
                                groups_open = !groups_open;
                                ui.ctx()
                                    .data_mut(|d| d.insert_temp(groups_toggle_id, groups_open));
                            }
                        }
                    });

                    if !*pace_panel_open && schedule_prefs.is_low_interval() {
                        ui.add_space(SPACE_2);
                        ui.add(
                            egui::Label::new(
                                RichText::new(translate(
                                    locale,
                                    "gui.schedule.low_interval_warning",
                                    &[],
                                ))
                                .size(FONT_META)
                                .color(p.warning_fg),
                            )
                            .wrap(),
                        );
                    }

                    if *pace_panel_open {
                        ui.add_space(SPACE_3);
                        ui.separator();
                        ui.add_space(SPACE_3);
                        if render_pace_controls(
                            ui,
                            schedule_prefs,
                            p,
                            locale,
                            tier,
                            false,
                            show_tech,
                            "check_page_pace",
                        ) {
                            outcome.schedule_changed = true;
                        }
                    }
                });

            // ── 区域 3：独立可滚动目标分组列表（补充 A + 问题 2 自定义分组展示）────────
            if groups_open {
                ui.add_space(SPACE_2);
                render_scrollable_target_groups(
                    ui,
                    registry,
                    selected_profile,
                    group_search,
                    show_all_groups,
                    checked_targets,
                    has_run,
                    p,
                    locale,
                    tier,
                    show_tech,
                    actions,
                );
            }

            // ── 区域 4：移动端主 CTA 由 `app.rs` 的常驻悬浮按钮统一提供（唯一主按钮）────────
        });

    outcome
}

fn scope_chip(
    ui: &mut Ui,
    p: Palette,
    label: &str,
    selected: bool,
    touch_friendly: bool,
) -> egui::Response {
    let min_h = if touch_friendly { 44.0 } else { 32.0 };
    let (fill, stroke, fg) = if selected {
        (p.accent_soft, Stroke::new(1.0_f32, p.accent), p.accent)
    } else {
        (p.card_alt, Stroke::new(1.0_f32, p.border), p.text_primary)
    };
    ui.add(
        egui::Button::new(RichText::new(label).size(FONT_BODY).strong().color(fg))
            .fill(fill)
            .stroke(stroke)
            .rounding(Rounding::same(RADIUS_MD))
            .min_size(vec2(0.0, min_h)),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_scrollable_target_groups(
    ui: &mut Ui,
    registry: &Registry,
    selected_profile: &str,
    group_search: &mut String,
    show_all_groups: &mut bool,
    checked_targets: &mut BTreeSet<String>,
    has_run: bool,
    p: Palette,
    locale: Locale,
    tier: ViewportTier,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let is_narrow = tier == ViewportTier::Narrow;
    let screen_h = ui.ctx().screen_rect().height();
    // 严格遵守补充 A 限高约束：桌面 <= 420px，移动 <= 40% 视口高度，确保移动首屏可见下方主 CTA 与底栏
    let scroll_max_h = if is_narrow {
        (screen_h * 0.19).clamp(90.0, 144.0)
    } else if has_run {
        200.0
    } else {
        212.0
    };

    // 标题栏 + 渐进披露切换 + 快速导入入口
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
        icon(ui, IconKind::Catalog, 14.0, p.accent);
        ui.label(
            RichText::new(translate(locale, "gui.check.groups.title", &[]))
                .size(FONT_BODY)
                .strong()
                .color(p.text_primary),
        );
        let mode_key = if *show_all_groups {
            "gui.check.groups.show_common"
        } else {
            "gui.check.groups.show_all"
        };
        if btn_outlined(ui, p, &translate(locale, mode_key, &[]), true).clicked() {
            *show_all_groups = !*show_all_groups;
        }
        if btn_tonal(
            ui,
            p,
            &translate(locale, "gui.check.groups.import_btn", &[]),
            true,
        )
        .clicked()
        {
            actions.push(UiAction::SwitchTab(ActiveTab::Import));
        }
    });

    ui.add_space(SPACE_2);

    // 搜索筛选框
    let search_w = ui
        .available_width()
        .min(if is_narrow { 480.0 } else { 360.0 });
    ui.add_sized(
        [search_w, if is_narrow { 40.0 } else { 32.0 }],
        egui::TextEdit::singleline(group_search).hint_text(translate(
            locale,
            "gui.check.groups.search_hint",
            &[],
        )),
    );

    ui.add_space(SPACE_2);

    // 计算当前选定 profile 命中的目标集合
    let active_scope_ids: BTreeSet<String> = if selected_profile == "full" {
        registry
            .entries()
            .iter()
            .map(|t| t.id().to_string())
            .collect()
    } else {
        registry
            .select(Some(selected_profile), &[], &[], false)
            .map(|sel| sel.ids.into_iter().collect())
            .unwrap_or_default()
    };

    let query = group_search.trim().to_ascii_lowercase();
    let matches_query = |t: &Target, group_label: &str| -> bool {
        if query.is_empty() {
            return true;
        }
        let disp = service_name(t.id()).to_ascii_lowercase();
        let id = t.id().to_ascii_lowercase();
        let host = t.host().to_ascii_lowercase();
        let gl = group_label.to_ascii_lowercase();
        disp.contains(&query) || id.contains(&query) || host.contains(&query) || gl.contains(&query)
    };

    // 1) 用户自定义分组（与 Google / 对照内置分组完全隔离，按用户建组名展示）
    let mut user_grouped: BTreeMap<String, Vec<&Target>> = BTreeMap::new();
    for entry in registry.entries() {
        if let Some(group_name) = registry.user_group_of(entry.id()) {
            if matches_query(entry, group_name) {
                user_grouped
                    .entry(group_name.to_string())
                    .or_default()
                    .push(entry);
            }
        }
    }

    // 2) 内置目标分组（渐进披露：默认常用组按「Google 服务 / 独立对照服务」，搜索或切「显示全部分组」按全部家族展开）
    struct BuiltinGroup<'a> {
        id: String,
        title: String,
        items: Vec<&'a Target>,
        default_open: bool,
    }
    let mut builtin_groups: Vec<BuiltinGroup<'_>> = Vec::new();
    if *show_all_groups || !query.is_empty() {
        let mut by_family: BTreeMap<&str, Vec<&Target>> = BTreeMap::new();
        for entry in registry.entries() {
            if registry.user_group_of(entry.id()).is_some() {
                continue;
            }
            let fam_title = localized_family_label(&entry.raw.family, locale);
            if matches_query(entry, fam_title) {
                by_family
                    .entry(entry.raw.family.as_str())
                    .or_default()
                    .push(entry);
            }
        }
        for fam in FAMILIES {
            if let Some(items) = by_family.remove(fam) {
                builtin_groups.push(BuiltinGroup {
                    id: format!("fam_{fam}"),
                    title: localized_family_label(fam, locale).to_string(),
                    items,
                    default_open: !query.is_empty(),
                });
            }
        }
    } else {
        // 常用组视图：直接按当前选定检查范围聚合「Google 服务」与「独立对照服务」
        let mut google_items = Vec::new();
        let mut control_items = Vec::new();
        for entry in registry.entries() {
            if registry.user_group_of(entry.id()).is_some() {
                continue;
            }
            if !active_scope_ids.contains(entry.id()) {
                continue;
            }
            if entry.id().starts_with("google.") {
                google_items.push(entry);
            } else {
                control_items.push(entry);
            }
        }
        if !google_items.is_empty() {
            builtin_groups.push(BuiltinGroup {
                id: "common_google".to_string(),
                title: if locale == Locale::EnUs {
                    "Google Services".to_string()
                } else {
                    "Google 服务".to_string()
                },
                items: google_items,
                default_open: !is_narrow,
            });
        }
        if !control_items.is_empty() {
            builtin_groups.push(BuiltinGroup {
                id: "common_control".to_string(),
                title: if locale == Locale::EnUs {
                    "Control Baselines".to_string()
                } else {
                    "独立对照服务".to_string()
                },
                items: control_items,
                default_open: false,
            });
        }
    }

    // 独立限高纵向滚动区（min-width/min-height=0，绝不横向撑爆，绝不遮盖下方按钮）
    subtle_well_frame(p)
        .inner_margin(Margin::symmetric(SPACE_2, SPACE_2))
        .show(ui, |ui| {
            let inner_w = ui.available_width().max(120.0);
            ui.set_min_width(0.0);
            ui.set_max_width(inner_w);
            ScrollArea::vertical()
                .id_salt(("check_target_groups_scroll", *show_all_groups))
                .max_height(scroll_max_h)
                .min_scrolled_height(scroll_max_h)
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| {
                    ui.set_min_width(0.0);
                    ui.set_max_width((inner_w - SPACE_3).max(100.0));

                    // 先渲染用户自定义分组（头部横向展示「自定义分组」徽章+组名，展开内容全宽排列）
                    let custom_badge = translate(locale, "gui.check.groups.custom_badge", &[]);
                    let open_user_by_default = user_grouped.len() == 1 && !is_narrow;
                    for (group_name, items) in &user_grouped {
                        let header = format!("{group_name} · {}", items.len());
                        let state_id = ui.make_persistent_id(("check_user_group", group_name));
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            state_id,
                            open_user_by_default || !query.is_empty(),
                        )
                        .show_header(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                                render_pill(ui, &custom_badge, p.accent, p.accent_soft, p.accent);
                                ui.label(
                                    RichText::new(&header)
                                        .size(FONT_BODY)
                                        .strong()
                                        .color(p.accent),
                                );
                            });
                        })
                        .body(|ui| {
                            for target in items {
                                render_target_row(
                                    ui,
                                    target,
                                    checked_targets,
                                    active_scope_ids.contains(target.id()),
                                    p,
                                    is_narrow,
                                    show_tech,
                                    actions,
                                );
                            }
                        });
                    }

                    // 再渲染内置分组
                    for grp in &builtin_groups {
                        let header = format!("{} · {}", grp.title, grp.items.len());
                        egui::CollapsingHeader::new(
                            RichText::new(header)
                                .size(FONT_BODY)
                                .strong()
                                .color(p.text_primary),
                        )
                        .id_salt(("check_builtin_group", &grp.id, selected_profile))
                        .default_open(grp.default_open)
                        .show(ui, |ui| {
                            for target in &grp.items {
                                render_target_row(
                                    ui,
                                    target,
                                    checked_targets,
                                    active_scope_ids.contains(target.id()),
                                    p,
                                    is_narrow,
                                    show_tech,
                                    actions,
                                );
                            }
                        });
                    }

                    if user_grouped.is_empty() && builtin_groups.is_empty() {
                        ui.label(
                            RichText::new(translate(locale, "gui.matrix.empty_filtered", &[]))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                    }
                });
        });

    // 若用户勾选了特定目标，显示就近「检测已选」操作条
    if !checked_targets.is_empty() {
        ui.add_space(SPACE_2);
        let cnt = checked_targets.len().to_string();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
            if btn_tonal(
                ui,
                p,
                &translate(locale, "gui.check.groups.selected_run", &[&cnt]),
                true,
            )
            .clicked()
            {
                actions.push(UiAction::RunCustomTargets(Vec::new()));
            }
            if btn_outlined(
                ui,
                p,
                &translate(locale, "gui.check.groups.clear_sel", &[]),
                true,
            )
            .clicked()
            {
                checked_targets.clear();
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn render_target_row(
    ui: &mut Ui,
    target: &Target,
    checked_targets: &mut BTreeSet<String>,
    in_active_scope: bool,
    p: Palette,
    is_narrow: bool,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let mut is_checked = checked_targets.contains(target.id());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_2;
        if ui.checkbox(&mut is_checked, "").changed() {
            if is_checked {
                checked_targets.insert(target.id().to_string());
            } else {
                checked_targets.remove(target.id());
            }
        }
        let title = service_name(target.id());
        let label_text = if show_tech {
            format!("{title} ({})", target.id())
        } else {
            title.to_string()
        };
        let btn = ui.add(
            egui::Button::new(RichText::new(&label_text).size(FONT_BODY).color(
                if in_active_scope {
                    p.text_primary
                } else {
                    p.text_secondary
                },
            ))
            .frame(false)
            .truncate(),
        );
        if btn.on_hover_text(target.host()).clicked() {
            actions.push(UiAction::SelectTarget(target.id().to_string()));
        }
        if !is_narrow {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(target.host())
                            .size(FONT_META)
                            .color(p.text_muted),
                    )
                    .truncate(),
                );
            });
        }
    });
}

fn localized_family_label(family: &str, locale: Locale) -> &'static str {
    if locale == Locale::EnUs {
        match family {
            "identity" => "Identity & Sign-in",
            "search" => "Search",
            "content" => "Content & News",
            "workspace" => "Workspace & Docs",
            "media" => "Media & Video",
            "store" => "Store & Apps",
            "ai" => "AI Services",
            "education" => "Education",
            "cloud" => "Cloud Platform",
            "developer" => "Developer",
            "static" => "Static Assets",
            "api" => "General API",
            "cdn" => "CDN Controls",
            "domestic" => "Domestic Controls",
            "social" => "Social & Dev Controls",
            "user" => "User Imported",
            _ => "Other",
        }
    } else {
        family_label(family)
    }
}
