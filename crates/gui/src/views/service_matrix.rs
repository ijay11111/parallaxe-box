//! 服务结果的显示投影：国家、判定、传输尝试来自三个不同字段，绝不互相推导。
//!
//! 高密度数据表设计（参考 the_grid + gpui-kit DataTable）：
//! - 固定最小列宽 + 横向滚动防折行，确保状态徽章与 `HTTP 200`/`NOT_OBSERVED` 不被挤碎；
//! - 等宽字体对齐国家码与双栈状态；
//! - 支持「舒适 / 紧凑」表格密度切换（仅调行高与内边距，绝不隐藏冲突/UNKNOWN）；
//! - 阈值配色克制：仅明确异常与冲突行以左侧 3px 语义条微弱突出。

use crate::{
    layout::{ActiveTab, ViewportTier},
    state::{AppState, RunView},
    theme::{
        btn_outlined, btn_tonal, card_frame, render_admonition, render_pill, render_state_badge,
        state_badge, subtle_well_frame, Palette, TableDensity, ThemeMode, FONT_BODY, FONT_META,
        FONT_TITLE, RADIUS_MD, RADIUS_SM, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
    },
    views::{eligibility_badge_button, render_eligibility_notes_panel, UiAction},
};
use egui::{pos2, vec2, Align, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::contracts::{reason, AttemptFamily, ResultRecord, State, ORIGIN_USER_IMPORTED};
use parallaxe_core::i18n::{translate, Locale};

pub fn service_name(id: &str) -> &str {
    match id {
        "google.gemini" => "Gemini",
        "google.books" => "Google Books",
        "google.one" => "Google One",
        "google.play" => "Google Play",
        "google.gmail" | "google.mail" => "Gmail",
        "google.drive" => "Google Drive",
        "google.home" => "Google 搜索",
        "google.news" => "Google 新闻",
        "google.youtube" => "YouTube",
        "google.maps" => "Google 地图",
        "google.photos" => "Google 相册",
        other => other.strip_prefix("google.").unwrap_or(other),
    }
}

// 仅展示顺序。CN 是值得复核的观测值，不等于该服务受限。
pub(super) fn priority(r: &ResultRecord) -> u8 {
    let order = match r.state {
        State::Blocked | State::Restricted | State::LocalPathError => 0,
        State::Conflict => 1,
        State::SignalStale | State::Unknown | State::Unsupported => 3,
        State::ReachableOnly => 4,
        State::Ok => 5,
    };
    if r.service_observed.observed_country.as_deref() == Some("CN") || !r.conflicts.is_empty() {
        order.min(2)
    } else {
        order
    }
}

pub fn summary(r: &ResultRecord, locale: Locale) -> String {
    let key = match r.reason.as_deref() {
        Some(reason::REDIRECT_TO_LOGIN) => "gui.matrix.summary.redirect_login",
        Some(reason::CAPTCHA_CHALLENGE) => "gui.matrix.summary.captcha",
        Some(reason::RATE_LIMITED) => "gui.matrix.summary.rate_limited",
        Some(reason::FIELD_MISSING | reason::NO_REGION_SIGNAL) => "gui.matrix.summary.no_signal",
        Some(reason::TRUNCATED) => "gui.matrix.summary.truncated",
        Some(reason::DENIED_OR_AUTH) => "gui.matrix.summary.denied",
        Some("REGION_RESTRICTED_FIXTURE") => "gui.matrix.summary.fixture_restricted",
        Some("POLICY_BLOCKED_FIXTURE") => "gui.matrix.summary.fixture_blocked",
        _ => match r.state {
            State::Ok => "gui.matrix.summary.ok",
            State::Restricted => "gui.matrix.summary.restricted",
            State::Blocked => "gui.matrix.summary.blocked",
            State::LocalPathError => "gui.matrix.summary.local_path_error",
            State::Conflict => "gui.matrix.summary.conflict",
            State::ReachableOnly => "gui.matrix.summary.reachable_only",
            State::Unknown => "gui.matrix.summary.unknown",
            State::Unsupported => "gui.matrix.summary.unsupported",
            State::SignalStale => "gui.matrix.summary.signal_stale",
        },
    };
    let why = translate(locale, key, &[]);
    let evidence = r.service_observed.evidence_ids.len();
    if evidence > 0 {
        let ev_str = evidence.to_string();
        translate(locale, "gui.matrix.summary.with_evidence", &[&why, &ev_str])
    } else {
        why
    }
}

fn path_note(r: &ResultRecord) -> Option<&'static str> {
    match r.transport.path_diagnostic.as_deref() {
        Some("IPv6_SLOW") => Some("IPv6 路径偏慢"),
        Some("IPv6_BROKEN") => Some("IPv6 路径异常"),
        Some("IPV6_ONLY_EGRESS") => Some("仅观察到 IPv6 出口"),
        Some("PROXY_REMOTE_UNVERIFIED") => Some("代理远端未验证"),
        _ => None,
    }
}

fn family(r: &ResultRecord, f: AttemptFamily, locale: Locale) -> String {
    match r.transport.attempts.iter().rev().find(|a| a.family == f) {
        None => translate(locale, "gui.matrix.no_record", &[]),
        Some(a) => match a.http_status {
            Some(n) => format!("HTTP {n}"),
            None => translate(locale, "gui.matrix.attempted", &[]),
        },
    }
}

/// 顶部优先异常提示条（采用现代 Admonition 左侧色条 + Tonal 按钮，确保全屏仅有一个 Filled 主按钮）。
pub fn finding(
    ui: &mut Ui,
    run: &RunView,
    theme: ThemeMode,
    locale: Locale,
    actions: &mut Vec<UiAction>,
) {
    if let Some(r) = run
        .ordered_results()
        .into_iter()
        .min_by_key(|r| priority(r))
        .filter(|r| priority(r) <= 2)
    {
        let p = Palette::for_theme(theme);
        let b = state_badge(r.state, theme);
        let unknown_reg = translate(locale, "gui.matrix.region_unknown", &[]);
        let (bg, stripe) = match r.state {
            State::Blocked | State::LocalPathError => (p.danger_bg, p.danger_fg),
            State::Restricted | State::Conflict => (p.warning_bg, p.warning_fg),
            // 其余状态理论上不会进入本提示条（priority > 2），仍显式列出以保持 9-State 穷举。
            State::Ok
            | State::ReachableOnly
            | State::Unknown
            | State::Unsupported
            | State::SignalStale => (p.info_bg, p.info_fg),
        };
        render_admonition(ui, bg, stripe, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                render_pill(
                    ui,
                    &translate(locale, "gui.matrix.priority_badge", &[]),
                    stripe,
                    p.card,
                    stripe,
                );
                ui.label(
                    RichText::new(format!(
                        "{}  ·  {}",
                        service_name(&r.target),
                        r.service_observed
                            .observed_country
                            .as_deref()
                            .unwrap_or(&unknown_reg),
                    ))
                    .size(FONT_TITLE)
                    .strong()
                    .color(p.text_primary),
                );
                render_state_badge(ui, r.state, theme, locale, true);
                ui.label(
                    RichText::new(format!("({})", b.code))
                        .monospace()
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if btn_tonal(ui, p, &translate(locale, "gui.matrix.open_plan", &[]), true)
                        .clicked()
                    {
                        actions.push(UiAction::OpenPlan(r.target.clone()));
                    }
                });
            });
        });
        ui.add_space(SPACE_4);
    }
}

fn operations(
    ui: &mut Ui,
    r: &ResultRecord,
    p: Palette,
    locale: Locale,
    actions: &mut Vec<UiAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_2;
        if priority(r) < 5
            && btn_tonal(ui, p, &translate(locale, "gui.matrix.open_plan", &[]), true).clicked()
        {
            actions.push(UiAction::OpenPlan(r.target.clone()));
        }
        if ui
            .link(translate(locale, "gui.matrix.evidence_detail", &[]))
            .clicked()
        {
            actions.push(UiAction::SelectTarget(r.target.clone()));
        }
    });
}

fn cell(ui: &mut Ui, w: f32, h: f32, draw: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(w, h), Layout::top_down(Align::Min), |ui| {
        ui.set_width(w);
        draw(ui);
    });
}

pub fn render(
    ui: &mut Ui,
    state: &mut AppState,
    theme: ThemeMode,
    tier: ViewportTier,
    table_density: &mut TableDensity,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);
    let locale = state.current_locale();
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());

        // 卡片标题栏 + 表格行密度预设切换（参考 the_grid 密度参数化）
        let total_count = state
            .active_run()
            .map(|r| r.result_order.len())
            .unwrap_or(0);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(translate(locale, "gui.matrix.title", &[]))
                    .size(FONT_TITLE)
                    .strong()
                    .color(p.text_primary),
            );
            if total_count > 0 {
                let cnt_str = total_count.to_string();
                render_pill(
                    ui,
                    &translate(locale, "gui.matrix.total_pill", &[&cnt_str]),
                    p.text_secondary,
                    p.card_alt,
                    p.border,
                );
            }
            if tier != ViewportTier::Narrow {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::Frame::none()
                        .fill(p.canvas)
                        .stroke(Stroke::new(1.0_f32, p.border_subtle))
                        .rounding(Rounding::same(RADIUS_SM))
                        .inner_margin(Margin::same(2.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for preset in [TableDensity::Comfortable, TableDensity::Compact] {
                                    let active = *table_density == preset;
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(preset.label_localized(locale))
                                                    .size(FONT_META)
                                                    .color(if active {
                                                        p.accent
                                                    } else {
                                                        p.text_secondary
                                                    }),
                                            )
                                            .fill(if active {
                                                p.accent_soft
                                            } else {
                                                egui::Color32::TRANSPARENT
                                            })
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(4.0))
                                            .min_size(vec2(46.0, 22.0)),
                                        )
                                        .on_hover_text(translate(
                                            locale,
                                            "gui.matrix.density_tip",
                                            &[],
                                        ))
                                        .clicked()
                                    {
                                        *table_density = preset;
                                    }
                                }
                            });
                        });
                    ui.label(
                        RichText::new(translate(locale, "gui.matrix.density_label", &[]))
                            .size(FONT_META)
                            .color(p.text_muted),
                    );
                });
            }
        });

        if total_count > 0 {
            ui.add_space(SPACE_2);
            let filter_header = if state.has_active_filters() {
                translate(locale, "gui.matrix.filter_active", &[])
            } else {
                translate(locale, "gui.matrix.filter", &[])
            };
            ui.collapsing(filter_header, |ui| {
                ui.add_space(SPACE_1);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_2;
                    ui.add(
                        egui::TextEdit::singleline(&mut state.search_query)
                            .desired_width(180.0)
                            .hint_text(translate(locale, "gui.matrix.search_hint", &[])),
                    );
                    let all_states_lbl = translate(locale, "gui.matrix.all_states", &[]);
                    let selected_text = state
                        .filter_state
                        .map(|s| state_badge(s, theme).label(locale))
                        .unwrap_or_else(|| all_states_lbl.clone());
                    egui::ComboBox::from_id_salt("state_filter")
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.filter_state, None, &all_states_lbl);
                            for s in State::ALL {
                                ui.selectable_value(
                                    &mut state.filter_state,
                                    Some(s),
                                    state_badge(s, theme).label(locale),
                                );
                            }
                        });
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.matrix.clear_filter", &[]),
                        state.has_active_filters(),
                    )
                    .clicked()
                    {
                        actions.push(UiAction::ClearFilters);
                    }
                });
            });
        }

        ui.add_space(SPACE_3);
        let Some(run) = state.active_run() else {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.matrix.empty_idle", &[]))
                        .size(FONT_BODY)
                        .color(p.text_secondary),
                );
            });
            return;
        };

        let mut rows = state.filtered_results(run);
        rows.sort_by_key(|r| priority(r));
        if rows.is_empty() {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if state.has_active_filters() {
                    ui.label(
                        RichText::new(translate(locale, "gui.matrix.empty_filtered", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                } else {
                    let msg_key = if run.is_zero_admission_online() {
                        "gui.matrix.empty_zero_admission"
                    } else if run.running {
                        "gui.matrix.empty_running"
                    } else {
                        "gui.matrix.empty_none"
                    };
                    ui.label(
                        RichText::new(translate(locale, msg_key, &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        if btn_outlined(
                            ui,
                            p,
                            &translate(locale, "gui.matrix.btn.coverage", &[]),
                            true,
                        )
                        .clicked()
                        {
                            actions.push(UiAction::SwitchTab(ActiveTab::Coverage));
                        }
                        if ui
                            .link(translate(locale, "gui.matrix.btn.demo_books", &[]))
                            .clicked()
                        {
                            actions.push(UiAction::RunScenario(
                                crate::commands::OfflineScenario::BooksAndSharedCdnGuard,
                            ));
                        }
                    });
                }
            });
            return;
        }

        let mut toggled_elig: Option<String> = None;
        let unknown_reg = translate(locale, "gui.matrix.region_unknown", &[]);
        let user_imp_lbl = translate(locale, "gui.matrix.user_imported", &[]);

        let stacked = tier == ViewportTier::Narrow || ui.available_width() < 760.0;
        if stacked {
            for (idx, r) in rows.into_iter().enumerate() {
                if idx > 0 {
                    ui.add_space(SPACE_3);
                }
                ui.push_id(&r.target, |ui| {
                    let country = r
                        .service_observed
                        .observed_country
                        .as_deref()
                        .unwrap_or(&unknown_reg);
                    let is_alert = matches!(
                        r.state,
                        State::Restricted
                            | State::Blocked
                            | State::LocalPathError
                            | State::Conflict
                    );
                    let row_bg = if is_alert {
                        p.warning_bg.gamma_multiply(0.45)
                    } else {
                        p.card_alt
                    };
                    let elig_expanded = state.expanded_eligibility.contains(&r.target);
                    egui::Frame::none()
                        .fill(row_bg)
                        .stroke(Stroke::new(1.0_f32, p.border_subtle))
                        .rounding(Rounding::same(RADIUS_MD))
                        .inner_margin(Margin::same(SPACE_3))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(service_name(&r.target))
                                        .size(FONT_TITLE)
                                        .strong(),
                                );
                                if r.origin.as_deref() == Some(ORIGIN_USER_IMPORTED) {
                                    render_pill(
                                        ui,
                                        &user_imp_lbl,
                                        p.warning_fg,
                                        p.warning_bg,
                                        p.warning_fg,
                                    );
                                }
                                ui.label(
                                    RichText::new(country)
                                        .monospace()
                                        .strong()
                                        .size(FONT_BODY)
                                        .color(if country == "CN" {
                                            p.warning_fg
                                        } else {
                                            p.text_primary
                                        }),
                                );
                                render_state_badge(ui, r.state, theme, locale, true);
                                if !r.eligibility_notes.is_empty()
                                    && eligibility_badge_button(
                                        ui,
                                        p,
                                        locale,
                                        r.eligibility_notes.len(),
                                        elig_expanded,
                                    )
                                    .clicked()
                                {
                                    toggled_elig = Some(r.target.clone());
                                }
                            });
                            ui.add_space(SPACE_1);
                            ui.label(
                                RichText::new(summary(r, locale))
                                    .size(FONT_BODY)
                                    .color(p.text_secondary),
                            );
                            ui.add_space(SPACE_1);
                            let v4 = family(r, AttemptFamily::V4, locale);
                            let v6 = family(r, AttemptFamily::V6, locale);
                            ui.label(
                                RichText::new(format!("IPv4  {v4}    ·    IPv6  {v6}"))
                                    .monospace()
                                    .size(FONT_META)
                                    .color(p.text_muted),
                            );
                            if let Some(note) = path_note(r) {
                                ui.colored_label(p.warning_fg, note);
                            }
                            if elig_expanded && !r.eligibility_notes.is_empty() {
                                ui.add_space(SPACE_2);
                                render_eligibility_notes_panel(ui, &r.eligibility_notes, locale, p);
                            }
                            ui.add_space(SPACE_2);
                            operations(ui, r, p, locale, actions);
                        });
                });
            }
            if let Some(id) = toggled_elig {
                state.toggle_eligibility_expanded(&id);
            }
            return;
        }

        // 桌面端：固定最小列宽 + 横向滚动数据表（参考 gpui-kit DataTable + the_grid）
        let col_gap = SPACE_2;
        let col_service = 156.0_f32;
        let col_region = 76.0_f32;
        let col_state = 112.0_f32;
        let col_stack = 148.0_f32;
        let col_ops = 180.0_f32;
        let fixed_sum = col_service + col_region + col_state + col_stack + col_ops + col_gap * 5.0;
        let table_outer_w = ui.available_width();
        let avail_inner = (table_outer_w - SPACE_4 - 6.0).max(0.0);
        let col_summary = (avail_inner - fixed_sum).max(160.0);
        let widths = [
            col_service,
            col_region,
            col_state,
            col_stack,
            col_summary,
            col_ops,
        ];
        let total_w: f32 = widths.iter().sum::<f32>() + col_gap * 5.0 + SPACE_4;
        let row_h = table_density.row_height();
        let v_pad = table_density.cell_v_pad();

        let headers = [
            translate(locale, "gui.matrix.col.service", &[]),
            translate(locale, "gui.matrix.col.region", &[]),
            translate(locale, "gui.matrix.col.verdict", &[]),
            translate(locale, "gui.matrix.col.stack", &[]),
            translate(locale, "gui.matrix.col.summary", &[]),
            translate(locale, "gui.matrix.col.actions", &[]),
        ];

        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .show(ui, |ui| {
                ui.set_max_width(table_outer_w);
                ScrollArea::horizontal()
                    .id_salt("service_matrix_hscroll")
                    .max_width(table_outer_w)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_width(total_w.min(table_outer_w - 2.0).max(total_w));
                        // 固定表头
                        egui::Frame::none()
                            .fill(p.table_head_bg)
                            .inner_margin(Margin::symmetric(SPACE_2, SPACE_2))
                            .show(ui, |ui| {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = col_gap;
                                    for (w, header) in widths.into_iter().zip(&headers) {
                                        cell(ui, w, 18.0, |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(header)
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

                        // 数据行
                        for (idx, r) in rows.into_iter().enumerate() {
                            ui.push_id(&r.target, |ui| {
                                let country = r
                                    .service_observed
                                    .observed_country
                                    .as_deref()
                                    .unwrap_or(&unknown_reg);
                                let stripe_color = match r.state {
                                    State::Blocked | State::LocalPathError => Some(p.danger_fg),
                                    State::Restricted | State::Conflict => Some(p.warning_fg),
                                    // 正常/仅可达/未知/不支持/信号失效：不画左侧阈值条。
                                    State::Ok
                                    | State::ReachableOnly
                                    | State::Unknown
                                    | State::Unsupported
                                    | State::SignalStale => None,
                                };
                                let row_bg = if idx % 2 == 1 {
                                    p.table_even_bg
                                } else {
                                    p.card
                                };
                                let elig_expanded = state.expanded_eligibility.contains(&r.target);
                                let row_resp = egui::Frame::none()
                                    .fill(row_bg)
                                    .inner_margin(Margin::symmetric(SPACE_2, v_pad))
                                    .show(ui, |ui| {
                                        ui.horizontal_top(|ui| {
                                            ui.spacing_mut().item_spacing.x = col_gap;
                                            let content_h = (row_h - v_pad * 2.0).max(28.0);
                                            cell(ui, widths[0], content_h, |ui| {
                                                ui.label(
                                                    RichText::new(service_name(&r.target))
                                                        .size(FONT_BODY)
                                                        .strong()
                                                        .color(p.text_primary),
                                                )
                                                .on_hover_text(&r.target);
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.spacing_mut().item_spacing =
                                                        vec2(SPACE_1, 2.0);
                                                    if r.origin.as_deref()
                                                        == Some(ORIGIN_USER_IMPORTED)
                                                    {
                                                        render_pill(
                                                            ui,
                                                            &user_imp_lbl,
                                                            p.warning_fg,
                                                            p.warning_bg,
                                                            p.warning_fg,
                                                        );
                                                    }
                                                    if !r.eligibility_notes.is_empty()
                                                        && eligibility_badge_button(
                                                            ui,
                                                            p,
                                                            locale,
                                                            r.eligibility_notes.len(),
                                                            elig_expanded,
                                                        )
                                                        .clicked()
                                                    {
                                                        toggled_elig = Some(r.target.clone());
                                                    }
                                                });
                                            });
                                            cell(ui, widths[1], content_h, |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(country)
                                                            .monospace()
                                                            .strong()
                                                            .size(FONT_BODY)
                                                            .color(if country == "CN" {
                                                                p.warning_fg
                                                            } else {
                                                                p.text_primary
                                                            }),
                                                    )
                                                    .wrap_mode(egui::TextWrapMode::Extend),
                                                );
                                            });
                                            cell(ui, widths[2], content_h, |ui| {
                                                render_state_badge(
                                                    ui, r.state, theme, locale, true,
                                                );
                                            });
                                            cell(ui, widths[3], content_h, |ui| {
                                                let v4 = family(r, AttemptFamily::V4, locale);
                                                let v6 = family(r, AttemptFamily::V6, locale);
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(format!(
                                                            "v4  {v4}\nv6  {v6}"
                                                        ))
                                                        .monospace()
                                                        .size(FONT_META)
                                                        .color(p.text_secondary),
                                                    )
                                                    .wrap_mode(egui::TextWrapMode::Extend),
                                                );
                                                if let Some(note) = path_note(r) {
                                                    ui.label(
                                                        RichText::new(note)
                                                            .size(FONT_META)
                                                            .color(p.warning_fg),
                                                    );
                                                }
                                            });
                                            cell(ui, widths[4], content_h, |ui| {
                                                ui.label(
                                                    RichText::new(summary(r, locale))
                                                        .size(FONT_BODY)
                                                        .color(p.text_primary),
                                                );
                                            });
                                            cell(ui, widths[5], content_h, |ui| {
                                                operations(ui, r, p, locale, actions);
                                            });
                                        });
                                        if elig_expanded && !r.eligibility_notes.is_empty() {
                                            ui.add_space(SPACE_2);
                                            render_eligibility_notes_panel(
                                                ui,
                                                &r.eligibility_notes,
                                                locale,
                                                p,
                                            );
                                        }
                                    });

                                // 顶部细分隔线 + 异常行左侧 3px 阈值指示条
                                let rect = row_resp.response.rect;
                                ui.painter().line_segment(
                                    [
                                        pos2(rect.left(), rect.top()),
                                        pos2(rect.right(), rect.top()),
                                    ],
                                    Stroke::new(1.0_f32, p.border_subtle),
                                );
                                if let Some(sc) = stripe_color {
                                    let bar = Rect::from_min_max(
                                        pos2(rect.left() + 1.0, rect.top() + 4.0),
                                        pos2(rect.left() + 4.0, rect.bottom() - 4.0),
                                    );
                                    ui.painter().rect_filled(bar, Rounding::same(1.5), sc);
                                }
                            });
                        }
                    });
            });

        if let Some(id) = toggled_elig {
            state.toggle_eligibility_expanded(&id);
        }
    });
}
