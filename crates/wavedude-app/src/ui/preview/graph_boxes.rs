//! A picture's probe graph boxes (docs/ui/40-preview-pane.md, Probe graph box): their data, cut again when what it
//! is cut from moved, laid out again when the scale or the device pixels changed, drawn as the view's shapes on whole
//! device pixels; a box's drag, its corner grip's resize, their keys and the box's context menu.

use eframe::egui::epaint::ClippedShape;
use eframe::egui::{
	CursorIcon, EventFilter, FontId, Id, Key, Modifiers, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui,
	WidgetInfo, vec2,
};
use egui_material_icons::icons::ICON_IOS_SHARE;
use wavedude_app_state::Event;
use wavedude_app_state::commands::{Command, Edit};
use wavedude_app_state::platform::HAS_EXPORTS;
use wavedude_app_state::selection::Row;
use wavedude_core::model::{BoxSize, ItemKind, ItemRef, Layer, ProbeEdit};
use wavedude_view::graph::{self, GraphLayout};
use wavedude_view::{DeviceGrid, FontMetrics};

use super::{Preview, pressed_on};
use crate::App;
use crate::menus::{MenuAction, On};
use crate::preview::graph_boxes::{BoxKey, BoxView, box_key, boxes, dragged_box, moved_box_edits, resized_box_edits};
use crate::preview::view::CanvasTransform;
use crate::ui::menu_bar::{self, Slot};
use crate::ui::theme::Palette;

/// A box resized by its grip keeps at least this size, points.
const MIN_SIZE: (f32, f32) = (60., 30.);

/// A picture's boxes, their layouts and what both were made from.
pub(super) struct Boxes {
	key: BoxKey,
	views: Vec<BoxView>,
	/// The scale and the pixels per point the layouts were made at.
	laid: (u32, u32),
	layouts: Vec<GraphLayout>,
}

impl Boxes {
	fn of(app: &App, layer: Layer, key: BoxKey) -> Self {
		Self { key, views: boxes(app, layer), laid: (0, 0), layouts: Vec::new() }
	}
}

/// A box drag in progress: probe `index`'s box from `start` (canvas pixels), or its size from `size` (points),
/// which `from` holds as the probe's fields.
#[derive(Clone, Copy)]
pub(super) enum BoxDrag {
	Move { index: usize, start: (i32, i32) },
	Resize { index: usize, size: (f32, f32), from: (BoxSize, u32, u32) },
}

/// The id of probe `index`'s box over the picture of `layer`.
/// The corner grip of a box at `rect`.
fn grip_rect(rect: Rect) -> Rect {
	Rect::from_min_max(rect.max - vec2(graph::GRIP, graph::GRIP), rect.max)
}

fn box_id(layer: Layer, index: usize) -> Id {
	Id::unique(("ProbeBox", layer as usize, index))
}

/// Where a box shows on the picture of `t`, whole points.
fn on_picture(view: &BoxView, t: &CanvasTransform) -> Rect {
	let picture = t.picture().min;
	let ((x, y), (w, h)) = graph::box_on_picture(view.origin, view.size, (picture.x, picture.y), t.scale);
	Rect::from_min_size((x, y).into(), (w, h).into())
}

/// The arrow keys pressed, one or ten (Shift) a key, as a step right and down.
fn arrow_steps(ui: &Ui) -> (f32, f32) {
	ui.input_mut(|i| {
		let mut d = (0., 0.);
		for (modifiers, n) in [(Modifiers::NONE, 1.), (Modifiers::SHIFT, 10.)] {
			let mut count = |key| i.count_and_consume_key(modifiers, key) as f32 * n;
			d.0 += count(Key::ArrowRight) - count(Key::ArrowLeft);
			d.1 += count(Key::ArrowDown) - count(Key::ArrowUp);
		}
		d
	})
}

fn lock_arrows(ui: &Ui, id: Id) {
	let filter = EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
	ui.memory_mut(|m| m.set_focus_lock_filter(id, filter));
}

fn send_edits(out: &mut Vec<Event>, edits: Vec<Edit>) {
	out.extend(edits.into_iter().map(|e| Event::Command(Command::Edit(e))));
}

impl Preview {
	/// Whether `id` has the focus, given by the keyboard rather than a press.
	fn keyboard_focus(&self, ui: &Ui, id: Id) -> bool {
		ui.memory(|m| m.has_focus(id)) && self.pointer_focus != Some(id)
	}

	/// Draws the graph boxes of `layer` clipped to `clip`, the selected ones last.
	pub(super) fn paint_boxes(&mut self, ui: &Ui, app: &App, layer: Layer, t: &CanvasTransform, clip: Rect) {
		let key = box_key(app);
		let b = match self.boxes[layer as usize].take() {
			Some(b) if b.key == key => b,
			_ => Boxes::of(app, layer, key),
		};
		let b = self.boxes[layer as usize].insert(b);
		if b.views.is_empty() {
			return;
		}
		let ctx = ui.ctx();
		let ppp = ctx.pixels_per_point();
		let steps = u64::from(app.document.project.simulation.steps);
		let accent = Palette::of(ctx.theme()).accent;
		let focused: Vec<bool> = b.views.iter().map(|v| ui.memory(|m| m.has_focus(box_id(layer, v.index)))).collect();
		let pointer_focus = self.pointer_focus;
		let pointer = ui.rect_contains_pointer(clip).then(|| ctx.pointer_hover_pos()).flatten();
		let dragged = ctx.dragged_id();
		let shapes = ctx.fonts_mut(|fonts| {
			let laid = (t.scale.to_bits(), ppp.to_bits());
			if b.laid != laid {
				let mut metrics = FontMetrics { fonts: &mut *fonts, font: FontId::proportional(graph::FONT_SIZE) };
				b.layouts = (b.views.iter())
					.map(|v| {
						let r = on_picture(v, t);
						graph::layout(&v.probe, &v.recording.data(), steps, r.width(), r.height(), &mut metrics)
					})
					.collect();
				b.laid = laid;
			}
			let mut shapes = Vec::new();
			for ((v, g), focused) in b.views.iter().zip(&b.layouts).zip(focused).filter(|((_, g), _)| g.visible) {
				let at = on_picture(v, t).min;
				let grid = DeviceGrid::new(ppp, at.x, at.y);
				let focus = (focused && pointer_focus != Some(box_id(layer, v.index))).then_some(accent);
				shapes.extend(graph::box_shapes(g, grid, clip, v.selected, focus, fonts));
				let hot = pointer.is_some_and(|p| grip_rect(on_picture(v, t)).contains(p))
					|| dragged == Some(box_id(layer, v.index).with("grip"));
				shapes.extend(graph::grip_shapes(g, grid, hot).into_iter().map(|shape| ClippedShape::new(clip, shape)));
			}
			shapes
		});
		ctx.graphics_mut(|g| {
			let list = g.entry(ui.layer_id());
			for s in shapes {
				list.add(s.clip_rect, s.shape);
			}
		});
	}

	/// The boxes of `layer` within `clip`: a press focuses a box and selects its probe, a drag moves it (snapping
	/// onto point A), its grip resizes it (snapping to the fixed sizes), arrows move or resize the focused one, Del
	/// removes its probe, a right-click opens its menu, whose entry is returned.
	pub(super) fn edit_boxes(
		&mut self,
		ui: &Ui,
		app: &mut App,
		layer: Layer,
		t: &CanvasTransform,
		clip: Rect,
		out: &mut Vec<Event>,
	) -> Option<(MenuAction, On)> {
		let b = self.boxes[layer as usize].take()?;
		let mut picked = None;
		if super::pinching(ui) {
			self.cancel_box_drag(app, out);
		}
		for v in &b.views {
			let rect = on_picture(v, t);
			let id = box_id(layer, v.index);
			let s = &app.strings;
			let tip = s.format("TipProbeBoxFormat", &[&v.probe.name]);
			let response = ui.interact(rect.intersect(clip), id, Sense::click_and_drag()).on_hover_text(tip);
			response.widget_info(|| WidgetInfo::labeled(Role::Figure, true, &v.probe.name));
			let grip_tip = s.format("TipProbeBoxCornerFormat", &[&v.probe.name]);
			let grip_rect = grip_rect(rect);
			let grip = ui.interact(grip_rect.intersect(clip), id.with("grip"), Sense::drag());
			let grip = grip.on_hover_cursor(CursorIcon::ResizeNwSe).on_hover_text(grip_tip);
			super::hold(&response);
			super::hold(&grip);
			if self.keyboard_focus(ui, grip.id) {
				let accent = Palette::of(ui.ctx().theme()).accent;
				ui.painter().rect_stroke(grip_rect, 0., Stroke::new(1., accent), StrokeKind::Inside);
			}
			self.edit_box(ui, app, (v.index, rect), t, (&response, &grip), out);
			let export_busy = app.live.export_busy();
			let item = ItemRef::new(ItemKind::Probe, v.index);
			if response.secondary_clicked() && !app.selection.contains(item) {
				app.selection.select(Row::Item(item));
			}
			if HAS_EXPORTS {
				response.context_menu(|ui| {
					ui.add_enabled_ui(!export_busy, |ui| {
						let text = app.strings.get("MenuExportProbeCsv");
						if menu_bar::entry(ui, Slot::Icon(ICON_IOS_SHARE), text, None, None).clicked() {
							picked = Some((MenuAction::ExportProbeCsv, On::Row(Some(Row::Item(item)))));
						}
					});
				});
			}
		}
		self.boxes[layer as usize] = Some(b);
		picked
	}

	/// A second finger during a box drag: puts the box back where and as large as it was.
	fn cancel_box_drag(&mut self, app: &App, out: &mut Vec<Event>) {
		let Some(drag) = self.box_drag.take() else { return };
		let probes = &app.document.project.probes;
		let edits = match drag {
			BoxDrag::Move { index, start } => probes.get(index).map(|p| moved_box_edits(index, p, start)),
			BoxDrag::Resize { index, from: (size, width, height), .. } => probes.get(index).map(|p| {
				let fields = [
					(p.box_size != size).then_some(ProbeEdit::BoxSize(size)),
					(p.box_width != width).then_some(ProbeEdit::BoxWidth(width)),
					(p.box_height != height).then_some(ProbeEdit::BoxHeight(height)),
				];
				fields.into_iter().flatten().map(|e| Edit::Probe(index, e)).collect()
			}),
		};
		send_edits(out, edits.unwrap_or_default());
		out.push(Event::MouseUp);
	}

	/// Probe `index`'s box at `rect`: its `response` and its `grip`'s.
	fn edit_box(
		&mut self,
		ui: &Ui,
		app: &mut App,
		(index, rect): (usize, Rect),
		t: &CanvasTransform,
		(response, grip): (&Response, &Response),
		out: &mut Vec<Event>,
	) {
		let item = ItemRef::new(ItemKind::Probe, index);
		for r in [response, grip] {
			if pressed_on(r) && !r.has_focus() {
				r.request_focus();
				self.pointer_focus = Some(r.id);
			}
			if r.lost_focus() && self.pointer_focus == Some(r.id) {
				self.pointer_focus = None;
			}
			if r.gained_focus() && !app.selection.contains(item) {
				app.selection.select(Row::Item(item));
			}
		}
		let Some(probe) = app.document.project.probes.get(index) else { return };
		let canvas = app.document.project.canvas_size();
		let size = graph::box_size(probe, canvas);
		if (response.drag_started() || grip.drag_started()) && !super::pinching(ui) {
			self.box_drag = Some(if grip.drag_started() {
				BoxDrag::Resize {
					index,
					size: rect.size().into(),
					from: (probe.box_size, probe.box_width, probe.box_height),
				}
			} else {
				BoxDrag::Move { index, start: (probe.box_x, probe.box_y) }
			});
			out.push(Event::MouseDown);
		}
		let total = |r: &Response| r.total_drag_delta().unwrap_or_default();
		match self.box_drag {
			Some(BoxDrag::Move { index: i, start }) if i == index && response.dragged() => {
				let d = total(response) / t.scale;
				let to = dragged_box(start, (probe.ax, probe.ay), (d.x, d.y), t.scale, canvas, size);
				send_edits(out, moved_box_edits(index, probe, to));
			}
			Some(BoxDrag::Resize { index: i, size: (w, h), .. }) if i == index && grip.dragged() => {
				let d = total(grip);
				let (w, h) = ((w + d.x).max(MIN_SIZE.0), (h + d.y).max(MIN_SIZE.1));
				send_edits(out, resized_box_edits(index, probe, w, h, t.scale));
			}
			_ => {}
		}
		if (response.drag_stopped() || grip.drag_stopped()) && self.box_drag.take().is_some() {
			out.push(Event::MouseUp);
		}
		if response.has_focus() {
			lock_arrows(ui, response.id);
			if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
				out.push(Event::Command(Command::Remove { target: item }));
				return;
			}
			let (dx, dy) = arrow_steps(ui);
			if (dx, dy) != (0., 0.) {
				let x = (probe.box_x + dx as i32).clamp(0, (canvas.0 as f32 - size.0).max(0.) as i32);
				let y = (probe.box_y + dy as i32).clamp(0, (canvas.1 as f32 - size.1).max(0.) as i32);
				send_edits(out, moved_box_edits(index, probe, (x, y)));
			}
		}
		if grip.has_focus() {
			lock_arrows(ui, grip.id);
			let (dw, dh) = arrow_steps(ui);
			if (dw, dh) != (0., 0.) {
				let (w, h) = ((rect.width() + dw).max(MIN_SIZE.0), (rect.height() + dh).max(MIN_SIZE.1));
				send_edits(out, resized_box_edits(index, probe, w, h, t.scale));
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use eframe::egui::{self, TouchPhase, pos2};
	use wavedude_core::model::{Measurement, Probe};

	use super::super::frames::{Frames, PICTURE, press, release, touch};
	use super::*;
	use crate::preview::view::View;

	/// A frame of the field picture's graph boxes, which `frames` holds one of.
	fn run(frames: &mut Frames, events: Vec<egui::Event>) -> Vec<Event> {
		frames.run(events, |preview, ui, app, t, out| {
			preview.edit_boxes(ui, app, Layer::Field, t, PICTURE, out);
		})
	}

	fn with_probe(probe: Probe) -> Frames {
		let mut frames = Frames::new();
		frames.app.document.project.probes = vec![Probe { measurement: Measurement::Amplitude, ..probe }];
		let boxes = Boxes::of(&frames.app, Layer::Field, box_key(&frames.app));
		assert_eq!(boxes.views.len(), 1);
		frames.preview.boxes[Layer::Field as usize] = Some(boxes);
		frames
	}

	/// Where the only box lies in the frames' picture.
	fn box_rect(frames: &Frames) -> Rect {
		let t = CanvasTransform::new(PICTURE, frames.app.document.project.canvas_size(), View::default());
		on_picture(&frames.preview.boxes[Layer::Field as usize].as_ref().unwrap().views[0], &t)
	}

	/// The second finger's events, the first finger at `at`.
	fn two_fingers(at: egui::Pos2) -> Vec<egui::Event> {
		vec![touch(0, TouchPhase::Start, at), touch(1, TouchPhase::Start, at + vec2(40., 0.))]
	}

	/// The events a second finger makes while `drag` is in progress.
	fn pinch_during(mut frames: Frames, drag: BoxDrag) -> (Frames, Vec<Event>) {
		frames.preview.box_drag = Some(drag);
		let at = pos2(150., 100.);
		run(&mut frames, vec![]);
		run(&mut frames, vec![egui::Event::PointerMoved(at)]);
		let out = run(&mut frames, two_fingers(at));
		(frames, out)
	}

	fn downs_and_ups(out: &[Event]) -> (usize, usize) {
		let count = |wanted: fn(&Event) -> bool| out.iter().filter(|e| wanted(e)).count();
		(count(|e| matches!(e, Event::MouseDown)), count(|e| matches!(e, Event::MouseUp)))
	}

	#[test]
	fn a_second_finger_puts_a_dragged_box_back() {
		let start = (5, 6);
		let frames = with_probe(Probe { box_x: 50, box_y: 40, ..Probe::default() });
		let (frames, out) = pinch_during(frames, BoxDrag::Move { index: 0, start });
		assert!(frames.preview.box_drag.is_none());
		let probe = &frames.app.document.project.probes[0];
		let edits = moved_box_edits(0, probe, start);
		assert!(!edits.is_empty());
		let wanted: Vec<Event> =
			edits.into_iter().map(|e| Event::Command(Command::Edit(e))).chain([Event::MouseUp]).collect();
		assert_eq!(out.len(), wanted.len(), "{out:?}");
		for (got, want) in out.iter().zip(&wanted) {
			match (got, want) {
				(Event::Command(Command::Edit(a)), Event::Command(Command::Edit(b))) => assert_eq!(a, b),
				(Event::MouseUp, Event::MouseUp) => {}
				_ => panic!("{out:?}"),
			}
		}
	}

	#[test]
	fn a_second_finger_leaves_an_untouched_resize_alone_and_restores_a_changed_one() {
		let custom = Probe { box_size: BoxSize::Custom, box_width: 101, box_height: 77, ..Probe::default() };
		let drag =
			|p: &Probe| BoxDrag::Resize { index: 0, size: (16., 12.), from: (p.box_size, p.box_width, p.box_height) };
		let (frames, out) = pinch_during(with_probe(custom.clone()), drag(&custom));
		assert!(frames.preview.box_drag.is_none());
		assert!(matches!(out.as_slice(), [Event::MouseUp]), "an untouched box makes no undo step: {out:?}");

		let resized = Probe { box_size: BoxSize::Small, box_width: 300, box_height: 150, ..custom.clone() };
		let (_, out) = pinch_during(with_probe(resized), drag(&custom));
		let wanted = [
			Edit::Probe(0, ProbeEdit::BoxSize(BoxSize::Custom)),
			Edit::Probe(0, ProbeEdit::BoxWidth(101)),
			Edit::Probe(0, ProbeEdit::BoxHeight(77)),
		];
		assert_eq!(out.len(), wanted.len() + 1, "{out:?}");
		for (got, want) in out.iter().zip(&wanted) {
			assert!(matches!(got, Event::Command(Command::Edit(e)) if e == want), "{out:?}");
		}
		assert!(matches!(out.last(), Some(Event::MouseUp)));
	}

	#[test]
	fn a_drag_cancelled_by_a_second_finger_sends_one_press_and_one_release() {
		let mut frames = with_probe(Probe::default());
		let (grab, to) = (box_rect(&frames).center(), box_rect(&frames).center() + vec2(20., 0.));
		let mut out = Vec::new();
		for events in [vec![], vec![egui::Event::PointerMoved(grab)], vec![press(grab)]] {
			out.extend(run(&mut frames, events));
		}
		out.extend(run(&mut frames, vec![egui::Event::PointerMoved(to)]));
		assert!(frames.preview.box_drag.is_some(), "the drag began");
		out.extend(run(&mut frames, two_fingers(to)));
		assert!(frames.preview.box_drag.is_none(), "the second finger ended it");
		out.extend(run(&mut frames, vec![release(to)]));
		assert_eq!(downs_and_ups(&out), (1, 1), "{out:?}");
	}

	#[test]
	fn a_box_drag_does_not_begin_while_two_fingers_are_down() {
		let mut frames = with_probe(Probe::default());
		let (grab, to) = (box_rect(&frames).center(), box_rect(&frames).center() + vec2(20., 0.));
		let mut out = Vec::new();
		for events in [vec![], vec![egui::Event::PointerMoved(grab)], vec![press(grab)]] {
			out.extend(run(&mut frames, events));
		}
		let mut both = two_fingers(grab);
		both.push(egui::Event::PointerMoved(to));
		out.extend(run(&mut frames, both));
		out.extend(run(&mut frames, vec![release(to)]));
		assert!(frames.preview.box_drag.is_none());
		assert_eq!(downs_and_ups(&out), (0, 0), "{out:?}");
	}
}
