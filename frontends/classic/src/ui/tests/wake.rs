//! The window asleep while nothing moves and woken by the run's worker: a goal the worker takes up off the UI thread
//! wakes it. A test binary of its own, as the wake is the process's; no log subscriber runs here, so only the
//! worker wakes it.

use super::common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant as StdInstant};

use crate::ui::Shell;
use eframe::egui::{self, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use waveduderust::model::{ProjectEdit, SimulationEdit};
use waveduderust::run::WorkState;
use waveduderust_app::commands::{Command, Edit};
use waveduderust_app::document::Source;
use waveduderust_app::live::Backend;
use waveduderust_app::platform::Instant;
use waveduderust_app::settings::AppDirs;
use waveduderust_app::{Event, Guarded};

use common::boot;

const EXAMPLE: &str = "../../crates/waveduderust/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj";

static WAKES: AtomicUsize = AtomicUsize::new(0);

/// The window on Example01 cut to a short run, with nothing run.
fn window(dir: &Path) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	let steps = Edit::Project(ProjectEdit::Simulation(SimulationEdit::Steps(200)));
	for event in [
		Event::SetAutoRerun(false),
		Event::Guarded(Guarded::Open(Source::Path(EXAMPLE.into()))),
		Event::Command(Command::Edit(steps)),
	] {
		app.handle(event, Instant::now());
	}
	// the edit's undo step stays open, and the window ticking, until the history is quiet
	std::thread::sleep(waveduderust_app::history::QUIET);
	let boot = boot(&dirs);
	Harness::builder()
		.with_size(vec2(1280., 800.))
		.build_eframe(move |cc| Shell::new(cc, app, boot).expect("the UI runtime starts"))
}

fn repaint_delay(harness: &Harness<'_, Shell>) -> Duration {
	harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay
}

#[test]
fn a_goal_the_worker_takes_up_wakes_the_sleeping_window() {
	waveduderust_app::wake::set(|| {
		WAKES.fetch_add(1, Ordering::Relaxed);
	});
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	common::preview_idle(&mut harness);
	harness.run_steps(1);
	assert_eq!(repaint_delay(&harness), Duration::MAX, "the window ticks while nothing moves");
	let before = WAKES.load(Ordering::Relaxed);
	// a click on the timeline's middle: a frame not stored yet
	harness.get_by_role(Role::Slider).click();
	harness.run_steps(1);
	let given_up = StdInstant::now() + Duration::from_secs(60);
	let done = |harness: &Harness<'_, Shell>| {
		let live = &harness.state().app().live;
		WAKES.load(Ordering::Relaxed) > before && live.backend().status().state == WorkState::Idle
	};
	// the window is not stepped: only a wake would make it look again
	while !done(&harness) {
		assert!(StdInstant::now() < given_up, "the worker's work woke no window");
		std::thread::sleep(Duration::from_millis(10));
	}
	common::preview_idle(&mut harness);
	assert!(harness.state().app().live.shown().is_some_and(|f| f > 0), "the frame clicked shows");
}
