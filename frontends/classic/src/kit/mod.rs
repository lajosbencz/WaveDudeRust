//! The WaveDude Next look as a stylekit on egui-mineral's core: its tokens, fonts and the egui style made from
//! them. The kit owns egui's styles, so [`Classic::style`] hands out the context's own.

pub(crate) mod dialog;
mod fonts;
pub(crate) mod form;
pub(crate) mod menu;
pub(crate) mod number;
pub(crate) mod split;
pub(crate) mod status;
pub(crate) mod table;
mod tokens;
pub(crate) mod tree;

use std::sync::Arc;

use eframe::egui::{self, Context, Style, Theme};
use egui_mineral::i18n::Catalog;
use egui_mineral::policy::Policies;
use egui_mineral::runtime::{InitError, Runtime, Stylekit};
use egui_mineral::widget::MineralUi;

pub(crate) use self::tokens::{BORDER, FONT_SIZE, FORM, INPUT_HEIGHT, Palette, ROW_HEIGHT, TITLE_HEIGHT, TREE};

/// The classic window's stylekit.
pub(crate) struct Classic;

/// The classic window's runtime.
pub(crate) type Rt = Runtime<Classic>;

/// The `Ui` the classic window's views get from [`Rt::pass`].
pub(crate) type ClassicUi<'a> = MineralUi<'a, Classic>;

impl Classic {
	/// The runtime on `ctx`, with the kit's fonts and both looks installed.
	pub(crate) fn runtime(ctx: &Context) -> Result<Arc<Rt>, InitError> {
		Runtime::new(ctx, Self)
	}
}

impl Stylekit for Classic {
	fn policies(&self, ctx: &Context) -> Policies {
		let mut policies = Policies::baseline();
		// Option+letter types a character on macOS
		policies.mnemonics = ctx.os() != egui::os::OperatingSystem::Mac;
		// egui's constant line scroll speed, without the core's notch acceleration
		policies.wheel = None;
		policies
	}

	fn catalogs(&self) -> &'static [Catalog] {
		&[]
	}

	fn setup(&self, ctx: &Context) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		install(ctx);
		Ok(())
	}

	fn style(&self, ctx: &Context, theme: Theme) -> Arc<Style> {
		ctx.style_of(theme)
	}

	/// Next's controls take hits on what they draw: no least target.
	fn min_target(&self, _: &ClassicUi<'_>, _: f32) -> f32 {
		0.
	}
}

/// Installs the fonts, text sizes and both looks on egui's styles.
pub(crate) fn install(ctx: &Context) {
	fonts::install(ctx);
	ctx.set_visuals_of(Theme::Dark, Palette::DARK.visuals(true));
	ctx.set_visuals_of(Theme::Light, Palette::LIGHT.visuals(false));
	ctx.options_mut(|o| {
		o.zoom_with_keyboard = false;
		// Next's look while the OS states no scheme
		o.fallback_theme = Theme::Light;
	});
}
