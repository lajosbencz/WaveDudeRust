//! Pulse envelopes and the detach fade of object emitters (spec/30-engine.md, Sources).
//!
//! Time `t` is in steps, `omega` in radians per step; pulse widths count cycles of the emitter's
//! own frequency from t = 0 (the phase is not folded in).

use std::f64::consts::{PI, TAU};

use super::types::Emitter;
use crate::model::PulseShape;
use crate::rng::xorshift32;

/// Steps over which a detached emitter fades out.
pub const DETACH_RAMP_STEPS: f64 = 30.0;

/// Most pulse periods a random-gap emitter advances in one evaluation.
pub const CYCLE_ADVANCE_GUARD: u32 = 1000;

/// Half-width in cycles of the smoothed square's edges.
const EDGE_WIDTH: f64 = 0.35;

/// Angular frequency in radians per step of a frequency level: `level/255 * 0.5 * scale`.
pub fn level_omega(level: f64, scale: f64) -> f64 {
	level / 255.0 * 0.5 * scale
}

/// Detach fade at time `t`: `None` once the emitter has stopped driving the field, else 1 before
/// the detach step falling linearly to 0 over [`DETACH_RAMP_STEPS`].
pub fn detach_fade(emitter: &Emitter, t: f64) -> Option<f32> {
	let Some(after) = emitter.detach_after_step else {
		return Some(1.0);
	};
	let since = t - after as f64;
	if since >= DETACH_RAMP_STEPS {
		None
	} else if since > 0.0 {
		Some((1.0 - since / DETACH_RAMP_STEPS) as f32)
	} else {
		Some(1.0)
	}
}

/// Per-emitter pulse state: the random gap's anchor, current gap and xorshift32 state.
/// Owned by the engine's driver thread; reset with the engine.
#[derive(Clone, Debug, PartialEq)]
pub struct Pulse {
	anchor: f64,
	current_off: u32,
	rng: u32,
}

impl Pulse {
	pub fn new(emitter: &Emitter) -> Self {
		debug_assert!(
			emitter.on_width_cycles >= 1 && emitter.off_width_cycles >= 1,
			"pulse widths of at least a cycle"
		);
		Self { anchor: 0.0, current_off: emitter.off_width_cycles, rng: emitter.rng_seed.get() }
	}

	/// Envelope 0..1 of `emitter` at time `t` (steps) for angular frequency `omega` (rad/step);
	/// 1 for a continuous emitter. Advances the random gap state, so `t` must not decrease.
	pub fn envelope(&mut self, emitter: &Emitter, omega: f64, t: f64) -> f64 {
		if !emitter.pulsed {
			return 1.0;
		}
		let on = emitter.on_width_cycles as i64;
		let (pos, off, first) = self.position(emitter, omega * t / TAU);
		let period = on + off;
		let mut on_pos = pos - off as f64;
		match emitter.shape {
			PulseShape::Gaussian => {
				if on_pos < 0.0 {
					return 0.0;
				}
				let sigma = on as f64 / 6.0;
				let z = (on_pos - on as f64 / 2.0) / sigma;
				(-0.5 * z * z).exp()
			}
			// A rising step about the burst's start times a falling step about its end; the gap is shared at
			// its middle between the tail of the burst before and the rise of the burst to come. The first gap
			// has no burst before it (a random gap starts in one).
			PulseShape::SmoothedSquare => {
				if on_pos < -(off as f64) / 2.0 && !first {
					on_pos += period as f64;
				}
				let fall = on_pos - on as f64;
				(0.5 + (on_pos / EDGE_WIDTH).atan() / PI) * (0.5 - (fall / EDGE_WIDTH).atan() / PI)
			}
		}
	}

	/// Position in cycles within the current period, the gap of that period and whether it is the first.
	/// A fixed gap starts at the burst; a random gap starts in its off span.
	fn position(&mut self, emitter: &Emitter, cycles: f64) -> (f64, i64, bool) {
		let on = emitter.on_width_cycles as i64;
		if emitter.randomize_off_width == 0 {
			let off = emitter.off_width_cycles as i64;
			let period = on + off;
			let adjusted = cycles + off as f64;
			let turns = (adjusted / period as f64).floor();
			return (adjusted - period as f64 * turns, off, turns == 0.0);
		}
		let amount = emitter.randomize_off_width as i64;
		let mut off = self.current_off as i64;
		let mut period = on + off;
		let mut since = cycles - self.anchor;
		let mut guard = 0;
		while since >= period as f64 && guard < CYCLE_ADVANCE_GUARD {
			self.anchor += period as f64;
			since -= period as f64;
			self.rng = xorshift32(self.rng);
			off = (emitter.off_width_base as i64 + (self.rng % (2 * amount + 1) as u32) as i64 - amount).max(1);
			self.current_off = off as u32;
			period = on + off;
			guard += 1;
		}
		(since.max(0.0), off, self.anchor == 0.0)
	}
}

#[cfg(test)]
mod tests {
	use std::num::NonZeroU32;

	use super::*;

	fn emitter(shape: PulseShape, on: u32, off: u32, random: u32) -> Emitter {
		Emitter {
			x: 0.0,
			y: 0.0,
			level: 100.0,
			amplitude: 1.0,
			phase_radians: 0.0,
			pulsed: true,
			shape,
			on_width_cycles: on,
			off_width_cycles: off,
			off_width_base: off,
			randomize_off_width: random,
			detach_after_step: None,
			rng_seed: NonZeroU32::new(12345).unwrap(),
		}
	}

	/// Envelope sampled at `cycles` (omega = TAU, so t counts cycles).
	fn at(pulse: &mut Pulse, e: &Emitter, cycles: f64) -> f64 {
		pulse.envelope(e, TAU, cycles)
	}

	#[test]
	fn continuous_is_one() {
		let mut e = emitter(PulseShape::Gaussian, 3, 5, 0);
		e.pulsed = false;
		let mut p = Pulse::new(&e);
		for t in 0..100 {
			assert_eq!(p.envelope(&e, 0.3, t as f64), 1.0);
		}
	}

	#[test]
	fn gaussian_peaks_at_the_burst_middle() {
		let e = emitter(PulseShape::Gaussian, 6, 4, 0);
		let mut p = Pulse::new(&e);
		assert!((at(&mut p, &e, 3.0) - 1.0).abs() < 1e-12);
		assert!(at(&mut p, &e, 2.0) < 1.0 && at(&mut p, &e, 4.0) < 1.0);
		assert!((at(&mut p, &e, 2.0) - at(&mut p, &e, 4.0)).abs() < 1e-12);
		assert_eq!(at(&mut p, &e, 7.0), 0.0);
		assert!((at(&mut p, &e, 13.0) - 1.0).abs() < 1e-12, "the next burst's middle, one period later");
	}

	#[test]
	fn smoothed_square_rises_and_falls() {
		let e = emitter(PulseShape::SmoothedSquare, 10, 10, 0);
		let mut p = Pulse::new(&e);
		assert!(at(&mut p, &e, 0.0) > 0.45 && at(&mut p, &e, 0.0) < 0.55, "half way up at the first rising edge");
		assert!(at(&mut p, &e, 5.0) > 0.95);
		assert!(at(&mut p, &e, 10.0) < 0.55);
		assert!(at(&mut p, &e, 15.0) < 0.05);
		assert!(at(&mut p, &e, 25.0) > 0.95);
	}

	#[test]
	fn smoothed_square_burst_longer_than_its_gap_lasts_its_width() {
		let e = emitter(PulseShape::SmoothedSquare, 20, 2, 0);
		let mut p = Pulse::new(&e);
		assert!(at(&mut p, &e, 1.0) > 0.85, "whole from a cycle in");
		assert!(at(&mut p, &e, 11.0) > 0.95, "not cut at half the period");
		assert!(at(&mut p, &e, 19.0) > 0.85, "whole to a cycle before the falling edge");
		let edge = at(&mut p, &e, 20.0);
		assert!((0.45..0.55).contains(&edge), "half way down at the falling edge: {edge}");
		let middle = at(&mut p, &e, 21.0);
		assert!(middle < 0.15, "a tenth at the gap's middle: {middle}");
		let (before, after) = (at(&mut p, &e, 20.999), at(&mut p, &e, 21.001));
		assert!((before - after).abs() < 1e-2, "continuous across the gap's middle: {before} {after}");
		assert!(at(&mut p, &e, 23.0) > 0.85, "the next burst, whole from a cycle in");
	}

	#[test]
	fn random_gap_changes_between_pulses_and_repeats_for_a_seed() {
		let e = emitter(PulseShape::Gaussian, 2, 10, 8);
		let trace = |e: &Emitter| {
			let mut p = Pulse::new(e);
			let mut gaps = Vec::new();
			for k in 0..4000 {
				at(&mut p, e, k as f64 * 0.05);
				if gaps.last() != Some(&p.current_off) {
					gaps.push(p.current_off);
				}
			}
			gaps
		};
		let a = trace(&e);
		assert_eq!(a, trace(&e));
		assert!(a.len() > 3, "{a:?}");
		assert!(a.iter().all(|&g| (2..=18).contains(&g)));
		let mut other = e.clone();
		other.rng_seed = NonZeroU32::new(777).unwrap();
		assert_ne!(a, trace(&other));
	}

	#[test]
	fn random_gap_starts_in_the_off_span() {
		let e = emitter(PulseShape::Gaussian, 2, 10, 3);
		let mut p = Pulse::new(&e);
		assert_eq!(at(&mut p, &e, 0.5), 0.0);
		assert!(at(&mut p, &e, 11.0) > 0.99);
	}

	#[test]
	fn detach_fades_to_zero_over_thirty_steps() {
		let mut e = emitter(PulseShape::Gaussian, 1, 1, 0);
		e.detach_after_step = Some(100);
		assert_eq!(detach_fade(&e, 50.0), Some(1.0));
		assert_eq!(detach_fade(&e, 100.0), Some(1.0));
		assert_eq!(detach_fade(&e, 115.0), Some(0.5));
		assert!(detach_fade(&e, 129.0).unwrap() < 0.04);
		assert_eq!(detach_fade(&e, 130.0), None);
		e.detach_after_step = None;
		assert_eq!(detach_fade(&e, 1e9), Some(1.0));
	}
}
