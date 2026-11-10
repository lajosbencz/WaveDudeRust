//! Text box: typed text applies on Enter or focus loss and Esc puts the old text back; a live box applies every
//! keystroke (a name the tree follows while typing).

use freya::prelude::*;

use crate::edit::{FocusRequest, use_edit_slot, use_focus_on_layout};
use crate::text_field::{FIELD_PADDING, TextField};
use crate::theme::{INPUT_HEIGHT, Theme, use_wave_theme};
use crate::util::is_named;

/// The frame of a text field (text box, number box): at least [`INPUT_HEIGHT`] high, square, a 1 px border
/// that turns to the hover colour under the pointer and to the accent while focused, the window's background
/// and grey text when disabled.
pub(crate) fn field_frame(theme: &Theme, enabled: bool, focused: bool, hovered: bool) -> Rect {
	let border = if focused {
		theme.accent
	} else if hovered && enabled {
		theme.input_hover_border
	} else {
		theme.input_border
	};
	rect()
		.min_height(Size::px(INPUT_HEIGHT))
		.padding(1.)
		.horizontal()
		.content(Content::flex())
		.cross_align(Alignment::center())
		.background(if enabled { theme.input_background } else { theme.window_background })
		.border(Border::new().fill(border).width(1.).alignment(BorderAlignment::Inner))
		.color(if enabled { theme.text } else { theme.text_disabled })
}

/// A borderless Freya input with the field's text padding, for the multi-line box inside [`field_frame`].
pub(crate) fn field_input(input: Input) -> Input {
	input.flat().compact().theme_layout(InputLayoutThemePartial::new().padding(FIELD_PADDING))
}

/// A text box bound to a value.
#[derive(Clone, PartialEq)]
pub struct TextBox {
	value: String,
	on_apply: EventHandler<String>,
	multiline: bool,
	live: bool,
	select_all: bool,
	id: Option<AccessibilityId>,
	focus_request: Option<FocusRequest>,
}

impl TextBox {
	pub fn new(value: impl Into<String>, on_apply: impl Into<EventHandler<String>>) -> Self {
		Self {
			value: value.into(),
			on_apply: on_apply.into(),
			multiline: false,
			live: false,
			select_all: false,
			id: None,
			focus_request: None,
		}
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

	/// Gaining focus selects the whole text (single-line boxes).
	pub fn select_all_on_focus(mut self, select_all: bool) -> Self {
		self.select_all = select_all;
		self
	}

	/// The box's id, for focusing it from outside.
	pub fn id(mut self, id: Option<AccessibilityId>) -> Self {
		self.id = id;
		self
	}

	/// Takes the focus when `request` is raised and the box is laid out.
	pub fn focus_request(mut self, request: Option<FocusRequest>) -> Self {
		self.focus_request = request;
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

		let slot = use_edit_slot(id, focus);
		let commit = {
			let apply = apply.clone();
			let slot = slot.clone();
			move || {
				slot.withdraw();
				if *typed.peek() {
					typed.set(false);
					apply.call(text.peek().clone());
				}
			}
		};
		let on_sized = use_focus_on_layout(id, self.focus_request.clone());
		let revert = bound.clone();
		let on_pre_key_down = {
			let mut commit = commit.clone();
			let slot = slot.clone();
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
						slot.withdraw();
					}
					return false;
				}
				if !is_named(&e, NamedKey::Tab) {
					e.stop_propagation();
				}
				true
			}
		};
		let theme = use_wave_theme();
		let mut hovered = use_state(|| false);
		let on_change = WritableSetter::new(text, move |t: String| {
			text.set(t.clone());
			if live {
				apply.call(t);
			} else {
				typed.set_if_modified(true);
				slot.offer(commit.clone());
			}
		});
		let input: Element = if multiline {
			field_input(Input::new(on_change))
				.a11y_id(id)
				.multiline(true)
				.width(Size::flex(1.))
				.on_pre_key_down(on_pre_key_down)
				.into()
		} else {
			TextField::new(on_change)
				.a11y_id(id)
				.select_all_on_focus(self.select_all)
				.width(Size::flex(1.))
				.on_pre_key_down(on_pre_key_down)
				.into()
		};
		field_frame(&theme, true, focus().is_focused(), hovered())
			.width(Size::fill())
			.on_sized(on_sized)
			.on_pointer_enter(move |_| hovered.set(true))
			.on_pointer_leave(move |_| hovered.set(false))
			.child(input)
	}
}
