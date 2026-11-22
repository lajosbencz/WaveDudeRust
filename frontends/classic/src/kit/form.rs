//! Next's property form (docs/ui/30, Pane behaviour; `FormPanel`) on egui-mineral's `LabelColumn`: section
//! headings, a label column per section as wide as its widest label, full-width rows, read-outs and the description
//! header. Rows are as high as their editors; a label names its editor for assistive technology.

use eframe::egui::{
	self, Align, FontId, Id, Layout, Rect, Response, RichText, Role, Sense, Ui, UiBuilder, Vec2, WidgetInfo, pos2, vec2,
};
use egui_mineral::layout::LabelColumn;
use waveduderust_egui_preview::drawn as icons;

use super::tokens::{FONT_SIZE, FORM, Palette};

/// A row's label column and tooltip.
#[derive(Clone, Copy, Default)]
pub(crate) struct Row<'a> {
	/// Spans both columns when none.
	pub label: Option<&'a str>,
	pub hint: &'a str,
	/// Greys the label and disables the editor unless set.
	pub enabled: bool,
	/// The editor the label names.
	pub names: Option<Id>,
	/// The label against the top of a tall editor.
	pub top: bool,
}

impl<'a> Row<'a> {
	pub(crate) fn new(label: Option<&'a str>, hint: &'a str) -> Self {
		Self { label, hint, enabled: true, ..Self::default() }
	}
}

/// How a text row reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Look {
	/// A read-out (`FormReadout`): the text colour, wrapping.
	Readout,
	/// A secondary value (`FormValue`): `GrayText` (Next's Themes/Light.xaml:28), inset.
	Value,
}

/// A section's rows (`FormPanel`): labels in a column as wide as the widest, at least [`FORM`]'s `label_min`, editors
/// filling the rest. A change of the column's width discards the pass.
#[must_use = "end the grid"]
pub(crate) struct Grid<'u> {
	ui: &'u mut Ui,
	id: Id,
	column: LabelColumn,
	rows: usize,
	/// Room the rows leave free on their right.
	trailing: f32,
}

impl<'u> Grid<'u> {
	/// The rows under `id`, below the cursor of `ui`.
	pub(crate) fn begin(ui: &'u mut Ui, id: Id) -> Self {
		let column = LabelColumn::begin(ui.ctx(), id);
		Self { ui, id, column, rows: 0, trailing: 0. }
	}

	/// Stores the column; the group's gap follows.
	pub(crate) fn end(self) {
		self.column.end(self.ui.ctx());
		self.ui.add_space(FORM.group_after);
	}

	pub(crate) fn ui(&mut self) -> &mut Ui {
		self.ui
	}

	fn label_width(&self) -> f32 {
		self.column.width().max(FORM.label_min)
	}

	/// An editor row: `editor` beside the label, or across the row without one.
	pub(crate) fn row<R>(&mut self, row: Row<'_>, editor: impl FnOnce(&mut Ui) -> R) -> R {
		self.place(row, FORM.row, editor)
	}

	/// A row of `text`.
	pub(crate) fn text(&mut self, row: Row<'_>, text: &str, look: Look) {
		self.place(row, 0., |ui| {
			let palette = Palette::of(ui.ctx().theme());
			let ink = match look {
				_ if !row.enabled => palette.text_disabled,
				Look::Readout => palette.text,
				Look::Value => palette.text_disabled,
			};
			let inset = if look == Look::Value { FORM.value_inset } else { 0. };
			ui.add_space(inset);
			ui.add(egui::Label::new(RichText::new(text).color(ink)).wrap());
		});
	}

	/// The rows that follow leave `width` more free on their right; a negative width gives it back.
	pub(crate) fn trail(&mut self, width: f32) {
		self.trailing += width;
	}

	fn place<R>(&mut self, row: Row<'_>, least: f32, editor: impl FnOnce(&mut Ui) -> R) -> R {
		if self.rows > 0 {
			self.ui.add_space(FORM.row_gap);
		}
		self.rows += 1;
		let natural = row.label.map(|text| text_size(self.ui, text));
		if let Some(size) = natural {
			self.column.measure(size.x);
		}
		let width = self.label_width();
		let column = if row.label.is_some() { width + FORM.column_gap } else { 0. };
		let top = self.ui.cursor().top();
		let (left, room) = (self.ui.max_rect().left(), self.ui.available_width() - self.trailing);
		let cell = Rect::from_min_size(pos2(left + column, top), vec2(room - column, least));
		// a child the grid does not grow with: an editor wider than its room is clipped by the pane
		let mut ui = self.ui.new_child(UiBuilder::new().max_rect(cell).layout(Layout::left_to_right(Align::Center)));
		if !row.enabled {
			ui.disable();
		}
		let inner = editor(&mut ui);
		let height = ui.min_rect().height().max(least).max(natural.map_or(0., |s| s.y));
		let rect = Rect::from_min_size(pos2(left, top), vec2(room, height));
		self.ui.advance_cursor_after_rect(rect);
		match row.label {
			Some(text) => {
				let y = if row.top { top + FORM.label_top } else { rect.center().y - natural.map_or(0., |s| s.y) / 2. };
				let at = Rect::from_min_max(pos2(left, y), pos2(left + width, y + natural.map_or(0., |s| s.y)));
				label(self.ui, at, text, row);
			}
			None if !row.hint.is_empty() => {
				self.ui.interact(rect, self.id.with(("Hint", self.rows)), Sense::hover()).on_hover_text(row.hint);
			}
			None => {}
		}
		inner
	}
}

/// The size of `text` on one line.
fn text_size(ui: &Ui, text: &str) -> Vec2 {
	let font = FontId::proportional(FONT_SIZE);
	ui.painter().layout_no_wrap(text.to_owned(), font, Palette::of(ui.ctx().theme()).text).size()
}

/// `row`'s label at `rect`, trimmed to it, naming the editor it labels.
fn label(ui: &mut Ui, rect: Rect, text: &str, row: Row<'_>) {
	let palette = Palette::of(ui.ctx().theme());
	let ink = if row.enabled { palette.text } else { palette.text_disabled };
	let shown = egui::Label::new(RichText::new(text).color(ink)).truncate().selectable(false);
	let mut cell = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
	let response = cell.add(shown);
	if let Some(editor) = row.names {
		labelled_by(ui, editor, &response);
	}
	if !row.hint.is_empty() {
		response.on_hover_text(row.hint);
	}
}

/// Names `editor` by `name` for assistive technology.
fn labelled_by(ui: &Ui, editor: Id, name: &Response) {
	ui.ctx().accesskit_node_builder(editor, |node| {
		node.clear_label();
		node.push_labelled_by(name.id.accesskit_id());
	});
}

/// A section heading (`FormSection`): its name over a rule across the width, `after` below the rule.
pub(crate) fn heading(ui: &mut Ui, title: &str, after: f32) {
	let palette = Palette::of(ui.ctx().theme());
	ui.add_space(FORM.heading_top);
	let font = FontId::new(FONT_SIZE, waveduderust::unstable::view::fonts::bold());
	let galley = ui.painter().layout_no_wrap(title.to_owned(), font, palette.text);
	let size = vec2(ui.available_width(), galley.size().y + FORM.heading_pad + 1.);
	let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
	ui.painter().galley(rect.min, galley, palette.text);
	ui.painter().rect_filled(Rect::from_min_max(pos2(rect.left(), rect.bottom() - 1.), rect.max), 0., palette.border);
	ui.add_space(after);
}

/// The parts' description header (`DescriptionToggle`): a chevron and the title over a rule, the accent on the rule
/// while focused; its response.
pub(crate) fn disclosure(ui: &mut Ui, id: Id, title: &str, tip: &str, open: bool) -> Response {
	let palette = Palette::of(ui.ctx().theme());
	ui.add_space(FORM.heading_top);
	let font = FontId::new(FONT_SIZE, waveduderust::unstable::view::fonts::bold());
	let galley = ui.painter().layout_no_wrap(title.to_owned(), font, palette.text);
	let size = vec2(ui.available_width(), galley.size().y + FORM.heading_pad + 1.);
	let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
	let response = ui.interact(rect, id, Sense::click()).on_hover_text(tip);
	response.widget_info(|| WidgetInfo::selected(Role::Button, true, open, title));
	if response.hovered() {
		ui.painter().rect_filled(rect, 0., palette.button_hover);
	}
	let rule = if response.has_focus() { palette.accent } else { palette.border };
	ui.painter().rect_filled(Rect::from_min_max(pos2(rect.left(), rect.bottom() - 1.), rect.max), 0., rule);
	let chevron = Rect::from_center_size(pos2(rect.left() + FORM.chevron, rect.center().y - 1.), Vec2::splat(16.));
	let glyph = if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT };
	icons::paint(ui.painter(), chevron, glyph, palette.text);
	ui.painter().galley(pos2(rect.left() + FORM.disclosure_text, rect.top()), galley, palette.text);
	ui.add_space(FORM.heading_after);
	response
}

/// The description's text under its header (`FormDescription`), wrapping, the group's gap after it.
pub(crate) fn description(ui: &mut Ui, text: &str) {
	ui.add(egui::Label::new(text).wrap());
	ui.add_space(FORM.group_after);
}

/// A secondary value across the width (`FormValue` without its inset): a folder's count.
pub(crate) fn value(ui: &mut Ui, text: &str) {
	let ink = Palette::of(ui.ctx().theme()).text_disabled;
	ui.add(egui::Label::new(RichText::new(text).color(ink)).wrap());
}

/// `label` in a column `width` wide, `control` right of it; the label names `names`.
pub(crate) fn labelled<R>(
	ui: &mut Ui,
	label: &str,
	width: f32,
	names: Option<Id>,
	control: impl FnOnce(&mut Ui) -> R,
) -> R {
	ui.horizontal(|ui| {
		let (cell, _) = ui.allocate_exact_size(vec2(width, super::INPUT_HEIGHT), Sense::hover());
		let inner = control(ui);
		let size = text_size(ui, label);
		let at = Rect::from_min_size(pos2(cell.left(), cell.center().y - size.y / 2.), vec2(width, size.y));
		self::label(ui, at, label, Row { names, ..Row::new(Some(label), "") });
		inner
	})
	.inner
}
