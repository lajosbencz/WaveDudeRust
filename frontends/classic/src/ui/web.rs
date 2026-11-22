//! The browser's side of the window: the browser's own actions of the shortcuts' and the menus' keys kept off.

use eframe::egui::os::OperatingSystem;
use eframe::egui::{Key, Modifiers};
use eframe::wasm_bindgen::JsCast as _;
use eframe::wasm_bindgen::closure::Closure;
use eframe::web_sys::{self, KeyboardEvent};

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
