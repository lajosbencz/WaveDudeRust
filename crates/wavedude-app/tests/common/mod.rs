//! The state the window tests start from, the renderer of their image snapshots and the wait for the preview.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use eframe::{egui, egui_wgpu, wgpu};
use egui_kittest::wgpu::WgpuTestRenderer;
use egui_kittest::{Harness, HarnessBuilder};
use wavedude_app::App;
use wavedude_app::ui::Shell;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::AppDirs;
use wavedude_app_state::{AppState, Event};
use wavedude_core::run::Memory;

/// The memory the frame budgets are computed from, so the status bar's frame cache reads the same everywhere.
const MEMORY: Memory = Memory::Fixed { available: 6 << 30, physical: 16 << 30 };

/// The state on the settings under `dirs` with the startup page's questions answered, so the page shows at the
/// `first_start` only, whether or not OpenH264 is compiled in; runs on the CPU engine, so the pictures do not
/// depend on the machine's GPU.
pub(crate) fn started(dirs: &AppDirs, first_start: bool) -> App {
	let mut app = AppState::start_with_memory(dirs, [], MEMORY).expect("the state starts");
	app.first_start = first_start;
	for event in [Event::SetUseOpenH264(false), Event::SetCheckForUpdates(false), Event::SetUseGpu(false)] {
		app.handle(event, Instant::now());
	}
	app
}

/// A builder rendering on Mesa's lavapipe, the adapter the image snapshots were taken on (others rasterise beyond
/// their tolerance); `None`, with the reason printed, where it is missing, unless `WAVEDUDE_REQUIRE_LAVAPIPE` is set.
#[allow(dead_code, reason = "used by the tests with image snapshots")]
pub(crate) fn lavapipe<S>() -> Option<HarnessBuilder<S>> {
	let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
	setup.instance_descriptor.backends = wgpu::Backends::VULKAN;
	setup.native_adapter_selector = Some(Arc::new(|adapters, _| {
		adapters
			.iter()
			.find(|a| {
				let info = a.get_info();
				info.device_type == wgpu::DeviceType::Cpu && info.driver == "llvmpipe"
			})
			.cloned()
			.ok_or_else(|| "no lavapipe among the Vulkan adapters".to_owned())
	}));
	let setup = egui_wgpu::WgpuSetup::CreateNew(setup);
	let instance = pollster::block_on(setup.new_instance());
	let config = egui_wgpu::WgpuConfiguration { wgpu_setup: setup, ..Default::default() };
	match pollster::block_on(egui_wgpu::RenderState::create(
		&config,
		&instance,
		None,
		egui_wgpu::RendererOptions::PREDICTABLE,
	)) {
		Ok(state) => Some(Harness::builder().renderer(WgpuTestRenderer::from_render_state(state))),
		Err(e) if std::env::var_os("WAVEDUDE_REQUIRE_LAVAPIPE").is_some() => panic!("no lavapipe: {e}"),
		Err(e) => {
			// libtest captures `eprintln!`, not writes to the handle; it names the thread after the test
			let thread = std::thread::current();
			let test = thread.name().unwrap_or_default();
			let _ = writeln!(std::io::stderr(), "{test}: image snapshot skipped: {e}");
			None
		}
	}
}

/// Waits for the preview's frame, which comes from a thread of its own: the pane polls after 4 ms while its picture
/// is due, the window's tick is longer.
#[allow(dead_code, reason = "used by the tests with image snapshots")]
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
