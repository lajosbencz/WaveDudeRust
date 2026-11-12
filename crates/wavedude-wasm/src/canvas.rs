//! The executor of the draw list on a Canvas2D, after the desktop's Skia `Painter` (wavedude-widgets, `paint.rs`)
//! shape for shape. Canvas2D antialiases every shape: the antialias flags of the draw list are the desktop's.

use std::collections::HashMap;
use std::f64::consts::TAU;
use std::hash::{DefaultHasher, Hash, Hasher};

use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use wavedude_view::draw::{Disc, Ink, Pen, Rect, Rgba, Shape, TextMetrics, TextStyle};
use web_sys::{CanvasRenderingContext2d, Document, HtmlCanvasElement, ImageData};

use crate::overlay::{Group, Screen};
use crate::session::Picture;

/// The font family of [`TextStyle::Label`] text.
const LABEL_FAMILY: &str = "sans-serif";

/// A canvas and its 2D context.
struct Bitmap {
	canvas: HtmlCanvasElement,
	ctx: CanvasRenderingContext2d,
}

impl Bitmap {
	fn of(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
		let ctx = canvas
			.get_context("2d")?
			.ok_or_else(|| JsError::new("the canvas has a context other than 2d"))?
			.dyn_into()
			.map_err(JsValue::from)?;
		Ok(Self { canvas, ctx })
	}

	/// A canvas outside the document.
	fn detached(document: &Document) -> Result<Self, JsValue> {
		Self::of(document.create_element("canvas")?.dyn_into().map_err(JsValue::from)?)
	}

	/// Resizes to `size`, which clears the bitmap and resets its context, unless it is that size already.
	fn resize(&self, size: (u32, u32)) {
		if (self.canvas.width(), self.canvas.height()) != size {
			self.canvas.set_width(size.0);
			self.canvas.set_height(size.1);
		}
	}
}

fn css(c: Rgba) -> String {
	format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
}

fn font(size: f32, family: &str) -> String {
	format!("{size}px {family}")
}

fn measure(ctx: &CanvasRenderingContext2d, text: &str) -> web_sys::TextMetrics {
	ctx.measure_text(text).expect("measureText does not throw")
}

/// Text in `family` at a size measured by a context's `measureText`, in pixels divided by the scale it is
/// drawn at.
pub(crate) struct CanvasText<'a> {
	ctx: &'a CanvasRenderingContext2d,
	font: String,
	scale: f32,
	line_height: f32,
}

impl<'a> CanvasText<'a> {
	/// Text at `size` x `scale` pixels.
	fn new(ctx: &'a CanvasRenderingContext2d, family: &str, size: f32, scale: f32) -> Self {
		let mut text = Self { ctx, font: font(size * scale, family), scale, line_height: 0. };
		let m = text.measured("0");
		text.line_height = (m.font_bounding_box_ascent() + m.font_bounding_box_descent()) as f32 / scale;
		text
	}

	fn measured(&self, text: &str) -> web_sys::TextMetrics {
		self.ctx.set_font(&self.font);
		measure(self.ctx, text)
	}
}

impl TextMetrics for CanvasText<'_> {
	fn line_height(&self) -> f32 {
		self.line_height
	}

	fn width(&self, text: &str) -> f32 {
		self.measured(text).width() as f32 / self.scale
	}
}

/// A canvas the page shows a picture on, and the painter of its draw lists. Its font families, those of graph-box
/// text, are the canvas's computed `font-family`, read once: create it once the canvas is in the document.
#[wasm_bindgen]
pub struct View {
	canvas: Bitmap,
	painter: Painter,
}

#[wasm_bindgen]
impl View {
	#[wasm_bindgen(constructor)]
	pub fn new(canvas: HtmlCanvasElement) -> Result<View, JsValue> {
		let window = web_sys::window().ok_or_else(|| JsError::new("there is no window"))?;
		let document = window.document().ok_or_else(|| JsError::new("the window has no document"))?;
		let families = window
			.get_computed_style(&canvas)?
			.map(|style| style.get_property_value("font-family"))
			.transpose()?
			.filter(|f| !f.is_empty())
			.ok_or_else(|| JsError::new("the canvas has no computed font-family: it is not in the document"))?;
		let painter = Painter {
			families,
			picture: Bitmap::detached(&document)?,
			dots: Bitmap::detached(&document)?,
			dots_key: None,
			ascents: HashMap::new(),
		};
		Ok(Self { canvas: Bitmap::of(canvas)?, painter })
	}
}

impl View {
	/// Metrics of label text at `size` CSS pixels.
	pub(crate) fn label_text(&self, size: f32) -> CanvasText<'_> {
		CanvasText::new(&self.canvas.ctx, LABEL_FAMILY, size, 1.)
	}

	/// Metrics of graph-box text at `size` CSS pixels on a screen of `dpr`.
	pub(crate) fn box_text(&self, size: f32, dpr: f32) -> CanvasText<'_> {
		CanvasText::new(&self.canvas.ctx, &self.painter.families, size, dpr)
	}

	/// Sizes the canvas to the picture of `screen` and the edge line's room around it, draws `picture` scaled to it
	/// and `groups` over it, each in its own saved state.
	pub(crate) fn draw(&mut self, screen: Screen, picture: &Picture, groups: &[Group]) -> Result<(), JsValue> {
		let (width, height) = screen.device();
		let pad = screen.hairline() as u32;
		let size = (width + 2 * pad, height + 2 * pad);
		let ctx = &self.canvas.ctx;
		self.canvas.resize(size);
		ctx.clear_rect(0., 0., f64::from(size.0), f64::from(size.1));
		let bitmap = &self.painter.picture;
		bitmap.resize((picture.width, picture.height));
		let pixels = ImageData::new_with_u8_clamped_array_and_sh(
			Clamped(picture.rgba.as_slice()),
			picture.width,
			picture.height,
		)?;
		bitmap.ctx.put_image_data(&pixels, 0., 0.)?;
		let pad = f64::from(pad);
		ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
			&bitmap.canvas,
			pad,
			pad,
			f64::from(width),
			f64::from(height),
		)?;
		for Group { scale, origin, shapes } in groups {
			let s = f64::from(*scale);
			ctx.save();
			let drawn = ctx
				.set_transform(s, 0., 0., s, pad + f64::from(origin.0), pad + f64::from(origin.1))
				.and_then(|()| self.painter.shapes(ctx, shapes, *scale));
			ctx.restore();
			drawn?;
		}
		Ok(())
	}
}

/// What a [`View`] keeps between frames: two bitmaps, the picture's pixels and the [`Shape::Dots`] at device
/// pixels, the latter rebuilt when the dots, their place or the device scale change as the desktop's `Painter` does;
/// and the ascent of the graph-box font by size.
struct Painter {
	families: String,
	picture: Bitmap,
	dots: Bitmap,
	dots_key: Option<u64>,
	/// The graph-box font and its `fontBoundingBoxAscent` by the bits of its size.
	ascents: HashMap<u32, (String, f64)>,
}

impl Painter {
	/// Draws `shapes` in order in units of `scale` device pixels.
	fn shapes(&mut self, ctx: &CanvasRenderingContext2d, shapes: &[Shape], scale: f32) -> Result<(), JsValue> {
		for shape in shapes {
			ctx.begin_path();
			match shape {
				Shape::Line { from, to, pen } => {
					ctx.move_to(f64::from(from.0), f64::from(from.1));
					ctx.line_to(f64::from(to.0), f64::from(to.1));
					stroke(ctx, pen)?;
				}
				Shape::Path { points, closed, pen } => {
					for (i, p) in points.iter().enumerate() {
						if i == 0 {
							ctx.move_to(f64::from(p.0), f64::from(p.1));
						} else {
							ctx.line_to(f64::from(p.0), f64::from(p.1));
						}
					}
					if *closed {
						ctx.close_path();
					}
					stroke(ctx, pen)?;
				}
				Shape::Circle { centre, radius, ink } => {
					ctx.arc(f64::from(centre.0), f64::from(centre.1), f64::from(*radius), 0., TAU)?;
					paint(ctx, ink)?;
				}
				Shape::Rect { rect: r, ink } => {
					rect(ctx, r);
					paint(ctx, ink)?;
				}
				Shape::Text { text, at, style: TextStyle::Label { halo }, size, color } => {
					let (x, y) = (f64::from(at.0), f64::from(at.1));
					ctx.set_font(&font(*size, LABEL_FAMILY));
					set_pen(ctx, &Pen::new(halo.color, halo.width))?;
					ctx.set_line_join("round");
					ctx.stroke_text(text, x, y)?;
					ctx.set_fill_style_str(&css(*color));
					ctx.fill_text(text, x, y)?;
				}
				Shape::Text { text, at, style: TextStyle::GraphBox, size, color } => {
					let families = &self.families;
					let (font, ascent) = self.ascents.entry(size.to_bits()).or_insert_with(|| {
						let font = font(*size, families);
						ctx.set_font(&font);
						let ascent = measure(ctx, "0").font_bounding_box_ascent();
						(font, ascent)
					});
					ctx.set_font(font);
					ctx.set_fill_style_str(&css(*color));
					ctx.fill_text(text, f64::from(at.0), f64::from(at.1) + *ascent)?;
				}
				Shape::Dots { origin, size, points, halo, dot } => {
					self.draw_dots(ctx, *origin, *size, points, [*halo, *dot], scale)?;
				}
				Shape::Clip { rect: r, .. } => {
					ctx.save();
					rect(ctx, r);
					ctx.clip();
				}
				Shape::Unclip => ctx.restore(),
			}
		}
		Ok(())
	}

	fn draw_dots(
		&mut self,
		ctx: &CanvasRenderingContext2d,
		origin: (f32, f32),
		size: (f32, f32),
		points: &[(f32, f32)],
		discs: [Disc; 2],
		device: f32,
	) -> Result<(), JsValue> {
		let mut h = DefaultHasher::new();
		for p in points {
			(p.0.to_bits(), p.1.to_bits()).hash(&mut h);
		}
		for d in discs {
			(d.radius.to_bits(), d.color).hash(&mut h);
		}
		(origin.0.to_bits(), origin.1.to_bits(), size.0.to_bits(), size.1.to_bits(), device.to_bits()).hash(&mut h);
		let key = h.finish();
		let d = f64::from(device);
		if self.dots_key != Some(key) {
			// Setting the size, even to the same one, clears the bitmap and resets its context.
			self.dots.canvas.set_width((size.0 * device).ceil().max(1.) as u32);
			self.dots.canvas.set_height((size.1 * device).ceil().max(1.) as u32);
			let c = &self.dots.ctx;
			c.scale(d, d)?;
			for disc in discs {
				c.set_fill_style_str(&css(disc.color));
				for p in points {
					c.begin_path();
					c.arc(f64::from(p.0), f64::from(p.1), f64::from(disc.radius), 0., TAU)?;
					c.fill();
				}
			}
			self.dots_key = Some(key);
		}
		let (w, h) = (f64::from(self.dots.canvas.width()) / d, f64::from(self.dots.canvas.height()) / d);
		ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
			&self.dots.canvas,
			f64::from(origin.0),
			f64::from(origin.1),
			w,
			h,
		)
	}
}

fn rect(ctx: &CanvasRenderingContext2d, r: &Rect) {
	let (left, top) = (f64::from(r.left), f64::from(r.top));
	ctx.rect(left, top, f64::from(r.right) - left, f64::from(r.bottom) - top);
}

/// Miter joins at Skia's default limit and butt caps, or round ones.
fn set_pen(ctx: &CanvasRenderingContext2d, pen: &Pen) -> Result<(), JsValue> {
	ctx.set_stroke_style_str(&css(pen.color));
	ctx.set_line_width(f64::from(pen.width));
	let dash = js_sys::Array::new();
	for length in pen.dash.iter().flatten() {
		dash.push(&JsValue::from(*length));
	}
	ctx.set_line_dash(&dash)?;
	ctx.set_line_dash_offset(0.);
	let (join, cap) = if pen.round { ("round", "round") } else { ("miter", "butt") };
	ctx.set_line_join(join);
	ctx.set_line_cap(cap);
	ctx.set_miter_limit(4.);
	Ok(())
}

/// Strokes the current path with `pen`.
fn stroke(ctx: &CanvasRenderingContext2d, pen: &Pen) -> Result<(), JsValue> {
	set_pen(ctx, pen)?;
	ctx.stroke();
	Ok(())
}

/// Fills or strokes the current path.
fn paint(ctx: &CanvasRenderingContext2d, ink: &Ink) -> Result<(), JsValue> {
	match ink {
		Ink::Stroke(pen) => stroke(ctx, pen),
		Ink::Fill { color, .. } => {
			ctx.set_fill_style_str(&css(*color));
			ctx.fill();
			Ok(())
		}
	}
}
