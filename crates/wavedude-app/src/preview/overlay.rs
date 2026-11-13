//! The handle overlay's edits (docs/ui/40-preview-pane.md, Interaction): what a drag or a key on a handle does
//! to the model, and the overlay model and tooltips of a picture.

use wavedude_app_state::commands::Edit;
use wavedude_core::model::{
	FiberEdit, ItemKind, ItemRef, Layer, OutlineEdit, PartBaseEdit, PartEdit, PartKind, PartKindEdit, ProbeEdit,
	ProbeKind, Project, SourceArrayEdit, SourceEdit,
};
use wavedude_view::overlay::{self as view, Handle, HandleKind, OverlayModel};

use super::handles::{MarqueeMode, grip_edits, lever_angle, moved_node, moved_point, to_part_frame, turn};
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

/// The item's first anchor: where a whole-item drag measures its moves from.
pub fn anchor_of(project: &Project, item: ItemRef) -> Option<(i32, i32)> {
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
		HandleKind::ProbeA => project.probes.get(item.index).is_some_and(|p| p.kind == ProbeKind::Line),
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
	use wavedude_core::model::{Part, Probe, Rectangle, RectangleEdit, Source, SourceArray};

	use wavedude_app_state::commands::Command;
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
