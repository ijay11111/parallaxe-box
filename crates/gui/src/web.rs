//! Web/WASM 端入口：离线解释 + 剪贴板导出（ADR-006：Web 不做系统阻断）。
//!
//! wasm32 下没有系统 DNS / 原生 TCP 能力，core 的 `WasmTransport` 会返回明确的
//! `WASM_OFFLINE`，判定语义不变；本界面只投影 core 的静态报告，不发送任何网络请求。

use std::cell::RefCell;
use std::rc::Rc;

use eframe::egui;
use parallaxe_core::contracts::{Coverage, Mode, Report};
use parallaxe_core::reports;
use parallaxe_core::targets::Registry;
use parallaxe_core::Locale;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Web 端只读应用容器。
struct WebApp {
    registry: Registry,
    status: String,
    locale: Locale,
    pending: Rc<RefCell<Option<String>>>,
}

impl WebApp {
    fn new(registry: Registry) -> Self {
        WebApp {
            registry,
            status: "就绪：离线解释，不发送网络请求".to_string(),
            locale: Locale::ZhCn,
            pending: Rc::new(RefCell::new(None)),
        }
    }

    fn offline_report(&self, locale: Locale) -> String {
        let mut report = Report::new(
            parallaxe_core::VERSION,
            "离线占位（Web 端无系统时间源）",
            "run-web-offline",
            Mode::Fixture,
        );
        report.coverage = Coverage::empty(self.registry.catalog_size());
        report.exit_code = 3;
        reports::to_markdown(&report, locale)
    }

    fn copy(&self, ctx: &egui::Context) {
        let text = self.offline_report(self.locale);
        let Some(clipboard) = web_sys::window().map(|window| window.navigator().clipboard()) else {
            return;
        };
        let pending = self.pending.clone();
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let message = match clipboard.write_text(&text).await {
                Ok(_) => "已复制 Markdown 报告到剪贴板".to_string(),
                Err(_) => "剪贴板写入失败：请手动复制".to_string(),
            };
            *pending.borrow_mut() = Some(message);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for WebApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(message) = self.pending.borrow_mut().take() {
            self.status = message;
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Parallaxe box · Web（只读）");
            ui.add_space(8.0);
            ui.label("本端为离线解释：不进行网络探测、不写系统、不做规则替换。");
            ui.label(format!(
                "目录共 {} 项目标；真实检测请使用桌面端（同一 core 与同一判定）。",
                self.registry.catalog_size()
            ));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("复制 Markdown 报告（离线占位）").clicked() {
                    self.copy(ctx);
                }
                if ui.button("切换语言").clicked() {
                    self.locale = match self.locale {
                        Locale::ZhCn => Locale::EnUs,
                        Locale::EnUs => Locale::ZhCn,
                    };
                }
            });
            ui.add_space(8.0);
            ui.label(&self.status);
            ui.separator();
            ui.label("能力边界：Web 端不提供系统阻断、不回环订阅、不按 IP/CDN/ASN 阻断。");
        });
    }
}

/// wasm 入口：由 trunk/wasm-bindgen 注入到页面。
#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    let registry = Registry::embedded().map_err(|error| JsValue::from_str(&error.to_string()))?;
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("缺少 document"))?;
    let canvas = document
        .get_element_by_id("parallaxe_canvas")
        .ok_or_else(|| JsValue::from_str("页面缺少 #parallaxe_canvas"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("#parallaxe_canvas 不是 canvas 元素"))?;
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(move |_cc| Ok(Box::new(WebApp::new(registry)))),
        )
        .await
}
