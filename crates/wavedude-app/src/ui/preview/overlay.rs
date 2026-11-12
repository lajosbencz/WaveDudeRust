//! The handle overlay of a picture: drawing ([`draw`]) and the pointer and key handling that turns presses,
//! drags and keys on handles into selection changes and model commands (docs/ui/40-preview-pane.md, Interaction).
//!
//! Drawing happens in the picture's render callback in the cell's logical pixels, clipped to the picture: the
//! overlay's shapes ([`wavedude_view::overlay::shapes`]) through the picture's [`Painter`], then the marquee.
//! Threads: the UI thread.

use freya::engine::prelude::{Canvas, Paint, PaintStyle, PathEffect, SkRect};
use freya::prelude::*;
use wavedude_core::model::{
	FiberEdit, ItemKind, ItemRef, Layer, OutlineEdit, PartBaseEdit, PartEdit, PartKind, PartKindEdit, ProbeEdit,
	ProbeKind, Project, SourceArrayEdit, SourceEdit,
};
use wavedude_view::overlay::{self as view, Handle, HandleKind, LABEL_SIZE, OverlayModel};
use wavedude_widgets::picture::{CanvasTransform, PictureEvent, PointerPhase};
use wavedude_widgets::{ElementFonts, LabelText, Painter, rgba_of};

use super::handles::{
	DRAG_THRESHOLD, MarqueeMode, drag_delta, grip_edits, lever_angle, marquee_hits, marquee_select, moved_node,
	moved_point, to_part_frame, turn,
};
use super::{PaneStates, open_picture_menu};
use crate::ui::shared::{App, Shared};
use wavedude_app_state::Event as AppEvent;
use wavedude_app_state::commands::{Command, Edit};
use wavedude_app_state::selection::Row;

/// What one picture's overlay draws besides the model.
pub struct OverlayFrame<'a> {
	pub model: &'a OverlayModel,
	pub focused: Option<usize>,
	pub marquee: Option<((f32, f32), (f32, f32))>,
	pub accent: Color,
}

/// Draws the overlay of one picture in the cell's logical pixels.
pub fn draw(
	canvas: &Canvas,
	painter: &mut Painter,
	fonts: ElementFonts<'_>,
	t: &CanvasTransform,
	f: &OverlayFrame<'_>,
) {
	let shapes = view::shapes(f.model, t.scale, t.picture(), f.focused, rgba_of(f.accent), &LabelText::new(LABEL_SIZE));
	painter.paint(canvas, fonts, &shapes);
	if let Some((a, b)) = f.marquee {
		let (a, b) = (t.to_screen(a.0, a.1), t.to_screen(b.0, b.1));
		let r = SkRect::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
		let mut fill = Paint::default();
		fill.set_anti_alias(true);
		fill.set_style(PaintStyle::Fill);
		fill.set_color(f.accent);
		fill.set_alpha_f(0.15);
		canvas.draw_rect(r, &fill);
		let mut edge = Paint::default();
		edge.set_anti_alias(true);
		edge.set_style(PaintStyle::Stroke);
		edge.set_stroke_width(1.);
		edge.set_color(f.accent);
		edge.set_path_effect(PathEffect::dash(&[4., 3.], 0.));
		canvas.draw_rect(r, &edge);
	}
}

/// A press in progress on a picture.
#[derive(Clone, Debug, PartialEq)]
pub enum Grab {
	/// A whole-item drag through group moves; `origin` is the item's first anchor at the press.
	Items {
		item: ItemRef,
		press: (f32, f32),
		origin: (i32, i32),
		group: bool,
		moved: bool,
	},
	/// A drag that edits fields of one item; `before` are the edits that restore them, for Esc.
	Edit {
		item: ItemRef,
		kind: HandleKind,
		before: Vec<Edit>,
	},
	Marquee {
		layer: Layer,
		a: (f32, f32),
		b: (f32, f32),
		mode: MarqueeMode,
	},
	/// A middle-button drag moving the zoomed view; `last` is the pointer in window pixels.
	Pan {
		last: (f64, f64),
	},
}

fn anchor_of(project: &Project, item: ItemRef) -> Option<(i32, i32)> {
	match item.kind {
		ItemKind::Source => project.sources.get(item.index).map(|s| (s.x.floor() as i32, s.y.floor() as i32)),
		ItemKind::Part => project.parts.get(item.index).map(|p| (p.base.x, p.base.y)),
		ItemKind::Probe => project.probes.get(item.index).map(|p| (p.ax, p.ay)),
		ItemKind::Scheme => None,
	}
}

/// The edits that put handle `kind` of `item` at the canvas point `p` (`snap`: Shift on a lever).
pub fn handle_edits(project: &Project, item: ItemRef, kind: HandleKind, p: (f32, f32), snap: bool) -> Vec<Edit> {
	let pixel = (p.0.floor() as i32, p.1.floor() as i32);
	let i = item.index;
	match (item.kind, kind) {
		(ItemKind::Source, HandleKind::SourceLever) => project.sources.get(i).map_or_else(Vec::new, |s| {
			let angle = lever_angle((s.x as f32 + 0.5, s.y as f32 + 0.5), p, snap);
			vec![Edit::Source(i, SourceEdit::Array(SourceArrayEdit::Angle(angle)))]
		}),
		(ItemKind::Probe, HandleKind::ProbeA) => project.probes.get(i).map_or_else(Vec::new, |probe| {
			let mut edits = vec![Edit::Probe(i, ProbeEdit::Ax(pixel.0)), Edit::Probe(i, ProbeEdit::Ay(pixel.1))];
			if (probe.box_x, probe.box_y) == (probe.ax, probe.ay) {
				edits.extend([Edit::Probe(i, ProbeEdit::BoxX(pixel.0)), Edit::Probe(i, ProbeEdit::BoxY(pixel.1))]);
			}
			edits
		}),
		(ItemKind::Probe, HandleKind::ProbeB) => {
			vec![Edit::Probe(i, ProbeEdit::Bx(pixel.0)), Edit::Probe(i, ProbeEdit::By(pixel.1))]
		}
		(ItemKind::Part, _) => {
			let Some(part) = project.parts.get(i) else { return Vec::new() };
			let part_edits: Vec<PartEdit> = match (kind, &part.kind) {
				(HandleKind::PartLever, _) => {
					let angle = lever_angle((part.base.x as f32, part.base.y as f32), p, snap);
					vec![PartEdit::Base(PartBaseEdit::Angle(angle))]
				}
				(HandleKind::Grip, k) => grip_edits(k, to_part_frame(part, p)),
				(HandleKind::Point(at), PartKind::Outline(o)) => {
					let points = moved_point(&o.points, at, to_part_frame(part, p));
					vec![PartEdit::Kind(PartKindEdit::Outline(OutlineEdit::Points(points)))]
				}
				(HandleKind::Node(_) | HandleKind::InHandle(_) | HandleKind::OutHandle(_), PartKind::Fiber(f)) => {
					let nodes = moved_node(f, kind, to_part_frame(part, p));
					vec![PartEdit::Kind(PartKindEdit::Fiber(FiberEdit::Nodes(nodes)))]
				}
				(kind, _) => unreachable!("{kind:?} is no handle of part {}", part.base.name),
			};
			part_edits.into_iter().map(|e| Edit::Part(i, e)).collect()
		}
		(item_kind, kind) => unreachable!("{kind:?} is no edit handle of a {item_kind:?}"),
	}
}

fn send_edits(shared: &Shared, edits: Vec<Edit>) {
	for edit in edits {
		shared.send(AppEvent::Command(Command::Edit(edit)));
	}
}

/// Adds the edits `olds` (the fields as they are now) to `before` unless it already restores that field: a drag
/// can start editing more fields mid-way (a box that leaves its corner), and Esc restores each field as it was
/// before its first edit.
fn remember(before: &mut Vec<Edit>, olds: Vec<Edit>) {
	for old in olds {
		if !before.iter().any(|b| b.field_ref() == old.field_ref()) {
			before.push(old);
		}
	}
}

/// A whole-item handle that a property edit moves rather than a group move: a line probe's A alone, and B.
fn moves_by_edit(project: &Project, item: ItemRef, kind: HandleKind) -> bool {
	match kind {
		HandleKind::ProbeB => true,
		HandleKind::ProbeA => project.probes.get(item.index).is_some_and(|p| p.kind == ProbeKind::Line),
		_ => !kind.moves_item(),
	}
}

fn ctrl(m: Modifiers) -> bool {
	m.contains(Modifiers::CONTROL) || m.contains(Modifiers::META)
}

/// Handles a pointer event of the picture of `layer` with the overlay `model` it was built with.
pub fn on_pointer(
	shared: &Shared,
	states: PaneStates,
	layer: Layer,
	model: &OverlayModel,
	scale: f32,
	e: PictureEvent,
) {
	let mut grab = states.grab;
	let mut focus = states.focus;
	let index = e.handle.map(|h| h as usize);
	match e.phase {
		PointerPhase::Secondary => open_picture_menu(shared, states, layer, e.global),
		PointerPhase::Down if e.button == Some(MouseButton::Middle) => {
			if states.view.peek().zoom > 1. {
				grab.set(Some(Grab::Pan { last: e.global }));
			}
		}
		PointerPhase::Down => {
			focus.write()[layer as usize] = None;
			let Some(h) = index.and_then(|i| model.handles.get(i)) else {
				let mode = if ctrl(e.modifiers) {
					MarqueeMode::Toggle
				} else if e.modifiers.contains(Modifiers::SHIFT) {
					MarqueeMode::Add
				} else {
					MarqueeMode::Replace
				};
				grab.set(Some(Grab::Marquee { layer, a: e.canvas, b: e.canvas, mode }));
				return;
			};
			let item = h.item;
			if ctrl(e.modifiers) {
				shared.update(|app| app.selection.toggle(item));
				return;
			}
			if e.modifiers.contains(Modifiers::SHIFT) {
				shared.update(|app| {
					if !app.selection.contains(item) {
						let mut items = app.selection.items().to_vec();
						items.push(item);
						app.selection.set_items(items);
					}
				});
				return;
			}
			let (group, by_edit, anchor) = shared.update(|app| {
				let group = h.kind.moves_item() && app.selection.contains(item) && app.selection.items().len() > 1;
				if !group && app.selection.current_item() != Some(item) {
					app.selection.select(Row::Item(item));
				}
				let p = &app.document.project;
				(group, !group && moves_by_edit(p, item, h.kind), anchor_of(p, item))
			});
			shared.send(AppEvent::MouseDown);
			grab.set(Some(if by_edit {
				Grab::Edit { item, kind: h.kind, before: Vec::new() }
			} else {
				let origin = anchor.expect("a handle's item exists");
				Grab::Items { item, press: e.canvas, origin, group, moved: false }
			}));
		}
		PointerPhase::Move => {
			let current = grab.peek().clone();
			match current {
				Some(Grab::Items { item, press, origin, group, moved }) => {
					let anchor = shared.peek(|app| anchor_of(&app.document.project, item)).unwrap_or(origin);
					let applied = (anchor.0 - origin.0, anchor.1 - origin.1);
					let (dx, dy) = drag_delta(press, e.canvas, applied);
					let far = (e.canvas.0 - press.0).hypot(e.canvas.1 - press.1) * scale >= DRAG_THRESHOLD;
					if (dx, dy) != (0, 0) && (moved || far) {
						shared.send(AppEvent::Command(Command::MoveBy { target: item, dx, dy }));
						grab.set(Some(Grab::Items { item, press, origin, group, moved: true }));
					}
				}
				Some(Grab::Edit { item, kind, mut before }) => {
					let (edits, olds) = shared.peek(|app| {
						let p = &app.document.project;
						let edits = handle_edits(p, item, kind, e.canvas, e.modifiers.contains(Modifiers::SHIFT));
						let olds: Vec<Edit> = edits.iter().filter_map(|edit| edit.current(p)).collect();
						(edits, olds)
					});
					remember(&mut before, olds);
					send_edits(shared, edits);
					grab.set(Some(Grab::Edit { item, kind, before }));
				}
				Some(Grab::Marquee { layer, a, mode, .. }) => {
					grab.set(Some(Grab::Marquee { layer, a, b: e.canvas, mode }));
				}
				Some(Grab::Pan { last }) => {
					let (dx, dy) = ((e.global.0 - last.0) as f32, (e.global.1 - last.1) as f32);
					let mut view = states.view;
					let panned = view.peek().panned(dx, dy, scale);
					view.set(panned);
					grab.set(Some(Grab::Pan { last: e.global }));
				}
				None => {}
			}
		}
		PointerPhase::Up => {
			let current = grab.peek().clone();
			grab.set(None);
			match current {
				Some(Grab::Items { item, group, moved, .. }) => {
					if group && !moved {
						shared.update(|app| app.selection.select(Row::Item(item)));
					}
					shared.send(AppEvent::MouseUp);
				}
				Some(Grab::Edit { .. }) => {
					shared.send(AppEvent::MouseUp);
				}
				Some(Grab::Pan { .. }) => {}
				Some(Grab::Marquee { a, b, mode, .. }) => {
					let far = (b.0 - a.0).hypot(b.1 - a.1) * scale >= DRAG_THRESHOLD;
					if !far {
						if mode == MarqueeMode::Replace {
							shared.update(wavedude_app_state::AppState::deselect);
						}
						return;
					}
					let hits = marquee_hits(model, a, b);
					shared.update(|app| {
						let items = marquee_select(app.selection.items(), &hits, mode);
						app.selection.set_items(items);
					});
				}
				None => {}
			}
		}
	}
}

/// Esc during a drag: puts the item or the group back. Returns whether a drag was cancelled.
pub fn cancel_drag(shared: &Shared, states: PaneStates) -> bool {
	let mut grab = states.grab;
	let current = grab.peek().clone();
	grab.set(None);
	match current {
		Some(Grab::Items { item, origin, .. }) => {
			if let Some(anchor) = shared.peek(|app| anchor_of(&app.document.project, item))
				&& anchor != origin
			{
				shared.send(AppEvent::Command(Command::MoveBy {
					target: item,
					dx: origin.0 - anchor.0,
					dy: origin.1 - anchor.1,
				}));
			}
			shared.send(AppEvent::MouseUp);
			true
		}
		Some(Grab::Edit { before, .. }) => {
			send_edits(shared, before);
			shared.send(AppEvent::MouseUp);
			true
		}
		Some(Grab::Marquee { .. } | Grab::Pan { .. }) => true,
		None => false,
	}
}

/// Keys on a focused picture: Tab through the handles, arrows nudge, Del removes, Esc cancels a drag.
pub fn on_key(shared: &Shared, states: PaneStates, layer: Layer, model: &OverlayModel, e: &Event<KeyboardEventData>) {
	let mut focus = states.focus;
	let shift = e.modifiers.contains(Modifiers::SHIFT);
	let focused = focus.peek()[layer as usize]
		.and_then(|(item, kind)| model.handles.iter().position(|h| h.item == item && h.kind == kind));
	match &e.key {
		Key::Named(NamedKey::Escape) => {
			if cancel_drag(shared, states) {
				e.prevent_default();
				e.stop_propagation();
			}
		}
		Key::Named(NamedKey::Tab) if !ctrl(e.modifiers) => {
			let n = model.handles.len();
			let next = match (focused, shift) {
				(None, false) if n > 0 => Some(0),
				(None, true) if n > 0 => Some(n - 1),
				(Some(i), false) if i + 1 < n => Some(i + 1),
				(Some(i), true) if i > 0 => Some(i - 1),
				_ => None,
			};
			focus.write()[layer as usize] = next.map(|i| (model.handles[i].item, model.handles[i].kind));
			if let Some(i) = next {
				let item = model.handles[i].item;
				shared.update(|app| {
					if !app.selection.contains(item) {
						app.selection.select(Row::Item(item));
					}
				});
				e.prevent_default();
				e.stop_propagation();
			}
		}
		Key::Named(NamedKey::Delete) => {
			if let Some(h) = focused.and_then(|i| model.handles.get(i)) {
				shared.send(AppEvent::Command(Command::Remove { target: h.item }));
				focus.write()[layer as usize] = None;
				e.prevent_default();
				e.stop_propagation();
			}
		}
		Key::Named(arrow @ (NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::ArrowUp | NamedKey::ArrowDown)) => {
			let Some(h) = focused.and_then(|i| model.handles.get(i)) else { return };
			let n = if shift { 10 } else { 1 };
			let (dx, dy) = match arrow {
				NamedKey::ArrowLeft => (-n, 0),
				NamedKey::ArrowRight => (n, 0),
				NamedKey::ArrowUp => (0, -n),
				_ => (0, n),
			};
			nudge(shared, h, dx, dy);
			e.prevent_default();
			e.stop_propagation();
		}
		_ => {}
	}
}

/// Arrow keys on handle `h`: a whole-item handle moves its item or the selection it belongs to by whole pixels;
/// a lever turns by a degree a pixel (Right and Up counter-clockwise); other handles move like a drag.
pub fn nudge(shared: &Shared, h: &Handle, dx: i32, dy: i32) {
	let item = h.item;
	let edits = shared.peek(|app| {
		let p = &app.document.project;
		let grouped = app.selection.contains(item) && app.selection.items().len() > 1;
		if h.kind.is_lever() {
			let delta = f64::from(dx - dy);
			let i = item.index;
			let turned = match item.kind {
				ItemKind::Source => p
					.sources
					.get(i)
					.and_then(|s| s.array.as_ref())
					.map(|a| Edit::Source(i, SourceEdit::Array(SourceArrayEdit::Angle(turn(a.angle, delta))))),
				ItemKind::Part => p
					.parts
					.get(i)
					.map(|part| Edit::Part(i, PartEdit::Base(PartBaseEdit::Angle(turn(part.base.angle, delta))))),
				ItemKind::Probe | ItemKind::Scheme => unreachable!("a {:?} has no lever", item.kind),
			};
			return Some(turned.into_iter().collect());
		}
		if h.kind.moves_item() && (grouped || !moves_by_edit(p, item, h.kind)) {
			return None;
		}
		Some(handle_edits(p, item, h.kind, (h.at.0 + dx as f32, h.at.1 + dy as f32), false))
	});
	match edits {
		None => {
			shared.send(AppEvent::Command(Command::MoveBy { target: item, dx, dy }));
		}
		Some(edits) => send_edits(shared, edits),
	}
}

/// The overlay model of the picture of `layer` at `scale`.
pub fn model_of(app: &App, layer: Layer, scale: f32) -> OverlayModel {
	let d = &app.document.project.display;
	view::build(
		&app.document.project,
		app.selection.items(),
		app.selection.current_item(),
		layer,
		scale,
		d.handles_in_preview,
		d.labels_in_preview,
	)
}

#[cfg(test)]
mod tests {
	use wavedude_core::model::{Part, Probe, Rectangle, RectangleEdit, Source, SourceArray};

	use wavedude_app_state::selection::Selection;
	use wavedude_core::model::normalize::Normalize;

	use super::*;

	fn apply_to(p: &mut Project, e: &Edit) {
		wavedude_app_state::commands::apply(p, &mut Selection::default(), Command::Edit(e.clone())).expect("applies");
	}

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 200;
		p.scene.height = 100;
		p.sources.push(Source { x: 10.0, y: 10.0, array: Some(SourceArray::default()), ..Source::default() });
		p.probes.push(Probe { ax: 5, ay: 5, box_x: 5, box_y: 5, kind: ProbeKind::Line, ..Probe::default() });
		p.parts.push(Part::new(PartKind::Rectangle(Rectangle::default()), 50, 50));
		p
	}

	#[test]
	fn edits_put_handles_at_the_pointer() {
		let p = project();
		let probe = ItemRef::new(ItemKind::Probe, 0);
		let a = handle_edits(&p, probe, HandleKind::ProbeA, (7.9, 8.2), false);
		assert_eq!(
			a,
			vec![
				Edit::Probe(0, ProbeEdit::Ax(7)),
				Edit::Probe(0, ProbeEdit::Ay(8)),
				Edit::Probe(0, ProbeEdit::BoxX(7)),
				Edit::Probe(0, ProbeEdit::BoxY(8)),
			],
			"an attached box follows A"
		);
		let lever = handle_edits(&p, ItemRef::new(ItemKind::Source, 0), HandleKind::SourceLever, (10.5, 0.), false);
		assert_eq!(lever, vec![Edit::Source(0, SourceEdit::Array(SourceArrayEdit::Angle(90.0)))]);
		let grip = handle_edits(&p, ItemRef::new(ItemKind::Part, 0), HandleKind::Grip, (80., 60.), false);
		let rect = |e| Edit::Part(0, PartEdit::Kind(PartKindEdit::Rectangle(e)));
		assert_eq!(grip, vec![rect(RectangleEdit::Length(60)), rect(RectangleEdit::Width(20))]);
	}

	#[test]
	fn a_line_probes_ends_move_by_edits() {
		let p = project();
		let probe = ItemRef::new(ItemKind::Probe, 0);
		assert!(moves_by_edit(&p, probe, HandleKind::ProbeA));
		assert!(moves_by_edit(&p, probe, HandleKind::ProbeB));
		assert!(!moves_by_edit(&p, ItemRef::new(ItemKind::Source, 0), HandleKind::Source));
		assert!(moves_by_edit(&p, ItemRef::new(ItemKind::Part, 0), HandleKind::Grip));
	}

	#[test]
	fn a_later_move_adds_new_fields_and_keeps_the_first_values() {
		let at = |e| Edit::Probe(0, e);
		let mut before = vec![at(ProbeEdit::Ax(5)), at(ProbeEdit::Ay(5))];
		remember(
			&mut before,
			vec![at(ProbeEdit::Ax(7)), at(ProbeEdit::Ay(8)), at(ProbeEdit::BoxX(5)), at(ProbeEdit::BoxY(5))],
		);
		assert_eq!(
			before,
			vec![at(ProbeEdit::Ax(5)), at(ProbeEdit::Ay(5)), at(ProbeEdit::BoxX(5)), at(ProbeEdit::BoxY(5))]
		);
		remember(&mut before, vec![Edit::Probe(1, ProbeEdit::Ax(9))]);
		assert_eq!(before.len(), 5, "the same field of another probe is another field");
	}

	#[test]
	fn a_drag_restores_the_fields_it_edited() {
		// applying a command normalises (names), so the start is normalised too
		let mut p = project();
		p.normalize();
		let probe = ItemRef::new(ItemKind::Probe, 0);
		let edits = handle_edits(&p, probe, HandleKind::ProbeA, (7.9, 8.2), false);
		let before: Vec<Edit> = edits.iter().filter_map(|e| e.current(&p)).collect();
		assert_eq!(before.len(), edits.len());
		let mut moved = p.clone();
		for e in &edits {
			apply_to(&mut moved, e);
		}
		assert_ne!(moved, p);
		for e in &before {
			apply_to(&mut moved, e);
		}
		assert_eq!(moved, p);
	}
}
