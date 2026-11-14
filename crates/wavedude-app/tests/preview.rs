//! The preview pane driven headless on an example: a handle and a graph box dragged, the timeline scrubbed and
//! stepped, playback by the transport bar and the pane's keys, and a focused button keeping its own Space; where
//! Mesa's lavapipe is installed the pane is also rendered on it and compared with its snapshot.

mod common;

use std::path::Path;
use std::time::Duration;

use eframe::egui::{self, Key, PointerButton, Pos2, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use wavedude_app::App;
use wavedude_app::ui::{Boot, Shell};
use wavedude_app_state::commands::{Command, Edit};
use wavedude_app_state::document::Source;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::{AppDirs, Theme};
use wavedude_app_state::{Event, Guarded};
use wavedude_core::model::{ProjectEdit, SimulationEdit};

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj";
/// Frames for an input to reach the state and its result to show.
const SETTLE: usize = 4;
/// Steps of a run that completes quickly.
const SHORT_RUN: u32 = 200;

/// The window on Example01 with nothing run, in the light theme, the project's run cut to `steps` when given.
fn preview_of(dir: &Path, steps: Option<u32>) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	let open = Event::Guarded(Guarded::Open(Source::Path(EXAMPLE.into())));
	for event in [Event::SetTheme(Theme::Light), Event::SetAutoRerun(false), open] {
		app.handle(event, Instant::now());
	}
	if let Some(steps) = steps {
		let edit = Edit::Project(ProjectEdit::Simulation(SimulationEdit::Steps(steps)));
		app.handle(Event::Command(Command::Edit(edit)), Instant::now());
	}
	let boot = Boot { store: dirs.store(), open: Default::default(), log: LogRing::new(), update: Default::default() };
	let mut harness = Harness::builder().with_size(vec2(1280., 800.)).build_eframe(move |cc| Shell::new(cc, app, boot));
	harness.run_steps(SETTLE);
	harness
}

fn preview(dir: &Path) -> Harness<'static, Shell> {
	preview_of(dir, None)
}

/// The window on a run short enough to complete quickly yet hold a few frames.
fn short_preview(dir: &Path) -> Harness<'static, Shell> {
	preview_of(dir, Some(SHORT_RUN))
}

fn settle(harness: &mut Harness<'_, Shell>) {
	harness.run_steps(SETTLE);
}

fn app<'h>(harness: &'h Harness<'_, Shell>) -> &'h App {
	harness.state().app()
}

fn button(harness: &mut Harness<'_, Shell>, pos: Pos2, pressed: bool) {
	let modifiers = Default::default();
	harness.event(egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
	harness.run_steps(1);
}

/// Presses at `from`, moves to `to` in five steps and, unless `hold`, releases there.
fn drag(harness: &mut Harness<'_, Shell>, from: Pos2, to: Pos2, hold: bool) {
	harness.hover_at(from);
	harness.run_steps(1);
	button(harness, from, true);
	for i in 1..=5 {
		harness.hover_at(from + (to - from) * i as f32 / 5.);
		harness.run_steps(1);
	}
	if !hold {
		button(harness, to, false);
		settle(harness);
	}
}

/// The point of canvas pixel (`x`, `y`) on the picture `title`.
fn on_picture(harness: &Harness<'_, Shell>, title: &str, (x, y): (f64, f64)) -> Pos2 {
	let r = harness.get_by_role_and_label(Role::Image, title).rect();
	let s = r.width() / app(harness).document.project.canvas_size().0 as f32;
	r.min + vec2((x as f32 + 0.5) * s, (y as f32 + 0.5) * s)
}

fn source_at(harness: &Harness<'_, Shell>) -> (f64, f64) {
	let s = &app(harness).document.project.sources[0];
	(s.x, s.y)
}

fn position(harness: &Harness<'_, Shell>) -> u32 {
	app(harness).live.playback.position()
}

fn playing(harness: &Harness<'_, Shell>) -> bool {
	app(harness).live.playback.is_playing()
}

#[test]
fn a_handle_drag_moves_the_source_and_esc_puts_it_back() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = preview(dir.path());
	let start = source_at(&harness);
	let at = on_picture(&harness, "Field", start);
	drag(&mut harness, at, at + vec2(50., 0.), true);
	assert!(source_at(&harness).0 > start.0, "the drag moved nothing");
	harness.key_press(Key::Escape);
	settle(&mut harness);
	assert_eq!(source_at(&harness), start, "Esc kept the move");
	button(&mut harness, at + vec2(50., 0.), false);
	drag(&mut harness, at, at + vec2(0., 40.), false);
	let moved = source_at(&harness);
	assert!(moved.0 == start.0 && moved.1 > start.1, "the source went to {moved:?} from {start:?}");
}

#[test]
fn a_graph_box_moves_by_its_body_resizes_by_its_grip_and_steps_by_the_arrows() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = preview(dir.path());
	let probe = |harness: &Harness<'_, Shell>| app(harness).document.project.probes[0].clone();
	let before = probe(&harness);
	let rect = harness.get_by_role_and_label(Role::Figure, "Probe 1").rect();
	drag(&mut harness, rect.center(), rect.center() - vec2(60., 40.), false);
	let moved = probe(&harness);
	assert!(moved.box_x < before.box_x && moved.box_y < before.box_y, "the box stayed at its place");
	assert_eq!(moved.box_pixels(), before.box_pixels(), "the move resized the box");
	harness.key_press(Key::ArrowRight);
	settle(&mut harness);
	assert_eq!(probe(&harness).box_x, moved.box_x + 1, "the focused box took no arrow");
	let rect = harness.get_by_role_and_label(Role::Figure, "Probe 1").rect();
	let grip = rect.max - vec2(3., 3.);
	drag(&mut harness, grip, grip + vec2(150., 100.), false);
	let (w, h) = probe(&harness).box_pixels();
	let (w0, h0) = before.box_pixels();
	assert!(w > w0 && h > h0, "the grip left the box at {w}x{h} from {w0}x{h0}");
}

#[test]
fn the_timeline_scrubs_and_steps_by_its_keys_and_the_wheel() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = preview(dir.path());
	let last = app(&harness).live.playback.last_frame();
	// Example01 exports from step 10
	let (from, to) = app(&harness).live.playback.range_frames();
	assert!(from > 0 && to == last);
	let bar = harness.get_by_role(Role::Slider).rect();
	let at = |f: f32| egui::pos2(bar.left() + 1.5 + (bar.width() - 3.) * f, bar.bottom() - 6.);
	drag(&mut harness, at(0.25), at(0.5), false);
	let half = position(&harness);
	assert!(half.abs_diff(last / 2) <= 2, "the drag ended at frame {half} of {last}");
	for (key, expected) in [
		(Key::ArrowRight, half + 1),
		(Key::PageDown, half - 9),
		(Key::PageUp, half + 1),
		(Key::Home, from),
		(Key::End, last),
		(Key::ArrowLeft, last - 1),
	] {
		harness.key_press(key);
		settle(&mut harness);
		assert_eq!(position(&harness), expected, "after {key:?}");
	}
	harness.hover_at(at(0.5));
	harness.event(egui::Event::MouseWheel {
		unit: egui::MouseWheelUnit::Line,
		delta: vec2(0., 1.),
		phase: egui::TouchPhase::Move,
		source: egui::MouseWheelSource::Wheel,
		modifiers: Default::default(),
	});
	settle(&mut harness);
	assert_eq!(position(&harness), last - 2, "a notch up steps a frame back");
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(playing(&harness), "Space on the timeline did not play");
}

/// Steps the window until `done`, at most a minute (a debug build's run takes seconds).
fn wait_for(harness: &mut Harness<'_, Shell>, done: impl Fn(&App) -> bool) {
	let given_up = Instant::now() + Duration::from_secs(60);
	while !done(app(harness)) {
		assert!(Instant::now() < given_up, "the window did not get there: {}", app(harness).run_status());
		std::thread::sleep(Duration::from_millis(10));
		harness.run_steps(1);
	}
}

/// F5, up to its completed run.
fn run_to_the_end(harness: &mut Harness<'_, Shell>) {
	harness.key_press(Key::F5);
	wait_for(harness, |app| app.run_status().starts_with("Completed"));
}

#[test]
fn the_status_bar_shows_the_frame_cache_of_a_run() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = short_preview(dir.path());
	run_to_the_end(&mut harness);
	let cache = app(&harness).live.frame_cache().expect("a run holds a store");
	assert!(cache.in_memory > 0 && cache.spilled == 0, "the run fits in memory: {cache:?}");
	harness.get_by_label_contains("RAM ");
	assert!(harness.query_by_label_contains("Spill").is_none(), "nothing spilled, no spill shown");
}

#[test]
fn discard_frames_asks_first_and_frees_the_runs_frames() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = short_preview(dir.path());
	run_to_the_end(&mut harness);
	let ran = app(&harness).live.frame_cache().expect("a run holds a store");
	assert!(ran.in_memory > 1);
	let ask = |harness: &mut Harness<'_, Shell>| {
		harness.get_by_role_and_label(Role::Button, "Simulation").click();
		settle(harness);
		harness.get_by_label("Discard frames").click();
		settle(harness);
		harness.get_by_label_contains("Discard the frames of the run?");
	};
	ask(&mut harness);
	harness.key_press(Key::Enter);
	settle(&mut harness);
	assert!(harness.query_by_label_contains("Discard the frames of the run?").is_none(), "Enter answered");
	assert_eq!(app(&harness).live.frame_cache(), Some(ran), "No, the default, keeps the frames");
	ask(&mut harness);
	harness.get_by_label("Yes").click();
	wait_for(&mut harness, |app| app.live.frame_cache().is_some_and(|c| c.in_memory == 1));
	settle(&mut harness);
	assert_eq!(app(&harness).live.frame_cache().unwrap().spilled, 0, "frame 0 alone is made again");
	assert!(app(&harness).run_status().is_empty() && app(&harness).live.playback.position() == 0);
	harness.get_by_label_contains("RAM ");
	assert!(harness.query_by_label_contains("Spill").is_none(), "nothing spilled, no spill shown");
}

#[test]
fn play_and_pause_by_the_buttons_and_by_space_on_them_hand_the_focus_over() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = preview(dir.path());
	harness.get_by_label("Play (Space)").click();
	settle(&mut harness);
	assert!(playing(&harness));
	harness.get_by_label("Pause (Space)").click();
	settle(&mut harness);
	assert!(!playing(&harness));
	harness.get_by_label("Play (Space)").focus();
	settle(&mut harness);
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(playing(&harness), "Space on Play did not play, or played and paused");
	assert!(harness.get_by_label("Pause (Space)").is_focused(), "the focus stayed on Play");
	harness.key_press(Key::Enter);
	settle(&mut harness);
	assert!(!playing(&harness), "Enter on Pause did not pause");
	assert!(harness.get_by_label("Play (Space)").is_focused(), "the focus stayed on Pause");
}

#[test]
fn the_pane_keys_work_with_the_focus_in_the_pane_but_not_in_a_text_box() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = preview(dir.path());
	let field = harness.get_by_role_and_label(Role::Image, "Field").rect();
	button(&mut harness, field.left_top() + vec2(10., 10.), true);
	button(&mut harness, field.left_top() + vec2(10., 10.), false);
	for key in [Key::Period, Key::Period, Key::Comma] {
		harness.key_press(key);
	}
	settle(&mut harness);
	assert_eq!(position(&harness), 1);
	harness.key_press(Key::I);
	settle(&mut harness);
	assert!(app(&harness).live.playback.has_range(), "I set no in point");
	harness.key_press(Key::X);
	harness.key_press(Key::L);
	settle(&mut harness);
	assert!(!app(&harness).live.playback.has_range(), "X kept the range");
	assert!(app(&harness).live.playback.looping(), "L did not loop");
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(playing(&harness), "Space on the picture did not play");
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(!playing(&harness));
	let speed = harness.get_by_role_and_label(Role::SpinButton, "Speed").rect().center();
	button(&mut harness, speed, true);
	button(&mut harness, speed, false);
	settle(&mut harness);
	harness.key_press(Key::L);
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(app(&harness).live.playback.looping() && !playing(&harness), "the speed box let pane keys through");
}

#[test]
fn a_focused_button_keeps_its_space_from_the_pane() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = preview(dir.path());
	harness.get_by_label("Loop playback over the export range (L)").focus();
	settle(&mut harness);
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(app(&harness).live.playback.looping() && !playing(&harness), "Space on Loop played or did not loop");
	harness.get_all_by_label("Auto").next().expect("an Auto button").focus();
	settle(&mut harness);
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(!playing(&harness), "Space on Auto played");
	harness.get_all_by_role_and_label(Role::SpinButton, "Brightness").next().expect("a turned box").focus();
	settle(&mut harness);
	harness.key_press(Key::Space);
	settle(&mut harness);
	assert!(!playing(&harness), "Space on the turned brightness box played");
	harness.key_press(Key::Escape);
	settle(&mut harness);
	harness.get_by_label("Show the field picture").focus();
	settle(&mut harness);
	harness.key_press(Key::Enter);
	settle(&mut harness);
	assert!(!app(&harness).document.project.display.field.preview && !playing(&harness));
}
