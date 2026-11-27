//! `eframe::App` 主窗口：三维度正交重构（单主视图 + Tab + 按需动画抽屉 + 窄屏单列底栏）。
//!
//! 三个正交维度：
//! - `DensityMode`：`Normal`（仪表盘+Tab） vs `Minimal`（三卡极简）
//! - `ThemeMode`：`Dark` vs `Light`（纯色板切换，绝不改变 `LayoutSpec`）
//! - `ViewportTier`：`Wide`/`Compact` 桌面 vs `Narrow`（`<840px` 单列堆叠 + 底部 4 栏导航）

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use egui::{RichText, Ui};
use parallaxe_core::contracts::{
    Event, EventName, ACCOUNT_MODE, ORIGIN_AGENT_AUTONOMOUS, SESSION_MODE,
};
use parallaxe_core::fixtures;
use parallaxe_core::guard_export::RuleFormat;
use parallaxe_core::i18n::{translate, Locale};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use parallaxe_core::plugins::HookEvent;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::settings::Settings;
use parallaxe_core::targets::{Registry, HARD_LIMITS};

use crate::commands::{
    render_export_locale, write_export_file, CommandBus, ExportFormat, GuiCommand, OfflineScenario,
    StaticTargetExplain, WorkerMessage,
};
use crate::icons::IconKind;
use crate::layout::{ActiveTab, LayoutSpec, MobileTab, ViewportTier};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::plugin_host::PluginHost;
use crate::schedule_ui::SchedulePrefs;
use crate::settings_store;
use crate::state::AppState;
use crate::theme::{
    apply_theme, btn_filled, btn_outlined, btn_tonal, card_frame, install_cjk_fonts, kv_row,
    render_admonition, render_modal_scrim, render_pill, render_running_pulse_bar, spaced_columns,
    subtle_well_frame, DensityMode, Palette, TableDensity, ThemeMode, FONT_BODY, FONT_DISPLAY,
    FONT_META, FONT_TITLE, RADIUS_LG, RADIUS_MD, SPACE_1, SPACE_2, SPACE_3, SPACE_4, SPACE_5,
    SPACE_6,
};
use crate::views::import_panel::{self, ImportViewState};
use crate::views::plan::PlanDraft;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::views::settings_plugins;
use crate::views::{
    ai_debug_panel, check_panel, coverage_panel, default_bundle_panel, distribution_mode_panel,
    explain_drawer, guard_panel, overview_minimal, overview_normal, service_matrix, settings_page,
    target_tree, timeline_view, top_bar, UiAction,
};

/// 启动原生 `eframe/egui` 窗口（仅绑定本机图形会话，不监听任何网络端口）。
pub fn run_native(mut registry: Registry) -> Result<(), String> {
    let settings_path = settings_store::default_desktop_settings_path();
    let (settings, _) = settings_store::load_or_default(settings_path.as_deref());
    registry.set_distribution_mode(registry.distribution_mode() || settings.distribution_mode);
    // 发行包"永久生效"授权：仅在有发行包模式时从用户状态目录恢复（绝不写回 assets/）。
    if registry.distribution_mode() {
        if let Some(state_dir) = parallaxe_core::state::default_user_state_dir() {
            let _ = registry.restore_default_bundle_state(&state_dir);
        }
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Parallaxe box · Google 全服务出口观测与链路诊断（只读）")
        .with_decorations(false)
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([380.0, 620.0]);
    if let Some(icon) = crate::app_icon::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Parallaxe box",
        options,
        Box::new(move |cc| {
            install_cjk_fonts(&cc.egui_ctx);
            let app = ParallaxeGuiApp::new_with_settings_path(
                registry,
                cc.egui_ctx.clone(),
                settings_path,
            );
            apply_theme(&cc.egui_ctx, app.theme);
            Ok(Box::new(app))
        }),
    )
    .map_err(|err| format!("无法初始化图形窗口：{err}"))
}

/// Android 原生启动：把 `AndroidApp` 通过 winit 的 event-loop builder 传入。
#[cfg(target_os = "android")]
pub fn run_native_android(
    mut registry: Registry,
    app: android_activity::AndroidApp,
) -> Result<(), String> {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    let settings_path = app
        .internal_data_path()
        .map(|dir| dir.join("settings.jsonl"));
    let (settings, _) = settings_store::load_or_default(settings_path.as_deref());
    registry.set_distribution_mode(registry.distribution_mode() || settings.distribution_mode);
    if registry.distribution_mode() {
        if let Some(state_dir) = settings_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|dir| dir.join("user-state"))
        {
            let _ = registry.restore_default_bundle_state(&state_dir);
        }
    }
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Parallaxe box（只读）")
            .with_inner_size([420.0, 860.0])
            .with_min_inner_size([320.0, 560.0]),
        ..Default::default()
    };
    options.event_loop_builder = Some(Box::new(move |builder| {
        builder.with_android_app(app);
    }));
    eframe::run_native(
        "Parallaxe box",
        options,
        Box::new(move |cc| {
            install_cjk_fonts(&cc.egui_ctx);
            let gui_app = ParallaxeGuiApp::new_with_settings_path(
                registry,
                cc.egui_ctx.clone(),
                settings_path,
            );
            apply_theme(&cc.egui_ctx, gui_app.theme);
            Ok(Box::new(gui_app))
        }),
    )
    .map_err(|err| format!("无法初始化图形窗口：{err}"))
}

/// Parallaxe box GUI 应用容器。
pub struct ParallaxeGuiApp {
    registry: Registry,
    bus: CommandBus,
    state: AppState,
    theme: ThemeMode,
    applied_theme: Option<ThemeMode>,
    density: DensityMode,
    active_tab: ActiveTab,
    draft: Option<PlanDraft>,
    show_ip: bool,
    address_rows: String,
    expected_peer_country: String,
    address_feedback: Option<String>,
    /// 会话级 AI 自主调试状态（严格仅存内存、绝不写入持久化配置文件）。
    ai_debug: ai_debug_panel::AiDebugState,
    pending_autonomous_run_id: Option<String>,
    /// 任务 A：GUI 侧 AI 授权桥（文件级、无监听端口；桌面端可用）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    ai_bridge: crate::ai_bridge_host::AiBridgeHost,
    /// 已弹窗、等待用户一键确认的桥请求。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    bridge_pending: Option<parallaxe_core::ai_bridge::BridgeRequest>,
    /// 正在执行、等待回传结果的桥请求（含是否自主模式）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    bridge_active: Option<(parallaxe_core::ai_bridge::BridgeRequest, bool, String)>,
    pending: bool,
    catalog_drawer_open: bool,
    explain_drawer_open: bool,
    about_modal_open: bool,
    /// 源码构建「切换到发行包模式」强制确认弹窗。
    distribution_mode_confirm_open: bool,
    ipv4_only: bool,
    fast_mode: bool,
    /// 最大检测时长（毫秒）：0 = 不限时；非零值覆盖 profile 上限（仍受硬限守护）。
    max_duration_ms: u64,
    proxy_mode: parallaxe_core::ProxyMode,
    proxy_input: String,
    pac_input: String,
    only_selected_uncovered: bool,
    checked_targets: BTreeSet<String>,
    tree_filter: String,
    static_explain: Option<StaticTargetExplain>,
    selftest_banner: Option<String>,
    export_path_input: String,
    export_preview: Option<(ExportFormat, String)>,
    export_status: Option<String>,
    import_view: ImportViewState,
    default_bundle: default_bundle_panel::DefaultBundleState,
    guard_view: guard_panel::GuardViewState,
    resolved_link_report: Option<parallaxe_core::linkcheck::LinkReport>,
    table_density: TableDensity,
    /// 结果页按用户分组筛选（`None` = 全部）。
    matrix_group_filter: Option<String>,
    schedule_prefs: SchedulePrefs,
    selected_profile: String,
    pace_panel_open: bool,
    group_search: String,
    show_all_groups: bool,
    show_technical_details: bool,
    preview_profile: String,
    default_profile: String,
    settings_path: Option<PathBuf>,
    /// 默认规则包永久授权的用户状态目录；Android 使用 App 内部数据目录，无需存储权限。
    default_bundle_state_dir: Option<PathBuf>,
    settings: Settings,
    settings_error: Option<String>,
    settings_notice: Option<String>,
    /// 设置页当前层级（一级列表 / 二级独立页）。
    settings_page: settings_page::SettingsPage,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    plugin_host: PluginHost,
}

impl ParallaxeGuiApp {
    pub fn new(registry: Registry, ctx: egui::Context) -> Self {
        Self::new_with_settings_path(registry, ctx, None)
    }

    /// Creates the GUI app with an explicit settings file path for persistent preferences.
    pub fn new_with_settings_path(
        registry: Registry,
        ctx: egui::Context,
        settings_path: Option<PathBuf>,
    ) -> Self {
        let (mut settings, settings_error) =
            settings_store::load_or_default(settings_path.as_deref());
        let mut registry = registry;
        registry.set_distribution_mode(registry.distribution_mode() || settings.distribution_mode);
        settings.distribution_mode = registry.distribution_mode();
        #[cfg(target_os = "android")]
        let default_bundle_state_dir = settings_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|dir| dir.join("user-state"));
        #[cfg(not(target_os = "android"))]
        let default_bundle_state_dir = parallaxe_core::state::default_user_state_dir();
        let wake_ctx = ctx.clone();
        let bus = CommandBus::spawn(
            registry.clone(),
            Arc::new(move || {
                wake_ctx.request_repaint();
            }),
        );
        let theme = if settings.theme == "light" {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        let density = if settings.density == "compact" {
            DensityMode::Minimal
        } else {
            DensityMode::Normal
        };
        let mut state = AppState::new();
        if let Some(loc) = Locale::parse(&settings.language) {
            state.set_locale(loc);
        }
        let mut guard_view = guard_panel::GuardViewState::default();
        guard_view.export_format = match settings.rule_format.as_str() {
            "singbox" => RuleFormat::SingBox,
            "domainlist" => RuleFormat::DomainList,
            _ => RuleFormat::Clash,
        };
        let mut schedule_prefs = SchedulePrefs::default();
        let default_settings = Settings::default();
        if settings.min_interval_ms != default_settings.min_interval_ms {
            schedule_prefs.manual_interval_ms = settings
                .min_interval_ms
                .min(HARD_LIMITS.max_min_interval_ms);
            schedule_prefs.custom_interval_ms = Some(schedule_prefs.manual_interval_ms);
            schedule_prefs.lock_interval = true;
        }
        if settings.batch_size != default_settings.batch_size {
            schedule_prefs.custom_batch_size =
                Some(settings.batch_size.clamp(1, HARD_LIMITS.max_batch_size));
        }
        schedule_prefs.inter_target_interval_ms = settings
            .inter_target_interval_ms
            .min(HARD_LIMITS.max_inter_target_interval_ms);
        let default_profile = if check_panel::ALL_PROFILE_IDS.contains(&settings.profile.as_str()) {
            settings.profile.clone()
        } else {
            "simple".to_string()
        };
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let mut plugin_host = PluginHost::default();
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            plugin_host.sync_from_settings(&settings);
            plugin_host.invoke_enabled(&settings, "default-profile", None, None);
        }
        ParallaxeGuiApp {
            registry,
            bus,
            state,
            theme,
            applied_theme: None,
            density,
            active_tab: ActiveTab::Overview,
            draft: None,
            show_ip: false,
            address_rows: String::new(),
            expected_peer_country: String::new(),
            address_feedback: None,
            ai_debug: ai_debug_panel::AiDebugState::default(),
            pending_autonomous_run_id: None,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            ai_bridge: crate::ai_bridge_host::AiBridgeHost::new(),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            bridge_pending: None,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            bridge_active: None,
            pending: false,
            catalog_drawer_open: false,
            explain_drawer_open: false,
            about_modal_open: false,
            distribution_mode_confirm_open: false,
            ipv4_only: false,
            fast_mode: false,
            // 默认不限时：simple 只受单请求超时与 QoS 约束，能跑完 16 项。
            max_duration_ms: 0,
            proxy_mode: parallaxe_core::ProxyMode::Auto,
            proxy_input: String::new(),
            pac_input: String::new(),
            only_selected_uncovered: true,
            checked_targets: BTreeSet::new(),
            tree_filter: String::new(),
            static_explain: None,
            selftest_banner: None,
            export_path_input: "parallaxe-report.json".to_string(),
            export_preview: None,
            export_status: None,
            import_view: ImportViewState::default(),
            default_bundle: default_bundle_panel::DefaultBundleState::default(),
            guard_view,
            resolved_link_report: None,
            table_density: TableDensity::Comfortable,
            matrix_group_filter: None,
            schedule_prefs,
            selected_profile: default_profile.clone(),
            pace_panel_open: false,
            group_search: String::new(),
            show_all_groups: false,
            show_technical_details: false,
            preview_profile: default_profile.clone(),
            default_profile,
            settings_path,
            default_bundle_state_dir,
            settings,
            settings_error,
            settings_notice: None,
            settings_page: settings_page::SettingsPage::Root,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            plugin_host,
        }
    }

    /// 供无头渲染测试注入状态与视图维度（仅测试/演示只读使用）。
    pub fn set_view_modes(&mut self, density: DensityMode, theme: ThemeMode) {
        self.density = density;
        self.theme = theme;
    }

    /// 供无头渲染测试切换界面语言（与主题/密度正交，仅改文字不改布局与判定）。
    pub fn set_locale(&mut self, locale: Locale) {
        self.state.set_locale(locale);
    }

    /// 把设置页的“最大检测时长 + 网络与代理”带入本次 CheckOptions。
    fn apply_network_settings(&self, options: &mut CheckOptions) {
        options.timeout_ms = Some(self.max_duration_ms);
        options.proxy_mode = self.proxy_mode;
        match self.proxy_mode {
            parallaxe_core::ProxyMode::Http => {
                let proxy = self.proxy_input.trim();
                if !proxy.is_empty() {
                    options.proxy = Some(proxy.to_string());
                }
            }
            parallaxe_core::ProxyMode::Pac => {
                let pac = self.pac_input.trim();
                if !pac.is_empty() {
                    options.pac_url = Some(pac.to_string());
                }
            }
            _ => {}
        }
    }

    /// 供无头渲染测试读取底层状态机。
    pub fn app_state(&self) -> &AppState {
        &self.state
    }

    /// 供测试与外部调用读取当前持久化设置快照。
    pub fn settings_snapshot(&self) -> &Settings {
        &self.settings
    }

    /// 当前是否处于发行包模式（正式发行构建恒为 true）。
    pub fn is_distribution_mode_enabled(&self) -> bool {
        self.registry.distribution_mode()
    }

    /// 测试/验收：打开源码构建的发行包模式强制确认框。
    pub fn open_distribution_mode_confirm_for_test(&mut self) {
        if !parallaxe_core::distribution::DISTRIBUTION_RELEASE_BUILD {
            self.distribution_mode_confirm_open = true;
        }
    }

    /// 测试/验收：发行包模式确认框是否打开。
    pub fn distribution_mode_confirm_open_for_test(&self) -> bool {
        self.distribution_mode_confirm_open
    }

    /// 测试/验收：明确确认后切换发行包模式（与点击确认按钮走同一内部路径）。
    pub fn confirm_distribution_mode_for_test(&mut self) {
        self.apply_distribution_mode(true);
    }

    /// 测试/验收：关闭发行包模式（正式发行构建中核心会保持恒开）。
    pub fn disable_distribution_mode_for_test(&mut self) {
        self.apply_distribution_mode(false);
    }

    fn apply_distribution_mode(&mut self, enabled: bool) {
        self.registry.set_distribution_mode(enabled);
        if !self.registry.distribution_mode() {
            self.registry.revoke_default_bundle_authorization();
        }
        self.settings.distribution_mode = self.registry.distribution_mode();
        self.distribution_mode_confirm_open = false;
        self.bus.send(GuiCommand::SyncRegistry {
            registry: Box::new(self.registry.clone()),
        });
        self.persist_current_settings();
        let locale = self.state.current_locale();
        let save_failed = self.settings_error.is_some();
        self.settings_notice = Some(if self.registry.distribution_mode() {
            if save_failed {
                if locale == parallaxe_core::Locale::EnUs {
                    "Distribution package mode is enabled for this session, but saving the setting failed."
                } else {
                    "发行包模式已在当前会话开启，但设置保存失败。"
                }
            } else if locale == parallaxe_core::Locale::EnUs {
                "Distribution package mode enabled. Built-in targets may send real requests; QoS remains enforced."
            } else {
                "已开启发行包模式：内置目标可真实发包，QoS 限制仍然有效。"
            }
            .to_string()
        } else if locale == parallaxe_core::Locale::EnUs {
            "Returned to zero-request mode.".to_string()
        } else {
            "已关闭发行包模式，恢复零发包基线。".to_string()
        });
    }

    /// 供测试与 agent 桥接查询当前会话级「AI 自主调试 → 自主检测」开关状态（默认关闭，不落盘）。
    pub fn is_ai_autonomous_enabled(&self) -> bool {
        self.ai_debug.autonomous_enabled
    }

    /// 用户手动切换会话级「AI 自主调试 → 自主检测」开关（绝不写入持久化设置文件）。
    pub fn set_ai_autonomous_enabled(&mut self, enabled: bool) {
        let locale = self.state.current_locale();
        self.ai_debug.set_autonomous_by_user(enabled, locale);
        if !enabled {
            self.pending_autonomous_run_id = None;
        }
    }

    /// AI 自主关闭「自主检测」开关，权限即时收回并给出明确状态提示。
    pub fn ai_self_disable_autonomous(&mut self, reason: Option<&str>) -> bool {
        let locale = self.state.current_locale();
        self.pending_autonomous_run_id = None;
        self.ai_debug.self_disable_by_ai(reason, locale)
    }

    /// 路径 A：由 AI 发起一次性检测请求，弹出 F3 人话确认框待用户一键授权。
    pub fn request_ai_f3_authorization(&mut self, req: ai_debug_panel::AiCheckRequest) {
        self.ai_debug.pending_f3_request = Some(req);
    }

    /// 供测试与 UI 验收只读检查当前 AI 自主调试状态。
    pub fn ai_debug_state(&self) -> &ai_debug_panel::AiDebugState {
        &self.ai_debug
    }

    /// 供 UI 与诊断读取桌面端 AI 授权桥状态（仅桌面端）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn ai_bridge_error(&self) -> Option<&str> {
        self.ai_bridge.last_error.as_deref()
    }

    /// 测试/验收：把文件桥重定向到隔离目录，避免影响真实用户的桥目录。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn use_bridge_root_for_test(&mut self, root: PathBuf) {
        self.ai_bridge = crate::ai_bridge_host::AiBridgeHost::with_root(root);
    }

    /// 测试/验收：立即轮询一次文件桥（真实 GUI 在每帧自动调用）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn poll_ai_bridge_for_test(&mut self) {
        self.poll_ai_bridge();
    }

    /// 测试/验收：读取当前活动桥请求（请求 ID、是否自主、运行 ID）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn bridge_active_for_test(&self) -> Option<(String, bool, String)> {
        self.bridge_active
            .as_ref()
            .map(|(request, autonomous, run_id)| {
                (request.request_id.clone(), *autonomous, run_id.clone())
            })
    }

    /// 测试/验收：把 UI 线程的 Registry（含刚导入的目标）同步给后台工作线程。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn sync_registry_for_test(&mut self) {
        self.bus.send(GuiCommand::SyncRegistry {
            registry: Box::new(self.registry.clone()),
        });
    }

    /// 测试/验收：启动一次用户导入目标的离线预演（零网络），用于 GUI 结果页与导出断言。
    pub fn run_user_offline_preview_for_test(&mut self, target_ids: Vec<String>) {
        self.start(GuiCommand::RunUserOfflinePreview { target_ids });
    }

    /// 测试/验收：打开默认规则包专属确认框（安全默认：全选、仅本次、未二次确认）。
    pub fn open_default_bundle_for_test(&mut self) {
        self.default_bundle.reset_for_open();
    }

    /// 测试/验收：专属确认框是否处于打开状态。
    pub fn default_bundle_modal_open_for_test(&self) -> bool {
        self.default_bundle.modal_open
    }

    /// 测试/验收：是否仍有检查在后台运行。
    pub fn has_pending_run_for_test(&self) -> bool {
        self.pending
    }

    /// 测试/验收：启动一次「我的授权版」真实检测（走 GUI 同一路径，需网络）。
    pub fn run_authorized_bundle_for_test(&mut self) -> Result<(), String> {
        let mut targets = self.registry.authorized_user_target_ids();
        if targets.is_empty() {
            if let Some(grant) = self.registry.active_session_grant_snapshot() {
                targets = grant.targets.keys().cloned().collect();
            }
        }
        if targets.is_empty() {
            return Err("没有有效的授权版范围".to_string());
        }
        let mut options = CheckOptions {
            targets,
            ipv4: self.ipv4_only,
            fast: self.fast_mode,
            run_id: Some(self.bus.next_run_id("run-gui-authorized")),
            confirm_budget: true,
            ..Default::default()
        };
        self.schedule_prefs.apply_to_options(&mut options);
        self.apply_network_settings(&mut options);
        self.start(GuiCommand::RunCheck { options });
        Ok(())
    }

    /// 测试/验收：走与界面导出按钮完全相同的渲染 + 写文件路径。
    pub fn export_active_report_for_test(
        &self,
        format: ExportFormat,
        path: &PathBuf,
    ) -> Result<(), String> {
        let run = self
            .state
            .active_run()
            .ok_or_else(|| "没有可导出的运行".to_string())?;
        let report = run
            .report
            .as_ref()
            .ok_or_else(|| "运行尚未结束".to_string())?;
        let mut copy = report.clone();
        for row in &mut copy.results {
            row.destination.set_show_ip(self.show_ip);
        }
        let text =
            render_export_locale(&copy, &run.raw_events, format, self.state.current_locale());
        write_export_file(path, &text)
    }

    /// 测试/截图：走与界面相同的 Markdown 导出预览路径并切换到运行历史。
    pub fn preview_active_markdown_export_for_test(&mut self) {
        self.dispatch_actions(
            &egui::Context::default(),
            vec![
                UiAction::SwitchTab(ActiveTab::Timeline),
                UiAction::ExportPreview(ExportFormat::Markdown),
            ],
        );
    }

    /// 测试/验收：路径 A 用户点按「确认授权」后执行待确认的桥请求。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn approve_pending_bridge_for_test(&mut self) -> bool {
        if self.bridge_pending.is_none() {
            return false;
        }
        let ctx = egui::Context::default();
        self.dispatch_actions(&ctx, vec![UiAction::ApproveAiF3Auth]);
        true
    }

    /// 桌面端：把会话级开关状态投影给 agent，并消费「AI 自主关闭」指令。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn poll_ai_bridge(&mut self) {
        let imported: Vec<String> = self
            .registry
            .entries()
            .iter()
            .filter(|entry| self.registry.is_user_imported(entry.id()))
            .map(|entry| entry.id().to_string())
            .collect();
        let autonomous = self.is_ai_autonomous_enabled();
        self.ai_bridge.publish_status(autonomous, &imported, false);
        for reason in self.ai_bridge.take_disable_requests(false) {
            self.ai_self_disable_autonomous(reason.as_deref());
        }
        // 单活动运行：已有运行或已有待确认弹窗时，新请求排队并在 TTL 后自然过期。
        if self.pending
            || self.bridge_pending.is_some()
            || self.ai_debug.pending_f3_request.is_some()
        {
            return;
        }
        let Some(request) = self.ai_bridge.take_first_request(false) else {
            return;
        };
        if autonomous {
            if let Err(error) = self.start_bridge_run(&request, true) {
                let _ = self.ai_bridge.fail(&request, error);
            }
        } else {
            let locale = self.state.current_locale();
            let label = crate::ai_bridge_host::request_scope_label(&request);
            let count = if !request.scope.targets.is_empty() {
                request.scope.targets.len()
            } else if let Some(profile) = request.scope.profile.as_deref() {
                check_panel::profile_target_count(&self.registry, profile)
            } else {
                0
            };
            let mut req = ai_debug_panel::AiCheckRequest::sample_for_profile(
                request.scope.profile.as_deref().unwrap_or("bridge-targets"),
                label,
                count,
                request.min_interval_ms,
                request.batch_size,
                locale,
            );
            req.requester = request.requester.clone();
            req.purpose = if request.scope.purpose.is_empty() {
                req.purpose
            } else {
                request.scope.purpose.clone()
            };
            req.bridge_request_id = Some(request.request_id.clone());
            req.targets = request.scope.targets.clone();
            req.autonomous = false;
            self.ai_debug.last_notice =
                Some(translate(locale, "gui.ai.bridge.pending_notice", &[]));
            self.ai_debug.pending_f3_request = Some(req);
            self.bridge_pending = Some(request);
        }
    }

    /// 桌面端：按桥请求的范围与 QoS 启动一次检查；目标必须是宿主已导入的用户网址。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn start_bridge_run(
        &mut self,
        request: &parallaxe_core::ai_bridge::BridgeRequest,
        autonomous: bool,
    ) -> Result<String, String> {
        if !request.scope.targets.is_empty() {
            let imported: Vec<String> = self
                .registry
                .entries()
                .iter()
                .filter(|entry| self.registry.is_user_imported(entry.id()))
                .map(|entry| entry.id().to_string())
                .collect();
            let distribution_mode = self.registry.distribution_mode();
            if !distribution_mode
                && !crate::ai_bridge_host::request_is_importable(request, &imported)
            {
                return Err("AI 请求的目标尚未在 GUI 导入；按零发包拒绝，未发出任何请求。".into());
            }
            if distribution_mode
                && request.scope.targets.iter().any(|id| {
                    !self.registry.is_user_imported(id)
                        && !self.registry.is_embedded_google_target(id)
                })
            {
                return Err("AI 请求包含未知或外部目标；拒绝注入会话许可，未发出任何请求。".into());
            }
            // GUI 是唯一发包者：路径 A 的用户点按、路径 B 的会话级自主开关都构成显式同意。
            // 源码模式只接受用户导入目标；发行包模式额外接受精确选择的内置目标。
            self.registry
                .authorize_session_once(&request.scope.targets, true)
                .map_err(|err| format!("无法注入一次性会话许可：{err}"))?;
        }
        let run_id = self.bus.next_run_id("run-ai-bridge");
        let mut options = CheckOptions {
            profile: request.scope.profile.clone(),
            targets: request.scope.targets.clone(),
            run_id: Some(run_id.clone()),
            ipv4: self.ipv4_only,
            fast: self.fast_mode,
            confirm_budget: true,
            ..Default::default()
        };
        self.schedule_prefs.apply_to_options(&mut options);
        self.apply_network_settings(&mut options);
        options.min_interval_ms = Some(
            options
                .min_interval_ms
                .unwrap_or(request.min_interval_ms)
                .max(parallaxe_core::ai_bridge::MIN_INTERVAL_MS),
        );
        options.batch_size = Some(
            options
                .batch_size
                .unwrap_or(request.batch_size)
                .clamp(1, parallaxe_core::ai_bridge::MAX_BATCH_SIZE),
        );
        self.start(GuiCommand::RunCheck { options });
        if !self.pending {
            return Err("后台命令通道不可用，AI 请求未执行。".into());
        }
        if autonomous {
            self.pending_autonomous_run_id = Some(run_id.clone());
        }
        self.bridge_active = Some((request.clone(), autonomous, run_id.clone()));
        Ok(run_id)
    }

    /// 路径 A（GUI 内）：按当前 Profile 生成一次性检测预览/确认请求。
    fn open_local_f3_preview(&mut self) {
        let locale = self.state.current_locale();
        let eff = self.schedule_prefs.effective();
        let scope_label =
            check_panel::profile_display_label(&self.registry, &self.selected_profile, locale);
        let count = check_panel::profile_target_count(&self.registry, &self.selected_profile);
        let req = ai_debug_panel::AiCheckRequest::sample_for_profile(
            &self.selected_profile,
            scope_label,
            count,
            eff.min_interval_ms,
            eff.batch_size,
            locale,
        );
        self.request_ai_f3_authorization(req);
    }

    /// 路径 A（GUI 内）：用户确认后按当前 Profile 执行一次性检测。
    fn approve_ai_f3_local(&mut self) {
        if let Some(req) = self.ai_debug.pending_f3_request.take() {
            let is_full = req.profile_id == "full";
            let mut options = CheckOptions {
                profile: (!is_full).then_some(req.profile_id),
                full: is_full,
                ipv4: self.ipv4_only,
                fast: self.fast_mode,
                confirm_budget: true,
                run_id: Some(self.bus.next_run_id("run-ai-f3")),
                ..Default::default()
            };
            self.schedule_prefs.apply_to_options(&mut options);
            self.apply_network_settings(&mut options);
            options.min_interval_ms = Some(
                options
                    .min_interval_ms
                    .unwrap_or(req.min_interval_ms)
                    .max(ai_debug_panel::AI_MIN_INTERVAL_MS),
            );
            options.batch_size = Some(
                options
                    .batch_size
                    .unwrap_or(req.batch_size)
                    .clamp(1, ai_debug_panel::AI_MAX_BATCH_SIZE),
            );
            self.start(GuiCommand::RunCheck { options });
        }
    }

    /// 路径 B：在「AI 自主调试 → 自主检测」开启时，允许 AI 自主发起受 QoS 约束的检测运行；
    /// 开关关闭时严格拒绝（零发包）并要求走路径 A 人工 F3 授权。
    pub fn try_ai_autonomous_check(&mut self, profile_id: &str) -> Result<String, &'static str> {
        if !self.ai_debug.autonomous_enabled {
            return Err("AUTONOMOUS_DISABLED");
        }
        let run_id = self.bus.next_run_id("run-ai-auto");
        let mut options = CheckOptions {
            profile: (profile_id != "full").then(|| profile_id.to_string()),
            full: profile_id == "full",
            ipv4: self.ipv4_only,
            fast: self.fast_mode,
            confirm_budget: true,
            run_id: Some(run_id.clone()),
            ..Default::default()
        };
        self.schedule_prefs.apply_to_options(&mut options);
        self.apply_network_settings(&mut options);
        // 强制守 QoS 红线：间隔 >= 3500ms、每批 <= 10、优先 TCP（服务端批量也不得无节制）。
        options.min_interval_ms = Some(
            options
                .min_interval_ms
                .unwrap_or(ai_debug_panel::AI_MIN_INTERVAL_MS)
                .max(ai_debug_panel::AI_MIN_INTERVAL_MS),
        );
        options.batch_size = Some(
            options
                .batch_size
                .unwrap_or(5)
                .clamp(1, ai_debug_panel::AI_MAX_BATCH_SIZE),
        );
        self.pending_autonomous_run_id = Some(run_id.clone());
        self.ai_debug.autonomous_runs = self.ai_debug.autonomous_runs.saturating_add(1);
        self.start(GuiCommand::RunCheck { options });
        Ok(run_id)
    }

    /// 路径 B：在「AI 自主调试 → 自主检测」开启时，允许 AI 自主导出脱敏报告字符串（不写任意磁盘路径）。
    pub fn try_ai_autonomous_export(&mut self, fmt: ExportFormat) -> Result<String, &'static str> {
        if !self.ai_debug.autonomous_enabled {
            return Err("AUTONOMOUS_DISABLED");
        }
        let Some(run) = self.state.active_run() else {
            return Err("NO_ACTIVE_RUN");
        };
        let Some(report) = &run.report else {
            return Err("RUN_NOT_TERMINAL");
        };
        let mut copy = report.clone();
        if copy.origin.is_none() {
            copy.origin = Some(ORIGIN_AGENT_AUTONOMOUS.to_string());
            copy.autonomous_mode = Some(true);
        }
        for row in &mut copy.results {
            row.destination.set_show_ip(self.show_ip);
        }
        let content =
            render_export_locale(&copy, &run.raw_events, fmt, self.state.current_locale());
        self.ai_debug.autonomous_exports = self.ai_debug.autonomous_exports.saturating_add(1);
        Ok(content)
    }

    /// 供测试与演示脚本显式开关本地原始 IP 展示（仅会话内存态）。
    pub fn set_show_ip(&mut self, show: bool) {
        self.show_ip = show;
        if !show {
            self.export_preview = None;
            self.resolved_link_report = None;
        }
    }

    /// 供后台通道与离线演练同步本机内存态 socket 地址细节（与 `WorkerMessage::SocketDetails` 同源）。
    pub fn apply_socket_details(
        &mut self,
        run_id: &str,
        details: Vec<(String, parallaxe_core::destination::Destination)>,
    ) {
        if let Some(run) = self.state.runs.get_mut(run_id) {
            for (target_id, destination) in details {
                if let Some(row) = run.results_by_target.get_mut(&target_id) {
                    row.destination = destination.clone();
                }
                if let Some(row) = run
                    .report
                    .as_mut()
                    .and_then(|report| report.results.iter_mut().find(|r| r.target == target_id))
                {
                    row.destination = destination;
                }
            }
        }
    }

    /// 供无头测试切换到指定标签页。
    pub fn navigate_to(&mut self, page: ActiveTab) {
        self.navigate(page);
    }

    /// 供无头测试/走读直接进入设置二级页（真实用户经一级栏目列表点击进入）。
    pub fn open_settings_page(&mut self, page: settings_page::SettingsPage) {
        self.settings_page = page;
    }

    /// 供无头测试读取当前设置页层级。
    pub fn current_settings_page(&self) -> settings_page::SettingsPage {
        self.settings_page
    }

    /// 供无头测试检查当前标签页。
    pub fn active_tab(&self) -> ActiveTab {
        self.active_tab
    }

    /// 供无头测试访问处置中心视图状态（如注入文件夹选择器测试回调）。
    pub fn guard_view_mut(&mut self) -> &mut guard_panel::GuardViewState {
        &mut self.guard_view
    }

    /// 供无头测试与演练切换「显示技术细节」总开关（默认关闭）。
    pub fn set_show_technical_details(&mut self, enabled: bool) {
        self.show_technical_details = enabled;
    }

    /// 供无头测试读取「显示技术细节」总开关状态。
    pub fn show_technical_details(&self) -> bool {
        self.show_technical_details
    }

    /// 供无头测试读取/切换当前检测首页选定的检测范围。
    pub fn selected_profile(&self) -> &str {
        &self.selected_profile
    }

    pub fn set_selected_profile(&mut self, profile: &str) {
        self.selected_profile = profile.to_string();
    }

    /// 供无头测试与截图演练展开/收起首页「检测节奏」面板。
    pub fn set_pace_panel_open(&mut self, open: bool) {
        self.pace_panel_open = open;
    }

    /// 供无头测试与截图演练切换「显示全部分组」。
    pub fn set_show_all_groups(&mut self, show_all: bool) {
        self.show_all_groups = show_all;
    }

    /// 供无头测试读写调度节奏配置（验证首页与设置页共享同一状态）。
    pub fn schedule_prefs(&self) -> &SchedulePrefs {
        &self.schedule_prefs
    }

    pub fn schedule_prefs_mut(&mut self) -> &mut SchedulePrefs {
        &mut self.schedule_prefs
    }

    /// 供无头测试访问导入页视图状态（如注入测试文件选择器或切换入口）。
    pub fn import_view_mut(&mut self) -> &mut ImportViewState {
        &mut self.import_view
    }

    /// 供无头测试读取/操作目标目录（验证用户自定义分组）。
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    /// 供无头测试与截图演练开关「关于与隐私声明」弹窗。
    pub fn set_about_modal_open(&mut self, open: bool) {
        self.about_modal_open = open;
    }

    /// 供无头测试读取与操作插件宿主（仅桌面端可用）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn plugin_host(&self) -> &PluginHost {
        &self.plugin_host
    }

    /// 供无头测试操作插件宿主（仅桌面端可用）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn plugin_host_mut(&mut self) -> &mut PluginHost {
        &mut self.plugin_host
    }

    /// 供无头测试在审核后批准已暂存插件并持久化设置（仅桌面端可用）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn approve_staged_plugin(&mut self) -> Result<String, String> {
        let id = self.plugin_host.approve_staged(&mut self.settings)?;
        self.persist_current_settings();
        self.refresh_plugins(None);
        Ok(id)
    }

    /// 供无头测试触发一次插件调用（不改动任何判定或退出码）。
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn refresh_plugins(&mut self, event: Option<HookEvent>) {
        let counts = self
            .state
            .active_run()
            .and_then(|r| r.coverage.as_ref())
            .map(|c| (c.covered.len(), c.selected_count as usize));
        self.plugin_host
            .invoke_enabled(&self.settings, "default-profile", event, counts);
    }

    fn sync_settings_from_ui(&mut self) {
        self.settings.language = self.state.current_locale().as_str().to_string();
        self.settings.theme = match self.theme {
            ThemeMode::Dark => "dark".to_string(),
            ThemeMode::Light => "light".to_string(),
        };
        self.settings.density = match self.density {
            DensityMode::Normal => "normal".to_string(),
            DensityMode::Minimal => "compact".to_string(),
        };
        self.settings.profile = self.default_profile.clone();
        self.settings.rule_format = self.guard_view.export_format.as_str().to_string();
        let eff = self.schedule_prefs.effective();
        self.settings.min_interval_ms = eff.min_interval_ms;
        self.settings.inter_target_interval_ms = self.schedule_prefs.inter_target_interval_ms;
        self.settings.batch_size = eff.batch_size;
        self.settings.batch_cooldown_ms = eff.batch_cooldown_ms;
        self.settings.distribution_mode = self.registry.distribution_mode();
    }

    fn persist_current_settings(&mut self) {
        self.sync_settings_from_ui();
        let snapshot = self.settings.clone();
        match settings_store::persist(self.settings_path.as_deref(), |cfg| {
            *cfg = snapshot;
            Ok(())
        }) {
            Ok(saved) => {
                self.settings = saved;
                self.settings_error = None;
            }
            Err(err) => {
                self.settings_error = Some(err);
            }
        }
    }

    /// 供无头渲染测试喂入核心事件。
    pub fn apply_event(&mut self, event: &Event) {
        let was_terminal = self
            .state
            .runs
            .get(&event.run_id)
            .is_some_and(|r| r.terminal.is_some());
        let new_run = !self.state.runs.contains_key(&event.run_id);
        self.state.apply(event);
        if new_run && event.event == EventName::RunStarted {
            self.clear_run_ui();
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            self.refresh_plugins(Some(HookEvent::Start));
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        if event.event == EventName::Error {
            self.refresh_plugins(Some(HookEvent::Error));
        }
        if !was_terminal
            && matches!(
                event.event,
                EventName::RunFinished | EventName::RunCancelled
            )
        {
            self.pending = false;
            if self.pending_autonomous_run_id.as_deref() == Some(event.run_id.as_str()) {
                self.pending_autonomous_run_id = None;
                if let Some(run) = self.state.runs.get_mut(&event.run_id) {
                    if let Some(report) = run.report.as_mut() {
                        report.origin = Some(ORIGIN_AGENT_AUTONOMOUS.to_string());
                        report.autonomous_mode = Some(true);
                    }
                }
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            if self
                .bridge_active
                .as_ref()
                .is_some_and(|(_, _, run_id)| run_id == &event.run_id)
            {
                if let Some((request, autonomous, _)) = self.bridge_active.take() {
                    // 只回传脱敏报告；取消/异常一律显式失败，绝不伪造完整报告。
                    let report = self
                        .state
                        .runs
                        .get(&event.run_id)
                        .and_then(|run| run.report.clone())
                        .map(|mut report| {
                            let user_imported_scope = !request.scope.targets.is_empty()
                                && request
                                    .scope
                                    .targets
                                    .iter()
                                    .all(|id| self.registry.is_user_imported(id));
                            if report.origin.is_none() {
                                if autonomous {
                                    report.origin = Some(ORIGIN_AGENT_AUTONOMOUS.to_string());
                                } else if user_imported_scope {
                                    report.origin = Some(
                                        parallaxe_core::contracts::ORIGIN_USER_IMPORTED.to_string(),
                                    );
                                }
                            }
                            report.autonomous_mode = Some(autonomous);
                            report
                        });
                    match (event.event, report) {
                        (EventName::RunFinished, Some(report)) => {
                            if let Err(error) =
                                self.ai_bridge.complete(&request, report, autonomous)
                            {
                                self.ai_debug.last_notice = Some(error);
                            }
                        }
                        (_, _) => {
                            let _ = self
                                .ai_bridge
                                .fail(&request, "运行被取消或异常终止，未产出完整报告");
                        }
                    }
                }
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                let hook_event = match event.event {
                    EventName::RunFinished => HookEvent::Complete,
                    _ => HookEvent::Error,
                };
                self.refresh_plugins(Some(hook_event));
            }
            if let Some(r) = self
                .state
                .active_run()
                .filter(|r| r.run_id == event.run_id && r.terminal.is_some())
            {
                let markers: Vec<String> = r
                    .ordered_results()
                    .into_iter()
                    .flat_map(|rec| rec.risk_signal.markers.iter().cloned())
                    .collect();
                self.schedule_prefs
                    .on_run_qos_observed(&event.run_id, &markers);
                let page = if r.profile.as_deref() == Some("guard-preview") {
                    ActiveTab::Guard
                } else if self.density == DensityMode::Minimal {
                    ActiveTab::Overview
                } else {
                    ActiveTab::Services
                };
                self.navigate(page);
                self.state.clear_filters();
            }
        }
    }
    fn clear_run_ui(&mut self) {
        self.export_preview = None;
        self.export_status = None;
        self.draft = None;
        self.show_ip = false;
        self.static_explain = None;
        self.state.selected_target_id = None;
        self.selftest_banner = None;
        self.resolved_link_report = None;
    }
    fn navigate(&mut self, page: ActiveTab) {
        self.active_tab = page;
        self.explain_drawer_open = false;
        self.catalog_drawer_open = false;
        if page == ActiveTab::Settings {
            // 每次进入设置页都从一级栏目列表开始。
            self.settings_page = settings_page::SettingsPage::Root;
        }
    }
    fn start(&mut self, mut cmd: GuiCommand) {
        if let GuiCommand::RunCheck { options } = &mut cmd {
            if self.registry.distribution_mode() {
                options.inter_target_interval_ms = Some(
                    options
                        .inter_target_interval_ms
                        .unwrap_or(0)
                        .max(ai_debug_panel::AI_MIN_INTERVAL_MS),
                );
            }
        }
        if !self.pending {
            self.state.last_command_error = None;
            self.pending = self.bus.send(cmd);
            if !self.pending {
                self.state.last_command_error =
                    Some("后台检测线程不可用，请重新打开应用后重试。".into());
            }
        }
    }

    fn drain_worker(&mut self) {
        for msg in self.bus.drain() {
            match msg {
                WorkerMessage::Event(event) => {
                    self.apply_event(&event);
                }
                WorkerMessage::StaticExplain(se) => {
                    if self.state.selected_target_id.as_ref() == Some(&se.target_id) {
                        self.static_explain = Some(se);
                    }
                }
                WorkerMessage::SelftestSummary {
                    run_id,
                    passed,
                    failed,
                    summary_line,
                } => {
                    self.selftest_banner = Some(format!(
                        "离线自检 ({run_id})：{summary_line}（通过 {passed} / 失败 {failed}）"
                    ));
                }
                WorkerMessage::CancelAck { run_id, status } => {
                    self.selftest_banner = Some(if self.show_technical_details {
                        format!("取消指令 ({run_id})：{status}")
                    } else {
                        "已发送取消检测指令，正在等待当前请求结束。".to_string()
                    });
                }
                WorkerMessage::GuardAuditReady(audit) => {
                    if let Some(run) = self.state.runs.get_mut(&audit.run_id) {
                        run.guard_audit = Some(*audit);
                    }
                }
                WorkerMessage::SocketDetails(id, details) => {
                    self.apply_socket_details(&id, details);
                }
                WorkerMessage::LinkReportReady(report) => {
                    self.pending = false;
                    self.selftest_banner = Some(format!(
                        "链路体检只读 DNS 解析完成：共查询 {} 次系统 DNS（网络 HTTP 发包 0）",
                        report.dns_query_count
                    ));
                    self.resolved_link_report = Some(*report);
                }
                WorkerMessage::CommandError(err) => {
                    self.pending = false;
                    self.state.last_command_error = Some(err.clone());
                    // B 模式：后台拒绝/失败时立即向 agent 回传失败结果，避免其无限等待。
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    if let Some((request, _, _)) = self.bridge_active.take() {
                        let _ = self.ai_bridge.fail(&request, err.clone());
                    }
                }
            }
        }
    }

    /// 默认规则包专属确认：只导入用户勾选的目标并全部按"更换"处理；
    /// 用户未点确认前不会调用此函数，因此不会静默改变零发包基线。
    pub fn confirm_default_bundle(&mut self, permanent: bool, indices: &[usize]) {
        if !self.registry.distribution_mode() {
            self.default_bundle.feedback = Some(Err(
                "请先在设置→关于软件中确认开启发行包模式，再启用默认规则包。".to_string(),
            ));
            return;
        }
        let Some(bundle) = self.default_bundle.bundle.clone() else {
            self.default_bundle.feedback = Some(Err("发行包默认规则包不可用。".to_string()));
            return;
        };
        let mut filtered = parallaxe_core::import_bundle::ImportBundle::new(
            parallaxe_core::import_bundle::ImportSource::ZipBundle,
        );
        filtered.readme = bundle.readme.clone();
        filtered.rules = bundle.rules.clone();
        let mut index_set: Vec<usize> = indices.to_vec();
        index_set.sort_unstable();
        index_set.dedup();
        for index in index_set {
            if let Some(entry) = bundle.targets.get(index.saturating_sub(1)) {
                let mut entry = entry.clone();
                entry.replace_existing = true;
                filtered.targets.push(entry);
            }
        }
        if filtered.targets.is_empty() {
            self.default_bundle.feedback = Some(Err("请至少勾选一个目标后再确认。".to_string()));
            return;
        }
        let report = match self.registry.import_bundle(&filtered, true, true) {
            Ok(report) => report,
            Err(err) => {
                self.default_bundle.feedback = Some(Err(format!("更换失败：{err}")));
                return;
            }
        };
        let ids: Vec<String> = report
            .imported_targets
            .iter()
            .map(|meta| meta.id.clone())
            .collect();
        let now = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
        if permanent {
            let Some(state_dir) = self.default_bundle_state_dir.clone() else {
                self.default_bundle.feedback = Some(Err(
                    "当前平台不支持永久落盘，请改选「仅本次生效」。".to_string(),
                ));
                return;
            };
            if let Err(err) = self.registry.authorize_default_bundle_permanent(&ids, &now) {
                self.default_bundle.feedback = Some(Err(format!("授权失败：{err}")));
                return;
            }
            if let Err(err) = self.registry.save_default_bundle_consent(&state_dir) {
                self.registry.revoke_default_bundle_authorization();
                self.default_bundle.feedback =
                    Some(Err(format!("写入本机授权失败，已回滚：{err}")));
                return;
            }
            self.default_bundle.feedback = Some(Ok(format!(
                "已更换并永久授权 {} 项（保存于用户状态目录，可随时在设置页撤销）。",
                ids.len()
            )));
        } else {
            if let Err(err) = self.registry.authorize_default_bundle_session(&ids, true) {
                self.default_bundle.feedback = Some(Err(format!("授权失败：{err}")));
                return;
            }
            self.default_bundle.feedback = Some(Ok(format!(
                "已更换并授权 {} 项：仅本次运行有效，关闭软件后自动失效。",
                ids.len()
            )));
        }
        self.default_bundle.modal_open = false;
        self.default_bundle.protected_confirmed = false;
        self.bus.send(GuiCommand::SyncRegistry {
            registry: Box::new(self.registry.clone()),
        });
    }

    fn dispatch_actions(&mut self, ctx: &egui::Context, actions: Vec<UiAction>) {
        for action in actions {
            match action {
                UiAction::RunProfile(profile) => {
                    let mut options = CheckOptions {
                        profile: Some(profile),
                        ipv4: self.ipv4_only,
                        fast: self.fast_mode,
                        run_id: Some(self.bus.next_run_id("run-gui")),
                        ..Default::default()
                    };
                    self.schedule_prefs.apply_to_options(&mut options);
                    self.apply_network_settings(&mut options);
                    self.start(GuiCommand::RunCheck { options });
                }
                UiAction::RunFull => {
                    let mut options = CheckOptions {
                        full: true,
                        ipv4: self.ipv4_only,
                        fast: self.fast_mode,
                        run_id: Some(self.bus.next_run_id("run-gui-full")),
                        ..Default::default()
                    };
                    self.schedule_prefs.apply_to_options(&mut options);
                    self.apply_network_settings(&mut options);
                    self.start(GuiCommand::RunCheck { options });
                }
                UiAction::RunAuthorizedBundledTargets => {
                    let mut targets = self.registry.authorized_user_target_ids();
                    if targets.is_empty() {
                        if let Some(grant) = self.registry.active_session_grant_snapshot() {
                            targets = grant.targets.keys().cloned().collect();
                        }
                    }
                    if targets.is_empty() {
                        self.state.last_command_error = Some(
                            "「我的授权版」当前没有有效授权：请先完成默认规则包确认（仅本次或永久）。"
                                .to_string(),
                        );
                    } else {
                        let mut options = CheckOptions {
                            targets,
                            ipv4: self.ipv4_only,
                            fast: self.fast_mode,
                            run_id: Some(self.bus.next_run_id("run-gui-authorized")),
                            confirm_budget: true,
                            ..Default::default()
                        };
                        self.schedule_prefs.apply_to_options(&mut options);
                        self.apply_network_settings(&mut options);
                        self.start(GuiCommand::RunCheck { options });
                    }
                }
                UiAction::ConfirmDefaultBundleAuth { permanent, indices } => {
                    self.confirm_default_bundle(permanent, &indices);
                }
                UiAction::RevokeDefaultBundleAuth => {
                    let state_dir = self.default_bundle_state_dir.clone();
                    if let Some(dir) = state_dir {
                        match self.registry.revoke_default_bundle_state(&dir) {
                            Ok(()) => {
                                self.default_bundle.feedback = Some(Ok(
                                    if self.registry.distribution_mode() {
                                        "已撤销默认包授权并删除本机授权文件；发行包模式下的内置目标仍按模式设置运行。"
                                    } else {
                                        "已撤销默认包授权，恢复零发包基线；本机保存的授权文件已删除。"
                                    }
                                    .to_string(),
                                ));
                            }
                            Err(err) => {
                                self.default_bundle.feedback =
                                    Some(Err(format!("撤销失败：{err}")));
                            }
                        }
                    } else {
                        self.registry.revoke_default_bundle_authorization();
                        self.default_bundle.feedback =
                            Some(Ok(if self.registry.distribution_mode() {
                                "已撤销本次默认包授权；发行包模式下的内置目标仍按模式设置运行。"
                            } else {
                                "已撤销本次授权，恢复零发包基线。"
                            }
                            .to_string()));
                    }
                    self.bus.send(GuiCommand::SyncRegistry {
                        registry: Box::new(self.registry.clone()),
                    });
                }
                UiAction::RunCustomTargets(explicit) => {
                    let targets = if explicit.is_empty() {
                        self.checked_targets.iter().cloned().collect()
                    } else {
                        explicit
                    };
                    if targets.is_empty() {
                        self.state.last_command_error =
                            Some("请先在目标目录中勾选至少一个目标".to_string());
                    } else {
                        let mut options = CheckOptions {
                            targets,
                            ipv4: self.ipv4_only,
                            fast: self.fast_mode,
                            run_id: Some(self.bus.next_run_id("run-gui-custom")),
                            ..Default::default()
                        };
                        self.schedule_prefs.apply_to_options(&mut options);
                        self.apply_network_settings(&mut options);
                        self.start(GuiCommand::RunCheck { options });
                    }
                }
                UiAction::RunSingleReprobe(target_id) => {
                    let mut options = CheckOptions {
                        targets: vec![target_id],
                        ipv4: self.ipv4_only,
                        fast: self.fast_mode,
                        run_id: Some(self.bus.next_run_id("run-reprobe")),
                        ..Default::default()
                    };
                    self.schedule_prefs.apply_to_options(&mut options);
                    self.apply_network_settings(&mut options);
                    self.start(GuiCommand::RunCheck { options });
                }
                UiAction::RunFixture(case_id) => {
                    let options = CheckOptions {
                        fixture: Some(case_id),
                        run_id: Some(self.bus.next_run_id("run-fixture")),
                        ..Default::default()
                    };
                    self.start(GuiCommand::RunCheck { options });
                }
                UiAction::RunScenario(scenario) => {
                    self.start(GuiCommand::RunOfflineScenario { scenario });
                }
                UiAction::RunSelftest => {
                    self.start(GuiCommand::RunSelftest { suite: None });
                }
                UiAction::RunUserImportOfflinePreview(target_ids) => {
                    self.bus.send(GuiCommand::SyncRegistry {
                        registry: Box::new(self.registry.clone()),
                    });
                    self.start(GuiCommand::RunUserOfflinePreview { target_ids });
                }
                UiAction::RunLinkResolve(target_ids) => {
                    self.bus.send(GuiCommand::SyncRegistry {
                        registry: Box::new(self.registry.clone()),
                    });
                    let source_report = self
                        .state
                        .active_run()
                        .and_then(|r| r.report.as_ref().map(|rep| Box::new(rep.clone())));
                    self.start(GuiCommand::RunLinkResolve {
                        target_ids,
                        source_report,
                        show_ip: self.show_ip,
                    });
                }
                UiAction::SyncRegistry => {
                    self.bus.send(GuiCommand::SyncRegistry {
                        registry: Box::new(self.registry.clone()),
                    });
                }
                UiAction::CancelActiveRun => {
                    if let Some(run_id) = self.state.active_run_id.clone() {
                        self.state.mark_cancel_requested(&run_id);
                        self.bus.send(GuiCommand::CancelRun { run_id });
                    }
                }
                UiAction::RequestGuardPreview {
                    source_run_id,
                    targets,
                } => {
                    let source = self.state.runs.get(&source_run_id);
                    let source_results = source
                        .map(|r| r.ordered_results().into_iter().cloned().collect())
                        .unwrap_or_default();
                    self.start(GuiCommand::RequestGuardPreview {
                        is_fixture: source.is_some_and(|r| r.is_fixture),
                        source_interrupted: source.and_then(|r| r.termination())
                            != Some(parallaxe_core::Termination::Complete),
                        source_run_id,
                        targets,
                        source_results,
                    });
                    self.navigate(ActiveTab::Guard);
                }
                UiAction::OpenPlan(id) => {
                    let run = self.state.active_run();
                    let host = self.registry.get(&id).map(|t| t.host()).unwrap_or_default();
                    let is_healthy_or_untested =
                        !run.and_then(|r| r.result_for(&id)).is_some_and(|rec| {
                            matches!(
                                rec.state,
                                parallaxe_core::contracts::State::Restricted
                                    | parallaxe_core::contracts::State::Blocked
                            )
                        });
                    if self.registry.get(&id).is_some()
                        || run.is_some_and(|r| r.result_for(&id).is_some())
                    {
                        self.draft = Some(PlanDraft::for_service(
                            &id,
                            host,
                            run,
                            is_healthy_or_untested,
                        ));
                        self.navigate(ActiveTab::Guard);
                    }
                }
                UiAction::SelectTarget(target_id) => {
                    self.static_explain = None;
                    self.catalog_drawer_open = false;
                    self.state.selected_target_id = Some(target_id.clone());
                    self.explain_drawer_open = true;
                    self.bus.send(GuiCommand::ExplainStaticTarget { target_id });
                }
                UiAction::SelectRun(run_id) => {
                    if self.state.runs.contains_key(&run_id) {
                        self.clear_run_ui();
                        self.state.clear_filters();
                        self.state.active_run_id = Some(run_id);
                        self.export_preview = None;
                        self.export_status = None;
                    }
                }
                UiAction::SwitchTab(tab) => {
                    self.navigate(tab);
                }
                UiAction::SwitchMobileTab(tab) => {
                    self.navigate(match tab {
                        MobileTab::Overview => ActiveTab::Overview,
                        MobileTab::Services => ActiveTab::Services,
                        MobileTab::Plan => ActiveTab::Guard,
                        MobileTab::Settings => ActiveTab::Settings,
                    });
                }
                UiAction::SetTheme(mode) => {
                    self.theme = mode;
                    self.persist_current_settings();
                    ctx.request_repaint();
                }
                UiAction::ToggleTheme => {
                    self.theme = self.theme.toggle();
                    self.persist_current_settings();
                    ctx.request_repaint();
                }
                UiAction::SetDensity(mode) => {
                    self.density = mode;
                    if mode == DensityMode::Minimal {
                        self.catalog_drawer_open = false;
                        if self.active_tab == ActiveTab::Services {
                            self.active_tab = ActiveTab::Overview;
                        }
                    }
                    self.persist_current_settings();
                    ctx.request_repaint();
                }
                UiAction::ToggleDensity => {
                    self.density = self.density.toggle();
                    if self.density == DensityMode::Minimal {
                        self.catalog_drawer_open = false;
                        if self.active_tab == ActiveTab::Services {
                            self.active_tab = ActiveTab::Overview;
                        }
                    }
                    self.persist_current_settings();
                    ctx.request_repaint();
                }
                UiAction::SetLocale(locale) => {
                    self.state.set_locale(locale);
                    self.persist_current_settings();
                    ctx.request_repaint();
                }
                UiAction::ToggleCatalogDrawer => {
                    self.explain_drawer_open = false;
                    self.catalog_drawer_open = !self.catalog_drawer_open;
                }
                UiAction::CloseCatalogDrawer => {
                    self.catalog_drawer_open = false;
                }
                UiAction::ToggleExplainDrawer => {
                    self.explain_drawer_open = !self.explain_drawer_open;
                }
                UiAction::CloseExplainDrawer => {
                    self.explain_drawer_open = false;
                }
                UiAction::ToggleAboutModal => {
                    self.about_modal_open = !self.about_modal_open;
                }
                UiAction::ExportPreview(fmt) => {
                    if let Some(run) = self.state.active_run() {
                        if let Some(report) = &run.report {
                            let mut copy = report.clone();
                            for row in &mut copy.results {
                                row.destination.set_show_ip(self.show_ip);
                            }
                            let text = render_export_locale(
                                &copy,
                                &run.raw_events,
                                fmt,
                                self.state.current_locale(),
                            );
                            self.export_preview = Some((fmt, text));
                            self.export_status =
                                Some(format!("已生成 {} 预览（同源 core 导出器）", fmt.label()));
                        }
                    }
                }
                UiAction::ExportToFile(fmt, path) => {
                    if let Some(run) = self.state.active_run() {
                        if let Some(report) = &run.report {
                            let mut copy = report.clone();
                            for row in &mut copy.results {
                                row.destination.set_show_ip(self.show_ip);
                            }
                            let text = render_export_locale(
                                &copy,
                                &run.raw_events,
                                fmt,
                                self.state.current_locale(),
                            );
                            match write_export_file(&path, &text) {
                                Ok(()) => {
                                    self.export_status = Some(format!(
                                        "已保存 {} 至 {}",
                                        fmt.label(),
                                        path.display()
                                    ));
                                }
                                Err(err) => {
                                    self.export_status = Some(err);
                                }
                            }
                        }
                    }
                }
                UiAction::ClearFilters => {
                    self.state.clear_filters();
                }
                UiAction::RequestDistributionModeEnable => {
                    if !parallaxe_core::distribution::DISTRIBUTION_RELEASE_BUILD {
                        self.distribution_mode_confirm_open = true;
                    }
                }
                UiAction::ConfirmDistributionModeEnable => {
                    self.apply_distribution_mode(true);
                }
                UiAction::DisableDistributionMode => {
                    self.apply_distribution_mode(false);
                }
                UiAction::SetAiAutonomous(enabled) => {
                    self.set_ai_autonomous_enabled(enabled);
                }
                UiAction::AiSelfDisableAutonomous => {
                    self.ai_self_disable_autonomous(None);
                }
                UiAction::OpenAiF3AuthModal => {
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    if self.bridge_pending.is_some() {
                        // 已有外部 AI 请求在等待确认：保持该请求的弹窗与范围，不被本地预览覆盖。
                    } else {
                        self.open_local_f3_preview();
                    }
                    #[cfg(any(target_os = "android", target_os = "ios"))]
                    self.open_local_f3_preview();
                }
                UiAction::ApproveAiF3Auth => {
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    if let Some(request) = self.bridge_pending.take() {
                        // 来自 agent 的授权桥请求：先写回授权决定，再按请求范围执行。
                        self.ai_debug.pending_f3_request = None;
                        match self.ai_bridge.approve(&request, false) {
                            Ok(_) => {
                                if let Err(error) = self.start_bridge_run(&request, false) {
                                    let _ = self.ai_bridge.fail(&request, error.clone());
                                    self.bridge_active = None;
                                    self.ai_debug.last_notice = Some(error);
                                }
                            }
                            Err(error) => {
                                let _ = self.ai_bridge.fail(&request, error.clone());
                                self.ai_debug.last_notice = Some(error);
                            }
                        }
                    } else {
                        self.approve_ai_f3_local();
                    }
                    #[cfg(any(target_os = "android", target_os = "ios"))]
                    self.approve_ai_f3_local();
                }
                UiAction::RejectAiF3Auth => {
                    let locale = self.state.current_locale();
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    if let Some(request) = self.bridge_pending.take() {
                        let _ = self.ai_bridge.deny(&request);
                    }
                    self.ai_debug.pending_f3_request = None;
                    self.ai_debug.last_notice =
                        Some(translate(locale, "gui.ai.f3.rejected_notice", &[]));
                }
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                UiAction::OpenPluginPage { plugin_id, page_id } => {
                    self.plugin_host.active_page = Some((plugin_id, page_id));
                    self.navigate(ActiveTab::PluginPage);
                }
            }
        }
    }

    pub fn render_ui(&mut self, ctx: &egui::Context) {
        self.drain_worker();
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        self.poll_ai_bridge();
        if self.applied_theme != Some(self.theme) {
            apply_theme(ctx, self.theme);
            self.applied_theme = Some(self.theme);
        }

        let spec = LayoutSpec::compute_for_platform(
            self.density,
            ctx.screen_rect().width(),
            cfg!(target_os = "android"),
        );
        let p = Palette::for_theme(self.theme);
        let locale = self.state.current_locale();
        let mut actions = vec![];

        // 统一返回键 / Escape 处理（不抢占已打开的下拉弹出层与 F3 确认弹窗）
        if !ctx.memory(|m| m.any_popup_open())
            && !self.import_view.online_confirm_open
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            let closed_notices =
                if self.plugin_host.plugin_notices_open || self.plugin_host.dep_notices_open {
                    self.plugin_host.plugin_notices_open = false;
                    self.plugin_host.dep_notices_open = false;
                    true
                } else {
                    false
                };
            #[cfg(any(target_os = "android", target_os = "ios"))]
            let closed_notices = false;

            if closed_notices {
                // handled
            } else if self.distribution_mode_confirm_open {
                self.distribution_mode_confirm_open = false;
            } else if self.default_bundle.modal_open {
                self.default_bundle.modal_open = false;
            } else if self.ai_debug.pending_f3_request.is_some() {
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                if let Some(request) = self.bridge_pending.take() {
                    let _ = self.ai_bridge.deny(&request);
                }
                self.ai_debug.pending_f3_request = None;
                self.ai_debug.last_notice =
                    Some(translate(locale, "gui.ai.f3.rejected_notice", &[]));
            } else if self.guard_view.any_modal_open() {
                self.guard_view.close_modals();
            } else if self.about_modal_open {
                self.about_modal_open = false;
            } else if self.explain_drawer_open {
                self.explain_drawer_open = false;
            } else if self.catalog_drawer_open {
                self.catalog_drawer_open = false;
            } else if spec.tier == ViewportTier::Narrow && self.active_tab != ActiveTab::Overview {
                self.navigate(ActiveTab::Overview);
            }
        }

        top_bar::title(
            ctx,
            p,
            spec.show_top_subtitle,
            self.density,
            self.theme,
            locale,
            spec.safe_area.top,
            &mut actions,
        );
        if spec.show_bottom_nav {
            let nav_panel = egui::TopBottomPanel::bottom("mobile_nav")
                .frame(
                    egui::Frame::none()
                        .fill(p.sidebar)
                        .inner_margin(egui::Margin {
                            left: SPACE_2,
                            right: SPACE_2,
                            top: SPACE_2,
                            bottom: SPACE_2 + spec.safe_area.bottom,
                        }),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SPACE_1;
                        let w = (ui.available_width() - SPACE_1 * 3.0) / 4.0;
                        for (tab, page, ic) in [
                            (MobileTab::Overview, ActiveTab::Overview, IconKind::Overview),
                            (MobileTab::Services, ActiveTab::Services, IconKind::Services),
                            (MobileTab::Plan, ActiveTab::Guard, IconKind::Guard),
                            (MobileTab::Settings, ActiveTab::Settings, IconKind::Settings),
                        ] {
                            let selected = self.active_tab == page
                                || (tab == MobileTab::Services
                                    && self.active_tab == ActiveTab::Coverage)
                                || (tab == MobileTab::Settings
                                    && matches!(
                                        self.active_tab,
                                        ActiveTab::Timeline | ActiveTab::Import
                                    ));
                            let lbl = tab.label_localized(locale);
                            if top_bar::mobile_nav_item(ui, p, ic, &lbl, selected, w).clicked() {
                                actions.push(UiAction::SwitchMobileTab(tab));
                            }
                        }
                    });
                });
            let rect = nav_panel.response.rect;
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("mobile_nav_hairline"),
            ))
            .line_segment(
                [
                    egui::pos2(rect.left(), rect.top() + 0.5),
                    egui::pos2(rect.right(), rect.top() + 0.5),
                ],
                egui::Stroke::new(1.0_f32, p.border_subtle),
            );
        } else {
            let st_panel = egui::TopBottomPanel::bottom("status")
                .frame(
                    egui::Frame::none()
                        .fill(p.sidebar)
                        .inner_margin(egui::Margin::symmetric(SPACE_4, SPACE_2)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let show_cta = self.active_tab == ActiveTab::Overview
                            && !self.explain_drawer_open
                            && !self.catalog_drawer_open;
                        if show_cta {
                            let scope_label = check_panel::profile_display_label(
                                &self.registry,
                                &self.selected_profile,
                                locale,
                            );
                            ui.label(
                                RichText::new(if locale == Locale::EnUs {
                                    format!("Check scope: {scope_label}")
                                } else {
                                    format!("检测范围：{scope_label}")
                                })
                                .size(FONT_META)
                                .color(p.text_secondary),
                            );
                        } else {
                            ui.label(
                                RichText::new(translate(locale, "gui.status.readonly_bar", &[]))
                                    .size(FONT_META)
                                    .color(p.text_muted),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if show_cta {
                                let cta_label = if self.pending {
                                    translate(locale, "gui.action.checking", &[])
                                } else {
                                    translate(locale, "gui.action.start_check", &[])
                                };
                                if btn_filled(ui, p, &cta_label, !self.pending).clicked() {
                                    actions.push(check_panel::profile_run_action(
                                        &self.selected_profile,
                                    ));
                                }
                            } else if ui
                                .link(translate(locale, "gui.status.about", &[]))
                                .clicked()
                            {
                                actions.push(UiAction::ToggleAboutModal);
                            }
                        });
                    });
                });
            let rect = st_panel.response.rect;
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("status_bar_hairline"),
            ))
            .line_segment(
                [
                    egui::pos2(rect.left(), rect.top() + 0.5),
                    egui::pos2(rect.right(), rect.top() + 0.5),
                ],
                egui::Stroke::new(1.0_f32, p.border_subtle),
            );
        }
        if spec.show_bottom_nav
            && self.active_tab == ActiveTab::Overview
            && !self.explain_drawer_open
            && !self.catalog_drawer_open
        {
            let cta_label = if self.pending {
                translate(locale, "gui.action.checking", &[])
            } else {
                translate(locale, "gui.action.start_check", &[])
            };
            let bottom_offset = 70.0 + spec.safe_area.bottom;
            egui::Area::new(egui::Id::new("mobile_primary_cta"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -bottom_offset])
                .show(ctx, |ui| {
                    let w = (ctx.screen_rect().width() - 32.0).clamp(240.0, 420.0);
                    let resp = ui.add_enabled(
                        !self.pending,
                        egui::Button::new(
                            RichText::new(&cta_label).strong().size(FONT_TITLE).color(
                                if self.pending {
                                    p.text_muted
                                } else {
                                    p.accent_fg
                                },
                            ),
                        )
                        .fill(if self.pending { p.card_alt } else { p.accent })
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            if self.pending { p.border } else { p.accent },
                        ))
                        .rounding(egui::Rounding::same(RADIUS_MD))
                        .min_size(egui::vec2(w, 48.0)),
                    );
                    if resp.clicked() {
                        actions.push(check_panel::profile_run_action(&self.selected_profile));
                    }
                });
        }
        if spec.tier == ViewportTier::Wide {
            let side_panel = egui::SidePanel::left("workbench_nav")
                .resizable(false)
                .exact_width(200.0)
                .frame(egui::Frame::none().fill(p.sidebar).inner_margin(SPACE_4))
                .show(ctx, |ui| {
                    ui.add_space(SPACE_1);
                    ui.horizontal(|ui| {
                        ui.add_space(SPACE_2);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(translate(locale, "gui.nav.workbench", &[]))
                                    .size(FONT_TITLE)
                                    .strong()
                                    .color(p.text_primary),
                            );
                            ui.label(
                                RichText::new(translate(locale, "gui.nav.readonly_badge", &[]))
                                    .size(FONT_META)
                                    .color(p.text_muted),
                            );
                        });
                    });

                    // 分组 1：核心诊断（同组内 4px，组间 20px，严格遵循 4/8 间距从属规则）
                    ui.add_space(SPACE_5);
                    top_bar::sidebar_group_label(
                        ui,
                        p,
                        &translate(locale, "gui.nav.group.core", &[]),
                    );
                    ui.spacing_mut().item_spacing.y = SPACE_1;
                    for (tab, label_key) in [
                        (ActiveTab::Overview, "gui.nav.dashboard"),
                        (ActiveTab::Guard, "gui.nav.guard_center"),
                    ] {
                        let current = self.active_tab == tab
                            || (tab == ActiveTab::Overview
                                && matches!(
                                    self.active_tab,
                                    ActiveTab::Services | ActiveTab::Coverage
                                ));
                        let lbl = translate(locale, label_key, &[]);
                        if top_bar::sidebar_nav(
                            ui,
                            p,
                            tab_icon(tab),
                            &lbl,
                            current && !self.catalog_drawer_open,
                            ui.available_width(),
                        )
                        .clicked()
                        {
                            actions.push(UiAction::SwitchTab(tab));
                        }
                    }

                    // 分组 2：数据与扩展
                    ui.add_space(SPACE_5);
                    top_bar::sidebar_group_label(
                        ui,
                        p,
                        &translate(locale, "gui.nav.group.data", &[]),
                    );
                    for (tab, label_key) in [
                        (ActiveTab::Import, "gui.nav.import"),
                        (ActiveTab::Timeline, "gui.nav.history"),
                    ] {
                        let current = self.active_tab == tab;
                        let lbl = translate(locale, label_key, &[]);
                        if top_bar::sidebar_nav(
                            ui,
                            p,
                            tab_icon(tab),
                            &lbl,
                            current && !self.catalog_drawer_open,
                            ui.available_width(),
                        )
                        .clicked()
                        {
                            actions.push(UiAction::SwitchTab(tab));
                        }
                    }

                    // 分组 3：工具与偏好
                    ui.add_space(SPACE_5);
                    top_bar::sidebar_group_label(
                        ui,
                        p,
                        &translate(locale, "gui.nav.group.tools", &[]),
                    );
                    if top_bar::sidebar_nav(
                        ui,
                        p,
                        IconKind::Catalog,
                        &translate(locale, "gui.nav.catalog", &[]),
                        self.catalog_drawer_open,
                        ui.available_width(),
                    )
                    .clicked()
                    {
                        actions.push(UiAction::ToggleCatalogDrawer);
                    }
                    if top_bar::sidebar_nav(
                        ui,
                        p,
                        IconKind::Settings,
                        &translate(locale, "gui.nav.settings", &[]),
                        self.active_tab == ActiveTab::Settings && !self.catalog_drawer_open,
                        ui.available_width(),
                    )
                    .clicked()
                    {
                        actions.push(UiAction::SwitchTab(ActiveTab::Settings));
                    }

                    // 分组 4：重型插件独立页面入口（桌面端按需显示，移动端零插件面）
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    {
                        let plugin_pages = self.plugin_host.available_pages();
                        if !plugin_pages.is_empty() {
                            ui.add_space(SPACE_5);
                            top_bar::sidebar_group_label(
                                ui,
                                p,
                                &translate(locale, "gui.nav.group.plugins", &[]),
                            );
                            for (pid, _pname, page) in plugin_pages {
                                let selected = self.active_tab == ActiveTab::PluginPage
                                    && !self.catalog_drawer_open
                                    && self.plugin_host.active_page.as_ref()
                                        == Some(&(pid.clone(), page.id.clone()));
                                if top_bar::sidebar_nav(
                                    ui,
                                    p,
                                    IconKind::Plugin,
                                    &page.title,
                                    selected,
                                    ui.available_width(),
                                )
                                .clicked()
                                {
                                    actions.push(UiAction::OpenPluginPage {
                                        plugin_id: pid,
                                        page_id: page.id,
                                    });
                                }
                            }
                        }
                    }

                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(SPACE_2);
                            ui.label(
                                RichText::new(translate(locale, "gui.nav.footer_badge", &[]))
                                    .size(FONT_META)
                                    .color(p.text_muted),
                            );
                        });
                    });
                });
            let rect = side_panel.response.rect;
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("sidebar_hairline"),
            ))
            .line_segment(
                [
                    egui::pos2(rect.right() - 0.5, rect.top()),
                    egui::pos2(rect.right() - 0.5, rect.bottom()),
                ],
                egui::Stroke::new(1.0_f32, p.border_subtle),
            );
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(p.canvas).inner_margin(
                if ctx.screen_rect().width() < 380.0 {
                    egui::Margin::symmetric(SPACE_2, SPACE_3)
                } else if spec.tier == ViewportTier::Narrow {
                    egui::Margin::same(SPACE_4)
                } else {
                    egui::Margin::symmetric(SPACE_6, SPACE_4)
                },
            ))
            .show(ctx, |ui| {
                if self.explain_drawer_open {
                    explain_drawer::render(
                        ui,
                        &self.registry,
                        &self.state,
                        self.static_explain.as_ref(),
                        self.theme,
                        true,
                        self.show_technical_details,
                        self.show_ip,
                        &mut actions,
                    );
                    return;
                }
                if self.catalog_drawer_open {
                    target_tree::render(
                        ui,
                        &self.registry,
                        &self.state,
                        self.theme,
                        &mut self.checked_targets,
                        &mut self.tree_filter,
                        self.show_technical_details,
                        &mut actions,
                    );
                    return;
                }
                let max_content_w = match self.active_tab {
                    ActiveTab::Settings => 960.0_f32,
                    ActiveTab::Import => 1040.0_f32,
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    ActiveTab::PluginPage => 960.0_f32,
                    ActiveTab::Overview if self.density == DensityMode::Minimal => 1040.0_f32,
                    _ => 1160.0_f32,
                };
                if spec.tier == ViewportTier::Compact
                    || (spec.tier == ViewportTier::Wide && spec.show_desktop_tabs)
                {
                    let tab_avail_w = ui.available_width();
                    let tab_side_pad =
                        if spec.tier != ViewportTier::Narrow && tab_avail_w > max_content_w {
                            ((tab_avail_w - max_content_w) * 0.5).floor()
                        } else {
                            0.0
                        };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if tab_side_pad > 0.0 {
                            ui.add_space(tab_side_pad);
                        }
                        // 顶部分段 Tab 统一轨道（参考 shadcn-admin + Axolotl NavTabs，自适应宽度零尾空）
                        egui::Frame::none()
                            .fill(p.sidebar)
                            .stroke(egui::Stroke::new(1.0_f32, p.border_subtle))
                            .rounding(egui::Rounding::same(RADIUS_MD))
                            .inner_margin(egui::Margin::same(SPACE_1))
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = SPACE_1;
                                    for tab in ActiveTab::ALL {
                                        let lbl = tab.label_localized(locale);
                                        if top_bar::segment_tab(
                                            ui,
                                            p,
                                            tab_icon(tab),
                                            &lbl,
                                            self.active_tab == tab,
                                        )
                                        .clicked()
                                        {
                                            actions.push(UiAction::SwitchTab(tab));
                                        }
                                    }
                                    if spec.tier == ViewportTier::Compact
                                        && top_bar::segment_tab(
                                            ui,
                                            p,
                                            IconKind::Settings,
                                            &ActiveTab::Settings.label_localized(locale),
                                            self.active_tab == ActiveTab::Settings,
                                        )
                                        .clicked()
                                    {
                                        actions.push(UiAction::SwitchTab(ActiveTab::Settings));
                                    }
                                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                                    if spec.tier == ViewportTier::Compact {
                                        for (pid, _pname, page) in
                                            self.plugin_host.available_pages()
                                        {
                                            let selected = self.active_tab == ActiveTab::PluginPage
                                                && self.plugin_host.active_page.as_ref()
                                                    == Some(&(pid.clone(), page.id.clone()));
                                            if top_bar::segment_tab(
                                                ui,
                                                p,
                                                IconKind::Plugin,
                                                &page.title,
                                                selected,
                                            )
                                            .clicked()
                                            {
                                                actions.push(UiAction::OpenPluginPage {
                                                    plugin_id: pid,
                                                    page_id: page.id,
                                                });
                                            }
                                        }
                                    }
                                });
                            });
                    });
                    ui.add_space(SPACE_4);
                }
                egui::ScrollArea::vertical()
                    .id_salt(("workspace", self.active_tab as u8))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let avail_w = ui.available_width();
                        let avail_h = ui.available_height().max(600.0);
                        if spec.tier != ViewportTier::Narrow && avail_w > max_content_w {
                            let side_pad = ((avail_w - max_content_w) * 0.5).floor();
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.add_space(side_pad);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(max_content_w, avail_h),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(max_content_w);
                                        ui.set_max_width(max_content_w);
                                        self.workspace(ui, spec, &mut actions);
                                    },
                                );
                            });
                        } else {
                            self.workspace(ui, spec, &mut actions);
                        }
                        if spec.touch_friendly && ctx.wants_keyboard_input() {
                            // 移动端/窄屏软键盘避让留白，防止输入框被屏幕键盘遮挡
                            ui.add_space(220.0);
                        } else if spec.show_bottom_nav && self.active_tab == ActiveTab::Overview {
                            // 给常驻悬浮主按钮留出滚动余量，避免遮住最后一组目标。
                            ui.add_space(72.0);
                        }
                    });
            });
        guard_panel::render_modals(
            ctx,
            &self.registry,
            &self.state,
            &mut self.guard_view,
            self.theme,
            self.show_technical_details,
        );
        ai_debug_panel::render_f3_auth_modal(
            ctx,
            &self.ai_debug,
            p,
            locale,
            self.show_technical_details,
            &mut actions,
        );
        let permanent_available = self.default_bundle_state_dir.is_some();
        default_bundle_panel::render_modal(
            ctx,
            &self.registry,
            &mut self.default_bundle,
            locale,
            p,
            permanent_available,
            &mut actions,
        );
        distribution_mode_panel::render_confirm_modal(
            ctx,
            &mut self.distribution_mode_confirm_open,
            locale,
            p,
            &mut actions,
        );
        if self.about_modal_open {
            render_modal_scrim(ctx, "about_modal_scrim", p);
            let screen = ctx.screen_rect();
            let safe = crate::layout::SafeAreaInsets::for_platform();
            let h_margin = if screen.width() < 480.0 { 12.0 } else { 24.0 };
            let inner_margin = if screen.width() < 480.0 {
                SPACE_3
            } else {
                SPACE_5
            };
            let about_w =
                (screen.width() - h_margin * 2.0 - inner_margin * 2.0 - 4.0).clamp(220.0, 440.0);
            let safe_top = safe.top + 12.0;
            let safe_bottom = safe.bottom + 12.0;
            let offset_y = (safe_top - safe_bottom) * 0.5;
            let max_win_h =
                (screen.height() - safe_top - safe_bottom - inner_margin * 2.0 - 4.0).max(150.0);
            let max_body_h = (max_win_h - 84.0).max(88.0);
            let mut keep_about_open = self.about_modal_open;
            let about_title = translate(locale, "gui.status.about", &[]);
            egui::Area::new(egui::Id::new("about_privacy_modal"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, offset_y])
                .constrain(true)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style())
                        .fill(p.card)
                        .stroke(egui::Stroke::new(1.0_f32, p.border))
                        .rounding(egui::Rounding::same(RADIUS_LG))
                        .inner_margin(egui::Margin::same(inner_margin))
                        .show(ui, |ui| {
                            ui.set_width(about_w);
                            ui.set_max_width(about_w);
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2((about_w - 34.0).max(150.0), 0.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&about_title)
                                                    .size(FONT_TITLE)
                                                    .strong()
                                                    .color(p.text_primary),
                                            )
                                            .wrap(),
                                        );
                                    },
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Min),
                                    |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("×")
                                                        .size(16.0)
                                                        .color(p.text_secondary),
                                                )
                                                .frame(false),
                                            )
                                            .clicked()
                                        {
                                            keep_about_open = false;
                                        }
                                    },
                                );
                            });
                            ui.add_space(SPACE_2);
                            ui.separator();
                            ui.add_space(SPACE_2);
                            egui::ScrollArea::vertical()
                                .id_salt("about_privacy_modal_scroll")
                                .max_height(max_body_h)
                                .min_scrolled_height(max_body_h.min(340.0))
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.set_width(about_w);
                                    ui.set_max_width(about_w);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(translate(
                                                locale,
                                                "gui.nav.readonly_badge",
                                                &[],
                                            ))
                                            .size(FONT_TITLE)
                                            .strong()
                                            .color(p.text_primary),
                                        )
                                        .wrap(),
                                    );
                                    ui.add_space(SPACE_2);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(translate(
                                                locale,
                                                "gui.settings.about_privacy",
                                                &[],
                                            ))
                                            .size(FONT_BODY)
                                            .color(p.text_secondary),
                                        )
                                        .wrap(),
                                    );
                                    ui.add_space(SPACE_2);
                                    kv_row(
                                        ui,
                                        p,
                                        &translate(locale, "gui.settings.about_version", &[]),
                                        80.0,
                                        parallaxe_core::VERSION,
                                        true,
                                    );
                                    kv_row(
                                        ui,
                                        p,
                                        &translate(locale, "gui.settings.about_license", &[]),
                                        80.0,
                                        "MIT OR Apache-2.0",
                                        false,
                                    );
                                    kv_row(
                                        ui,
                                        p,
                                        if locale == Locale::EnUs {
                                            "Access Mode"
                                        } else {
                                            "运行模式"
                                        },
                                        80.0,
                                        if locale == Locale::EnUs {
                                            "Anonymous only · Stateless read-only"
                                        } else {
                                            "匿名无账号 · 无状态只读观测"
                                        },
                                        false,
                                    );
                                    ui.add_space(SPACE_2);
                                    let show_tech = self.show_technical_details;
                                    egui::CollapsingHeader::new(
                                        RichText::new(translate(
                                            locale,
                                            "gui.advanced.toggle",
                                            &[],
                                        ))
                                        .size(FONT_META)
                                        .color(p.text_secondary),
                                    )
                                    .id_salt("about_modal_advanced_tech")
                                    .open(show_tech.then_some(true))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(format!(
                                                    "version={}\nlicense=MIT OR Apache-2.0\naccount_mode={ACCOUNT_MODE}\nsession_mode={SESSION_MODE}",
                                                    parallaxe_core::VERSION
                                                ))
                                                .monospace()
                                                .size(FONT_META),
                                            )
                                            .wrap(),
                                        );
                                    });
                                    ui.add_space(SPACE_2);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(translate(
                                                locale,
                                                "gui.minimal.egress.note",
                                                &[],
                                            ))
                                            .size(FONT_META)
                                            .color(p.text_muted),
                                        )
                                        .wrap(),
                                    );
                                    ui.add_space(SPACE_2);
                                    ui.hyperlink_to(
                                        RichText::new(translate(
                                            locale,
                                            "gui.guard.docs_link",
                                            &[],
                                        ))
                                        .size(FONT_META)
                                        .color(p.accent),
                                        guard_panel::SELF_BLOCK_DOCS_URL,
                                    );
                                });
                        });
                });
            self.about_modal_open = keep_about_open;
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        settings_plugins::render_notices_modals(ctx, &mut self.plugin_host, p, locale);
        top_bar::resize_edges(ctx);
        self.dispatch_actions(ctx, actions);
    }

    fn workspace(&mut self, ui: &mut Ui, spec: LayoutSpec, actions: &mut Vec<UiAction>) {
        let p = Palette::for_theme(self.theme);
        let locale = self.state.current_locale();
        let run = self.state.active_run();
        let eyebrow_key = if run.is_some_and(|r| r.is_fixture) {
            "gui.eyebrow.fixture"
        } else if run.is_some() {
            "gui.eyebrow.live"
        } else {
            "gui.eyebrow.idle"
        };
        ui.label(
            RichText::new(translate(locale, eyebrow_key, &[]))
                .size(FONT_META)
                .strong()
                .color(p.accent),
        );
        ui.add_space(SPACE_1);
        let title_key = match self.active_tab {
            ActiveTab::Overview => "gui.page.overview",
            ActiveTab::Services => "gui.page.services",
            ActiveTab::Coverage => "gui.page.coverage",
            ActiveTab::Guard => "gui.page.guard",
            ActiveTab::Timeline => "gui.page.timeline",
            ActiveTab::Import => "gui.page.import",
            ActiveTab::Settings => "gui.page.settings",
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            ActiveTab::PluginPage => "gui.page.plugin_page",
        };
        let title_text = translate(locale, title_key, &[]);
        if spec.tier == ViewportTier::Narrow {
            ui.add(
                egui::Label::new(
                    RichText::new(&title_text)
                        .size(FONT_DISPLAY)
                        .strong()
                        .color(p.text_primary),
                )
                .wrap(),
            );
            if matches!(self.active_tab, ActiveTab::Overview | ActiveTab::Services) {
                let show_header_cta = self.active_tab == ActiveTab::Services;
                let has_export = run.is_some_and(|r| r.report.is_some());
                let has_minimal_plan = self.density == DensityMode::Minimal;
                if show_header_cta || has_export || has_minimal_plan {
                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_2);
                        if show_header_cta {
                            let cta_label = if self.pending {
                                translate(locale, "gui.action.checking", &[])
                            } else {
                                translate(locale, "gui.action.start_check", &[])
                            };
                            if btn_filled(ui, p, &cta_label, !self.pending).clicked() {
                                actions
                                    .push(check_panel::profile_run_action(&self.selected_profile));
                            }
                        }
                        if has_export
                            && btn_outlined(
                                ui,
                                p,
                                &translate(locale, "gui.export.quick_link", &[]),
                                true,
                            )
                            .clicked()
                        {
                            actions.push(UiAction::SwitchTab(ActiveTab::Timeline));
                            actions.push(UiAction::ExportPreview(ExportFormat::Markdown));
                        }
                        if has_minimal_plan
                            && btn_tonal(
                                ui,
                                p,
                                &translate(locale, "gui.action.view_plan", &[]),
                                true,
                            )
                            .clicked()
                        {
                            actions.push(UiAction::SwitchTab(ActiveTab::Guard));
                        }
                    });
                }
            } else if self.active_tab == ActiveTab::Guard && self.draft.is_none() {
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.action.view_services", &[]),
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::SwitchTab(ActiveTab::Services));
                    }
                    if let Some(run) = self.state.active_run() {
                        for r in run.ordered_results().into_iter().take(2) {
                            let label = translate(
                                locale,
                                "gui.action.generate_plan",
                                &[service_matrix::service_name(&r.target)],
                            );
                            if btn_tonal(ui, p, &label, true).clicked() {
                                actions.push(UiAction::OpenPlan(r.target.clone()));
                            }
                        }
                    }
                });
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(&title_text)
                        .size(FONT_DISPLAY)
                        .strong()
                        .color(p.text_primary),
                );
                if matches!(self.active_tab, ActiveTab::Overview | ActiveTab::Services) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = SPACE_2;
                        if self.active_tab == ActiveTab::Services {
                            let cta_label = if self.pending {
                                translate(locale, "gui.action.checking", &[])
                            } else {
                                translate(locale, "gui.action.start_check", &[])
                            };
                            if btn_filled(ui, p, &cta_label, !self.pending).clicked() {
                                actions
                                    .push(check_panel::profile_run_action(&self.selected_profile));
                            }
                        }
                        if run.is_some_and(|r| r.report.is_some())
                            && btn_outlined(
                                ui,
                                p,
                                &translate(locale, "gui.export.quick_link", &[]),
                                true,
                            )
                            .clicked()
                        {
                            actions.push(UiAction::SwitchTab(ActiveTab::Timeline));
                            actions.push(UiAction::ExportPreview(ExportFormat::Markdown));
                        }
                        if self.density == DensityMode::Minimal
                            && btn_tonal(
                                ui,
                                p,
                                &translate(locale, "gui.action.view_plan", &[]),
                                true,
                            )
                            .clicked()
                        {
                            actions.push(UiAction::SwitchTab(ActiveTab::Guard));
                        }
                    });
                } else if self.active_tab == ActiveTab::Guard && self.draft.is_none() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = SPACE_2;
                        if let Some(run) = self.state.active_run() {
                            for r in run.ordered_results().into_iter().take(2) {
                                let label = translate(
                                    locale,
                                    "gui.action.generate_plan",
                                    &[service_matrix::service_name(&r.target)],
                                );
                                if btn_tonal(ui, p, &label, true).clicked() {
                                    actions.push(UiAction::OpenPlan(r.target.clone()));
                                }
                            }
                        }
                        if btn_outlined(
                            ui,
                            p,
                            &translate(locale, "gui.action.view_services", &[]),
                            true,
                        )
                        .clicked()
                        {
                            actions.push(UiAction::SwitchTab(ActiveTab::Services));
                        }
                    });
                }
            });
        }
        if matches!(self.active_tab, ActiveTab::Overview | ActiveTab::Services) {
            ai_debug_panel::render_session_banner(
                ui,
                &mut self.ai_debug,
                p,
                locale,
                self.show_technical_details,
                actions,
            );
        }
        render_running_pulse_bar(ui, self.pending, p);
        if self.pending {
            ui.add_space(SPACE_2);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(translate(locale, "gui.common.running_wait", &[]));
                if btn_outlined(
                    ui,
                    p,
                    &translate(locale, "gui.action.cancel_check", &[]),
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::CancelActiveRun);
                }
            });
        }
        if let Some(err) = &self.state.last_command_error {
            ui.add_space(SPACE_2);
            ui.colored_label(p.danger_fg, err);
        }
        if let Some(msg) = &self.selftest_banner {
            ui.add_space(SPACE_2);
            ui.colored_label(p.info_fg, msg);
        }
        if self.schedule_prefs.qos_banner_active {
            ui.add_space(SPACE_2);
            let eff_ms = self.schedule_prefs.effective().min_interval_ms.to_string();
            let notice_key = if self.schedule_prefs.lock_interval {
                "gui.schedule.qos.locked_notice"
            } else {
                "gui.schedule.qos.backoff_notice"
            };
            render_admonition(ui, p.info_bg, p.info_fg, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    ui.label(
                        RichText::new(translate(locale, notice_key, &[&eff_ms]))
                            .size(FONT_META)
                            .color(p.text_primary),
                    );
                    if !self.schedule_prefs.lock_interval
                        && btn_outlined(
                            ui,
                            p,
                            &translate(locale, "gui.schedule.qos.lock_now", &[]),
                            true,
                        )
                        .clicked()
                    {
                        self.schedule_prefs.lock_current();
                    }
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.schedule.qos.reset_baseline", &[]),
                        true,
                    )
                    .clicked()
                    {
                        self.schedule_prefs.reset_to_baseline();
                    }
                });
            });
        }
        if let Some(run) = run {
            use parallaxe_core::contracts::Termination as T;
            let msg_key = match run.termination() {
                Some(T::Cancelled) => Some("gui.common.terminated_cancelled"),
                Some(T::Failed) => Some("gui.common.terminated_failed"),
                Some(T::GlobalTimeout) => Some("gui.common.terminated_timeout"),
                Some(T::Partial) => Some("gui.common.terminated_partial"),
                Some(T::Complete) | None => None,
            };
            if let Some(key) = msg_key {
                ui.add_space(SPACE_2);
                ui.colored_label(p.warning_fg, translate(locale, key, &[]));
            }
            if let Some(err) = run.errors.last() {
                ui.add_space(SPACE_1);
                ui.colored_label(
                    p.danger_fg,
                    translate(locale, "gui.common.error_prefix", &[&err.message_safe]),
                );
            }
        }
        let effective_scope = run
            .and_then(|rv| rv.report.as_ref())
            .and_then(|rep| rep.scope)
            .or_else(|| {
                parallaxe_core::contracts::ObservationScope::for_platform(std::env::consts::OS)
            });
        if matches!(
            effective_scope,
            Some(parallaxe_core::contracts::ObservationScope::AndroidSimulatedUser)
        ) {
            ui.add_space(SPACE_2);
            render_admonition(ui, p.info_bg, p.info_fg, |ui| {
                ui.horizontal_wrapped(|ui| {
                    render_pill(
                        ui,
                        &translate(locale, "gui.scope.android.badge", &[]),
                        p.info_fg,
                        p.card,
                        p.info_fg,
                    );
                    ui.label(
                        RichText::new(translate(locale, "gui.scope.android.desc", &[]))
                            .size(FONT_META)
                            .color(p.text_primary),
                    );
                });
            });
        }
        ui.add_space(SPACE_4);
        match self.active_tab {
            ActiveTab::Overview | ActiveTab::Services => {
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                settings_plugins::render_plugin_suggestions_strip(ui, &self.plugin_host, p, locale);
                if self.active_tab == ActiveTab::Overview {
                    let check_outcome = check_panel::render_check_card(
                        ui,
                        &self.registry,
                        &mut self.default_bundle,
                        &mut self.selected_profile,
                        &mut self.schedule_prefs,
                        &mut self.pace_panel_open,
                        &mut self.group_search,
                        &mut self.show_all_groups,
                        &mut self.checked_targets,
                        self.pending,
                        self.state.active_run().is_some(),
                        self.density,
                        self.theme,
                        locale,
                        spec.tier,
                        self.show_technical_details,
                        actions,
                    );
                    if check_outcome.profile_changed {
                        self.preview_profile = self.selected_profile.clone();
                    }
                    if check_outcome.schedule_changed {
                        self.persist_current_settings();
                    }
                    ui.add_space(SPACE_4);
                }
                let run = self.state.active_run();
                if self.active_tab == ActiveTab::Overview && self.density == DensityMode::Minimal {
                    overview_minimal::render(
                        ui,
                        run,
                        self.theme,
                        locale,
                        spec.tier,
                        self.show_technical_details,
                        actions,
                    );
                } else {
                    if let Some(r) = run {
                        service_matrix::finding(
                            ui,
                            r,
                            self.theme,
                            locale,
                            self.show_technical_details,
                            actions,
                        );
                    }
                    overview_normal::metrics(ui, p, run, locale, spec.summary_columns);
                    ui.add_space(SPACE_4);
                    service_matrix::render(
                        ui,
                        &self.registry,
                        &mut self.state,
                        self.theme,
                        spec.tier,
                        &mut self.table_density,
                        self.show_technical_details,
                        self.show_ip,
                        &mut self.matrix_group_filter,
                        actions,
                    );
                    if self.active_tab == ActiveTab::Overview {
                        ui.add_space(SPACE_4);
                        overview_normal::context(ui, p, self.state.active_run(), locale, actions);
                    }
                }
                if self.state.active_run().is_none() {
                    ui.add_space(SPACE_4);
                    card_frame(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(translate(
                                        locale,
                                        "gui.common.demo_card_title",
                                        &[],
                                    ))
                                    .size(FONT_TITLE)
                                    .strong()
                                    .color(p.text_primary),
                                );
                                ui.add_space(SPACE_1);
                                ui.label(
                                    RichText::new(translate(
                                        locale,
                                        "gui.common.demo_card_desc",
                                        &[],
                                    ))
                                    .size(FONT_BODY)
                                    .color(p.text_secondary),
                                );
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if btn_tonal(
                                        ui,
                                        p,
                                        &translate(locale, "gui.common.demo_card_btn", &[]),
                                        true,
                                    )
                                    .clicked()
                                    {
                                        actions.push(UiAction::RunScenario(
                                            OfflineScenario::BooksAndSharedCdnGuard,
                                        ));
                                    }
                                },
                            );
                        });
                    });
                }
                if spec.tier == ViewportTier::Narrow {
                    ui.add_space(SPACE_3);
                    if ui
                        .link(translate(locale, "gui.common.view_coverage_link", &[]))
                        .clicked()
                    {
                        actions.push(UiAction::SwitchTab(ActiveTab::Coverage));
                    }
                }
            }
            ActiveTab::Coverage => coverage_panel::render(
                ui,
                &self.state,
                self.theme,
                spec.tier,
                &mut self.only_selected_uncovered,
                actions,
            ),
            ActiveTab::Guard => {
                if let Some(draft) = self.draft.as_mut() {
                    let run = self.state.active_run();
                    let rec = run.and_then(|rv| rv.result_for(&draft.target));
                    draft.render(ui, p, run, rec);
                    if draft.closed {
                        self.draft = None;
                    }
                    ui.add_space(SPACE_4);
                }
                guard_panel::render(
                    ui,
                    &self.registry,
                    &self.state,
                    &mut self.guard_view,
                    self.resolved_link_report.as_ref(),
                    self.show_ip,
                    self.theme,
                    self.show_technical_details,
                    actions,
                );
            }
            ActiveTab::Timeline => timeline_view::render(
                ui,
                &self.state,
                self.theme,
                &mut self.export_path_input,
                self.export_preview.as_ref().map(|(f, t)| (*f, t.as_str())),
                self.export_status.as_deref(),
                self.show_technical_details,
                actions,
            ),
            ActiveTab::Import => import_panel::render(
                ui,
                &mut self.registry,
                &mut self.import_view,
                self.theme,
                locale,
                spec.tier,
                self.show_technical_details,
                actions,
            ),
            ActiveTab::Settings => self.settings(ui, p, spec.tier, actions),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            ActiveTab::PluginPage => settings_plugins::render_active_plugin_page(
                ui,
                &self.plugin_host,
                p,
                locale,
                actions,
            ),
        }
    }

    fn settings(
        &mut self,
        ui: &mut Ui,
        p: Palette,
        tier: ViewportTier,
        actions: &mut Vec<UiAction>,
    ) {
        let locale = self.state.current_locale();
        self.guard_view.ensure_pending_audits_scanned();
        if !self.guard_view.pending_audits.is_empty() {
            guard_panel::render_pending_audit_banner(
                ui,
                &mut self.guard_view,
                locale,
                p,
                self.show_technical_details,
            );
            ui.add_space(SPACE_4);
        }
        // 多级设置：一级栏目列表 → 二级独立页（语言项例外，保留在一级页直选）。
        if self.settings_page == settings_page::SettingsPage::Root {
            settings_page::render_root(ui, p, locale, &mut self.settings_page, actions);
            return;
        }
        settings_page::render_header(ui, p, locale, &mut self.settings_page);
        let page = self.settings_page;
        // 1. 外观与主题：主题 / 密度 / 显示本地 IP / 技术细节 / 持久化状态
        if page == settings_page::SettingsPage::Appearance {
            card_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(translate(locale, "gui.settings.appearance", &[]))
                            .strong()
                            .size(FONT_TITLE)
                            .color(p.text_primary),
                    );
                    let persist_key = if self.settings_path.is_some() {
                        "gui.settings.persistence_active"
                    } else {
                        "gui.settings.persistence_memory"
                    };
                    render_pill(
                        ui,
                        &translate(locale, persist_key, &[]),
                        p.text_secondary,
                        p.card_alt,
                        p.border,
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .link(translate(locale, "gui.status.about", &[]))
                            .clicked()
                        {
                            actions.push(UiAction::ToggleAboutModal);
                        }
                    });
                });
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_2;
                    ui.label(
                        RichText::new(translate(locale, "gui.settings.density", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                    for mode in [DensityMode::Normal, DensityMode::Minimal] {
                        if ui
                            .selectable_label(self.density == mode, mode.label_localized(locale))
                            .clicked()
                        {
                            actions.push(UiAction::SetDensity(mode));
                        }
                    }
                    ui.add_space(SPACE_4);
                    ui.label(
                        RichText::new(translate(locale, "gui.settings.theme", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                    for mode in [ThemeMode::Dark, ThemeMode::Light] {
                        if ui
                            .selectable_label(self.theme == mode, mode.label_localized(locale))
                            .clicked()
                        {
                            actions.push(UiAction::SetTheme(mode));
                        }
                    }
                    ui.add_space(SPACE_4);
                    if ui
                        .checkbox(
                            &mut self.show_ip,
                            translate(locale, "gui.settings.show_ip", &[]),
                        )
                        .changed()
                        && !self.show_ip
                    {
                        self.export_preview = None;
                        self.resolved_link_report = None;
                    }
                    ui.add_space(SPACE_2);
                    ui.checkbox(
                        &mut self.show_technical_details,
                        translate(locale, "gui.settings.show_tech", &[]),
                    )
                    .on_hover_text(translate(
                        locale,
                        "gui.settings.show_tech_tip",
                        &[],
                    ));
                });
                if self.show_ip {
                    ui.add_space(SPACE_1);
                    ui.add(
                        egui::Label::new(
                            RichText::new(translate(locale, "gui.settings.show_ip_note", &[]))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        )
                        .wrap(),
                    );
                }
                if let Some(err) = self.settings_error.clone() {
                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            p.danger_fg,
                            translate(locale, "gui.settings.persistence_error", &[&err]),
                        );
                        if self.settings_path.is_some()
                            && btn_outlined(
                                ui,
                                p,
                                &translate(locale, "gui.settings.repair_btn", &[]),
                                true,
                            )
                            .clicked()
                        {
                            match settings_store::repair(self.settings_path.as_deref()) {
                                Ok(recovered) => {
                                    self.settings = recovered;
                                    self.registry.set_distribution_mode(
                                        self.registry.distribution_mode()
                                            || self.settings.distribution_mode,
                                    );
                                    self.settings.distribution_mode =
                                        self.registry.distribution_mode();
                                    self.bus.send(GuiCommand::SyncRegistry {
                                        registry: Box::new(self.registry.clone()),
                                    });
                                    self.settings_error = None;
                                    self.settings_notice =
                                        Some(translate(locale, "gui.settings.repaired_ok", &[]));
                                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                                    self.plugin_host.sync_from_settings(&self.settings);
                                }
                                Err(repair_err) => {
                                    self.settings_error = Some(repair_err);
                                }
                            }
                        }
                    });
                }
                if let Some(msg) = &self.settings_notice {
                    ui.add_space(SPACE_1);
                    ui.colored_label(p.ok_fg, msg);
                }
            });
            ui.add_space(SPACE_4);
            return;
        }
        // 2. 检测范围与默认 profile + 离线演练
        if page == settings_page::SettingsPage::Scope {
            let mut profile_changed = false;
            let registry_ref = &self.registry;
            let draw_scope_card = |ui: &mut Ui,
                                   default_profile: &mut String,
                                   selected_profile: &mut String,
                                   preview_profile: &mut String,
                                   profile_changed: &mut bool,
                                   ipv4_only: &mut bool,
                                   fast_mode: &mut bool,
                                   actions: &mut Vec<UiAction>| {
                card_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = SPACE_3;
                        ui.label(
                            RichText::new(translate(locale, "gui.settings.scope", &[]))
                                .strong()
                                .size(FONT_TITLE)
                                .color(p.text_primary),
                        );
                        ui.checkbox(ipv4_only, translate(locale, "gui.settings.ipv4_only", &[]));
                        ui.checkbox(fast_mode, translate(locale, "gui.settings.fast_mode", &[]));
                    });
                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                        ui.label(
                            RichText::new(translate(locale, "gui.settings.default_profile", &[]))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                        for code in check_panel::PRIMARY_PROFILES {
                            let lbl =
                                check_panel::profile_display_label(registry_ref, code, locale);
                            if ui.selectable_label(*default_profile == code, lbl).clicked() {
                                *default_profile = code.to_string();
                                *selected_profile = code.to_string();
                                *preview_profile = code.to_string();
                                *profile_changed = true;
                            }
                        }
                    });
                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                        for (key, act) in [
                            (
                                "gui.settings.btn.simple",
                                UiAction::RunProfile("simple".into()),
                            ),
                            (
                                "gui.settings.btn.google_all",
                                UiAction::RunProfile("google-all".into()),
                            ),
                            ("gui.settings.btn.full", UiAction::RunFull),
                            ("gui.settings.btn.catalog", UiAction::ToggleCatalogDrawer),
                        ] {
                            if btn_outlined(ui, p, &translate(locale, key, &[]), true).clicked() {
                                actions.push(act);
                            }
                        }
                    });
                });
            };
            let draw_drill_card = |ui: &mut Ui, actions: &mut Vec<UiAction>| {
                card_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(translate(locale, "gui.settings.offline_drill", &[]))
                                .strong()
                                .size(FONT_TITLE)
                                .color(p.text_primary),
                        );
                        ui.label(
                            RichText::new(translate(locale, "gui.settings.drill_hint", &[]))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                    });
                    ui.add_space(SPACE_2);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                        demo_picker(ui, false, locale, actions);
                        demo_picker(ui, true, locale, actions);
                        if btn_tonal(
                            ui,
                            p,
                            &translate(locale, "gui.settings.run_selftest", &[]),
                            true,
                        )
                        .clicked()
                        {
                            actions.push(UiAction::RunSelftest);
                        }
                    });
                });
            };
            if ui.available_width() >= 760.0 {
                spaced_columns(ui, 2, SPACE_4, |col_ui, idx| {
                    if idx == 0 {
                        draw_scope_card(
                            col_ui,
                            &mut self.default_profile,
                            &mut self.selected_profile,
                            &mut self.preview_profile,
                            &mut profile_changed,
                            &mut self.ipv4_only,
                            &mut self.fast_mode,
                            actions,
                        );
                    } else {
                        draw_drill_card(col_ui, actions);
                    }
                });
            } else {
                draw_scope_card(
                    ui,
                    &mut self.default_profile,
                    &mut self.selected_profile,
                    &mut self.preview_profile,
                    &mut profile_changed,
                    &mut self.ipv4_only,
                    &mut self.fast_mode,
                    actions,
                );
                ui.add_space(SPACE_4);
                draw_drill_card(ui, actions);
            }
            if profile_changed {
                self.persist_current_settings();
            }
            ui.add_space(SPACE_4);
            return;
        }

        // 2.5 AI 自主调试（任务 A：路径 A F3 一键授权 + 路径 B 会话级「自主检测」开关，默认关闭不落盘）
        if page == settings_page::SettingsPage::AiDebug {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            let bridge_dir = self.ai_bridge.paths().root().to_string_lossy().to_string();
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            let bridge_view = Some(ai_debug_panel::AiBridgeView {
                imported_targets: self
                    .registry
                    .entries()
                    .iter()
                    .filter(|entry| self.registry.is_user_imported(entry.id()))
                    .count(),
                distribution_mode: self.registry.distribution_mode(),
                bridge_dir: Some(bridge_dir.as_str()),
                last_error: self.ai_bridge_error(),
            });
            #[cfg(any(target_os = "android", target_os = "ios"))]
            let bridge_view = None;
            ai_debug_panel::render_settings_card(
                ui,
                &self.ai_debug,
                p,
                locale,
                self.show_technical_details,
                bridge_view,
                actions,
            );
            ui.add_space(SPACE_4);
            return;
        }

        // 3. T-7-11：检测节奏与网络环境 + 运行前耗时与连接预估（与检测首页共享同一份 SchedulePrefs）
        if page == settings_page::SettingsPage::Advanced {
            ui.add_space(SPACE_4);
            let mut schedule_changed = false;
            card_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.schedule.title", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_1);
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.schedule.subtitle", &[]))
                            .size(FONT_META)
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(SPACE_3);

                if check_panel::render_pace_controls(
                    ui,
                    &mut self.schedule_prefs,
                    p,
                    locale,
                    tier,
                    true,
                    self.show_technical_details,
                    "settings_pace",
                ) {
                    schedule_changed = true;
                }

                // 运行前耗时与连接预估（直接调用 parallaxe_core::runtime::network_plan，零发包）
                ui.add_space(SPACE_3);
                let eff = self.schedule_prefs.effective();
                let mut preview_opts = CheckOptions {
                    profile: if self.preview_profile == "full" {
                        None
                    } else {
                        Some(self.preview_profile.clone())
                    },
                    full: self.preview_profile == "full",
                    ipv4: self.ipv4_only,
                    fast: self.fast_mode,
                    ..Default::default()
                };
                self.schedule_prefs.apply_to_options(&mut preview_opts);

                subtle_well_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                        ui.label(
                            RichText::new(translate(locale, "gui.schedule.preview.title", &[]))
                                .strong()
                                .size(FONT_BODY)
                                .color(p.text_primary),
                        );
                        render_pill(
                            ui,
                            &translate(locale, "gui.schedule.preview.zero_pill", &[]),
                            p.ok_fg,
                            p.ok_bg,
                            p.ok_fg,
                        );
                        for code in ["simple", "google-all", "full"] {
                            let lbl =
                                check_panel::profile_display_label(&self.registry, code, locale);
                            if ui
                                .selectable_label(self.preview_profile == code, lbl)
                                .clicked()
                            {
                                self.preview_profile = code.to_string();
                            }
                        }
                    });
                    ui.add_space(SPACE_2);

                    match runtime::network_plan(&self.registry, &preview_opts) {
                        Ok(plan) => {
                            kv_row(
                                ui,
                                p,
                                if locale == Locale::EnUs {
                                    "Pace Source"
                                } else {
                                    "当前节奏"
                                },
                                84.0,
                                eff.source_text(locale),
                                false,
                            );
                            let sched_summary = if locale == Locale::EnUs {
                                format!(
                                    "Interval {} ms · Batch <= {} · Cooldown {} s",
                                    plan.min_interval_ms,
                                    plan.batch_size,
                                    plan.batch_cooldown_ms / 1000
                                )
                            } else {
                                format!(
                                    "间隔 {} ms · 每批 ≤{} · 批次冷却 {} s",
                                    plan.min_interval_ms,
                                    plan.batch_size,
                                    plan.batch_cooldown_ms / 1000
                                )
                            };
                            kv_row(
                                ui,
                                p,
                                if locale == Locale::EnUs {
                                    "Schedule"
                                } else {
                                    "生效参数"
                                },
                                84.0,
                                &sched_summary,
                                true,
                            );
                            let est_summary = if locale == Locale::EnUs {
                                format!(
                                    "{} targets · ~{} new connections · ~{} s{}",
                                    plan.target_count,
                                    plan.estimated_new_connections,
                                    plan.estimated_wall_ms.div_ceil(1000),
                                    if plan.requires_confirmation {
                                        " (extra confirmation required for large batch)"
                                    } else {
                                        ""
                                    }
                                )
                            } else {
                                format!(
                                    "目标 {} 项 · 预计新建连接 {} · 预计耗时 ~{} s{}",
                                    plan.target_count,
                                    plan.estimated_new_connections,
                                    plan.estimated_wall_ms.div_ceil(1000),
                                    if plan.requires_confirmation {
                                        "（数量较多，执行前会再次确认）"
                                    } else {
                                        ""
                                    }
                                )
                            };
                            kv_row(
                                ui,
                                p,
                                if locale == Locale::EnUs {
                                    "Estimate"
                                } else {
                                    "预计连接"
                                },
                                84.0,
                                &est_summary,
                                false,
                            );
                        }
                        Err(err) => {
                            ui.colored_label(p.warning_fg, err.localized(locale));
                        }
                    }
                });

                if !self.schedule_prefs.adjustments.is_empty() {
                    ui.add_space(SPACE_2);
                    ui.collapsing(
                        translate(
                            locale,
                            "gui.settings.slowdown_title",
                            &[&self.schedule_prefs.adjustments.len().to_string()],
                        ),
                        |ui| {
                            for (idx, adj) in self.schedule_prefs.adjustments.iter().enumerate() {
                                let reason_text = if self.show_technical_details {
                                    adj.reason.clone()
                                } else {
                                    translate(locale, "gui.settings.slowdown_reason", &[])
                                };
                                let line = translate(
                                    locale,
                                    "gui.settings.slowdown_line",
                                    &[
                                        &adj.from_ms.to_string(),
                                        &adj.to_ms.to_string(),
                                        &reason_text,
                                    ],
                                );
                                let label = if self.show_technical_details {
                                    RichText::new(format!("{} · {}", adj.run_id, line))
                                        .monospace()
                                        .size(FONT_META)
                                } else {
                                    RichText::new(format!("{}. {}", idx + 1, line))
                                        .size(FONT_META)
                                        .color(p.text_secondary)
                                };
                                ui.label(label);
                            }
                        },
                    );
                }
            });
            if schedule_changed {
                self.persist_current_settings();
            }
            ui.add_space(SPACE_4);
            let mut network_changed = false;
            card_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.settings.network", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_1);
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.settings.proxy_auto_note", &[]))
                            .size(FONT_META)
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    ui.label(
                        RichText::new(translate(locale, "gui.settings.proxy_mode", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                    for (mode, key) in [
                        (parallaxe_core::ProxyMode::Auto, "gui.settings.proxy_auto"),
                        (parallaxe_core::ProxyMode::None, "gui.settings.proxy_none"),
                        (parallaxe_core::ProxyMode::Http, "gui.settings.proxy_http"),
                        (parallaxe_core::ProxyMode::Socks, "gui.settings.proxy_socks"),
                        (parallaxe_core::ProxyMode::Pac, "gui.settings.proxy_pac"),
                    ] {
                        if ui
                            .selectable_label(self.proxy_mode == mode, translate(locale, key, &[]))
                            .clicked()
                        {
                            self.proxy_mode = mode;
                            network_changed = true;
                        }
                    }
                });
                match self.proxy_mode {
                    parallaxe_core::ProxyMode::Http => {
                        ui.add_space(SPACE_1);
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.proxy_input)
                                    .hint_text("http://127.0.0.1:7890")
                                    .desired_width(320.0),
                            );
                            ui.label(
                                RichText::new(translate(
                                    locale,
                                    "gui.settings.proxy_http_hint",
                                    &[],
                                ))
                                .size(FONT_META)
                                .color(p.text_secondary),
                            );
                        });
                    }
                    parallaxe_core::ProxyMode::Socks => {
                        ui.add_space(SPACE_1);
                        ui.label(
                            RichText::new(translate(locale, "gui.settings.proxy_socks_note", &[]))
                                .size(FONT_META)
                                .color(p.warning_fg),
                        );
                    }
                    parallaxe_core::ProxyMode::Pac => {
                        ui.add_space(SPACE_1);
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.pac_input)
                                    .hint_text("http://127.0.0.1:9090/proxy.pac")
                                    .desired_width(320.0),
                            );
                            ui.label(
                                RichText::new(translate(
                                    locale,
                                    "gui.settings.proxy_pac_hint",
                                    &[],
                                ))
                                .size(FONT_META)
                                .color(p.text_secondary),
                            );
                        });
                        ui.add_space(SPACE_1);
                        ui.label(
                            RichText::new(translate(locale, "gui.settings.proxy_pac_note", &[]))
                                .size(FONT_META)
                                .color(p.text_secondary),
                        );
                    }
                    _ => {}
                }
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    ui.label(
                        RichText::new(translate(locale, "gui.settings.max_duration", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                    for (key, value) in [
                        ("gui.settings.duration_unlimited", 0u64),
                        ("gui.settings.duration_30s", 30_000),
                        ("gui.settings.duration_60s", 60_000),
                        ("gui.settings.duration_2m", 120_000),
                    ] {
                        if ui
                            .selectable_label(
                                self.max_duration_ms == value,
                                translate(locale, key, &[]),
                            )
                            .clicked()
                        {
                            self.max_duration_ms = value;
                            network_changed = true;
                        }
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut self.max_duration_ms)
                                .range(0..=120_000)
                                .speed(1_000.0)
                                .suffix(" ms"),
                        )
                        .changed()
                    {
                        network_changed = true;
                    }
                });
                ui.add_space(SPACE_1);
                ui.label(
                    RichText::new(translate(locale, "gui.settings.duration_hint", &[]))
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                if let Some(label) = self
                    .state
                    .active_run()
                    .and_then(|run| run.report.as_ref())
                    .and_then(|report| report.results.first())
                    .map(|result| result.path.proxy_mode.clone())
                {
                    ui.add_space(SPACE_1);
                    ui.label(
                        RichText::new(format!(
                            "{}：{}",
                            translate(locale, "gui.settings.proxy_last", &[]),
                            proxy_path_text(locale, &label)
                        ))
                        .size(FONT_META)
                        .color(p.text_secondary),
                    );
                }
                let _ = network_changed;
            });
            ui.add_space(SPACE_4);
            default_bundle_panel::render_revoke_card(ui, &self.registry, locale, p, actions);
            ui.add_space(SPACE_4);
            return;
        }

        // 4. 导出与阻断默认项
        if page == settings_page::SettingsPage::Rules {
            let mut rule_fmt_changed = false;
            card_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.settings.rule_format_title", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    ui.label(
                        RichText::new(translate(locale, "gui.settings.rule_format_label", &[]))
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                    for (fmt, label) in [
                        (RuleFormat::Clash, "Clash / mihomo"),
                        (RuleFormat::SingBox, "sing-box"),
                        (RuleFormat::DomainList, "DomainList"),
                    ] {
                        if ui
                            .selectable_label(self.guard_view.export_format == fmt, label)
                            .clicked()
                        {
                            self.guard_view.export_format = fmt;
                            rule_fmt_changed = true;
                        }
                    }
                });
                ui.add_space(SPACE_1);
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.settings.rule_format_note", &[]))
                            .size(FONT_META)
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.settings.btn.import_nav", &[]),
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::SwitchTab(ActiveTab::Import));
                    }
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.settings.btn.history_nav", &[]),
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::SwitchTab(ActiveTab::Timeline));
                    }
                    ui.hyperlink_to(
                        RichText::new(translate(locale, "gui.guard.docs_link", &[]))
                            .size(FONT_META)
                            .color(p.accent),
                        guard_panel::SELF_BLOCK_DOCS_URL,
                    );
                });
            });
            if rule_fmt_changed {
                self.persist_current_settings();
            }
            ui.add_space(SPACE_4);
            return;
        }

        // 5. 扩展插件（仅桌面端显示，移动端零插件面）
        if page == settings_page::SettingsPage::Notices {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                let delta = settings_plugins::render_plugins_card(
                    ui,
                    &mut self.plugin_host,
                    &mut self.settings,
                    p,
                    locale,
                    actions,
                );
                if let Some(err) = delta.persist_error {
                    self.settings_error = Some(err);
                }
                if delta.settings_changed {
                    self.persist_current_settings();
                }
                if delta.re_invoke {
                    self.refresh_plugins(None);
                }
                ui.add_space(SPACE_4);
            }
            card_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.settings.notices_title", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_1);
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.settings.notices_note", &[]))
                            .size(FONT_META)
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    {
                        if btn_outlined(
                            ui,
                            p,
                            &translate(locale, "gui.settings.view_dep_notices", &[]),
                            true,
                        )
                        .clicked()
                        {
                            self.plugin_host.dep_notices_open = true;
                        }
                    }
                    #[cfg(any(target_os = "android", target_os = "ios"))]
                    {
                        ui.add(
                            egui::Label::new(
                                RichText::new(translate(
                                    locale,
                                    "gui.settings.notices_mobile",
                                    &[],
                                ))
                                .size(FONT_META)
                                .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                });
            });
            ui.add_space(SPACE_4);
            return;
        }

        // 本地地址数据：地址簿 + 预期对端国家（只在本机使用，不上传）
        if page == settings_page::SettingsPage::AddressBook {
            card_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.address.book_title", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_1);
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.address.rows", &[]))
                            .size(FONT_META)
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(SPACE_1);
                ui.add(
                    egui::TextEdit::multiline(&mut self.address_rows)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text("192.0.2.0/24,US,64496\n203.0.113.0/24,CN,64497"),
                );
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    ui.label(
                        RichText::new(translate(locale, "gui.address.expected", &[]))
                            .size(FONT_META)
                            .color(p.text_secondary),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.expected_peer_country)
                            .desired_width(96.0)
                            .hint_text("US"),
                    );
                    if btn_tonal(ui, p, &translate(locale, "gui.address.apply", &[]), true)
                        .clicked()
                    {
                        let expected =
                            parallaxe_core::normalize_country(&self.expected_peer_country);
                        match parallaxe_core::destination::AddressBook::parse(&self.address_rows) {
                            Ok(book)
                                if expected.is_some()
                                    || self.expected_peer_country.trim().is_empty() =>
                            {
                                self.bus
                                    .send(GuiCommand::ConfigureAddresses(book, expected));
                                self.state.last_command_error = None;
                                self.address_feedback =
                                    Some(translate(locale, "gui.address.applied", &[]));
                            }
                            _ => {
                                let err = translate(locale, "gui.address.invalid", &[]);
                                self.state.last_command_error = Some(err.clone());
                                self.address_feedback = Some(err);
                            }
                        }
                    }
                    if btn_outlined(
                        ui,
                        p,
                        &translate(locale, "gui.address.fill_sample", &[]),
                        true,
                    )
                    .clicked()
                    {
                        self.address_rows =
                            "192.0.2.0/24,US,64496\n203.0.113.0/24,CN,64497".to_string();
                        self.expected_peer_country = "US".to_string();
                        self.address_feedback = None;
                    }
                });
                if let Some(msg) = &self.address_feedback {
                    ui.add_space(SPACE_1);
                    let color = if self.state.last_command_error.is_some() {
                        p.danger_fg
                    } else {
                        p.ok_fg
                    };
                    ui.add(
                        egui::Label::new(RichText::new(msg).size(FONT_META).color(color)).wrap(),
                    );
                }
            });
            ui.add_space(SPACE_4);
            return;
        }

        // 6. 关于与第三方声明
        if page == settings_page::SettingsPage::About {
            card_frame(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(translate(locale, "gui.settings.about_title", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_3, SPACE_1);
                    ui.label(
                        RichText::new(format!(
                            "{}: {}",
                            translate(locale, "gui.settings.about_version", &[]),
                            parallaxe_core::VERSION
                        ))
                        .monospace()
                        .size(FONT_META)
                        .color(p.text_primary),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}: MIT OR Apache-2.0",
                            translate(locale, "gui.settings.about_license", &[])
                        ))
                        .size(FONT_META)
                        .color(p.text_secondary),
                    );
                });
                ui.add_space(SPACE_1);
                ui.add(
                    egui::Label::new(
                        RichText::new(translate(locale, "gui.settings.about_privacy", &[]))
                            .size(FONT_META)
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(SPACE_2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SPACE_2, SPACE_1);
                    if btn_outlined(ui, p, &translate(locale, "gui.status.about", &[]), true)
                        .clicked()
                    {
                        actions.push(UiAction::ToggleAboutModal);
                    }
                });
            });
            ui.add_space(SPACE_4);
            distribution_mode_panel::render_source_mode_card(
                ui,
                &self.registry,
                locale,
                p,
                actions,
            );
            if let Some(msg) = &self.settings_notice {
                ui.add_space(SPACE_2);
                ui.label(RichText::new(msg).size(FONT_META).color(p.ok_fg));
            }
        }
    }
}
/// 把 `path.proxy_mode` 机器码翻成人话（不改变报告字段本身）。
fn proxy_path_text(locale: Locale, label: &str) -> String {
    let key = match label {
        "DIRECT_SELECTED" => "gui.settings.path_direct",
        "PROXY_SELECTED" => "gui.settings.path_proxy",
        "HTTP" => "gui.settings.path_http",
        "PAC" => "gui.settings.path_pac",
        _ => "gui.settings.path_system",
    };
    translate(locale, key, &[])
}

fn tab_icon(tab: ActiveTab) -> IconKind {
    match tab {
        ActiveTab::Overview => IconKind::Overview,
        ActiveTab::Services => IconKind::Services,
        ActiveTab::Coverage => IconKind::Coverage,
        ActiveTab::Guard => IconKind::Guard,
        ActiveTab::Timeline => IconKind::Timeline,
        ActiveTab::Import => IconKind::Globe,
        ActiveTab::Settings => IconKind::Settings,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        ActiveTab::PluginPage => IconKind::Plugin,
    }
}
fn demo_picker(ui: &mut Ui, fixture: bool, locale: Locale, actions: &mut Vec<UiAction>) {
    let id = ui.make_persistent_id(("demo_picker", fixture));
    let label_key = if fixture {
        "gui.settings.btn.nine_fixtures"
    } else {
        "gui.settings.btn.six_demos"
    };
    let response = ui.button(translate(locale, label_key, &[]));
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    egui::popup_below_widget(
        ui,
        id,
        &response,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_max_width(300.);
            ui.set_min_width(210.);
            egui::ScrollArea::vertical()
                .max_height(310.)
                .show(ui, |ui| {
                    if fixture {
                        for case in fixtures::cases() {
                            if ui.button(case.id).on_hover_text(case.description).clicked() {
                                actions.push(UiAction::RunFixture(case.id.into()));
                                ui.memory_mut(|m| m.close_popup());
                            }
                        }
                        ui.separator();
                        for (diag_id, desc) in [
                            ("address-mismatch", "地址落点与解析不一致示例（US→CN）"),
                            ("address-proxy", "显式代理分层落点示例（不误报）"),
                            ("product-restricted", "产品明确地区受限示例（Restricted）"),
                            ("product-blocked", "产品明确政策阻断示例（Blocked）"),
                        ] {
                            if ui.button(diag_id).on_hover_text(desc).clicked() {
                                actions.push(UiAction::RunFixture(diag_id.into()));
                                ui.memory_mut(|m| m.close_popup());
                            }
                        }
                    } else {
                        for sc in OfflineScenario::ALL {
                            if ui.button(sc.label()).clicked() {
                                actions.push(UiAction::RunScenario(sc));
                                ui.memory_mut(|m| m.close_popup());
                            }
                        }
                    }
                });
        },
    );
}
impl eframe::App for ParallaxeGuiApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // AI 授权桥的状态投影与请求轮询依赖 render_ui → poll_ai_bridge。
        // eframe 仅在窗口重绘时调用 update；窗口失焦/被遮挡时若不主动请求重绘，
        // status.json 会超过 agent 侧的 120s TTL，导致 AI 请求收到 BRIDGE_UNAVAILABLE。
        // 每秒请求一次重绘，让桥状态在无用户交互时也保持新鲜（开销极小，不发包）。
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        self.render_ui(ctx);
    }
}

#[cfg(test)]
mod rework_tests {
    use super::*;
    use crate::commands::build_offline_scenario_events;
    use egui::{Event as Input, Pos2};

    fn frame(
        app: &mut ParallaxeGuiApp,
        ctx: &egui::Context,
        w: f32,
        events: Vec<Input>,
    ) -> Vec<(String, Pos2)> {
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(w, 820.))),
                events,
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        out.shapes
            .into_iter()
            .filter_map(|e| {
                if let egui::Shape::Text(t) = e.shape {
                    let pos = t.pos + t.galley.size() * 0.5;
                    e.clip_rect
                        .contains(pos)
                        .then(|| (t.galley.text().to_string(), pos))
                } else {
                    None
                }
            })
            .collect()
    }
    fn click(pos: Pos2) -> Vec<Input> {
        let mut events = vec![Input::PointerMoved(pos)];
        for pressed in [true, false] {
            events.push(Input::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        events
    }
    fn find(labels: &[(String, Pos2)], s: &str) -> Pos2 {
        labels
            .iter()
            .find(|(l, _)| l == s)
            .unwrap_or_else(|| panic!("不可见：{s}"))
            .1
    }
    #[test]
    fn pickers_require_click_close_on_escape_and_outside_and_switch_directly() {
        let ctx = egui::Context::default();
        let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
        app.navigate(ActiveTab::Settings);
        app.open_settings_page(settings_page::SettingsPage::Scope);
        frame(&mut app, &ctx, 1280., vec![]);
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        let six = find(&labels, "六类契约演示");
        let nine = find(&labels, "九项内置样本");
        let outside = find(&labels, "工作台设置");
        frame(&mut app, &ctx, 1280., vec![Input::PointerMoved(six)]);
        assert!(!ctx.memory(|m| m.any_popup_open()));
        frame(&mut app, &ctx, 1280., click(six));
        assert!(ctx.memory(|m| m.any_popup_open()));
        frame(&mut app, &ctx, 1280., vec![]);
        frame(&mut app, &ctx, 1280., click(nine));
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        find(&labels, "gemini-positive");
        assert!(!labels
            .iter()
            .any(|(s, _)| s == OfflineScenario::DriveLoginHandoff.label()));
        frame(
            &mut app,
            &ctx,
            1280.,
            vec![Input::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(!ctx.memory(|m| m.any_popup_open()));
        frame(&mut app, &ctx, 1280., click(six));
        frame(&mut app, &ctx, 1280., vec![]);
        frame(&mut app, &ctx, 1280., click(outside));
        assert!(!ctx.memory(|m| m.any_popup_open()));
    }
    #[tokio::test]
    async fn only_new_terminal_routes_to_results_and_new_run_clears_private_view() {
        let ctx = egui::Context::default();
        let reg = Registry::embedded().unwrap();
        let events = build_offline_scenario_events(
            &reg,
            OfflineScenario::BooksAndSharedCdnGuard,
            "run-synthetic",
        )
        .await
        .unwrap();
        let mut app = ParallaxeGuiApp::new(reg.clone(), ctx.clone());
        app.navigate(ActiveTab::Settings);
        for e in &events {
            app.apply_event(e);
        }
        assert_eq!(app.active_tab, ActiveTab::Services);
        app.dispatch_actions(&ctx, vec![UiAction::OpenPlan("google.books".into())]);
        app.apply_event(events.last().unwrap());
        assert_eq!(app.active_tab, ActiveTab::Guard);
        assert!(app.draft.is_some());
        app.show_ip = true;
        let next =
            build_offline_scenario_events(&reg, OfflineScenario::MidRunCancelled, "run-cancel")
                .await
                .unwrap();
        for e in next {
            app.apply_event(&e);
        }
        assert!(app.draft.is_none());
        assert!(!app.show_ip);
        assert_eq!(app.active_tab, ActiveTab::Services);
        assert_eq!(app.state.run_order.len(), 2);
        let labels = frame(&mut app, &ctx, 800., vec![]);
        find(&labels, "检测已取消，仅展示已取得的结果。");
        find(&labels, "服务");
        assert!(!labels.iter().any(|(s, _)| s == "诊断工作台"));
    }
    #[test]
    fn viewport_switch_preserves_page_and_shows_correct_navigation() {
        let ctx = egui::Context::default();
        let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
        app.navigate(ActiveTab::Settings);
        for (w, wide, mobile) in [
            (1120., true, false),
            (840., false, false),
            (839., false, true),
            (1120., true, false),
        ] {
            let labels = frame(&mut app, &ctx, w, vec![]);
            find(&labels, "工作台设置");
            assert_eq!(labels.iter().any(|(l, _)| l == "诊断工作台"), wide);
            assert_eq!(labels.iter().any(|(l, _)| l == "方案"), mobile);
        }
    }
    #[test]
    fn top_bar_persistent_density_and_theme_switches_and_import_view_disclaimers() {
        let ctx = egui::Context::default();
        let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
        // U1：首屏不进设置页，顶栏常驻显示“完整 / 极简 / 深色 / 浅色”
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        find(&labels, "完整");
        let min_pos = find(&labels, "极简");
        find(&labels, "深色");
        let light_pos = find(&labels, "浅色");

        // 点击顶栏“极简”与“浅色”直接切换为极简三卡 + 浅色主题
        frame(&mut app, &ctx, 1280., click(min_pos));
        frame(&mut app, &ctx, 1280., click(light_pos));
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        assert_eq!(app.density, DensityMode::Minimal);
        assert_eq!(app.theme, ThemeMode::Light);
        find(&labels, "当前出口");
        find(&labels, "重要服务");
        find(&labels, "需要注意");

        // 切回完整模式并打开“自定义导入”页，验证统一导入包三入口 + 分组预览 + 动态双层责任声明
        let normal_pos = find(&labels, "完整");
        frame(&mut app, &ctx, 1280., click(normal_pos));
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        let import_tab = find(&labels, "自定义导入");
        frame(&mut app, &ctx, 1280., click(import_tab));
        let out_import = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 960.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let labels: Vec<(String, Pos2)> = out_import
            .shapes
            .into_iter()
            .filter_map(|e| {
                if let egui::Shape::Text(t) = e.shape {
                    let pos = t.pos + t.galley.size() * 0.5;
                    e.clip_rect
                        .contains(pos)
                        .then(|| (t.galley.text().to_string(), pos))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(app.active_tab, ActiveTab::Import);
        find(&labels, "导入自定义网址与检查方式");
        find(&labels, "手动配置");
        find(&labels, "文本导入");
        let zip_tab = find(&labels, "ZIP 导入包");
        assert!(labels.iter().any(|(l, _)| l.contains("属于用户自身行为")));

        // 切到 ZIP 导入包并载入内置示例 ZIP（含规则+网址），验证两段责任声明同时出现
        frame(&mut app, &ctx, 1280., click(zip_tab));
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        let sample_zip_btn = find(&labels, "载入内置示例 ZIP 包（含规则+网址）");
        frame(&mut app, &ctx, 1280., click(sample_zip_btn));
        // 拉高视口高度以确保底部双声明均在视口内可见
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let zip_labels: Vec<String> = out
            .shapes
            .into_iter()
            .filter_map(|e| match e.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        assert!(zip_labels.iter().any(|l| l.contains("属于用户自身行为")));
        assert!(zip_labels
            .iter()
            .any(|l| l.contains("不是本软件内置、开发、测试或背书的功能")));
    }

    #[test]
    fn f3_online_confirm_modal_popup_cancel_escape_and_confirm() {
        let ctx = egui::Context::default();
        let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
        app.navigate(ActiveTab::Import);

        // 1. 在统一导入包页勾选网址声明并导入 1 个目标（使用 1100 高视口覆盖完整卡片）
        let run_tall = |app: &mut ParallaxeGuiApp, events: Vec<Input>| -> Vec<(String, Pos2)> {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(1280., 1100.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.render_ui(ctx),
            );
            out.shapes
                .into_iter()
                .filter_map(|e| {
                    if let egui::Shape::Text(t) = e.shape {
                        let pos = t.pos + t.galley.size() * 0.5;
                        e.clip_rect
                            .contains(pos)
                            .then(|| (t.galley.text().to_string(), pos))
                    } else {
                        None
                    }
                })
                .collect()
        };

        let labels = run_tall(&mut app, vec![]);
        let consent_pos = find(
            &labels,
            "我已阅读并同意《导入自定义网址责任声明》（确认属于用户自身行为，后果本软件不负责）",
        );
        run_tall(&mut app, click(consent_pos));
        let labels = run_tall(&mut app, vec![]);
        let import_btn = find(&labels, "导入 1 个目标到目录（默认零发包）");
        run_tall(&mut app, click(import_btn));
        assert_eq!(app.registry.user_targets().len(), 1);
        assert!(app.registry.active_session_grant_snapshot().is_none());

        // 2. 点击“申请一次性在线检查 (需运行前确认)…”应弹出模态窗口
        let labels = run_tall(&mut app, vec![]);
        let open_modal_btn = find(&labels, "申请一次性在线检查 (需运行前确认)…");
        run_tall(&mut app, click(open_modal_btn));
        let labels = run_tall(&mut app, vec![]);
        assert!(app.import_view.online_confirm_open);
        find(&labels, "【运行前确认】一次性在线检测授权");

        // 3. 按下 Esc 关闭弹窗：不注入许可，保持零发包
        run_tall(
            &mut app,
            vec![Input::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(!app.import_view.online_confirm_open);
        assert!(app.registry.active_session_grant_snapshot().is_none());

        // 4. 再次打开弹窗并点击“取消（保持默认零发包）”：同样不注入许可
        let labels = run_tall(&mut app, vec![]);
        let open_modal_btn = find(&labels, "申请一次性在线检查 (需运行前确认)…");
        run_tall(&mut app, click(open_modal_btn));
        let labels = run_tall(&mut app, vec![]);
        assert!(app.import_view.online_confirm_open);
        let cancel_btn = find(&labels, "取消（保持默认零发包）");
        run_tall(&mut app, click(cancel_btn));
        assert!(!app.import_view.online_confirm_open);
        assert!(app.registry.active_session_grant_snapshot().is_none());
    }

    #[tokio::test]
    async fn stage4_guard_audit_rollback_and_linkcheck_four_columns_render() {
        let ctx = egui::Context::default();
        let reg = Registry::embedded().unwrap();
        let events = build_offline_scenario_events(
            &reg,
            OfflineScenario::BooksAndSharedCdnGuard,
            "run-stage4-guard-ui",
        )
        .await
        .unwrap();
        let mut app = ParallaxeGuiApp::new(reg, ctx.clone());
        for ev in &events {
            app.apply_event(ev);
        }
        app.navigate(ActiveTab::Guard);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1100.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let labels: Vec<(String, Pos2)> = out
            .shapes
            .into_iter()
            .filter_map(|e| {
                if let egui::Shape::Text(t) = e.shape {
                    let pos = t.pos + t.galley.size() * 0.5;
                    e.clip_rect
                        .contains(pos)
                        .then(|| (t.galley.text().to_string(), pos))
                } else {
                    None
                }
            })
            .collect();

        // 验证人话版处置中心：零英文等式遗留于默认表面、四列中文标题清晰、可主动选择屏蔽正常服务（如 Google One）
        find(&labels, "本机隔离预演与主动屏蔽评估（只读）");
        find(&labels, "仅限本机流量");
        find(&labels, "离线示例 · DNS 查询已停用");
        find(&labels, "链路体检四列只读视图");
        find(&labels, "1. 域名解析");
        find(&labels, "2. 连接边缘");
        find(&labels, "3. 网页出口");
        find(&labels, "4. 双栈状态");
        find(&labels, "当前选择的即时预演与安全审计");
        assert!(
            !labels
                .iter()
                .any(|(l, _)| l == "effect_scope=LOCAL_TRAFFIC_ONLY"),
            "英文等式代码应隐藏于折叠区，不暴露在默认主界面"
        );

        // 验证可主动为未测/正常服务 Google One 打开只读处置清单（支持主动屏蔽正常服务）
        app.dispatch_actions(&ctx, vec![UiAction::OpenPlan("google.one".into())]);
        assert!(app.draft.is_some());
        assert_eq!(app.draft.as_ref().unwrap().target, "google.one");

        // 渲染绝不篡改底层状态机的判定与退出码
        let run = app.app_state().active_run().unwrap();
        assert_eq!(run.exit_code(), Some(2));
        assert_eq!(run.plans.len(), 1);
        assert!(!run.plans[0].safe_to_apply);
        assert!(!run.plans[0].applied);
    }

    #[tokio::test]
    async fn stage6_locale_switch_eligibility_notes_and_verbatim_legal_notice() {
        use crate::views::format_eligibility_summary;
        use parallaxe_core::eligibility::{EligibilityCategory, EligibilityNote};

        let ctx = egui::Context::default();
        let reg = Registry::embedded().unwrap();
        let events = build_offline_scenario_events(
            &reg,
            OfflineScenario::BooksAndSharedCdnGuard,
            "run-stage6-gui",
        )
        .await
        .unwrap();
        let mut app = ParallaxeGuiApp::new(reg, ctx.clone());
        for ev in &events {
            app.apply_event(ev);
        }
        app.navigate(ActiveTab::Overview);

        // 1. 默认中文界面可见顶栏 [中文|EN] 与 9 态中文标签 + 资格说明标记（标准 1280×960 桌面高度）
        let run_960 = |app: &mut ParallaxeGuiApp, events: Vec<Input>| -> Vec<(String, Pos2)> {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(1280., 960.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.render_ui(ctx),
            );
            out.shapes
                .into_iter()
                .filter_map(|e| {
                    if let egui::Shape::Text(t) = e.shape {
                        let pos = t.pos + t.galley.size() * 0.5;
                        e.clip_rect
                            .contains(pos)
                            .then(|| (t.galley.text().to_string(), pos))
                    } else {
                        None
                    }
                })
                .collect()
        };
        let labels = run_960(&mut app, vec![]);
        assert_eq!(app.app_state().current_locale(), Locale::ZhCn);
        find(&labels, "网络诊断总览");
        find(&labels, "明确异常");
        find(&labels, "中文");
        let en_pos = find(&labels, "EN");
        assert!(labels.iter().any(|(l, _)| l.contains("资格说明")));

        // 2. 点击顶栏 EN 切换英文：布局不崩、9 态改用 StateBadge::label(EnUs)、判定/退出码不变
        run_960(&mut app, click(en_pos));
        let labels = run_960(&mut app, vec![]);
        assert_eq!(app.app_state().current_locale(), Locale::EnUs);
        find(&labels, "Network Diagnostics Overview");
        find(&labels, "Definite Anomalies");
        find(&labels, "Start Check");
        assert!(labels.iter().any(|(l, _)| l.contains("Restricted")));
        assert!(labels.iter().any(|(l, _)| l.contains("Eligibility")));
        assert_eq!(app.app_state().active_run().unwrap().exit_code(), Some(2));

        // 3. 展开解释抽屉：默认显示人话未验证标签，开启技术细节后显示 verified=false；缺失 key 安全回退不 panic
        app.dispatch_actions(&ctx, vec![UiAction::SelectTarget("google.books".into())]);
        let labels = run_960(&mut app, vec![]);
        find(&labels, "Account Unverified");
        app.set_show_technical_details(true);
        let tech_labels = run_960(&mut app, vec![]);
        find(&tech_labels, "verified=false");
        app.set_show_technical_details(false);
        let missing = EligibilityNote {
            category: EligibilityCategory::EntitlementGate,
            summary_key: "eligibility.gate.nonexistent_test_key".into(),
            source_url: "https://support.google.com/".into(),
            verified: false,
        };
        let fallback_text = format_eligibility_summary(&missing, Locale::EnUs);
        assert!(fallback_text.contains("eligibility.gate.nonexistent_test_key"));

        // 4. 切到自定义导入页（英文界面）：两段责任声明中文原文必须保持一字不改
        app.navigate(ActiveTab::Import);
        let labels = run_960(&mut app, vec![]);
        find(&labels, "Import Custom URLs & Checks");
        assert!(
            labels.iter().any(|(l, _)| l.contains("属于用户自身行为")),
            "英文界面下责任声明必须保持核定中文原文，不得自动机翻"
        );
    }

    #[tokio::test]
    async fn stage7_blocking_hub_secondary_full_replace_and_no_third_option_in_ui() {
        let ctx = egui::Context::default();
        let reg = Registry::embedded().unwrap();
        let events = build_offline_scenario_events(
            &reg,
            OfflineScenario::BooksAndSharedCdnGuard,
            "run-stage7-guard",
        )
        .await
        .unwrap();
        let mut app = ParallaxeGuiApp::new(reg, ctx.clone());
        for ev in &events {
            app.apply_event(ev);
        }
        app.navigate(ActiveTab::Guard);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let labels: Vec<(String, Pos2)> = out
            .shapes
            .into_iter()
            .filter_map(|e| {
                if let egui::Shape::Text(t) = e.shape {
                    let pos = t.pos + t.galley.size() * 0.5;
                    e.clip_rect
                        .contains(pos)
                        .then(|| (t.galley.text().to_string(), pos))
                } else {
                    None
                }
            })
            .collect();

        // 测试版可导出规则，隐藏系统阻断操作。
        let export_pos = find(&labels, "导出规则（推荐）");
        assert!(!labels.iter().any(|(l, _)| l.contains("启用本机阻断")));
        find(&labels, "更多导出选项 ...");
        find(&labels, "帮助与进阶文档（GitHub）");

        // 2. 第二备选（整包替换）未打开次级菜单前不得出现在主界面；第三备选（Android/iOS 系统配置）永不进 UI
        assert!(
            !labels.iter().any(|(l, _)| l.contains("整包替换")),
            "整包替换不得直接出现在主界面推荐位"
        );
        assert!(
            !labels
                .iter()
                .any(|(l, _)| l.contains("私人 DNS") || l.contains("Android/iOS 系统")),
            "第三备选不得进入软件 UI"
        );

        // 3. 点击「导出规则（推荐）」打开弹窗，核心闸门诚实拒绝并保持零修改
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                events: click(export_pos),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        assert!(app.guard_view.export_modal_open);
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        find(&labels, "导出规则（推荐 · 不改动原有分流）");

        // Esc 关闭弹窗
        frame(
            &mut app,
            &ctx,
            1280.,
            vec![Input::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(!app.guard_view.export_modal_open);

        // 关闭后重新渲染一帧以获取稳定坐标，再从次级菜单「更多导出选项 ▾」进入第二备选
        app.guard_view.close_modals();
        let fresh = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let fresh_labels: Vec<(String, Pos2)> = fresh
            .shapes
            .into_iter()
            .filter_map(|e| match e.shape {
                egui::Shape::Text(t) => {
                    Some((t.galley.text().to_string(), t.pos + t.galley.size() * 0.5))
                }
                _ => None,
            })
            .collect();
        let more_pos = find(&fresh_labels, "更多导出选项 ...");
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                events: click(more_pos),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let out_menu = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let menu_labels: Vec<(String, Pos2)> = out_menu
            .shapes
            .into_iter()
            .filter_map(|e| match e.shape {
                egui::Shape::Text(t) => {
                    Some((t.galley.text().to_string(), t.pos + t.galley.size() * 0.5))
                }
                _ => None,
            })
            .collect();
        let full_item = find(&menu_labels, "完整规则（整包替换，需谨慎）");
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                events: click(full_item),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        assert!(app.guard_view.full_modal_open);
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        assert!(labels
            .iter()
            .any(|(l, _)| l.contains("整包替换会覆盖你原有的分流规则")));
    }

    #[tokio::test]
    async fn stage7_t7_10_export_and_t7_11_profile_risk_preview_and_qos_backoff() {
        let ctx = egui::Context::default();
        let reg = Registry::embedded().unwrap();
        let mut events = build_offline_scenario_events(
            &reg,
            OfflineScenario::BooksAndSharedCdnGuard,
            "run-stage7-qos",
        )
        .await
        .unwrap();

        // 在核心终态报告的既有 risk_signal.markers 中注入 LOCAL_QOS_SUSPECT（模拟 core 报告）
        if let Some(last) = events.last_mut() {
            if let Some(results) = last
                .payload
                .get_mut("report")
                .and_then(|r| r.get_mut("results"))
                .and_then(|r| r.as_array_mut())
            {
                if let Some(first) = results.first_mut() {
                    if let Some(markers) = first
                        .get_mut("risk_signal")
                        .and_then(|rs| rs.get_mut("markers"))
                        .and_then(|m| m.as_array_mut())
                    {
                        markers.push(serde_json::json!("LOCAL_QOS_SUSPECT"));
                    }
                }
            }
        }

        let mut app = ParallaxeGuiApp::new(reg, ctx.clone());
        for ev in &events {
            app.apply_event(ev);
        }

        // T-7-11：QoS 退让触发克制提示，且不改变退出码与状态
        assert!(app.schedule_prefs.qos_banner_active);
        assert_eq!(app.schedule_prefs.effective().min_interval_ms, 5_000);
        assert_eq!(app.app_state().active_run().unwrap().exit_code(), Some(2));

        let labels = frame(&mut app, &ctx, 1280., vec![]);
        assert!(labels
            .iter()
            .any(|(l, _)| l.contains("检测到网络波动，已自动放慢至 5000 ms；可手动锁定")));

        // T-7-10：切到运行历史与导出，触发 Markdown / 纯文本预览，断言与 core 同源字节一致
        app.dispatch_actions(&ctx, vec![UiAction::ExportPreview(ExportFormat::Markdown)]);
        let report = app
            .app_state()
            .active_run()
            .unwrap()
            .report
            .clone()
            .unwrap();
        let expected_md = parallaxe_core::reports::to_markdown(&report, Locale::ZhCn);
        assert_eq!(
            app.export_preview.as_ref().map(|(_, s)| s.as_str()),
            Some(expected_md.as_str())
        );

        app.dispatch_actions(&ctx, vec![UiAction::ExportPreview(ExportFormat::Text)]);
        let expected_txt = parallaxe_core::reports::to_text(&report, Locale::ZhCn);
        assert_eq!(
            app.export_preview.as_ref().map(|(_, s)| s.as_str()),
            Some(expected_txt.as_str())
        );

        // 设置页「高级网络」二级页可见网络画像与零发包风险预览
        app.navigate(ActiveTab::Settings);
        app.open_settings_page(settings_page::SettingsPage::Advanced);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1500.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let settings_labels: Vec<(String, Pos2)> = out
            .shapes
            .into_iter()
            .filter_map(|e| match e.shape {
                egui::Shape::Text(t) => {
                    Some((t.galley.text().to_string(), t.pos + t.galley.size() * 0.5))
                }
                _ => None,
            })
            .collect();
        find(&settings_labels, "检测节奏与网络环境");
        find(&settings_labels, "移动蜂窝");
        find(&settings_labels, "运行前耗时与连接预估");
        find(&settings_labels, "仅本地预估 · 零发包");

        // 移动端窄屏（420x840）正常渲染且不 panic
        let mobile_out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(420., 840.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        assert!(!mobile_out.shapes.is_empty());
    }

    #[test]
    fn pending_sysblock_audit_banner_shows_in_guard_and_settings_without_blocking() {
        let dir =
            std::env::temp_dir().join(format!("parallaxe_gui_audit_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp audit dir");
        std::fs::write(
            dir.join("sysblock-pending.json"),
            serde_json::json!({
                "schema": "parallaxe.sysblock.audit/v2",
                "status": "INTENT",
                "rule_tag": "pending",
                "code": "SYSBLOCK_RECOVERY_REQUIRED",
                "detail": "上次系统写入状态未知，请先核对规则。",
                "recovery": "parallaxe guard unblock --rule-tag pending"
            })
            .to_string(),
        )
        .expect("write pending audit");
        std::fs::write(
            dir.join("sysblock-done.json"),
            serde_json::json!({
                "schema": "parallaxe.sysblock.audit/v2",
                "status": "REVOKED",
                "rule_tag": "done"
            })
            .to_string(),
        )
        .expect("write revoked audit");

        let scanned = guard_panel::scan_pending_sysblock_audits(&dir);
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].rule_tag, "pending");
        assert_eq!(scanned[0].status, "INTENT");
        assert_eq!(scanned[0].code, "SYSBLOCK_RECOVERY_REQUIRED");

        let registry = Registry::embedded().expect("embedded");
        let ctx = egui::Context::default();
        let mut app = ParallaxeGuiApp::new(registry, ctx.clone());
        app.guard_view.audit_dir_override = Some(dir.clone());
        app.guard_view.refresh_pending_audits();

        // 处置中心显示克制恢复条，不弹窗阻塞
        app.navigate(ActiveTab::Guard);
        let guard_out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        assert!(!app.guard_view.any_modal_open());
        let guard_texts: Vec<String> = guard_out
            .shapes
            .into_iter()
            .filter_map(|e| match e.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        // 默认视角隐藏底层机器码（问题 3），仍展示恢复命令与一键复制
        assert!(!guard_texts
            .iter()
            .any(|t| t.contains("SYSBLOCK_RECOVERY_REQUIRED")));
        assert!(guard_texts
            .iter()
            .any(|t| t.contains("parallaxe guard unblock --rule-tag pending")));
        assert!(!guard_texts
            .iter()
            .any(|t| t.contains("UNBLOCK_FOREIGN_RULE")));
        assert!(guard_texts.iter().any(|t| t.contains("复制恢复命令")));

        // 开启「显示技术细节」后可见完整机器码，便于排障
        app.set_show_technical_details(true);
        let guard_tech_out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let guard_tech_texts: Vec<String> = guard_tech_out
            .shapes
            .into_iter()
            .filter_map(|e| match e.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        assert!(guard_tech_texts
            .iter()
            .any(|t| t.contains("SYSBLOCK_RECOVERY_REQUIRED")));
        assert!(guard_tech_texts
            .iter()
            .any(|t| t.contains("UNBLOCK_FOREIGN_RULE")));
        app.set_show_technical_details(false);

        // 设置页同样显示只读恢复提示（默认隐藏机器码，保留恢复命令）
        app.navigate(ActiveTab::Settings);
        let settings_out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1280., 1200.),
                )),
                ..Default::default()
            },
            |ctx| app.render_ui(ctx),
        );
        let settings_texts: Vec<String> = settings_out
            .shapes
            .into_iter()
            .filter_map(|e| match e.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        assert!(!settings_texts
            .iter()
            .any(|t| t.contains("SYSBLOCK_RECOVERY_REQUIRED")));
        assert!(settings_texts
            .iter()
            .any(|t| t.contains("parallaxe guard unblock --rule-tag pending")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn beta_cannot_reopen_system_block_from_stale_modal_state() {
        let ctx = egui::Context::default();
        let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
        app.navigate(ActiveTab::Guard);
        app.guard_view.sysblock_modal_open = true;
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        assert!(!app.guard_view.sysblock_modal_open);
        assert!(!labels
            .iter()
            .any(|(label, _)| label.contains("管理员撤销命令")));
    }

    #[test]
    fn android_simulated_user_scope_renders_clearly_without_altering_verdicts() {
        use parallaxe_core::contracts::ObservationScope;

        let registry = Registry::embedded().expect("embedded");
        let ctx = egui::Context::default();
        let mut app = ParallaxeGuiApp::new(registry, ctx.clone());
        app.dispatch_actions(
            &ctx,
            vec![UiAction::RunFixture("gemini-positive".to_string())],
        );
        for _ in 0..80 {
            app.drain_worker();
            if app
                .state
                .active_run()
                .and_then(|r| r.report.as_ref())
                .is_some()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let active_id = app.state.active_run_id.clone().expect("active fixture run");
        let original_exit = {
            let rv = app.state.runs.get_mut(&active_id).unwrap();
            let rep = rv.report.as_mut().unwrap();
            rep.scope = Some(ObservationScope::AndroidSimulatedUser);
            rep.exit_code
        };

        // 总览与时间线导出页均清晰展示 ANDROID_SIMULATED_USER 观测边界
        for tab in [ActiveTab::Overview, ActiveTab::Timeline] {
            app.navigate(tab);
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(1280., 1200.),
                    )),
                    ..Default::default()
                },
                |ctx| app.render_ui(ctx),
            );
            let texts: Vec<String> = out
                .shapes
                .into_iter()
                .filter_map(|e| match e.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect();
            assert!(texts.iter().any(|t| t.contains("ANDROID_SIMULATED_USER")));
            assert!(texts.iter().any(|t| t.contains("非系统级观测")));
        }
        assert_eq!(
            app.state
                .active_run()
                .and_then(|r| r.report.as_ref())
                .map(|r| r.exit_code),
            Some(original_exit)
        );
    }
}
