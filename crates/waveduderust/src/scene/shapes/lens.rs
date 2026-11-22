use super::{Band, Body, Face, PartShape, Vec2};
use crate::model::lens_geometry::{fit_radii_zoned, surface_sag, zone_bound, zone_index, zone_sag};
use crate::model::{Lens, ZoneDivision};

/// A lens or curved mirror: the strip |y'| <= D/2 between the surfaces with vertices at -T/2 and +T/2; with
/// more than one zone a Fresnel lens, each surface's arc restarted at the vertex plane at every zone's inner edge.
pub(super) struct LensShape {
	half: f64,
	vertex1: f64,
	vertex2: f64,
	r1: f64,
	r2: f64,
	zones: u32,
	division: ZoneDivision,
	body: Body,
	extent: f64,
}

impl LensShape {
	pub(super) fn new(lens: &Lens) -> Self {
		Self::zoned(lens, 1, ZoneDivision::EqualWidth)
	}

	pub(super) fn zoned(lens: &Lens, zones: u32, division: ZoneDivision) -> Self {
		let half = lens.diameter as f64 / 2.0;
		let vertex1 = -(lens.thickness as f64) / 2.0;
		let vertex2 = -vertex1;
		let (r1, r2) = (lens.r1, lens.r2);
		debug_assert_eq!(
			fit_radii_zoned(lens.diameter, lens.thickness, r1, r2, zones, division),
			(r1, r2),
			"radii fitted by normalisation"
		);
		let reach = vertex2 + zone_sag(r1, half, zones, division).max(zone_sag(r2, half, zones, division));
		Self {
			half,
			vertex1,
			vertex2,
			r1,
			r2,
			zones,
			division,
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

	/// [`Self::surface_depth`] for a surface cut into zones: the point's zone by |y'|, the arc that zone's circle
	/// moved along the axis so it starts at the vertex plane at the zone's inner edge, and the step across the
	/// axis that bounds the zone's glass: at its inner edge for a convex surface (the arc recedes outward, the
	/// next zone restarts ahead of it), at its outer edge for a concave one (the arc advances, the zone's glass
	/// ends there); the step's distance along y', or to its inner corner past the step's reach into the lens.
	fn zoned_depth(&self, p: Vec2, vertex: f64, radius: f64, side: f64) -> f64 {
		if self.zones == 1 || radius == 0.0 {
			return Self::surface_depth(p, vertex, radius, side);
		}
		let bound = |k| zone_bound(radius, self.half, self.zones, self.division, k);
		let across = p.y.abs();
		let zone = zone_index(radius, self.half, self.zones, self.division, across);
		let inner = bound(zone);
		let shift = surface_sag(radius, inner);
		let mut depth = Self::surface_depth(p, vertex - shift, radius, side);
		// the point's place along the axis from the vertex plane, positive into the lens
		let into = (p.x - vertex) * -side;
		if radius * side < 0.0 {
			if zone >= 1 {
				// convex: the step at the inner edge reaches into the lens by the previous zone's sag
				let reach = (shift - surface_sag(radius, bound(zone - 1))) * -side;
				let along = across - inner;
				let past = into - reach;
				depth = depth.min(if past <= 0.0 { along } else { along.hypot(past) });
			}
		} else if zone + 2 <= self.zones {
			// concave: the step at the outer edge stands out of the vertex plane by this zone's sag, its inner
			// corner on the plane
			let along = bound(zone + 1) - across;
			depth = depth.min(if into <= 0.0 { along } else { along.hypot(into) });
		}
		depth
	}

	fn depths3(&self, p: Vec2) -> (f64, f64, f64) {
		(
			self.zoned_depth(p, self.vertex1, self.r1, -1.0),
			self.zoned_depth(p, self.vertex2, self.r2, 1.0),
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
	use crate::model::lens_geometry::sag;

	#[test]
	fn flat_threshold_on_104_px() {
		assert!(sag(2704.0, 52.0) >= 0.5);
		assert!(sag(2705.0, 52.0) < 0.5);
		assert!(sag(14401.0, 52.0) < 0.5);
	}
}
