//! The splitter between two cells (docs/ui/90, 16) on egui_mineral's Splitter; its keys are the core's.

use eframe::egui::{Rect, Response, UiBuilder};
use egui_mineral::layout::Axis;
use egui_mineral::splitter::Splitter;

use super::{ClassicUi, Palette};

const THICKNESS: f32 = 4.;

/// What [`Split::show`] laid out.
pub(crate) struct SplitOutput {
	pub cells: [Rect; 2],
	pub response: Response,
}

/// A splitter across a room, its share kept per id for the session.
pub(crate) struct Split<'a> {
	salt: &'static str,
	label: &'a str,
	axis: Axis,
	/// The least share of either cell.
	limit: f32,
}

impl<'a> Split<'a> {
	/// Cells side by side; `label` names the handle.
	pub(crate) fn new(salt: &'static str, label: &'a str) -> Self {
		Self { salt, label, axis: Axis::Horizontal, limit: 0. }
	}

	/// Stacked cells when `stacked`.
	pub(crate) fn stacked(mut self, stacked: bool) -> Self {
		self.axis = if stacked { Axis::Vertical } else { Axis::Horizontal };
		self
	}

	pub(crate) fn limit(mut self, limit: f32) -> Self {
		self.limit = limit;
		self
	}

	/// The two cells of `room` and the handle between them.
	pub(crate) fn show(self, ui: &mut ClassicUi<'_>, room: Rect) -> SplitOutput {
		let rt = ui.rt();
		let vertical = self.axis == Axis::Vertical;
		let length = if vertical { room.height() } else { room.width() } - THICKNESS;
		let builder = UiBuilder::new().max_rect(room).id_salt(self.salt);
		let out = ui
			.scope_builder(builder, |ui| {
				let ui = &mut rt.ui(ui);
				let splitter = Splitter::new(self.salt, self.label)
					.axis(self.axis)
					.handle_thickness(THICKNESS)
					.min_first(self.limit * length)
					.min_second(self.limit * length);
				splitter.show(ui, |ui| ui.max_rect(), |ui| ui.max_rect())
			})
			.inner;
		let response = out.response.on_hover_text(self.label);
		let palette = Palette::of(ui.ctx().theme());
		let color = if response.has_focus() || response.dragged() { palette.accent } else { palette.window_background };
		ui.painter().rect_filled(out.handle_rect, 0., color);
		SplitOutput { cells: [out.first, out.second], response }
	}
}
