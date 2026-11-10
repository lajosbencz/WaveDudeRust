//! A picture's header strip: the title at the start, the brightness box and its Auto button at the far end, and a
//! caption just before them that gives way first, then the title (trimmed with an ellipsis). Above a picture the
//! strip lies flat; beside one the whole strip is turned a quarter clockwise, reading top to bottom and ending at
//! the picture's bottom edge.

use crate::button::Button;
use freya::prelude::*;

use crate::icon::{IconView, icons};
use crate::number_box::{NumberBox, NumberSpec};
use crate::text_box::field_frame;
use crate::theme::{INPUT_HEIGHT, use_wave_theme};
use crate::tooltip::Tip;
use crate::util::{TestIdExt, focus_border, is_named};

/// Above the picture, or turned on its side to the picture's left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StripOrientation {
	#[default]
	Horizontal,
	Vertical,
}

/// Which parts fit along a strip's length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripLayout {
	pub caption: bool,
	/// Room for the title; it is trimmed with an ellipsis below its natural width.
	pub title_width: f32,
}

/// Lengths in logical pixels along the strip; `caption` includes its trailing margin, `spacing` is the least space
/// after the title.
pub fn strip_layout(length: f32, title: f32, caption: f32, tools: f32, spacing: f32) -> StripLayout {
	let caption_shown = title + spacing + caption + tools <= length;
	let others = tools + if caption_shown { caption } else { 0. };
	StripLayout { caption: caption_shown, title_width: title.min((length - spacing - others).max(0.)) }
}

/// Whether the strip goes beside the picture: the frame (the cell inside its margins) is wider than the picture's
/// aspect, so the spare room is at the sides.
pub fn strip_beside(frame: (f32, f32), canvas: (u32, u32)) -> bool {
	let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
	frame.1 > 0. && frame.0 * ch > frame.1 * cw
}

const SPACING: f32 = 12.;
const CAPTION_MARGIN: f32 = 4.;
const AUTO_GAP: f32 = 4.;
const BOX_WIDTH: f32 = 64.;
/// The number box's spin buttons at its far end.
const SPIN_WIDTH: f32 = 14.;
/// Margin before the strip's start when it is on its side.
const SIDE_START: f32 = 10.;
/// Margin after the strip: below it when flat, before its end when on its side.
const END_MARGIN: f32 = 4.;
/// Where the side strip's box waits while not focused: left of the strip by this much, outside the window, so its
/// pointer handlers never see a press.
const AWAY: f32 = 100_000.;

/// The strip, bound to a brightness value.
#[derive(Clone, PartialEq)]
pub struct HeaderStrip {
	title: String,
	caption: String,
	value: WritableSetter<f64>,
	spec: NumberSpec,
	on_auto: EventHandler<()>,
	orientation: StripOrientation,
	tooltip: Option<String>,
	auto_text: String,
	auto_tooltip: Option<String>,
	test_id: String,
	box_id: Option<AccessibilityId>,
}

impl HeaderStrip {
	pub fn new(
		title: impl Into<String>,
		value: impl Into<WritableSetter<f64>>,
		on_auto: impl Into<EventHandler<()>>,
	) -> Self {
		Self {
			title: title.into(),
			caption: "Brightness".into(),
			value: value.into(),
			spec: NumberSpec::logarithmic(0.0001, 100_000.),
			on_auto: on_auto.into(),
			orientation: StripOrientation::Horizontal,
			tooltip: None,
			auto_text: "Auto".into(),
			auto_tooltip: None,
			test_id: String::new(),
			box_id: None,
		}
	}

	/// The Auto button's word and tooltip.
	pub fn auto(mut self, text: impl Into<String>, tooltip: impl Into<String>) -> Self {
		self.auto_text = text.into();
		self.auto_tooltip = Some(tooltip.into());
		self
	}

	pub fn caption(mut self, caption: impl Into<String>) -> Self {
		self.caption = caption.into();
		self
	}

	pub fn spec(mut self, spec: NumberSpec) -> Self {
		self.spec = spec;
		self
	}

	pub fn orientation(mut self, orientation: StripOrientation) -> Self {
		self.orientation = orientation;
		self
	}

	pub fn tooltip(mut self, text: impl Into<String>) -> Self {
		self.tooltip = Some(text.into());
		self
	}

	/// The brightness box's id, for focusing it from outside.
	pub fn box_id(mut self, id: AccessibilityId) -> Self {
		self.box_id = Some(id);
		self
	}

	/// Base of the ids: `<id>Brightness`, `<id>AutoBrightness`.
	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = id.into();
		self
	}
}

/// An invisible copy of `text` that reports its natural width into `target`.
fn measure(text: String, weight: FontWeight, mut target: State<f32>) -> Rect {
	rect().position(Position::new_absolute()).width(Size::px(4000.)).opacity(0.).interactive(Interactive::No).child(
		rect()
			.on_sized(move |e: Event<SizedEventData>| target.set_if_modified(e.area.width()))
			.child(label().text(text).font_weight(weight).max_lines(1)),
	)
}

fn tipped(tip: Option<String>, inner: Rect) -> Element {
	match tip {
		Some(t) => Tip::new(t).child(inner).into_element(),
		None => inner.into_element(),
	}
}

impl HeaderStrip {
	/// The title, the flexible gap and, as fits, the caption; the tools follow.
	fn row(&self, layout: StripLayout, title_w: State<f32>, caption_w: State<f32>) -> Rect {
		let theme = use_wave_theme();
		rect()
			.height(Size::px(theme.row_height))
			.horizontal()
			.content(Content::flex())
			.cross_align(Alignment::center())
			.background(theme.window_background)
			.color(theme.text)
			.font_size(theme.font_size)
			.child(measure(self.title.clone(), FontWeight::SEMI_BOLD, title_w))
			.child(measure(self.caption.clone(), FontWeight::NORMAL, caption_w))
			.child(
				label()
					.text(self.title.clone())
					.font_weight(FontWeight::SEMI_BOLD)
					.max_lines(1)
					.text_overflow(TextOverflow::Ellipsis)
					.width(Size::px(layout.title_width)),
			)
			.child(rect().width(Size::flex(1.)))
			.maybe_child(layout.caption.then(|| {
				rect().padding((0., CAPTION_MARGIN, 0., 0.)).child(label().text(self.caption.clone()).max_lines(1))
			}))
	}
}

impl Component for HeaderStrip {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let own_box_id = use_a11y();
		let box_id = self.box_id.unwrap_or(own_box_id);
		let box_focus = use_focus(box_id);
		let auto_id = use_a11y();
		let auto_focus = use_focus(auto_id);
		let mut auto_hover = use_state(|| false);
		let mut length = use_state(|| 0f32);
		let title_w = use_state(|| 0f32);
		let caption_w = use_state(|| 0f32);
		let mut auto_w = use_state(|| 0f32);
		let vertical = self.orientation == StripOrientation::Vertical;
		let thickness = theme.row_height;
		let l = *length.read();
		let aw = *auto_w.read();
		let layout =
			strip_layout(l, *title_w.read(), *caption_w.read() + CAPTION_MARGIN, BOX_WIDTH + AUTO_GAP + aw, SPACING);
		let row = self.row(layout, title_w, caption_w);
		let number = NumberBox::new(self.value.clone(), self.spec.clone()).width(Size::px(BOX_WIDTH)).a11y_id(box_id);
		let measure_auto = move |e: Event<SizedEventData>| auto_w.set_if_modified(e.area.width());

		if !vertical {
			let mut number = number.test_id(format!("{}Brightness", self.test_id));
			if let Some(tip) = &self.tooltip {
				number = number.tooltip(tip.clone());
			}
			let on_auto = self.on_auto.clone();
			let auto = rect().on_sized(measure_auto).test_id(format!("{}AutoBrightness", self.test_id)).child(
				Button::new()
					.compact()
					.on_press(move |_| on_auto.call(()))
					.child(label().text(self.auto_text.clone()).max_lines(1)),
			);
			return rect()
				.width(Size::fill())
				.padding((0., 0., END_MARGIN, 0.))
				.on_sized(move |e: Event<SizedEventData>| length.set_if_modified(e.area.width()))
				.child(
					row.width(Size::fill())
						.child(number)
						.child(rect().width(Size::px(AUTO_GAP)))
						.child(tipped(self.auto_tooltip.clone(), auto)),
				)
				.into_element();
		}

		// On its side the row shows likenesses of the box and the button: Freya rotates the drawing but neither the
		// hit testing nor the clipping of a text box, so the box edits upright over its likeness while focused and
		// waits outside the window otherwise, and transparent stand-ins over the turned parts take the pointer.
		let editing = box_focus().is_focused();
		let spin_face = |icon| {
			rect().width(Size::fill()).height(Size::percent(50.)).center().child(IconView::new(icon).color(theme.text))
		};
		let box_face = field_frame(&theme, true, false, false)
			.width(Size::px(BOX_WIDTH))
			.height(Size::px(INPUT_HEIGHT))
			.child(
				rect()
					.width(Size::flex(1.))
					.padding((0., 3., 0., 3.))
					.maybe_child((!editing).then(|| label().text(self.spec.format(*self.value.read())).max_lines(1))),
			)
			.child(
				rect()
					.width(Size::px(SPIN_WIDTH))
					.height(Size::fill())
					.child(spin_face(icons::SPIN_UP))
					.child(spin_face(icons::SPIN_DOWN)),
			);
		let auto_face = rect()
			.height(Size::px(thickness))
			.padding((0., 7., 0., 7.))
			.center()
			.background(if *auto_hover.read() { theme.button_hover } else { theme.button_background })
			.border(Border::new().fill(theme.button_border).width(1.).alignment(BorderAlignment::Inner))
			.border(focus_border(auto_focus(), theme.accent))
			.on_sized(measure_auto)
			.child(label().text(self.auto_text.clone()).max_lines(1));
		let turned = row
			.width(Size::px(l))
			.child(box_face)
			.child(rect().width(Size::px(AUTO_GAP)))
			.child(auto_face)
			.interactive(Interactive::No)
			.position(Position::new_absolute().left(thickness).top(0.))
			.rotate(90.)
			.transform_origin(TransformOrigin::top_left());

		// In the strip's own coordinates: the row's position along it is y, its top half the right half.
		let auto_top = l - aw;
		let box_end = auto_top - AUTO_GAP;
		let spin_top = box_end - SPIN_WIDTH;
		let box_top = box_end - BOX_WIDTH;
		let half = thickness / 2.;
		let area = |left: f32, top: f32, width: f32, height: f32| {
			rect()
				.position(Position::new_absolute().left(left).top(top))
				.width(Size::px(width))
				.height(Size::px(height.max(0.)))
		};
		let left_press = |e: &Event<PointerEventData>| e.button() == Some(MouseButton::Left);
		let step = {
			let spec = self.spec.clone();
			let value = self.value.clone();
			move |steps: i32| {
				let mut value = value.clone();
				let next = spec.step(*value.peek(), steps).value;
				value.set_if_modified(next);
			}
		};
		let spin = |steps: i32, left: f32| {
			let step = step.clone();
			area(left, spin_top, half, SPIN_WIDTH).on_pointer_down(move |e: Event<PointerEventData>| {
				if left_press(&e) {
					box_id.request_focus();
					step(steps);
				}
			})
		};
		let (up, down) = (spin(1, half), spin(-1, 0.));
		let text_area = area(0., box_top, thickness, BOX_WIDTH - SPIN_WIDTH)
			.test_id(format!("{}Brightness", self.test_id))
			.on_pointer_down(move |e: Event<PointerEventData>| {
				if left_press(&e) {
					box_id.request_focus();
				}
			})
			.on_wheel(move |e: Event<WheelEventData>| {
				if box_id.is_focused() && e.delta_y != 0. {
					e.stop_propagation();
					step(if e.delta_y > 0. { 1 } else { -1 });
				}
			});
		let editor = if editing {
			area(half - BOX_WIDTH / 2., box_top + (BOX_WIDTH - thickness) / 2., BOX_WIDTH, thickness)
				.layer(Layer::Overlay)
				.center()
		} else {
			area(-AWAY, 0., BOX_WIDTH, thickness)
		};
		let (press, key_press) = (self.on_auto.clone(), self.on_auto.clone());
		let auto_area = area(0., auto_top, thickness, aw)
			.a11y_id(auto_id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Button)
			.test_id(format!("{}AutoBrightness", self.test_id))
			.on_pointer_enter(move |_| auto_hover.set(true))
			.on_pointer_leave(move |_| auto_hover.set(false))
			.on_pointer_press(move |e: Event<PointerEventData>| {
				if left_press(&e) {
					auto_id.request_focus();
					press.call(());
				}
			})
			.on_key_down(move |e: Event<KeyboardEventData>| {
				if is_named(&e, NamedKey::Enter) || e.key == Key::Character(" ".into()) {
					e.stop_propagation();
					e.prevent_default();
					key_press.call(());
				}
			});

		rect()
			.width(Size::px(thickness))
			.height(Size::fill())
			.padding((SIDE_START, 0., END_MARGIN, 0.))
			.child(
				rect()
					.width(Size::fill())
					.height(Size::fill())
					.on_sized(move |e: Event<SizedEventData>| length.set_if_modified(e.area.height()))
					.child(turned)
					.child(tipped(self.tooltip.clone(), text_area))
					.child(up)
					.child(down)
					.child(tipped(self.auto_tooltip.clone(), auto_area))
					.child(editor.child(number)),
			)
			.into_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn caption_gives_way_before_the_title() {
		assert_eq!(strip_layout(300., 40., 64., 110., 12.), StripLayout { caption: true, title_width: 40. });
		assert_eq!(strip_layout(200., 40., 64., 110., 12.), StripLayout { caption: false, title_width: 40. });
		assert_eq!(strip_layout(140., 40., 64., 110., 12.), StripLayout { caption: false, title_width: 18. });
		assert_eq!(strip_layout(50., 40., 64., 110., 12.), StripLayout { caption: false, title_width: 0. });
	}

	#[test]
	fn strip_goes_beside_a_wide_frame() {
		assert!(strip_beside((800., 200.), (100, 100)));
		assert!(!strip_beside((200., 800.), (100, 100)));
		assert!(!strip_beside((400., 200.), (200, 100)));
		assert!(!strip_beside((400., 0.), (200, 100)));
	}
}
