//! `WaveDude` Rust: the desktop application.
// A GUI program on Windows: no console window opens with it. Its stdout and stderr lead nowhere there, so a
// failure before the window shows is reported in a native message box, and the log file holds everything.
#![cfg_attr(windows, windows_subsystem = "windows")]

use freya::prelude::*;
use wavedude_app::ui::app::{Boot, BootInfo, CloseGate, MIN_SIZE, root, theme_background};
use wavedude_app::ui::open_requests::OpenRequests;
use wavedude_app::ui::placement::{self, Placement};
use wavedude_app_state::AppState;
use wavedude_app_state::log::{self, LogRing};
use wavedude_app_state::settings::AppDirs;

const ICON: &[u8] = include_bytes!("../assets/icons/256x256.png");

fn main() {
	if let Err(e) = run() {
		let message = format!("{e:#}");
		tracing::error!("{message}");
		eprintln!("{message}");
		rfd::MessageDialog::new()
			.set_level(rfd::MessageLevel::Error)
			.set_title("WaveDude Rust")
			.set_description(&message)
			.show();
		std::process::exit(1);
	}
}

/// Logging, settings, the state and the window; an error here is one the user has to be told of.
fn run() -> anyhow::Result<()> {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let platform = AppDirs::platform();
	let dirs = platform
		.clone()
		.unwrap_or_else(|| AppDirs::under(&std::env::temp_dir().join(wavedude_app_state::settings::APP_FOLDER)));
	let ring = LogRing::new();
	let _guard = log::init(&dirs.logs(), ring.clone())?;
	log::install_panic_hook(&dirs.logs());
	if platform.is_none() {
		tracing::warn!("the platform names no home folder: settings and logs go to '{}'", dirs.config.display());
	}
	for unknown in args.iter().filter(|a| a.starts_with("--") && !a.starts_with("--zoom=")) {
		tracing::warn!("unknown switch '{unknown}'");
	}
	let _os_theme = wavedude_app::ui::os_theme::watch();
	let app = AppState::start(&dirs, args.iter().map(String::as_str))?;
	let open = OpenRequests::default();
	if let Some(path) = args.iter().find(|a| !a.starts_with("--")) {
		open.push(path.into());
	}
	let (zoom, width, height, title, theme) =
		(app.settings.ui_zoom, app.settings.window_width, app.settings.window_height, app.title(), app.settings.theme);
	let close = CloseGate::default();
	let update = wavedude_app::ui::update::Pending::default();
	let placement = Placement::default();
	let settings = app.settings.clone();
	let info = BootInfo {
		ring,
		dirs,
		close: close.clone(),
		open: open.clone(),
		update: update.clone(),
		placement: placement.clone(),
	};
	let boot = Boot::new(app, info);
	let title: &'static str = Box::leak(title.into_boxed_str());
	let window = WindowConfig::new(root)
		.with_root_context(boot)
		.with_title(title)
		.with_size(width * zoom, height * zoom)
		.with_min_size(MIN_SIZE.0 * zoom, MIN_SIZE.1 * zoom)
		.with_window_attributes(move |attrs, _| placement::attributes(&settings, attrs))
		.with_custom_scale_factor(zoom)
		.with_background(theme_background(theme))
		// the name of the .desktop file cargo-packager installs, so Wayland and X11 find the icon
		.with_app_id("wavedude-app")
		.with_icon(LaunchConfig::window_icon(ICON))
		.with_on_close(
			move |_, _| if close.on_close_request() { CloseDecision::Close } else { CloseDecision::KeepOpen },
		);
	let config = LaunchConfig::new().with_window(window).with_plugin(placement.plugin());
	#[cfg(target_os = "macos")]
	let (config, _delegate) = {
		let event_loop = freya::winit::event_loop::EventLoop::<NativeEvent>::with_user_event().build()?;
		let delegate = wavedude_app::ui::open_requests::install(&open);
		(config.with_event_loop(event_loop), delegate)
	};
	launch(config);
	update.install();
	Ok(())
}
