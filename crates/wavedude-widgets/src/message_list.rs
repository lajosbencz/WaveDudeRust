//! The Messages pane: a 5000-line log with level toggles and counts, a search box (plain or regex),
//! auto-scroll, clear, resizable columns, multi-select, copy, and virtualised rows.

use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::rc::Rc;

use freya::prelude::*;
use regex::{Regex, RegexBuilder};

use crate::menu::{ContextMenuHost, ContextMenuRequest, MenuEntry, MenuItem, MenuModel};
use crate::theme::{Theme, use_wave_theme};
use crate::util::{TestIdExt, ctrl, focus_border, is_char, is_named, modifier_tracker, shift, use_modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
	Trace,
	Debug,
	Info,
	Warn,
	Error,
	Fatal,
}

impl LogLevel {
	pub const ALL: [LogLevel; 6] =
		[LogLevel::Trace, LogLevel::Debug, LogLevel::Info, LogLevel::Warn, LogLevel::Error, LogLevel::Fatal];

	pub fn name(self) -> &'static str {
		match self {
			LogLevel::Trace => "Trace",
			LogLevel::Debug => "Debug",
			LogLevel::Info => "Info",
			LogLevel::Warn => "Warn",
			LogLevel::Error => "Error",
			LogLevel::Fatal => "Fatal",
		}
	}

	fn index(self) -> usize {
		self as usize
	}

	pub fn color(self, theme: &Theme) -> Color {
		match self {
			LogLevel::Trace => theme.level_trace,
			LogLevel::Debug => theme.level_debug,
			LogLevel::Info => theme.level_info,
			LogLevel::Warn => theme.level_warn,
			LogLevel::Error | LogLevel::Fatal => theme.level_error,
		}
	}
}

/// One log entry. `id` is assigned by [`MessageLog::push`] and never reused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
	pub id: u64,
	/// Already formatted, e.g. `21:58:52.123`.
	pub time: String,
	pub level: LogLevel,
	/// The full logger name.
	pub source: String,
	pub text: String,
	/// Exception text or a quoted file; the row's tooltip.
	pub detail: Option<String>,
}

impl LogLine {
	pub fn new(time: impl Into<String>, level: LogLevel, source: impl Into<String>, text: impl Into<String>) -> Self {
		Self { id: 0, time: time.into(), level, source: source.into(), text: text.into(), detail: None }
	}

	pub fn detail(mut self, detail: impl Into<String>) -> Self {
		self.detail = Some(detail.into());
		self
	}

	/// The logger name without its namespace (after the last `.` or `::`).
	pub fn source_short(&self) -> &str {
		let s = self.source.rsplit("::").next().unwrap_or(&self.source);
		s.rsplit('.').next().unwrap_or(s)
	}

	/// The copied form: time, level, source, message, tab-separated.
	pub fn copy_text(&self) -> String {
		format!("{}\t{}\t{}\t{}", self.time, self.level.name(), self.source_short(), self.text)
	}
}

/// The ring buffer behind the pane: the newest `capacity` lines, counts per level.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageLog {
	lines: VecDeque<LogLine>,
	capacity: usize,
	counts: [usize; 6],
	next_id: u64,
	/// Bumped by every change; a view compares it to notice arrivals.
	pub revision: u64,
}

impl Default for MessageLog {
	fn default() -> Self {
		Self::with_capacity(5000)
	}
}

impl MessageLog {
	pub fn with_capacity(capacity: usize) -> Self {
		Self { lines: VecDeque::new(), capacity: capacity.max(1), counts: [0; 6], next_id: 0, revision: 0 }
	}

	pub fn push(&mut self, mut line: LogLine) {
		line.id = self.next_id;
		self.next_id += 1;
		self.counts[line.level.index()] += 1;
		self.lines.push_back(line);
		while self.lines.len() > self.capacity {
			if let Some(old) = self.lines.pop_front() {
				self.counts[old.level.index()] -= 1;
			}
		}
		self.revision += 1;
	}

	/// Appends a batch; one revision for all of it.
	pub fn extend(&mut self, lines: impl IntoIterator<Item = LogLine>) {
		let revision = self.revision;
		for line in lines {
			self.push(line);
		}
		if self.revision != revision {
			self.revision = revision + 1;
		}
	}

	pub fn clear(&mut self) {
		self.lines.clear();
		self.counts = [0; 6];
		self.revision += 1;
	}

	pub fn lines(&self) -> impl ExactSizeIterator<Item = &LogLine> + DoubleEndedIterator {
		self.lines.iter()
	}

	pub fn len(&self) -> usize {
		self.lines.len()
	}

	pub fn is_empty(&self) -> bool {
		self.lines.is_empty()
	}

	pub fn count(&self, level: LogLevel) -> usize {
		self.counts[level.index()]
	}

	/// The line `id`, while the ring holds it.
	pub fn get(&self, id: u64) -> Option<&LogLine> {
		// Ids are consecutive within the ring.
		let first = self.lines.front()?.id;
		self.lines.get(usize::try_from(id.checked_sub(first)?).ok()?)
	}

	/// The ids of the lines the filter lets through, oldest first.
	pub fn visible_ids(&self, filter: &MessageFilter) -> Vec<u64> {
		let matcher = filter.matcher();
		self.lines.iter().filter(|l| filter.shown[l.level.index()] && matcher.matches(l)).map(|l| l.id).collect()
	}

	/// The lines the filter lets through, oldest first.
	pub fn visible(&self, filter: &MessageFilter) -> Vec<LogLine> {
		self.visible_ids(filter).into_iter().filter_map(|id| self.get(id).cloned()).collect()
	}
}

/// Which lines the pane shows.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageFilter {
	pub shown: [bool; 6],
	pub search: String,
	pub regex: bool,
}

impl Default for MessageFilter {
	fn default() -> Self {
		Self::new(cfg!(debug_assertions))
	}
}

impl MessageFilter {
	/// Info and up shown; Trace and Debug only when `verbose`.
	pub fn new(verbose: bool) -> Self {
		Self { shown: [verbose, verbose, true, true, true, true], search: String::new(), regex: false }
	}

	pub fn is_shown(&self, level: LogLevel) -> bool {
		self.shown[level.index()]
	}

	pub fn toggle(&mut self, level: LogLevel) {
		self.shown[level.index()] ^= true;
	}

	/// Case-insensitive; an unfinished pattern matches everything until it parses.
	pub fn matcher(&self) -> Matcher {
		let search = self.search.trim();
		if search.is_empty() {
			Matcher::All
		} else if self.regex {
			RegexBuilder::new(search).case_insensitive(true).build().map_or(Matcher::All, Matcher::Regex)
		} else {
			Matcher::Text(search.to_lowercase())
		}
	}
}

/// A compiled search over message, logger and detail.
pub enum Matcher {
	All,
	Text(String),
	Regex(Regex),
}

impl Matcher {
	pub fn matches(&self, line: &LogLine) -> bool {
		let fields = [Some(line.text.as_str()), Some(line.source.as_str()), line.detail.as_deref()];
		match self {
			Matcher::All => true,
			Matcher::Text(t) => fields.iter().flatten().any(|f| f.to_lowercase().contains(t)),
			Matcher::Regex(r) => fields.iter().flatten().any(|f| r.is_match(f)),
		}
	}
}

/// Selected line ids and the range anchor.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MessageSelection {
	pub ids: BTreeSet<u64>,
	pub anchor: Option<u64>,
	focus: Option<u64>,
}

impl MessageSelection {
	/// A click: plain selects one, Ctrl toggles, Shift selects from the anchor over `visible` (ids in order).
	pub fn click(&mut self, id: u64, ctrl: bool, shift: bool, visible: &[u64]) {
		match (shift, self.anchor) {
			(true, Some(anchor)) => {
				let range = range_ids(visible, anchor, id);
				if !ctrl {
					self.ids.clear();
				}
				self.ids.extend(range);
			}
			_ if ctrl => {
				if !self.ids.remove(&id) {
					self.ids.insert(id);
				}
				self.anchor = Some(id);
			}
			_ => {
				self.ids = BTreeSet::from([id]);
				self.anchor = Some(id);
			}
		}
		self.focus = Some(id);
	}

	/// Up or Down: the next line from the newest selected one; Shift extends from the anchor.
	pub fn step(&mut self, visible: &[u64], forward: bool, extend: bool) -> Option<u64> {
		let current = self.focus().and_then(|f| visible.iter().position(|&v| v == f));
		let next = match current {
			Some(i) if forward => (i + 1).min(visible.len().checked_sub(1)?),
			Some(i) => i.saturating_sub(1),
			None if forward => 0,
			None => visible.len().checked_sub(1)?,
		};
		let id = *visible.get(next)?;
		if extend && self.anchor.is_some() {
			let anchor = self.anchor;
			self.ids = range_ids(visible, anchor?, id).collect();
			self.anchor = anchor;
			self.focus = Some(id);
		} else {
			self.click(id, false, false, visible);
		}
		Some(id)
	}

	pub fn select_all(&mut self, visible: &[u64]) {
		self.ids = visible.iter().copied().collect();
	}

	/// The line keyboard steps start from.
	fn focus(&self) -> Option<u64> {
		self.focus.or(self.anchor)
	}

	/// The selected lines in `visible`, in order, as copied text.
	pub fn copy_text(&self, visible: &[LogLine]) -> String {
		visible.iter().filter(|l| self.ids.contains(&l.id)).map(LogLine::copy_text).collect::<Vec<_>>().join("\n")
	}
}

fn range_ids(visible: &[u64], a: u64, b: u64) -> impl Iterator<Item = u64> + '_ {
	let ia = visible.iter().position(|&v| v == a);
	let ib = visible.iter().position(|&v| v == b);
	let (lo, hi) = match (ia, ib) {
		(Some(x), Some(y)) => (x.min(y), x.max(y)),
		(None, Some(y)) => (y, y),
		_ => (1, 0),
	};
	visible.iter().copied().skip(lo).take((hi + 1).saturating_sub(lo))
}

/// Every visible line as copied text.
pub fn copy_all(visible: &[LogLine]) -> String {
	visible.iter().map(LogLine::copy_text).collect::<Vec<_>>().join("\n")
}

/// Column widths of Time, Level and Source in logical pixels; Message takes the rest.
pub const DEFAULT_COLUMNS: [f32; 3] = [88., 52., 120.];
const MIN_COLUMN: f32 = 30.;

#[derive(Clone)]
struct Rows {
	/// The visible lines' ids, recomputed only when the log or the filter changes.
	ids: Rc<Vec<u64>>,
	selected: BTreeSet<u64>,
	columns: [f32; 3],
}

impl PartialEq for Rows {
	fn eq(&self, other: &Self) -> bool {
		Rc::ptr_eq(&self.ids, &other.ids) && self.selected == other.selected && self.columns == other.columns
	}
}

/// The visible ids of a log revision under a filter.
type VisibleCache = Option<(u64, MessageFilter, Rc<Vec<u64>>)>;

/// The pane's words; menu texts may carry `_` mnemonics.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageLabels {
	pub copy: String,
	pub copy_all: String,
	pub clear: String,
	pub clear_tip: String,
	pub search: String,
	pub regex: String,
	pub regex_tip: String,
	pub auto_scroll: String,
	pub auto_scroll_tip: String,
	pub level_tip: String,
	/// Time, Level, Source, Message.
	pub columns: [String; 4],
}

impl Default for MessageLabels {
	fn default() -> Self {
		Self {
			copy: "_Copy".into(),
			copy_all: "Copy _all".into(),
			clear: "C_lear".into(),
			clear_tip: "Clear all messages".into(),
			search: "Search".into(),
			regex: "Regex".into(),
			regex_tip: "Treat the search as a regular expression".into(),
			auto_scroll: "Auto-scroll".into(),
			auto_scroll_tip: "Follow new messages".into(),
			level_tip: "Show or hide this level".into(),
			columns: ["Time".into(), "Level".into(), "Source".into(), "Message".into()],
		}
	}
}

/// The pane's content, bound to the log the application appends to (in batches, so bursts render once).
#[derive(Clone, PartialEq)]
pub struct MessageList {
	log: Writable<MessageLog>,
	verbose: bool,
	labels: MessageLabels,
	list_id: Option<AccessibilityId>,
}

impl MessageList {
	pub fn new(log: impl IntoWritable<MessageLog>) -> Self {
		Self {
			log: log.into_writable(),
			verbose: cfg!(debug_assertions),
			labels: MessageLabels::default(),
			list_id: None,
		}
	}

	/// The list's id, for focusing it from outside.
	pub fn list_id(mut self, id: AccessibilityId) -> Self {
		self.list_id = Some(id);
		self
	}

	pub fn labels(mut self, labels: MessageLabels) -> Self {
		self.labels = labels;
		self
	}

	/// Whether Trace and Debug start shown.
	pub fn verbose(mut self, verbose: bool) -> Self {
		self.verbose = verbose;
		self
	}
}

impl Component for MessageList {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let verbose = self.verbose;
		let mut filter = use_state(|| MessageFilter::new(verbose));
		let mut selection = use_state(MessageSelection::default);
		let mut auto_scroll = use_state(|| true);
		let columns = use_state(|| DEFAULT_COLUMNS);
		let menu = use_state(|| None::<ContextMenuRequest>);
		let mut search = use_state(String::new);
		let mut seen_revision = use_state(|| u64::MAX);
		let mut scroll = use_scroll_controller(|| ScrollConfig {
			default_vertical_position: ScrollPosition::End,
			..Default::default()
		});
		let modifiers = use_modifiers();
		let own_id = use_a11y();
		let list_id = self.list_id.unwrap_or(own_id);
		let list_focus = use_focus(list_id);
		let mut list_origin = use_state(|| (0., 0.));

		let cache = use_hook(|| Rc::new(RefCell::new(VisibleCache::None)));
		let current = filter.read().clone();
		let log = self.log.read();
		let counts = LogLevel::ALL.map(|l| log.count(l));
		let revision = log.revision;
		let ids = {
			let mut cache = cache.borrow_mut();
			match &*cache {
				Some((r, f, ids)) if *r == revision && *f == current => ids.clone(),
				_ => {
					let ids = Rc::new(log.visible_ids(&current));
					*cache = Some((revision, current, ids.clone()));
					ids
				}
			}
		};
		drop(log);
		if *seen_revision.peek() != revision {
			seen_revision.set(revision);
			let present: BTreeSet<u64> = ids.iter().copied().collect();
			if selection.peek().ids.iter().any(|id| !present.contains(id)) {
				selection.write().ids.retain(|id| present.contains(id));
			}
			if *auto_scroll.peek() {
				spawn(async move {
					scroll.scroll_to(ScrollPosition::End, Direction::Vertical);
				});
			}
		}
		let visible_lines = {
			let (log, ids) = (self.log.clone(), ids.clone());
			move || -> Vec<LogLine> {
				let log = log.peek();
				ids.iter().filter_map(|&id| log.get(id).cloned()).collect()
			}
		};

		let copy_selected = {
			let visible_lines = visible_lines.clone();
			move || {
				let _ = Clipboard::set(selection.peek().copy_text(&visible_lines()));
			}
		};
		let on_menu = {
			let mut log = self.log.clone();
			let copy_selected = copy_selected.clone();
			move |id: String| match id.as_str() {
				"messages:copy" => copy_selected(),
				"messages:copy_all" => {
					let _ = Clipboard::set(copy_all(&visible_lines()));
				}
				"messages:clear" => log.write().clear(),
				_ => {}
			}
		};
		let words = self.labels.clone();
		let menu_words = use_reactive(&self.labels);
		let open_menu = move |at: (f64, f64)| {
			let has_selection = !selection.peek().ids.is_empty();
			let entries: Vec<MenuEntry> = vec![
				MenuItem::new("messages:copy", menu_words.peek().copy.clone())
					.shortcut("Ctrl+C")
					.enabled(has_selection)
					.into(),
				MenuItem::new("messages:copy_all", menu_words.peek().copy_all.clone()).into(),
				MenuEntry::Separator,
				MenuItem::new("messages:clear", menu_words.peek().clear.clone()).into(),
			];
			let mut menu = menu;
			menu.set(Some(ContextMenuRequest { at, model: MenuModel::new(entries) }));
		};

		let toggles = LogLevel::ALL.iter().map(|&level| {
			ToggleChip {
				text: format!("{} ({})", level.name(), counts[level.index()]),
				color: level.color(&theme),
				on: filter.read().is_shown(level),
				tooltip: words.level_tip.clone(),
				id: format!("Level:{}", level.name()),
				on_toggle: EventHandler::new(move |_| filter.write().toggle(level)),
			}
			.into_element()
		});
		let mut clear_log = self.log.clone();
		let toolbar = rect()
			.horizontal()
			.cross_align(Alignment::center())
			.spacing(4.)
			.padding(2.)
			.width(Size::fill())
			.children(toggles)
			.child(
				Input::new(WritableSetter::new(search, move |t: String| {
					search.set(t.clone());
					filter.write().search = t;
				}))
				.placeholder(words.search.clone())
				.compact()
				.width(Size::px(160.)),
			)
			.child(ToggleChip {
				text: words.regex.clone(),
				color: theme.text,
				on: filter.read().regex,
				tooltip: words.regex_tip.clone(),
				id: "Regex".into(),
				on_toggle: EventHandler::new(move |_| {
					let mut f = filter.write();
					f.regex = !f.regex;
				}),
			})
			.child(ToggleChip {
				text: words.auto_scroll.clone(),
				color: theme.text,
				on: *auto_scroll.read(),
				tooltip: words.auto_scroll_tip.clone(),
				id: "AutoScroll".into(),
				on_toggle: EventHandler::new(move |_| {
					let on = !*auto_scroll.peek();
					auto_scroll.set(on);
					if on {
						scroll.scroll_to(ScrollPosition::End, Direction::Vertical);
					}
				}),
			})
			.child(
				TooltipContainer::new(Tooltip::new_text(words.clear_tip.clone())).child(
					Button::new()
						.compact()
						.on_press(move |_| clear_log.write().clear())
						.child(label().text(words.clear.replace('_', ""))),
				),
			);

		let length = ids.len();
		let rows = Rows { ids: ids.clone(), selected: selection.read().ids.clone(), columns: *columns.read() };
		let row_theme = theme.clone();
		let row_log = self.log.clone();
		let list = VirtualScrollView::new_with_data_controlled(
			rows,
			move |item, rows: &Rows| {
				let log = row_log.peek();
				let Some(line) = rows.ids.get(item.index).and_then(|&id| log.get(id)) else {
					return rect().height(Size::px(item.size)).into_element();
				};
				let ids = rows.ids.clone();
				row_view(&row_theme, line, rows.selected.contains(&line.id), rows.columns, item.size, move |e| {
					let id = e.0;
					let mut selection = selection;
					let m = *modifiers.peek();
					let (c, s) = (m.contains(Modifiers::CONTROL), m.contains(Modifiers::SHIFT));
					list_id.request_focus();
					match e.1 {
						MouseButton::Right => {
							if !selection.peek().ids.contains(&id) {
								selection.write().click(id, false, false, &ids);
							}
							open_menu(e.2);
						}
						_ => selection.write().click(id, c, s, &ids),
					}
				})
			},
			scroll,
		)
		.length(length)
		.item_size(theme.row_height)
		.scroll_with_arrows(false);

		let key_ids = ids.clone();
		let on_key = {
			let copy_selected = copy_selected.clone();
			move |e: Event<KeyboardEventData>| {
				let handled = if is_named(&e, NamedKey::ArrowDown) || is_named(&e, NamedKey::ArrowUp) {
					let forward = is_named(&e, NamedKey::ArrowDown);
					if let Some(id) = selection.write().step(&key_ids, forward, shift(&e)) {
						let index =
							key_ids.iter().position(|&v| v == id).expect("a step lands on a listed line") as f32;
						scroll_row_into_view(scroll, index, theme.row_height);
						auto_scroll.set_if_modified(false);
					}
					true
				} else if ctrl(&e) && is_char(&e, "c") {
					copy_selected();
					true
				} else if ctrl(&e) && is_char(&e, "a") {
					selection.write().select_all(&key_ids);
					true
				} else if is_named(&e, NamedKey::ContextMenu) || (shift(&e) && is_named(&e, NamedKey::F10)) {
					open_menu(*list_origin.peek());
					true
				} else {
					false
				};
				if handled {
					e.stop_propagation();
					e.prevent_default();
				}
			}
		};

		modifier_tracker(
			modifiers,
			rect()
				.test_id("MessageList")
				.width(Size::fill())
				.height(Size::fill())
				.background(theme.tool_window_background)
				.color(theme.text)
				.font_size(theme.font_size)
				.child(toolbar)
				.child(ColumnHeader { columns, names: self.labels.columns.clone() })
				.child(
					rect()
						.a11y_id(list_id)
						.a11y_focusable(true)
						.width(Size::fill())
						.height(Size::fill())
						.border(focus_border(list_focus(), theme.accent))
						.on_sized(move |e: Event<SizedEventData>| {
							let o = e.area.origin;
							list_origin.set_if_modified((f64::from(o.x) + 20., f64::from(o.y) + 20.));
						})
						.on_key_down(on_key)
						.child(list),
				)
				.child(ContextMenuHost::new(menu, on_menu)),
		)
	}
}

fn scroll_row_into_view(mut scroll: ScrollController, index: f32, row: f32) {
	let top = -scroll.position().y;
	let y = index * row;
	if y < top {
		scroll.scroll_to_y(-(y as i32));
	} else {
		let viewport = 6. * row;
		if y + row > top + viewport * 2. {
			scroll.scroll_to_y(-((y - viewport) as i32));
		}
	}
}

type RowPress = (u64, MouseButton, (f64, f64));

fn row_view(
	theme: &Theme,
	line: &LogLine,
	selected: bool,
	columns: [f32; 3],
	height: f32,
	on_press: impl Fn(RowPress) + 'static,
) -> Element {
	let id = line.id;
	let (bg, fg) =
		if selected { (theme.selection, theme.selection_text) } else { (Color::TRANSPARENT, line.level.color(theme)) };
	let cell = |text: &str, width: Size| {
		label().text(text.to_owned()).max_lines(1).text_overflow(TextOverflow::Ellipsis).width(width)
	};
	let source = TooltipContainer::new(Tooltip::new_text(line.source.clone()))
		.child(cell(line.source_short(), Size::px(columns[2] - 6.)));
	let row = rect()
		.key(id)
		.horizontal()
		.cross_align(Alignment::center())
		.spacing(6.)
		.padding((0., 4.))
		.width(Size::fill())
		.height(Size::px(height))
		.background(bg)
		.color(fg)
		.on_pointer_down(move |e: Event<PointerEventData>| {
			let p = e.global_location();
			on_press((id, e.button().unwrap_or(MouseButton::Left), (p.x, p.y)));
		})
		.child(cell(&line.time, Size::px(columns[0] - 6.)))
		.child(cell(line.level.name(), Size::px(columns[1] - 6.)))
		.child(source)
		.child(cell(&line.text, Size::flex(1.)))
		.content(Content::flex());
	match &line.detail {
		Some(d) => crate::util::wide_tooltip(d.clone(), row),
		None => row.into_element(),
	}
}

#[derive(Clone, PartialEq)]
struct ColumnHeader {
	columns: State<[f32; 3]>,
	names: [String; 4],
}

impl Component for ColumnHeader {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let mut columns = self.columns;
		let mut drag = use_state(|| None::<(usize, f64, f32)>);
		let widths = *columns.read();
		let names = self.names.clone();
		let cells = (0..3).map(|i| {
			rect()
				.horizontal()
				.width(Size::px(widths[i]))
				.height(Size::fill())
				.cross_align(Alignment::center())
				.content(Content::flex())
				.child(label().text(names[i].clone()).max_lines(1).width(Size::flex(1.)))
				.child(rect().width(Size::px(4.)).height(Size::fill()).background(theme.splitter).on_pointer_down(
					move |e: Event<PointerEventData>| {
						drag.set(Some((i, e.global_location().x, columns.peek()[i])));
					},
				))
				.into_element()
		});
		rect()
			.horizontal()
			.width(Size::fill())
			.height(Size::px(theme.row_height))
			.padding((0., 0., 0., 4.))
			.cross_align(Alignment::center())
			.background(theme.control_background)
			.color(theme.text_dim)
			.on_global_pointer_move(move |e: Event<PointerEventData>| {
				let Some((i, x0, w0)) = *drag.peek() else { return };
				let w = (w0 + (e.global_location().x - x0) as f32).max(MIN_COLUMN).round();
				if columns.peek()[i] != w {
					columns.write()[i] = w;
				}
			})
			.on_global_pointer_up(move |_| drag.set_if_modified(None))
			.children(cells)
			.child(label().text(self.names[3].clone()).max_lines(1))
	}
}

/// A focusable on/off chip; Space or Enter toggles.
#[derive(Clone, PartialEq)]
struct ToggleChip {
	text: String,
	color: Color,
	on: bool,
	tooltip: String,
	id: String,
	on_toggle: EventHandler<()>,
}

impl Component for ToggleChip {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let a11y = use_a11y();
		let focus = use_focus(a11y);
		let (press, key) = (self.on_toggle.clone(), self.on_toggle.clone());
		let chip =
			rect()
				.a11y_id(a11y)
				.a11y_focusable(true)
				.test_id(self.id.clone())
				.padding((2., 6.))
				.corner_radius(2.)
				.background(if self.on { theme.hover_background } else { Color::TRANSPARENT })
				.border(focus_border(focus(), theme.accent).or_else(|| {
					Some(Border::new().fill(theme.input_border).width(1.).alignment(BorderAlignment::Inner))
				}))
				.color(if self.on { self.color } else { theme.text_disabled })
				.on_pointer_down(move |e: Event<PointerEventData>| {
					if e.button() == Some(MouseButton::Left) {
						a11y.request_focus();
						press.call(());
					}
				})
				.on_key_down(move |e: Event<KeyboardEventData>| {
					if is_named(&e, NamedKey::Enter) || matches!(&e.key, Key::Character(c) if c == " ") {
						e.stop_propagation();
						key.call(());
					}
				})
				.child(label().text(self.text.clone()).max_lines(1));
		TooltipContainer::new(Tooltip::new_text(self.tooltip.clone())).child(chip)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn log() -> MessageLog {
		let mut log = MessageLog::with_capacity(4);
		log.extend([
			LogLine::new("00:00:01.000", LogLevel::Debug, "WaveDude.Engine", "grid 1920 x 1080"),
			LogLine::new("00:00:02.000", LogLevel::Info, "WaveDude.App", "Opened tiny.wdproj"),
			LogLine::new("00:00:03.000", LogLevel::Warn, "wavedude_app::store", "spilled to disk")
				.detail("budget 6 GB"),
			LogLine::new("00:00:04.000", LogLevel::Error, "WaveDude.Export", "The export failed."),
		]);
		log
	}

	#[test]
	fn ring_buffer_drops_the_oldest() {
		let mut log = log();
		assert_eq!(log.revision, 1);
		log.push(LogLine::new("00:00:05.000", LogLevel::Info, "A", "five"));
		assert_eq!(log.len(), 4);
		assert_eq!(log.count(LogLevel::Debug), 0);
		assert_eq!(log.count(LogLevel::Info), 2);
		assert_eq!(log.lines().next().map(|l| l.id), Some(1));
		log.clear();
		assert!(log.is_empty() && log.count(LogLevel::Info) == 0);
	}

	#[test]
	fn filter_by_level_text_and_regex() {
		let log = log();
		let mut f = MessageFilter::new(false);
		assert_eq!(log.visible(&f).len(), 3);
		f.toggle(LogLevel::Debug);
		assert_eq!(log.visible(&f).len(), 4);
		f.search = "EXPORT".into();
		assert_eq!(log.visible(&f).len(), 1);
		f.search = "budget".into();
		assert_eq!(log.visible(&f)[0].level, LogLevel::Warn);
		f.regex = true;
		f.search = r"^(opened|the)\b".into();
		assert_eq!(log.visible(&f).len(), 2);
		f.search = "(unfinished".into();
		assert_eq!(log.visible(&f).len(), 4);
	}

	#[test]
	fn lines_are_found_by_id_across_the_ring() {
		let mut log = log();
		log.push(LogLine::new("00:00:05.000", LogLevel::Info, "A", "five"));
		assert_eq!(log.get(0), None, "dropped by the ring");
		assert_eq!(log.get(4).map(|l| l.text.as_str()), Some("five"));
		assert_eq!(log.get(5), None);
		assert_eq!(log.visible_ids(&MessageFilter::new(true)), [1, 2, 3, 4]);
		log.clear();
		assert_eq!(log.get(4), None);
	}

	#[test]
	fn selection_and_copy() {
		let log = log();
		let visible = log.visible(&MessageFilter::new(true));
		let ids: Vec<u64> = visible.iter().map(|l| l.id).collect();
		let mut s = MessageSelection::default();
		s.click(1, false, false, &ids);
		s.click(3, false, true, &ids);
		assert_eq!(s.ids, BTreeSet::from([1, 2, 3]));
		s.click(2, true, false, &ids);
		assert_eq!(s.ids, BTreeSet::from([1, 3]));
		assert_eq!(s.copy_text(&visible).lines().count(), 2);
		assert_eq!(visible[2].copy_text(), "00:00:03.000\tWarn\tstore\tspilled to disk");
		let mut s = MessageSelection::default();
		assert_eq!(s.step(&ids, true, false), Some(0));
		assert_eq!(s.step(&ids, true, true), Some(1));
		assert_eq!(s.step(&ids, true, true), Some(2));
		assert_eq!(s.ids, BTreeSet::from([0, 1, 2]));
		assert_eq!(s.step(&ids, false, false), Some(1));
		assert_eq!(s.ids, BTreeSet::from([1]));
		s.select_all(&ids);
		assert_eq!(copy_all(&visible).lines().count(), 4);
	}
}
