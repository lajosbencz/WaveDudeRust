//! The handle overlay's edits (docs/ui/40-preview-pane.md, Interaction): what a press, a drag or a key on a handle
//! does to the selection and the model, and the overlay model and tooltips of a picture.

use crate::Event;
use crate::commands::{Command, Edit};
use crate::selection::Row;
use waveduderust::model::{
	FiberEdit, ItemRef, Layer, OutlineEdit, PartBaseEdit, PartEdit, PartKind, PartKindEdit, ProbeEdit, ProbeKind,
	Project, SourceArrayEdit, SourceEdit,
};
use waveduderust::unstable::view::overlay::{self as view, Handle, HandleKind, OverlayModel};

use super::handles::{
	DRAG_THRESHOLD, MarqueeMode, drag_delta, grip_edits, hit, lever_angle, marquee_hits, marquee_select, moved_node,
	moved_point, to_part_frame, turn,
};
use crate::App;

/// A press in progress on a picture.
#[derive(Clone, Debug, PartialEq)]
pub enum Grab {
	/// A whole-item drag through group moves; `origin` is the item's first anchor at the press.
	Items { item: ItemRef, press: (f32, f32), origin: (i32, i32), group: bool, moved: bool },
	/// A drag that edits fields of one item; `before` are the edits that restore them, for Esc.
	Edit { item: ItemRef, kind: HandleKind, before: Vec<Edit> },
	/// A marquee from `a` to `b`, canvas pixels.
	Marquee { a: (f32, f32), b: (f32, f32), mode: MarqueeMode },
}

/// The modifier keys of a press: Ctrl (Command on macOS) toggles, Shift adds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Press {
	pub toggle: bool,
	pub add: bool,
}

/// The events applying `edits`.
pub fn edit_events(edits: Vec<Edit>) -> impl Iterator<Item = Event> {
	edits.into_iter().map(|e| Event::Command(Command::Edit(e)))
}

/// A primary press at the canvas point `c` of `model` at `scale`: on a handle it selects (`m.toggle` toggles,
/// `m.add` adds) and grabs its item, elsewhere it starts a marquee; `None` when it only changed the selection.
pub fn press(
	app: &mut App,
	model: &OverlayModel,
	scale: f32,
	c: (f32, f32),
	m: Press,
	out: &mut Vec<Event>,
) -> Option<Grab> {
	let Some(h) = hit(model, c, scale).map(|i| &model.handles[i]) else {
		let mode = if m.toggle {
			MarqueeMode::Toggle
		} else if m.add {
			MarqueeMode::Add
		} else {
			MarqueeMode::Replace
		};
		return Some(Grab::Marquee { a: c, b: c, mode });
	};
	let (item, selection) = (h.item, &mut app.selection);
	if m.toggle {
		selection.toggle(item);
		return None;
	}
	if m.add {
		if !selection.contains(item) {
			let mut items = selection.items().to_vec();
			items.push(item);
			selection.set_items(items);
		}
		return None;
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
	Some(grab)
}

impl Grab {
	/// The pointer at `c` while grabbing: whole items move by whole pixels once past the drag threshold, a handle
	/// edits its fields (`snap`: Shift on a lever), a marquee follows.
	pub fn drag(&mut self, app: &App, c: (f32, f32), scale: f32, snap: bool, out: &mut Vec<Event>) {
		let p = &app.document.project;
		match self {
			Self::Items { item, press, origin, moved, .. } => {
				let anchor = anchor_of(p, *item).unwrap_or(*origin);
				let (dx, dy) = drag_delta(*press, c, (anchor.0 - origin.0, anchor.1 - origin.1));
				let far = (c.0 - press.0).hypot(c.1 - press.1) * scale >= DRAG_THRESHOLD;
				if (dx, dy) != (0, 0) && (*moved || far) {
					out.push(Event::Command(Command::MoveBy { target: *item, dx, dy }));
					*moved = true;
				}
			}
			Self::Edit { item, kind, before } => {
				let edits = handle_edits(p, *item, *kind, c, snap);
				remember(before, edits.iter().filter_map(|e| e.current(p, &app.document.view)).collect());
				out.extend(edit_events(edits));
			}
			Self::Marquee { b, .. } => *b = c,
		}
	}

	/// The primary button let go: a group grab that did not move selects its item alone; a marquee selects what it
	/// spans of `model`, or deselects as a click on empty space.
	pub fn release(self, app: &mut App, model: &OverlayModel, scale: f32, out: &mut Vec<Event>) {
		match self {
			Self::Items { item, group, moved, .. } => {
				if group && !moved {
					app.selection.select(Row::Item(item));
				}
				out.push(Event::MouseUp);
			}
			Self::Edit { .. } => out.push(Event::MouseUp),
			Self::Marquee { a, b, mode } => {
				if (b.0 - a.0).hypot(b.1 - a.1) * scale < DRAG_THRESHOLD {
					if mode == MarqueeMode::Replace {
						app.deselect();
					}
				} else {
					let items = marquee_select(app.selection.items(), &marquee_hits(model, a, b), mode);
					app.selection.set_items(items);
				}
			}
		}
	}

	/// Esc or a second finger during a drag: puts the item, the group or the fields back.
	pub fn cancel(self, app: &App, out: &mut Vec<Event>) {
		match self {
			Self::Items { item, origin, .. } => {
				if let Some(anchor) = anchor_of(&app.document.project, item).filter(|a| *a != origin) {
					out.push(Event::Command(Command::MoveBy {
						target: item,
						dx: origin.0 - anchor.0,
						dy: origin.1 - anchor.1,
					}));
				}
				out.push(Event::MouseUp);
			}
			Self::Edit { before, .. } => {
				out.extend(edit_events(before));
				out.push(Event::MouseUp);
			}
			Self::Marquee { .. } => {}
		}
	}

	/// The marquee's corners, canvas pixels.
	pub fn marquee(&self) -> Option<((f32, f32), (f32, f32))> {
		match self {
			Self::Marquee { a, b, .. } => Some((*a, *b)),
			Self::Items { .. } | Self::Edit { .. } => None,
		}
	}

	/// Whether the grab moves or edits the model, as opposed to a marquee.
	pub fn edits(&self) -> bool {
		!matches!(self, Self::Marquee { .. })
	}
}

/// Arrow keys on handle `h`, `dx` and `dy` pixels: [`nudge_edits`], or a move of its item or group.
pub fn nudge(app: &App, h: &Handle, dx: i32, dy: i32, out: &mut Vec<Event>) {
	match nudge_edits(app, h, dx, dy) {
		Some(edits) => out.extend(edit_events(edits)),
		None => out.push(Event::Command(Command::MoveBy { target: h.item, dx, dy })),
	}
}

/// The canvas rectangle around the selected items' handles, probe lines and part outline: min and max corners.
pub fn selection_bounds(model: &OverlayModel) -> Option<((f32, f32), (f32, f32))> {
	let handles = model.handles.iter().filter(|h| h.selected).map(|h| h.at);
	let lines = model.lines.iter().filter(|l| l.selected).flat_map(|l| [l.a, l.b]);
	let outlines = model.outlines.iter().flat_map(|o| o.points.iter().copied());
	handles.chain(lines).chain(outlines).fold(None, |r, p| match r {
		None => Some((p, p)),
		Some((lo, hi)) => Some(((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1)))),
	})
}

/// The item's first anchor: where a whole-item drag measures its moves from.
pub fn anchor_of(project: &Project, item: ItemRef) -> Option<(i32, i32)> {
	match item {
		ItemRef::Source(id) => project.source(id).map(|s| (s.x.floor() as i32, s.y.floor() as i32)),
		ItemRef::Part(id) => project.part(id).map(|p| (p.base.x, p.base.y)),
		ItemRef::Probe(id) => project.probe(id).map(|p| (p.ax, p.ay)),
		_ => None,
	}
}

/// The edits that put handle `kind` of `item` at the canvas point `p` (`snap`: Shift on a lever).
pub fn handle_edits(project: &Project, item: ItemRef, kind: HandleKind, p: (f32, f32), snap: bool) -> Vec<Edit> {
	let pixel = (p.0.floor() as i32, p.1.floor() as i32);
	match (item, kind) {
		(ItemRef::Source(i), HandleKind::SourceLever) => project.source(i).map_or_else(Vec::new, |s| {
			let angle = lever_angle((s.x as f32 + 0.5, s.y as f32 + 0.5), p, snap);
			vec![Edit::Source(i, SourceEdit::Array(SourceArrayEdit::Angle(angle)))]
		}),
		(ItemRef::Probe(i), HandleKind::ProbeA) => project.probe(i).map_or_else(Vec::new, |probe| {
			let mut edits = vec![Edit::Probe(i, ProbeEdit::Ax(pixel.0)), Edit::Probe(i, ProbeEdit::Ay(pixel.1))];
			if (probe.box_x, probe.box_y) == (probe.ax, probe.ay) {
				edits.extend([Edit::Probe(i, ProbeEdit::BoxX(pixel.0)), Edit::Probe(i, ProbeEdit::BoxY(pixel.1))]);
			}
			edits
		}),
		(ItemRef::Probe(i), HandleKind::ProbeB) => {
			vec![Edit::Probe(i, ProbeEdit::Bx(pixel.0)), Edit::Probe(i, ProbeEdit::By(pixel.1))]
		}
		(ItemRef::Part(i), _) => {
			let Some(part) = project.part(i) else { return Vec::new() };
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
		(item, kind) => unreachable!("{kind:?} is no edit handle of {item:?}"),
	}
}

/// Adds the edits `olds` (the fields as they are now) to `before` unless it already restores that field: a drag
/// can start editing more fields mid-way (a box that leaves its corner), and Esc restores each field as it was
/// before its first edit.
pub fn remember(before: &mut Vec<Edit>, olds: Vec<Edit>) {
	for old in olds {
		if !before.iter().any(|b| b.field_ref() == old.field_ref()) {
			before.push(old);
		}
	}
}

/// A whole-item handle that a property edit moves rather than a group move: a line probe's A alone, and B.
pub fn moves_by_edit(project: &Project, item: ItemRef, kind: HandleKind) -> bool {
	match kind {
		HandleKind::ProbeB => true,
		HandleKind::ProbeA => match item {
			ItemRef::Probe(id) => project.probe(id).is_some_and(|p| p.kind == ProbeKind::Line),
			_ => false,
		},
		_ => !kind.moves_item(),
	}
}

/// Arrow keys on handle `h`: the edits of a lever, which turns by a degree a pixel (Right and Up
/// counter-clockwise), or of a handle that moves like a drag; `None` for a whole-item handle, which moves its item
/// or the selection it belongs to by whole pixels (`Command::MoveBy`).
pub fn nudge_edits(app: &App, h: &Handle, dx: i32, dy: i32) -> Option<Vec<Edit>> {
	let item = h.item;
	let p = &app.document.project;
	let grouped = app.selection.contains(item) && app.selection.items().len() > 1;
	if h.kind.is_lever() {
		let delta = f64::from(dx - dy);
		let turned = match item {
			ItemRef::Source(i) => p
				.source(i)
				.and_then(|s| s.array.as_ref())
				.map(|a| Edit::Source(i, SourceEdit::Array(SourceArrayEdit::Angle(turn(a.angle, delta))))),
			ItemRef::Part(i) => {
				p.part(i).map(|part| Edit::Part(i, PartEdit::Base(PartBaseEdit::Angle(turn(part.base.angle, delta)))))
			}
			_ => None,
		};
		return Some(turned.into_iter().collect());
	}
	if h.kind.moves_item() && (grouped || !moves_by_edit(p, item, h.kind)) {
		return None;
	}
	Some(handle_edits(p, item, h.kind, (h.at.0 + dx as f32, h.at.1 + dy as f32), false))
}

/// The overlay model of the picture of `layer` at `scale`.
pub fn model_of(app: &App, layer: Layer, scale: f32) -> OverlayModel {
	let d = &app.document.view;
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

/// The tooltip of handle `h`: what it is and how to move it.
pub fn handle_tip(app: &App, h: &Handle) -> String {
	let s = &app.strings;
	let name = h.item.name(&app.document.project).unwrap_or_default().to_owned();
	match h.kind {
		HandleKind::Source | HandleKind::PartCentre | HandleKind::ProbeA | HandleKind::ProbeB => {
			s.format("TipMarkerFormat", &[&name])
		}
		HandleKind::SourceLever | HandleKind::PartLever => s.format("TipAngleMarkerFormat", &[&name]),
		HandleKind::Grip => s.format("TipGripFormat", &[&name]),
		HandleKind::Point(i) => s.format("TipPointFormat", &[&name, &(i + 1).to_string()]),
		HandleKind::Node(i) => s.format("TipNodeFormat", &[&name, &(i + 1).to_string()]),
		HandleKind::InHandle(i) => s.format("TipInHandleFormat", &[&name, &(i + 1).to_string()]),
		HandleKind::OutHandle(i) => s.format("TipOutHandleFormat", &[&name, &(i + 1).to_string()]),
	}
}

#[cfg(test)]
mod tests {
	use waveduderust::model::{ItemKind, Part, Probe, ProbeId, Rectangle, RectangleEdit, Source, SourceArray};

	use crate::commands::Command;
	use crate::selection::Selection;
	use waveduderust::model::normalize::Normalize;

	use super::*;

	fn apply_to(p: &mut Project, e: &Edit) {
		let view = &mut waveduderust::model::View::default();
		crate::commands::apply(p, view, &mut Selection::default(), Command::Edit(e.clone())).expect("applies");
	}

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 200;
		p.scene.height = 100;
		p.sources.push(Source::default().with_x(10.0).with_y(10.0).with_array(Some(SourceArray::default())));
		p.probes.push(Probe::default().with_ax(5).with_ay(5).with_box_x(5).with_box_y(5).with_kind(ProbeKind::Line));
		p.parts.push(Part::new(PartKind::Rectangle(Rectangle::default()), 50, 50));
		p
	}

	#[test]
	fn edits_put_handles_at_the_pointer() {
		let p = project();
		let probe = ItemRef::at(ItemKind::Probe, 0, &p).unwrap();
		let a = handle_edits(&p, probe, HandleKind::ProbeA, (7.9, 8.2), false);
		assert_eq!(
			a,
			vec![
				Edit::Probe(p.probes[0].id(), ProbeEdit::Ax(7)),
				Edit::Probe(p.probes[0].id(), ProbeEdit::Ay(8)),
				Edit::Probe(p.probes[0].id(), ProbeEdit::BoxX(7)),
				Edit::Probe(p.probes[0].id(), ProbeEdit::BoxY(8)),
			],
			"an attached box follows A"
		);
		let lever =
			handle_edits(&p, ItemRef::at(ItemKind::Source, 0, &p).unwrap(), HandleKind::SourceLever, (10.5, 0.), false);
		assert_eq!(lever, vec![Edit::Source(p.sources[0].id(), SourceEdit::Array(SourceArrayEdit::Angle(90.0)))]);
		let grip = handle_edits(&p, ItemRef::at(ItemKind::Part, 0, &p).unwrap(), HandleKind::Grip, (80., 60.), false);
		let rect = |e| Edit::Part(p.parts[0].id(), PartEdit::Kind(PartKindEdit::Rectangle(e)));
		assert_eq!(grip, vec![rect(RectangleEdit::Length(60)), rect(RectangleEdit::Width(20))]);
	}

	#[test]
	fn a_line_probes_ends_move_by_edits() {
		let p = project();
		let probe = ItemRef::at(ItemKind::Probe, 0, &p).unwrap();
		assert!(moves_by_edit(&p, probe, HandleKind::ProbeA));
		assert!(moves_by_edit(&p, probe, HandleKind::ProbeB));
		assert!(!moves_by_edit(&p, ItemRef::at(ItemKind::Source, 0, &p).unwrap(), HandleKind::Source));
		assert!(moves_by_edit(&p, ItemRef::at(ItemKind::Part, 0, &p).unwrap(), HandleKind::Grip));
	}

	#[test]
	fn a_later_move_adds_new_fields_and_keeps_the_first_values() {
		let mut p = Project::default();
		let (first, second): (ProbeId, ProbeId) = (p.add_probe(), p.add_probe());
		let at = |e| Edit::Probe(first, e);
		let mut before = vec![at(ProbeEdit::Ax(5)), at(ProbeEdit::Ay(5))];
		remember(
			&mut before,
			vec![at(ProbeEdit::Ax(7)), at(ProbeEdit::Ay(8)), at(ProbeEdit::BoxX(5)), at(ProbeEdit::BoxY(5))],
		);
		assert_eq!(
			before,
			vec![at(ProbeEdit::Ax(5)), at(ProbeEdit::Ay(5)), at(ProbeEdit::BoxX(5)), at(ProbeEdit::BoxY(5))]
		);
		remember(&mut before, vec![Edit::Probe(second, ProbeEdit::Ax(9))]);
		assert_eq!(before.len(), 5, "the same field of another probe is another field");
	}

	#[test]
	fn a_drag_restores_the_fields_it_edited() {
		// applying a command normalises (names), so the start is normalised too
		let mut p = project();
		p.normalize();
		let probe = ItemRef::at(ItemKind::Probe, 0, &p).unwrap();
		let edits = handle_edits(&p, probe, HandleKind::ProbeA, (7.9, 8.2), false);
		let before: Vec<Edit> = edits.iter().filter_map(|e| e.current(&p, &Default::default())).collect();
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
