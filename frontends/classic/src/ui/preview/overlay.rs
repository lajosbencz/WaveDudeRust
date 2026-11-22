//! This window's side of the pictures' handle overlays: the overlay painted in its accent, the handles' keyboard focus
//! with Tab through them.

use eframe::egui::{EventFilter, FocusDirection, Key, Modifiers, Rect, Response, Ui};
use waveduderust::model::{ItemRef, Layer};
use waveduderust::unstable::view::overlay::{HandleKind, OverlayModel};
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::preview::view::CanvasTransform;
use waveduderust_egui_preview::overlay::{drive_pointer, focus_selects, handle_keys};

use super::Preview;
use crate::kit::Palette;

/// The index of the handle `focus` names in `model`.
fn focused(model: &OverlayModel, focus: Option<(ItemRef, HandleKind)>) -> Option<usize> {
	let (item, kind) = focus?;
	model.handles.iter().position(|h| h.item == item && h.kind == kind)
}

impl Preview {
	/// Draws the overlay of `layer` and its marquee clipped to `clip`.
	pub(super) fn paint_overlay(&mut self, ui: &Ui, app: &App, layer: Layer, t: &CanvasTransform, clip: Rect) {
		let focus = focused(self.overlays.model(app, layer, t), self.focus[layer as usize]);
		let marquee = self.grab.as_ref().filter(|(l, _)| *l == layer).and_then(|(_, g)| g.marquee());
		let accent = Palette::of(ui.ctx().theme()).accent;
		self.overlays.paint(ui, app, layer, t, clip, focus, marquee, accent);
	}

	/// The handles of `layer` under the picture's `response`: [`drive_pointer`], then the keys while it has the
	/// focus.
	pub(super) fn edit_handles(
		&mut self,
		ui: &Ui,
		response: &Response,
		app: &mut App,
		layer: Layer,
		t: &CanvasTransform,
		out: &mut Vec<Event>,
	) {
		let outcome = drive_pointer(ui, response, app, &mut self.overlays, &mut self.grab, false, layer, t, out);
		if outcome.pressed {
			self.focus[layer as usize] = None;
		}
		if response.has_focus() {
			self.picture_keys(ui, response, app, layer, t, out);
		}
	}

	/// Keys on the focused picture: Tab through the handles (focusing one selects its item, past the ends the focus
	/// leaves the picture), then [`handle_keys`] on the focused handle.
	fn picture_keys(
		&mut self,
		ui: &Ui,
		response: &Response,
		app: &mut App,
		layer: Layer,
		t: &CanvasTransform,
		out: &mut Vec<Event>,
	) {
		let model = self.overlays.model(app, layer, t);
		let focus = &mut self.focus[layer as usize];
		let current = focused(model, *focus);
		let n = model.handles.len();
		ui.memory_mut(|m| {
			let arrows = current.is_some();
			let filter = EventFilter { tab: n > 0, horizontal_arrows: arrows, vertical_arrows: arrows, escape: false };
			m.set_focus_lock_filter(response.id, filter);
		});
		if n > 0 {
			let (forward, back) =
				ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Tab), i.consume_key(Modifiers::SHIFT, Key::Tab)));
			if forward || back {
				let next = match (current, back) {
					(None, false) => Some(0),
					(None, true) => Some(n - 1),
					(Some(i), false) => (i + 1 < n).then_some(i + 1),
					(Some(i), true) => i.checked_sub(1),
				};
				*focus = next.map(|i| (model.handles[i].item, model.handles[i].kind));
				match next {
					Some(i) => focus_selects(app, model.handles[i].item),
					None => ui.memory_mut(|m| {
						m.move_focus(if back { FocusDirection::Previous } else { FocusDirection::Next });
					}),
				}
				return;
			}
		}
		let Some(h) = current.map(|i| &model.handles[i]) else { return };
		if handle_keys(ui, app, h, out) {
			*focus = None;
		}
	}
}
