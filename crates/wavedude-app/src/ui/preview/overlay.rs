//! A picture's handle overlay (docs/ui/40-preview-pane.md, Handles and overlays, Interaction): the overlay model,
//! built again when the document, the selection or the scale changed, drawn as the view's shapes clipped to the
//! picture with the marquee; the presses, drags and keys on the handles that change the selection and edit the
//! model, and the handles' tooltips.

use eframe::egui::{EventFilter, FocusDirection, Key, Modifiers, Rect, Response, Shape, Stroke, Ui, Vec2};
use wavedude_app_state::Event;
use wavedude_app_state::commands::{Command, Edit};
use wavedude_app_state::selection::Row;
use wavedude_core::model::{ItemRef, Layer};
use wavedude_view::overlay::{self as view, HandleKind, OverlayModel};

use super::Preview;
use crate::App;
use crate::preview::handles::{DRAG_THRESHOLD, MarqueeMode, drag_delta, hit, marquee_hits, marquee_select};
use crate::preview::overlay::{
	Grab, anchor_of, handle_edits, handle_tip, model_of, moves_by_edit, nudge_edits, remember,
};
use crate::preview::view::CanvasTransform;
use crate::ui::theme::Palette;

/// The marquee's fill over the accent.
const MARQUEE_FILL: f32 = 0.15;

/// A picture's overlay model and what it was built from: the document and selection revisions and the scale.
pub(super) struct Overlay {
	key: (u64, u64, u32),
	model: OverlayModel,
}

fn ctrl(m: Modifiers) -> bool {
	m.ctrl || m.command
}

/// The index of the handle `focus` names in `model`.
fn focused(model: &OverlayModel, focus: Option<(ItemRef, HandleKind)>) -> Option<usize> {
	let (item, kind) = focus?;
	model.handles.iter().position(|h| h.item == item && h.kind == kind)
}

fn send_edits(out: &mut Vec<Event>, edits: Vec<Edit>) {
	out.extend(edits.into_iter().map(|e| Event::Command(Command::Edit(e))));
}

impl Preview {
	/// The overlay of `layer` at `t`'s scale, built when what it shows changed; taken out of the pane while used.
	fn take_overlay(&mut self, app: &App, layer: Layer, t: &CanvasTransform) -> Overlay {
		let r = app.revisions();
		let key = (r.document, r.selection, t.scale.to_bits());
		match self.overlays[layer as usize].take() {
			Some(o) if o.key == key => o,
			_ => Overlay { key, model: model_of(app, layer, t.scale) },
		}
	}

	/// Draws the overlay of `layer` and its marquee clipped to `clip`.
	pub(super) fn paint_overlay(&mut self, ui: &Ui, app: &App, layer: Layer, t: &CanvasTransform, clip: Rect) {
		let overlay = self.take_overlay(app, layer, t);
		let ctx = ui.ctx();
		self.outlines.set_pixels_per_point(ctx.pixels_per_point());
		let accent = Palette::of(ctx.theme()).accent;
		let focus = focused(&overlay.model, self.focus[layer as usize]);
		let shapes =
			ctx.fonts_mut(|f| view::shapes(&overlay.model, t.scale, t.picture(), focus, accent, f, &mut self.outlines));
		let painter = ui.painter().with_clip_rect(clip);
		painter.extend(shapes);
		if let Some((l, Grab::Marquee { a, b, .. })) = &self.grab
			&& *l == layer
		{
			let r = Rect::from_two_pos(t.to_screen(a.0, a.1), t.to_screen(b.0, b.1));
			painter.rect_filled(r, 0., accent.gamma_multiply(MARQUEE_FILL));
			let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
			painter.extend(Shape::dashed_line(&corners, Stroke::new(1., accent), 4., 3.));
		}
		self.overlays[layer as usize] = Some(overlay);
	}

	/// The handles of `layer` under the picture's `response`: presses, drags, Esc, keys while it has the focus, and
	/// the tooltip of the handle under a resting pointer.
	pub(super) fn edit_handles(
		&mut self,
		ui: &Ui,
		response: &Response,
		app: &mut App,
		layer: Layer,
		t: &CanvasTransform,
		out: &mut Vec<Event>,
	) {
		let overlay = self.take_overlay(app, layer, t);
		let model = &overlay.model;
		let (pointer, modifiers, pressed, released, moving) = ui.input(|i| {
			let p = &i.pointer;
			(p.interact_pos(), i.modifiers, p.primary_pressed(), p.primary_released(), p.delta() != Vec2::ZERO)
		});
		let at = pointer.map(|p| t.to_canvas(p));
		let grabbed = self.grab.as_ref().is_some_and(|(l, _)| *l == layer);
		if let Some(c) = at {
			if pressed && response.hovered() {
				self.press(app, layer, model, t.scale, (c, modifiers), out);
			} else if grabbed && moving {
				self.drag(app, c, t.scale, modifiers.shift, out);
			}
		}
		if released && self.grab.as_ref().is_some_and(|(l, _)| *l == layer) {
			self.release(app, model, t.scale, out);
		}
		if self.grab.is_some() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
			self.cancel(app, out);
		}
		if response.has_focus() {
			self.handle_keys(ui, response, app, layer, model, out);
		}
		if self.grab.is_none()
			&& let Some(i) = response.hover_pos().and_then(|p| hit(model, t.to_canvas(p), t.scale))
		{
			response.clone().on_hover_text_at_pointer(handle_tip(app, &model.handles[i]));
		}
		self.overlays[layer as usize] = Some(overlay);
	}

	/// A primary press at `c` with `m` held: on a handle it selects (Ctrl toggles, Shift adds) and grabs its item,
	/// elsewhere it starts a marquee.
	fn press(
		&mut self,
		app: &mut App,
		layer: Layer,
		model: &OverlayModel,
		scale: f32,
		(c, m): ((f32, f32), Modifiers),
		out: &mut Vec<Event>,
	) {
		self.focus[layer as usize] = None;
		let Some(h) = hit(model, c, scale).map(|i| &model.handles[i]) else {
			let mode = if ctrl(m) {
				MarqueeMode::Toggle
			} else if m.shift {
				MarqueeMode::Add
			} else {
				MarqueeMode::Replace
			};
			self.grab = Some((layer, Grab::Marquee { a: c, b: c, mode }));
			return;
		};
		let (item, selection) = (h.item, &mut app.selection);
		if ctrl(m) {
			selection.toggle(item);
			return;
		}
		if m.shift {
			if !selection.contains(item) {
				let mut items = selection.items().to_vec();
				items.push(item);
				selection.set_items(items);
			}
			return;
		}
		let group = h.kind.moves_item() && selection.contains(item) && selection.items().len() > 1;
		if !group && selection.current_item() != Some(item) {
			selection.select(Row::Item(item));
		}
		let p = &app.document.project;
		let grab = if !group && moves_by_edit(p, item, h.kind) {
			Grab::Edit { item, kind: h.kind, before: Vec::new() }
		} else {
			let origin = anchor_of(p, item).expect("a handle's item exists");
			Grab::Items { item, press: c, origin, group, moved: false }
		};
		out.push(Event::MouseDown);
		self.grab = Some((layer, grab));
	}

	/// The pointer at `c` while grabbing: whole items move by whole pixels once past the drag threshold, a handle
	/// edits its fields (`snap`: Shift on a lever), a marquee follows.
	fn drag(&mut self, app: &App, c: (f32, f32), scale: f32, snap: bool, out: &mut Vec<Event>) {
		let Some((_, grab)) = &mut self.grab else { return };
		let p = &app.document.project;
		match grab {
			Grab::Items { item, press, origin, moved, .. } => {
				let anchor = anchor_of(p, *item).unwrap_or(*origin);
				let (dx, dy) = drag_delta(*press, c, (anchor.0 - origin.0, anchor.1 - origin.1));
				let far = (c.0 - press.0).hypot(c.1 - press.1) * scale >= DRAG_THRESHOLD;
				if (dx, dy) != (0, 0) && (*moved || far) {
					out.push(Event::Command(Command::MoveBy { target: *item, dx, dy }));
					*moved = true;
				}
			}
			Grab::Edit { item, kind, before } => {
				let edits = handle_edits(p, *item, *kind, c, snap);
				remember(before, edits.iter().filter_map(|e| e.current(p)).collect());
				send_edits(out, edits);
			}
			Grab::Marquee { b, .. } => *b = c,
		}
	}

	/// The primary button let go: a group grab that did not move selects its item alone; a marquee selects what it
	/// spans, or deselects as a click on empty space.
	fn release(&mut self, app: &mut App, model: &OverlayModel, scale: f32, out: &mut Vec<Event>) {
		match self.grab.take().map(|(_, g)| g) {
			Some(Grab::Items { item, group, moved, .. }) => {
				if group && !moved {
					app.selection.select(Row::Item(item));
				}
				out.push(Event::MouseUp);
			}
			Some(Grab::Edit { .. }) => out.push(Event::MouseUp),
			Some(Grab::Marquee { a, b, mode }) => {
				if (b.0 - a.0).hypot(b.1 - a.1) * scale < DRAG_THRESHOLD {
					if mode == MarqueeMode::Replace {
						app.deselect();
					}
				} else {
					let items = marquee_select(app.selection.items(), &marquee_hits(model, a, b), mode);
					app.selection.set_items(items);
				}
			}
			None => {}
		}
	}

	/// Esc during a drag: puts the item, the group or the fields back.
	fn cancel(&mut self, app: &App, out: &mut Vec<Event>) {
		match self.grab.take().map(|(_, g)| g) {
			Some(Grab::Items { item, origin, .. }) => {
				if let Some(anchor) = anchor_of(&app.document.project, item).filter(|a| *a != origin) {
					out.push(Event::Command(Command::MoveBy {
						target: item,
						dx: origin.0 - anchor.0,
						dy: origin.1 - anchor.1,
					}));
				}
				out.push(Event::MouseUp);
			}
			Some(Grab::Edit { before, .. }) => {
				send_edits(out, before);
				out.push(Event::MouseUp);
			}
			Some(Grab::Marquee { .. }) | None => {}
		}
	}

	/// Keys on the focused picture: Tab through the handles (focusing one selects its item, past the ends the focus
	/// leaves the picture), arrows nudge the focused handle, Del removes its item.
	fn handle_keys(
		&mut self,
		ui: &Ui,
		response: &Response,
		app: &mut App,
		layer: Layer,
		model: &OverlayModel,
		out: &mut Vec<Event>,
	) {
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
					Some(i) => {
						let item = model.handles[i].item;
						if !app.selection.contains(item) {
							app.selection.select(Row::Item(item));
						}
					}
					None => ui.memory_mut(|m| {
						m.move_focus(if back { FocusDirection::Previous } else { FocusDirection::Next });
					}),
				}
				return;
			}
		}
		let Some(h) = current.map(|i| &model.handles[i]) else { return };
		if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
			out.push(Event::Command(Command::Remove { target: h.item }));
			*focus = None;
			return;
		}
		let (dx, dy) = ui.input_mut(|i| {
			let mut d = (0, 0);
			for (modifiers, n) in [(Modifiers::NONE, 1), (Modifiers::SHIFT, 10)] {
				let mut count = |key| i.count_and_consume_key(modifiers, key) as i32 * n;
				d.0 += count(Key::ArrowRight) - count(Key::ArrowLeft);
				d.1 += count(Key::ArrowDown) - count(Key::ArrowUp);
			}
			d
		});
		if (dx, dy) != (0, 0) {
			match nudge_edits(app, h, dx, dy) {
				Some(edits) => send_edits(out, edits),
				None => out.push(Event::Command(Command::MoveBy { target: h.item, dx, dy })),
			}
		}
	}
}
