//! Tests: the preview pane's pointer code driven frame by frame on a bare egui context.

use eframe::egui::{self, Context, PointerButton, Pos2, Rect, TouchDeviceId, TouchId, TouchPhase, Ui, pos2};
use wavedude_app_state::Event;
use wavedude_app_state::settings::AppDirs;

use super::Preview;
use crate::App;
use crate::preview::view::{CanvasTransform, View};

/// The area the picture shows in.
pub(super) const PICTURE: Rect = Rect::from_min_max(pos2(0., 0.), pos2(300., 200.));

pub(super) fn touch(id: u64, phase: TouchPhase, pos: Pos2) -> egui::Event {
	egui::Event::Touch { device_id: TouchDeviceId(0), id: TouchId(id), phase, pos, force: None }
}

pub(super) fn press(pos: Pos2) -> egui::Event {
	egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }
}

pub(super) fn release(pos: Pos2) -> egui::Event {
	egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Default::default() }
}

/// A pane on a fresh state, run one frame at a time.
pub(super) struct Frames {
	ctx: Context,
	pub(super) preview: Preview,
	pub(super) app: App,
	_dir: tempfile::TempDir,
}

impl Frames {
	pub(super) fn new() -> Self {
		let dir = tempfile::tempdir().unwrap();
		let app = App::start(&AppDirs::under(dir.path()), []).expect("state starts");
		Self { ctx: Context::default(), preview: Preview::new(None), app, _dir: dir }
	}

	/// One frame given `events`: `pane` runs on the picture's transform and returns the state's events it sent.
	pub(super) fn run(
		&mut self,
		events: Vec<egui::Event>,
		mut pane: impl FnMut(&mut Preview, &Ui, &mut App, &CanvasTransform, &mut Vec<Event>),
	) -> Vec<Event> {
		let input = egui::RawInput { screen_rect: Some(PICTURE), time: Some(0.), events, ..Default::default() };
		let mut out = Vec::new();
		let shown = self.ctx.run_ui(input, |ui| {
			let t = CanvasTransform::new(PICTURE, self.app.document.project.canvas_size(), View::default());
			pane(&mut self.preview, ui, &mut self.app, &t, &mut out);
		});
		shown.drop_without_applying_deltas();
		out
	}
}
