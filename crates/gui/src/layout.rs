//! 响应式断点、三维度正交布局规范与页签定义。
//!
//! 三个正交维度：
//! 1. `DensityMode`（`Normal` 完整仪表盘 vs `Minimal` 三卡极简）
//! 2. `ThemeMode`（`Dark` vs `Light` —— 纯配色，绝不改变 `LayoutSpec`）
//! 3. `ViewportTier`（`Wide` >= 1120px / `Compact` 840..1119px / `Narrow` < 840px）

use crate::theme::DensityMode;
use parallaxe_core::i18n::{translate, Locale};

/// 窗口宽度断点分档（窄屏 `< 840px` 自动切换移动式单列堆叠 + 底部四栏导航）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportTier {
    /// `>= 1120px`：桌面宽屏。
    Wide,
    /// `840..1119px`：桌面中等屏（自动收拢次要副标题、折叠抽屉）。
    Compact,
    /// `< 840px`：移动/窄屏单列堆叠 + 底部四栏导航。
    Narrow,
}

impl ViewportTier {
    pub fn from_width(width: f32) -> Self {
        if width >= 1120.0 {
            ViewportTier::Wide
        } else if width >= 840.0 {
            ViewportTier::Compact
        } else {
            ViewportTier::Narrow
        }
    }
}

/// 总览页结构变体（由 `DensityMode × ViewportTier` 唯一决定，与 `ThemeMode` 无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewVariant {
    /// 桌面 normal 完整仪表盘：状态摘要条 + 双栈并列卡 + 服务矩阵 + 底部时间线/风险分栏。
    DesktopNormalDashboard,
    /// 桌面 minimal 极简三卡：大标题 + 主按钮 + 三个等高并列卡（当前路径与覆盖 / 重要服务 / 需要注意）。
    DesktopMinimalThreeCards,
    /// 窄屏/移动 normal：单列堆叠完整诊断卡 + 底部四栏导航。
    MobileNormalStacked,
    /// 窄屏/移动 minimal：单列堆叠三卡 + 通栏大检测按钮 + 底部四栏导航。
    MobileMinimalStacked,
}

/// 三端安全区留白（Android 状态栏/手势条；桌面与 Web 默认为 0）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SafeAreaInsets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl SafeAreaInsets {
    pub fn for_platform() -> Self {
        if cfg!(target_os = "android") {
            SafeAreaInsets {
                top: 28.0,
                bottom: 20.0,
                left: 0.0,
                right: 0.0,
            }
        } else {
            SafeAreaInsets {
                top: 0.0,
                bottom: 0.0,
                left: 0.0,
                right: 0.0,
            }
        }
    }
}

/// 纯函数式布局规格（不依赖 `ThemeMode`，从结构上确保明暗切换不改变布局）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutSpec {
    pub tier: ViewportTier,
    pub density: DensityMode,
    pub overview_variant: OverviewVariant,
    /// 桌面 normal 显示顶部主工作区 Tab；minimal 极简视图隐藏多余 Tab 干扰。
    pub show_desktop_tabs: bool,
    /// 窄屏 `< 840px` 显示底部四栏导航（总览 / 服务 / 方案 / 设置）。
    pub show_bottom_nav: bool,
    /// 桌面宽屏/中等屏显示 26px 底部只读状态栏；窄屏并入「设置/更多」避免占高。
    pub show_desktop_status_bar: bool,
    /// 顶部副标题是否显示（宽屏显示，缩小时自动让位避免挤压）。
    pub show_top_subtitle: bool,
    /// 状态摘要指标卡列数（6 / 3 / 2 动态折行）。
    pub summary_columns: usize,
    /// 双栈并列卡列数（桌面 2 列，窄屏 1 列）。
    pub dual_stack_columns: usize,
    /// Minimal 三卡列数（桌面 3 列等宽等高，窄屏 1 列堆叠）。
    pub minimal_card_columns: usize,
    /// 底部时间线/风险分栏列数（桌面 2 列，窄屏 1 列）。
    pub bottom_split_columns: usize,
    /// 侧抽屉目标宽度（随窗口宽度弹性缩放，永不挤占主区）。
    pub drawer_width: f32,
    /// 是否运行在移动平台（Android）。为 true 时无论横竖屏都使用移动布局族，
    /// 不会因为横屏宽度 >= 840px 被误判成桌面 Compact/Wide。
    pub mobile_layout: bool,
    /// 移动端 / 窄屏触控友好标志（启用 >= 44dp 最小触控高度与键盘避让）。
    pub touch_friendly: bool,
    /// 交互控件最小高度（窄屏/Android >= 44.0dp，桌面 32.0px）。
    pub min_touch_height: f32,
    /// 当前平台安全区留白。
    pub safe_area: SafeAreaInsets,
}

impl LayoutSpec {
    /// 桌面/Web 入口：只按宽度分档（保持既有行为）。
    pub fn compute(density: DensityMode, width: f32) -> Self {
        Self::compute_for_platform(density, width, false)
    }

    /// 平台感知入口：移动平台（Android）强制移动布局族，横屏不会被当成桌面 dashboard。
    pub fn compute_for_platform(
        density: DensityMode,
        width: f32,
        is_mobile_platform: bool,
    ) -> Self {
        let width_tier = ViewportTier::from_width(width);
        // 移动平台横屏可能 >= 840px：仍固定 Narrow 档，让下游视图选择、字体层级、
        // 底部导航与返回键行为都走移动端分支。
        let tier = if is_mobile_platform {
            ViewportTier::Narrow
        } else {
            width_tier
        };
        let overview_variant = match (density, tier) {
            (DensityMode::Normal, ViewportTier::Narrow) => OverviewVariant::MobileNormalStacked,
            (DensityMode::Normal, _) => OverviewVariant::DesktopNormalDashboard,
            (DensityMode::Minimal, ViewportTier::Narrow) => OverviewVariant::MobileMinimalStacked,
            (DensityMode::Minimal, _) => OverviewVariant::DesktopMinimalThreeCards,
        };
        // 移动端横屏 >= 640px 允许两列，避免单列过稀；任何情况都不超过 2 列。
        let mobile_two_columns = is_mobile_platform && width >= 640.0;
        let summary_columns = if is_mobile_platform {
            if mobile_two_columns {
                2
            } else {
                1
            }
        } else if width >= 1040.0 {
            6
        } else if width >= 640.0 {
            3
        } else {
            2
        };
        let is_narrow = tier == ViewportTier::Narrow;
        let touch_friendly = is_narrow || is_mobile_platform;
        LayoutSpec {
            tier,
            density,
            overview_variant,
            show_desktop_tabs: !is_mobile_platform && !is_narrow && density == DensityMode::Normal,
            show_bottom_nav: is_mobile_platform || is_narrow,
            show_desktop_status_bar: !is_mobile_platform && !is_narrow,
            show_top_subtitle: !is_mobile_platform && width >= 1080.0,
            summary_columns,
            dual_stack_columns: if is_mobile_platform {
                if mobile_two_columns {
                    2
                } else {
                    1
                }
            } else if is_narrow {
                1
            } else {
                2
            },
            minimal_card_columns: if is_mobile_platform {
                if mobile_two_columns {
                    2
                } else {
                    1
                }
            } else if is_narrow {
                1
            } else {
                3
            },
            bottom_split_columns: if is_mobile_platform {
                if mobile_two_columns {
                    2
                } else {
                    1
                }
            } else if is_narrow {
                1
            } else {
                2
            },
            drawer_width: (width * 0.34).clamp(300.0, 440.0),
            mobile_layout: is_mobile_platform,
            touch_friendly,
            min_touch_height: if touch_friendly { 44.0 } else { 32.0 },
            safe_area: SafeAreaInsets::for_platform(),
        }
    }
}

/// 桌面 normal 主工作区 Tab（6 个功能视图，含自定义导入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActiveTab {
    #[default]
    Overview,
    Services,
    Coverage,
    Guard,
    Timeline,
    Import,
    Settings,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    PluginPage,
}

impl ActiveTab {
    pub fn label(self) -> &'static str {
        match self {
            ActiveTab::Overview => "总览",
            ActiveTab::Services => "服务矩阵",
            ActiveTab::Coverage => "覆盖明细",
            ActiveTab::Guard => "处置方案",
            ActiveTab::Timeline => "时间线",
            ActiveTab::Import => "自定义导入",
            ActiveTab::Settings => "设置",
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            ActiveTab::PluginPage => "扩展页面",
        }
    }

    pub fn label_localized(self, locale: Locale) -> String {
        let key = match self {
            ActiveTab::Overview => "gui.tab.overview",
            ActiveTab::Services => "gui.tab.services",
            ActiveTab::Coverage => "gui.tab.coverage",
            ActiveTab::Guard => "gui.tab.guard",
            ActiveTab::Timeline => "gui.tab.timeline",
            ActiveTab::Import => "gui.tab.import",
            ActiveTab::Settings => "gui.tab.settings",
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            ActiveTab::PluginPage => "gui.tab.plugin_page",
        };
        translate(locale, key, &[])
    }

    pub const ALL: [ActiveTab; 6] = [
        ActiveTab::Overview,
        ActiveTab::Services,
        ActiveTab::Coverage,
        ActiveTab::Guard,
        ActiveTab::Timeline,
        ActiveTab::Import,
    ];
}

/// 窄屏/移动底部四栏导航（与参考图 -4.png / -1.png / -2.png 四栏结构对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MobileTab {
    #[default]
    Overview,
    Services,
    Plan,
    Settings,
}

impl MobileTab {
    pub fn label(self) -> &'static str {
        match self {
            MobileTab::Overview => "总览",
            MobileTab::Services => "服务",
            MobileTab::Plan => "方案",
            MobileTab::Settings => "设置",
        }
    }

    pub fn label_localized(self, locale: Locale) -> String {
        let key = match self {
            MobileTab::Overview => "gui.tab.overview",
            MobileTab::Services => "gui.mobile.services",
            MobileTab::Plan => "gui.mobile.plan",
            MobileTab::Settings => "gui.tab.settings",
        };
        translate(locale, key, &[])
    }

    pub const ALL: [MobileTab; 4] = [
        MobileTab::Overview,
        MobileTab::Services,
        MobileTab::Plan,
        MobileTab::Settings,
    ];
}
