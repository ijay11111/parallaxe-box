//! `eframe::App` 主窗口：三维度正交重构（单主视图 + Tab + 按需动画抽屉 + 窄屏单列底栏）。
//!
//! 三个正交维度：
//! - `DensityMode`：`Normal`（仪表盘+Tab） vs `Minimal`（三卡极简）
//! - `ThemeMode`：`Dark` vs `Light`（纯色板切换，绝不改变 `LayoutSpec`）
//! - `ViewportTier`：`Wide`/`Compact` 桌面 vs `Narrow`（`<840px` 单列堆叠 + 底部 4 栏导航）

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui;
use egui::{RichText, Ui};
use parallaxe_core::contracts::{Event, EventName, ACCOUNT_MODE, SESSION_MODE};
use parallaxe_core::fixtures;
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::runtime::CheckOptions;
use parallaxe_core::targets::Registry;

use crate::commands::{
    render_export_locale, write_export_file, CommandBus, ExportFormat, GuiCommand, OfflineScenario,
    StaticTargetExplain, WorkerMessage,
};
use crate::icons::IconKind;
use crate::layout::{ActiveTab, LayoutSpec, MobileTab, ViewportTier};
use crate::state::AppState;
use crate::theme::{
    apply_theme, btn_filled, btn_outlined, btn_tonal, card_frame, install_cjk_fonts,
    render_modal_scrim, render_running_pulse_bar, DensityMode, Palette, TableDensity, ThemeMode,
    FONT_BODY, FONT_DISPLAY, FONT_META, FONT_TITLE, RADIUS_LG, RADIUS_MD, SPACE_1, SPACE_2,
    SPACE_3, SPACE_4, SPACE_5, SPACE_6,
};
use crate::views::import_panel::{self, ImportViewState};
use crate::views::plan::PlanDraft;
use crate::views::{
    coverage_panel, explain_drawer, guard_panel, overview_minimal, overview_normal, service_matrix,
    target_tree, timeline_view, top_bar, UiAction,
};

/// 启动原生 `eframe/egui` 窗口（仅绑定本机图形会话，不监听任何网络端口）。
pub fn run_native(registry: Registry) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Parallaxe box · Google 全服务出口观测与链路诊断（只读）")
            .with_decorations(false)
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([380.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Parallaxe box",
        options,
        Box::new(move |cc| {
            install_cjk_fonts(&cc.egui_ctx);
            apply_theme(&cc.egui_ctx, ThemeMode::Dark);
            Ok(Box::new(ParallaxeGuiApp::new(
                registry,
                cc.egui_ctx.clone(),
            )))
        }),
    )
    .map_err(|err| format!("无法初始化图形窗口：{err}"))
}

/// Android 原生启动：把 `AndroidApp` 通过 winit 的 event-loop builder 传入。
#[cfg(target_os = "android")]
pub fn run_native_android(
    registry: Registry,
    app: android_activity::AndroidApp,
) -> Result<(), String> {
    use winit::platform::android::EventLoopBuilderExtAndroid;
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
            apply_theme(&cc.egui_ctx, ThemeMode::Dark);
            Ok(Box::new(ParallaxeGuiApp::new(
                registry,
                cc.egui_ctx.clone(),
            )))
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
    pending: bool,
    catalog_drawer_open: bool,
    explain_drawer_open: bool,
    about_modal_open: bool,
    ipv4_only: bool,
    fast_mode: bool,
    only_selected_uncovered: bool,
    checked_targets: BTreeSet<String>,
    tree_filter: String,
    static_explain: Option<StaticTargetExplain>,
    selftest_banner: Option<String>,
    export_path_input: String,
    export_preview: Option<(ExportFormat, String)>,
    export_status: Option<String>,
    import_view: ImportViewState,
    guard_view: guard_panel::GuardViewState,
    resolved_link_report: Option<parallaxe_core::linkcheck::LinkReport>,
    table_density: TableDensity,
}

impl ParallaxeGuiApp {
    pub fn new(registry: Registry, ctx: egui::Context) -> Self {
        let wake_ctx = ctx.clone();
        let bus = CommandBus::spawn(
            registry.clone(),
            Arc::new(move || {
                wake_ctx.request_repaint();
            }),
        );
        ParallaxeGuiApp {
            registry,
            bus,
            state: AppState::new(),
            theme: ThemeMode::Dark,
            applied_theme: None,
            density: DensityMode::Normal,
            active_tab: ActiveTab::Overview,
            draft: None,
            show_ip: false,
            pending: false,
            catalog_drawer_open: false,
            explain_drawer_open: false,
            about_modal_open: false,
            ipv4_only: false,
            fast_mode: false,
            only_selected_uncovered: true,
            checked_targets: BTreeSet::new(),
            tree_filter: String::new(),
            static_explain: None,
            selftest_banner: None,
            export_path_input: "parallaxe-report.json".to_string(),
            export_preview: None,
            export_status: None,
            import_view: ImportViewState::default(),
            guard_view: guard_panel::GuardViewState::default(),
            resolved_link_report: None,
            table_density: TableDensity::Comfortable,
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

    /// 供无头渲染测试读取底层状态机。
    pub fn app_state(&self) -> &AppState {
        &self.state
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
        }
        if !was_terminal
            && matches!(
                event.event,
                EventName::RunFinished | EventName::RunCancelled
            )
        {
            self.pending = false;
            if let Some(r) = self
                .state
                .active_run()
                .filter(|r| r.run_id == event.run_id && r.terminal.is_some())
            {
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
    }
    fn start(&mut self, cmd: GuiCommand) {
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
                    self.selftest_banner = Some(format!("取消指令 ({run_id})：{status}"));
                }
                WorkerMessage::GuardAuditReady(audit) => {
                    if let Some(run) = self.state.runs.get_mut(&audit.run_id) {
                        run.guard_audit = Some(*audit);
                    }
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
                    self.state.last_command_error = Some(err);
                }
            }
        }
    }

    fn dispatch_actions(&mut self, ctx: &egui::Context, actions: Vec<UiAction>) {
        for action in actions {
            match action {
                UiAction::RunProfile(profile) => {
                    let options = CheckOptions {
                        profile: Some(profile),
                        ipv4: self.ipv4_only,
                        fast: self.fast_mode,
                        run_id: Some(self.bus.next_run_id("run-gui")),
                        ..Default::default()
                    };
                    self.start(GuiCommand::RunCheck { options });
                }
                UiAction::RunFull => {
                    let options = CheckOptions {
                        full: true,
                        ipv4: self.ipv4_only,
                        fast: self.fast_mode,
                        run_id: Some(self.bus.next_run_id("run-gui-full")),
                        ..Default::default()
                    };
                    self.start(GuiCommand::RunCheck { options });
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
                        let options = CheckOptions {
                            targets,
                            ipv4: self.ipv4_only,
                            fast: self.fast_mode,
                            run_id: Some(self.bus.next_run_id("run-gui-custom")),
                            ..Default::default()
                        };
                        self.start(GuiCommand::RunCheck { options });
                    }
                }
                UiAction::RunSingleReprobe(target_id) => {
                    let options = CheckOptions {
                        targets: vec![target_id],
                        ipv4: self.ipv4_only,
                        fast: self.fast_mode,
                        run_id: Some(self.bus.next_run_id("run-reprobe")),
                        ..Default::default()
                    };
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
                    ctx.request_repaint();
                }
                UiAction::ToggleTheme => {
                    self.theme = self.theme.toggle();
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
                    ctx.request_repaint();
                }
                UiAction::SetLocale(locale) => {
                    self.state.set_locale(locale);
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
                            let text = render_export_locale(
                                report,
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
                            let text = render_export_locale(
                                report,
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
            }
        }
    }

    pub fn render_ui(&mut self, ctx: &egui::Context) {
        self.drain_worker();
        if self.applied_theme != Some(self.theme) {
            apply_theme(ctx, self.theme);
            self.applied_theme = Some(self.theme);
        }

        let spec = LayoutSpec::compute(self.density, ctx.screen_rect().width());
        let p = Palette::for_theme(self.theme);
        let locale = self.state.current_locale();
        let mut actions = vec![];
        top_bar::title(
            ctx,
            p,
            spec.show_top_subtitle,
            self.density,
            self.theme,
            locale,
            &mut actions,
        );
        if spec.show_bottom_nav {
            let nav_panel = egui::TopBottomPanel::bottom("mobile_nav")
                .frame(
                    egui::Frame::none()
                        .fill(p.sidebar)
                        .inner_margin(egui::Margin::symmetric(SPACE_2, SPACE_1 + 2.0)),
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
                        .inner_margin(egui::Margin::symmetric(SPACE_4, SPACE_1 + 2.0)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(translate(locale, "gui.status.readonly_bar", &[]))
                                .size(FONT_META)
                                .color(p.text_muted),
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
                if spec.tier == ViewportTier::Narrow {
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
                        &mut actions,
                    );
                    return;
                }
                if spec.tier == ViewportTier::Compact
                    || (spec.tier == ViewportTier::Wide && spec.show_desktop_tabs)
                {
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
                            });
                        });
                    ui.add_space(SPACE_4);
                }
                egui::ScrollArea::vertical()
                    .id_salt(("workspace", self.active_tab as u8))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.workspace(ui, spec, &mut actions);
                    });
            });
        if self.about_modal_open {
            render_modal_scrim(ctx, "about_modal_scrim", p);
            egui::Window::new("关于与隐私")
                .order(egui::Order::Foreground)
                .open(&mut self.about_modal_open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(360.0)
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
                    ui.label(
                        RichText::new("只读匿名观测")
                            .size(FONT_TITLE)
                            .strong()
                            .color(p.text_primary),
                    );
                    ui.add_space(SPACE_2);
                    ui.label(
                        RichText::new("不读取账号、Cookie 或浏览器配置；不保存原始公网地址。")
                            .size(FONT_BODY)
                            .color(p.text_secondary),
                    );
                    ui.add_space(SPACE_2);
                    ui.monospace(format!(
                        "account_mode={ACCOUNT_MODE}\nsession_mode={SESSION_MODE}"
                    ));
                    ui.add_space(SPACE_2);
                    ui.label(
                        RichText::new("只能描述当前服务页面，不能证明账号地区或共享出口画像。")
                            .size(FONT_META)
                            .color(p.text_muted),
                    );
                });
        }
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
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(translate(locale, title_key, &[]))
                    .size(FONT_DISPLAY)
                    .strong()
                    .color(p.text_primary),
            );
            if matches!(self.active_tab, ActiveTab::Overview | ActiveTab::Services) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_2;
                    let cta_label = if self.pending {
                        translate(locale, "gui.action.checking", &[])
                    } else {
                        translate(locale, "gui.action.start_check", &[])
                    };
                    if btn_filled(ui, p, &cta_label, !self.pending).clicked() {
                        actions.push(UiAction::RunProfile("simple".into()));
                    }
                    if self.density == DensityMode::Minimal
                        && btn_tonal(ui, p, &translate(locale, "gui.action.view_plan", &[]), true)
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
        ui.add_space(SPACE_4);
        match self.active_tab {
            ActiveTab::Overview | ActiveTab::Services => {
                if self.active_tab == ActiveTab::Overview && self.density == DensityMode::Minimal {
                    overview_minimal::render(ui, run, self.theme, locale, spec.tier, actions);
                } else {
                    if let Some(r) = run {
                        service_matrix::finding(ui, r, self.theme, locale, actions);
                    }
                    overview_normal::metrics(ui, p, run, locale);
                    ui.add_space(SPACE_4);
                    service_matrix::render(
                        ui,
                        &mut self.state,
                        self.theme,
                        spec.tier,
                        &mut self.table_density,
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
                actions,
            ),
            ActiveTab::Import => import_panel::render(
                ui,
                &mut self.registry,
                &mut self.import_view,
                self.theme,
                locale,
                spec.tier,
                actions,
            ),
            ActiveTab::Settings => self.settings(ui, p, actions),
        }
    }

    fn settings(&mut self, ui: &mut Ui, p: Palette, actions: &mut Vec<UiAction>) {
        let locale = self.state.current_locale();
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(translate(locale, "gui.settings.appearance", &[]))
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
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
                ui.label(
                    RichText::new(translate(locale, "gui.settings.language", &[]))
                        .size(FONT_BODY)
                        .color(p.text_secondary),
                );
                for loc in Locale::ALL {
                    if ui.selectable_label(locale == loc, loc.label()).clicked() {
                        actions.push(UiAction::SetLocale(loc));
                    }
                }
                ui.add_space(SPACE_4);
                ui.checkbox(
                    &mut self.show_ip,
                    translate(locale, "gui.settings.show_ip", &[]),
                );
            });
            if self.show_ip {
                ui.add_space(SPACE_1);
                ui.label(
                    RichText::new(
                        "当前核心未提供可展示的原始出口地址。不会额外查询、记录或导出 IP。",
                    )
                    .size(FONT_META)
                    .color(p.text_secondary),
                );
            }
        });
        ui.add_space(SPACE_4);
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_3;
                ui.label(
                    RichText::new("检测范围")
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.checkbox(&mut self.ipv4_only, "仅 IPv4");
                ui.checkbox(&mut self.fast_mode, "快速检测");
            });
            ui.add_space(SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                for (label, act) in [
                    ("简单检测", UiAction::RunProfile("simple".into())),
                    ("Google 全服务", UiAction::RunProfile("google-all".into())),
                    ("完整检测", UiAction::RunFull),
                    ("自选目标", UiAction::ToggleCatalogDrawer),
                ] {
                    if btn_outlined(ui, p, label, true).clicked() {
                        actions.push(act);
                    }
                }
            });
        });
        ui.add_space(SPACE_4);
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("离线演练")
                        .strong()
                        .size(FONT_TITLE)
                        .color(p.text_primary),
                );
                ui.label(
                    RichText::new("（使用合成样本，不代表你的实际网络）")
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
            });
            ui.add_space(SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                demo_picker(ui, false, actions);
                demo_picker(ui, true, actions);
                if btn_tonal(ui, p, "运行离线自检", true).clicked() {
                    actions.push(UiAction::RunSelftest);
                }
            });
        });
        ui.add_space(SPACE_4);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_2;
            if btn_outlined(ui, p, "自定义网址与检查方式导入", true).clicked() {
                actions.push(UiAction::SwitchTab(ActiveTab::Import));
            }
            if btn_outlined(ui, p, "运行历史与报告导出", true).clicked() {
                actions.push(UiAction::SwitchTab(ActiveTab::Timeline));
            }
        });
    }
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
    }
}
fn demo_picker(ui: &mut Ui, fixture: bool, actions: &mut Vec<UiAction>) {
    let id = ui.make_persistent_id(("demo_picker", fixture));
    let response = ui.button(if fixture {
        "九项内置样本"
    } else {
        "六类契约演示"
    });
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

        // 切回完整模式并打开“自定义导入”页，验证统一导入包三入口 + 预览表 + 动态双层责任声明
        let normal_pos = find(&labels, "完整");
        frame(&mut app, &ctx, 1280., click(normal_pos));
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        let import_tab = find(&labels, "自定义导入");
        frame(&mut app, &ctx, 1280., click(import_tab));
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        assert_eq!(app.active_tab, ActiveTab::Import);
        find(&labels, "统一导入包（网址与检查方式一体绑定）");
        find(&labels, "手动配置");
        find(&labels, "文本文件 (txt / md / csv)");
        let zip_tab = find(&labels, "ZIP 导入包 (.zip)");
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
        find(
            &labels,
            "【运行前强制确认】一次性会话级联网授权 (F3 许可闸门)",
        );

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

        // 1. 默认中文界面可见顶栏 [中文|EN] 与 9 态中文标签 + 资格说明标记
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        assert_eq!(app.app_state().current_locale(), Locale::ZhCn);
        find(&labels, "网络诊断总览");
        find(&labels, "明确异常");
        find(&labels, "中文");
        let en_pos = find(&labels, "EN");
        assert!(labels.iter().any(|(l, _)| l.contains("资格说明")));

        // 2. 点击顶栏 EN 切换英文：布局不崩、9 态改用 StateBadge::label(EnUs)、判定/退出码不变
        frame(&mut app, &ctx, 1280., click(en_pos));
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        assert_eq!(app.app_state().current_locale(), Locale::EnUs);
        find(&labels, "Network Diagnostics Overview");
        find(&labels, "Definite Anomalies");
        find(&labels, "Start Check");
        assert!(labels.iter().any(|(l, _)| l.contains("Restricted")));
        assert!(labels.iter().any(|(l, _)| l.contains("Eligibility")));
        assert_eq!(app.app_state().active_run().unwrap().exit_code(), Some(2));

        // 3. 展开解释抽屉：可见官方帮助链接与 verified=false，且缺失 key 安全回退不 panic
        app.dispatch_actions(&ctx, vec![UiAction::SelectTarget("google.books".into())]);
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        find(&labels, "Unverified (verified=false)");
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
        let labels = frame(&mut app, &ctx, 1280., vec![]);
        find(
            &labels,
            "Unified Import Bundle (URLs & Rules Bound Together)",
        );
        assert!(
            labels.iter().any(|(l, _)| l.contains("属于用户自身行为")),
            "英文界面下责任声明必须保持核定中文原文，不得自动机翻"
        );
    }
}
