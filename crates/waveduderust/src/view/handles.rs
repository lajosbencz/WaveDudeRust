//! What dragging a picture's handle does to the model (docs/ui/40-preview-pane.md, Interaction): levers, size grips,
//! outline points and fibre nodes. Canvas pixels as continuous values; angles in degrees, counter-clockwise on
//! screen.

use crate::model::{FiberNode, Part, PartKind, Point};
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
	fn a_linked_rectangle_takes_the_farther_coordinate() {
		let mut k = PartKind::Rectangle(Rectangle { linked: true, ..Rectangle::default() });
		drag_grip(&mut k, Vec2::new(10.2, -3.));
		let PartKind::Rectangle(r) = k else { unreachable!() };
		assert_eq!((r.length, r.width), (20, 20));
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
