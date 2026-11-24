use super::{Fields, SchemeId, Word, ranges};
use derive_setters::Setters;

/// An 8-bit RGB colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
#[derive(Setters)]
#[setters(prefix = "with_")]
pub struct Rgb {
	pub r: u8,
	pub g: u8,
	pub b: u8,
}

impl Rgb {
	pub const fn new(r: u8, g: u8, b: u8) -> Self {
		Self { r, g, b }
	}

	/// Parses "#RRGGBB" (case-insensitive, the hash optional).
	pub fn parse_hex(s: &str) -> Option<Self> {
		let s = s.trim();
		let s = s.strip_prefix('#').unwrap_or(s);
		if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
			return None;
		}
		let v = u32::from_str_radix(s, 16).ok()?;
		Some(Self::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
	}

	pub fn to_hex(self) -> String {
		format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
	}

	/// Hue in degrees 0..360 (0 for a grey), saturation and value 0..1.
	pub fn to_hsv(self) -> (f64, f64, f64) {
		let [r, g, b] = [self.r, self.g, self.b].map(|c| f64::from(c) / 255.0);
		let (max, min) = (r.max(g).max(b), r.min(g).min(b));
		let delta = max - min;
		let hue = if delta == 0.0 {
			0.0
		} else if max == r {
			60.0 * (g - b) / delta
		} else if max == g {
			60.0 * ((b - r) / delta + 2.0)
		} else {
			60.0 * ((r - g) / delta + 4.0)
		};
		(if hue < 0.0 { hue + 360.0 } else { hue }, if max > 0.0 { delta / max } else { 0.0 }, max)
	}

	/// The colour of `hue` (degrees, wrapped), `saturation` and `value` (clamped to 0..1), channels rounded half to even.
	pub fn from_hsv(hue: f64, saturation: f64, value: f64) -> Self {
		let hue = hue.rem_euclid(360.0);
		let (s, v) = (saturation.clamp(0.0, 1.0), value.clamp(0.0, 1.0));
		let c = v * s;
		let x = c * (1.0 - (hue / 60.0 % 2.0 - 1.0).abs());
		let (r, g, b) = match hue {
			h if h < 60.0 => (c, x, 0.0),
			h if h < 120.0 => (x, c, 0.0),
			h if h < 180.0 => (0.0, c, x),
			h if h < 240.0 => (0.0, x, c),
			h if h < 300.0 => (x, 0.0, c),
			_ => (c, 0.0, x),
		};
		let m = v - c;
		let channel = |ch: f64| ((ch + m) * 255.0).round_ties_even().clamp(0.0, 255.0) as u8;
		Self::new(channel(r), channel(g), channel(b))
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SchemeKind {
	#[default]
	Unspecified,
	Field,
	Intensity,
}

#[derive(Clone, Copy, Debug, PartialEq, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
#[derive(Setters)]
#[setters(prefix = "with_")]
pub struct Stop {
	/// Position 0..1.
	#[field(range = ranges::STOP_AT)]
	pub at: f64,
	#[field(nested)]
	pub color: Rgb,
}

impl Stop {
	pub const fn new(at: f64, color: Rgb) -> Self {
		Self { at, color }
	}
}

/// A gradient; how many colours are taken from it is the renderer's choice.
#[derive(Clone, Debug, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
#[derive(Setters)]
#[setters(prefix = "with_")]
pub struct ColorScheme {
	#[field(skip)]
	#[setters(skip)]
	#[cfg_attr(feature = "serde", serde(skip, default = "SchemeId::mint"))]
	pub(crate) id: SchemeId,
	pub name: String,
	pub kind: SchemeKind,
	pub description: String,
	/// File order; evaluation sorts by `at` (stable).
	pub stops: Vec<Stop>,
}

impl Default for ColorScheme {
	fn default() -> Self {
		Self {
			id: SchemeId::mint(),
			name: String::new(),
			kind: SchemeKind::Unspecified,
			description: String::new(),
			stops: Vec::new(),
		}
	}
}

super::ids::eq_by_value!(ColorScheme { name, kind, description, stops });

impl ColorScheme {
	pub fn id(&self) -> SchemeId {
		self.id
	}

	/// The stops sorted by position (stable); the grey ramp without any.
	pub fn sorted_stops(&self) -> Vec<Stop> {
		if self.stops.is_empty() {
			return Self::grey_ramp().stops;
		}
		let mut stops = self.stops.clone();
		stops.sort_by(|a, b| a.at.total_cmp(&b.at));
		stops
	}

	/// The gradient's colour at `at` (0..1), each channel rounded half away from zero.
	pub fn color_at(&self, at: f64) -> Rgb {
		let [r, g, b] = gradient_at(&self.sorted_stops(), at);
		Rgb::new(round_channel(r), round_channel(g), round_channel(b))
	}

	/// Adds a stop in the middle of the widest gap between stops in the gradient's colour there; with fewer than two,
	/// at 1, or at 0 when the one stop is at 1. Returns its index.
	pub fn add_stop(&mut self) -> usize {
		let mut sorted = self.stops.clone();
		sorted.sort_by(|a, b| a.at.total_cmp(&b.at));
		let at = match sorted.as_slice() {
			[a, b, rest @ ..] => {
				let pairs = std::iter::once((a, b)).chain(sorted[1..].iter().zip(rest));
				let mut widest = (a, b);
				for (x, y) in pairs.skip(1) {
					if y.at - x.at > widest.1.at - widest.0.at {
						widest = (x, y);
					}
				}
				f64::midpoint(widest.0.at, widest.1.at)
			}
			[only] if only.at >= 1.0 => 0.0,
			_ => 1.0,
		};
		self.add_stop_at(at)
	}

	/// Adds a stop at `at` (clamped to 0..1) in the gradient's colour there, after the stops at or before it.
	/// Returns its index.
	pub fn add_stop_at(&mut self, at: f64) -> usize {
		let at = at.clamp(0.0, 1.0);
		let stop = Stop::new(at, self.color_at(at));
		let k = self.stops.iter().filter(|s| s.at <= at).count();
		self.stops.insert(k, stop);
		k
	}

	/// Moves stop `k` to `at` (clamped to 0..1), passing the stops it crosses in the list. Returns its new index; None
	/// without stop `k`.
	pub fn move_stop(&mut self, k: usize, at: f64) -> Option<usize> {
		if k >= self.stops.len() {
			return None;
		}
		let at = at.clamp(0.0, 1.0);
		let mut to = k;
		if to > 0 && self.stops[to - 1].at > at {
			while to > 0 && self.stops[to - 1].at > at {
				to -= 1;
			}
		} else {
			while to + 1 < self.stops.len() && self.stops[to + 1].at < at {
				to += 1;
			}
		}
		let mut stop = self.stops.remove(k);
		stop.at = at;
		self.stops.insert(to, stop);
		Some(to)
	}

	/// Removes stop `k` unless it is the last one; whether it did.
	pub fn remove_stop(&mut self, k: usize) -> bool {
		let removed = self.stops.len() > 1 && k < self.stops.len();
		if removed {
			self.stops.remove(k);
		}
		removed
	}

	/// A new scheme's stops, and those of one without: nine greys spread evenly from black to white.
	pub fn grey_ramp() -> Self {
		const STOPS: u32 = 9;
		let stops = (0..STOPS)
			.map(|k| {
				let level = (f64::from(k) * 255.0 / f64::from(STOPS - 1)).round_ties_even() as u8;
				Stop { at: f64::from(k) / f64::from(STOPS - 1), color: Rgb::new(level, level, level) }
			})
			.collect();
		Self { name: "Grey".into(), stops, ..Self::default() }
	}

	/// The fallback for a missing scheme.
	pub fn grey() -> Self {
		Self {
			name: "Grey".into(),
			stops: vec![
				Stop { at: 0.0, color: Rgb::new(0, 0, 0) },
				Stop { at: 1.0, color: Rgb::new(0xFE, 0xFE, 0xFE) },
			],
			..Self::default()
		}
	}
}

/// The colour at `pos` (0..1) along `stops`, sorted by position and at least one; channels 0..255 unrounded. At
/// or below the first stop or NaN: the first colour; at or beyond the last: the last colour. Between, the segment
/// ends at the first stop at or beyond `pos`, so at the position of duplicate stops the first of them holds.
pub(crate) fn gradient_at(stops: &[Stop], pos: f64) -> [f64; 3] {
	let channels = |c: Rgb| [f64::from(c.r), f64::from(c.g), f64::from(c.b)];
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

/// A channel level rounded half away from zero, clamped to 0..255.
pub(crate) fn round_channel(c: f64) -> u8 {
	c.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod stop_tests {
	use super::*;

	#[test]
	fn hsv_round_trips_and_rounds_half_to_even() {
		for c in [
			Rgb::new(0, 0, 0),
			Rgb::new(255, 0, 0),
			Rgb::new(18, 200, 77),
			Rgb::new(128, 128, 128),
			Rgb::new(1, 2, 250),
		] {
			let (h, s, v) = c.to_hsv();
			assert_eq!(Rgb::from_hsv(h, s, v), c);
		}
		assert_eq!(Rgb::new(0, 255, 255).to_hsv(), (180.0, 1.0, 1.0));
		assert_eq!(Rgb::from_hsv(-120.0, 1.0, 1.0), Rgb::new(0, 0, 255));
		assert_eq!(Rgb::from_hsv(0.0, 0.0, 0.3), Rgb::new(76, 76, 76));
	}

	#[test]
	fn the_grey_ramp_has_nine_levels_rounded_to_even() {
		let levels: Vec<u8> = ColorScheme::grey_ramp().stops.iter().map(|s| s.color.r).collect();
		assert_eq!(levels, [0, 32, 64, 96, 128, 159, 191, 223, 255]);
		assert_eq!(ColorScheme::default().sorted_stops(), ColorScheme::grey_ramp().stops);
	}

	fn scheme(ats: &[f64]) -> ColorScheme {
		let stops = ats.iter().map(|&at| Stop::new(at, Rgb::new((at * 200.0) as u8, 0, 0))).collect();
		ColorScheme { stops, ..ColorScheme::default() }
	}

	fn ats(s: &ColorScheme) -> Vec<f64> {
		s.stops.iter().map(|s| s.at).collect()
	}

	#[test]
	fn a_new_stop_splits_the_widest_gap_in_the_gradients_colour() {
		let mut s = scheme(&[0.0, 0.2, 1.0]);
		assert_eq!(s.add_stop(), 2);
		assert_eq!(ats(&s), [0.0, 0.2, 0.6, 1.0]);
		assert_eq!(s.stops[2].color, Rgb::new(120, 0, 0));
		let mut one = scheme(&[1.0]);
		one.add_stop();
		assert_eq!(ats(&one), [0.0, 1.0]);
		let mut none = ColorScheme::default();
		none.add_stop();
		assert_eq!(none.stops, [Stop::new(1.0, Rgb::new(255, 255, 255))]);
	}

	#[test]
	fn a_moved_stop_passes_the_ones_it_crosses() {
		let mut s = scheme(&[0.0, 0.5, 1.0]);
		assert_eq!(s.move_stop(0, 0.7), Some(1));
		assert_eq!(ats(&s), [0.5, 0.7, 1.0]);
		assert_eq!(s.move_stop(2, -1.0), Some(0));
		assert_eq!(ats(&s), [0.0, 0.5, 0.7]);
		assert_eq!(s.move_stop(3, 0.1), None);
	}

	#[test]
	fn hex_is_six_digits_after_at_most_one_hash() {
		assert_eq!(Rgb::parse_hex(" #0aFf10 "), Some(Rgb::new(0x0A, 0xFF, 0x10)));
		for bad in ["+FFFFF", "##FF0000", "#FFF", "#GG0000"] {
			assert_eq!(Rgb::parse_hex(bad), None, "{bad}");
		}
	}

	#[test]
	fn the_last_stop_stays() {
		let mut s = scheme(&[0.0, 1.0]);
		assert!(s.remove_stop(0));
		assert!(!s.remove_stop(0));
		assert_eq!(ats(&s), [1.0]);
	}
}
