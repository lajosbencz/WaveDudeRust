//! Keyboard access to the menus of docs/ui/10 (Conventions, Global keys) and docs/ui/00 (Keyboard first): the
//! mnemonics the string tables mark with `_` are underlined while Alt is held or keys work the menus. Alt+letter
//! opens a menu of the bar; F10 or a lone Alt enters the bar, where Left and Right walk the headers and Down, Enter
//! or a letter opens one; in an open menu a letter picks its entry (several with one letter take the focus in
//! turn), Home and End reach its ends, Left and Right open the neighbouring menu of the bar and Esc steps out one
//! level. Up, Down, Enter and Right into a submenu are egui's own focus walk of a menu. Leaving the menus gives the
//! focus back to where the keys took it from.
//!
//! The menus record their headers, entries and open popups in every pass; [`MenuKeys`] acts on the keys before
//! egui sees them, on the record of the pass before.

use std::mem;

use eframe::egui::os::OperatingSystem;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
	self, Button, Color32, Context, Event, Id, Key, Modifiers, Popup, RawInput, Response, Role, Sense, Stroke,
	TextStyle, Ui, WidgetInfo, WidgetText, accesskit,
};
use wavedude_app_state::strings::{mnemonic, split_mnemonic};

/// An enabled entry of an open menu, or a header of the bar.
#[derive(Clone, Copy, Debug)]
struct Entry {
	id: Id,
	/// The popup it is in; a header's own popup.
	menu: Id,
	mnemonic: Option<char>,
	submenu: bool,
}

/// What the menus showed in one pass.
#[derive(Default)]
struct Record {
	headers: Vec<Entry>,
	/// The open menus: their popup and the widget it hangs from.
	menus: Vec<(Id, Id)>,
	entries: Vec<Entry>,
}

/// The open menu keys go to.
struct Target {
	entries: Vec<Entry>,
	focused: Option<usize>,
	/// A submenu, not a menu of the bar or a pane's.
	nested: bool,
	/// The bar's header the menu hangs from, through its parents.
	header: Option<usize>,
}

impl Record {
	fn holds(&self, id: Id) -> bool {
		self.headers.iter().chain(&self.entries).any(|e| e.id == id)
	}

	/// The menu holding the entry `menu` hangs from.
	fn parent(&self, menu: Id) -> Option<Id> {
		let anchor = self.menus.iter().find(|m| m.0 == menu)?.1;
		self.entries.iter().find(|e| e.id == anchor).map(|e| e.menu)
	}

	/// The menu holding the focus, else the deepest open one.
	fn target(&self, focused: Option<Id>) -> Option<Target> {
		let menu = focused.and_then(|f| self.entries.iter().find(|e| e.id == f)).map(|e| e.menu).or_else(|| {
			self.menus.iter().map(|m| m.0).find(|&m| !self.menus.iter().any(|o| self.parent(o.0) == Some(m)))
		})?;
		let mut root = menu;
		while let Some(parent) = self.parent(root) {
			root = parent;
		}
		let anchor = self.menus.iter().find(|m| m.0 == root).map(|m| m.1);
		let entries: Vec<Entry> = self.entries.iter().filter(|e| e.menu == menu).copied().collect();
		Some(Target {
			focused: entries.iter().position(|e| Some(e.id) == focused),
			entries,
			nested: root != menu,
			header: self.headers.iter().position(|h| Some(h.id) == anchor),
		})
	}
}

/// The entries of `entries` whose mnemonic is `letter`.
fn hits(entries: &[Entry], letter: char) -> Vec<usize> {
	entries.iter().enumerate().filter(|(_, e)| e.mnemonic == Some(letter)).map(|(i, _)| i).collect()
}

/// The hit after `current`, wrapping.
fn next_hit(hits: &[usize], current: Option<usize>) -> usize {
	hits.iter().copied().find(|&h| Some(h) > current).unwrap_or(hits[0])
}

/// The neighbour of `i` among `n`, wrapping.
fn step(i: usize, n: usize, forward: bool) -> usize {
	if forward { (i + 1) % n } else { (i + n - 1) % n }
}

/// The letter or digit a key types, lower-cased.
pub(super) fn letter_of(key: Key) -> Option<char> {
	let mut chars = key.name().chars();
	match (chars.next(), chars.next()) {
		(Some(c), None) if c.is_alphanumeric() => Some(c.to_ascii_lowercase()),
		_ => None,
	}
}

fn focused(ctx: &Context) -> Option<Id> {
	ctx.memory(egui::Memory::focused)
}

fn focus(ctx: &Context, id: Id) {
	ctx.memory_mut(|m| m.request_focus(id));
}

/// What becomes of a key event.
enum Fate {
	Keep,
	Drop,
	/// Replaced by a press of this key.
	Press(Key),
}

/// The plugin that works the menus with the keys; the menus record into it through this module's functions.
#[derive(Default)]
pub(super) struct MenuKeys {
	last: Record,
	record: Record,
	/// Alt is held; `alone` while nothing else came with it.
	alt: bool,
	alone: bool,
	/// Keys work the menus: the mnemonics show.
	keys: bool,
	/// The focus before keys took the menus.
	previous: Option<Option<Id>>,
	/// A menu keys opened, and whether its last entry, else its first, takes the focus once it is shown.
	opened: Option<(Id, bool)>,
}

impl MenuKeys {
	fn header_of(&self, focused: Option<Id>) -> Option<usize> {
		self.last.headers.iter().position(|h| Some(h.id) == focused)
	}

	fn engaged(&self, focused: Option<Id>) -> bool {
		!self.last.menus.is_empty() || self.header_of(focused).is_some()
	}

	/// Remembers the focus the keys take, unless it is the menus' own.
	fn take_focus(&mut self, focused: Option<Id>) {
		self.keys = true;
		if self.previous.is_none() && !focused.is_some_and(|f| self.last.holds(f)) {
			self.previous = Some(focused);
		}
	}

	/// F10 or a lone Alt: the first header takes the focus.
	fn enter(&mut self, ctx: &Context, focused: Option<Id>) {
		if let Some(first) = self.last.headers.first() {
			let first = first.id;
			self.take_focus(focused);
			focus(ctx, first);
		}
	}

	/// Opens header `h`'s menu.
	fn open(&mut self, ctx: &Context, h: usize, last: bool, focused: Option<Id>) {
		let header = self.last.headers[h];
		self.take_focus(focused);
		Popup::open_id(ctx, header.menu);
		focus(ctx, header.id);
		self.opened = Some((header.menu, last));
	}

	/// Closes the menus and gives the focus back.
	fn leave(&mut self, ctx: &Context, focused: Option<Id>) {
		Popup::close_all(ctx);
		self.keys = false;
		match self.previous.take().flatten() {
			Some(id) => focus(ctx, id),
			None => {
				if let Some(f) = focused.filter(|&f| self.last.holds(f)) {
					ctx.memory_mut(|m| m.surrender_focus(f));
				}
			}
		}
	}

	fn modifiers(&mut self, ctx: &Context, modifiers: Modifiers) {
		if modifiers == Modifiers::ALT && !self.alt {
			self.alone = true;
		} else if !modifiers.alt && mem::take(&mut self.alone) {
			let focused = focused(ctx);
			if self.engaged(focused) { self.leave(ctx, focused) } else { self.enter(ctx, focused) }
		} else if modifiers != Modifiers::ALT {
			self.alone = false;
		}
		self.alt = modifiers.alt;
	}

	fn key(&mut self, ctx: &Context, key: Key, modifiers: Modifiers, letter: Option<char>) -> Fate {
		if modifiers.ctrl || modifiers.command || modifiers.mac_cmd {
			return Fate::Keep;
		}
		let focused = focused(ctx);
		if let Some(fate) = self.engaged_key(ctx, Some(key), letter, focused) {
			return fate;
		}
		if key == Key::F10 && modifiers.is_none() {
			self.enter(ctx, focused);
			return Fate::Drop;
		}
		// Option+letter types a character on macOS.
		if modifiers.alt
			&& ctx.os() != OperatingSystem::Mac
			&& let Some(c) = letter
			&& let [h] = hits(&self.last.headers, c)[..]
		{
			self.open(ctx, h, false, focused);
			return Fate::Drop;
		}
		Fate::Keep
	}

	/// A key, or a letter typed without one, while an open menu or a focused header takes the keys.
	fn engaged_key(
		&mut self,
		ctx: &Context,
		key: Option<Key>,
		letter: Option<char>,
		focused: Option<Id>,
	) -> Option<Fate> {
		if let Some(target) = self.last.target(focused) {
			self.keys = true;
			Some(self.in_menu(ctx, &target, key, letter, focused))
		} else if let Some(h) = self.header_of(focused) {
			self.keys = true;
			Some(self.in_bar(ctx, h, key, letter, focused))
		} else {
			None
		}
	}

	fn in_bar(&mut self, ctx: &Context, h: usize, key: Option<Key>, letter: Option<char>, focused: Option<Id>) -> Fate {
		let headers = &self.last.headers;
		match key {
			Some(k @ (Key::ArrowLeft | Key::ArrowRight)) => {
				focus(ctx, headers[step(h, headers.len(), k == Key::ArrowRight)].id);
			}
			Some(Key::ArrowDown | Key::Enter | Key::Space) => self.open(ctx, h, false, focused),
			Some(Key::ArrowUp) => self.open(ctx, h, true, focused),
			Some(Key::Escape) => self.leave(ctx, focused),
			Some(Key::Tab | Key::Home | Key::End) => {}
			_ => match letter.map(|c| hits(headers, c)) {
				None => return Fate::Keep,
				Some(hits) => match hits[..] {
					[] => {}
					[i] => self.open(ctx, i, false, focused),
					_ => focus(ctx, headers[next_hit(&hits, Some(h))].id),
				},
			},
		}
		Fate::Drop
	}

	fn in_menu(
		&mut self,
		ctx: &Context,
		t: &Target,
		key: Option<Key>,
		letter: Option<char>,
		focused: Option<Id>,
	) -> Fate {
		let on_submenu = t.focused.is_some_and(|i| t.entries[i].submenu);
		match key {
			Some(Key::ArrowUp | Key::ArrowDown | Key::Enter | Key::Space) => return Fate::Keep,
			Some(Key::ArrowRight) if on_submenu => return Fate::Keep,
			Some(Key::ArrowLeft) if t.nested => return Fate::Keep,
			Some(Key::Escape) if t.nested => return Fate::Press(Key::ArrowLeft),
			Some(k @ (Key::ArrowLeft | Key::ArrowRight)) => {
				if let Some(h) = t.header {
					Popup::close_all(ctx);
					self.open(ctx, step(h, self.last.headers.len(), k == Key::ArrowRight), false, focused);
				}
			}
			Some(Key::Escape) => {
				Popup::close_all(ctx);
				if let Some(h) = t.header {
					focus(ctx, self.last.headers[h].id);
				}
			}
			Some(k @ (Key::Home | Key::End)) => {
				if let Some(e) = if k == Key::Home { t.entries.first() } else { t.entries.last() } {
					focus(ctx, e.id);
				}
			}
			Some(Key::Tab) => {}
			_ => match letter.map(|c| hits(&t.entries, c)) {
				None => return Fate::Keep,
				Some(hits) => match hits[..] {
					[] => {}
					[i] => {
						focus(ctx, t.entries[i].id);
						return Fate::Press(Key::Enter);
					}
					_ => focus(ctx, t.entries[next_hit(&hits, t.focused)].id),
				},
			},
		}
		Fate::Drop
	}
}

impl egui::Plugin for MenuKeys {
	fn debug_name(&self) -> &'static str {
		"menu_keys"
	}

	fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
		self.last = mem::take(&mut self.record);
		// A key that stood for a letter takes the letter's text with it.
		let mut lettered = false;
		let events = mem::take(&mut input.events);
		for event in events {
			let fate = match &event {
				Event::ModifiersChanged(modifiers) => {
					self.modifiers(ctx, *modifiers);
					Fate::Keep
				}
				Event::WindowFocused(false) => {
					(self.alt, self.alone) = (false, false);
					Fate::Keep
				}
				Event::PointerButton { pressed: true, .. } => {
					(self.alone, self.keys, self.previous) = (false, false, None);
					Fate::Keep
				}
				Event::Key { key, pressed: true, modifiers, .. } => {
					self.alone = false;
					let letter = letter_of(*key);
					let fate = self.key(ctx, *key, *modifiers, letter);
					lettered = letter.is_some() && !matches!(fate, Fate::Keep);
					fate
				}
				// A letter no key event stood for, such as one with an accent.
				Event::Text(text) => {
					let focused = focused(ctx);
					let letter = text.chars().next().and_then(|c| c.to_lowercase().next());
					if mem::take(&mut lettered) || self.engaged_key(ctx, None, letter, focused).is_some() {
						Fate::Drop
					} else {
						Fate::Keep
					}
				}
				_ => Fate::Keep,
			};
			match fate {
				Fate::Keep => input.events.push(event),
				Fate::Drop => {}
				Fate::Press(key) => input.events.push(Event::Key {
					key,
					physical_key: None,
					pressed: true,
					repeat: false,
					modifiers: Modifiers::NONE,
				}),
			}
		}
	}

	fn on_end_pass(&mut self, ui: &mut Ui) {
		let ctx = ui.ctx();
		// A popup's first pass only sizes it: its entries are disabled until the next.
		if let Some((popup, last)) = self.opened {
			let mut entries = self.record.entries.iter().filter(|e| e.menu == popup);
			if let Some(entry) = if last { entries.next_back() } else { entries.next() } {
				focus(ctx, entry.id);
				self.opened = None;
			} else if !Popup::is_id_open(ctx, popup) {
				self.opened = None;
			}
		}
		let focused = focused(ctx);
		let on_header = self.record.headers.iter().any(|h| Some(h.id) == focused);
		if self.record.menus.is_empty() && !on_header {
			self.keys = false;
			if let Some(previous) = self.previous.take()
				&& focused.is_none_or(|f| self.last.holds(f) || self.record.holds(f))
			{
				match previous {
					Some(id) => focus(ctx, id),
					None => {
						if let Some(f) = focused {
							ctx.memory_mut(|m| m.surrender_focus(f));
						}
					}
				}
			}
		}
	}
}

/// Runs `f` on the plugin, which the window adds at its start.
fn with<R>(ctx: &Context, f: impl FnOnce(&mut MenuKeys) -> R) -> Option<R> {
	ctx.with_plugin(f)
}

/// Gives a widget its mnemonic for assistive technology, and says whether it opens a menu.
fn describe(response: &Response, letter: Option<char>, opens_menu: bool) {
	response.ctx.accesskit_node_builder(response.id, |node| {
		if let Some(c) = letter {
			node.set_access_key(c.to_string());
		}
		if opens_menu {
			node.set_has_popup(accesskit::HasPopup::Menu);
		}
	});
}

/// How a header of the bar senses: it takes the focus only while keys work the menus, which keeps the bar out of
/// the Tab cycle.
pub(super) fn header_sense(ctx: &Context) -> Sense {
	if with(ctx, |k| k.keys).unwrap_or(false) { Sense::click() } else { Sense::CLICK }
}

/// A menu text with its mnemonic underlined while the mnemonics show, and its plain text.
pub(super) fn label(ui: &Ui, marked: &str) -> (WidgetText, String) {
	let (text, at) = split_mnemonic(marked);
	let shown = with(ui.ctx(), |k| k.alt || k.keys).unwrap_or(false);
	let Some(at) = at.filter(|_| shown) else { return (text.clone().into(), text) };
	let end = at + text[at..].chars().next().map_or(0, char::len_utf8);
	let plain = TextFormat::simple(TextStyle::Button.resolve(ui.style()), Color32::PLACEHOLDER);
	let underlined = TextFormat { underline: Stroke::new(1., Color32::PLACEHOLDER), ..plain.clone() };
	let mut job = LayoutJob::default();
	job.append(&text[..at], 0., plain.clone());
	job.append(&text[at..end], 0., underlined);
	job.append(&text[end..], 0., plain);
	(job.into(), text)
}

/// Records a header of the bar, `marked` its text, and its menu while `shown`; a disabled bar takes no keys.
pub(super) fn header(response: &Response, marked: &str, shown: bool) {
	let letter = mnemonic(marked);
	describe(response, letter, true);
	if !response.enabled() {
		return;
	}
	let popup = Popup::default_response_id(response);
	with(&response.ctx, |k| {
		k.record.headers.push(Entry { id: response.id, menu: popup, mnemonic: letter, submenu: true });
		if shown {
			k.record.menus.push((popup, response.id));
		}
	});
}

/// Records an entry of the menu `ui` is in; `marked` is its text.
pub(super) fn entry(ui: &Ui, response: &Response, marked: &str, submenu: bool) {
	let letter = mnemonic(marked);
	describe(response, letter, submenu);
	if response.enabled() {
		let entry = Entry { id: response.id, menu: ui.layer_id().id, mnemonic: letter, submenu };
		with(ui.ctx(), |k| k.record.entries.push(entry));
	}
}

/// Records a menu shown this pass: its popup and the widget it hangs from.
pub(super) fn menu(ctx: &Context, popup: Id, anchor: Id) {
	with(ctx, |k| k.record.menus.push((popup, anchor)));
}

/// A plain entry of a menu, `marked` its text; clicking it closes the menu.
pub(super) fn item(ui: &mut Ui, marked: &str) -> Response {
	let (label, name) = label(ui, marked);
	let response = ui.add(Button::new(label));
	let enabled = response.enabled();
	response.widget_info(|| WidgetInfo::labeled(Role::MenuItem, enabled, &name));
	entry(ui, &response, marked, false);
	if response.clicked() {
		ui.close();
	}
	response
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(id: &str, menu: &str, mnemonic: char, submenu: bool) -> Entry {
		Entry { id: Id::unique(id), menu: Id::unique(menu), mnemonic: Some(mnemonic), submenu }
	}

	#[test]
	fn the_keys_go_to_the_focused_menu_else_the_deepest() {
		let record = Record {
			headers: vec![entry("view", "view menu", 'v', true)],
			menus: vec![(Id::unique("view menu"), Id::unique("view")), (Id::unique("theme menu"), Id::unique("theme"))],
			entries: vec![
				entry("panes", "view menu", 'p', true),
				entry("theme", "view menu", 't', true),
				entry("dark", "theme menu", 'd', false),
			],
		};
		let deepest = record.target(None).unwrap();
		assert!(deepest.nested && deepest.header == Some(0) && deepest.focused.is_none());
		assert_eq!(deepest.entries.len(), 1);
		let focused = record.target(Some(Id::unique("theme"))).unwrap();
		assert!(!focused.nested && focused.focused == Some(1));
		assert_eq!(hits(&focused.entries, 't'), [1]);
	}

	#[test]
	fn a_shared_letter_takes_its_entries_in_turn() {
		assert_eq!(next_hit(&[1, 4], None), 1);
		assert_eq!(next_hit(&[1, 4], Some(1)), 4);
		assert_eq!(next_hit(&[1, 4], Some(4)), 1);
		assert_eq!((step(0, 5, false), step(4, 5, true)), (4, 0));
		assert_eq!((letter_of(Key::F), letter_of(Key::Num1), letter_of(Key::F1)), (Some('f'), Some('1'), None));
	}
}
