//! The app state driven as the views will drive it, on the scripted backend and a fake clock.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde_json::json;
use wavedude_app_state::clipboard::PasteTarget;
use wavedude_app_state::commands::{Command, NewPart, Target};
use wavedude_app_state::history::History;
use wavedude_app_state::live::fake::FakeBackend;
use wavedude_app_state::live::{Notice, RunKind, Shown};
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::settings::Settings;
use wavedude_app_state::{AppState, Event, ExportKind, Guarded, Response, SaveChoice};
use wavedude_core::io::wdproj;
use wavedude_core::model::Point;
use wavedude_core::run::MachineFacts;

fn app() -> AppState<FakeBackend> {
	let mut s =
		AppState::with_backend(Settings::default(), None, FakeBackend::default(), Arc::new(AtomicBool::new(false)));
	let t = Instant::now();
	s.document.project.scene.width = 64;
	s.document.project.scene.height = 48;
	s.document.project.simulation.steps = 100;
	s.document.history = History::new(&s.document.project);
	s.live.project_replaced(&s.document.project, t);
	s
}

fn at(t: Instant, ms: u64) -> Instant {
	t + Duration::from_millis(ms)
}

fn source(i: usize) -> ItemRef {
	ItemRef::new(ItemKind::Source, i)
}

#[test]
fn undo_restores_the_project_and_redo_repeats_it() {
	let t = Instant::now();
	let mut s = app();
	s.handle(Event::Command(Command::AddSource { at: None }), t);
	let added = s.document.project.clone();
	let set = |v: f64| {
		Event::Command(Command::Set { target: Target::Item(source(0)), path: "frequency".into(), value: json!(v) })
	};
	s.handle(set(0.1 + 0.2), at(t, 10));
	s.tick(at(t, 600));
	let edited = s.document.project.clone();
	assert_eq!(s.document.history.len(), 3, "the add is a step by itself, the edit one after quiet time");
	s.handle(Event::Undo, at(t, 700));
	assert_eq!(s.document.project, added);
	assert_eq!(s.selection.current(), Some(Row::Item(source(0))), "the selection follows by name");
	s.handle(Event::Redo, at(t, 800));
	assert_eq!(s.document.project, edited, "floats survive the snapshot bit for bit");
	s.handle(Event::Undo, at(t, 900));
	s.handle(Event::Undo, at(t, 1000));
	assert!(s.document.project.sources.is_empty());
	assert!(s.selection.items().is_empty());
	assert!(!s.document.dirty, "back at the saved step");
}

#[test]
fn a_drag_is_one_step() {
	let t = Instant::now();
	let mut s = app();
	s.handle(Event::Command(Command::AddSource { at: None }), t);
	let steps = s.document.history.len();
	s.handle(Event::MouseDown, at(t, 10));
	for i in 0..10 {
		s.handle(Event::Command(Command::MoveBy { target: source(0), dx: 1, dy: 0 }), at(t, 20 + i * 200));
		s.tick(at(t, 30 + i * 200));
	}
	s.tick(at(t, 5000));
	assert_eq!(s.document.history.len(), steps, "the step stays open while the button is held");
	s.handle(Event::MouseUp, at(t, 5000));
	s.tick(at(t, 5499));
	assert_eq!(s.document.history.len(), steps);
	s.tick(at(t, 5500));
	assert_eq!(s.document.history.len(), steps + 1);
	s.handle(Event::Undo, at(t, 6000));
	assert_eq!(s.document.project.sources[0].x, 32.0);
}

#[test]
fn edits_drive_scratch_frames_and_the_automatic_rerun() {
	let t = Instant::now();
	let mut s = app();
	s.tick(t);
	s.live.backend_mut().finish_scratch();
	s.tick(t);
	let requests = s.live.backend().scratch_requests.len();
	s.handle(Event::Command(Command::AddPart { kind: NewPart::Rectangle, at: Some(Point::new(10, 10)) }), at(t, 100));
	s.tick(at(t, 219));
	assert_eq!(s.live.backend().scratch_requests.len(), requests);
	s.tick(at(t, 220));
	assert_eq!(s.live.backend().scratch_requests.len(), requests + 1);
	s.live.backend_mut().finish_scratch();
	s.tick(at(t, 1599));
	assert!(s.live.backend().runs.is_empty());
	s.tick(at(t, 1600));
	assert_eq!(s.live.run_kind(), Some(RunKind::Automatic));
	s.handle(Event::SetAutoRerun(false), at(t, 1700));
	s.handle(Event::Command(Command::ToggleEnabled { target: ItemRef::new(ItemKind::Part, 0) }), at(t, 1800));
	assert!(s.live.run_is_stale());
	s.live.backend_mut().finish_scratch();
	s.tick(at(t, 5000));
	assert_eq!(s.live.backend().runs.len(), 1, "no re-run with the setting off");
	assert!(s.run_status().ends_with(", edited since"), "{}", s.run_status());
}

#[test]
fn user_run_follows_and_a_scrub_turns_live_off() {
	let t = Instant::now();
	let mut s = app();
	s.handle(Event::Run, t);
	assert!(s.live.is_live());
	s.live.backend().last_run().publish(20);
	s.tick(at(t, 10));
	assert_eq!(s.live.playback.position(), 19);
	assert!(s.live.take_notices().contains(&Notice::ShowRunFrame(19)));
	s.handle(Event::Scrub(5), at(t, 20));
	assert!(!s.live.is_live() && !s.live.playback.is_playing());
	s.tick(at(t, 30));
	assert!(s.live.take_notices().contains(&Notice::ShowRunFrame(5)), "the store can show frame 5");
	s.handle(Event::SetInPoint, at(t, 40));
	assert_eq!(s.document.project.display.export_from, 10, "in point stored in steps");
	assert!(s.document.dirty);
}

#[test]
fn unsaved_edits_are_asked_about() {
	let t = Instant::now();
	let dir = tempfile::tempdir().unwrap();
	let mut s = app();
	s.handle(Event::Command(Command::AddProbe { at: None }), t);
	assert_eq!(s.handle(Event::Guarded(Guarded::New), t), Response::AskSave { name: "Untitled".into() });
	assert_eq!(s.handle(Event::SaveChoice(SaveChoice::Cancel), t), Response::None);
	assert_eq!(s.document.project.probes.len(), 1, "Cancel keeps the project");
	s.handle(Event::Guarded(Guarded::OpenExample(6)), t);
	let ask = s.handle(Event::SaveChoice(SaveChoice::Save), t);
	assert_eq!(ask, Response::AskSavePath { proposed: None });
	let path = dir.path().join("mine.wdr");
	assert_eq!(s.handle(Event::SaveAs(path.clone()), t), Response::None);
	assert!(path.is_file());
	assert!(s.title().ends_with("Example07_Random_emitter_lattice.wdproj"), "{}", s.title());
	assert_eq!(s.settings.recent_files[0], path);
	assert_eq!(s.live.playback.position(), 0);
	s.handle(Event::Command(Command::AddSource { at: None }), t);
	assert!(matches!(s.handle(Event::DropFile(dir.path().join("x.txt")), t), Response::Error(_)));
	assert!(matches!(s.handle(Event::DropFile(path.clone()), t), Response::AskSave { .. }));
	s.handle(Event::SaveChoice(SaveChoice::DontSave), t);
	assert_eq!(s.document.path.as_deref(), Some(path.as_path()));
	assert_eq!(s.document.project.probes.len(), 1);
}

#[test]
fn copy_paste_cut_through_text() {
	let t = Instant::now();
	let mut s = app();
	s.handle(Event::Command(Command::AddSource { at: Some(Point::new(5, 5)) }), t);
	s.handle(Event::Command(Command::AddSource { at: Some(Point::new(9, 9)) }), t);
	s.selection.select(Row::Item(source(0)));
	s.selection.toggle(source(1));
	let Response::Clipboard(text) = s.handle(Event::Copy, t) else { panic!("copy gives text") };
	assert_eq!(text.matches("<Source ").count(), 2);
	let target = PasteTarget { current: Some(Row::Item(source(0))), before: false };
	s.handle(Event::Paste { text: text.clone(), target, place_at: None }, t);
	let names: Vec<&str> = s.document.project.sources.iter().map(|x| x.name.as_str()).collect();
	assert_eq!(names, ["Source 1", "Source 1 copy", "Source 2 copy", "Source 2"]);
	assert_eq!(s.selection.items(), [source(1), source(2)]);
	let before = s.document.project.clone();
	s.handle(Event::Paste { text: "<Source/><Nonsense/>".into(), target, place_at: None }, t);
	assert_eq!(s.document.project, before, "an unknown element refuses the whole fragment");
	s.handle(Event::Paste { text, target: PasteTarget::default(), place_at: Some(Point::new(60, 40)) }, t);
	assert_eq!(s.document.project.sources.len(), 6);
	assert_eq!(
		(s.document.project.sources[4].x, s.document.project.sources[5].x),
		(59.0, 63.0),
		"held inside the canvas"
	);
	let Response::Clipboard(cut) = s.handle(Event::Cut { target: source(4) }, t) else { panic!("cut gives text") };
	assert_eq!(cut.matches("<Source ").count(), 2);
	assert_eq!(s.document.project.sources.len(), 4);
}

#[test]
fn settings_round_trip_and_first_start() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("cfg").join("settings.json");
	let facts =
		|| MachineFacts { physical_bytes: 64 << 30, threads: 16, has_gpu: Some(true), cache_drive_seek_penalty: None };
	let loaded = Settings::load(&path, facts);
	assert!(loaded.first_start && path.is_file());
	assert_eq!(loaded.settings.preview_sample_bytes, 2);
	let mut s = loaded.settings;
	s.language = "de".into();
	s.push_recent(dir.path());
	s.save(&path).unwrap();
	let again = Settings::load(&path, || panic!("not a first start"));
	assert!(!again.first_start);
	assert_eq!(again.settings, s);
	std::fs::write(&path, "{ not json").unwrap();
	let broken = Settings::load(&path, || panic!("an unreadable file is not a first start"));
	assert!(broken.unreadable.is_some());
	assert_eq!(broken.settings, Settings::default());
}

#[test]
fn language_switch_and_export_proposal() {
	let t = Instant::now();
	let dir = tempfile::tempdir().unwrap();
	let mut s = app();
	s.handle(Event::SetLanguage("fr".into()), t);
	assert_eq!(s.strings.language(), wavedude_app_state::strings::Language::French);
	let project = dir.path().join("p.wdr");
	s.handle(Event::SaveAs(project.clone()), t);
	assert_eq!(s.export_proposal(ExportKind::Run), dir.path().join("p_field"));
	let out = dir.path().join("videos").join("p_intensity.mp4");
	s.export_done(ExportKind::Run, &out);
	assert_eq!(s.export_proposal(ExportKind::Run), out);
	assert_eq!(s.settings.export_hint(&project).unwrap().run.as_deref(), Some(out.as_path()));
	assert_eq!(s.export_proposal(ExportKind::Still), dir.path().join("p"));
}

#[test]
fn log_reaches_file_and_ring() {
	use wavedude_app_state::log::{LogRing, init};
	let dir = tempfile::tempdir().unwrap();
	let ring = LogRing::new();
	let guard = init(dir.path(), ring.clone()).unwrap();
	tracing::info!(frame = 7, "preview frame requested");
	tracing::trace!("Source 1.frequency: 100.0 -> 120.0");
	drop(guard);
	// Other tests of this binary log into the same global subscriber: look at this test's lines only.
	let mine: Vec<_> = ring.since(None).into_iter().filter(|l| l.target == module_path!()).collect();
	assert_eq!(mine[0].message, "preview frame requested frame=7");
	assert!(mine[0].reaches_status_bar());
	assert_eq!(mine.len(), if cfg!(debug_assertions) { 2 } else { 1 }, "Trace only in debug builds");
	let file = std::fs::read_dir(dir.path()).unwrap().next().unwrap().unwrap().path();
	assert!(std::fs::read_to_string(file).unwrap().contains("preview frame requested"));
}

/// Opens `name` holding a 64 x 48 project with one source at the original's brightness, written by its extension.
/// Opens `p` written to `dir/name`; a `.wdproj` without its `Generator`, as WaveDude Next writes it.
fn open_written(dir: &std::path::Path, name: &str) -> AppState<FakeBackend> {
	let mut p = wavedude_core::model::Project::default();
	p.display.field.brightness = 30.0;
	p.display.intensity.brightness = 30.0;
	p.scene.width = 64;
	p.scene.height = 48;
	p.simulation.steps = 100;
	p.add_source();
	let path = dir.join(name);
	wavedude_core::io::save_project(&p, &path).unwrap();
	if name.ends_with(".wdproj") {
		let text = std::fs::read_to_string(&path).unwrap();
		let generator = format!(" Generator=\"{} {}\"", wdproj::GENERATOR, env!("CARGO_PKG_VERSION"));
		assert!(text.contains(&generator));
		std::fs::write(&path, text.replace(&generator, "")).unwrap();
	}
	let mut s = app();
	assert_eq!(s.handle(Event::Guarded(Guarded::Open(path)), Instant::now()), Response::None);
	s
}

#[test]
fn an_imported_project_gets_its_brightness_fitted_once() {
	let dir = tempfile::tempdir().unwrap();
	let mut s = open_written(dir.path(), "old.wdproj");
	let t = Instant::now();
	let original = s.document.project.display.clone();
	assert_eq!((original.field.brightness, original.intensity.brightness), (30.0, 30.0));
	s.tick(t);
	s.live.backend_mut().finish_scratch_uniform(0.0, 0.0);
	s.tick(at(t, 10));
	assert_eq!(s.document.project.display, original, "a dark frame has nothing to fit");
	s.live.scrub(20, at(t, 20));
	s.tick(at(t, 200));
	let requests = s.live.backend().scratch_requests.len();
	s.live.backend_mut().finish_scratch_uniform(0.5, 0.0625);
	s.tick(at(t, 210));
	assert_eq!(s.live.backend().scratch_requests.len(), requests + 1, "an 8-bit frame is computed again in 16-bit");
	s.tick(at(t, 220));
	assert_eq!(s.live.backend().scratch_requests.len(), requests + 1, "asked once per frame");
	s.live.backend_mut().finish_scratch_uniform(0.5, 0.0625);
	s.tick(at(t, 230));
	let d = &s.document.project.display;
	assert_eq!((d.field.brightness, d.intensity.brightness), (2.0, 4.0));
	assert!(!s.document.needs_brightness_fit);
	assert!(!s.document.dirty, "the fit does not make the project dirty");
	assert_eq!(s.document.history.len(), 1);
	assert!(!s.document.history.can_undo(), "the fit is no history step");
	s.live.scrub(30, at(t, 300));
	s.tick(at(t, 500));
	s.live.backend_mut().finish_scratch_uniform(0.1, 0.01);
	s.tick(at(t, 510));
	assert_eq!(s.document.project.display.field.brightness, 2.0, "fitted once");
	s.handle(Event::Command(Command::AddSource { at: None }), at(t, 600));
	s.handle(Event::Undo, at(t, 700));
	assert_eq!(s.document.project.display.field.brightness, 2.0, "an undo keeps the fit");
}

#[test]
fn the_open_fit_neither_follows_the_position_nor_shows_its_frame() {
	let dir = tempfile::tempdir().unwrap();
	let mut s = open_written(dir.path(), "old.wdproj");
	let t = Instant::now();
	s.live.scrub(20, t);
	s.tick(at(t, 200));
	s.live.backend_mut().finish_scratch_uniform(0.5, 0.0625);
	s.tick(at(t, 210));
	let requests = s.live.backend().scratch_requests.len();
	assert_eq!(s.live.shown(), Some(Shown::Scratch(20)));
	for (i, frame) in (30..80).step_by(10).enumerate() {
		s.live.scrub(frame, at(t, 220 + i as u64));
		s.tick(at(t, 220 + i as u64));
	}
	assert_eq!(s.live.backend().scratch_requests.len(), requests, "one fit request, not one per position");
	s.live.backend_mut().finish_scratch_uniform(0.5, 0.0625);
	s.tick(at(t, 300));
	assert!(!s.document.needs_brightness_fit, "fitted from its frame although the position moved");
	assert_eq!(s.live.shown(), Some(Shown::Scratch(20)), "the fit's frame is never shown");
}

#[test]
fn a_project_this_program_wrote_is_not_fitted() {
	let dir = tempfile::tempdir().unwrap();
	let own = open_written(dir.path(), "own.wdr");
	let mut saved = app();
	let path = dir.path().join("own.wdproj");
	saved.document.project = own.document.project.clone();
	saved.document.save_as(&path).unwrap();
	assert_eq!(saved.handle(Event::Guarded(Guarded::Open(path)), Instant::now()), Response::None);
	for mut s in [own, saved] {
		let t = Instant::now();
		assert!(!s.document.needs_brightness_fit);
		let before = s.document.project.display.clone();
		s.live.scrub(20, t);
		s.tick(at(t, 200));
		s.live.backend_mut().finish_scratch_uniform(0.5, 0.0625);
		s.tick(at(t, 210));
		s.tick(at(t, 220));
		assert_eq!(s.document.project.display, before);
	}
}

#[test]
fn a_playback_tick_moves_the_playback_revision_alone() {
	let t = Instant::now();
	let mut s = app();
	s.handle(Event::Run, t);
	s.live.backend().last_run().publish(50);
	s.live.backend().last_run().complete();
	s.tick(at(t, 10));
	s.handle(Event::Scrub(0), at(t, 20));
	s.handle(Event::Play, at(t, 30));
	assert!(s.live.playback.is_playing());
	s.tick(at(t, 40));
	let before = s.revisions();
	for i in 1..20 {
		s.tick(at(t, 40 + i * 17));
	}
	let after = s.revisions();
	assert!(after.playback > before.playback, "the position moved");
	assert_eq!(
		(after.document, after.scene, after.selection, after.settings),
		(before.document, before.scene, before.selection, before.settings)
	);
}

#[test]
fn the_scene_revision_follows_scene_and_part_edits_alone() {
	let t = Instant::now();
	let mut s = app();
	let scene = |s: &AppState<FakeBackend>| s.revisions().scene;
	let r0 = scene(&s);
	s.handle(Event::Command(Command::AddSource { at: None }), t);
	let set = |target, path: &str, value| Event::Command(Command::Set { target, path: path.into(), value });
	s.handle(set(Target::Item(source(0)), "frequency", json!(20.0)), at(t, 10));
	assert_eq!(scene(&s), r0, "sources are not the scene");
	s.handle(Event::Command(Command::AddPart { kind: NewPart::Lens, at: None }), at(t, 20));
	assert_eq!(scene(&s), r0 + 1);
	s.handle(set(Target::Scene, "refractive_index", json!(1.5)), at(t, 30));
	assert_eq!(scene(&s), r0 + 2);
	s.tick(at(t, 1000));
	s.handle(Event::Undo, at(t, 1100));
	assert_eq!(scene(&s), r0 + 3, "an undo of a scene edit");
}
