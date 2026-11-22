//! The tokens of the dnSpy-like dark and light looks of docs/ui/00 (Look): colours, metrics, and egui's visuals
//! made from them.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Vec2, Visuals, vec2};

/// The UI font's size, logical pixels.
pub(crate) const FONT_SIZE: f32 = 12.;
/// The small text style's size, logical pixels.
pub(super) const SMALL_FONT_SIZE: f32 = 11.;
/// The height of a row of controls, logical pixels.
pub(crate) const ROW_HEIGHT: f32 = 23.;
/// The height of a text or number box, logical pixels.
pub(crate) const INPUT_HEIGHT: f32 = 22.;
/// The height of a pane's title and tab bar, logical pixels.
pub(crate) const TITLE_HEIGHT: f32 = 21.;
/// The width of borders and outlines, logical pixels.
pub(crate) const BORDER: f32 = 1.;

/// The metrics of Next's menu bar and menus, logical pixels; a row's x offsets are from its left edge, which is
/// `padding` inside the panel's border.
pub(crate) struct MenuMetrics {
	pub bar_height: f32,
	/// The headers' room from the bar's left edge.
	pub bar_inset: f32,
	pub header_height: f32,
	/// Either side of a header's text.
	pub header_padding: f32,
	/// Between a panel's border and its rows, all around.
	pub padding: f32,
	pub row: f32,
	/// A separator's height and its line's offset from its top.
	pub separator: f32,
	pub separator_line: f32,
	/// The separator line's start from a row's left edge and its end from a row's right edge.
	pub separator_start: f32,
	pub separator_end: f32,
	pub icon: f32,
	pub icon_center: f32,
	/// The text's start.
	pub text: f32,
	/// After a swatch before the text.
	pub swatch_gap: f32,
	/// Between the widest text and the shortcut column.
	pub shortcut_gap: f32,
	/// After the shortcut column, the submenu arrow's room.
	pub trail: f32,
	/// The submenu arrow's width and height, and its right edge from a row's right edge.
	pub arrow: Vec2,
	pub arrow_end: f32,
	pub radio: f32,
	/// The check mark's polyline around the icon column's centre, and its stroke.
	pub check: [Vec2; 3],
	pub check_width: f32,
}

pub(crate) const MENU: MenuMetrics = MenuMetrics {
	bar_height: 23.,
	bar_inset: 2.,
	header_height: 21.,
	header_padding: 7.,
	padding: 2.,
	row: 22.,
	separator: 14.,
	separator_line: 7.,
	separator_start: 28.,
	separator_end: 4.,
	icon: 16.,
	icon_center: 14.,
	text: 30.,
	swatch_gap: 6.,
	shortcut_gap: 24.,
	trail: 22.,
	arrow: vec2(4., 8.),
	arrow_end: 8.,
	radio: 3.,
	check: [vec2(-4., 0.), vec2(-1., 3.), vec2(4., -3.)],
	check_width: 1.5,
};

/// The metrics of Next's project tree, logical pixels. A row's header (its selection band) starts `expander` plus
/// `indent` per level from the tree's left edge; x offsets of the row's parts are from the header's start.
pub(crate) struct TreeMetrics {
	pub row: f32,
	pub expander: f32,
	pub indent: f32,
	/// The expander's chevron, pointing down, around its centre `chevron_center` before the header, and its stroke.
	pub chevron: [Vec2; 3],
	pub chevron_center: f32,
	pub chevron_width: f32,
	/// Before the first drawing.
	pub lead: f32,
	/// The check box's side, the gap after it, and its mark around the box's centre.
	pub check: f32,
	pub check_gap: f32,
	pub check_mark: [Vec2; 3],
	pub check_width: f32,
	pub icon: f32,
	/// After the icon or swatch, before the label.
	pub icon_gap: f32,
	/// The label's lift above the row's centre line.
	pub label_rise: f32,
	/// A scheme's gradient strip.
	pub strip: Vec2,
	/// A trailing button's side, the pitch of their centres, and the last centre's offset from the right edge.
	pub button: f32,
	pub button_pitch: f32,
	pub button_end: f32,
	/// The up glyph's chevron around the button's centre (down mirrors it), and its stroke.
	pub glyph_chevron: [Vec2; 3],
	pub glyph_width: f32,
	/// A plus or minus bar's arm, thickness and opacity: Next's 1 px bars straddle two pixels.
	pub glyph_arm: f32,
	pub glyph_bar: f32,
	pub glyph_bar_opacity: f32,
	/// The selection band's gap to the right edge.
	pub band_end: f32,
	/// The drop mark's thickness and its gap to the right edge; it starts at `lead`.
	pub drop: f32,
	pub drop_end: f32,
}

pub(crate) const TREE: TreeMetrics = TreeMetrics {
	row: ROW_HEIGHT,
	expander: 18.,
	indent: 16.,
	chevron: [vec2(-4.5, -2.), vec2(0., 2.5), vec2(4.5, -2.)],
	chevron_center: 8.,
	chevron_width: 1.1,
	lead: 3.,
	check: 14.,
	check_gap: 10.,
	check_mark: [vec2(-3.5, 0.5), vec2(-0.5, 2.5), vec2(3., -2.)],
	check_width: 1.5,
	icon: 16.,
	icon_gap: 4.,
	label_rise: 2.,
	strip: vec2(36., 10.),
	button: 18.,
	button_pitch: 20.,
	button_end: 14.,
	glyph_chevron: [vec2(-3.5, 1.75), vec2(0., -1.75), vec2(3.5, 1.75)],
	glyph_width: 1.25,
	glyph_arm: 4.,
	glyph_bar: 2.,
	glyph_bar_opacity: 0.6,
	band_end: 2.,
	drop: 2.,
	drop_end: 5.,
};

/// The metrics of Next's number box, logical pixels, laid out flat: the spin column at the right edge, inside the
/// border.
pub(crate) struct NumberMetrics {
	pub spin: f32,
	/// The text's room inside the border, each side: Next under wine draws it 9 px from the outer edge (PIL).
	pub text_inset: f32,
	/// The up chevron around the pixel at a button's centre (down mirrors it), and its stroke.
	pub chevron: [Vec2; 3],
	pub chevron_width: f32,
}

pub(crate) const NUMBER: NumberMetrics = NumberMetrics {
	spin: 14.,
	text_inset: 8.,
	chevron: [vec2(-2., 1.), vec2(0., -1.), vec2(2., 1.)],
	chevron_width: 1.,
};

/// The metrics of Next's property form (`FormPanel`, Themes/Editors.xaml `FormSection` and `DescriptionToggle`),
/// logical pixels.
pub(crate) struct FormMetrics {
	/// The pane's margin, all around.
	pub padding: f32,
	/// The label column's least width, and its gap to the editors.
	pub label_min: f32,
	pub column_gap: f32,
	pub row_gap: f32,
	/// An editor row's least height: a box's (Editors.xaml MinHeight 22).
	pub row: f32,
	/// A top-aligned label's drop below its row's top.
	pub label_top: f32,
	/// A heading's margin above, its rule's gap below the text, and the gap after the rule.
	pub heading_top: f32,
	pub heading_pad: f32,
	pub heading_after: f32,
	/// A folder's heading's gap after the rule.
	pub folder_after: f32,
	/// After a section's rows and after the description's text.
	pub group_after: f32,
	/// A secondary value's inset either side.
	pub value_inset: f32,
	/// The description header's chevron centre and its text's start, from the header's left edge.
	pub chevron: f32,
	pub disclosure_text: f32,
}

pub(crate) const FORM: FormMetrics = FormMetrics {
	padding: 8.,
	label_min: 110.,
	column_gap: 8.,
	row_gap: 4.,
	row: INPUT_HEIGHT,
	label_top: 4.,
	heading_top: 2.,
	heading_pad: 2.,
	heading_after: 6.,
	folder_after: 4.,
	group_after: 10.,
	value_inset: 4.,
	chevron: 4.,
	disclosure_text: 14.,
};

/// Colour tokens of one look.
pub(crate) struct Palette {
	pub window_background: Color32,
	pub text: Color32,
	pub border: Color32,
	pub accent: Color32,
	pub selection: Color32,
	pub menu_background: Color32,
	pub menu_border: Color32,
	pub menu_hover: Color32,
	/// A menu row's shortcut text.
	pub menu_shortcut: Color32,
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
	/// A tree row under the pointer.
	pub row_hover: Color32,
	/// A tree row's check box.
	pub check_border: Color32,
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
	/// The status bar's background, while its line is a warning, an error (also a message window's error disc),
	/// and its text.
	pub status: Color32,
	pub status_warn: Color32,
	pub status_error: Color32,
	pub status_text: Color32,
}

impl Palette {
	pub(crate) const DARK: Self = Self {
		window_background: Color32::from_rgb(0x2D, 0x2D, 0x30),
		text: Color32::from_rgb(0xF1, 0xF1, 0xF1),
		border: Color32::from_rgb(0x3F, 0x3F, 0x46),
		accent: Color32::from_rgb(0x00, 0x7A, 0xCC),
		selection: Color32::from_rgb(0x33, 0x99, 0xFF),
		menu_background: Color32::from_rgb(0x1B, 0x1B, 0x1C),
		menu_border: Color32::from_rgb(0x33, 0x33, 0x37),
		menu_hover: Color32::from_rgb(0x33, 0x33, 0x34),
		menu_shortcut: Color32::from_rgb(0x99, 0x99, 0x99),
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
		row_hover: Color32::from_rgb(0x3E, 0x3E, 0x40),
		check_border: Color32::from_rgb(0x6D, 0x6D, 0x70),
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
		status: Color32::from_rgb(0x00, 0x7A, 0xCC),
		status_warn: Color32::from_rgb(0xCA, 0x51, 0x00),
		status_error: Color32::from_rgb(0xB4, 0x21, 0x1D),
		status_text: Color32::WHITE,
	};

	pub(crate) const LIGHT: Self = Self {
		window_background: Color32::from_rgb(0xEE, 0xEE, 0xF2),
		text: Color32::from_rgb(0x1E, 0x1E, 0x1E),
		border: Color32::from_rgb(0xCC, 0xCE, 0xDB),
		accent: Color32::from_rgb(0x00, 0x7A, 0xCC),
		selection: Color32::from_rgb(0x33, 0x99, 0xFF),
		menu_background: Color32::from_rgb(0xF6, 0xF6, 0xF6),
		menu_border: Color32::from_rgb(0xCC, 0xCE, 0xDB),
		menu_hover: Color32::from_rgb(0xC9, 0xDE, 0xF5),
		menu_shortcut: Color32::from_rgb(0xA2, 0xA4, 0xA5),
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
		row_hover: Color32::from_rgb(0xE5, 0xE5, 0xE5),
		check_border: Color32::from_rgb(0x9A, 0x9A, 0x9E),
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
		status: Color32::from_rgb(0x00, 0x7A, 0xCC),
		status_warn: Color32::from_rgb(0xCA, 0x51, 0x00),
		status_error: Color32::from_rgb(0xB4, 0x21, 0x1D),
		status_text: Color32::WHITE,
	};

	/// The palette of `look`.
	pub(crate) fn of(look: egui::Theme) -> &'static Self {
		match look {
			egui::Theme::Dark => &Self::DARK,
			egui::Theme::Light => &Self::LIGHT,
		}
	}

	pub(super) fn visuals(&self, dark: bool) -> Visuals {
		let mut v = if dark { Visuals::dark() } else { Visuals::light() };
		v.panel_fill = self.window_background;
		v.window_fill = self.menu_background;
		v.window_stroke = Stroke::new(BORDER, self.menu_border);
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
