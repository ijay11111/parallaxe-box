//! stage13 GUI：关于页源码开关、强制确认、Release 隐藏与 AI 真值表。

#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use egui::Pos2;
use parallaxe_core::distribution::DISTRIBUTION_RELEASE_BUILD;
use parallaxe_core::targets::Registry;
use parallaxe_gui::app::ParallaxeGuiApp;
use parallaxe_gui::layout::ActiveTab;
use parallaxe_gui::views::settings_page::SettingsPage;

fn texts(app: &mut ParallaxeGuiApp, ctx: &egui::Context, w: f32, h: f32) -> Vec<String> {
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))),
            ..Default::default()
        },
        |ctx| app.render_ui(ctx),
    );
    out.shapes
        .into_iter()
        .filter_map(|entry| match entry.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

fn has(rows: &[String], needle: &str) -> bool {
    rows.iter().any(|row| row.contains(needle))
}

fn about(app: &mut ParallaxeGuiApp, ctx: &egui::Context, w: f32, h: f32) -> Vec<String> {
    app.navigate_to(ActiveTab::Settings);
    app.open_settings_page(SettingsPage::About);
    texts(app, ctx, w, h)
}

fn builtin_admitted(app: &ParallaxeGuiApp) -> bool {
    let target = app.registry().get("google.gemini").unwrap().clone();
    let now = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
    app.registry().admission(&target, false, &now).admitted
}

#[cfg(not(feature = "distribution-release"))]
#[test]
fn source_about_shows_switch_and_requires_confirmation() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    assert!(!app.is_distribution_mode_enabled());

    for size in [(1440.0, 900.0), (420.0, 860.0)] {
        let rows = about(&mut app, &ctx, size.0, size.1);
        assert!(
            has(&rows, "发行包模式"),
            "源码关于页缺少模式入口：{rows:#?}"
        );
        assert!(has(&rows, "切换到发行包模式"));
    }

    app.open_distribution_mode_confirm_for_test();
    assert!(app.distribution_mode_confirm_open_for_test());
    assert!(!app.is_distribution_mode_enabled(), "打开弹窗本身不得提权");
    let _ = texts(&mut app, &ctx, 420.0, 860.0);
    let modal = texts(&mut app, &ctx, 420.0, 860.0);
    assert!(
        app.distribution_mode_confirm_open_for_test(),
        "确认弹窗状态被意外关闭"
    );
    assert!(
        has(&modal, "确认切换到发行包模式"),
        "确认弹窗未出现：{modal:#?}"
    );
    assert!(has(&modal, "3.5 秒间隔"));
    assert!(has(&modal, "403/429"));
    assert!(has(&modal, "AI 自主调试与本开关完全独立"));
    assert!(has(&modal, "取消（保持零发包）"));

    app.confirm_distribution_mode_for_test();
    assert!(app.is_distribution_mode_enabled());
    assert!(!app.is_ai_autonomous_enabled(), "开关 A 不得联动开关 B");
    assert!(app.settings_snapshot().distribution_mode);
}

#[cfg(feature = "distribution-release")]
#[test]
fn release_about_hides_source_switch_on_desktop_and_mobile() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    assert!(DISTRIBUTION_RELEASE_BUILD);
    assert!(app.is_distribution_mode_enabled());

    for size in [(1440.0, 900.0), (420.0, 860.0)] {
        let rows = about(&mut app, &ctx, size.0, size.1);
        assert!(
            !has(&rows, "切换到发行包模式"),
            "Release 关于页不得显示源码开关"
        );
        assert!(
            !has(&rows, "发行包模式"),
            "Release 关于页不得显示零发包模式控制：{rows:#?}"
        );
        assert!(
            !has(&rows, "零发包"),
            "Release 关于页不得显示零发包设置：{rows:#?}"
        );
    }
    app.open_distribution_mode_confirm_for_test();
    assert!(!app.distribution_mode_confirm_open_for_test());
    app.disable_distribution_mode_for_test();
    assert!(
        app.is_distribution_mode_enabled(),
        "Release 模式不可运行时关闭"
    );
}

#[test]
fn distribution_and_ai_switches_are_independent() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    let initial_mode = app.is_distribution_mode_enabled();
    assert!(!app.is_ai_autonomous_enabled());

    app.confirm_distribution_mode_for_test();
    assert_eq!(
        app.is_distribution_mode_enabled(),
        initial_mode || !DISTRIBUTION_RELEASE_BUILD
    );
    assert!(!app.is_ai_autonomous_enabled());

    app.set_ai_autonomous_enabled(true);
    assert!(app.is_ai_autonomous_enabled());
    assert_eq!(
        app.is_distribution_mode_enabled(),
        initial_mode || !DISTRIBUTION_RELEASE_BUILD
    );

    app.set_ai_autonomous_enabled(false);
    assert!(!app.is_ai_autonomous_enabled());
    assert_eq!(
        app.is_distribution_mode_enabled(),
        initial_mode || !DISTRIBUTION_RELEASE_BUILD
    );
}

#[cfg(not(feature = "distribution-release"))]
#[test]
fn source_truth_table_mode_gate_and_ai_switch_four_cells() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());

    // 源码 A 关 + B 开：AI 开关可用，但内置准入仍为零发包。
    app.set_ai_autonomous_enabled(true);
    assert!(!builtin_admitted(&app));
    assert!(app
        .try_ai_autonomous_check("stage13-invalid-profile")
        .is_ok());

    // 源码 A 开 + B 关：内置可发包，但 AI 自主被拒绝，需 F3。
    app.set_ai_autonomous_enabled(false);
    app.confirm_distribution_mode_for_test();
    assert!(builtin_admitted(&app));
    assert_eq!(
        app.try_ai_autonomous_check("stage13-invalid-profile"),
        Err("AUTONOMOUS_DISABLED")
    );

    // 源码 A 开 + B 开：内置准入 + AI 自主均放行（无效 profile 保证测试不联网）。
    app.set_ai_autonomous_enabled(true);
    assert!(builtin_admitted(&app));
    assert!(app
        .try_ai_autonomous_check("stage13-invalid-profile")
        .is_ok());

    // 撤销 A：内置准入立即恢复零发包，B 状态不被联动改变。
    app.disable_distribution_mode_for_test();
    assert!(!builtin_admitted(&app));
    assert!(app.is_ai_autonomous_enabled());
}

#[cfg(feature = "distribution-release")]
#[test]
fn release_truth_table_ai_off_needs_f3_and_ai_on_is_autonomous() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    assert!(builtin_admitted(&app));
    assert_eq!(
        app.try_ai_autonomous_check("stage13-invalid-profile"),
        Err("AUTONOMOUS_DISABLED")
    );

    app.set_ai_autonomous_enabled(true);
    assert!(builtin_admitted(&app));
    assert!(app
        .try_ai_autonomous_check("stage13-invalid-profile")
        .is_ok());
}

#[test]
fn default_bundle_requires_mode_then_only_selects_scope() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    if !DISTRIBUTION_RELEASE_BUILD {
        app.confirm_default_bundle(false, &[1]);
        assert!(
            app.registry().user_targets().is_empty(),
            "A 关闭时不得导入默认包"
        );
    }

    app.confirm_distribution_mode_for_test();
    app.navigate_to(ActiveTab::Overview);
    let _ = texts(&mut app, &ctx, 1440.0, 1000.0);
    app.open_default_bundle_for_test();
    let _ = texts(&mut app, &ctx, 1440.0, 1000.0);
    let modal = texts(&mut app, &ctx, 1440.0, 1000.0);
    assert!(has(&modal, "选择授权版生效范围"), "{modal:#?}");
    assert!(has(&modal, "仅本次生效"));
    assert!(has(&modal, "永久生效（可随时撤销）"));
    assert!(!has(&modal, "全选目标"));
    assert!(!has(&modal, "受保护"));
}

#[cfg(not(feature = "distribution-release"))]
#[test]
fn source_mode_setting_persists_and_can_be_revoked() {
    let base = std::env::temp_dir().join("opencode").join(format!(
        "parallaxe_stage13_settings_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let path = base.join("settings.jsonl");
    let ctx = egui::Context::default();

    let mut app = ParallaxeGuiApp::new_with_settings_path(
        Registry::embedded().unwrap(),
        ctx.clone(),
        Some(path.clone()),
    );
    app.confirm_distribution_mode_for_test();
    assert!(
        parallaxe_core::settings::load(&path)
            .unwrap()
            .distribution_mode
    );

    let mut restored = ParallaxeGuiApp::new_with_settings_path(
        Registry::embedded().unwrap(),
        ctx.clone(),
        Some(path.clone()),
    );
    assert!(restored.is_distribution_mode_enabled());
    restored.disable_distribution_mode_for_test();
    assert!(
        !parallaxe_core::settings::load(&path)
            .unwrap()
            .distribution_mode
    );

    let restored_again = ParallaxeGuiApp::new_with_settings_path(
        Registry::embedded().unwrap(),
        ctx,
        Some(path.clone()),
    );
    assert!(!restored_again.is_distribution_mode_enabled());
    let _ = std::fs::remove_dir_all(&base);
}
