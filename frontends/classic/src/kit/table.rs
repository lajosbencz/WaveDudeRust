//! A list in columns (docs/ui/90, 22): titled headers whose right-end grips resize egui_mineral's [`ColumnWidths`],
//! the last column taking the rest; the rows on its [`VirtualList`], each in its [`RowStyle`]'s tone or the
//! selection's colours, the list itself a focusable widget.

use eframe::egui::{
	self, Align, Color32, CursorIcon, Id, Modifiers, Pos2, Rect, Response, Role, Sense, Stroke, StrokeKind, TextStyle,
	Ui, UiBuilder, WidgetInfo, pos2, vec2,
};
use egui_mineral::input::drag_from_start;
use egui_mineral::perf::VirtualList;
use egui_mineral::table::{ColumnWidths, RowStyle};
use egui_mineral::tone::Tone;

use super::{Palette, ROW_HEIGHT};

/// A column header's resize grip, at its right end.
const GRIP: f32 = 4.;
/// The space after a cell, and before the first.
const CELL_GAP: f32 = 6.;
const ROW_INSET: f32 = 4.;

/// One cell: its text, cut with an ellipsis, and its own tooltip.
pub(crate) struct Cell<'a> {
	pub text: &'a str,
	pub tip: Option<&'a str>,
}

/// How a row shows.
pub(crate) struct RowLook<'a> {
	/// One per column.
	pub cells: Vec<Cell<'a>>,
	pub style: RowStyle,
	pub selected: bool,
	/// The row's tooltip off a cell's own.
	pub tip: Option<&'a str>,
	/// The accessible name.
	pub label: &'a str,
}

/// A press on a row.
pub(crate) enum Press {
	/// A click, with its modifiers.
	Primary(Modifiers),
	/// A right click, where it was.
	Secondary(Pos2),
}

/// What [`Table::show`] got.
pub(crate) struct TableOutput {
	/// The rows' room: the focus, keys and its own presses.
	pub response: Response,
	/// A row pressed, by index.
	pub pressed: Option<(usize, Press)>,
}

pub(crate) struct Table<'a> {
	id: Id,
	label: &'a str,
	titles: &'a [&'a str],
	widths: &'a mut ColumnWidths,
	scroll_to: Option<(usize, Option<Align>)>,
	stick_to_end: bool,
}

impl<'a> Table<'a> {
	/// `titles` has one more than `widths`: the last column takes the rest.
	pub(crate) fn new(id: Id, label: &'a str, titles: &'a [&'a str], widths: &'a mut ColumnWidths) -> Self {
		Self { id, label, titles, widths, scroll_to: None, stick_to_end: false }
	}

	/// Shows `row` at this pass, at `align` or just in view.
	pub(crate) fn scroll_to(mut self, row: usize, align: Option<Align>) -> Self {
		self.scroll_to = Some((row, align));
		self
	}

	/// Keeps the view at the last row while scrolled there.
	pub(crate) fn stick_to_end(mut self, stick_to_end: bool) -> Self {
		self.stick_to_end = stick_to_end;
		self
	}

	/// The headers and `rows` rows in `ui`'s room, `look` giving the shown ones' looks.
	pub(crate) fn show<'r>(
		mut self,
		ui: &mut Ui,
		rows: usize,
		mut look: impl FnMut(usize) -> RowLook<'r>,
	) -> TableOutput {
		let palette = Palette::of(ui.ctx().theme());
		self.header(ui, palette);
		let room = ui.available_rect_before_wrap();
		let response = ui.interact(room, self.id, Sense::click());
		response.widget_info(|| WidgetInfo::labeled(Role::List, true, self.label));
		let widths: Vec<f32> = (0..self.widths.len()).map(|i| self.widths.width(i)).collect();
		let mut list_ui = ui.new_child(UiBuilder::new().max_rect(room).id_salt(self.id));
		list_ui.spacing_mut().item_spacing.y = 0.;
		let mut list = VirtualList::uniform("rows", ROW_HEIGHT).stick_to_end(self.stick_to_end);
		if let Some((row, align)) = self.scroll_to {
			list = list.scroll_to(row);
			if let Some(align) = align {
				list = list.scroll_align(align);
			}
		}
		let mut pressed = None;
		list.show(&mut list_ui, rows, |ui, i| {
			let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
			if let Some(press) = row(ui, rect, self.id.with(i), &look(i), &widths, palette) {
				pressed = Some((i, press));
			}
		});
		if response.has_focus() {
			ui.painter().rect_stroke(room, 0., Stroke::new(1., palette.accent), StrokeKind::Inside);
		}
		TableOutput { response, pressed }
	}

	/// The titles; a grip at each one's right end resizes it.
	fn header(&mut self, ui: &mut Ui, palette: &Palette) {
		let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
		ui.painter().rect_filled(rect, 0., palette.control_background);
		let mut x = rect.left() + ROW_INSET;
		for (i, title) in self.titles.iter().enumerate() {
			if i >= self.widths.len() {
				cell(ui, title, x, rect.right() - x, rect, palette.text_dim);
				break;
			}
			let width = self.widths.width(i);
			cell(ui, title, x, width - GRIP, rect, palette.text_dim);
			let grip = Rect::from_x_y_ranges(x + width - GRIP..=x + width, rect.y_range());
			ui.painter().rect_filled(grip, 0., palette.window_background);
			let drag = ui.interact(grip, self.id.with(("column", i)), Sense::drag());
			if let Some((from, travel)) = drag_from_start(&drag, width) {
				self.widths.set(i, (from + travel.x).round());
			}
			drag.on_hover_cursor(CursorIcon::ResizeHorizontal);
			x += self.widths.width(i);
		}
	}
}

/// One row at `rect`; its press.
fn row(ui: &Ui, rect: Rect, id: Id, look: &RowLook<'_>, widths: &[f32], palette: &Palette) -> Option<Press> {
	let response = ui.interact(rect, id, Sense::click());
	response.widget_info(|| WidgetInfo::selected(Role::ListItem, true, look.selected, look.label));
	let (fill, ink) = if look.selected {
		(palette.selection, palette.selection_text)
	} else {
		match look.style.tone {
			Tone::Error => (Color32::TRANSPARENT, palette.level_error),
			Tone::Custom { container, content } => (container, content),
			_ => (Color32::TRANSPARENT, palette.text),
		}
	};
	ui.painter().rect_filled(rect, 0., fill);
	let mut x = rect.left() + ROW_INSET;
	let mut tip = look.tip;
	for (i, c) in look.cells.iter().enumerate() {
		let width = widths.get(i).copied().unwrap_or(rect.right() - x + CELL_GAP);
		cell(ui, c.text, x, width - CELL_GAP, rect, ink);
		if c.tip.is_some() && response.hover_pos().is_some_and(|p| (x..=x + width).contains(&p.x)) {
			tip = c.tip;
		}
		x += width;
	}
	let response = match tip {
		Some(tip) => response.on_hover_text(tip),
		None => response,
	};
	if response.clicked() {
		Some(Press::Primary(ui.input(|i| i.modifiers)))
	} else if response.secondary_clicked() {
		response.interact_pointer_pos().map(Press::Secondary)
	} else {
		None
	}
}

/// `text` from `x` within `width`, cut with an ellipsis, centred on `row`'s height.
fn cell(ui: &Ui, text: &str, x: f32, width: f32, row: Rect, color: Color32) {
	if width <= 0. {
		return;
	}
	let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), TextStyle::Body.resolve(ui.style()), color);
	job.wrap = egui::text::TextWrapping::truncate_at_width(width);
	let galley = ui.painter().layout_job(job);
	let at = pos2(x, row.center().y - galley.size().y / 2.);
	ui.painter().with_clip_rect(row.intersect(ui.clip_rect())).galley(at, galley, color);
}
