//! A zone plate's zones: `r_n^2 = n lam f + (n lam/2)^2` from the axis, every second one a block.

use super::{ZonePlate, wavelength};

/// The zone, from 0 at the axis, that the distance `r` falls in: the n with `r_n <= r < r_(n+1)`,
/// `r_n^2 = n lam f + (n lam/2)^2`, for a finite positive wavelength. Pixels.
pub fn zone_at(r: f64, lambda: f64, focal_length: u32) -> i64 {
	debug_assert!(lambda.is_finite() && lambda > 0.0, "wavelength {lambda}");
	let a = lambda * lambda / 4.0;
	let b = lambda * focal_length as f64;
	((-b + (b * b + 4.0 * a * r * r).sqrt()) / (2.0 * a)).floor() as i64
}

impl ZonePlate {
	/// The wavelength in pixels in a medium of index 1 that the design frequency gives with the simulation's
	/// frequency `scale`; None without a finite positive one.
	pub fn wavelength(&self, scale: f64) -> Option<f64> {
		let lambda = wavelength(self.design_frequency, 1.0, scale);
		(lambda.is_finite() && lambda > 0.0).then_some(lambda)
	}

	/// How many zones, open and blocked, lie within half the diameter, the last cut by the rim; 0 without a
	/// wavelength.
	pub fn zone_count(&self, scale: f64) -> u32 {
		self.wavelength(scale).map_or(0, |lambda| {
			let half = f64::from(self.diameter.max(2)) / 2.0;
			u32::try_from(zone_at(half, lambda, self.focal_length.max(1)).saturating_add(1)).unwrap_or(0)
		})
	}

	/// The width in pixels of the outermost whole zone within the diameter, `r_n - r_(n-1)`: what the grid must
	/// resolve; half the diameter with fewer than two zones, None without a wavelength.
	pub fn outer_zone_width(&self, scale: f64) -> Option<f64> {
		let lambda = self.wavelength(scale)?;
		let count = self.zone_count(scale);
		if count < 2 {
			return Some(f64::from(self.diameter.max(2)) / 2.0);
		}
		let f = f64::from(self.focal_length.max(1));
		let radius = |n: u32| {
			let step = f64::from(n) * lambda;
			(step * f + step / 2.0 * (step / 2.0)).sqrt()
		};
		Some(radius(count - 1) - radius(count - 2))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_outer_zone_narrows_as_the_zones_grow() {
		let plate = ZonePlate::default();
		let scale = crate::model::Simulation::default().source_frequency_scale;
		let count = plate.zone_count(scale);
		assert!(count >= 2, "{count}");
		let wide = plate.outer_zone_width(scale).expect("a wavelength");
		let wider =
			ZonePlate { diameter: plate.diameter / 2, ..plate.clone() }.outer_zone_width(scale).expect("a wavelength");
		assert!(wide < wider, "{wide} {wider}");
	}
}
