//! The desktop's side of a window: its title, its place, the clipboard and the leave guard.

use eframe::egui;
use waveduderust_app::clipboard::read_fragment;
use waveduderust_app::menus::On;
use waveduderust_app::settings::Settings;
use waveduderust_app::window::MIN_SIZE;

use crate::placement::{self, Place};

pub fn set_title(ctx: &egui::Context, title: &str) {
	ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_owned()));
}

/// Scales the window by `ratio` after a change to `zoom`.
pub fn rezoom(frame: &eframe::Frame, ratio: f64, zoom: f64) {
	if let Some(window) = frame.winit_window() {
		placement::rezoom(window, ratio, (MIN_SIZE.0 * zoom, MIN_SIZE.1 * zoom));
	}
}

/// Writes the window's place into `settings`.
pub fn remember_place(frame: &eframe::Frame, settings: &mut Settings) {
	if let Some(window) = frame.winit_window() {
		Place::of(window, settings.ui_zoom).save(settings);
	}
}

/// The clipboard's text.
fn clipboard_text() -> Option<String> {
	arboard::Clipboard::new()
		.and_then(|mut c| c.get_text())
		.inspect_err(|e| tracing::debug!("nothing to paste: {e}"))
		.ok()
}

/// Whether the clipboard's text is a fragment of objects to paste.
pub fn pastable() -> bool {
	clipboard_text().is_some_and(|text| read_fragment(&text).is_ok())
}

/// The clipboard's text for a paste.
pub fn read_clipboard(_ctx: &egui::Context, _on: On) -> Option<String> {
	clipboard_text()
}

/// Read synchronously by [`read_clipboard`].
pub fn pasted() -> Option<(String, On)> {
	None
}

/// Nothing to listen for: the close box asks about unsaved edits through the state.
#[derive(Default)]
pub struct LeaveGuard(());

impl LeaveGuard {
	pub fn set(&mut self, _on: bool) {}
}
