//! 阶段 12 GUI 端到端断言：发行包默认规则包（更换 → 授权 → 检测 → 结果页 → 导出）。
#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use egui::Pos2;
use parallaxe_core::targets::Registry;
use parallaxe_gui::app::ParallaxeGuiApp;
use parallaxe_gui::commands::ExportFormat;
use parallaxe_gui::layout::ActiveTab;

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
        .filter_map(|e| match e.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

fn has(texts: &[String], needle: &str) -> bool {
    texts.iter().any(|t| t.contains(needle))
}

#[test]
fn default_bundle_replace_authorize_detect_result_and_export() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());

    // 1. 未确认前：入口卡可见、零发包基线。
    app.navigate_to(ActiveTab::Overview);
    let before = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(
        has(&before, "我的授权版"),
        "总览页缺少授权版入口：{before:#?}"
    );
    assert!(has(&before, "107"), "入口应显示 107 项");

    // 2. 按 stage13 顺序先确认发行包模式，再在专属确认框确认「仅本次」并全选 107 项。
    app.confirm_distribution_mode_for_test();
    let indices: Vec<usize> = (1..=107).collect();
    app.confirm_default_bundle(false, &indices);
    assert_eq!(
        app.registry_mut().user_targets().len(),
        107,
        "批量更换后应有 107 个 user.* 目标"
    );
    assert!(
        app.registry_mut()
            .user_targets()
            .iter()
            .all(|meta| meta.replaced_builtin_id.is_some()),
        "每个目标都必须记录被更换的内置目标"
    );
    assert!(
        app.registry_mut().active_session_grant_snapshot().is_some(),
        "「仅本次」必须注入一次性会话许可"
    );

    // 3. 导入页显示完整 user.* ID（交接文档 1.3 第 1 项）。
    app.navigate_to(ActiveTab::Import);
    let import_texts = texts(&mut app, &ctx, 1440.0, 3000.0);
    assert!(
        has(&import_texts, "user."),
        "导入清单必须显示完整 user.* ID：{import_texts:#?}"
    );
    assert!(
        has(&import_texts, "我的授权版") || has(&import_texts, "已登记的用户导入清单"),
        "导入页应能看到授权版清单"
    );

    // 4. 总览页常驻主按钮 + 独立授权范围入口。
    app.navigate_to(ActiveTab::Overview);
    let overview = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(has(&overview, "我的授权版"), "授权版独立范围入口应始终可见");
    assert!(has(&overview, "检测范围："), "主检测按钮应常驻显示范围摘要");

    // 5. 用离线预演跑一个替换目标，验证 GUI 结果页含该 ID（零网络）。
    let target_id = app
        .registry_mut()
        .user_targets()
        .first()
        .expect("至少一个用户目标")
        .id
        .clone();
    app.run_user_offline_preview_for_test(vec![target_id.clone()]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let _ = texts(&mut app, &ctx, 1440.0, 900.0);
        if !app.has_pending_run_for_test() && app.app_state().active_run().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    let run = app.app_state().active_run().expect("预演运行必须结束");
    assert!(
        run.result_for(&target_id).is_some(),
        "结果页必须包含被替换目标 {target_id}"
    );

    app.navigate_to(ActiveTab::Services);
    let result_texts = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(
        has(&result_texts, "用户导入") || has(&result_texts, "授权版"),
        "结果页缺少用户导入/授权版徽章：{result_texts:#?}"
    );

    // 6. 导出走 GUI 同一路径，导出内容必须含该 ID。
    let out = std::env::temp_dir().join(format!(
        "parallaxe_stage12_export_{}_{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    app.export_active_report_for_test(ExportFormat::Json, &out)
        .expect("GUI 导出");
    let content = std::fs::read_to_string(&out).expect("读取导出文件");
    assert!(
        content.contains(&target_id),
        "导出内容必须包含替换目标 ID {target_id}"
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn result_views_hide_machine_codes_by_default() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.confirm_distribution_mode_for_test();
    app.confirm_default_bundle(false, &(1..=107).collect::<Vec<_>>());
    let ids: Vec<String> = app
        .registry_mut()
        .user_targets()
        .iter()
        .take(3)
        .map(|m| m.id.clone())
        .collect();
    app.run_user_offline_preview_for_test(ids.clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let _ = texts(&mut app, &ctx, 1440.0, 900.0);
        if !app.has_pending_run_for_test() && app.app_state().active_run().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    assert!(app.app_state().active_run().is_some(), "预演必须完成");

    const FORBIDDEN: &[&str] = &[
        "origin=",
        "rule_digest",
        "ADDRESS_MISMATCH",
        "PROXY_NODE",
        "DIRECT_SOCKET",
        "NOT_OBSERVED",
        "UNEXPECTED_REGION",
        "PROXY_NOT_COMPARABLE",
        "LOCAL_TRAFFIC_ONLY",
        "NOT_PROVABLE",
        "ALLOW_ONLINE_FALSE",
        "SAFE_DEFAULT_FALSE",
        "allow_online",
        "DENIED_",
        "ADMITTED",
        "network_request_count",
        "exit=",
        "parallaxe.link/v1",
        "parallaxe.fixtures/v1",
        "parallaxe.agent/v1",
        "parallaxe.user-import/v1",
        "verified=",
        "schema=",
        "account_mode=",
        "session_mode=",
        "consent_version=",
        "imported_at=",
        "run-user-preview",
        "run-gui",
        "run-fixture",
        "session-grant",
        "BUNDLED_DEFAULT_BUNDLE",
        "path-user-offline-preview",
    ];
    let tabs = [
        ActiveTab::Overview,
        ActiveTab::Services,
        ActiveTab::Coverage,
        ActiveTab::Guard,
        ActiveTab::Timeline,
        ActiveTab::Import,
    ];
    let mut hits = Vec::new();
    for (w, h) in [(1440.0_f32, 900.0_f32), (420.0_f32, 860.0_f32)] {
        for tab in tabs {
            app.navigate_to(tab);
            let visible = texts(&mut app, &ctx, w, h);
            for token in FORBIDDEN {
                if has(&visible, token) {
                    hits.push(format!("{tab:?}/{w}x{h}: {token}"));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "默认视图（技术细节关闭）仍出现机器码 {} 处：{hits:#?}",
        hits.len()
    );
}

#[test]
#[ignore = "live network: run explicitly with --ignored --nocapture"]
fn live_authorized_bundle_gui_run_sends_real_requests() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    // 只对前 3 个替换目标做真实网络验收，避免 107 项按 3.5s/批的节流把测试拖过 2 分钟；
    // 107 项的全量启用由离线断言与 core 测试覆盖。
    app.confirm_distribution_mode_for_test();
    app.confirm_default_bundle(false, &[1, 2, 3]);
    let target_id = app
        .registry_mut()
        .user_targets()
        .first()
        .expect("授权版目标")
        .id
        .clone();
    app.run_authorized_bundle_for_test()
        .expect("GUI 授权版真实检测启动");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        let _ = texts(&mut app, &ctx, 1440.0, 900.0);
        if !app.has_pending_run_for_test() && app.app_state().active_run().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let run = app.app_state().active_run().expect("运行必须结束");
    assert!(
        run.result_for(&target_id).is_some(),
        "真实检测结果必须包含授权版目标 {target_id}"
    );
    let report = run.report.as_ref().expect("终态报告");
    assert!(
        report.network_request_count > 0,
        "授权版目标应产生真实网络请求：{report:?}"
    );
    let row = report
        .results
        .iter()
        .find(|r| r.target == target_id)
        .expect("目标结果");
    assert_eq!(
        row.origin.as_deref(),
        Some(parallaxe_core::contracts::ORIGIN_USER_IMPORTED)
    );
}

#[cfg(not(feature = "distribution-release"))]
#[test]
fn default_bundle_modal_shows_full_ids_and_no_machine_codes() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.navigate_to(ActiveTab::Overview);
    let _ = texts(&mut app, &ctx, 1440.0, 1000.0);
    app.open_default_bundle_for_test();
    assert!(app.default_bundle_modal_open_for_test());
    let _ = texts(&mut app, &ctx, 1440.0, 1000.0);
    let modal = texts(&mut app, &ctx, 1440.0, 1000.0);
    assert!(has(&modal, "确认更换默认规则包"), "弹窗未出现：{modal:#?}");
    assert!(has(&modal, "共 107 项"));
    assert!(has(&modal, "受保护 82 项"));
    assert!(
        has(&modal, "user.accounts"),
        "专属确认框必须显示完整 user.* ID"
    );
    assert!(has(&modal, "我确认更换这 82 个受保护目标"));
    for token in [
        "allow_online",
        "origin=",
        "schema=",
        "consent_version=",
        "imported_at=",
        "BUNDLED_DEFAULT_BUNDLE",
        "run-",
    ] {
        assert!(
            !has(&modal, token),
            "专属确认框出现机器码 {token}：{modal:#?}"
        );
    }
}

#[test]
fn unconfirmed_default_bundle_keeps_zero_packet_baseline() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    let before = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(has(&before, "我的授权版"));
    // 未确认时不导入任何目标、不注入授权。
    assert_eq!(app.registry_mut().user_targets().len(), 0);
    assert!(app.registry_mut().active_session_grant_snapshot().is_none());
    assert!(app.registry_mut().authorized_user_target_ids().is_empty());
}

#[cfg(feature = "distribution-release")]
#[test]
fn distribution_default_bundle_modal_only_selects_scope() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
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
    assert!(!has(&modal, "确认更换并授权"));
}
