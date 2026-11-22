//! The bundled UI font at the kit's text sizes, and the icon font.

use eframe::egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use eframe::egui::{self, FontData, FontFamily, FontId, TextStyle};
use egui_material_icons::{FONT_FAMILY, FONT_FAMILY_OUTLINED};

use super::tokens::{FONT_SIZE, SMALL_FONT_SIZE};

/// Sets the bundled fonts and the text sizes on every style of `ctx`, then adds the icons.
pub(super) fn install(ctx: &egui::Context) {
	ctx.set_fonts(waveduderust::unstable::view::fonts::definitions());
	ctx.all_styles_mut(|style| {
		style.text_styles = [
			(TextStyle::Small, FontId::proportional(SMALL_FONT_SIZE)),
			(TextStyle::Body, FontId::proportional(FONT_SIZE)),
			(TextStyle::Button, FontId::proportional(FONT_SIZE)),
			(TextStyle::Heading, FontId::proportional(FONT_SIZE)),
			(TextStyle::Monospace, FontId::monospace(FONT_SIZE)),
		]
		.into();
	});
	install_icons(ctx);
}

/// Registers the Material Symbols subset to the icons the sources use (build.rs) under `egui_material_icons`' families.
fn install_icons(ctx: &egui::Context) {
	let mut data =
		FontData::from_static(include_bytes!(concat!(env!("OUT_DIR"), "/MaterialSymbolsRounded-Regular.ttf")));
	// egui_material_icons' baseline tweak
	data.tweak.y_offset_factor = 0.05;
	let family = |family: &str, priority| InsertFontFamily { family: FontFamily::Name(family.into()), priority };
	ctx.add_font(FontInsert::new(
		FONT_FAMILY,
		data,
		vec![
			InsertFontFamily { family: FontFamily::Proportional, priority: FontPriority::Lowest },
			family(FONT_FAMILY, FontPriority::Highest),
			family(FONT_FAMILY_OUTLINED, FontPriority::Highest),
		],
	));
}
