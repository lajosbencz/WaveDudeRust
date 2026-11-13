//! The dnSpy-like dark and light looks of docs/ui/00 (Look): the colour tokens, egui's visuals and metrics made
//! from them, and the bundled UI font.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Visuals};
use wavedude_app_state::settings::Theme as ThemeSetting;

/// The UI font's size, logical pixels.
pub const FONT_SIZE: f32 = 12.;
/// The height of a row of controls, logical pixels.
pub const ROW_HEIGHT: f32 = 23.;
/// The height of a text or number box, logical pixels.
pub const INPUT_HEIGHT: f32 = 22.;
const SMALL_FONT_SIZE: f32 = 11.;

/// Colour tokens of one look.
pub struct Palette {
	pub window_background: Color32,
	pub text: Color32,
	pub border: Color32,
	pub accent: Color32,
	pub selection: Color32,
	pub menu_background: Color32,
	pub menu_border: Color32,
	pub menu_hover: Color32,
	/// A tool button under the pointer.
	pub hover_background: Color32,
	pub control_background: Color32,
	pub input_background: Color32,
	pub text_disabled: Color32,
	pub timeline_tick: Color32,
	/// A pane's body.
	pub tool_window_background: Color32,
	/// A thin outline that shows on the selection colours.
	pub hairline: Color32,
	pub selection_text: Color32,
	/// The selection of a list without the keyboard focus, and its other selected rows.
	pub inactive_selection: Color32,
	pub inactive_selection_text: Color32,
	pub drop_indicator: Color32,
	/// The dots of a pane title bar's drag room.
	pub title_grip: Color32,
	/// Secondary text: headings, ordinals.
	pub text_dim: Color32,
	/// A text or number box under the pointer.
	pub input_hover_border: Color32,
	pub button_background: Color32,
	pub button_border: Color32,
	pub button_hover: Color32,
	pub combo_border: Color32,
	pub combo_hover: Color32,
	/// A number box's border when a step or a commit was cut to its range.
	pub clamp_flash: Color32,
	/// A Messages line's text by its level; Fatal shares Error's.
	pub level_trace: Color32,
	pub level_debug: Color32,
	pub level_info: Color32,
	pub level_warn: Color32,
	pub level_error: Color32,
}

impl Palette {
	pub const DARK: Self = Self {
		window_background: Color32::from_rgb(0x2D, 0x2D, 0x30),
		text: Color32::from_rgb(0xF1, 0xF1, 0xF1),
		border: Color32::from_rgb(0x3F, 0x3F, 0x46),
		accent: Color32::from_rgb(0x00, 0x7A, 0xCC),
		selection: Color32::from_rgb(0x33, 0x99, 0xFF),
		menu_background: Color32::from_rgb(0x1B, 0x1B, 0x1C),
		menu_border: Color32::from_rgb(0x33, 0x33, 0x37),
		menu_hover: Color32::from_rgb(0x33, 0x33, 0x34),
		hover_background: Color32::from_rgb(0x3E, 0x3E, 0x40),
		control_background: Color32::from_rgb(0x3F, 0x3F, 0x46),
		input_background: Color32::from_rgb(0x33, 0x33, 0x37),
		text_disabled: Color32::from_rgb(0x99, 0x99, 0x99),
		timeline_tick: Color32::from_rgb(0x78, 0x78, 0x7F),
		tool_window_background: Color32::from_rgb(0x25, 0x25, 0x26),
		hairline: Color32::from_rgb(0x55, 0x55, 0x5A),
		selection_text: Color32::WHITE,
		inactive_selection: Color32::from_rgb(0x3F, 0x3F, 0x46),
		inactive_selection_text: Color32::from_rgb(0xF1, 0xF1, 0xF1),
		drop_indicator: Color32::from_rgb(0x00, 0x97, 0xFB),
		title_grip: Color32::from_rgb(0x46, 0x46, 0x4A),
		text_dim: Color32::from_rgb(0x99, 0x99, 0x99),
		input_hover_border: Color32::from_rgb(0x5A, 0x5A, 0x5E),
		button_background: Color32::from_rgb(0x3F, 0x3F, 0x46),
		button_border: Color32::from_rgb(0x55, 0x55, 0x58),
		button_hover: Color32::from_rgb(0x4F, 0x4F, 0x56),
		combo_border: Color32::from_rgb(0x43, 0x43, 0x46),
		combo_hover: Color32::from_rgb(0x3F, 0x3F, 0x46),
		clamp_flash: Color32::from_rgb(0xE0, 0x6C, 0x2B),
		level_trace: Color32::from_rgb(0x80, 0x80, 0x80),
		level_debug: Color32::from_rgb(0xA0, 0xA0, 0xA0),
		level_info: Color32::from_rgb(0xF1, 0xF1, 0xF1),
		level_warn: Color32::from_rgb(0xFF, 0xB0, 0x40),
		level_error: Color32::from_rgb(0xFF, 0x60, 0x60),
	};

	pub const LIGHT: Self = Self {
		window_background: Color32::from_rgb(0xEE, 0xEE, 0xF2),
		text: Color32::from_rgb(0x1E, 0x1E, 0x1E),
		border: Color32::from_rgb(0xCC, 0xCE, 0xDB),
		accent: Color32::from_rgb(0x00, 0x7A, 0xCC),
		selection: Color32::from_rgb(0x33, 0x99, 0xFF),
		menu_background: Color32::from_rgb(0xF6, 0xF6, 0xF6),
		menu_border: Color32::from_rgb(0xCC, 0xCE, 0xDB),
		menu_hover: Color32::from_rgb(0xC9, 0xDE, 0xF5),
		hover_background: Color32::from_rgb(0xC9, 0xDE, 0xF5),
		control_background: Color32::from_rgb(0xE6, 0xE7, 0xE8),
		input_background: Color32::WHITE,
		text_disabled: Color32::from_rgb(0xA2, 0xA4, 0xA5),
		timeline_tick: Color32::from_rgb(0xA6, 0xA8, 0xB2),
		tool_window_background: Color32::from_rgb(0xF5, 0xF5, 0xF5),
		hairline: Color32::from_rgb(0xA0, 0xA0, 0xA8),
		selection_text: Color32::WHITE,
		inactive_selection: Color32::from_rgb(0xCC, 0xCE, 0xDB),
		inactive_selection_text: Color32::from_rgb(0x1E, 0x1E, 0x1E),
		drop_indicator: Color32::from_rgb(0x00, 0x7A, 0xCC),
		title_grip: Color32::from_rgb(0x99, 0x99, 0x99),
		text_dim: Color32::from_rgb(0x71, 0x71, 0x71),
		input_hover_border: Color32::from_rgb(0xA0, 0xA0, 0xA8),
		button_background: Color32::from_rgb(0xEC, 0xEC, 0xF0),
		button_border: Color32::from_rgb(0xAC, 0xAC, 0xAC),
		button_hover: Color32::from_rgb(0xC9, 0xDE, 0xF5),
		combo_border: Color32::from_rgb(0xCC, 0xCE, 0xDB),
		combo_hover: Color32::from_rgb(0xE6, 0xE7, 0xED),
		clamp_flash: Color32::from_rgb(0xE0, 0x6C, 0x2B),
		level_trace: Color32::from_rgb(0x90, 0x90, 0x90),
		level_debug: Color32::from_rgb(0x70, 0x70, 0x70),
		level_info: Color32::from_rgb(0x1E, 0x1E, 0x1E),
		level_warn: Color32::from_rgb(0xA0, 0x50, 0x00),
		level_error: Color32::from_rgb(0xC0, 0x10, 0x10),
	};

	/// The palette of `look`.
	pub fn of(look: egui::Theme) -> &'static Self {
		match look {
			egui::Theme::Dark => &Self::DARK,
			egui::Theme::Light => &Self::LIGHT,
		}
	}

	fn visuals(&self, dark: bool) -> Visuals {
		let mut v = if dark { Visuals::dark() } else { Visuals::light() };
		v.panel_fill = self.window_background;
		v.window_fill = self.menu_background;
		v.window_stroke = Stroke::new(1., self.menu_border);
		v.window_corner_radius = CornerRadius::ZERO;
		v.menu_corner_radius = CornerRadius::ZERO;
		v.selection.bg_fill = self.selection;
		v.hyperlink_color = self.accent;
		let w = &mut v.widgets;
		for state in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
			state.corner_radius = CornerRadius::ZERO;
			state.fg_stroke.color = self.text;
		}
		w.noninteractive.bg_stroke.color = self.border;
		for state in [&mut w.hovered, &mut w.open] {
			state.weak_bg_fill = self.menu_hover;
			state.bg_fill = self.menu_hover;
			state.bg_stroke = Stroke::NONE;
		}
		w.active.weak_bg_fill = self.accent;
		w.active.bg_fill = self.accent;
		v
	}
}

/// The look a setting means; `os` is the system's preference, if it states one (light otherwise).
pub fn resolve(setting: ThemeSetting, os: Option<egui::Theme>) -> egui::Theme {
	match setting {
		ThemeSetting::Light => egui::Theme::Light,
		ThemeSetting::Dark => egui::Theme::Dark,
		ThemeSetting::System => os.unwrap_or(egui::Theme::Light),
	}
}

/// Installs the bundled fonts, the text sizes and both looks; [`egui::Context::set_theme`] picks one.
pub fn install(ctx: &egui::Context) {
	ctx.set_fonts(wavedude_view::fonts::definitions());
	ctx.set_visuals_of(egui::Theme::Dark, Palette::DARK.visuals(true));
	ctx.set_visuals_of(egui::Theme::Light, Palette::LIGHT.visuals(false));
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
	ctx.options_mut(|o| o.zoom_with_keyboard = false);
}
