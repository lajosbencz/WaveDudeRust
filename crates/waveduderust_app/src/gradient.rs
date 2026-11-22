//! A colour scheme's gradient (docs/ui/30, Colour scheme): its stops kept sorted, their edits, the colour at a
//! position, and the hex text of a colour.

use std::borrow::Cow;

use waveduderust::model::{Rgb, Stop};

/// Parses `#RRGGBB`, `RRGGBB` or `#RGB`, any case.
pub fn parse_hex(text: &str) -> Option<Rgb> {
	let t = text.trim().trim_start_matches('#');
	if t.len() == 3 && t.chars().all(|c| c.is_ascii_hexdigit()) {
		let d = |i: usize| u8::from_str_radix(&t[i..=i], 16).ok().map(|v| v * 17);
		return Some(Rgb::new(d(0)?, d(1)?, d(2)?));
	}
	Rgb::parse_hex(t)
}

/// Straight interpolation in sRGB, `t` 0..1.
pub fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
	let t = t.clamp(0., 1.);
	let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
	Rgb::new(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
}

/// Colour stops sorted by position.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient<'a> {
	pub stops: Cow<'a, [Stop]>,
}

impl<'a> Gradient<'a> {
	/// `stops` in place when sorted, else a sorted copy with the positions clamped.
	pub fn of(stops: &'a [Stop]) -> Self {
		if stops.is_sorted_by(|a, b| a.at <= b.at) && stops.iter().all(|s| (0. ..=1.).contains(&s.at)) {
			return Self { stops: Cow::Borrowed(stops) };
		}
		let mut v: Vec<Stop> = stops.iter().map(|s| Stop::new(s.at.clamp(0., 1.), s.color)).collect();
		v.sort_by(|a, b| a.at.total_cmp(&b.at));
		Self { stops: Cow::Owned(v) }
	}

	/// The stops, owned for an edit.
	pub fn into_stops(self) -> Vec<Stop> {
		self.stops.into_owned()
	}

	fn stops_mut(&mut self) -> &mut Vec<Stop> {
		self.stops.to_mut()
	}

	/// The colour at `t`, straight sRGB interpolation between neighbours; held beyond the ends.
	pub fn color_at(&self, t: f64) -> Rgb {
		let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) else { return Rgb::default() };
		if t <= first.at {
			return first.color;
		}
		for pair in self.stops.windows(2) {
			let (a, b) = (pair[0], pair[1]);
			if t <= b.at {
				let span = b.at - a.at;
				return if span <= 0. { b.color } else { lerp(a.color, b.color, (t - a.at) / span) };
			}
		}
		last.color
	}

	/// The middle of the widest gap, the ends counting as gaps.
	pub fn widest_gap(&self) -> f64 {
		let mut best = (0., 0.5);
		let mut left = 0.;
		for right in self.stops.iter().map(|s| s.at).chain([1.]) {
			if right - left > best.0 {
				best = (right - left, (left + right) / 2.);
			}
			left = right;
		}
		best.1
	}

	/// Inserts a stop and returns its index.
	pub fn insert(&mut self, at: f64, color: Rgb) -> usize {
		let at = at.clamp(0., 1.);
		let index = self.stops.iter().position(|s| s.at > at).unwrap_or(self.stops.len());
		self.stops_mut().insert(index, Stop::new(at, color));
		index
	}

	/// Add stop: in the widest gap, with the gradient's colour there.
	pub fn add_in_widest_gap(&mut self) -> usize {
		let t = self.widest_gap();
		let c = self.color_at(t);
		self.insert(t, c)
	}

	/// Removes a stop; the last one stays.
	pub fn remove(&mut self, index: usize) -> bool {
		if self.stops.len() <= 1 || index >= self.stops.len() {
			return false;
		}
		self.stops_mut().remove(index);
		true
	}

	/// Moves a stop and returns its index after re-sorting.
	pub fn set_position(&mut self, index: usize, at: f64) -> usize {
		let Some(stop) = self.stops.get(index).copied() else { return index };
		self.stops_mut().remove(index);
		self.insert(at, stop.color)
	}

	pub fn set_color(&mut self, index: usize, color: Rgb) {
		if let Some(s) = self.stops_mut().get_mut(index) {
			s.color = color;
		}
	}

	/// The stop whose marker is within `tolerance` of `t`: the nearest, among equals the one drawn on top.
	pub fn stop_near(&self, t: f64, tolerance: f64) -> Option<usize> {
		self.stops
			.iter()
			.enumerate()
			.rev()
			.map(|(i, s)| (i, (s.at - t).abs()))
			.filter(|(_, d)| *d <= tolerance)
			.min_by(|a, b| a.1.total_cmp(&b.1))
			.map(|(i, _)| i)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn hex_reads_both_lengths() {
		assert_eq!(parse_hex("#007acc"), Some(Rgb::new(0, 0x7A, 0xCC)));
		assert_eq!(parse_hex("FFF"), Some(Rgb::new(255, 255, 255)));
		assert_eq!(parse_hex("#12345"), None);
		assert_eq!(parse_hex("#GG0000"), None);
	}

	fn bw() -> Vec<Stop> {
		vec![Stop::new(1., Rgb::new(255, 255, 255)), Stop::new(0., Rgb::new(0, 0, 0))]
	}

	#[test]
	fn add_goes_into_the_widest_gap() {
		let stops = bw();
		let mut g = Gradient::of(&stops);
		assert_eq!(g.widest_gap(), 0.5);
		assert_eq!(g.add_in_widest_gap(), 1);
		assert_eq!(g.stops[1].at, 0.5);
		assert_eq!(g.stops[1].color, Rgb::new(128, 128, 128));
		let i = g.add_in_widest_gap();
		assert_eq!((i, g.stops[i].at), (1, 0.25));
		let one = [Stop::new(0.2, Rgb::default())];
		assert_eq!(Gradient::of(&one).widest_gap(), 0.6);
	}

	#[test]
	fn move_remove_and_hit() {
		let stops = bw();
		let mut g = Gradient::of(&stops);
		g.insert(0.5, Rgb::new(255, 0, 0));
		assert_eq!(g.set_position(1, 0.9), 1);
		assert_eq!(g.set_position(1, 1.5), 2);
		assert_eq!(g.stops[2].color, Rgb::new(255, 0, 0));
		assert_eq!(g.stop_near(0.98, 0.05), Some(2));
		assert_eq!(g.stop_near(0.5, 0.05), None);
		assert!(g.remove(0));
		assert!(g.remove(0));
		assert!(!g.remove(0));
		assert_eq!(g.stops.len(), 1);
	}
}
