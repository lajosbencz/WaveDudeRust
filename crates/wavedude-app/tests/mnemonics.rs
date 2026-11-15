//! The menus' mnemonics in every language, read off the window driven headless.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use eframe::egui::{self, Key};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use wavedude_app::ui::Shell;
use wavedude_app_state::Event;
use wavedude_app_state::history::History;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::{AppDirs, Theme};
use wavedude_app_state::strings::{Language, Strings, strip_mnemonic};

use common::{SETTLE, boot, settle};

/// The window on a returning user's settings under `dir`, in `language` and the light theme.
fn shell_with(dir: &Path, language: Language) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	// the empty project's seed is random: fixed for a stable Properties pane
	app.document.project.seed = 1;
	app.document.history = History::new(&app.document.project);
	app.live.project_replaced(&app.document.project, Instant::now());
	for event in [Event::SetLanguage(language.code().into()), Event::SetTheme(Theme::Light)] {
		app.handle(event, Instant::now());
	}
	let boot = boot(&dirs);
	let mut harness =
		Harness::builder().with_size(egui::vec2(1280., 800.)).build_eframe(move |cc| Shell::new(cc, app, boot));
	harness.run_steps(SETTLE);
	harness
}

/// The menus shown as the pointer opens every menu of the bar and each of their submenus: each menu's mnemonics,
/// with the entries' texts.
fn mnemonics(harness: &mut Harness<'_, Shell>, headers: &[String]) -> BTreeMap<u64, Vec<(String, String)>> {
	let mut menus: BTreeMap<u64, Vec<(String, String)>> = BTreeMap::new();
	let mut collect = |harness: &Harness<'_, Shell>| {
		for node in harness.query_all_by(|n| n.data().access_key().is_some()) {
			let node = node.accesskit_node();
			let mut menu = node.parent();
			while let Some(m) = menu.filter(|m| m.role() != egui::Role::Menu) {
				menu = m.parent();
			}
			let entries = menus.entry(menu.map_or(0, |m| m.locate().0.0)).or_default();
			let entry = (node.data().access_key().unwrap().to_owned(), node.label().unwrap_or_default());
			if !entries.contains(&entry) {
				entries.push(entry);
			}
		}
	};
	for header in headers {
		harness.get_by_role_and_label(egui::Role::Button, header).click();
		settle(harness);
		collect(harness);
		let submenus: Vec<String> = harness
			.query_all_by(|n| n.data().has_popup().is_some() && n.role() == egui::Role::MenuItem)
			.filter_map(|n| n.accesskit_node().label())
			.collect();
		for submenu in submenus {
			harness.get_by_role_and_label(egui::Role::MenuItem, &submenu).hover();
			settle(harness);
			collect(harness);
		}
		harness.key_press(Key::Escape);
		settle(harness);
	}
	menus
}

#[test]
fn every_menu_has_unique_mnemonics_in_every_language() {
	let mut shared = Vec::new();
	for language in Language::ALL {
		let dir = tempfile::tempdir().unwrap();
		let mut harness = shell_with(dir.path(), language);
		let s = Strings::new(language);
		let headers =
			["MenuFile", "MenuEdit", "MenuView", "MenuSimulation", "MenuHelp"].map(|k| strip_mnemonic(s.get(k)));
		let menus = mnemonics(&mut harness, &headers);
		assert!(menus.len() > headers.len(), "{language:?}: the submenus did not open");
		for entries in menus.values() {
			for (i, (letter, text)) in entries.iter().enumerate() {
				if let Some((_, other)) = entries[i + 1..].iter().find(|(l, _)| l == letter) {
					shared.push(format!("{language:?} '{letter}': {text} / {other}"));
				}
			}
		}
	}
	assert!(shared.is_empty(), "mnemonics shared within a menu: {shared:#?}");
}
