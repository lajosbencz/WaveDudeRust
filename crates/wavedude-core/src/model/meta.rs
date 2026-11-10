//! A meta surface's design: the phase delay its cells ask and the fill factor each ridge gets for it.
//!
//! A ridge of effective index `n_eff` delays by `(2 pi / lambda) H (n_eff - 1)`; a cell filled to `f` has
//! `n_eff^2 = f n_p^2 + (1 - f)` (the lamellar effective medium), so `f = (n_eff^2 - 1) / (n_p^2 - 1)`, clamped
//! to 0..1: a full turn needs `H = lambda / (n_p - 1)`, and a lower pattern saturates.

use std::f64::consts::TAU;

use super::{MetaDesign, MetaLensType, MetaSurface, wavelength};
use crate::model::profiled::MAX_PIXELS;

impl MetaSurface {
	/// The wavelength in pixels in a medium of index 1 that the design frequency gives with the simulation's
	/// frequency `scale`; None without a finite positive one.
	pub fn wavelength(&self, scale: f64) -> Option<f64> {
		let lambda = wavelength(self.design_frequency, 1.0, scale);
		(lambda.is_finite() && lambda > 0.0).then_some(lambda)
	}

	/// The pitch as the pattern uses it: 1 to the width.
	pub fn guarded_pitch(&self) -> f64 {
		let width = self.width.max(2) as f64;
		if self.pitch.is_finite() { self.pitch.clamp(1.0, width) } else { 4.0 }
	}

	/// The height as the pattern uses it: 1 to [`MAX_PIXELS`].
	pub fn guarded_height(&self) -> f64 {
		if self.height.is_finite() { self.height.clamp(1.0, MAX_PIXELS) } else { 32.0 }
	}

	/// The height at which a filled cell delays by a whole turn, `lambda / (n_p - 1)`.
	pub fn full_turn_height(&self, scale: f64) -> Option<f64> {
		self.wavelength(scale).map(|lambda| lambda / (self.pattern_index - 1.0).max(1e-9))
	}

	/// Cells across the face: the width over the pitch, rounded down, 1 at least.
	pub fn cell_count(&self) -> usize {
		((self.width.max(2) as f64 / self.guarded_pitch()).floor() as usize).max(1)
	}

	/// The y' of the centre of `cell` (0 to `cell_count - 1`): the cells laid from one end, centred together on
	/// the axis.
	pub fn cell_centre(&self, cell: usize) -> f64 {
		(cell as f64 - (self.cell_count() - 1) as f64 / 2.0) * self.guarded_pitch()
	}

	/// `phase` wrapped into 0..2 pi.
	fn wrap(phase: f64) -> f64 {
		phase - TAU * (phase / TAU).floor()
	}

	/// A lens's phase lead at `y` from the axis relative to the centre, `(2 pi / lambda)(sqrt(F^2 + y^2) - F)`.
	fn lens_phase(focal_length: u32, lambda: f64, y: f64) -> f64 {
		let f = focal_length.max(1) as f64;
		TAU / lambda * (f.hypot(y) - f)
	}

	/// The phase delay the surface asks of the cell at `y` from the axis, in radians: a lens a full turn less
	/// the wrapped lens phase (converging) or that phase itself (diverging); a prism a line across the face,
	/// `-(2 pi / lambda) y sin(theta)`; an axicon a full turn less `(2 pi / lambda) |y| sin(beta)` wrapped.
	pub fn delay(&self, lambda: f64, y: f64) -> f64 {
		match &self.design {
			MetaDesign::Lens(l) => {
				let wrapped = Self::wrap(Self::lens_phase(l.focal_length, lambda, y));
				if l.lens_type == MetaLensType::Diverging { wrapped } else { TAU - wrapped }
			}
			MetaDesign::Prism(p) => -TAU / lambda * y * p.deflection.to_radians().sin(),
			MetaDesign::Axicon(a) => TAU - Self::wrap(TAU / lambda * y.abs() * a.cone_angle.to_radians().sin()),
		}
	}

	/// The fill factor of every cell, 0 (no ridge) to 1 (solid): the delay wrapped to a turn, a full turn kept
	/// as one (a converging centre cell is solid, not empty), the effective index the ridge must give,
	/// `1 + (delay / 2 pi) lambda / H`, and the fill from the lamellar mean of n^2. Empty without a wavelength.
	pub fn fill_factors(&self, scale: f64) -> Vec<f64> {
		let cells = self.cell_count();
		let Some(lambda) = self.wavelength(scale) else {
			return vec![0.0; cells];
		};
		let height = self.guarded_height();
		let n2 = self.pattern_index * self.pattern_index;
		(0..cells)
			.map(|j| {
				let asked = self.delay(lambda, self.cell_centre(j));
				let delay = if (0.0..=TAU).contains(&asked) { asked } else { Self::wrap(asked) };
				let effective = 1.0 + delay / TAU * lambda / height;
				if n2 > 1.0 { ((effective * effective - 1.0) / (n2 - 1.0)).clamp(0.0, 1.0) } else { 0.0 }
			})
			.collect()
	}

	/// The width of a prism's or an axicon's zone, a full turn of phase, `lambda / sin(angle)`; None for a lens or
	/// without a wavelength.
	pub fn zone_width(&self, scale: f64) -> Option<f64> {
		let lambda = self.wavelength(scale)?;
		match &self.design {
			MetaDesign::Lens(_) => None,
			MetaDesign::Prism(p) => Some(lambda / p.deflection.to_radians().sin().abs().max(1e-12)),
			MetaDesign::Axicon(a) => Some(lambda / a.cone_angle.to_radians().sin()),
		}
	}

	/// How far behind an axicon the line of focus reaches: the half width over tan(beta); None for the others.
	pub fn line_length(&self) -> Option<f64> {
		match &self.design {
			MetaDesign::Axicon(a) => Some(self.width.max(2) as f64 / 2.0 / a.cone_angle.to_radians().tan()),
			_ => None,
		}
	}

	/// How many zones, each a turn of phase, the surface has across its face (a lens's and an axicon's from the
	/// centre to the rim, a prism's across the whole), the last one cut by the rim; 0 without a wavelength.
	pub fn zone_count(&self, scale: f64) -> u32 {
		let Some(lambda) = self.wavelength(scale) else {
			return 0;
		};
		let half = self.width.max(2) as f64 / 2.0;
		let zones = match &self.design {
			MetaDesign::Lens(l) => Self::lens_phase(l.focal_length, lambda, half) / TAU,
			MetaDesign::Prism(_) => 2.0 * half / self.zone_width(scale).unwrap_or(f64::INFINITY),
			MetaDesign::Axicon(_) => half / self.zone_width(scale).unwrap_or(f64::INFINITY),
		};
		zones.floor() as u32 + 1
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{MetaAxiconDesign, MetaLensDesign, MetaPrismDesign};

	const SCALE: f64 = 2.0;

	#[test]
	fn a_converging_lens_fills_its_centre_and_thins_outward_within_a_zone() {
		let height = MetaSurface::default().full_turn_height(SCALE).unwrap();
		assert_eq!(height.round(), 32.0, "a full turn at the default wavelength and index");
		let lens = MetaSurface { height, ..MetaSurface::default() };
		assert_eq!(lens.wavelength(SCALE).map(f64::round), Some(32.0));
		assert_eq!(lens.cell_count(), 125);
		let fills = lens.fill_factors(SCALE);
		let centre = lens.cell_count() / 2;
		assert_eq!(fills[centre], 1.0, "a full turn is kept as one");
		assert!(fills[centre + 1] < 1.0 && fills[centre + 2] < fills[centre + 1]);
		assert!(fills.iter().all(|f| (0.0..=1.0).contains(f)));
		let diverging = MetaSurface {
			design: MetaDesign::Lens(MetaLensDesign { lens_type: MetaLensType::Diverging, ..Default::default() }),
			..lens.clone()
		};
		let d = diverging.fill_factors(SCALE);
		assert!(d[centre] < 0.01 && d[centre + 2] > d[centre + 1]);
		assert!(lens.zone_count(SCALE) >= 2);
		assert_eq!((lens.zone_width(SCALE), lens.line_length()), (None, None));
	}

	#[test]
	fn a_prism_climbs_across_each_zone_and_an_axicon_falls_from_the_axis() {
		let prism = MetaSurface { design: MetaDesign::Prism(MetaPrismDesign::default()), ..MetaSurface::default() };
		let width = prism.zone_width(SCALE).unwrap();
		let lambda = prism.wavelength(SCALE).unwrap();
		assert!((width - lambda / 20f64.to_radians().sin()).abs() < 1e-9);
		assert_eq!(prism.zone_count(SCALE), (500.0 / width) as u32 + 1);
		let fills = prism.fill_factors(SCALE);
		assert!(fills[1] != fills[0]);
		let axicon = MetaSurface { design: MetaDesign::Axicon(MetaAxiconDesign::default()), ..MetaSurface::default() };
		let fills = axicon.fill_factors(SCALE);
		let centre = axicon.cell_count() / 2;
		assert_eq!(fills[centre], 1.0);
		assert!((fills[centre + 1] - fills[centre - 1]).abs() < 1e-9, "symmetric about the axis");
		assert!((axicon.line_length().unwrap() - 250.0 / 10f64.to_radians().tan()).abs() < 1e-9);
		let low = MetaSurface { height: 4.0, ..axicon };
		assert!(low.fill_factors(SCALE).iter().filter(|&&f| f == 1.0).count() > 10, "a low pattern saturates");
	}
}
