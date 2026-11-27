//! 统一导入页（自定义网址、用户自定义分组与检查方式）：
//! - 三入口（手动配置 / 文本导入 / ZIP 导入包）位于卡片输入区正上方，窄屏横向等宽排布（>= 100px），绝不竖排逐字折行；
//! - 支持用户自定义分组（默认「我的导入」，与 Google / 对照内置分组完全隔离，仅用于展示与筛选）；
//! - 桌面端接入原生系统文件选择器（文本文件 / ZIP 包），移动端直给粘贴文本框与诚实指引；
//! - 默认视角隐藏 `origin=USER_IMPORTED` / `rule. 开头` / `9 种 Reader` / `pattern(防 ReDoS)` 等底层术语，
//!   完整保留核定中文责任声明原文与 F3 一次性会话授权闸门。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{vec2, Align, Layout, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::contracts::{
    Confidence, Layer, Source, LIABILITY_CONSENT_VERSION, ORIGIN_USER_IMPORTED,
    USER_RULE_LIABILITY_NOTICE, USER_TARGET_LIABILITY_NOTICE,
};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::import_bundle::{
    build_stored_zip_bytes, parse_import_text_with_default_group, parse_import_zip_bytes,
    read_import_text_file, read_import_zip, BundleTargetPreviewRow, ImportBundle, ImportSource,
    ImportTargetEntry, IMPORT_BUNDLE_SCHEMA,
};
use parallaxe_core::targets::{
    normalize_user_group, Extractor, InspectionCategory, Registry, Rule, DEFAULT_USER_GROUP,
};

use crate::folder_picker::{pick_import_file, ImportFileFilter};
use crate::icons::{icon_tile, IconKind};
use crate::layout::ViewportTier;
use crate::theme::{
    btn_danger_tonal, btn_filled, btn_outlined, btn_tonal, card_frame, render_admonition,
    render_modal_scrim, render_pill, spaced_columns, subtle_well_frame, Palette, ThemeMode,
    FONT_BODY, FONT_META, FONT_TITLE, RADIUS_LG, RADIUS_MD, RADIUS_SM, SPACE_1, SPACE_2, SPACE_3,
    SPACE_4, SPACE_5,
};
use crate::views::UiAction;

/// 测试可注入的文件选择器回调类型。
pub type ImportFilePickerFn =
    Arc<dyn Fn(&str, ImportFileFilter) -> Result<Option<PathBuf>, String> + Send + Sync>;

/// 统一导入页顶部三入口模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportEntryTab {
    #[default]
    Manual,
    TextFile,
    ZipBundle,
}

impl ImportEntryTab {
    pub fn label(self, locale: Locale) -> String {
        let key = match self {
            ImportEntryTab::Manual => "gui.import.tab.manual",
            ImportEntryTab::TextFile => "gui.import.tab.text",
            ImportEntryTab::ZipBundle => "gui.import.tab.zip",
        };
        translate(locale, key, &[])
    }

    pub fn tip(self, locale: Locale) -> String {
        let key = match self {
            ImportEntryTab::Manual => "gui.import.tab.manual_tip",
            ImportEntryTab::TextFile => "gui.import.tab.text_tip",
            ImportEntryTab::ZipBundle => "gui.import.tab.zip_tip",
        };
        translate(locale, key, &[])
    }

    pub const ALL: [ImportEntryTab; 3] = [
        ImportEntryTab::Manual,
        ImportEntryTab::TextFile,
        ImportEntryTab::ZipBundle,
    ];
}

/// 统一导入界面的持久交互状态。
pub struct ImportViewState {
    /// 当前选择的导入入口（手动配置 / 文本导入 / ZIP 导入包）。
    pub entry_tab: ImportEntryTab,

    /// 责任声明勾选状态（按当前 bundle 内容动态生效）。
    pub target_consent_checked: bool,
    pub rule_consent_checked: bool,

    // --- 问题 2：用户自定义分组 ---
    /// 当前新导入条目默认归属的分组名（默认「我的导入」）。
    pub selected_group: String,
    /// 新建分组输入框。
    pub new_group_input: String,
    /// 用户在本会话内创建的分组列表（始终包含默认组「我的导入」）。
    pub custom_groups: Vec<String>,
    /// 已登记清单的分组筛选（`None` 表示全部）。
    pub inventory_group_filter: Option<String>,

    // --- 入口 1：手动配置表单 ---
    pub single_url_input: String,
    pub single_id_input: String,
    pub surface_input: String,
    pub allow_insecure_http: bool,
    pub selected_signals: BTreeSet<String>,
    pub manual_targets: Vec<ImportTargetEntry>,
    pub manual_rules: Vec<Rule>,
    pub show_inline_rule_editor: bool,
    pub rule_id_input: String,
    pub rule_category: InspectionCategory,
    pub rule_pattern_input: String,
    pub rule_capture_input: String,
    pub rule_layer: Layer,
    pub rule_channel: String,
    pub rule_notes_input: String,

    // --- 入口 2：文本导入 ---
    pub text_file_path: String,
    pub text_editor_content: String,
    pub text_allow_insecure_http: bool,
    pub text_loaded_bundle: Option<ImportBundle>,
    pub text_parse_error: Option<String>,

    // --- 入口 3：ZIP 导入包 ---
    pub zip_file_path: String,
    pub zip_loaded_bundle: Option<ImportBundle>,
    pub zip_parse_error: Option<String>,

    /// 测试注入的本地文件选择器回调（`None` 时走系统原生对话框）。
    pub file_picker_override: Option<ImportFilePickerFn>,

    /// 用户态规则保存目录（不写回内置 assets/）。
    pub user_state_dir_input: String,

    /// 运行前一次性在线许可确认弹层是否打开（F3）。
    pub online_confirm_open: bool,
    pub online_run_confirmed_checkbox: bool,

    /// 最近一次导入或授权操作反馈（`Ok(成功信息)` / `Err(错误提示)`）。
    pub feedback: Option<Result<String, String>>,

    /// 用户在预览表中显式勾选"更换"的重复网址行（1-based，与 `ImportBundle.targets` 顺序一致）。
    pub replace_targets: BTreeSet<usize>,
}

const SAMPLE_TEXT_LINES: &str = concat!(
    "# 支持按 [分组名] 或「分组名 | 网址」归类，也支持每行直接粘贴网址\n",
    "[日常自测]\n",
    "https://edge-a.example.com/cdn-cgi/trace | rule.http,rule.trace | edge-a | 出口与连通性\n",
    "https://edge-b.example.com/books | rule.http,rule.inline3 | edge-b | 页面地区码检查\n",
    "[办公专线]\n",
    "https://edge-c.example.com/status | rule.http | edge-c | 状态码连通性检查",
);

impl Default for ImportViewState {
    fn default() -> Self {
        let mut selected_signals = BTreeSet::new();
        selected_signals.insert("rule.trace".to_string());
        selected_signals.insert("rule.http".to_string());
        let default_group = DEFAULT_USER_GROUP.to_string();
        let initial_text_bundle = parse_import_text_with_default_group(
            SAMPLE_TEXT_LINES,
            &[],
            false,
            Some(&default_group),
        )
        .ok();
        Self {
            entry_tab: ImportEntryTab::Manual,
            target_consent_checked: false,
            rule_consent_checked: false,
            selected_group: default_group.clone(),
            new_group_input: String::new(),
            custom_groups: vec![default_group],
            inventory_group_filter: None,
            single_url_input: "https://icanhazip.example.com/cdn-cgi/trace".to_string(),
            single_id_input: "custom-trace".to_string(),
            surface_input: "自定义出口与连通性".to_string(),
            allow_insecure_http: false,
            selected_signals,
            manual_targets: Vec::new(),
            manual_rules: Vec::new(),
            show_inline_rule_editor: false,
            rule_id_input: "user-region-code".to_string(),
            rule_category: InspectionCategory::BodyRegex,
            rule_pattern_input: "\"country_code\":\"([A-Z]{2})\"".to_string(),
            rule_capture_input: "1".to_string(),
            rule_layer: Layer::ServicePolicy,
            rule_channel: "SERVICE".to_string(),
            rule_notes_input: "提取页面中的两字母地区码".to_string(),
            text_file_path: String::new(),
            text_editor_content: SAMPLE_TEXT_LINES.to_string(),
            text_allow_insecure_http: false,
            text_loaded_bundle: initial_text_bundle,
            text_parse_error: None,
            zip_file_path: String::new(),
            zip_loaded_bundle: None,
            zip_parse_error: None,
            file_picker_override: None,
            user_state_dir_input: "user-custom-state".to_string(),
            online_confirm_open: false,
            online_run_confirmed_checkbox: false,
            feedback: None,
            replace_targets: BTreeSet::new(),
        }
    }
}

impl ImportViewState {
    /// 返回所有可选用户分组（始终包含默认组 + 会话新建组 + 目录已登记组，保持去重有序）。
    pub fn all_groups(&self, registry: &Registry) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut push = |g: &str| {
            if seen.insert(g.to_string()) {
                out.push(g.to_string());
            }
        };
        push(DEFAULT_USER_GROUP);
        for g in &self.custom_groups {
            push(g);
        }
        for g in registry.user_groups() {
            push(&g);
        }
        out
    }

    /// 创建并选中一个用户自定义分组（拒绝与内置 google / control 命名空间冲突）。
    pub fn create_and_select_group(&mut self, raw_name: &str) -> Result<String, String> {
        let trimmed = raw_name.trim();
        if trimmed.is_empty() {
            return Err("请输入分组名称".to_string());
        }
        let normalized = normalize_user_group(Some(trimmed)).map_err(|e| e.to_string())?;
        if !self.custom_groups.iter().any(|g| g == &normalized) {
            self.custom_groups.push(normalized.clone());
        }
        self.selected_group = normalized.clone();
        self.new_group_input.clear();
        Ok(normalized)
    }

    /// 调用文件选择器（优先使用测试注入回调，否则使用平台原生文件对话框）。
    fn pick_file(&self, prompt: &str, filter: ImportFileFilter) -> Result<Option<PathBuf>, String> {
        if let Some(cb) = &self.file_picker_override {
            cb(prompt, filter)
        } else {
            pick_import_file(prompt, filter)
        }
    }

    /// 立即载入并解析文本文件。
    pub fn load_text_file(&mut self, path: &Path) {
        self.text_file_path = path.display().to_string();
        self.replace_targets.clear();
        match read_import_text_file(path, &[], self.text_allow_insecure_http) {
            Ok(mut bundle) => {
                if let Ok(raw) = std::fs::read_to_string(path) {
                    self.text_editor_content = raw;
                    if let Ok(re_parsed) = parse_import_text_with_default_group(
                        &self.text_editor_content,
                        &[],
                        self.text_allow_insecure_http,
                        Some(&self.selected_group),
                    ) {
                        bundle = re_parsed;
                    }
                }
                self.text_loaded_bundle = Some(bundle);
                self.text_parse_error = None;
                self.feedback = Some(Ok(format!(
                    "已读取并解析文件 {}，请在预览区核对分组与检查方式。",
                    path.display()
                )));
            }
            Err(err) => {
                self.text_parse_error = Some(err.to_string());
                self.text_loaded_bundle = None;
            }
        }
    }

    /// 立即载入并解包 ZIP 文件。
    pub fn load_zip_file(&mut self, path: &Path) {
        self.zip_file_path = path.display().to_string();
        self.replace_targets.clear();
        match read_import_zip(path) {
            Ok(bundle) => {
                self.zip_loaded_bundle = Some(bundle);
                self.zip_parse_error = None;
                self.feedback = Some(Ok(format!(
                    "已安全解包并解析 {}，请在预览区核对后导入。",
                    path.display()
                )));
            }
            Err(err) => {
                self.zip_loaded_bundle = None;
                self.zip_parse_error = Some(err.to_string());
            }
        }
    }

    /// 根据当前选中的入口 Tab，构造待预览/待导入的统一 `ImportBundle`。
    fn active_bundle(&self) -> Option<ImportBundle> {
        match self.entry_tab {
            ImportEntryTab::Manual => {
                let mut bundle = ImportBundle::new(ImportSource::Manual);
                bundle.rules = self.manual_rules.clone();
                if self.manual_targets.is_empty() {
                    let url = self.single_url_input.trim();
                    if !url.is_empty() {
                        let signals: Vec<String> = if self.selected_signals.is_empty() {
                            vec!["rule.http".to_string()]
                        } else {
                            self.selected_signals.iter().cloned().collect()
                        };
                        bundle.targets.push(ImportTargetEntry {
                            id: (!self.single_id_input.trim().is_empty())
                                .then(|| self.single_id_input.trim().to_string()),
                            url: url.to_string(),
                            surface: (!self.surface_input.trim().is_empty())
                                .then(|| self.surface_input.trim().to_string()),
                            signals,
                            allow_insecure_http: self.allow_insecure_http,
                            user_group: Some(self.selected_group.clone()),
                            replace_existing: self.replace_targets.contains(&1),
                        });
                    }
                } else {
                    bundle.targets = self.manual_targets.clone();
                    for (idx, entry) in bundle.targets.iter_mut().enumerate() {
                        entry.replace_existing = self.replace_targets.contains(&(idx + 1));
                    }
                }
                if bundle.targets.is_empty() && bundle.rules.is_empty() {
                    None
                } else {
                    Some(bundle)
                }
            }
            ImportEntryTab::TextFile => self.text_loaded_bundle.clone().map(|mut bundle| {
                for (idx, entry) in bundle.targets.iter_mut().enumerate() {
                    entry.replace_existing = self.replace_targets.contains(&(idx + 1));
                }
                bundle
            }),
            ImportEntryTab::ZipBundle => self.zip_loaded_bundle.clone().map(|mut bundle| {
                for (idx, entry) in bundle.targets.iter_mut().enumerate() {
                    entry.replace_existing = self.replace_targets.contains(&(idx + 1));
                }
                bundle
            }),
        }
    }

    pub fn load_sample_zip_bundle(&mut self) {
        let manifest =
            format!(r#"{{"schema":"{IMPORT_BUNDLE_SCHEMA}","source":"ZIP_BUNDLE","targets":[]}}"#);
        let rules_toml = r#"[[rules]]
id = "rule.zip-sample-region"
version = 1
enabled = true
layer = "SERVICE_POLICY"
channel = "SERVICE"
credibility = "LOW"
source_grade = "DIRECT_ANONYMOUS_OBSERVATION"
notes = "示例导入包附带的地区码检查方式"
negative_case = "未命中时仅记缺失，不推断国家"
extract = { kind = "regex", pattern = "region=([A-Z]{2})", capture = "1" }
applies_to = []
"#;
        let targets_txt = "# ZIP 包内目标列表（可按分组归类并绑定检查方式）\n\
[示例分组]\n\
https://zip-edge-a.example.com/api | rule.zip-sample-region,rule.http | zip-edge-a | 绑定包内新增规则\n\
https://zip-edge-b.example.com/trace | rule.trace,rule.http | zip-edge-b | 绑定内置出口规则\n";
        let readme = "示例导入包说明：包含 1 条自定义检查规则与 2 个自定义目标网址。说明文档仅作只读展示，不执行任何脚本。";

        let bytes = build_stored_zip_bytes(&[
            ("sample-bundle/manifest.json", manifest.as_bytes()),
            ("sample-bundle/rules.toml", rules_toml.as_bytes()),
            ("sample-bundle/targets.txt", targets_txt.as_bytes()),
            ("sample-bundle/README.md", readme.as_bytes()),
        ]);
        match parse_import_zip_bytes(&bytes) {
            Ok(bundle) => {
                self.zip_loaded_bundle = Some(bundle);
                self.zip_parse_error = None;
                self.feedback = Some(Ok(
                    "已载入内置示例 ZIP 导入包（含 2 个网址与 1 条自定义检查方式），请核对预览后勾选声明并导入。"
                        .to_string(),
                ));
            }
            Err(err) => {
                self.zip_parse_error = Some(err.to_string());
            }
        }
    }
}

/// 将内置规则 ID 转换为通俗中文名称（悬停或在技术细节模式下仍可看原始规则 ID）。
fn friendly_signal_label(sig: &str, show_tech: bool) -> String {
    let plain = match sig {
        "rule.http" => "HTTP 状态码",
        "rule.trace" => "IP 出口",
        "rule.inline3" => "三字地区码",
        "rule.header" => "响应头地区",
        "rule.redirect" => "重定向地址",
        "rule.title" => "页面标题",
        "rule.deny" => "限制提示词",
        "rule.html" => "页面正文匹配",
        "rule.json" => "接口字段",
        other => return other.strip_prefix("rule.").unwrap_or(other).to_string(),
    };
    if show_tech {
        format!("{plain} ({sig})")
    } else {
        plain.to_string()
    }
}

/// 将底层 extractor kind 转换为通俗中文说明。
fn friendly_reader_kind(kind: &str, show_tech: bool) -> String {
    let plain = match kind {
        "status_code" => "状态码检查",
        "trace_kv" => "出口键值对",
        "header" => "响应头读取",
        "redirect_location" => "跳转目标",
        "html_title" => "网页标题",
        "regex" => "正文正则匹配",
        "json_path" => "JSON 字段",
        "contains" => "关键词包含",
        "deny_marker" => "限制标记",
        other => other,
    };
    if show_tech {
        format!("{plain} [{kind}]")
    } else {
        plain.to_string()
    }
}

/// 渲染统一导入包主视图（三入口顶置 + 用户分组 + 实时分组预览 + 动态责任声明 + 一次性授权弹窗）。
#[allow(clippy::too_many_arguments)]
pub fn render(
    ui: &mut Ui,
    registry: &mut Registry,
    view: &mut ImportViewState,
    theme: ThemeMode,
    locale: Locale,
    tier: ViewportTier,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);
    let is_narrow = tier == ViewportTier::Narrow;
    let very_narrow = ui.ctx().screen_rect().width() < 380.0;

    // 1. 顶部已登记资产与一次性在线授权闸门卡（移动端空态压缩为单行提示带，把首屏让给三入口与输入框）
    render_gate_header_card(ui, registry, view, locale, p, tier, show_tech, actions);
    ui.add_space(if is_narrow { SPACE_2 } else { SPACE_3 });

    // 一次性在线检查模态确认弹窗
    if view.online_confirm_open {
        let ctx = ui.ctx().clone();
        render_online_session_confirm_modal(&ctx, registry, view, p, show_tech, actions);
    }

    // 操作反馈条
    if let Some(outcome) = &view.feedback {
        let (bg, fg, ic, prefix) = match outcome {
            Ok(_) => (p.ok_bg, p.ok_fg, IconKind::Check, "操作成功："),
            Err(_) => (p.danger_bg, p.danger_fg, IconKind::Close, "校验未通过："),
        };
        let text = match outcome {
            Ok(msg) | Err(msg) => msg.clone(),
        };
        egui::Frame::none()
            .fill(bg)
            .stroke(Stroke::new(1.0_f32, fg.gamma_multiply(0.7)))
            .rounding(Rounding::same(RADIUS_MD))
            .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    crate::icons::icon(ui, ic, 14.0, fg);
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{prefix}{text}"))
                                .size(FONT_META)
                                .strong()
                                .color(fg),
                        )
                        .wrap(),
                    );
                });
            });
        ui.add_space(SPACE_3);
    }

    // 2. 导入主卡片：标题独占一行 -> 三入口水平标签栏独占一行（绝不被标题挤成右侧窄条）-> 输入区与预览
    let card_h_pad = if very_narrow {
        SPACE_2
    } else if is_narrow {
        SPACE_3
    } else {
        SPACE_4
    };
    card_frame(p)
        .inner_margin(Margin::symmetric(card_h_pad, SPACE_3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // 行 1：卡片标题（独占全宽，绝不与三入口 Tab 争抢同一行宽度）
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                icon_tile(ui, IconKind::Globe, 24.0, p.accent, p.accent_soft);
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.import.bundle.title", &[]))
                            .size(FONT_TITLE)
                            .strong()
                            .color(p.text_primary),
                    )
                    .wrap(),
                );
            });

            ui.add_space(SPACE_2);

            // 行 2：三入口水平 Tab 栏（位于输入区正上方，每项 >= 100px，超长省略 + 悬停提示，零逐字折行）
            render_entry_tab_bar(ui, view, locale, p, is_narrow);

            ui.add_space(SPACE_2);

            let active_bundle = view.active_bundle();
            let preview_opt = active_bundle
                .as_ref()
                .map(|b| registry.preview_import_bundle(b));

            // 桌面宽屏双栏（左输入 / 右分组预览）；移动窄屏单列上下堆叠（输入紧跟 Tab 正下方）
            if !is_narrow && ui.available_width() >= 820.0 {
                spaced_columns(ui, 2, SPACE_4, |col_ui, idx| {
                    if idx == 0 {
                        match view.entry_tab {
                            ImportEntryTab::Manual => render_manual_entry(
                                col_ui, registry, view, locale, p, false, show_tech,
                            ),
                            ImportEntryTab::TextFile => {
                                render_text_file_entry(col_ui, registry, view, locale, p, false)
                            }
                            ImportEntryTab::ZipBundle => {
                                render_zip_bundle_entry(col_ui, view, locale, p)
                            }
                        }
                    } else if let (Some(bundle), Some(preview)) = (&active_bundle, &preview_opt) {
                        render_unified_preview_table(
                            col_ui,
                            view,
                            preview,
                            bundle.readme.as_deref(),
                            locale,
                            p,
                            show_tech,
                        );
                    } else {
                        subtle_well_frame(p).show(col_ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    RichText::new("请在左侧填写网址、粘贴文本或选择本地导入包，解析后将按分组展示预览与校验状态。")
                                        .size(FONT_BODY)
                                        .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        });
                    }
                });
            } else {
                match view.entry_tab {
                    ImportEntryTab::Manual => {
                        render_manual_entry(ui, registry, view, locale, p, true, show_tech)
                    }
                    ImportEntryTab::TextFile => {
                        render_text_file_entry(ui, registry, view, locale, p, true)
                    }
                    ImportEntryTab::ZipBundle => render_zip_bundle_entry(ui, view, locale, p),
                }
                ui.add_space(SPACE_3);
                if let (Some(bundle), Some(preview)) = (&active_bundle, &preview_opt) {
                    render_unified_preview_table(
                        ui,
                        view,
                        preview,
                        bundle.readme.as_deref(),
                        locale,
                        p,
                        show_tech,
                    );
                } else {
                    subtle_well_frame(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new("请在上方填写网址、粘贴文本或载入导入包，解析后将按分组展示预览与校验状态。")
                                    .size(FONT_BODY)
                                    .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    });
                }
            }

            if let (Some(bundle), Some(preview)) = (&active_bundle, &preview_opt) {
                ui.add_space(SPACE_2);
                render_dynamic_consent_and_submit(
                    ui, registry, view, bundle, preview, locale, p, show_tech, actions,
                );
            }
        });

    ui.add_space(SPACE_4);
    render_imported_inventory_card(ui, registry, view, locale, p, show_tech, actions);
}

/// 独立一行的三入口横向 Tab 栏（移动端等宽 >= 100px、高 44px；桌面自适应 >= 112px、高 36px）。
fn render_entry_tab_bar(
    ui: &mut Ui,
    view: &mut ImportViewState,
    locale: Locale,
    p: Palette,
    is_narrow: bool,
) {
    let outer_w = if is_narrow {
        ui.available_width()
    } else {
        ui.available_width().min(540.0)
    };
    let tab_h = if is_narrow { 44.0 } else { 36.0 };
    let gap = SPACE_1;
    let inner_avail = (outer_w - SPACE_2).max(120.0);
    let equal_w = ((inner_avail - gap * 2.0) / 3.0).floor();

    egui::Frame::none()
        .fill(p.sidebar)
        .stroke(Stroke::new(1.0_f32, p.border_subtle))
        .rounding(Rounding::same(RADIUS_MD))
        .inner_margin(Margin::same(SPACE_1))
        .show(ui, |ui| {
            ui.set_width(inner_avail);
            if equal_w >= 88.0 {
                // 严格水平单行排布，3 个 Tab 等宽且超长省略 + 悬停提示，绝不逐字折行或掉到第二行
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for tab in ImportEntryTab::ALL {
                        let active = view.entry_tab == tab;
                        let lbl = tab.label(locale);
                        let tip = tab.tip(locale);
                        let resp = ui
                            .add_sized(
                                [equal_w, tab_h],
                                egui::Button::new(
                                    RichText::new(&lbl)
                                        .size(FONT_BODY)
                                        .strong()
                                        .color(if active { p.accent } else { p.text_secondary }),
                                )
                                .fill(if active {
                                    p.card
                                } else {
                                    egui::Color32::TRANSPARENT
                                })
                                .stroke(if active {
                                    Stroke::new(1.0_f32, p.accent.gamma_multiply(0.75))
                                } else {
                                    Stroke::NONE
                                })
                                .rounding(Rounding::same(RADIUS_SM))
                                .truncate(),
                            )
                            .on_hover_text(tip);
                        if resp.clicked() {
                            if view.entry_tab != tab {
                                view.replace_targets.clear();
                            }
                            view.entry_tab = tab;
                        }
                    }
                });
            } else {
                // 极窄回退：仍强制每个 Tab >= 100px 宽度，避免单字竖排
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(gap, gap);
                    for tab in ImportEntryTab::ALL {
                        let active = view.entry_tab == tab;
                        let lbl = tab.label(locale);
                        let tip = tab.tip(locale);
                        let resp = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(&lbl)
                                        .size(FONT_BODY)
                                        .strong()
                                        .color(if active { p.accent } else { p.text_secondary }),
                                )
                                .fill(if active {
                                    p.card
                                } else {
                                    egui::Color32::TRANSPARENT
                                })
                                .stroke(if active {
                                    Stroke::new(1.0_f32, p.accent.gamma_multiply(0.75))
                                } else {
                                    Stroke::NONE
                                })
                                .rounding(Rounding::same(RADIUS_SM))
                                .min_size(vec2(100.0, tab_h))
                                .truncate(),
                            )
                            .on_hover_text(tip);
                        if resp.clicked() {
                            if view.entry_tab != tab {
                                view.replace_targets.clear();
                            }
                            view.entry_tab = tab;
                        }
                    }
                });
            }
        });
}

/// 用户自定义分组选择器 + 内联「+ 新建分组」控件（手动配置与文本导入共享）。
fn render_group_selector(
    ui: &mut Ui,
    registry: &Registry,
    view: &mut ImportViewState,
    locale: Locale,
    p: Palette,
) -> bool {
    let mut group_changed = false;
    let groups = view.all_groups(registry);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
        ui.label(
            RichText::new(translate(locale, "gui.import.group.label", &[]))
                .size(FONT_META)
                .strong()
                .color(p.text_secondary),
        );
        for g in &groups {
            let selected = view.selected_group == *g;
            if ui.selectable_label(selected, g).clicked() {
                view.selected_group = g.clone();
                group_changed = true;
            }
        }
        let input_w = (ui.available_width() - 108.0).clamp(140.0, 200.0);
        ui.add(
            egui::TextEdit::singleline(&mut view.new_group_input)
                .desired_width(input_w)
                .hint_text(translate(locale, "gui.import.group.new_hint", &[])),
        );
        if btn_outlined(
            ui,
            p,
            &translate(locale, "gui.import.group.create_btn", &[]),
            !view.new_group_input.trim().is_empty(),
        )
        .clicked()
        {
            let name = view.new_group_input.clone();
            match view.create_and_select_group(&name) {
                Ok(created) => {
                    view.feedback = Some(Ok(format!("已创建并选中自定义分组「{created}」。")));
                    group_changed = true;
                }
                Err(err) => {
                    view.feedback = Some(Err(err));
                }
            }
        }
    });
    group_changed
}

#[allow(clippy::too_many_arguments)]
fn render_manual_entry(
    ui: &mut Ui,
    registry: &Registry,
    view: &mut ImportViewState,
    locale: Locale,
    p: Palette,
    stacked_fields: bool,
    show_tech: bool,
) {
    subtle_well_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // 1. 所属分组选择 / 新建分组（问题 2）
            render_group_selector(ui, registry, view, locale, p);
            ui.add_space(SPACE_2);

            // 2. 网址 / 名称 / 备注输入（窄屏严格单列全宽 > 150px，绝不左右挤压）
            ui.label(
                RichText::new("目标网址（默认 https://）")
                    .size(FONT_META)
                    .color(p.text_secondary),
            );
            ui.add(
                egui::TextEdit::singleline(&mut view.single_url_input)
                    .desired_width(f32::INFINITY)
                    .hint_text("https://example.com/cdn-cgi/trace"),
            );
            ui.add_space(SPACE_1);

            if stacked_fields {
                ui.label(
                    RichText::new(if show_tech {
                        "名称标识（自动补 user. 前缀）"
                    } else {
                        "名称标识（选填）"
                    })
                    .size(FONT_META)
                    .color(p.text_secondary),
                );
                ui.add_space(SPACE_1);
                ui.add(
                    egui::TextEdit::singleline(&mut view.single_id_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("例如：my-office-site"),
                );
                ui.add_space(SPACE_2);
                ui.label(
                    RichText::new("备注说明（选填）")
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                ui.add_space(SPACE_1);
                ui.add(
                    egui::TextEdit::singleline(&mut view.surface_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("用途或说明备注"),
                );
            } else {
                spaced_columns(ui, 2, SPACE_2, |col_ui, idx| {
                    if idx == 0 {
                        col_ui.label(
                            RichText::new("名称标识（选填）")
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        col_ui.add(
                            egui::TextEdit::singleline(&mut view.single_id_input)
                                .desired_width(f32::INFINITY)
                                .hint_text("例如：my-office-site"),
                        );
                    } else {
                        col_ui.label(
                            RichText::new("备注说明（选填）")
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        col_ui.add(
                            egui::TextEdit::singleline(&mut view.surface_input)
                                .desired_width(f32::INFINITY),
                        );
                    }
                });
            }

            ui.add_space(SPACE_2);
            ui.label(
                RichText::new("选择检查方式（可多选，悬停查看说明）：")
                    .size(FONT_META)
                    .strong()
                    .color(p.text_secondary),
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                for cat in InspectionCategory::ALL {
                    let rule_id = cat.default_builtin_rule_id();
                    let mut checked = view.selected_signals.contains(rule_id);
                    let (proves, cannot_prove) = cat.proof_boundary();
                    let rule_tip = if show_tech {
                        format!(
                            "{}\n【能证明】{proves}\n【不能证明】{cannot_prove}\n（内置规则：{rule_id}）",
                            cat.label()
                        )
                    } else {
                        format!(
                            "{}\n【能证明】{proves}\n【不能证明】{cannot_prove}",
                            cat.label()
                        )
                    };
                    if ui
                        .checkbox(&mut checked, RichText::new(cat.short_label()).size(FONT_META))
                        .on_hover_text(rule_tip)
                        .changed()
                    {
                        if checked {
                            view.selected_signals.insert(rule_id.to_string());
                        } else {
                            view.selected_signals.remove(rule_id);
                        }
                    }
                }
            });

            // 展示本次包内新增规则与已登记用户规则供勾选绑定
            let mut extra_rule_ids: Vec<(String, String, bool)> = Vec::new();
            for r in &view.manual_rules {
                extra_rule_ids.push((r.id.clone(), r.extract.kind.clone(), true));
            }
            for ur in registry.user_rules() {
                if !extra_rule_ids.iter().any(|(id, _, _)| id == &ur.id) {
                    extra_rule_ids.push((ur.id.clone(), ur.kind.clone(), false));
                }
            }
            if !extra_rule_ids.is_empty() {
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                    ui.label(
                        RichText::new("自定义检查方式：")
                            .size(FONT_META)
                            .strong()
                            .color(p.accent),
                    );
                    for (rid, kind, is_staged) in extra_rule_ids {
                        let mut checked = view.selected_signals.contains(&rid);
                        let tag = if is_staged { "待导入" } else { "已登记" };
                        let disp = if show_tech {
                            format!("{rid} [{kind} · {tag}]")
                        } else {
                            format!(
                                "{}（{}）",
                                rid.strip_prefix("rule.").unwrap_or(&rid),
                                tag
                            )
                        };
                        let resp = ui
                            .checkbox(&mut checked, RichText::new(disp).size(FONT_META));
                        let resp = if show_tech {
                            resp.on_hover_text(format!("{rid} [{kind}]"))
                        } else {
                            resp
                        };
                        if resp.changed() {
                            if checked {
                                view.selected_signals.insert(rid);
                            } else {
                                view.selected_signals.remove(&rid);
                            }
                        }
                    }
                });
            }

            ui.add_space(SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                ui.checkbox(&mut view.allow_insecure_http, "允许 http://");
                if btn_tonal(ui, p, "+ 加入待导入列表", true).clicked() {
                    let signals: Vec<String> = if view.selected_signals.is_empty() {
                        vec!["rule.http".to_string()]
                    } else {
                        view.selected_signals.iter().cloned().collect()
                    };
                    view.manual_targets.push(ImportTargetEntry {
                        id: (!view.single_id_input.trim().is_empty())
                            .then(|| view.single_id_input.trim().to_string()),
                        url: view.single_url_input.trim().to_string(),
                        surface: (!view.surface_input.trim().is_empty())
                            .then(|| view.surface_input.trim().to_string()),
                        signals,
                        allow_insecure_http: view.allow_insecure_http,
                        user_group: Some(view.selected_group.clone()),
                        replace_existing: false,
                    });
                    view.single_url_input.clear();
                    view.single_id_input.clear();
                }
                if btn_outlined(
                    ui,
                    p,
                    if view.show_inline_rule_editor {
                        "收起自定义规则"
                    } else {
                        "+ 自定义检查规则"
                    },
                    true,
                )
                .clicked()
                {
                    view.show_inline_rule_editor = !view.show_inline_rule_editor;
                }
                if (!view.manual_targets.is_empty() || !view.manual_rules.is_empty())
                    && btn_outlined(ui, p, "清空草稿", true).clicked()
                {
                    view.manual_targets.clear();
                    view.manual_rules.clear();
                }
            });

            // 可选折叠：自定义检查规则（默认人话标签，技术细节收进悬停提示）
            if view.show_inline_rule_editor {
                ui.add_space(SPACE_3);
                ui.separator();
                ui.add_space(SPACE_2);
                ui.label(
                    RichText::new("添加自定义检查规则（添加后将自动绑定到本次导入列表）")
                        .size(FONT_META)
                        .strong()
                        .color(p.warning_fg),
                );
                ui.add_space(SPACE_2);

                ui.label(
                    RichText::new("规则名称")
                        .size(FONT_META)
                        .color(p.text_secondary),
                )
                .on_hover_text("自动补全 rule. 前缀");
                ui.add(
                    egui::TextEdit::singleline(&mut view.rule_id_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("例如：my-region-rule"),
                );
                ui.add_space(SPACE_2);

                ui.label(
                    RichText::new("检查方式")
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                egui::ComboBox::from_id_salt("bundle_rule_cat_combo")
                    .width((ui.available_width() - SPACE_2).max(160.0))
                    .selected_text(if show_tech {
                        format!(
                            "{} [{}]",
                            view.rule_category.short_label(),
                            view.rule_category.reader_kind()
                        )
                    } else {
                        view.rule_category.label().to_string()
                    })
                    .show_ui(ui, |ui| {
                        for cat in InspectionCategory::ALL {
                            let opt = if show_tech {
                                format!("{} [{}]", cat.label(), cat.reader_kind())
                            } else {
                                cat.label().to_string()
                            };
                            if ui
                                .selectable_value(&mut view.rule_category, cat, opt)
                                .clicked()
                            {
                                let (pat, cap) = cat.default_pattern_and_capture();
                                view.rule_pattern_input = pat.to_string();
                                view.rule_capture_input = cap.to_string();
                            }
                        }
                    });
                ui.add_space(SPACE_2);

                ui.label(
                    RichText::new("匹配内容")
                        .size(FONT_META)
                        .color(p.text_secondary),
                )
                .on_hover_text("支持安全的线性时间匹配表达式，自动拒绝高风险回溯模式");
                ui.add(
                    egui::TextEdit::singleline(&mut view.rule_pattern_input)
                        .desired_width(f32::INFINITY)
                        .code_editor(),
                );
                ui.add_space(SPACE_2);

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                    ui.label(
                        RichText::new("提取位置：")
                            .size(FONT_META)
                            .color(p.text_secondary),
                    );
                    egui::ComboBox::from_id_salt("bundle_rule_cap_combo")
                        .width(80.0)
                        .selected_text(&view.rule_capture_input)
                        .show_ui(ui, |ui| {
                            for cap in ["value", "1", "0"] {
                                ui.selectable_value(
                                    &mut view.rule_capture_input,
                                    cap.to_string(),
                                    cap,
                                );
                            }
                        });
                    ui.label(
                        RichText::new("归属层：")
                            .size(FONT_META)
                            .color(p.text_secondary),
                    );
                    egui::ComboBox::from_id_salt("bundle_rule_layer_combo")
                        .width(120.0)
                        .selected_text(match view.rule_layer {
                            Layer::ServicePolicy => "服务策略",
                            Layer::EgressNetwork => "出口网络",
                            Layer::Preference => "公开版次",
                            _ => "服务策略",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut view.rule_layer,
                                Layer::ServicePolicy,
                                "服务策略",
                            );
                            ui.selectable_value(
                                &mut view.rule_layer,
                                Layer::EgressNetwork,
                                "出口网络",
                            );
                            ui.selectable_value(
                                &mut view.rule_layer,
                                Layer::Preference,
                                "公开版次",
                            );
                        });
                    if btn_tonal(ui, p, "添加到本次规则列表", true).clicked() {
                        let raw_id = view.rule_id_input.trim();
                        let rule_id = if raw_id.starts_with("rule.") {
                            raw_id.to_string()
                        } else {
                            format!("rule.{raw_id}")
                        };
                        let rule = Rule {
                            id: rule_id.clone(),
                            version: 1,
                            enabled: true,
                            layer: view.rule_layer,
                            channel: view.rule_channel.clone(),
                            credibility: Confidence::Low,
                            source_grade: Source::DirectAnonymousObservation,
                            notes: view.rule_notes_input.trim().to_string(),
                            negative_case: "自定义规则未命中时仅记缺失，不推断国家".to_string(),
                            extract: Extractor {
                                kind: view.rule_category.reader_kind().to_string(),
                                pattern: view.rule_pattern_input.trim().to_string(),
                                capture: view.rule_capture_input.trim().to_string(),
                            },
                            applies_to: Vec::new(),
                        };
                        view.manual_rules.retain(|r| r.id != rule_id);
                        view.manual_rules.push(rule);
                        view.selected_signals.insert(rule_id);
                    }
                });
            }
        });
}

fn render_text_file_entry(
    ui: &mut Ui,
    registry: &Registry,
    view: &mut ImportViewState,
    locale: Locale,
    p: Palette,
    _stacked: bool,
) {
    subtle_well_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // 1. 默认分组选择 / 新建分组（用于未指定分组的行）
            let mut reparse = render_group_selector(ui, registry, view, locale, p);
            ui.add_space(SPACE_3);

            // 2. 桌面端：系统文件选择按钮 + 示例按钮；移动端：诚实提示，直接用下方粘贴框
            if cfg!(target_os = "android") || cfg!(target_os = "ios") {
                render_admonition(ui, p.info_bg, p.info_fg, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(translate(
                                locale,
                                "gui.import.file.mobile_paste_hint",
                                &[],
                            ))
                            .size(FONT_META)
                            .color(p.text_primary),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(SPACE_2);
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                    if btn_tonal(
                        ui,
                        p,
                        &translate(locale, "gui.import.file.pick_text", &[]),
                        true,
                    )
                    .clicked()
                    {
                        match view
                            .pick_file("请选择要导入的网址文本文件", ImportFileFilter::TextBundle)
                        {
                            Ok(Some(path)) => {
                                view.load_text_file(&path);
                            }
                            Ok(None) => {
                                view.feedback =
                                    Some(Ok("已取消文件选择，未更改当前内容。".to_string()));
                            }
                            Err(err) => {
                                view.text_parse_error = Some(err);
                            }
                        }
                    }
                    if btn_outlined(ui, p, "载入分组示例", true).clicked() {
                        view.text_editor_content = SAMPLE_TEXT_LINES.to_string();
                        reparse = true;
                    }
                });
                if !view.text_file_path.is_empty() {
                    ui.add_space(SPACE_1);
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("当前文件：{}", view.text_file_path))
                                .size(FONT_META)
                                .color(p.text_muted),
                        )
                        .truncate(),
                    );
                }
                ui.add_space(SPACE_2);
            }

            // 3. 多行粘贴文本框（桌面与移动均可直接粘贴，宽度 100% 占满）
            let resp = ui.add(
                egui::TextEdit::multiline(&mut view.text_editor_content)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
            if resp.changed() {
                reparse = true;
            }
            ui.add_space(SPACE_2);

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                if ui
                    .checkbox(&mut view.text_allow_insecure_http, "允许 http://")
                    .changed()
                {
                    reparse = true;
                }
                if btn_outlined(ui, p, "刷新预览", true).clicked() {
                    reparse = true;
                }
                ui.add(
                    egui::Label::new(
                        RichText::new("每行一条网址，可用「[分组名]」或「分组名 | 网址」归类")
                            .size(FONT_META)
                            .color(p.text_muted),
                    )
                    .wrap(),
                );
            });

            if reparse {
                match parse_import_text_with_default_group(
                    &view.text_editor_content,
                    &[],
                    view.text_allow_insecure_http,
                    Some(&view.selected_group),
                ) {
                    Ok(b) => {
                        view.text_loaded_bundle = Some(b);
                        view.text_parse_error = None;
                    }
                    Err(e) => {
                        view.text_loaded_bundle = None;
                        view.text_parse_error = Some(e.to_string());
                    }
                }
            }
            if let Some(err) = &view.text_parse_error {
                ui.add_space(SPACE_2);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("解析失败：{err}"))
                            .size(FONT_META)
                            .color(p.danger_fg),
                    )
                    .wrap(),
                );
            }
        });
}

fn render_zip_bundle_entry(ui: &mut Ui, view: &mut ImportViewState, locale: Locale, p: Palette) {
    subtle_well_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if cfg!(target_os = "android") || cfg!(target_os = "ios") {
                render_admonition(ui, p.info_bg, p.info_fg, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(translate(locale, "gui.import.file.mobile_zip_hint", &[]))
                                .size(FONT_META)
                                .color(p.text_primary),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(SPACE_2);
                if btn_outlined(ui, p, "载入内置示例 ZIP 包（含规则+网址）", true).clicked() {
                    view.load_sample_zip_bundle();
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                    if btn_tonal(
                        ui,
                        p,
                        &translate(locale, "gui.import.file.pick_zip", &[]),
                        true,
                    )
                    .clicked()
                    {
                        match view
                            .pick_file("请选择本地 ZIP 导入包", ImportFileFilter::ZipBundle)
                        {
                            Ok(Some(path)) => {
                                view.load_zip_file(&path);
                            }
                            Ok(None) => {
                                view.feedback =
                                    Some(Ok("已取消选择 ZIP 包，未更改当前内容。".to_string()));
                            }
                            Err(err) => {
                                view.zip_parse_error = Some(err);
                            }
                        }
                    }
                    if btn_outlined(ui, p, "载入内置示例 ZIP 包（含规则+网址）", true).clicked() {
                        view.load_sample_zip_bundle();
                    }
                });
                if !view.zip_file_path.is_empty() {
                    ui.add_space(SPACE_1);
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("当前压缩包：{}", view.zip_file_path))
                                .size(FONT_META)
                                .color(p.text_muted),
                        )
                        .truncate(),
                    );
                }
            }
            ui.add_space(SPACE_2);
            ui.add(
                egui::Label::new(
                    RichText::new("安全校验：单包 ≤ 10 MiB、解压 ≤ 50 MiB，自动拦截路径穿越、符号链接与脚本文件。")
                        .size(FONT_META)
                        .color(p.text_muted),
                )
                .wrap(),
            );
            if let Some(err) = &view.zip_parse_error {
                ui.add_space(SPACE_2);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("压缩包校验未通过：{err}"))
                            .size(FONT_META)
                            .color(p.danger_fg),
                    )
                    .wrap(),
                );
            }
        });
}

fn render_unified_preview_table(
    ui: &mut Ui,
    view: &mut ImportViewState,
    preview: &parallaxe_core::import_bundle::BundlePreviewReport,
    readme: Option<&str>,
    locale: Locale,
    p: Palette,
    show_tech: bool,
) {
    ui.label(
        RichText::new(translate(locale, "gui.import.preview.title", &[]))
            .size(FONT_BODY)
            .strong()
            .color(p.text_primary),
    );
    ui.add_space(SPACE_1);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_1, SPACE_1);
        let src_key = match preview.source {
            ImportSource::Manual => "gui.import.source.manual",
            ImportSource::TextFile => "gui.import.source.text",
            ImportSource::ZipBundle => "gui.import.source.zip",
        };
        render_pill(
            ui,
            &translate(locale, src_key, &[]),
            p.info_fg,
            p.info_bg,
            p.info_fg,
        );
        render_pill(
            ui,
            &format!("{} 条网址", preview.targets.len()),
            p.accent,
            p.accent_soft,
            p.accent,
        );
        if !preview.rules.is_empty() {
            render_pill(
                ui,
                &format!("+{} 条自定义规则", preview.rules.len()),
                p.warning_fg,
                p.warning_bg,
                p.warning_fg,
            );
        }
        if preview.all_valid {
            render_pill(
                ui,
                &translate(locale, "gui.import.preview.all_valid", &[]),
                p.ok_fg,
                p.ok_bg,
                p.ok_fg,
            );
        } else {
            render_pill(
                ui,
                &translate(locale, "gui.import.preview.has_error", &[]),
                p.danger_fg,
                p.danger_bg,
                p.danger_fg,
            );
        }
    });

    if let Some(rm) = readme {
        ui.add_space(SPACE_2);
        subtle_well_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(format!("导入包说明（只读预览）：{}", rm.trim()))
                        .size(FONT_META)
                        .color(p.text_secondary),
                )
                .wrap(),
            );
        });
    }

    if !preview.rules.is_empty() {
        ui.add_space(SPACE_2);
        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .inner_margin(Margin::same(SPACE_2))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("将新增的自定义检查方式")
                        .size(FONT_META)
                        .strong()
                        .color(p.warning_fg),
                );
                ui.add_space(SPACE_1);
                for rr in &preview.rules {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                        let rule_title = if show_tech {
                            rr.id.clone()
                        } else {
                            rr.id.strip_prefix("rule.").unwrap_or(&rr.id).to_string()
                        };
                        let rule_resp = ui.label(
                            RichText::new(rule_title)
                                .strong()
                                .size(FONT_META)
                                .color(p.text_primary),
                        );
                        if show_tech {
                            rule_resp.on_hover_text(&rr.id);
                        }
                        render_pill(
                            ui,
                            &friendly_reader_kind(&rr.kind, show_tech),
                            p.info_fg,
                            p.info_bg,
                            p.info_fg,
                        );
                        if show_tech {
                            ui.label(
                                RichText::new(format!("pattern={}", rr.pattern))
                                    .monospace()
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                            );
                        } else {
                            ui.label(
                                RichText::new(format!("匹配：{}", rr.pattern))
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                            );
                        }
                        if rr.valid {
                            render_pill(
                                ui,
                                &translate(locale, "gui.import.preview.valid", &[]),
                                p.ok_fg,
                                p.ok_bg,
                                p.ok_fg,
                            );
                        } else {
                            ui.colored_label(
                                p.danger_fg,
                                format!("错误：{}", rr.error.as_deref().unwrap_or("校验失败")),
                            );
                        }
                    });
                }
            });
    }

    // 按用户自定义分组折叠展示待导入网址（问题 2）
    if !preview.targets.is_empty() {
        ui.add_space(SPACE_2);
        let mut grouped: BTreeMap<&str, Vec<&BundleTargetPreviewRow>> = BTreeMap::new();
        for row in &preview.targets {
            grouped
                .entry(row.user_group.as_str())
                .or_default()
                .push(row);
        }
        let outer_w = ui.available_width();
        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .inner_margin(Margin::same(SPACE_2))
            .show(ui, |ui| {
                ui.set_width((outer_w - SPACE_4).max(160.0));
                ScrollArea::vertical()
                    .id_salt("import_bundle_preview_vscroll")
                    .max_height(240.0)
                    .min_scrolled_height(180.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_width(0.0);
                        for (group_name, rows) in grouped {
                            let header = format!("{group_name} · {} 项", rows.len());
                            egui::CollapsingHeader::new(
                                RichText::new(header)
                                    .size(FONT_META)
                                    .strong()
                                    .color(p.accent),
                            )
                            .id_salt(("import_preview_group", group_name))
                            .default_open(true)
                            .show(ui, |ui| {
                                for (idx, row) in rows.iter().enumerate() {
                                    if idx > 0 {
                                        ui.add_space(SPACE_1);
                                        let rect =
                                            ui.allocate_space(vec2(ui.available_width(), 1.0)).1;
                                        ui.painter().rect_filled(
                                            rect,
                                            Rounding::ZERO,
                                            p.border_subtle,
                                        );
                                        ui.add_space(SPACE_1);
                                    }
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = vec2(SPACE_1, SPACE_1);
                                        // 交接文档 1.3：预览默认显示完整 `user.*` ID（不再隐藏前缀）。
                                        let disp_id = row.planned_id.clone();
                                        ui.label(
                                            RichText::new(disp_id)
                                                .strong()
                                                .size(FONT_META)
                                                .color(p.text_primary),
                                        );
                                        if row.duplicate_of.is_some() {
                                            let mut replace = view.replace_targets.contains(&row.index);
                                            if ui
                                                .checkbox(
                                                    &mut replace,
                                                    RichText::new(if locale == Locale::EnUs {
                                                        "Replace"
                                                    } else {
                                                        "更换"
                                                    })
                                                    .size(FONT_META)
                                                    .strong()
                                                    .color(p.warning_fg),
                                                )
                                                .on_hover_text(if locale == Locale::EnUs {
                                                    "Create your authorized edition for this duplicate URL"
                                                } else {
                                                    "为该重复网址新建「我的授权版」目标"
                                                })
                                                .changed()
                                            {
                                                if replace {
                                                    view.replace_targets.insert(row.index);
                                                } else {
                                                    view.replace_targets.remove(&row.index);
                                                }
                                            }
                                            render_pill(
                                                ui,
                                                if row.duplicate_is_user {
                                                    if locale == Locale::EnUs {
                                                        "Already imported"
                                                    } else {
                                                        "已导入"
                                                    }
                                                } else if locale == Locale::EnUs {
                                                    "Duplicate of built-in"
                                                } else {
                                                    "与内置重复"
                                                },
                                                p.info_fg,
                                                p.info_bg,
                                                p.info_fg,
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
                                        }
                                        for (sig, is_new) in &row.bound_signals {
                                            let lbl = friendly_signal_label(sig, show_tech);
                                            if *is_new {
                                                let pill = render_pill(
                                                    ui,
                                                    &format!("{lbl} · 新增"),
                                                    p.warning_fg,
                                                    p.warning_bg,
                                                    p.warning_fg,
                                                );
                                                if show_tech {
                                                    pill.on_hover_text(sig);
                                                }
                                            } else {
                                                let pill = render_pill(
                                                    ui, &lbl, p.info_fg, p.info_bg, p.info_fg,
                                                );
                                                if show_tech {
                                                    pill.on_hover_text(sig);
                                                }
                                            }
                                        }
                                        if row.valid {
                                            render_pill(
                                                ui,
                                                &translate(locale, "gui.import.preview.ready", &[]),
                                                p.ok_fg,
                                                p.ok_bg,
                                                p.ok_fg,
                                            );
                                        } else {
                                            ui.colored_label(
                                                p.danger_fg,
                                                format!(
                                                    "错误：{}",
                                                    row.error.as_deref().unwrap_or("校验失败")
                                                ),
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
                        }
                    });
            });
    }
}

#[allow(clippy::too_many_arguments)]
fn render_dynamic_consent_and_submit(
    ui: &mut Ui,
    registry: &mut Registry,
    view: &mut ImportViewState,
    bundle: &ImportBundle,
    preview: &parallaxe_core::import_bundle::BundlePreviewReport,
    locale: Locale,
    p: Palette,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    if locale == Locale::EnUs {
        ui.add(
            egui::Label::new(
                RichText::new(translate(locale, "gui.import.legal_en_banner", &[]))
                    .size(FONT_META)
                    .color(p.text_muted),
            )
            .wrap(),
        );
        ui.add_space(SPACE_1);
    }
    let need_both = preview.requires_target_consent && preview.requires_rule_consent;
    let t_cnt = bundle.targets.len().to_string();
    let r_cnt = bundle.rules.len().to_string();
    let unresolved_duplicates = preview
        .targets
        .iter()
        .filter(|row| {
            row.duplicate_of.is_some()
                && !bundle
                    .targets
                    .get(row.index.saturating_sub(1))
                    .is_some_and(|entry| entry.replace_existing)
        })
        .count();
    let duplicate_hint = if unresolved_duplicates > 0 {
        Some(if locale == Locale::EnUs {
            format!(
                "{unresolved_duplicates} duplicate URL(s) still need an explicit Replace decision."
            )
        } else {
            format!("还有 {unresolved_duplicates} 条重复网址未确认「更换」，默认不会导入。")
        })
    } else {
        None
    };
    let submit_label = if !bundle.rules.is_empty() {
        translate(
            locale,
            "gui.import.submit.targets_and_rules",
            &[&t_cnt, &r_cnt],
        )
    } else {
        translate(locale, "gui.import.submit.targets_only", &[&t_cnt])
    };

    let mut trigger_import = false;

    if preview.requires_target_consent && !preview.requires_rule_consent {
        render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
            ui.spacing_mut().item_spacing.y = SPACE_1;
            ui.label(
                RichText::new("【导入网址责任声明（必读原文）】")
                    .size(FONT_META)
                    .strong()
                    .color(p.warning_fg),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(USER_TARGET_LIABILITY_NOTICE)
                        .size(FONT_META)
                        .color(p.text_primary),
                )
                .wrap(),
            );
            ui.add_space(SPACE_1);
            let can_import =
                preview.all_valid && view.target_consent_checked && unresolved_duplicates == 0;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                ui.checkbox(
                    &mut view.target_consent_checked,
                    RichText::new(
                        "我已阅读并同意《导入自定义网址责任声明》（确认属于用户自身行为，后果本软件不负责）",
                    )
                    .strong()
                    .size(FONT_META),
                );
                if btn_filled(ui, p, &submit_label, can_import).clicked() {
                    trigger_import = true;
                }
            });
            if let Some(err) = &preview.summary_error {
                ui.label(RichText::new(err).size(FONT_META).color(p.danger_fg));
            }
            if let Some(hint) = &duplicate_hint {
                ui.label(RichText::new(hint).size(FONT_META).color(p.warning_fg));
            }
        });
    } else {
        if need_both && ui.available_width() >= 820.0 {
            spaced_columns(ui, 2, SPACE_3, |col_ui, idx| {
                if idx == 0 {
                    render_target_consent_box(col_ui, view, p);
                } else {
                    render_rule_consent_box(col_ui, view, p);
                }
            });
        } else {
            if preview.requires_target_consent {
                render_target_consent_box(ui, view, p);
            }
            if preview.requires_rule_consent {
                if preview.requires_target_consent {
                    ui.add_space(SPACE_2);
                }
                render_rule_consent_box(ui, view, p);
            }
        }

        ui.add_space(SPACE_2);
        let consents_ok = (!preview.requires_target_consent || view.target_consent_checked)
            && (!preview.requires_rule_consent || view.rule_consent_checked);
        let can_import = preview.all_valid && consents_ok && unresolved_duplicates == 0;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_3, SPACE_2);
            if btn_filled(ui, p, &submit_label, can_import).clicked() {
                trigger_import = true;
            }
            if !consents_ok {
                ui.label(
                    RichText::new("请先勾选上方所需的责任声明后方可导入")
                        .size(FONT_META)
                        .color(p.warning_fg),
                );
            } else if let Some(hint) = &duplicate_hint {
                ui.label(RichText::new(hint).size(FONT_META).color(p.warning_fg));
            } else if let Some(err) = &preview.summary_error {
                ui.label(RichText::new(err).size(FONT_META).color(p.danger_fg));
            }
        });
    }

    if trigger_import {
        match registry.import_bundle(
            bundle,
            view.target_consent_checked,
            view.rule_consent_checked,
        ) {
            Ok(report) => {
                actions.push(UiAction::SyncRegistry);
                let msg = if show_tech {
                    format!(
                        "已导入 {} 个自定义网址与 {} 条检查方式（origin={}，默认零发包；如需发起检查请点击上方「申请一次性在线检查」）。",
                        report.imported_targets.len(),
                        report.imported_rules.len(),
                        report.origin
                    )
                } else {
                    format!(
                        "已导入 {} 个自定义网址与 {} 条检查方式（默认零发包；如需在线检测，请点击上方「申请一次性在线检查」）。",
                        report.imported_targets.len(),
                        report.imported_rules.len()
                    )
                };
                view.feedback = Some(Ok(msg));
                if view.entry_tab == ImportEntryTab::Manual {
                    view.manual_targets.clear();
                    view.manual_rules.clear();
                }
            }
            Err(err) => {
                view.feedback = Some(Err(err.to_string()));
            }
        }
    }
}

fn render_target_consent_box(ui: &mut Ui, view: &mut ImportViewState, p: Palette) {
    render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
        ui.label(
            RichText::new("【导入网址责任声明（必读原文）】")
                .size(FONT_META)
                .strong()
                .color(p.warning_fg),
        );
        ui.add(
            egui::Label::new(
                RichText::new(USER_TARGET_LIABILITY_NOTICE)
                    .size(FONT_META)
                    .color(p.text_primary),
            )
            .wrap(),
        );
        ui.add_space(SPACE_1);
        ui.checkbox(
            &mut view.target_consent_checked,
            RichText::new(
                "我已阅读并同意《导入自定义网址责任声明》（确认属于用户自身行为，后果本软件不负责）",
            )
            .strong()
            .size(FONT_META),
        );
    });
}

fn render_rule_consent_box(ui: &mut Ui, view: &mut ImportViewState, p: Palette) {
    render_admonition(ui, p.danger_bg, p.danger_fg, |ui| {
        ui.label(
            RichText::new("【导入检查方式责任声明（必读原文 · 更严格）】")
                .size(FONT_META)
                .strong()
                .color(p.danger_fg),
        );
        ui.add(
            egui::Label::new(
                RichText::new(USER_RULE_LIABILITY_NOTICE)
                    .size(FONT_META)
                    .color(p.text_primary),
            )
            .wrap(),
        );
        ui.add_space(SPACE_1);
        ui.checkbox(
            &mut view.rule_consent_checked,
            RichText::new(
                "我已阅读并同意《导入自定义检查方式责任声明》（确认非本软件内置行为，后果由我承担全部责任）",
            )
            .strong()
            .size(FONT_META),
        );
    });
}

#[allow(clippy::too_many_arguments)]
fn render_gate_header_card(
    ui: &mut Ui,
    registry: &mut Registry,
    view: &mut ImportViewState,
    locale: Locale,
    p: Palette,
    tier: ViewportTier,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let user_targets_count = registry.user_targets().len();
    let user_rules_count = registry.user_rules().len();
    let active_grant = registry.active_session_grant_snapshot();
    let has_targets = user_targets_count > 0;
    let t_str = user_targets_count.to_string();
    let r_str = user_rules_count.to_string();

    // 移动端尚未导入任何目标时，精简为紧凑单行状态带，确保 360×640 首屏直接看到三入口与输入控件
    if tier == ViewportTier::Narrow && !has_targets && user_rules_count == 0 {
        subtle_well_frame(p)
            .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                    render_pill(
                        ui,
                        &translate(locale, "gui.import.gate.zero_packet_pill", &[]),
                        p.info_fg,
                        p.info_bg,
                        p.info_fg,
                    );
                    ui.label(
                        RichText::new(translate(
                            locale,
                            "gui.import.gate.counts",
                            &[&t_str, &r_str],
                        ))
                        .size(FONT_META)
                        .color(p.text_secondary),
                    );
                });
            });
        return;
    }

    card_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                icon_tile(ui, IconKind::Lock, 24.0, p.accent, p.accent_soft);
                ui.label(
                    RichText::new(translate(locale, "gui.import.gate.title", &[]))
                        .size(FONT_TITLE)
                        .strong()
                        .color(p.text_primary),
                );
                let origin_pill_text = if show_tech {
                    format!(
                        "{} (origin={ORIGIN_USER_IMPORTED})",
                        translate(locale, "gui.import.gate.origin_pill", &[])
                    )
                } else {
                    translate(locale, "gui.import.gate.origin_pill", &[])
                };
                render_pill(
                    ui,
                    &origin_pill_text,
                    p.warning_fg,
                    p.warning_bg,
                    p.warning_fg,
                );
                if show_tech {
                    ui.label(
                        RichText::new(format!("origin={ORIGIN_USER_IMPORTED}"))
                            .monospace()
                            .size(FONT_META)
                            .color(p.text_muted),
                    );
                }
                let zero_pill = render_pill(
                    ui,
                    &translate(locale, "gui.import.gate.zero_packet_pill", &[]),
                    p.info_fg,
                    p.info_bg,
                    p.info_fg,
                );
                if show_tech {
                    zero_pill.on_hover_text(format!(
                        "allow_online=false · consent={LIABILITY_CONSENT_VERSION}"
                    ));
                }
                ui.label(
                    RichText::new(translate(
                        locale,
                        "gui.import.gate.counts",
                        &[&t_str, &r_str],
                    ))
                    .size(FONT_META)
                    .color(p.text_secondary),
                );
                let offline_label = translate(locale, "gui.import.gate.offline_preview", &[&t_str]);
                if btn_tonal(ui, p, &offline_label, has_targets)
                    .on_hover_text("使用内置合成响应样本离线演练您导入的目标与规则，网络请求数为 0")
                    .clicked()
                {
                    let ids: Vec<String> = registry
                        .user_targets()
                        .into_iter()
                        .map(|m| m.id.clone())
                        .collect();
                    actions.push(UiAction::RunUserImportOfflinePreview(ids));
                }

                if btn_outlined(
                    ui,
                    p,
                    &translate(locale, "gui.import.gate.request_online", &[]),
                    has_targets,
                )
                .on_hover_text("弹出运行前二次确认，仅对单次运行注入一次性可撤销许可")
                .clicked()
                {
                    view.online_confirm_open = true;
                    view.online_run_confirmed_checkbox = false;
                }

                if active_grant.is_some()
                    && btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.import.gate.revoke", &[]),
                        true,
                    )
                    .clicked()
                {
                    registry.revoke_session_grant();
                    actions.push(UiAction::SyncRegistry);
                    view.feedback = Some(Ok(
                        "已撤销一次性会话许可；后续运行对导入目标保持零发包。".to_string(),
                    ));
                }

                if (user_targets_count > 0 || user_rules_count > 0)
                    && btn_danger_tonal(
                        ui,
                        p,
                        &translate(locale, "gui.import.gate.clear_all", &[]),
                        true,
                    )
                    .clicked()
                {
                    registry.clear_user_imports();
                    view.online_confirm_open = false;
                    view.inventory_group_filter = None;
                    actions.push(UiAction::SyncRegistry);
                    view.feedback = Some(Ok(
                        "已清除全部用户导入的网址、检查方式与会话许可，恢复至 135 项内置只读目录。"
                            .to_string(),
                    ));
                }
            });
        });
}

fn render_online_session_confirm_modal(
    ctx: &egui::Context,
    registry: &mut Registry,
    view: &mut ImportViewState,
    p: Palette,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        view.online_confirm_open = false;
        view.online_run_confirmed_checkbox = false;
        return;
    }

    render_modal_scrim(ctx, "f3_online_confirm_scrim", p);

    let ids: Vec<String> = registry
        .user_targets()
        .into_iter()
        .map(|m| m.id.clone())
        .collect();
    let prompt = registry
        .build_session_confirmation_prompt(&ids)
        .unwrap_or_else(|e| e.to_string());

    let mut window_open = view.online_confirm_open;
    let mut close_without_grant = false;
    let mut close_after_grant = false;

    let screen = ctx.screen_rect();
    let safe = crate::layout::SafeAreaInsets::for_platform();
    let narrow = screen.width() < 520.0;
    let h_margin = if screen.width() < 480.0 { 12.0 } else { 24.0 };
    let inner_margin = if screen.width() < 480.0 {
        SPACE_3
    } else {
        SPACE_5
    };
    let modal_inner_w =
        (screen.width() - h_margin * 2.0 - inner_margin * 2.0 - 4.0).clamp(220.0, 560.0);
    let safe_top = safe.top + 12.0;
    let safe_bottom = safe.bottom + 12.0;
    let offset_y = (safe_top - safe_bottom) * 0.5;
    let max_win_h =
        (screen.height() - safe_top - safe_bottom - inner_margin * 2.0 - 4.0).max(150.0);
    let max_body_h = (max_win_h - 110.0).max(88.0);

    egui::Area::new(egui::Id::new("f3_online_session_confirm_modal"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, offset_y])
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::window(&ctx.style())
                .fill(p.card)
                .stroke(Stroke::new(1.5_f32, p.warning_fg))
                .rounding(Rounding::same(RADIUS_LG))
                .inner_margin(Margin::same(inner_margin))
                .show(ui, |ui| {
                    ui.set_width(modal_inner_w);
                    ui.set_max_width(modal_inner_w);
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            vec2((modal_inner_w - 34.0).max(150.0), 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                let title = if show_tech {
                                    "【运行前确认】一次性在线检测授权 (F3)"
                                } else {
                                    "【运行前确认】一次性在线检测授权"
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(title)
                                            .size(if narrow { FONT_BODY } else { FONT_TITLE })
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
                                window_open = false;
                            }
                        });
                    });
                    ui.add_space(SPACE_2);
                    ui.separator();
                    ui.add_space(SPACE_2);

                    egui::ScrollArea::vertical()
                        .id_salt("f3_online_confirm_modal_scroll")
                        .max_height(max_body_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.set_width(modal_inner_w);
                            ui.set_max_width(modal_inner_w);
                            render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new("请逐项核对即将发起一次性在线检查的自定义目标")
                                            .size(FONT_BODY)
                                            .strong()
                                            .color(p.warning_fg),
                                    )
                                    .wrap(),
                                );
                                ui.add_space(SPACE_1);
                                for target in registry.user_targets() {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!(
                                                "· [{}] {}",
                                                target.user_group, target.url
                                            ))
                                            .size(FONT_META)
                                            .color(p.text_primary),
                                        )
                                        .wrap(),
                                    );
                                }
                                if show_tech {
                                    ui.add_space(SPACE_1);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&prompt)
                                                .monospace()
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                        )
                                        .wrap(),
                                    );
                                }
                            });

                            ui.add_space(SPACE_3);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        "安全限制：并发与响应大小受控、不自动跳转、遇验证码或限流立即停止。本次授权仅对单次检测生效，启动后即自动销毁，不写入内置目录。",
                                    )
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                            ui.add_space(SPACE_3);
                            ui.checkbox(
                                &mut view.online_run_confirmed_checkbox,
                                RichText::new(
                                    "我已确认上述网址为自有或获明确授权目标，知悉这是我自身行为并承担全部后果",
                                )
                                .size(FONT_BODY)
                                .strong(),
                            );
                            ui.add_space(SPACE_4);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                                let confirm_label = format!("确认授权并检测 ({} 项)", ids.len());
                                if btn_filled(
                                    ui,
                                    p,
                                    &confirm_label,
                                    view.online_run_confirmed_checkbox && !ids.is_empty(),
                                )
                                .clicked()
                                {
                                    match registry.authorize_session_once(
                                        &ids,
                                        view.online_run_confirmed_checkbox,
                                    ) {
                                        Ok(grant) => {
                                            close_after_grant = true;
                                            view.feedback = Some(Ok(if show_tech {
                                                format!(
                                                    "已注入一次性会话许可 ({}) 并启动单次检查。",
                                                    grant.grant_id
                                                )
                                            } else {
                                                "已完成一次性授权并启动单次在线检测。".to_string()
                                            }));
                                            actions.push(UiAction::SyncRegistry);
                                            actions.push(UiAction::RunCustomTargets(ids.clone()));
                                        }
                                        Err(err) => {
                                            view.feedback = Some(Err(err.to_string()));
                                        }
                                    }
                                }
                                if btn_outlined(ui, p, "取消（保持默认零发包）", true).clicked() {
                                    close_without_grant = true;
                                }
                            });
                        });
                });
        });

    if !window_open || close_without_grant || close_after_grant {
        view.online_confirm_open = false;
        view.online_run_confirmed_checkbox = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn render_imported_inventory_card(
    ui: &mut Ui,
    registry: &Registry,
    view: &mut ImportViewState,
    locale: Locale,
    p: Palette,
    show_tech: bool,
    actions: &mut Vec<UiAction>,
) {
    let user_targets = registry.user_targets();
    let user_rules = registry.user_rules();

    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
            ui.label(
                RichText::new(format!(
                    "已登记的用户导入清单（网址 {} 项 · 自定义规则 {} 项）",
                    user_targets.len(),
                    user_rules.len()
                ))
                .size(FONT_TITLE)
                .strong()
                .color(p.text_primary),
            );
            if !user_rules.is_empty() && !cfg!(any(target_os = "android", target_os = "ios")) {
                ui.add(
                    egui::TextEdit::singleline(&mut view.user_state_dir_input)
                        .desired_width(148.0),
                );
                if btn_outlined(ui, p, "保存规则到用户目录", true).clicked() {
                    let dir = Path::new(view.user_state_dir_input.trim());
                    match registry.save_user_rules_to_state_dir(dir) {
                        Ok(saved) => {
                            view.feedback = Some(Ok(format!(
                                "已将自定义检查方式保存至：{}（未修改内置目录）",
                                saved.display()
                            )));
                        }
                        Err(err) => {
                            view.feedback = Some(Err(err.to_string()));
                        }
                    }
                }
            }
        });
        ui.add_space(SPACE_2);

        if user_targets.is_empty() && user_rules.is_empty() {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "当前尚未导入自定义网址或检查方式。内置 107 个 Google 目标与 28 个对照目标保持只读不变。",
                        )
                        .size(FONT_BODY)
                        .color(p.text_secondary),
                    )
                    .wrap(),
                );
            });
            return;
        }

        // 按自定义分组聚合统计与筛选（问题 2）
        if !user_targets.is_empty() {
            let mut by_group: BTreeMap<String, Vec<&parallaxe_core::targets::UserTargetMeta>> =
                BTreeMap::new();
            for meta in &user_targets {
                by_group
                    .entry(meta.user_group.clone())
                    .or_default()
                    .push(meta);
            }

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                let all_lbl = format!(
                    "{} · {}",
                    translate(locale, "gui.import.group.all", &[]),
                    user_targets.len()
                );
                if ui
                    .selectable_label(view.inventory_group_filter.is_none(), all_lbl)
                    .clicked()
                {
                    view.inventory_group_filter = None;
                }
                for (grp, list) in &by_group {
                    let selected = view.inventory_group_filter.as_deref() == Some(grp.as_str());
                    if ui
                        .selectable_label(selected, format!("{grp} · {}", list.len()))
                        .clicked()
                    {
                        view.inventory_group_filter = Some(grp.clone());
                    }
                }
            });

            ui.add_space(SPACE_2);
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical()
                    .id_salt("imported_targets_inventory_scroll")
                    .max_height(220.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_width(0.0);
                        for (grp, list) in &by_group {
                            if let Some(filter) = &view.inventory_group_filter {
                                if filter != grp {
                                    continue;
                                }
                            }
                            let header = format!("{grp} · {} 项", list.len());
                            egui::CollapsingHeader::new(
                                RichText::new(header)
                                    .size(FONT_BODY)
                                    .strong()
                                    .color(p.accent),
                            )
                            .id_salt(("inventory_group", grp))
                            .default_open(true)
                            .show(ui, |ui| {
                                for (idx, meta) in list.iter().enumerate() {
                                    if idx > 0 {
                                        ui.add_space(SPACE_2);
                                    }
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                                        let disp_id = meta.id.clone();
                                        ui.label(
                                            RichText::new(disp_id)
                                                .strong()
                                                .size(FONT_META)
                                                .color(p.text_primary),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&meta.url)
                                                    .size(FONT_META)
                                                    .color(p.text_secondary),
                                            )
                                            .wrap(),
                                        );
                                        let sig_summary = meta
                                            .signals
                                            .iter()
                                            .map(|s| friendly_signal_label(s, show_tech))
                                            .collect::<Vec<_>>()
                                            .join("、");
                                        render_pill(
                                            ui,
                                            &sig_summary,
                                            p.info_fg,
                                            p.info_bg,
                                            p.info_fg,
                                        );
                                        if show_tech {
                                            ui.label(
                                                RichText::new(meta.signals.join(", "))
                                                    .monospace()
                                                    .size(FONT_META)
                                                    .color(p.text_muted),
                                            );
                                        }
                                        if ui.small_button("离线预览").clicked() {
                                            actions.push(UiAction::RunUserImportOfflinePreview(
                                                vec![meta.id.clone()],
                                            ));
                                        }
                                        if ui.small_button("查看说明").clicked() {
                                            actions.push(UiAction::SelectTarget(meta.id.clone()));
                                        }
                                    });
                                }
                            });
                        }
                    });
            });
        }

        if !user_rules.is_empty() {
            ui.add_space(SPACE_3);
            ui.label(
                RichText::new("已导入自定义检查方式（仅作用于用户导入目标，不修改内置目录）")
                    .size(FONT_META)
                    .strong()
                    .color(p.text_secondary),
            );
            ui.add_space(SPACE_1);
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (idx, rm) in user_rules.into_iter().enumerate() {
                    if idx > 0 {
                        ui.add_space(SPACE_1);
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                        let disp_id = if show_tech {
                            rm.id.clone()
                        } else {
                            rm.id.strip_prefix("rule.").unwrap_or(&rm.id).to_string()
                        };
                        let rule_resp = ui.label(
                            RichText::new(disp_id)
                                .strong()
                                .size(FONT_META)
                                .color(p.text_primary),
                        );
                        if show_tech {
                            rule_resp.on_hover_text(&rm.id);
                        }
                        render_pill(
                            ui,
                            &friendly_reader_kind(&rm.kind, show_tech),
                            p.warning_fg,
                            p.warning_bg,
                            p.warning_fg,
                        );
                        ui.label(
                            RichText::new(format!("匹配内容：{}", rm.pattern))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                    });
                }
            });
        }
    });
}
