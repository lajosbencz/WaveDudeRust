//! What dragging a picture's handle does to the model (docs/ui/40-preview-pane.md, Interaction): levers, size grips,
//! outline points and fibre nodes. Canvas pixels as continuous values; angles in degrees, counter-clockwise on
//! screen.

use crate::model::lens_geometry::{rim_x, set_vertex, vertex_x};
use crate::model::{FiberNode, Lens, Part, PartKind, Point, ZoneDivision};
use crate::scene::shapes::{Frame, Vec2};
use crate::view::overlay::HandleKind;

/// The angle a lever pivoting at `pivot` points at `pointer`: whole degrees, 15-degree steps with `snap`, in
/// (-180, 180].
pub fn lever_angle(pivot: (f64, f64), pointer: (f64, f64), snap: bool) -> f64 {
	let step = if snap { 15. } else { 1. };
	let a = ((pivot.1 - pointer.1).atan2(pointer.0 - pivot.0).to_degrees() / step).round() * step;
	// + 0 turns -0 into 0
	if a <= -180. { a + 360. } else { a + 0. }
}

/// `angle` turned by `by` degrees: whole degrees in (-180, 180].
pub fn turn_lever(angle: f64, by: f64) -> f64 {
	let a = (angle + by).round().rem_euclid(360.);
	if a > 180. { a - 360. } else { a }
}

/// The canvas point `p` in the frame of `part`.
pub fn to_part_frame(part: &Part, p: (f64, f64)) -> Vec2 {
	Frame::of(&part.base).to_frame(Vec2::new(p.0, p.1))
}

/// Sizes `kind` from its grip dragged to the part-frame point `q`: whole pixels, at least the kind's minimum; a
/// linked rectangle or ellipse takes the farther coordinate for both sides, a polygon's vertex stays on its ray.
pub fn drag_grip(kind: &mut PartKind, q: Vec2) {
	let span = |c: f64, min: u32| ((2. * c.abs()).round() as u32).max(min);
	let (length, width) = (span(q.x, 1), span(q.y, 1));
	match kind {
		PartKind::Rectangle(r) => {
			(r.length, r.width) = if r.linked { (length.max(width), length.max(width)) } else { (length, width) };
		}
		PartKind::Ellipse(e) => {
			(e.length, e.width) = if e.linked { (length.max(width), length.max(width)) } else { (length, width) };
		}
		PartKind::Polygon(p) => {
			let n = f64::from(p.sides.max(3));
			p.side = (2. * q.length() * (std::f64::consts::PI / n).sin()).round().max(2.) as u32;
		}
		PartKind::Lens(l) => l.diameter = span(q.y, 2),
		PartKind::FresnelLens(f) => f.lens.diameter = span(q.y, 2),
		PartKind::Diffuser(d) => (d.length, d.width) = (length, span(q.y, 2)),
		PartKind::FacettedGrating(g) => (g.length, g.width) = (length, span(q.y, 2)),
		PartKind::MetaSurface(m) => (m.length, m.width) = (length, span(q.y, 2)),
		PartKind::InterferenceFilter(f) => (f.length, f.width) = (length, width),
		PartKind::GrinLens(g) => (g.length, g.diameter) = (length, span(q.y, 2)),
		_ => {}
	}
}

/// Sizes `kind` by a key: its grip at the part-frame point `grip` moved by `d`. A linked rectangle or ellipse and a
/// polygon have one size, stepped by the move along the grip's ray (at least a pixel per step), so keys shrink them as
/// they grow them; the rest as a drag to `grip + d`.
pub fn nudge_grip(kind: &mut PartKind, grip: Vec2, d: Vec2) {
	let along = if grip.length() > 0. { d.dot(grip) / grip.length() } else { 0. };
	let step = along.signum() * along.abs().round().max(1.);
	let resize = |size: &mut u32, by: f64, min: f64| *size = (f64::from(*size) + by).round().max(min) as u32;
	match kind {
		PartKind::Rectangle(r) if r.linked => {
			resize(&mut r.length, 2. * step, 1.);
			r.width = r.length;
		}
		PartKind::Ellipse(e) if e.linked => {
			resize(&mut e.length, 2. * step, 1.);
			e.width = e.length;
		}
		PartKind::Polygon(p) => resize(&mut p.side, step, 2.),
		kind => drag_grip(kind, Vec2::new(grip.x + d.x, grip.y + d.y)),
	}
}

/// What a drag of a lens surface holds still, canvas pixels: its rim and the other vertex.
pub type Anchors = [(f64, f64); 2];

/// A lens kind's lens and zones: a Fresnel lens's, else one zone.
pub fn lens_of(kind: &PartKind) -> Option<(&Lens, u32, ZoneDivision)> {
	match kind {
		PartKind::Lens(l) => Some((l, 1, ZoneDivision::EqualWidth)),
		PartKind::FresnelLens(f) => Some((&f.lens, f.zones, f.division)),
		_ => None,
	}
}

/// What a drag of `surface`'s vertex holds still, in canvas pixels: its rim and the other vertex.
pub fn surface_anchors(part: &Part, surface: usize) -> Option<Anchors> {
	let (lens, zones, division) = lens_of(&part.kind)?;
	let frame = Frame::of(&part.base);
	let canvas = |x: f64| {
		let p = frame.to_canvas(Vec2::new(x, 0.));
		(p.x, p.y)
	};
	let other = if surface == 1 { 2 } else { 1 };
	Some([canvas(rim_x(lens, zones, division, surface)), canvas(vertex_x(lens.thickness, other))])
}

/// Where `surface`'s vertex lies in canvas pixels, reckoned from the other vertex of `anchors` and the thickness,
/// so the part's rounded centre does not creep into a run of keys.
pub fn surface_vertex(part: &Part, surface: usize, [_, other]: Anchors) -> Option<(f64, f64)> {
	let (lens, ..) = lens_of(&part.kind)?;
	let frame = Frame::of(&part.base);
	let side = if surface == 1 { -1. } else { 1. };
	let x = frame.to_frame(Vec2::new(other.0, other.1)).x + side * f64::from(lens.thickness.max(1));
	let p = frame.to_canvas(Vec2::new(x, 0.));
	Some((p.x, p.y))
}

/// Drags `surface`'s vertex along the axis to the canvas point `at`, the `anchors` staying: the part's centre
/// follows the middle of the vertices.
pub fn drag_surface(part: &mut Part, surface: usize, at: (f64, f64), [rim, other]: Anchors) {
	let frame = Frame::of(&part.base);
	let along = |p: (f64, f64)| frame.to_frame(Vec2::new(p.0, p.1)).x;
	let xs = (along(at), along(rim), along(other));
	let centre = match &mut part.kind {
		PartKind::Lens(l) => set_vertex(l, 1, ZoneDivision::EqualWidth, surface, xs),
		PartKind::FresnelLens(f) => set_vertex(&mut f.lens, f.zones, f.division, surface, xs),
		_ => return,
	};
	let c = frame.to_canvas(Vec2::new(centre, 0.));
	(part.base.x, part.base.y) = (c.x.round() as i32, c.y.round() as i32);
}

/// Puts outline point `i` at the part-frame point `q`.
pub fn drag_point(points: &mut [Point], i: usize, q: Vec2) {
	if let Some(p) = points.get_mut(i) {
		*p = Point::new(q.x.round() as i32, q.y.round() as i32);
	}
}

/// Drags a fibre's node or one of its Bezier handles (`kind`) to the part-frame point `q`. A node carries its
/// handles; a smooth node turns the opposite handle to stay opposite, keeping its length.
pub fn drag_fibre(nodes: &mut [FiberNode], kind: HandleKind, q: Vec2) {
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
		_ => {}
	}
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Polygon, Rectangle};

	#[test]
	fn a_lever_turns_counter_clockwise_and_snaps() {
		assert_eq!(lever_angle((0., 0.), (0., -10.), false), 90.);
		assert_eq!(lever_angle((0., 0.), (-10., 0.), false), 180.);
		assert_eq!(lever_angle((0., 0.), (-10., 0.1), false), -179.);
		assert_eq!(lever_angle((0., 0.), (10., -3.), true), 15.);
	}

	#[test]
	fn a_lever_turned_by_keys_wraps() {
		assert_eq!(turn_lever(175., 10.), -175.);
		assert_eq!(turn_lever(-175., -10.), 175.);
		assert_eq!(turn_lever(179.6, 0.), 180.);
	}

	#[test]
	fn a_linked_rectangle_takes_the_farther_coordinate() {
		let mut k = PartKind::Rectangle(Rectangle { linked: true, ..Rectangle::default() });
		drag_grip(&mut k, Vec2::new(10.2, -3.));
		let PartKind::Rectangle(r) = k else { unreachable!() };
		assert_eq!((r.length, r.width), (20, 20));
	}

	#[test]
	fn keys_shrink_a_linked_rectangle_and_step_a_polygon() {
		let mut k = PartKind::Rectangle(Rectangle { linked: true, length: 20, width: 20, ..Rectangle::default() });
		nudge_grip(&mut k, Vec2::new(10., 10.), Vec2::new(-1., 0.));
		let PartKind::Rectangle(r) = k else { unreachable!() };
		assert_eq!((r.length, r.width), (18, 18));
		let mut k = PartKind::Polygon(Polygon { sides: 20, side: 10, ..Polygon::default() });
		nudge_grip(&mut k, Vec2::new(0., 30.), Vec2::new(0., 1.));
		let PartKind::Polygon(p) = k else { unreachable!() };
		assert_eq!(p.side, 11);
	}

	#[test]
	fn a_polygon_vertex_sets_the_side_on_its_ray() {
		let mut k = PartKind::Polygon(Polygon { sides: 6, ..Polygon::default() });
		drag_grip(&mut k, Vec2::new(0., 30.));
		let PartKind::Polygon(p) = k else { unreachable!() };
		assert_eq!(p.side, 30);
	}

	#[test]
	fn a_smooth_node_turns_the_opposite_handle() {
		let mut nodes =
			[FiberNode { x: 0, y: 0, in_x: -10, in_y: 0, out_x: 10, out_y: 0, smooth: true, ..Default::default() }];
		drag_fibre(&mut nodes, HandleKind::OutHandle(0), Vec2::new(0., 5.));
		assert_eq!((nodes[0].in_x, nodes[0].in_y, nodes[0].out_x, nodes[0].out_y), (0, -10, 0, 5));
	}
}
