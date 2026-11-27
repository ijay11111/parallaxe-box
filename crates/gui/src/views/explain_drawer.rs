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
    bind_status_label_plain, btn_outlined, btn_tonal, card_frame, confidence_label_plain,
    expectation_status_label_plain, freshness_label_plain, layer_label_localized,
    layer_ref_label_plain, observed_family_label, render_state_badge, requested_family_label,
    source_label_plain, subtle_well_frame, Palette, ThemeMode, FONT_BODY, FONT_META, FONT_TITLE,
    SPACE_1, SPACE_2, SPACE_3, SPACE_4,
};
use crate::views::{render_eligibility_notes_panel, service_matrix::service_name, UiAction};

fn plain_reason_label(code: &str) -> &str {
    use parallaxe_core::contracts::reason;
    match code {
        reason::REDIRECT_TO_LOGIN => "跳转到登录页，未观察到地区",
        reason::CAPTCHA_CHALLENGE => "遇到验证码挑战",
        reason::RATE_LIMITED => "请求被限流",
        reason::FIELD_MISSING | reason::NO_REGION_SIGNAL => "页面缺少可用地区信号",
        reason::TRUNCATED => "页面响应不完整",
        reason::DENIED_OR_AUTH => "服务拒绝访问或要求登录",
        reason::NO_ADMISSION | reason::ALLOW_ONLINE_FALSE => "未获联网许可",
        reason::NOT_SELECTED => "不在本次检测范围",
        reason::SAFE_DEFAULT_FALSE => "默认不自动检测",
        reason::GLOBAL_TIMEOUT => "检测已超时",
        reason::CANCELLED => "检测已取消",
        reason::BUDGET_EXHAUSTED => "已达检测预算上限",
        reason::ADMISSION_GRANT_EXPIRED => "联网许可已过期",
        reason::REGION_RESTRICTED => "服务返回明确地区限制信号",
        reason::POLICY_BLOCKED => "服务返回明确访问阻断信号",
        "REGION_RESTRICTED_FIXTURE" => "离线演示中的地区限制",
        "POLICY_BLOCKED_FIXTURE" => "离线演示中的访问阻断",
        "PRIVATE_LAYER_FORBIDDEN" => "涉及账号或私人信息，禁止采集",
        "NOT_OBSERVED" => "本次未观察到该层信息",
        other => other,
    }
}

/// 渲染选中目标的完整证据与解释抽屉（带显式关闭/返回按钮、资格说明 ⓘ 面板与骨架空状态）。
#[allow(clippy::too_many_arguments)]
pub fn render(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    static_explain: Option<&StaticTargetExplain>,
    theme: ThemeMode,
    is_narrow_sheet: bool,
    show_tech: bool,
    show_ip: bool,
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
                "1. 传输连接尝试与 IPv4/IPv6 双栈状态",
                "2. 服务页面观察到的地区与原因说明",
                "3. 验证码或限流提示与本机保护状态",
                "4. 七层地区归因明细与脱敏证据记录",
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
                let heading = if show_tech {
                    format!("{} ({})", service_name(target_id), target_id)
                } else {
                    service_name(target_id).to_string()
                };
                let resp = ui.label(
                    RichText::new(heading)
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.accent),
                );
                if show_tech {
                    resp.on_hover_text(target_id);
                }
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
                render_result_detail(ui, result, theme, locale, show_tech, show_ip);
            } else {
                let static_notes = registry.eligibility_notes_for(target_id);
                if !static_notes.is_empty() {
                    render_eligibility_notes_panel(ui, &static_notes, locale, p, show_tech);
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
                            let reason_disp = if show_tech {
                                uncov.reason.as_str()
                            } else {
                                plain_reason_label(&uncov.reason)
                            };
                            ui.label(format!(
                                "已选中：{} · 原因：{}",
                                if uncov.selected { "是" } else { "否" },
                                reason_disp
                            ));
                        });
                        ui.add_space(SPACE_4);
                    }
                }
            }

            if let Some(se) = static_explain.filter(|s| s.target_id == target_id) {
                ui.add_space(SPACE_4);
                render_static_explain(ui, se, theme, locale, show_tech);
            }
        });
}

fn render_result_detail(
    ui: &mut Ui,
    result: &ResultRecord,
    theme: ThemeMode,
    locale: Locale,
    show_tech: bool,
    show_ip: bool,
) {
    let p = Palette::for_theme(theme);

    // 顶层判定卡片
    card_frame(p).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            render_state_badge(ui, result.state, theme, locale, !show_tech);
            ui.monospace(format!(
                "{} ms · {} 次请求",
                result.duration_ms, result.request_count
            ));
        });
        if let Some(reason) = &result.reason {
            let text = if show_tech {
                format!("原因：{} ({reason})", plain_reason_label(reason))
            } else {
                format!("原因：{}", plain_reason_label(reason))
            };
            ui.label(RichText::new(text).color(p.warning_fg));
        }
        ui.label(format!(
            "可信度：{} · 来源：{}",
            confidence_label_plain(result.confidence, show_tech),
            result
                .source
                .map(|s| source_label_plain(s, show_tech))
                .unwrap_or("无")
        ));
        if let Some(exp) = &result.expectation {
            ui.label(format!(
                "预期比对：{}={}，实测={:?} → {}",
                exp.key,
                exp.expected,
                exp.actual,
                expectation_status_label_plain(exp.status, show_tech)
            ));
        }
        if let Some(origin) = &result.origin {
            ui.separator();
            let origin_desc = if origin == parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS {
                translate(locale, "gui.ai.badge.autonomous_run", &[])
            } else {
                "来源：用户自定义导入目标".to_string()
            };
            ui.colored_label(p.warning_fg, origin_desc);
            egui::CollapsingHeader::new(
                RichText::new(translate(locale, "gui.advanced.audit_fields", &[]))
                    .size(FONT_META)
                    .color(p.text_secondary),
            )
            .id_salt(("explain_origin_audit", &result.target))
            .open(show_tech.then_some(true))
            .show(ui, |ui| {
                ui.monospace(format!(
                    "origin={} · consent_version={} · imported_at={} · rule_digest={}",
                    origin,
                    result.consent_version.as_deref().unwrap_or("—"),
                    result.imported_at.as_deref().unwrap_or("—"),
                    result.rule_digest.as_deref().unwrap_or("—"),
                ));
            });
        }
    });

    // 静态资格说明（ⓘ 资格说明 · 不参与判定）
    if !result.eligibility_notes.is_empty() {
        ui.add_space(SPACE_4);
        render_eligibility_notes_panel(ui, &result.eligibility_notes, locale, p, show_tech);
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

        // 1. 传输与路径层
        ui.label(
            RichText::new(if show_tech {
                "1. 传输与路径层 (transport)"
            } else {
                "1. 传输与路径层"
            })
            .strong(),
        );
        ui.label(format!(
            "路径：{} · 请求族：{} · 实见族：{} · 绑定：{}",
            result.path.path_id,
            requested_family_label(result.path.requested_family),
            observed_family_label(result.path.observed_family),
            bind_status_label_plain(result.path.bind_status, show_tech)
        ));
        if let Some(diag) = &result.transport.path_diagnostic {
            let diag_plain = match diag.as_str() {
                "IPv6_SLOW" => "IPv6 路径偏慢",
                "IPv6_BROKEN" => "IPv6 路径异常",
                "IPV6_ONLY_EGRESS" => "仅观察到 IPv6 出口",
                "PROXY_REMOTE_UNVERIFIED" => "代理远端未验证",
                other => other,
            };
            ui.label(if show_tech {
                format!("路径诊断：{diag_plain} ({diag})")
            } else {
                format!("路径诊断：{diag_plain}")
            });
        }
        if result.destination.has_observation() {
            let mut dest = result.destination.clone();
            dest.set_show_ip(show_ip);
            ui.add_space(SPACE_1);
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.dest.map.title", &[]))
                        .strong()
                        .size(FONT_META)
                        .color(p.accent),
                );
                let role = crate::views::service_matrix::peer_role_label(
                    dest.peer_role,
                    locale,
                    show_tech,
                );
                let resolved_s = crate::views::service_matrix::format_resolved_summary(
                    &dest, locale, show_ip,
                );
                let peer_s = crate::views::service_matrix::format_peer_summary(
                    &dest, locale, show_ip,
                );
                ui.label(
                    RichText::new(format!(
                        "{}：{role} · {}：{resolved_s} · {}：{peer_s}",
                        translate(locale, "gui.dest.col.role", &[]),
                        translate(locale, "gui.dest.col.resolved", &[]),
                        translate(locale, "gui.dest.col.peer", &[])
                    ))
                    .size(FONT_META)
                    .color(p.text_primary),
                );
                let cmp = if dest.expected_country.is_some() {
                    dest.region_consistency
                } else {
                    dest.address_consistency
                };
                ui.label(
                    RichText::new(format!(
                        "{}：{}",
                        translate(locale, "gui.dest.col.consistency", &[]),
                        crate::views::service_matrix::comparison_label(cmp, locale, show_tech)
                    ))
                    .size(FONT_META)
                    .color(if dest.has_risk_markers() {
                        p.warning_fg
                    } else {
                        p.text_secondary
                    }),
                );
            });
        }
        if !result.transport.attempts.is_empty() {
            egui::CollapsingHeader::new(
                RichText::new(format!("传输尝试记录（{} 次）", result.transport.attempts.len()))
                    .size(FONT_META)
                    .color(p.text_secondary),
            )
            .id_salt(("explain_attempts", &result.target))
            .open(show_tech.then_some(true))
            .show(ui, |ui| {
                for (idx, att) in result.transport.attempts.iter().enumerate() {
                    let line = if show_tech {
                        format!(
                            "尝试#{}: family={:?} phase={} http={:?} connect_ms={:?} first_byte_ms={:?} fallback={} sent={}",
                            idx + 1,
                            att.family,
                            att.phase,
                            att.http_status,
                            att.connect_ms,
                            att.first_byte_ms,
                            att.fallback,
                            att.request_sent
                        )
                    } else {
                        let status = att
                            .http_status
                            .map(|c| format!("HTTP {c}"))
                            .unwrap_or_else(|| "无 HTTP 响应".to_string());
                        format!("第 {} 次尝试：{:?} · {}", idx + 1, att.family, status)
                    };
                    ui.label(RichText::new(line).size(FONT_META).color(p.text_secondary));
                }
            });
        }
        ui.add_space(SPACE_1);

        // 2. 服务表面观测
        ui.label(
            RichText::new(if show_tech {
                "2. 服务页面观测 (service_observed)"
            } else {
                "2. 服务页面观测"
            })
            .strong(),
        );
        let svc_reason = result
            .service_observed
            .reason
            .as_deref()
            .map(|r| if show_tech { r } else { plain_reason_label(r) })
            .unwrap_or("无");
        ui.label(format!(
            "检测表面：{} · 观察地区：{} · 说明：{}",
            result.service_observed.surface,
            result
                .service_observed
                .observed_country
                .as_deref()
                .unwrap_or("无地区信号"),
            svc_reason
        ));
        ui.add_space(SPACE_1);

        // 3. 风控与挑战迹象
        ui.label(
            RichText::new(if show_tech {
                "3. 风险与挑战提示 (risk_signal)"
            } else {
                "3. 风险与挑战提示（不作地区定论）"
            })
            .strong(),
        );
        let markers_disp = if result.risk_signal.markers.is_empty() {
            "无".to_string()
        } else if show_tech {
            result.risk_signal.markers.join(", ")
        } else {
            format!("检测到 {} 项风险提示（开启技术细节查看原码）", result.risk_signal.markers.len())
        };
        let risk_reason = result
            .risk_signal
            .reason
            .as_deref()
            .map(|r| if show_tech { r } else { plain_reason_label(r) })
            .unwrap_or("无");
        ui.label(format!("标记：{markers_disp} · 说明：{risk_reason}"));
        ui.add_space(SPACE_1);

        // 4. 本机保护与处置资格
        ui.label(
            RichText::new(if show_tech {
                "4. 本机保护与处置资格 (local_policy)"
            } else {
                "4. 本机保护与处置资格"
            })
            .strong(),
        );
        ui.label(format!(
            "受保护服务：{} · 允许生成屏蔽建议：{}",
            if result.local_policy.protected {
                "是"
            } else {
                "否"
            },
            if result.local_policy.guard_eligible {
                "是"
            } else {
                "否"
            }
        ));
    });

    ui.add_space(SPACE_4);

    // 七层归因（固定 7 行，穷举 Layer，不隐去无信号原因）
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
            .spacing([SPACE_3, SPACE_2])
            .show(ui, |ui| {
                ui.label(RichText::new(translate(locale, "gui.explain.col.layer", &[])).strong());
                ui.label(RichText::new(translate(locale, "gui.explain.col.value", &[])).strong());
                ui.label(RichText::new(translate(locale, "gui.explain.col.reason", &[])).strong());
                ui.end_row();
                for lv in &result.region_layers {
                    let (loc_name, code) = layer_label_localized(lv.layer, locale);
                    if show_tech {
                        ui.label(format!("{loc_name} ({code})"));
                    } else {
                        ui.label(&loc_name).on_hover_text(code);
                    }
                    ui.monospace(lv.value.as_deref().unwrap_or("—"));
                    let reason_text = lv
                        .reason
                        .as_deref()
                        .map(|r| if show_tech { r } else { plain_reason_label(r) })
                        .unwrap_or("—");
                    ui.label(
                        RichText::new(reason_text)
                            .size(FONT_META)
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
        ui.add_space(SPACE_3);
        card_frame(p).show(ui, |ui| {
            ui.label(
                RichText::new("冲突与多源比对（不取平均、平票不强判）")
                    .strong()
                    .color(p.warning_fg),
            );
            ui.separator();
            for c in &result.conflicts {
                let text = if show_tech {
                    format!(
                        "≠ claim={} · 冲突证据: {}",
                        c.claim,
                        c.evidence_ids.join(", ")
                    )
                } else {
                    format!("≠ 观测信号存在冲突（涉及 {} 条证据）", c.evidence_ids.len())
                };
                ui.label(RichText::new(text).color(p.warning_fg));
            }
            for ag in &result.agreement {
                let votes_desc: Vec<String> = ag
                    .votes
                    .iter()
                    .map(|v| format!("{}（{}票）", v.value, v.sample_ids.len()))
                    .collect();
                ui.label(format!(
                    "分组 {} · 投票分布：[{}]{}",
                    ag.bucket,
                    votes_desc.join(" / "),
                    if ag.tie { " · 平票无胜出" } else { "" }
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
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                layer_ref_label_plain(ev.layer, show_tech),
                                ev.value
                            ))
                            .strong()
                            .size(FONT_BODY)
                            .color(p.text_primary),
                        );
                        if ev.fixture {
                            ui.label(RichText::new("离线示例").size(FONT_META).color(p.info_fg));
                        }
                    });
                    ui.label(
                        RichText::new(format!(
                            "可信度：{} · 来源：{} · 时效：{}",
                            confidence_label_plain(ev.confidence, show_tech),
                            source_label_plain(ev.source_type, show_tech),
                            freshness_label_plain(ev.freshness.status, show_tech),
                        ))
                        .size(FONT_META)
                        .color(p.text_secondary),
                    );
                    if show_tech {
                        ui.monospace(format!(
                            "id={} · rule={} v{} · claim={} · ref={} · at={}",
                            ev.id,
                            ev.rule_id,
                            ev.rule_version,
                            ev.claim,
                            ev.source_ref,
                            ev.observed_at
                        ));
                    }
                    if let Some(r) = &ev.reason {
                        ui.label(
                            RichText::new(format!(
                                "说明：{}",
                                if show_tech { r } else { plain_reason_label(r) }
                            ))
                            .size(FONT_META)
                            .color(p.warning_fg),
                        );
                    }
                });
            }
        }
    });
}

fn render_static_explain(
    ui: &mut Ui,
    se: &StaticTargetExplain,
    theme: ThemeMode,
    locale: Locale,
    show_tech: bool,
) {
    let p = Palette::for_theme(theme);
    card_frame(p).show(ui, |ui| {
        ui.label(
            RichText::new("静态目录与依赖说明（不发请求）")
                .strong()
                .color(p.accent),
        );
        ui.separator();
        let t = &se.target_doc["target"];
        let admission_raw = t["admission_status"].as_str().unwrap_or("—");
        let admission_disp = if show_tech {
            admission_raw
        } else {
            match admission_raw {
                "ADMITTED" => "已准入",
                "DENIED" | "NOT_ADMITTED" => "未准入",
                other => plain_reason_label(other),
            }
        };
        ui.label(format!(
            "地址：{}\n检测表面：{}\n受保护服务：{} · 在线准入：{}",
            t["url"].as_str().unwrap_or("—"),
            t["surface"].as_str().unwrap_or("—"),
            if t["protected"].as_bool().unwrap_or(false) {
                "是"
            } else {
                "否"
            },
            admission_disp,
        ));
        if let Some(origin) = t["origin"].as_str() {
            ui.colored_label(p.warning_fg, "来源：用户自定义导入");
            egui::CollapsingHeader::new(
                RichText::new(translate(locale, "gui.advanced.audit_fields", &[]))
                    .size(FONT_META)
                    .color(p.text_secondary),
            )
            .id_salt(("static_origin_audit", &se.target_id))
            .open(show_tech.then_some(true))
            .show(ui, |ui| {
                ui.monospace(format!(
                    "origin={} · consent_version={} · imported_at={} · rule_digest={}",
                    origin,
                    t["consent_version"].as_str().unwrap_or("—"),
                    t["imported_at"].as_str().unwrap_or("—"),
                    t["rule_digest"].as_str().unwrap_or("—"),
                ));
            });
        }
        if let Some(failure) = t["failure"].as_str() {
            ui.label(
                RichText::new(format!("已知失效说明：{failure}"))
                    .size(FONT_META)
                    .color(p.text_secondary),
            );
        }
        if let Some(edges) = se.dependencies_doc["edges"].as_array() {
            ui.add_space(SPACE_1);
            ui.label(RichText::new(format!("已登记关联依赖 ({})", edges.len())).strong());
            if edges.is_empty() {
                ui.label(
                    RichText::new("无显式依赖记录（无记录不等于独占域名，不可据此自动阻断）。")
                        .size(FONT_META)
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
