//! The transport bar's and the timeline's callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use super::Handle;
use crate::playback::Playback;
use crate::state::State;
use crate::ui::AppWindow;
use crate::{number, transport, wake};

/// The transport bar and the timeline: actions, seeking, the range brackets and the speed.
pub(crate) fn wire_transport(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let h = Handle::new(state, app);
	app.on_transport_action(move |action| h.with(|s, app| transport(s, app, action)));
	let st = state.clone();
	app.on_seek(move |frame| scrub(&st, |p| p.seek(i64::from(frame))));
	let st = state.clone();
	app.on_bracket(move |bracket, frame| scrub(&st, |p| p.drag_bracket(bracket, i64::from(frame))));
	let st = state.clone();
	app.on_transport_speed(move |text| {
		let speed = &mut st.borrow_mut().playback.speed;
		if let Some(v) = number::parse(&text) {
			*speed = v.clamp(0.01, 100.0);
		}
		speed.to_string().into()
	});
}

/// A seek or bracket drag; none without an enabled source, which has nothing to show.
fn scrub(state: &RefCell<State>, f: impl FnOnce(&mut Playback)) {
	let s = &mut *state.borrow_mut();
	if s.doc.project.has_enabled_source() {
		f(&mut s.playback);
		wake::wake();
	}
}
