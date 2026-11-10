//! Push button in WaveDude Next's look: square, a 1 px border, the hover, pressed (accent, white text), focused
//! (accent border) and disabled (grey text) states. The API follows Freya's `Button`, which cannot show them.

use freya::prelude::*;

use crate::theme::{INPUT_HEIGHT, Theme, use_wave_theme};

/// WPF's button: at least 23 high.
const BUTTON_HEIGHT: f32 = 23.;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Look {
	/// WPF's Button: padding 10,3.
	#[default]
	Normal,
	/// The toolbar's TextButton: transparent until hovered, padding 6,1,6,0, margin 1,0.
	Toolbar,
	/// The combo box's toggle: the field's colours, padding 4,2, 22 high.
	Combo,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct Variant {
	flat: bool,
	compact: bool,
	expanded: bool,
}

impl Variant {
	fn look(self) -> Look {
		if self.expanded {
			Look::Combo
		} else if self.flat || self.compact {
			Look::Toolbar
		} else {
			Look::Normal
		}
	}
}

/// A button; children are its content (text, icon).
#[derive(Clone, PartialEq)]
pub struct Button {
	children: Vec<Element>,
	on_press: Option<EventHandler<Event<PressEventData>>>,
	on_pointer_down: Option<EventHandler<Event<PointerEventData>>>,
	enabled: bool,
	focusable: bool,
	variant: Variant,
	key: DiffKey,
}

impl Default for Button {
	fn default() -> Self {
		Self::new()
	}
}

impl Button {
	pub fn new() -> Self {
		Self {
			children: Vec::new(),
			on_press: None,
			on_pointer_down: None,
			enabled: true,
			focusable: true,
			variant: Variant::default(),
			key: DiffKey::None,
		}
	}

	pub fn enabled(mut self, enabled: impl Into<bool>) -> Self {
		self.enabled = enabled.into();
		self
	}

	pub fn focusable(mut self, focusable: impl Into<bool>) -> Self {
		self.focusable = focusable.into();
		self
	}

	/// Left click, Enter, Space or an accessibility action.
	pub fn on_press(mut self, on_press: impl Into<EventHandler<Event<PressEventData>>>) -> Self {
		self.on_press = Some(on_press.into());
		self
	}

	pub fn on_pointer_down(mut self, on_pointer_down: impl Into<EventHandler<Event<PointerEventData>>>) -> Self {
		self.on_pointer_down = Some(on_pointer_down.into());
		self
	}

	/// Transparent until hovered, as the toolbar's buttons.
	pub fn flat(mut self) -> Self {
		self.variant.flat = true;
		self
	}

	/// The toolbar's padding (6,1,6,0).
	pub fn compact(mut self) -> Self {
		self.variant.compact = true;
		self
	}

	/// The combo box's toggle: field colours, 22 px high, filling its width.
	pub fn expanded(mut self) -> Self {
		self.variant.expanded = true;
		self
	}
}

impl ChildrenExt for Button {
	fn children_mut(&mut self) -> &mut Vec<Element> {
		&mut self.children
	}
}

impl KeyExt for Button {
	fn key_mut(&mut self) -> &mut DiffKey {
		&mut self.key
	}
}

/// Background, border and text of a button in its state.
fn colours(
	theme: &Theme,
	look: Look,
	flat: bool,
	enabled: bool,
	hovered: bool,
	pressed: bool,
	focused: bool,
) -> [Color; 3] {
	let (rest, border, hover, hover_border) = match look {
		Look::Combo => (theme.input_background, theme.combo_border, theme.combo_hover, theme.input_hover_border),
		_ if flat => (Color::TRANSPARENT, Color::TRANSPARENT, theme.button_hover, Color::TRANSPARENT),
		_ => (theme.button_background, theme.button_border, theme.button_hover, theme.button_border),
	};
	let (background, border) = match (enabled, pressed, hovered) {
		(true, true, _) if look != Look::Combo => (theme.pressed_background, border),
		(true, _, true) => (hover, hover_border),
		_ => (rest, border),
	};
	let border = if focused && enabled { theme.accent } else { border };
	let text = if !enabled {
		theme.text_disabled
	} else if pressed && look != Look::Combo {
		Color::WHITE
	} else {
		theme.text
	};
	[background, border, text]
}

impl Component for Button {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let a11y = use_a11y();
		let focus = use_focus(a11y);
		let mut hovered = use_state(|| false);
		let mut pressed = use_state(|| false);
		let enabled = self.enabled;
		let look = self.variant.look();
		let [background, border, text] =
			colours(&theme, look, self.variant.flat, enabled, hovered(), pressed(), focus().is_focused());
		let (padding, height, margin): (Gaps, f32, Gaps) = match look {
			Look::Normal => (Gaps::new(4., 11., 4., 11.), BUTTON_HEIGHT, Gaps::new_all(0.)),
			Look::Toolbar => (Gaps::new(2., 7., 1., 7.), BUTTON_HEIGHT, Gaps::new(0., 1., 0., 1.)),
			Look::Combo => (Gaps::new(3., 5., 3., 5.), INPUT_HEIGHT, Gaps::new_all(0.)),
		};
		let on_press = self.on_press.clone();
		let on_pointer_down = self.on_pointer_down.clone();
		rect()
			.a11y_id(a11y)
			.a11y_enabled(enabled)
			.a11y_focusable(enabled && self.focusable)
			.a11y_role(AccessibilityRole::Button)
			.overflow(Overflow::Clip)
			.height(Size::px(height))
			.maybe(look == Look::Combo, |r| r.width(Size::fill()))
			.margin(margin)
			.padding(padding)
			.center()
			.background(background)
			.border(Border::new().fill(border).width(1.).alignment(BorderAlignment::Inner))
			.color(text)
			.cursor(if enabled { CursorIcon::Default } else { CursorIcon::NotAllowed })
			.on_pointer_enter(move |_| hovered.set(true))
			.on_pointer_leave(move |_| {
				hovered.set_if_modified(false);
				pressed.set_if_modified(false);
			})
			.on_global_pointer_up(move |_| pressed.set_if_modified(false))
			.maybe(enabled, |r| {
				r.on_pointer_down(move |e: Event<PointerEventData>| {
					if e.button() == Some(MouseButton::Left) {
						pressed.set(true);
					}
					if let Some(h) = &on_pointer_down {
						h.call(e);
					}
				})
				.on_all_press(move |e: Event<PressEventData>| {
					a11y.request_focus();
					let left = match e.data() {
						PressEventData::Mouse(data) => data.button == Some(MouseButton::Left),
						_ => true,
					};
					if left && let Some(h) = &on_press {
						h.call(e);
					}
				})
			})
			.children(self.children.clone())
	}

	fn render_key(&self) -> DiffKey {
		self.key.clone().or(self.default_key())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn states_follow_wpf() {
		let t = Theme::dark();
		let normal =
			|hovered, pressed, focused, enabled| colours(&t, Look::Normal, false, enabled, hovered, pressed, focused);
		assert_eq!(normal(false, false, false, true), [t.button_background, t.button_border, t.text]);
		assert_eq!(normal(true, false, false, true)[0], t.button_hover);
		assert_eq!(normal(true, true, false, true), [t.pressed_background, t.button_border, Color::WHITE]);
		assert_eq!(normal(false, false, true, true)[1], t.accent);
		assert_eq!(normal(true, true, true, false), [t.button_background, t.button_border, t.text_disabled]);
		let flat = colours(&t, Look::Toolbar, true, true, false, false, false);
		assert_eq!(flat, [Color::TRANSPARENT, Color::TRANSPARENT, t.text]);
		let combo = colours(&t, Look::Combo, false, true, true, true, false);
		assert_eq!(combo, [t.combo_hover, t.input_hover_border, t.text]);
	}
}
