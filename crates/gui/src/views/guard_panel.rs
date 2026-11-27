//! 处置中心：本机屏蔽影响预演、主动屏蔽任意服务（含当前正常服务，如 Google One）、
//! 链路体检四列只读视图与安全审计回滚说明。
//!
//! 设计原则：
//! - 全界面采用清晰大白话中文说明"每个处置方法是干什么的、解决什么问题"，
//!   彻底隐藏默认界面上的英文等式代码（技术字段收拢至默认折叠的「查看技术字段」区域）；
//! - 明确支持用户主动选择任意服务（包括当前访问正常的服务，例如 Google Books 受限、Google One 正常时主动屏蔽 Google One）
//!   进行静态依赖误伤预演并生成只读处置清单；
//! - 严守阶段 4/5 红线：全部数据原样消费自 `parallaxe_core::guard` / `linkcheck` / `targets`，
//!   绝不自行判定、绝不自动写入系统规则（`applied=false`、`rules_written=0`）。

use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{vec2, Align, Layout, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::contracts::{GuardPlan, State};
use parallaxe_core::forensics;
use parallaxe_core::guard::{self, GuardRequest};
use parallaxe_core::guard_export::{
    self, ExportMode, ExportRequest, RuleFormat, FULL_MODE_WARNING,
};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::linkcheck::{self, LinkReport};
use parallaxe_core::sysblock;
use parallaxe_core::targets::Registry;

use crate::state::AppState;
use crate::theme::{
    btn_danger_tonal, btn_filled, btn_outlined, btn_tonal, card_frame, kv_row, render_admonition,
    render_modal_scrim, render_pill, spaced_columns, state_badge, subtle_well_frame, Palette,
    ThemeMode, FONT_BODY, FONT_META, FONT_TITLE, RADIUS_LG, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3,
    SPACE_4, SPACE_5,
};
use crate::views::service_matrix::service_name;
use crate::views::UiAction;

/// 帮助与进阶文档仓库链接（第三备选不进软件 UI，仅由文档入口跳转 GitHub 仓库文档）。
pub const SELF_BLOCK_DOCS_URL: &str =
    "https://github.com/ijay11111/parallaxe-box/blob/main/docs/self-block-guide.md";

/// 只读扫描到的未完成本机阻断意图审计记录（与 CLI `guard_journal` 的 `parallaxe.sysblock.audit/v2` 对齐）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSysblockAudit {
    pub rule_tag: String,
    pub status: String,
    pub code: String,
    pub detail: String,
    pub recovery_command: String,
}

/// 返回固定特权意图审计目录（与 `parallaxe-cli::guard_journal::directory()` 一致，纯只读）。
pub fn default_sysblock_audit_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Parallaxe/guard"))
    } else if cfg!(target_os = "linux") {
        Some(PathBuf::from("/var/lib/parallaxe/guard"))
    } else {
        None
    }
}

/// 纯只读扫描指定目录中未完成的 `sysblock-*.json` 意图记录（`INTENT` / `REVOKE_INTENT` 或损坏文件）。
///
/// 目录不存在或无权读取时静默返回空列表，绝不创建目录、绝不写入文件。
pub fn scan_pending_sysblock_audits(dir: &std::path::Path) -> Vec<PendingSysblockAudit> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(tag_stem) = name
            .strip_prefix("sysblock-")
            .and_then(|s| s.strip_suffix(".json"))
        else {
            continue;
        };
        if !sysblock::valid_rule_tag(tag_stem) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(doc) => {
                let schema_ok = doc
                    .get("schema")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s == "parallaxe.sysblock.audit/v2");
                let status = doc
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN");
                if !schema_ok {
                    found.push(PendingSysblockAudit {
                        rule_tag: tag_stem.to_string(),
                        status: "INVALID".to_string(),
                        code: "SYSBLOCK_AUDIT_INVALID".to_string(),
                        detail: "意图记录格式不匹配，请先在管理员终端核对本机规则。".to_string(),
                        recovery_command: format!("parallaxe guard unblock --rule-tag {tag_stem}"),
                    });
                    continue;
                }
                if matches!(status, "INTENT" | "REVOKE_INTENT") {
                    let rule_tag = doc
                        .get("rule_tag")
                        .and_then(|v| v.as_str())
                        .filter(|t| sysblock::valid_rule_tag(t))
                        .unwrap_or(tag_stem)
                        .to_string();
                    let code = doc
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("SYSBLOCK_RECOVERY_REQUIRED")
                        .to_string();
                    let detail = doc
                        .get("detail")
                        .and_then(|v| v.as_str())
                        .unwrap_or("上次系统写入状态未知，请先核对规则。")
                        .to_string();
                    let recovery_command = doc
                        .get("recovery")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            format!("parallaxe guard unblock --rule-tag {rule_tag}")
                        });
                    found.push(PendingSysblockAudit {
                        rule_tag,
                        status: status.to_string(),
                        code,
                        detail,
                        recovery_command,
                    });
                }
            }
            Err(_) => {
                found.push(PendingSysblockAudit {
                    rule_tag: tag_stem.to_string(),
                    status: "INVALID".to_string(),
                    code: "SYSBLOCK_AUDIT_INVALID".to_string(),
                    detail: "无法解析本机意图记录，请先在管理员终端核对本机规则。".to_string(),
                    recovery_command: format!("parallaxe guard unblock --rule-tag {tag_stem}"),
                });
            }
        }
    }
    found.sort_by(|a, b| a.rule_tag.cmp(&b.rule_tag));
    found
}

/// 可注入的原生文件夹选择器回调类型（生产默认 `None` 调用系统原生对话框）。
pub type FolderPickerFn = fn(&str) -> Result<Option<PathBuf>, String>;

/// 处置中心交互状态（支持用户主动勾选任意服务进行屏蔽预演、规则导出与系统阻断预览）。
#[derive(Debug)]
pub struct GuardViewState {
    /// 用户主动选中的拟屏蔽/拟预演目标（可包含当前正常服务，如 `google.one`）。
    pub selected_services: BTreeSet<String>,
    /// 全目录服务搜索关键字。
    pub service_search: String,
    /// 主方案 A：不污染式规则导出（rule-provider）弹窗开关。
    pub export_modal_open: bool,
    pub export_format: RuleFormat,
    /// 最近一次用户在原生文件夹对话框中显式选定的导出目录（绝不预填静默默认目录）。
    pub export_selected_dir: Option<PathBuf>,
    pub export_feedback: Option<String>,
    /// 主方案 B：Win/Linux 本机系统阻断（预览 / 确认 / 管理员撤销命令）弹窗开关。
    pub sysblock_modal_open: bool,
    pub sysblock_tag_input: String,
    pub sysblock_ack: bool,
    resolved_preview: Option<(
        String,
        Result<sysblock::SysBlockPlan, sysblock::SysBlockError>,
    )>,
    pub sysblock_feedback: Option<String>,
    /// 第二备选：整包替换完整规则弹窗（仅从次级菜单「更多导出选项」触发，不主动推荐）。
    pub full_modal_open: bool,
    pub full_format: RuleFormat,
    pub full_ack: bool,
    /// 最近一次用户在原生文件夹对话框中显式选定的整包导出目录（绝不预填静默默认目录）。
    pub full_selected_dir: Option<PathBuf>,
    pub full_feedback: Option<String>,
    /// 可选文件夹选择器回调（测试注入用；生产为 None 时调用原生系统对话框）。
    pub folder_picker_override: Option<FolderPickerFn>,
    /// 只读意图审计扫描结果（存在未完成 `INTENT` / `REVOKE_INTENT` 时展示克制恢复条）。
    pub pending_audits: Vec<PendingSysblockAudit>,
    pub pending_audits_scanned: bool,
    /// 可注入的只读扫描目录（测试或自定义排错用；生产默认读取固定特权目录）。
    pub audit_dir_override: Option<PathBuf>,
    pub recovery_feedback: Option<String>,
}

impl Default for GuardViewState {
    fn default() -> Self {
        GuardViewState {
            selected_services: BTreeSet::new(),
            service_search: String::new(),
            export_modal_open: false,
            export_format: RuleFormat::Clash,
            export_selected_dir: None,
            export_feedback: None,
            sysblock_modal_open: false,
            sysblock_tag_input: "local".to_string(),
            sysblock_ack: false,
            resolved_preview: None,
            sysblock_feedback: None,
            full_modal_open: false,
            full_format: RuleFormat::Clash,
            full_ack: false,
            full_selected_dir: None,
            full_feedback: None,
            folder_picker_override: None,
            pending_audits: Vec::new(),
            pending_audits_scanned: false,
            audit_dir_override: None,
            recovery_feedback: None,
        }
    }
}

impl GuardViewState {
    /// 是否有任意阻断/导出模态框处于打开状态。
    pub fn any_modal_open(&self) -> bool {
        self.export_modal_open || self.sysblock_modal_open || self.full_modal_open
    }

    /// 关闭全部阻断/导出模态框（用于 Esc / Android 返回键）。
    pub fn close_modals(&mut self) {
        self.export_modal_open = false;
        self.sysblock_modal_open = false;
        self.full_modal_open = false;
    }

    /// 执行一次只读意图审计扫描（不会创建目录，不会写文件）。
    pub fn refresh_pending_audits(&mut self) {
        let dir = self
            .audit_dir_override
            .clone()
            .or_else(default_sysblock_audit_dir);
        self.pending_audits = dir
            .as_deref()
            .map(scan_pending_sysblock_audits)
            .unwrap_or_default();
        self.pending_audits_scanned = true;
    }

    /// 首次进入处置中心或设置页时懒扫描一次，避免每帧重复读盘。
    pub fn ensure_pending_audits_scanned(&mut self) {
        if !self.pending_audits_scanned {
            self.refresh_pending_audits();
        }
    }
}

/// 若特权目录中存在未完成的本机阻断意图记录，渲染克制、非弹窗阻塞的只读恢复横幅。
pub fn render_pending_audit_banner(
    ui: &mut Ui,
    view: &mut GuardViewState,
    locale: Locale,
    p: Palette,
    show_tech: bool,
) {
    view.ensure_pending_audits_scanned();
    if view.pending_audits.is_empty() {
        return;
    }
    let audits = view.pending_audits.clone();
    let mut should_refresh = false;
    render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(translate(locale, "gui.guard.recovery.title", &[]))
                    .strong()
                    .size(FONT_BODY)
                    .color(p.warning_fg),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if btn_outlined(
                    ui,
                    p,
                    &translate(locale, "gui.guard.recovery.refresh", &[]),
                    true,
                )
                .clicked()
                {
                    should_refresh = true;
                }
            });
        });
        ui.add_space(SPACE_1);
        ui.label(
            RichText::new(translate(locale, "gui.guard.recovery.body", &[]))
                .size(FONT_META)
                .color(p.text_primary),
        );
        ui.add_space(SPACE_2);
        for item in &audits {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                let tag_pill = if show_tech {
                    format!("tag={}", item.rule_tag)
                } else {
                    format!("规则标签：{}", item.rule_tag)
                };
                render_pill(ui, &tag_pill, p.warning_fg, p.card, p.warning_fg);
                let status_pill = if show_tech {
                    format!("status={}", item.status)
                } else {
                    match item.status.as_str() {
                        "INTENT" => "待确认".to_string(),
                        "REVOKE_INTENT" => "撤销待确认".to_string(),
                        _ => "记录待核对".to_string(),
                    }
                };
                let pill = render_pill(ui, &status_pill, p.danger_fg, p.danger_bg, p.danger_fg);
                if show_tech {
                    pill.on_hover_text(&item.code);
                }
                ui.label(
                    RichText::new(&item.recovery_command)
                        .monospace()
                        .size(FONT_META)
                        .color(p.text_primary),
                );
                if btn_tonal(
                    ui,
                    p,
                    &translate(locale, "gui.guard.recovery.copy_cmd", &[]),
                    true,
                )
                .clicked()
                {
                    ui.ctx().copy_text(item.recovery_command.clone());
                    view.recovery_feedback = Some(translate(locale, "gui.guard.modal.copied", &[]));
                }
            });
        }
        ui.add_space(SPACE_1);
        ui.label(
            RichText::new(translate(locale, "gui.guard.recovery.foreign_hint", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        );
        ui.add_space(SPACE_1);
        egui::CollapsingHeader::new(
            RichText::new(translate(locale, "gui.advanced.toggle", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        )
        .id_salt("pending_audit_banner_tech")
        .open(show_tech.then_some(true))
        .show(ui, |ui| {
            for item in &audits {
                ui.monospace(format!(
                    "code={} · status={} · tag={} · foreign_guard=UNBLOCK_FOREIGN_RULE",
                    item.code, item.status, item.rule_tag
                ));
            }
        });
        if let Some(msg) = view.recovery_feedback.as_deref() {
            ui.add_space(SPACE_1);
            ui.label(RichText::new(msg).size(FONT_META).color(p.info_fg));
        }
    });
    if should_refresh {
        view.refresh_pending_audits();
    }
}

/// 渲染处置中心主视图。
#[allow(clippy::too_many_arguments)]
pub fn render(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    view: &mut GuardViewState,
    resolved_link_report: Option<&LinkReport>,
    show_ip: bool,
    theme: ThemeMode,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);
    let locale = state.current_locale();

    // 0. 若存在未完成的本机阻断意图审计记录，展示克制、非弹窗阻塞的只读恢复横幅
    view.ensure_pending_audits_scanned();
    if !view.pending_audits.is_empty() {
        render_pending_audit_banner(ui, view, locale, p, show_tech);
        ui.add_space(SPACE_4);
    }

    // 1. 顶部定位说明 + 阻断与规则导出入口 + 主动选择拟屏蔽服务（含正常服务，如 Google One）
    render_purpose_and_selector_card(ui, registry, state, view, theme, p, actions);
    ui.add_space(SPACE_4);

    // 2. 链路体检四列只读视图（全中文人话 + 可折叠技术字段）
    render_stage4_linkcheck_card(
        ui,
        registry,
        state,
        view,
        resolved_link_report,
        show_ip,
        p,
        show_tech,
        actions,
    );
    ui.add_space(SPACE_4);

    // 3 & 4：底部双栏 16px 间距（「隔离预演与误伤分析」与「安全审计与只读回滚说明」）
    if ui.available_width() >= 860.0 {
        spaced_columns(ui, 2, SPACE_4, |col_ui, idx| {
            if idx == 0 {
                render_active_plans_section(col_ui, state, view, p, show_tech, actions);
            } else {
                render_stage4_guard_audit_and_rollback_card(
                    col_ui, registry, state, view, p, show_tech,
                );
            }
        });
    } else {
        render_active_plans_section(ui, state, view, p, show_tech, actions);
        ui.add_space(SPACE_4);
        render_stage4_guard_audit_and_rollback_card(ui, registry, state, view, p, show_tech);
    }
}

/// 渲染阻断/规则导出三类模态框（在 `CentralPanel` 外层渲染以保持半透明遮罩与居中）。
pub fn render_modals(
    ctx: &egui::Context,
    registry: &Registry,
    state: &AppState,
    view: &mut GuardViewState,
    theme: ThemeMode,
    show_tech: bool,
) {
    if !sysblock::SYSTEM_BLOCK_ENABLED {
        view.sysblock_modal_open = false;
    }
    if !view.any_modal_open() {
        return;
    }
    let p = Palette::for_theme(theme);
    let locale = state.current_locale();
    let plan = current_guard_plan(registry, state, view);
    let forensic_pause = state
        .active_run()
        .and_then(|run| run.report.as_ref())
        .and_then(forensics::guard_pause_from_report);

    if view.export_modal_open {
        render_rule_export_modal(
            ctx,
            registry,
            &plan,
            forensic_pause.as_deref(),
            view,
            locale,
            p,
            show_tech,
        );
    } else if view.sysblock_modal_open {
        render_sysblock_modal(ctx, registry, &plan, view, locale, p);
    } else if view.full_modal_open {
        render_full_replace_modal(
            ctx,
            registry,
            &plan,
            forensic_pause.as_deref(),
            view,
            locale,
            p,
            show_tech,
        );
    }
}

fn render_blocking_export_hub_inline(
    ui: &mut Ui,
    state: &AppState,
    view: &mut GuardViewState,
    p: Palette,
) {
    let locale = state.current_locale();
    subtle_well_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
        .show(ui, |ui| {
            let hub_w = ui.available_width();
            ui.set_width(hub_w);
            let stack_hub = hub_w < 520.0;
            let sysblock_supported = sysblock::Platform::current().is_some();
            if stack_hub {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                    ui.label(
                        RichText::new(translate(locale, "gui.guard.block_hub.title", &[]))
                            .strong()
                            .size(FONT_META)
                            .color(p.text_primary),
                    );
                    if !sysblock::SYSTEM_BLOCK_ENABLED {
                        render_pill(
                            ui,
                            &translate(locale, "gui.guard.sysblock_beta_note", &[]),
                            p.text_muted,
                            p.card,
                            p.border,
                        );
                    }
                });
                ui.add_space(SPACE_2);
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                if !stack_hub {
                    ui.label(
                        RichText::new(translate(locale, "gui.guard.block_hub.title", &[]))
                            .strong()
                            .size(FONT_META)
                            .color(p.text_primary),
                    );
                }
                // 主方案 A（主按钮）：导出规则（推荐）—— 不污染式 rule-provider
                if btn_filled(
                    ui,
                    p,
                    &translate(locale, "gui.guard.btn.export_rules", &[]),
                    true,
                )
                .clicked()
                {
                    view.export_feedback = None;
                    view.export_modal_open = true;
                    view.sysblock_modal_open = false;
                    view.full_modal_open = false;
                }

                // 主方案 B（双路径互补）：测试版不开放系统级阻断（T-9-00 裁决项 2）
                if sysblock::SYSTEM_BLOCK_ENABLED {
                    if btn_tonal(
                        ui,
                        p,
                        &translate(locale, "gui.guard.btn.sysblock", &[]),
                        sysblock_supported,
                    )
                    .on_disabled_hover_text(translate(
                        locale,
                        "gui.guard.modal.sysblock_unsupported",
                        &[],
                    ))
                    .clicked()
                    {
                        view.resolved_preview = None;
                        view.sysblock_feedback = None;
                        view.sysblock_modal_open = true;
                        view.export_modal_open = false;
                        view.full_modal_open = false;
                    }
                } else if !stack_hub {
                    render_pill(
                        ui,
                        &translate(locale, "gui.guard.sysblock_beta_note", &[]),
                        p.text_muted,
                        p.card,
                        p.border,
                    );
                }

                // 次级菜单：第二备选「完整规则（整包替换，需谨慎）」仅收纳于此，绝不放推荐位
                let more_id = ui.make_persistent_id("guard_more_export_menu");
                let more_resp = btn_outlined(
                    ui,
                    p,
                    &translate(locale, "gui.guard.btn.more_export", &[]),
                    true,
                );
                if more_resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(more_id));
                }
                egui::popup_below_widget(
                    ui,
                    more_id,
                    &more_resp,
                    egui::PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        ui.set_min_width(240.0);
                        if ui
                            .button(
                                RichText::new(translate(locale, "gui.guard.btn.full_replace", &[]))
                                    .color(p.warning_fg),
                            )
                            .clicked()
                        {
                            view.full_ack = false;
                            view.full_feedback = None;
                            view.full_modal_open = true;
                            view.export_modal_open = false;
                            view.sysblock_modal_open = false;
                            ui.memory_mut(|m| m.close_popup());
                        }
                    },
                );

                // 帮助与进阶文档链接（GitHub 仓库文档；第三备选绝不进软件 UI）
                if !stack_hub {
                    ui.hyperlink_to(
                        RichText::new(translate(locale, "gui.guard.docs_link", &[]))
                            .size(FONT_META)
                            .color(p.accent),
                        SELF_BLOCK_DOCS_URL,
                    );
                }
            });
            if stack_hub {
                ui.add_space(SPACE_1);
                ui.hyperlink_to(
                    RichText::new(translate(locale, "gui.guard.docs_link", &[]))
                        .size(FONT_META)
                        .color(p.accent),
                    SELF_BLOCK_DOCS_URL,
                );
            }
        });
}

fn current_guard_plan(registry: &Registry, state: &AppState, view: &GuardViewState) -> GuardPlan {
    if view.selected_services.is_empty() {
        if let Some(plan) = state.active_run().and_then(|run| run.plans.last()) {
            return plan.clone();
        }
    }
    let (source_run_id, targets) = build_preview_targets(state, view);
    let results: Vec<parallaxe_core::contracts::ResultRecord> = state
        .active_run()
        .map(|r| r.ordered_results().into_iter().cloned().collect())
        .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let source_interrupted = state.active_run().and_then(|r| r.termination())
        != Some(parallaxe_core::Termination::Complete);
    guard::plan_preview(
        registry,
        GuardRequest {
            run_id: &format!("preview-{source_run_id}"),
            source_run_id: Some(&source_run_id),
            targets: &targets,
            results: &results,
            recorded_at: parallaxe_core::state::format_time(now),
            as_of: now.date(),
            source_interrupted,
        },
    )
    .plan
}

fn format_label(fmt: RuleFormat, mode: ExportMode, locale: Locale) -> &'static str {
    match (fmt, mode, locale) {
        (RuleFormat::Clash, ExportMode::Provider, _) => "Clash / mihomo (rule-provider)",
        (RuleFormat::Clash, ExportMode::Full, Locale::ZhCn) => "Clash / mihomo (完整规则)",
        (RuleFormat::Clash, ExportMode::Full, Locale::EnUs) => "Clash / mihomo (Full Rules)",
        (RuleFormat::SingBox, ExportMode::Provider, _) => "sing-box (local rule_set)",
        (RuleFormat::SingBox, ExportMode::Full, Locale::ZhCn) => "sing-box (完整配置)",
        (RuleFormat::SingBox, ExportMode::Full, Locale::EnUs) => "sing-box (Full Config)",
        (RuleFormat::DomainList, _, Locale::ZhCn) => "纯域名清单 (domainlist)",
        (RuleFormat::DomainList, _, Locale::EnUs) => "Domain List (FQDN)",
    }
}

#[derive(Clone, Copy, Debug)]
struct ResponsiveModalMetrics {
    narrow: bool,
    inner_w: f32,
    inner_margin: f32,
    max_body_h: f32,
    offset_y: f32,
}

impl ResponsiveModalMetrics {
    fn compute(ctx: &egui::Context) -> Self {
        let screen = ctx.screen_rect();
        let safe = crate::layout::SafeAreaInsets::for_platform();
        let narrow = screen.width() < 520.0;
        let h_margin = if screen.width() < 480.0 { 12.0 } else { 24.0 };
        let inner_margin = if screen.width() < 480.0 {
            SPACE_3
        } else {
            SPACE_5
        };
        let inner_w =
            (screen.width() - h_margin * 2.0 - inner_margin * 2.0 - 4.0).clamp(220.0, 640.0);
        let safe_top = safe.top + 12.0;
        let safe_bottom = safe.bottom + 12.0;
        let avail_h = (screen.height() - safe_top - safe_bottom).max(180.0);
        let max_win_h = (avail_h - inner_margin * 2.0 - 4.0).max(150.0);
        let offset_y = (safe_top - safe_bottom) * 0.5;
        let max_body_h = (max_win_h - 156.0).max(88.0);
        ResponsiveModalMetrics {
            narrow,
            inner_w,
            inner_margin,
            max_body_h,
            offset_y,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn show_responsive_modal(
    ctx: &egui::Context,
    id: &str,
    scroll_id: &str,
    title: &str,
    border_color: egui::Color32,
    open: &mut bool,
    locale: Locale,
    p: Palette,
    body: impl FnOnce(&mut Ui, ResponsiveModalMetrics),
) {
    let m = ResponsiveModalMetrics::compute(ctx);
    let mut keep_open = *open;
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, m.offset_y])
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::window(&ctx.style())
                .fill(p.card)
                .stroke(Stroke::new(1.0_f32, border_color))
                .rounding(Rounding::same(RADIUS_LG))
                .inner_margin(Margin::same(m.inner_margin))
                .show(ui, |ui| {
                    ui.set_width(m.inner_w);
                    ui.set_max_width(m.inner_w);

                    // 自定义响应式标题栏：窄屏自动折行，绝不撑破手机竖屏宽度
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SPACE_2;
                        let title_w = (m.inner_w - 34.0).max(150.0);
                        ui.allocate_ui_with_layout(
                            vec2(title_w, 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(title)
                                            .size(if m.narrow {
                                                FONT_BODY + 2.0
                                            } else {
                                                FONT_TITLE
                                            })
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
                                keep_open = false;
                            }
                        });
                    });
                    ui.add_space(SPACE_2);
                    ui.separator();
                    ui.add_space(SPACE_2);

                    ScrollArea::vertical()
                        .id_salt(scroll_id)
                        .max_height(m.max_body_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.set_width(m.inner_w);
                            ui.set_max_width(m.inner_w);
                            body(ui, m);
                        });

                    ui.add_space(SPACE_2);
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if btn_outlined(
                                ui,
                                p,
                                &translate(locale, "gui.guard.modal.close", &[]),
                                true,
                            )
                            .clicked()
                            {
                                keep_open = false;
                            }
                        });
                    });
                });
        });
    *open = keep_open;
}

#[allow(clippy::too_many_arguments)]
fn render_rule_export_modal(
    ctx: &egui::Context,
    registry: &Registry,
    plan: &GuardPlan,
    forensic_pause: Option<&str>,
    view: &mut GuardViewState,
    locale: Locale,
    p: Palette,
    show_tech: bool,
) {
    render_modal_scrim(ctx, "guard_export_modal_scrim", p);
    let mut open = view.export_modal_open;
    let title = translate(locale, "gui.guard.modal.export_title", &[]);
    show_responsive_modal(
        ctx,
        "guard_rule_export_modal",
        "guard_rule_export_modal_scroll",
        &title,
        p.border,
        &mut open,
        locale,
        p,
        |ui, m| {
            ui.add(
                egui::Label::new(
                    RichText::new(translate(locale, "gui.guard.modal.export_desc", &[]))
                        .size(FONT_BODY)
                        .color(p.text_secondary),
                )
                .wrap(),
            );
            ui.add_space(SPACE_3);

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                ui.label(
                    RichText::new(translate(locale, "gui.guard.modal.format_label", &[]))
                        .size(FONT_META)
                        .strong()
                        .color(p.text_primary),
                );
                for fmt in RuleFormat::ALL {
                    if ui
                        .selectable_label(
                            view.export_format == fmt,
                            format_label(fmt, ExportMode::Provider, locale),
                        )
                        .clicked()
                    {
                        view.export_format = fmt;
                        view.export_feedback = None;
                    }
                }
            });
            ui.add_space(SPACE_3);

            let generated_at = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
            let export_res = guard_export::export_rules(ExportRequest {
                registry,
                plan,
                format: view.export_format,
                mode: ExportMode::Provider,
                generated_at,
                forensic_pause: forensic_pause.map(str::to_string),
            });

            let picker = view.folder_picker_override;
            match export_res {
                Ok(bundle) => {
                    render_export_bundle_body(
                        ctx,
                        ui,
                        &bundle,
                        &mut view.export_selected_dir,
                        &mut view.export_feedback,
                        picker,
                        m,
                        locale,
                        p,
                    );
                }
                Err(refusal) => {
                    render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
                        let title_text = if show_tech {
                            format!(
                                "{} ({})",
                                translate(locale, "gui.guard.modal.refused_title", &[]),
                                refusal.code
                            )
                        } else {
                            translate(locale, "gui.guard.modal.refused_title", &[])
                        };
                        let resp = ui.add(
                            egui::Label::new(
                                RichText::new(title_text)
                                    .strong()
                                    .size(FONT_BODY)
                                    .color(p.warning_fg),
                            )
                            .wrap(),
                        );
                        if show_tech {
                            resp.on_hover_text(refusal.code);
                        }
                        ui.add_space(SPACE_1);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&refusal.detail)
                                    .size(FONT_META)
                                    .color(p.text_primary),
                            )
                            .wrap(),
                        );
                    });
                    ui.add_space(SPACE_2);
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "CLI: parallaxe guard export --target <id> --format {} --mode provider --out-dir <dir>",
                                view.export_format.as_str()
                            ))
                            .monospace()
                            .size(FONT_META)
                            .color(p.text_muted),
                        )
                        .wrap(),
                    );
                }
            }
        },
    );
    view.export_modal_open = open;
}

#[allow(clippy::too_many_arguments)]
fn render_full_replace_modal(
    ctx: &egui::Context,
    registry: &Registry,
    plan: &GuardPlan,
    forensic_pause: Option<&str>,
    view: &mut GuardViewState,
    locale: Locale,
    p: Palette,
    show_tech: bool,
) {
    render_modal_scrim(ctx, "guard_full_modal_scrim", p);
    let mut open = view.full_modal_open;
    let title = translate(locale, "gui.guard.modal.full_title", &[]);
    show_responsive_modal(
        ctx,
        "guard_full_replace_modal",
        "guard_full_replace_modal_scroll",
        &title,
        p.danger_fg,
        &mut open,
        locale,
        p,
        |ui, m| {
            // 强警示：原样展示 core 的 FULL_MODE_WARNING
            render_admonition(ui, p.danger_bg, p.danger_fg, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(FULL_MODE_WARNING)
                            .strong()
                            .size(FONT_BODY)
                            .color(p.danger_fg),
                    )
                    .wrap(),
                );
            });
            ui.add_space(SPACE_3);

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                ui.label(
                    RichText::new(translate(locale, "gui.guard.modal.format_label", &[]))
                        .size(FONT_META)
                        .strong()
                        .color(p.text_primary),
                );
                for fmt in [RuleFormat::Clash, RuleFormat::SingBox] {
                    if ui
                        .selectable_label(
                            view.full_format == fmt,
                            format_label(fmt, ExportMode::Full, locale),
                        )
                        .clicked()
                    {
                        view.full_format = fmt;
                        view.full_feedback = None;
                    }
                }
            });
            ui.add_space(SPACE_2);

            ui.checkbox(
                &mut view.full_ack,
                RichText::new(translate(locale, "gui.guard.modal.full_ack", &[]))
                    .size(FONT_BODY)
                    .color(p.text_primary),
            );
            ui.add_space(SPACE_3);

            if view.full_ack {
                let generated_at =
                    parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
                let res = guard_export::export_rules(ExportRequest {
                    registry,
                    plan,
                    format: view.full_format,
                    mode: ExportMode::Full,
                    generated_at,
                    forensic_pause: forensic_pause.map(str::to_string),
                });
                let picker = view.folder_picker_override;
                match res {
                    Ok(bundle) => {
                        render_export_bundle_body(
                            ctx,
                            ui,
                            &bundle,
                            &mut view.full_selected_dir,
                            &mut view.full_feedback,
                            picker,
                            m,
                            locale,
                            p,
                        );
                    }
                    Err(refusal) => {
                        render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
                            let title_text = if show_tech {
                                format!(
                                    "{} ({})",
                                    translate(locale, "gui.guard.modal.refused_title", &[]),
                                    refusal.code
                                )
                            } else {
                                translate(locale, "gui.guard.modal.refused_title", &[])
                            };
                            let resp = ui.add(
                                egui::Label::new(
                                    RichText::new(title_text)
                                        .strong()
                                        .size(FONT_BODY)
                                        .color(p.warning_fg),
                                )
                                .wrap(),
                            );
                            if show_tech {
                                resp.on_hover_text(refusal.code);
                            }
                            ui.add_space(SPACE_1);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&refusal.detail)
                                        .size(FONT_META)
                                        .color(p.text_primary),
                                )
                                .wrap(),
                            );
                        });
                    }
                }
            } else {
                subtle_well_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(translate(locale, "gui.guard.modal.full_generate", &[]))
                                .size(FONT_META)
                                .color(p.text_muted),
                        )
                        .wrap(),
                    );
                });
            }
        },
    );
    view.full_modal_open = open;
}

#[allow(clippy::too_many_arguments)]
fn render_export_bundle_body(
    ctx: &egui::Context,
    ui: &mut Ui,
    bundle: &guard_export::ExportBundle,
    selected_dir: &mut Option<PathBuf>,
    feedback: &mut Option<String>,
    picker_override: Option<FolderPickerFn>,
    m: ResponsiveModalMetrics,
    locale: Locale,
    p: Palette,
) {
    if !bundle.warning_codes.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            for code in &bundle.warning_codes {
                render_pill(ui, code, p.warning_fg, p.warning_bg, p.warning_fg);
            }
        });
        ui.add_space(SPACE_1);
    }
    for warning in &bundle.warnings {
        ui.add(
            egui::Label::new(
                RichText::new(format!("• {warning}"))
                    .size(FONT_META)
                    .color(p.warning_fg),
            )
            .wrap(),
        );
    }
    ui.add_space(SPACE_2);

    let combined: String = bundle
        .artifacts
        .iter()
        .map(|a| format!("# === {} ===\n{}", a.name, a.content))
        .collect::<Vec<_>>()
        .join("\n");

    ui.add(
        egui::Label::new(
            RichText::new(translate(locale, "gui.guard.modal.export_no_silent", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        )
        .wrap(),
    );
    ui.add_space(SPACE_2);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
        if btn_tonal(
            ui,
            p,
            &translate(locale, "gui.guard.modal.copy_all", &[]),
            true,
        )
        .clicked()
        {
            ctx.copy_text(combined.clone());
            *feedback = Some(translate(locale, "gui.guard.modal.copied", &[]));
        }
        if btn_outlined(
            ui,
            p,
            &translate(locale, "gui.guard.modal.save_btn", &[]),
            true,
        )
        .clicked()
        {
            let prompt = translate(locale, "gui.guard.modal.picker_prompt", &[]);
            let pick_result = match picker_override {
                Some(override_fn) => override_fn(&prompt),
                None => crate::folder_picker::pick_export_folder(&prompt),
            };
            match pick_result {
                Ok(Some(dir)) => match write_export_bundle_to_chosen_dir(&dir, bundle) {
                    Ok(n) => {
                        let n_str = n.to_string();
                        let dir_str = dir.display().to_string();
                        *selected_dir = Some(dir);
                        *feedback = Some(translate(
                            locale,
                            "gui.guard.modal.export_written",
                            &[&n_str, &dir_str],
                        ));
                    }
                    Err(err) => {
                        *feedback = Some(err);
                    }
                },
                Ok(None) => {
                    *feedback = Some(translate(locale, "gui.guard.modal.export_cancelled", &[]));
                }
                Err(err) => {
                    *feedback = Some(err);
                }
            }
        }
    });
    if let Some(dir) = selected_dir.as_ref() {
        ui.add_space(SPACE_1);
        ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "{} {}",
                    translate(locale, "gui.guard.modal.save_dir", &[]),
                    dir.display()
                ))
                .monospace()
                .size(FONT_META)
                .color(p.text_muted),
            )
            .wrap(),
        );
    }
    if let Some(msg) = feedback.as_deref() {
        ui.add_space(SPACE_1);
        ui.add(egui::Label::new(RichText::new(msg).size(FONT_META).color(p.info_fg)).wrap());
    }
    ui.add_space(SPACE_2);

    ScrollArea::vertical()
        .id_salt("guard_export_artifacts_scroll")
        .max_height(if m.narrow { 140.0 } else { 210.0 })
        .show(ui, |ui| {
            let mut preview = combined.clone();
            ui.add(
                egui::TextEdit::multiline(&mut preview)
                    .code_editor()
                    .desired_rows(6)
                    .desired_width((m.inner_w - 16.0).max(180.0)),
            );
        });
}

/// Writes exported rule artifacts strictly into an existing directory explicitly selected by the user.
/// Never silently creates arbitrary fallback paths.
fn write_export_bundle_to_chosen_dir(
    dir: &std::path::Path,
    bundle: &guard_export::ExportBundle,
) -> Result<usize, String> {
    if !dir.is_dir() {
        return Err(format!("所选目录不存在或不可访问：{}", dir.display()));
    }
    let mut written = 0;
    for artifact in &bundle.artifacts {
        let target = dir.join(&artifact.name);
        std::fs::write(&target, &artifact.content)
            .map_err(|e| format!("写入 {} 失败：{e}", artifact.name))?;
        written += 1;
    }
    Ok(written)
}

fn render_sysblock_modal(
    ctx: &egui::Context,
    registry: &Registry,
    plan: &GuardPlan,
    view: &mut GuardViewState,
    locale: Locale,
    p: Palette,
) {
    render_modal_scrim(ctx, "guard_sysblock_modal_scrim", p);
    let mut open = view.sysblock_modal_open;
    let title = translate(locale, "gui.guard.modal.sysblock_title", &[]);
    show_responsive_modal(
        ctx,
        "guard_sysblock_modal",
        "guard_sysblock_modal_scroll",
        &title,
        p.border,
        &mut open,
        locale,
        p,
        |ui, _m| {
            ui.add(
                egui::Label::new(
                    RichText::new(translate(locale, "gui.guard.modal.sysblock_desc", &[]))
                        .size(FONT_BODY)
                        .color(p.text_secondary),
                )
                .wrap(),
            );
            ui.add_space(SPACE_3);

            let Some(platform) = sysblock::Platform::current() else {
                render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(translate(
                                locale,
                                "gui.guard.modal.sysblock_unsupported",
                                &[],
                            ))
                            .color(p.warning_fg),
                        )
                        .wrap(),
                    );
                });
                return;
            };

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                ui.label(
                    RichText::new(translate(locale, "gui.guard.modal.sysblock_tag", &[]))
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut view.sysblock_tag_input).desired_width(160.0),
                );
                render_pill(
                    ui,
                    &format!("platform={}", platform.as_str()),
                    p.info_fg,
                    p.info_bg,
                    p.info_fg,
                );
            });
            ui.add_space(SPACE_2);

            let tag = view.sysblock_tag_input.trim();
            let now = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
            let sys_res = sysblock::candidate_hosts(registry, plan, tag).and_then(|hosts| {
                let key = format!(
                    "{}|{:?}|{}|{:?}",
                    plan.plan_id, plan.source_run_id, tag, hosts
                );
                ui.label(
                    RichText::new(translate(
                        locale,
                        "gui.guard.modal.sysblock_candidates_label",
                        &[],
                    ))
                    .size(FONT_META)
                    .color(p.text_secondary),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                    for host in &hosts {
                        render_pill(ui, host, p.text_primary, p.card_alt, p.border);
                    }
                });
                ui.add_space(SPACE_1);
                if btn_tonal(
                    ui,
                    p,
                    &translate(locale, "gui.guard.modal.sysblock_resolve_btn", &[]),
                    true,
                )
                .clicked()
                {
                    view.resolved_preview = Some((
                        key.clone(),
                        sysblock::resolve_plan(registry, plan, platform, now, tag),
                    ));
                }
                ui.add_space(SPACE_2);
                view.resolved_preview
                    .as_ref()
                    .filter(|(saved, _)| saved == &key)
                    .map(|(_, res)| res.clone())
                    .unwrap_or_else(|| {
                        Err(sysblock::SysBlockError {
                            code: "SYSBLOCK_RESOLUTION_REQUIRED",
                            detail: "解析后可预览精确地址。".into(),
                        })
                    })
            });
            let unblock_res = sysblock::unblock_commands(platform, tag);

            let can_apply = match &sys_res {
                Ok(sb_plan) => {
                    subtle_well_frame(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(
                                "请先将本次源报告保存为 report.json，再到管理员终端运行复制的命令。",
                            )
                            .size(FONT_META)
                            .color(p.text_secondary),
                        );
                        ui.add_space(SPACE_2);
                        egui::Grid::new("sysblock_resolved_table")
                            .striped(true)
                            .num_columns(3)
                            .spacing(vec2(SPACE_4, SPACE_1))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(translate(
                                        locale,
                                        "gui.guard.modal.sysblock_col_fqdn",
                                        &[],
                                    ))
                                    .strong()
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new(translate(
                                        locale,
                                        "gui.guard.modal.sysblock_col_ip",
                                        &[],
                                    ))
                                    .strong()
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                                );
                                ui.label(
                                    RichText::new(translate(
                                        locale,
                                        "gui.guard.modal.sysblock_col_audit",
                                        &[],
                                    ))
                                    .strong()
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                                );
                                ui.end_row();
                                for entry in &sb_plan.entries {
                                    ui.label(
                                        RichText::new(&entry.fqdn)
                                            .monospace()
                                            .size(FONT_META)
                                            .color(p.text_primary),
                                    );
                                    ui.label(
                                        RichText::new(format!("{} ({})", entry.ip, entry.family))
                                            .monospace()
                                            .size(FONT_META)
                                            .color(p.text_primary),
                                    );
                                    render_pill(
                                        ui,
                                        &translate(
                                            locale,
                                            "gui.guard.modal.sysblock_audit_pass",
                                            &[],
                                        ),
                                        p.ok_fg,
                                        p.ok_bg,
                                        p.ok_fg,
                                    );
                                    ui.end_row();
                                }
                            });
                        ui.add_space(SPACE_2);
                        kv_row(ui, p, "规则标识", 76.0, &sb_plan.rule_tag, true);
                        kv_row(ui, p, "作用范围", 76.0, &sb_plan.effect_scope, true);
                        kv_row(
                            ui,
                            p,
                            "零残留回滚",
                            76.0,
                            if sb_plan.zero_residue {
                                "true"
                            } else {
                                "false"
                            },
                            true,
                        );
                    });
                    true
                }
                Err(err) => {
                    render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} ({})",
                                translate(locale, "gui.guard.modal.refused_title", &[]),
                                err.code
                            ))
                            .strong()
                            .size(FONT_BODY)
                            .color(p.warning_fg),
                        );
                        ui.add_space(SPACE_1);
                        ui.label(
                            RichText::new(&err.detail)
                                .size(FONT_META)
                                .color(p.text_primary),
                        );
                    });
                    false
                }
            };

            ui.add_space(SPACE_2);
            ui.checkbox(
                &mut view.sysblock_ack,
                RichText::new(translate(locale, "gui.guard.modal.sysblock_ack", &[]))
                    .size(FONT_META)
                    .color(p.text_primary),
            );
            ui.add_space(SPACE_2);

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                let apply_enabled = can_apply && view.sysblock_ack;
                if btn_filled(
                    ui,
                    p,
                    &translate(locale, "gui.guard.modal.sysblock_apply_cmd", &[]),
                    apply_enabled,
                )
                .clicked()
                {
                    if let Ok(sb_plan) = &sys_res {
                        ctx.copy_text(sb_plan.apply_commands.join("\n"));
                        view.sysblock_feedback =
                            Some(translate(locale, "gui.guard.modal.copied", &[]));
                    }
                }
                let unblock_enabled = unblock_res.is_ok();
                if btn_danger_tonal(
                    ui,
                    p,
                    &translate(locale, "gui.guard.modal.sysblock_unblock_cmd", &[]),
                    unblock_enabled,
                )
                .clicked()
                {
                    ctx.copy_text(format!("parallaxe guard unblock --rule-tag {tag} --apply"));
                    view.sysblock_feedback = Some(translate(locale, "gui.guard.modal.copied", &[]));
                }
            });

            if let Ok(cmds) = &unblock_res {
                ui.add_space(SPACE_2);
                subtle_well_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(format!("Rollback: {}", cmds.join(" ; ")))
                            .monospace()
                            .size(FONT_META)
                            .color(p.text_secondary),
                    );
                });
            }

            ui.add_space(SPACE_2);
            ui.label(
                RichText::new(translate(
                    locale,
                    "gui.guard.modal.sysblock_foreign_note",
                    &[],
                ))
                .size(FONT_META)
                .color(p.text_secondary),
            );

            if let Some(msg) = view.sysblock_feedback.as_deref() {
                ui.add_space(SPACE_1);
                ui.add(
                    egui::Label::new(RichText::new(msg).size(FONT_META).color(p.info_fg)).wrap(),
                );
            }
        },
    );
    view.sysblock_modal_open = open;
}

fn render_purpose_and_selector_card(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    view: &mut GuardViewState,
    theme: ThemeMode,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let locale = state.current_locale();
    let card_outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let card_inner_w = (card_outer_w - SPACE_4 * 2.0).max(200.0);
        ui.set_width(card_inner_w);
        ui.set_max_width(card_inner_w);

        let (source_run_id, effective_targets) = build_preview_targets(state, view);

        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(translate(locale, "gui.guard.title", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_1);
                ui.label(
                    RichText::new(translate(locale, "gui.guard.subtitle", &[]))
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
            });
        });

        ui.add_space(SPACE_2);
        let compact_card = ui.available_width() < 880.0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            render_pill(
                ui,
                &translate(locale, "gui.guard.pill.local_only", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            );
            render_pill(
                ui,
                &translate(locale, "gui.guard.pill.no_remote", &[]),
                p.text_secondary,
                p.card_alt,
                p.border,
            );
            render_pill(
                ui,
                &translate(locale, "gui.guard.pill.readonly", &[]),
                p.ok_fg,
                p.ok_bg,
                p.ok_fg,
            );

            if !compact_card {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_2;
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.guard.btn.load_demo", &[]),
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::RunScenario(
                            crate::commands::OfflineScenario::BooksAndSharedCdnGuard,
                        ));
                    }
                    let eff_cnt = effective_targets.len().to_string();
                    let preview_label = translate(locale, "gui.guard.btn.preview", &[&eff_cnt]);
                    if btn_tonal(ui, p, &preview_label, !effective_targets.is_empty()).clicked() {
                        actions.push(UiAction::RequestGuardPreview {
                            source_run_id: source_run_id.clone(),
                            targets: effective_targets.clone(),
                        });
                    }
                });
            }
        });
        if compact_card {
            ui.add_space(SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                let eff_cnt = effective_targets.len().to_string();
                let preview_label = translate(locale, "gui.guard.btn.preview", &[&eff_cnt]);
                if btn_tonal(ui, p, &preview_label, !effective_targets.is_empty()).clicked() {
                    actions.push(UiAction::RequestGuardPreview {
                        source_run_id: source_run_id.clone(),
                        targets: effective_targets.clone(),
                    });
                }
                if btn_outlined(
                    ui,
                    p,
                    &translate(locale, "gui.guard.btn.load_demo", &[]),
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::RunScenario(
                        crate::commands::OfflineScenario::BooksAndSharedCdnGuard,
                    ));
                }
            });
        }

        ui.add_space(SPACE_2);
        // 双路径阻断与规则导出入口条（紧凑嵌入顶部卡，避免挤占下方四列链路与审计视野）
        render_blocking_export_hub_inline(ui, state, view, p);

        ui.add_space(SPACE_2);
        // 主动选择拟屏蔽的服务（支持任意正常服务，例如 Google One！）
        subtle_well_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                ui.label(
                    RichText::new(translate(locale, "gui.guard.selector.label", &[]))
                        .strong()
                        .size(FONT_META)
                        .color(p.text_primary),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut view.service_search)
                        .desired_width(248.0)
                        .hint_text(translate(locale, "gui.guard.selector.search_hint", &[])),
                );
                if !view.selected_services.is_empty() {
                    if let Some(first_id) = view.selected_services.iter().next().cloned() {
                        let first_name = service_name(&first_id);
                        let btn_text = if view.selected_services.len() == 1 {
                            translate(locale, "gui.guard.selector.proactive_one", &[first_name])
                        } else {
                            let n_str = view.selected_services.len().to_string();
                            translate(
                                locale,
                                "gui.guard.selector.proactive_multi",
                                &[first_name, &n_str],
                            )
                        };
                        if btn_tonal(ui, p, &btn_text, true).clicked() {
                            actions.push(UiAction::OpenPlan(first_id));
                        }
                    }
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.guard.selector.clear", &[]),
                        true,
                    )
                    .clicked()
                    {
                        view.selected_services.clear();
                    }
                }
            });

            ui.add_space(SPACE_2);
            // 构造候选芯片列表：常用核心服务（显式包含 Google One / Google Books / Gemini / Play / Gmail / Drive）+ 当前运行结果 + 搜索匹配项
            let mut candidate_ids: Vec<String> = Vec::new();
            let search = view.service_search.trim().to_ascii_lowercase();
            if search.is_empty() {
                if let Some(run) = state.active_run() {
                    for r in run.ordered_results().into_iter().take(6) {
                        if !candidate_ids.contains(&r.target) {
                            candidate_ids.push(r.target.clone());
                        }
                    }
                }
                for preset in [
                    "google.one",
                    "google.books",
                    "google.gemini",
                    "google.play",
                    "google.gmail",
                    "google.drive",
                ] {
                    let s = preset.to_string();
                    if registry.get(preset).is_some() && !candidate_ids.contains(&s) {
                        candidate_ids.push(s);
                    }
                }
                for sel in &view.selected_services {
                    if !candidate_ids.contains(sel) {
                        candidate_ids.push(sel.clone());
                    }
                }
            } else {
                for entry in registry.entries() {
                    let id = entry.id();
                    let name_low = service_name(id).to_ascii_lowercase();
                    let host_low = entry.host().to_ascii_lowercase();
                    if id.to_ascii_lowercase().contains(&search)
                        || name_low.contains(&search)
                        || host_low.contains(&search)
                    {
                        candidate_ids.push(id.to_string());
                        if candidate_ids.len() >= 10 {
                            break;
                        }
                    }
                }
            }

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                for id in candidate_ids {
                    let checked = view.selected_services.contains(&id);
                    let run_state = state
                        .active_run()
                        .and_then(|r| r.result_for(&id))
                        .map(|r| r.state);
                    let state_suffix = match run_state {
                        Some(State::Ok) => translate(locale, "gui.guard.chip.ok_blockable", &[]),
                        Some(s) => state_badge(s, theme).label(locale),
                        None => {
                            if registry.get(&id).is_some_and(|t| t.raw.protected) {
                                translate(locale, "gui.guard.chip.protected_manual", &[])
                            } else {
                                translate(locale, "gui.guard.chip.blockable", &[])
                            }
                        }
                    };
                    let prefix = if checked {
                        translate(locale, "gui.guard.chip.selected", &[])
                    } else {
                        "+".to_string()
                    };
                    let chip_text = if locale == Locale::EnUs {
                        format!("{prefix} {} ({state_suffix})", service_name(&id))
                    } else {
                        format!("{prefix} {}（{state_suffix}）", service_name(&id))
                    };
                    let resp = ui.add(
                        egui::Button::new(RichText::new(chip_text).size(FONT_META).color(
                            if checked {
                                p.accent
                            } else {
                                p.text_primary
                            },
                        ))
                        .fill(if checked { p.accent_soft } else { p.card })
                        .stroke(Stroke::new(
                            1.0_f32,
                            if checked { p.accent } else { p.border },
                        ))
                        .rounding(Rounding::same(14.0))
                        .min_size(vec2(0.0, 26.0)),
                    );
                    if resp.clicked() {
                        if checked {
                            view.selected_services.remove(&id);
                        } else {
                            view.selected_services.insert(id);
                        }
                    }
                }
            });

            // 若用户主动勾选了服务（例如 Google One），即时用大白话展示其主机与静态依赖误伤分析
            if !view.selected_services.is_empty() {
                ui.add_space(SPACE_2);
                ui.separator();
                ui.add_space(SPACE_1);
                for id in &view.selected_services {
                    if let Some(target) = registry.get(id) {
                        let host = target.host();
                        let same_host_others: Vec<&str> = registry
                            .entries()
                            .iter()
                            .filter(|e| e.id() != id && e.host() == host)
                            .map(|e| service_name(e.id()))
                            .collect();
                        let dep_edges: Vec<String> = registry
                            .dependencies
                            .edges
                            .iter()
                            .filter(|edge| {
                                edge.src == format!("svc:{id}")
                                    || edge.dst == format!("svc:{id}")
                                    || edge.src == format!("host:{host}")
                                    || edge.dst == format!("host:{host}")
                            })
                            .map(|e| format!("{} → {}", e.src, e.dst))
                            .collect();
                        let impact_note = if !same_host_others.is_empty() {
                            format!(
                                "注意：与 {} 共用域名 {host}，屏蔽会连带阻断这些服务",
                                same_host_others.join("、")
                            )
                        } else if !dep_edges.is_empty() {
                            format!("静态目录登记了 {} 条关联依赖边，屏蔽前建议核对下方影响闭包", dep_edges.len())
                        } else {
                            format!("静态目录中 {host} 未发现与其他服务共用入口域名（仍需留意共享 CDN 资源）")
                        };
                        let protect_hint = if target.raw.protected {
                            "核心受保护服务（系统不自动屏蔽；如需主动屏蔽请点击上方「生成处置清单」手动配置）"
                        } else {
                            "非核心受保护服务（可预演隔离并生成处置清单）"
                        };
                        ui.label(
                            RichText::new(format!(
                                "• {}（{}）：{}；{}。",
                                service_name(id),
                                host,
                                protect_hint,
                                impact_note
                            ))
                            .size(FONT_META)
                            .color(p.text_secondary),
                        );
                    }
                }
            }
        });
    });
}

fn render_active_plans_section(
    ui: &mut Ui,
    state: &AppState,
    view: &GuardViewState,
    p: Palette,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    if let Some(run) = state.active_run() {
        if run.plans.is_empty() {
            render_empty_plan_card(ui, state, view, p, actions);
        } else {
            for (idx, plan) in run.plans.iter().enumerate() {
                if idx > 0 {
                    ui.add_space(SPACE_4);
                }
                render_guard_plan_card(ui, plan, state.current_locale(), p, show_tech);
            }
        }
    } else {
        render_empty_plan_card(ui, state, view, p, actions);
    }
}

fn render_empty_plan_card(
    ui: &mut Ui,
    state: &AppState,
    view: &GuardViewState,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let locale = state.current_locale();
    let card_outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let card_inner_w = (card_outer_w - SPACE_4 * 2.0).max(200.0);
        ui.set_width(card_inner_w);
        ui.set_max_width(card_inner_w);
        ui.label(
            RichText::new(translate(locale, "gui.guard.plan.title", &[]))
                .strong()
                .size(FONT_TITLE)
                .color(p.text_primary),
        );
        ui.add_space(SPACE_2);
        ui.label(
            RichText::new(translate(locale, "gui.guard.plan.empty_hint", &[]))
                .size(FONT_BODY)
                .color(p.text_secondary),
        );
        ui.add_space(SPACE_3);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_2;
            let (source_run_id, targets) = build_preview_targets(state, view);
            let cnt = targets.len().to_string();
            let label = translate(locale, "gui.guard.btn.preview", &[&cnt]);
            if btn_tonal(ui, p, &label, !targets.is_empty()).clicked() {
                actions.push(UiAction::RequestGuardPreview {
                    source_run_id,
                    targets,
                });
            }
            if btn_outlined(
                ui,
                p,
                &translate(locale, "gui.guard.btn.load_demo", &[]),
                true,
            )
            .clicked()
            {
                actions.push(UiAction::RunScenario(
                    crate::commands::OfflineScenario::BooksAndSharedCdnGuard,
                ));
            }
        });
    });
}

fn humanize_veto_reason(code: &str) -> &'static str {
    match code {
        "GUARD_DEFERRED_SHARED_CDN" => "涉及共享 CDN（如静态资源域名），屏蔽会误伤其他正常服务",
        "BLOCK_DEFERRED_DEPENDENCY_CONFLICT" => "存在依赖冲突，直接屏蔽会连带影响受保护的关键服务",
        "NO_SAFE_BLOCK" => "当前没有可安全自动屏蔽的独占域名（已拦截以防误伤）",
        "PROTECTED_HOST" => "属于核心受保护服务，系统拒绝自动屏蔽（如需主动屏蔽请通过处置清单手动配置）",
        "NOT_GUARD_ELIGIBLE" => "该服务目前不是高可信受限/阻断状态，系统不自动建议屏蔽（如需主动屏蔽正常服务请使用处置清单）",
        "INCONCLUSIVE" => "本次尚未获取该服务的检测结果，系统暂不自动建议屏蔽（仍可生成手动处置清单）",
        _ => "按安全保守策略暂缓自动屏蔽，请参考影响清单手动评估",
    }
}

fn render_guard_plan_card(
    ui: &mut Ui,
    plan: &parallaxe_core::contracts::GuardPlan,
    locale: Locale,
    p: Palette,
    show_tech: bool,
) {
    let card_outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let card_inner_w = (card_outer_w - SPACE_4 * 2.0).max(200.0);
        ui.set_width(card_inner_w);
        ui.set_max_width(card_inner_w);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            ui.label(
                RichText::new(translate(locale, "gui.guard.plan.title", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
            render_pill(
                ui,
                &translate(locale, "gui.guard.plan.readonly_pill", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            );
            if plan.safe_to_apply {
                render_pill(
                    ui,
                    &translate(locale, "gui.guard.plan.safe_pill", &[]),
                    p.ok_fg,
                    p.ok_bg,
                    p.ok_fg,
                );
            } else {
                render_pill(
                    ui,
                    &translate(locale, "gui.guard.plan.blocked_pill", &[]),
                    p.warning_fg,
                    p.warning_bg,
                    p.warning_fg,
                );
            }
        });

        ui.add_space(SPACE_3);
        subtle_well_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let candidates = if plan.candidates.is_empty() {
                "无符合自动屏蔽条件的服务".to_string()
            } else {
                plan.candidates
                    .iter()
                    .map(|c| service_name(c))
                    .collect::<Vec<_>>()
                    .join("、")
            };
            kv_row(ui, p, "评估目标", 76.0, &candidates, false);
            ui.add_space(SPACE_1);
            kv_row(
                ui,
                p,
                "依赖完整度",
                76.0,
                if plan.closure_complete {
                    "依赖关系完整（已闭合检查）"
                } else {
                    "依赖信息不完整（按安全保守原则拦截）"
                },
                false,
            );
            ui.add_space(SPACE_1);
            kv_row(
                ui,
                p,
                "本机改动",
                76.0,
                if plan.applied {
                    "已应用"
                } else {
                    "未修改任何本机或代理规则"
                },
                false,
            );
        });

        if let Some(primary) = &plan.primary_reason {
            ui.add_space(SPACE_3);
            render_admonition(ui, p.danger_bg, p.danger_fg, |ui| {
                ui.label(
                    RichText::new(format!("拦截原因：{}", humanize_veto_reason(primary)))
                        .strong()
                        .size(FONT_BODY)
                        .color(p.danger_fg),
                );
                if !plan.affected_shared_paths.is_empty() {
                    ui.add_space(SPACE_1);
                    ui.label(
                        RichText::new(format!(
                            "可能被误伤的共享路径：{}",
                            plan.affected_shared_paths.join("；")
                        ))
                        .size(FONT_META)
                        .color(p.text_primary),
                    );
                }
            });
        }

        if !plan.impact_closure.is_empty() {
            ui.add_space(SPACE_3);
            ui.label(
                RichText::new(format!(
                    "若屏蔽该服务，可能连带影响以下 {} 个域名/服务：",
                    plan.impact_closure.len()
                ))
                .strong()
                .size(FONT_META)
                .color(p.text_secondary),
            );
            ui.add_space(SPACE_2);
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (idx, node) in plan.impact_closure.iter().enumerate() {
                    if idx > 0 {
                        ui.add_space(SPACE_2);
                    }
                    ui.horizontal_wrapped(|ui| {
                        let role_raw = format!("{:?}", node.role);
                        let (zh_role, fg, bg) = if role_raw.contains("Affected") {
                            ("直接受影响", p.warning_fg, p.warning_bg)
                        } else {
                            ("关联服务", p.info_fg, p.info_bg)
                        };
                        render_pill(ui, zh_role, fg, bg, fg);
                        let clean_node = if show_tech {
                            node.node.as_str()
                        } else {
                            node.node
                                .strip_prefix("host:")
                                .or_else(|| node.node.strip_prefix("svc:"))
                                .unwrap_or(node.node.as_str())
                        };
                        let node_resp = ui.label(
                            RichText::new(clean_node)
                                .monospace()
                                .size(FONT_META)
                                .color(p.text_primary),
                        );
                        if show_tech {
                            node_resp.on_hover_text(&node.node);
                        }
                        if let Some(via) = node.via.as_deref() {
                            let clean_via = if show_tech {
                                via
                            } else {
                                via.strip_prefix("host:")
                                    .or_else(|| via.strip_prefix("svc:"))
                                    .unwrap_or(via)
                            };
                            let via_resp = ui.label(
                                RichText::new(format!("经由 {clean_via}"))
                                    .size(FONT_META)
                                    .color(p.text_muted),
                            );
                            if show_tech {
                                via_resp.on_hover_text(via);
                            }
                        }
                    });
                }
            });
        }

        ui.add_space(SPACE_2);
        egui::CollapsingHeader::new(
            RichText::new(translate(locale, "gui.advanced.toggle", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        )
        .id_salt(("guard_plan_tech", &plan.plan_id))
        .open(show_tech.then_some(true))
        .show(ui, |ui| {
            ui.monospace(format!(
                "plan_id={}\nop={} · safe_to_apply={} · applied={} · closure_complete={}\nprimary_reason={}",
                plan.plan_id,
                plan.operation,
                plan.safe_to_apply,
                plan.applied,
                plan.closure_complete,
                plan.primary_reason.as_deref().unwrap_or("none")
            ));
        });
    });
}

fn render_stage4_guard_audit_and_rollback_card(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    view: &GuardViewState,
    p: Palette,
    show_tech: bool,
) {
    let locale = state.current_locale();
    if view.selected_services.is_empty() {
        if let Some(audit) = state.active_run().and_then(|run| run.guard_audit.as_ref()) {
            render_audit(
                ui,
                p,
                locale,
                audit,
                &translate(locale, "gui.guard.audit.title", &[]),
                show_tech,
            );
            return;
        }
    }
    let (source_run_id, targets) = build_preview_targets(state, view);
    let results: Vec<parallaxe_core::contracts::ResultRecord> = state
        .active_run()
        .map(|r| r.ordered_results().into_iter().cloned().collect())
        .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let source_interrupted = state.active_run().and_then(|r| r.termination())
        != Some(parallaxe_core::Termination::Complete);
    let preview = guard::plan_preview(
        registry,
        GuardRequest {
            run_id: &format!("draft-{source_run_id}"),
            source_run_id: Some(&source_run_id),
            targets: &targets,
            results: &results,
            recorded_at: parallaxe_core::state::format_time(now),
            as_of: now.date(),
            source_interrupted,
        },
    );
    render_audit(
        ui,
        p,
        locale,
        &preview.audit,
        &translate(locale, "gui.guard.audit.live_title", &[]),
        show_tech,
    );
}

fn render_audit(
    ui: &mut Ui,
    p: Palette,
    locale: Locale,
    audit: &guard::GuardAudit,
    title: &str,
    show_tech: bool,
) {
    let outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let inner_w = (outer_w - SPACE_4 * 2.0).max(160.0);
        ui.set_width(inner_w);
        ui.set_max_width(inner_w);
        ui.label(
            RichText::new(title)
                .strong()
                .size(FONT_TITLE)
                .color(p.text_primary),
        );
        ui.add_space(SPACE_2);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            let rw_str = audit.after.rules_written.to_string();
            render_pill(
                ui,
                &translate(locale, "gui.guard.audit.rules_written", &[&rw_str]),
                p.ok_fg,
                p.ok_bg,
                p.ok_fg,
            );
            render_pill(
                ui,
                &translate(locale, "gui.guard.audit.no_rollback", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            );
        });

        ui.add_space(SPACE_3);
        subtle_well_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let result_zh = if audit.safe_to_apply {
                "预演通过（仍仅只读，不自动写入）"
            } else {
                "已拦截自动应用（仅作只读预演，防误伤）"
            };
            kv_row(ui, p, "审计结论", 76.0, result_zh, false);
            ui.add_space(SPACE_1);
            let cand = if audit.selection.candidates.is_empty() {
                "无".to_string()
            } else {
                audit
                    .selection
                    .candidates
                    .iter()
                    .map(|c| service_name(c))
                    .collect::<Vec<_>>()
                    .join("、")
            };
            kv_row(
                ui,
                p,
                "服务筛选",
                76.0,
                &format!(
                    "共评估 {} 项 · 符合自动条件 {} 项（{}）· 拦截 {} 项",
                    audit.selection.requested.len(),
                    audit.selection.candidates.len(),
                    cand,
                    audit.selection.rejected.len(),
                ),
                false,
            );
            if !audit.selection.rejected.is_empty() {
                ui.add_space(SPACE_1);
                let rej_summary: Vec<String> = audit
                    .selection
                    .rejected
                    .iter()
                    .take(3)
                    .map(|r| {
                        format!(
                            "{}：{}",
                            service_name(&r.target),
                            humanize_veto_reason(&r.reason)
                        )
                    })
                    .collect();
                kv_row(ui, p, "拦截说明", 76.0, &rej_summary.join("；"), false);
            }
        });

        ui.add_space(SPACE_3);
        render_admonition(ui, p.info_bg, p.info_fg, |ui| {
            ui.label(
                RichText::new(translate(locale, "gui.guard.audit.rollback_banner", &[]))
                    .strong()
                    .size(FONT_META)
                    .color(p.info_fg),
            );
            ui.add_space(SPACE_1);
            ui.label(
                RichText::new(&audit.rollback.note)
                    .size(FONT_META)
                    .color(p.text_primary),
            );
        });

        ui.add_space(SPACE_2);
        egui::CollapsingHeader::new(
            RichText::new(translate(locale, "gui.guard.audit.tech_fields", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        )
        .id_salt(("guard_audit_tech", &audit.plan_id))
        .open(show_tech.then_some(true))
        .show(ui, |ui| {
            ui.monospace(format!(
                "schema={}\nplan_id={}\nresult={} · safe_to_apply={} · applied={}\nrules_written={}/{} · rollback={} (required={}, performed={})",
                audit.schema,
                audit.plan_id,
                audit.result,
                audit.safe_to_apply,
                audit.applied,
                audit.before.rules_written,
                audit.after.rules_written,
                audit.rollback.status,
                audit.rollback.required,
                audit.rollback.performed
            ));
        });
    });
}

fn link_cell(ui: &mut Ui, w: f32, draw: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(w);
        draw(ui);
    });
}

fn humanize_link_status(status: &str, locale: Locale) -> (String, bool, bool) {
    let (key, is_ok, is_err) = match status {
        "OBSERVED" | "RESOLVED" => ("gui.link.status.observed", true, false),
        "OBSERVED_OK" => ("gui.link.status.ok", true, false),
        "RESOLVE_FAILED" => ("gui.link.status.resolve_failed", false, true),
        "FAILED" | "ERROR" => ("gui.link.status.failed", false, true),
        "NOT_OBSERVED" => ("gui.link.status.not_observed", false, false),
        "UNTESTED" | "SKIPPED" => ("gui.link.status.untested", false, false),
        "UNAVAILABLE" => ("gui.link.status.unavailable", false, true),
        _ => ("gui.link.status.pending", false, false),
    };
    (translate(locale, key, &[]), is_ok, is_err)
}

fn human_status_pill(
    ui: &mut Ui,
    prefix: Option<&str>,
    raw_status: &str,
    locale: Locale,
    p: Palette,
) {
    let (localized, is_ok, is_err) = humanize_link_status(raw_status, locale);
    let label = match prefix {
        Some(pre) => format!("{pre} {localized}"),
        None => localized,
    };
    let (fg, bg) = if is_ok {
        (p.info_fg, p.info_bg)
    } else if is_err {
        (p.danger_fg, p.danger_bg)
    } else {
        (p.text_secondary, p.card_alt)
    };
    render_pill(ui, &label, fg, bg, fg);
}

fn humanize_link_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("RESOLVER_ANSWER_NOT_CAPTURED") => "未捕获 DNS 解析记录",
        Some("TRANSPORT_RESOLVE") => "本机域名解析未返回结果",
        Some("NO_REGION_SIGNAL") => "页面未携带明确地区信号",
        Some("PEER_NOT_OBSERVED") => "未观测到实际连接对端",
        Some("REGION_RESTRICTED") => "服务返回明确地区限制信号",
        Some("POLICY_BLOCKED") => "服务返回明确访问阻断信号",
        Some("REGION_RESTRICTED_FIXTURE") => "演示：服务返回地区限制",
        Some("POLICY_BLOCKED_FIXTURE") => "演示：策略拒绝访问",
        Some(_) => "详见折叠技术字段",
        None => "无附加异常说明",
    }
}

fn humanize_layer_code(layer: Option<&str>) -> &'static str {
    match layer {
        Some("SERVICE_POLICY") => "服务策略层",
        Some("EGRESS_NETWORK") => "出口网络层",
        Some("PREFERENCE") => "公开版次层",
        Some("ACCOUNT") => "账号层（未采集）",
        Some(_) => "其他归因层",
        None => "未归因",
    }
}

#[allow(clippy::too_many_arguments)]
fn render_stage4_linkcheck_card(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    view: &GuardViewState,
    resolved_link_report: Option<&LinkReport>,
    show_ip: bool,
    p: Palette,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let computed_report: LinkReport;
    let link_report: &LinkReport = if let Some(resolved) = resolved_link_report
        .filter(|r| r.source_run_id.as_deref() == state.active_run_id.as_deref())
    {
        resolved
    } else if let Some(run) = state.active_run() {
        if view.selected_services.is_empty() {
            if let Some(rep) = &run.report {
                computed_report =
                    linkcheck::analyze_with_ip(registry, rep, rep.generated_at.clone(), show_ip);
                &computed_report
            } else {
                let (_, ids) = build_preview_targets(state, view);
                computed_report = linkcheck::analyze_selection(
                    registry,
                    &ids,
                    parallaxe_core::state::format_time(time::OffsetDateTime::now_utc()),
                );
                &computed_report
            }
        } else {
            let (_, ids) = build_preview_targets(state, view);
            computed_report = linkcheck::analyze_selection(
                registry,
                &ids,
                parallaxe_core::state::format_time(time::OffsetDateTime::now_utc()),
            );
            &computed_report
        }
    } else {
        let (_, ids) = build_preview_targets(state, view);
        computed_report = linkcheck::analyze_selection(
            registry,
            &ids,
            parallaxe_core::state::format_time(time::OffsetDateTime::now_utc()),
        );
        &computed_report
    };

    let locale = state.current_locale();
    let card_outer_w = ui.available_width();
    card_frame(p).show(ui, |ui| {
        let card_inner_w = (card_outer_w - SPACE_4 * 2.0).max(200.0);
        ui.set_width(card_inner_w);
        ui.set_max_width(card_inner_w);

        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(translate(locale, "gui.link.title", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
            let resolve_targets: Vec<String> = link_report
                .rows
                .iter()
                .map(|r| r.target.clone())
                .collect();
            let offline = state.active_run().is_some_and(|run| run.is_fixture);
            let cnt = resolve_targets.len().to_string();
            let label = if offline {
                translate(locale, "gui.link.btn.offline", &[])
            } else {
                translate(locale, "gui.link.btn.resolve", &[&cnt])
            };
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if btn_outlined(ui, p, &label, !offline && !resolve_targets.is_empty()).clicked() {
                    actions.push(UiAction::RunLinkResolve(resolve_targets));
                }
            });
        });

        ui.add_space(2.0);
        ui.label(
            RichText::new(translate(locale, "gui.link.subtitle", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        );

        ui.add_space(SPACE_2);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.route", &[]),
                p.info_fg,
                p.info_bg,
                p.info_fg,
            );
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.dns_compare", &[]),
                p.warning_fg,
                p.warning_bg,
                p.warning_fg,
            );
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.dns_leak", &[]),
                p.text_secondary,
                p.card_alt,
                p.border,
            );
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.quic", &[]),
                p.text_secondary,
                p.card_alt,
                p.border,
            );
            render_pill(
                ui,
                &translate(locale, "gui.link.cap.port_scan", &[]),
                p.danger_fg,
                p.danger_bg,
                p.danger_fg,
            );
        });

        ui.add_space(SPACE_2);
        let tun_key = match link_report.local_path.via_tun {
            Some(true) => "gui.link.tun.yes",
            Some(false) => "gui.link.tun.no",
            None => "gui.link.tun.unknown",
        };
        let tun_desc = translate(locale, tun_key, &[]);
        let iface_fallback = translate(locale, "gui.link.iface_unknown", &[]);
        let iface_desc = link_report
            .local_path
            .interface_alias
            .as_deref()
            .unwrap_or(&iface_fallback);
        let dns_cnt = link_report.dns_query_count.to_string();
        let http_cnt = link_report.http_request_count.to_string();
        ui.label(
            RichText::new(translate(
                locale,
                "gui.link.local_overview",
                &[&tun_desc, iface_desc, &dns_cnt, &http_cnt],
            ))
            .size(FONT_META)
            .color(p.text_secondary),
        );

        ui.add_space(SPACE_3);

        // 固定列宽 + 横向滚动四列数据表（全中文标签）
        let table_outer_w = ui.available_width();
        let col_gap = SPACE_2;
        let col_widths = [140.0_f32, 188.0, 210.0, 188.0, 188.0];
        let total_w: f32 = col_widths.iter().sum::<f32>() + col_gap * 4.0 + SPACE_4;

        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .show(ui, |ui| {
                ui.set_max_width(table_outer_w);
                ScrollArea::horizontal()
                    .id_salt("linkcheck_four_col_hscroll")
                    .max_width(table_outer_w)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_width(total_w.max(table_outer_w - 2.0));
                        egui::Frame::none()
                            .fill(p.table_head_bg)
                            .inner_margin(Margin::symmetric(SPACE_2, SPACE_2))
                            .show(ui, |ui| {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = col_gap;
                                    let link_headers = [
                                        translate(locale, "gui.link.col.service", &[]),
                                        translate(locale, "gui.link.col.dns", &[]),
                                        translate(locale, "gui.link.col.edge", &[]),
                                        translate(locale, "gui.link.col.egress", &[]),
                                        translate(locale, "gui.link.col.dualstack", &[]),
                                    ];
                                    for (w, h) in col_widths.into_iter().zip(&link_headers) {
                                        link_cell(ui, w, |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(h)
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

                        for (idx, row) in link_report.rows.iter().enumerate() {
                            let is_user_target = registry
                                .user_targets()
                                .into_iter()
                                .any(|m| m.id == row.target);
                            let bg = if idx % 2 == 1 {
                                p.table_even_bg
                            } else {
                                p.card
                            };
                            let resp = egui::Frame::none()
                                .fill(bg)
                                .inner_margin(Margin::symmetric(SPACE_2, SPACE_2))
                                .show(ui, |ui| {
                                    ui.horizontal_top(|ui| {
                                        ui.spacing_mut().item_spacing.x = col_gap;

                                        // 列 0：目标服务
                                        link_cell(ui, col_widths[0], |ui| {
                                            ui.label(
                                                RichText::new(service_name(&row.target))
                                                    .strong()
                                                    .size(FONT_BODY)
                                                    .color(p.text_primary),
                                            );
                                            ui.add_space(2.0);
                                            ui.label(
                                                RichText::new(&row.host)
                                                    .monospace()
                                                    .size(FONT_META)
                                                    .color(p.text_secondary),
                                            );
                                            ui.add_space(2.0);
                                            render_pill(
                                                ui,
                                                if is_user_target { "用户导入" } else { "内置目录" },
                                                if is_user_target {
                                                    p.warning_fg
                                                } else {
                                                    p.text_muted
                                                },
                                                p.card_alt,
                                                p.border_subtle,
                                            );
                                        });

                                        // 列 1：域名解析
                                        link_cell(ui, col_widths[1], |ui| {
                                            human_status_pill(
                                                ui,
                                                None,
                                                &row.resolver.status,
                                                locale,
                                                p,
                                            );
                                            ui.add_space(SPACE_1);
                                            ui.label(
                                                RichText::new(format!(
                                                    "IPv4 {} · IPv6 {}",
                                                    row.resolver.v4, row.resolver.v6
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                            );
                                            if show_ip && !row.resolver.addresses.is_empty() {
                                                ui.label(
                                                    RichText::new(row.resolver.addresses.join(", "))
                                                        .monospace()
                                                        .size(FONT_META)
                                                        .color(p.text_primary),
                                                );
                                            }
                                            ui.label(
                                                RichText::new(humanize_link_reason(
                                                    row.resolver.reason.as_deref(),
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_muted),
                                            );
                                        });

                                        // 列 2：连接边缘
                                        link_cell(ui, col_widths[2], |ui| {
                                            human_status_pill(
                                                ui,
                                                None,
                                                &row.connection_edge.status,
                                                locale,
                                                p,
                                            );
                                            ui.add_space(SPACE_1);
                                            let fam = row
                                                .connection_edge
                                                .observed_family
                                                .as_deref()
                                                .unwrap_or("—");
                                            ui.label(
                                                RichText::new(format!(
                                                    "{fam} · {} tries",
                                                    row.connection_edge.attempts.len()
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                            );
                                            let peer = row
                                                .connection_edge
                                                .peer_address
                                                .as_deref()
                                                .unwrap_or("—");
                                            if row.destination.has_observation() {
                                                let role = crate::views::service_matrix::peer_role_label(
                                                    row.destination.peer_role,
                                                    locale,
                                                    show_tech,
                                                );
                                                let peer_summary = crate::views::service_matrix::format_peer_summary(
                                                    &row.destination,
                                                    locale,
                                                    show_ip,
                                                );
                                                ui.label(
                                                    RichText::new(format!("{role} · {peer_summary}"))
                                                        .size(FONT_META)
                                                        .color(if row.destination.has_risk_markers() {
                                                            p.warning_fg
                                                        } else {
                                                            p.text_muted
                                                        }),
                                                );
                                                for m in row.destination.markers() {
                                                    let lbl = crate::views::service_matrix::dest_marker_label(
                                                        m, locale, show_tech,
                                                    );
                                                    let pill = render_pill(
                                                        ui,
                                                        &lbl,
                                                        p.warning_fg,
                                                        p.warning_bg,
                                                        p.warning_fg,
                                                    );
                                                    if show_tech {
                                                        pill.on_hover_text(m);
                                                    }
                                                }
                                            } else {
                                                ui.label(
                                                    RichText::new(peer)
                                                        .size(FONT_META)
                                                        .color(p.text_muted),
                                                );
                                            }
                                        });

                                        // 列 3：网页出口
                                        link_cell(ui, col_widths[3], |ui| {
                                            human_status_pill(
                                                ui,
                                                None,
                                                &row.http_egress.status,
                                                locale,
                                                p,
                                            );
                                            ui.add_space(SPACE_1);
                                            let country = row
                                                .http_egress
                                                .observed_country
                                                .as_deref()
                                                .unwrap_or("—");
                                            ui.label(
                                                RichText::new(format!(
                                                    "{country} · {}",
                                                    humanize_layer_code(
                                                        row.http_egress.layer.as_deref()
                                                    )
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                            );
                                            ui.label(
                                                RichText::new(humanize_link_reason(
                                                    row.http_egress.reason.as_deref(),
                                                ))
                                                .size(FONT_META)
                                                .color(p.text_muted),
                                            );
                                        });

                                        // 列 4：双栈状态
                                        link_cell(ui, col_widths[4], |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = SPACE_1;
                                                human_status_pill(
                                                    ui,
                                                    Some("IPv4"),
                                                    &row.address_family.v4,
                                                    locale,
                                                    p,
                                                );
                                            });
                                            ui.add_space(2.0);
                                            let (v6_lbl, _, _) = humanize_link_status(
                                                &row.address_family.v6,
                                                locale,
                                            );
                                            ui.label(
                                                RichText::new(format!("IPv6: {v6_lbl}"))
                                                    .size(FONT_META)
                                                    .color(p.text_secondary),
                                            );
                                            ui.label(
                                                RichText::new(if row.address_family.fallback {
                                                    "已触发双栈回退"
                                                } else {
                                                    "未触发回退"
                                                })
                                                .size(FONT_META)
                                                .color(p.text_muted),
                                            );
                                        });
                                    });
                                });
                            let rect = resp.response.rect;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.left(), rect.top()),
                                    egui::pos2(rect.right(), rect.top()),
                                ],
                                Stroke::new(1.0_f32, p.border_subtle),
                            );
                        }
                    });
            });

        ui.add_space(SPACE_2);
        egui::CollapsingHeader::new(
            RichText::new(translate(locale, "gui.advanced.toggle", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        )
        .id_salt("guard_linkcheck_tech")
        .open(show_tech.then_some(true))
        .show(ui, |ui| {
            ui.monospace(format!(
                "schema=parallaxe.link/v1 · show_ip={show_ip}\nroute_get={} · dns_compare={} · dns_leak={} · quic={} · port_scan={}\nvia_tun={:?} · interface_alias={:?}",
                link_report.capabilities.route_get,
                link_report.capabilities.dns_compare,
                link_report.capabilities.dns_leak,
                link_report.capabilities.quic,
                link_report.capabilities.port_scan,
                link_report.local_path.via_tun,
                link_report.local_path.interface_alias,
            ));
        });
    });
}

fn build_preview_targets(state: &AppState, view: &GuardViewState) -> (String, Vec<String>) {
    let source_run_id = state
        .active_run()
        .map(|r| r.run_id.clone())
        .unwrap_or_else(|| "run-manual-preview".to_string());
    if !view.selected_services.is_empty() {
        return (
            source_run_id,
            view.selected_services.iter().cloned().collect(),
        );
    }
    if let Some(run) = state.active_run() {
        let mut ids: Vec<String> = run
            .ordered_results()
            .into_iter()
            .map(|r| r.target.clone())
            .collect();
        if ids.is_empty() {
            ids = run.selected_ids.iter().take(6).cloned().collect();
        }
        if !ids.is_empty() {
            return (source_run_id, ids);
        }
    }
    (
        source_run_id,
        vec![
            "google.books".to_string(),
            "google.one".to_string(),
            "google.play".to_string(),
        ],
    )
}
