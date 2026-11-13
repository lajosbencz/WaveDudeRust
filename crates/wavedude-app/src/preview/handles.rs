//! What a picture's handles do (docs/ui/40-preview-pane.md, Handles and overlays): the hit test over the overlay
//! model, the marquee, and the drag arithmetic of levers, grips and point handles. Pure, tested without a window.
//!
//! Coordinates: canvas pixels as continuous values, as in the overlay model. Angles in degrees, counter-clockwise
//! on screen. `scale` is logical pixels per canvas pixel.

use wavedude_core::model::{
	DiffuserEdit, EllipseEdit, FacettedGratingEdit, Fiber, FiberNode, FresnelLensEdit, GrinLensEdit,
	InterferenceFilterEdit, ItemRef, LensEdit, MetaSurfaceEdit, Part, PartEdit, PartKind, PartKindEdit, Point,
	PolygonEdit, RectangleEdit,
};
use wavedude_core::scene::shapes::{Frame, Vec2};
use wavedude_view::overlay::{HandleKind, OverlayModel};

/// Screen radius in logical pixels within which a press hits a handle.
pub const HIT_RADIUS: f32 = 10.;
/// Screen distance a press must travel to become a drag, logical pixels.
pub const DRAG_THRESHOLD: f32 = 3.;

/// The topmost handle within [`HIT_RADIUS`] screen pixels of the canvas point `p` (the last drawn wins).
pub fn hit(model: &OverlayModel, p: (f32, f32), scale: f32) -> Option<usize> {
	let r = HIT_RADIUS / scale.max(1e-3);
	model.handles.iter().enumerate().rev().find(|(_, h)| (h.at.0 - p.0).hypot(h.at.1 - p.1) <= r).map(|(i, _)| i)
}

/// How a marquee combines with the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarqueeMode {
	Replace,
	/// Shift.
	Add,
	/// Ctrl.
	Toggle,
}

/// Items whose whole-item handles lie inside the canvas rectangle spanned by `a` and `b`, in drawing order.
pub fn marquee_hits(model: &OverlayModel, a: (f32, f32), b: (f32, f32)) -> Vec<ItemRef> {
	let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
	let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
	let mut hits = Vec::new();
	for h in model.handles.iter().filter(|h| h.kind.moves_item()) {
		let inside = h.at.0 >= x0 && h.at.0 <= x1 && h.at.1 >= y0 && h.at.1 <= y1;
		if inside && !hits.contains(&h.item) {
			hits.push(h.item);
		}
	}
	hits
}

/// The selection a marquee leaves; the last item becomes current.
pub fn marquee_select(current: &[ItemRef], hits: &[ItemRef], mode: MarqueeMode) -> Vec<ItemRef> {
	match mode {
		MarqueeMode::Replace => hits.to_vec(),
		MarqueeMode::Add => {
			let mut out = current.to_vec();
			out.extend(hits.iter().filter(|h| !current.contains(h)));
			out
		}
		MarqueeMode::Toggle => {
			let mut out: Vec<ItemRef> = current.iter().filter(|c| !hits.contains(c)).copied().collect();
			out.extend(hits.iter().filter(|h| !current.contains(h)));
			out
		}
	}
}

/// The whole-pixel move still wanted when a drag that started at `press` is at `now` and `applied` pixels of it
/// have been applied: measured against the absolute pointer, so a clamped move does not drift.
pub fn drag_delta(press: (f32, f32), now: (f32, f32), applied: (i32, i32)) -> (i32, i32) {
	let total = ((now.0 - press.0).round() as i32, (now.1 - press.1).round() as i32);
	(total.0 - applied.0, total.1 - applied.1)
}

/// The angle a lever pivoting at `pivot` points at `pointer`: whole degrees, 15-degree steps with `snap`, in
/// (-180, 180].
pub fn lever_angle(pivot: (f32, f32), pointer: (f32, f32), snap: bool) -> f64 {
	let dx = f64::from(pointer.0 - pivot.0);
	let dy = f64::from(pivot.1 - pointer.1);
	let step = if snap { 15. } else { 1. };
	let mut a = (dy.atan2(dx).to_degrees() / step).round() * step;
	if a <= -180. {
		a += 360.;
	}
	a
}

/// `angle` turned by `delta` degrees, kept in (-180, 180].
pub fn turn(angle: f64, delta: f64) -> f64 {
	let mut a = (angle + delta).rem_euclid(360.);
	if a > 180. {
		a -= 360.;
	}
	a
}

/// The edits of dragging a part's grip to the part-frame point `q`: whole pixels, at least the kind's
/// minimum; a linked rectangle or ellipse takes the farther coordinate for both.
pub fn grip_edits(kind: &PartKind, q: Vec2) -> Vec<PartEdit> {
	let span = |c: f64, min: u32| ((2. * c.abs()).round() as u32).max(min);
	let k = PartEdit::Kind;
	match kind {
		PartKind::Rectangle(r) => {
			let (length, width) = linked_extents(r.linked, span(q.x, 1), span(q.y, 1));
			let rect = |e| k(PartKindEdit::Rectangle(e));
			length.map(RectangleEdit::Length).into_iter().chain([RectangleEdit::Width(width)]).map(rect).collect()
		}
		PartKind::Ellipse(e) => {
			let (length, width) = linked_extents(e.linked, span(q.x, 1), span(q.y, 1));
			let ellipse = |e| k(PartKindEdit::Ellipse(e));
			length.map(EllipseEdit::Length).into_iter().chain([EllipseEdit::Width(width)]).map(ellipse).collect()
		}
		PartKind::Polygon(p) => {
			let n = f64::from(p.sides.max(3));
			let side = (2. * q.length() * (std::f64::consts::PI / n).sin()).round().max(1.) as u32;
			vec![k(PartKindEdit::Polygon(PolygonEdit::Side(side)))]
		}
		PartKind::Lens(_) => vec![k(PartKindEdit::Lens(LensEdit::Diameter(span(q.y, 2))))],
		PartKind::FresnelLens(_) => {
			vec![k(PartKindEdit::FresnelLens(FresnelLensEdit::Lens(LensEdit::Diameter(span(q.y, 2)))))]
		}
		PartKind::Diffuser(_) => vec![
			k(PartKindEdit::Diffuser(DiffuserEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::Diffuser(DiffuserEdit::Width(span(q.y, 2)))),
		],
		PartKind::FacettedGrating(_) => vec![
			k(PartKindEdit::FacettedGrating(FacettedGratingEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::FacettedGrating(FacettedGratingEdit::Width(span(q.y, 2)))),
		],
		PartKind::MetaSurface(_) => vec![
			k(PartKindEdit::MetaSurface(MetaSurfaceEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::MetaSurface(MetaSurfaceEdit::Width(span(q.y, 2)))),
		],
		PartKind::InterferenceFilter(_) => vec![
			k(PartKindEdit::InterferenceFilter(InterferenceFilterEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::InterferenceFilter(InterferenceFilterEdit::Width(span(q.y, 1)))),
		],
		PartKind::GrinLens(_) => vec![
			k(PartKindEdit::GrinLens(GrinLensEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::GrinLens(GrinLensEdit::Diameter(span(q.y, 2)))),
		],
		_ => Vec::new(),
	}
}

/// The length to set (`None` for a linked shape, whose width sets it) and the width.
fn linked_extents(linked: bool, length: u32, width: u32) -> (Option<u32>, u32) {
	if linked { (None, length.max(width)) } else { (Some(length), width) }
}

/// An outline's points with point `i` at the part-frame point `q`.
pub fn moved_point(points: &[Point], i: usize, q: Vec2) -> Vec<Point> {
	let mut out = points.to_vec();
	if let Some(p) = out.get_mut(i) {
		*p = Point::new(q.x.round() as i32, q.y.round() as i32);
	}
	out
}

/// A fibre's nodes after dragging `kind` (a node or one of its Bezier handles) to the part-frame point `q`. A node
/// carries its handles; a smooth node turns the opposite handle to stay opposite, keeping its length.
pub fn moved_node(fiber: &Fiber, kind: HandleKind, q: Vec2) -> Vec<FiberNode> {
	let mut nodes = fiber.nodes.clone();
	let (qx, qy) = (q.x.round() as i32, q.y.round() as i32);
	match kind {
		HandleKind::Node(i) => {
			if let Some(n) = nodes.get_mut(i) {
				(n.x, n.y) = (qx, qy);
			}
		}
		HandleKind::InHandle(i) => {
			if let Some(n) = nodes.get_mut(i) {
				let h = (qx - n.x, qy - n.y);
				if n.smooth {
					(n.out_x, n.out_y) = opposite(h, (n.out_x, n.out_y));
				}
				(n.in_x, n.in_y) = h;
			}
		}
		HandleKind::OutHandle(i) => {
			if let Some(n) = nodes.get_mut(i) {
				let h = (qx - n.x, qy - n.y);
				if n.smooth {
					(n.in_x, n.in_y) = opposite(h, (n.in_x, n.in_y));
				}
				(n.out_x, n.out_y) = h;
			}
		}
		kind => unreachable!("{kind:?} is no fibre handle"),
	}
	nodes
}

/// `keep` turned to point away from `h`, its length kept.
fn opposite(h: (i32, i32), keep: (i32, i32)) -> (i32, i32) {
	let len = f64::from(keep.0).hypot(f64::from(keep.1));
	let d = f64::from(h.0).hypot(f64::from(h.1));
	if d == 0. {
		return keep;
	}
	((-f64::from(h.0) / d * len).round() as i32, (-f64::from(h.1) / d * len).round() as i32)
}

/// The canvas point `p` in the part frame of `part`.
pub fn to_part_frame(part: &Part, p: (f32, f32)) -> Vec2 {
	Frame::of(&part.base).to_frame(Vec2::new(f64::from(p.0), f64::from(p.1)))
}

#[cfg(test)]
mod tests {
	use wavedude_core::model::{self as m, Edit as _, ItemKind, Layer, Probe, ProbeKind, Project, Rectangle, Source};
	use wavedude_view::overlay::{build, grip_of};

	use super::*;

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 200;
		p.scene.height = 100;
		p.sources.push(Source { name: "S".into(), x: 10.0, y: 20.0, ..Source::default() });
		p.probes.push(Probe {
			name: "P".into(),
			ax: 50,
			ay: 50,
			bx: 80,
			by: 50,
			kind: ProbeKind::Line,
			..Probe::default()
		});
		let mut part =
			Part::new(PartKind::Rectangle(Rectangle { length: 40, width: 20, ..Rectangle::default() }), 100, 50);
		part.base.name = "R".into();
		p.parts.push(part);
		p
	}

	const S: ItemRef = ItemRef::new(ItemKind::Source, 0);
	const R: ItemRef = ItemRef::new(ItemKind::Part, 0);
	const P: ItemRef = ItemRef::new(ItemKind::Probe, 0);

	#[test]
	fn hit_takes_the_topmost_handle_within_the_screen_radius() {
		let m = build(&project(), &[], None, Layer::Field, 2., true, true);
		assert_eq!(m.handles[hit(&m, (10., 20.), 2.).unwrap()].item, S);
		assert_eq!(hit(&m, (10.5 + 4.9, 20.5), 2.).map(|i| m.handles[i].item), Some(S));
		assert_eq!(hit(&m, (10.5 + 5.1, 20.5), 2.), None);
		let mut overlapping = m.clone();
		overlapping.handles[2].at = overlapping.handles[0].at;
		assert_eq!(hit(&overlapping, (10.5, 20.5), 2.), Some(2));
	}

	#[test]
	fn marquee_selects_by_whole_item_handles() {
		let m = build(&project(), &[R], Some(R), Layer::Field, 1., true, true);
		assert_eq!(marquee_hits(&m, (0., 0.), (60., 60.)), vec![S, P]);
		assert_eq!(
			marquee_hits(&m, (115., 55.), (130., 65.)),
			Vec::<ItemRef>::new(),
			"a grip is not a whole-item handle"
		);
		assert_eq!(marquee_hits(&m, (70., 60.), (101., 40.)), vec![R, P], "the rectangle may be dragged either way");
		assert_eq!(marquee_select(&[R], &[S, P], MarqueeMode::Replace), vec![S, P]);
		assert_eq!(marquee_select(&[R, S], &[S, P], MarqueeMode::Add), vec![R, S, P]);
		assert_eq!(marquee_select(&[R, S], &[S, P], MarqueeMode::Toggle), vec![R, P]);
	}

	#[test]
	fn drags_follow_the_absolute_pointer_and_hold_the_formation() {
		assert_eq!(drag_delta((10., 10.), (13.4, 7.6), (0, 0)), (3, -2));
		assert_eq!(drag_delta((10., 10.), (13.4, 7.6), (3, -2)), (0, 0));
		assert_eq!(drag_delta((10., 10.), (15., 10.), (1, 0)), (4, 0), "a clamped move catches up once room is back");
		let mut p = project();
		let applied = wavedude_app_state::selection::move_group(&mut p, &[S, P], -20, 0);
		assert_eq!(applied, (-10, 0), "the source at x 10 stops the group at the left edge");
		assert_eq!((p.sources[0].x, p.probes[0].ax, p.probes[0].bx), (0.0, 40, 70));
	}

	#[test]
	fn levers_snap_and_stay_in_range() {
		assert_eq!(lever_angle((0., 0.), (10., 0.), false), 0.);
		assert_eq!(lever_angle((0., 0.), (0., -10.), false), 90.);
		assert_eq!(lever_angle((0., 0.), (-10., 0.01), false), 180.);
		assert_eq!(lever_angle((0., 0.), (10., -3.), true), 15.);
		assert_eq!(turn(179., 3.), -178.);
		assert_eq!(turn(-179., -1.), 180.);
	}

	#[test]
	fn grips_set_whole_extents() {
		let rect = |e| PartEdit::Kind(PartKindEdit::Rectangle(e));
		let r = PartKind::Rectangle(Rectangle::default());
		let edits = grip_edits(&r, Vec2::new(-20.4, 7.));
		assert_eq!(edits, vec![rect(RectangleEdit::Length(41)), rect(RectangleEdit::Width(14))]);
		let linked = PartKind::Rectangle(Rectangle { linked: true, ..Rectangle::default() });
		assert_eq!(grip_edits(&linked, Vec2::new(5., 30.)), vec![rect(RectangleEdit::Width(60))]);
		assert_eq!(grip_edits(&r, Vec2::new(0., 0.))[0], rect(RectangleEdit::Length(1)));
	}

	/// Every kind with a grip: dropping the grip where it stands leaves the part as it is, so a drag edits
	/// what the grip shows.
	#[test]
	fn grips_round_trip_through_their_edits() {
		let kinds = [
			PartKind::Rectangle(Rectangle::default()),
			PartKind::Ellipse(m::Ellipse::default()),
			PartKind::Lens(m::Lens::default()),
			PartKind::FresnelLens(m::FresnelLens::new_plate()),
			PartKind::Diffuser(m::Diffuser::default()),
			PartKind::FacettedGrating(m::FacettedGrating::default()),
			PartKind::InterferenceFilter(m::InterferenceFilter::new_part()),
			PartKind::MetaSurface(m::MetaSurface::new_part(m::MetaDesign::default())),
			PartKind::GrinLens(m::GrinLens::default()),
		];
		for kind in kinds {
			let grip = grip_of(&kind).expect("a grip");
			let part = Part::new(kind, 0, 0);
			let mut edited = part.clone();
			let edits = grip_edits(&part.kind, grip);
			assert!(!edits.is_empty(), "{:?}", part.kind);
			for edit in &edits {
				edit.apply(&mut edited).expect("the edit fits its kind");
			}
			assert_eq!(edited, part, "{:?}", part.kind);
		}
	}

	#[test]
	fn fibre_handles_mirror_on_smooth_nodes() {
		let fiber = Fiber {
			nodes: vec![
				FiberNode { x: 0, y: 0, out_x: 10, ..FiberNode::default() },
				FiberNode { x: 100, in_x: -20, out_x: 20, ..FiberNode::default() },
				FiberNode { x: 200, in_x: -10, ..FiberNode::default() },
			],
			..Fiber::default()
		};
		let n = moved_node(&fiber, HandleKind::InHandle(1), Vec2::new(100., -20.));
		assert_eq!((n[1].in_x, n[1].in_y, n[1].out_x, n[1].out_y), (0, -20, 0, 20));
		let n = moved_node(&fiber, HandleKind::OutHandle(1), Vec2::new(100., 10.));
		assert_eq!((n[1].in_x, n[1].in_y, n[1].out_x, n[1].out_y), (0, -20, 0, 10));
		let n = moved_node(&fiber, HandleKind::Node(2), Vec2::new(210.4, 5.6));
		assert_eq!((n[2].x, n[2].y, n[2].in_x), (210, 6, -10));
		assert_eq!(
			moved_point(&[Point::new(1, 1), Point::new(2, 2)], 1, Vec2::new(4.6, -1.2)),
			vec![Point::new(1, 1), Point::new(5, -1)]
		);
	}
}
