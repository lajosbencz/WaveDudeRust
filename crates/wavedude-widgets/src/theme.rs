//! Colour tokens and metrics of the dnSpy-like dark and light themes, provided through Freya's context.

use freya::prelude::{Color, State, WritableUtils, consume_context, dark_theme, light_theme, use_hook};

/// Which palette a [`Theme`] carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeKind {
	Dark,
	Light,
}

/// Colour tokens and metrics shared by every widget. Sizes in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
	pub kind: ThemeKind,
	pub window_background: Color,
	pub tool_window_background: Color,
	pub control_background: Color,
	pub input_background: Color,
	pub input_border: Color,
	pub border: Color,
	pub hairline: Color,
	pub text: Color,
	pub text_dim: Color,
	pub text_disabled: Color,
	pub accent: Color,
	pub accent_text: Color,
	pub hover_background: Color,
	pub pressed_background: Color,
	pub selection: Color,
	pub selection_text: Color,
	pub inactive_selection: Color,
	pub inactive_selection_text: Color,
	pub title_active: Color,
	pub title_active_text: Color,
	pub title_inactive: Color,
	pub title_inactive_text: Color,
	pub tab_background: Color,
	pub tab_active: Color,
	pub splitter: Color,
	pub drop_indicator: Color,
	pub menu_background: Color,
	pub menu_border: Color,
	pub menu_hover: Color,
	pub status_background: Color,
	pub status_text: Color,
	pub status_warn: Color,
	pub status_error: Color,
	pub tooltip_background: Color,
	pub tooltip_border: Color,
	pub tooltip_text: Color,
	pub clamp_flash: Color,
	pub level_trace: Color,
	pub level_debug: Color,
	pub level_info: Color,
	pub level_warn: Color,
	pub level_error: Color,
	pub font_size: f32,
	pub small_font_size: f32,
	pub row_height: f32,
	pub icon_size: f32,
}

impl Theme {
	pub fn dark() -> Self {
		Self {
			kind: ThemeKind::Dark,
			window_background: Color::from_rgb(0x2D, 0x2D, 0x30),
			tool_window_background: Color::from_rgb(0x25, 0x25, 0x26),
			control_background: Color::from_rgb(0x3F, 0x3F, 0x46),
			input_background: Color::from_rgb(0x33, 0x33, 0x37),
			input_border: Color::from_rgb(0x43, 0x43, 0x46),
			border: Color::from_rgb(0x3F, 0x3F, 0x46),
			hairline: Color::from_rgb(0x55, 0x55, 0x5A),
			text: Color::from_rgb(0xF1, 0xF1, 0xF1),
			text_dim: Color::from_rgb(0x99, 0x99, 0x99),
			text_disabled: Color::from_rgb(0x65, 0x65, 0x65),
			accent: Color::from_rgb(0x00, 0x7A, 0xCC),
			accent_text: Color::WHITE,
			hover_background: Color::from_rgb(0x3E, 0x3E, 0x40),
			pressed_background: Color::from_rgb(0x00, 0x7A, 0xCC),
			selection: Color::from_rgb(0x33, 0x99, 0xFF),
			selection_text: Color::WHITE,
			inactive_selection: Color::from_rgb(0x3F, 0x3F, 0x46),
			inactive_selection_text: Color::from_rgb(0xF1, 0xF1, 0xF1),
			title_active: Color::from_rgb(0x00, 0x7A, 0xCC),
			title_active_text: Color::WHITE,
			title_inactive: Color::from_rgb(0x2D, 0x2D, 0x30),
			title_inactive_text: Color::from_rgb(0xD0, 0xD0, 0xD0),
			tab_background: Color::from_rgb(0x2D, 0x2D, 0x30),
			tab_active: Color::from_rgb(0x3E, 0x3E, 0x40),
			splitter: Color::from_rgb(0x2D, 0x2D, 0x30),
			drop_indicator: Color::from_rgb(0x00, 0x97, 0xFB),
			menu_background: Color::from_rgb(0x1B, 0x1B, 0x1C),
			menu_border: Color::from_rgb(0x33, 0x33, 0x37),
			menu_hover: Color::from_rgb(0x33, 0x33, 0x34),
			status_background: Color::from_rgb(0x00, 0x7A, 0xCC),
			status_text: Color::WHITE,
			status_warn: Color::from_rgb(0xCA, 0x51, 0x00),
			status_error: Color::from_rgb(0xB4, 0x21, 0x1D),
			tooltip_background: Color::from_rgb(0x42, 0x42, 0x45),
			tooltip_border: Color::from_rgb(0x4D, 0x4D, 0x50),
			tooltip_text: Color::from_rgb(0xF1, 0xF1, 0xF1),
			clamp_flash: Color::from_rgb(0xE0, 0x6C, 0x2B),
			level_trace: Color::from_rgb(0x80, 0x80, 0x80),
			level_debug: Color::from_rgb(0xA0, 0xA0, 0xA0),
			level_info: Color::from_rgb(0xF1, 0xF1, 0xF1),
			level_warn: Color::from_rgb(0xFF, 0xB0, 0x40),
			level_error: Color::from_rgb(0xFF, 0x60, 0x60),
			font_size: 12.,
			small_font_size: 11.,
			row_height: 23.,
			icon_size: 16.,
		}
	}

	pub fn light() -> Self {
		Self {
			kind: ThemeKind::Light,
			window_background: Color::from_rgb(0xEE, 0xEE, 0xF2),
			tool_window_background: Color::from_rgb(0xF5, 0xF5, 0xF5),
			control_background: Color::from_rgb(0xE6, 0xE7, 0xE8),
			input_background: Color::WHITE,
			input_border: Color::from_rgb(0xCC, 0xCE, 0xDB),
			border: Color::from_rgb(0xCC, 0xCE, 0xDB),
			hairline: Color::from_rgb(0xA0, 0xA0, 0xA8),
			text: Color::from_rgb(0x1E, 0x1E, 0x1E),
			text_dim: Color::from_rgb(0x71, 0x71, 0x71),
			text_disabled: Color::from_rgb(0xA2, 0xA4, 0xA5),
			accent: Color::from_rgb(0x00, 0x7A, 0xCC),
			accent_text: Color::WHITE,
			hover_background: Color::from_rgb(0xC9, 0xDE, 0xF5),
			pressed_background: Color::from_rgb(0x00, 0x7A, 0xCC),
			selection: Color::from_rgb(0x33, 0x99, 0xFF),
			selection_text: Color::WHITE,
			inactive_selection: Color::from_rgb(0xCC, 0xCE, 0xDB),
			inactive_selection_text: Color::from_rgb(0x1E, 0x1E, 0x1E),
			title_active: Color::from_rgb(0x00, 0x7A, 0xCC),
			title_active_text: Color::WHITE,
			title_inactive: Color::from_rgb(0xEE, 0xEE, 0xF2),
			title_inactive_text: Color::from_rgb(0x44, 0x44, 0x44),
			tab_background: Color::from_rgb(0xEE, 0xEE, 0xF2),
			tab_active: Color::WHITE,
			splitter: Color::from_rgb(0xEE, 0xEE, 0xF2),
			drop_indicator: Color::from_rgb(0x00, 0x7A, 0xCC),
			menu_background: Color::from_rgb(0xF6, 0xF6, 0xF6),
			menu_border: Color::from_rgb(0xCC, 0xCE, 0xDB),
			menu_hover: Color::from_rgb(0xC9, 0xDE, 0xF5),
			status_background: Color::from_rgb(0x00, 0x7A, 0xCC),
			status_text: Color::WHITE,
			status_warn: Color::from_rgb(0xCA, 0x51, 0x00),
			status_error: Color::from_rgb(0xB4, 0x21, 0x1D),
			tooltip_background: Color::from_rgb(0xFC, 0xFC, 0xFC),
			tooltip_border: Color::from_rgb(0xCC, 0xCE, 0xDB),
			tooltip_text: Color::from_rgb(0x1E, 0x1E, 0x1E),
			clamp_flash: Color::from_rgb(0xE0, 0x6C, 0x2B),
			level_trace: Color::from_rgb(0x90, 0x90, 0x90),
			level_debug: Color::from_rgb(0x70, 0x70, 0x70),
			level_info: Color::from_rgb(0x1E, 0x1E, 0x1E),
			level_warn: Color::from_rgb(0xA0, 0x50, 0x00),
			level_error: Color::from_rgb(0xC0, 0x10, 0x10),
			font_size: 12.,
			small_font_size: 11.,
			row_height: 23.,
			icon_size: 16.,
		}
	}

	pub fn of(kind: ThemeKind) -> Self {
		match kind {
			ThemeKind::Dark => Self::dark(),
			ThemeKind::Light => Self::light(),
		}
	}

	/// Freya's component theme recoloured with these tokens, so its Input, Button and ScrollView match.
	pub fn freya_theme(&self) -> freya::prelude::Theme {
		let mut theme = match self.kind {
			ThemeKind::Dark => dark_theme(),
			ThemeKind::Light => light_theme(),
		};
		let c = &mut theme.colors;
		c.primary = self.accent;
		c.secondary = self.selection;
		c.tertiary = self.accent;
		c.background = self.tool_window_background;
		c.surface_primary = self.control_background;
		c.surface_secondary = self.hover_background;
		c.surface_tertiary = self.input_background;
		c.border = self.input_border;
		c.border_focus = self.accent;
		c.text_primary = self.text;
		c.text_secondary = self.text_dim;
		c.text_placeholder = self.text_disabled;
		c.text_highlight = self.selection;
		c.focus = self.hover_background;
		theme
	}
}

impl Default for Theme {
	fn default() -> Self {
		Self::dark()
	}
}

/// Provides the theme for the subtree (and Freya's recoloured one); returns the state to switch it live.
pub fn use_init_wave_theme(init: impl FnOnce() -> Theme) -> State<Theme> {
	let theme = use_hook(|| {
		let state = State::create(init());
		freya::prelude::provide_context(state);
		state
	});
	let mut freya_theme = freya::prelude::use_init_theme(|| theme.peek().freya_theme());
	let kind = theme.read().kind;
	if freya_kind(&freya_theme) != kind {
		freya_theme.set(theme.peek().freya_theme());
	}
	theme
}

fn freya_kind(theme: &State<freya::prelude::Theme>) -> ThemeKind {
	if theme.peek().name == "light" { ThemeKind::Light } else { ThemeKind::Dark }
}

/// The current theme, subscribing the caller to changes; [`use_init_wave_theme`] provides it above every view.
pub fn use_wave_theme() -> Theme {
	consume_context::<State<Theme>>().read().clone()
}
