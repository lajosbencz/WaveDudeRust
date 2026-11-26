//! The menu's, the shortcuts' and the message window's callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use super::Handle;
#[cfg(not(target_arch = "wasm32"))]
use crate::keep_window;
use crate::state::State;
use crate::ui::{self, AppWindow, Command};
use crate::{Action, act, add, answer, command, models};

/// Menu commands, adding items, recent files, the message window's answers and closing the window.
pub(crate) fn wire_menu(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let h = Handle::new(state, app);
	app.on_command(move |c| h.with(|s, app| command(s, app, c)));
	let h = Handle::new(state, app);
	app.on_add(move |item| {
		h.with(|s, app| {
			add(s, item);
			s.show(app);
			app.global::<ui::Form<'_>>().set_focus_pending(true);
		});
	});
	let h = Handle::new(state, app);
	app.on_open_recent(move |i| {
		h.with(|s, app| {
			// the path now: saving first may move it in the list
			if let Some(path) = models::index_from_ui(i).and_then(|i| s.settings.recent_files.get(i)).cloned() {
				act(s, app, Action::OpenFile(path));
			}
		});
	});
	let h = Handle::new(state, app);
	app.on_modal_answer(move |button| h.show(|s, app| answer(s, app, button)));
	let h = Handle::new(state, app);
	// gone, the window hides: the response's default
	app.window().on_close_requested(move || {
		h.with(|s, app| {
			if s.doc.dirty() || s.exporting.is_some() {
				command(s, app, Command::Exit);
				slint::CloseRequestResponse::KeepWindowShown
			} else {
				#[cfg(not(target_arch = "wasm32"))]
				keep_window(s, app);
				slint::CloseRequestResponse::HideWindow
			}
		})
	});
}
