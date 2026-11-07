//! WaveDude Rust: the desktop application.

use freya::prelude::*;
use wave_app::ui::app::{Boot, BootInfo, CloseGate, root, theme_background};
use wave_app_state::AppState;
use wave_app_state::log::{self, LogRing};
use wave_app_state::settings::AppDirs;

const ICON: &[u8] = include_bytes!("../assets/icon.png");
const MIN_SIZE: (f64, f64) = (800.0, 500.0);

fn main() -> anyhow::Result<()> {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let platform = AppDirs::platform();
	let dirs = platform.clone().unwrap_or_else(|| {
		let temp = std::env::temp_dir().join("WaveDude");
		AppDirs { config: temp.clone(), cache: temp }
	});
	let ring = LogRing::new();
	let _guard = log::init(&dirs.config.join("logs"), ring.clone())?;
	if platform.is_none() {
		tracing::warn!("the platform names no home folder: settings and logs go to '{}'", dirs.config.display());
	}
	for unknown in args.iter().filter(|a| a.starts_with("--") && !a.starts_with("--zoom=")) {
		tracing::warn!("unknown switch '{unknown}'");
	}
	let app = AppState::start(&dirs, args.iter().map(String::as_str))?;
	let open = args.iter().find(|a| !a.starts_with("--")).map(Into::into);
	let (zoom, width, height, title, theme) =
		(app.settings.ui_zoom, app.settings.window_width, app.settings.window_height, app.title(), app.settings.theme);
	let close = CloseGate::default();
	let info = BootInfo { ring, dirs, close: close.clone(), open };
	let boot = Boot::new(app, info);
	let title: &'static str = Box::leak(title.into_boxed_str());
	let window = WindowConfig::new(root)
		.with_root_context(boot)
		.with_title(title)
		.with_size(width * zoom, height * zoom)
		.with_min_size(MIN_SIZE.0, MIN_SIZE.1)
		.with_custom_scale_factor(zoom)
		.with_background(theme_background(theme))
		.with_app_id("wavedude")
		.with_icon(LaunchConfig::window_icon(ICON))
		.with_on_close(
			move |_, _| if close.on_close_request() { CloseDecision::Close } else { CloseDecision::KeepOpen },
		);
	launch(LaunchConfig::new().with_window(window));
	Ok(())
}
