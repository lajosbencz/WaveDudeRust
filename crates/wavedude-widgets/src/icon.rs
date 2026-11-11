//! 16 px icons: a glyph from a font or a stroked SVG path on a 16 x 16 grid, drawn in the text colour.

use std::borrow::Cow;

use freya::engine::prelude::SkPath;
use freya::prelude::*;

use crate::paint::{fill, stroke};

/// An icon. Path data is SVG syntax on a 16 x 16 grid with ink in rows and columns 0..14; strokes 1 px on
/// half pixels.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Icon {
	#[default]
	None,
	/// A character of the default (or an icon) font, centred.
	Glyph(Cow<'static, str>),
	/// A stroked path, optionally with a filled path under it.
	Path { stroke: Cow<'static, str>, fill: Option<Cow<'static, str>> },
}

impl Icon {
	pub const fn path(stroke: &'static str) -> Self {
		Icon::Path { stroke: Cow::Borrowed(stroke), fill: None }
	}

	pub const fn filled(fill: &'static str) -> Self {
		Icon::Path { stroke: Cow::Borrowed(""), fill: Some(Cow::Borrowed(fill)) }
	}

	pub fn glyph(text: impl Into<Cow<'static, str>>) -> Self {
		Icon::Glyph(text.into())
	}

	pub fn is_none(&self) -> bool {
		matches!(self, Icon::None)
	}
}

/// Built-in drawings the widgets themselves need.
pub mod icons {
	use super::Icon;

	pub const CHEVRON_RIGHT: Icon = Icon::path("M5.5 3.5 L9.5 7.5 L5.5 11.5");
	pub const CHEVRON_DOWN: Icon = Icon::path("M3.5 5.5 L7.5 9.5 L11.5 5.5");
	pub const CHEVRON_UP: Icon = Icon::path("M3.5 9.5 L7.5 5.5 L11.5 9.5");
	/// A row's drag grip: three short lines.
	pub const GRIP: Icon = Icon::path("M5 5.5 L10 5.5 M5 7.5 L10 7.5 M5 9.5 L10 9.5");
	/// The number box's spin arrows: 4 x 2 around the centre.
	pub const SPIN_UP: Icon = Icon::path("M6 9 L8 7 L10 9");
	pub const SPIN_DOWN: Icon = Icon::path("M6 7 L8 9 L10 7");
	pub const CLOSE: Icon = Icon::path("M3.5 3.5 L11.5 11.5 M11.5 3.5 L3.5 11.5");
	pub const PLUS: Icon = Icon::path("M7.5 2.5 L7.5 12.5 M2.5 7.5 L12.5 7.5");
	pub const MINUS: Icon = Icon::path("M2.5 7.5 L12.5 7.5");
	pub const CHECK: Icon = Icon::path("M2.5 7.5 L5.5 10.5 L12.5 3.5");
	pub const RADIO: Icon = Icon::filled("M7.5 4.5 A3 3 0 1 1 7.49 4.5 Z");
	pub const DROPDOWN: Icon = Icon::filled("M4 6 L11 6 L7.5 9.5 Z");
	pub const SUBMENU: Icon = Icon::filled("M6 4 L9.5 7.5 L6 11 Z");
	pub const PLAY: Icon = Icon::filled("M4 2 L12 7.5 L4 13 Z");
	pub const PAUSE: Icon = Icon::filled("M4 2 L6.5 2 L6.5 13 L4 13 Z M8.5 2 L11 2 L11 13 L8.5 13 Z");
	pub const SOURCE: Icon = Icon::path(
		"M7.5 3.5 A4 4 0 1 1 7.49 3.5 Z M7.5 0.5 L7.5 2.5 M7.5 12.5 L7.5 14.5 M0.5 7.5 L2.5 7.5 M12.5 7.5 L14.5 7.5",
	);
	pub const PROBE: Icon =
		Icon::path("M3.5 3.5 L11.5 3.5 L11.5 11.5 L3.5 11.5 Z M7.5 0.5 L7.5 3.5 M7.5 11.5 L7.5 14.5");
	pub const FOLDER: Icon = Icon::path("M0.5 3.5 L5.5 3.5 L7.5 5.5 L14.5 5.5 L14.5 12.5 L0.5 12.5 Z");
	pub const GEAR: Icon = Icon::path(
		"M7.5 4.5 A3 3 0 1 1 7.49 4.5 Z M7.5 0.5 L7.5 2.5 M7.5 12.5 L7.5 14.5 M0.5 7.5 L2.5 7.5 M12.5 7.5 L14.5 7.5 M2.5 2.5 L4 4 M11 11 L12.5 12.5 M12.5 2.5 L11 4 M4 11 L2.5 12.5",
	);
}

/// Draws an [`Icon`] in a square of `size` logical pixels.
#[derive(Clone, PartialEq)]
pub struct IconView {
	icon: Icon,
	color: Option<Color>,
	size: f32,
}

impl IconView {
	pub fn new(icon: Icon) -> Self {
		Self { icon, color: None, size: 16. }
	}

	pub fn color(mut self, color: Color) -> Self {
		self.color = Some(color);
		self
	}

	pub fn size(mut self, size: f32) -> Self {
		self.size = size;
		self
	}
}

impl Component for IconView {
	fn render(&self) -> impl IntoElement {
		let color = self.color.unwrap_or_else(|| crate::theme::use_wave_theme().text);
		let size = self.size;
		let base = rect().width(Size::px(size)).height(Size::px(size));
		match self.icon.clone() {
			Icon::None => base,
			Icon::Glyph(text) => base.center().child(label().text(text).color(color).font_size(size * 0.8)),
			Icon::Path { stroke: s, fill: f } => {
				let stroke_path = SkPath::from_svg(s.as_ref());
				let fill_path = f.as_ref().and_then(|f| SkPath::from_svg(f.as_ref()));
				base.background(RenderCallback::new(move |ctx: &mut FillRenderContext<'_>| {
					let RenderTarget::Background { canvas, .. } = &mut ctx.target else { return };
					canvas.save();
					canvas.scale((size / 16., size / 16.));
					if let Some(p) = &fill_path {
						let mut paint = fill(color);
						paint.set_anti_alias(true);
						canvas.draw_path(p, &paint);
					}
					if let Some(p) = &stroke_path {
						let mut paint = stroke(color, 1.);
						paint.set_anti_alias(true);
						canvas.draw_path(p, &paint);
					}
					canvas.restore();
				}))
			}
		}
	}
}
