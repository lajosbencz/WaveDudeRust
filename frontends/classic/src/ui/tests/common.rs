//! The state and the window the window tests start from, the steps that let an input settle and the wait for the
//! preview.

use std::time::Duration;

use crate::ui::Shell;
use eframe::egui::{self, PointerButton, Pos2};
use egui_kittest::Harness;
use waveduderust::run::Memory;
use waveduderust_app::App;
use waveduderust_app::log::LogRing;
use waveduderust_app::platform::Instant;
use waveduderust_app::settings::AppDirs;
use waveduderust_app::shell::Boot;
use waveduderust_app::{AppState, Event};

/// Frames for an input to reach the state and its result to show.
pub(crate) const SETTLE: usize = 4;

/// The memory the frame budgets are computed from, so the status bar's frame cache reads the same everywhere.
const MEMORY: Memory = Memory::Fixed { available: 6 << 30, physical: 16 << 30 };

/// The state on the settings under `dirs` with OpenH264 and the update check off and the welcome page's launch display
/// on at the `first_start` only, so the page shows then only; runs on the CPU engine, so the pictures do not depend on
/// the machine's GPU.
pub(crate) fn started(dirs: &AppDirs, first_start: bool) -> App {
	let mut app = AppState::start_with_memory(dirs, [], MEMORY).expect("the state starts");
	app.first_start = first_start;
	for event in [
		Event::SetUseOpenH264(false),
		Event::SetCheckForUpdates(false),
		Event::SetShowStartup(first_start),
		Event::SetUseGpu(false),
	] {
		app.handle(event, Instant::now());
	}
	app
}

/// What the window starts with on the settings under `dirs`: no file asked for, a log of its own.
pub(crate) fn boot(dirs: &AppDirs) -> Boot {
	Boot { store: dirs.store(), open: Default::default(), log: LogRing::new(), update: Default::default() }
}

/// Runs [`SETTLE`] frames.
#[allow(dead_code, reason = "not every window test steps by inputs")]
pub(crate) fn settle(harness: &mut Harness<'_, Shell>) {
	harness.run_steps(SETTLE);
}

/// The state the window shows.
#[allow(dead_code, reason = "not every window test reads the state")]
pub(crate) fn app<'h>(harness: &'h Harness<'_, Shell>) -> &'h App {
	harness.state().app()
}

/// Presses (`pressed`) or releases the primary button at `pos`, one frame.
#[allow(dead_code, reason = "used by the pointer tests")]
pub(crate) fn button(harness: &mut Harness<'_, Shell>, pos: Pos2, pressed: bool) {
	let modifiers = Default::default();
	harness.event(egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
	harness.run_steps(1);
}

/// Waits for the preview's frame, which comes from a thread of its own: the pane polls after 4 ms while its picture
/// is due, the window's tick is longer.
#[allow(dead_code, reason = "used by the wake tests")]
pub(crate) fn preview_idle(harness: &mut Harness<'_, Shell>) {
	let polling = |harness: &Harness<'_, Shell>| {
		harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay <= Duration::from_millis(4)
	};
	let mut idle = 0;
	for _ in 0..500 {
		std::thread::sleep(Duration::from_millis(10));
		harness.run_steps(1);
		let live = &harness.state().app().live;
		idle =
			if polling(harness) || live.shown().is_none() || live.computing_frame().is_some() { 0 } else { idle + 1 };
		if idle == 3 {
			return;
		}
	}
	panic!("the preview kept polling");
}
