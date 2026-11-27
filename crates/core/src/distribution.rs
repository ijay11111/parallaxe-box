//! 分发模式边界：源码构建默认零发包，正式发行流水线通过 Cargo feature 显式开启。
//!
//! 该常量在编译期确定，运行时不能通过配置文件、环境变量或设置页伪造。
//! 源码构建仍可由用户在「关于软件」中显式确认后切到发行包模式；关闭后恢复零发包。

/// 当前二进制是否由正式发行流水线构建。
pub const DISTRIBUTION_RELEASE_BUILD: bool = cfg!(feature = "distribution-release");

/// 嵌入产物的唯一编译期标记，供 CLI / Web / Android 发布门禁只读校验。
pub const BUILD_MARKER: &str = if DISTRIBUTION_RELEASE_BUILD {
    "parallaxe.distribution-release/v1"
} else {
    "parallaxe.source-build/v1"
};

/// 独立构建信息 JSON；不写入 `parallaxe.report/v1`，避免改变报告 schema。
pub fn build_info_json() -> String {
    serde_json::json!({
        "schema": "parallaxe.build-info/v1",
        "version": crate::VERSION,
        "distribution_release_build": DISTRIBUTION_RELEASE_BUILD,
        "marker": BUILD_MARKER,
    })
    .to_string()
}
