//! The bundled Selawik typefaces (`assets/fonts`, SIL Open Font License 1.1), the metric-compatible stand-in for
//! WaveDude Next's Segoe UI: the window's text and the text drawn over pictures. A subset of Noto Sans Math (same
//! license) follows in every family for the Greek and the math signs Selawik lacks.

use std::sync::Arc;

use epaint::text::{FontData, FontDefinitions, FontFamily};

const REGULAR: &str = "Selawik";
const BOLD: &str = "Selawik Bold";
/// Selawik Regular's font file.
pub(crate) const REGULAR_DATA: &[u8] = include_bytes!("../assets/fonts/Selawik-Regular.ttf");
const BOLD_DATA: &[u8] = include_bytes!("../assets/fonts/Selawik-Bold.ttf");
const FALLBACK: &str = "Noto Sans Math";
/// The fallback's font file: Greek and Coptic, super- and subscripts, letterlike symbols, arrows and math operators.
pub(crate) const FALLBACK_DATA: &[u8] = include_bytes!("../assets/fonts/NotoSansMath-Subset.ttf");

/// The bold family of [`definitions`].
pub fn bold() -> FontFamily {
	FontFamily::Name(BOLD.into())
}

/// Selawik as the proportional and the monospace family, Selawik Bold as [`bold`], each followed by the fallback.
pub fn definitions() -> FontDefinitions {
	let mut fonts = FontDefinitions::empty();
	for (name, data) in [(REGULAR, REGULAR_DATA), (BOLD, BOLD_DATA), (FALLBACK, FALLBACK_DATA)] {
		fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(data)));
	}
	for (family, primary) in [(FontFamily::Proportional, REGULAR), (FontFamily::Monospace, REGULAR), (bold(), BOLD)] {
		fonts.families.insert(family, vec![primary.into(), FALLBACK.into()]);
	}
	fonts
}
