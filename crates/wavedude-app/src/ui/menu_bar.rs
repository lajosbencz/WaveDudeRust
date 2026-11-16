//! The menu bar of docs/ui/10 (Menu bar), drawn from [`MENU_BAR`]: each entry a [`MenuAction`] with its icon (or
//! check mark) in a 24 px column and its shortcut on the right.

use eframe::egui::containers::menu::{MenuButton, SubMenu, SubMenuButton};
use eframe::egui::{self, Atom, AtomExt, Button, IdSalt, Rect, Role, Vec2, WidgetInfo};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::ICON_CHECK;
use wavedude_app_state::strings::Strings;

use super::dock::Dock;
use super::{icons, menu_keys, shortcuts};
use crate::App;
use crate::menus::{self, Entry, Item, MENU_BAR, MenuAction, Needs, On};
use crate::window::Pane;

const ICON_SLOT: f32 = 24.;
const ICON_SIZE: f32 = 16.;
const ENTRY_HEIGHT: f32 = 22.;
const MIN_PANEL_WIDTH: f32 = 200.;

/// The menu bar; the action of the entry clicked this frame. `pastable` is read when the Paste entry shows and
/// forgotten when it does not; `has_last` enables File > Export last.
pub(super) fn show(
	ui: &mut egui::Ui,
	app: &App,
	dock: &Dock,
	pastable: &mut Option<bool>,
	has_last: bool,
) -> Option<MenuAction> {
	let mut menu = Menu { app, s: &app.strings, dock, pastable, paste_shown: false, has_last, picked: None };
	egui::MenuBar::new().ui(ui, |ui| {
		for (key, entries) in MENU_BAR {
			menu.top(ui, key, entries);
		}
	});
	if !menu.paste_shown {
		*menu.pastable = None;
	}
	menu.picked
}

/// What an entry shows in its icon column.
#[derive(Clone, Copy)]
pub(super) enum Slot {
	Empty,
	Icon(MaterialIcon),
	Drawn(icons::Icon),
}

impl From<Option<MaterialIcon>> for Slot {
	fn from(icon: Option<MaterialIcon>) -> Self {
		icon.map_or(Slot::Empty, Slot::Icon)
	}
}

/// The id of a drawn icon's room in its entry.
fn drawn_id() -> IdSalt {
	IdSalt::new("drawn_icon")
}

/// Where an entry's icon goes: the icon, or room for a drawn one or for nothing in the column's width.
fn slot(icon: Slot) -> Atom<'static> {
	let size = Vec2::new(ICON_SLOT, ICON_SIZE);
	match icon {
		Slot::Icon(icon) => icon.rich_text().size(ICON_SIZE).atom_size(size),
		Slot::Drawn(_) => Atom::custom(drawn_id(), size),
		Slot::Empty => Atom::default().atom_size(size),
	}
}

/// Adds the entry `button` with `icon` in its column.
fn add_entry(ui: &mut egui::Ui, button: Button<'_>, icon: Slot) -> egui::Response {
	let shown = button.atom_ui(ui);
	if let (Slot::Drawn(drawn), Some(rect)) = (icon, shown.rect(drawn_id())) {
		let color = ui.style().interact(&shown.response).fg_stroke.color;
		icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(ICON_SIZE)), drawn, color);
	}
	shown.response
}

/// Names an entry by its text alone for assistive technology, not by its icon glyph and shortcut.
fn named(response: &egui::Response, text: &str, checked: Option<bool>) {
	let enabled = response.enabled();
	response.widget_info(|| match checked {
		Some(on) => WidgetInfo::selected(Role::MenuItemCheckBox, enabled, on, text),
		None => WidgetInfo::labeled(Role::MenuItem, enabled, text),
	});
}

/// A menu entry, `marked` its text with its mnemonic, `icon` in its column, `checked` its check mark's state for
/// one that has it; clicking it closes the menu.
pub(super) fn entry(
	ui: &mut egui::Ui,
	icon: impl Into<Slot>,
	marked: &str,
	checked: Option<bool>,
	shortcut: Option<String>,
) -> egui::Response {
	let icon = icon.into();
	let (label, text) = menu_keys::label(ui, marked);
	let mut button = Button::new((slot(icon), label)).min_size(Vec2::new(0., ENTRY_HEIGHT));
	if let Some(shortcut) = shortcut {
		button = button.shortcut_text(shortcut);
	}
	let response = add_entry(ui, button, icon);
	named(&response, &text, checked);
	menu_keys::entry(ui, &response, marked, false);
	if response.clicked() {
		ui.close();
	}
	response
}

/// A drop-down entry's swatch: its size and what paints it.
pub(super) type Swatch<'a> = (Vec2, &'a dyn Fn(&egui::Ui, Rect));

/// An entry of a drop-down: `text` as it is (no mnemonic), its check mark while `checked`, and before the text a
/// `size` room that `paint` draws into, if any; `italic` for an action. Clicking it closes the menu.
pub(super) fn choice_entry(
	ui: &mut egui::Ui,
	text: &str,
	checked: bool,
	italic: bool,
	swatch: Option<Swatch<'_>>,
) -> egui::Response {
	let room = IdSalt::new("choice_swatch");
	let icon: Slot = checked.then_some(ICON_CHECK).into();
	let word = if italic { egui::RichText::new(text).italics() } else { egui::RichText::new(text) };
	let mut atoms = egui::Atoms::new(slot(icon));
	if let Some((size, _)) = swatch {
		atoms.push_right(Atom::custom(room, size));
	}
	atoms.push_right(word);
	let shown = Button::new(atoms).min_size(Vec2::new(0., ENTRY_HEIGHT)).atom_ui(ui);
	if let (Some((_, paint)), Some(rect)) = (swatch, shown.rect(room)) {
		paint(ui, rect);
	}
	let response = shown.response;
	named(&response, text, Some(checked));
	menu_keys::entry(ui, &response, "", false);
	if response.clicked() {
		ui.close();
	}
	response
}

/// An entry with a check mark (or radio dot) while `on`, outside the menu bar.
pub(super) fn check_entry(ui: &mut egui::Ui, marked: &str, on: bool) -> egui::Response {
	entry(ui, on.then_some(ICON_CHECK), marked, Some(on), None)
}

/// A submenu's entry, `marked` its text with its mnemonic, `icon` in its column; `content` fills the submenu.
pub(super) fn submenu(
	ui: &mut egui::Ui,
	icon: Slot,
	marked: &str,
	enabled: bool,
	content: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
	ui.add_enabled_ui(enabled, |ui| {
		let (label, text) = menu_keys::label(ui, marked);
		let button = Button::new((slot(icon), label))
			.min_size(Vec2::new(0., ENTRY_HEIGHT))
			.right_text(SubMenuButton::arrow_atom(None));
		let (response, shown) = SubMenuButton::from_button(button).ui(ui, |ui| {
			ui.set_min_width(MIN_PANEL_WIDTH);
			content(ui);
		});
		if let Slot::Drawn(drawn) = icon {
			let x = response.rect.left() + ui.spacing().button_padding.x + ICON_SLOT / 2.;
			let room = Rect::from_center_size(egui::pos2(x, response.rect.center().y), Vec2::splat(ICON_SIZE));
			icons::paint(ui, room, drawn, ui.style().interact(&response).fg_stroke.color);
		}
		named(&response, &text, None);
		menu_keys::entry(ui, &response, marked, true);
		if shown.is_some() {
			menu_keys::menu(ui.ctx(), SubMenu::id_from_widget_id(response.id), response.id);
		}
		response
	})
	.inner
}

/// Whether this build shows `entry`.
fn shows(entry: &Entry) -> bool {
	match entry {
		Entry::Item(item) => item.show.now(),
		Entry::Submenu { show, .. } => show.now(),
		Entry::Separator | Entry::Recent | Entry::Panes => true,
	}
}

struct Menu<'a> {
	app: &'a App,
	s: &'a Strings,
	dock: &'a Dock,
	pastable: &'a mut Option<bool>,
	paste_shown: bool,
	has_last: bool,
	picked: Option<MenuAction>,
}

impl Menu<'_> {
	fn top(&mut self, ui: &mut egui::Ui, key: &str, entries: &[Entry]) {
		let marked = self.s.get(key);
		let button = Button::new(menu_keys::label(ui, marked).0).sense(menu_keys::header_sense(ui.ctx()));
		let (response, shown) = MenuButton::from_button(button).ui(ui, |ui| {
			ui.set_min_width(MIN_PANEL_WIDTH);
			self.entries(ui, entries);
		});
		menu_keys::header(&response, marked, shown.is_some());
	}

	/// The shown `entries`, without a separator at either end or after another.
	fn entries(&mut self, ui: &mut egui::Ui, entries: &[Entry]) {
		let shown: Vec<&Entry> = entries.iter().filter(|e| shows(e)).collect();
		let mut after_entry = false;
		for (i, entry) in shown.iter().enumerate() {
			match **entry {
				Entry::Separator => {
					if after_entry && shown.get(i + 1).is_some_and(|e| !matches!(e, Entry::Separator)) {
						ui.separator();
					}
					after_entry = false;
					continue;
				}
				Entry::Item(item) => self.item(ui, item),
				Entry::Submenu { key, icon, entries, .. } => {
					let enabled = self.enabled(entry);
					submenu(ui, Slot::Icon(icon), self.s.get(key), enabled, |ui| self.entries(ui, entries));
				}
				Entry::Recent => {
					for (i, path) in self.app.settings.recent_files.iter().enumerate() {
						// A path has no mnemonic: its underscores are literal.
						let text = path.display().to_string().replace('_', "__");
						self.button(ui, MenuAction::Recent(i), None, &text, None);
					}
				}
				Entry::Panes => {
					let s = self.s;
					for pane in Pane::ALL {
						let on = self.dock.is_shown(pane);
						self.button(
							ui,
							MenuAction::Pane(pane),
							on.then_some(ICON_CHECK),
							s.get(pane.title_key()),
							Some(on),
						);
					}
				}
			}
			after_entry = true;
		}
	}

	/// An entry, `marked` its text with its mnemonic; `checked` is its check mark's state, for one that has it.
	fn button(
		&mut self,
		ui: &mut egui::Ui,
		action: MenuAction,
		icon: Option<MaterialIcon>,
		marked: &str,
		checked: Option<bool>,
	) {
		if entry(ui, icon, marked, checked, shortcuts::text(ui.ctx(), action)).clicked() {
			self.picked = Some(action);
		}
	}

	/// `item`, its alternative's label while that holds; enabled as [`menus::enabled`] and its needs say.
	fn item(&mut self, ui: &mut egui::Ui, item: Item) {
		let (key, icon) = match item.alt {
			Some(alt) if (alt.when)(self.app) => (alt.key, Some(alt.icon)),
			_ => (item.key, item.icon),
		};
		let enabled = self.ready(item.needs) && menus::enabled(self.app, item.action, On::Current);
		let marked = self.s.get(key);
		ui.add_enabled_ui(enabled, |ui| self.button(ui, item.action, icon, marked, None));
	}

	/// Whether the window holds what `needs` asks for.
	fn ready(&mut self, needs: Needs) -> bool {
		match needs {
			Needs::Nothing => true,
			Needs::Pastable => {
				self.paste_shown = true;
				*self.pastable.get_or_insert_with(super::platform::pastable)
			}
			Needs::LastExport => self.has_last,
		}
	}

	/// Whether `entry` can be picked as [`menus::enabled`] says, a submenu while one of its entries can.
	fn enabled(&self, entry: &Entry) -> bool {
		match entry {
			Entry::Item(item) => menus::enabled(self.app, item.action, On::Current),
			Entry::Submenu { entries, .. } => entries.iter().any(|e| self.enabled(e)),
			Entry::Recent | Entry::Panes => true,
			Entry::Separator => false,
		}
	}
}
