use super::{Body, PartShape, Vec2};
use crate::model::{Coating, SimpleGrating};

/// A simple 2D grating: the lattice of round dots, each a disc about its centre, one material (a mirror, or
/// glass with an index and absorption), no band. The depth is the radius less the distance to the nearest dot
/// centre, which the axis-aligned lattice gives by rounding along each axis, clamped to the lattice: the exact
/// signed distance to the union of the discs, which neither touch nor overlap (the diameter is at most half a
/// spacing).
pub(super) struct GratingShape {
	columns: u32,
	rows: u32,
	column_spacing: f64,
	row_spacing: f64,
	radius: f64,
	body: Body,
}

impl GratingShape {
	pub(super) fn new(g: &SimpleGrating) -> Self {
		Self {
			columns: g.columns.max(1),
			rows: g.rows.max(1),
			column_spacing: SimpleGrating::spacing(g.column_spacing) as f64,
			row_spacing: SimpleGrating::spacing(g.row_spacing) as f64,
			radius: g.guarded_diameter() as f64 / 2.0,
			body: Body::of(&g.material, &Coating { depth: 0.0, ..Coating::default() }),
		}
	}

	/// The lattice coordinate nearest to `v` among `count` positions `spacing` apart, centred on 0.
	fn nearest(v: f64, count: u32, spacing: f64) -> f64 {
		let half = (count - 1) as f64 / 2.0;
		let i = (v / spacing + half).round().clamp(0.0, (count - 1) as f64);
		(i - half) * spacing
	}
}

impl PartShape for GratingShape {
	fn depth(&self, p: Vec2) -> f64 {
		let dx = p.x - Self::nearest(p.x, self.columns, self.column_spacing);
		let dy = p.y - Self::nearest(p.y, self.rows, self.row_spacing);
		self.radius - dx.hypot(dy)
	}

	fn body_index_at(&self, _p: Vec2) -> f64 {
		if self.body.mirror { 0.0 } else { self.body.index }
	}

	fn extent(&self) -> f64 {
		let l = (self.columns - 1) as f64 / 2.0 * self.column_spacing + self.radius;
		let w = (self.rows - 1) as f64 / 2.0 * self.row_spacing + self.radius;
		l.hypot(w)
	}

	fn depth_is_distance(&self) -> bool {
		true
	}

	fn is_mirror(&self) -> bool {
		self.body.mirror
	}

	fn absorption(&self) -> u32 {
		self.body.absorption
	}

	fn absorption_ramp(&self) -> f64 {
		self.body.ramp
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_nearest_dot_by_rounding() {
		let g = SimpleGrating {
			columns: 2,
			rows: 3,
			column_spacing: 10,
			row_spacing: 8,
			diameter: 4,
			..Default::default()
		};
		let s = GratingShape::new(&g);
		assert_eq!(s.depth(Vec2::new(5.0, 8.0)), 2.0, "on a dot centre");
		assert_eq!(s.depth(Vec2::new(0.0, 0.0)), 2.0 - 5.0, "between the two columns");
		assert_eq!(s.depth(Vec2::new(5.0, 20.0)), 2.0 - 12.0, "beyond the last row, clamped to it");
		assert!((s.extent() - (5.0f64 + 2.0).hypot(8.0 + 2.0)).abs() < 1e-12);
	}
}
