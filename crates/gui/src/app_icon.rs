//! Embedded first-party application icon loader for desktop windows.

use std::sync::Arc;

use egui::IconData;

const ICON_64_PNG: &[u8] = include_bytes!("../../../assets/icons/icon-64.png");

/// Decodes the embedded 64x64 RGBA PNG into an `egui::IconData` for window / taskbar display.
pub fn window_icon() -> Option<Arc<IconData>> {
    let decoded = image::load_from_memory_with_format(ICON_64_PNG, image::ImageFormat::Png).ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(Arc::new(IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    }))
}
