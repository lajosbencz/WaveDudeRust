//! The desktop's side of the window: the OS colour scheme and the window's place.

use eframe::egui;
use wavedude_app_state::clipboard::read_fragment;
use wavedude_app_state::settings::{Settings, Theme};

use crate::placement::{self, Place};
use crate::window::MIN_SIZE;

/// The OS colour scheme; winit's report where mundy has none.
pub(super) fn system_theme(ctx: &egui::Context) -> Option<egui::Theme> {
	match crate::os_theme::current() {
		Some(Theme::Dark) => Some(egui::Theme::Dark),
		Some(Theme::Light) => Some(egui::Theme::Light),
		Some(Theme::System) | None => ctx.system_theme(),
	}
}

/// The window's title.
pub(super) fn set_title(ctx: &egui::Context, title: &str) {
	ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_owned()));
}

/// Scales the window by `ratio` after a change to `zoom`.
pub(super) fn rezoom(frame: &eframe::Frame, ratio: f64, zoom: f64) {
	if let Some(window) = frame.winit_window() {
		placement::rezoom(window, ratio, (MIN_SIZE.0 * zoom, MIN_SIZE.1 * zoom));
	}
}

/// Writes the window's place into `settings`.
pub(super) fn remember_place(frame: &eframe::Frame, settings: &mut Settings) {
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
pub(super) fn pastable() -> bool {
	clipboard_text().is_some_and(|text| read_fragment(&text).is_ok())
}

/// The clipboard's text for a paste.
pub(super) fn read_clipboard(_ctx: &egui::Context) -> Option<String> {
	clipboard_text()
}

/// Read synchronously by [`read_clipboard`].
pub(super) fn pasted() -> Option<String> {
	None
}

/// Nothing to listen for: the close box asks about unsaved edits through the state.
#[derive(Default)]
pub(super) struct LeaveGuard(());

impl LeaveGuard {
	pub(super) fn set(&mut self, _on: bool) {}
}
