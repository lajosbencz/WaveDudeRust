//! The modals driven headless: the unsaved-edits question with its buttons and keys, the message window of a failed
//! open, the crash report an earlier session left, the modals waiting in turn and the window's keys waiting behind
//! them, the startup page, About, Examples and the manual window.

mod common;

use std::path::{Path, PathBuf};

use eframe::egui::accesskit::Toggled;
use eframe::egui::{self, Key, Modifiers, OutputCommand};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use wavedude_app::open_requests::OpenRequests;
use wavedude_app::ui::{Boot, Shell};
use wavedude_app_state::document::Source;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::{AppDirs, Theme};
use wavedude_app_state::{AppState, Event, Guarded};
use wavedude_core::quality::{PreviewDefaults, PreviewTier};
use wavedude_examples::Topic;

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj";
/// Frames for an input to reach the state and its result to show.
const SETTLE: usize = 4;
const MISSING: &str = "/nonexistent/missing.wdproj";

/// The window on the settings under `dir` in the light theme, nothing run, with `project` open and `asked` asked
/// for by the command line; `first_start` shows the startup page.
fn window_with(dir: &Path, first_start: bool, project: Option<&Path>, asked: &[&str]) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, first_start);
	for event in [Event::SetTheme(Theme::Light), Event::SetAutoRerun(false)] {
		app.handle(event, Instant::now());
	}
	if let Some(path) = project {
		app.handle(Event::Guarded(Guarded::Open(Source::Path(path.into()))), Instant::now());
	}
	let open = OpenRequests::default();
	for path in asked {
		open.push(path.into());
	}
	let boot = Boot { store: dirs.store(), gpu: None, open, log: LogRing::new(), update: Default::default() };
	let mut harness =
		Harness::builder().with_size(egui::vec2(1280., 800.)).build_eframe(move |cc| Shell::new(cc, app, boot));
	harness.run_steps(SETTLE);
	harness
}

fn window(dir: &Path, project: Option<&Path>, asked: &[&str]) -> Harness<'static, Shell> {
	window_with(dir, false, project, asked)
}

fn settle(harness: &mut Harness<'_, Shell>) {
	harness.run_steps(SETTLE);
}

/// A copy of the example in `dir`, which a save may overwrite.
fn example_in(dir: &Path) -> PathBuf {
	let path = dir.join("example.wdproj");
	std::fs::copy(EXAMPLE, &path).unwrap();
	path
}

fn sources(harness: &Harness<'_, Shell>) -> usize {
	harness.state().app().document.project.sources.len()
}

/// Adds a source through the tree's toolbar: the project has unsaved edits.
fn edit(harness: &mut Harness<'_, Shell>) {
	harness.get_by_role_and_label(egui::Role::Button, "Source").click();
	settle(harness);
}

const QUESTION: &str = "Save the changes to example.wdproj?";

#[test]
fn the_unsaved_question_keeps_the_edits_on_cancel_and_drops_them_on_dont_save() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), Some(&example_in(dir.path())), &[]);
	edit(&mut harness);
	harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
	settle(&mut harness);
	harness.get_by_label(QUESTION);
	harness.key_press(Key::Escape);
	settle(&mut harness);
	assert!(harness.query_by_label(QUESTION).is_none(), "Esc left the question open");
	assert_eq!(sources(&harness), 2, "Cancel dropped the edits");
	harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
	settle(&mut harness);
	harness.key_press(Key::D);
	settle(&mut harness);
	assert!(harness.query_by_label(QUESTION).is_none(), "D left the question open");
	let doc = &harness.state().app().document;
	assert!(doc.path.is_none() && doc.project.sources.is_empty(), "Don't save kept the project");
}

#[test]
fn save_in_the_unsaved_question_saves_then_goes_on() {
	let dir = tempfile::tempdir().unwrap();
	let path = example_in(dir.path());
	let mut harness = window(dir.path(), Some(&path), &[]);
	edit(&mut harness);
	harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
	settle(&mut harness);
	harness.key_press(Key::Enter);
	settle(&mut harness);
	assert!(harness.state().app().document.path.is_none(), "the new project did not follow the save");
	let mut saved = AppState::start(&AppDirs::under(dir.path()), []).unwrap();
	saved.handle(Event::Guarded(Guarded::Open(Source::Path(path))), Instant::now());
	assert_eq!(saved.document.project.sources.len(), 2, "the edits were not saved");
}

#[test]
fn a_failed_open_shows_its_error_and_the_window_keys_wait_behind_it() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), Some(&example_in(dir.path())), &[MISSING]);
	let error = format!("Could not open '{MISSING}'.");
	harness.get_by_label(&error);
	harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
	settle(&mut harness);
	assert_eq!(sources(&harness), 1, "Ctrl+N acted behind the error");
	harness.get_by_label("OK").click();
	settle(&mut harness);
	assert!(harness.query_by_label(&error).is_none(), "OK left the error open");
	harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
	settle(&mut harness);
	assert_eq!(sources(&harness), 0, "Ctrl+N waits after the error closed");
}

#[test]
fn the_crash_report_shows_once_before_the_modals_asked_for_meanwhile() {
	let dir = tempfile::tempdir().unwrap();
	let store = AppDirs::under(dir.path()).store();
	let report = store.path("logs/crash.txt");
	std::fs::create_dir_all(report.parent().unwrap()).unwrap();
	std::fs::write(&report, "boom").unwrap();
	let mut harness = window(dir.path(), None, &[MISSING]);
	let shown = store.path("logs/last-crash.txt");
	assert!(shown.is_file() && !report.exists(), "the report is not set aside");
	let text = format!(
		"WaveDude Rust hit an internal error in the last session. The report, to attach to a bug report:\n{}",
		shown.display()
	);
	harness.get_by_label(&text);
	harness.get_by_label("Copy path").click();
	harness.step();
	let copied = harness
		.output()
		.platform_output
		.commands
		.iter()
		.any(|c| *c == OutputCommand::CopyText(shown.display().to_string()));
	settle(&mut harness);
	assert!(copied, "Copy path put nothing on the clipboard");
	harness.get_by_label(&text);
	harness.key_press(Key::Enter);
	settle(&mut harness);
	assert!(harness.query_by_label(&text).is_none(), "Enter left the report open");
	harness.get_by_label(&format!("Could not open '{MISSING}'."));
	drop(harness);
	let harness = window(dir.path(), None, &[]);
	assert!(harness.query_by_label(&text).is_none(), "the report showed twice");
}

/// Opens `entry` of the menu `menu`.
fn menu(harness: &mut Harness<'_, Shell>, menu: &str, entry: &str) {
	harness.get_by_label(menu).click();
	settle(harness);
	harness.get_by_label(entry).click();
	settle(harness);
}

#[test]
fn the_startup_page_shows_at_the_first_start_and_ok_saves_its_answers() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window_with(dir.path(), true, None, &[]);
	harness.get_by_label("Welcome to WaveDude Rust");
	harness.get_by_label("High").click();
	settle(&mut harness);
	harness.get_by_label("Show this page at each launch").click();
	settle(&mut harness);
	// Enter would toggle the focused check box again
	harness.get_by_label("OK").click();
	settle(&mut harness);
	assert!(harness.query_by_label("Welcome to WaveDude Rust").is_none(), "OK left the page open");
	let st = &harness.state().app().settings;
	assert_eq!(st.preview_quality(), PreviewDefaults::quality_of(PreviewTier::High), "the tier was not saved");
	assert_eq!(st.preview_dither_bits, PreviewDefaults::dither_bits_of(PreviewTier::High));
	assert!(!st.show_startup, "the launch display was not turned off");
}

#[test]
fn the_startup_pages_examples_button_saves_its_answers_and_opens_examples() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window_with(dir.path(), true, None, &[]);
	harness.get_by_label("High").click();
	settle(&mut harness);
	harness.get_by_label("Examples...").click();
	settle(&mut harness);
	assert!(harness.query_by_label("Welcome to WaveDude Rust").is_none(), "Examples... left the page open");
	harness.get_by_label("Examples");
	let st = &harness.state().app().settings;
	assert_eq!(st.preview_quality(), PreviewDefaults::quality_of(PreviewTier::High), "the tier was not saved");
}

#[test]
fn about_copies_the_system_lines_toggles_the_license_and_closes_on_esc() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), None, &[]);
	menu(&mut harness, "Help", "About WaveDude Rust");
	harness.get_by_label(&format!("Version {}", env!("CARGO_PKG_VERSION")));
	harness.key_press(Key::T);
	settle(&mut harness);
	harness.get_by_label("Hide the license text");
	harness.get_by_label("Copy system info").click();
	harness.step();
	let lines = harness.output().platform_output.commands.iter().find_map(|c| match c {
		OutputCommand::CopyText(text) => Some(text.clone()),
		_ => None,
	});
	settle(&mut harness);
	assert!(
		lines.is_some_and(|l| l.starts_with("Program: WaveDude Rust")),
		"Copy put no system lines on the clipboard"
	);
	harness.get_by_label("Hide the license text");
	harness.key_press(Key::Escape);
	settle(&mut harness);
	assert!(harness.query_by_label("Hide the license text").is_none(), "Esc left About open");
}

#[test]
fn the_welcome_page_entry_opens_the_startup_page() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), None, &[]);
	assert!(harness.query_by_label("Welcome to WaveDude Rust").is_none(), "the page opened at launch");
	menu(&mut harness, "File", "Welcome Page...");
	harness.get_by_label("Welcome to WaveDude Rust");
}

#[test]
fn examples_searches_every_topic_and_opens_the_chosen_example() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), None, &[]);
	let example = wavedude_examples::all().iter().find(|e| e.topic != Topic::ALL[0]).unwrap();
	menu(&mut harness, "File", "Examples...");
	harness.get_by_label("Examples");
	harness.get_by_role(egui::Role::TextInput).click();
	settle(&mut harness);
	harness.get_by_role(egui::Role::TextInput).type_text(&example.title);
	settle(&mut harness);
	harness.get_by_role_and_label(egui::Role::ListItem, &example.title);
	harness.get_by_label("Open").click();
	settle(&mut harness);
	assert!(harness.query_by_label("Examples").is_none(), "Open left the dialog open");
	let stem = example.file_name.trim_end_matches(".wdproj");
	assert!(
		harness.state().app().title().contains(stem),
		"the example did not open: {}",
		harness.state().app().title()
	);
}

#[test]
fn f1_opens_the_manual_a_row_takes_its_section_and_esc_closes_it() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), None, &[]);
	harness.key_press(Key::F1);
	settle(&mut harness);
	assert!(harness.query_all_by_label("WaveDude Rust manual").next().is_some(), "F1 opened no manual");
	let rows = harness.get_all_by_role(egui::Role::ListItem).count();
	assert!(rows > 3, "the contents list {rows} rows");
	harness.get_all_by_role(egui::Role::ListItem).nth(2).unwrap().click();
	settle(&mut harness);
	let third = harness.get_all_by_role(egui::Role::ListItem).nth(2).unwrap();
	assert_eq!(third.accesskit_node().toggled(), Some(Toggled::True), "the row clicked is not taken");
	harness.key_press(Key::Escape);
	settle(&mut harness);
	assert!(harness.query_all_by_label("WaveDude Rust manual").next().is_none(), "Esc left the manual open");
}
