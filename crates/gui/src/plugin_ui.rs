//! Shared declarative plugin page renderer (`parallaxe.plugin/v1`) for Desktop and Web.
//!
//! Excluded on Android and iOS via `#[cfg(not(any(target_os = "android", target_os = "ios")))]`.
//! Security boundaries:
//! - Plugins never access egui/DOM directly; this host renders only the 9 bounded component types.
//! - Markdown rejects/neutralizes embedded HTML and unsafe links (`javascript:`, `data:`, `file:`).
//! - Forms and buttons are static read-only descriptions in v1 (disabled with clear tooltips).
//! - Plugin text is display-only and never modifies core verdicts or exit codes.

#![cfg(not(any(target_os = "android", target_os = "ios")))]

use egui::{Color32, Margin, RichText, Rounding, ScrollArea, Stroke, Ui};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::plugins::{Component, Manifest, Page};

/// Public third-party plugin notices embedded read-only for the in-app viewer.
pub const EMBEDDED_THIRD_PARTY_PLUGINS_MD: &str = include_str!("../../../THIRD_PARTY_PLUGINS.md");

/// Public third-party dependency notices embedded read-only for the in-app viewer.
pub const EMBEDDED_THIRD_PARTY_NOTICES_MD: &str = include_str!("../../../THIRD_PARTY_NOTICES.md");

/// Public repository links for third-party declarations and plugin protocol.
pub const THIRD_PARTY_PLUGINS_URL: &str =
    "https://github.com/ijay11111/parallaxe-box/blob/main/THIRD_PARTY_PLUGINS.md";
pub const THIRD_PARTY_NOTICES_URL: &str =
    "https://github.com/ijay11111/parallaxe-box/blob/main/THIRD_PARTY_NOTICES.md";

/// Platform-neutral palette subset shared by Desktop (`Palette`) and Web (`WebApp`).
#[derive(Debug, Clone, Copy)]
pub struct PluginUiPalette {
    pub card: Color32,
    pub card_alt: Color32,
    pub border: Color32,
    pub border_subtle: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub info_bg: Color32,
    pub info_fg: Color32,
    pub warning_bg: Color32,
    pub warning_fg: Color32,
}

/// Checks if a markdown line contains raw HTML tags or unsafe URI schemes.
fn sanitize_markdown_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if lower.contains("<script")
        || lower.contains("<iframe")
        || lower.contains("<img")
        || lower.contains("<html")
        || lower.contains("<svg")
        || lower.contains("javascript:")
        || lower.contains("data:")
        || lower.contains("vbscript:")
    {
        return "[blocked unsafe markup]".to_string();
    }
    // Strip angle-bracketed tags defensively so raw HTML can never be misinterpreted.
    if line.contains('<') && line.contains('>') {
        line.replace('<', "(").replace('>', ")")
    } else {
        line.to_string()
    }
}

/// Renders a heavy plugin's declarative `Page` with provenance header and all 9 component types.
pub fn render_plugin_page(
    ui: &mut Ui,
    manifest: &Manifest,
    page: &Page,
    sha256: &str,
    p: PluginUiPalette,
    locale: Locale,
) {
    // Provenance & safety banner
    egui::Frame::none()
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.border))
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                ui.label(
                    RichText::new(&page.title)
                        .size(18.0)
                        .strong()
                        .color(p.text_primary),
                );
                render_plugin_pill(
                    ui,
                    &translate(locale, "gui.plugins.page_readonly_badge", &[]),
                    p.info_fg,
                    p.info_bg,
                    p.info_fg,
                );
            });
            ui.add_space(6.0);
            let short_sha: String = sha256.chars().take(16).collect();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(12.0, 4.0);
                ui.label(
                    RichText::new(format!(
                        "{} ({}) · v{}",
                        manifest.name, manifest.id, manifest.version
                    ))
                    .size(12.0)
                    .color(p.text_secondary),
                );
                ui.label(
                    RichText::new(format!(
                        "{}: {} · {}: {}",
                        translate(locale, "gui.plugins.field.author", &[]),
                        manifest.author,
                        translate(locale, "gui.plugins.field.license", &[]),
                        manifest.license,
                    ))
                    .size(12.0)
                    .color(p.text_muted),
                );
                ui.label(
                    RichText::new(format!("sha256: {short_sha}..."))
                        .monospace()
                        .size(12.0)
                        .color(p.text_muted),
                );
            });
        });

    ui.add_space(16.0);

    // Page components body
    egui::Frame::none()
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.border))
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if page.components.is_empty() {
                ui.label(
                    RichText::new(translate(locale, "gui.plugins.page_empty", &[]))
                        .size(14.0)
                        .color(p.text_secondary),
                );
                return;
            }
            for (idx, component) in page.components.iter().enumerate() {
                if idx > 0 {
                    ui.add_space(12.0);
                }
                ui.push_id(("plugin_comp", &page.id, idx), |ui| {
                    render_component(ui, component, p, locale);
                });
            }
        });
}

fn render_plugin_pill(ui: &mut Ui, text: &str, fg: Color32, bg: Color32, border: Color32) {
    egui::Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, border.gamma_multiply(0.75)))
        .rounding(Rounding::same(16.0))
        .inner_margin(Margin::symmetric(8.0, 2.0))
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).color(fg).strong().size(12.0)).wrap());
        });
}

fn render_component(ui: &mut Ui, component: &Component, p: PluginUiPalette, locale: Locale) {
    match component {
        Component::Heading { text } => {
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .size(16.0)
                        .strong()
                        .color(p.text_primary),
                )
                .wrap(),
            );
        }
        Component::Badge { text } => {
            ui.horizontal_wrapped(|ui| {
                render_plugin_pill(ui, text, p.accent, p.accent_soft, p.accent);
            });
        }
        Component::Callout { text } => {
            let resp = egui::Frame::none()
                .fill(p.info_bg)
                .stroke(Stroke::new(1.0_f32, p.info_fg.gamma_multiply(0.55)))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin {
                    left: 14.0,
                    right: 12.0,
                    top: 10.0,
                    bottom: 10.0,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(RichText::new(text).size(14.0).color(p.text_primary))
                            .wrap(),
                    );
                });
            let rect = resp.response.rect;
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 1.0, rect.top() + 4.0),
                egui::pos2(rect.left() + 4.0, rect.bottom() - 4.0),
            );
            ui.painter()
                .rect_filled(bar, Rounding::same(2.0), p.info_fg);
        }
        Component::Markdown { text } => {
            egui::Frame::none()
                .fill(p.card_alt)
                .stroke(Stroke::new(1.0_f32, p.border_subtle))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::same(12.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for raw_line in text.lines() {
                        let trimmed = raw_line.trim();
                        if trimmed.is_empty() {
                            ui.add_space(4.0);
                            continue;
                        }
                        let safe = sanitize_markdown_line(trimmed);
                        if let Some(h) = safe.strip_prefix("### ") {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(h).size(14.5).strong().color(p.text_primary),
                                )
                                .wrap(),
                            );
                        } else if let Some(h) = safe.strip_prefix("## ") {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(h).size(15.5).strong().color(p.text_primary),
                                )
                                .wrap(),
                            );
                        } else if let Some(h) = safe.strip_prefix("# ") {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(h).size(16.5).strong().color(p.text_primary),
                                )
                                .wrap(),
                            );
                        } else if let Some(item) =
                            safe.strip_prefix("- ").or_else(|| safe.strip_prefix("* "))
                        {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("• {item}"))
                                        .size(14.0)
                                        .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        } else {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(safe).size(14.0).color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        }
                    }
                });
        }
        Component::Code { text } => {
            egui::Frame::none()
                .fill(p.card_alt)
                .stroke(Stroke::new(1.0_f32, p.border_subtle))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(text)
                                    .monospace()
                                    .size(12.5)
                                    .color(p.text_primary),
                            )
                            .wrap(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(translate(locale, "gui.plugins.copy_code", &[]))
                                .clicked()
                            {
                                ui.ctx().copy_text(text.clone());
                            }
                        });
                    });
                });
        }
        Component::Progress { label, value } => {
            let clamped = (*value).min(100);
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::Label::new(RichText::new(label).size(13.0).color(p.text_secondary))
                        .wrap(),
                );
                ui.label(
                    RichText::new(format!("{clamped}%"))
                        .monospace()
                        .size(12.0)
                        .color(p.accent),
                );
            });
            ui.add_space(4.0);
            let width = ui.available_width().max(60.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
            if ui.is_rect_visible(rect) {
                ui.painter()
                    .rect_filled(rect, Rounding::same(3.0), p.border_subtle);
                let fill_w = rect.width() * (f32::from(clamped) / 100.0);
                if fill_w > 0.0 {
                    let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, 6.0));
                    ui.painter()
                        .rect_filled(fill_rect, Rounding::same(3.0), p.accent);
                }
            }
        }
        Component::Table { headers, rows } => {
            let outer_w = ui.available_width();
            egui::Frame::none()
                .fill(p.card_alt)
                .stroke(Stroke::new(1.0_f32, p.border_subtle))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_max_width(outer_w);
                    ScrollArea::horizontal()
                        .id_salt("plugin_table_hscroll")
                        .max_width(outer_w)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            egui::Grid::new("plugin_table_grid")
                                .num_columns(headers.len().max(1))
                                .striped(true)
                                .spacing([12.0, 6.0])
                                .min_col_width(80.0)
                                .show(ui, |ui| {
                                    for header in headers {
                                        ui.label(
                                            RichText::new(header)
                                                .strong()
                                                .size(12.0)
                                                .color(p.text_primary),
                                        );
                                    }
                                    ui.end_row();
                                    for row in rows {
                                        for cell in row {
                                            ui.label(
                                                RichText::new(cell)
                                                    .size(12.5)
                                                    .color(p.text_secondary),
                                            );
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                });
        }
        Component::Form { label } => {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::Label::new(RichText::new(label).size(13.0).color(p.text_secondary))
                        .wrap(),
                );
            });
            ui.add_space(4.0);
            let mut placeholder = translate(locale, "gui.plugins.static_form_hint", &[]);
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut placeholder)
                    .desired_width(ui.available_width().min(360.0)),
            )
            .on_disabled_hover_text(translate(
                locale,
                "gui.plugins.static_control_tip",
                &[],
            ));
        }
        Component::Button { label } => {
            ui.add_enabled(
                false,
                egui::Button::new(RichText::new(label).size(13.0).color(p.text_muted))
                    .fill(p.card_alt)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .rounding(Rounding::same(8.0)),
            )
            .on_disabled_hover_text(translate(
                locale,
                "gui.plugins.static_control_tip",
                &[],
            ));
        }
    }
}
