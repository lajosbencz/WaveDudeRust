//! Skia helpers for the callback-drawn widgets: device-pixel snapping and one-line text.

use std::borrow::Cow;

use freya::engine::prelude::{
	Canvas, FontCollection, Paint, PaintStyle, ParagraphBuilder, ParagraphStyle, Shader, SkColor4f, SkParagraph,
	TextStyle, TileMode, gradient,
};
use freya::prelude::{Color, FillRenderContext};

/// Coordinates of a render callback switched to whole device pixels.
///
/// After [`DeviceGrid::enter`] the canvas unit is one device pixel and integers lie on the screen's pixel
/// grid; [`DeviceGrid::x`] and [`DeviceGrid::y`] map the element's logical coordinates into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DeviceGrid {
	pub scale: f32,
	pub fx: f32,
	pub fy: f32,
}

impl DeviceGrid {
	pub fn new(scale: f32, origin_x: f32, origin_y: f32) -> Self {
		let ox = origin_x * scale;
		let oy = origin_y * scale;
		Self { scale, fx: ox - ox.floor(), fy: oy - oy.floor() }
	}

	pub fn from_ctx(ctx: &FillRenderContext) -> Self {
		Self::new(ctx.scale_factor as f32, ctx.origin.x, ctx.origin.y)
	}

	/// Saves the canvas and switches it to device pixels; restore with `canvas.restore()`.
	pub fn enter(&self, canvas: &Canvas) {
		canvas.save();
		canvas.scale((1. / self.scale, 1. / self.scale));
		canvas.translate((-self.fx, -self.fy));
	}

	pub fn x(&self, logical: f32) -> f32 {
		logical * self.scale + self.fx
	}

	pub fn y(&self, logical: f32) -> f32 {
		logical * self.scale + self.fy
	}

	/// The whole number of device pixels nearest to one logical pixel, at least 1.
	pub fn hairline(&self) -> f32 {
		self.scale.round().max(1.)
	}

	/// Centre of a hairline of [`Self::hairline`] width whose first device column is the one under `device`.
	pub fn snap(&self, device: f32) -> f32 {
		device.floor() + self.hairline() / 2.
	}

	pub fn width(&self, logical: f32) -> f32 {
		(logical * self.scale).round()
	}
}

pub(crate) fn stroke(color: Color, width: f32) -> Paint {
	let mut paint = Paint::default();
	paint.set_style(PaintStyle::Stroke);
	paint.set_stroke_width(width);
	paint.set_color(color);
	paint
}

pub(crate) fn fill(color: Color) -> Paint {
	let mut paint = Paint::default();
	paint.set_style(PaintStyle::Fill);
	paint.set_color(color);
	paint
}

/// A linear gradient from `from` to `to` (canvas units); `positions` 0..1 per colour, or evenly spaced.
pub(crate) fn linear_shader(
	from: (f32, f32),
	to: (f32, f32),
	colors: &[Color],
	positions: Option<&[f32]>,
) -> Option<Shader> {
	let colors: Vec<SkColor4f> = colors.iter().map(|c| SkColor4f::from(*c)).collect();
	let spec = gradient::Gradient::new(
		gradient::Colors::new(&colors, positions, TileMode::Clamp, None),
		gradient::Interpolation::default(),
	);
	gradient::shaders::linear_gradient((from, to), &spec, None)
}

/// A laid-out single line of text at `size` (canvas units).
pub(crate) fn text_line(
	fonts: &mut FontCollection,
	families: &[Cow<'static, str>],
	text: &str,
	size: f32,
	color: Color,
) -> SkParagraph {
	let mut style = TextStyle::new();
	style.set_color(color);
	style.set_font_size(size);
	// skia-safe reads an empty family list through a null pointer, which debug builds abort on.
	if !families.is_empty() {
		let families: Vec<&str> = families.iter().map(|f| f.as_ref()).collect();
		style.set_font_families(&families);
	}
	let mut paragraph_style = ParagraphStyle::new();
	paragraph_style.set_text_style(&style);
	paragraph_style.set_max_lines(1);
	let mut builder = ParagraphBuilder::new(&paragraph_style, fonts.clone());
	builder.push_style(&style);
	builder.add_text(text);
	let mut paragraph = builder.build();
	paragraph.layout(100_000.);
	paragraph
}

pub(crate) fn text_width(paragraph: &SkParagraph) -> f32 {
	paragraph.max_intrinsic_width()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn snapping_lands_on_pixel_centres() {
		let g = DeviceGrid::new(1., 10.25, 0.);
		assert_eq!(g.fx, 0.25);
		assert_eq!(g.snap(g.x(3.)), 3.5);
		let g2 = DeviceGrid::new(2., 0., 0.);
		assert_eq!(g2.hairline(), 2.);
		assert_eq!(g2.snap(g2.x(3.)), 7.);
		let g15 = DeviceGrid::new(1.5, 0., 0.);
		assert_eq!(g15.hairline(), 2.);
	}
}
