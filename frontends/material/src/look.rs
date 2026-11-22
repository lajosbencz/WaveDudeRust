//! Fonts, colour scheme and density (DESIGN.md, Theming), installed once; the settings the window follows are
//! [`waveduderust_egui_platform::shown::Shown`]'s.

use std::sync::Arc;

use eframe::egui::{self, Color32};
use egui_mineral::runtime::{GlobalStyle, InitError, Runtime};
use egui_mineral_material::color::SchemeSpec;
use egui_mineral_material::fonts;
use egui_mineral_material::prelude::{Density, Material, Rt};

/// Material Symbols subset to the icons the sources use (build.rs).
const ICON_FONTS: fonts::IconFonts = fonts::IconFonts::new(
	include_bytes!(concat!(env!("OUT_DIR"), "/MaterialSymbolsRounded_Filled-Regular.ttf")),
	include_bytes!(concat!(env!("OUT_DIR"), "/MaterialSymbolsRounded-Regular.ttf")),
);

/// The scheme's seed: WaveDude's accent.
const SEED: Color32 = Color32::from_rgb(0x00, 0x7A, 0xCC);
/// Density with a mouse: 32 dp list rows, 40 dp text fields; touch-first devices keep the stylekit's default.
const POINTER_DENSITY: i8 = -4;

/// The bundled fonts (overlay labels and UI text share them), the runtime with the stylekit's icons, policies and
/// scheme.
pub(crate) fn install(ctx: &egui::Context, touch_first: bool) -> Result<Arc<Rt>, InitError> {
	ctx.set_fonts(waveduderust::unstable::view::fonts::definitions());
	let rt = Runtime::new(ctx, Material::from_icon_fonts(ICON_FONTS.fonts()))?;
	// egui's root tooltips and popups and the window decorations follow the runtime's style and theme
	rt.core().set_global_style(GlobalStyle::Kit);
	fonts::install_medium(ctx, "Selawik Semibold", waveduderust::unstable::view::fonts::SEMIBOLD_DATA);
	// egui's zoom and quit keys are app commands
	ctx.options_mut(|o| {
		o.zoom_with_keyboard = false;
		o.quit_shortcuts.clear();
	});
	egui_extras::install_image_loaders(ctx);
	rt.kit().set_density(ctx, Density::new(if touch_first { 0 } else { POINTER_DENSITY }));
	rt.kit().apply(ctx, SchemeSpec::new(SEED));
	Ok(rt)
}

/// The focus ring of a hand-built control, drawn only when the focus came from the keyboard.
pub(crate) fn focus_ring(ui: &egui::Ui, response: &egui::Response, tokens: &egui_mineral_material::theme::Tokens) {
	if egui_mineral::focus::visible(ui.ctx(), response.id) {
		ui.painter().rect_stroke(
			response.rect,
			4.,
			egui::Stroke::new(2., tokens.color.secondary),
			egui::StrokeKind::Inside,
		);
	}
}
