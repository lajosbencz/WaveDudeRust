//! The Messages pane (docs/ui/70, Messages pane): the log ring's lines with level toggles and counts, a search
//! (plain or regex), auto-scroll, Clear, resizable columns, multi-select, copy and virtualised rows. Its lines are
//! copies of the ring's, taken on every pass whether it shows or not.

mod log;

use eframe::egui::{
	self, Align, Button, Color32, Id, Key, Modifiers, Popup, Pos2, Rect, Response, Role, ScrollArea, Sense, Stroke,
	StrokeKind, TextEdit, TextStyle, TextWrapMode, Ui, UiBuilder, Vec2, WidgetInfo, WidgetText, pos2, vec2,
};
use wavedude_app_state::log::LogRing;
use wavedude_app_state::strings::{Strings, strip_mnemonic};

use self::log::{LogLevel, LogLine, MessageFilter, MessageLog, MessageSelection};
use super::menu_bar::{self, Slot};
use super::theme::{Palette, ROW_HEIGHT};
use super::{menu_keys, shortcuts};
use crate::menus::MenuAction;

/// Widths of Time, Level and Source; Message takes the rest.
const DEFAULT_COLUMNS: [f32; 3] = [90., 55., 130.];
const MIN_COLUMN: f32 = 30.;
/// A column header's resize grip, at its right end.
const GRIP: f32 = 4.;
/// The space between a row's cells, and before its first.
const CELL_GAP: f32 = 6.;
const ROW_INSET: f32 = 4.;
const TOOLBAR_PADDING: f32 = 2.;
const SEARCH_WIDTH: f32 = 180.;
const SEARCH_GAP: f32 = 6.;
/// A toggle chip: its padding and least height.
const CHIP_PADDING: Vec2 = Vec2::new(7., 2.);
const CHIP_HEIGHT: f32 = 20.;
/// Where Shift+F10 opens the menu, from the list's corner.
const MENU_OFFSET: Vec2 = Vec2::new(20., 20.);

/// What the list scrolls to at the next pass.
#[derive(Clone, Copy)]
enum ScrollTo {
	End,
	Line(u64),
}

pub(super) struct MessagesPane {
	log: MessageLog,
	/// The ring's newest line copied.
	seq: Option<u64>,
	filter: MessageFilter,
	selection: MessageSelection,
	auto_scroll: bool,
	columns: [f32; 3],
	/// The lines shown, oldest first, and the log's revision and the filter they were made for.
	ids: Vec<u64>,
	made_for: Option<(u64, MessageFilter)>,
	scroll_to: Option<ScrollTo>,
	/// The context menu's place while open.
	menu: Option<Pos2>,
	/// The list takes the focus at the next pass.
	focus: bool,
	/// A column's resize: its index and its width at the press.
	resizing: Option<(usize, f32)>,
	list_id: Option<Id>,
}

impl Default for MessagesPane {
	fn default() -> Self {
		Self {
			log: MessageLog::default(),
			seq: None,
			filter: MessageFilter::new(cfg!(debug_assertions)),
			selection: MessageSelection::default(),
			auto_scroll: true,
			columns: DEFAULT_COLUMNS,
			ids: Vec::new(),
			made_for: None,
			scroll_to: None,
			menu: None,
			focus: false,
			resizing: None,
			list_id: None,
		}
	}
}

impl MessagesPane {
	/// Copies the ring's new lines.
	pub(super) fn poll(&mut self, ring: &LogRing) {
		let lines = ring.since(self.seq);
		let Some(last) = lines.last() else { return };
		self.seq = Some(last.seq);
		self.log.extend(lines.iter().map(LogLine::of));
	}

	/// The list takes the focus once shown.
	pub(super) fn focus(&mut self) {
		self.focus = true;
	}

	/// Copy and Select all while the list has the focus; whether it took `action`.
	pub(super) fn act(&mut self, ctx: &egui::Context, action: MenuAction) -> bool {
		if !self.list_id.is_some_and(|id| ctx.memory(|m| m.has_focus(id))) {
			return false;
		}
		match action {
			MenuAction::Copy => ctx.copy_text(self.copy_selected()),
			MenuAction::SelectAll => self.selection.select_all(&self.ids),
			_ => return false,
		}
		true
	}

	fn copy_selected(&self) -> String {
		self.log.copy_text(self.ids.iter().filter(|id| self.selection.ids.contains(id)))
	}

	fn clear(&mut self, ring: &LogRing) {
		self.log.clear();
		ring.clear();
	}

	/// The shown lines follow the log and the filter; new lines drop the selection's lost ones and, with
	/// auto-scroll, scroll to the end.
	fn refresh(&mut self) {
		let revision = self.log.revision;
		match &self.made_for {
			Some((r, f)) if *r == revision && *f == self.filter => return,
			_ => {}
		}
		let arrived = self.made_for.as_ref().is_none_or(|(r, _)| *r != revision);
		self.log.visible_ids(&self.filter, &mut self.ids);
		self.made_for = Some((revision, self.filter.clone()));
		if arrived {
			let ids = &self.ids;
			self.selection.ids.retain(|id| ids.binary_search(id).is_ok());
			if self.auto_scroll {
				self.scroll_to = Some(ScrollTo::End);
			}
		}
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, s: &Strings, ring: &LogRing) {
		let palette = Palette::of(ui.ctx().theme());
		let pane = ui.max_rect();
		ui.painter().rect_filled(pane, 0., palette.tool_window_background);
		self.refresh();
		ui.scope(|ui| {
			ui.set_clip_rect(pane.intersect(ui.clip_rect()));
			self.toolbar(ui, s, ring, palette);
		});
		let rule = ui.cursor().top();
		ui.painter().hline(pane.x_range(), rule - 0.5, Stroke::new(1., palette.border));
		self.header(ui, s, palette, pane);
		let room = Rect::from_min_max(pos2(pane.left(), ui.cursor().top()), pane.max);
		let list_id = ui.scope_id().with("MessageList");
		self.list_id = Some(list_id);
		let list = ui.interact(room, list_id, Sense::click());
		list.widget_info(|| WidgetInfo::labeled(Role::List, true, s.get("PaneMessages")));
		if list.clicked() || std::mem::take(&mut self.focus) {
			list.request_focus();
		}
		if list.secondary_clicked() {
			list.request_focus();
			self.menu = list.interact_pointer_pos();
		}
		if list.has_focus() {
			let arrows = egui::EventFilter { vertical_arrows: true, ..Default::default() };
			ui.memory_mut(|m| m.set_focus_lock_filter(list_id, arrows));
			self.keys(ui, room);
		}
		self.rows(ui, room, list_id, palette);
		if list.has_focus() {
			ui.painter().rect_stroke(room, 0., Stroke::new(1., palette.accent), StrokeKind::Inside);
		}
		self.context_menu(ui, s, &list, ring);
	}

	/// The level toggles with their counts, the search, Regex, Auto-scroll and Clear.
	fn toolbar(&mut self, ui: &mut Ui, s: &Strings, ring: &LogRing, palette: &Palette) {
		ui.add_space(TOOLBAR_PADDING);
		ui.horizontal(|ui| {
			ui.add_space(TOOLBAR_PADDING);
			ui.spacing_mut().item_spacing.x = 2.;
			for level in LogLevel::ALL {
				let text = format!("{} ({})", level.name(), self.log.count(level));
				let on = self.filter.is_shown(level);
				if chip(ui, &text, level.color(palette), on, palette).on_hover_text(s.get("TipMessagesLevel")).clicked()
				{
					self.filter.toggle(level);
				}
			}
			ui.add_space(SEARCH_GAP);
			let search = TextEdit::singleline(&mut self.filter.search)
				.hint_text(s.get("MessagesSearchHint"))
				.desired_width(SEARCH_WIDTH);
			ui.add(search);
			let regex = chip(ui, s.get("MessagesRegex"), palette.text, self.filter.regex, palette);
			if regex.on_hover_text(s.get("TipMessagesRegex")).clicked() {
				self.filter.regex ^= true;
			}
			let follow = chip(ui, s.get("MessagesAutoScroll"), palette.text, self.auto_scroll, palette);
			if follow.on_hover_text(s.get("TipMessagesAutoScroll")).clicked() {
				self.auto_scroll ^= true;
				if self.auto_scroll {
					self.scroll_to = Some(ScrollTo::End);
				}
			}
			let clear = ui.add(Button::new(strip_mnemonic(s.get("MessagesClear"))));
			if clear.on_hover_text(s.get("TipMessagesClear")).clicked() {
				self.clear(ring);
			}
		});
		ui.add_space(TOOLBAR_PADDING);
	}

	/// The column titles; a grip at each one's right end resizes it.
	fn header(&mut self, ui: &mut Ui, s: &Strings, palette: &Palette, pane: Rect) {
		let (rect, _) = ui.allocate_exact_size(vec2(pane.width(), ROW_HEIGHT), Sense::hover());
		ui.painter().rect_filled(rect, 0., palette.control_background);
		let names = ["MessagesColumnTime", "MessagesColumnLevel", "MessagesColumnLogger", "MessagesColumnMessage"];
		let mut x = rect.left() + ROW_INSET;
		for (i, name) in names.into_iter().enumerate() {
			let Some(&width) = self.columns.get(i) else {
				cell(ui, s.get(name), x, rect.right() - x, rect, palette.text_dim);
				break;
			};
			cell(ui, s.get(name), x, width - GRIP, rect, palette.text_dim);
			let grip = Rect::from_x_y_ranges(x + width - GRIP..=x + width, rect.y_range());
			ui.painter().rect_filled(grip, 0., palette.window_background);
			let drag = ui.interact(grip, ui.scope_id().with(("MessagesColumn", i)), Sense::drag());
			let drag = drag.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
			if drag.drag_started() {
				self.resizing = Some((i, width));
			}
			if let Some((at, from)) = self.resizing.filter(|(at, _)| *at == i && drag.dragged())
				&& let (Some(origin), Some(pointer)) =
					ui.input(|input| (input.pointer.press_origin(), input.pointer.interact_pos()))
			{
				self.columns[at] = (from + pointer.x - origin.x).max(MIN_COLUMN).round();
			}
			if drag.drag_stopped() {
				self.resizing = None;
			}
			x += self.columns[i];
		}
	}

	/// Up and Down step through the lines, Shift extends; Shift+F10 and the menu key open the menu.
	fn keys(&mut self, ui: &Ui, room: Rect) {
		let mut steps = Vec::new();
		let mut menu = false;
		ui.input_mut(|i| {
			i.events.retain(|e| {
				let egui::Event::Key { key, pressed: true, modifiers, .. } = e else { return true };
				let (none, shift) = (modifiers.is_none(), modifiers.matches_exact(Modifiers::SHIFT));
				match key {
					Key::ArrowUp | Key::ArrowDown if none || shift => steps.push((*key == Key::ArrowDown, shift)),
					Key::F10 if shift => menu = true,
					Key::Menu if none => menu = true,
					_ => return true,
				}
				false
			});
		});
		for (forward, extend) in steps {
			if let Some(id) = self.selection.step(&self.ids, forward, extend) {
				self.scroll_to = Some(ScrollTo::Line(id));
				self.auto_scroll = false;
			}
		}
		if menu {
			self.menu = Some(room.min + MENU_OFFSET);
		}
	}

	/// The lines on screen in `room`; a click selects, a right click opens the menu.
	fn rows(&mut self, ui: &mut Ui, room: Rect, list_id: Id, palette: &Palette) {
		let mut list_ui = ui.new_child(UiBuilder::new().max_rect(room));
		// show_rows steps by the row height and the spacing at its call
		list_ui.spacing_mut().item_spacing.y = 0.;
		let scroll_to = self.scroll_to.take();
		let (ids, log, selection, columns) = (&self.ids, &self.log, &self.selection, self.columns);
		let mut pressed = None;
		ScrollArea::vertical().auto_shrink(false).show_rows(&mut list_ui, ROW_HEIGHT, ids.len(), |ui, range| {
			let top = ui.max_rect().top() - range.start as f32 * ROW_HEIGHT;
			let at =
				|i: usize| Rect::from_min_size(pos2(room.left(), top + i as f32 * ROW_HEIGHT), vec2(1., ROW_HEIGHT));
			let shown = match scroll_to {
				Some(ScrollTo::End) => ids.len().checked_sub(1).map(|i| (i, Align::BOTTOM)),
				Some(ScrollTo::Line(id)) => ids.binary_search(&id).ok().map(|i| (i, Align::Center)),
				None => None,
			};
			if let Some((i, align)) = shown {
				// a stepped-to line already in view stays where it is
				let align = matches!(scroll_to, Some(ScrollTo::End)).then_some(align);
				ui.scroll_to_rect_animation(at(i), align, egui::style::ScrollAnimation::none());
			}
			for i in range {
				let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
				let Some(line) = ids.get(i).and_then(|&id| log.get(id)) else { continue };
				let selected = selection.ids.contains(&line.id);
				if let Some(press) = row(ui, rect, line, selected, columns, list_id, palette) {
					pressed = Some((line.id, press));
				}
			}
		});
		let Some((id, press)) = pressed else { return };
		ui.memory_mut(|m| m.request_focus(list_id));
		match press {
			Press::Primary(m) => self.selection.click(id, m.command, m.shift, &self.ids),
			Press::Secondary(at) => {
				if !self.selection.ids.contains(&id) {
					self.selection.click(id, false, false, &self.ids);
				}
				self.menu = Some(at);
			}
		}
	}

	/// Copy, Copy all and Clear, while open.
	fn context_menu(&mut self, ui: &Ui, s: &Strings, list: &Response, ring: &LogRing) {
		let Some(at) = self.menu else { return };
		let ctx = ui.ctx().clone();
		let mut open = true;
		let id = ui.scope_id().with("MessagesMenu");
		let has_selection = self.ids.iter().any(|id| self.selection.ids.contains(id));
		let shown = Popup::menu(list).id(id).at_position(at).open_bool(&mut open).show(|ui| {
			let copy = ui
				.add_enabled_ui(has_selection, |ui| {
					let keys = shortcuts::text(&ctx, MenuAction::Copy);
					menu_bar::entry(ui, Slot::Empty, s.get("MessagesCopy"), None, keys)
				})
				.inner
				.clicked();
			let copy_all = menu_bar::entry(ui, Slot::Empty, s.get("MessagesCopyAll"), None, None).clicked();
			ui.separator();
			let clear = menu_bar::entry(ui, Slot::Empty, s.get("MessagesClear"), None, None).clicked();
			(copy, copy_all, clear)
		});
		menu_keys::menu(&ctx, id, list.id);
		if !open {
			self.menu = None;
		}
		match shown.map(|r| r.inner) {
			Some((true, _, _)) => ctx.copy_text(self.copy_selected()),
			Some((_, true, _)) => ctx.copy_text(self.log.copy_text(self.ids.iter())),
			Some((_, _, true)) => self.clear(ring),
			_ => {}
		}
	}
}

/// A press on a row: a click with its modifiers, or a right click where it was.
enum Press {
	Primary(Modifiers),
	Secondary(Pos2),
}

/// One line at `rect`: time, level, short source and message in the level's colour or the selection's. The source
/// shows its full name and a multi-line message its whole text under the pointer.
fn row(
	ui: &mut Ui,
	rect: Rect,
	line: &LogLine,
	selected: bool,
	columns: [f32; 3],
	list_id: Id,
	palette: &Palette,
) -> Option<Press> {
	let response = ui.interact(rect, list_id.with(line.id), Sense::click());
	response.widget_info(|| WidgetInfo::selected(Role::ListItem, true, selected, &line.text));
	let (fill, ink) = if selected {
		(palette.selection, palette.selection_text)
	} else {
		(Color32::TRANSPARENT, line.level.color(palette))
	};
	ui.painter().rect_filled(rect, 0., fill);
	let mut x = rect.left() + ROW_INSET;
	let cells = [line.time.as_str(), line.level.name(), line.source_short()];
	let mut source = Rect::NOTHING;
	for (i, (text, width)) in cells.into_iter().zip(columns).enumerate() {
		cell(ui, text, x, width - CELL_GAP, rect, ink);
		if i == 2 {
			source = Rect::from_x_y_ranges(x..=x + width, rect.y_range());
		}
		x += width;
	}
	cell(ui, &line.text, x, rect.right() - x, rect, ink);
	let on_source = response.hover_pos().is_some_and(|p| source.contains(p));
	let tip = if on_source { Some(&line.source) } else { line.detail.as_ref() };
	let response = match tip {
		Some(tip) => response.on_hover_text(tip.as_str()),
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

/// A flat toggle: its text in `color` while on, greyed while off; filled while on or under the pointer, outlined
/// while on or focused. Space and Enter toggle it.
fn chip(ui: &mut Ui, text: &str, color: Color32, on: bool, palette: &Palette) -> Response {
	let galley = WidgetText::from(text).into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, TextStyle::Button);
	let size = galley.size() + 2. * CHIP_PADDING;
	let (rect, response) = ui.allocate_exact_size(vec2(size.x, size.y.max(CHIP_HEIGHT)), Sense::click());
	response.widget_info(|| WidgetInfo::selected(Role::Button, true, on, text));
	let fill = if response.hovered() {
		palette.button_hover
	} else if on {
		palette.button_background
	} else {
		Color32::TRANSPARENT
	};
	let border = if response.has_focus() {
		palette.accent
	} else if on {
		palette.button_border
	} else {
		Color32::TRANSPARENT
	};
	let painter = ui.painter();
	painter.rect(rect, 0., fill, Stroke::new(1., border), StrokeKind::Inside);
	let ink = if on { color } else { palette.text_disabled };
	painter.galley(rect.center() - galley.size() / 2., galley, ink);
	response
}
