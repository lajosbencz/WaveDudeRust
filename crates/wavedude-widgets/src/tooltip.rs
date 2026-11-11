//! Tooltips as WPF shows them: a square box in the UI font under the mouse pointer, after the hover delay.

use std::time::Duration;

use freya::prelude::*;

use crate::delay::use_delayed;
use crate::theme::{Theme, UiFontExt, use_wave_theme};

/// WPF's initial show delay (`ToolTipService.InitialShowDelay`, the system hover time).
pub const SHOW_DELAY: Duration = Duration::from_millis(400);
/// Gap between the pointer's hot spot and the box: the height of the arrow cursor.
const BELOW_POINTER: f32 = 20.;

/// The box itself: 1 px border, padding 6,3, no corner radius, no shadow.
pub fn tip_box(theme: &Theme, text: impl Into<String>) -> Rect {
	rect()
		.interactive(Interactive::No)
		.padding((3., 6.))
		.background(theme.tooltip_background)
		.border(Border::new().fill(theme.tooltip_border).width(1.).alignment(BorderAlignment::Inner))
		.color(theme.tooltip_text)
		.ui_font(theme)
		.child(label().text(text.into()).max_lines(1))
}

/// Wraps its children and shows `text` beside the pointer once it rests on them for [`SHOW_DELAY`]; a press
/// or leaving hides it.
#[derive(Clone, PartialEq)]
pub struct Tip {
	text: String,
	children: Vec<Element>,
	layout: LayoutData,
	key: DiffKey,
}

impl Tip {
	pub fn new(text: impl Into<String>) -> Self {
		Self { text: text.into(), children: Vec::new(), layout: LayoutData::default(), key: DiffKey::None }
	}
}

impl ChildrenExt for Tip {
	fn children_mut(&mut self) -> &mut Vec<Element> {
		&mut self.children
	}
}

impl LayoutExt for Tip {
	fn layout_mut(&mut self) -> &mut LayoutData {
		&mut self.layout
	}
}

impl KeyExt for Tip {
	fn key_mut(&mut self) -> &mut DiffKey {
		&mut self.key
	}
}

impl Component for Tip {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let mut shown_at = use_state(|| None::<(f32, f32)>);
		let mut pointer = use_state(|| (0f32, 0f32));
		let mut size = use_state(|| None::<Size2D>);
		let mut delayed = use_delayed();

		let hide = move |_| {
			delayed.cancel();
			shown_at.set_if_modified(None);
		};
		let on_move = move |e: Event<PointerEventData>| {
			let p = e.global_location();
			pointer.set((p.x as f32, p.y as f32));
			if shown_at.peek().is_none() {
				delayed.restart(SHOW_DELAY, move || shown_at.set(Some(*pointer.peek())));
			}
		};

		let tip = shown_at.read().filter(|_| !self.text.is_empty()).map(|(x, y)| {
			let window = Platform::get().current_window().root_size.read().to_owned();
			let (left, top) = match *size.read() {
				Some(s) => {
					let below = y + BELOW_POINTER;
					let top = if below + s.height > window.height { (y - s.height).max(0.) } else { below };
					((x.min(window.width - s.width)).max(0.), top)
				}
				None => (x, y + BELOW_POINTER),
			};
			rect()
				.layer(Layer::Overlay)
				.position(Position::new_global().left(left).top(top))
				.a11y_role(AccessibilityRole::Tooltip)
				.opacity(if size.read().is_some() { 1. } else { 0. })
				.on_sized(move |e: Event<SizedEventData>| size.set_if_modified(Some(e.area.size)))
				.child(tip_box(&theme, self.text.clone()))
		});

		rect()
			.layout(self.layout.clone())
			.a11y_focusable(false)
			.on_pointer_move(on_move)
			.on_pointer_leave(hide)
			.on_pointer_down(hide)
			.children(self.children.clone())
			.maybe_child(tip)
	}

	fn render_key(&self) -> DiffKey {
		self.key.clone().or(self.default_key())
	}
}

/// A tooltip that appears after [`SHOW_DELAY`] where it is placed, for rows that show their own tip while
/// highlighted (menu entries).
#[derive(Clone, PartialEq)]
pub struct DelayedTip {
	text: String,
}

impl DelayedTip {
	pub fn new(text: impl Into<String>) -> Self {
		Self { text: text.into() }
	}
}

impl Component for DelayedTip {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let mut due = use_state(|| false);
		let mut delayed = use_delayed();
		use_hook(move || delayed.restart(SHOW_DELAY, move || due.set(true)));
		rect().maybe_child(due().then(|| tip_box(&theme, self.text.clone())))
	}
}
