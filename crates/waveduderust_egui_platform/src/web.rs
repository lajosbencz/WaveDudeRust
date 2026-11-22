//! The browser's side of a window: the page's title, the clipboard read for Paste and the question before the page
//! is left; the page places the canvas.

use std::cell::RefCell;

use eframe::egui;
use eframe::wasm_bindgen::JsCast as _;
use eframe::wasm_bindgen::closure::Closure;
use eframe::web_sys;
use wasm_bindgen_futures::JsFuture;
use waveduderust_app::menus::On;
use waveduderust_app::settings::Settings;

thread_local! {
	/// The clipboard's text read for the last Paste and what it pastes on, until the next frame takes it.
	static PASTED: RefCell<Option<(String, On)>> = const { RefCell::new(None) };
}

/// The page's title, which eframe leaves to the page.
pub fn set_title(_ctx: &egui::Context, title: &str) {
	if let Some(document) = web_sys::window().and_then(|w| w.document()) {
		document.set_title(title);
	}
}

/// The canvas follows the page, not the zoom.
pub fn rezoom(_frame: &eframe::Frame, _ratio: f64, _zoom: f64) {}

/// The page has no window place to keep.
pub fn remember_place(_frame: &eframe::Frame, _settings: &mut Settings) {}

/// Paste stays enabled: the browser reads the clipboard only asynchronously and with the user's permission.
pub fn pastable() -> bool {
	true
}

/// Starts reading the clipboard's text for [`pasted`] on `on`, asking the user where the browser does.
pub fn read_clipboard(ctx: &egui::Context, on: On) -> Option<String> {
	let window = web_sys::window()?;
	let read = window.navigator().clipboard().read_text();
	let ctx = ctx.clone();
	wasm_bindgen_futures::spawn_local(async move {
		match JsFuture::from(read).await {
			Ok(text) => {
				PASTED.set(text.as_string().map(|text| (text, on)));
				ctx.request_repaint();
			}
			Err(e) => tracing::debug!("nothing to paste: {e:?}"),
		}
	});
	None
}

/// The clipboard's text read since the last frame and what it pastes on.
pub fn pasted() -> Option<(String, On)> {
	PASTED.take()
}

/// The browser's question before the page is left (closed, reloaded or navigated away), listened for while there is
/// something to lose; the page cannot ask its own.
#[derive(Default)]
pub struct LeaveGuard(Option<Closure<dyn Fn(web_sys::Event)>>);

impl LeaveGuard {
	/// Asks before the page is left while `on`.
	pub fn set(&mut self, on: bool) {
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
