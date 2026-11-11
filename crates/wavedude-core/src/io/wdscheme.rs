//! The `.wdscheme` colour scheme XML; the same `ColorScheme` element sits inside a `.wdproj`.

use std::path::Path;

use super::xml::{self, DECLARATION, Element, XmlEnum};
use super::{FormatError, Result, UnreadableFile};
use crate::model::normalize::Normalize;
use crate::model::{ColorScheme, Rgb, SchemeKind, Stop};

/// File extension of a colour scheme.
pub const EXTENSION: &str = "wdscheme";

impl XmlEnum for SchemeKind {
	const MEMBERS: &'static [(Self, &'static str)] =
		&[(SchemeKind::Unspecified, "Unspecified"), (SchemeKind::Field, "Field"), (SchemeKind::Intensity, "Intensity")];
}

/// Reads a `ColorScheme` element (not normalised).
pub(crate) fn from_element(el: &Element) -> std::result::Result<ColorScheme, FormatError> {
	let mut stops = Vec::new();
	for s in el.children("Stop") {
		let color = s.get("Color").unwrap_or("#000000");
		let color = Rgb::parse_hex(color).ok_or_else(|| FormatError {
			message: format!("element 'Stop': attribute 'Color' has the value '{color}', which is not #RRGGBB"),
			at: Some(s.at),
		})?;
		stops.push(Stop { at: s.f64("At", 0.0)?, color });
	}
	Ok(ColorScheme {
		name: el.string("Name", ""),
		kind: el.enumv("Kind", SchemeKind::Unspecified)?,
		description: el.string("Description", ""),
		stops,
	})
}

/// The `ColorScheme` element of a scheme, sparse.
pub(crate) fn to_element(scheme: &ColorScheme) -> Element {
	let mut el = Element::new("ColorScheme");
	el.attr("Name", scheme.name.as_str()).enum_attr("Kind", scheme.kind, SchemeKind::Unspecified).str_attr(
		"Description",
		&scheme.description,
		"",
	);
	for s in &scheme.stops {
		let mut stop = Element::new("Stop");
		stop.attr("At", xml::fmt_f64(s.at)).attr("Color", s.color.to_hex());
		el.child_el(stop);
	}
	el
}

/// Parses a `.wdscheme` text; an empty name becomes `fallback_name`. Normalised.
pub fn read_str(text: &str, fallback_name: &str) -> std::result::Result<ColorScheme, FormatError> {
	let doc = xml::parse(text)?;
	if doc.root.name != "ColorScheme" {
		return Err(FormatError { message: "the document is not a colour scheme".into(), at: Some(doc.root.at) });
	}
	let mut scheme = from_element(&doc.root)?;
	if scheme.name.is_empty() {
		scheme.name = fallback_name.to_owned();
	}
	scheme.normalize();
	Ok(scheme)
}

/// The `.wdscheme` text of a scheme, as `WaveDude` Next writes it (no trailing newline).
pub fn write_string(scheme: &ColorScheme) -> String {
	let mut out = String::from(DECLARATION);
	out.push('\n');
	to_element(scheme).write(&mut out, 0);
	out
}

fn stem(path: &Path) -> String {
	path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Loads a `.wdscheme` file; an unnamed scheme takes the file's name.
pub fn load(path: &Path) -> Result<ColorScheme> {
	let text = super::read_text(path)?;
	read_str(&text, &stem(path)).map_err(|e| UnreadableFile::new(path, &text, e).into())
}

pub fn save(scheme: &ColorScheme, path: &Path) -> Result<()> {
	super::write_file(path, write_string(scheme).as_bytes())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trip_keeps_everything() {
		let text = include_str!("../../schemes/Default field.wdscheme");
		let scheme = read_str(text, "x").unwrap();
		assert_eq!(scheme.name, "Default field");
		assert_eq!(scheme.kind, SchemeKind::Field);
		assert_eq!(scheme.stops.len(), 9);
		assert_eq!(scheme.stops[1].color, Rgb::new(0x30, 0x20, 0xF4));
		assert_eq!(write_string(&scheme), text);
	}

	#[test]
	fn unnamed_takes_fallback_and_bad_colour_fails() {
		let s = read_str("<ColorScheme><Stop At=\"0.5\" Color=\"#010203\" /></ColorScheme>", "File").unwrap();
		assert_eq!(s.name, "File");
		assert!(read_str("<ColorScheme><Stop At=\"0\" Color=\"red\" /></ColorScheme>", "f").is_err());
		assert!(read_str("<Project />", "f").is_err());
	}
}
