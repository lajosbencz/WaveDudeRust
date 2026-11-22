use super::{Band, Face, PartShape, Vec2};
use crate::model::MetaSurface;

/// A meta surface's pattern: one ridge per cell across the face, standing in front of the plate from
/// `x' = -length/2 - height` to the plate's front, centred in its cell and as wide as the cell's fill factor times
/// the pitch; the pattern index, no absorption, the band on the ridges' tops alone (a ridge's flanks and its
/// foot on the plate are bare). The depth is the signed distance to the union of the ridges, the greatest of the
/// nearby ridges' rectangle distances (the ridges never overlap, each inside its cell).
pub(super) struct MetaPatternShape {
	fills: Vec<f64>,
	pitch: f64,
	half_width: f64,
	outer: f64,
	inner: f64,
	index: f64,
	first_centre: f64,
	band: Band,
}

/// Signed distance inside the rectangle of half extents `hl` x `hw` about the origin, negative outside.
fn rectangle_depth(hl: f64, hw: f64, x: f64, y: f64) -> f64 {
	let dx = x.abs() - hl;
	let dy = y.abs() - hw;
	if dx <= 0.0 && dy <= 0.0 { -dx.max(dy) } else { -dx.max(0.0).hypot(dy.max(0.0)) }
}

impl MetaPatternShape {
	/// None when no cell holds a ridge wider than a hundredth of a pixel.
	pub(super) fn new(m: &MetaSurface, scale: f64) -> Option<Self> {
		let fills = m.fill_factors(scale);
		let pitch = m.guarded_pitch();
		if fills.iter().all(|f| f * pitch < 0.01) {
			return None;
		}
		let inner = -(m.length.max(1) as f64) / 2.0;
		let mirror = m.material.mirror;
		Some(Self {
			first_centre: m.cell_centre(0),
			fills,
			pitch,
			half_width: m.width.max(2) as f64 / 2.0,
			outer: inner - m.guarded_height(),
			inner,
			index: m.pattern_index,
			band: Band {
				width: if mirror { 0.0 } else { m.coating.depth },
				back: false,
				sides: false,
				..Band::of(&m.coating)
			},
		})
	}

	fn cell_at(&self, y: f64) -> i64 {
		((y - self.first_centre) / self.pitch).round() as i64
	}

	/// The signed distance to the ridge of `cell`, -inf for an empty one.
	fn ridge_depth(&self, cell: i64, p: Vec2) -> f64 {
		let Some(&fill) = usize::try_from(cell).ok().and_then(|c| self.fills.get(c)) else {
			return f64::NEG_INFINITY;
		};
		let half_ridge = fill * self.pitch / 2.0;
		if half_ridge < 0.005 {
			return f64::NEG_INFINITY;
		}
		let centre_x = (self.outer + self.inner) / 2.0;
		let centre_y = self.first_centre + cell as f64 * self.pitch;
		rectangle_depth((self.inner - self.outer) / 2.0, half_ridge, p.x - centre_x, p.y - centre_y)
	}
}

impl PartShape for MetaPatternShape {
	/// The nearest ridges: the cell under y' and its two neighbours (farther cells lie beyond them).
	fn depth(&self, p: Vec2) -> f64 {
		let cell = self.cell_at(p.y);
		let depth = (cell - 1..=cell + 1).map(|k| self.ridge_depth(k, p)).fold(f64::NEG_INFINITY, f64::max);
		if depth.is_finite() {
			depth
		} else {
			// no ridge nearby: outside by at least the distance to the pattern's box
			(-(self.outer - p.x).max(p.x - self.inner).max(p.y.abs() - self.half_width)).min(-1.0)
		}
	}

	fn body_index_at(&self, _p: Vec2) -> f64 {
		self.index
	}

	fn extent(&self) -> f64 {
		self.outer.abs().max(self.inner.abs()).hypot(self.half_width)
	}

	fn depth_is_distance(&self) -> bool {
		true
	}

	/// The top of the ridge (-x') is the front, its foot on the plate the back, its flanks the sides.
	fn face_at(&self, p: Vec2) -> Face {
		let cell = self.cell_at(p.y).clamp(0, self.fills.len() as i64 - 1);
		let centre_y = self.first_centre + cell as f64 * self.pitch;
		let half_ridge = self.fills[cell as usize] * self.pitch / 2.0;
		let top = p.x - self.outer;
		let foot = self.inner - p.x;
		let flank = half_ridge - (p.y - centre_y).abs();
		if top <= foot && top <= flank {
			Face::Front
		} else if foot <= flank {
			Face::Back
		} else {
			Face::Sides
		}
	}

	fn band(&self) -> Band {
		self.band
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Coating, MetaDesign, MetaPrismDesign};

	#[test]
	fn ridges_stand_in_their_cells_before_the_plate() {
		let m = MetaSurface {
			width: 16,
			length: 10,
			pitch: 4.0,
			height: 8.0,
			coating: Coating { depth: 2.0, ..Coating::default() },
			design: MetaDesign::Prism(MetaPrismDesign { deflection: 20.0 }),
			..MetaSurface::default()
		};
		let s = MetaPatternShape::new(&m, 2.0).unwrap();
		assert_eq!((s.outer, s.inner, s.fills.len()), (-13.0, -5.0, 4));
		let solid = s.fills.iter().position(|&f| f > 0.9).expect("a cell near a full turn");
		let y = s.first_centre + solid as f64 * s.pitch;
		assert!(s.depth(Vec2::new(-9.0, y)) > 1.0, "inside the ridge");
		assert!(s.depth(Vec2::new(-2.0, y)) < 0.0 && s.depth(Vec2::new(-20.0, 0.0)) <= -1.0);
		assert_eq!(s.face_at(Vec2::new(-12.9, y)), Face::Front);
		assert_eq!(s.face_at(Vec2::new(-5.1, y)), Face::Back);
		assert_eq!((s.band().front, s.band().back, s.band().sides), (true, false, false));
		assert!(MetaPatternShape::new(&MetaSurface { design_frequency: 0.0, ..m }, 2.0).is_none());
	}
}
