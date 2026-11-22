use super::{Band, Face, PartShape, Vec2};
use crate::model::GrinLens;

/// A rectangle `Length` x `Diameter` of index `nc - (nc - ne) (y'/(D/2))^2`.
pub(super) struct GrinShape {
	half_length: f64,
	half_diameter: f64,
	centre_index: f64,
	edge_index: f64,
	band: Band,
}

impl GrinShape {
	pub(super) fn new(g: &GrinLens) -> Self {
		Self {
			half_length: g.length as f64 / 2.0,
			half_diameter: g.diameter as f64 / 2.0,
			centre_index: g.centre_index,
			edge_index: g.edge_index,
			band: Band::of(&g.coating),
		}
	}
}

impl PartShape for GrinShape {
	fn depth(&self, p: Vec2) -> f64 {
		(self.half_length - p.x.abs()).min(self.half_diameter - p.y.abs())
	}

	/// Beyond the long sides the edge index (asked there for a pixel the outline cuts).
	fn body_index_at(&self, p: Vec2) -> f64 {
		let t = (p.y / self.half_diameter).clamp(-1.0, 1.0);
		self.centre_index - (self.centre_index - self.edge_index) * t * t
	}

	fn extent(&self) -> f64 {
		(self.half_length * self.half_length + self.half_diameter * self.half_diameter).sqrt()
	}

	fn depth_is_distance(&self) -> bool {
		true
	}

	/// The end faces are front (-x') and back, the long sides the sides.
	fn face_at(&self, p: Vec2) -> Face {
		if self.half_length - p.x.abs() <= self.half_diameter - p.y.abs() {
			if p.x < 0.0 { Face::Front } else { Face::Back }
		} else {
			Face::Sides
		}
	}

	fn band(&self) -> Band {
		self.band
	}
}
