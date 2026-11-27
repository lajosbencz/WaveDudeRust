//! The window's callbacks, by area, on the app's state.

mod canvas;
mod dialogs;
mod export;
mod form;
mod log;
mod menu;
mod transport;
mod tree;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

pub(crate) use self::canvas::wire_canvas;
pub(crate) use self::dialogs::wire_dialogs;
pub(crate) use self::export::wire_export;
pub(crate) use self::form::{wire_form, wire_widgets};
pub(crate) use self::log::wire_log;
pub(crate) use self::menu::wire_menu;
pub(crate) use self::transport::wire_transport;
pub(crate) use self::tree::wire_tree;
pub(crate) use self::window::wire_window_events;
use crate::state::State;
use crate::tree::Node;
use crate::ui::AppWindow;

/// The state and the window as a callback holds them; the window weakly, as it owns its callbacks.
pub(crate) struct Handle<T = State> {
	state: Rc<RefCell<T>>,
	app: slint::Weak<AppWindow>,
}

impl<T> Clone for Handle<T> {
	fn clone(&self) -> Self {
		Self { state: self.state.clone(), app: self.app.clone() }
	}
}

impl<T> Handle<T> {
	pub(crate) fn new(state: &Rc<RefCell<T>>, app: &AppWindow) -> Self {
		Self { state: state.clone(), app: app.as_weak() }
	}

	/// `f` on the state borrowed mutably and the window; the default once the window is gone.
	pub(crate) fn with<R: Default>(&self, f: impl FnOnce(&mut T, &AppWindow) -> R) -> R {
		let Some(app) = self.app.upgrade() else { return R::default() };
		f(&mut self.state.borrow_mut(), &app)
	}

	/// `f` on the state borrowed shared and the window; nothing once the window is gone.
	pub(crate) fn read(&self, f: impl FnOnce(&T, &AppWindow)) {
		if let Some(app) = self.app.upgrade() {
			f(&self.state.borrow(), &app);
		}
	}
}

impl Handle {
	/// [`Self::with`], the window refreshed after.
	pub(crate) fn show(&self, f: impl FnOnce(&mut State, &AppWindow)) {
		self.with(|s, app| {
			f(s, app);
			s.show(app);
		});
	}
}

/// A tree row's handler: `f` on the row's node with the callback's argument and the
/// window refreshed after.
fn on_row<A: 'static>(
	state: &Rc<RefCell<State>>,
	app: &AppWindow,
	f: impl Fn(&mut State, Node, A) + 'static,
) -> impl Fn(i32, A) + 'static {
	let h = Handle::new(state, app);
	move |row, arg| {
		h.with(|s, app| {
			let Some(node) = s.node(row) else { return };
			f(s, node, arg);
			s.show(app);
		});
	}
}

/// The tree row a drag carries.
fn dragged(data: &slint::DataTransfer) -> Option<Node> {
	data.user_data().and_then(|d| d.downcast_ref::<Node>().copied())
}
