//! The preview pane driven headless on an example: a handle dragged and put back by Esc, and discarding the run's
//! frames.

use super::common;

use std::path::Path;
use std::time::Duration;

use crate::ui::Shell;
use eframe::egui::{Key, Pos2, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use waveduderust::model::{ProjectEdit, SimulationEdit};
use waveduderust_app::App;
use waveduderust_app::commands::{Command, Edit};
use waveduderust_app::document::Source;
use waveduderust_app::platform::Instant;
use waveduderust_app::settings::{AppDirs, Theme};
use waveduderust_app::{Event, Guarded};

use common::{SETTLE, app, boot, button, settle};

const EXAMPLE: &str = "../../crates/waveduderust/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj";
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
	let boot = boot(&dirs);
	let mut harness = Harness::builder()
		.with_size(vec2(1280., 800.))
		.build_eframe(move |cc| Shell::new(cc, app, boot).expect("the UI runtime starts"));
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
