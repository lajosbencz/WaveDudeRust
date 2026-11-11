//! The app state driven as the views will drive it, on the scripted backend and a fake clock.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde_json::json;
use wavedude_app_state::clipboard::PasteTarget;
use wavedude_app_state::commands::{Command, NewPart, Target};
use wavedude_app_state::history::History;
use wavedude_app_state::live::Notice;
use wavedude_app_state::live::fake::FakeBackend;
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::settings::Settings;
use wavedude_app_state::{AppState, Event, ExportKind, Guarded, Response, SaveChoice};
use wavedude_core::model::Point;
use wavedude_core::model::ProbeDetector;
use wavedude_core::run::MachineFacts;
use wavedude_core::run::Wanted;

fn app() -> AppState<FakeBackend> {
	let mut s =
		AppState::with_backend(Settings::default(), None, FakeBackend::default(), Arc::new(AtomicBool::new(false)));
	let t = Instant::now();
	s.document.project.scene.width = 64;
	s.document.project.scene.height = 48;
	s.document.project.simulation.steps = 100;
	s.document.history = History::new(&s.document.project);
	s.live.project_replaced(&s.document.project, t);
	s.tick(t);
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
	assert_eq!(s.selection.current(), Some(Row::Project), "nothing left: the user is put at the root");
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
fn edits_want_every_frame_until_the_setting_says_otherwise() {
	let t = Instant::now();
	let mut s = app();
	s.tick(t);
	assert_eq!(s.live.backend().wanted(), Wanted::Position, "an opened project wants its first frame alone");
	s.live.backend_mut().finish_position();
	s.tick(t);
	s.handle(Event::Command(Command::AddPart { kind: NewPart::Rectangle, at: Some(Point::new(10, 10)) }), at(t, 100));
	assert_eq!(s.live.backend().wanted(), Wanted::All, "an edit asks for every frame at once");
	s.tick(at(t, 110));
	assert!(s.run_status().starts_with("Running"), "{}", s.run_status());
	s.live.backend_mut().complete();
	s.tick(at(t, 120));
	assert!(s.run_status().starts_with("Completed"), "{}", s.run_status());
	s.handle(Event::SetAutoRerun(false), at(t, 1700));
	// The scripted backend's scene has no pixels, so a part edit leaves its frames; a source changes them.
	s.handle(Event::Command(Command::AddSource { at: None }), at(t, 1800));
	assert_eq!(s.live.backend().wanted(), Wanted::Position, "no re-run with the setting off");
	s.live.backend_mut().finish_position();
	s.tick(at(t, 1900));
	assert!(s.run_status().starts_with("Stopped"), "{}", s.run_status());
}

#[test]
fn user_run_follows_and_a_scrub_turns_live_off() {
	let t = Instant::now();
	let mut s = app();
	s.handle(Event::Run, t);
	assert!(s.live.is_live());
	s.live.backend_mut().publish(20);
	s.tick(at(t, 10));
	assert_eq!(s.live.playback.position(), 19);
	assert!(s.live.take_notices().contains(&Notice::ShowFrame(19)));
	s.handle(Event::Scrub(5), at(t, 20));
	assert!(!s.live.is_live() && !s.live.playback.is_playing());
	s.tick(at(t, 30));
	assert!(s.live.take_notices().contains(&Notice::ShowFrame(5)), "the store can show frame 5");
	s.handle(Event::SetInPoint, at(t, 40));
	assert_eq!(s.document.project.display.export_from, 10, "in point stored in steps");
	assert!(s.document.dirty);
}

#[test]
fn unsaved_edits_are_asked_about() {
	let t = Instant::now();
	let dir = tempfile::tempdir().unwrap();
	let mut s = app();
	s.handle(Event::Command(Command::AddProbe { at: None, detector: ProbeDetector::Standard }), t);
	assert_eq!(s.handle(Event::Guarded(Guarded::New), t), Response::AskSave { name: "Untitled".into() });
	assert_eq!(s.handle(Event::SaveChoice(SaveChoice::Cancel), t), Response::None);
	assert_eq!(s.document.project.probes.len(), 1, "Cancel keeps the project");
	s.handle(Event::Guarded(Guarded::OpenExample(6)), t);
	let ask = s.handle(Event::SaveChoice(SaveChoice::Save), t);
	assert_eq!(ask, Response::AskSavePath { proposed: None });
	let path = dir.path().join("mine.wdproj");
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
fn a_restart_asks_about_unsaved_edits_like_an_exit() {
	let t = Instant::now();
	let dir = tempfile::tempdir().unwrap();
	let mut s = app();
	assert_eq!(s.handle(Event::Guarded(Guarded::Restart), t), Response::Restart, "nothing to lose");
	s.handle(Event::Command(Command::AddProbe { at: None, detector: ProbeDetector::Standard }), t);
	assert_eq!(s.handle(Event::Guarded(Guarded::Restart), t), Response::AskSave { name: "Untitled".into() });
	assert_eq!(s.handle(Event::SaveChoice(SaveChoice::Cancel), t), Response::None, "Cancel keeps running");
	assert!(s.document.dirty);
	s.handle(Event::Guarded(Guarded::Restart), t);
	assert_eq!(s.handle(Event::SaveChoice(SaveChoice::Save), t), Response::AskSavePath { proposed: None });
	let path = dir.path().join("kept.wdproj");
	assert_eq!(s.handle(Event::SaveAs(path.clone()), t), Response::Restart, "restarts once saved");
	assert!(path.is_file() && !s.document.dirty);
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
	let project = dir.path().join("p.wdproj");
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

#[test]
fn a_playback_tick_moves_the_playback_revision_alone() {
	let t = Instant::now();
	let mut s = app();
	s.handle(Event::Run, t);
	s.live.backend_mut().complete();
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
	assert_eq!((after.document, after.selection, after.settings), (before.document, before.selection, before.settings));
}

#[test]
fn a_running_run_moves_the_progress_revision_alone() {
	let t = Instant::now();
	let mut s = app();
	s.handle(Event::Run, t);
	s.tick(at(t, 10));
	s.handle(Event::Scrub(0), at(t, 20));
	s.live.backend_mut().publish(1);
	s.tick(at(t, 200));
	s.tick(at(t, 210));
	let before = s.revisions();
	for i in 1..10u32 {
		s.live.backend_mut().publish(i * 3);
		s.tick(at(t, 210 + u64::from(i) * 17));
	}
	let after = s.revisions();
	assert!(after.progress > before.progress, "the run published frames");
	assert_eq!(
		(after.playback, after.run, after.document, after.settings),
		(before.playback, before.run, before.document, before.settings),
		"the views of the menus and the pictures stay"
	);
}

/// The app on the core's engines: Stop ends the run within half a second, and the position's frame shows
/// again after an edit.
#[test]
fn on_the_cpu_backend_stop_ends_the_run_and_an_edit_shows_the_position_again() {
	core_backend_round(false);
}

#[test]
fn on_the_gpu_backend_stop_ends_the_run_and_an_edit_shows_the_position_again() {
	if wavedude_core::engine::gpu::context().is_err() {
		eprintln!("no GPU: skipped");
		return;
	}
	core_backend_round(true);
}

fn core_backend_round(use_gpu: bool) {
	use wavedude_app_state::live::CoreBackend;
	use wavedude_core::run::RunState;
	let dir = tempfile::tempdir().unwrap();
	let gpu = Arc::new(AtomicBool::new(use_gpu));
	let backend = CoreBackend::new(gpu.clone(), dir.path().to_path_buf()).unwrap();
	let mut s = AppState::with_backend(Settings::default(), None, backend, gpu);
	let t = Instant::now();
	s.document.project.scene.width = 96;
	s.document.project.scene.height = 72;
	s.document.project.simulation.steps = 4000;
	s.document.project.simulation.visualize_every = 4;
	s.document.project.add_source();
	s.document.history = History::new(&s.document.project);
	s.live.project_replaced(&s.document.project, t);
	type Done = Box<dyn FnMut(&AppState<CoreBackend>) -> bool>;
	let until = |s: &mut AppState<CoreBackend>, mut done: Done| {
		let deadline = Instant::now() + Duration::from_secs(30);
		loop {
			s.tick(Instant::now());
			if done(s) {
				return;
			}
			assert!(Instant::now() < deadline, "status {:?}", s.live.run_status());
			std::thread::sleep(Duration::from_millis(5));
		}
	};
	until(&mut s, Box::new(|s| s.live.shown() == Some(0)));
	s.handle(Event::Run, Instant::now());
	until(&mut s, Box::new(|s| s.live.present_spans().first().is_some_and(|&(_, to)| to >= 50)));
	s.handle(Event::StopRun, Instant::now());
	let stopped = Instant::now();
	until(&mut s, Box::new(|s| s.live.run_status().is_some_and(|(state, ..)| state.is_finished())));
	assert!(stopped.elapsed() < Duration::from_millis(500), "stopped after {:?}", stopped.elapsed());
	assert_eq!(s.live.run_status().map(|r| r.0), Some(RunState::Cancelled));
	let spans = s.live.present_spans();
	assert!(spans.first().is_some_and(|&(from, to)| from == 0 && to >= 50), "the frames so far stay: {spans:?}");
	s.handle(Event::Scrub(20), Instant::now());
	s.handle(
		Event::Command(Command::Set { target: Target::Item(source(0)), path: "frequency".into(), value: json!(123.0) }),
		Instant::now(),
	);
	until(
		&mut s,
		Box::new(|s| matches!(s.live.shown_source(), Some(wavedude_app_state::live::ShownSource::Stored(_, 20)))),
	);
	assert_eq!(s.live.shown(), Some(20));
}

#[test]
fn a_device_switch_rebuilds_the_engine_and_the_same_quality_is_no_edit() {
	let mut s = app();
	let t = Instant::now();
	let on = s.settings.use_gpu;
	s.handle(Event::SetUseGpu(!on), t);
	assert_eq!(s.live.backend().engine_changes, 1);
	s.handle(Event::SetUseGpu(!on), t);
	assert_eq!(s.live.backend().engine_changes, 1, "the same device again changes nothing");
	let revision = s.live.backend().goal().revision;
	s.handle(Event::SetPreviewQuality(s.settings.preview_quality()), t);
	assert_eq!(s.live.backend().goal().revision, revision, "the quality in force is no edit");
}
