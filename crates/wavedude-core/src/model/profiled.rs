//! Plates whose front face is a profile over y', one sample per pixel across the width: a diffuser's roughness
//! and a facetted grating's relief. The profile is what the painter's profiled shape and the overlay's outline
//! read; it is never stored.

use std::f64::consts::TAU;

use super::{Diffuser, FacetPattern, FacettedGrating};
use crate::rng::{Pcg32, stable_hash};

/// Longest wavelength, amplitude, sigma, pitch and relief in pixels.
pub const MAX_PIXELS: f64 = 10_000.0;
/// Shortest ripple or pitch in pixels: what a profile sampled a pixel apart can hold.
pub const MIN_WAVELENGTH: f64 = 2.0;

/// A plate of `length` along the axis and `width` across whose front face (-x') is `front`: its x' at every whole
/// pixel from y' = -width/2 to +width/2 (`width + 1` values), each at least a pixel before the back.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfiledPlate {
	pub length: u32,
	pub width: u32,
	pub front: Vec<f64>,
}

impl ProfiledPlate {
	/// The plate whose front face is `-length/2` plus `profile` (positive into the plate), held a pixel before the
	/// back so the plate is never cut through.
	fn of(length: u32, width: u32, profile: &[f64]) -> Self {
		let half = length.max(1) as f64 / 2.0;
		Self {
			length: length.max(1),
			width: width.max(2),
			front: profile.iter().map(|p| (-half + p).min(half - 1.0)).collect(),
		}
	}

	pub fn half_length(&self) -> f64 {
		self.length as f64 / 2.0
	}

	pub fn half_width(&self) -> f64 {
		self.width as f64 / 2.0
	}

	/// The outline: down the front face, pixel by pixel, then the two back corners.
	pub fn vertices(&self) -> Vec<(f64, f64)> {
		let (hl, hw) = (self.half_length(), self.half_width());
		let mut v: Vec<(f64, f64)> = self.front.iter().enumerate().map(|(i, &x)| (x, -hw + i as f64)).collect();
		v.push((hl, hw));
		v.push((hl, -hw));
		v
	}
}

/// How far the spread reaches either side of the centre wavelength, in sigmas, and how many components lie in each
/// sigma: a component every half sigma out to three sigmas, 13 in all, the outermost at e^-4.5 of the centre's.
const SPREAD_SIGMAS: i32 = 3;
const STEPS_PER_SIGMA: i32 = 2;
/// Stream selector XOR-ed onto the name hash, so the phases come from another stream than a source's jitter.
const ROUGHNESS_STREAM: u64 = 0x4449_4646;

/// One ripple of a diffuser's roughness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Component {
	pub wavelength: f64,
	pub amplitude: f64,
	/// Its place in the spread, -6..=6, which picks its phase.
	pub index: i32,
}

impl Diffuser {
	/// The wavelengths of the roughness and their amplitudes: the centre wavelength at the full amplitude and,
	/// with a sigma above 0, six more on either side a half sigma apart, each at the amplitude times
	/// e^(-(lambda - centre)^2 / 2 sigma^2); a wavelength below two pixels is left out.
	pub fn components(&self) -> Vec<Component> {
		let centre = self.center_wavelength.clamp(MIN_WAVELENGTH, MAX_PIXELS);
		let amplitude = self.amplitude.clamp(0.0, MAX_PIXELS);
		let sigma = self.sigma.clamp(0.0, MAX_PIXELS);
		if sigma <= 0.0 {
			return vec![Component { wavelength: centre, amplitude, index: 0 }];
		}
		let reach = SPREAD_SIGMAS * STEPS_PER_SIGMA;
		(-reach..=reach)
			.map(|k| (k, centre + f64::from(k) * sigma / f64::from(STEPS_PER_SIGMA)))
			.filter(|&(_, wavelength)| wavelength >= MIN_WAVELENGTH)
			.map(|(index, wavelength)| {
				let distance = wavelength - centre;
				Component {
					wavelength,
					amplitude: amplitude * (-distance * distance / (2.0 * sigma * sigma)).exp(),
					index,
				}
			})
			.collect()
	}

	/// The rough face's displacement from the plate's mean front plane at every whole pixel across the face,
	/// positive into the plate: the sum of the components, each a sine of 2 pi y' / lambda with a phase drawn
	/// from the project `seed` and the part's `name` (one stream per part, a draw per possible component in
	/// order, so a component keeps its phase whether its neighbours are there or not).
	pub fn profile(&self, name: &str, seed: i32) -> Vec<f64> {
		let mut rng = Pcg32::new(seed as u32 as u64, u64::from(stable_hash(name)) ^ ROUGHNESS_STREAM);
		let reach = SPREAD_SIGMAS * STEPS_PER_SIGMA;
		let phases: Vec<f64> = (0..2 * reach + 1).map(|_| TAU * rng.next_f64()).collect();
		let width = self.width.max(2) as usize;
		let half = width as f64 / 2.0;
		let mut profile = vec![0.0; width + 1];
		for c in self.components() {
			let phase = phases[(c.index + reach) as usize];
			let k = TAU / c.wavelength;
			for (i, p) in profile.iter_mut().enumerate() {
				*p += c.amplitude * (k * (i as f64 - half) + phase).sin();
			}
		}
		profile
	}

	pub fn plate(&self, name: &str, seed: i32) -> ProfiledPlate {
		ProfiledPlate::of(self.length, self.width, &self.profile(name, seed))
	}
}

impl FacettedGrating {
	/// The relief at every whole pixel across the face, positive into the plate: the pattern over the position in
	/// its pitch, from the face's centre.
	pub fn profile(&self) -> Vec<f64> {
		let width = self.width.max(2) as usize;
		let pitch = self.pitch.clamp(MIN_WAVELENGTH, MAX_PIXELS);
		let amplitude = self.amplitude.clamp(0.0, MAX_PIXELS);
		let half = width as f64 / 2.0;
		(0..=width)
			.map(|i| {
				let t = (i as f64 - half) / pitch;
				let t = t - t.floor();
				amplitude
					* match self.pattern {
						FacetPattern::Sawtooth => 2.0 * t - 1.0,
						FacetPattern::SquareWave => {
							if t < 0.5 {
								-1.0
							} else {
								1.0
							}
						}
						FacetPattern::Sinusoidal => (TAU * t).sin(),
					}
			})
			.collect()
	}

	pub fn plate(&self) -> ProfiledPlate {
		ProfiledPlate::of(self.length, self.width, &self.profile())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_spread_and_the_phases() {
		let d = Diffuser { sigma: 0.0, ..Diffuser::default() };
		assert_eq!(d.components().len(), 1);
		let d = Diffuser::default();
		let c = d.components();
		assert_eq!(c.len(), 11, "13 a half sigma apart, the two below 2 px left out");
		let centre = c.iter().find(|c| c.index == 0).unwrap();
		assert_eq!((centre.wavelength, centre.amplitude), (16.0, 1.0));
		assert!((c[0].amplitude - (-2.0f64).exp()).abs() < 1e-12 && c[0].wavelength == 4.0 && c[0].index == -4);
		let wide = Diffuser { center_wavelength: 40.0, ..Diffuser::default() };
		assert_eq!(wide.components().len(), 13);
		let narrow = Diffuser { center_wavelength: 4.0, ..Diffuser::default() };
		assert!(narrow.components().iter().all(|c| c.wavelength >= MIN_WAVELENGTH));
		let a = d.profile("Diffuser 1", 7);
		assert_eq!(a.len(), 501);
		assert_eq!(a, d.profile("Diffuser 1", 7), "the same project gives the same face");
		assert_ne!(a, d.profile("Diffuser 2", 7));
		assert_ne!(a, d.profile("Diffuser 1", 8));
		let plate = d.plate("Diffuser 1", 7);
		assert!(plate.front.iter().all(|&x| x < plate.half_length() - 1.0 + 1e-9));
		assert_eq!(plate.vertices().len(), 503);
	}

	#[test]
	fn facets() {
		let g = FacettedGrating { width: 64, pitch: 32.0, amplitude: 4.0, ..FacettedGrating::default() };
		let p = g.profile();
		assert_eq!((p[0], p[16], p[32]), (-4.0, 0.0, -4.0), "a sawtooth ramps from the trough and steps back");
		let square = FacettedGrating { pattern: FacetPattern::SquareWave, ..g.clone() }.profile();
		assert_eq!((square[0], square[15], square[16], square[31]), (-4.0, -4.0, 4.0, 4.0));
		let sine = FacettedGrating { pattern: FacetPattern::Sinusoidal, ..g }.profile();
		assert!((sine[8] - 4.0).abs() < 1e-9 && sine[0].abs() < 1e-9);
	}
}
