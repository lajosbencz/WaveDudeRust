//! `WaveDude` Rust's Material window on egui-mineral, over [`waveduderust_app`] (DESIGN.md): natively a window, in the
//! browser on the page's canvas `#app`. Bin-only, so `dx serve --hotpatch` patches every UI function.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod commands;
mod export;
mod form;
mod look;
mod menus;
mod messages;
mod modal;
mod pages;
mod route;
mod shell;
mod status;
mod workspace;

use egui_mineral::semantic::{KeyParts, KeySalt};
use shell::Shell;
use waveduderust::model::{ItemKind, ItemRef};
use waveduderust::quality::PreviewTier;
use waveduderust_app::App;
use waveduderust_app::export::session::Target as ExportTarget;
use waveduderust_app::selection::Row;

/// The `Ui` the views show Material widgets in.
type Mui<'a> = egui_mineral::widget::MineralUi<'a, egui_mineral_material::Material>;

/// A WaveDude value as a widget salt: hashed as the value itself, keyed by its kind and id (`source-17`).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct Salt<T>(T);

impl Salt<ItemKind> {
	const fn word(self) -> &'static str {
		match self.0 {
			ItemKind::Source => "source",
			ItemKind::Part => "part",
			ItemKind::Probe => "probe",
			ItemKind::Scheme => "scheme",
			_ => "unknown",
		}
	}
}

impl KeySalt for Salt<ItemKind> {
	fn write_key(&self, out: &mut String) {
		out.push_str(self.word());
	}

	fn key_parts(&self, parts: &mut KeyParts) -> bool {
		parts.push_str(self.word())
	}
}

impl KeySalt for Salt<PreviewTier> {
	fn write_key(&self, out: &mut String) {
		out.push_str(match self.0 {
			PreviewTier::Low => "low",
			PreviewTier::Medium => "medium",
			PreviewTier::High => "high",
		});
	}
}

impl Salt<ItemRef> {
	/// The id's session-local number.
	fn number(self) -> u64 {
		match self.0 {
			ItemRef::Source(id) => id.get(),
			ItemRef::Part(id) => id.get(),
			ItemRef::Probe(id) => id.get(),
			ItemRef::Scheme(id) => id.get(),
			_ => 0,
		}
	}
}

impl KeySalt for Salt<ItemRef> {
	fn write_key(&self, out: &mut String) {
		(Salt(self.0.kind()), self.number()).write_key(out);
	}

	fn key_parts(&self, parts: &mut KeyParts) -> bool {
		(Salt(self.0.kind()), self.number()).key_parts(parts)
	}
}

impl KeySalt for Salt<Row> {
	fn write_key(&self, out: &mut String) {
		match self.0 {
			Row::Project => out.push_str("project"),
			Row::Scene => out.push_str("scene"),
			Row::Simulation => out.push_str("simulation"),
			Row::Display => out.push_str("display"),
			Row::Folder(kind) => ("folder", Salt(kind)).write_key(out),
			Row::Item(item) => Salt(item).write_key(out),
		}
	}

	fn key_parts(&self, parts: &mut KeyParts) -> bool {
		match self.0 {
			Row::Project => parts.push_str("project"),
			Row::Scene => parts.push_str("scene"),
			Row::Simulation => parts.push_str("simulation"),
			Row::Display => parts.push_str("display"),
			Row::Folder(kind) => ("folder", Salt(kind)).key_parts(parts),
			Row::Item(item) => Salt(item).key_parts(parts),
		}
	}
}

impl KeySalt for Salt<ExportTarget> {
	fn write_key(&self, out: &mut String) {
		match self.0 {
			ExportTarget::Output(i) => ("output", i).write_key(out),
			ExportTarget::Archive => out.push_str("archive"),
		}
	}

	fn key_parts(&self, parts: &mut KeyParts) -> bool {
		match self.0 {
			ExportTarget::Output(i) => ("output", i).key_parts(parts),
			ExportTarget::Archive => parts.push_str("archive"),
		}
	}
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
	if let Err(e) = native::run() {
		let message = format!("{e:#}");
		tracing::error!("{message}");
		eprintln!("{message}");
		rfd::MessageDialog::new()
			.set_level(rfd::MessageLevel::Error)
			.set_title(native::NAME)
			.set_description(&message)
			.show();
		std::process::exit(1);
	}
}

#[cfg(target_arch = "wasm32")]
fn main() -> Result<(), eframe::wasm_bindgen::JsValue> {
	use crate::Shell;
	use eframe::wasm_bindgen::JsCast as _;
	use waveduderust_app::AppState;
	use waveduderust_app::log::LogRing;
	use waveduderust_app::platform::{Store, init_log, install_panic_hook};
	use waveduderust_app::shell::Boot;
	use waveduderust_app::window::CANVAS;

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
	waveduderust_egui_platform::share_device(&mut options.wgpu_options);
	wasm_bindgen_futures::spawn_local(async move {
		let creator: eframe::AppCreator<'static> = Box::new(move |cc| {
			let ctx = cc.egui_ctx.clone();
			waveduderust_app::wake::set(move || ctx.request_repaint());
			let app = AppState::start(store.clone(), []);
			Ok(Box::new(Shell::new(cc, app, Boot { store, log, open: Default::default() })?))
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

#[cfg(not(target_arch = "wasm32"))]
mod native {
	use crate::Shell;
	use waveduderust_app::AppState;
	use waveduderust_app::log::LogRing;
	use waveduderust_app::platform::{init_log, install_panic_hook};
	use waveduderust_app::settings::{APP_FOLDER, AppDirs};
	use waveduderust_app::shell::Boot;
	use waveduderust_app::window::MIN_SIZE;
	use waveduderust_egui_platform::{os_theme, placement};

	pub(super) const NAME: &str = "WaveDude Rust";

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
		let open = waveduderust_app::open_requests::OpenRequests::default();
		if let Some(path) = args.iter().find(|a| !a.starts_with("--")) {
			open.push(path.into());
		}
		let icon =
			eframe::icon_data::from_png_bytes(waveduderust_app::about::WINDOW_ICON).expect("the bundled icon is a PNG");
		let mut options = eframe::NativeOptions {
			viewport: placement::viewport(&app.settings, MIN_SIZE)
				.with_title(app.title())
				.with_app_id("waveduderust-material")
				.with_icon(icon),
			centered: app.settings.window_left.is_none(),
			..Default::default()
		};
		waveduderust_egui_platform::share_device(&mut options.wgpu_options);
		let update = waveduderust_app::update::Pending::default();
		let pending = update.clone();
		let creator: eframe::AppCreator<'_> = Box::new(move |cc| {
			let ctx = cc.egui_ctx.clone();
			waveduderust_app::wake::set(move || ctx.request_repaint());
			if let Some(gpu) = waveduderust_egui_platform::renderer_gpu(cc) {
				waveduderust::unstable::engine::gpu::set_context(gpu);
			}
			Ok(Box::new(Shell::new(cc, app, Boot { store, log, open, update })?))
		});
		eframe::run_native(NAME, options, creator).map_err(|e| anyhow::anyhow!("the window failed: {e}"))?;
		pending.install(guard)
	}
}
