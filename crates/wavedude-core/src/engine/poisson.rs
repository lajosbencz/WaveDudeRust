//! A unit-rate Poisson process on the half-line, walked from 0: a probabilistic detector's detections.
//!
//! The number of events up to a position is Poisson-distributed with that position as its mean, and the counts
//! at two positions differ by the events between them. So one process, asked for the count at the detector's
//! cumulative expectation (intensity integrated over the steps, times the sensitivity), gives the same detections
//! whether asked once at the end or step by step, and the counts never fall. Seeded from the project's seed and
//! the probe's name, so a project gives the same detections every time.

use crate::rng::Pcg32;

/// The longest span, in expected events, walked event by event in one call: a span beyond it is jumped with a
/// normal approximation of its count (a sensitivity typed far too high would otherwise walk for ever), so
/// beyond it the two ways of asking agree in distribution only.
pub const WALK_LIMIT: f64 = 100_000.0;

#[derive(Clone, Debug)]
pub struct PoissonProcess {
	random: Pcg32,
	next: f64,
	count: u32,
}

impl PoissonProcess {
	/// A process whose events are drawn from `seed` on `stream`.
	pub fn new(seed: u64, stream: u64) -> Self {
		let mut random = Pcg32::new(seed, stream);
		let next = Self::exponential(&mut random);
		Self { random, next, count: 0 }
	}

	/// The events counted so far: the count at the largest position asked for.
	pub fn count(&self) -> u32 {
		self.count
	}

	/// The number of events up to `position` (the cumulative expectation); never less than a count given before.
	pub fn count_up_to(&mut self, position: f64) -> u32 {
		if !position.is_finite() {
			return self.count;
		}
		if position - self.next > WALK_LIMIT {
			// too far to walk: the count of the span up to a margin before the position is normal about its
			// length, the rest walked
			let span = position - self.next - WALK_LIMIT / 2.0;
			let jumped = (span + span.sqrt() * self.gaussian()).round();
			self.count = self.count.saturating_add(jumped.clamp(0.0, u32::MAX as f64) as u32);
			self.next += span;
		}
		while self.next <= position {
			self.count = self.count.saturating_add(1);
			self.next += Self::exponential(&mut self.random);
		}
		self.count
	}

	/// A gap between events: exponential with mean 1.
	fn exponential(random: &mut Pcg32) -> f64 {
		-(1.0 - random.next_f64()).ln()
	}

	/// A standard normal deviate (Box-Muller).
	fn gaussian(&mut self) -> f64 {
		let u = 1.0 - self.random.next_f64();
		let v = self.random.next_f64();
		(-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn counts_grow_with_the_position_and_repeat_per_seed() {
		let mut a = PoissonProcess::new(7, 3);
		let mut b = PoissonProcess::new(7, 3);
		let once = b.count_up_to(50.0);
		let mut last = 0;
		for k in 1..=50 {
			let c = a.count_up_to(k as f64);
			assert!(c >= last);
			last = c;
		}
		assert_eq!(last, once, "step by step or once at the end, the same count");
		assert_eq!(a.count_up_to(40.0), last, "a position behind the newest changes nothing");
		assert_eq!(a.count_up_to(f64::NAN), last);
		assert_ne!(PoissonProcess::new(8, 3).count_up_to(50.0), once);
		assert_ne!(PoissonProcess::new(7, 4).count_up_to(50.0), once);
	}

	/// Over many processes the count at 20 averages about 20 with a variance about 20.
	#[test]
	fn the_mean_is_the_position() {
		let counts: Vec<f64> = (0..2000).map(|s| f64::from(PoissonProcess::new(s, 1).count_up_to(20.0))).collect();
		let mean = counts.iter().sum::<f64>() / counts.len() as f64;
		let var = counts.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / counts.len() as f64;
		assert!((mean - 20.0).abs() < 0.5, "{mean}");
		assert!((var - 20.0).abs() < 3.0, "{var}");
	}

	#[test]
	fn a_far_position_is_jumped() {
		let mut p = PoissonProcess::new(1, 1);
		let c = f64::from(p.count_up_to(1e7));
		assert!((c - 1e7).abs() < 1e7 * 0.01, "{c}");
		assert!(p.count_up_to(1e7 + 10.0) >= c as u32);
	}
}
