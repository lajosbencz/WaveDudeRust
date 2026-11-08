//! A picture's header strip: title, a caption that gives way first, then a number box and an Auto button
//! at the far end; laid out horizontally above a picture, or beside it with the title on its side and the
//! box and the button upright below it (a rotated control takes no clicks in Freya, rotation is visual).

use freya::prelude::*;

use crate::number_box::{NumberBox, NumberSpec};
use crate::theme::use_wave_theme;
use crate::util::TestIdExt;

/// Above the picture, or on its left with the title reading bottom to top.
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

/// Lengths in logical pixels; `gap` sits between neighbouring parts.
pub fn strip_layout(length: f32, title: f32, caption: f32, tools: f32, gap: f32) -> StripLayout {
	let rest = (length - tools - gap).max(0.);
	if title + gap + caption <= rest {
		StripLayout { caption: true, title_width: title }
	} else {
		StripLayout { caption: false, title_width: title.min(rest) }
	}
}

/// Whether the picture frame puts the strip beside the picture: the cell is wider than the picture's
/// aspect, so the spare room is at the sides.
pub fn strip_beside(cell: (f32, f32), canvas: (u32, u32)) -> bool {
	let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
	cell.0 * ch > cell.1 * cw
}

const GAP: f32 = 6.;
const BOX_WIDTH: f32 = 64.;
const AUTO_WIDTH: f32 = 40.;
/// Width of the vertical strip: the box plus its padding.
pub const VERTICAL_WIDTH: f32 = BOX_WIDTH + 8.;

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

impl Component for HeaderStrip {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let mut length = use_state(|| 0f32);
		let mut across = use_state(|| 0f32);
		let title_w = use_state(|| 0f32);
		let caption_w = use_state(|| 0f32);
		let vertical = self.orientation == StripOrientation::Vertical;
		let tools = BOX_WIDTH + GAP + AUTO_WIDTH;
		let layout = strip_layout(*length.read(), *title_w.read(), *caption_w.read(), tools, GAP);
		let height = theme.row_height;

		let measure = |text: String, mut target: State<f32>| {
			rect()
				.position(Position::new_absolute())
				.width(Size::px(4000.))
				.opacity(0.)
				.interactive(Interactive::No)
				.child(
					rect()
						.on_sized(move |e: Event<SizedEventData>| target.set_if_modified(e.area.width()))
						.child(label().text(text).max_lines(1)),
				)
		};

		let on_auto = self.on_auto.clone();
		let mut number = NumberBox::new(self.value.clone(), self.spec.clone())
			.width(Size::px(BOX_WIDTH))
			.test_id(format!("{}Brightness", self.test_id));
		if let Some(tip) = &self.tooltip {
			number = number.tooltip(tip.clone());
		}
		if let Some(id) = self.box_id {
			number = number.a11y_id(id);
		}
		let auto = Button::new()
			.flat()
			.compact()
			.on_press(move |_| on_auto.call(()))
			.child(label().text(self.auto_text.clone()))
			.test_id_wrap(format!("{}AutoBrightness", self.test_id));
		let auto = match self.auto_tooltip.clone() {
			Some(tip) => TooltipContainer::new(Tooltip::new_text(tip)).child(auto).into_element(),
			None => auto.into_element(),
		};

		if vertical {
			let l = *length.read();
			let title_len = (l - 2. * height - 3. * GAP).max(0.);
			rect()
				.width(Size::px(VERTICAL_WIDTH))
				.height(Size::fill())
				.background(theme.window_background)
				.color(theme.text)
				.font_size(theme.font_size)
				.on_sized(move |e: Event<SizedEventData>| {
					length.set_if_modified(e.area.height() - 8.);
					across.set_if_modified(e.area.width());
				})
				.child(measure(self.title.clone(), title_w))
				.child(
					rect()
						.position(
							Position::new_absolute()
								.left((VERTICAL_WIDTH - title_len) / 2.)
								.top((title_len - height) / 2. + 4.),
						)
						.width(Size::px(title_len))
						.height(Size::px(height))
						.rotation(-90.)
						.horizontal()
						.cross_align(Alignment::center())
						.child(
							label()
								.text(self.title.clone())
								.max_lines(1)
								.text_overflow(TextOverflow::Ellipsis)
								.width(Size::px(title_len)),
						),
				)
				.child(
					rect()
						.position(Position::new_absolute().left(4.).top(title_len + 2. * GAP + 4.))
						.width(Size::px(BOX_WIDTH))
						.spacing(GAP)
						.cross_align(Alignment::center())
						.child(number)
						.child(auto),
				)
				.into_element()
		} else {
			let row = rect()
				.width(if vertical { Size::px(*length.read()) } else { Size::fill() })
				.height(Size::px(height))
				.horizontal()
				.content(Content::flex())
				.cross_align(Alignment::center())
				.padding((0., 4., 0., 4.))
				.spacing(GAP)
				.background(theme.window_background)
				.color(theme.text)
				.font_size(theme.font_size)
				.child(measure(self.title.clone(), title_w))
				.child(measure(self.caption.clone(), caption_w))
				.child(
					label()
						.text(self.title.clone())
						.max_lines(1)
						.text_overflow(TextOverflow::Ellipsis)
						.width(Size::px(layout.title_width)),
				)
				.child(rect().width(Size::flex(1.)))
				.maybe_child(
					layout.caption.then(|| label().text(self.caption.clone()).color(theme.text_dim).max_lines(1)),
				)
				.child(number)
				.child(auto);
			rect()
				.width(Size::fill())
				.on_sized(move |e: Event<SizedEventData>| length.set_if_modified(e.area.width() - 8.))
				.child(row)
				.into_element()
		}
	}
}

trait ButtonTestId {
	fn test_id_wrap(self, id: String) -> Rect;
}

impl ButtonTestId for Button {
	fn test_id_wrap(self, id: String) -> Rect {
		rect().test_id(id).child(self)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn caption_gives_way_before_the_title() {
		assert_eq!(strip_layout(300., 40., 60., 110., 6.), StripLayout { caption: true, title_width: 40. });
		assert_eq!(strip_layout(200., 40., 60., 110., 6.), StripLayout { caption: false, title_width: 40. });
		assert_eq!(strip_layout(140., 40., 60., 110., 6.), StripLayout { caption: false, title_width: 24. });
		assert_eq!(strip_layout(50., 40., 60., 110., 6.), StripLayout { caption: false, title_width: 0. });
	}

	#[test]
	fn strip_goes_beside_a_wide_cell() {
		assert!(strip_beside((800., 200.), (100, 100)));
		assert!(!strip_beside((200., 800.), (100, 100)));
		assert!(!strip_beside((400., 200.), (200, 100)));
	}
}
