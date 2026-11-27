//! Windows 桌面不可截取时，直接读取真实 eframe 窗口的帧缓冲。只编入验收 example。
//! 从界面实际绘出的文字定位按钮，用 egui 指针事件点击；不注入报告，不调用内部 action。
use eframe::egui::{self, Event, Pos2, ViewportCommand};
use parallaxe_core::contracts::{Coverage, GuardPlan, Mode, Report, Termination};
use parallaxe_core::events::EventLog;
use parallaxe_core::import_bundle::parse_import_text_with_default_group;
use parallaxe_core::reports;
use parallaxe_core::targets::Registry;
use parallaxe_gui::{
    app::ParallaxeGuiApp,
    app_icon,
    layout::ActiveTab,
    theme::{install_cjk_fonts, DensityMode, ThemeMode},
    views::import_panel::ImportEntryTab,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Walk {
    app: ParallaxeGuiApp,
    steps: VecDeque<&'static str>,
    inputs: Vec<Event>,
    next: Instant,
    deadline: Instant,
    shot: PathBuf,
    capturing: bool,
}
fn visible(ctx: &egui::Context) -> Vec<(String, Pos2)> {
    let layers = ctx.memory(|m| m.layer_ids().collect::<Vec<_>>());
    ctx.graphics(|g| {
        let mut labels = vec![];
        for layer in layers {
            if let Some(list) = g.get(layer) {
                for e in list.all_entries() {
                    if let egui::Shape::Text(t) = &e.shape {
                        let center = t.pos + t.galley.size() * 0.5;
                        if e.clip_rect.contains(center) {
                            labels.push((t.galley.text().to_string(), center));
                        }
                    }
                }
            }
        }
        labels
    })
}
impl eframe::App for Walk {
    fn raw_input_hook(&mut self, _: &egui::Context, i: &mut egui::RawInput) {
        i.events.append(&mut self.inputs);
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.app.render_ui(ctx);
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(im) = shot {
            let bytes: Vec<u8> = im.pixels.iter().flat_map(|c| c.to_array()).collect();
            image::save_buffer(
                &self.shot,
                &bytes,
                im.width() as u32,
                im.height() as u32,
                image::ColorType::Rgba8,
            )
            .unwrap();
            println!("截图 {} × {}", im.width(), im.height());
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        if Instant::now() > self.deadline {
            panic!(
                "验收操作未完成：{:?}；当前可见标签：{:?}",
                self.steps.front(),
                visible(ctx)
            );
        }
        if !self.capturing && Instant::now() > self.next {
            if let Some(&step) = self.steps.front() {
                if step == "预览导出：Markdown" {
                    println!("{step}");
                    self.app.preview_active_markdown_export_for_test();
                    self.steps.pop_front();
                    self.next = Instant::now() + Duration::from_millis(220);
                } else if let Some(dy_str) = step.strip_prefix("滚：") {
                    let dy: f32 = dy_str.parse().unwrap_or(-360.0);
                    let center = ctx.screen_rect().center();
                    println!("{step}");
                    self.inputs.push(Event::PointerMoved(center));
                    self.inputs.push(Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, dy),
                        modifiers: Default::default(),
                    });
                    self.steps.pop_front();
                    self.next = Instant::now() + Duration::from_millis(260);
                } else {
                    let wait = step.strip_prefix("等：");
                    let text = wait.unwrap_or(step);
                    let matches_text = |s: &str| {
                        if let Some(prefix) = text.strip_suffix('…') {
                            s.starts_with(prefix)
                        } else {
                            s == text
                        }
                    };
                    if let Some((_, pos)) = visible(ctx).into_iter().find(|(s, _)| matches_text(s))
                    {
                        println!("{step}");
                        if wait.is_none() {
                            self.inputs.push(Event::PointerMoved(pos));
                            for pressed in [true, false] {
                                self.inputs.push(Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: Default::default(),
                                });
                            }
                        }
                        self.steps.pop_front();
                        self.next = Instant::now() + Duration::from_millis(220);
                    }
                }
            } else {
                ctx.send_viewport_cmd(ViewportCommand::Screenshot);
                self.capturing = true;
            }
        }
        ctx.request_repaint_after(Duration::from_millis(45));
    }
}
fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "视图 PNG 宽 高");
    let mut steps = vec![
        "开始检测",
        "等：服务检测结果",
        "查看离线示例：Books 地区受限",
        "等：查看处置方案",
    ];
    match args[1].as_str() {
        "check-home" => {
            steps = vec!["等：简单检查（15 项）"];
        }
        "check-home-light" => {
            steps = vec!["等：简单检查（15 项）"];
        }
        "check-home-en" => {
            steps = vec!["等：Quick Check (15)"];
        }
        "check-pace" => {
            steps = vec!["等：间隔越大越安全，默认 3500ms 已符合 3–5 秒规范"];
        }
        "default-bundle" => {
            steps = vec![
                "等：简单检查（15 项）",
                "我的授权版 · 107 项",
                "等：确认更换默认规则包",
            ];
        }
        "default-bundle-scope" => {
            steps = vec![
                "等：简单检查（15 项）",
                "我的授权版 · 107 项",
                "等：选择授权版生效范围",
            ];
        }
        "default-bundle-confirmed" => {
            let vw: f32 = args[3].parse().unwrap_or(1440.0);
            if vw <= 820.0 {
                steps = vec!["滚：-1100", "等：user.ai-plans"];
            } else {
                steps = vec!["滚：-1000", "等：已登记的用户导入清单…"];
            }
        }
        "default-bundle-result" => {
            steps = vec!["等：用户导入"];
        }
        "default-bundle-export" => {
            steps = vec![
                "等：用户导入",
                "预览导出：Markdown",
                "滚：-420",
                "等：复制预览内容",
            ];
        }
        "distribution-mode-confirm" => {
            steps = vec![
                "设置",
                "等：关于与隐私",
                "切换到发行包模式…",
                "等：确认切换到发行包模式",
            ];
        }
        "overview" => steps.extend(["总览", "等：优先查看"]),
        "overview-en" => steps.extend([
            "总览",
            "等：网络诊断总览",
            "EN",
            "等：Network Diagnostics Overview",
        ]),
        "overview-elig" => steps.extend(["资格说明 (3)", "等：未验证账号"]),
        "minimal" => steps.extend(["极简", "浅色", "等：重要服务"]),
        "minimal-dark" => steps.extend(["极简", "等：重要服务"]),
        "result" => {}
        "plan" => steps.extend(["查看处置方案", "保持监控，不改规则", "导出处置清单"]),
        "guard-link" => steps.extend(["处置中心", "等：链路体检四列只读视图"]),
        "guard-en" => steps.extend([
            "处置中心",
            "等：链路体检四列只读视图",
            "EN",
            "等：Link Check · 4-Column Read-Only View",
        ]),
        "import-en" => {
            steps = vec![
                "自定义导入",
                "等：导入自定义网址与检查方式",
                "EN",
                "等：Import Custom URLs & Checks",
            ];
        }
        "guard-proactive" => steps.extend([
            "处置中心",
            "等：本机隔离预演与主动屏蔽评估（只读）",
            "+ Google One（受保护·可手动清单）",
            "等：主动屏蔽 Google One · 生成处置清单",
            "主动屏蔽 Google One · 生成处置清单",
            "等：Google One · 只读处置清单",
        ]),
        "guard-export-modal" => steps.extend([
            "处置中心",
            "等：阻断与规则导出",
            "导出规则（推荐）",
            "等：导出规则（推荐 · 不改动原有分流）",
        ]),
        "guard-export-bundle" => {
            steps = vec![
                "处置中心",
                "等：阻断与规则导出",
                "导出规则（推荐）",
                "等：选择导出目录并写入...",
            ];
        }
        "mobile-guard-export-bundle" => {
            steps = vec![
                "方案",
                "等：阻断与规则导出",
                "导出规则（推荐）",
                "等：选择导出目录并写入...",
            ];
        }
        "mobile-guard-export-en" => {
            steps = vec!["等：Export Rules (Recommended · Non-Polluting)"];
        }
        "settings-en" => {
            steps = vec!["设置", "等：外观与主题", "EN", "等：Appearance & theme"];
        }
        "settings-light" => {
            steps = vec!["设置", "外观与主题", "等：外观与偏好", "浅色"];
        }
        "mobile-en" => {
            steps = vec![
                "设置",
                "等：工作台设置",
                "EN",
                "等：Settings",
                "Overview",
                "等：Services",
            ];
        }
        "plugin-page" => {
            steps = vec!["报告阅读指南", "等：如何读懂一次检测结果"];
        }
        "about-modal" => {
            steps = vec!["等：匿名无账号 · 无状态只读观测"];
        }
        "notices-modal" => {
            steps = vec!["等：第三方扩展声明（THIRD_PARTY_PLUGINS.md）"];
        }
        "dest-mismatch" | "dest-mismatch-ip" => {
            let vw: f32 = args[3].parse().unwrap_or(1440.0);
            let vh: f32 = args[4].parse().unwrap_or(900.0);
            if vw <= 820.0 && vh <= 860.0 {
                steps = vec![
                    "等：服务检测结果",
                    if vh <= 420.0 {
                        "滚：-600"
                    } else {
                        "滚：-420"
                    },
                    "等：落点与解析不一致",
                ];
            } else {
                steps = vec!["等：服务送往哪里（实际连接落点映射）"];
            }
        }
        "dest-mismatch-en" => {
            steps = vec!["等：Where Each Service Connected (Destination Map)"];
        }
        "dest-proxy" => {
            let vw: f32 = args[3].parse().unwrap_or(1440.0);
            let vh: f32 = args[4].parse().unwrap_or(900.0);
            if vw <= 820.0 && vh <= 860.0 {
                steps = vec!["等：服务检测结果", "滚：-420", "等：代理分层 · 出口不直比"];
            } else {
                steps = vec!["等：代理分层 · 出口不直比"];
            }
        }
        "product-restricted" => {
            let vw: f32 = args[3].parse().unwrap_or(1440.0);
            let vh: f32 = args[4].parse().unwrap_or(900.0);
            if vw <= 820.0 && vh <= 860.0 {
                steps = vec![
                    "等：服务检测结果",
                    "滚：-460",
                    "等：产品返回明确地区不可用信号",
                ];
            } else {
                steps = vec!["等：产品返回明确地区不可用信号"];
            }
        }
        "dest-guard-link-ip" => {
            steps = vec!["等：链路体检四列只读视图", "滚：-420", "等：192.0.2.8"];
        }
        "ai-f3-modal" => {
            steps = vec!["等：AI 请求发起一次网络检测"];
        }
        "ai-f3-modal-en" => {
            steps = vec!["等：AI Requests a Network Check"];
        }
        "ai-settings-off" | "ai-settings-on" => {
            steps = vec!["等：AI 自主调试"];
        }
        "ai-self-disabled" => {
            steps = vec!["等：AI 已自主关闭 · 权限已收回"];
        }
        "mobile-import-manual" => {
            steps = vec!["等：手动配置"];
        }
        "mobile-import-text" => {
            steps = vec!["等：文本导入"];
        }
        "mobile-import-zip" => {
            steps = vec!["等：ZIP 导入包"];
        }
        "guard-sysblock-modal" => steps.extend([
            "处置中心",
            "等：阻断与规则导出",
            "启用本机阻断（需管理员）",
            "等：本机系统阻断（需管理员）",
        ]),
        "guard-full-modal" => steps.extend([
            "处置中心",
            "等：阻断与规则导出",
            "更多导出选项 ...",
            "等：完整规则（整包替换，需谨慎）",
            "完整规则（整包替换，需谨慎）",
            "等：完整规则导出（第二备选 · 整包替换）",
        ]),
        "settings-schedule" => {
            steps = vec![
                "设置",
                "高级网络",
                "等：检测节奏与网络环境",
                "移动蜂窝",
                "等：运行前耗时与连接预估",
            ];
        }
        "timeline-export-md" => steps.extend([
            "运行历史",
            "等：预览 Markdown (.md)",
            "预览 Markdown (.md)",
            "等：复制预览内容",
        ]),
        "mobile-minimal" => {
            steps = vec!["完整", "等：当前出口"];
        }
        "mobile-minimal-light" => {
            steps = vec![
                "设置",
                "等：工作台设置",
                "极简",
                "浅色",
                "总览",
                "等：当前出口",
            ];
        }
        "mobile-guard" => {
            steps = vec!["方案", "等：阻断与规则导出"];
        }
        "import" => {
            steps = vec!["自定义导入", "等：导入自定义网址与检查方式"];
        }
        "import-text" => {
            steps = vec![
                "自定义导入",
                "等：导入自定义网址与检查方式",
                "文本导入",
                "等：导入前预览（核对分组、网址与检查方式）",
            ];
        }
        "import-zip" => {
            steps = vec![
                "自定义导入",
                "等：导入自定义网址与检查方式",
                "ZIP 导入包",
                "载入内置示例 ZIP 包（含规则+网址）",
                "等：将新增的自定义检查方式",
            ];
        }
        "import-checked" => {
            steps = vec![
                "自定义导入",
                "等：导入自定义网址与检查方式",
                "文本导入",
                "等：导入 3 个目标到目录（默认零发包）",
                "我已阅读并同意《导入自定义网址责任声明》（确认属于用户自身行为，后果本软件不负责）",
                "导入 3 个目标到目录（默认零发包）",
                "申请一次性在线检查 (需运行前确认)…",
                "等：【运行前确认】一次性在线检测授权",
            ];
        }
        "menus" => steps.extend([
            "设置",
            "六类契约演示",
            "① Drive 登录跳转（地区未知）",
            "等：Google Drive",
            "设置",
            "九项内置样本",
            "gemini-positive",
            "等：US",
        ]),
        _ => {
            if let Some(rest) = args[1].strip_prefix("settings-") {
                let base = rest
                    .trim_end_matches("-en")
                    .trim_end_matches("-light")
                    .trim_end_matches("-mobile");
                let en = rest.ends_with("-en");
                let (zh, english) = match base {
                    "root" => ("外观与主题", "Appearance & theme"),
                    "appearance" => ("外观与偏好", "Appearance & preferences"),
                    "scope" => ("默认检测范围", "Default Check Scope"),
                    "advanced" => ("检测节奏与网络环境", "Check Pace & Network Environment"),
                    "ai" => ("AI 自主调试", "AI self-debugging"),
                    "address" => ("本地地址数据", "Local address data"),
                    "rules" => ("导出与阻断默认项", "Export & block defaults"),
                    "notices" => ("第三方声明与许可", "Third-party notices & licences"),
                    "about" => ("关于与许可声明", "About & Licenses"),
                    _ => panic!("未知设置页 {base}"),
                };
                let label = if en { english } else { zh };
                steps = vec![Box::leak(format!("等：{label}").into_boxed_str())];
            } else {
                panic!("未知视图")
            }
        }
    };
    let view_name = args[1].clone();
    let path = PathBuf::from(&args[2]);
    let size = [args[3].parse().unwrap(), args[4].parse().unwrap()];
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(size)
        .with_decorations(false);
    if let Some(icon) = app_icon::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "Parallaxe box · 验收",
        eframe::NativeOptions {
            viewport,
            persist_window: false,
            ..Default::default()
        },
        Box::new(move |cc| {
            install_cjk_fonts(&cc.egui_ctx);
            let reg = Registry::embedded().unwrap();
            let mut app =
                ParallaxeGuiApp::new_with_settings_path(reg.clone(), cc.egui_ctx.clone(), None);
            if matches!(
                view_name.as_str(),
                "default-bundle-scope"
                    | "default-bundle-confirmed"
                    | "default-bundle-result"
                    | "default-bundle-export"
            ) {
                app.confirm_distribution_mode_for_test();
            }
            if matches!(
                view_name.as_str(),
                "default-bundle-confirmed" | "default-bundle-result" | "default-bundle-export"
            ) {
                let indices: Vec<usize> = if view_name == "default-bundle-confirmed" {
                    (1..=107).collect()
                } else {
                    vec![1, 2, 3]
                };
                app.confirm_default_bundle(false, &indices);
                if view_name == "default-bundle-confirmed" {
                    app.navigate_to(ActiveTab::Import);
                } else {
                    let ids: Vec<String> = app
                        .registry()
                        .user_targets()
                        .iter()
                        .take(3)
                        .map(|meta| meta.id.clone())
                        .collect();
                    app.run_user_offline_preview_for_test(ids);
                    app.navigate_to(ActiveTab::Services);
                }
            } else if view_name == "check-home"
                || view_name == "check-home-light"
                || view_name == "check-home-en"
                || view_name == "check-pace"
            {
                let sample = "[办公专线]\n\
https://office-a.example.com/status | rule.http | office-a | 办公内网连通性\n\
[媒体自测]\n\
https://media-b.example.com/trace | rule.trace,rule.http | media-b | 媒体出口自测\n";
                if let Ok(bundle) =
                    parse_import_text_with_default_group(sample, &[], false, Some("我的导入"))
                {
                    let _ = app.registry_mut().import_bundle(&bundle, true, false);
                }
                if view_name == "check-home-light" {
                    app.set_view_modes(DensityMode::Normal, ThemeMode::Light);
                } else if view_name == "check-home-en" {
                    app.set_locale(parallaxe_core::Locale::EnUs);
                } else if view_name == "check-pace" {
                    app.set_pace_panel_open(true);
                }
            } else if view_name == "mobile-import-manual"
                || view_name == "mobile-import-text"
                || view_name == "mobile-import-zip"
            {
                let _ = app.import_view_mut().create_and_select_group("办公专线");
                app.navigate_to(ActiveTab::Import);
                app.import_view_mut().entry_tab = match view_name.as_str() {
                    "mobile-import-text" => ImportEntryTab::TextFile,
                    "mobile-import-zip" => {
                        app.import_view_mut().load_sample_zip_bundle();
                        ImportEntryTab::ZipBundle
                    }
                    _ => ImportEntryTab::Manual,
                };
            } else if view_name == "about-modal" {
                app.set_about_modal_open(true);
            } else if view_name == "notices-modal" {
                app.navigate_to(ActiveTab::Settings);
                app.plugin_host_mut().plugin_notices_open = true;
            } else if matches!(
                view_name.as_str(),
                "dest-mismatch"
                    | "dest-mismatch-ip"
                    | "dest-mismatch-en"
                    | "dest-proxy"
                    | "product-restricted"
                    | "dest-guard-link-ip"
            ) {
                let fixture_id = match view_name.as_str() {
                    "dest-proxy" => "address-proxy",
                    "product-restricted" => "product-restricted",
                    _ => "address-mismatch",
                };
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime");
                let output = rt
                    .block_on(parallaxe_core::runtime::run_check(
                        &reg,
                        &parallaxe_core::runtime::CheckOptions {
                            fixture: Some(fixture_id.to_string()),
                            ..Default::default()
                        },
                        None,
                        false,
                        None,
                    ))
                    .expect("fixture run");
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
                if view_name == "dest-mismatch-ip" || view_name == "dest-guard-link-ip" {
                    app.set_show_ip(true);
                }
                if view_name == "dest-mismatch-en" {
                    app.set_locale(parallaxe_core::Locale::EnUs);
                }
                if view_name == "dest-guard-link-ip" {
                    app.navigate_to(ActiveTab::Guard);
                } else {
                    app.navigate_to(ActiveTab::Services);
                }
            } else if view_name == "ai-f3-modal" || view_name == "ai-f3-modal-en" {
                let loc = if view_name == "ai-f3-modal-en" {
                    app.set_locale(parallaxe_core::Locale::EnUs);
                    parallaxe_core::Locale::EnUs
                } else {
                    parallaxe_core::Locale::ZhCn
                };
                let scope_label = if loc == parallaxe_core::Locale::EnUs {
                    "Quick Check (15)".to_string()
                } else {
                    "简单检查（15 项）".to_string()
                };
                let req = parallaxe_gui::views::ai_debug_panel::AiCheckRequest::sample_for_profile(
                    "simple",
                    scope_label,
                    15,
                    3_500,
                    5,
                    loc,
                );
                app.request_ai_f3_authorization(req);
            } else if view_name == "ai-settings-off" || view_name == "ai-settings-on" {
                app.navigate_to(ActiveTab::Settings);
                app.open_settings_page(parallaxe_gui::views::settings_page::SettingsPage::AiDebug);
                if view_name == "ai-settings-on" {
                    app.set_ai_autonomous_enabled(true);
                }
            } else if let Some(rest) = view_name.strip_prefix("settings-") {
                let base = rest
                    .trim_end_matches("-en")
                    .trim_end_matches("-light")
                    .trim_end_matches("-mobile");
                use parallaxe_gui::views::settings_page::SettingsPage;
                let page = match base {
                    "root" => SettingsPage::Root,
                    "appearance" => SettingsPage::Appearance,
                    "scope" => SettingsPage::Scope,
                    "advanced" => SettingsPage::Advanced,
                    "ai" => SettingsPage::AiDebug,
                    "address" => SettingsPage::AddressBook,
                    "rules" => SettingsPage::Rules,
                    "notices" => SettingsPage::Notices,
                    "about" => SettingsPage::About,
                    _ => SettingsPage::Root,
                };
                app.navigate_to(ActiveTab::Settings);
                if rest.ends_with("-en") {
                    app.set_locale(parallaxe_core::Locale::EnUs);
                }
                if rest.ends_with("-light") {
                    app.set_view_modes(DensityMode::Normal, ThemeMode::Light);
                }
                app.open_settings_page(page);
            } else if view_name == "ai-self-disabled" {
                app.set_ai_autonomous_enabled(true);
                app.ai_self_disable_autonomous(Some("批量验收任务完成，自动收回权限"));
                app.navigate_to(ActiveTab::Overview);
            } else if view_name == "plugin-page" {
                let sample_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("..")
                    .join("plugins")
                    .join("samples")
                    .join("heavy-report-guide");
                app.plugin_host_mut().stage_directory(&sample_dir);
                app.approve_staged_plugin()
                    .expect("approve heavy sample plugin for walkthrough");
            } else if view_name == "mobile-guard-export-en" {
                app.set_locale(parallaxe_core::Locale::EnUs);
                app.navigate_to(parallaxe_gui::layout::ActiveTab::Guard);
                app.guard_view_mut().export_modal_open = true;
            } else if view_name == "guard-export-bundle"
                || view_name == "mobile-guard-export-bundle"
            {
                let now = time::OffsetDateTime::now_utc();
                let recorded_at = parallaxe_core::state::format_time(now);
                let plan = GuardPlan {
                    plan_id: "plan-walkthrough-export".to_string(),
                    run_id: "run-walkthrough-export".to_string(),
                    selected_by: "USER".to_string(),
                    operation: "PREVIEW".to_string(),
                    candidates: vec!["google.gemini".to_string()],
                    affected_domains: vec!["gemini.google.com".to_string()],
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
                    source_run_id: Some("run-source-walkthrough".to_string()),
                };
                let mut log = EventLog::new("run-walkthrough-export", true, now);
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
                    "run-walkthrough-export",
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
            }
            Ok(Box::new(Walk {
                app,
                steps: steps.into(),
                inputs: vec![],
                next: Instant::now() + Duration::from_millis(400),
                deadline: Instant::now() + Duration::from_secs(15),
                shot: path,
                capturing: false,
            }))
        }),
    )
}
