//! The Messages pane driven headless over the log ring: the status bar's double-click opens it with its list
//! focused, the level toggles, the search plain and as a regex, the selection's copy by the keys and the menu, and
//! Clear.

mod common;

use std::path::Path;

use eframe::egui::accesskit::Toggled;
use eframe::egui::{self, Key, Modifiers, OutputCommand, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use tracing::Level;
use wavedude_app::ui::{Boot, Shell};
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::{AppDirs, Theme};

/// Frames for an input to reach the state and its result to show.
const SETTLE: usize = 4;
/// A frame's time: a double-click's two clicks land within egui's double-click delay.
const STEP: f32 = 1. / 60.;

/// The window on an empty project with `ring` as its log, which holds a line of each level from Debug up and a
/// two-line warning.
fn window(dir: &Path, ring: &LogRing) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	app.handle(wavedude_app_state::Event::SetTheme(Theme::Light), Instant::now());
	ring.push(Level::DEBUG, "wavedude_core::engine", "grid 1920 x 1080".into());
	ring.push(Level::WARN, "wavedude_app::store", "spilled to disk\nbudget 6 GB".into());
	ring.push(Level::ERROR, "wavedude_export", "The export failed.".into());
	ring.push(Level::INFO, "wavedude_app_state::document", "Opened tiny.wdproj".into());
	let boot = Boot { store: dirs.store(), open: Default::default(), log: ring.clone(), update: Default::default() };
	let mut harness = Harness::builder()
		.with_size(vec2(1280., 800.))
		.with_step_dt(STEP)
		.build_eframe(move |cc| Shell::new(cc, app, boot));
	harness.run_steps(SETTLE);
	harness
}

fn settle(harness: &mut Harness<'_, Shell>) {
	harness.run_steps(SETTLE);
}

/// Opens the pane by a double-click on the status bar's line.
fn open(harness: &mut Harness<'_, Shell>) {
	let line = harness.get_by_label("Opened tiny.wdproj");
	line.click();
	line.click();
	settle(harness);
}

/// The lines shown, oldest first.
fn lines(harness: &Harness<'_, Shell>) -> Vec<String> {
	harness.query_all_by_role(Role::ListItem).filter_map(|n| n.accesskit_node().label()).collect()
}

/// What the system's copy gesture, which egui receives as a copy event, copies.
fn copy(harness: &mut Harness<'_, Shell>) -> Option<String> {
	harness.event(egui::Event::Copy);
	harness.step();
	copied(harness)
}

/// What the last pass copied.
fn copied(harness: &Harness<'_, Shell>) -> Option<String> {
	harness.output().platform_output.commands.iter().find_map(|c| match c {
		OutputCommand::CopyText(text) => Some(text.clone()),
		_ => None,
	})
}

fn list_is_focused(harness: &Harness<'_, Shell>) -> bool {
	harness.get_by_role_and_label(Role::List, "Messages").is_focused()
}

#[test]
fn a_double_click_on_the_status_bar_opens_the_pane_with_its_list_focused() {
	let dir = tempfile::tempdir().unwrap();
	let ring = LogRing::new();
	let mut harness = window(dir.path(), &ring);
	assert!(harness.query_by_role_and_label(Role::List, "Messages").is_none(), "Messages starts hidden");
	open(&mut harness);
	assert!(list_is_focused(&harness), "the list has no focus");
	assert_eq!(lines(&harness), ["grid 1920 x 1080", "spilled to disk", "The export failed.", "Opened tiny.wdproj"]);
	ring.push(Level::INFO, "a", "later".into());
	settle(&mut harness);
	assert_eq!(lines(&harness).last().map(String::as_str), Some("later"), "a new line shows");
}

#[test]
fn the_level_toggles_and_the_search_plain_or_as_a_regex_filter_the_lines() {
	let dir = tempfile::tempdir().unwrap();
	let ring = LogRing::new();
	let mut harness = window(dir.path(), &ring);
	open(&mut harness);
	harness.get_by_role_and_label(Role::Button, "Warn (1)").click();
	settle(&mut harness);
	assert_eq!(lines(&harness), ["grid 1920 x 1080", "The export failed.", "Opened tiny.wdproj"]);
	harness.get_by_role_and_label(Role::Button, "Warn (1)").click();
	let search = harness.get_by_role(Role::TextInput);
	search.focus();
	search.type_text("BUDGET");
	settle(&mut harness);
	assert_eq!(lines(&harness), ["spilled to disk"], "the plain search reads the detail, any case");
	harness.get_by_role_and_label(Role::Button, "Regex").click();
	settle(&mut harness);
	assert_eq!(lines(&harness), ["spilled to disk"], "the same text as a regex");
	let search = harness.get_by_role(Role::TextInput);
	search.focus();
	harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
	search.type_text(r"^(opened|the)\b");
	settle(&mut harness);
	assert_eq!(lines(&harness), ["The export failed.", "Opened tiny.wdproj"]);
	harness.key_press(Key::Backspace);
	harness.key_press(Key::Backspace);
	harness.key_press(Key::Backspace);
	settle(&mut harness);
	assert_eq!(lines(&harness).len(), 4, "an unfinished pattern shows every line");
}

#[test]
fn the_keys_and_the_menu_copy_the_selected_or_every_shown_line_and_clear_empties_it() {
	let dir = tempfile::tempdir().unwrap();
	let ring = LogRing::new();
	let mut harness = window(dir.path(), &ring);
	open(&mut harness);
	harness.get_by_role_and_label(Role::ListItem, "spilled to disk").click();
	settle(&mut harness);
	harness.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
	settle(&mut harness);
	let text = copy(&mut harness).expect("the selection was copied");
	let rows: Vec<Vec<&str>> = text.lines().map(|l| l.split('\t').collect()).collect();
	assert_eq!(rows.len(), 2);
	assert_eq!(rows[0][1..], ["Warn", "store", "spilled to disk"]);
	assert_eq!(rows[1][1..], ["Error", "wavedude_export", "The export failed."]);
	assert!(list_is_focused(&harness));
	harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
	settle(&mut harness);
	assert_eq!(copy(&mut harness).map(|t| t.lines().count()), Some(4), "Ctrl+A selected every line");
	harness.get_by_role_and_label(Role::ListItem, "Opened tiny.wdproj").click_secondary();
	settle(&mut harness);
	harness.get_by_role_and_label(Role::MenuItem, "Copy all").click_accesskit();
	harness.step();
	assert_eq!(copied(&harness).map(|t| t.lines().count()), Some(4));
	settle(&mut harness);
	harness.get_by_role_and_label(Role::ListItem, "Opened tiny.wdproj").click_secondary();
	settle(&mut harness);
	harness.get_by_role_and_label(Role::MenuItem, "Clear").click();
	settle(&mut harness);
	assert!(lines(&harness).is_empty());
	assert!(ring.is_empty(), "Clear empties the ring");
	assert_eq!(harness.get_by_role_and_label(Role::Button, "Info (0)").accesskit_node().toggled(), Some(Toggled::True));
}
