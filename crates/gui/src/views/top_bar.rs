//! 窗口标题栏、分组侧栏导航、分段 Tab 与移动底栏导航组件。
//!
//! 视觉与交互原则：
//! - 顶栏常驻两组分段胶囊（完整/极简、深色/浅色），与拖动区/窗口按钮命中隔离；
//! - 侧栏/底栏未选中项去边框（零"盒子套盒子"），激活项以柔和底色 + 指示条突出；
//! - 分段 Tab 按文字真实宽度自适应排布，杜绝固定宽造成的右侧空洞。

use crate::icons::{paint_icon, IconKind};
use crate::theme::{
    lerp_color, DensityMode, Palette, ThemeMode, FONT_BODY, FONT_META, FONT_TITLE, RADIUS_MD,
    RADIUS_SM, SPACE_1, SPACE_2,
};
use crate::views::UiAction;
use egui::{
    pos2, vec2, Align2, Color32, Context, FontId, Margin, Rect, RichText, Rounding, Sense, Stroke,
    Ui, ViewportCommand,
};
use parallaxe_core::i18n::{translate, Locale};

#[allow(clippy::too_many_arguments)]
pub fn title(
    ctx: &Context,
    p: Palette,
    wide: bool,
    density: DensityMode,
    theme: ThemeMode,
    locale: Locale,
    top_safe_inset: f32,
    actions: &mut Vec<UiAction>,
) {
    let screen_w = ctx.screen_rect().width();
    let compact_mobile = screen_w < 640.0;
    let show_win_controls = !cfg!(target_os = "android") && !compact_mobile;
    let bar_h = 52.0 + top_safe_inset;
    let panel = egui::TopBottomPanel::top("window_title")
        .exact_height(bar_h)
        .frame(egui::Frame::none().fill(p.sidebar).inner_margin(Margin {
            left: 0.0,
            right: if compact_mobile { SPACE_2 } else { 0.0 },
            top: top_safe_inset,
            bottom: 0.0,
        }))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = if compact_mobile { SPACE_1 } else { SPACE_2 };
                let density_normal_lbl = DensityMode::Normal.label_localized(locale);
                let density_minimal_lbl = DensityMode::Minimal.label_localized(locale);
                let theme_dark_lbl = ThemeMode::Dark.label_localized(locale);
                let theme_light_lbl = ThemeMode::Light.label_localized(locale);
                let locale_zh_lbl = translate(locale, "gui.top.locale.zh", &[]);
                let locale_en_lbl = translate(locale, "gui.top.locale.en", &[]);

                let seg_w = |text: &str, with_icon: bool| -> f32 {
                    let text_w = ui.fonts(|f| {
                        f.layout_no_wrap(
                            text.to_owned(),
                            FontId::proportional(FONT_META),
                            p.text_primary,
                        )
                        .size()
                        .x
                    });
                    if with_icon {
                        (24.0 + text_w + 10.0).ceil().max(54.0)
                    } else {
                        (10.0 + text_w + 10.0).ceil().max(38.0)
                    }
                };
                if compact_mobile {
                    // 移动窄屏（<640px）：左侧品牌 + 右侧紧凑单键切换（密度 / 主题 / 语言），保证 380px 不溢出不裁切
                    let d_lbl = density.label_localized(locale);
                    let d_w = seg_w(&d_lbl, true).min(86.0);
                    let controls_w = d_w + 34.0 + 40.0 + SPACE_1 * 4.0 + 4.0;
                    let drag_w = (ui.available_width() - controls_w).max(96.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(drag_w, 52.0), Sense::hover());
                    let painter = ui.painter().with_clip_rect(rect);
                    paint_icon(
                        &painter,
                        Rect::from_center_size(
                            pos2(rect.left() + 18.0, rect.center().y),
                            vec2(20.0, 20.0),
                        ),
                        IconKind::Brand,
                        p.accent,
                    );
                    painter.text(
                        pos2(rect.left() + 36.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        "Parallaxe",
                        FontId::proportional(FONT_TITLE),
                        p.text_primary,
                    );

                    // 触控友好（36px 高紧凑胶囊）快速切换：密度、主题、语言
                    if top_segment(
                        ui,
                        p,
                        Some(if density == DensityMode::Minimal {
                            IconKind::Layers
                        } else {
                            IconKind::Services
                        }),
                        &d_lbl,
                        true,
                        d_w,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::ToggleDensity);
                    }
                    let t_ic = match theme {
                        ThemeMode::Dark => IconKind::Moon,
                        ThemeMode::Light => IconKind::Sun,
                    };
                    if top_segment(ui, p, Some(t_ic), "", true, 34.0)
                        .on_hover_text(theme.label_localized(locale))
                        .clicked()
                    {
                        actions.push(UiAction::ToggleTheme);
                    }
                    let next_loc = match locale {
                        Locale::ZhCn => Locale::EnUs,
                        Locale::EnUs => Locale::ZhCn,
                    };
                    let cur_loc_lbl = match locale {
                        Locale::ZhCn => &locale_zh_lbl,
                        Locale::EnUs => &locale_en_lbl,
                    };
                    if top_segment(ui, p, None, cur_loc_lbl, true, 40.0).clicked() {
                        actions.push(UiAction::SetLocale(next_loc));
                    }
                    return;
                }

                let w_dn = seg_w(&density_normal_lbl, true);
                let w_dm = seg_w(&density_minimal_lbl, true);
                let w_td = seg_w(&theme_dark_lbl, true);
                let w_tl = seg_w(&theme_light_lbl, true);
                let w_lz = seg_w(&locale_zh_lbl, false);
                let w_le = seg_w(&locale_en_lbl, false);

                // 右侧三组常驻分段胶囊（完整/极简、深色/浅色、中文/EN）+ 三个窗口控制按钮
                let win_ctrl_w = if show_win_controls { 132.0 } else { 12.0 };
                let right_controls_w = (w_dn + w_dm + 20.0)
                    + (w_td + w_tl + 20.0)
                    + (w_lz + w_le + 20.0)
                    + win_ctrl_w
                    + SPACE_2 * 4.0;
                let drag_w = (ui.available_width() - right_controls_w).max(96.0);
                let (rect, hit) =
                    ui.allocate_exact_size(vec2(drag_w, 52.0), Sense::click_and_drag());
                let painter = ui.painter().with_clip_rect(rect);
                paint_icon(
                    &painter,
                    Rect::from_center_size(
                        pos2(rect.left() + 24.0, rect.center().y),
                        vec2(22.0, 22.0),
                    ),
                    IconKind::Brand,
                    p.accent,
                );
                painter.text(
                    pos2(rect.left() + 44.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    "Parallaxe box",
                    FontId::proportional(FONT_TITLE),
                    p.text_primary,
                );
                if wide && drag_w >= 340.0 {
                    let sub = translate(locale, "gui.top.subtitle", &[]);
                    painter.text(
                        pos2(rect.left() + 168.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        sub,
                        FontId::proportional(FONT_META),
                        p.text_muted,
                    );
                }
                if hit.double_clicked() {
                    toggle_max(ctx);
                } else if hit.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }

                // 顶栏常驻分组 1：密度分段胶囊（完整 / 极简）
                egui::Frame::none()
                    .fill(p.canvas)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .rounding(Rounding::same(RADIUS_MD))
                    .inner_margin(Margin::same(SPACE_1))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = SPACE_1;
                            if top_segment(
                                ui,
                                p,
                                Some(IconKind::Services),
                                &density_normal_lbl,
                                density == DensityMode::Normal,
                                w_dn,
                            )
                            .on_hover_text(translate(locale, "gui.top.density.tip_normal", &[]))
                            .clicked()
                            {
                                actions.push(UiAction::SetDensity(DensityMode::Normal));
                            }
                            if top_segment(
                                ui,
                                p,
                                Some(IconKind::Layers),
                                &density_minimal_lbl,
                                density == DensityMode::Minimal,
                                w_dm,
                            )
                            .on_hover_text(translate(locale, "gui.top.density.tip_minimal", &[]))
                            .clicked()
                            {
                                actions.push(UiAction::SetDensity(DensityMode::Minimal));
                            }
                        });
                    });

                // 顶栏常驻分组 2：主题分段胶囊（深色 / 浅色）
                egui::Frame::none()
                    .fill(p.canvas)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .rounding(Rounding::same(RADIUS_MD))
                    .inner_margin(Margin::same(SPACE_1))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = SPACE_1;
                            if top_segment(
                                ui,
                                p,
                                Some(IconKind::Moon),
                                &theme_dark_lbl,
                                theme == ThemeMode::Dark,
                                w_td,
                            )
                            .on_hover_text(translate(locale, "gui.top.theme.tip_dark", &[]))
                            .clicked()
                            {
                                actions.push(UiAction::SetTheme(ThemeMode::Dark));
                            }
                            if top_segment(
                                ui,
                                p,
                                Some(IconKind::Sun),
                                &theme_light_lbl,
                                theme == ThemeMode::Light,
                                w_tl,
                            )
                            .on_hover_text(translate(locale, "gui.top.theme.tip_light", &[]))
                            .clicked()
                            {
                                actions.push(UiAction::SetTheme(ThemeMode::Light));
                            }
                        });
                    });

                // 顶栏常驻分组 3：语言分段胶囊（中文 / EN，只换界面文字、绝不改布局与机器码）
                egui::Frame::none()
                    .fill(p.canvas)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .rounding(Rounding::same(RADIUS_MD))
                    .inner_margin(Margin::same(SPACE_1))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = SPACE_1;
                            if top_segment(
                                ui,
                                p,
                                None,
                                &locale_zh_lbl,
                                locale == Locale::ZhCn,
                                w_lz,
                            )
                            .on_hover_text(translate(locale, "gui.top.locale.tip_zh", &[]))
                            .clicked()
                            {
                                actions.push(UiAction::SetLocale(Locale::ZhCn));
                            }
                            if top_segment(
                                ui,
                                p,
                                None,
                                &locale_en_lbl,
                                locale == Locale::EnUs,
                                w_le,
                            )
                            .on_hover_text(translate(locale, "gui.top.locale.tip_en", &[]))
                            .clicked()
                            {
                                actions.push(UiAction::SetLocale(Locale::EnUs));
                            }
                        });
                    });

                ui.add_space(SPACE_1);

                if show_win_controls {
                    for (label, tip_key) in [
                        ("−", "gui.top.win.minimize"),
                        ("□", "gui.top.win.maximize"),
                        ("×", "gui.top.win.close"),
                    ] {
                        if ui
                            .add_sized(
                                [32.0, 32.0],
                                egui::Button::new(
                                    RichText::new(label).size(16.0).color(p.text_secondary),
                                )
                                .frame(false),
                            )
                            .on_hover_text(translate(locale, tip_key, &[]))
                            .clicked()
                        {
                            match label {
                                "−" => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                                "□" => toggle_max(ctx),
                                _ => ctx.send_viewport_cmd(ViewportCommand::Close),
                            }
                        }
                    }
                }
            });
        });

    // 标题栏底部分隔细线
    let rect = panel.response.rect;
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("title_bar_hairline"),
    ));
    painter.line_segment(
        [
            pos2(rect.left(), rect.bottom() - 0.5),
            pos2(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1.0_f32, p.border_subtle),
    );
}

fn top_segment(
    ui: &mut Ui,
    p: Palette,
    kind: Option<IconKind>,
    label: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    let (r, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let sel_t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("pill_sel"), selected, 0.16);
    let hov_t =
        ui.ctx()
            .animate_bool_with_time(response.id.with("pill_hov"), response.hovered(), 0.12);
    let base_fill = lerp_color(Color32::TRANSPARENT, p.surface_hover, hov_t);
    let fill = lerp_color(base_fill, p.accent_soft, sel_t);
    let stroke = if selected {
        Stroke::new(1.0_f32, p.accent.gamma_multiply(0.75))
    } else {
        Stroke::NONE
    };
    let fg = lerp_color(p.text_secondary, p.accent, sel_t.max(hov_t * 0.45));
    ui.painter()
        .rect(r, Rounding::same(RADIUS_SM), fill, stroke);
    if let Some(ic) = kind {
        if label.is_empty() {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(r.center(), vec2(14.0, 14.0)),
                ic,
                fg,
            );
        } else {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(12.5, 12.5)),
                ic,
                fg,
            );
            ui.painter().text(
                pos2(r.left() + 24.0, r.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(FONT_META),
                fg,
            );
        }
    } else {
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(FONT_META),
            fg,
        );
    }
    response
}

fn toggle_max(ctx: &Context) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
}

/// 侧栏无边框现代导航项（未选中透明扁平，悬停柔和填充，选中亮底 + 左侧 3px 强调指示条）。
pub fn sidebar_nav(
    ui: &mut Ui,
    p: Palette,
    kind: IconKind,
    label: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    let (r, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let sel_t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("snav_sel"), selected, 0.16);
    let hov_t =
        ui.ctx()
            .animate_bool_with_time(response.id.with("snav_hov"), response.hovered(), 0.12);

    let fg = lerp_color(
        lerp_color(p.text_secondary, p.text_primary, hov_t),
        p.accent,
        sel_t,
    );
    let base_fill = lerp_color(Color32::TRANSPARENT, p.surface_hover, hov_t);
    let fill = lerp_color(base_fill, p.accent_soft, sel_t);
    let stroke = if selected {
        Stroke::new(1.0_f32, p.accent.gamma_multiply(0.35))
    } else if response.has_focus() {
        Stroke::new(1.0_f32, p.border_hover)
    } else {
        Stroke::NONE
    };
    ui.painter()
        .rect(r, Rounding::same(RADIUS_MD), fill, stroke);

    if sel_t > 0.05 {
        let bar_h = 18.0 * sel_t;
        let bar = Rect::from_center_size(pos2(r.left() + 3.0, r.center().y), vec2(3.0, bar_h));
        ui.painter().rect_filled(bar, Rounding::same(2.0), p.accent);
    }

    paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(15.5, 15.5)),
        kind,
        fg,
    );
    let text_clip = Rect::from_min_max(
        pos2(r.left() + 34.0, r.top()),
        pos2(r.right() - 6.0, r.bottom()),
    );
    let avail_text_w = (text_clip.width()).max(16.0);
    let galley =
        ui.fonts(|f| f.layout_no_wrap(label.to_owned(), FontId::proportional(FONT_BODY), fg));
    let truncated = galley.size().x > avail_text_w;
    let display_text = if truncated {
        let mut s = String::new();
        for ch in label.chars() {
            let mut next = s.clone();
            next.push(ch);
            let w = ui.fonts(|f| {
                f.layout_no_wrap(format!("{next}..."), FontId::proportional(FONT_BODY), fg)
                    .size()
                    .x
            });
            if w > avail_text_w {
                break;
            }
            s = next;
        }
        format!("{s}...")
    } else {
        label.to_owned()
    };
    ui.painter().with_clip_rect(text_clip).text(
        pos2(r.left() + 36.0, r.center().y),
        Align2::LEFT_CENTER,
        display_text,
        FontId::proportional(FONT_BODY),
        fg,
    );
    if truncated {
        response.on_hover_text(label)
    } else {
        response
    }
}

/// 顶部分段 Tab 轨道内的自适应 Tab 项（按文字实测宽度排布，选中浮起，未选中无边框）。
pub fn segment_tab(
    ui: &mut Ui,
    p: Palette,
    kind: IconKind,
    label: &str,
    selected: bool,
) -> egui::Response {
    let font = FontId::proportional(FONT_BODY);
    let text_w = ui.fonts(|f| {
        f.layout_no_wrap(label.to_owned(), font.clone(), p.text_primary)
            .size()
            .x
    });
    let width = (12.0 + 15.0 + 6.0 + text_w + 14.0).ceil();
    let (r, response) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let sel_t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("stab_sel"), selected, 0.16);
    let hov_t =
        ui.ctx()
            .animate_bool_with_time(response.id.with("stab_hov"), response.hovered(), 0.12);

    let fg = lerp_color(
        lerp_color(p.text_secondary, p.text_primary, hov_t),
        p.accent,
        sel_t,
    );
    let base_fill = lerp_color(Color32::TRANSPARENT, p.surface_hover, hov_t);
    let fill = lerp_color(base_fill, p.card, sel_t);
    let stroke = if selected {
        Stroke::new(1.0_f32, p.border)
    } else {
        Stroke::NONE
    };
    ui.painter()
        .rect(r, Rounding::same(RADIUS_SM), fill, stroke);

    paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(14.5, 14.5)),
        kind,
        fg,
    );
    ui.painter().text(
        pos2(r.left() + 31.0, r.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        fg,
    );
    response
}

/// 窄屏/移动底部四栏导航项（>= 44px 触控友好高度，顶部 2.5px 激活指示条）。
pub fn mobile_nav_item(
    ui: &mut Ui,
    p: Palette,
    kind: IconKind,
    label: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    let (r, response) = ui.allocate_exact_size(vec2(width, 46.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let sel_t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("mnav_sel"), selected, 0.16);
    let hov_t =
        ui.ctx()
            .animate_bool_with_time(response.id.with("mnav_hov"), response.hovered(), 0.12);

    let fg = lerp_color(p.text_secondary, p.accent, sel_t.max(hov_t * 0.4));
    let base_fill = lerp_color(Color32::TRANSPARENT, p.surface_hover, hov_t);
    let fill = lerp_color(base_fill, p.accent_soft, sel_t);
    ui.painter().rect_filled(r, Rounding::same(RADIUS_MD), fill);

    if sel_t > 0.05 {
        let bar_w = 24.0 * sel_t;
        let bar = Rect::from_center_size(pos2(r.center().x, r.top() + 2.5), vec2(bar_w, 2.5));
        ui.painter().rect_filled(bar, Rounding::same(1.5), p.accent);
    }

    paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(r.center().x, r.top() + 16.0), vec2(16.0, 16.0)),
        kind,
        fg,
    );
    ui.painter().text(
        pos2(r.center().x, r.bottom() - 10.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(FONT_META),
        fg,
    );
    response
}

/// 兼容旧调用签名的通用导航按钮包装。
pub fn nav(
    ui: &mut Ui,
    p: Palette,
    kind: IconKind,
    label: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    sidebar_nav(ui, p, kind, label, selected, width)
}

/// 无系统装饰时仍可从四条边及四角调整窗口，仅在最外 5 逻辑像素命中。
pub fn resize_edges(ctx: &Context) {
    if ctx.input(|i| i.viewport().maximized == Some(true) || i.viewport().fullscreen == Some(true))
    {
        return;
    }
    let Some(pos) = ctx.pointer_hover_pos() else {
        return;
    };
    let r = ctx.screen_rect();
    let left = pos.x <= r.left() + 5.0;
    let right = pos.x >= r.right() - 5.0;
    let top = pos.y <= r.top() + 5.0;
    let bottom = pos.y >= r.bottom() - 5.0;
    use egui::{CursorIcon as C, ResizeDirection as D};
    let (direction, cursor) = if top && left {
        (D::NorthWest, C::ResizeNwSe)
    } else if bottom && right {
        (D::SouthEast, C::ResizeNwSe)
    } else if top && right {
        (D::NorthEast, C::ResizeNeSw)
    } else if bottom && left {
        (D::SouthWest, C::ResizeNeSw)
    } else if left {
        (D::West, C::ResizeHorizontal)
    } else if right {
        (D::East, C::ResizeHorizontal)
    } else if top {
        (D::North, C::ResizeVertical)
    } else if bottom {
        (D::South, C::ResizeVertical)
    } else {
        return;
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}

/// 侧栏小节分组标题（参考 shadcn-admin / gpui-kit SidebarGroup：浅色静音标签）。
pub fn sidebar_group_label(ui: &mut Ui, p: Palette, title: &str) {
    ui.horizontal(|ui| {
        ui.add_space(SPACE_2);
        ui.label(
            RichText::new(title)
                .size(FONT_META)
                .strong()
                .color(p.text_muted),
        );
    });
    ui.add_space(SPACE_1);
}
