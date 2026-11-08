//! Text box: typed text applies on Enter or focus loss and Esc puts the old text back; a live box applies every
//! keystroke (a name the tree follows while typing).

use freya::prelude::*;

use crate::util::is_named;

/// A text box bound to a value.
#[derive(Clone, PartialEq)]
pub struct TextBox {
	value: String,
	on_apply: EventHandler<String>,
	multiline: bool,
	live: bool,
	id: Option<AccessibilityId>,
}

impl TextBox {
	pub fn new(value: impl Into<String>, on_apply: impl Into<EventHandler<String>>) -> Self {
		Self { value: value.into(), on_apply: on_apply.into(), multiline: false, live: false, id: None }
	}

	/// Enter inserts a newline; the text applies on focus loss.
	pub fn multiline(mut self, multiline: bool) -> Self {
		self.multiline = multiline;
		self
	}

	/// Every keystroke applies.
	pub fn live(mut self) -> Self {
		self.live = true;
		self
	}

	/// The box's id, for focusing it from outside.
	pub fn id(mut self, id: Option<AccessibilityId>) -> Self {
		self.id = id;
		self
	}
}

impl Component for TextBox {
	fn render(&self) -> impl IntoElement {
		let own = use_a11y();
		let id = self.id.unwrap_or(own);
		let focus = use_focus(id);
		let mut text = use_state(|| self.value.clone());
		let mut typed = use_state(|| false);
		let bound = self.value.clone();
		if !*typed.peek() && *text.peek() != bound {
			text.set(bound.clone());
		}
		let apply = self.on_apply.clone();
		let live = self.live;
		let multiline = self.multiline;

		let mut commit = {
			let apply = apply.clone();
			move || {
				if *typed.peek() {
					typed.set(false);
					apply.call(text.peek().clone());
				}
			}
		};
		let mut was_focused = use_state(|| false);
		let focused_now = focus().is_focused();
		if *was_focused.peek() != focused_now {
			was_focused.set(focused_now);
			if !focused_now {
				commit();
			}
		}
		let revert = bound.clone();
		let on_pre_key_down = {
			let mut commit = commit.clone();
			move |e: Event<KeyboardEventData>| -> bool {
				if is_named(&e, NamedKey::Enter) && !multiline {
					e.stop_propagation();
					commit();
					return false;
				}
				if is_named(&e, NamedKey::Escape) {
					if *typed.peek() {
						e.stop_propagation();
						text.set(revert.clone());
						typed.set(false);
					}
					return false;
				}
				if !is_named(&e, NamedKey::Tab) {
					e.stop_propagation();
				}
				true
			}
		};
		Input::new(WritableSetter::new(text, move |t: String| {
			text.set(t.clone());
			if live {
				apply.call(t);
			} else {
				typed.set_if_modified(true);
			}
		}))
		.a11y_id(id)
		.multiline(multiline)
		.width(Size::fill())
		.compact()
		.on_pre_key_down(on_pre_key_down)
	}
}
