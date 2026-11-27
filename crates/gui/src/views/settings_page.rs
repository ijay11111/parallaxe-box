//! 设置页多级导航（一级栏目列表 → 二级独立页）。
//!
//! 设计约束（用户 2026-11-25 参考图）：
//! - 一级页：一个圆角卡片，彩色方角图标 + 标题 + 一句话副标题 + 右箭头，项间细分割线；
//! - 二级页：顶部「← 返回」+ 居中标题 +（高级网络）黄色警示条 + 具体控件；
//! - 语言项**例外**：作为一级页内的直选项（分段胶囊），不进入二级页，与顶栏常驻胶囊同步；
//! - 手机端单列、触控行高 ≥ 56px（≥44px 触控区）。

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Rounding, Sense, Stroke, Ui};

use parallaxe_core::i18n::{translate, Locale};

use crate::icons::{paint_icon, IconKind};
use crate::theme::{
    render_admonition, Palette, FONT_BODY, FONT_META, FONT_TITLE, RADIUS_MD, SPACE_1, SPACE_2,
    SPACE_3, SPACE_4,
};
use crate::views::UiAction;

/// 设置页层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Root,
    Appearance,
    Scope,
    Advanced,
    AiDebug,
    AddressBook,
    Rules,
    Notices,
    About,
}

impl SettingsPage {
    /// 二级页标题（返回 `gui.settings.nav.*` 的 i18n key）。
    pub fn title_key(self) -> &'static str {
        match self {
            SettingsPage::Root => "gui.settings.root.title",
            SettingsPage::Appearance => "gui.settings.nav.appearance",
            SettingsPage::Scope => "gui.settings.nav.scope",
            SettingsPage::Advanced => "gui.settings.nav.advanced",
            SettingsPage::AiDebug => "gui.settings.nav.ai",
            SettingsPage::AddressBook => "gui.settings.nav.address",
            SettingsPage::Rules => "gui.settings.nav.rules",
            SettingsPage::Notices => "gui.settings.nav.notices",
            SettingsPage::About => "gui.settings.nav.about",
        }
    }

    /// 该页是否需要顶部黄色警示条（专业选项）。
    pub fn warning_key(self) -> Option<&'static str> {
        match self {
            SettingsPage::Advanced => Some("gui.settings.warning.advanced"),
            _ => None,
        }
    }
}

struct NavItem {
    page: SettingsPage,
    icon: IconKind,
    title_key: &'static str,
    subtitle_key: &'static str,
    tint: Tint,
}

#[derive(Clone, Copy)]
enum Tint {
    Blue,
    Orange,
    Purple,
    Cyan,
    Green,
    Slate,
}

fn tint_color(tint: Tint, p: Palette) -> Color32 {
    match tint {
        Tint::Blue => p.accent,
        Tint::Orange => p.warning_fg,
        Tint::Purple => Color32::from_rgb(124, 96, 214),
        Tint::Cyan => Color32::from_rgb(18, 138, 158),
        Tint::Green => p.ok_fg,
        Tint::Slate => p.text_secondary,
    }
}

fn tint_bg(color: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 32)
}

fn nav_items() -> &'static [NavItem] {
    &[
        NavItem {
            page: SettingsPage::Appearance,
            icon: IconKind::Sun,
            title_key: "gui.settings.nav.appearance",
            subtitle_key: "gui.settings.nav.appearance_sub",
            tint: Tint::Blue,
        },
        NavItem {
            page: SettingsPage::Scope,
            icon: IconKind::Coverage,
            title_key: "gui.settings.nav.scope",
            subtitle_key: "gui.settings.nav.scope_sub",
            tint: Tint::Orange,
        },
        NavItem {
            page: SettingsPage::Advanced,
            icon: IconKind::Settings,
            title_key: "gui.settings.nav.advanced",
            subtitle_key: "gui.settings.nav.advanced_sub",
            tint: Tint::Purple,
        },
        NavItem {
            page: SettingsPage::AiDebug,
            icon: IconKind::Spark,
            title_key: "gui.settings.nav.ai",
            subtitle_key: "gui.settings.nav.ai_sub",
            tint: Tint::Blue,
        },
        NavItem {
            page: SettingsPage::AddressBook,
            icon: IconKind::Catalog,
            title_key: "gui.settings.nav.address",
            subtitle_key: "gui.settings.nav.address_sub",
            tint: Tint::Cyan,
        },
        NavItem {
            page: SettingsPage::Rules,
            icon: IconKind::Layers,
            title_key: "gui.settings.nav.rules",
            subtitle_key: "gui.settings.nav.rules_sub",
            tint: Tint::Green,
        },
        NavItem {
            page: SettingsPage::Notices,
            icon: IconKind::Plugin,
            title_key: "gui.settings.nav.notices",
            subtitle_key: "gui.settings.nav.notices_sub",
            tint: Tint::Purple,
        },
        NavItem {
            page: SettingsPage::About,
            icon: IconKind::Info,
            title_key: "gui.settings.nav.about",
            subtitle_key: "gui.settings.nav.about_sub",
            tint: Tint::Slate,
        },
    ]
}

fn row_height() -> f32 {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        64.0
    } else {
        56.0
    }
}

/// 一级页：栏目列表卡片（含语言直选项）。
pub fn render_root(
    ui: &mut Ui,
    p: Palette,
    locale: Locale,
    page: &mut SettingsPage,
    actions: &mut Vec<UiAction>,
) {
    egui::Frame::none()
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.border))
        .rounding(Rounding::same(crate::theme::RADIUS_LG))
        .inner_margin(egui::Margin {
            left: SPACE_2,
            right: SPACE_2,
            top: SPACE_2,
            bottom: SPACE_2,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            language_row(ui, p, locale, actions);
            divider(ui, p);
            for (idx, item) in nav_items().iter().enumerate() {
                let response = nav_row(
                    ui,
                    p,
                    item.icon,
                    tint_color(item.tint, p),
                    &translate(locale, item.title_key, &[]),
                    &translate(locale, item.subtitle_key, &[]),
                );
                if response.clicked() {
                    *page = item.page;
                }
                if idx + 1 < nav_items().len() {
                    divider(ui, p);
                }
            }
        });
}

fn divider(ui: &mut Ui, p: Palette) {
    let y = ui.cursor().top();
    let rect = Rect::from_min_max(
        pos2(ui.max_rect().left() + 56.0, y),
        pos2(ui.max_rect().right() - SPACE_2, y),
    );
    ui.painter()
        .hline(rect.x_range(), y, Stroke::new(1.0_f32, p.border_subtle));
}

fn nav_row(
    ui: &mut Ui,
    p: Palette,
    icon: IconKind,
    tint: Color32,
    title: &str,
    subtitle: &str,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, row_height()), Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, Rounding::same(RADIUS_MD), p.surface_hover);
        }
        let tile =
            Rect::from_center_size(pos2(rect.left() + 28.0, rect.center().y), vec2(36.0, 36.0));
        ui.painter()
            .rect_filled(tile, Rounding::same(9.0), tint_bg(tint));
        paint_icon(ui.painter(), tile.shrink(9.0), icon, tint);

        let text_left = rect.left() + 56.0;
        let max_w = (rect.right() - 44.0 - text_left).max(40.0);
        let title_galley = ui.painter().layout(
            title.to_owned(),
            FontId::proportional(FONT_BODY),
            p.text_primary,
            max_w,
        );
        let sub_galley = ui.painter().layout(
            subtitle.to_owned(),
            FontId::proportional(FONT_META),
            p.text_secondary,
            max_w,
        );
        let block_h = title_galley.size().y + SPACE_1 + sub_galley.size().y;
        let top = rect.center().y - block_h * 0.5;
        ui.painter()
            .galley(pos2(text_left, top), title_galley, p.text_primary);
        ui.painter().galley(
            pos2(text_left, top + (block_h - sub_galley.size().y)),
            sub_galley,
            p.text_secondary,
        );

        paint_icon(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(16.0, 16.0)),
            IconKind::ChevronRight,
            p.text_muted,
        );
    }
    response
}

/// 语言项：高频全局切换，保留为一级页直选项（与顶栏胶囊同步，不进入二级页）。
fn language_row(ui: &mut Ui, p: Palette, locale: Locale, actions: &mut Vec<UiAction>) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, row_height()), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let tint = tint_color(Tint::Cyan, p);
    let tile = Rect::from_center_size(pos2(rect.left() + 28.0, rect.center().y), vec2(36.0, 36.0));
    ui.painter()
        .rect_filled(tile, Rounding::same(9.0), tint_bg(tint));
    paint_icon(ui.painter(), tile.shrink(9.0), IconKind::Globe, tint);

    let text_left = rect.left() + 56.0;
    let title = translate(locale, "gui.settings.language", &[]);
    let subtitle = translate(locale, "gui.settings.language_hint", &[]);
    let max_w = (rect.right() - 140.0 - text_left).max(40.0);
    let title_galley = ui.painter().layout(
        title,
        FontId::proportional(FONT_BODY),
        p.text_primary,
        max_w,
    );
    let sub_galley = ui.painter().layout(
        subtitle,
        FontId::proportional(FONT_META),
        p.text_secondary,
        max_w,
    );
    let block_h = title_galley.size().y + SPACE_1 + sub_galley.size().y;
    let top = rect.center().y - block_h * 0.5;
    ui.painter()
        .galley(pos2(text_left, top), title_galley, p.text_primary);
    ui.painter().galley(
        pos2(text_left, top + (block_h - sub_galley.size().y)),
        sub_galley,
        p.text_secondary,
    );

    // 分段胶囊：中文 / EN（触控高 36px，含内边距后 ≥44px 触控区）。
    let seg_w = 54.0;
    let gap = SPACE_1;
    let en_rect = Rect::from_min_size(
        pos2(rect.right() - SPACE_2 - seg_w, rect.center().y - 16.0),
        vec2(seg_w, 32.0),
    );
    let zh_rect = Rect::from_min_size(
        pos2(en_rect.left() - gap - seg_w, en_rect.top()),
        vec2(seg_w, 32.0),
    );
    for (seg_rect, loc, label) in [
        (zh_rect, Locale::ZhCn, "中文"),
        (en_rect, Locale::EnUs, "EN"),
    ] {
        let selected = locale == loc;
        let resp = ui.interact(
            seg_rect,
            ui.id().with(("settings_lang_seg", label)),
            Sense::click(),
        );
        let fill = if selected { p.accent_soft } else { p.card_alt };
        let border = if selected { p.accent } else { p.border_subtle };
        ui.painter()
            .rect_filled(seg_rect, Rounding::same(8.0), fill);
        ui.painter()
            .rect_stroke(seg_rect, Rounding::same(8.0), Stroke::new(1.0_f32, border));
        ui.painter().text(
            seg_rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(FONT_META),
            if selected { p.accent } else { p.text_secondary },
        );
        if resp.clicked() {
            actions.push(UiAction::SetLocale(loc));
        }
    }
}

/// 二级页头：返回按钮 + 居中标题 +（按需）黄色警示条。
pub fn render_header(ui: &mut Ui, p: Palette, locale: Locale, page: &mut SettingsPage) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 44.0), Sense::hover());
    let back_rect =
        Rect::from_min_size(pos2(rect.left(), rect.center().y - 20.0), vec2(96.0, 40.0));
    let back = ui.interact(back_rect, ui.id().with("settings_back"), Sense::click());
    if ui.is_rect_visible(rect) {
        if back.hovered() {
            ui.painter()
                .rect_filled(back_rect, Rounding::same(RADIUS_MD), p.surface_hover);
        }
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                pos2(back_rect.left() + 14.0, back_rect.center().y),
                vec2(14.0, 14.0),
            ),
            IconKind::ChevronLeft,
            p.text_secondary,
        );
        ui.painter().text(
            pos2(back_rect.left() + 26.0, back_rect.center().y),
            Align2::LEFT_CENTER,
            translate(locale, "gui.settings.back", &[]),
            FontId::proportional(FONT_BODY),
            p.text_primary,
        );
        ui.painter().text(
            pos2(rect.center().x, rect.center().y),
            Align2::CENTER_CENTER,
            translate(locale, page.title_key(), &[]),
            FontId::proportional(FONT_TITLE),
            p.text_primary,
        );
    }
    if back.clicked() {
        *page = SettingsPage::Root;
    }

    if let Some(key) = page.warning_key() {
        ui.add_space(SPACE_2);
        render_admonition(ui, p.warning_bg, p.warning_fg, |ui| {
            ui.horizontal_top(|ui| {
                paint_icon(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min, vec2(18.0, 18.0)),
                    IconKind::Alert,
                    p.warning_fg,
                );
                ui.add_space(SPACE_3 + 6.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(translate(locale, key, &[]))
                            .size(FONT_META)
                            .color(p.warning_fg),
                    )
                    .wrap(),
                );
            });
        });
    }
    ui.add_space(SPACE_4);
}

/// 供测试/走读直接进入指定二级页（不影响真实用户路径）。
pub fn open(page: &mut SettingsPage, target: SettingsPage) {
    *page = target;
}
