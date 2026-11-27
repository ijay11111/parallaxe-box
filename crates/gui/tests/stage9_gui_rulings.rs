//! Stage 9 GUI rulings & official app icon verification:
//! - Ruling #1: Rule export forces explicit user directory selection (no silent default directory write; cancel writes 0 files).
//! - Ruling #2: Beta disables & hides system-level blocking entry while keeping rule export and beta notice visible.
//! - App Icon: Embedded 64x64 RGBA window icon decodes cleanly with transparent squircle corners.

#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use std::path::PathBuf;
use std::sync::Mutex;

use egui::Pos2;
use parallaxe_core::contracts::{Coverage, GuardPlan, Mode, Report, Termination};
use parallaxe_core::events::EventLog;
use parallaxe_core::i18n::Locale;
use parallaxe_core::reports;
use parallaxe_core::sysblock;
use parallaxe_core::targets::Registry;
use parallaxe_gui::app::ParallaxeGuiApp;
use parallaxe_gui::app_icon;
use parallaxe_gui::commands::{build_offline_scenario_events, OfflineScenario};
use parallaxe_gui::layout::ActiveTab;

static TEST_PICKER_TARGET: Mutex<Option<PathBuf>> = Mutex::new(None);

fn mock_picker_choose(_prompt: &str) -> Result<Option<PathBuf>, String> {
    Ok(TEST_PICKER_TARGET.lock().unwrap().clone())
}

fn mock_picker_cancel(_prompt: &str) -> Result<Option<PathBuf>, String> {
    Ok(None)
}

fn run_frame(
    app: &mut ParallaxeGuiApp,
    ctx: &egui::Context,
    w: f32,
    h: f32,
    events: Vec<egui::Event>,
) -> Vec<(String, Pos2)> {
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))),
            events,
            ..Default::default()
        },
        |c| app.render_ui(c),
    );
    out.shapes
        .into_iter()
        .filter_map(|e| match e.shape {
            egui::Shape::Text(t) => {
                Some((t.galley.text().to_string(), t.pos + t.galley.size() * 0.5))
            }
            _ => None,
        })
        .collect()
}

fn click_text(
    app: &mut ParallaxeGuiApp,
    ctx: &egui::Context,
    w: f32,
    h: f32,
    needle: &str,
) -> Vec<(String, Pos2)> {
    let labels = run_frame(app, ctx, w, h, vec![]);
    let (_, pos) = labels
        .iter()
        .find(|(s, _)| s == needle || s.contains(needle))
        .unwrap_or_else(|| {
            panic!(
                "did not find label matching `{needle}` in {:?}",
                labels.iter().map(|(s, _)| s).collect::<Vec<_>>()
            )
        });
    let pos = *pos;
    let events = vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ];
    let _ = run_frame(app, ctx, w, h, events);
    run_frame(app, ctx, w, h, vec![])
}

#[tokio::test]
async fn stage9_ruling1_export_requires_explicit_folder_picker_and_cancel_writes_nothing() {
    let base = std::env::temp_dir().join(format!("parallaxe_stage9_export_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let chosen_dir = base.join("user-chosen-export");
    std::fs::create_dir_all(&chosen_dir).unwrap();

    let ctx = egui::Context::default();
    let reg = Registry::embedded().unwrap();
    let mut app = ParallaxeGuiApp::new_with_settings_path(reg.clone(), ctx.clone(), None);

    // Emit a complete guard-preview run with exact-FQDN candidate (google.gemini)
    // and NO_SAFE_BLOCK reason so we also verify EXPORT_WARNING_NO_SAFE_BLOCK renders in UI.
    let now = time::OffsetDateTime::now_utc();
    let recorded_at = parallaxe_core::state::format_time(now);
    let plan = GuardPlan {
        plan_id: "plan-stage9-test".to_string(),
        run_id: "run-stage9-export".to_string(),
        selected_by: "USER".to_string(),
        operation: "PREVIEW".to_string(),
        candidates: vec!["google.gemini".to_string()],
        affected_domains: vec!["support.google.com".to_string()],
        affected_services: Vec::new(),
        affected_shared_paths: Vec::new(),
        impact_closure: Vec::new(),
        reasons: vec!["NO_SAFE_BLOCK".to_string()],
        primary_reason: Some("NO_SAFE_BLOCK".to_string()),
        safe_to_apply: false,
        closure_complete: true,
        applied: false,
        rollback: Vec::new(),
        limitations: Vec::new(),
        effect_scope: "LOCAL_TRAFFIC_ONLY".to_string(),
        remote_account_impact: "NOT_PROVABLE".to_string(),
        shared_egress_guarantee: "UNPROVEN".to_string(),
        source_run_id: Some("run-source-stage9".to_string()),
    };
    let mut log = EventLog::new("run-stage9-export", true, now);
    log.run_started(
        "FIXTURE",
        Some("guard-preview"),
        "parallaxe.assets/v1",
        &["google.gemini".to_string()],
    );
    log.guard_plan_ready(&plan);
    let mut report = Report::new(
        parallaxe_core::VERSION,
        recorded_at,
        "run-stage9-export",
        Mode::Fixture,
    );
    report.coverage = Coverage::empty(reg.catalog_size());
    report.plans = vec![plan];
    report.termination = Termination::Complete;
    report.exit_code = reports::aggregate_report(&report);
    log.run_finished(&report);
    for ev in log.into_events() {
        app.apply_event(&ev);
    }
    app.navigate_to(ActiveTab::Guard);
    assert!(
        app.guard_view_mut().export_selected_dir.is_none(),
        "must never pre-fill a silent default export directory"
    );

    // Open rule export modal (egui::Window settles layout across 2 frames)
    app.guard_view_mut().export_modal_open = true;
    app.guard_view_mut().folder_picker_override = Some(mock_picker_cancel);

    let _ = run_frame(&mut app, &ctx, 1280.0, 1000.0, vec![]);
    let labels = run_frame(&mut app, &ctx, 1280.0, 1000.0, vec![]);
    assert!(
        labels.iter().any(|(s, _)| s.contains("不静默写入固定路径")),
        "modal must clearly state no silent fixed-path writes; got labels: {:?}",
        labels.iter().map(|(s, _)| s).collect::<Vec<_>>()
    );
    assert!(
        labels
            .iter()
            .any(|(s, _)| s.contains("EXPORT_WARNING_NO_SAFE_BLOCK")),
        "EXPORT_WARNING_NO_SAFE_BLOCK pill must remain visible in manual export modal"
    );

    // 1) User cancels folder dialog -> 0 files written, cancel message shown
    let after_cancel = click_text(&mut app, &ctx, 1280.0, 1000.0, "选择导出目录并写入...");
    assert!(
        after_cancel
            .iter()
            .any(|(s, _)| s.contains("已取消选择目录，未写入任何文件")),
        "cancel feedback must be shown"
    );
    assert_eq!(
        std::fs::read_dir(&chosen_dir).unwrap().count(),
        0,
        "cancel must write zero files"
    );

    // 2) User selects explicit directory -> files written ONLY into chosen_dir
    *TEST_PICKER_TARGET.lock().unwrap() = Some(chosen_dir.clone());
    app.guard_view_mut().folder_picker_override = Some(mock_picker_choose);
    let after_write = click_text(&mut app, &ctx, 1280.0, 1000.0, "选择导出目录并写入...");
    assert!(
        after_write
            .iter()
            .any(|(s, _)| s.contains("已写入 3 个规则文件至")),
        "write confirmation must be shown; got: {:?}",
        after_write.iter().map(|(s, _)| s).collect::<Vec<_>>()
    );
    let written_files: Vec<String> = std::fs::read_dir(&chosen_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        !written_files.is_empty(),
        "chosen directory should contain exported rule bundle files"
    );
    assert_eq!(
        app.guard_view_mut().export_selected_dir.as_ref(),
        Some(&chosen_dir)
    );
    assert!(
        !PathBuf::from("./parallaxe-rules").exists(),
        "must not silently create ./parallaxe-rules in workspace"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[tokio::test]
async fn stage9_ruling2_sysblock_disabled_and_hidden_in_beta_both_locales() {
    const _: () = assert!(
        !sysblock::SYSTEM_BLOCK_ENABLED,
        "beta must keep SYSTEM_BLOCK_ENABLED == false"
    );

    let ctx = egui::Context::default();
    let reg = Registry::embedded().unwrap();
    let mut app = ParallaxeGuiApp::new_with_settings_path(reg.clone(), ctx.clone(), None);
    let events = build_offline_scenario_events(
        &reg,
        OfflineScenario::BooksAndSharedCdnGuard,
        "run-stage9-sysblock",
    )
    .await
    .unwrap();
    for ev in &events {
        app.apply_event(ev);
    }
    app.navigate_to(ActiveTab::Guard);

    // Force stale modal state and confirm render_ui discards it immediately
    app.guard_view_mut().sysblock_modal_open = true;
    let zh_labels = run_frame(&mut app, &ctx, 1280.0, 900.0, vec![]);
    assert!(!app.guard_view_mut().sysblock_modal_open);
    assert!(
        !zh_labels.iter().any(|(s, _)| s.contains("启用本机阻断")),
        "sysblock trigger button must be hidden in beta (zh-CN)"
    );
    assert!(
        zh_labels
            .iter()
            .any(|(s, _)| s.contains("测试版不含系统级阻断")),
        "beta sysblock pill should be shown (zh-CN)"
    );

    // Switch to English and re-verify
    app.set_locale(Locale::EnUs);
    let en_labels = run_frame(&mut app, &ctx, 1280.0, 900.0, vec![]);
    assert!(
        !en_labels
            .iter()
            .any(|(s, _)| s.contains("Enable local blocking")),
        "sysblock trigger button must be hidden in beta (en-US)"
    );
    assert!(
        en_labels
            .iter()
            .any(|(s, _)| s.contains("OS blocking disabled in beta")),
        "beta sysblock pill should be shown (en-US)"
    );
}

#[test]
fn stage9_official_app_icon_decodes_cleanly_as_64x64_squircle() {
    let icon = app_icon::window_icon().expect("embedded app icon must decode");
    assert_eq!(icon.width, 64);
    assert_eq!(icon.height, 64);
    assert_eq!(icon.rgba.len(), 64 * 64 * 4);
    // Corner (0, 0) should be transparent (rounded squircle)
    assert_eq!(icon.rgba[3], 0, "top-left corner must be transparent");
    // Center pixel (32, 32) should be opaque
    let center_alpha = icon.rgba[(32 * 64 + 32) * 4 + 3];
    assert_eq!(center_alpha, 255, "center pixel must be opaque");
}

#[test]
fn stage9_export_modals_stay_within_mobile_portrait_and_landscape_bounds_across_rotation() {
    let ctx = egui::Context::default();
    let reg = Registry::embedded().unwrap();
    let mut app = ParallaxeGuiApp::new_with_settings_path(reg.clone(), ctx.clone(), None);

    // Emit a complete-closure GuardPlan so the full bundle + picker controls render inside the modal
    let now = time::OffsetDateTime::now_utc();
    let recorded_at = parallaxe_core::state::format_time(now);
    let plan = GuardPlan {
        plan_id: "plan-stage9-mobile-modal".to_string(),
        run_id: "run-stage9-mobile-modal".to_string(),
        selected_by: "USER".to_string(),
        operation: "PREVIEW".to_string(),
        candidates: vec!["google.gemini".to_string()],
        affected_domains: vec!["support.google.com".to_string()],
        affected_services: Vec::new(),
        affected_shared_paths: Vec::new(),
        impact_closure: Vec::new(),
        reasons: vec!["NO_SAFE_BLOCK".to_string()],
        primary_reason: Some("NO_SAFE_BLOCK".to_string()),
        safe_to_apply: false,
        closure_complete: true,
        applied: false,
        rollback: Vec::new(),
        limitations: Vec::new(),
        effect_scope: "LOCAL_TRAFFIC_ONLY".to_string(),
        remote_account_impact: "NOT_PROVABLE".to_string(),
        shared_egress_guarantee: "UNPROVEN".to_string(),
        source_run_id: Some("run-source-mobile".to_string()),
    };
    let mut log = EventLog::new("run-stage9-mobile-modal", true, now);
    log.run_started(
        "FIXTURE",
        Some("guard-preview"),
        "parallaxe.assets/v1",
        &["google.gemini".to_string()],
    );
    log.guard_plan_ready(&plan);
    let mut report = Report::new(
        parallaxe_core::VERSION,
        recorded_at,
        "run-stage9-mobile-modal",
        Mode::Fixture,
    );
    report.coverage = Coverage::empty(reg.catalog_size());
    report.plans = vec![plan];
    report.termination = Termination::Complete;
    report.exit_code = reports::aggregate_report(&report);
    log.run_finished(&report);
    for ev in log.into_events() {
        app.apply_event(&ev);
    }
    app.navigate_to(ActiveTab::Guard);

    // Test landscape -> portrait rotation on the SAME egui::Context to verify zero stale cached width!
    let viewports = [
        ("landscape-800x360", 800.0_f32, 360.0_f32),
        ("portrait-360x640", 360.0_f32, 640.0_f32),
        ("portrait-390x844", 390.0_f32, 844.0_f32),
        ("portrait-412x732", 412.0_f32, 732.0_f32),
    ];

    for locale in [Locale::EnUs, Locale::ZhCn] {
        app.set_locale(locale);
        for (full_modal, modal_name) in [(false, "export_modal"), (true, "full_modal")] {
            app.guard_view_mut().export_modal_open = !full_modal;
            app.guard_view_mut().full_modal_open = full_modal;
            app.guard_view_mut().full_ack = true;

            for (vp_name, w, h) in viewports {
                // Settle egui::Window auto_sized layout across 2 frames after rotation
                for _ in 0..2 {
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
                }
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))),
                        ..Default::default()
                    },
                    |c| app.render_ui(c),
                );
                let mut saw_close = false;
                for clipped in &out.shapes {
                    if let egui::Shape::Text(t) = &clipped.shape {
                        let txt = t.galley.text();
                        let vis_left = t.pos.x.max(clipped.clip_rect.left());
                        let vis_right =
                            (t.pos.x + t.galley.size().x).min(clipped.clip_rect.right());
                        if txt == "Close" || txt == "关闭" {
                            saw_close = true;
                            assert!(
                                vis_left >= 0.0 && vis_right <= w + 1.0,
                                "{modal_name} {locale:?} {vp_name}: Close button clipped: [{vis_left}, {vis_right}] vs [0, {w}]"
                            );
                        }
                        // Foreground modal shapes must never overflow screen horizontally
                        if t.pos.x < -1.0
                            || (t.pos.x + t.galley.size().x > w + 2.0
                                && clipped.clip_rect.right() > w + 1.0)
                        {
                            panic!(
                                "{modal_name} {locale:?} {vp_name}: text `{txt}` overflows horizontal bounds: pos.x={}, width={}, clip_right={}, screen_w={w}",
                                t.pos.x,
                                t.galley.size().x,
                                clipped.clip_rect.right()
                            );
                        }
                    }
                }
                assert!(
                    saw_close,
                    "{modal_name} {locale:?} {vp_name}: Close button must remain visible without scrolling; texts={:?}",
                    out.shapes
                        .iter()
                        .filter_map(|c| match &c.shape {
                            egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos, c.clip_rect)),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}
