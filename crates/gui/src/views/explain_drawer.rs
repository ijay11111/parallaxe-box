//! 右侧证据与解释抽屉：四层结果 + 七层归因 + 证据明细 + 冲突/平票 + 静态目录解释。
//!
//! 铁律：全部字段直接取自 `ResultRecord`（不拆字符串，不自行推断国家，不忽略 null 原因）。

use egui::{RichText, ScrollArea, Ui};
use parallaxe_core::contracts::{ResultRecord, State};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::targets::Registry;

use crate::commands::StaticTargetExplain;
use crate::state::AppState;
use crate::theme::{
    bind_status_label, btn_outlined, btn_tonal, card_frame, confidence_label,
    expectation_status_label, freshness_label, layer_label_localized, layer_ref_label,
    observed_family_label, render_state_badge, requested_family_label, source_label,
    subtle_well_frame, Palette, ThemeMode, FONT_BODY, FONT_META, FONT_TITLE, SPACE_1, SPACE_2,
    SPACE_3, SPACE_4,
};
use crate::views::{render_eligibility_notes_panel, UiAction};

/// 渲染选中目标的完整证据与解释抽屉（带显式关闭/返回按钮、资格说明 ⓘ 面板与骨架空状态）。
pub fn render(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    static_explain: Option<&StaticTargetExplain>,
    theme: ThemeMode,
    is_narrow_sheet: bool,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);
    let locale = state.current_locale();

    ui.horizontal(|ui| {
        if is_narrow_sheet {
            if btn_outlined(ui, p, &translate(locale, "gui.explain.back", &[]), true).clicked() {
                actions.push(UiAction::CloseExplainDrawer);
            }
            ui.add_space(SPACE_2);
            ui.label(
                RichText::new(translate(locale, "gui.explain.title_narrow", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
        } else {
            ui.label(
                RichText::new(translate(locale, "gui.explain.title", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.accent),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(
                        RichText::new(translate(locale, "gui.explain.close", &[]))
                            .color(p.text_secondary),
                    )
                    .clicked()
                {
                    actions.push(UiAction::CloseExplainDrawer);
                }
            });
        }
    });
    ui.add_space(SPACE_2);
    ui.separator();
    ui.add_space(SPACE_3);

    let Some(target_id) = state.selected_target_id.as_deref() else {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(translate(locale, "gui.explain.empty_title", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
            ui.add_space(SPACE_2);
            ui.label(
                RichText::new(translate(locale, "gui.explain.empty_desc", &[]))
                    .size(FONT_BODY)
                    .color(p.text_secondary),
            );
            ui.add_space(SPACE_2);
            for line in [
                "1. transport 传输尝试与双栈三态诊断",
                "2. service_observed 表面观察国家与原因",
                "3. risk_signal 挑战/限流迹象与 local_policy 保护",
                "4. 固定 7 层地区归因与脱敏白名单证据",
            ] {
                ui.label(RichText::new(line).size(FONT_META).color(p.text_secondary));
            }
            ui.add_space(SPACE_3);
            if btn_tonal(
                ui,
                p,
                &translate(locale, "gui.explain.open_catalog", &[]),
                true,
            )
            .clicked()
            {
                actions.push(UiAction::ToggleCatalogDrawer);
            }
        });
        return;
    };

    ScrollArea::vertical()
        .id_salt("explain_drawer_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                ui.label(
                    RichText::new(target_id)
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.accent),
                );
                if let Some(run) = state.active_run() {
                    if !run.is_fixture
                        && btn_outlined(ui, p, &translate(locale, "gui.explain.reprobe", &[]), true)
                            .clicked()
                    {
                        actions.push(UiAction::RunSingleReprobe(target_id.to_string()));
                    }
                    if btn_tonal(
                        ui,
                        p,
                        &translate(locale, "gui.explain.guard_preview", &[]),
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::RequestGuardPreview {
                            source_run_id: run.run_id.clone(),
                            targets: vec![target_id.to_string()],
                        });
                    }
                }
            });
            ui.add_space(SPACE_3);

            if let Some(result) = state.active_run().and_then(|r| r.result_for(target_id)) {
                render_result_detail(ui, result, theme, locale);
            } else {
                let static_notes = registry.eligibility_notes_for(target_id);
                if !static_notes.is_empty() {
                    render_eligibility_notes_panel(ui, &static_notes, locale, p);
                    ui.add_space(SPACE_3);
                }
                if let Some(run) = state.active_run() {
                    if let Some(uncov) = run
                        .coverage
                        .as_ref()
                        .and_then(|c| c.uncovered.iter().find(|u| u.target == target_id))
                    {
                        card_frame(p).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new("本次未覆盖").strong().color(p.warning_fg));
                            ui.label(format!(
                                "已选中：{} · 原因：{}",
                                uncov.selected, uncov.reason
                            ));
                        });
                        ui.add_space(SPACE_4);
                    }
                }
            }

            if let Some(se) = static_explain.filter(|s| s.target_id == target_id) {
                ui.add_space(SPACE_4);
                render_static_explain(ui, se, theme);
            }
        });
}

fn render_result_detail(ui: &mut Ui, result: &ResultRecord, theme: ThemeMode, locale: Locale) {
    let p = Palette::for_theme(theme);

    // 顶层判定卡片
    card_frame(p).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            render_state_badge(ui, result.state, theme, locale, false);
            ui.monospace(format!(
                "{} ms · req {}",
                result.duration_ms, result.request_count
            ));
        });
        if let Some(reason) = &result.reason {
            ui.label(RichText::new(format!("原因：{reason}")).color(p.warning_fg));
        }
        ui.label(format!(
            "可信度：{} · 来源：{}",
            confidence_label(result.confidence),
            result.source.map(source_label).unwrap_or("无 (null)")
        ));
        if let Some(exp) = &result.expectation {
            ui.label(format!(
                "--expect 比对：{}={}，实测={:?} → {}",
                exp.key,
                exp.expected,
                exp.actual,
                expectation_status_label(exp.status)
            ));
        }
        if let Some(origin) = &result.origin {
            ui.separator();
            ui.colored_label(
                p.warning_fg,
                format!(
                    "用户导入审计留痕：origin={} · consent_version={} · imported_at={} · rule_digest={}",
                    origin,
                    result.consent_version.as_deref().unwrap_or("—"),
                    result.imported_at.as_deref().unwrap_or("—"),
                    result.rule_digest.as_deref().unwrap_or("—"),
                ),
            );
        }
    });

    // 静态资格说明（ⓘ 资格说明 · verified=false · 不参与判定）
    if !result.eligibility_notes.is_empty() {
        ui.add_space(SPACE_4);
        render_eligibility_notes_panel(ui, &result.eligibility_notes, locale, p);
    }

    ui.add_space(SPACE_4);

    // 四层结果（直接结构化渲染，绝不拆字符串）
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(translate(locale, "gui.explain.four_layers", &[]))
                .strong()
                .size(FONT_TITLE)
                .color(p.accent),
        );
        ui.separator();

        // 1. transport
        ui.label(RichText::new("1. transport（传输与路径层）").strong());
        ui.label(format!(
            "路径：{} · 请求族={} · 实见族={} · 绑定={}",
            result.path.path_id,
            requested_family_label(result.path.requested_family),
            observed_family_label(result.path.observed_family),
            bind_status_label(result.path.bind_status)
        ));
        if let Some(diag) = &result.transport.path_diagnostic {
            ui.label(format!("路径诊断：{diag}"));
        }
        for (idx, att) in result.transport.attempts.iter().enumerate() {
            ui.label(
                RichText::new(format!(
                    "  尝试#{}: family={:?} phase={} http={:?} connect_ms={:?} first_byte_ms={:?} fallback={} sent={}",
                    idx + 1,
                    att.family,
                    att.phase,
                    att.http_status,
                    att.connect_ms,
                    att.first_byte_ms,
                    att.fallback,
                    att.request_sent
                ))
                .size(11.5)
                .monospace()
                .color(p.text_secondary),
            );
        }
        ui.add_space(4.0);

        // 2. service_observed
        ui.label(RichText::new("2. service_observed（服务表面观测）").strong());
        ui.label(format!(
            "表面：{} · 观察国家：{} · 原因：{}",
            result.service_observed.surface,
            result
                .service_observed
                .observed_country
                .as_deref()
                .unwrap_or("null（无地区信号）"),
            result
                .service_observed
                .reason
                .as_deref()
                .unwrap_or("无")
        ));
        ui.add_space(4.0);

        // 3. risk_signal
        ui.label(RichText::new("3. risk_signal（风控/挑战迹象，不作封国定论）").strong());
        ui.label(format!(
            "标记：{} · 原因：{}",
            if result.risk_signal.markers.is_empty() {
                "无".to_string()
            } else {
                result.risk_signal.markers.join(", ")
            },
            result.risk_signal.reason.as_deref().unwrap_or("无")
        ));
        ui.add_space(4.0);

        // 4. local_policy
        ui.label(RichText::new("4. local_policy（本机保护与 Guard 资格）").strong());
        ui.label(format!(
            "受保护={} · guard_eligible={} · plan_id={} · 原因={}",
            result.local_policy.protected,
            result.local_policy.guard_eligible,
            result.local_policy.plan_id.as_deref().unwrap_or("无"),
            result.local_policy.reason.as_deref().unwrap_or("无")
        ));
    });

    ui.add_space(SPACE_4);

    // 七层归因（固定 7 行，穷举 Layer，不隐去 null + reason）
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let layers_len = result.region_layers.len().to_string();
        ui.label(
            RichText::new(translate(
                locale,
                "gui.explain.seven_layers",
                &[&layers_len],
            ))
            .strong()
            .size(FONT_TITLE)
            .color(p.accent),
        );
        ui.separator();
        egui::Grid::new("seven_layers_grid")
            .num_columns(3)
            .striped(true)
            .spacing([SPACE_3, SPACE_1 + 2.0])
            .show(ui, |ui| {
                ui.label(RichText::new(translate(locale, "gui.explain.col.layer", &[])).strong());
                ui.label(RichText::new(translate(locale, "gui.explain.col.value", &[])).strong());
                ui.label(RichText::new(translate(locale, "gui.explain.col.reason", &[])).strong());
                ui.end_row();
                for lv in &result.region_layers {
                    let (loc_name, code) = layer_label_localized(lv.layer, locale);
                    ui.label(format!("{loc_name} ({code})"));
                    ui.monospace(lv.value.as_deref().unwrap_or("null"));
                    ui.label(
                        RichText::new(lv.reason.as_deref().unwrap_or("—"))
                            .size(11.5)
                            .color(p.text_secondary),
                    );
                    ui.end_row();
                }
            });
    });

    // 冲突与分桶投票（完整展开，不平均、不吞票）
    if !result.conflicts.is_empty()
        || !result.agreement.is_empty()
        || result.state == State::Conflict
    {
        ui.add_space(6.0);
        card_frame(p).show(ui, |ui| {
            ui.label(
                RichText::new("冲突与分桶投票（不平均、平票无赢家）")
                    .strong()
                    .color(p.warning_fg),
            );
            ui.separator();
            for c in &result.conflicts {
                ui.label(
                    RichText::new(format!(
                        "≠ claim={} · 冲突证据 IDs: {}",
                        c.claim,
                        c.evidence_ids.join(", ")
                    ))
                    .color(p.warning_fg),
                );
            }
            for ag in &result.agreement {
                let votes_desc: Vec<String> = ag
                    .votes
                    .iter()
                    .map(|v| format!("{}({}票)", v.value, v.sample_ids.len()))
                    .collect();
                ui.label(format!(
                    "桶 {} · 票分布=[{}] · winner={:?} · tie={}",
                    ag.bucket,
                    votes_desc.join(" vs "),
                    ag.winner,
                    ag.tie
                ));
            }
        });
    }

    // 脱敏证据明细
    ui.add_space(SPACE_4);
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let ev_len = result.evidence.len().to_string();
        ui.label(
            RichText::new(translate(locale, "gui.explain.evidence_title", &[&ev_len]))
                .strong()
                .size(FONT_TITLE)
                .color(p.accent),
        );
        ui.separator();
        if result.evidence.is_empty() {
            ui.label(RichText::new("无提取到的白名单证据项。").color(p.text_secondary));
        } else {
            for (idx, ev) in result.evidence.iter().enumerate() {
                if idx > 0 {
                    ui.add_space(SPACE_2);
                }
                subtle_well_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(&ev.id);
                        ui.label(format!(
                            "规则 {} v{} · 层={}",
                            ev.rule_id,
                            ev.rule_version,
                            layer_ref_label(ev.layer)
                        ));
                        if ev.fixture {
                            ui.label(RichText::new("[FIXTURE]").color(p.info_fg));
                        }
                    });
                    ui.label(format!("claim={} · value={}", ev.claim, ev.value));
                    ui.label(
                        RichText::new(format!(
                            "可信度={} · 来源={} · 参考={}",
                            confidence_label(ev.confidence),
                            source_label(ev.source_type),
                            ev.source_ref
                        ))
                        .size(FONT_META)
                        .color(p.text_secondary),
                    );
                    ui.label(
                        RichText::new(format!(
                            "新鲜度={} (连续缺失={}) · 时刻={}",
                            freshness_label(ev.freshness.status),
                            ev.freshness.comparable_misses,
                            ev.observed_at
                        ))
                        .size(FONT_META)
                        .color(p.text_secondary),
                    );
                    if let Some(r) = &ev.reason {
                        ui.label(
                            RichText::new(format!("原因：{r}"))
                                .size(FONT_META)
                                .color(p.warning_fg),
                        );
                    }
                });
            }
        }
    });
}

fn render_static_explain(ui: &mut Ui, se: &StaticTargetExplain, theme: ThemeMode) {
    let p = Palette::for_theme(theme);
    card_frame(p).show(ui, |ui| {
        ui.label(
            RichText::new("静态目录与依赖解释（不发请求）")
                .strong()
                .color(p.accent),
        );
        ui.separator();
        let t = &se.target_doc["target"];
        ui.label(format!(
            "地址：{}\n家族/分组：{} / {} · 表面：{}\n保护：{} · 在线准入：{} ({})",
            t["url"].as_str().unwrap_or("—"),
            t["family"].as_str().unwrap_or("—"),
            t["group"].as_str().unwrap_or("—"),
            t["surface"].as_str().unwrap_or("—"),
            t["protected"].as_bool().unwrap_or(false),
            t["admission_status"].as_str().unwrap_or("—"),
            t["reason"].as_str().unwrap_or("无")
        ));
        if let Some(origin) = t["origin"].as_str() {
            ui.colored_label(
                p.warning_fg,
                format!(
                    "用户导入审计：origin={} · consent_version={} · imported_at={} · rule_digest={}",
                    origin,
                    t["consent_version"].as_str().unwrap_or("—"),
                    t["imported_at"].as_str().unwrap_or("—"),
                    t["rule_digest"].as_str().unwrap_or("—"),
                ),
            );
        }
        if let Some(failure) = t["failure"].as_str() {
            ui.label(
                RichText::new(format!("已知失效说明：{failure}"))
                    .size(11.5)
                    .color(p.text_secondary),
            );
        }
        if let Some(edges) = se.dependencies_doc["edges"].as_array() {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("已登记依赖边 ({})", edges.len())).strong());
            if edges.is_empty() {
                ui.label(
                    RichText::new("无显式边（无来源不等于专属性，不可据此自动阻断）。")
                        .size(11.5)
                        .color(p.text_secondary),
                );
            } else {
                for edge in edges {
                    ui.monospace(format!(
                        "{} → {} [{}]",
                        edge["src"].as_str().unwrap_or(""),
                        edge["dst"].as_str().unwrap_or(""),
                        edge["kind"].as_str().unwrap_or("")
                    ));
                }
            }
        }
    });
}
