//! `WaveDude` Rust: the desktop application, and the same window on a page's canvas in the browser.
// A GUI program on Windows: no console window opens with it. Its stdout and stderr lead nowhere there, so a
// failure at start is reported in a native message box, and the log file holds everything.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() {
	if let Err(e) = native::run() {
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

#[cfg(not(target_arch = "wasm32"))]
mod native {
	use wavedude_app::open_requests::OpenRequests;
	use wavedude_app::ui::{self, Boot, Shell};
	use wavedude_app::update::Pending;
	use wavedude_app::window::MIN_SIZE;
	use wavedude_app::{os_theme, placement};
	use wavedude_app_state::AppState;
	use wavedude_app_state::log::LogRing;
	use wavedude_app_state::platform::{init_log, install_panic_hook};
	use wavedude_app_state::settings::{APP_FOLDER, AppDirs};

	const NAME: &str = "WaveDude Rust";
	const ICON: &[u8] = include_bytes!("../assets/icons/256x256.png");

	/// Logging, settings, the state and the window; an error here is one the user has to be told of.
	pub(super) fn run() -> anyhow::Result<()> {
		let args: Vec<String> = std::env::args().skip(1).collect();
		let platform = AppDirs::platform();
		let dirs = platform.clone().unwrap_or_else(|| AppDirs::under(&std::env::temp_dir().join(APP_FOLDER)));
		let store = dirs.store();
		let log = LogRing::new();
		let guard = init_log(&store, log.clone())?;
		install_panic_hook(&store);
		if platform.is_none() {
			tracing::warn!("the platform names no home folder: settings and logs go to '{}'", dirs.config.display());
		}
		for unknown in args.iter().filter(|a| a.starts_with("--") && !a.starts_with("--zoom=")) {
			tracing::warn!("unknown switch '{unknown}'");
		}
		let _os_theme = os_theme::watch();
		let app = AppState::start(&dirs, args.iter().map(String::as_str))?;
		let open = OpenRequests::default();
		if let Some(path) = args.iter().find(|a| !a.starts_with("--")) {
			open.push(path.into());
		}
		let icon = eframe::icon_data::from_png_bytes(ICON).expect("the bundled icon is a PNG");
		let mut options = eframe::NativeOptions {
			viewport: placement::viewport(&app.settings, MIN_SIZE)
				.with_title(app.title())
				// the name of the .desktop file cargo-packager installs, so Wayland and X11 find the icon
				.with_app_id("wavedude-app")
				.with_icon(icon),
			centered: app.settings.window_left.is_none(),
			..Default::default()
		};
		ui::share_device(&mut options.wgpu_options);
		#[cfg(target_os = "macos")]
		let finder = open.clone();
		let update = Pending::default();
		let pending = update.clone();
		let creator: eframe::AppCreator<'_> = Box::new(move |cc| {
			let ctx = cc.egui_ctx.clone();
			wavedude_app_state::wake::set(move || ctx.request_repaint());
			if let Some(gpu) = ui::renderer_gpu(cc) {
				wavedude_core::engine::gpu::set_context(gpu);
			}
			Ok(Box::new(Shell::new(cc, app, Boot { store, log, open, update })))
		});
		#[cfg(not(target_os = "macos"))]
		eframe::run_native(NAME, options, creator).map_err(|e| anyhow::anyhow!("the window failed: {e}"))?;
		// The Finder's open requests reach the application delegate, which is set once the event loop is built.
		#[cfg(target_os = "macos")]
		{
			let event_loop = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event().build()?;
			let _delegate = wavedude_app::open_requests::install(&finder);
			let mut window = eframe::create_native(NAME, options, creator, &event_loop);
			event_loop.run_app(&mut window)?;
		}
		pending.install(guard)
	}
}

/// The page's start: logs to the console, settings in local storage, the window on the canvas `#app`. An error is
/// thrown to the page.
#[cfg(target_arch = "wasm32")]
fn main() -> Result<(), eframe::wasm_bindgen::JsValue> {
	use eframe::wasm_bindgen::JsCast as _;
	use wavedude_app::ui::{self, Boot, CANVAS, Shell};
	use wavedude_app_state::AppState;
	use wavedude_app_state::log::LogRing;
	use wavedude_app_state::platform::{Store, init_log, install_panic_hook};

	let store = Store::open().ok_or("the browser keeps no local storage for the settings")?;
	let log = LogRing::new();
	init_log(&store, log.clone()).map_err(|e| e.to_string())?;
	install_panic_hook(&store);
	let canvas = eframe::web_sys::window()
		.and_then(|w| w.document())
		.and_then(|d| d.get_element_by_id(CANVAS))
		.and_then(|e| e.dyn_into::<eframe::web_sys::HtmlCanvasElement>().ok())
		.ok_or("the page has no canvas #app")?;
	let mut options = eframe::WebOptions::default();
	ui::share_device(&mut options.wgpu_options);
	wasm_bindgen_futures::spawn_local(async move {
		let creator: eframe::AppCreator<'static> = Box::new(move |cc| {
			let ctx = cc.egui_ctx.clone();
			wavedude_app_state::wake::set(move || ctx.request_repaint());
			let app = AppState::start(store.clone(), []);
			Ok(Box::new(Shell::new(cc, app, Boot { store, log })))
		});
		if let Err(e) = eframe::WebRunner::new().start(canvas, options, creator).await {
			let message = format!("WaveDude Rust could not start: {e:?}");
			tracing::error!("{message}");
			if let Some(body) = eframe::web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
				body.set_text_content(Some(&message));
			}
		}
	});
	Ok(())
}
