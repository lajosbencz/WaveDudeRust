//! The bundled fonts draw every character of every string table, in every family.

use eframe::egui::FontId;
use eframe::egui::epaint::text::{FontFamily, Fonts, TextOptions};
use wavedude_app_state::strings::{Language, Strings, keys};

#[test]
fn the_fonts_draw_every_character_of_every_table() {
	let mut fonts = Fonts::new(TextOptions::default(), wavedude_view::fonts::definitions());
	let families = [FontFamily::Proportional, FontFamily::Monospace, wavedude_view::fonts::bold()];
	let mut missing = Vec::new();
	for language in Language::ALL {
		let s = Strings::new(language);
		for key in keys(language) {
			for c in s.get(key).chars().filter(|c| !c.is_control()) {
				for family in &families {
					if !fonts.has_glyph(&FontId::new(12., family.clone()), c) {
						missing.push((language, key, c, family.clone()));
					}
				}
			}
		}
	}
	assert!(missing.is_empty(), "characters no font draws: {missing:?}");
}
