//! What editors share with the window: the one pending commit the window flushes before it reads the model, and
//! the focus request an editor serves once it is laid out.

use std::cell::RefCell;
use std::rc::Rc;

use freya::prelude::*;

type Commit = Rc<RefCell<dyn FnMut()>>;

/// The commit of the text typed into the focused editor. The window provides one through the context and calls
/// [`flush`](Self::flush) before a command, a save, a run or a selection change reads the model; an editor
/// commits on its own when it loses the focus.
#[derive(Clone, Default)]
pub struct PendingEdit(Rc<RefCell<Option<(AccessibilityId, Commit)>>>);

impl PendingEdit {
	/// Runs and clears the pending commit.
	pub fn flush(&self) {
		let taken = self.0.borrow_mut().take();
		if let Some((_, commit)) = taken {
			(commit.borrow_mut())();
		}
	}

	fn flush_of(&self, id: AccessibilityId) {
		let mine = self.0.borrow().as_ref().is_some_and(|(owner, _)| *owner == id);
		if mine {
			self.flush();
		}
	}

	fn offer(&self, id: AccessibilityId, commit: impl FnMut() + 'static) {
		let other = self.0.borrow().as_ref().is_some_and(|(owner, _)| *owner != id);
		if other {
			self.flush();
		}
		*self.0.borrow_mut() = Some((id, Rc::new(RefCell::new(commit))));
	}

	fn withdraw(&self, id: AccessibilityId) {
		let mine = self.0.borrow().as_ref().is_some_and(|(owner, _)| *owner == id);
		if mine {
			self.0.borrow_mut().take();
		}
	}
}

/// An editor's side of [`PendingEdit`].
#[derive(Clone)]
pub(crate) struct EditSlot {
	pending: PendingEdit,
	id: AccessibilityId,
}

impl EditSlot {
	/// Text was typed: `commit` is what the window flushes.
	pub fn offer(&self, commit: impl FnMut() + 'static) {
		self.pending.offer(self.id, commit);
	}

	/// The text was applied or reverted.
	pub fn withdraw(&self) {
		self.pending.withdraw(self.id);
	}
}

/// The editor's [`EditSlot`]; the pending text commits when the focus leaves `id`.
pub(crate) fn use_edit_slot(id: AccessibilityId, focus: Memo<Focus>) -> EditSlot {
	let pending = use_hook(|| try_consume_context::<PendingEdit>().unwrap_or_default());
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
		use_drop(move || pending.withdraw(id));
	}
	EditSlot { pending, id }
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
