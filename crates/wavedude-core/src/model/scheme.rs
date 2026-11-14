use super::{Fields, Word, ranges};

/// An 8-bit RGB colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
		let s = s.trim().trim_start_matches('#');
		if s.len() != 6 {
			return None;
		}
		let v = u32::from_str_radix(s, 16).ok()?;
		Some(Self::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
	}

	pub fn to_hex(self) -> String {
		format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SchemeKind {
	#[default]
	Unspecified,
	Field,
	Intensity,
}

#[derive(Clone, Copy, Debug, PartialEq, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Stop {
	/// Position 0..1.
	#[field(range = ranges::STOP_AT)]
	pub at: f64,
	#[field(nested)]
	pub color: Rgb,
}

/// A gradient; how many colours are taken from it is the renderer's choice.
#[derive(Clone, Debug, PartialEq, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColorScheme {
	pub name: String,
	pub kind: SchemeKind,
	pub description: String,
	/// File order; evaluation sorts by `at` (stable).
	pub stops: Vec<Stop>,
}

impl Default for ColorScheme {
	fn default() -> Self {
		Self { name: String::new(), kind: SchemeKind::Unspecified, description: String::new(), stops: Vec::new() }
	}
}

impl ColorScheme {
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
