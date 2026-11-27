//! Web/WASM 端入口：离线解释 + 报告导出 + 同源代理规则导出（ADR-006：Web 不做系统阻断）。
//!
//! wasm32 下没有系统 DNS / 原生 TCP 能力，core 的 `WasmTransport` 会返回专用非网络错误
//! `TransportErrorKind::WasmOffline`（`SCOPE_WASM_OFFLINE` / `State::Unsupported`），绝不伪装成
//! `TRANSPORT_OTHER` 或 `LOCAL_PATH_ERROR`。本界面只生成静态规则与报告，不执行阻断，不发送网络请求。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use eframe::egui;
use egui::{Color32, FontData, FontDefinitions, FontFamily, Margin, RichText, Rounding, Stroke};
use parallaxe_core::contracts::{GuardPlan, Report};
use parallaxe_core::forensics;
use parallaxe_core::guard::{self, GuardRequest};
use parallaxe_core::guard_export::{
    self, ExportMode, ExportRequest, RuleFormat, FULL_MODE_WARNING,
};
use parallaxe_core::i18n::translate;
use parallaxe_core::plugins::{self, HookEvent, Manifest, Runtime};
use parallaxe_core::reports;
use parallaxe_core::settings::Settings;
use parallaxe_core::targets::Registry;
use parallaxe_core::Locale;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::plugin_ui::{
    render_plugin_page, PluginUiPalette, EMBEDDED_THIRD_PARTY_PLUGINS_MD, THIRD_PARTY_PLUGINS_URL,
};

const WEB_SETTINGS_KEY: &str = "parallaxe.settings/v1";
const WEB_PLUGIN_RAW_KEY: &str = "parallaxe.web_plugins/v1";
const SAMPLE_LIGHT_MANIFEST: &str =
    include_str!("../../../plugins/samples/light-report-badge/manifest.json");
const SAMPLE_HEAVY_MANIFEST: &str =
    include_str!("../../../plugins/samples/heavy-report-guide/manifest.json");

#[wasm_bindgen(inline_js = r#"
export async function parallaxe_save_text_file(suggestedName, content) {
  if (typeof window === 'undefined' || typeof window.showSaveFilePicker !== 'function') {
    return 'unsupported';
  }
  try {
    const handle = await window.showSaveFilePicker({ suggestedName });
    const writable = await handle.createWritable();
    await writable.write(content);
    await writable.close();
    return 'saved:' + (handle.name || suggestedName);
  } catch (err) {
    if (err && err.name === 'AbortError') {
      return 'cancelled';
    }
    return 'error:' + String(err);
  }
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = parallaxe_save_text_file)]
    async fn parallaxe_save_text_file(suggested_name: &str, content: &str) -> JsValue;
}

fn web_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn load_web_settings() -> Settings {
    let Some(storage) = web_storage() else {
        return Settings::default();
    };
    let Ok(Some(raw)) = storage.get_item(WEB_SETTINGS_KEY) else {
        return Settings::default();
    };
    match serde_json::from_str::<Settings>(&raw) {
        Ok(settings) if settings.validate().is_ok() => settings,
        _ => Settings::default(),
    }
}

fn save_web_settings(settings: &Settings) {
    if settings.validate().is_err() {
        return;
    }
    if let Some(storage) = web_storage() {
        if let Ok(raw) = serde_json::to_string(settings) {
            let _ = storage.set_item(WEB_SETTINGS_KEY, &raw);
        }
    }
}

fn load_web_plugin_manifests() -> BTreeMap<String, String> {
    let Some(storage) = web_storage() else {
        return BTreeMap::new();
    };
    let Ok(Some(raw)) = storage.get_item(WEB_PLUGIN_RAW_KEY) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_web_plugin_manifests(manifests: &BTreeMap<String, String>) {
    if let Some(storage) = web_storage() {
        if let Ok(raw) = serde_json::to_string(manifests) {
            let _ = storage.set_item(WEB_PLUGIN_RAW_KEY, &raw);
        }
    }
}

const SELF_BLOCK_DOCS_URL: &str =
    "https://github.com/ijay11111/parallaxe-box/blob/main/docs/self-block-guide.md";

const FIXTURE_IDS: [&str; 4] = [
    "drive-login",
    "gemini-positive",
    "books-missing",
    "legacy-region-negative",
];

/// 内嵌开源 CJK 子集，确保 Web 端中文无豆腐块。
fn install_web_cjk_fonts(ctx: &egui::Context) {
    let embedded: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Regular-subset.ttf");
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk_embedded".to_owned(), FontData::from_static(embedded));
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "cjk_embedded".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
        family.push("cjk_embedded".to_owned());
    }
    ctx.set_fonts(fonts);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WebExportKind {
    Markdown,
    Text,
}

fn replay_builtin_fixture(registry: &Registry, case_id: &str) -> Result<Report, String> {
    parallaxe_core::runtime::replay_fixture(
        registry,
        case_id,
        &format!("run-web-{case_id}"),
        time::macros::datetime!(2026-11-23 0:00 UTC),
    )
    .map(|out| out.report)
    .map_err(|err| err.to_string())
}

fn format_label(format: RuleFormat, mode: ExportMode, locale: Locale) -> &'static str {
    match (format, mode, locale) {
        (RuleFormat::Clash, ExportMode::Provider, Locale::ZhCn) => "Clash (rule-provider)",
        (RuleFormat::Clash, ExportMode::Provider, Locale::EnUs) => "Clash (rule-provider)",
        (RuleFormat::Clash, ExportMode::Full, Locale::ZhCn) => "Clash (完整规则)",
        (RuleFormat::Clash, ExportMode::Full, Locale::EnUs) => "Clash (full overwrite)",
        (RuleFormat::SingBox, ExportMode::Provider, Locale::ZhCn) => "sing-box (rule_set)",
        (RuleFormat::SingBox, ExportMode::Provider, Locale::EnUs) => "sing-box (rule_set)",
        (RuleFormat::SingBox, ExportMode::Full, Locale::ZhCn) => "sing-box (完整配置)",
        (RuleFormat::SingBox, ExportMode::Full, Locale::EnUs) => "sing-box (full overwrite)",
        (RuleFormat::DomainList, _, Locale::ZhCn) => "纯域名清单 (FQDN)",
        (RuleFormat::DomainList, _, Locale::EnUs) => "Domain List (FQDN)",
    }
}

struct WebStagedPlugin {
    raw_json: String,
    sha256: String,
    manifest: Manifest,
}

/// Web 端只读应用容器。
struct WebApp {
    registry: Registry,
    report: Report,
    active_fixture: String,
    json_import_input: String,
    json_import_error: Option<String>,
    status: String,
    locale: Locale,
    dark: bool,
    preview: Option<(WebExportKind, String)>,
    rule_format: RuleFormat,
    rule_mode: ExportMode,
    rule_full_ack: bool,
    rule_target_override: String,
    rule_status: Option<String>,
    pending: Rc<RefCell<Option<String>>>,
    pending_rule: Rc<RefCell<Option<String>>>,
    settings: Settings,
    plugin_manifests_raw: BTreeMap<String, String>,
    plugin_runtimes: BTreeMap<String, Runtime>,
    plugin_input: String,
    plugin_staged: Option<WebStagedPlugin>,
    plugin_error: Option<String>,
    plugin_suggestions: Vec<(String, Vec<String>)>,
    active_plugin_page: Option<(String, String)>,
    show_plugin_notices: bool,
}

impl WebApp {
    fn new(registry: Registry, report: Report) -> Self {
        let default_target = report
            .results
            .first()
            .map(|r| r.target.clone())
            .unwrap_or_else(|| "google.drive".to_string());
        let settings = load_web_settings();
        let locale = Locale::parse(&settings.language).unwrap_or(Locale::ZhCn);
        let dark = settings.theme != "light";
        let rule_format = match settings.rule_format.as_str() {
            "singbox" => RuleFormat::SingBox,
            "domainlist" => RuleFormat::DomainList,
            _ => RuleFormat::Clash,
        };
        let plugin_manifests_raw = load_web_plugin_manifests();
        let mut app = WebApp {
            registry,
            report,
            active_fixture: "drive-login".to_string(),
            json_import_input: String::new(),
            json_import_error: None,
            status: "就绪：Web 端只生成规则与离线报告、不执行阻断、不发网络请求".to_string(),
            locale,
            dark,
            preview: None,
            rule_format,
            rule_mode: ExportMode::Provider,
            rule_full_ack: false,
            rule_target_override: default_target,
            rule_status: None,
            pending: Rc::new(RefCell::new(None)),
            pending_rule: Rc::new(RefCell::new(None)),
            settings,
            plugin_manifests_raw,
            plugin_runtimes: BTreeMap::new(),
            plugin_input: String::new(),
            plugin_staged: None,
            plugin_error: None,
            plugin_suggestions: Vec::new(),
            active_plugin_page: None,
            show_plugin_notices: false,
        };
        app.refresh_web_plugins(Some(HookEvent::Complete));
        app
    }

    fn persist_web_settings(&mut self) {
        self.settings.language = self.locale.as_str().to_string();
        self.settings.theme = if self.dark {
            "dark".to_string()
        } else {
            "light".to_string()
        };
        self.settings.rule_format = self.rule_format.as_str().to_string();
        save_web_settings(&self.settings);
        save_web_plugin_manifests(&self.plugin_manifests_raw);
    }

    fn refresh_web_plugins(&mut self, event: Option<HookEvent>) {
        self.plugin_suggestions.clear();
        let counts = Some((
            self.report.coverage.covered.len(),
            self.report.coverage.selected_count as usize,
        ));
        let ids: Vec<String> = self.settings.plugins.keys().cloned().collect();
        for id in ids {
            let Some(approved_sha) = self.settings.plugins.get(&id).cloned() else {
                continue;
            };
            let Some(raw) = self.plugin_manifests_raw.get(&id).cloned() else {
                continue;
            };
            let runtime = self.plugin_runtimes.entry(id.clone()).or_default();
            if let Ok(messages) = runtime.invoke(
                raw.as_bytes(),
                &approved_sha,
                "default-profile",
                event,
                counts,
            ) {
                if let Ok(manifest) = plugins::verified(raw.as_bytes(), &approved_sha) {
                    if !messages.is_empty() {
                        self.plugin_suggestions.push((manifest.name, messages));
                    }
                }
            }
        }
    }

    fn stage_web_manifest(&mut self) {
        self.plugin_staged = None;
        self.plugin_error = None;
        let bytes = self.plugin_input.as_bytes();
        match Manifest::parse(bytes) {
            Ok(manifest) => {
                let sha256 = plugins::digest(bytes);
                self.plugin_staged = Some(WebStagedPlugin {
                    raw_json: self.plugin_input.clone(),
                    sha256,
                    manifest,
                });
            }
            Err(err) => {
                self.plugin_error = Some(err);
            }
        }
    }

    fn approve_web_staged(&mut self) {
        let Some(staged) = self.plugin_staged.take() else {
            return;
        };
        let id = staged.manifest.id.clone();
        self.settings.plugins.insert(id.clone(), staged.sha256);
        self.plugin_manifests_raw.insert(id, staged.raw_json);
        self.persist_web_settings();
        self.refresh_web_plugins(Some(HookEvent::Complete));
    }

    fn load_fixture(&mut self, case_id: &str) {
        match replay_builtin_fixture(&self.registry, case_id) {
            Ok(report) => {
                if let Some(first) = report.results.first() {
                    self.rule_target_override = first.target.clone();
                }
                self.report = report;
                self.active_fixture = case_id.to_string();
                self.json_import_error = None;
                self.preview = None;
                self.rule_status = None;
                self.status = match self.locale {
                    Locale::ZhCn => format!("已切换离线样本：{case_id}（零网络请求）"),
                    Locale::EnUs => format!("Loaded offline fixture: {case_id} (zero network)"),
                };
                self.refresh_web_plugins(Some(HookEvent::Complete));
            }
            Err(err) => {
                self.status = format!("样本加载失败：{err}");
            }
        }
    }

    fn import_desktop_report_json(&mut self) {
        let trimmed = self.json_import_input.trim();
        if trimmed.is_empty() {
            return;
        }
        match serde_json::from_str::<Report>(trimmed) {
            Ok(report) => {
                if let Some(first) = report.results.first() {
                    self.rule_target_override = first.target.clone();
                }
                let run_id = report.run_id.clone();
                self.report = report;
                self.active_fixture = format!("imported:{run_id}");
                self.json_import_error = None;
                self.preview = None;
                self.rule_status = None;
                self.status = match self.locale {
                    Locale::ZhCn => format!("已本地解析桌面端报告 {run_id}（纯本地、零发包）"),
                    Locale::EnUs => {
                        format!("Parsed desktop report {run_id} locally (zero network)")
                    }
                };
            }
            Err(err) => {
                self.json_import_error = Some(match self.locale {
                    Locale::ZhCn => format!("报告 JSON 解析失败：{err}"),
                    Locale::EnUs => format!("Failed to parse report JSON: {err}"),
                });
            }
        }
    }

    fn render_report(&self, kind: WebExportKind) -> String {
        match kind {
            WebExportKind::Markdown => reports::to_markdown(&self.report, self.locale),
            WebExportKind::Text => reports::to_text(&self.report, self.locale),
        }
    }

    fn copy_text_to_clipboard(
        ctx: &egui::Context,
        text: String,
        label: String,
        target_slot: Rc<RefCell<Option<String>>>,
        fallback_target: &mut String,
    ) {
        let Some(clipboard) = web_sys::window()
            .map(|window| window.navigator().clipboard())
            .filter(|value| !value.is_undefined() && !value.is_null())
        else {
            *fallback_target = format!("已生成 {label} 预览（浏览器未提供剪贴板接口）");
            return;
        };
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let message = match clipboard.write_text(&text).await {
                Ok(_) => format!("已复制 {label} 到剪贴板"),
                Err(_) => format!("剪贴板写入失败：可从下方预览手动复制 {label}"),
            };
            *target_slot.borrow_mut() = Some(message);
            ctx.request_repaint();
        });
    }

    fn copy(&mut self, ctx: &egui::Context, kind: WebExportKind) {
        let text = self.render_report(kind);
        self.preview = Some((kind, text.clone()));
        let label = match (kind, self.locale) {
            (WebExportKind::Markdown, Locale::ZhCn) => "Markdown (.md) 报告",
            (WebExportKind::Text, Locale::ZhCn) => "纯文本 (.txt) 报告",
            (WebExportKind::Markdown, Locale::EnUs) => "Markdown (.md) Report",
            (WebExportKind::Text, Locale::EnUs) => "Plain Text (.txt) Report",
        }
        .to_string();
        Self::copy_text_to_clipboard(ctx, text, label, self.pending.clone(), &mut self.status);
    }

    fn save_text_with_browser_picker(
        ctx: &egui::Context,
        suggested_name: String,
        content: String,
        locale: Locale,
        target_slot: Rc<RefCell<Option<String>>>,
    ) {
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = parallaxe_save_text_file(&suggested_name, &content)
                .await
                .as_string()
                .unwrap_or_else(|| "unsupported".to_string());
            let message = if let Some(name) = result.strip_prefix("saved:") {
                match locale {
                    Locale::ZhCn => format!("已通过浏览器保存对话写入：{name}"),
                    Locale::EnUs => format!("Saved via browser file picker: {name}"),
                }
            } else if result == "cancelled" {
                match locale {
                    Locale::ZhCn => "已取消保存，未写入任何文件。".to_string(),
                    Locale::EnUs => "Save cancelled; no file was written.".to_string(),
                }
            } else if result == "unsupported" {
                match locale {
                    Locale::ZhCn => {
                        "当前浏览器不支持文件保存窗口；请使用上方预览与复制按钮手动保存（不静默下载）。"
                            .to_string()
                    }
                    Locale::EnUs => {
                        "Browser file-save picker is unavailable here; use the preview and clipboard copy button (no silent download)."
                            .to_string()
                    }
                }
            } else {
                match locale {
                    Locale::ZhCn => format!("浏览器保存失败（{result}）；请使用复制预览手动保存。"),
                    Locale::EnUs => {
                        format!("Browser save failed ({result}); use clipboard copy instead.")
                    }
                }
            };
            *target_slot.borrow_mut() = Some(message);
            ctx.request_repaint();
        });
    }

    fn build_guard_plan(&self) -> GuardPlan {
        if let Some(existing) = self.report.plans.first() {
            return existing.clone();
        }
        let targets: Vec<String> = self
            .rule_target_override
            .split([',', ' ', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let now = time::macros::datetime!(2026-11-23 0:00 UTC);
        let run_id = format!("web-preview-{}", self.report.run_id);
        let source_interrupted =
            self.report.termination != parallaxe_core::contracts::Termination::Complete;
        guard::plan_preview(
            &self.registry,
            GuardRequest {
                run_id: &run_id,
                source_run_id: Some(&self.report.run_id),
                targets: &targets,
                results: &self.report.results,
                recorded_at: parallaxe_core::state::format_time(now),
                as_of: now.date(),
                source_interrupted,
            },
        )
        .plan
    }
}

impl eframe::App for WebApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(message) = self.pending.borrow_mut().take() {
            self.status = message;
        }
        if let Some(message) = self.pending_rule.borrow_mut().take() {
            self.rule_status = Some(message);
        }
        ctx.set_visuals(if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let (canvas, card, border, accent, muted, warn_fg, warn_bg) = if self.dark {
            (
                Color32::from_rgb(10, 16, 28),
                Color32::from_rgb(17, 27, 46),
                Color32::from_rgb(35, 55, 88),
                Color32::from_rgb(34, 211, 238),
                Color32::from_rgb(156, 177, 204),
                Color32::from_rgb(251, 191, 36),
                Color32::from_rgb(46, 36, 16),
            )
        } else {
            (
                Color32::from_rgb(244, 247, 251),
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(214, 224, 238),
                Color32::from_rgb(0, 115, 140),
                Color32::from_rgb(71, 85, 105),
                Color32::from_rgb(161, 98, 7),
                Color32::from_rgb(254, 249, 195),
            )
        };

        let screen_w = ctx.screen_rect().width();
        let narrow = screen_w < 560.0;
        let outer_pad = if narrow { 8.0 } else { 16.0 };
        let card_pad = if narrow { 10.0 } else { 16.0 };
        let card_inner_w = (screen_w - outer_pad * 2.0 - card_pad * 2.0 - 16.0).max(120.0);

        let card_frame = || {
            egui::Frame::none()
                .fill(card)
                .stroke(Stroke::new(1.0_f32, border))
                .rounding(Rounding::same(12.0))
                .inner_margin(Margin::same(card_pad))
        };

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(canvas).inner_margin(Margin::same(outer_pad)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                    // Card 1: 离线解释与报告导出
                    card_frame().show(ui, |ui| {
                        ui.set_width(card_inner_w);
                        ui.set_max_width(card_inner_w);
                        let title_str = match self.locale {
                            Locale::ZhCn => "Parallaxe box · Web（只读离线）",
                            Locale::EnUs => "Parallaxe box · Web (Read-Only Offline)",
                        };
                        let title_size = if narrow { 18.0 } else { 22.0 };
                        if narrow {
                            ui.add(
                                egui::Label::new(RichText::new(title_str).size(title_size).strong())
                                    .wrap(),
                            );
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .button(if self.dark { "浅色" } else { "深色" })
                                    .clicked()
                                {
                                    self.dark = !self.dark;
                                    self.persist_web_settings();
                                }
                                if ui
                                    .button(match self.locale {
                                        Locale::ZhCn => "EN",
                                        Locale::EnUs => "中文",
                                    })
                                    .clicked()
                                {
                                    self.locale = match self.locale {
                                        Locale::ZhCn => Locale::EnUs,
                                        Locale::EnUs => Locale::ZhCn,
                                    };
                                    self.persist_web_settings();
                                }
                            });
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(title_str).size(title_size).strong(),
                                    )
                                    .wrap(),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button(if self.dark { "浅色" } else { "深色" })
                                            .clicked()
                                        {
                                            self.dark = !self.dark;
                                            self.persist_web_settings();
                                        }
                                        if ui
                                            .button(match self.locale {
                                                Locale::ZhCn => "EN",
                                                Locale::EnUs => "中文",
                                            })
                                            .clicked()
                                        {
                                            self.locale = match self.locale {
                                                Locale::ZhCn => Locale::EnUs,
                                                Locale::EnUs => Locale::ZhCn,
                                            };
                                            self.persist_web_settings();
                                        }
                                    },
                                );
                            });
                        }
                        ui.add_space(8.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(match self.locale {
                                    Locale::ZhCn => {
                                        "Web 端只生成规则、不执行阻断、不发网络请求；测试版不含系统级阻断，导出通过预览/剪贴板或浏览器手动保存。"
                                    }
                                    Locale::EnUs => {
                                        "Web generates rules only — no OS blocking (disabled in beta), no network requests; exports use preview, clipboard, or browser file picker."
                                    }
                                })
                                .strong()
                                .color(accent),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(match self.locale {
                                    Locale::ZhCn => format!(
                                        "内置目录共 {} 项目标；当前离线样本：{}（wasm32 离线传输返回 SCOPE_WASM_OFFLINE / UNSUPPORTED，绝不伪装成网络传输失败）。",
                                        self.registry.catalog_size(),
                                        self.active_fixture
                                    ),
                                    Locale::EnUs => format!(
                                        "{} catalog targets embedded; active offline source: {} (wasm32 offline transport emits SCOPE_WASM_OFFLINE / UNSUPPORTED, never TRANSPORT_OTHER).",
                                        self.registry.catalog_size(),
                                        self.active_fixture
                                    ),
                                })
                                .color(muted),
                            )
                            .wrap(),
                        );
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(match self.locale {
                                Locale::ZhCn => "内置离线样本：",
                                Locale::EnUs => "Built-in offline fixtures:",
                            });
                            for case_id in FIXTURE_IDS {
                                if ui
                                    .selectable_label(self.active_fixture == case_id, case_id)
                                    .clicked()
                                {
                                    self.load_fixture(case_id);
                                }
                            }
                        });
                        ui.add_space(6.0);
                        ui.collapsing(
                            match self.locale {
                                Locale::ZhCn => "导入桌面端报告 JSON（纯本地解析）",
                                Locale::EnUs => "Import Desktop Report JSON (Local Only)",
                            },
                            |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.json_import_input)
                                        .code_editor()
                                        .desired_rows(4)
                                        .desired_width(f32::INFINITY),
                                );
                                ui.add_space(4.0);
                                if ui
                                    .button(match self.locale {
                                        Locale::ZhCn => "解析本地 JSON 报告",
                                        Locale::EnUs => "Parse Local JSON Report",
                                    })
                                    .clicked()
                                {
                                    self.import_desktop_report_json();
                                }
                                if let Some(err) = &self.json_import_error {
                                    ui.add(
                                        egui::Label::new(RichText::new(err).color(warn_fg)).wrap(),
                                    );
                                }
                            },
                        );
                        ui.add_space(10.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .button(match self.locale {
                                    Locale::ZhCn => "复制 Markdown (.md)",
                                    Locale::EnUs => "Copy Markdown (.md)",
                                })
                                .clicked()
                            {
                                self.copy(ctx, WebExportKind::Markdown);
                            }
                            if ui
                                .button(match self.locale {
                                    Locale::ZhCn => "复制纯文本 (.txt)",
                                    Locale::EnUs => "Copy Plain Text (.txt)",
                                })
                                .clicked()
                            {
                                self.copy(ctx, WebExportKind::Text);
                            }
                            ui.hyperlink_to(
                                RichText::new(match self.locale {
                                    Locale::ZhCn => "帮助与进阶文档（GitHub）",
                                    Locale::EnUs => "Help & Advanced Docs (GitHub)",
                                })
                                .color(accent),
                                SELF_BLOCK_DOCS_URL,
                            );
                        });
                        ui.add_space(8.0);
                        ui.add(egui::Label::new(RichText::new(&self.status).color(accent)).wrap());
                        if let Some((_, content)) = &self.preview {
                            ui.add_space(8.0);
                            let mut text = content.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .code_editor()
                                    .desired_rows(8)
                                    .desired_width(f32::INFINITY),
                            );
                        }
                    });

                    ui.add_space(16.0);

                    // Card 2: 同源代理规则导出（直接调用 parallaxe_core::guard_export::export_rules）
                    card_frame().show(ui, |ui| {
                        ui.set_width(card_inner_w);
                        ui.set_max_width(card_inner_w);
                        ui.add(
                            egui::Label::new(
                                RichText::new(match self.locale {
                                    Locale::ZhCn => {
                                        "代理规则导出（同源 parallaxe_core::guard_export）"
                                    }
                                    Locale::EnUs => {
                                        "Proxy Rule Export (Shared guard_export)"
                                    }
                                })
                                .size(if narrow { 17.0 } else { 20.0 })
                                .strong(),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(match self.locale {
                                    Locale::ZhCn => {
                                        "Web 端只生成规则、不执行阻断、不发网络请求。仅对通过核心安全闸门的精确 FQDN 生成规则，绝不按 IP/CDN/ASN 泛阻断。"
                                    }
                                    Locale::EnUs => {
                                        "Web generates rules only — no OS blocking, no network requests. Exports exact FQDNs passing core safety gates; never broad IP/CDN/ASN blocking."
                                    }
                                })
                                .color(muted),
                            )
                            .wrap(),
                        );
                        ui.add_space(8.0);

                        ui.horizontal_wrapped(|ui| {
                            ui.label(match self.locale {
                                Locale::ZhCn => "导出模式：",
                                Locale::EnUs => "Export Mode:",
                            });
                            if ui
                                .selectable_label(
                                    self.rule_mode == ExportMode::Provider,
                                    match self.locale {
                                        Locale::ZhCn => "推荐：rule-provider（不污染分流）",
                                        Locale::EnUs => "Recommended: rule-provider",
                                    },
                                )
                                .clicked()
                            {
                                self.rule_mode = ExportMode::Provider;
                                self.rule_status = None;
                            }
                            if ui
                                .selectable_label(
                                    self.rule_mode == ExportMode::Full,
                                    match self.locale {
                                        Locale::ZhCn => "备选：完整规则（整包替换）",
                                        Locale::EnUs => "Secondary: Full Overwrite",
                                    },
                                )
                                .clicked()
                            {
                                self.rule_mode = ExportMode::Full;
                                self.rule_status = None;
                            }
                        });
                        ui.add_space(6.0);

                        ui.horizontal_wrapped(|ui| {
                            ui.label(match self.locale {
                                Locale::ZhCn => "规则格式：",
                                Locale::EnUs => "Rule Format:",
                            });
                            let formats: &[RuleFormat] = if self.rule_mode == ExportMode::Full {
                                &[RuleFormat::Clash, RuleFormat::SingBox]
                            } else {
                                &RuleFormat::ALL
                            };
                            if self.rule_mode == ExportMode::Full
                                && self.rule_format == RuleFormat::DomainList
                            {
                                self.rule_format = RuleFormat::Clash;
                            }
                            for fmt in formats {
                                if ui
                                    .selectable_label(
                                        self.rule_format == *fmt,
                                        format_label(*fmt, self.rule_mode, self.locale),
                                    )
                                    .clicked()
                                {
                                    self.rule_format = *fmt;
                                    self.rule_status = None;
                                    self.persist_web_settings();
                                }
                            }
                        });
                        ui.add_space(6.0);

                        if self.report.plans.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(match self.locale {
                                    Locale::ZhCn => "预演目标 ID：",
                                    Locale::EnUs => "Preview Target ID:",
                                });
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.rule_target_override)
                                        .desired_width((card_inner_w - 96.0).clamp(110.0, 220.0)),
                                );
                            });
                            ui.add_space(6.0);
                        }

                        let mode_ready = if self.rule_mode == ExportMode::Full {
                            egui::Frame::none()
                                .fill(warn_bg)
                                .stroke(Stroke::new(1.0_f32, warn_fg))
                                .rounding(Rounding::same(8.0))
                                .inner_margin(Margin::same(10.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new(FULL_MODE_WARNING).strong().color(warn_fg),
                                    );
                                });
                            ui.add_space(6.0);
                            ui.checkbox(
                                &mut self.rule_full_ack,
                                match self.locale {
                                    Locale::ZhCn => {
                                        "我已备份原有配置，了解覆盖原有分流的风险，且由我自主决定是否替换"
                                    }
                                    Locale::EnUs => {
                                        "I have backed up my existing config, understand the overwrite risk, and choose this myself"
                                    }
                                },
                            );
                            ui.add_space(6.0);
                            self.rule_full_ack
                        } else {
                            true
                        };

                        if mode_ready {
                            let plan = self.build_guard_plan();
                            let forensic_pause = forensics::guard_pause_from_report(&self.report);
                            let res = guard_export::export_rules(ExportRequest {
                                registry: &self.registry,
                                plan: &plan,
                                format: self.rule_format,
                                mode: self.rule_mode,
                                generated_at: "2026-11-23T00:00:00Z".to_string(),
                                forensic_pause,
                            });
                            match res {
                                Ok(bundle) => {
                                    if !bundle.warning_codes.is_empty() {
                                        ui.label(
                                            RichText::new(format!(
                                                "warning_codes: {}",
                                                bundle.warning_codes.join(", ")
                                            ))
                                            .monospace()
                                            .color(warn_fg),
                                        );
                                    }
                                    for warning in &bundle.warnings {
                                        ui.label(
                                            RichText::new(format!("• {warning}")).color(warn_fg),
                                        );
                                    }
                                    ui.add_space(6.0);
                                    let combined: String = bundle
                                        .artifacts
                                        .iter()
                                        .map(|a| format!("# === {} ===\n{}", a.name, a.content))
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    ui.horizontal_wrapped(|ui| {
                                        if ui
                                            .button(match self.locale {
                                                Locale::ZhCn => "复制代理规则到剪贴板",
                                                Locale::EnUs => "Copy Proxy Rules to Clipboard",
                                            })
                                            .clicked()
                                        {
                                            let mut fallback = String::new();
                                            Self::copy_text_to_clipboard(
                                                ctx,
                                                combined.clone(),
                                                format!(
                                                    "{} ({})",
                                                    self.rule_format.as_str(),
                                                    self.rule_mode.as_str()
                                                ),
                                                self.pending_rule.clone(),
                                                &mut fallback,
                                            );
                                            if !fallback.is_empty() {
                                                self.rule_status = Some(fallback);
                                            }
                                        }
                                        if ui
                                            .button(match self.locale {
                                                Locale::ZhCn => "选择位置保存规则...",
                                                Locale::EnUs => "Save Rules via Picker...",
                                            })
                                            .clicked()
                                        {
                                            let ext = match self.rule_format {
                                                RuleFormat::Clash => "yaml",
                                                RuleFormat::SingBox => "json",
                                                RuleFormat::DomainList => "txt",
                                            };
                                            let suggested = format!(
                                                "parallaxe-rules-{}.{ext}",
                                                self.rule_format.as_str()
                                            );
                                            Self::save_text_with_browser_picker(
                                                ctx,
                                                suggested,
                                                combined.clone(),
                                                self.locale,
                                                self.pending_rule.clone(),
                                            );
                                        }
                                    });
                                    if let Some(msg) = &self.rule_status {
                                        ui.add(
                                            egui::Label::new(RichText::new(msg).color(accent))
                                                .wrap(),
                                        );
                                    }
                                    ui.add_space(6.0);
                                    let mut preview = combined;
                                    ui.add(
                                        egui::TextEdit::multiline(&mut preview)
                                            .code_editor()
                                            .desired_rows(10)
                                            .desired_width(f32::INFINITY),
                                    );
                                }
                                Err(refusal) => {
                                    egui::Frame::none()
                                        .fill(warn_bg)
                                        .stroke(Stroke::new(1.0_f32, warn_fg))
                                        .rounding(Rounding::same(8.0))
                                        .inner_margin(Margin::same(10.0))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} ({})",
                                                    match self.locale {
                                                        Locale::ZhCn => "核心闸门已拒绝生成规则",
                                                        Locale::EnUs => {
                                                            "Core Safety Gate Refused Rule Export"
                                                        }
                                                    },
                                                    refusal.code
                                                ))
                                                .strong()
                                                .color(warn_fg),
                                            );
                                            ui.label(RichText::new(&refusal.detail).color(muted));
                                        });
                                }
                            }
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.label(
                            RichText::new(match self.locale {
                                Locale::ZhCn => {
                                    "能力边界：Web 端只生成规则、不执行阻断、不发网络请求；真实在线检测与 Win/Linux 本机系统阻断请使用桌面端或 CLI。"
                                }
                                Locale::EnUs => {
                                    "Capability Boundary: Web generates rules only — no OS blocking, no network requests. Use Desktop or CLI for live probes and Win/Linux OS blocking."
                                }
                            })
                            .color(muted),
                        );
                    });

                    ui.add_space(16.0);

                    // Card 3: Web 插件适配层与共享声明式页面协议（parallaxe.plugin/v1）
                    let plugin_palette = PluginUiPalette {
                        card,
                        card_alt: if self.dark {
                            Color32::from_rgb(22, 34, 58)
                        } else {
                            Color32::from_rgb(246, 249, 253)
                        },
                        border,
                        border_subtle: if self.dark {
                            Color32::from_rgb(26, 41, 68)
                        } else {
                            Color32::from_rgb(229, 236, 245)
                        },
                        accent,
                        accent_soft: if self.dark {
                            Color32::from_rgb(16, 54, 74)
                        } else {
                            Color32::from_rgb(224, 244, 250)
                        },
                        text_primary: if self.dark {
                            Color32::from_rgb(241, 245, 249)
                        } else {
                            Color32::from_rgb(15, 23, 42)
                        },
                        text_secondary: muted,
                        text_muted: muted,
                        info_bg: if self.dark {
                            Color32::from_rgb(14, 42, 68)
                        } else {
                            Color32::from_rgb(226, 243, 254)
                        },
                        info_fg: accent,
                        warning_bg: warn_bg,
                        warning_fg: warn_fg,
                    };
                    card_frame().show(ui, |ui| {
                        ui.set_width(card_inner_w);
                        ui.set_max_width(card_inner_w);
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(translate(self.locale, "gui.plugins.title", &[]))
                                        .size(if narrow { 17.0 } else { 20.0 })
                                        .strong(),
                                )
                                .wrap(),
                            );
                            if ui
                                .button(translate(
                                    self.locale,
                                    "gui.settings.view_plugin_notices",
                                    &[],
                                ))
                                .clicked()
                            {
                                self.show_plugin_notices = !self.show_plugin_notices;
                            }
                            ui.hyperlink_to(
                                RichText::new(translate(
                                    self.locale,
                                    "gui.plugins.notices_repo_link",
                                    &[],
                                ))
                                .color(accent),
                                THIRD_PARTY_PLUGINS_URL,
                            );
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(translate(self.locale, "gui.plugins.subtitle", &[]))
                                .color(muted),
                        );

                        if !self.plugin_suggestions.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(translate(
                                    self.locale,
                                    "gui.plugins.suggestion_strip_title",
                                    &[],
                                ))
                                .strong()
                                .color(accent),
                            );
                            for (pname, msgs) in &self.plugin_suggestions {
                                for m in msgs {
                                    ui.label(format!("• {pname}: {m}"));
                                }
                            }
                        }

                        if self.show_plugin_notices {
                            ui.add_space(8.0);
                            egui::ScrollArea::vertical()
                                .id_salt("web_plugin_notices")
                                .max_height(220.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(EMBEDDED_THIRD_PARTY_PLUGINS_MD)
                                                .color(muted),
                                        )
                                        .wrap(),
                                    );
                                });
                        }

                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .button(translate(self.locale, "gui.plugins.btn.sample_light", &[]))
                                .clicked()
                            {
                                self.plugin_input = SAMPLE_LIGHT_MANIFEST.to_string();
                                self.stage_web_manifest();
                            }
                            if ui
                                .button(translate(self.locale, "gui.plugins.btn.sample_heavy", &[]))
                                .clicked()
                            {
                                self.plugin_input = SAMPLE_HEAVY_MANIFEST.to_string();
                                self.stage_web_manifest();
                            }
                            if ui
                                .button(translate(self.locale, "gui.plugins.btn.verify", &[]))
                                .clicked()
                            {
                                self.stage_web_manifest();
                            }
                        });
                        ui.add_space(6.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut self.plugin_input)
                                .code_editor()
                                .desired_rows(4)
                                .desired_width(f32::INFINITY),
                        );
                        if let Some(err) = &self.plugin_error {
                            ui.label(RichText::new(err).color(warn_fg));
                        }
                        if let Some(staged) = &self.plugin_staged {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} ({}) v{} · {} · {} · sha256={}",
                                    staged.manifest.name,
                                    staged.manifest.id,
                                    staged.manifest.version,
                                    staged.manifest.author,
                                    staged.manifest.license,
                                    staged.sha256
                                ))
                                .color(accent),
                            );
                            if ui
                                .button(translate(
                                    self.locale,
                                    "gui.plugins.btn.approve_enable",
                                    &[],
                                ))
                                .clicked()
                            {
                                self.approve_web_staged();
                            }
                        }

                        let mut remove_id: Option<String> = None;
                        let mut select_page: Option<(String, String)> = None;
                        if !self.settings.plugins.is_empty() {
                            ui.add_space(8.0);
                            for (id, sha) in &self.settings.plugins {
                                let verified_manifest = self
                                    .plugin_manifests_raw
                                    .get(id)
                                    .and_then(|raw| plugins::verified(raw.as_bytes(), sha).ok());
                                ui.horizontal_wrapped(|ui| {
                                    let title = verified_manifest
                                        .as_ref()
                                        .map(|m| m.name.as_str())
                                        .unwrap_or(id.as_str());
                                    ui.label(RichText::new(format!("{title} ({id})")).strong());
                                    if let Some(manifest) = &verified_manifest {
                                        for page in &manifest.pages {
                                            if ui
                                                .button(format!(
                                                    "{}: {}",
                                                    translate(
                                                        self.locale,
                                                        "gui.plugins.btn.open_page",
                                                        &[]
                                                    ),
                                                    page.title
                                                ))
                                                .clicked()
                                            {
                                                select_page = Some((id.clone(), page.id.clone()));
                                            }
                                        }
                                    }
                                    if ui
                                        .button(translate(
                                            self.locale,
                                            "gui.plugins.btn.disable",
                                            &[],
                                        ))
                                        .clicked()
                                    {
                                        remove_id = Some(id.clone());
                                    }
                                });
                            }
                        }
                        if let Some(sel) = select_page {
                            self.active_plugin_page = Some(sel);
                        }
                        if let Some(id) = remove_id {
                            self.settings.plugins.remove(&id);
                            self.plugin_manifests_raw.remove(&id);
                            if self
                                .active_plugin_page
                                .as_ref()
                                .is_some_and(|(pid, _)| pid == &id)
                            {
                                self.active_plugin_page = None;
                            }
                            self.persist_web_settings();
                            self.refresh_web_plugins(Some(HookEvent::Complete));
                        }
                    });

                    if let Some((pid, page_id)) = self.active_plugin_page.clone() {
                        if let (Some(sha), Some(raw)) = (
                            self.settings.plugins.get(&pid),
                            self.plugin_manifests_raw.get(&pid),
                        ) {
                            if let Ok(manifest) = plugins::verified(raw.as_bytes(), sha) {
                                if let Some(page) = manifest.pages.iter().find(|p| p.id == page_id)
                                {
                                    ui.add_space(16.0);
                                    render_plugin_page(
                                        ui,
                                        &manifest,
                                        page,
                                        sha,
                                        plugin_palette,
                                        self.locale,
                                    );
                                }
                            }
                        }
                    }
                });
            });
    }
}

/// 供发行门禁在浏览器中读取编译期构建标记。
#[wasm_bindgen(js_name = parallaxeBuildInfo)]
pub fn build_info() -> String {
    parallaxe_core::distribution::build_info_json()
}

/// wasm 入口：由 trunk/wasm-bindgen 注入到页面。
#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    let registry = Registry::embedded().map_err(|error| JsValue::from_str(&error.to_string()))?;
    // 固定回放时刻使同一样本在不同端产生可复核的事件、覆盖与退出码。
    let report = replay_builtin_fixture(&registry, "drive-login")
        .map_err(|error| JsValue::from_str(&error))?;
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("缺少 document"))?;
    let canvas = document
        .get_element_by_id("parallaxe_canvas")
        .ok_or_else(|| JsValue::from_str("页面缺少 #parallaxe_canvas"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("#parallaxe_canvas 不是 canvas 元素"))?;
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(move |cc| {
                install_web_cjk_fonts(&cc.egui_ctx);
                Ok(Box::new(WebApp::new(registry, report)))
            }),
        )
        .await
}
