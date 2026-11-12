//! Skia helpers for the callback-drawn widgets: device-pixel snapping, one-line text, and the executor of
//! [`Shape`] lists.

use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};

use freya::engine::prelude::{
	Canvas, ClipOp, Font, FontCollection, FontMgr, FontStyle, ISize, Paint, PaintCap, PaintJoin, PaintStyle,
	ParagraphBuilder, ParagraphStyle, PathBuilder, PathEffect, Shader, SkColor4f, SkImage, SkParagraph, SkRect, Slant,
	TextStyle, TileMode, Typeface, Weight, Width, gradient, raster_n32_premul,
};
use freya::prelude::{Color, FillRenderContext};
pub(crate) use wavedude_view::DeviceGrid;
use wavedude_view::draw::{Baseline, Disc, Ink, Pen, Rect, Rgba, Shape, TextMetrics};

/// A render callback's [`DeviceGrid`] and the canvas switched to it.
pub(crate) trait DeviceGridExt {
	fn from_ctx(ctx: &FillRenderContext<'_>) -> Self;

	/// Saves the canvas and switches it to device pixels; restore with `canvas.restore()`.
	fn enter(&self, canvas: &Canvas);
}

impl DeviceGridExt for DeviceGrid {
	fn from_ctx(ctx: &FillRenderContext<'_>) -> Self {
		Self::new(ctx.scale_factor as f32, ctx.origin.x, ctx.origin.y)
	}

	fn enter(&self, canvas: &Canvas) {
		canvas.save();
		canvas.scale((1. / self.scale, 1. / self.scale));
		canvas.translate((-self.fx, -self.fy));
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

/// A laid-out single line of text at `size` (canvas units) in the regular weight.
pub(crate) fn text_line(
	fonts: &mut FontCollection,
	families: &[Cow<'static, str>],
	text: &str,
	size: f32,
	color: Color,
) -> SkParagraph {
	weighted_text_line(fonts, families, text, size, color, Weight::NORMAL)
}

/// A laid-out single line of text at `size` (canvas units) in `weight`.
pub(crate) fn weighted_text_line(
	fonts: &mut FontCollection,
	families: &[Cow<'static, str>],
	text: &str,
	size: f32,
	color: Color,
	weight: Weight,
) -> SkParagraph {
	let mut style = TextStyle::new();
	style.set_color(color);
	style.set_font_size(size);
	style.set_font_style(FontStyle::new(weight, Width::NORMAL, Slant::Upright));
	// skia-safe reads an empty family list through a null pointer, which debug builds abort on.
	if !families.is_empty() {
		let families: Vec<&str> = families.iter().map(AsRef::as_ref).collect();
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

/// The Skia colour of a draw-list colour.
pub fn color_of(c: Rgba) -> Color {
	Color::from_argb(c.a, c.r, c.g, c.b)
}

/// The draw-list colour of a Skia colour.
pub fn rgba_of(c: Color) -> Rgba {
	Rgba::new(c.r(), c.g(), c.b(), c.a())
}

fn sk_rect(r: &Rect) -> SkRect {
	SkRect::new(r.left, r.top, r.right, r.bottom)
}

fn pen_paint(pen: &Pen) -> Paint {
	let mut paint = stroke(color_of(pen.color), pen.width);
	paint.set_anti_alias(pen.antialias);
	if pen.round {
		paint.set_stroke_join(PaintJoin::Round);
		paint.set_stroke_cap(PaintCap::Round);
	}
	if let Some(dash) = pen.dash {
		paint.set_path_effect(PathEffect::dash(&dash, 0.));
	}
	paint
}

fn ink_paint(ink: &Ink) -> Paint {
	match ink {
		Ink::Stroke(pen) => pen_paint(pen),
		Ink::Fill { color, antialias } => {
			let mut paint = fill(color_of(*color));
			paint.set_anti_alias(*antialias);
			paint
		}
	}
}

thread_local! {
	static TYPEFACE: Option<Typeface> = FontMgr::default().legacy_make_typeface(None, FontStyle::normal());
}

/// The platform's default typeface at `size`: the font of alphabetic-baseline text.
fn baseline_font(size: f32) -> Font {
	let mut font = Font::default();
	TYPEFACE.with(|t| {
		if let Some(t) = t {
			font.set_typeface(t.clone());
		}
	});
	font.set_size(size);
	font
}

/// Metrics of text in the platform's default typeface at a size: alphabetic-baseline text as [`draw_shapes`]
/// draws it.
pub struct DefaultFontText(Font);

impl DefaultFontText {
	pub fn new(size: f32) -> Self {
		Self(baseline_font(size))
	}
}

impl TextMetrics for DefaultFontText {
	fn line_height(&self) -> f32 {
		self.0.spacing()
	}

	fn width(&self, text: &str) -> f32 {
		self.0.measure_str(text, None).0
	}
}

/// An element's fonts: its collection and the families it inherits, the font of top-anchored text.
pub struct ElementFonts<'a> {
	pub collection: &'a FontCollection,
	pub families: &'a [Cow<'static, str>],
}

/// The dots of a [`Shape::Dots`] as a bitmap at device pixels, rebuilt when the dots, their place or the device
/// scale change, so thousands of dots cost one image draw a frame. One slot: a cache serves one `Dots` shape of a
/// list; a second would rebuild the bitmap on every draw.
#[derive(Default)]
pub struct DotCache(RefCell<Option<(u64, SkImage)>>);

impl DotCache {
	fn draw(&self, canvas: &Canvas, origin: (f32, f32), size: (f32, f32), points: &[(f32, f32)], discs: [Disc; 2]) {
		let device = canvas.local_to_device_as_3x3().scale_x().abs().max(1e-3);
		let mut h = DefaultHasher::new();
		for p in points {
			(p.0.to_bits(), p.1.to_bits()).hash(&mut h);
		}
		for d in discs {
			(d.radius.to_bits(), d.color).hash(&mut h);
		}
		(origin.0.to_bits(), origin.1.to_bits(), size.0.to_bits(), size.1.to_bits(), device.to_bits()).hash(&mut h);
		let key = h.finish();
		let mut slot = self.0.borrow_mut();
		if slot.as_ref().is_none_or(|(k, _)| *k != key) {
			let pixels = ISize::new((size.0 * device).ceil().max(1.) as i32, (size.1 * device).ceil().max(1.) as i32);
			let Some(mut surface) = raster_n32_premul(pixels) else { return };
			let c = surface.canvas();
			c.scale((device, device));
			for disc in discs {
				let paint = ink_paint(&Ink::fill(disc.color));
				for &p in points {
					c.draw_circle(p, disc.radius, &paint);
				}
			}
			*slot = Some((key, surface.image_snapshot()));
		}
		if let Some((_, image)) = slot.as_ref() {
			canvas.save();
			canvas.translate(origin);
			canvas.scale((1. / device, 1. / device));
			canvas.draw_image(image, (0., 0.), None);
			canvas.restore();
		}
	}
}

/// Draws `shapes` in order. Top-anchored text needs `fonts` and a `Dots` shape needs `dots`; either missing
/// panics, as does a halo on top-anchored text.
pub fn draw_shapes(canvas: &Canvas, shapes: &[Shape], fonts: Option<&ElementFonts<'_>>, dots: Option<&DotCache>) {
	for shape in shapes {
		match shape {
			Shape::Line { from, to, pen } => {
				canvas.draw_line(*from, *to, &pen_paint(pen));
			}
			Shape::Path { points, closed, pen } => {
				let mut path = PathBuilder::new();
				for (i, &p) in points.iter().enumerate() {
					if i == 0 {
						path.move_to(p);
					} else {
						path.line_to(p);
					}
				}
				if *closed {
					path.close();
				}
				canvas.draw_path(&path.detach(), &pen_paint(pen));
			}
			Shape::Circle { centre, radius, ink } => {
				canvas.draw_circle(*centre, *radius, &ink_paint(ink));
			}
			Shape::Rect { rect, ink } => {
				canvas.draw_rect(sk_rect(rect), &ink_paint(ink));
			}
			Shape::Text { text, at, baseline: Baseline::Alphabetic, size, color, halo } => {
				let font = baseline_font(*size);
				if let Some(halo) = halo {
					let mut paint = stroke(color_of(halo.color), halo.width);
					paint.set_anti_alias(true);
					paint.set_stroke_join(PaintJoin::Round);
					canvas.draw_str(text, *at, &font, &paint);
				}
				canvas.draw_str(text, *at, &font, &ink_paint(&Ink::fill(*color)));
			}
			Shape::Text { text, at, baseline: Baseline::Top, size, color, halo } => {
				assert!(halo.is_none(), "a halo is drawn under alphabetic-baseline text only");
				let fonts = fonts.expect("top-anchored text is drawn in an element's fonts");
				let mut collection = fonts.collection.clone();
				text_line(&mut collection, fonts.families, text, *size, color_of(*color)).paint(canvas, *at);
			}
			Shape::Dots { origin, size, points, halo, dot } => {
				let dots = dots.expect("dots are drawn through a cache");
				dots.draw(canvas, *origin, *size, points, [*halo, *dot]);
			}
			Shape::Clip { rect, antialias } => {
				canvas.save();
				canvas.clip_rect(sk_rect(rect), ClipOp::Intersect, *antialias);
			}
			Shape::Unclip => {
				canvas.restore();
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use freya::engine::prelude::Surface;

	use super::*;

	fn pixel(surface: &mut Surface, x: i32, y: i32) -> Rgba {
		rgba_of(Color::from(surface.peek_pixels().expect("a raster surface").get_color((x, y))))
	}

	#[test]
	fn shapes_land_on_their_pixels_in_order_and_clipped() {
		let (white, red, green, blue) =
			(Rgba::rgb(255, 255, 255), Rgba::rgb(255, 0, 0), Rgba::rgb(0, 255, 0), Rgba::rgb(0, 0, 255));
		let aliased = |color| Ink::Fill { color, antialias: false };
		let shapes = [
			Shape::Rect { rect: Rect::new(0., 0., 20., 20.), ink: aliased(white) },
			Shape::Rect { rect: Rect::new(2., 2., 8., 8.), ink: aliased(red) },
			Shape::Line { from: (2., 12.5), to: (18., 12.5), pen: Pen::new(blue, 1.).aliased() },
			Shape::Clip { rect: Rect::new(10., 0., 20., 10.), antialias: false },
			Shape::Rect { rect: Rect::new(5., 2., 15., 8.), ink: aliased(green) },
			Shape::Unclip,
			Shape::Dots {
				origin: (0., 15.),
				size: (20., 5.),
				points: vec![(3.5, 2.5)],
				halo: Disc { radius: 2., color: blue },
				dot: Disc { radius: 1., color: red },
			},
		];
		let mut surface = raster_n32_premul(ISize::new(20, 20)).expect("a raster surface");
		draw_shapes(surface.canvas(), &shapes, None, Some(&DotCache::default()));
		assert_eq!(pixel(&mut surface, 3, 3), red);
		assert_eq!(pixel(&mut surface, 7, 3), red, "the clip keeps the green rectangle right of x 10");
		assert_eq!(pixel(&mut surface, 12, 3), green);
		assert_eq!(pixel(&mut surface, 12, 12), blue);
		assert_eq!(pixel(&mut surface, 12, 11), white, "an aliased hairline covers one row");
		assert_eq!(pixel(&mut surface, 3, 17), red, "a dot over its halo, placed from its origin");
		assert_eq!(pixel(&mut surface, 10, 17), white);
	}
}
