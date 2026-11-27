//! Desktop Settings Center plugin panel, light-plugin suggestion strip, and notices modals.
//!
//! Excluded on Android and iOS (zero plugin surface on mobile).

#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use std::path::Path;

use egui::{RichText, ScrollArea, Ui};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::plugins::Permission;
use parallaxe_core::settings::Settings;

use crate::layout::ActiveTab;
use crate::plugin_host::PluginHost;
use crate::plugin_ui::{
    render_plugin_page, PluginUiPalette, EMBEDDED_THIRD_PARTY_NOTICES_MD,
    EMBEDDED_THIRD_PARTY_PLUGINS_MD, THIRD_PARTY_NOTICES_URL, THIRD_PARTY_PLUGINS_URL,
};
use crate::theme::{
    btn_danger_tonal, btn_outlined, btn_tonal, card_frame, render_admonition, render_modal_scrim,
    render_pill, subtle_well_frame, Palette, FONT_BODY, FONT_META, FONT_TITLE, RADIUS_LG, SPACE_1,
    SPACE_2, SPACE_3, SPACE_4, SPACE_5,
};
use crate::views::UiAction;

/// Outcome of user interactions inside the plugins settings card.
#[derive(Debug, Default)]
pub struct PluginPanelDelta {
    pub settings_changed: bool,
    pub re_invoke: bool,
    pub persist_error: Option<String>,
}

pub fn to_plugin_palette(p: Palette) -> PluginUiPalette {
    PluginUiPalette {
        card: p.card,
        card_alt: p.card_alt,
        border: p.border,
        border_subtle: p.border_subtle,
        accent: p.accent,
        accent_soft: p.accent_soft,
        text_primary: p.text_primary,
        text_secondary: p.text_secondary,
        text_muted: p.text_muted,
        info_bg: p.info_bg,
        info_fg: p.info_fg,
        warning_bg: p.warning_bg,
        warning_fg: p.warning_fg,
    }
}

/// Renders the Plugins card inside the Settings Center (desktop only).
pub fn render_plugins_card(
    ui: &mut Ui,
    host: &mut PluginHost,
    settings: &mut Settings,
    p: Palette,
    locale: Locale,
    actions: &mut Vec<UiAction>,
) -> PluginPanelDelta {
    let mut delta = PluginPanelDelta::default();
    let enabled_count = settings.plugins.len().to_string();

    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
            ui.label(
                RichText::new(translate(locale, "gui.plugins.title", &[]))
                    .strong()
                    .size(FONT_TITLE)
                    .color(p.text_primary),
            );
            render_pill(
                ui,
                &translate(locale, "gui.plugins.enabled_count", &[&enabled_count]),
                p.accent,
                p.accent_soft,
                p.accent,
            );
            if btn_outlined(
                ui,
                p,
                &translate(locale, "gui.settings.view_plugin_notices", &[]),
                true,
            )
            .clicked()
            {
                host.plugin_notices_open = true;
            }
        });
        ui.add_space(SPACE_1);
        ui.add(
            egui::Label::new(
                RichText::new(translate(locale, "gui.plugins.subtitle", &[]))
                    .size(FONT_META)
                    .color(p.text_secondary),
            )
            .wrap(),
        );
        ui.add_space(SPACE_2);

        if ui
            .checkbox(
                &mut host.master_enabled,
                translate(locale, "gui.plugins.master_toggle", &[]),
            )
            .changed()
        {
            delta.re_invoke = true;
        }
        ui.add_space(SPACE_2);

        // Enabled plugins list
        if host.entries.is_empty() {
            subtle_well_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.plugins.empty_list", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
            });
        } else {
            let mut disable_id: Option<String> = None;
            let mut recheck_needed = false;
            for (idx, (id, entry)) in host.entries.iter().enumerate() {
                if idx > 0 {
                    ui.add_space(SPACE_2);
                }
                subtle_well_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let is_heavy = entry.manifest.as_ref().is_some_and(|m| !m.pages.is_empty());
                    let display_name = entry
                        .manifest
                        .as_ref()
                        .map(|m| m.name.as_str())
                        .unwrap_or(id.as_str());

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                        ui.label(
                            RichText::new(display_name)
                                .strong()
                                .size(FONT_BODY)
                                .color(p.text_primary),
                        );
                        ui.label(
                            RichText::new(id)
                                .monospace()
                                .size(FONT_META)
                                .color(p.text_muted),
                        );
                        let kind_key = if is_heavy {
                            "gui.plugins.kind.heavy"
                        } else {
                            "gui.plugins.kind.light"
                        };
                        render_pill(
                            ui,
                            &translate(locale, kind_key, &[]),
                            p.info_fg,
                            p.info_bg,
                            p.info_fg,
                        );
                        if entry.circuit_open {
                            render_pill(
                                ui,
                                &translate(locale, "gui.plugins.status.circuit_open", &[]),
                                p.danger_fg,
                                p.danger_bg,
                                p.danger_fg,
                            );
                        } else if entry.last_error.is_some() || entry.manifest.is_none() {
                            render_pill(
                                ui,
                                &translate(locale, "gui.plugins.status.error", &[]),
                                p.warning_fg,
                                p.warning_bg,
                                p.warning_fg,
                            );
                        } else {
                            render_pill(
                                ui,
                                &translate(locale, "gui.plugins.status.verified", &[]),
                                p.ok_fg,
                                p.ok_bg,
                                p.ok_fg,
                            );
                        }
                    });

                    if let Some(manifest) = &entry.manifest {
                        let perms = if manifest.permissions.is_empty() {
                            translate(locale, "gui.plugins.perm.none", &[])
                        } else {
                            manifest
                                .permissions
                                .iter()
                                .map(|perm| match perm {
                                    Permission::ReportCounts => "report_counts",
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        let short_sha: String = entry.approved_sha256.chars().take(16).collect();
                        ui.add_space(SPACE_1);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "v{} · {}: {} · {}: {} · {}: {} · sha256: {}...",
                                    manifest.version,
                                    translate(locale, "gui.plugins.field.author", &[]),
                                    manifest.author,
                                    translate(locale, "gui.plugins.field.license", &[]),
                                    manifest.license,
                                    translate(locale, "gui.plugins.field.permissions", &[]),
                                    perms,
                                    short_sha,
                                ))
                                .size(FONT_META)
                                .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                    if let Some(err) = &entry.last_error {
                        ui.add_space(SPACE_1);
                        ui.add(
                            egui::Label::new(RichText::new(err).size(FONT_META).color(p.danger_fg))
                                .wrap(),
                        );
                    }

                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                        if host.master_enabled && !entry.circuit_open {
                            if let Some(manifest) = &entry.manifest {
                                for page in &manifest.pages {
                                    let btn_lbl = format!(
                                        "{}: {}",
                                        translate(locale, "gui.plugins.btn.open_page", &[]),
                                        page.title
                                    );
                                    if btn_tonal(ui, p, &btn_lbl, true).clicked() {
                                        actions.push(UiAction::OpenPluginPage {
                                            plugin_id: id.clone(),
                                            page_id: page.id.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        if btn_outlined(
                            ui,
                            p,
                            &translate(locale, "gui.plugins.btn.recheck", &[]),
                            true,
                        )
                        .clicked()
                        {
                            recheck_needed = true;
                        }
                        if btn_danger_tonal(
                            ui,
                            p,
                            &translate(locale, "gui.plugins.btn.disable", &[]),
                            true,
                        )
                        .clicked()
                        {
                            disable_id = Some(id.clone());
                        }
                    });
                });
            }
            if let Some(id) = disable_id {
                settings.plugins.remove(&id);
                settings.plugin_directories.remove(&id);
                host.sync_from_settings(settings);
                delta.settings_changed = true;
                delta.re_invoke = true;
            } else if recheck_needed {
                host.sync_from_settings(settings);
                delta.re_invoke = true;
            }
        }

        ui.add_space(SPACE_3);

        // Add local plugin directory
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
            ui.label(
                RichText::new(translate(locale, "gui.plugins.add_dir_label", &[]))
                    .size(FONT_META)
                    .color(p.text_secondary),
            );
            let input_w = (ui.available_width() - 120.0).clamp(160.0, 420.0);
            ui.add(
                egui::TextEdit::singleline(&mut host.dir_input)
                    .desired_width(input_w)
                    .hint_text(translate(locale, "gui.plugins.add_dir_hint", &[])),
            );
            if btn_tonal(
                ui,
                p,
                &translate(locale, "gui.plugins.btn.verify", &[]),
                !host.dir_input.trim().is_empty(),
            )
            .clicked()
            {
                let trimmed = host.dir_input.trim().to_string();
                host.stage_directory(Path::new(&trimmed));
            }
        });

        let samples = PluginHost::discover_local_sample_dirs();
        if !samples.is_empty() {
            ui.add_space(SPACE_1);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                for (name, path) in samples {
                    let key = if name.contains("light") {
                        "gui.plugins.btn.sample_light"
                    } else {
                        "gui.plugins.btn.sample_heavy"
                    };
                    if btn_outlined(ui, p, &translate(locale, key, &[]), true).clicked() {
                        host.dir_input = path.display().to_string();
                        host.stage_directory(&path);
                    }
                }
            });
        }

        if let Some(err) = &host.staged_error {
            ui.add_space(SPACE_2);
            ui.add(egui::Label::new(RichText::new(err).size(FONT_META).color(p.danger_fg)).wrap());
        }

        if let Some(staged) = host.staged.clone() {
            ui.add_space(SPACE_2);
            render_admonition(ui, p.info_bg, p.info_fg, |ui| {
                ui.label(
                    RichText::new(translate(locale, "gui.plugins.stage_title", &[]))
                        .strong()
                        .size(FONT_BODY)
                        .color(p.info_fg),
                );
                ui.add_space(SPACE_1);
                let perms = if staged.manifest.permissions.is_empty() {
                    translate(locale, "gui.plugins.perm.none", &[])
                } else {
                    staged
                        .manifest
                        .permissions
                        .iter()
                        .map(|perm| match perm {
                            Permission::ReportCounts => "report_counts",
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{} ({}) v{} · {}: {} · {}: {} · {}: {}",
                            staged.manifest.name,
                            staged.manifest.id,
                            staged.manifest.version,
                            translate(locale, "gui.plugins.field.author", &[]),
                            staged.manifest.author,
                            translate(locale, "gui.plugins.field.license", &[]),
                            staged.manifest.license,
                            translate(locale, "gui.plugins.field.permissions", &[]),
                            perms,
                        ))
                        .size(FONT_META)
                        .color(p.text_primary),
                    )
                    .wrap(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{}: {}",
                            translate(locale, "gui.plugins.field.source", &[]),
                            staged.manifest.source
                        ))
                        .size(FONT_META)
                        .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("sha256: {}", staged.sha256))
                            .monospace()
                            .size(FONT_META)
                            .color(p.text_primary),
                    )
                    .wrap(),
                );
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    if btn_tonal(
                        ui,
                        p,
                        &translate(locale, "gui.plugins.btn.approve_enable", &[]),
                        true,
                    )
                    .clicked()
                    {
                        match host.approve_staged(settings) {
                            Ok(_) => {
                                delta.settings_changed = true;
                                delta.re_invoke = true;
                            }
                            Err(err) => {
                                delta.persist_error = Some(err);
                            }
                        }
                    }
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.plugins.btn.cancel_stage", &[]),
                        true,
                    )
                    .clicked()
                    {
                        host.staged = None;
                    }
                });
            });
        }

        if let Some(msg) = &host.status_feedback {
            ui.add_space(SPACE_2);
            ui.add(egui::Label::new(RichText::new(msg).size(FONT_META).color(p.info_fg)).wrap());
        }
    });

    delta
}

/// Renders the non-verdict light-plugin suggestion strip on Overview / Services.
pub fn render_plugin_suggestions_strip(ui: &mut Ui, host: &PluginHost, p: Palette, locale: Locale) {
    if !host.master_enabled || host.suggestions.is_empty() {
        return;
    }
    render_admonition(ui, p.card_alt, p.accent, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
            render_pill(
                ui,
                &translate(locale, "gui.plugins.suggestion_strip_title", &[]),
                p.accent,
                p.accent_soft,
                p.accent,
            );
            for item in &host.suggestions {
                for msg in &item.messages {
                    render_pill(
                        ui,
                        &format!("{}: {}", item.plugin_name, msg),
                        p.text_primary,
                        p.card,
                        p.border,
                    );
                }
            }
        });
    });
    ui.add_space(SPACE_4);
}

/// Renders the active heavy plugin independent page inside the main workspace.
pub fn render_active_plugin_page(
    ui: &mut Ui,
    host: &PluginHost,
    p: Palette,
    locale: Locale,
    actions: &mut Vec<UiAction>,
) {
    if let Some((manifest, page, sha256)) = host.resolve_active_page() {
        render_plugin_page(ui, manifest, page, sha256, to_plugin_palette(p), locale);
    } else {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(translate(locale, "gui.plugins.page_missing", &[]))
                        .size(FONT_BODY)
                        .color(p.text_secondary),
                )
                .wrap(),
            );
            ui.add_space(SPACE_2);
            if btn_outlined(ui, p, &ActiveTab::Settings.label_localized(locale), true).clicked() {
                actions.push(UiAction::SwitchTab(ActiveTab::Settings));
            }
        });
    }
}

/// Renders read-only in-app viewers for `THIRD_PARTY_PLUGINS.md` and `THIRD_PARTY_NOTICES.md`.
pub fn render_notices_modals(
    ctx: &egui::Context,
    host: &mut PluginHost,
    p: Palette,
    locale: Locale,
) {
    if host.plugin_notices_open {
        render_modal_scrim(ctx, "plugin_notices_scrim", p);
        let max_w = (ctx.screen_rect().width() - 32.0).clamp(300.0, 760.0);
        egui::Window::new(translate(locale, "gui.plugins.notices_title", &[]))
            .id(egui::Id::new("third_party_plugins_modal"))
            .order(egui::Order::Foreground)
            .open(&mut host.plugin_notices_open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(max_w)
            .max_width(max_w)
            .collapsible(false)
            .resizable(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(p.card)
                    .stroke(egui::Stroke::new(1.0_f32, p.border))
                    .rounding(egui::Rounding::same(RADIUS_LG))
                    .inner_margin(egui::Margin::same(SPACE_5)),
            )
            .show(ctx, |ui| {
                ui.hyperlink_to(
                    RichText::new(translate(locale, "gui.plugins.notices_repo_link", &[]))
                        .size(FONT_META)
                        .color(p.accent),
                    THIRD_PARTY_PLUGINS_URL,
                );
                ui.add_space(SPACE_2);
                ScrollArea::vertical()
                    .id_salt("third_party_plugins_scroll")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(EMBEDDED_THIRD_PARTY_PLUGINS_MD)
                                    .size(FONT_META)
                                    .color(p.text_primary),
                            )
                            .wrap(),
                        );
                    });
            });
    }
    if host.dep_notices_open {
        render_modal_scrim(ctx, "dep_notices_scrim", p);
        let max_w = (ctx.screen_rect().width() - 32.0).clamp(300.0, 760.0);
        egui::Window::new(translate(locale, "gui.plugins.dep_notices_title", &[]))
            .id(egui::Id::new("third_party_deps_modal"))
            .order(egui::Order::Foreground)
            .open(&mut host.dep_notices_open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(max_w)
            .max_width(max_w)
            .collapsible(false)
            .resizable(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(p.card)
                    .stroke(egui::Stroke::new(1.0_f32, p.border))
                    .rounding(egui::Rounding::same(RADIUS_LG))
                    .inner_margin(egui::Margin::same(SPACE_5)),
            )
            .show(ctx, |ui| {
                ui.hyperlink_to(
                    RichText::new(translate(locale, "gui.plugins.notices_repo_link", &[]))
                        .size(FONT_META)
                        .color(p.accent),
                    THIRD_PARTY_NOTICES_URL,
                );
                ui.add_space(SPACE_2);
                ScrollArea::vertical()
                    .id_salt("third_party_deps_scroll")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(EMBEDDED_THIRD_PARTY_NOTICES_MD)
                                    .size(FONT_META)
                                    .color(p.text_primary),
                            )
                            .wrap(),
                        );
                    });
            });
    }
}
