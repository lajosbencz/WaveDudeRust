//! The system's colour scheme a window's `System` theme setting shows.

use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
use waveduderust_app::settings::Theme as ThemeSetting;

/// The OS colour scheme; winit's report where mundy has none.
#[cfg(not(target_arch = "wasm32"))]
pub fn system_theme(ctx: &egui::Context) -> Option<egui::Theme> {
	match crate::os_theme::current() {
		Some(ThemeSetting::Dark) => Some(egui::Theme::Dark),
		Some(ThemeSetting::Light) => Some(egui::Theme::Light),
		Some(ThemeSetting::System) | None => ctx.system_theme(),
	}
}

/// The page's colour scheme, as eframe reads it.
#[cfg(target_arch = "wasm32")]
pub fn system_theme(ctx: &egui::Context) -> Option<egui::Theme> {
	ctx.system_theme()
}
