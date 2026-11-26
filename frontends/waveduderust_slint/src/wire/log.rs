//! The Messages pane's callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use super::Handle;
use crate::clipboard;
use crate::messages::Messages;
use crate::state::State;
use crate::ui::AppWindow;

/// The Messages pane: search, levels, selection, copy and clear.
pub(crate) fn wire_log(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let on_log = |f: fn(&mut Messages, slint::SharedString)| {
		let h = Handle::new(state, app);
		move |arg: slint::SharedString| {
			h.with(|s, app| {
				f(&mut s.messages, arg);
				s.messages.show(app);
			});
		}
	};
	let search = on_log(|m, t| m.search(&t));
	app.on_log_search(search);
	let clear = on_log(|m, _| m.clear());
	app.on_log_clear(move || clear(slint::SharedString::new()));
	let h = Handle::new(state, app);
	app.on_log_regex(move |on| {
		h.with(|s, app| {
			s.messages.set_regex(on);
			s.messages.show(app);
		});
	});
	let h = Handle::new(state, app);
	app.on_log_select(move |row, control, shift| {
		let Ok(row) = usize::try_from(row) else { return };
		h.with(|s, app| {
			s.messages.select(row, control, shift);
			s.messages.show(app);
		});
	});
	let st = state.clone();
	app.on_log_copy(move |all| {
		let text = st.borrow().messages.text(all);
		if text.is_empty() {
			return false;
		}
		if let Err(e) = clipboard::write(text) {
			tracing::warn!("clipboard: {e}");
		}
		true
	});
	let h = Handle::new(state, app);
	app.on_log_toggle(move |level| {
		h.with(|s, app| {
			s.messages.toggle(level);
			s.messages.show(app);
		});
	});
}
