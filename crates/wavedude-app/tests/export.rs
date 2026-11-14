//! Exports driven headless on a small project: a run exported through the dialog after its full-quality run,
//! repeated by Export last and asking before it replaces a file; a waiting export in the status bar, cancelled by
//! Esc; Exit while an export waits asks first; a frame export's proposed name follows the layer.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{self, Key, Modifiers, Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use wavedude_app::ui::{Boot, Shell};
use wavedude_app_state::document::Source;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::AppDirs;
use wavedude_app_state::{Event, Guarded};

/// A run far longer than a test waits for.
const LONG: &str = "../wavedude-core/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj";
/// A short run on a small scene.
const SHORT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Project Version="3" Seed="1">
	<Scene Width="160" Height="90" />
	<Simulation Steps="100" VisualizeEvery="10" />
	<Source Name="Source 1" X="40" Y="45" />
</Project>
"#;
/// Frames for an input to reach the state and its result to show.
const SETTLE: usize = 4;
const WAITING: &str = "Export: waiting for the run";

/// The window on `project` saved in `dir`, nothing run.
fn window(dir: &Path, project: &str) -> Harness<'static, Shell> {
	let path = dir.join("example.wdproj");
	std::fs::write(&path, project).unwrap();
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	for event in [Event::SetAutoRerun(false), Event::Guarded(Guarded::Open(Source::Path(path)))] {
		app.handle(event, Instant::now());
	}
	let boot = Boot { store: dirs.store(), open: Default::default(), log: LogRing::new(), update: Default::default() };
	let mut harness =
		Harness::builder().with_size(egui::vec2(1280., 800.)).build_eframe(move |cc| Shell::new(cc, app, boot));
	harness.run_steps(SETTLE);
	harness
}

fn settle(harness: &mut Harness<'_, Shell>) {
	harness.run_steps(SETTLE);
}

fn exporting(harness: &Harness<'_, Shell>) -> bool {
	harness.state().app().live.export_busy()
}

/// Runs frames until no export runs or waits.
fn exported(harness: &mut Harness<'_, Shell>) {
	for _ in 0..3000 {
		std::thread::sleep(Duration::from_millis(10));
		harness.run_steps(1);
		if !exporting(harness) {
			settle(harness);
			return;
		}
	}
	panic!("the export did not end");
}

/// Ctrl+Shift+E's dialog with MPEG-1 picked.
fn field_dialog_as_mpeg(harness: &mut Harness<'_, Shell>) {
	harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
	settle(harness);
	harness.get_by_role_and_label(Role::ComboBox, "Export the field").click();
	settle(harness);
	harness.get_by_label("MPEG-1 video (*.mpg)").click();
	settle(harness);
}

fn ok(harness: &mut Harness<'_, Shell>) {
	harness.get_by_role_and_label(Role::Button, "OK").click();
	settle(harness);
}

fn video(dir: &Path) -> PathBuf {
	dir.join("example_field.mpg")
}

#[test]
fn a_run_exports_after_its_run_repeats_by_export_last_and_asks_before_replacing() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), SHORT);
	field_dialog_as_mpeg(&mut harness);
	ok(&mut harness);
	assert!(exporting(&harness), "the export waits for its run");
	exported(&mut harness);
	assert!(video(dir.path()).is_file(), "the video was not written");
	assert_eq!(harness.state().app().settings.export_filter_index, 1, "the format is remembered");
	std::fs::remove_file(video(dir.path())).unwrap();
	harness.key_press_modifiers(Modifiers::COMMAND, Key::E);
	settle(&mut harness);
	exported(&mut harness);
	assert!(video(dir.path()).is_file(), "Export last did not write the video again");
	harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
	settle(&mut harness);
	ok(&mut harness);
	let question = format!("The file {} exists. Replace it?", video(dir.path()).display());
	harness.get_by_label(&question);
	harness.key_press(Key::N);
	settle(&mut harness);
	assert!(harness.query_by_label(&question).is_none() && !exporting(&harness), "No went on with the export");
}

#[test]
fn a_waiting_export_shows_in_the_status_bar_and_esc_cancels_it() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), &std::fs::read_to_string(LONG).unwrap());
	harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
	settle(&mut harness);
	harness.get_by_role_and_label(Role::Label, "Export the frame");
	ok(&mut harness);
	harness.get_by_label(WAITING);
	harness.key_press(Key::Escape);
	settle(&mut harness);
	assert!(!exporting(&harness) && harness.query_by_label(WAITING).is_none(), "Esc left the export waiting");
}

#[test]
fn exit_while_an_export_waits_asks_first() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), &std::fs::read_to_string(LONG).unwrap());
	harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
	settle(&mut harness);
	ok(&mut harness);
	harness.get_by_label("File").click();
	settle(&mut harness);
	harness.get_by_label("Exit").click();
	settle(&mut harness);
	let question = "An export is running. Stop it and quit?";
	harness.get_by_label(question);
	harness.key_press(Key::N);
	settle(&mut harness);
	assert!(exporting(&harness), "No stopped the export");
	harness.get_by_label("File").click();
	settle(&mut harness);
	harness.get_by_label("Exit").click();
	settle(&mut harness);
	harness.key_press(Key::Y);
	settle(&mut harness);
	assert!(!exporting(&harness), "Yes left the export waiting");
}

#[test]
fn esc_in_the_path_box_closes_the_dialog_and_enter_there_exports() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), &std::fs::read_to_string(LONG).unwrap());
	for (key, export) in [(Key::Escape, false), (Key::Enter, true)] {
		harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
		settle(&mut harness);
		harness.get_by_role_and_label(Role::TextInput, "Export the frame").click();
		settle(&mut harness);
		harness.key_press(key);
		settle(&mut harness);
		assert!(harness.query_by_role_and_label(Role::Label, "Export the frame").is_none(), "{key:?} left it open");
		assert_eq!(exporting(&harness), export, "{key:?}");
	}
}

#[test]
fn a_frame_export_proposes_the_other_layers_name_after_one_was_made() {
	use wavedude_app::export::request;
	use wavedude_app::menus::ExportKindChoice;
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path(), SHORT);
	harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
	settle(&mut harness);
	ok(&mut harness);
	exported(&mut harness);
	let app = harness.state().app();
	let name = |choice| request(app, choice).path.file_name().unwrap().to_string_lossy().into_owned();
	assert!(name(ExportKindChoice::FieldFrame).starts_with("example_field_"), "{}", name(ExportKindChoice::FieldFrame));
	assert!(
		name(ExportKindChoice::IntensityFrame).starts_with("example_intensity_"),
		"{}",
		name(ExportKindChoice::IntensityFrame)
	);
}
