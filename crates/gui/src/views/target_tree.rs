//! 按需展开的目标目录抽屉（零 Emoji 豆腐块，明确显示登记、准入、保护三种独立标记）。

use std::collections::{BTreeMap, BTreeSet};

use egui::{RichText, ScrollArea, Ui};
use parallaxe_core::targets::{Registry, FAMILIES};

use crate::icons::{icon, IconKind};
use crate::state::AppState;
use crate::theme::{family_label, render_pill, Palette, ThemeMode};
use crate::views::UiAction;

/// 渲染按需打开的目标目录抽屉（不再塞入 9 项 fixture 碎片与 6 类场景长文本，保持清爽无遮挡）。
pub fn render(
    ui: &mut Ui,
    registry: &Registry,
    state: &AppState,
    theme: ThemeMode,
    checked_targets: &mut BTreeSet<String>,
    tree_filter: &mut String,
    actions: &mut Vec<UiAction>,
) {
    let p = Palette::for_theme(theme);

    ui.horizontal(|ui| {
        icon(ui, IconKind::Catalog, 16.0, p.accent);
        ui.label(
            RichText::new(format!("目标目录（{} 项）", registry.catalog_size()))
                .strong()
                .size(14.0)
                .color(p.text_primary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(RichText::new("关闭 ×").color(p.text_secondary))
                .clicked()
            {
                actions.push(UiAction::CloseCatalogDrawer);
            }
        });
    });

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        render_pill(ui, "保护", p.warning_fg, p.warning_bg, p.warning_fg);
        render_pill(ui, "未准入", p.text_secondary, p.card_alt, p.border);
        render_pill(ui, "已准入", p.ok_fg, p.ok_bg, p.ok_fg);
    });
    ui.add_space(6.0);

    // 搜索与已勾选操作条
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(tree_filter)
                .hint_text("搜索目标 ID / 分组 / 家族...")
                .desired_width(ui.available_width().max(120.0)),
        );
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if !checked_targets.is_empty() {
            if ui
                .small_button(
                    RichText::new(format!("检测已选 ({})", checked_targets.len()))
                        .color(p.accent)
                        .strong(),
                )
                .clicked()
            {
                actions.push(UiAction::RunCustomTargets(Vec::new()));
            }
            if ui.small_button("清空勾选").clicked() {
                checked_targets.clear();
            }
        } else {
            ui.label(
                RichText::new("点击目标名查看证据解释；勾选可发起自选检测。")
                    .size(11.5)
                    .color(p.text_secondary),
            );
        }
    });

    ui.separator();

    // 按家族分组目录（仅保留目标树本身，独立 ScrollArea 永不越界）
    let mut grouped: BTreeMap<&str, Vec<&parallaxe_core::Target>> = BTreeMap::new();
    let filter_lower = tree_filter.trim().to_ascii_lowercase();
    for entry in registry.entries() {
        if !filter_lower.is_empty()
            && !entry.raw.id.to_ascii_lowercase().contains(&filter_lower)
            && !entry.raw.group.to_ascii_lowercase().contains(&filter_lower)
            && !entry
                .raw
                .family
                .to_ascii_lowercase()
                .contains(&filter_lower)
        {
            continue;
        }
        grouped
            .entry(entry.raw.family.as_str())
            .or_default()
            .push(entry);
    }

    let now_str = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
    let mut all_families: Vec<&str> = Vec::with_capacity(FAMILIES.len() + 1);
    if grouped.contains_key("user") {
        all_families.push("user");
    }
    all_families.extend_from_slice(&FAMILIES);

    ScrollArea::vertical()
        .id_salt("catalog_drawer_tree_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for family in all_families {
                let Some(items) = grouped.get(family) else {
                    continue;
                };
                let header_title =
                    format!("{} ({}) · {}", family_label(family), family, items.len());
                egui::CollapsingHeader::new(RichText::new(header_title).size(12.5).strong().color(
                    if family == "user" {
                        p.accent
                    } else {
                        p.text_primary
                    },
                ))
                .id_salt(format!("drawer_fam_{family}"))
                .default_open(
                    !filter_lower.is_empty()
                        || family == "user"
                        || family == "ai"
                        || family == "identity",
                )
                .show(ui, |ui| {
                    for target in items {
                        let admission = registry.admission(target, false, &now_str);
                        let mut is_checked = checked_targets.contains(target.id());
                        let is_selected = state.selected_target_id.as_deref() == Some(target.id());

                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut is_checked, "").changed() {
                                if is_checked {
                                    checked_targets.insert(target.id().to_string());
                                } else {
                                    checked_targets.remove(target.id());
                                }
                            }
                            if target.raw.protected {
                                icon(ui, IconKind::Lock, 12.0, p.warning_fg);
                            }
                            let label =
                                RichText::new(target.id()).size(12.0).color(if is_selected {
                                    p.accent
                                } else {
                                    p.text_primary
                                });
                            let resp = ui.selectable_label(is_selected, label);
                            if resp.clicked() {
                                actions.push(UiAction::SelectTarget(target.id().to_string()));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let (tag, col) = if admission.admitted {
                                        ("准入", p.ok_fg)
                                    } else {
                                        ("待准入", p.text_secondary)
                                    };
                                    ui.label(RichText::new(tag).size(10.5).color(col));
                                },
                            );
                        });
                    }
                });
            }
        });
}
