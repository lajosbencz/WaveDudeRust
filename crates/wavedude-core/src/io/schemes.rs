//! Colour scheme lookup: project schemes, then the user's, then the 23 built-ins embedded
//! from `schemes/*.wdscheme`; and the Wave Dude 2D JSON scheme reader.

use std::sync::OnceLock;

use serde_json::Value;

use super::{FormatError, wdscheme};
use crate::model::normalize::Normalize;
use crate::model::{ColorScheme, Rgb, SchemeKind, Stop};

/// The shipped scheme files, verbatim.
pub const BUILT_IN_FILES: &[&str] = &[
	include_str!("../../schemes/Blue to yellow.wdscheme"),
	include_str!("../../schemes/Bright rainbow.wdscheme"),
	include_str!("../../schemes/Copper.wdscheme"),
	include_str!("../../schemes/Default field.wdscheme"),
	include_str!("../../schemes/Default intensity.wdscheme"),
	include_str!("../../schemes/Flare inverted.wdscheme"),
	include_str!("../../schemes/Gray scales inverted.wdscheme"),
	include_str!("../../schemes/Gray scales.wdscheme"),
	include_str!("../../schemes/Green blue.wdscheme"),
	include_str!("../../schemes/Green pink.wdscheme"),
	include_str!("../../schemes/Greens.wdscheme"),
	include_str!("../../schemes/Hot orange.wdscheme"),
	include_str!("../../schemes/Ice and fire.wdscheme"),
	include_str!("../../schemes/Inferno.wdscheme"),
	include_str!("../../schemes/Magma.wdscheme"),
	include_str!("../../schemes/Orange hot.wdscheme"),
	include_str!("../../schemes/Orange symmetry.wdscheme"),
	include_str!("../../schemes/Plasma.wdscheme"),
	include_str!("../../schemes/Purple rain.wdscheme"),
	include_str!("../../schemes/Rainbow.wdscheme"),
	include_str!("../../schemes/Simply red.wdscheme"),
	include_str!("../../schemes/Turkey brown.wdscheme"),
	include_str!("../../schemes/Yellow purple.wdscheme"),
];

/// The built-in schemes, parsed once, in file name order.
pub fn built_in() -> &'static [ColorScheme] {
	static SCHEMES: OnceLock<Vec<ColorScheme>> = OnceLock::new();
	SCHEMES.get_or_init(|| {
		BUILT_IN_FILES.iter().map(|text| wdscheme::read_str(text, "").expect("built-in schemes parse")).collect()
	})
}

/// Wave Dude 2D names whose spaced form is not their underscores replaced.
const LEGACY_NAMES: &[(&str, &str)] = &[
	("Blue_to_yellow_GNU", "Blue to yellow"),
	("Flare_inv", "Flare inverted"),
	("Gray_scales_inv", "Gray scales inverted"),
	("Turky_brown", "Turkey brown"),
];

/// Finds `name` case-insensitively; a legacy name with underscores is retried under its spaced name.
pub fn find<'a>(schemes: &'a [ColorScheme], name: &str) -> Option<&'a ColorScheme> {
	let by_name = |n: &str| schemes.iter().find(|s| s.name.to_lowercase() == n.to_lowercase());
	if let Some(found) = by_name(name) {
		return Some(found);
	}
	if !name.contains('_') {
		return None;
	}
	let renamed = LEGACY_NAMES
		.iter()
		.find(|(old, _)| old.eq_ignore_ascii_case(name))
		.map_or_else(|| name.replace('_', " "), |(_, new)| (*new).to_owned());
	by_name(&renamed)
}

/// Resolves scheme names for rendering: the project's schemes, then the user's, then the built-ins.
#[derive(Clone, Debug, Default)]
pub struct SchemeStore {
	/// The user's schemes.
	pub user: Vec<ColorScheme>,
}

impl SchemeStore {
	/// The scheme `name` resolves to; [`ColorScheme::grey`] when none matches.
	pub fn resolve(&self, project_schemes: &[ColorScheme], name: &str) -> ColorScheme {
		find(project_schemes, name)
			.or_else(|| find(&self.user, name))
			.or_else(|| find(built_in(), name))
			.cloned()
			.unwrap_or_else(ColorScheme::grey)
	}
}

/// Entries of the original's colour tables.
const AUTHOR_ENTRY_COUNT: usize = 255;
/// Stops of the original's editor form.
const AUTHOR_STOP_COUNT: usize = 9;

fn colour_list(v: &Value) -> Result<Vec<Rgb>, FormatError> {
	let bad = || FormatError::new("expected a list of [r,g,b] triplets");
	v.as_array()
		.ok_or_else(bad)?
		.iter()
		.map(|item| {
			let ch: Vec<f64> = item
				.as_array()
				.ok_or_else(bad)?
				.iter()
				.map(|c| c.as_f64().ok_or_else(|| FormatError::new("a colour channel must be a number")))
				.collect::<Result<_, _>>()?;
			if ch.len() < 3 {
				return Err(FormatError::new("a colour needs three values [r,g,b]"));
			}
			let c = |x: f64| x.round().clamp(0.0, 255.0) as u8;
			Ok(Rgb::new(c(ch[0]), c(ch[1]), c(ch[2])))
		})
		.collect()
}

fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
	let ch = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
	Rgb::new(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b))
}

/// Whether a 255-entry table is the original's interpolation of `stops`.
fn is_interpolation_of(table: &[Rgb], stops: &[Rgb]) -> bool {
	if table.len() != AUTHOR_ENTRY_COUNT || stops.len() < 2 {
		return false;
	}
	(0..AUTHOR_ENTRY_COUNT).all(|i| {
		let position = (i * (stops.len() - 1)) as f64 / (AUTHOR_ENTRY_COUNT - 1) as f64;
		let index = position.floor() as usize;
		let expected = if index >= stops.len() - 1 {
			stops[stops.len() - 1]
		} else {
			lerp(stops[index], stops[index + 1], position - index as f64)
		};
		expected == table[i]
	})
}

/// Stops for colours at k / (n - 1); a single colour sits at 0.
fn spread(colors: &[Rgb]) -> Vec<Stop> {
	let n = colors.len();
	colors
		.iter()
		.enumerate()
		.map(|(k, &color)| Stop { at: if n == 1 { 0.0 } else { k as f64 / (n - 1) as f64 }, color })
		.collect()
}

fn kind_of(scheme_type: &str) -> SchemeKind {
	match scheme_type.trim().to_lowercase().as_str() {
		"field" => SchemeKind::Field,
		"intensity" => SchemeKind::Intensity,
		_ => SchemeKind::Unspecified,
	}
}

fn text_of(v: Option<&Value>) -> Option<String> {
	match v? {
		Value::String(s) => Some(s.clone()),
		Value::Number(n) => Some(n.to_string()),
		Value::Bool(b) => Some(b.to_string()),
		_ => None,
	}
}

/// Parses a Wave Dude 2D JSON scheme (`name`, `origin`, `scheme_type`, `colors`, `stops`): nine `stops`
/// are taken when `colors` is missing or is their interpolation, else `colors` become evenly spaced stops.
pub fn parse_author_json(text: &str, fallback_name: &str) -> Result<ColorScheme, FormatError> {
	let root: Value = serde_json::from_str(text)
		.map_err(|e| FormatError { message: format!("not valid JSON: {e}"), at: Some((e.line(), e.column())) })?;
	let obj =
		root.as_object().ok_or_else(|| FormatError::new("not a colour scheme file (a JSON object was expected)"))?;
	let colors = obj.get("colors").map(colour_list).transpose()?;
	let stops = obj.get("stops").map(colour_list).transpose()?.filter(|s| s.len() == AUTHOR_STOP_COUNT);
	let kept = match (&stops, &colors) {
		(Some(s), c) if c.as_ref().is_none_or(|c| c.len() < 2 || is_interpolation_of(c, s)) => s,
		(_, Some(c)) if c.len() >= 2 => c,
		_ => return Err(FormatError::new("the file contains no usable 'colors' list")),
	};
	let mut scheme = ColorScheme {
		name: text_of(obj.get("name")).unwrap_or_else(|| fallback_name.to_owned()),
		kind: kind_of(&text_of(obj.get("scheme_type")).unwrap_or_default()),
		description: text_of(obj.get("origin")).unwrap_or_default(),
		stops: spread(kept),
	};
	scheme.normalize();
	Ok(scheme)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn built_ins_are_named_after_their_files() {
		let schemes = built_in();
		assert_eq!(schemes.len(), 23);
		for s in schemes {
			assert!(!s.name.contains('_'));
			assert!(s.name.chars().next().unwrap().is_uppercase(), "{}", s.name);
			assert!(!s.stops.is_empty());
		}
		let field = find(schemes, "default FIELD").unwrap();
		assert_eq!(field.stops.len(), 9);
		assert_eq!(field.kind, SchemeKind::Field);
	}

	#[test]
	fn saving_reproduces_every_shipped_file() {
		for text in BUILT_IN_FILES {
			let s = wdscheme::read_str(text, "").unwrap();
			assert_eq!(wdscheme::write_string(&s), *text, "{}", s.name);
		}
	}

	#[test]
	fn resolution_order_and_legacy_names() {
		let store = SchemeStore {
			user: vec![ColorScheme { name: "Inferno".into(), description: "user".into(), ..ColorScheme::grey() }],
		};
		let project =
			vec![ColorScheme { name: "Rainbow".into(), description: "project".into(), ..ColorScheme::grey() }];
		assert_eq!(store.resolve(&project, "rainbow").description, "project");
		assert_eq!(store.resolve(&project, "Inferno").description, "user");
		assert_eq!(store.resolve(&project, "Default_Field").name, "Default field");
		assert_eq!(store.resolve(&project, "Turky_brown").name, "Turkey brown");
		assert_eq!(store.resolve(&project, "Gray_scales_inv").name, "Gray scales inverted");
		assert_eq!(store.resolve(&project, "Nope"), ColorScheme::grey());
	}

	#[test]
	fn author_json() {
		let stops: Vec<Rgb> = (0..9).map(|i| Rgb::new(i * 30, 0, 255 - i * 30)).collect();
		let triplet = |c: &Rgb| format!("[{},{},{}]", c.r, c.g, c.b);
		let table: Vec<Rgb> = (0..255)
			.map(|i| {
				let p = (i * 8) as f64 / 254.0;
				let k = p.floor() as usize;
				if k >= 8 { stops[8] } else { lerp(stops[k], stops[k + 1], p - k as f64) }
			})
			.collect();
		let list = |v: &[Rgb]| v.iter().map(triplet).collect::<Vec<_>>().join(",");
		let json = format!(
			"{{\"name\":\"Mine\",\"origin\":\"o\",\"scheme_type\":\"Intensity\",\"bits_per_channel\":8,\"entry_count\":255,\"stops\":[{}],\"colors\":[{}]}}",
			list(&stops),
			list(&table)
		);
		let s = parse_author_json(&json, "f").unwrap();
		assert_eq!((s.name.as_str(), s.kind, s.description.as_str()), ("Mine", SchemeKind::Intensity, "o"));
		assert_eq!(s.stops.len(), 9);
		assert_eq!(s.stops[4], Stop { at: 0.5, color: stops[4] });
		let colours_only = "{\"colors\":[[0,0,0],[255,128,0],[255,255,255]]}";
		let s = parse_author_json(colours_only, "File").unwrap();
		assert_eq!(s.name, "File");
		assert_eq!(s.stops.iter().map(|s| s.at).collect::<Vec<_>>(), [0.0, 0.5, 1.0]);
		let mismatch = json.replacen("[0,0,255]", "[1,1,1]", 2);
		assert_eq!(parse_author_json(&mismatch, "f").unwrap().stops.len(), 255);
		assert!(parse_author_json("[1]", "f").is_err());
		assert!(parse_author_json("{\"colors\":[[1,2]]}", "f").is_err());
	}
}
