//! The number box's arithmetic (docs/ui/30, Number box; Next's `Controls/NumberBox.cs`): steps, scrubs, clamping and
//! rounding on the box's text, so `ui/number-box.slint` only routes input.

pub(crate) use crate::ui::NumberSpec;

/// A logarithmic box's step factor.
const STEP_FACTOR: f64 = 1.02;
/// A logarithmic scrub doubles the value every this many pixels.
const PIXELS_PER_DOUBLING: f64 = 40.0;
/// A linear scrub moves one increment every this many pixels, unless the box sets its own drag step.
const PIXELS_PER_STEP: f64 = 2.0;

/// A whole-number box over `min..=max`, stepping by 1.
pub(crate) fn whole(min: f32, max: f32) -> NumberSpec {
	NumberSpec { min, max, increment: 1.0, decimals: 0, ..Default::default() }
}

/// A setting as the decimal it was written as: `0.1f32` is 0.1, not 0.100000001490116.
pub(crate) fn wide(x: f32) -> f64 {
	x.to_string().parse().unwrap_or(f64::from(x))
}

/// The box's text as a number; a decimal comma counts as a point.
pub(crate) fn parse(text: &str) -> Option<f64> {
	text.trim().replace(',', ".").parse().ok().filter(|v: &f64| v.is_finite())
}

/// A logarithmic box's start: the value when positive, else the minimum when positive, else 1.
fn positive(spec: &NumberSpec, value: Option<f64>) -> f64 {
	match value {
		Some(v) if v > 0.0 => v,
		_ if spec.min > 0.0 => wide(spec.min),
		_ => 1.0,
	}
}

/// The decimals that show `increment` exactly, up to 10.
fn decimals_of(increment: f64) -> i32 {
	(0..10)
		.find(|&d| {
			let scaled = increment * 10f64.powi(d);
			(scaled - scaled.round()).abs() < 1e-9
		})
		.unwrap_or(10)
}

/// `value` clamped to the box's range and rounded as the box shows it.
fn fit(spec: &NumberSpec, value: f64) -> f64 {
	// a bound bound to data may cross the other: the minimum wins
	let min = wide(spec.min);
	let value = value.clamp(min, wide(spec.max).max(min));
	let value = if spec.log {
		format!("{value:.2e}").parse().unwrap_or(value)
	} else {
		let decimals = if spec.decimals >= 0 { spec.decimals } else { decimals_of(wide(spec.increment)) }.clamp(0, 10);
		let scale = 10f64.powi(decimals);
		(value * scale).round() / scale
	};
	// -0 shows as 0
	value + 0.0
}

/// [`fit`] as text.
fn write(spec: &NumberSpec, value: f64) -> String {
	fit(spec, value).to_string()
}

/// The value the box's text commits: parsed, clamped and rounded; `None` for text that is no number.
pub(crate) fn commit(spec: &NumberSpec, text: &str) -> Option<f64> {
	parse(text).map(|v| fit(spec, v))
}

/// `text` moved by `steps` increments (factors of 1.02 on a logarithmic box); text that is no number counts as 0.
pub(crate) fn step(spec: &NumberSpec, text: &str, steps: i32) -> String {
	let value = parse(text);
	write(
		spec,
		if spec.log {
			positive(spec, value) * STEP_FACTOR.powi(steps)
		} else {
			value.unwrap_or(0.0) + f64::from(steps) * wide(spec.increment)
		},
	)
}

/// The value a scrub `dx` pixels from where it started at `start` gives; `scale` is 0.1 with Shift, 10 with Ctrl.
pub(crate) fn scrub(spec: &NumberSpec, start: &str, dx: f64, scale: f64) -> String {
	let value = parse(start);
	let step = if spec.log {
		1.0 / PIXELS_PER_DOUBLING
	} else if spec.drag_step > 0.0 {
		wide(spec.drag_step)
	} else {
		wide(spec.increment) / PIXELS_PER_STEP
	} * scale;
	write(spec, if spec.log { positive(spec, value) * 2f64.powf(dx * step) } else { value.unwrap_or(0.0) + dx * step })
}

#[cfg(test)]
mod tests {
	use super::*;

	fn spec(min: f32, max: f32, increment: f32, decimals: i32, log: bool) -> NumberSpec {
		NumberSpec { min, max, increment, decimals, log, drag_step: 0.0 }
	}

	#[test]
	fn a_maximum_below_the_minimum_gives_the_minimum() {
		let spec = whole(1.0, 0.0);
		assert_eq!(fit(&spec, 5.0), 1.0);
	}

	#[test]
	fn steps_round_to_the_increment_and_clamp() {
		let s = spec(0.0, 2.55, 0.01, -1, false);
		assert_eq!(step(&s, "1.5", 1), "1.51");
		assert_eq!(step(&s, "1.5", -10), "1.4");
		assert_eq!(step(&s, "2.54", 5), "2.55");
		assert_eq!(step(&s, "x", 1), "0.01");
		assert_eq!(step(&spec(-10.0, 10.0, 1.0, -1, false), "0.4", -1), "-1");
		assert_eq!(step(&spec(0.0, 100.0, 1.0, 2, false), "1,25", 1), "2.25");
		assert_eq!(step(&spec(0.0, 10.0, 0.1, 8, false), "0", 3), "0.3");
		assert_eq!(step(&spec(0.0, 1.0, 0.00001, -1, false), "0", 1), "0.00001");
	}

	#[test]
	fn a_logarithmic_box_steps_by_factors_to_three_digits() {
		let s = spec(0.0001, 100000.0, 1.0, -1, true);
		assert_eq!(step(&s, "1", 1), "1.02");
		// not positive: starts from the minimum
		assert_eq!(step(&s, "0", 1), "0.000102");
		assert_eq!(step(&s, "1234", 1), "1260");
	}

	#[test]
	fn scrubs_follow_the_pointer() {
		let s = spec(0.0, 1000.0, 10.0, -1, false);
		assert_eq!(scrub(&s, "100", 20.0, 1.0), "200");
		assert_eq!(scrub(&s, "100", 20.0, 0.1), "110");
		assert_eq!(scrub(&spec(0.0001, 100000.0, 1.0, -1, true), "1", 40.0, 1.0), "2");
		assert_eq!(scrub(&NumberSpec { drag_step: 0.5, ..s }, "100", -10.0, 1.0), "95");
	}
}
