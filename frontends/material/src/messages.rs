//! Messages (DESIGN.md, Workspace; docs/ui/70, Messages pane): the log's lines over [`Messages`] with level toggles
//! and counts, a search (plain or regex), Auto-scroll, Clear, resizable columns Time, Level, Source and Message,
//! a selection by click, Ctrl, Shift and arrows, Copy, Copy all and Clear in the context menu. A bottom panel on
//! Expanded and up, a modal side sheet on Medium, a full-screen dialog on Compact; shown for the session.

use eframe::egui::{
	self, Align, Color32, CursorIcon, Id, Key, Modifiers, Rect, Response, Role, ScrollArea, Sense, TextStyle, Ui,
	UiBuilder, WidgetInfo, pos2, vec2,
};
use egui_mineral::range::{RangeModel, Track};
use egui_mineral::semantic::{self, KeySalt, SemanticNode, StableId};
use egui_mineral::widget::{Availability, SizeExt as _};
use egui_mineral_material::prelude::{
	ButtonVariant, Chip, MaterialExt as _, Menu, MenuItem, PushButton, Rt, TextField, TextFieldVariant,
};
use egui_mineral_material::theme::Tokens;
use waveduderust_app::log::LogRing;
use waveduderust_app::menus::MenuAction;
use waveduderust_app::messages::{LogLevel, LogLine, Messages};
use waveduderust_app::strings::{Strings, strip_mnemonic};

use crate::Mui;

/// Widths of Time, Level and Source; Message takes the rest.
const COLUMNS: [f32; 3] = [96., 60., 140.];
const MIN_COLUMN: f32 = 32.;
const MAX_COLUMN: f32 = 1200.;
/// Arrow-key step of a column grip.
const COLUMN_STEP: f32 = 8.;
/// A column title's resize grip, at its right end.
const GRIP: f32 = 6.;
const CELL_GAP: f32 = 8.;
const INSET: f32 = 8.;
const SEARCH_W: f32 = 220.;

/// What the list scrolls to at the next pass.
#[derive(Clone, Copy)]
enum ScrollTo {
	End,
	Line(u64),
}

/// The view's state for the session.
pub(crate) struct MessagesView {
	model: Messages,
	/// Shown: the panel, the sheet or the dialog.
	pub(crate) open: bool,
	columns: [f32; 3],
	scroll_to: Option<ScrollTo>,
	/// The list takes the focus at the next pass.
	focus: bool,
	list: Option<Id>,
}

impl Default for MessagesView {
	fn default() -> Self {
		Self { model: Messages::default(), open: false, columns: COLUMNS, scroll_to: None, focus: false, list: None }
	}
}

impl MessagesView {
	/// Copies the ring's new lines, whether the view shows or not.
	pub(crate) fn poll(&mut self, ring: &LogRing) {
		self.model.poll(ring);
	}

	/// Shows the view, its list focused.
	pub(crate) fn show_focused(&mut self) {
		self.open = true;
		self.focus = true;
	}

	/// Copy and Select all while the list has the focus; whether it took `action`.
	pub(crate) fn act(&mut self, ctx: &egui::Context, action: MenuAction) -> bool {
		if !self.list.is_some_and(|id| ctx.memory(|m| m.has_focus(id))) {
			return false;
		}
		match action {
			MenuAction::Copy => ctx.copy_text(self.model.copy_selected()),
			MenuAction::SelectAll => self.model.select_all(),
			_ => return false,
		}
		true
	}

	/// The toolbar, the column titles and the lines in `ui`'s room.
	pub(crate) fn ui(&mut self, ui: &mut Mui<'_>, s: &Strings, ring: &LogRing) {
		if self.model.refresh() {
			self.scroll_to = Some(ScrollTo::End);
		}
		self.toolbar(ui, s, ring);
		self.header(ui, s);
		// whole rows: a list scrolled to its end starts on a row's top, not under the column titles
		let top = ui.cursor().top();
		let h = row_height(ui);
		let rows = ((ui.max_rect().bottom() - top) / h).floor().max(1.);
		let room = Rect::from_min_max(pos2(ui.max_rect().left(), top), pos2(ui.max_rect().right(), top + rows * h));
		let list_id = ui.scope_id().with("messages-list");
		self.list = Some(list_id);
		let list = ui.interact(room, list_id, Sense::click());
		list.widget_info(|| WidgetInfo::labeled(Role::List, true, s.get("PaneMessages")));
		if list.clicked() || std::mem::take(&mut self.focus) {
			list.request_focus();
		}
		if list.has_focus() {
			let arrows = egui::EventFilter { vertical_arrows: true, ..Default::default() };
			ui.memory_mut(|m| m.set_focus_lock_filter(list_id, arrows));
			self.keys(ui);
		}
		let shown = self.rows(ui, room, list_id);
		crate::look::focus_ring(ui, &list, &ui.tokens());
		// the list fills its room: a panel sized by its content keeps its height
		ui.advance_cursor_after_rect(room);
		self.press(ui, &list, &shown);
		self.context_menu(ui.rt(), s, &list, ring);
	}

	/// The level toggles with their counts, the search, Regex, Auto-scroll and Clear.
	fn toolbar(&mut self, ui: &mut Mui<'_>, s: &Strings, ring: &LogRing) {
		let rt = ui.rt();
		ui.horizontal_wrapped(|ui| {
			let ui = &mut rt.ui(ui);
			for level in LogLevel::ALL {
				let text = format!("{} ({})", level.name(), self.model.log.count(level));
				let mut shown = self.model.filter.is_shown(level);
				let chip = Chip::filter(("messages-level", level.name()), text, &mut shown);
				if ui.mineral(chip).into_response().on_hover_text(s.get("TipMessagesLevel")).changed() {
					self.model.filter.toggle(level);
				}
			}
			let search = TextField::new("messages-search", s.get("MessagesSearchHint"), &mut self.model.filter.search)
				.variant(TextFieldVariant::Outlined)
				.preferred_width(SEARCH_W);
			ui.mineral(search);
			let regex =
				Chip::filter("messages-regex", strip_mnemonic(s.get("MessagesRegex")), &mut self.model.filter.regex);
			ui.mineral(regex).into_response().on_hover_text(s.get("TipMessagesRegex"));
			let follow = Chip::filter(
				"messages-follow",
				strip_mnemonic(s.get("MessagesAutoScroll")),
				&mut self.model.auto_scroll,
			);
			if ui.mineral(follow).into_response().on_hover_text(s.get("TipMessagesAutoScroll")).changed()
				&& self.model.auto_scroll
			{
				self.scroll_to = Some(ScrollTo::End);
			}
			let clear =
				PushButton::new("messages-clear", strip_mnemonic(s.get("MessagesClear"))).variant(ButtonVariant::Text);
			if ui.mineral(clear).into_response().on_hover_text(s.get("TipMessagesClear")).clicked() {
				self.model.clear(ring);
			}
		});
	}

	/// The column titles; a grip at each one's right end resizes it.
	fn header(&mut self, ui: &mut Mui<'_>, s: &Strings) {
		let c = ui.tokens().color;
		let h = row_height(ui);
		let w = ui.available_width();
		let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
		ui.painter().rect_filled(rect, 0., c.surface_container_high);
		let names = ["MessagesColumnTime", "MessagesColumnLevel", "MessagesColumnLogger", "MessagesColumnMessage"];
		let mut x = rect.left() + INSET;
		for (i, name) in names.into_iter().enumerate() {
			let Some(&width) = self.columns.get(i) else {
				cell(ui, s.get(name), x, rect.right() - x, rect, c.on_surface_variant);
				break;
			};
			cell(ui, s.get(name), x, width - GRIP, rect, c.on_surface_variant);
			let grip = Rect::from_x_y_ranges(x + width - GRIP..=x + width, rect.y_range());
			ui.painter().vline(grip.center().x, rect.y_range(), (1., c.outline_variant));
			let drag = ui.interact(grip, ui.scope_id().with(("messages-column", i)), Sense::click_and_drag());
			let range = RangeModel::new(f64::from(MIN_COLUMN), f64::from(MAX_COLUMN)).step(f64::from(COLUMN_STEP));
			let track = Track::horizontal(ui.rt().i18n().direction());
			let mut next = width;
			if drag.dragged() {
				next += track.sign() * drag.drag_delta().x;
			}
			if drag.hovered() || drag.dragged() {
				ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
			}
			self.columns[i] = range.drive(ui.rt().core(), drag.id, f64::from(next), track).round() as f32;
			let (label, value) = (s.get(name), range.numeric(f64::from(self.columns[i])));
			semantic::emit(ui.ctx(), Some(drag.id), grip, || {
				SemanticNode::new(StableId::new(format!("messages-column-{i}")), Role::Splitter, label)
					.ranged(value, Availability::Enabled)
			});
			x += self.columns[i];
		}
	}

	/// Up and Down step through the lines, Shift extends.
	fn keys(&mut self, ui: &Ui) {
		let mut steps = Vec::new();
		ui.input_mut(|i| {
			i.events.retain(|e| {
				let egui::Event::Key { key, pressed: true, modifiers, .. } = e else { return true };
				let (none, shift) = (modifiers.is_none(), modifiers.matches_exact(Modifiers::SHIFT));
				match key {
					Key::ArrowUp | Key::ArrowDown if none || shift => steps.push((*key == Key::ArrowDown, shift)),
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
	}

	/// The lines on screen in `room`; their rects and ids.
	fn rows(&mut self, ui: &mut Mui<'_>, room: Rect, list_id: Id) -> Vec<(Rect, u64)> {
		let h = row_height(ui);
		let t = ui.tokens();
		let mut list_ui = ui.new_child(UiBuilder::new().max_rect(room));
		// show_rows steps by the row height and the spacing at its call
		list_ui.spacing_mut().item_spacing.y = 0.;
		let scroll_to = self.scroll_to.take();
		let (model, columns) = (&self.model, self.columns);
		let mut shown = Vec::new();
		ScrollArea::vertical().auto_shrink(false).show_rows(&mut list_ui, h, model.ids.len(), |ui, range| {
			let top = ui.max_rect().top() - range.start as f32 * h;
			let at = |i: usize| Rect::from_min_size(pos2(room.left(), top + i as f32 * h), vec2(1., h));
			let to = match scroll_to {
				Some(ScrollTo::End) => model.ids.len().checked_sub(1).map(|i| (i, Some(Align::BOTTOM))),
				// a stepped-to line already in view stays where it is
				Some(ScrollTo::Line(id)) => model.ids.binary_search(&id).ok().map(|i| (i, None)),
				None => None,
			};
			if let Some((i, align)) = to {
				ui.scroll_to_rect_animation(at(i), align, egui::style::ScrollAnimation::none());
			}
			for i in range {
				let w = ui.available_width();
				let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
				let Some(line) = model.ids.get(i).and_then(|&id| model.log.get(id)) else { continue };
				row(ui, &t, rect, line, model.selection.ids.contains(&line.id), columns, list_id);
				shown.push((rect.intersect(room), line.id));
			}
		});
		shown
	}

	/// A click on a line selects it with its modifiers; a right click selects the line under it unless selected.
	fn press(&mut self, ui: &Ui, list: &Response, shown: &[(Rect, u64)]) {
		let (primary, secondary) = (list.clicked(), list.secondary_clicked());
		let Some(at) = list.interact_pointer_pos().filter(|_| primary || secondary) else { return };
		let Some(&(_, id)) = shown.iter().find(|(rect, _)| rect.contains(at)) else { return };
		let ids = &self.model.ids;
		if primary {
			let m = ui.input(|i| i.modifiers);
			self.model.selection.click(id, m.command, m.shift, ids);
		} else if !self.model.selection.ids.contains(&id) {
			self.model.selection.click(id, false, false, ids);
		}
	}

	/// Copy, Copy all and Clear on a right click or Shift+F10.
	fn context_menu(&mut self, rt: &Rt, s: &Strings, list: &Response, ring: &LogRing) {
		let menu = Menu::new("messages-menu", s.get("PaneMessages"))
			.item(
				Entry::Copy,
				MenuItem::new(strip_mnemonic(s.get("MessagesCopy"))).disabled(!self.model.has_selection()),
			)
			.item(Entry::CopyAll, strip_mnemonic(s.get("MessagesCopyAll")))
			.divider()
			.item(Entry::Clear, strip_mnemonic(s.get("MessagesClear")));
		match menu.show_context(rt, list).clicked {
			Some(Entry::Copy) => list.ctx.copy_text(self.model.copy_selected()),
			Some(Entry::CopyAll) => list.ctx.copy_text(self.model.copy_all()),
			Some(Entry::Clear) => self.model.clear(ring),
			None => {}
		}
	}
}

/// The context menu's entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, KeySalt)]
enum Entry {
	Copy,
	CopyAll,
	Clear,
}

/// A row's height: the body text's line and a gap.
fn row_height(ui: &Ui) -> f32 {
	ui.text_style_height(&TextStyle::Body) + 6.
}

/// The ink of a level's line.
fn level_ink(t: &Tokens, level: LogLevel) -> Color32 {
	let c = &t.color;
	match level {
		LogLevel::Trace | LogLevel::Debug => c.on_surface_variant,
		LogLevel::Info => c.on_surface,
		LogLevel::Warn => c.tertiary,
		LogLevel::Error | LogLevel::Fatal => c.error,
	}
}

/// One line at `rect`: time, level, short source and message in the level's ink or the selection's. The source
/// shows its full name and a multi-line message its whole text under the pointer; the list takes the clicks.
fn row(ui: &Ui, t: &Tokens, rect: Rect, line: &LogLine, selected: bool, columns: [f32; 3], list_id: Id) {
	let c = &t.color;
	let response = ui.interact(rect, list_id.with(line.id), Sense::hover());
	response.widget_info(|| WidgetInfo::selected(Role::ListItem, true, selected, &line.text));
	let ink = if selected {
		ui.painter().rect_filled(rect, 0., c.secondary_container);
		c.on_secondary_container
	} else {
		level_ink(t, line.level)
	};
	let mut x = rect.left() + INSET;
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
	if let Some(tip) = tip {
		response.on_hover_text(tip.as_str());
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
