use super::{Band, Body, Face, PartShape, Vec2};
use crate::model::Lens;
use crate::model::normalize::{fit_radii, sag};

/// A lens or curved mirror: the strip |y'| <= D/2 between the surfaces with vertices at -T/2 and +T/2.
pub(super) struct LensShape {
	half: f64,
	vertex1: f64,
	vertex2: f64,
	r1: f64,
	r2: f64,
	body: Body,
	extent: f64,
}

impl LensShape {
	pub fn new(lens: &Lens) -> Self {
		let half = lens.diameter as f64 / 2.0;
		let vertex1 = -(lens.thickness as f64) / 2.0;
		let vertex2 = -vertex1;
		let (r1, r2) = (lens.r1, lens.r2);
		debug_assert_eq!(fit_radii(lens.diameter, lens.thickness, r1, r2), (r1, r2), "radii fitted by normalisation");
		let reach = vertex2 + sag(r1, half).max(sag(r2, half));
		Self {
			half,
			vertex1,
			vertex2,
			r1,
			r2,
			body: Body::of(&lens.material, &lens.coating),
			extent: (reach * reach + half * half).sqrt(),
		}
	}

	/// How far inside one surface `p` lies; `side` -1 for the first surface (interior toward +x').
	/// Not a distance: beyond a convex surface's centre it is the radius, beyond a concave one's -inf.
	fn surface_depth(p: Vec2, vertex: f64, radius: f64, side: f64) -> f64 {
		if radius == 0.0 {
			return (p.x - vertex) * -side;
		}
		let centre = vertex + radius;
		let along = (p.x - centre) * -side;
		let distance = ((p.x - centre) * (p.x - centre) + p.y * p.y).sqrt();
		let magnitude = radius.abs();
		if radius * side < 0.0 {
			if along > 0.0 { magnitude } else { magnitude - distance }
		} else if along < 0.0 {
			f64::NEG_INFINITY
		} else {
			distance - magnitude
		}
	}

	fn depths3(&self, p: Vec2) -> (f64, f64, f64) {
		(
			Self::surface_depth(p, self.vertex1, self.r1, -1.0),
			Self::surface_depth(p, self.vertex2, self.r2, 1.0),
			self.half - p.y.abs(),
		)
	}
}

impl PartShape for LensShape {
	fn depth(&self, p: Vec2) -> f64 {
		let (first, second, rim) = self.depths3(p);
		first.min(second).min(rim)
	}

	fn body_index_at(&self, _p: Vec2) -> f64 {
		if self.body.mirror { 0.0 } else { self.body.index }
	}

	fn extent(&self) -> f64 {
		self.extent
	}

	/// The first surface is the front, the second the back, the rim the sides.
	fn face_at(&self, p: Vec2) -> Face {
		let (first, second, rim) = self.depths3(p);
		if first <= second && first <= rim {
			Face::Front
		} else if second <= rim {
			Face::Back
		} else {
			Face::Sides
		}
	}

	fn is_mirror(&self) -> bool {
		self.body.mirror
	}

	fn band(&self) -> Band {
		self.body.band
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
	fn flat_threshold_on_104_px() {
		assert!(sag(2704.0, 52.0) >= 0.5);
		assert!(sag(2705.0, 52.0) < 0.5);
		assert!(sag(14401.0, 52.0) < 0.5);
	}
}
