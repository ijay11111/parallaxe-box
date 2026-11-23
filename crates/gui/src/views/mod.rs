//! GUI 视图组件集（纯只读渲染层，三维度正交重构版）。

pub mod coverage_panel;
pub mod explain_drawer;
pub mod guard_panel;
pub mod import_panel;
pub mod overview_minimal;
pub mod overview_normal;
pub mod plan;
pub mod service_matrix;
pub mod target_tree;
pub mod timeline_view;
pub mod top_bar;

use std::path::PathBuf;

use egui::{vec2, Margin, RichText, Rounding, Stroke, Ui};
use parallaxe_core::eligibility::EligibilityNote;
use parallaxe_core::i18n::{has_key, translate, Locale};

use crate::commands::{ExportFormat, OfflineScenario};
use crate::icons::{icon, IconKind};
use crate::layout::{ActiveTab, MobileTab};
use crate::theme::{
    render_admonition, render_pill, subtle_well_frame, DensityMode, Palette, ThemeMode, FONT_BODY,
    FONT_META, RADIUS_PILL, SPACE_1, SPACE_2,
};

/// UI 控件产生的纯用户意图，由 `ParallaxeGuiApp` 统一转交给 `CommandBus` 或只读本地状态。
#[derive(Debug, Clone)]
pub enum UiAction {
    RunProfile(String),
    RunFull,
    RunCustomTargets(Vec<String>),
    RunSingleReprobe(String),
    RunFixture(String),
    RunScenario(OfflineScenario),
    RunSelftest,
    RunUserImportOfflinePreview(Vec<String>),
    RunLinkResolve(Vec<String>),
    SyncRegistry,
    CancelActiveRun,
    RequestGuardPreview {
        source_run_id: String,
        targets: Vec<String>,
    },
    OpenPlan(String),
    SelectTarget(String),
    SelectRun(String),
    SwitchTab(ActiveTab),
    SwitchMobileTab(MobileTab),
    SetTheme(ThemeMode),
    ToggleTheme,
    SetDensity(DensityMode),
    ToggleDensity,
    SetLocale(Locale),
    ToggleCatalogDrawer,
    CloseCatalogDrawer,
    ToggleExplainDrawer,
    CloseExplainDrawer,
    ToggleAboutModal,
    ExportPreview(ExportFormat),
    ExportToFile(ExportFormat, PathBuf),
    ClearFilters,
}

/// 格式化单条资格说明文本：若当前语言包缺失该 `summary_key`，安全回退到中文并显式附注缺失 key（不 panic）。
pub fn format_eligibility_summary(note: &EligibilityNote, locale: Locale) -> String {
    if has_key(locale, &note.summary_key) {
        note.summary(locale)
    } else if has_key(Locale::ZhCn, &note.summary_key) {
        format!(
            "{} {}",
            note.summary(Locale::ZhCn),
            translate(
                locale,
                "gui.eligibility.missing_key_fallback",
                &[&note.summary_key],
            )
        )
    } else {
        translate(
            locale,
            "gui.eligibility.missing_key_fallback",
            &[&note.summary_key],
        )
    }
}

/// 绘制克制的 `ⓘ 资格说明` 交互式胶囊按钮（矢量 `IconKind::Info` 零豆腐块，不弹窗、不阻塞）。
pub fn eligibility_badge_button(
    ui: &mut Ui,
    p: Palette,
    locale: Locale,
    count: usize,
    expanded: bool,
) -> egui::Response {
    let count_str = count.to_string();
    let label = if count > 1 {
        translate(locale, "gui.eligibility.badge_count", &[&count_str])
    } else {
        translate(locale, "gui.eligibility.badge", &[])
    };
    let fill = if expanded { p.accent_soft } else { p.info_bg };
    let border = if expanded { p.accent } else { p.info_fg };
    let fg = if expanded { p.accent } else { p.info_fg };
    let frame = egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, border.gamma_multiply(0.75)))
        .rounding(Rounding::same(RADIUS_PILL))
        .inner_margin(Margin::symmetric(SPACE_2, 2.0));
    let inner = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            icon(ui, IconKind::Info, 12.5, fg);
            ui.add(
                egui::Label::new(RichText::new(label).color(fg).strong().size(FONT_META))
                    .wrap_mode(egui::TextWrapMode::Extend),
            );
        });
    });
    let resp = ui.interact(
        inner.response.rect,
        ui.make_persistent_id(("elig_badge_btn", count)),
        egui::Sense::click(),
    );
    resp.on_hover_text(translate(locale, "gui.eligibility.fixed_notice", &[]))
}

/// 渲染展开后的只读资格说明列表（逐条显示摘要 + 可点击官方帮助链接 + `verified=false` 标记 + 固定免责说明）。
pub fn render_eligibility_notes_panel(
    ui: &mut Ui,
    notes: &[EligibilityNote],
    locale: Locale,
    p: Palette,
) {
    if notes.is_empty() {
        return;
    }
    render_admonition(ui, p.info_bg, p.info_fg, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(SPACE_2, SPACE_1);
            icon(ui, IconKind::Info, 14.0, p.info_fg);
            ui.label(
                RichText::new(translate(locale, "gui.eligibility.panel_title", &[]))
                    .size(FONT_META)
                    .strong()
                    .color(p.info_fg),
            );
            render_pill(
                ui,
                &translate(locale, "gui.eligibility.unverified_pill", &[]),
                p.text_secondary,
                p.card,
                p.border,
            );
        });
        ui.add_space(SPACE_1);
        ui.label(
            RichText::new(translate(locale, "gui.eligibility.fixed_notice", &[]))
                .size(FONT_META)
                .color(p.text_secondary),
        );
        ui.add_space(SPACE_2);
        for (idx, note) in notes.iter().enumerate() {
            if idx > 0 {
                ui.add_space(SPACE_1);
            }
            subtle_well_frame(p)
                .inner_margin(Margin::symmetric(SPACE_2, SPACE_1 + 2.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(format_eligibility_summary(note, locale))
                            .size(FONT_BODY)
                            .color(p.text_primary),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(SPACE_2, 2.0);
                        ui.hyperlink_to(
                            RichText::new(format!(
                                "{}  ({})",
                                translate(locale, "gui.eligibility.official_link", &[]),
                                note.source_url
                            ))
                            .size(FONT_META)
                            .color(p.accent),
                            &note.source_url,
                        );
                        ui.label(
                            RichText::new(format!("verified={}", note.verified))
                                .monospace()
                                .size(FONT_META)
                                .color(p.text_muted),
                        );
                    });
                });
        }
    });
}
