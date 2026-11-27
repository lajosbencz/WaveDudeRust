//! The Messages pane's callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use super::Handle;
use crate::clipboard;
use crate::messages::Messages;
use crate::models;
use crate::state::State;
use crate::ui::AppWindow;

/// The Messages pane: its rows, search, levels, selection, copy and clear.
pub(crate) fn wire_log(state: &Rc<RefCell<State>>, app: &AppWindow) {
	app.set_log_rows(state.borrow().messages.rows());
	app.on_log_search(on_log(state, app, |m, t: slint::SharedString| m.search(&t)));
	let clear = on_log(state, app, |m, ()| m.clear());
	app.on_log_clear(move || clear(()));
	app.on_log_regex(on_log(state, app, Messages::set_regex));
	let select = on_log(state, app, |m, (row, control, shift)| {
		if let Some(row) = models::index_from_ui(row) {
			m.select(row, control, shift);
		}
	});
	app.on_log_select(move |row, control, shift| select((row, control, shift)));
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
	app.on_log_toggle(on_log(state, app, Messages::toggle));
}

/// A Messages pane handler: `f` on the messages with the callback's argument, the pane refreshed after.
fn on_log<A: 'static>(
	state: &Rc<RefCell<State>>,
	app: &AppWindow,
	f: impl Fn(&mut Messages, A) + 'static,
) -> impl Fn(A) + 'static {
	let h = Handle::new(state, app);
	move |arg| {
		h.with(|s, app| {
			f(&mut s.messages, arg);
			s.messages.show(app);
		});
	}
}
