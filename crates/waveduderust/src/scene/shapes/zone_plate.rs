use super::{PartShape, Vec2, absorption_profile};
pub use crate::model::zone_at;
use crate::model::{Simulation, ZonePlate};

/// Blue level in the middle of a block.
pub const BLOCK_ABSORPTION: u32 = 96;

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
		let lambda = z.wavelength(sim.source_frequency_scale)?;
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
