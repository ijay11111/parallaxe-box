//! 平台感知布局回归：Android 横屏必须保持移动布局族，桌面窄窗口行为不变。

use parallaxe_gui::layout::{LayoutSpec, OverviewVariant, ViewportTier};
use parallaxe_gui::theme::DensityMode;

#[test]
fn desktop_1078_stays_compact_dashboard() {
    let spec = LayoutSpec::compute_for_platform(DensityMode::Normal, 1078.0, false);
    assert_eq!(spec.tier, ViewportTier::Compact);
    assert!(!spec.mobile_layout);
    assert!(spec.show_desktop_tabs);
    assert!(!spec.show_bottom_nav);
    assert!(spec.show_desktop_status_bar);
    assert_eq!(spec.minimal_card_columns, 3);
    assert_eq!(spec.summary_columns, 6);
    assert_eq!(spec.min_touch_height, 32.0);
}

#[test]
fn android_landscape_1078_uses_mobile_family() {
    let spec = LayoutSpec::compute_for_platform(DensityMode::Normal, 1078.0, true);
    assert_eq!(spec.tier, ViewportTier::Narrow);
    assert!(spec.mobile_layout);
    assert!(spec.show_bottom_nav, "横屏仍需底部四栏导航");
    assert!(!spec.show_desktop_tabs, "横屏不得出现桌面顶部 Tab");
    assert!(!spec.show_desktop_status_bar);
    assert_eq!(spec.overview_variant, OverviewVariant::MobileNormalStacked);
    assert_eq!(spec.minimal_card_columns, 2, "横屏最多两列卡片");
    assert_eq!(spec.dual_stack_columns, 2);
    assert_eq!(spec.bottom_split_columns, 2);
    assert_eq!(spec.summary_columns, 2);
    assert!(spec.touch_friendly);
    assert_eq!(spec.min_touch_height, 44.0);
}

#[test]
fn android_landscape_1600_still_caps_at_two_columns() {
    let spec = LayoutSpec::compute_for_platform(DensityMode::Normal, 1600.0, true);
    assert_eq!(spec.tier, ViewportTier::Narrow);
    assert!(spec.show_bottom_nav);
    assert!(!spec.show_desktop_tabs);
    assert!(spec.minimal_card_columns <= 2);
    assert!(spec.dual_stack_columns <= 2);
    assert!(spec.bottom_split_columns <= 2);
}

#[test]
fn android_portrait_420_is_single_column() {
    let spec = LayoutSpec::compute_for_platform(DensityMode::Normal, 420.0, true);
    assert_eq!(spec.tier, ViewportTier::Narrow);
    assert!(spec.show_bottom_nav);
    assert_eq!(spec.minimal_card_columns, 1);
    assert_eq!(spec.dual_stack_columns, 1);
    assert_eq!(spec.bottom_split_columns, 1);
    assert_eq!(spec.summary_columns, 1, "竖屏摘要卡也必须单列");
    assert_eq!(spec.min_touch_height, 44.0);
}

#[test]
fn android_minimal_density_uses_mobile_minimal_variant() {
    let spec = LayoutSpec::compute_for_platform(DensityMode::Minimal, 1078.0, true);
    assert_eq!(spec.overview_variant, OverviewVariant::MobileMinimalStacked);
    assert!(!spec.show_desktop_tabs);
    assert_eq!(spec.minimal_card_columns, 2);
}

#[test]
fn desktop_narrow_window_behaviour_is_unchanged() {
    let spec = LayoutSpec::compute_for_platform(DensityMode::Normal, 800.0, false);
    assert_eq!(spec.tier, ViewportTier::Narrow);
    assert!(!spec.mobile_layout);
    assert!(spec.show_bottom_nav);
    assert!(!spec.show_desktop_tabs);
    assert_eq!(spec.minimal_card_columns, 1);
    assert_eq!(spec.min_touch_height, 44.0);
}

#[test]
fn compute_matches_desktop_platform_entry() {
    for (density, width) in [
        (DensityMode::Normal, 420.0),
        (DensityMode::Normal, 800.0),
        (DensityMode::Normal, 1078.0),
        (DensityMode::Normal, 1280.0),
        (DensityMode::Minimal, 1078.0),
        (DensityMode::Minimal, 1600.0),
    ] {
        assert_eq!(
            LayoutSpec::compute(density, width),
            LayoutSpec::compute_for_platform(density, width, false),
            "compute() 必须保持桌面既有行为（{density:?} {width}）"
        );
    }
}
