//! Parallaxe box 原生 egui/eframe 界面。
//!
//! 架构红线：
//! - 本 crate 只消费 `parallaxe-core` 产出的事件流与报告，不含任何判定、网络探测或正则提取逻辑。
//! - 全部 9 个 `State` 均做穷举渲染，绝不使用 `_ =>` 兜底，也不从 HTTP 状态码或时延推导绿色/正常。
//! - 仅绑定本机窗口，不监听端口、不嵌浏览器、不读取账号或 Cookie。

#![deny(unsafe_code)]

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub mod ai_bridge_host;
#[cfg(not(target_arch = "wasm32"))]
pub mod app;
#[cfg(not(target_arch = "wasm32"))]
pub mod app_icon;
#[cfg(not(target_arch = "wasm32"))]
pub mod commands;
#[cfg(not(target_arch = "wasm32"))]
pub mod folder_picker;
#[cfg(not(target_arch = "wasm32"))]
pub mod icons;
#[cfg(not(target_arch = "wasm32"))]
pub mod layout;
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub mod plugin_host;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod plugin_ui;
#[cfg(not(target_arch = "wasm32"))]
pub mod schedule_ui;
#[cfg(not(target_arch = "wasm32"))]
pub mod settings_store;
#[cfg(not(target_arch = "wasm32"))]
pub mod state;
#[cfg(not(target_arch = "wasm32"))]
pub mod theme;
#[cfg(not(target_arch = "wasm32"))]
pub mod views;

#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(not(target_arch = "wasm32"))]
use parallaxe_core::targets::Registry;

/// 启动原生 egui 窗口（仅在主线程调用）。
#[cfg(not(target_arch = "wasm32"))]
pub fn run_gui(registry: Registry) -> Result<(), String> {
    app::run_native(registry)
}

/// Android 原生入口：由 game-activity 的 Java 胶水调用。
/// 不创建 VpnService / TUN（单槽冲突，见 ADR-006）；只提供三端一致的只读 GUI。
#[cfg(target_os = "android")]
// 单一平台 FFI 入口：android-activity 的 Java 胶水按符号名调用，必须 no_mangle。
// 这是本 crate 唯一允许的 unsafe 属性使用，其余代码仍受 deny(unsafe_code) 约束。
#[allow(unsafe_code)]
#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn android_main(app: android_activity::AndroidApp) {
    eprintln!("{}", parallaxe_core::distribution::BUILD_MARKER);
    let registry = match Registry::embedded() {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("Parallaxe box 目录加载失败：{error}");
            return;
        }
    };
    if let Err(error) = app::run_native_android(registry, app) {
        eprintln!("Parallaxe box Android 启动失败：{error}");
    }
}
