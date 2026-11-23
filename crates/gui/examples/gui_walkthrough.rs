//! Windows 桌面不可截取时，直接读取真实 eframe 窗口的帧缓冲。只编入验收 example。
//! 从界面实际绘出的文字定位按钮，用 egui 指针事件点击；不注入报告，不调用内部 action。
use eframe::egui::{self, Event, Pos2, ViewportCommand};
use parallaxe_core::targets::Registry;
use parallaxe_gui::{app::ParallaxeGuiApp, theme::install_cjk_fonts};
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
                let wait = step.strip_prefix("等：");
                let text = wait.unwrap_or(step);
                if let Some((_, pos)) = visible(ctx).into_iter().find(|(s, _)| s == text) {
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
        "overview" => steps.extend(["总览", "等：共享路径影响"]),
        "overview-en" => steps.extend([
            "总览",
            "等：共享路径影响",
            "EN",
            "等：Network Diagnostics Overview",
        ]),
        "overview-elig" => steps.extend(["资格说明 (3)", "等：未验证账号 (verified=false)"]),
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
                "等：统一导入包（网址与检查方式一体绑定）",
                "EN",
                "等：Unified Import Bundle (URLs & Rules Bound Together)",
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
        "import" => {
            steps = vec!["自定义导入", "等：统一导入包（网址与检查方式一体绑定）"];
        }
        "import-text" => {
            steps = vec![
                "自定义导入",
                "等：统一导入包（网址与检查方式一体绑定）",
                "文本文件 (txt / md / csv)",
                "等：导入包预览（导入前核对「哪个网址用哪些检查方式」）",
            ];
        }
        "import-zip" => {
            steps = vec![
                "自定义导入",
                "等：统一导入包（网址与检查方式一体绑定）",
                "ZIP 导入包 (.zip)",
                "载入内置示例 ZIP 包（含规则+网址）",
                "等：将新增的自定义检查方式",
            ];
        }
        "import-checked" => {
            steps = vec![
                "自定义导入",
                "等：统一导入包（网址与检查方式一体绑定）",
                "文本文件 (txt / md / csv)",
                "等：导入 3 个目标到目录（默认零发包）",
                "我已阅读并同意《导入自定义网址责任声明》（确认属于用户自身行为，后果本软件不负责）",
                "导入 3 个目标到目录（默认零发包）",
                "申请一次性在线检查 (需运行前确认)…",
                "等：【运行前强制确认】一次性会话级联网授权 (F3 许可闸门)",
            ];
        }
        "menus" => steps.extend([
            "设置",
            "六类契约演示",
            "① Drive 302 登录跳转 (UNKNOWN)",
            "等：Google Drive",
            "设置",
            "九项内置样本",
            "gemini-positive",
            "等：US",
        ]),
        _ => panic!("未知视图"),
    };
    let path = PathBuf::from(&args[2]);
    let size = [args[3].parse().unwrap(), args[4].parse().unwrap()];
    eframe::run_native(
        "Parallaxe box · 验收",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(size)
                .with_decorations(false),
            persist_window: false,
            ..Default::default()
        },
        Box::new(move |cc| {
            install_cjk_fonts(&cc.egui_ctx);
            Ok(Box::new(Walk {
                app: ParallaxeGuiApp::new(Registry::embedded().unwrap(), cc.egui_ctx.clone()),
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
