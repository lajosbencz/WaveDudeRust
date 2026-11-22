//! A picture's probe graph boxes (docs/ui/40-preview-pane.md, Probe graph box): their data, cut again when what it
//! is cut from moved, laid out again when the scale or the device pixels changed, drawn as the view's shapes on whole
//! device pixels ([`GraphBoxes`]); a box's and its corner grip's responses, the box's drag, the grip's resize and
//! their keys ([`edit_box`]).

use eframe::egui::epaint::ClippedShape;
use eframe::egui::{
	Color32, CursorIcon, Id, Key, Modifiers, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, vec2,
};
use waveduderust::model::{ItemRef, Layer, ProbeId};
use waveduderust::unstable::view::DeviceGrid;
use waveduderust::unstable::view::graph::{self, GraphLayout};
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::commands::Command;
use waveduderust_app::preview::graph_boxes::{
	BoxDrag, BoxKey, BoxView, box_key, boxes, grown_box_edits, nudged_box_edits,
};
use waveduderust_app::preview::overlay::edit_events;
use waveduderust_app::preview::view::CanvasTransform;
use waveduderust_app::selection::Row;

use waveduderust_egui_platform::input::{arrow_steps, lock_arrows, pinching, pressed_on};

/// A picture's boxes, their layouts and what both were made from.
struct Boxes {
	key: BoxKey,
	views: Vec<BoxView>,
	/// The scale and the pixels per point the layouts were made at.
	laid: (u32, u32),
	layouts: Vec<GraphLayout>,
}

/// Each picture's graph boxes, kept while what they show stays.
#[derive(Default)]
pub struct GraphBoxes {
	layers: [Option<Boxes>; 2],
}

/// The corner grip of a box at `rect`.
pub fn grip_rect(rect: Rect) -> Rect {
	Rect::from_min_max(rect.max - vec2(graph::GRIP, graph::GRIP), rect.max)
}

/// Where a box shows on the picture of `t`, whole points.
pub fn on_picture(view: &BoxView, t: &CanvasTransform) -> Rect {
	let picture = t.picture().min;
	let ((x, y), (w, h)) = graph::box_on_picture(view.origin, view.size, (picture.x, picture.y), t.scale);
	Rect::from_min_size((x, y).into(), (w, h).into())
}

/// The id of a grip of the box `id`.
pub fn grip_id(id: Id) -> Id {
	id.with("grip")
}

impl GraphBoxes {
	/// The boxes of `layer`, the selected ones last, cut again when what they show changed.
	pub fn views(&mut self, app: &App, layer: Layer) -> &[BoxView] {
		let key = box_key(app);
		let slot = &mut self.layers[layer as usize];
		if slot.as_ref().is_none_or(|b| b.key != key) {
			*slot = Some(Boxes { key, views: boxes(app, layer), laid: (0, 0), layouts: Vec::new() });
		}
		&slot.as_ref().expect("cut above").views
	}

	/// Draws the boxes of `layer` clipped to `clip`, the selected ones last; `id` names a probe's box. A box or a
	/// grip with the focus a press did not give (`pointer_focus`) has an `accent` ring, a grip under the pointer or
	/// dragged is hot.
	#[expect(clippy::too_many_arguments, reason = "what one picture's boxes are drawn with")]
	pub fn paint(
		&mut self,
		ui: &Ui,
		app: &App,
		layer: Layer,
		t: &CanvasTransform,
		clip: Rect,
		id: impl Fn(ProbeId) -> Id,
		pointer_focus: Option<Id>,
		accent: Color32,
	) {
		self.views(app, layer);
		let b = self.layers[layer as usize].as_mut().expect("cut above");
		if b.views.is_empty() {
			return;
		}
		let ctx = ui.ctx();
		let ppp = ctx.pixels_per_point();
		let steps = u64::from(app.document.project.simulation.steps);
		let focused: Vec<bool> = b.views.iter().map(|v| ui.memory(|m| m.has_focus(id(v.id)))).collect();
		let pointer = ui.rect_contains_pointer(clip).then(|| ctx.pointer_hover_pos()).flatten();
		let dragged = ctx.dragged_id();
		let shapes = ctx.fonts_mut(|fonts| {
			let laid = (t.scale.to_bits(), ppp.to_bits());
			if b.laid != laid {
				b.layouts = (b.views.iter())
					.map(|v| {
						let r = on_picture(v, t);
						graph::layout_with(fonts, &v.probe, &v.recording.data(), steps, r.width(), r.height())
					})
					.collect();
				b.laid = laid;
			}
			let mut shapes = Vec::new();
			for ((v, g), focused) in b.views.iter().zip(&b.layouts).zip(focused).filter(|((_, g), _)| g.visible) {
				let at = on_picture(v, t).min;
				let grid = DeviceGrid::new(ppp, at.x, at.y);
				let focus = (focused && pointer_focus != Some(id(v.id))).then_some(accent);
				shapes.extend(graph::box_shapes(g, grid, clip, v.selected, focus, fonts));
				let hot = pointer.is_some_and(|p| grip_rect(on_picture(v, t)).contains(p))
					|| dragged == Some(grip_id(id(v.id)));
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
}

/// The box of `v` at `rect` as the widget `id` and its corner grip, within `clip`, with their tooltips; the grip
/// with an `accent` ring while it has the focus a press did not give (`pointer_focus`).
#[expect(clippy::too_many_arguments, reason = "what one box's widgets are made of")]
pub fn box_responses(
	ui: &Ui,
	app: &App,
	v: &BoxView,
	rect: Rect,
	clip: Rect,
	id: Id,
	pointer_focus: Option<Id>,
	accent: Color32,
) -> (Response, Response) {
	let s = &app.strings;
	let tip = s.format("TipProbeBoxFormat", &[&v.probe.name]);
	let response = ui.interact(rect.intersect(clip), id, Sense::click_and_drag()).on_hover_text(tip);
	response.widget_info(|| WidgetInfo::labeled(Role::Figure, true, &v.probe.name));
	let grip_tip = s.format("TipProbeBoxCornerFormat", &[&v.probe.name]);
	let grip_rect = grip_rect(rect);
	let grip = ui.interact(grip_rect.intersect(clip), grip_id(id), Sense::drag());
	let grip = grip.on_hover_cursor(CursorIcon::ResizeNwSe).on_hover_text(grip_tip);
	if ui.memory(|m| m.has_focus(grip.id)) && pointer_focus != Some(grip.id) {
		ui.painter().rect_stroke(grip_rect, 0., Stroke::new(1., accent), StrokeKind::Inside);
	}
	(response, grip)
}

/// Probe `index`'s box at `rect` on the picture of `t`: its `response` and its `grip`'s. A press focuses either
/// (noted in `pointer_focus`) and selects the probe, a drag moves the box (snapping onto point A) or resizes it
/// (snapping to the fixed sizes) as `drag`, a second finger puts it back, arrows move or resize the focused one, Del
/// removes its probe.
#[expect(clippy::too_many_arguments, reason = "one box's widgets and the window's drag state")]
pub fn edit_box(
	ui: &Ui,
	app: &mut App,
	(index, rect): (ProbeId, Rect),
	t: &CanvasTransform,
	(response, grip): (&Response, &Response),
	drag: &mut Option<BoxDrag>,
	pointer_focus: &mut Option<Id>,
	out: &mut Vec<Event>,
) {
	let item = ItemRef::Probe(index);
	for r in [response, grip] {
		if pressed_on(r) && !r.has_focus() {
			r.request_focus();
			*pointer_focus = Some(r.id);
		}
		if r.lost_focus() && *pointer_focus == Some(r.id) {
			*pointer_focus = None;
		}
		if r.gained_focus() && !app.selection.contains(item) {
			app.selection.select(Row::Item(item));
		}
	}
	let project = &app.document.project;
	let Some(probe) = project.probe(index) else { return };
	if (response.drag_started() || grip.drag_started()) && !pinching(ui) {
		*drag = Some(BoxDrag::start(probe, grip.drag_started(), rect.size().into()));
		out.push(Event::MouseDown);
	}
	let total = |r: &Response| r.total_drag_delta().unwrap_or_default();
	match *drag {
		Some(d @ BoxDrag::Move { id, .. }) if id == index && response.dragged() => {
			out.extend(edit_events(d.edits(project, total(response).into(), t.scale)));
		}
		Some(d @ BoxDrag::Resize { id, .. }) if id == index && grip.dragged() => {
			out.extend(edit_events(d.edits(project, total(grip).into(), t.scale)));
		}
		_ => {}
	}
	if (response.drag_stopped() || grip.drag_stopped()) && drag.take().is_some() {
		out.push(Event::MouseUp);
	}
	if response.has_focus() {
		lock_arrows(ui, response.id);
		if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
			out.push(Event::Command(Command::Remove { target: item }));
			return;
		}
		let d = arrow_steps(ui);
		if d != (0, 0) {
			out.extend(edit_events(nudged_box_edits(probe, project.canvas_size(), d)));
		}
	}
	if grip.has_focus() {
		lock_arrows(ui, grip.id);
		let d = arrow_steps(ui);
		if d != (0, 0) {
			out.extend(edit_events(grown_box_edits(probe, rect.size().into(), d, t.scale)));
		}
	}
}

/// A second finger during a box drag: puts the box back where and as large as it was.
pub fn cancel_box_drag(drag: &mut Option<BoxDrag>, app: &App, out: &mut Vec<Event>) {
	let Some(d) = drag.take() else { return };
	out.extend(edit_events(d.cancel_edits(&app.document.project)));
	out.push(Event::MouseUp);
}
