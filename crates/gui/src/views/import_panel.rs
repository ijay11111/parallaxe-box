//! 统一导入逻辑（T-5-07 · `parallaxe.import/v1`）：
//! 将「自定义网址」与「自定义检查方式」统一为同一导入包（Import Bundle），
//! 提供三种入口（手动配置 / 文本文件 txt·md·csv / ZIP 导入包）+ 统一预览表 + 动态双层责任声明 + F3 一次性授权弹窗。
//!
//! 红线：
//! - 所有导入目标默认 `allow_online=false`、`safe_default=false`、`origin=USER_IMPORTED`，未获 F3 确认前绝对零发包；
//! - 责任声明按包内容动态强制：含目标必勾网址声明，含规则必额外勾更严格的检查方式声明；
//! - 原子导入：预览表逐项实时校验，导入时任一条目失败整体回滚。

use std::collections::BTreeSet;
use std::path::Path;

use egui::{vec2, Align, Layout, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::contracts::{
    Confidence, Layer, Source, LIABILITY_CONSENT_VERSION, ORIGIN_USER_IMPORTED,
    USER_RULE_LIABILITY_NOTICE, USER_TARGET_LIABILITY_NOTICE,
};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::import_bundle::{
    build_stored_zip_bytes, parse_import_text, parse_import_zip_bytes, read_import_text_file,
    read_import_zip, ImportBundle, ImportSource, ImportTargetEntry, IMPORT_BUNDLE_SCHEMA,
};
use parallaxe_core::targets::{Extractor, InspectionCategory, Registry, Rule};

use crate::icons::{icon_tile, IconKind};
use crate::layout::ViewportTier;
use crate::theme::{
    btn_danger_tonal, btn_filled, btn_outlined, btn_tonal, card_frame, render_admonition,
    render_modal_scrim, render_pill, spaced_columns, subtle_well_frame, Palette, ThemeMode,
    FONT_BODY, FONT_META, FONT_TITLE, RADIUS_LG, RADIUS_MD, RADIUS_SM, SPACE_1, SPACE_2, SPACE_3,
    SPACE_4, SPACE_5,
};
use crate::views::UiAction;

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

    pub const ALL: [ImportEntryTab; 3] = [
        ImportEntryTab::Manual,
        ImportEntryTab::TextFile,
        ImportEntryTab::ZipBundle,
    ];
}

/// 统一导入界面的持久交互状态。
pub struct ImportViewState {
    /// 当前选择的导入入口（手动 / 文本文件 / ZIP 导入包）。
    pub entry_tab: ImportEntryTab,

    /// 责任声明勾选状态（按当前 bundle 内容动态生效）。
    pub target_consent_checked: bool,
    pub rule_consent_checked: bool,

    // --- 入口 1：手动配置表单（支持逐网址独立绑定检查方式，并可随包附加自定义规则） ---
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

    // --- 入口 2：文本文件导入（txt / md / csv） ---
    pub text_file_path: String,
    pub text_editor_content: String,
    pub text_allow_insecure_http: bool,
    pub text_loaded_bundle: Option<ImportBundle>,
    pub text_parse_error: Option<String>,

    // --- 入口 3：ZIP 导入包 ---
    pub zip_file_path: String,
    pub zip_loaded_bundle: Option<ImportBundle>,
    pub zip_parse_error: Option<String>,

    /// 用户态规则保存目录（不写回内置 assets/）。
    pub user_state_dir_input: String,

    /// 运行前一次性在线许可确认弹层是否打开（F3）。
    pub online_confirm_open: bool,
    pub online_run_confirmed_checkbox: bool,

    /// 最近一次导入或授权操作反馈（`Ok(成功信息)` / `Err(错误提示)`）。
    pub feedback: Option<Result<String, String>>,
}

const SAMPLE_TEXT_LINES: &str = "# 格式：URL | 规则ID1,规则ID2 | 自定义ID | 表面说明\n\
https://edge-a.example.com/cdn-cgi/trace | rule.http,rule.trace | edge-a | 出口 Trace 与状态码\n\
https://edge-b.example.com/books | rule.http,rule.inline3 | edge-b | 内联三字地区码检查\n\
https://edge-c.example.com/status | rule.http | edge-c | 仅连通性状态码检查";

impl Default for ImportViewState {
    fn default() -> Self {
        let mut selected_signals = BTreeSet::new();
        selected_signals.insert("rule.trace".to_string());
        selected_signals.insert("rule.http".to_string());
        let initial_text_bundle = parse_import_text(SAMPLE_TEXT_LINES, &[], false).ok();
        Self {
            entry_tab: ImportEntryTab::Manual,
            target_consent_checked: false,
            rule_consent_checked: false,
            single_url_input: "https://icanhazip.example.com/cdn-cgi/trace".to_string(),
            single_id_input: "user.custom-trace".to_string(),
            surface_input: "用户自定义出口与可达性表面".to_string(),
            allow_insecure_http: false,
            selected_signals,
            manual_targets: Vec::new(),
            manual_rules: Vec::new(),
            show_inline_rule_editor: false,
            rule_id_input: "rule.user-region-code".to_string(),
            rule_category: InspectionCategory::BodyRegex,
            rule_pattern_input: "\"country_code\":\"([A-Z]{2})\"".to_string(),
            rule_capture_input: "1".to_string(),
            rule_layer: Layer::ServicePolicy,
            rule_channel: "SERVICE".to_string(),
            rule_notes_input: "提取页面内联两字母国家码（用户自定义规则）".to_string(),
            text_file_path: String::new(),
            text_editor_content: SAMPLE_TEXT_LINES.to_string(),
            text_allow_insecure_http: false,
            text_loaded_bundle: initial_text_bundle,
            text_parse_error: None,
            zip_file_path: String::new(),
            zip_loaded_bundle: None,
            zip_parse_error: None,
            user_state_dir_input: "user-custom-state".to_string(),
            online_confirm_open: false,
            online_run_confirmed_checkbox: false,
            feedback: None,
        }
    }
}

impl ImportViewState {
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
                        });
                    }
                } else {
                    bundle.targets = self.manual_targets.clone();
                }
                if bundle.targets.is_empty() && bundle.rules.is_empty() {
                    None
                } else {
                    Some(bundle)
                }
            }
            ImportEntryTab::TextFile => self.text_loaded_bundle.clone(),
            ImportEntryTab::ZipBundle => self.zip_loaded_bundle.clone(),
        }
    }

    fn load_sample_zip_bundle(&mut self) {
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
notes = "示例 ZIP 包随带自定义地区提取规则"
negative_case = "未命中时仅记缺失，不推断国家"
extract = { kind = "regex", pattern = "region=([A-Z]{2})", capture = "1" }
applies_to = []
"#;
        let targets_txt = "# ZIP 包内目标列表（可同时引用包内新增规则与内置规则）\n\
https://zip-edge-a.example.com/api | rule.zip-sample-region,rule.http | zip-edge-a | 绑定包内新增规则\n\
https://zip-edge-b.example.com/trace | rule.trace,rule.http | zip-edge-b | 绑定内置 Trace 规则\n";
        let readme = "示例导入包说明：包含 1 条自定义正则规则（rule.zip-sample-region）与 2 个自定义目标网址。README 仅作只读展示，不执行任何脚本。";

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
                    "已载入内置示例 ZIP 导入包（含 2 个网址 + 1 条包内规则），请在下方预览表核对后勾选声明并导入。"
                        .to_string(),
                ));
            }
            Err(err) => {
                self.zip_parse_error = Some(err.to_string());
            }
        }
    }
}

/// 渲染统一导入包主视图（三入口 + 实时预览表 + 动态责任声明 + 一次性授权弹窗）。
pub fn render(
    ui: &mut Ui,
    registry: &mut Registry,
    view: &mut ImportViewState,
    theme: ThemeMode,
    locale: Locale,
    tier: ViewportTier,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);

    // 1. 顶部已登记资产与 F3 授权闸门卡（紧凑单带，留出充足首屏空间）
    render_gate_header_card(ui, registry, view, locale, p, actions);
    ui.add_space(SPACE_3);

    // F3 模态二次确认弹窗
    if view.online_confirm_open {
        let ctx = ui.ctx().clone();
        render_online_session_confirm_modal(&ctx, registry, view, p, actions);
    }

    // 紧凑操作反馈条
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
            .inner_margin(Margin::symmetric(SPACE_3, SPACE_1))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    crate::icons::icon(ui, ic, 14.0, fg);
                    ui.label(
                        RichText::new(format!("{prefix}{text}"))
                            .size(FONT_META)
                            .strong()
                            .color(fg),
                    );
                });
            });
        ui.add_space(SPACE_2);
    }

    // 2. 统一导入包主工作卡片：三入口切换 + 输入区 + 统一预览表 + 动态责任声明 + 单一主按钮
    card_frame(p)
        .inner_margin(Margin::symmetric(SPACE_4, SPACE_3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // 标题 + 三入口分段条并排（宽屏同一行，窄屏自动折行），显著压缩纵向占用
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_2);
                icon_tile(ui, IconKind::Globe, 24.0, p.accent, p.accent_soft);
                ui.label(
                    RichText::new(translate(locale, "gui.import.bundle.title", &[]))
                        .size(FONT_TITLE)
                        .strong()
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_2);
                egui::Frame::none()
                    .fill(p.sidebar)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .rounding(Rounding::same(RADIUS_MD))
                    .inner_margin(Margin::same(3.0))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            for tab in ImportEntryTab::ALL {
                                let active = view.entry_tab == tab;
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(tab.label(locale))
                                                .size(FONT_META)
                                                .strong()
                                                .color(if active {
                                                    p.accent
                                                } else {
                                                    p.text_secondary
                                                }),
                                        )
                                        .fill(if active {
                                            p.card
                                        } else {
                                            egui::Color32::TRANSPARENT
                                        })
                                        .stroke(if active {
                                            Stroke::new(1.0_f32, p.border)
                                        } else {
                                            Stroke::NONE
                                        })
                                        .rounding(Rounding::same(RADIUS_SM))
                                        .min_size(vec2(0.0, 26.0)),
                                    )
                                    .clicked()
                                {
                                    view.entry_tab = tab;
                                }
                            }
                        });
                    });
            });

            ui.add_space(SPACE_2);
            let active_bundle = view.active_bundle();
            let preview_opt = active_bundle
                .as_ref()
                .map(|b| registry.preview_import_bundle(b));

            // 宽屏左右双栏（左：输入配置 | 右：统一实时预览表），窄屏上下堆叠；确保 1280×820 首屏免滚即可完整看到预览+责任声明+导入主按钮
            if tier != ViewportTier::Narrow && ui.available_width() >= 820.0 {
                spaced_columns(ui, 2, SPACE_3, |col_ui, idx| {
                    if idx == 0 {
                        match view.entry_tab {
                            ImportEntryTab::Manual => {
                                render_manual_entry(col_ui, registry, view, p, ViewportTier::Narrow)
                            }
                            ImportEntryTab::TextFile => render_text_file_entry(col_ui, view, p),
                            ImportEntryTab::ZipBundle => render_zip_bundle_entry(col_ui, view, p),
                        }
                    } else if let (Some(bundle), Some(preview)) = (&active_bundle, &preview_opt) {
                        render_unified_preview_table(
                            col_ui,
                            preview,
                            bundle.readme.as_deref(),
                            locale,
                            p,
                        );
                    } else {
                        subtle_well_frame(p).show(col_ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("请先在左侧填写网址、解析文本文件或载入 ZIP 导入包，解析完成后此处将展示逐行预览与校验状态。")
                                    .size(FONT_BODY)
                                    .color(p.text_secondary),
                            );
                        });
                    }
                });
            } else {
                match view.entry_tab {
                    ImportEntryTab::Manual => render_manual_entry(ui, registry, view, p, tier),
                    ImportEntryTab::TextFile => render_text_file_entry(ui, view, p),
                    ImportEntryTab::ZipBundle => render_zip_bundle_entry(ui, view, p),
                }
                ui.add_space(SPACE_3);
                if let (Some(bundle), Some(preview)) = (&active_bundle, &preview_opt) {
                    render_unified_preview_table(ui, preview, bundle.readme.as_deref(), locale, p);
                } else {
                    subtle_well_frame(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("请先在上方填写网址、解析文本文件或载入 ZIP 导入包，解析完成后此处将展示逐行预览与校验状态。")
                                .size(FONT_BODY)
                                .color(p.text_secondary),
                        );
                    });
                }
            }

            if let (Some(bundle), Some(preview)) = (&active_bundle, &preview_opt) {
                ui.add_space(SPACE_2);
                // 4. 动态两层责任声明 + 单一主导入按钮（全宽展示在双栏正下方，首屏立即可见）
                render_dynamic_consent_and_submit(
                    ui, registry, view, bundle, preview, locale, p, actions,
                );
            }
        });

    ui.add_space(SPACE_4);
    render_imported_inventory_card(ui, registry, view, p, actions);
}

fn render_manual_entry(
    ui: &mut Ui,
    registry: &Registry,
    view: &mut ImportViewState,
    p: Palette,
    tier: ViewportTier,
) {
    subtle_well_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if tier == ViewportTier::Narrow {
                ui.label(
                    RichText::new("目标网址（默认 https://，拒绝私有/回环/凭据）")
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut view.single_url_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("https://example.com/cdn-cgi/trace"),
                );
                ui.add_space(SPACE_1);
                spaced_columns(ui, 2, SPACE_2, |col_ui, idx| {
                    if idx == 0 {
                        col_ui.label(
                            RichText::new("目标 ID（自动补 user.）")
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        col_ui.add(
                            egui::TextEdit::singleline(&mut view.single_id_input)
                                .desired_width(f32::INFINITY),
                        );
                    } else {
                        col_ui.label(
                            RichText::new("表面说明")
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        col_ui.add(
                            egui::TextEdit::singleline(&mut view.surface_input)
                                .desired_width(f32::INFINITY),
                        );
                    }
                });
            } else {
                spaced_columns(ui, 3, SPACE_2, |col_ui, idx| match idx {
                    0 => {
                        col_ui.label(
                            RichText::new("目标网址（默认 https://）")
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        col_ui.add(
                            egui::TextEdit::singleline(&mut view.single_url_input)
                                .desired_width(f32::INFINITY)
                                .hint_text("https://example.com/cdn-cgi/trace"),
                        );
                    }
                    1 => {
                        col_ui.label(
                            RichText::new("目标 ID（自动补 user.）")
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        col_ui.add(
                            egui::TextEdit::singleline(&mut view.single_id_input)
                                .desired_width(f32::INFINITY)
                                .hint_text("user.my-target"),
                        );
                    }
                    _ => {
                        col_ui.label(
                            RichText::new("表面说明（用途备注）")
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

            ui.add_space(SPACE_1);
            ui.label(
                RichText::new("为该网址绑定检查方式（可多选，悬停查看 Reader 与证明边界）：")
                    .size(FONT_META)
                    .strong()
                    .color(p.text_secondary),
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, 2.0);
                for cat in InspectionCategory::ALL {
                    let rule_id = cat.default_builtin_rule_id();
                    let mut checked = view.selected_signals.contains(rule_id);
                    let (proves, cannot_prove) = cat.proof_boundary();
                    if ui
                        .checkbox(&mut checked, RichText::new(cat.short_label()).size(FONT_META))
                        .on_hover_text(format!(
                            "{} [{} · {}]\n【能证明】{proves}\n【不能证明】{cannot_prove}",
                            cat.label(),
                            rule_id,
                            cat.reader_kind()
                        ))
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

        // 展示本次包内新增规则与已登记用户规则供直接勾选绑定
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
            ui.add_space(SPACE_1);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("自定义检查方式：")
                        .size(FONT_META)
                        .strong()
                        .color(p.accent),
                );
                for (rid, kind, is_staged) in extra_rule_ids {
                    let mut checked = view.selected_signals.contains(&rid);
                    let tag = if is_staged { "包内新增" } else { "已登记" };
                    if ui
                        .checkbox(
                            &mut checked,
                            RichText::new(format!("{rid} [{kind} · {tag}]")).size(FONT_META),
                        )
                        .changed()
                    {
                        if checked {
                            view.selected_signals.insert(rid);
                        } else {
                            view.selected_signals.remove(&rid);
                        }
                    }
                }
            });
        }

        ui.add_space(SPACE_1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_2;
            ui.checkbox(&mut view.allow_insecure_http, "允许 http://");
            if btn_tonal(ui, p, "+ 加入导入包", true).clicked() {
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
                });
                view.single_url_input.clear();
                view.single_id_input.clear();
            }
            if btn_outlined(
                ui,
                p,
                if view.show_inline_rule_editor {
                    "收起检查方式定义"
                } else {
                    "+ 随包新增检查方式"
                },
                true,
            )
            .clicked()
            {
                view.show_inline_rule_editor = !view.show_inline_rule_editor;
            }
            if (!view.manual_targets.is_empty() || !view.manual_rules.is_empty())
                && btn_outlined(ui, p, "清空待导入草稿", true).clicked()
            {
                view.manual_targets.clear();
                view.manual_rules.clear();
            }
        });

        // 可选折叠：随包附加自定义检查方式（与目标同属一个 Bundle！）
        if view.show_inline_rule_editor {
            ui.add_space(SPACE_3);
            ui.separator();
            ui.add_space(SPACE_2);
            ui.label(
                RichText::new("随包新增自定义检查方式（添加后将自动加入上方绑定选项，并随本次导入包一并校验）")
                    .size(FONT_META)
                    .strong()
                    .color(p.warning_fg),
            );
            ui.add_space(SPACE_2);
            spaced_columns(ui, 3, SPACE_2, |col_ui, idx| match idx {
                0 => {
                    col_ui.label(RichText::new("规则 ID（rule. 开头）").size(FONT_META).color(p.text_secondary));
                    col_ui.add(
                        egui::TextEdit::singleline(&mut view.rule_id_input)
                            .desired_width(f32::INFINITY),
                    );
                }
                1 => {
                    col_ui.label(RichText::new("检查方式分类（9 种 Reader）").size(FONT_META).color(p.text_secondary));
                    egui::ComboBox::from_id_salt("bundle_rule_cat_combo")
                        .width((col_ui.available_width() - 4.0).max(110.0))
                        .selected_text(format!(
                            "{} [{}]",
                            view.rule_category.short_label(),
                            view.rule_category.reader_kind()
                        ))
                        .show_ui(col_ui, |ui| {
                            for cat in InspectionCategory::ALL {
                                if ui
                                    .selectable_value(
                                        &mut view.rule_category,
                                        cat,
                                        format!("{} [{}]", cat.label(), cat.reader_kind()),
                                    )
                                    .clicked()
                                {
                                    let (pat, cap) = cat.default_pattern_and_capture();
                                    view.rule_pattern_input = pat.to_string();
                                    view.rule_capture_input = cap.to_string();
                                }
                            }
                        });
                }
                _ => {
                    col_ui.label(RichText::new("提取模式 (pattern，防 ReDoS)").size(FONT_META).color(p.text_secondary));
                    col_ui.add(
                        egui::TextEdit::singleline(&mut view.rule_pattern_input)
                            .desired_width(f32::INFINITY)
                            .code_editor(),
                    );
                }
            });
            ui.add_space(SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                ui.label(RichText::new("捕获组：").size(FONT_META).color(p.text_secondary));
                egui::ComboBox::from_id_salt("bundle_rule_cap_combo")
                    .width(72.0)
                    .selected_text(&view.rule_capture_input)
                    .show_ui(ui, |ui| {
                        for cap in ["value", "1", "0"] {
                            ui.selectable_value(&mut view.rule_capture_input, cap.to_string(), cap);
                        }
                    });
                ui.label(RichText::new("归因层：").size(FONT_META).color(p.text_secondary));
                egui::ComboBox::from_id_salt("bundle_rule_layer_combo")
                    .width(132.0)
                    .selected_text(match view.rule_layer {
                        Layer::ServicePolicy => "服务策略",
                        Layer::EgressNetwork => "出口网络",
                        Layer::Preference => "公开版次",
                        _ => "服务策略",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut view.rule_layer, Layer::ServicePolicy, "服务策略");
                        ui.selectable_value(&mut view.rule_layer, Layer::EgressNetwork, "出口网络");
                        ui.selectable_value(&mut view.rule_layer, Layer::Preference, "公开版次");
                    });
                if btn_tonal(ui, p, "加入本次导入包的规则列表", true).clicked() {
                    let rule_id = view.rule_id_input.trim().to_string();
                    let rule = Rule {
                        id: rule_id.clone(),
                        version: 1,
                        enabled: true,
                        layer: view.rule_layer,
                        channel: view.rule_channel.clone(),
                        credibility: Confidence::Low,
                        source_grade: Source::DirectAnonymousObservation,
                        notes: view.rule_notes_input.trim().to_string(),
                        negative_case: "用户自定义规则未命中时仅记缺失，不推断国家".to_string(),
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

fn render_text_file_entry(ui: &mut Ui, view: &mut ImportViewState, p: Palette) {
    subtle_well_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_1, SPACE_1);
                ui.add(
                    egui::TextEdit::singleline(&mut view.text_file_path)
                        .desired_width(190.0)
                        .hint_text(r"本地路径 (.txt/.md/.csv/.json)"),
                );
                if btn_tonal(
                    ui,
                    p,
                    "读取本地文件",
                    !view.text_file_path.trim().is_empty(),
                )
                .clicked()
                {
                    let path = Path::new(view.text_file_path.trim());
                    match read_import_text_file(path, &[], view.text_allow_insecure_http) {
                        Ok(bundle) => {
                            if let Ok(raw) = std::fs::read_to_string(path) {
                                view.text_editor_content = raw;
                            }
                            view.text_loaded_bundle = Some(bundle);
                            view.text_parse_error = None;
                            view.feedback = Some(Ok(format!(
                                "已读取并解析文件 {}，请在右侧预览表核对绑定关系。",
                                path.display()
                            )));
                        }
                        Err(err) => {
                            view.text_parse_error = Some(err.to_string());
                            view.text_loaded_bundle = None;
                        }
                    }
                }
                if btn_outlined(ui, p, "载入 3 行多规则示例", true).clicked() {
                    view.text_editor_content = SAMPLE_TEXT_LINES.to_string();
                    match parse_import_text(
                        &view.text_editor_content,
                        &[],
                        view.text_allow_insecure_http,
                    ) {
                        Ok(b) => {
                            view.text_loaded_bundle = Some(b);
                            view.text_parse_error = None;
                        }
                        Err(e) => {
                            view.text_parse_error = Some(e.to_string());
                        }
                    }
                }
            });

            ui.add_space(SPACE_1);
            let resp = ui.add(
                egui::TextEdit::multiline(&mut view.text_editor_content)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
            ui.add_space(SPACE_1);
            let mut reparse = resp.changed();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                if ui
                    .checkbox(&mut view.text_allow_insecure_http, "允许 http://")
                    .changed()
                {
                    reparse = true;
                }
                if btn_outlined(ui, p, "刷新解析预览", true).clicked() {
                    reparse = true;
                }
                ui.label(
                    RichText::new("格式：URL | 规则1,规则2 | ID | 说明")
                        .size(FONT_META)
                        .color(p.text_muted),
                );
            });
            if reparse {
                match parse_import_text(
                    &view.text_editor_content,
                    &[],
                    view.text_allow_insecure_http,
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
                ui.add_space(SPACE_1);
                ui.colored_label(p.danger_fg, format!("解析失败：{err}"));
            }
        });
}

fn render_zip_bundle_entry(ui: &mut Ui, view: &mut ImportViewState, p: Palette) {
    subtle_well_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                ui.label(
                    RichText::new("本地 ZIP 导入包路径：")
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut view.zip_file_path)
                        .desired_width(260.0)
                        .hint_text(r"例如：G:\my-bundle.zip"),
                );
                if btn_tonal(
                    ui,
                    p,
                    "安全解包并预览",
                    !view.zip_file_path.trim().is_empty(),
                )
                .clicked()
                {
                    let path = Path::new(view.zip_file_path.trim());
                    match read_import_zip(path) {
                        Ok(bundle) => {
                            view.zip_loaded_bundle = Some(bundle);
                            view.zip_parse_error = None;
                            view.feedback = Some(Ok(format!(
                                "已安全解压并解析 ZIP 包 {}（临时目录已自动清理）。",
                                path.display()
                            )));
                        }
                        Err(err) => {
                            view.zip_loaded_bundle = None;
                            view.zip_parse_error = Some(err.to_string());
                        }
                    }
                }
                if btn_outlined(ui, p, "载入内置示例 ZIP 包（含规则+网址）", true).clicked() {
                    view.load_sample_zip_bundle();
                }
            });
            ui.add_space(SPACE_1);
            ui.label(
                RichText::new("ZIP 安全规范：支持 manifest.json + targets.{txt,md,csv,json} + 可选 rules.toml + 可选 README.md；单包 ≤ 10 MiB、解压 ≤ 50 MiB，自动拒绝路径穿越、符号链接与脚本文件。")
                    .size(FONT_META)
                    .color(p.text_muted),
            );
            if let Some(err) = &view.zip_parse_error {
                ui.add_space(SPACE_1);
                ui.colored_label(p.danger_fg, format!("ZIP 校验未通过：{err}"));
            }
        });
}

fn render_unified_preview_table(
    ui: &mut Ui,
    preview: &parallaxe_core::import_bundle::BundlePreviewReport,
    readme: Option<&str>,
    locale: Locale,
    p: Palette,
) {
    ui.label(
        RichText::new(translate(locale, "gui.import.preview.title", &[]))
            .size(FONT_BODY)
            .strong()
            .color(p.text_primary),
    );
    ui.add_space(2.0);
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
            &format!("{} URLs", preview.targets.len()),
            p.accent,
            p.accent_soft,
            p.accent,
        );
        if !preview.rules.is_empty() {
            render_pill(
                ui,
                &format!("+{} rules", preview.rules.len()),
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
            ui.label(
                RichText::new(format!("导入包说明（README 只读预览）：{}", rm.trim()))
                    .size(FONT_META)
                    .color(p.text_secondary),
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
                        ui.spacing_mut().item_spacing.x = SPACE_2;
                        ui.monospace(&rr.id);
                        render_pill(ui, &rr.kind, p.info_fg, p.info_bg, p.info_fg);
                        ui.label(
                            RichText::new(format!("pattern={}", rr.pattern))
                                .monospace()
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        if rr.valid {
                            render_pill(ui, "有效", p.ok_fg, p.ok_bg, p.ok_fg);
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

    if !preview.targets.is_empty() {
        ui.add_space(SPACE_1);
        let table_outer_w = ui.available_width();
        egui::Frame::none()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .rounding(Rounding::same(RADIUS_MD))
            .inner_margin(Margin::symmetric(SPACE_2, SPACE_1))
            .show(ui, |ui| {
                ui.set_width((table_outer_w - SPACE_4).max(160.0));
                if table_outer_w < 680.0 {
                    // 紧凑双栏右列卡片行：免横向滚动即可一次看清「目标 ID + 网址 + 绑定规则 + 状态」
                    let draw_rows = |ui: &mut Ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (idx, row) in preview.targets.iter().enumerate() {
                            if idx > 0 {
                                ui.add_space(2.0);
                                let rect = ui.allocate_space(vec2(ui.available_width(), 1.0)).1;
                                ui.painter()
                                    .rect_filled(rect, Rounding::ZERO, p.border_subtle);
                                ui.add_space(2.0);
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(SPACE_1, 1.0);
                                ui.label(
                                    RichText::new(&row.planned_id)
                                        .monospace()
                                        .strong()
                                        .size(FONT_META)
                                        .color(p.text_primary),
                                );
                                for (sig, is_new) in &row.bound_signals {
                                    if *is_new {
                                        render_pill(
                                            ui,
                                            &format!("{sig} [包内新增]"),
                                            p.warning_fg,
                                            p.warning_bg,
                                            p.warning_fg,
                                        );
                                    } else {
                                        render_pill(ui, sig, p.info_fg, p.info_bg, p.info_fg);
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
                            ui.label(
                                RichText::new(&row.url)
                                    .monospace()
                                    .size(FONT_META)
                                    .color(p.text_secondary),
                            );
                        }
                    };
                    if preview.targets.len() > 4 {
                        ScrollArea::vertical()
                            .id_salt("import_bundle_preview_vscroll")
                            .max_height(200.0)
                            .auto_shrink([false, true])
                            .show(ui, draw_rows);
                    } else {
                        draw_rows(ui);
                    }
                } else {
                    ScrollArea::horizontal()
                        .id_salt("import_bundle_preview_hscroll")
                        .max_width(table_outer_w)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            egui::Grid::new("import_bundle_preview_grid")
                                .num_columns(4)
                                .striped(true)
                                .spacing([SPACE_3, 4.0])
                                .min_col_width(96.0)
                                .show(ui, |ui| {
                                    for h in ["目标 ID", "目标网址", "绑定的检查方式", "校验状态"]
                                    {
                                        ui.label(
                                            RichText::new(h)
                                                .size(FONT_META)
                                                .strong()
                                                .color(p.text_secondary),
                                        );
                                    }
                                    ui.end_row();
                                    for row in &preview.targets {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&row.planned_id)
                                                    .monospace()
                                                    .size(FONT_META)
                                                    .color(p.text_primary),
                                            )
                                            .wrap_mode(egui::TextWrapMode::Extend),
                                        );
                                        ui.label(
                                            RichText::new(&row.url)
                                                .monospace()
                                                .size(FONT_META)
                                                .color(p.text_secondary),
                                        );
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing = vec2(SPACE_1, SPACE_1);
                                            for (sig, is_new) in &row.bound_signals {
                                                if *is_new {
                                                    render_pill(
                                                        ui,
                                                        &format!("{sig} [包内新增]"),
                                                        p.warning_fg,
                                                        p.warning_bg,
                                                        p.warning_fg,
                                                    );
                                                } else {
                                                    render_pill(
                                                        ui, sig, p.info_fg, p.info_bg, p.info_fg,
                                                    );
                                                }
                                            }
                                        });
                                        if row.valid {
                                            render_pill(ui, "就绪", p.ok_fg, p.ok_bg, p.ok_fg);
                                        } else {
                                            ui.colored_label(
                                                p.danger_fg,
                                                format!(
                                                    "错误：{}",
                                                    row.error.as_deref().unwrap_or("校验失败")
                                                ),
                                            );
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                }
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
    actions: &mut Vec<UiAction>,
) {
    if locale == Locale::EnUs {
        ui.label(
            RichText::new(translate(locale, "gui.import.legal_en_banner", &[]))
                .size(FONT_META)
                .color(p.text_muted),
        );
        ui.add_space(SPACE_1);
    }
    let need_both = preview.requires_target_consent && preview.requires_rule_consent;
    let t_cnt = bundle.targets.len().to_string();
    let r_cnt = bundle.rules.len().to_string();
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

    // 仅需网址声明时，把声明正文 + 勾选框与主导入按钮集成在同一张警示卡的底行，1280×820 首屏免滚即可一次看清并点击
    if preview.requires_target_consent && !preview.requires_rule_consent {
        render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(
                RichText::new("【导入网址责任声明（必读原文）】")
                    .size(FONT_META)
                    .strong()
                    .color(p.warning_fg),
            );
            ui.label(
                RichText::new(USER_TARGET_LIABILITY_NOTICE)
                    .size(FONT_META)
                    .color(p.text_primary),
            );
            ui.add_space(2.0);
            let can_import = preview.all_valid && view.target_consent_checked;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
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
        });
    } else {
        // 含自定义规则时，并排（宽屏）或堆叠展示两层责任声明，随后紧跟单一主导入按钮
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
        let can_import = preview.all_valid && consents_ok;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_3;
            if btn_filled(ui, p, &submit_label, can_import).clicked() {
                trigger_import = true;
            }
            if !consents_ok {
                ui.label(
                    RichText::new("请先勾选上方所需的责任声明后方可导入")
                        .size(FONT_META)
                        .color(p.warning_fg),
                );
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
                view.feedback = Some(Ok(format!(
                    "已原子导入 {} 个自定义网址与 {} 条检查方式（origin={}，默认零发包；如需发起检查请点击上方「申请一次性在线检查」）。",
                    report.imported_targets.len(),
                    report.imported_rules.len(),
                    report.origin
                )));
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
        ui.label(
            RichText::new(USER_TARGET_LIABILITY_NOTICE)
                .size(FONT_META)
                .color(p.text_primary),
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
        ui.label(
            RichText::new(USER_RULE_LIABILITY_NOTICE)
                .size(FONT_META)
                .color(p.text_primary),
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

fn render_gate_header_card(
    ui: &mut Ui,
    registry: &mut Registry,
    view: &mut ImportViewState,
    locale: Locale,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let user_targets_count = registry.user_targets().len();
    let user_rules_count = registry.user_rules().len();
    let active_grant = registry.active_session_grant_snapshot();
    let has_targets = user_targets_count > 0;
    let t_str = user_targets_count.to_string();
    let r_str = user_rules_count.to_string();

    card_frame(p)
        .inner_margin(Margin::symmetric(SPACE_3, SPACE_2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
                icon_tile(ui, IconKind::Lock, 22.0, p.accent, p.accent_soft);
                ui.label(
                    RichText::new(translate(locale, "gui.import.gate.title", &[]))
                        .size(FONT_TITLE)
                        .strong()
                        .color(p.text_primary),
                );
                render_pill(
                    ui,
                    &translate(locale, "gui.import.gate.origin_pill", &[]),
                    p.warning_fg,
                    p.warning_bg,
                    p.warning_fg,
                )
                .on_hover_text(format!("origin={ORIGIN_USER_IMPORTED}"));
                render_pill(
                    ui,
                    &translate(locale, "gui.import.gate.zero_packet_pill", &[]),
                    p.info_fg,
                    p.info_bg,
                    p.info_fg,
                )
                .on_hover_text(format!(
                    "allow_online=false · consent={LIABILITY_CONSENT_VERSION}"
                ));
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

    egui::Window::new("【运行前强制确认】一次性会话级联网授权 (F3 许可闸门)")
        .id(egui::Id::new("f3_online_session_confirm_modal"))
        .order(egui::Order::Foreground)
        .open(&mut window_open)
        .collapsible(false)
        .resizable(false)
        .default_width(580.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(p.card)
                .stroke(Stroke::new(1.5_f32, p.warning_fg))
                .rounding(Rounding::same(RADIUS_LG))
                .inner_margin(Margin::same(SPACE_5)),
        )
        .show(ctx, |ui| {
            ui.set_max_width(540.0);
            render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
                ui.label(
                    RichText::new("请逐项核对即将发起一次性在线检查的自定义目标与规则摘要")
                        .size(FONT_BODY)
                        .strong()
                        .color(p.warning_fg),
                );
                ui.add_space(SPACE_1);
                ui.label(RichText::new(&prompt).size(FONT_META).color(p.text_primary));
            });

            ui.add_space(SPACE_3);
            ui.label(
                RichText::new(
                    "硬边界：预算维持全局并发≤6、单主机≤2、单响应≤16 MiB、单次逻辑 GET、不自动跟跳；遇验证码/403/429 立即停步。本次许可在运行启动时即原子销毁，绝不持久化、不写入 assets/。",
                )
                .size(FONT_META)
                .color(p.text_secondary),
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
                ui.spacing_mut().item_spacing.x = SPACE_2;
                let confirm_label = format!("确认注入一次性许可并检查 ({} 项)", ids.len());
                if btn_filled(
                    ui,
                    p,
                    &confirm_label,
                    view.online_run_confirmed_checkbox && !ids.is_empty(),
                )
                .clicked()
                {
                    match registry.authorize_session_once(&ids, view.online_run_confirmed_checkbox)
                    {
                        Ok(grant) => {
                            close_after_grant = true;
                            view.feedback = Some(Ok(format!(
                                "已注入一次性会话许可 ({}) 并启动单次检查。",
                                grant.grant_id
                            )));
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

    if !window_open || close_without_grant || close_after_grant {
        view.online_confirm_open = false;
        view.online_run_confirmed_checkbox = false;
    }
}

fn render_imported_inventory_card(
    ui: &mut Ui,
    registry: &Registry,
    view: &mut ImportViewState,
    p: Palette,
    actions: &mut Vec<UiAction>,
) {
    let user_targets = registry.user_targets();
    let user_rules = registry.user_rules();

    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "已登记的用户导入清单（网址 {} 项 · 自定义检查方式 {} 项）",
                    user_targets.len(),
                    user_rules.len()
                ))
                .size(FONT_TITLE)
                .strong()
                .color(p.text_primary),
            );
            if !user_rules.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if btn_outlined(ui, p, "保存规则到用户态目录", true).clicked() {
                        let dir = Path::new(view.user_state_dir_input.trim());
                        match registry.save_user_rules_to_state_dir(dir) {
                            Ok(saved) => {
                                view.feedback = Some(Ok(format!(
                                    "已将自定义检查方式保存至用户态文件：{}（未修改内置 assets/）",
                                    saved.display()
                                )));
                            }
                            Err(err) => {
                                view.feedback = Some(Err(err.to_string()));
                            }
                        }
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut view.user_state_dir_input)
                            .desired_width(148.0),
                    );
                });
            }
        });
        ui.add_space(SPACE_2);

        if user_targets.is_empty() && user_rules.is_empty() {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(
                        "当前会话尚未导入自定义网址或检查方式。内置 107 个 Google 目标 + 28 个对照目标保持只读不变。",
                    )
                    .size(FONT_BODY)
                    .color(p.text_secondary),
                );
            });
            return;
        }

        if !user_targets.is_empty() {
            ui.label(
                RichText::new("已导入网址（每项独立绑定所选检查方式，默认零发包）")
                    .size(FONT_META)
                    .strong()
                    .color(p.text_secondary),
            );
            ui.add_space(SPACE_1);
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical()
                    .id_salt("imported_targets_inventory_scroll")
                    .max_height(180.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (idx, meta) in user_targets.into_iter().enumerate() {
                            if idx > 0 {
                                ui.add_space(SPACE_2);
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.monospace(&meta.id);
                                ui.label(&meta.url);
                                render_pill(
                                    ui,
                                    &format!("规则：{}", meta.signals.join(", ")),
                                    p.info_fg,
                                    p.info_bg,
                                    p.info_fg,
                                );
                                if ui.small_button("离线预览此项").clicked() {
                                    actions.push(UiAction::RunUserImportOfflinePreview(vec![
                                        meta.id.clone(),
                                    ]));
                                }
                                if ui.small_button("静态解释").clicked() {
                                    actions.push(UiAction::SelectTarget(meta.id.clone()));
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
                        ui.monospace(&rm.id);
                        render_pill(ui, &rm.kind, p.warning_fg, p.warning_bg, p.warning_fg);
                        ui.label(
                            RichText::new(format!("匹配模式：{}", rm.pattern))
                                .monospace()
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                    });
                }
            });
        }
    });
}
