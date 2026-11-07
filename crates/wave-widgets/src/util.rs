//! Small helpers shared by the views: test ids, modifier tracking, key matching.

use freya::prelude::*;

/// Attaches a stable test id (AccessKit author id) to an element, the port of WPF's AutomationId.
pub trait TestIdExt: AccessibilityExt {
	fn test_id(self, id: impl Into<String>) -> Self {
		let id: String = id.into();
		self.a11y_builder(|node| node.set_author_id(id))
	}

	fn maybe_test_id(self, id: Option<impl Into<String>>) -> Self {
		match id {
			Some(id) => self.test_id(id),
			None => self,
		}
	}
}

impl<T: AccessibilityExt> TestIdExt for T {}

/// The keyboard modifiers last seen by any widget's global key handler, shared through the root scope.
///
/// Pointer events carry no modifiers in Freya 0.5, so widgets that need Ctrl or Shift on a click read this.
pub fn use_modifiers() -> State<Modifiers> {
	use_hook(|| {
		try_consume_root_context::<ModifierState>().map(|m| m.0).unwrap_or_else(|| {
			let state = State::create_in_scope(Modifiers::empty(), ScopeId::ROOT);
			provide_context_for_scope_id(ModifierState(state), ScopeId::ROOT);
			state
		})
	})
}

#[derive(Clone, Copy)]
struct ModifierState(State<Modifiers>);

/// Wraps `inner` in a rect that keeps `state` current from global key events.
pub(crate) fn modifier_tracker(mut state: State<Modifiers>, inner: impl IntoElement) -> Rect {
	let mut up = state;
	rect()
		.on_global_key_down(move |e: Event<KeyboardEventData>| state.set_if_modified(e.modifiers))
		.on_global_key_up(move |e: Event<KeyboardEventData>| up.set_if_modified(e.modifiers))
		.child(inner)
}

pub(crate) fn is_named(e: &KeyboardEventData, key: NamedKey) -> bool {
	e.key == Key::Named(key)
}

pub(crate) fn is_char(e: &KeyboardEventData, c: &str) -> bool {
	matches!(&e.key, Key::Character(s) if s.eq_ignore_ascii_case(c))
}

pub(crate) fn ctrl(e: &KeyboardEventData) -> bool {
	e.modifiers.contains(Modifiers::CONTROL) || e.modifiers.contains(Modifiers::META)
}

pub(crate) fn shift(e: &KeyboardEventData) -> bool {
	e.modifiers.contains(Modifiers::SHIFT)
}

/// The focus border every focusable widget shows under keyboard navigation.
pub(crate) fn focus_border(focus: Focus, color: Color) -> Option<Border> {
	(focus == Focus::Keyboard).then(|| Border::new().fill(color).width(1.).alignment(BorderAlignment::Inner))
}

/// A tooltip wrapper that fills its parent's width.
pub(crate) fn wide_tooltip(text: String, inner: impl IntoElement) -> Element {
	let mut layout = LayoutData::default();
	layout.layout.width = Size::fill();
	TooltipContainer::new(Tooltip::new_text(text)).layout(layout).child(inner).into_element()
}
