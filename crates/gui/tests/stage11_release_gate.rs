//! 阶段 11 放行前断言：
//! 1) 设置页多级结构（一级栏目列表 + 二级独立页 + 语言项直选例外）；
//! 2) 默认视图机器码清零（7 大页 × 中英 × 明暗 × 桌面/移动，技术码 0 命中）；
//! 3) 技术码只在「显示技术细节」开启后才出现（同一视图对照）。
#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use egui::Pos2;
use parallaxe_core::ai_bridge::{AiScope, BridgePaths, BridgeRequest};
use parallaxe_core::i18n::Locale;
use parallaxe_core::targets::Registry;
use parallaxe_gui::app::ParallaxeGuiApp;
use parallaxe_gui::layout::ActiveTab;
use parallaxe_gui::theme::{DensityMode, ThemeMode};
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
        .filter_map(|e| match e.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

fn has(texts: &[String], needle: &str) -> bool {
    texts.iter().any(|t| t.contains(needle))
}

/// 默认视图不得出现的机器码/内部字段。
const FORBIDDEN: &[&str] = &[
    "origin=",
    "rule_digest",
    "ADDRESS_MISMATCH",
    "PROXY_NODE",
    "DIRECT_SOCKET",
    "NOT_OBSERVED",
    "UNEXPECTED_REGION",
    "network_request_count",
    "exit=",
    "parallaxe.link/v1",
    "parallaxe.fixtures/v1",
    "parallaxe.agent/v1",
    "verified=",
    "schema=",
    "account_mode=",
    "session_mode=",
    "consent_version=",
    "imported_at=",
    "run-gui",
    "run-fixture",
];

#[test]
fn default_views_have_zero_machine_codes_across_locales_themes_and_viewports() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    let tabs = [
        ActiveTab::Overview,
        ActiveTab::Services,
        ActiveTab::Coverage,
        ActiveTab::Guard,
        ActiveTab::Timeline,
        ActiveTab::Import,
        ActiveTab::Settings,
    ];
    let viewports = [(1440.0_f32, 900.0_f32), (420.0_f32, 860.0_f32)];
    let mut hits = Vec::new();
    for locale in Locale::ALL {
        app.set_locale(locale);
        for theme in [ThemeMode::Dark, ThemeMode::Light] {
            app.set_view_modes(DensityMode::Normal, theme);
            for (w, h) in viewports {
                for tab in tabs {
                    app.navigate_to(tab);
                    // 设置页一级列表与每个二级页都扫描一遍。
                    let pages: &[SettingsPage] = if tab == ActiveTab::Settings {
                        &[
                            SettingsPage::Root,
                            SettingsPage::Appearance,
                            SettingsPage::Scope,
                            SettingsPage::Advanced,
                            SettingsPage::AiDebug,
                            SettingsPage::AddressBook,
                            SettingsPage::Rules,
                            SettingsPage::Notices,
                            SettingsPage::About,
                        ]
                    } else {
                        &[SettingsPage::Root]
                    };
                    for page in pages {
                        if tab == ActiveTab::Settings {
                            app.open_settings_page(*page);
                        }
                        let visible = texts(&mut app, &ctx, w, h);
                        for token in FORBIDDEN {
                            if has(&visible, token) {
                                hits.push(format!(
                                    "{:?}/{:?}/{:?}/{}x{}: {token}",
                                    locale, theme, page, w, h
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "默认视图出现机器码 {} 处：{:#?}",
        hits.len(),
        hits
    );
}

#[test]
fn tech_codes_only_appear_when_technical_details_enabled() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.navigate_to(ActiveTab::Settings);
    app.open_settings_page(SettingsPage::Advanced);
    let plain = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(
        !has(&plain, "source="),
        "默认视图不应出现技术码：{plain:#?}"
    );
    app.set_show_technical_details(true);
    let verbose = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(
        has(&verbose, "source=") || has(&verbose, "cooldown="),
        "开启技术细节后仍看不到技术码：{verbose:#?}"
    );
}

#[test]
fn settings_root_is_a_nav_list_and_language_stays_a_direct_option() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.navigate_to(ActiveTab::Settings);
    let root = texts(&mut app, &ctx, 1440.0, 900.0);
    for label in [
        "界面语言",
        "外观与主题",
        "检测范围",
        "高级网络",
        "AI 自主调试",
        "本地地址数据",
        "规则与导出",
        "第三方声明",
        "关于与隐私",
    ] {
        assert!(has(&root, label), "一级列表缺少「{label}」：{root:#?}");
    }
    // 语言项是直选分段胶囊（中文 / EN），不是栏目跳转。
    assert!(has(&root, "中文") && has(&root, "EN"));
    // 一级页不直接展示二级页的控件。
    assert!(!has(&root, "检测节奏与网络环境"));

    // 语言切换后一级页与顶栏同步（英文标签）。
    app.set_locale(Locale::EnUs);
    let root_en = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(has(&root_en, "Language"));
    assert!(has(&root_en, "Appearance & theme"));
    assert!(has(&root_en, "Advanced network"));
}

#[test]
fn settings_subpages_are_independent_pages_with_header_and_warning() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.navigate_to(ActiveTab::Settings);

    app.open_settings_page(SettingsPage::Advanced);
    let advanced = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(has(&advanced, "返回"), "二级页缺少返回按钮");
    assert!(has(&advanced, "高级网络"), "二级页缺少居中标题");
    assert!(
        has(&advanced, "以下为专业选项"),
        "高级网络页缺少黄色警示条：{advanced:#?}"
    );
    assert!(has(&advanced, "检测节奏与网络环境"));

    app.open_settings_page(SettingsPage::AiDebug);
    let ai = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(has(&ai, "AI 自主调试"));
    assert!(has(&ai, "自主检测"));

    app.open_settings_page(SettingsPage::AddressBook);
    let address = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(has(&address, "地址簿"));
    assert!(has(&address, "预期对端国家码"));

    app.open_settings_page(SettingsPage::Rules);
    let rules = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(has(&rules, "导出与阻断默认项"));

    app.open_settings_page(SettingsPage::About);
    let about = texts(&mut app, &ctx, 1440.0, 900.0);
    assert!(has(&about, "关于与许可声明"));
}

/// B 模式用户视角闭环（需网络与 `--ignored`）：GUI 开启自主检测 → agent 写请求 →
/// GUI 自动执行真实发包 → 回传脱敏报告，报告带 AGENT_AUTONOMOUS 来源。
#[test]
#[ignore = "live network: run explicitly with --ignored --nocapture"]
fn live_gui_autonomous_bridge_runs_and_returns_redacted_report() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    let root = std::env::temp_dir().join(format!(
        "parallaxe_gui_bridge_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let _ = std::fs::remove_dir_all(&root);
    app.use_bridge_root_for_test(root.clone());

    // 1. 用户导入自己的网址（B 模式只对用户导入目标发包）。
    let bundle = parallaxe_core::import_bundle::parse_import_text(
        "GUI 自主验收 | https://example.com/ | rule.http,rule.trace | Example 首页 | example-home",
        &["rule.http".to_string(), "rule.trace".to_string()],
        false,
    )
    .expect("解析导入文本");
    let imported = app
        .registry_mut()
        .import_bundle(&bundle, true, true)
        .expect("导入自定义网址");
    let target_id = imported.imported_targets[0].id.clone();
    app.sync_registry_for_test();

    // 2. 用户打开「AI 自主调试 → 自主检测」开关。
    app.set_ai_autonomous_enabled(true);

    // 3. agent 通过文件桥发送请求。
    let scope = AiScope::for_targets([target_id.clone()], "GUI 自主验收");
    let request = BridgeRequest::new(scope, 3_500, 5).expect("合法请求");
    BridgePaths::with_root(&root)
        .write_request(&request)
        .expect("写请求");

    // 4. GUI 轮询后自动执行（无需用户点按）。
    app.poll_ai_bridge_for_test();
    let (request_id, autonomous, run_id) = app
        .bridge_active_for_test()
        .expect("自主模式应自动启动运行");
    assert!(autonomous, "B 模式必须以自主模式执行");

    // 5. 驱动 GUI 帧直到运行结束并回传结果（真实网络）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        if app.bridge_active_for_test().is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    let result = BridgePaths::with_root(&root)
        .read_result(&request_id)
        .expect("GUI 必须回传结果");
    assert_eq!(result.run_id.as_deref(), Some(run_id.as_str()));
    assert!(result.autonomous);
    assert_eq!(
        result.origin.as_deref(),
        Some(parallaxe_core::contracts::ORIGIN_AGENT_AUTONOMOUS)
    );
    assert!(
        result.network_request_count > 0,
        "B 模式缺少真实发包证据：{result:?}"
    );
    let report = result.report.expect("回传报告");
    assert!(report.results.iter().any(|r| r.target == target_id));

    // 6. 回传报告必须脱敏：不写原始对端地址 / 原始解析地址。
    let raw = serde_json::to_string(&report).unwrap();
    assert!(
        !raw.contains("\"peer_address\":\""),
        "回传报告不得包含原始对端地址"
    );
    let leaked_ip = raw
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .any(|part| {
            let octets: Vec<&str> = part.split('.').collect();
            octets.len() == 4
                && octets
                    .iter()
                    .all(|o| !o.is_empty() && o.len() <= 3 && o.parse::<u8>().is_ok())
        });
    assert!(!leaked_ip, "回传报告不得包含原始 IP 地址");

    // 7. 状态投影显示自主检测已开启（agent 侧可见）。
    let status = BridgePaths::with_root(&root)
        .read_status()
        .expect("状态投影");
    assert!(status.autonomous_enabled);
    assert!(status.imported_targets.contains(&target_id));

    // 8. 路径 A 回归：关闭自主检测后，请求必须等待用户点按，点按后同样真实发包。
    app.set_ai_autonomous_enabled(false);
    let scope_a = AiScope::for_targets([target_id.clone()], "GUI 路径 A 验收");
    let request_a = BridgeRequest::new(scope_a, 3_500, 5).expect("合法请求 A");
    BridgePaths::with_root(&root)
        .write_request(&request_a)
        .expect("写请求 A");
    // 宿主轮询有 500ms 节流；等待一个节流窗口后再取请求。
    std::thread::sleep(std::time::Duration::from_millis(650));
    app.poll_ai_bridge_for_test();
    assert!(app.bridge_active_for_test().is_none(), "A 模式不得自动执行");
    assert!(
        app.approve_pending_bridge_for_test(),
        "A 模式应出现一次性授权确认"
    );
    let (request_id_a, autonomous_a, _) =
        app.bridge_active_for_test().expect("用户确认后应启动运行");
    assert!(!autonomous_a, "路径 A 不是自主模式");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        if app.bridge_active_for_test().is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    let result_a = BridgePaths::with_root(&root)
        .read_result(&request_id_a)
        .expect("A 模式必须回传结果");
    assert!(result_a.network_request_count > 0, "A 模式缺少真实发包证据");
    assert_eq!(
        result_a.origin.as_deref(),
        Some(parallaxe_core::contracts::ORIGIN_USER_IMPORTED)
    );

    let _ = std::fs::remove_dir_all(&root);
}
