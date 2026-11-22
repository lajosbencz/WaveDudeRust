//! The Messages pane (docs/ui/70, Messages pane): the log ring's lines with level toggles and counts, a search
//! (plain or regex), auto-scroll, Clear, resizable columns, multi-select, copy and virtualised rows. Its lines are
//! copies of the ring's, taken on every pass whether it shows or not.

use eframe::egui::{
	self, Align, Button, Color32, Id, Key, Modifiers, Pos2, Response, Role, Sense, Stroke, StrokeKind, TextEdit,
	TextStyle, TextWrapMode, Ui, Vec2, WidgetInfo, WidgetText, vec2,
};
use egui_mineral::table::{ColumnSize, ColumnWidths, RowStyle};
use egui_mineral::tone::Tone;
use waveduderust_app::log::LogRing;
use waveduderust_app::menus::MenuAction;
use waveduderust_app::messages::{LogLevel, Messages};
use waveduderust_app::strings::{Strings, strip_mnemonic};

use super::shortcuts;
use crate::kit::Palette;
use crate::kit::menu::{self, Entry};
use crate::kit::table::{Cell, Press, RowLook, Table};

/// Widths of Time, Level and Source; Message takes the rest.
const DEFAULT_COLUMNS: [f32; 3] = [90., 55., 130.];
const MIN_COLUMN: f32 = 30.;
const COLUMNS: [&str; 4] =
	["MessagesColumnTime", "MessagesColumnLevel", "MessagesColumnLogger", "MessagesColumnMessage"];
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
	model: Messages,
	columns: ColumnWidths,
	scroll_to: Option<ScrollTo>,
	/// The context menu's place while open.
	menu: Option<Pos2>,
	/// The list takes the focus at the next pass.
	focus: bool,
	list_id: Option<Id>,
}

impl Default for MessagesPane {
	fn default() -> Self {
		Self {
			model: Messages::default(),
			columns: ColumnWidths::new(DEFAULT_COLUMNS.map(|width| ColumnSize { width, min: MIN_COLUMN })),
			scroll_to: None,
			menu: None,
			focus: false,
			list_id: None,
		}
	}
}

impl MessagesPane {
	/// Copies the ring's new lines.
	pub(super) fn poll(&mut self, ring: &LogRing) {
		self.model.poll(ring);
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
			MenuAction::Copy => ctx.copy_text(self.model.copy_selected()),
			MenuAction::SelectAll => self.model.select_all(),
			_ => return false,
		}
		true
	}

	/// The shown lines follow the log and the filter; new lines with auto-scroll scroll to the end.
	fn refresh(&mut self) {
		if self.model.refresh() {
			self.scroll_to = Some(ScrollTo::End);
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
		let list_id = ui.scope_id().with("MessageList");
		self.list_id = Some(list_id);
		let menu_keys = ui.memory(|m| m.has_focus(list_id)) && {
			let arrows = egui::EventFilter { vertical_arrows: true, ..Default::default() };
			ui.memory_mut(|m| m.set_focus_lock_filter(list_id, arrows));
			self.keys(ui)
		};
		let list = self.rows(ui, s, list_id, palette);
		if list.clicked() || std::mem::take(&mut self.focus) {
			list.request_focus();
		}
		if list.secondary_clicked() {
			list.request_focus();
			self.menu = list.interact_pointer_pos();
		}
		if menu_keys {
			self.menu = Some(list.rect.min + MENU_OFFSET);
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
				let text = format!("{} ({})", level.name(), self.model.log.count(level));
				let on = self.model.filter.is_shown(level);
				if chip(ui, &text, level_color(level, palette), on, palette)
					.on_hover_text(s.get("TipMessagesLevel"))
					.clicked()
				{
					self.model.filter.toggle(level);
				}
			}
			ui.add_space(SEARCH_GAP);
			let search = TextEdit::singleline(&mut self.model.filter.search)
				.hint_text(s.get("MessagesSearchHint"))
				.desired_width(SEARCH_WIDTH);
			ui.add(search);
			let regex = chip(ui, s.get("MessagesRegex"), palette.text, self.model.filter.regex, palette);
			if regex.on_hover_text(s.get("TipMessagesRegex")).clicked() {
				self.model.filter.regex ^= true;
			}
			let follow = chip(ui, s.get("MessagesAutoScroll"), palette.text, self.model.auto_scroll, palette);
			if follow.on_hover_text(s.get("TipMessagesAutoScroll")).clicked() {
				self.model.auto_scroll ^= true;
				if self.model.auto_scroll {
					self.scroll_to = Some(ScrollTo::End);
				}
			}
			let clear = ui.add(Button::new(strip_mnemonic(s.get("MessagesClear"))));
			if clear.on_hover_text(s.get("TipMessagesClear")).clicked() {
				self.model.clear(ring);
			}
		});
		ui.add_space(TOOLBAR_PADDING);
	}

	/// Up and Down step through the lines, Shift extends; whether Shift+F10 or the menu key asks for the menu.
	fn keys(&mut self, ui: &Ui) -> bool {
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
			if let Some(id) = self.model.selection.step(&self.model.ids, forward, extend) {
				self.scroll_to = Some(ScrollTo::Line(id));
				self.model.auto_scroll = false;
			}
		}
		menu
	}

	/// The column titles and the lines on screen; a click selects, a right click opens the menu. The list's response.
	fn rows(&mut self, ui: &mut Ui, s: &Strings, list_id: Id, palette: &Palette) -> Response {
		let titles = COLUMNS.map(|c| s.get(c));
		let mut table =
			Table::new(list_id, s.get("PaneMessages"), &titles, &mut self.columns).stick_to_end(self.model.auto_scroll);
		let (ids, log, selection) = (&self.model.ids, &self.model.log, &self.model.selection);
		match self.scroll_to.take() {
			Some(ScrollTo::End) if !ids.is_empty() => table = table.scroll_to(ids.len() - 1, Some(Align::Max)),
			// a stepped-to line already in view stays where it is
			Some(ScrollTo::Line(id)) => {
				if let Ok(i) = ids.binary_search(&id) {
					table = table.scroll_to(i, None);
				}
			}
			_ => {}
		}
		let out = table.show(ui, ids.len(), |i| {
			let Some(line) = ids.get(i).and_then(|&id| log.get(id)) else {
				return RowLook {
					cells: Vec::new(),
					style: RowStyle::default(),
					selected: false,
					tip: None,
					label: "",
				};
			};
			let text = |text| Cell { text, tip: None };
			RowLook {
				cells: vec![
					text(line.time.as_str()),
					text(line.level.name()),
					Cell { text: line.source_short(), tip: Some(&line.source) },
					text(&line.text),
				],
				style: RowStyle::tone(Tone::Custom {
					container: Color32::TRANSPARENT,
					content: level_color(line.level, palette),
				}),
				selected: selection.ids.contains(&line.id),
				tip: line.detail.as_deref(),
				label: &line.text,
			}
		});
		let Some((i, press)) = out.pressed else { return out.response };
		let Some(&id) = self.model.ids.get(i) else { return out.response };
		ui.memory_mut(|m| m.request_focus(list_id));
		match press {
			Press::Primary(m) => self.model.selection.click(id, m.command, m.shift, &self.model.ids),
			Press::Secondary(at) => {
				if !self.model.selection.ids.contains(&id) {
					self.model.selection.click(id, false, false, &self.model.ids);
				}
				self.menu = Some(at);
			}
		}
		out.response
	}

	/// Copy, Copy all and Clear, while open.
	fn context_menu(&mut self, ui: &Ui, s: &Strings, list: &Response, ring: &LogRing) {
		let Some(at) = self.menu else { return };
		let ctx = ui.ctx().clone();
		let mut open = true;
		let id = ui.scope_id().with("MessagesMenu");
		let has_selection = self.model.has_selection();
		let shown = menu::popup(list, id, at, &mut open, |m| {
			let keys = shortcuts::text(&ctx, MenuAction::Copy);
			let copy = m.item(Entry::new(s.get("MessagesCopy")).shortcut(keys).enabled(has_selection)).picked;
			let copy_all = m.item(Entry::new(s.get("MessagesCopyAll"))).picked;
			m.separator();
			let clear = m.item(Entry::new(s.get("MessagesClear"))).picked;
			(copy, copy_all, clear)
		});
		if !open {
			self.menu = None;
		}
		match shown {
			Some((true, _, _)) => ctx.copy_text(self.model.copy_selected()),
			Some((_, true, _)) => ctx.copy_text(self.model.copy_all()),
			Some((_, _, true)) => self.model.clear(ring),
			_ => {}
		}
	}
}

fn level_color(level: LogLevel, palette: &Palette) -> Color32 {
	match level {
		LogLevel::Trace => palette.level_trace,
		LogLevel::Debug => palette.level_debug,
		LogLevel::Info => palette.level_info,
		LogLevel::Warn => palette.level_warn,
		LogLevel::Error | LogLevel::Fatal => palette.level_error,
	}
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
