//! The transport bar's and the timeline's callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use super::Handle;
use crate::state::State;
use crate::ui::AppWindow;
use crate::{number, transport, wake};

/// The transport bar and the timeline: actions, seeking, the range brackets and the speed.
pub(crate) fn wire_transport(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let h = Handle::new(state, app);
	app.on_transport_action(move |action| h.with(|s, app| transport(s, app, action)));
	let st = state.clone();
	app.on_seek(move |frame| {
		st.borrow_mut().playback.seek(i64::from(frame));
		wake::wake();
	});
	let st = state.clone();
	app.on_bracket(move |bracket, frame| {
		st.borrow_mut().playback.drag_bracket(bracket, i64::from(frame));
		wake::wake();
	});
	let st = state.clone();
	app.on_transport_speed(move |text| {
		let speed = &mut st.borrow_mut().playback.speed;
		if let Some(v) = number::parse(&text) {
			*speed = v.clamp(0.01, 100.0);
		}
		speed.to_string().into()
	});
}
