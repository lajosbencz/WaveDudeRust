//! A number box's [`NumberSpec`] as egui-mineral's range model, which steps, scrubs, rounds and clamps it.

use egui_mineral::range::{RangeModel, Scale};
use waveduderust_app::number::{NumberSpec, Stepping};

/// Significant digits of a logarithmic box.
const LOG_DIGITS: u8 = 3;

/// The range of a box. Linear: the increment added to the value, rounded to the spec's decimals; logarithmic: the
/// factor per step on three significant digits, on whole numbers for a whole box.
pub fn range_of(spec: &NumberSpec) -> RangeModel {
	let range = RangeModel::new(spec.min, spec.max);
	match spec.stepping {
		Stepping::Logarithmic(factor) => {
			debug_assert!(spec.min > 0., "a log box needs a positive minimum");
			let range = range.scale(Scale::Log).ratio(factor).significant(LOG_DIGITS);
			// a step that rounds back to the value moves one grid step instead, a page at least that many
			if spec.whole() { range.step(1.).jump(f64::from(spec.page_steps)) } else { range }
		}
		Stepping::Linear(inc) => range
			.increment(inc)
			.fraction_digits(u8::try_from(spec.decimals).unwrap_or(u8::MAX))
			.jump(inc * f64::from(spec.page_steps)),
	}
}

#[cfg(test)]
mod tests {
	use eframe::egui::Key;
	use egui_mineral::range::Track;

	use super::*;

	/// `n` steps (`page`: one page, its sign) from `v`.
	fn step(spec: &NumberSpec, v: f64, n: i64, page: bool) -> f64 {
		let range = range_of(spec);
		if page {
			let key = if n > 0 { Key::PageUp } else { Key::PageDown };
			range.on_key(v, key, Track::vertical()).expect("a page key")
		} else {
			range.step_by(v, n)
		}
	}

	/// The box's arithmetic as specified (docs/ui/30, Number box; NumberBox.cs:157-185): `steps` steps from `v`, rounded
	/// to the decimals or three significant digits, a whole box by at least one a step, clamped.
	fn reference(spec: &NumberSpec, v: f64, steps: i32) -> f64 {
		let stepped = match spec.stepping {
			Stepping::Linear(inc) => {
				let p = 10f64.powi(spec.decimals as i32);
				((v + inc * f64::from(steps)) * p).round() / p
			}
			Stepping::Logarithmic(factor) => {
				let base = if v > 0. { v } else { spec.min };
				let x = base * factor.powi(steps);
				let digits = 2 - x.abs().log10().floor() as i32;
				let p = 10f64.powi(digits);
				let x = ((x * p * 1e6).round() / 1e6).round() / p;
				let x = if spec.whole() { x.round() } else { x };
				if spec.whole() && (x - v).abs() < f64::from(steps.unsigned_abs()) { v + f64::from(steps) } else { x }
			}
		};
		stepped.clamp(spec.min, spec.max)
	}

	/// The range's step, asserted equal to the reference.
	fn agrees(spec: &NumberSpec, v: f64, n: i32, page: bool) -> f64 {
		let lib = step(spec, v, i64::from(n), page);
		let steps = if page { n * spec.page_steps as i32 } else { n };
		assert_eq!(lib, reference(spec, v, steps), "{spec:?} from {v}, {n} steps, page {page}");
		lib
	}

	#[test]
	fn linear_boxes_agree_with_the_reference_over_values_and_steps() {
		let amplitude = NumberSpec::linear(0.01, 255., 1.).with_decimals(2);
		let angle = NumberSpec::linear(-360., 360., 5.).with_decimals(1);
		let half = NumberSpec::linear(0., 100., 0.5);
		let speed = NumberSpec::linear(1e-6, 1e6, 0.01);
		let level = NumberSpec::linear(-10., 1., 0.01);
		let seed = NumberSpec::integer(f64::from(i32::MIN), f64::from(i32::MAX));
		let values: [f64; 9] = [-400., 0., 0.01, 0.3, 1., 1.2345, 12.34, 254.5, 1e6];
		for spec in [&amplitude, &angle, &half, &speed, &level, &seed] {
			for v in values.map(|v| v.clamp(spec.min, spec.max)) {
				for (n, page) in [(1, false), (-1, false), (3, false), (1, true), (-1, true)] {
					agrees(spec, v, n, page);
				}
			}
		}
	}

	#[test]
	fn logarithmic_boxes_agree_with_the_reference_across_the_range() {
		let spec = NumberSpec::logarithmic(0.0001, 100_000.);
		let up = std::iter::successors(Some(spec.min), |&v| Some(agrees(&spec, v, 1, false)).filter(|&n| n != v));
		assert!(up.count() > 900);
		let down = std::iter::successors(Some(spec.max), |&v| Some(agrees(&spec, v, -1, false)).filter(|&n| n != v));
		assert!(down.count() > 900);
		let undo = NumberSpec::logarithmic_whole(10., 100_000.);
		for v in [10., 11., 24., 25., 99., 1000., 99_999.] {
			for (n, page) in [(1, false), (-1, false), (1, true), (-1, true)] {
				agrees(&undo, v, n, page);
			}
		}
	}

	#[test]
	fn linear_boxes_add_the_increment_round_to_the_decimals_and_clamp() {
		let half = NumberSpec::linear(0., 10., 0.5);
		assert_eq!(step(&half, 1., 1, false), 1.5);
		assert_eq!(step(&half, 1., -3, false), 0.);
		assert_eq!(step(&half, 1., 1, true), 6.);
		assert_eq!(step(&half, 6., 1, true), 10.);
		assert_eq!(step(&NumberSpec::linear(0., 1., 0.1), 0.2, 1, false), 0.3);
		assert_eq!(step(&NumberSpec::linear(0.01, 100., 0.25), 0.1, -1, false), 0.01);
		let amplitude = NumberSpec::linear(0.01, 255., 1.).with_decimals(2);
		assert_eq!((step(&amplitude, 1.5, 1, false), step(&amplitude, 0.01, 1, false)), (2.5, 1.01));
		assert_eq!(step(&NumberSpec::linear(-360., 360., 5.).with_decimals(1), 12.3, 1, false), 17.3);
	}

	#[test]
	fn logarithmic_boxes_step_two_percent_on_three_digits() {
		let spec = NumberSpec::logarithmic(0.0001, 100_000.);
		assert_eq!((step(&spec, 1., 1, false), step(&spec, 1., -1, false)), (1.02, 0.98));
		assert_eq!((step(&spec, 1.02, 1, false), step(&spec, 1234., -1, false)), (1.04, 1210.));
		assert_eq!((step(&spec, 0.00225, 1, false), step(&spec, 2750., 1, false)), (0.0023, 2810.));
		assert_eq!(step(&spec, 1., 1, true), 1.22);
		let up = std::iter::successors(Some(spec.min), |&v| Some(step(&spec, v, 1, false)).filter(|&n| n != v));
		assert!(up.count() > 900, "stalled below the maximum");
	}

	#[test]
	fn a_whole_logarithmic_box_steps_by_at_least_one() {
		let undo = NumberSpec::logarithmic_whole(10., 100_000.);
		for (from, n, to) in [(10., 1, 11.), (24., 1, 25.), (11., -1, 10.), (1000., 1, 1020.), (10., -1, 10.)] {
			assert_eq!(step(&undo, from, n, false), to, "{from} by {n}");
		}
		assert_eq!((step(&undo, 1000., 1, true), step(&undo, 10., 1, true)), (1220., 20.));
	}
}
