//! Gradient evaluation of a colour scheme's stops.

use crate::model::{Rgb, Stop};

/// A normalised scheme's stops, ready for evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
	stops: Vec<Stop>,
}

impl Gradient {
	/// The stops of a normalised scheme: at least one, sorted by `at`.
	pub fn new(stops: &[Stop]) -> Self {
		assert!(!stops.is_empty(), "a normalised scheme has stops");
		debug_assert!(stops.is_sorted_by(|a, b| a.at <= b.at), "a normalised scheme's stops are sorted");
		Self { stops: stops.to_vec() }
	}

	pub fn stops(&self) -> &[Stop] {
		&self.stops
	}

	/// The colour at `pos` (0..1), channels 0..255 unrounded. At or below the first stop or NaN: the first colour;
	/// at or beyond the last: the last colour. Between, the segment ends at the first stop at or beyond `pos`, so at
	/// the position of duplicate stops the first of them holds.
	pub fn evaluate(&self, pos: f64) -> [f64; 3] {
		let stops = &self.stops;
		let first = &stops[0];
		let last = &stops[stops.len() - 1];
		if pos.is_nan() || pos <= first.at {
			return channels(first.color);
		}
		if pos >= last.at {
			return channels(last.color);
		}
		let hi = stops.partition_point(|s| s.at < pos);
		let (a, b) = (&stops[hi - 1], &stops[hi]);
		let t = (pos - a.at) / (b.at - a.at);
		let [ar, ag, ab] = channels(a.color);
		let [br, bg, bb] = channels(b.color);
		[ar + (br - ar) * t, ag + (bg - ag) * t, ab + (bb - ab) * t]
	}

	/// `count` colours at i / (count - 1), each channel rounded half away from zero.
	pub fn interpolate(&self, count: usize) -> Vec<Rgb> {
		let last = count.saturating_sub(1).max(1) as f64;
		(0..count)
			.map(|i| {
				let [r, g, b] = self.evaluate(i as f64 / last);
				Rgb::new(round_channel(r), round_channel(g), round_channel(b))
			})
			.collect()
	}
}

/// One channel (0..255, unrounded) through the black level `bl`, as a fraction 0..1:
/// `clamp(c / 255 * (1 - bl) + bl, 0, 1)`.
pub fn level(c: f64, bl: f64) -> f64 {
	(c / 255.0 * (1.0 - bl) + bl).clamp(0.0, 1.0)
}

fn channels(c: Rgb) -> [f64; 3] {
	[c.r as f64, c.g as f64, c.b as f64]
}

fn round_channel(c: f64) -> u8 {
	c.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
	use super::*;

	fn stop(at: f64, r: u8, g: u8, b: u8) -> Stop {
		Stop { at, color: Rgb::new(r, g, b) }
	}

	fn evaluate(stops: &[Stop], pos: f64) -> [f64; 3] {
		Gradient::new(stops).evaluate(pos)
	}

	#[test]
	fn ends_hold_and_segments_interpolate() {
		let stops = [stop(0.2, 0, 0, 0), stop(0.6, 200, 100, 40), stop(1.0, 255, 255, 255)];
		assert_eq!(evaluate(&stops, 0.0), [0.0, 0.0, 0.0]);
		assert_eq!(evaluate(&stops, f64::NAN), [0.0, 0.0, 0.0]);
		assert_eq!(evaluate(&stops, 1.5), [255.0, 255.0, 255.0]);
		assert_eq!(evaluate(&stops, 0.6), [200.0, 100.0, 40.0]);
		let [r, g, b] = evaluate(&stops, 0.4);
		assert!((r - 100.0).abs() < 1e-9 && (g - 50.0).abs() < 1e-9 && (b - 20.0).abs() < 1e-9);
	}

	#[test]
	fn the_first_of_duplicate_stops_holds() {
		let stops = [stop(0.0, 0, 0, 0), stop(0.5, 1, 1, 1), stop(0.5, 2, 2, 2), stop(1.0, 9, 9, 9)];
		assert_eq!(evaluate(&stops, 0.5), [1.0, 1.0, 1.0]);
		assert_eq!(evaluate(&stops, 0.75), [5.5, 5.5, 5.5]);
		assert_eq!(evaluate(&stops, 0.25), [0.5, 0.5, 0.5]);
	}

	#[test]
	fn interpolate_rounds_half_away() {
		let colours = Gradient::new(&[stop(0.0, 0, 0, 0), stop(1.0, 1, 3, 5)]).interpolate(3);
		assert_eq!(colours[1], Rgb::new(1, 2, 3));
	}

	#[test]
	fn black_level_lifts_and_clamps() {
		assert_eq!(level(0.0, 0.5), 0.5);
		assert_eq!(level(255.0, 0.5), 1.0);
		assert_eq!(level(0.0, -0.05), 0.0);
	}
}
