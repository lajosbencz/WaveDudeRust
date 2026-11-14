//! The window's shell driven headless: the menu bar and its keys, a shortcut, the theme and the panes with their
//! drops.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use wavedude_app::ui::{Boot, Shell};
use wavedude_app_state::document::Source;
use wavedude_app_state::history::History;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::selection::Row;
use wavedude_app_state::settings::{AppDirs, Theme};
use wavedude_app_state::strings::{Language, Strings, strip_mnemonic};
use wavedude_app_state::{AppState, Event, Guarded};
use wavedude_core::model::{ItemKind, ItemRef};

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj";
/// Frames for a click or a key to reach the state and its result to show.
const SETTLE: usize = 4;

/// The window on a returning user's settings under `dir`, in `language` and the light theme, with `project` open.
fn shell_with(dir: &Path, language: Language, project: Option<&str>) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	// the empty project's seed is random: fixed for a stable Properties pane
	app.document.project.seed = 1;
	app.document.history = History::new(&app.document.project);
	app.live.project_replaced(&app.document.project, Instant::now());
	for event in [Event::SetLanguage(language.code().into()), Event::SetTheme(Theme::Light)] {
		app.handle(event, Instant::now());
	}
	if let Some(path) = project {
		app.handle(Event::Guarded(Guarded::Open(Source::Path(path.into()))), Instant::now());
	}
	let boot = Boot {
		store: dirs.store(),
		gpu: None,
		open: Default::default(),
		log: LogRing::new(),
		update: Default::default(),
	};
	let mut harness =
		Harness::builder().with_size(egui::vec2(1280., 800.)).build_eframe(move |cc| Shell::new(cc, app, boot));
	harness.run_steps(SETTLE);
	harness
}

fn shell(dir: &Path, project: Option<&str>) -> Harness<'static, Shell> {
	shell_with(dir, Language::English, project)
}

fn settle(harness: &mut Harness<'_, Shell>) {
	harness.run_steps(SETTLE);
}

#[test]
fn the_file_menu_opens_with_its_entries() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	harness.get_by_label("File").click();
	settle(&mut harness);
	for entry in ["New Project", "Open Project...", "Save", "Save As...", "Exit"] {
		harness.get_by_label(entry);
	}
}

#[test]
fn an_idle_window_asks_for_no_repaint() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	common::preview_idle(&mut harness);
	harness.run_steps(1);
	let delay = harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
	assert_eq!(delay, std::time::Duration::MAX, "nothing moves, yet the window ticks");
}

#[test]
fn select_all_by_its_shortcut_enables_cut() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	let cut_enabled = |harness: &mut Harness<'_, Shell>| {
		harness.get_by_label("Edit").click();
		settle(harness);
		let enabled = !harness.get_by_label("Cut").accesskit_node().is_disabled();
		harness.key_press(Key::Escape);
		settle(harness);
		enabled
	};
	assert!(!cut_enabled(&mut harness), "nothing is selected yet");
	harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
	settle(&mut harness);
	assert!(cut_enabled(&mut harness), "Ctrl+A selected no item");
}

#[test]
fn view_theme_switches_the_look() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	assert_eq!(harness.ctx.theme(), egui::Theme::Light);
	harness.get_by_label("View").click();
	settle(&mut harness);
	harness.get_by_label("Theme").hover();
	settle(&mut harness);
	harness.get_by_label("Dark").click();
	settle(&mut harness);
	assert_eq!(harness.ctx.theme(), egui::Theme::Dark);
}

#[test]
fn view_panes_hides_and_shows_a_pane() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	let toggle = |harness: &mut Harness<'_, Shell>, pane: &str| {
		harness.get_by_label("View").click();
		settle(harness);
		harness.get_by_label("Panes").hover();
		settle(harness);
		harness.get_by_role_and_label(egui::Role::MenuItemCheckBox, pane).click();
		settle(harness);
	};
	assert!(harness.query_by_label("Messages").is_none(), "Messages starts hidden");
	toggle(&mut harness, "Messages");
	harness.get_by_role_and_label(egui::Role::List, "Messages");
	let tab = |harness: &Harness<'_, Shell>| harness.query_by_role_and_label(egui::Role::Button, "Project").is_some();
	toggle(&mut harness, "Project");
	assert!(!tab(&harness), "Project stayed");
	toggle(&mut harness, "Project");
	assert!(tab(&harness), "Project did not come back");
}

/// Drags with the primary button from `from` to `to` in steps, pressing Esc before the release if `cancel`.
fn drag(harness: &mut Harness<'_, Shell>, from: egui::Pos2, to: egui::Pos2, cancel: bool) {
	let button = |pos, pressed| egui::Event::PointerButton {
		pos,
		button: egui::PointerButton::Primary,
		pressed,
		modifiers: Modifiers::NONE,
	};
	harness.event(egui::Event::PointerMoved(from));
	harness.event(button(from, true));
	harness.step();
	for t in [0.1, 0.5, 1.] {
		harness.event(egui::Event::PointerMoved(from + (to - from) * t));
		harness.step();
	}
	if cancel {
		harness.key_press(Key::Escape);
		harness.step();
	}
	harness.event(button(to, false));
	settle(harness);
}

#[test]
fn a_tab_drops_into_a_group_but_not_into_the_preview_nor_after_esc() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	let tab = |harness: &Harness<'_, Shell>, pane: &str| harness.get_by_role_and_label(egui::Role::Button, pane).rect();
	let (project, properties) = (tab(&harness, "Project"), tab(&harness, "Properties"));
	let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.));
	drag(&mut harness, project.center(), window.center(), false);
	assert_eq!(tab(&harness, "Project"), project, "the Preview took a tab");
	let into_properties = properties.center() + egui::vec2(120., 200.);
	drag(&mut harness, project.center(), into_properties, true);
	assert_eq!(tab(&harness, "Project"), project, "Esc did not cancel the drag");
	drag(&mut harness, project.center(), into_properties, false);
	let (project, properties) = (tab(&harness, "Project"), tab(&harness, "Properties"));
	assert!(
		project.top() == properties.top() && project.left() >= properties.right(),
		"Project is no tab of Properties"
	);
}

#[test]
fn a_group_dragged_by_its_tab_bar_drops_beside_the_preview() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	let properties = harness.get_by_role_and_label(egui::Role::Button, "Properties").rect();
	drag(&mut harness, properties.right_center() + egui::vec2(60., 0.), egui::pos2(1100., 400.), false);
	let properties = harness.get_by_role_and_label(egui::Role::Button, "Properties").rect();
	assert!(properties.left() > 900., "Properties is not beside the Preview: {properties:?}");
}

#[test]
fn the_arrangement_is_saved_as_it_changes_and_comes_back_at_the_next_start() {
	let dir = tempfile::tempdir().unwrap();
	let layout = || AppState::start(&AppDirs::under(dir.path()), []).unwrap().settings.layout;
	let mut harness = shell(dir.path(), None);
	let tab = |harness: &Harness<'_, Shell>, pane: &str| harness.get_by_role_and_label(egui::Role::Button, pane).rect();
	let (project, properties) = (tab(&harness, "Project"), tab(&harness, "Properties"));
	harness.get_by_label("Properties").click();
	settle(&mut harness);
	assert_eq!(layout(), None, "a click on the active tab changed nothing");
	drag(&mut harness, project.center(), properties.center() + egui::vec2(120., 200.), false);
	let saved = layout();
	assert!(saved.is_some(), "the drop is not saved");
	let mut harness = shell(dir.path(), None);
	let (project, properties) = (tab(&harness, "Project"), tab(&harness, "Properties"));
	assert!(project.top() == properties.top() && project.left() >= properties.right(), "the drop is not restored");
	assert_eq!(layout(), saved, "the next start rewrote the arrangement");
	let column = egui::pos2(properties.left() + 320., 400.);
	drag(&mut harness, column, column + egui::vec2(80., 0.), false);
	assert_ne!(layout(), saved, "the resize is not saved");
}

fn is_focused(harness: &Harness<'_, Shell>, label: &str) -> bool {
	harness.get_by_label(label).is_focused()
}

#[test]
fn alt_and_a_letter_open_a_menu_on_its_first_entry() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	harness.key_press_modifiers(Modifiers::ALT, Key::F);
	settle(&mut harness);
	assert!(is_focused(&harness, "New Project"), "Alt+F left the File menu unfocused");
}

#[test]
fn the_arrows_walk_the_menus_and_esc_steps_out() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	harness.key_press_modifiers(Modifiers::ALT, Key::F);
	settle(&mut harness);
	harness.key_press(Key::ArrowDown);
	settle(&mut harness);
	assert!(is_focused(&harness, "Open Project..."));
	harness.key_press(Key::ArrowLeft);
	settle(&mut harness);
	assert!(harness.query_by_label("New Project").is_none(), "File stayed open");
	assert!(is_focused(&harness, "About WaveDude Rust"), "Left from File opens Help");
	harness.key_press(Key::ArrowRight);
	settle(&mut harness);
	assert!(is_focused(&harness, "New Project"), "Right from Help wraps to File");
	harness.key_press(Key::Escape);
	settle(&mut harness);
	assert!(harness.query_by_label("New Project").is_none(), "Esc left File open");
	assert!(is_focused(&harness, "File"), "Esc leaves the bar on File");
	harness.key_press(Key::ArrowRight);
	harness.run_steps(SETTLE);
	harness.key_press(Key::Enter);
	settle(&mut harness);
	assert!(harness.query_by_label("Undo").is_some(), "Right, Enter opens Edit");
	harness.key_press(Key::Escape);
	settle(&mut harness);
	harness.key_press(Key::Escape);
	settle(&mut harness);
	assert!(!is_focused(&harness, "Edit"), "the second Esc leaves the bar");
}

#[test]
fn a_lone_alt_enters_the_bar_and_letters_pick_entries() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
	harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
	settle(&mut harness);
	assert!(is_focused(&harness, "File"), "a lone Alt enters the bar on File");
	for key in [Key::V, Key::T] {
		harness.key_press(key);
		settle(&mut harness);
	}
	assert!(is_focused(&harness, "System"), "V, T open View > Theme");
	harness.key_press(Key::Escape);
	settle(&mut harness);
	assert!(harness.query_by_label("System").is_none() && is_focused(&harness, "Theme"), "Esc closes the submenu only");
	harness.key_press(Key::ArrowRight);
	settle(&mut harness);
	harness.key_press(Key::End);
	settle(&mut harness);
	harness.key_press(Key::Enter);
	settle(&mut harness);
	assert_eq!(harness.ctx.theme(), egui::Theme::Dark, "Right, End, Enter picks Dark");
	assert!(harness.query_by_label("System").is_none(), "picking closes the menus");
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
		let mut harness = shell_with(dir.path(), language, None);
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

fn project_of<'a>(harness: &'a Harness<'_, Shell>) -> &'a wavedude_core::model::Project {
	&harness.state().app().document.project
}

fn row(harness: &Harness<'_, Shell>, label: &str) -> egui::Rect {
	harness.get_by_role_and_label(egui::Role::TreeItem, label).rect()
}

#[test]
fn a_click_on_a_row_selects_it_and_ctrl_click_adds_to_it() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	harness.get_by_role_and_label(egui::Role::TreeItem, "Source 1").click();
	settle(&mut harness);
	let source = ItemRef::new(ItemKind::Source, 0);
	let part = ItemRef::new(ItemKind::Part, 0);
	assert_eq!(harness.state().app().selection.current_item(), Some(source));
	harness.get_by_role_and_label(egui::Role::TreeItem, "Rectangle 1").click_modifiers(Modifiers::COMMAND);
	settle(&mut harness);
	let selection = &harness.state().app().selection;
	assert!(selection.contains(source) && selection.contains(part), "Ctrl+click replaced the selection");
}

#[test]
fn the_toolbar_adds_a_source_and_the_keys_rename_and_remove_it() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	harness.get_by_role_and_label(egui::Role::Button, "Source").click();
	settle(&mut harness);
	assert_eq!(project_of(&harness).sources.len(), 2);
	let added = ItemRef::new(ItemKind::Source, 1);
	assert_eq!(harness.state().app().selection.current_item(), Some(added));
	harness.get_by_role_and_label(egui::Role::TreeItem, "Source 2").click();
	settle(&mut harness);
	harness.key_press(Key::F2);
	settle(&mut harness);
	harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
	harness.event(egui::Event::Text("Beam".into()));
	harness.key_press(Key::Enter);
	settle(&mut harness);
	assert_eq!(added.name(project_of(&harness)), Some("Beam"));
	harness.key_press(Key::Delete);
	settle(&mut harness);
	assert_eq!(project_of(&harness).sources.len(), 1, "Delete kept the source");
}

#[test]
fn alt_and_an_arrow_move_a_part_and_the_row_menu_lists_its_actions() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	harness.get_by_role_and_label(egui::Role::Button, "Part").click();
	settle(&mut harness);
	harness.get_by_label("Rectangle").click();
	settle(&mut harness);
	let names = |harness: &Harness<'_, Shell>| ItemKind::Part.names(project_of(harness)).join(",");
	assert_eq!(names(&harness), "Rectangle 1,Rectangle 2");
	harness.get_by_role_and_label(egui::Role::TreeItem, "Rectangle 2").click();
	settle(&mut harness);
	harness.key_press_modifiers(Modifiers::ALT, Key::ArrowUp);
	settle(&mut harness);
	assert_eq!(names(&harness), "Rectangle 2,Rectangle 1", "Alt+Up left the part");
	harness.get_by_role_and_label(egui::Role::TreeItem, "Rectangle 1").click_secondary();
	settle(&mut harness);
	for entry in ["Cut", "Duplicate", "Move down", "Remove"] {
		harness.get_by_label(entry);
	}
}

#[test]
fn a_row_dropped_on_another_reorders_the_list() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	harness.get_by_role_and_label(egui::Role::Button, "Source").click();
	settle(&mut harness);
	let first = row(&harness, "Source 1");
	let second = row(&harness, "Source 2");
	drag(&mut harness, first.center(), egui::pos2(second.center().x, second.bottom() - 2.), false);
	assert_eq!(ItemKind::Source.names(project_of(&harness)), ["Source 2", "Source 1"]);
}

#[test]
fn a_long_tree_scrolls_to_the_current_row() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	for _ in 0..12 {
		harness.get_by_role_and_label(egui::Role::Button, "Source").click();
		settle(&mut harness);
	}
	// rounded to whole pixels
	let tree = harness.get_by_role(egui::Role::Tree).rect().expand(1.);
	let shown = |harness: &Harness<'_, Shell>, label: &str| {
		harness.query_by_role_and_label(egui::Role::TreeItem, label).is_some_and(|n| tree.contains_rect(n.rect()))
	};
	assert!(shown(&harness, "Source 13"), "the added source is out of sight");
	assert!(!shown(&harness, "Project"), "the tree is short enough to show all");
	harness.get_by_role_and_label(egui::Role::TreeItem, "Source 13").click();
	settle(&mut harness);
	harness.key_press(Key::Home);
	settle(&mut harness);
	assert!(shown(&harness, "Project"), "Home left the root out of sight");
	harness.key_press(Key::End);
	settle(&mut harness);
	assert!(shown(&harness, "Colour schemes"), "End left the last row out of sight");
	let row = row(&harness, "Colour schemes");
	harness.hover_at(row.center());
	harness.get_by_role_and_label(egui::Role::TreeItem, "Colour schemes").click();
	settle(&mut harness);
	let current = harness.state().app().selection.current();
	assert_eq!(current, Some(Row::Folder(ItemKind::Scheme)), "the row drawn there is another");
}

#[test]
fn a_click_on_a_check_box_keeps_the_keys_in_the_tree() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	harness.get_by_role_and_label(egui::Role::TreeItem, "Source 1").click();
	settle(&mut harness);
	harness.get_by_role_and_label(egui::Role::CheckBox, "Source 1").click();
	settle(&mut harness);
	assert!(!project_of(&harness).sources[0].enabled, "the check box left the source on");
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(project_of(&harness).sources[0].enabled, "Space after the click left the tree");
}

#[test]
fn a_right_click_in_the_selection_keeps_it_and_its_menu_acts_on_the_row() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), Some(EXAMPLE));
	harness.get_by_role_and_label(egui::Role::TreeItem, "Source 1").click();
	settle(&mut harness);
	harness.get_by_role_and_label(egui::Role::TreeItem, "Rectangle 1").click_modifiers(Modifiers::COMMAND);
	settle(&mut harness);
	let before = harness.state().app().selection.clone();
	harness.get_by_role_and_label(egui::Role::TreeItem, "Source 1").click_secondary();
	settle(&mut harness);
	assert!(harness.state().app().selection == before, "the right-click changed the selection");
	harness.get_by_label("Duplicate").click();
	settle(&mut harness);
	let project = project_of(&harness);
	assert_eq!((project.sources.len(), project.parts.len()), (2, 2), "Duplicate left out a selected item");
}

#[test]
fn close_is_enabled_on_any_project_but_the_empty_one_of_the_start() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	let close_enabled = |harness: &mut Harness<'_, Shell>| {
		harness.get_by_label("File").click();
		settle(harness);
		let enabled = !harness.get_by_label("Close Project").accesskit_node().is_disabled();
		harness.key_press(Key::Escape);
		settle(harness);
		enabled
	};
	assert!(!close_enabled(&mut harness), "the empty project closes");
	harness.get_by_label("File").click();
	settle(&mut harness);
	harness.get_by_label("New Project").click();
	settle(&mut harness);
	assert!(close_enabled(&mut harness), "a new project without a file does not close");
}

/// A file dropped on the window, as the platform hands it over.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
	fn path(&self) -> &Path {
		&self.0
	}

	fn bytes(&self) -> Result<Vec<u8>, String> {
		std::fs::read(&self.0).map_err(|e| e.to_string())
	}
}

fn drop_file(harness: &mut Harness<'_, Shell>, path: &str) {
	let path = std::fs::canonicalize(path).unwrap();
	harness.input_mut().dropped_files.push(std::sync::Arc::new(Dropped(path)));
	settle(harness);
}

#[test]
fn a_dropped_project_opens_and_a_dropped_image_becomes_the_scene() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = shell(dir.path(), None);
	drop_file(&mut harness, EXAMPLE);
	assert_eq!(project_of(&harness).sources.len(), 1, "the dropped project did not open");
	let image = "../wavedude-core/tests/fixtures/legacy/Example12_Dispersion/scene_images/frequency_race_track (2).png";
	drop_file(&mut harness, image);
	// the image is read on a thread of its own
	for _ in 0..200 {
		if project_of(&harness).scene.image_data.is_some() {
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(10));
		harness.run_steps(1);
	}
	assert_eq!(project_of(&harness).scene.image, "frequency_race_track (2).png");
}
