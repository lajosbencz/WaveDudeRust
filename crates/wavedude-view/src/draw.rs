//! What the shapes of [`crate::overlay`] and [`crate::graph`] share: text measured and laid out by epaint's fonts,
//! text over a halo stroked along its glyphs' outlines, and the device grid that puts hairlines and text on whole
//! device pixels.
//!
//! Shapes are epaint shapes in points; a canvas drawn at one device pixel per point (an export) has its pixels as
//! points.

use std::collections::HashMap;
use std::sync::Arc;

use epaint::text::{FontId, FontsView, Galley};
use epaint::{Color32, Pos2, Rect, Shape, Stroke, TextShape, pos2, vec2};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, HintingInstance, OutlinePen, SmoothMode, Target};
use skrifa::{FontRef, MetadataProvider};

use crate::fonts;

/// Text metrics of a layout, in the pixels of the layout.
pub trait TextMetrics {
	fn line_height(&mut self) -> f32;
	fn width(&mut self, text: &str) -> f32;
}

/// [`TextMetrics`] of one font through epaint's fonts, in points.
pub struct FontMetrics<'a, 'f> {
	pub fonts: &'a mut FontsView<'f>,
	pub font: FontId,
}

impl TextMetrics for FontMetrics<'_, '_> {
	fn line_height(&mut self) -> f32 {
		self.fonts.row_height(&self.font)
	}

	fn width(&mut self, text: &str) -> f32 {
		self.fonts.layout_no_wrap(text.to_owned(), self.font.clone(), Color32::PLACEHOLDER).size().x
	}
}

/// The distance from a galley's top to its first line's alphabetic baseline; 0 without glyphs.
pub fn baseline(galley: &Galley) -> f32 {
	galley.rows.first().and_then(|r| r.row.glyphs.first().map(|g| r.pos.y + g.pos.y)).unwrap_or(0.)
}

/// The halo under a text: the glyphs' outlines stroked `width` wide in `color`, so it reaches half of `width` past
/// the glyphs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Halo {
	pub color: Color32,
	pub width: f32,
}

/// Contours of a laid-out text relative to its galley's top-left, in points.
type Contours = Arc<[Vec<Pos2>]>;

/// The glyph outlines of Selawik Regular, else of the fallback, as closed contours in points, hinted as epaint hints the glyphs it draws,
/// curves flattened to a tenth of a device pixel; cached per text and size.
pub struct Outlines {
	/// Selawik Regular, then the fallback.
	fonts: [FontRef<'static>; 2],
	pixels_per_point: f32,
	cache: HashMap<(String, u32), Contours>,
}

/// Texts cached at most; the cache starts over beyond.
const OUTLINES_CACHED: usize = 1024;

impl Outlines {
	/// Outlines flattened for `pixels_per_point` device pixels per point.
	pub fn new(pixels_per_point: f32) -> Self {
		Self {
			fonts: [fonts::REGULAR_DATA, fonts::FALLBACK_DATA].map(|data| FontRef::new(data).expect("a bundled font")),
			pixels_per_point,
			cache: HashMap::new(),
		}
	}

	/// Flattens for another scale from now on.
	pub fn set_pixels_per_point(&mut self, pixels_per_point: f32) {
		if pixels_per_point != self.pixels_per_point {
			self.pixels_per_point = pixels_per_point;
			self.cache.clear();
		}
	}

	/// The contours of `galley`, laid out in Selawik Regular (with the fallback) at `size` points.
	fn contours(&mut self, galley: &Galley, size: f32) -> Contours {
		let key = (galley.text().to_owned(), size.to_bits());
		if let Some(c) = self.cache.get(&key) {
			return c.clone();
		}
		if self.cache.len() >= OUTLINES_CACHED {
			self.cache.clear();
		}
		let ppp = self.pixels_per_point;
		// epaint's default hinting, so the outlines meet the glyphs it rasterises
		let target =
			Target::Smooth { mode: SmoothMode::Normal, symmetric_rendering: true, preserve_linear_metrics: true };
		let faces = self.fonts.each_ref().map(|font| {
			let glyphs = font.outline_glyphs();
			let hinting = HintingInstance::new(&glyphs, Size::new(size * ppp), LocationRef::default(), target);
			(font.charmap(), glyphs, hinting)
		});
		let mut pen = Flattener { tolerance: 0.1, scale: 1. / ppp, ..Flattener::default() };
		for row in &galley.rows {
			for g in &row.row.glyphs {
				let found = faces.iter().find_map(|(charmap, glyphs, hinting)| {
					charmap.map(g.chr).and_then(|id| glyphs.get(id)).map(|outline| (outline, hinting))
				});
				let Some((outline, hinting)) = found else { continue };
				pen.origin = row.pos + g.pos.to_vec2();
				let settings = match hinting {
					Ok(instance) => DrawSettings::hinted(instance, false),
					Err(_) => DrawSettings::unhinted(Size::new(size * ppp), LocationRef::default()),
				};
				// An outline that fails to draw leaves its glyph without a halo; the text is drawn all the same.
				let _ = outline.draw(settings, &mut pen);
				pen.close();
			}
		}
		let contours: Contours = pen.contours.into();
		self.cache.insert(key, contours.clone());
		contours
	}
}

/// A pen that flattens outlines drawn in device pixels into closed contours in points (`scale` points per device
/// pixel), y down from `origin`, within `tolerance` device pixels.
#[derive(Default)]
struct Flattener {
	tolerance: f32,
	scale: f32,
	origin: Pos2,
	at: Pos2,
	current: Vec<Pos2>,
	contours: Vec<Vec<Pos2>>,
}

impl Flattener {
	fn point(&self, x: f32, y: f32) -> Pos2 {
		self.origin + vec2(x, -y) * self.scale
	}

	/// Segments for a curve whose largest second difference of control points is `m` and whose degree's Wang
	/// factor is `k`: the flattened curve within the tolerance.
	fn segments(&self, m: f32, k: f32) -> usize {
		(k * m / (self.tolerance * self.scale)).sqrt().ceil().clamp(1., 64.) as usize
	}
}

impl OutlinePen for Flattener {
	fn move_to(&mut self, x: f32, y: f32) {
		self.close();
		self.at = self.point(x, y);
		self.current.push(self.at);
	}

	fn line_to(&mut self, x: f32, y: f32) {
		self.at = self.point(x, y);
		self.current.push(self.at);
	}

	fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
		let (p0, c, p1) = (self.at, self.point(cx, cy), self.point(x, y));
		let n = self.segments((p0.to_vec2() - 2. * c.to_vec2() + p1.to_vec2()).length(), 0.25);
		for i in 1..=n {
			let t = i as f32 / n as f32;
			let u = 1. - t;
			self.current.push(pos2(
				u * u * p0.x + 2. * u * t * c.x + t * t * p1.x,
				u * u * p0.y + 2. * u * t * c.y + t * t * p1.y,
			));
		}
		self.at = p1;
	}

	fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
		let (p0, c0, c1, p1) = (self.at, self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
		let d0 = (p0.to_vec2() - 2. * c0.to_vec2() + c1.to_vec2()).length();
		let d1 = (c0.to_vec2() - 2. * c1.to_vec2() + p1.to_vec2()).length();
		let n = self.segments(d0.max(d1), 0.75);
		for i in 1..=n {
			let t = i as f32 / n as f32;
			let u = 1. - t;
			let (a, b, c, d) = (u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t);
			self.current
				.push(pos2(a * p0.x + b * c0.x + c * c1.x + d * p1.x, a * p0.y + b * c0.y + c * c1.y + d * p1.y));
		}
		self.at = p1;
	}

	fn close(&mut self) {
		if self.current.len() > 1 {
			self.contours.push(std::mem::take(&mut self.current));
		}
		self.current.clear();
	}
}

/// Pushes `galley` (Selawik Regular with the fallback at `size` points) at `pos`, its top-left, over its `halo`: one closed path per
/// contour of its glyphs, stroked, not filled.
pub fn haloed_text(
	out: &mut Vec<Shape>,
	outlines: &mut Outlines,
	pos: Pos2,
	galley: Arc<Galley>,
	size: f32,
	color: Color32,
	halo: Halo,
) {
	let stroke = Stroke::new(halo.width, halo.color);
	let offset = pos.to_vec2();
	out.extend(
		outlines
			.contours(&galley, size)
			.iter()
			.map(|c| Shape::closed_line(c.iter().map(|&p| p + offset).collect(), stroke)),
	);
	out.push(TextShape::new(pos, galley, color).with_override_text_color(color).into());
}

/// An element's logical pixels (points) mapped onto whole device pixels: `scale` device pixels per point, the
/// element's origin `fx`, `fy` of a device pixel past the device grid.
///
/// Device units are relative to the device pixel under the element's origin, so integers lie on the screen's pixel
/// grid; [`DeviceGrid::x`] and [`DeviceGrid::y`] map the element's logical coordinates into them and
/// [`DeviceGrid::point`] maps device units back to points of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceGrid {
	pub scale: f32,
	pub fx: f32,
	pub fy: f32,
	/// The device pixel under the element's origin.
	base: (f32, f32),
}

impl DeviceGrid {
	/// The grid of an element at `origin_x`, `origin_y` points in a window of `scale`.
	pub fn new(scale: f32, origin_x: f32, origin_y: f32) -> Self {
		let (ox, oy) = (origin_x * scale, origin_y * scale);
		Self { scale, fx: ox - ox.floor(), fy: oy - oy.floor(), base: (ox.floor(), oy.floor()) }
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

	/// The window's point at device units `x`, `y`.
	pub fn point(&self, x: f32, y: f32) -> Pos2 {
		pos2((self.base.0 + x) / self.scale, (self.base.1 + y) / self.scale)
	}

	/// The window's rectangle between device units `min` and `max`.
	pub fn rect(&self, min: (f32, f32), max: (f32, f32)) -> Rect {
		Rect::from_min_max(self.point(min.0, min.1), self.point(max.0, max.1))
	}

	/// The rectangle between the logical corners `from` and `to`, its edges on the nearest whole device pixels, in
	/// device units.
	pub fn snapped(&self, from: (f32, f32), to: (f32, f32)) -> [f32; 4] {
		[self.x(from.0).round(), self.y(from.1).round(), self.x(to.0).round(), self.y(to.1).round()]
	}
}

#[cfg(test)]
mod tests {
	use epaint::text::{Fonts, TextOptions};

	use super::*;

	#[test]
	fn snapping_lands_on_pixel_centres() {
		let g = DeviceGrid::new(1., 10.25, 0.);
		assert_eq!(g.fx, 0.25);
		assert_eq!(g.snap(g.x(3.)), 3.5);
		assert_eq!(g.point(3.5, 0.), pos2(13.5, 0.), "device units from the pixel under the origin");
		let g2 = DeviceGrid::new(2., 0., 0.);
		assert_eq!(g2.hairline(), 2.);
		assert_eq!(g2.snap(g2.x(3.)), 7.);
		assert_eq!(g2.point(7., 7.), pos2(3.5, 3.5));
		let g15 = DeviceGrid::new(1.5, 0., 0.);
		assert_eq!(g15.hairline(), 2.);
	}

	#[test]
	fn text_is_measured_and_drawn_over_its_halo() {
		let mut fonts = Fonts::new(TextOptions::default(), fonts::definitions());
		let mut view = fonts.with_pixels_per_point(1.);
		let mut m = FontMetrics { fonts: &mut view, font: FontId::proportional(11.) };
		let (wide, narrow) = (m.width("WWW"), m.width("iii"));
		assert!(wide > narrow && narrow > 0., "{wide} {narrow}");
		assert!((12. ..16.).contains(&m.line_height()), "{}", m.line_height());
		let galley = view.layout_no_wrap("Ag".into(), FontId::proportional(11.), Color32::WHITE);
		let base = baseline(&galley);
		assert!(base > 6. && base < galley.size().y, "{base}");
		let mut out = Vec::new();
		let mut outlines = Outlines::new(1.);
		let halo = Halo { color: Color32::BLACK, width: 3. };
		haloed_text(&mut out, &mut outlines, pos2(4., 4.), galley.clone(), 11., Color32::WHITE, halo);
		let paths: Vec<&epaint::PathShape> = out
			.iter()
			.filter_map(|s| match s {
				Shape::Path(p) => Some(p),
				_ => None,
			})
			.collect();
		assert_eq!(paths.len(), 4, "a path per contour: two of A, two of g");
		assert_eq!(out.len(), paths.len() + 1, "no copies of the text");
		assert!(paths.iter().all(|p| p.closed && p.fill == Color32::TRANSPARENT && p.stroke.width == 3.));
		let Shape::Text(top) = out.last().unwrap() else { panic!("text last") };
		assert_eq!((top.pos, top.override_text_color), (pos2(4., 4.), Some(Color32::WHITE)));
		let ink = Rect::from_points(&paths.iter().flat_map(|p| p.points.iter().copied()).collect::<Vec<_>>());
		let glyphs = galley.mesh_bounds.translate(vec2(4., 4.));
		assert!(
			glyphs.contains_rect(ink) && ink.expand(1.5).contains_rect(glyphs),
			"on the glyphs' quads: {ink:?} {glyphs:?}"
		);
		assert!(Arc::ptr_eq(&outlines.contours(&galley, 11.), &outlines.contours(&galley, 11.)), "cached");
	}
}
