//! 阶段 3 无头三端一致性回放测试与架构铁律验证。
//!
//! 覆盖：
//! - 6 类必检场景：Drive 302、同层冲突+BLOCKED 混合、验证码挑战、运行中途取消、Books/共享 CDN Guard 预览、IPv6 三态；
//! - 全部 9 项内置离线 fixture 与 `google-all` 零准入（107/0/3）逐字段一致性；
//! - 9 个 `State` 穷举徽章与仅 `OK` 为绿的红线；
//! - 筛选/minimal/双主题不改变全局冲突与未知计数；
//! - `(run_id, seq)` 去重、缺口感知与终结后拒收迟到事件；
//! - `crates/gui` 依赖防火墙与账号隔离常量核查。

use parallaxe_core::contracts::{
    Event, EventName, Layer, Mode, Report, State, Termination, ACCOUNT_MODE,
    BROWSER_PROFILE_SOURCES, CREDENTIAL_SOURCES, SESSION_MODE,
};
use parallaxe_core::fixtures;
use parallaxe_core::i18n::Locale;
use parallaxe_core::reports;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::Registry;
use parallaxe_gui::app::ParallaxeGuiApp;
use parallaxe_gui::commands::{
    build_offline_scenario_events, render_export, ExportFormat, OfflineScenario,
};
use parallaxe_gui::layout::{LayoutSpec, OverviewVariant, ViewportTier};
use parallaxe_gui::state::{AppState, TerminalKind};
use parallaxe_gui::theme::{layer_label, state_badge, DensityMode, Palette, ThemeMode};
use serde_json::Value;

/// 逐字段断言 GUI `AppState` 与 `Report` JSON（CLI `--json` 同源）及 JSONL/agent 线格式完全一致。
fn assert_gui_matches_report_and_wire(events: &[Event], expected_report: &Report) {
    // 1. 模拟 CLI --jsonl 线格式往返
    let jsonl = reports::to_jsonl(events);
    let parsed_jsonl_events: Vec<Event> = jsonl
        .lines()
        .map(|line| serde_json::from_str::<Event>(line).expect("每行应为合法 Event JSON"))
        .collect();
    assert_eq!(parsed_jsonl_events.len(), events.len());

    // 2. 模拟 agent --stdio 异步 record 信封往返（去除外层命令 id）
    let agent_events: Vec<Event> = events
        .iter()
        .map(|ev| {
            let wire = serde_json::json!({"id": "req-parity", "record": ev});
            serde_json::from_value::<Event>(wire["record"].clone()).expect("agent record 合法")
        })
        .collect();

    // 3. 分别喂给 GUI AppState 状态机
    let gui_from_direct = AppState::from_events(events);
    let gui_from_jsonl = AppState::from_events(&parsed_jsonl_events);
    let gui_from_agent = AppState::from_events(&agent_events);

    let cli_json: Value = serde_json::from_str(&reports::to_json(expected_report)).unwrap();

    for gui in [&gui_from_direct, &gui_from_jsonl, &gui_from_agent] {
        let run = gui.active_run().expect("应存在激活运行");
        assert!(!run.running, "终结后 running 必须为 false");
        assert!(run.terminal.is_some(), "终结标记不能为空");
        assert!(!run.seq_gap_detected, "合法事件流不得出现序号缺口");

        let gui_report = run.report.as_ref().expect("GUI 应持有终态报告");
        let gui_json: Value = serde_json::from_str(&reports::to_json(gui_report)).unwrap();
        assert_eq!(
            gui_json, cli_json,
            "GUI 投影终态 JSON 必须与 CLI --json 逐字段一致"
        );

        // 逐目标核对 ViewModel 字段
        let ordered = run.ordered_results();
        assert_eq!(ordered.len(), expected_report.results.len());
        for (gui_res, core_res) in ordered.iter().zip(expected_report.results.iter()) {
            assert_eq!(gui_res.target, core_res.target);
            assert_eq!(gui_res.state, core_res.state);
            assert_eq!(gui_res.reason, core_res.reason);
            assert_eq!(gui_res.confidence, core_res.confidence);
            assert_eq!(gui_res.source, core_res.source);
            assert_eq!(gui_res.duration_ms, core_res.duration_ms);
            assert_eq!(gui_res.request_count, core_res.request_count);
            assert_eq!(gui_res.region_layers.len(), 7);
            assert_eq!(
                serde_json::to_value(&gui_res.region_layers).unwrap(),
                serde_json::to_value(&core_res.region_layers).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&gui_res.evidence).unwrap(),
                serde_json::to_value(&core_res.evidence).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&gui_res.conflicts).unwrap(),
                serde_json::to_value(&core_res.conflicts).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&gui_res.transport).unwrap(),
                serde_json::to_value(&core_res.transport).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&gui_res.service_observed).unwrap(),
                serde_json::to_value(&core_res.service_observed).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&gui_res.risk_signal).unwrap(),
                serde_json::to_value(&core_res.risk_signal).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&gui_res.local_policy).unwrap(),
                serde_json::to_value(&core_res.local_policy).unwrap()
            );
        }

        // 覆盖两栏逐字段一致
        let gui_cov = run.coverage.as_ref().expect("应存在覆盖快照");
        assert_eq!(
            serde_json::to_value(gui_cov).unwrap(),
            serde_json::to_value(&expected_report.coverage).unwrap()
        );
        assert_eq!(run.exit_code(), Some(expected_report.exit_code));
        assert_eq!(run.termination(), Some(expected_report.termination));
        assert_eq!(
            run.request_counts(),
            (
                expected_report.network_request_count,
                expected_report.simulated_request_count
            )
        );
        assert_eq!(
            serde_json::to_value(&run.plans).unwrap(),
            serde_json::to_value(&expected_report.plans).unwrap()
        );

        // 导出同源校验（JSON / JSONL / HTML / CSV）
        assert_eq!(
            render_export(gui_report, &run.raw_events, ExportFormat::Json),
            reports::to_json(expected_report)
        );
        assert_eq!(
            render_export(gui_report, &run.raw_events, ExportFormat::Jsonl),
            reports::to_jsonl(events)
        );
        assert_eq!(
            render_export(gui_report, &run.raw_events, ExportFormat::Html),
            reports::to_html(expected_report)
        );
        assert_eq!(
            render_export(gui_report, &run.raw_events, ExportFormat::Csv),
            reports::to_csv(expected_report)
        );
        assert_eq!(
            render_export(gui_report, &run.raw_events, ExportFormat::Markdown),
            reports::to_markdown(expected_report, Locale::ZhCn)
        );
        assert_eq!(
            render_export(gui_report, &run.raw_events, ExportFormat::Text),
            reports::to_text(expected_report, Locale::ZhCn)
        );
    }
}

fn terminal_report(events: &[Event]) -> Report {
    let last = events.last().expect("至少一条事件");
    assert!(
        matches!(last.event, EventName::RunFinished | EventName::RunCancelled),
        "末尾事件必须为终结事件"
    );
    serde_json::from_value::<Report>(last.payload["report"].clone()).expect("解析终态报告")
}

#[tokio::test]
async fn scenario_1_drive_302_login_handoff_parity() {
    let registry = Registry::embedded().unwrap();
    let events =
        build_offline_scenario_events(&registry, OfflineScenario::DriveLoginHandoff, "run-s1")
            .await
            .unwrap();
    let report = terminal_report(&events);
    assert_gui_matches_report_and_wire(&events, &report);

    let gui = AppState::from_events(&events);
    let run = gui.active_run().unwrap();
    let drive = run.result_for("google.drive").unwrap();
    assert_eq!(drive.state, State::Unknown, "Drive 302 绝不能显示成 OK");
    assert_eq!(drive.reason.as_deref(), Some("REDIRECT_TO_LOGIN"));
    assert_eq!(drive.service_observed.observed_country, None);
    assert_eq!(run.exit_code(), Some(3));
    assert_eq!(run.request_counts(), (0, 1));
    assert!(run.coverage.as_ref().unwrap().covered[0].http_received);
}

#[tokio::test]
async fn scenario_2_blocked_and_conflict_mix_parity_and_filter_invariance() {
    let registry = Registry::embedded().unwrap();
    let events =
        build_offline_scenario_events(&registry, OfflineScenario::BlockedAndConflictMix, "run-s2")
            .await
            .unwrap();
    let report = terminal_report(&events);
    assert_gui_matches_report_and_wire(&events, &report);

    let mut gui = AppState::from_events(&events);
    let run = gui.active_run().unwrap();
    assert_eq!(
        run.exit_code(),
        Some(2),
        "BLOCKED(2) + CONFLICT(3) 必须优先取 2"
    );
    assert_eq!(
        run.result_for("google.gemini").unwrap().state,
        State::Conflict
    );
    assert_eq!(
        run.result_for("google.books").unwrap().state,
        State::Blocked
    );
    assert!(run.result_for("google.gemini").unwrap().agreement[0].tie);
    assert_eq!(
        run.result_for("google.gemini").unwrap().agreement[0].winner,
        None
    );

    // 启用筛选后：列表只剩 1 行，但顶部全局冲突计数与明确异常计数绝不丢失
    gui.filter_state = Some(State::Blocked);
    let run = gui.active_run().unwrap();
    assert_eq!(gui.filtered_results(run).len(), 1);
    assert_eq!(run.total_conflict_targets(), 1, "筛选不得隐藏全局冲突计数");
    assert_eq!(run.state_counts().definite_anomalies(), 1);
    assert_eq!(run.state_counts().conflict, 1);

    // 切换主题/密度不会改动任何状态机字段
    let _ = (ThemeMode::Dark.toggle(), DensityMode::Normal.toggle());
    assert_eq!(gui.active_run().unwrap().total_conflict_targets(), 1);
}

#[tokio::test]
async fn scenario_3_captcha_challenge_parity() {
    let registry = Registry::embedded().unwrap();
    let events =
        build_offline_scenario_events(&registry, OfflineScenario::CaptchaChallenge, "run-s3")
            .await
            .unwrap();
    let report = terminal_report(&events);
    assert_gui_matches_report_and_wire(&events, &report);

    let gui = AppState::from_events(&events);
    let run = gui.active_run().unwrap();
    let home = run.result_for("google.home").unwrap();
    assert_eq!(home.state, State::Unknown);
    assert_eq!(home.reason.as_deref(), Some("CAPTCHA_CHALLENGE"));
    assert_eq!(home.service_observed.observed_country, None);
    assert!(home
        .risk_signal
        .markers
        .iter()
        .any(|m| m.contains("CAPTCHA")));
    assert_eq!(run.exit_code(), Some(3));
}

#[tokio::test]
async fn scenario_4_mid_run_cancelled_and_duplicate_terminal_ignored() {
    let registry = Registry::embedded().unwrap();
    let events =
        build_offline_scenario_events(&registry, OfflineScenario::MidRunCancelled, "run-s4")
            .await
            .unwrap();
    let report = terminal_report(&events);
    assert_gui_matches_report_and_wire(&events, &report);

    let mut gui = AppState::from_events(&events);
    let run = gui.active_run().unwrap();
    assert_eq!(run.terminal, Some(TerminalKind::Cancelled));
    assert_eq!(run.termination(), Some(Termination::Cancelled));
    assert_eq!(run.exit_code(), Some(3));
    assert_eq!(
        run.request_counts(),
        (0, 1),
        "fixture 模式真实网络请求必须为 0"
    );
    assert!(run
        .coverage
        .as_ref()
        .unwrap()
        .uncovered
        .iter()
        .any(|u| u.target == "google.books" && u.selected && u.reason == "CANCELLED"));

    // 注入重复终结事件与迟到事件：不得重复计数、不得篡改终态
    let mut dup_terminal = events.last().unwrap().clone();
    gui.apply(&dup_terminal);
    dup_terminal.seq += 1;
    gui.apply(&dup_terminal);
    let run = gui.active_run().unwrap();
    assert_eq!(run.ignored_duplicate_events, 1);
    assert_eq!(run.ignored_late_events, 1);
    assert_eq!(run.ordered_results().len(), 1);
}

#[tokio::test]
async fn scenario_5_books_shared_cdn_guard_preview_parity() {
    let registry = Registry::embedded().unwrap();
    let events =
        build_offline_scenario_events(&registry, OfflineScenario::BooksAndSharedCdnGuard, "run-s5")
            .await
            .unwrap();
    let report = terminal_report(&events);
    assert_gui_matches_report_and_wire(&events, &report);

    let gui = AppState::from_events(&events);
    let run = gui.active_run().unwrap();
    assert_eq!(run.plans.len(), 1);
    let plan = &run.plans[0];
    assert!(!plan.safe_to_apply);
    assert!(!plan.applied);
    assert_eq!(plan.effect_scope, "LOCAL_TRAFFIC_ONLY");
    assert_eq!(plan.remote_account_impact, "NOT_PROVABLE");
    assert_eq!(plan.shared_egress_guarantee, "UNPROVEN");
    assert_eq!(plan.primary_reason.as_deref(), Some("NO_SAFE_BLOCK"));
    assert!(plan
        .reasons
        .iter()
        .any(|r| r == "BLOCK_DEFERRED_DEPENDENCY_CONFLICT"));
    assert!(plan.impact_closure.len() >= 3);
    // Restricted(2) 优先于 Guard 拒绝(3)
    assert_eq!(run.exit_code(), Some(2));
}

#[tokio::test]
async fn scenario_6_ipv6_three_states_parity() {
    let registry = Registry::embedded().unwrap();
    let events =
        build_offline_scenario_events(&registry, OfflineScenario::Ipv6ThreeStates, "run-s6")
            .await
            .unwrap();
    let report = terminal_report(&events);
    assert_gui_matches_report_and_wire(&events, &report);

    let gui = AppState::from_events(&events);
    let run = gui.active_run().unwrap();
    let slow = run.result_for("google.gmail").unwrap();
    let broken = run.result_for("google.drive").unwrap();
    let v6only = run.result_for("google.http204").unwrap();

    assert_eq!(slow.transport.path_diagnostic.as_deref(), Some("IPv6_SLOW"));
    assert_eq!(
        broken.transport.path_diagnostic.as_deref(),
        Some("IPv6_BROKEN")
    );
    assert_eq!(
        v6only.transport.path_diagnostic.as_deref(),
        Some("IPV6_ONLY_EGRESS")
    );
    assert_ne!(
        v6only.state,
        State::LocalPathError,
        "仅 IPv6 出口可用时不得判为节点故障"
    );
}

#[tokio::test]
async fn all_nine_builtin_fixtures_and_empty_google_all_match_gui_state() {
    let registry = Registry::embedded().unwrap();

    // 全部 9 项内置 fixture
    for case in fixtures::cases() {
        let options = CheckOptions {
            fixture: Some(case.id.to_string()),
            run_id: Some(format!("run-fx-{}", case.id)),
            ..Default::default()
        };
        let output = runtime::run_check(&registry, &options, None, false, None)
            .await
            .unwrap();
        assert_eq!(output.report.mode, Mode::Fixture);
        assert_gui_matches_report_and_wire(&output.events, &output.report);
    }

    // google-all 默认空许可：选中 107 / 发包 0 / 退出码 3
    let options = CheckOptions {
        profile: Some("google-all".to_string()),
        run_id: Some("run-google-all-empty".to_string()),
        ..Default::default()
    };
    let output = runtime::run_check(&registry, &options, None, true, None)
        .await
        .unwrap();
    assert_eq!(output.report.coverage.selected_count, 107);
    assert_eq!(output.report.network_request_count, 0);
    assert_eq!(output.report.exit_code, 3);
    assert_gui_matches_report_and_wire(&output.events, &output.report);

    let gui = AppState::from_events(&output.events);
    let run = gui.active_run().unwrap();
    assert!(
        run.is_zero_admission_online(),
        "零准入在线运行必须标明未发请求"
    );
}

#[test]
fn nine_state_badges_are_exhaustive_and_only_ok_is_green() {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        for state in State::ALL {
            let badge = state_badge(state, theme);
            assert_eq!(badge.state, state);
            assert_eq!(badge.code, state.as_str());
            assert!(!badge.symbol.is_empty(), "每个状态都必须带文字/符号编码");
            assert!(!badge.zh_label.is_empty());
            assert!(badge.label_key.starts_with("state."));
            assert!(!badge.label(Locale::ZhCn).is_empty());
            assert!(!badge.label(Locale::EnUs).is_empty());
            if state == State::Ok {
                assert!(badge.is_green, "OK 应为绿色");
            } else {
                assert!(!badge.is_green, "状态 {:?} 绝不能画成绿色", state);
            }
        }
    }
    for layer in Layer::ORDER {
        let (zh, code) = layer_label(layer);
        assert!(!zh.is_empty() && !code.is_empty());
    }
    assert_eq!(ViewportTier::from_width(1320.0), ViewportTier::Wide);
    assert_eq!(ViewportTier::from_width(960.0), ViewportTier::Compact);
    assert_eq!(ViewportTier::from_width(420.0), ViewportTier::Narrow);
}

#[test]
fn gui_crate_respects_dependency_firewall_and_account_isolation() {
    assert_eq!(ACCOUNT_MODE, "PROHIBITED");
    assert_eq!(SESSION_MODE, "ANONYMOUS_ONLY");
    assert!(CREDENTIAL_SOURCES.is_empty());
    assert!(BROWSER_PROFILE_SOURCES.is_empty());

    let cargo_toml = include_str!("../Cargo.toml");
    let deps_section = cargo_toml
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(cargo_toml);
    for forbidden in ["reqwest", "hickory-resolver", "regex"] {
        assert!(
            !deps_section.contains(forbidden),
            "crates/gui/Cargo.toml 正常依赖禁止包含 {forbidden}"
        );
    }
}

#[tokio::test]
async fn orthogonal_three_dimensions_layout_and_headless_render_all_eight_combinations() {
    // 1. 验证三维度正交结构规范（normal vs minimal 视图结构不同；dark vs light 仅换色板不改结构；narrow 切单列+底栏）
    let wide_normal = LayoutSpec::compute(DensityMode::Normal, 1280.0);
    let wide_minimal = LayoutSpec::compute(DensityMode::Minimal, 1280.0);
    let narrow_normal = LayoutSpec::compute(DensityMode::Normal, 460.0);
    let narrow_minimal = LayoutSpec::compute(DensityMode::Minimal, 460.0);

    assert_eq!(
        wide_normal.overview_variant,
        OverviewVariant::DesktopNormalDashboard
    );
    assert_eq!(
        wide_minimal.overview_variant,
        OverviewVariant::DesktopMinimalThreeCards
    );
    assert_ne!(wide_normal.overview_variant, wide_minimal.overview_variant);
    assert!(wide_normal.show_desktop_tabs);
    assert!(!wide_minimal.show_desktop_tabs);
    assert_eq!(wide_normal.dual_stack_columns, 2);
    assert_eq!(wide_minimal.minimal_card_columns, 3);
    assert!(!wide_normal.show_bottom_nav);

    assert_eq!(
        narrow_normal.overview_variant,
        OverviewVariant::MobileNormalStacked
    );
    assert_eq!(
        narrow_minimal.overview_variant,
        OverviewVariant::MobileMinimalStacked
    );
    assert!(narrow_normal.show_bottom_nav);
    assert!(narrow_minimal.show_bottom_nav);
    assert_eq!(narrow_normal.dual_stack_columns, 1);
    assert_eq!(narrow_minimal.minimal_card_columns, 1);
    assert!(!narrow_normal.show_desktop_tabs);

    // Dark vs Light 仅色板不同，结构规格完全一致
    assert_ne!(
        Palette::for_theme(ThemeMode::Dark),
        Palette::for_theme(ThemeMode::Light)
    );

    // 2. 用真实冲突+BLOCKED 场景在无头 egui::Context 上渲染全部 8 种正交组合，断言零 panic 且判定数据不变
    let registry = Registry::embedded().unwrap();
    let events = build_offline_scenario_events(
        &registry,
        OfflineScenario::BlockedAndConflictMix,
        "run-8combo",
    )
    .await
    .unwrap();
    let expected_report = terminal_report(&events);

    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(registry, ctx.clone());
    for ev in &events {
        app.apply_event(ev);
    }

    let combos = [
        (DensityMode::Normal, ThemeMode::Dark, 1280.0, 820.0),
        (DensityMode::Normal, ThemeMode::Light, 1280.0, 820.0),
        (DensityMode::Minimal, ThemeMode::Dark, 1280.0, 820.0),
        (DensityMode::Minimal, ThemeMode::Light, 1280.0, 820.0),
        (DensityMode::Normal, ThemeMode::Dark, 460.0, 820.0),
        (DensityMode::Normal, ThemeMode::Light, 460.0, 820.0),
        (DensityMode::Minimal, ThemeMode::Dark, 460.0, 820.0),
        (DensityMode::Minimal, ThemeMode::Light, 460.0, 820.0),
    ];

    for (density, theme, w, h) in combos {
        for locale in parallaxe_core::i18n::Locale::ALL {
            app.set_view_modes(density, theme);
            app.set_locale(locale);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w, h),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |c| {
                app.render_ui(c);
            });
            assert!(
                !out.shapes.is_empty(),
                "组合 {:?}/{:?}/{:?}/{}x{} 应产出有效绘制形状",
                locale,
                density,
                theme,
                w,
                h
            );

            // 切换任何视图维度（含语言/主题/密度/视口）都不得改变底层判定、冲突计数、覆盖或退出码
            let run = app.app_state().active_run().unwrap();
            assert_eq!(run.exit_code(), Some(expected_report.exit_code));
            assert_eq!(run.total_conflict_targets(), 1);
            assert_eq!(run.state_counts().definite_anomalies(), 1);
            assert_eq!(run.ordered_results().len(), expected_report.results.len());
        }
    }
}
