//! Stage 8 GUI contract tests:
//! - Settings persistence across simulated restarts + partial-record repair;
//! - Light & heavy plugin end-to-end GUI consumers, digest verification, circuit breaker,
//!   zero-verdict-mutation guarantee, and third-party notices viewer;
//! - Multi-viewport English & Chinese text overflow checks across every tab.

#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use std::io::Write;
use std::path::PathBuf;

use egui::Pos2;
use parallaxe_core::i18n::Locale;
use parallaxe_core::plugins::HookEvent;
use parallaxe_core::settings;
use parallaxe_core::targets::Registry;
use parallaxe_gui::app::ParallaxeGuiApp;
use parallaxe_gui::commands::{build_offline_scenario_events, OfflineScenario};
use parallaxe_gui::layout::ActiveTab;
use parallaxe_gui::theme::{DensityMode, ThemeMode};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn render_texts(app: &mut ParallaxeGuiApp, ctx: &egui::Context, w: f32, h: f32) -> Vec<String> {
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))),
            ..Default::default()
        },
        |c| app.render_ui(c),
    );
    out.shapes
        .into_iter()
        .filter_map(|e| match e.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn settings_persist_across_restarts_and_repair_corrupt_tail() {
    let dir = std::env::temp_dir().join(format!("parallaxe_stage8_persist_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let settings_path = dir.join("settings.jsonl");

    // 1. Fresh launch with missing settings file must NOT create any file on disk.
    let ctx = egui::Context::default();
    let reg = Registry::embedded().unwrap();
    let mut app = ParallaxeGuiApp::new_with_settings_path(
        reg.clone(),
        ctx.clone(),
        Some(settings_path.clone()),
    );
    assert!(!settings_path.exists(), "startup load must be zero-write");
    assert_eq!(app.app_state().current_locale(), Locale::ZhCn);

    // 2. Stage and approve the light sample plugin + persist English/Light/Compact settings.
    let light_dir = repo_root().join("plugins/samples/light-report-badge");
    app.plugin_host_mut().stage_directory(&light_dir);
    assert!(app.plugin_host().staged.is_some());
    app.set_view_modes(DensityMode::Minimal, ThemeMode::Light);
    app.set_locale(Locale::EnUs);
    let approved_id = app.approve_staged_plugin().expect("approve light sample");
    assert_eq!(approved_id, "sample.light.report-badge");
    assert!(settings_path.exists());

    // 3. Simulate restart with a fresh ParallaxeGuiApp reading the same settings file.
    let ctx2 = egui::Context::default();
    let app2 =
        ParallaxeGuiApp::new_with_settings_path(reg.clone(), ctx2, Some(settings_path.clone()));
    assert_eq!(app2.app_state().current_locale(), Locale::EnUs);
    assert_eq!(app2.settings_snapshot().theme, "light");
    assert_eq!(app2.settings_snapshot().density, "compact");
    assert!(app2
        .settings_snapshot()
        .plugins
        .contains_key("sample.light.report-badge"));

    // 4. Append a partial trailing record and confirm repair restores writes cleanly.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&settings_path)
        .unwrap()
        .write_all(b"{\"broken")
        .unwrap();
    assert!(settings::update(&settings_path, |_| Ok(())).is_err());
    let repaired = settings::repair(&settings_path).expect("repair");
    assert_eq!(repaired.language, "en-US");
    assert!(settings::update(&settings_path, |_| Ok(())).is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn light_and_heavy_plugins_render_safely_and_fail_closed_on_tamper() {
    let dir = std::env::temp_dir().join(format!("parallaxe_stage8_plugins_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Copy both sample plugins into temp dirs so we can test tampering safely.
    let light_tmp = dir.join("light-copy");
    let heavy_tmp = dir.join("heavy-copy");
    std::fs::create_dir_all(&light_tmp).unwrap();
    std::fs::create_dir_all(&heavy_tmp).unwrap();
    std::fs::copy(
        repo_root().join("plugins/samples/light-report-badge/manifest.json"),
        light_tmp.join("manifest.json"),
    )
    .unwrap();
    std::fs::copy(
        repo_root().join("plugins/samples/heavy-report-guide/manifest.json"),
        heavy_tmp.join("manifest.json"),
    )
    .unwrap();

    let settings_path = dir.join("settings.jsonl");
    let ctx = egui::Context::default();
    let reg = Registry::embedded().unwrap();
    let mut app = ParallaxeGuiApp::new_with_settings_path(
        reg.clone(),
        ctx.clone(),
        Some(settings_path.clone()),
    );

    // Approve both samples explicitly.
    app.plugin_host_mut().stage_directory(&light_tmp);
    app.approve_staged_plugin().expect("approve light");
    app.plugin_host_mut().stage_directory(&heavy_tmp);
    app.approve_staged_plugin().expect("approve heavy");

    // Feed real offline scenario events and verify core verdicts & exit code are untouched.
    let events = build_offline_scenario_events(
        &reg,
        OfflineScenario::BooksAndSharedCdnGuard,
        "run-stage8-plugin-gui",
    )
    .await
    .unwrap();
    for ev in &events {
        app.apply_event(ev);
    }
    assert_eq!(app.app_state().active_run().unwrap().exit_code(), Some(2));

    // Overview shows light plugin coverage badge + resolver + hook display suggestions.
    app.navigate_to(ActiveTab::Overview);
    let overview_texts = render_texts(&mut app, &ctx, 1280.0, 960.0);
    assert!(
        overview_texts.iter().any(|t| t.contains("扩展展示建议")),
        "expected plugin display suggestion strip on Overview"
    );
    assert!(
        overview_texts.iter().any(|t| t.contains("覆盖 ")),
        "expected coverage_badge output from light plugin"
    );
    assert!(
        overview_texts.iter().any(|t| t.contains("报告阅读指南")),
        "expected heavy plugin page entry in sidebar"
    );

    // Open heavy plugin page and confirm all 9 component kinds render cleanly.
    app.plugin_host_mut().active_page =
        Some(("sample.heavy.report-guide".to_string(), "guide".to_string()));
    app.navigate_to(ActiveTab::PluginPage);
    let page_texts = render_texts(&mut app, &ctx, 1280.0, 1400.0);
    assert!(page_texts
        .iter()
        .any(|t| t.contains("如何读懂一次检测结果")));
    assert!(page_texts.iter().any(|t| t.contains("示例页面 · 只读")));
    assert!(page_texts.iter().any(|t| t.contains("60%")));
    assert!(page_texts
        .iter()
        .any(|t| t.contains("parallaxe check --headless")));
    assert!(page_texts.iter().any(|t| t.contains("三个不要")));
    assert!(page_texts.iter().any(|t| t.contains("九态判定之一")));
    assert!(page_texts.iter().any(|t| t.contains("静态描述")));

    // Open third-party notices modal and confirm THIRD_PARTY_PLUGINS.md renders.
    app.plugin_host_mut().plugin_notices_open = true;
    let _ = render_texts(&mut app, &ctx, 1280.0, 900.0);
    let notice_texts = render_texts(&mut app, &ctx, 1280.0, 900.0);
    assert!(
        notice_texts
            .iter()
            .any(|t| t.contains("THIRD_PARTY_PLUGINS")),
        "expected THIRD_PARTY_PLUGINS content in notices modal, got {} text shapes",
        notice_texts.len()
    );
    app.plugin_host_mut().plugin_notices_open = false;

    // Tamper with light plugin manifest bytes -> 3 consecutive errors trip circuit breaker,
    // suggestions drop, and core exit code remains 2.
    let mut tampered = std::fs::read(light_tmp.join("manifest.json")).unwrap();
    tampered.push(b' ');
    std::fs::write(light_tmp.join("manifest.json"), tampered).unwrap();
    for _ in 0..3 {
        app.refresh_plugins(Some(HookEvent::Complete));
    }
    let light_entry = app
        .plugin_host()
        .entries
        .get("sample.light.report-badge")
        .unwrap();
    assert!(light_entry.last_error.is_some());
    // 4th invocation confirms circuit is open.
    app.refresh_plugins(Some(HookEvent::Complete));
    assert!(
        app.plugin_host()
            .entries
            .get("sample.light.report-badge")
            .unwrap()
            .circuit_open,
        "circuit breaker must open after 3 consecutive errors"
    );
    assert_eq!(
        app.app_state().active_run().unwrap().exit_code(),
        Some(2),
        "plugin failure must never alter core exit_code"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn english_and_chinese_views_do_not_overflow_viewport_widths() {
    let ctx = egui::Context::default();
    let reg = Registry::embedded().unwrap();
    let events = build_offline_scenario_events(
        &reg,
        OfflineScenario::BlockedAndConflictMix,
        "run-stage8-overflow",
    )
    .await
    .unwrap();
    let mut app = ParallaxeGuiApp::new(reg, ctx.clone());
    for ev in &events {
        app.apply_event(ev);
    }

    // Enable the heavy sample in memory (no settings file) to check PluginPage too.
    let heavy_dir = repo_root().join("plugins/samples/heavy-report-guide");
    app.plugin_host_mut().stage_directory(&heavy_dir);
    app.approve_staged_plugin().expect("approve heavy");
    app.plugin_host_mut().active_page =
        Some(("sample.heavy.report-guide".to_string(), "guide".to_string()));

    let viewports = [
        ("wide", 1280.0_f32, 900.0_f32),
        ("compact", 840.0_f32, 900.0_f32),
        ("tablet", 650.0_f32, 860.0_f32),
        ("narrow", 420.0_f32, 860.0_f32),
        ("mobile", 380.0_f32, 820.0_f32),
    ];
    let tabs = [
        ActiveTab::Overview,
        ActiveTab::Services,
        ActiveTab::Coverage,
        ActiveTab::Guard,
        ActiveTab::Timeline,
        ActiveTab::Import,
        ActiveTab::Settings,
        ActiveTab::PluginPage,
    ];

    let mut overflows = Vec::new();
    for locale in Locale::ALL {
        app.set_locale(locale);
        for density in [DensityMode::Normal, DensityMode::Minimal] {
            app.set_view_modes(density, ThemeMode::Dark);
            for (vp_name, w, h) in viewports {
                for tab in tabs {
                    app.navigate_to(tab);
                    // Warm up one layout frame, then measure on the settled frame.
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                Pos2::ZERO,
                                egui::vec2(w, h),
                            )),
                            ..Default::default()
                        },
                        |c| app.render_ui(c),
                    );
                    let out = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                Pos2::ZERO,
                                egui::vec2(w, h),
                            )),
                            ..Default::default()
                        },
                        |c| app.render_ui(c),
                    );
                    for clipped in out.shapes {
                        if let egui::Shape::Text(text) = clipped.shape {
                            let bbox = text.visual_bounding_rect();
                            let raw_right = bbox.right();
                            let visible_right = raw_right.min(clipped.clip_rect.right());
                            let is_top_bar = bbox.top() < 52.0;
                            let is_non_hscroll_tab = matches!(
                                tab,
                                ActiveTab::Overview
                                    | ActiveTab::Settings
                                    | ActiveTab::Import
                                    | ActiveTab::PluginPage
                            );
                            let is_inside_sub_scroll = clipped.clip_rect.right() < w - 20.0;
                            if clipped.clip_rect.right() > w + 2.0
                                || visible_right > w + 2.0
                                || bbox.left() < -2.0
                                || (is_top_bar && raw_right > w + 2.0)
                                || (is_non_hscroll_tab
                                    && !is_inside_sub_scroll
                                    && raw_right > w + 2.0)
                            {
                                overflows.push(format!(
                                    "{:?}/{:?} {vp_name}({w}x{h}) {:?}: '{}' bbox=[{:.1}..{:.1}] clip_r={:.1} w={w}",
                                    locale,
                                    density,
                                    tab,
                                    text.galley.text().replace('\n', " "),
                                    bbox.left(),
                                    raw_right,
                                    clipped.clip_rect.right()
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(
        overflows.is_empty(),
        "Detected {} text viewport overflow(s):\n{}",
        overflows.len(),
        overflows
            .into_iter()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n")
    );
}
