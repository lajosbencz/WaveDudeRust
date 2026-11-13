//! The window asleep while nothing moves and woken by the run's worker: a goal the worker takes up off the UI thread
//! wakes it. A test binary of its own, as the wake is the process's; no log subscriber runs here, so only the
//! worker wakes it.

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant as StdInstant};

use eframe::egui::{self, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use wavedude_app::ui::{Boot, Shell};
use wavedude_app_state::document::Source;
use wavedude_app_state::live::Backend;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::AppDirs;
use wavedude_app_state::{Event, Guarded};
use wavedude_core::run::WorkState;

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj";

static WAKES: AtomicUsize = AtomicUsize::new(0);

/// The window on Example01 with nothing run.
fn window(dir: &Path) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	for event in [Event::SetAutoRerun(false), Event::Guarded(Guarded::Open(Source::Path(EXAMPLE.into())))] {
		app.handle(event, Instant::now());
	}
	let boot = Boot {
		store: dirs.store(),
		gpu: None,
		open: Default::default(),
		log: LogRing::new(),
		update: Default::default(),
	};
	Harness::builder().with_size(vec2(1280., 800.)).build_eframe(move |cc| Shell::new(cc, app, boot))
}

fn repaint_delay(harness: &Harness<'_, Shell>) -> Duration {
	harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay
}

#[test]
fn a_goal_the_worker_takes_up_wakes_the_sleeping_window() {
	wavedude_app_state::wake::set(|| {
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
