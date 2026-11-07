//! Menu bar and context menus from one model: mnemonics (an underscore marks the letter), icons, shortcut
//! text, check marks and radio dots, separators, submenus, full keyboard navigation.

use freya::prelude::*;

use crate::icon::{Icon, IconView, icons};
use crate::theme::{Theme, use_wave_theme};
use crate::util::TestIdExt;

/// Check state shown in an entry's icon slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Check {
	#[default]
	None,
	Checked(bool),
	Radio(bool),
}

/// A command entry. `text` marks its mnemonic with an underscore (`"_Open"`); `"__"` is a literal one. `id` is
/// the test id; `action` is what activating the entry reports.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem<A = String> {
	pub id: String,
	pub action: A,
	pub text: String,
	pub shortcut: Option<String>,
	pub icon: Icon,
	pub check: Check,
	pub enabled: bool,
	pub tooltip: Option<String>,
}

impl MenuItem {
	/// An entry whose action is its id.
	pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
		let id = id.into();
		Self::with_action(id.clone(), text, id)
	}
}

impl<A> MenuItem<A> {
	pub fn with_action(id: impl Into<String>, text: impl Into<String>, action: A) -> Self {
		Self {
			id: id.into(),
			action,
			text: text.into(),
			shortcut: None,
			icon: Icon::None,
			check: Check::None,
			enabled: true,
			tooltip: None,
		}
	}

	pub fn shortcut(mut self, text: impl Into<String>) -> Self {
		self.shortcut = Some(text.into());
		self
	}

	pub fn icon(mut self, icon: Icon) -> Self {
		self.icon = icon;
		self
	}

	pub fn check(mut self, check: Check) -> Self {
		self.check = check;
		self
	}

	pub fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = enabled;
		self
	}

	pub fn tooltip(mut self, text: impl Into<String>) -> Self {
		self.tooltip = Some(text.into());
		self
	}
}

/// An entry that opens a nested menu; a menu bar's headers are submenus too.
#[derive(Clone, Debug, PartialEq)]
pub struct Submenu<A = String> {
	pub id: String,
	pub text: String,
	pub icon: Icon,
	pub enabled: bool,
	pub entries: Vec<MenuEntry<A>>,
}

impl<A> Submenu<A> {
	pub fn new(id: impl Into<String>, text: impl Into<String>, entries: Vec<MenuEntry<A>>) -> Self {
		Self { id: id.into(), text: text.into(), icon: Icon::None, enabled: true, entries }
	}

	pub fn icon(mut self, icon: Icon) -> Self {
		self.icon = icon;
		self
	}

	pub fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = enabled;
		self
	}
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry<A = String> {
	Item(MenuItem<A>),
	Submenu(Submenu<A>),
	Separator,
}

impl<A> MenuEntry<A> {
	fn text(&self) -> Option<&str> {
		match self {
			MenuEntry::Item(i) => Some(&i.text),
			MenuEntry::Submenu(s) => Some(&s.text),
			MenuEntry::Separator => None,
		}
	}

	fn is_enabled(&self) -> bool {
		match self {
			MenuEntry::Item(i) => i.enabled,
			MenuEntry::Submenu(s) => s.enabled,
			MenuEntry::Separator => false,
		}
	}

	/// The same entry with every action turned by `f`.
	pub fn map<B>(self, f: &mut impl FnMut(A) -> B) -> MenuEntry<B> {
		match self {
			MenuEntry::Item(i) => MenuEntry::Item(MenuItem {
				id: i.id,
				action: f(i.action),
				text: i.text,
				shortcut: i.shortcut,
				icon: i.icon,
				check: i.check,
				enabled: i.enabled,
				tooltip: i.tooltip,
			}),
			MenuEntry::Submenu(s) => MenuEntry::Submenu(Submenu {
				id: s.id,
				text: s.text,
				icon: s.icon,
				enabled: s.enabled,
				entries: s.entries.into_iter().map(|e| e.map(f)).collect(),
			}),
			MenuEntry::Separator => MenuEntry::Separator,
		}
	}
}

impl<A> From<MenuItem<A>> for MenuEntry<A> {
	fn from(item: MenuItem<A>) -> Self {
		MenuEntry::Item(item)
	}
}

impl<A> From<Submenu<A>> for MenuEntry<A> {
	fn from(submenu: Submenu<A>) -> Self {
		MenuEntry::Submenu(submenu)
	}
}

/// A list of entries: a context menu, or a menu bar whose entries are its header submenus.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuModel<A = String> {
	pub entries: Vec<MenuEntry<A>>,
}

impl<A> Default for MenuModel<A> {
	fn default() -> Self {
		Self { entries: Vec::new() }
	}
}

impl<A> MenuModel<A> {
	pub fn new(entries: Vec<MenuEntry<A>>) -> Self {
		Self { entries }
	}

	/// The same menu with every action turned by `f`.
	pub fn map<B>(self, mut f: impl FnMut(A) -> B) -> MenuModel<B> {
		MenuModel { entries: self.entries.into_iter().map(|e| e.map(&mut f)).collect() }
	}
}

/// Display text with the underscores resolved, and the char index of the mnemonic letter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MnemonicText {
	pub text: String,
	pub mnemonic: Option<usize>,
}

pub fn parse_mnemonic(raw: &str) -> MnemonicText {
	let mut text = String::with_capacity(raw.len());
	let mut mnemonic = None;
	let mut chars = raw.chars().peekable();
	let mut count = 0;
	while let Some(c) = chars.next() {
		if c == '_' {
			match chars.peek() {
				Some('_') => {
					chars.next();
					text.push('_');
					count += 1;
				}
				Some(_) if mnemonic.is_none() => mnemonic = Some(count),
				_ => {}
			}
			continue;
		}
		text.push(c);
		count += 1;
	}
	MnemonicText { text, mnemonic }
}

/// The mnemonic letter of an entry's text, lowercased.
pub fn mnemonic_char(raw: &str) -> Option<char> {
	let parsed = parse_mnemonic(raw);
	parsed.mnemonic.and_then(|i| parsed.text.chars().nth(i)).map(|c| c.to_lowercase().next().unwrap_or(c))
}

/// What a letter does in a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MnemonicHit {
	None,
	Unique(usize),
	/// Several enabled entries share the letter: the highlight walks them.
	Ambiguous(Vec<usize>),
}

pub fn resolve_mnemonic<A>(entries: &[MenuEntry<A>], letter: char) -> MnemonicHit {
	let letter = letter.to_lowercase().next().unwrap_or(letter);
	let hits: Vec<usize> = entries
		.iter()
		.enumerate()
		.filter(|(_, e)| e.is_enabled() && e.text().and_then(mnemonic_char) == Some(letter))
		.map(|(i, _)| i)
		.collect();
	match hits.len() {
		0 => MnemonicHit::None,
		1 => MnemonicHit::Unique(hits[0]),
		_ => MnemonicHit::Ambiguous(hits),
	}
}

/// Letters used by more than one entry of a menu: the uniqueness check the string tables must pass.
pub fn duplicate_mnemonics<A>(entries: &[MenuEntry<A>]) -> Vec<char> {
	let mut seen = Vec::new();
	let mut dup = Vec::new();
	for c in entries.iter().filter_map(|e| e.text().and_then(mnemonic_char)) {
		if seen.contains(&c) {
			if !dup.contains(&c) {
				dup.push(c);
			}
		} else {
			seen.push(c);
		}
	}
	dup
}

/// The next enabled entry after `from` (before it when `!forward`), wrapping; `None` when none is enabled.
pub fn next_enabled<A>(entries: &[MenuEntry<A>], from: Option<usize>, forward: bool) -> Option<usize> {
	let n = entries.len();
	if n == 0 {
		return None;
	}
	let start = match (from, forward) {
		(Some(i), _) => i,
		(None, true) => n - 1,
		(None, false) => 0,
	};
	(1..=n).map(|k| if forward { (start + k) % n } else { (start + n - k % n) % n }).find(|&i| entries[i].is_enabled())
}

/// Keyboard and pointer navigation of an open menu chain, pure.
///
/// `stack` is empty while the bar is active without a drop-down; `stack[0]` is the highlight in the
/// open header's drop-down, `stack[k]` in the submenu opened from `stack[k - 1]`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MenuNav {
	pub active: bool,
	pub header: usize,
	pub stack: Vec<Option<usize>>,
	/// Opened by keyboard: mnemonics are underlined and the first entry is highlighted.
	pub keyboard: bool,
}

/// The outcome of a key press while a menu is active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavOutcome<A = String> {
	Ignored,
	Handled,
	Activate(A),
	Closed,
}

impl MenuNav {
	/// Entries of the menu at `level` (0 = the header's drop-down).
	pub fn menu_at<'a, A>(&self, headers: &'a [MenuEntry<A>], level: usize) -> Option<&'a [MenuEntry<A>]> {
		let MenuEntry::Submenu(header) = headers.get(self.header)? else { return None };
		let mut entries: &[MenuEntry<A>] = &header.entries;
		for k in 0..level {
			let MenuEntry::Submenu(s) = entries.get((*self.stack.get(k)?)?)? else { return None };
			entries = &s.entries;
		}
		Some(entries)
	}

	pub fn open<A>(&mut self, headers: &[MenuEntry<A>], header: usize, keyboard: bool) {
		self.active = true;
		self.header = header;
		self.keyboard = keyboard;
		self.stack = vec![None];
		if keyboard {
			self.stack[0] = self.menu_at(headers, 0).and_then(|m| next_enabled(m, None, true));
		}
	}

	pub fn close(&mut self) {
		*self = Self::default();
	}

	/// F10 or a lone Alt: the bar takes the keys with its first header highlighted, nothing opened.
	pub fn activate_bar(&mut self) {
		*self = Self { active: true, header: 0, stack: Vec::new(), keyboard: true };
	}

	/// Pointer over entry `index` of `level`: highlights it, closes deeper levels, opens its submenu.
	pub fn hover<A>(&mut self, headers: &[MenuEntry<A>], level: usize, index: usize) {
		self.stack.truncate(level + 1);
		if self.stack.len() <= level {
			return;
		}
		self.stack[level] = Some(index);
		if matches!(self.menu_at(headers, level).and_then(|m| m.get(index)), Some(MenuEntry::Submenu(s)) if s.enabled) {
			self.stack.push(None);
		}
	}

	/// Activates the highlighted entry of the deepest level (Enter, a click, a unique mnemonic).
	pub fn press<A: Clone>(&mut self, headers: &[MenuEntry<A>], level: usize, index: usize) -> NavOutcome<A> {
		let Some(entry) = self.menu_at(headers, level).and_then(|m| m.get(index)) else { return NavOutcome::Ignored };
		match entry {
			MenuEntry::Item(item) if item.enabled => {
				let action = item.action.clone();
				self.close();
				NavOutcome::Activate(action)
			}
			MenuEntry::Submenu(s) if s.enabled => {
				let first = next_enabled(&s.entries, None, true);
				self.stack.truncate(level + 1);
				self.stack[level] = Some(index);
				self.stack.push(if self.keyboard { first } else { None });
				NavOutcome::Handled
			}
			_ => NavOutcome::Handled,
		}
	}

	/// `headers` are the bar's header submenus; `is_bar` false for a context menu (one fixed header).
	pub fn handle_key<A: Clone>(&mut self, headers: &[MenuEntry<A>], is_bar: bool, key: &Key) -> NavOutcome<A> {
		if !self.active {
			return NavOutcome::Ignored;
		}
		self.keyboard = true;
		let depth = self.stack.len();
		let level = depth.saturating_sub(1);
		let current = self.stack.last().copied().flatten();
		let menu = if depth == 0 { None } else { self.menu_at(headers, level) };
		match key {
			Key::Named(NamedKey::Escape) => {
				if depth > 1 || (depth == 1 && is_bar) {
					self.stack.pop();
					NavOutcome::Handled
				} else {
					self.close();
					NavOutcome::Closed
				}
			}
			Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::ArrowUp) => {
				let forward = *key == Key::Named(NamedKey::ArrowDown);
				match menu {
					None => {
						self.open(headers, self.header, true);
						if !forward {
							let m = self.menu_at(headers, 0).expect("a header is a submenu");
							self.stack[0] = next_enabled(m, None, false);
						}
					}
					Some(m) => self.stack[level] = next_enabled(m, current, forward),
				}
				NavOutcome::Handled
			}
			Key::Named(NamedKey::Home) | Key::Named(NamedKey::End) => {
				if let Some(m) = menu {
					self.stack[level] = next_enabled(m, None, *key == Key::Named(NamedKey::Home));
				}
				NavOutcome::Handled
			}
			Key::Named(NamedKey::ArrowRight) => {
				if let (Some(m), Some(i)) = (menu, current)
					&& matches!(m.get(i), Some(MenuEntry::Submenu(s)) if s.enabled)
				{
					return self.press(headers, level, i);
				}
				if is_bar {
					self.switch_header(headers, true, depth > 0);
				}
				NavOutcome::Handled
			}
			Key::Named(NamedKey::ArrowLeft) => {
				if depth > 1 {
					self.stack.pop();
				} else if is_bar {
					self.switch_header(headers, false, depth > 0);
				}
				NavOutcome::Handled
			}
			Key::Named(NamedKey::Enter) => match (menu, current) {
				(None, _) => {
					self.open(headers, self.header, true);
					NavOutcome::Handled
				}
				(Some(_), Some(i)) => self.press(headers, level, i),
				_ => NavOutcome::Handled,
			},
			Key::Character(c) if c == " " => match (menu, current) {
				(Some(_), Some(i)) => self.press(headers, level, i),
				_ => NavOutcome::Handled,
			},
			Key::Character(c) => {
				let Some(letter) = c.chars().next() else { return NavOutcome::Handled };
				let entries = match menu {
					Some(m) => m,
					None => headers,
				};
				match resolve_mnemonic(entries, letter) {
					MnemonicHit::None => NavOutcome::Handled,
					MnemonicHit::Unique(i) if menu.is_none() => {
						self.open(headers, i, true);
						NavOutcome::Handled
					}
					MnemonicHit::Unique(i) => self.press(headers, level, i),
					MnemonicHit::Ambiguous(hits) => {
						if menu.is_none() {
							let next = hits.iter().copied().find(|&h| h > self.header).unwrap_or(hits[0]);
							self.header = next;
						} else {
							let next = hits.iter().copied().find(|&h| Some(h) > current).unwrap_or(hits[0]);
							self.stack[level] = Some(next);
						}
						NavOutcome::Handled
					}
				}
			}
			Key::Named(NamedKey::Tab) => NavOutcome::Handled,
			_ => NavOutcome::Ignored,
		}
	}

	fn switch_header<A>(&mut self, headers: &[MenuEntry<A>], forward: bool, reopen: bool) {
		if let Some(h) = next_enabled(headers, Some(self.header), forward) {
			if reopen {
				self.open(headers, h, true);
			} else {
				self.header = h;
			}
		}
	}
}

const ICON_SLOT: f32 = 26.;
const SEPARATOR_HEIGHT: f32 = 7.;

/// A label with its mnemonic letter underlined when `underline` is set.
fn mnemonic_label(raw: &str, underline: bool, color: Color) -> Element {
	let parsed = parse_mnemonic(raw);
	let chars: Vec<char> = parsed.text.chars().collect();
	match parsed.mnemonic.filter(|_| underline) {
		Some(i) => {
			let before: String = chars[..i].iter().collect();
			let letter: String = chars[i..=i].iter().collect();
			let after: String = chars[i + 1..].iter().collect();
			paragraph()
				.color(color)
				.max_lines(1)
				.span(Span::new(before))
				.span(Span::new(letter).text_decoration(TextDecoration::Underline))
				.span(Span::new(after))
				.into()
		}
		None => label().text(parsed.text).color(color).max_lines(1).into(),
	}
}

const MIN_PANEL_WIDTH: f32 = 200.;
const PANEL_PADDING: f32 = 2.;
/// Room the label leaves for its own row's shortcut, which is anchored at the right edge.
const SHORTCUT_GAP: f32 = 90.;

fn entry_height<A>(theme: &Theme, entry: &MenuEntry<A>) -> f32 {
	match entry {
		MenuEntry::Separator => SEPARATOR_HEIGHT,
		_ => theme.row_height,
	}
}

fn entry_view<A>(
	theme: &Theme,
	entry: &MenuEntry<A>,
	highlighted: bool,
	underline: bool,
	on_hover: impl FnMut() + 'static,
	on_press: impl FnMut() + 'static,
) -> Element {
	if let MenuEntry::Separator = entry {
		// The line is the row itself: a sized child, fill or fill_minimum, measures as the window's width while
		// the panel fits its content, and the panel then spans the window.
		return rect()
			.width(Size::fill_minimum())
			.height(Size::px(1.))
			.margin((3., 2., 3., ICON_SLOT + 2.))
			.background(theme.menu_border)
			.into();
	}
	let enabled = entry.is_enabled();
	let ink = if enabled { theme.text } else { theme.text_disabled };
	let (icon, shortcut, submenu, tooltip, test_id) = match entry {
		MenuEntry::Item(i) => {
			let icon = match i.check {
				Check::Checked(true) => icons::CHECK,
				Check::Radio(true) => icons::RADIO,
				_ => i.icon.clone(),
			};
			(icon, i.shortcut.clone(), false, i.tooltip.clone(), i.id.clone())
		}
		MenuEntry::Submenu(s) => (s.icon.clone(), None, true, None, s.id.clone()),
		MenuEntry::Separator => unreachable!(),
	};
	let mut on_hover = on_hover;
	let mut on_press = on_press;
	let row = rect()
		.width(Size::fill_minimum())
		.height(Size::px(theme.row_height))
		.horizontal()
		.cross_align(Alignment::center())
		.background(if highlighted && enabled { theme.menu_hover } else { Color::TRANSPARENT })
		.a11y_role(AccessibilityRole::MenuItem)
		.test_id(test_id)
		.on_pointer_enter(move |_| on_hover())
		.on_pointer_down(move |e: Event<PointerEventData>| {
			e.stop_propagation();
			if enabled {
				on_press();
			}
		})
		.child(
			rect()
				.horizontal()
				.height(Size::fill())
				.cross_align(Alignment::center())
				.child(
					rect()
						.width(Size::px(ICON_SLOT))
						.height(Size::fill())
						.center()
						.child(IconView::new(icon).color(ink)),
				)
				.child(rect().padding((0., 24., 0., 2.)).child(mnemonic_label(
					entry.text().unwrap_or_default(),
					underline,
					ink,
				)))
				.child(rect().width(Size::px(SHORTCUT_GAP))),
		)
		.child(
			rect()
				.position(Position::new_absolute().top(0.).right(0.))
				.horizontal()
				.height(Size::px(theme.row_height))
				.cross_align(Alignment::center())
				.maybe_child(shortcut.map(|s| {
					label().text(s).color(if enabled { theme.text_dim } else { theme.text_disabled }).max_lines(1)
				}))
				.child(
					rect()
						.width(Size::px(16.))
						.center()
						.maybe_child(submenu.then(|| IconView::new(icons::SUBMENU).size(12.).color(ink))),
				),
		);
	// Drawn by the row itself: a wrapping container would end the panel's fit-content at the window edge.
	row.maybe_child(tooltip.filter(|_| highlighted).map(|tip| {
		rect()
			.position(Position::new_absolute().top(0.).right(-6.))
			.width(Size::px(0.))
			.height(Size::px(0.))
			.layer(Layer::Overlay)
			.child(rect().width(Size::window_percent(100.)).child(Tooltip::new_text(tip)))
	}))
	.into()
}

/// Where one level's panel lies: the anchor it hangs from, which the window's edge never moves, and the
/// panel itself.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PanelPlace {
	anchor: Area,
	area: Area,
}

fn place_at(
	mut panels: State<Vec<PanelPlace>>,
	level: usize,
	set: impl Fn(&mut PanelPlace, Area),
) -> impl FnMut(Event<SizedEventData>) {
	move |e: Event<SizedEventData>| {
		let mut v = panels.peek().clone();
		if v.len() <= level {
			v.resize(level + 1, PanelPlace::default());
		}
		let mut place = v[level];
		set(&mut place, e.area);
		if v[level] != place {
			v[level] = place;
			panels.set(v);
		}
	}
}

/// A panel hung from an anchor of no size at (`left`, `top`) of its parent, moved left by `shift` px. torin
/// resolves an absolute `right` against the parent's available width, not its fitted one, so a panel beside
/// a fit-content parent is placed by its left edge from the parent's measured width.
fn anchored(panels: State<Vec<PanelPlace>>, level: usize, left: f32, top: f32, shift: f32, panel: Element) -> Element {
	rect()
		.position(Position::new_absolute().top(top).left(left))
		.width(Size::px(0.))
		.height(Size::px(0.))
		.on_sized(place_at(panels, level, |p, a| p.anchor = a))
		.child(
			rect()
				.position(Position::new_absolute().top(0.).left(-shift))
				.width(Size::window_percent(100.))
				.child(panel),
		)
		.into_element()
}

/// Pixels the panel of `level` would pass the window's right edge by, as hung from its anchor: 0 until measured.
fn overflow(panels: &[PanelPlace], level: usize) -> f32 {
	let window = Platform::get().current_window().root_size.peek().width;
	panels.get(level).map_or(0., |p| (p.anchor.min_x() + p.area.width() - window).max(0.))
}

/// Shift for a drop-down or a context menu: moved left just enough to end inside the window, never past its
/// left edge.
fn shift_inside(panels: &[PanelPlace], level: usize) -> f32 {
	let anchor = panels.get(level).map_or(0., |p| p.anchor.min_x());
	overflow(panels, level).min(anchor.max(0.))
}

/// Shift for a submenu beside its parent: flipped to the parent's left side when it does not fit on the right,
/// or, when it fits on neither side, moved left over its parent like a drop-down.
fn shift_beside(panels: &[PanelPlace], level: usize) -> f32 {
	let Some(own) = panels.get(level).filter(|_| overflow(panels, level) > 0.) else { return 0. };
	let parent = panels.get(level - 1).map_or(MIN_PANEL_WIDTH, |p| p.area.width());
	let flipped = own.area.width() + parent - 2. * PANEL_PADDING;
	if own.anchor.min_x() >= flipped { flipped } else { shift_inside(panels, level) }
}

/// The open drop-downs of a chain, nested: level `level` and, under its highlighted submenu entry, the next.
fn panel_view<A: Clone + 'static>(
	theme: &Theme,
	headers: &[MenuEntry<A>],
	nav: State<MenuNav>,
	level: usize,
	on_activate: EventHandler<A>,
	panels: State<Vec<PanelPlace>>,
) -> Option<Element> {
	let snapshot = nav.read().clone();
	let entries = snapshot.menu_at(headers, level)?.to_vec();
	let highlight = snapshot.stack.get(level).copied().flatten();
	let underline = snapshot.keyboard;
	let deeper = snapshot.stack.len() > level + 1;
	let mut children = Vec::with_capacity(entries.len() + 2);
	let mut y = 0.;
	for (i, entry) in entries.iter().enumerate() {
		let headers_hover = headers.to_vec();
		let headers_press = headers.to_vec();
		let mut nav_h = nav;
		let mut nav_p = nav;
		let on_activate_row = on_activate.clone();
		let row = entry_view(
			theme,
			entry,
			highlight == Some(i),
			underline,
			move || {
				let mut n = nav_h.peek().clone();
				n.hover(&headers_hover, level, i);
				if *nav_h.peek() != n {
					nav_h.set(n);
				}
			},
			move || {
				let mut n = nav_p.peek().clone();
				let outcome = n.press(&headers_press, level, i);
				nav_p.set(n);
				if let NavOutcome::Activate(id) = outcome {
					on_activate_row.call(id);
				}
			},
		);
		children.push(row);
		if deeper
			&& highlight == Some(i)
			&& let Some(s) = panel_view(theme, headers, nav, level + 1, on_activate.clone(), panels)
		{
			let place = panels.read();
			let parent = place.get(level).map_or(MIN_PANEL_WIDTH, |p| p.area.width());
			let shift = shift_beside(&place, level + 1);
			children.push(anchored(panels, level + 1, parent - PANEL_PADDING, y - 3., shift, s));
		}
		y += entry_height(theme, entry);
	}
	// A fit-content panel takes its widest child; the spacer is the floor, since torin adds a min_width to the content.
	children.insert(0, rect().width(Size::px(MIN_PANEL_WIDTH)).height(Size::px(0.)).into_element());
	Some(
		rect()
			.layer(Layer::Overlay)
			.content(Content::fit())
			.background(theme.menu_background)
			.border(Border::new().fill(theme.menu_border).width(1.).alignment(BorderAlignment::Inner))
			.shadow((0., 2., 6., 0., Color::from_af32rgb(0.35, 0, 0, 0)))
			.padding(PANEL_PADDING)
			.color(theme.text)
			.font_size(theme.font_size)
			.a11y_role(AccessibilityRole::Menu)
			.on_sized(place_at(panels, level, |p, a| p.area = a))
			.children(children)
			.into(),
	)
}

fn contains(panels: &[PanelPlace], p: CursorPoint, levels: usize) -> bool {
	panels.iter().take(levels).any(|place| {
		let a = place.area.to_f64();
		p.x >= a.min_x() && p.x < a.max_x() && p.y >= a.min_y() && p.y < a.max_y()
	})
}

fn restore_focus(previous: State<Option<AccessibilityId>>) {
	if let Some(id) = *previous.peek() {
		id.request_focus();
	}
}

/// The window's menu bar: Alt+letter or F10 or a lone Alt enters it; arrows walk, letters pick, Esc closes.
#[derive(Clone, PartialEq)]
pub struct MenuBar<A: 'static = String> {
	model: MenuModel<A>,
	on_activate: EventHandler<A>,
}

impl<A> MenuBar<A> {
	/// `model.entries` are the headers, each a [`Submenu`].
	pub fn new(model: MenuModel<A>, on_activate: impl Into<EventHandler<A>>) -> Self {
		Self { model, on_activate: on_activate.into() }
	}
}

impl<A: Clone + PartialEq + 'static> Component for MenuBar<A> {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let sink = use_a11y();
		let mut nav = use_state(MenuNav::default);
		let mut alt_pending = use_state(|| false);
		let mut alt_held = use_state(|| false);
		let mut previous_focus = use_state(|| None::<AccessibilityId>);
		let mut bar_area = use_state(Area::default);
		let panels = use_state(Vec::<PanelPlace>::new);
		let headers = self.model.entries.clone();
		let on_activate = self.on_activate.clone();

		let mut take_keys = move || {
			if previous_focus.peek().is_none() {
				let focused = *Platform::get().current_window().focused_accessibility_id.peek();
				previous_focus.set(Some(focused));
			}
			spawn(async move { sink.request_focus() });
		};
		let mut give_back = move || {
			restore_focus(previous_focus);
			previous_focus.set(None);
		};

		let on_key_down = {
			let headers = headers.clone();
			let on_activate = on_activate.clone();
			move |e: Event<KeyboardEventData>| {
				let mut n = nav.peek().clone();
				let outcome = n.handle_key(&headers, true, &e.key);
				if matches!(outcome, NavOutcome::Ignored) {
					return;
				}
				e.stop_propagation();
				e.prevent_default();
				nav.set(n);
				match outcome {
					NavOutcome::Activate(action) => {
						give_back();
						on_activate.call(action);
					}
					NavOutcome::Closed => give_back(),
					_ => {}
				}
			}
		};

		let on_global_key_down = {
			let headers = headers.clone();
			move |e: Event<KeyboardEventData>| {
				alt_held.set_if_modified(e.modifiers.contains(Modifiers::ALT) || e.key == Key::Named(NamedKey::Alt));
				if nav.peek().active {
					return;
				}
				match &e.key {
					Key::Named(NamedKey::Alt) => alt_pending.set(true),
					Key::Named(NamedKey::F10) if e.modifiers.is_empty() => {
						alt_pending.set(false);
						nav.write().activate_bar();
						take_keys();
					}
					Key::Character(c) if e.modifiers.contains(Modifiers::ALT) => {
						alt_pending.set(false);
						if let Some(letter) = c.chars().next()
							&& let MnemonicHit::Unique(i) = resolve_mnemonic(&headers, letter)
						{
							e.stop_propagation();
							nav.write().open(&headers, i, true);
							take_keys();
						}
					}
					_ => alt_pending.set_if_modified(false),
				}
			}
		};

		let on_global_key_up = move |e: Event<KeyboardEventData>| {
			alt_held.set_if_modified(e.modifiers.contains(Modifiers::ALT) && e.key != Key::Named(NamedKey::Alt));
			if e.key == Key::Named(NamedKey::Alt) && *alt_pending.peek() {
				alt_pending.set(false);
				if nav.peek().active {
					nav.write().close();
					give_back();
				} else {
					nav.write().activate_bar();
					take_keys();
				}
			}
		};

		let on_global_down = move |e: Event<PointerEventData>| {
			let n = nav.peek().clone();
			if !n.active {
				return;
			}
			let p = e.global_location();
			let bar = bar_area.peek().to_f64();
			let on_bar = p.x >= bar.min_x() && p.x < bar.max_x() && p.y >= bar.min_y() && p.y < bar.max_y();
			if on_bar || contains(&panels.peek(), p, n.stack.len()) {
				return;
			}
			nav.write().close();
			give_back();
		};

		let snapshot = nav.read().clone();
		let underline = snapshot.active && snapshot.keyboard || *alt_held.read();
		let header_views = headers.iter().enumerate().map(|(i, header)| {
			let open = snapshot.active && snapshot.header == i;
			let headers_c = headers.clone();
			let headers_h = headers.clone();
			let enabled = header.is_enabled();
			let ink = if enabled { theme.text } else { theme.text_disabled };
			let background = if open && !snapshot.stack.is_empty() {
				theme.menu_background
			} else if open {
				theme.menu_hover
			} else {
				Color::TRANSPARENT
			};
			let drop = (open && !snapshot.stack.is_empty())
				.then(|| panel_view(&theme, &headers, nav, 0, on_activate.clone(), panels))
				.flatten();
			let test_id = match header {
				MenuEntry::Submenu(s) => s.id.clone(),
				_ => String::new(),
			};
			rect()
				.height(Size::px(theme.row_height))
				.padding((0., 8., 0., 8.))
				.main_align(Alignment::center())
				.background(background)
				.test_id(test_id)
				.on_pointer_down(move |e: Event<PointerEventData>| {
					e.stop_propagation();
					if !enabled {
						return;
					}
					let mut n = nav.peek().clone();
					if n.active && n.header == i && !n.stack.is_empty() {
						n.close();
						nav.set(n);
						give_back();
					} else {
						n.open(&headers_c, i, false);
						nav.set(n);
						take_keys();
					}
				})
				.on_pointer_enter(move |_| {
					let n = nav.peek().clone();
					if n.active && !n.stack.is_empty() && n.header != i && enabled {
						let mut m = n;
						m.open(&headers_h, i, false);
						nav.set(m);
					}
				})
				.child(mnemonic_label(header.text().unwrap_or_default(), underline, ink))
				.maybe_child(drop.map(|d| {
					let shift = shift_inside(&panels.read(), 0);
					anchored(panels, 0, 0., theme.row_height, shift, d)
				}))
				.into_element()
		});

		rect()
			.width(Size::fill())
			.height(Size::px(theme.row_height))
			.horizontal()
			.background(theme.window_background)
			.color(theme.text)
			.font_size(theme.font_size)
			.a11y_id(sink)
			.a11y_focusable(snapshot.active)
			.a11y_role(AccessibilityRole::MenuBar)
			.on_sized(move |e: Event<SizedEventData>| bar_area.set_if_modified(e.area))
			.on_key_down(on_key_down)
			.on_global_key_down(on_global_key_down)
			.on_global_key_up(on_global_key_up)
			.on_global_pointer_down(on_global_down)
			.children(header_views)
	}
}

/// An open context menu: the model and where it opens, in window logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuRequest<A = String> {
	pub at: (f64, f64),
	pub model: MenuModel<A>,
}

/// Shows a context menu while `request` holds one; same keys as the menu bar's drop-downs. Place it once
/// near the root of the window.
#[derive(Clone, PartialEq)]
pub struct ContextMenuHost<A: 'static = String> {
	request: Writable<Option<ContextMenuRequest<A>>>,
	on_activate: EventHandler<A>,
}

impl<A: 'static> ContextMenuHost<A> {
	pub fn new(
		request: impl IntoWritable<Option<ContextMenuRequest<A>>>,
		on_activate: impl Into<EventHandler<A>>,
	) -> Self {
		Self { request: request.into_writable(), on_activate: on_activate.into() }
	}
}

impl<A: Clone + PartialEq + 'static> Component for ContextMenuHost<A> {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let sink = use_a11y();
		let mut nav = use_state(MenuNav::default);
		let mut previous_focus = use_state(|| None::<AccessibilityId>);
		let panels = use_state(Vec::<PanelPlace>::new);
		let mut shown = use_state(|| None::<ContextMenuRequest<A>>);
		let request = self.request.read().clone();
		let mut request_w = self.request.clone();

		if *shown.peek() != request {
			shown.set(request.clone());
			match &request {
				Some(r) => {
					let headers = vec![MenuEntry::Submenu(Submenu::new("", "", r.model.entries.clone()))];
					let mut n = MenuNav::default();
					n.open(&headers, 0, true);
					nav.set(n);
					let focused = *Platform::get().current_window().focused_accessibility_id.peek();
					previous_focus.set(Some(focused));
					spawn(async move { sink.request_focus() });
				}
				None => nav.set(MenuNav::default()),
			}
		}

		let Some(req) = request else {
			return rect().into_element();
		};
		let headers = vec![MenuEntry::Submenu(Submenu::new("", "", req.model.entries.clone()))];
		let on_activate = self.on_activate.clone();
		let close = {
			let mut request_w = request_w.clone();
			move || {
				*request_w.write() = None;
				restore_focus(previous_focus);
				previous_focus.set(None);
			}
		};
		let activate = {
			let on_activate = on_activate.clone();
			let mut close = close.clone();
			EventHandler::new(move |action: A| {
				close();
				on_activate.call(action);
			})
		};
		let on_key_down = {
			let headers = headers.clone();
			let activate = activate.clone();
			let mut close = close.clone();
			move |e: Event<KeyboardEventData>| {
				let mut n = nav.peek().clone();
				let outcome = n.handle_key(&headers, false, &e.key);
				if matches!(outcome, NavOutcome::Ignored) {
					return;
				}
				e.stop_propagation();
				e.prevent_default();
				nav.set(n);
				match outcome {
					NavOutcome::Activate(action) => activate.call(action),
					NavOutcome::Closed => close(),
					_ => {}
				}
			}
		};
		let on_global_down = {
			let mut close = close.clone();
			move |e: Event<PointerEventData>| {
				let levels = nav.peek().stack.len();
				if !contains(&panels.peek(), e.global_location(), levels) {
					close();
				}
			}
		};
		let _ = &mut request_w;
		rect()
			.layer(Layer::Overlay)
			.position(Position::new_global().left(req.at.0.round() as f32).top(req.at.1.round() as f32))
			.a11y_id(sink)
			.a11y_focusable(true)
			.on_key_down(on_key_down)
			.on_global_pointer_down(on_global_down)
			.maybe_child(panel_view(&theme, &headers, nav, 0, activate, panels).map(|p| {
				let shift = shift_inside(&panels.read(), 0);
				anchored(panels, 0, 0., 0., shift, p)
			}))
			.into_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bar() -> Vec<MenuEntry> {
		vec![
			Submenu::new(
				"file",
				"_File",
				vec![
					MenuItem::new("new", "_New Project").shortcut("Ctrl+N").into(),
					MenuItem::new("open", "_Open Project...").into(),
					Submenu::new("recent", "_Recent Projects", vec![MenuItem::new("r1", "a.wdproj").into()]).into(),
					MenuEntry::Separator,
					MenuItem::new("save", "_Save").enabled(false).into(),
					MenuItem::new("saveas", "Save _As...").into(),
					MenuItem::new("exit", "E_xit").into(),
				],
			)
			.into(),
			Submenu::new(
				"edit",
				"_Edit",
				vec![MenuItem::new("undo", "_Undo").into(), MenuItem::new("redo", "_Redo").into()],
			)
			.into(),
			Submenu::new(
				"view",
				"_View",
				vec![MenuItem::new("p", "_Panes").into(), MenuItem::new("t", "_Theme").into()],
			)
			.into(),
		]
	}

	#[test]
	fn mnemonic_parsing() {
		assert_eq!(parse_mnemonic("_File"), MnemonicText { text: "File".into(), mnemonic: Some(0) });
		assert_eq!(parse_mnemonic("E_xit"), MnemonicText { text: "Exit".into(), mnemonic: Some(1) });
		assert_eq!(parse_mnemonic("a__b_c"), MnemonicText { text: "a_bc".into(), mnemonic: Some(3) });
		assert_eq!(parse_mnemonic("plain"), MnemonicText { text: "plain".into(), mnemonic: None });
		assert_eq!(mnemonic_char("_\u{d6}ffnen"), Some('\u{f6}'));
	}

	#[test]
	fn mnemonic_resolution_skips_disabled_and_reports_clashes() {
		let MenuEntry::Submenu(file) = &bar()[0] else { panic!() };
		assert_eq!(resolve_mnemonic(&file.entries, 'X'), MnemonicHit::Unique(6));
		assert_eq!(resolve_mnemonic(&file.entries, 's'), MnemonicHit::None);
		assert_eq!(resolve_mnemonic(&file.entries, 'a'), MnemonicHit::Unique(5));
		let clash = vec![MenuItem::new("a", "_Cut").into(), MenuItem::new("b", "_Copy").into()];
		assert_eq!(resolve_mnemonic(&clash, 'c'), MnemonicHit::Ambiguous(vec![0, 1]));
		assert_eq!(duplicate_mnemonics(&clash), vec!['c']);
		assert!(duplicate_mnemonics(&file.entries).is_empty());
	}

	#[test]
	fn arrows_skip_separators_and_disabled_entries() {
		let MenuEntry::Submenu(file) = &bar()[0] else { panic!() };
		assert_eq!(next_enabled(&file.entries, Some(2), true), Some(5));
		assert_eq!(next_enabled(&file.entries, Some(5), false), Some(2));
		assert_eq!(next_enabled(&file.entries, Some(6), true), Some(0));
		assert_eq!(next_enabled(&file.entries, None, false), Some(6));
	}

	#[test]
	fn keyboard_walk_through_the_bar() {
		let b = bar();
		let mut nav = MenuNav::default();
		nav.activate_bar();
		assert_eq!(nav.handle_key(&b, true, &Key::Named(NamedKey::ArrowRight)), NavOutcome::Handled);
		assert_eq!(nav.header, 1);
		assert!(nav.stack.is_empty());
		nav.handle_key(&b, true, &Key::Named(NamedKey::ArrowDown));
		assert_eq!(nav.stack, vec![Some(0)]);
		nav.handle_key(&b, true, &Key::Named(NamedKey::ArrowLeft));
		assert_eq!((nav.header, nav.stack.clone()), (0, vec![Some(0)]));
		nav.handle_key(&b, true, &Key::Named(NamedKey::ArrowDown));
		nav.handle_key(&b, true, &Key::Named(NamedKey::ArrowDown));
		assert_eq!(nav.stack, vec![Some(2)]);
		nav.handle_key(&b, true, &Key::Named(NamedKey::ArrowRight));
		assert_eq!(nav.stack, vec![Some(2), Some(0)]);
		assert_eq!(nav.handle_key(&b, true, &Key::Named(NamedKey::Enter)), NavOutcome::Activate("r1".into()));
		assert!(!nav.active);
	}

	#[test]
	fn letters_activate_and_escape_steps_out() {
		let b = bar();
		let mut nav = MenuNav::default();
		nav.activate_bar();
		nav.handle_key(&b, true, &Key::Character("e".into()));
		assert_eq!((nav.header, nav.stack.clone()), (1, vec![Some(0)]));
		assert_eq!(nav.handle_key(&b, true, &Key::Character("r".into())), NavOutcome::Activate("redo".into()));
		nav.open(&b, 0, true);
		assert_eq!(nav.handle_key(&b, true, &Key::Named(NamedKey::Escape)), NavOutcome::Handled);
		assert!(nav.active && nav.stack.is_empty());
		assert_eq!(nav.handle_key(&b, true, &Key::Named(NamedKey::Escape)), NavOutcome::Closed);
		let ctx = vec![Submenu::new("", "", vec![MenuItem::new("cut", "Cu_t").into()]).into()];
		nav.open(&ctx, 0, true);
		assert_eq!(nav.handle_key(&ctx, false, &Key::Named(NamedKey::Escape)), NavOutcome::Closed);
		nav.open(&ctx, 0, true);
		assert_eq!(nav.handle_key(&ctx, false, &Key::Character("t".into())), NavOutcome::Activate("cut".into()));
	}

	#[test]
	fn hover_opens_submenus_and_closes_deeper_levels() {
		let b = bar();
		let mut nav = MenuNav::default();
		nav.open(&b, 0, false);
		assert_eq!(nav.stack, vec![None]);
		nav.hover(&b, 0, 2);
		assert_eq!(nav.stack, vec![Some(2), None]);
		nav.hover(&b, 0, 1);
		assert_eq!(nav.stack, vec![Some(1)]);
	}
}
