//! The menu bar of docs/ui/10 (Menu bar), drawn from [`MENU_BAR`] on the kit's menus: each entry a [`MenuAction`]
//! with its icon (or check mark) and its shortcut.

use egui_material_icons::MaterialIcon;
use waveduderust_app::App;
use waveduderust_app::menus::{self, Entry, Item, MENU_BAR, MenuAction, Needs, On};
use waveduderust_app::strings::Strings;
use waveduderust_app::window::Pane;
use waveduderust_egui_platform::icons::material;

use super::dock::Dock;
use super::shortcuts;
use crate::kit::ClassicUi;
use crate::kit::menu::{self as kit, MenuUi};

/// The menu bar, greyed and deaf unless `enabled`; the action of the entry picked this frame. `pastable` is read when
/// the Paste entry shows and forgotten when it does not; `has_last` enables File > Export last.
pub(super) fn show(
	ui: &mut ClassicUi<'_>,
	app: &App,
	dock: &Dock,
	pastable: &mut Option<bool>,
	has_last: bool,
	enabled: bool,
) -> Option<MenuAction> {
	let s = &app.strings;
	let headers: Vec<&str> = MENU_BAR.iter().map(|(key, _)| s.get(key)).collect();
	let mut menu = Menu { app, s, dock, pastable, paste_shown: false, has_last, picked: None };
	kit::bar(ui, &headers, enabled, |i, m| menu.entries(m, MENU_BAR[i].1));
	if !menu.paste_shown {
		*menu.pastable = None;
	}
	menu.picked
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
	/// The shown `entries`.
	fn entries(&mut self, m: &mut MenuUi<'_, '_>, entries: &[Entry]) {
		for entry in entries.iter().filter(|e| shows(e)) {
			match *entry {
				Entry::Separator => m.separator(),
				Entry::Item(item) => self.item(m, item),
				Entry::Submenu { key, icon, entries, .. } => {
					let row = kit::Entry::new(self.s.get(key)).icon(material(icon)).enabled(self.enabled(entry));
					m.submenu(row, |m| self.entries(m, entries));
				}
				Entry::Recent => {
					for (i, path) in self.app.settings.recent_files.iter().enumerate() {
						let text = path.display().to_string();
						self.recent(m, i, &text);
					}
				}
				Entry::Panes => {
					for pane in Pane::ALL {
						let row = kit::Entry::new(self.s.get(pane.title_key())).check(self.dock.is_shown(pane));
						if m.item(row).picked {
							self.picked = Some(MenuAction::Pane(pane));
						}
					}
				}
			}
		}
	}

	/// Recent file `i`, `text` its path; a right-click shows it in its folder where the system can.
	fn recent(&mut self, m: &mut MenuUi<'_, '_>, i: usize, text: &str) {
		let row = kit::Entry::plain(text);
		#[cfg(not(target_arch = "wasm32"))]
		let row = row.tip(self.s.get("RecentRevealHint"));
		let shown = m.item(row);
		if shown.picked {
			self.picked = Some(MenuAction::Recent(i));
		}
		#[cfg(not(target_arch = "wasm32"))]
		if shown.response.secondary_clicked() {
			waveduderust_app::platform::show_in_folder(&self.app.settings.recent_files[i]);
			m.close();
		}
	}

	/// `item`, its alternative's label while that holds; enabled as [`menus::enabled`] and its needs say.
	fn item(&mut self, m: &mut MenuUi<'_, '_>, item: Item) {
		let (key, icon) = match item.alt {
			Some(alt) if (alt.when)(self.app) => (alt.key, Some(alt.icon)),
			_ => (item.key, item.icon),
		};
		let icon: Option<MaterialIcon> = icon.map(material);
		let enabled = self.ready(item.needs) && menus::enabled(self.app, item.action, On::Current);
		let row = kit::Entry::new(self.s.get(key))
			.icon(icon)
			.shortcut(shortcuts::text(m.ctx(), item.action))
			.enabled(enabled);
		if m.item(row).picked {
			self.picked = Some(item.action);
		}
	}

	/// Whether the window holds what `needs` asks for.
	fn ready(&mut self, needs: Needs) -> bool {
		match needs {
			Needs::Nothing => true,
			Needs::Pastable => {
				self.paste_shown = true;
				*self.pastable.get_or_insert_with(waveduderust_egui_platform::pastable)
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
