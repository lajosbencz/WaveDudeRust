//! What editors share with the window: the one pending commit the window flushes before it reads the model, and
//! the focus request an editor serves once it is laid out.

use std::cell::RefCell;
use std::rc::Rc;

use freya::prelude::*;

type Apply = Rc<dyn Fn(&str)>;

/// The text typed into the focused editor and what applies it. The window provides one through the context and
/// calls [`flush`](Self::flush) before a command, a save, a run or a selection change reads the model; a widget
/// whose press writes the model flushes first ([`use_pending_edit`]); an editor flushes its own text when it loses
/// the focus or unmounts. The text lives here, not in the editor's state, so an unmounted editor's text still
/// applies.
#[derive(Clone, Default)]
pub struct PendingEdit(Rc<RefCell<Option<(AccessibilityId, String, Apply)>>>);

impl PendingEdit {
	/// Applies and clears the pending text.
	pub fn flush(&self) {
		let taken = self.0.borrow_mut().take();
		if let Some((_, text, apply)) = taken {
			apply(&text);
		}
	}

	fn flush_of(&self, id: AccessibilityId) {
		if self.is_of(id) {
			self.flush();
		}
	}

	fn is_of(&self, id: AccessibilityId) -> bool {
		self.0.borrow().as_ref().is_some_and(|(owner, ..)| *owner == id)
	}

	fn offer(&self, id: AccessibilityId, text: String, apply: Apply) {
		let other = self.0.borrow().as_ref().is_some_and(|(owner, ..)| *owner != id);
		if other {
			self.flush();
		}
		*self.0.borrow_mut() = Some((id, text, apply));
	}

	fn withdraw(&self, id: AccessibilityId) {
		if self.is_of(id) {
			self.0.borrow_mut().take();
		}
	}
}

/// The window's [`PendingEdit`], for a press handler that writes the model: flush before reading it. One per
/// window: the first user provides it at the root when the window has not.
pub fn use_pending_edit() -> PendingEdit {
	use_hook(|| try_consume_context::<PendingEdit>().unwrap_or_else(|| provide_root_context(PendingEdit::default())))
}

/// An editor's side of [`PendingEdit`].
#[derive(Clone)]
pub(crate) struct EditSlot {
	pending: PendingEdit,
	id: AccessibilityId,
}

impl EditSlot {
	/// `text` was typed: `apply` is what the window applies it with. Called with each keystroke.
	pub(crate) fn offer(&self, text: String, apply: Apply) {
		self.pending.offer(self.id, text, apply);
	}

	/// The text was applied or reverted by the editor itself.
	pub(crate) fn withdraw(&self) {
		self.pending.withdraw(self.id);
	}
}

/// The editor's [`EditSlot`]; the pending text applies when the focus leaves `id` and when the editor unmounts.
pub(crate) fn use_edit_slot(id: AccessibilityId, focus: Memo<Focus>) -> EditSlot {
	let pending = use_pending_edit();
	{
		let pending = pending.clone();
		use_side_effect(move || {
			if !focus.read().is_focused() {
				pending.flush_of(id);
			}
		});
	}
	{
		let pending = pending.clone();
		use_drop(move || pending.flush_of(id));
	}
	EditSlot { pending, id }
}

/// How an editing session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditEnd {
	/// Enter applied the text.
	Applied,
	/// Esc put the old text back.
	Reverted,
	/// The focus went elsewhere; typed text was applied first.
	FocusLost,
}

/// A request to focus an editor, raised by a counter: the editor serves it once, when it is laid out, if the
/// counter is newer than `served`.
#[derive(Clone, PartialEq)]
pub struct FocusRequest {
	revision: Readable<u64>,
	served: State<u64>,
}

impl FocusRequest {
	pub fn new(revision: impl IntoReadable<u64>, served: State<u64>) -> Self {
		Self { revision: revision.into_readable(), served }
	}

	fn serve(&self, id: AccessibilityId) {
		let revision = *self.revision.peek();
		let mut served = self.served;
		if revision > *served.peek() {
			served.set(revision);
			id.request_focus();
		}
	}
}

/// The `on_sized` handler of the element carrying `id`: it serves `request` when the element is laid out and
/// again when the counter moves while it stands.
pub fn use_focus_on_layout(
	id: AccessibilityId,
	request: Option<FocusRequest>,
) -> impl FnMut(Event<SizedEventData>) + 'static {
	let mut laid_out = use_state(|| false);
	{
		let request = request.clone();
		use_side_effect(move || {
			if let Some(request) = &request {
				let _ = request.revision.read();
				if *laid_out.peek() {
					request.serve(id);
				}
			}
		});
	}
	move |_| {
		laid_out.set_if_modified(true);
		if let Some(request) = &request {
			request.serve(id);
		}
	}
}
