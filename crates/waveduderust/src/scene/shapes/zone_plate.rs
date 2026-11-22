use super::{PartShape, Vec2, absorption_profile};
use crate::model::{Simulation, ZonePlate, wavelength};

/// Blue level in the middle of a block.
pub const BLOCK_ABSORPTION: u32 = 96;

/// The zone, from 0 at the axis, that the distance `r` falls in: the n with `r_n <= r < r_(n+1)`,
/// `r_n^2 = n lam f + (n lam/2)^2`, for a finite positive wavelength. Pixels.
pub fn zone_at(r: f64, lambda: f64, focal_length: u32) -> i64 {
	debug_assert!(lambda.is_finite() && lambda > 0.0, "wavelength {lambda}");
	let a = lambda * lambda / 4.0;
	let b = lambda * focal_length as f64;
	((-b + (b * b + 4.0 * a * r * r).sqrt()) / (2.0 * a)).floor() as i64
}

/// A Fresnel zone plate: every second zone an absorbing block (blue only), the optional mount a
/// block beyond the diameter. The block's share rises as t^2 from the faces across the axis over the ramp.
pub(super) struct ZonePlateShape {
	half_thickness: f64,
	half_diameter: f64,
	outer_half_width: f64,
	ramp: f64,
	lambda: f64,
	focal_length: u32,
	inverse: bool,
}

impl ZonePlateShape {
	/// None when the design frequency gives no finite positive wavelength.
	pub(super) fn new(z: &ZonePlate, sim: &Simulation) -> Option<Self> {
		let lambda = wavelength(z.design_frequency, 1.0, sim.source_frequency_scale);
		if !lambda.is_finite() || lambda <= 0.0 {
			return None;
		}
		let half_thickness = z.thickness as f64 / 2.0;
		let half_diameter = z.diameter as f64 / 2.0;
		Some(Self {
			half_thickness,
			half_diameter,
			outer_half_width: half_diameter + if z.mount { z.mount_width as f64 } else { 0.0 },
			ramp: (z.coating as f64).min(half_thickness),
			lambda,
			focal_length: z.focal_length,
			inverse: z.inverse,
		})
	}

	fn is_block(&self, r: f64) -> bool {
		(zone_at(r.abs(), self.lambda, self.focal_length) % 2 == 1) != self.inverse
	}
}

impl PartShape for ZonePlateShape {
	fn depth(&self, p: Vec2) -> f64 {
		(self.half_thickness - p.x.abs()).min(self.outer_half_width - p.y.abs())
	}

	fn body_index_at(&self, _p: Vec2) -> f64 {
		1.0
	}

	fn extent(&self) -> f64 {
		(self.half_thickness * self.half_thickness + self.outer_half_width * self.outer_half_width).sqrt()
	}

	fn depth_is_distance(&self) -> bool {
		true
	}

	fn absorption(&self) -> u32 {
		BLOCK_ABSORPTION
	}

	fn absorption_ramp(&self) -> f64 {
		self.ramp.ceil().max(1.0)
	}

	fn absorption_share(&self, p: Vec2, _depth: f32, _ramp: f64) -> f64 {
		let across = p.y.abs();
		if across > self.outer_half_width || (across <= self.half_diameter && !self.is_block(p.y)) {
			return 0.0;
		}
		if self.ramp > 0.0 { absorption_profile((self.half_thickness - p.x.abs()) / self.ramp) } else { 1.0 }
	}

	fn paints_index(&self) -> bool {
		false
	}
}
