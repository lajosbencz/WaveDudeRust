//! What a number box edits (docs/ui/30, Number box): its range, linear or logarithmic stepping, precision, drag rate
//! and the text a value reads as. The frontends step, drag, round and clamp on their toolkit's range model.

/// How a step changes the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stepping {
	/// Adds the increment per step.
	Linear(f64),
	/// Multiplies by the factor per step (1.02: two percent); steps and drags keep three significant digits.
	Logarithmic(f64),
}

/// A number box.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberSpec {
	pub min: f64,
	pub max: f64,
	/// Fraction digits a linear box's steps and drags round to.
	pub decimals: u32,
	pub stepping: Stepping,
	/// Steps per PageUp/PageDown.
	pub page_steps: u32,
	/// Value change per dragged logical pixel of a linear box.
	drag_step: f64,
	/// Logical pixels of drag that double (or halve) a logarithmic box's value.
	doubling_pixels: f64,
	decimal_separator: char,
	/// A logarithmic box of whole numbers: rounds to integers and steps by at least one.
	whole: bool,
}

impl NumberSpec {
	/// A linear box; decimals follow the increment's, a drag moves one increment per 4 px.
	pub fn linear(min: f64, max: f64, step: f64) -> Self {
		Self {
			min,
			max,
			decimals: decimals_of(step),
			stepping: Stepping::Linear(step),
			page_steps: 10,
			drag_step: step / 4.,
			doubling_pixels: 100.,
			decimal_separator: '.',
			whole: false,
		}
	}

	pub fn integer(min: f64, max: f64) -> Self {
		Self::linear(min, max, 1.)
	}

	/// Two percent a step, ten steps a page, a doubling per 100 px of drag.
	pub fn logarithmic(min: f64, max: f64) -> Self {
		Self { stepping: Stepping::Logarithmic(1.02), decimals: 0, drag_step: 0., ..Self::linear(min, max, 1.) }
	}

	/// [`Self::logarithmic`] over whole numbers.
	pub fn logarithmic_whole(min: f64, max: f64) -> Self {
		Self { whole: true, ..Self::logarithmic(min, max) }
	}

	pub fn with_decimals(mut self, decimals: u32) -> Self {
		self.decimals = decimals;
		self
	}

	pub fn with_drag_step(mut self, per_pixel: f64) -> Self {
		self.drag_step = per_pixel;
		self
	}

	pub fn with_decimal_separator(mut self, separator: char) -> Self {
		self.decimal_separator = separator;
		self
	}

	/// Whether this is a [`Self::logarithmic_whole`] box.
	pub fn whole(&self) -> bool {
		self.whole
	}

	/// Logical pixels of drag per step of a linear box, per doubling of a logarithmic one.
	pub fn drag_pixels(&self) -> f64 {
		match self.stepping {
			Stepping::Linear(inc) => inc / self.drag_step,
			Stepping::Logarithmic(_) => self.doubling_pixels,
		}
	}

	/// The shortest text that reads back as `value` ("120", "1.5", "0.125"), with the box's separator.
	pub fn format(&self, value: f64) -> String {
		let text = format!("{}", value + 0.);
		if self.decimal_separator == '.' { text } else { text.replace('.', &self.decimal_separator.to_string()) }
	}

	/// Parses typed text; either decimal separator is accepted, surrounding blanks ignored.
	pub fn parse(&self, text: &str) -> Option<f64> {
		text.trim().replace([self.decimal_separator, ','], ".").parse::<f64>().ok().filter(|v| v.is_finite())
	}
}

/// Fraction digits of an increment as written (0.05 has 2, 10 has 0).
fn decimals_of(step: f64) -> u32 {
	let text = format!("{:.10}", step.abs());
	text.split('.').nth(1).unwrap_or("").trim_end_matches('0').len() as u32
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn decimals_follow_the_increment() {
		assert_eq!((decimals_of(1.), decimals_of(0.05), decimals_of(0.1), decimals_of(100.)), (0, 2, 1, 0));
		assert_eq!(NumberSpec::linear(1., 2.55, 0.01).decimals, 2);
	}

	#[test]
	fn drags_take_pixels_per_step_or_per_doubling() {
		assert_eq!(NumberSpec::linear(0., 100., 1.).drag_pixels(), 4.);
		assert_eq!(NumberSpec::linear(0., 100., 1.).with_decimals(1).with_drag_step(0.1).drag_pixels(), 10.);
		assert_eq!(NumberSpec::logarithmic(0.0001, 100_000.).drag_pixels(), 100.);
	}

	#[test]
	fn texts_read_back_with_either_separator() {
		let s = NumberSpec::linear(0., 10., 0.1);
		assert_eq!(s.format(1.5), "1.5");
		assert_eq!(s.format(-0.), "0");
		assert_eq!(s.parse(" 2,5 "), Some(2.5));
		assert_eq!(s.parse("x"), None);
		let comma = s.clone().with_decimal_separator(',');
		assert_eq!(comma.format(1.5), "1,5");
		assert_eq!(comma.parse("1.25"), Some(1.25));
	}
}
