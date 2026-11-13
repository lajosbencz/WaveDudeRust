//! The browser's side of the window: the page's colour scheme, the clipboard read for Paste and the browser's own
//! actions of the shortcuts' and the menus' keys kept off; the page places the canvas.

use std::cell::RefCell;

use eframe::egui::os::OperatingSystem;
use eframe::egui::{self, Key, Modifiers};
use eframe::wasm_bindgen::JsCast as _;
use eframe::wasm_bindgen::closure::Closure;
use eframe::web_sys::{self, KeyboardEvent};
use wasm_bindgen_futures::JsFuture;
use wavedude_app_state::settings::Settings;

thread_local! {
	/// The clipboard's text read for the last Paste, until the next frame takes it.
	static PASTED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The page's colour scheme, as eframe reads it.
pub(super) fn system_theme(ctx: &egui::Context) -> Option<egui::Theme> {
	ctx.system_theme()
}

/// The page's title, which eframe leaves to the page.
pub(super) fn set_title(_ctx: &egui::Context, title: &str) {
	if let Some(document) = web_sys::window().and_then(|w| w.document()) {
		document.set_title(title);
	}
}

/// The canvas follows the page, not the zoom.
pub(super) fn rezoom(_frame: &eframe::Frame, _ratio: f64, _zoom: f64) {}

/// The page has no window place to keep.
pub(super) fn remember_place(_frame: &eframe::Frame, _settings: &mut Settings) {}

/// Paste stays enabled: the browser reads the clipboard only asynchronously and with the user's permission.
pub(super) fn pastable() -> bool {
	true
}

/// Starts reading the clipboard's text for [`pasted`], asking the user where the browser does.
pub(super) fn read_clipboard(ctx: &egui::Context) -> Option<String> {
	let window = web_sys::window()?;
	let read = window.navigator().clipboard().read_text();
	let ctx = ctx.clone();
	wasm_bindgen_futures::spawn_local(async move {
		match JsFuture::from(read).await {
			Ok(text) => {
				PASTED.set(text.as_string());
				ctx.request_repaint();
			}
			Err(e) => tracing::debug!("nothing to paste: {e:?}"),
		}
	});
	None
}

/// The clipboard's text read since the last frame.
pub(super) fn pasted() -> Option<String> {
	PASTED.take()
}

/// Keeps the browser from acting on the shortcuts' keys and Alt+letter (where it lets a page), before eframe sees
/// them; `os` is the user's, whose command key is Cmd on macOS and Ctrl elsewhere.
pub(super) fn guard_shortcuts(os: OperatingSystem) {
	let Some(window) = web_sys::window() else { return };
	let mac = os == OperatingSystem::Mac;
	let guard = Closure::<dyn Fn(KeyboardEvent)>::new(move |e: KeyboardEvent| {
		let modifiers = Modifiers {
			alt: e.alt_key(),
			ctrl: e.ctrl_key(),
			shift: e.shift_key(),
			mac_cmd: mac && e.meta_key(),
			command: if mac { e.meta_key() } else { e.ctrl_key() },
		};
		let key = Key::from_name(&e.key()).or_else(|| Key::from_name(&e.code()));
		// Alt and a letter open the window's menus, not the browser's.
		let mnemonic = !mac && modifiers.alt && !modifiers.ctrl && e.key().chars().count() == 1;
		if mnemonic || key.is_some_and(|key| super::shortcuts::is_reserved(key, modifiers)) {
			e.prevent_default();
		}
	});
	if let Err(e) = window.add_event_listener_with_callback_and_bool("keydown", guard.as_ref().unchecked_ref(), true) {
		tracing::warn!("the browser keeps its own actions of the shortcuts' keys: {e:?}");
	}
	guard.forget();
}

/// The browser's question before the page is left (closed, reloaded or navigated away), listened for while there is
/// something to lose; the page cannot ask its own.
#[derive(Default)]
pub(super) struct LeaveGuard(Option<Closure<dyn Fn(web_sys::Event)>>);

impl LeaveGuard {
	/// Asks before the page is left while `on`.
	pub(super) fn set(&mut self, on: bool) {
		if on == self.0.is_some() {
			return;
		}
		let Some(window) = web_sys::window() else { return };
		if let Some(ask) = self.0.take() {
			if let Err(e) = window.remove_event_listener_with_callback("beforeunload", ask.as_ref().unchecked_ref()) {
				tracing::warn!("the page still asks before it is left: {e:?}");
				// still registered: it must outlive the page
				ask.forget();
			}
			return;
		}
		let ask = Closure::<dyn Fn(web_sys::Event)>::new(|e: web_sys::Event| e.prevent_default());
		if let Err(e) = window.add_event_listener_with_callback("beforeunload", ask.as_ref().unchecked_ref()) {
			tracing::warn!("the page is left without asking: {e:?}");
		}
		self.0 = Some(ask);
	}
}

impl Drop for LeaveGuard {
	fn drop(&mut self) {
		self.set(false);
	}
}
