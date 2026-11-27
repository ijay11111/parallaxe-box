//! Stage 10 最终执行（任务 A / B / C）GUI 端到端回归测试：
//! - 任务 A 路径 A：AI 发起检测 → F3 一键授权人话弹窗（桌面 + 移动 390×844 / 360×640，拒绝零发包、确认守 QoS）；
//! - 任务 A 路径 B：设置页「AI 自主调试 → 自主检测」默认关闭、仅存会话内存绝不落盘、打开后提权导出 `AGENT_AUTONOMOUS`、
//!   AI 自主关闭后立即切换关闭并即时收回权限；
//! - 任务 B：「服务送往哪里」地址落点映射（直连 US→CN 醒目提示、代理分层不误报、`show_ip` 默认脱敏/开启显露、
//!   LinkCheck 4 列与导出同步脱敏、零默认技术码泄露）；
//! - 任务 C：`Restricted` / `Blocked` 人话展示 + Google 官网可用性双层参照说明；
//! - 全端布局零横向溢出回归（1440×900 / 780×900 / 390×844 / 360×640 / 800×360 × 双语）。

#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use egui::{Pos2, Rect};
use parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS;
use parallaxe_core::i18n::Locale;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::Registry;
use parallaxe_gui::app::ParallaxeGuiApp;
use parallaxe_gui::commands::ExportFormat;
use parallaxe_gui::layout::ActiveTab;
use parallaxe_gui::theme::{DensityMode, ThemeMode};
use parallaxe_gui::views::ai_debug_panel::AiCheckRequest;

#[derive(Debug, Clone)]
struct TextBox {
    text: String,
    rect: Rect,
}

fn render_boxes(
    app: &mut ParallaxeGuiApp,
    ctx: &egui::Context,
    w: f32,
    h: f32,
    events: Vec<egui::Event>,
) -> Vec<TextBox> {
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))),
            events,
            ..Default::default()
        },
        |c| app.render_ui(c),
    );
    out.shapes
        .into_iter()
        .filter_map(|clipped| {
            if let egui::Shape::Text(t) = clipped.shape {
                let rect = t.visual_bounding_rect();
                let center = rect.center();
                clipped.clip_rect.contains(center).then(|| TextBox {
                    text: t.galley.text().to_string(),
                    rect,
                })
            } else {
                None
            }
        })
        .collect()
}

fn click_at(pos: Pos2) -> Vec<egui::Event> {
    vec![
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
    ]
}

fn inject_fixture_report(app: &mut ParallaxeGuiApp, fixture_id: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let registry = Registry::embedded().unwrap();
    let output = rt
        .block_on(runtime::run_check(
            &registry,
            &CheckOptions {
                fixture: Some(fixture_id.to_string()),
                ..Default::default()
            },
            None,
            false,
            None,
        ))
        .unwrap();
    for ev in &output.events {
        app.apply_event(ev);
    }
    let details: Vec<_> = output
        .report
        .results
        .into_iter()
        .map(|r| (r.target, r.destination))
        .collect();
    app.apply_socket_details(&output.report.run_id, details);
}

#[test]
fn task_a_path_a_f3_modal_human_language_reject_zero_packet_and_responsive() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    assert!(!app.is_ai_autonomous_enabled());

    let req = AiCheckRequest::sample_for_profile(
        "simple",
        "简单检查（15 项）".into(),
        15,
        200, // 故意传入低于 3500ms 的值，验证强制提升到 >= 3500ms
        50,  // 故意传入高于上限的批次，验证强制夹紧到 <= 10
        Locale::ZhCn,
    );
    assert!(req.min_interval_ms >= 3_500);
    assert!(req.batch_size <= 10);
    app.request_ai_f3_authorization(req);

    // 桌面 1280×900 与移动 360×640 均完整可见，无裸露机器码
    for (w, h) in [(1280.0, 900.0), (390.0, 844.0), (360.0, 640.0)] {
        let _ = render_boxes(&mut app, &ctx, w, h, vec![]);
        let boxes = render_boxes(&mut app, &ctx, w, h, vec![]);
        assert!(
            boxes.iter().any(|b| b.text == "AI 请求发起一次网络检测"),
            "missing modal title at {w}x{h}"
        );
        assert!(
            boxes.iter().any(|b| b.text == "一键授权并开始检测"),
            "missing approve CTA at {w}x{h}"
        );
        assert!(
            boxes.iter().any(|b| b.text == "拒绝（保持零发包）"),
            "missing reject button at {w}x{h}"
        );
        for b in &boxes {
            assert!(
                b.rect.right() <= w + 2.0,
                "F3 modal overflow at {w}x{h}: '{}' right={:.1}",
                b.text,
                b.rect.right()
            );
            assert!(
                !b.text.contains("AGENT_AUTONOMOUS"),
                "default view should not leak machine code: {}",
                b.text
            );
        }
    }

    // 点击「拒绝（保持零发包）」立即关闭且不发起任何运行
    let boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let reject = boxes
        .iter()
        .find(|b| b.text == "拒绝（保持零发包）")
        .unwrap();
    let _ = render_boxes(
        &mut app,
        &ctx,
        1280.0,
        900.0,
        click_at(reject.rect.center()),
    );
    assert!(app.ai_debug_state().pending_f3_request.is_none());
    assert!(app.app_state().active_run().is_none());
    assert!(app
        .ai_debug_state()
        .last_notice
        .as_deref()
        .is_some_and(|s| s.contains("零") || s.contains("未发出")));
}

#[test]
fn task_a_path_b_autonomous_switch_memory_only_elevates_and_ai_self_disables() {
    let tmp = std::env::temp_dir().join(format!(
        "parallaxe-stage10-ai-memonly-{}.jsonl",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp);

    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new_with_settings_path(
        Registry::embedded().unwrap(),
        ctx.clone(),
        Some(tmp.clone()),
    );

    // 1. 默认严格关闭，关闭时自主检测与导出均被拒绝（零发包）
    assert!(!app.is_ai_autonomous_enabled());
    assert_eq!(
        app.try_ai_autonomous_check("simple"),
        Err("AUTONOMOUS_DISABLED")
    );
    assert_eq!(
        app.try_ai_autonomous_export(ExportFormat::Json),
        Err("AUTONOMOUS_DISABLED")
    );

    // 2. 用户在设置页打开「AI 自主调试 → 自主检测」
    app.set_ai_autonomous_enabled(true);
    assert!(app.is_ai_autonomous_enabled());

    // 3. 即使此时触发其他持久化设置保存，配置文件绝不写入自主开关；重启后必然恢复关闭
    app.set_view_modes(DensityMode::Normal, ThemeMode::Light);
    if tmp.exists() {
        let raw = std::fs::read_to_string(&tmp).unwrap();
        assert!(
            !raw.contains("autonomous") && !raw.contains("AGENT_AUTONOMOUS"),
            "autonomous switch must NEVER persist to disk: {raw}"
        );
    }
    let reloaded = ParallaxeGuiApp::new_with_settings_path(
        Registry::embedded().unwrap(),
        ctx.clone(),
        Some(tmp.clone()),
    );
    assert!(
        !reloaded.is_ai_autonomous_enabled(),
        "restarted session must reset autonomous switch to OFF"
    );
    let _ = std::fs::remove_file(&tmp);

    // 4. 开启状态下可自主导出脱敏报告，强制携带 origin=AGENT_AUTONOMOUS
    inject_fixture_report(&mut app, "address-mismatch");
    let exported = app
        .try_ai_autonomous_export(ExportFormat::Json)
        .expect("autonomous export should succeed when switch is ON");
    assert!(exported.contains(ORIGIN_AGENT_AUTONOMOUS));
    assert!(
        !exported.contains("203.0.113.9"),
        "autonomous export must remain redacted by default"
    );
    assert_eq!(app.ai_debug_state().autonomous_exports, 1);

    // 5. AI 自主关闭开关后：状态立即回落为 false、显示收回提示、后续自主检测与导出立即被拒
    assert!(app.ai_self_disable_autonomous(Some("批量验收完毕")));
    assert!(!app.is_ai_autonomous_enabled());
    assert!(app.ai_debug_state().revoked_by_ai);
    assert_eq!(
        app.try_ai_autonomous_check("simple"),
        Err("AUTONOMOUS_DISABLED")
    );
    assert_eq!(
        app.try_ai_autonomous_export(ExportFormat::Markdown),
        Err("AUTONOMOUS_DISABLED")
    );

    // 6. 总览页渲染 AI 自关收回提示横幅，且零原始机器码泄露
    app.navigate_to(ActiveTab::Overview);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    assert!(
        boxes
            .iter()
            .any(|b| b.text.contains("AI 已自主关闭") || b.text.contains("权限已收回")),
        "Overview banner should visibly show AI self-disable revocation"
    );
}

#[test]
fn task_b_destination_map_mismatch_proxy_show_ip_and_linkcheck_sync() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());

    // 1. 直连异常样例（解析 US、实连 CN）：结果页呈现「服务送往哪里」与醒目风险提示，默认隐藏原始 IP 与机器码
    inject_fixture_report(&mut app, "address-mismatch");
    app.navigate_to(ActiveTab::Services);
    let _ = render_boxes(&mut app, &ctx, 1440.0, 900.0, vec![]);
    let boxes = render_boxes(&mut app, &ctx, 1440.0, 900.0, vec![]);
    assert!(boxes
        .iter()
        .any(|b| b.text == "服务送往哪里（实际连接落点映射）"));
    assert!(boxes.iter().any(|b| b.text == "落点与解析不一致"));
    assert!(boxes.iter().any(|b| b.text == "连接地区与预期不符"));
    assert!(
        !boxes
            .iter()
            .any(|b| b.text.contains("203.0.113.9") || b.text.contains("192.0.2.8")),
        "raw IPs must stay redacted when show_ip is false"
    );
    for forbidden in [
        "DIRECT_SOCKET",
        "PROXY_NODE",
        "ADDRESS_MISMATCH",
        "UNEXPECTED_REGION",
    ] {
        assert!(
            !boxes.iter().any(|b| b.text.contains(forbidden)),
            "default Services view must not leak raw code {forbidden}"
        );
    }

    // 2. 开启 show_ip 后，服务矩阵与处置中心 LinkCheck 均显露真实对端地址 203.0.113.9；关闭时处置中心同样脱敏
    app.navigate_to(ActiveTab::Guard);
    let _ = render_boxes(&mut app, &ctx, 1440.0, 1500.0, vec![]);
    let guard_redacted = render_boxes(&mut app, &ctx, 1440.0, 1500.0, vec![]);
    assert!(
        !guard_redacted
            .iter()
            .any(|b| b.text.contains("203.0.113.9") || b.text.contains("192.0.2.8")),
        "Guard LinkCheck must redact IPs when show_ip is false"
    );

    app.navigate_to(ActiveTab::Services);
    app.set_show_ip(true);
    let _ = render_boxes(&mut app, &ctx, 1440.0, 900.0, vec![]);
    let ip_boxes = render_boxes(&mut app, &ctx, 1440.0, 900.0, vec![]);
    assert!(
        ip_boxes.iter().any(|b| b.text.contains("203.0.113.9")),
        "peer IP should be visible in Services when show_ip is true"
    );
    app.navigate_to(ActiveTab::Guard);
    let _ = render_boxes(&mut app, &ctx, 1440.0, 1500.0, vec![]);
    let guard_boxes = render_boxes(&mut app, &ctx, 1440.0, 1500.0, vec![]);
    assert!(
        guard_boxes.iter().any(|b| b.text.contains("203.0.113.9")),
        "Guard LinkCheck 4-column table must honor show_ip via analyze_with_ip"
    );
    assert!(
        guard_boxes.iter().any(|b| b.text.contains("192.0.2.8")),
        "Guard LinkCheck DNS column must honor show_ip when enabled"
    );
    app.set_show_ip(false);

    // 3. 代理场景（address-proxy）：分层展示「代理节点」与「代理分层 · 出口不直比」，绝不误报地址不一致
    inject_fixture_report(&mut app, "address-proxy");
    app.navigate_to(ActiveTab::Services);
    let _ = render_boxes(&mut app, &ctx, 1440.0, 900.0, vec![]);
    let proxy_boxes = render_boxes(&mut app, &ctx, 1440.0, 900.0, vec![]);
    assert!(proxy_boxes.iter().any(|b| b.text == "代理节点"));
    assert!(proxy_boxes
        .iter()
        .any(|b| b.text == "代理分层 · 出口不直比"));
    assert!(
        !proxy_boxes.iter().any(|b| b.text == "落点与解析不一致"),
        "proxy scenario must never raise false ADDRESS_MISMATCH warning"
    );
}

#[test]
fn task_c_restricted_blocked_and_google_official_availability_reference() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());

    for (fixture_id, expected_summary_sub) in [
        ("product-restricted", "产品返回明确地区不可用信号"),
        ("product-blocked", "产品返回明确政策拒绝信号"),
    ] {
        inject_fixture_report(&mut app, fixture_id);
        app.navigate_to(ActiveTab::Services);
        let _ = render_boxes(&mut app, &ctx, 1440.0, 900.0, vec![]);
        let boxes = render_boxes(&mut app, &ctx, 1440.0, 900.0, vec![]);
        assert!(
            boxes.iter().any(|b| b.text.contains(expected_summary_sub)),
            "{fixture_id} should render humanized summary '{expected_summary_sub}'"
        );
        // Google 服务同时展示官方可用性参照说明（与 9 态判定严格分离）
        assert!(
            boxes.iter().any(|b| b.text == "官方参照 · 非判定"),
            "{fixture_id} should display separate Google official availability reference badge"
        );
        for forbidden in ["REGION_RESTRICTED", "POLICY_BLOCKED"] {
            assert!(
                !boxes.iter().any(|b| b.text.contains(forbidden)),
                "default view must not leak raw policy token {forbidden}"
            );
        }
    }
}

#[test]
fn stage10_new_views_zero_horizontal_overflow_across_viewports_and_locales() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    inject_fixture_report(&mut app, "address-mismatch");

    for locale in [Locale::ZhCn, Locale::EnUs] {
        app.set_locale(locale);
        for (w, h) in [
            (1440.0, 900.0),
            (780.0, 900.0),
            (390.0, 844.0),
            (360.0, 640.0),
            (800.0, 360.0),
        ] {
            for tab in [ActiveTab::Services, ActiveTab::Settings] {
                app.navigate_to(tab);
                let _ = render_boxes(&mut app, &ctx, w, h, vec![]);
                let boxes = render_boxes(&mut app, &ctx, w, h, vec![]);
                for b in &boxes {
                    assert!(
                        b.rect.right() <= w + 2.0,
                        "overflow on {tab:?} {locale:?} {w}x{h}: '{}' right={:.1}",
                        b.text,
                        b.rect.right()
                    );
                }
            }
        }
    }
}
