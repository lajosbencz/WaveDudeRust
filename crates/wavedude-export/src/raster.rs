//! A small antialiased rasteriser over RGBA8 pixels: coverage masks of discs, rings, round-capped segments,
//! polylines and text (the embedded Noto Sans), blended as premultiplied colour.
//!
//! Coordinates are pixels, origin top-left, pixel (x, y) spanning [x, x+1) x [y, y+1); a shape's coverage of a pixel
//! is taken at the pixel centre from the distance to the shape's edge, a one-pixel ramp.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, Glyph, GlyphId, PxScale, ScaleFont, point};

/// A straight-alpha colour `[r, g, b, a]`.
pub type Color = [u8; 4];

/// A colour from ARGB components, the order the specification lists them in.
pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Color {
	[r, g, b, a]
}

pub const BLACK: Color = [0, 0, 0, 255];
pub const WHITE: Color = [255, 255, 255, 255];

/// A rectangle of whole pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IRect {
	pub x: i32,
	pub y: i32,
	pub width: i32,
	pub height: i32,
}

impl IRect {
	pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
		Self { x, y, width, height }
	}

	pub fn right(&self) -> i32 {
		self.x + self.width
	}

	pub fn bottom(&self) -> i32 {
		self.y + self.height
	}

	pub fn is_empty(&self) -> bool {
		self.width <= 0 || self.height <= 0
	}

	pub fn intersect(&self, other: &IRect) -> IRect {
		let x = self.x.max(other.x);
		let y = self.y.max(other.y);
		IRect::new(x, y, (self.right().min(other.right()) - x).max(0), (self.bottom().min(other.bottom()) - y).max(0))
	}
}

/// One source-over blend of a premultiplied channel: `min(255, src + (dst * (255 - a) + 127) / 255)`.
fn over(src: u32, dst: u8, a: u32) -> u8 {
	(src + (dst as u32 * (255 - a) + 127) / 255).min(255) as u8
}

/// Premultiplied `[r, g, b, a]` of a colour at a coverage.
fn premultiply(c: Color, coverage: f32) -> [u32; 4] {
	let a = (c[3] as f32 * coverage.clamp(0.0, 1.0)).round() as u32;
	let p = |v: u8| (v as u32 * a + 127) / 255;
	[p(c[0]), p(c[1]), p(c[2]), a]
}

/// RGBA8 pixels to draw on, `width * height * 4` bytes, premultiplied (an opaque frame is its own premultiplied form).
pub struct Canvas<'a> {
	pixels: &'a mut [u8],
	width: usize,
	height: usize,
}

impl<'a> Canvas<'a> {
	pub fn new(pixels: &'a mut [u8], width: usize, height: usize) -> Self {
		assert_eq!(pixels.len(), width * height * 4, "RGBA8 pixels of the given size");
		Self { pixels, width, height }
	}

	pub fn width(&self) -> usize {
		self.width
	}

	pub fn height(&self) -> usize {
		self.height
	}

	pub fn bounds(&self) -> IRect {
		IRect::new(0, 0, self.width as i32, self.height as i32)
	}

	fn index(&self, x: i32, y: i32) -> Option<usize> {
		(x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height)
			.then(|| (y as usize * self.width + x as usize) * 4)
	}

	/// Overwrites a pixel; outside the canvas nothing.
	pub fn set(&mut self, x: i32, y: i32, c: Color) {
		if let Some(i) = self.index(x, y) {
			self.pixels[i..i + 4].copy_from_slice(&c);
		}
	}

	pub fn get(&self, x: i32, y: i32) -> Option<Color> {
		self.index(x, y).map(|i| [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3]])
	}

	/// Blends `c` at `coverage` (0..1) over a pixel.
	pub fn blend(&mut self, x: i32, y: i32, c: Color, coverage: f32) {
		if let Some(i) = self.index(x, y) {
			let s = premultiply(c, coverage);
			if s[3] == 0 {
				return;
			}
			for k in 0..4 {
				self.pixels[i + k] = over(s[k], self.pixels[i + k], s[3]);
			}
		}
	}

	/// Blends `c` over a rectangle of whole pixels, clipped.
	pub fn fill_rect(&mut self, rect: IRect, c: Color) {
		let r = rect.intersect(&self.bounds());
		let s = premultiply(c, 1.0);
		for y in r.y..r.bottom() {
			let row = (y as usize * self.width + r.x as usize) * 4;
			for px in self.pixels[row..row + r.width as usize * 4].as_chunks_mut::<4>().0 {
				for k in 0..4 {
					px[k] = over(s[k], px[k], s[3]);
				}
			}
		}
	}

	/// Blends the one-pixel outline of a rectangle (each pixel once).
	pub fn stroke_rect(&mut self, rect: IRect, c: Color) {
		if rect.is_empty() {
			return;
		}
		self.fill_rect(IRect::new(rect.x, rect.y, rect.width, 1), c);
		if rect.height > 1 {
			self.fill_rect(IRect::new(rect.x, rect.bottom() - 1, rect.width, 1), c);
		}
		let inner = IRect::new(rect.x, rect.y + 1, 1, rect.height - 2);
		self.fill_rect(inner, c);
		if rect.width > 1 {
			self.fill_rect(IRect { x: rect.right() - 1, ..inner }, c);
		}
	}

	/// Blends `c` through a coverage mask.
	pub fn fill_mask(&mut self, mask: &Mask, c: Color) {
		let r = mask.bounds().intersect(&self.bounds());
		for y in r.y..r.bottom() {
			for x in r.x..r.right() {
				let cov = mask.at(x, y);
				if cov > 0.0 {
					self.blend(x, y, c, cov);
				}
			}
		}
	}

	/// Blends a premultiplied RGBA8 layer of the canvas's size over it, within `rect` only.
	pub fn composite(&mut self, layer: &[u8], rect: IRect) {
		assert_eq!(layer.len(), self.pixels.len(), "a layer of the canvas's size");
		let r = rect.intersect(&self.bounds());
		for y in r.y..r.bottom() {
			let start = (y as usize * self.width + r.x as usize) * 4;
			let end = start + r.width as usize * 4;
			for (d, s) in
				self.pixels[start..end].as_chunks_mut::<4>().0.iter_mut().zip(layer[start..end].as_chunks::<4>().0)
			{
				let a = s[3] as u32;
				if a == 0 {
					continue;
				}
				for k in 0..4 {
					d[k] = over(s[k] as u32, d[k], a);
				}
			}
		}
	}
}

/// Coverage values (0..1) over a rectangle of pixels; shapes are unioned into it by their maximum, so overlapping
/// parts of one item are blended once.
#[derive(Clone, Debug)]
pub struct Mask {
	rect: IRect,
	coverage: Vec<f32>,
}

impl Mask {
	pub fn new(rect: IRect) -> Self {
		let rect = IRect { width: rect.width.max(0), height: rect.height.max(0), ..rect };
		Self { rect, coverage: vec![0.0; rect.width as usize * rect.height as usize] }
	}

	pub fn bounds(&self) -> IRect {
		self.rect
	}

	/// The coverage of a pixel; 0 outside the mask.
	pub fn at(&self, x: i32, y: i32) -> f32 {
		let (mx, my) = (x - self.rect.x, y - self.rect.y);
		if mx < 0 || my < 0 || mx >= self.rect.width || my >= self.rect.height {
			return 0.0;
		}
		self.coverage[(my * self.rect.width + mx) as usize]
	}

	fn union(&mut self, x: i32, y: i32, c: f32) {
		let i = ((y - self.rect.y) * self.rect.width + x - self.rect.x) as usize;
		if c > self.coverage[i] {
			self.coverage[i] = c.min(1.0);
		}
	}

	/// Every pixel of the mask whose centre lies within the box `[x0, x1] x [y0, y1]` widened by one pixel, with
	/// its centre.
	fn for_each_near(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, mut f: impl FnMut(f32, f32) -> f32) {
		let r = IRect::new(
			(x0 - 1.0).floor() as i32,
			(y0 - 1.0).floor() as i32,
			(x1 - x0 + 3.0).ceil() as i32,
			(y1 - y0 + 3.0).ceil() as i32,
		)
		.intersect(&self.rect);
		for y in r.y..r.bottom() {
			for x in r.x..r.right() {
				let c = f(x as f32 + 0.5, y as f32 + 0.5);
				if c > 0.0 {
					self.union(x, y, c);
				}
			}
		}
	}

	/// A filled disc of radius `r` about `(cx, cy)`.
	pub fn disc(&mut self, cx: f32, cy: f32, r: f32) {
		self.for_each_near(cx - r, cy - r, cx + r, cy + r, |x, y| (r - (x - cx).hypot(y - cy) + 0.5).clamp(0.0, 1.0));
	}

	/// A circle of radius `r` stroked `width` wide.
	pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32) {
		let o = r + width / 2.0;
		self.for_each_near(cx - o, cy - o, cx + o, cy + o, |x, y| {
			(width / 2.0 - ((x - cx).hypot(y - cy) - r).abs() + 0.5).clamp(0.0, 1.0)
		});
	}

	/// A segment stroked `width` wide with round caps.
	pub fn segment(&mut self, a: (f32, f32), b: (f32, f32), width: f32) {
		let h = width / 2.0;
		let (dx, dy) = (b.0 - a.0, b.1 - a.1);
		let len2 = dx * dx + dy * dy;
		self.for_each_near(a.0.min(b.0) - h, a.1.min(b.1) - h, a.0.max(b.0) + h, a.1.max(b.1) + h, |x, y| {
			let t = if len2 > 0.0 { (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
			let d = (x - a.0 - t * dx).hypot(y - a.1 - t * dy);
			(h - d + 0.5).clamp(0.0, 1.0)
		});
	}

	/// A polyline stroked `width` wide, round joins and caps.
	pub fn polyline(&mut self, points: &[(f32, f32)], width: f32) {
		match points {
			[] => {}
			[p] => self.segment(*p, *p, width),
			_ => points.windows(2).for_each(|w| self.segment(w[0], w[1], width)),
		}
	}

	/// The mask grown by `radius`: each pixel the largest coverage within reach, weighted by a one-pixel ramp at
	/// the reach's edge. A text outline drawn with a pen `2 * radius` wide.
	pub fn dilate(&self, radius: f32) -> Mask {
		let reach = (radius + 0.5).ceil() as i32;
		let mut out = Mask::new(IRect::new(
			self.rect.x - reach,
			self.rect.y - reach,
			self.rect.width + 2 * reach,
			self.rect.height + 2 * reach,
		));
		let offsets: Vec<(i32, i32, f32)> = (-reach..=reach)
			.flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
			.map(|(dx, dy)| (dx, dy, (radius + 0.5 - (dx as f32).hypot(dy as f32)).clamp(0.0, 1.0)))
			.filter(|o| o.2 > 0.0)
			.collect();
		for y in self.rect.y..self.rect.bottom() {
			for x in self.rect.x..self.rect.right() {
				let c = self.at(x, y);
				if c <= 0.0 {
					continue;
				}
				for &(dx, dy, w) in &offsets {
					out.union(x + dx, y + dy, c * w);
				}
			}
		}
		out
	}
}

/// The embedded Noto Sans Regular (SIL Open Font License) with text measuring and masks.
pub struct Typeface {
	font: FontRef<'static>,
}

static TYPEFACE: OnceLock<Typeface> = OnceLock::new();

/// The process-wide embedded typeface.
pub fn typeface() -> &'static Typeface {
	TYPEFACE.get_or_init(|| Typeface {
		font: FontRef::try_from_slice(include_bytes!("../assets/NotoSans-Regular.ttf")).expect("the embedded font"),
	})
}

/// Horizontal offset of the second pass of synthetic bold, in font sizes.
const BOLD_OFFSET: f32 = 0.05;

impl Typeface {
	/// The glyph scale of an em size in pixels.
	fn scale(&self, size: f32) -> PxScale {
		let em = self.font.units_per_em().expect("the embedded font has a head table");
		PxScale::from(size * self.font.height_unscaled() / em)
	}

	/// Distance from the line's top to its baseline, pixels.
	pub fn ascent(&self, size: f32) -> f32 {
		self.font.as_scaled(self.scale(size)).ascent()
	}

	/// Height of one line of text at an em size of `size` pixels: ascent, descent and line gap.
	pub fn line_height(&self, size: f32) -> f32 {
		let f = self.font.as_scaled(self.scale(size));
		f.ascent() - f.descent() + f.line_gap()
	}

	fn glyphs(&self, text: &str, size: f32, x: f32, baseline: f32) -> (Vec<Glyph>, f32) {
		let scaled = self.font.as_scaled(self.scale(size));
		let mut caret = x;
		let mut previous: Option<GlyphId> = None;
		let mut glyphs = Vec::with_capacity(text.len());
		for c in text.chars() {
			let id = scaled.glyph_id(c);
			if let Some(p) = previous {
				caret += scaled.kern(p, id);
			}
			glyphs.push(id.with_scale_and_position(scaled.scale(), point(caret, baseline)));
			caret += scaled.h_advance(id);
			previous = Some(id);
		}
		(glyphs, caret - x)
	}

	/// Advance width of one line of text, pixels; synthetic bold adds its offset.
	pub fn measure(&self, text: &str, size: f32, bold: bool) -> f32 {
		self.glyphs(text, size, 0.0, 0.0).1 + if bold { size * BOLD_OFFSET } else { 0.0 }
	}

	/// The text cut to `max_width` with an ellipsis, or whole when it fits.
	pub fn fit(&self, text: &str, size: f32, max_width: f32) -> String {
		if self.measure(text, size, false) <= max_width {
			return text.to_owned();
		}
		let mut chars: Vec<char> = text.chars().collect();
		while !chars.is_empty() {
			chars.pop();
			let candidate: String = chars.iter().collect::<String>() + "\u{2026}";
			if self.measure(&candidate, size, false) <= max_width {
				return candidate;
			}
		}
		String::new()
	}

	/// The coverage of one line of text whose line box has its top-left at `(x, y)`, clipped to `clip`.
	pub fn mask(&self, text: &str, size: f32, x: f32, y: f32, bold: bool, clip: IRect) -> Mask {
		let baseline = y + self.ascent(size);
		let width = self.measure(text, size, bold);
		let rect = IRect::new(
			x.floor() as i32 - 2,
			y.floor() as i32 - 2,
			width.ceil() as i32 + 5,
			self.line_height(size).ceil() as i32 + 5,
		)
		.intersect(&clip);
		let mut mask = Mask::new(rect);
		let passes: &[f32] = if bold { &[0.0, size * BOLD_OFFSET] } else { &[0.0] };
		for &dx in passes {
			for glyph in self.glyphs(text, size, x + dx, baseline).0 {
				let Some(outline) = self.font.outline_glyph(glyph) else { continue };
				let b = outline.px_bounds();
				let (ox, oy) = (b.min.x as i32, b.min.y as i32);
				outline.draw(|gx, gy, c| {
					let (px, py) = (ox + gx as i32, oy + gy as i32);
					if px >= rect.x && py >= rect.y && px < rect.right() && py < rect.bottom() {
						mask.union(px, py, c);
					}
				});
			}
		}
		mask
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn premultiplied_over() {
		let mut px = vec![100, 100, 100, 255];
		let mut c = Canvas::new(&mut px, 1, 1);
		c.blend(0, 0, argb(128, 255, 0, 0), 1.0);
		assert_eq!(px, vec![128 + 50, 50, 50, 255]);
	}

	#[test]
	fn disc_coverage_is_symmetric_and_full_inside() {
		let mut m = Mask::new(IRect::new(0, 0, 11, 11));
		m.disc(5.5, 5.5, 3.0);
		assert_eq!(m.at(5, 5), 1.0);
		assert_eq!(m.at(0, 0), 0.0);
		assert_eq!(m.at(2, 5), m.at(8, 5));
		assert!(m.at(2, 5) > 0.0 && m.at(2, 5) < 1.0);
	}

	#[test]
	fn text_has_ink_and_width() {
		let t = typeface();
		assert!(t.measure("3B", 12.0, true) > t.measure("3B", 12.0, false));
		let m = t.mask("3B", 12.0, 0.0, 0.0, false, IRect::new(0, 0, 100, 100));
		let ink: f32 = (0..30).flat_map(|y| (0..30).map(move |x| (x, y))).map(|(x, y)| m.at(x, y)).sum();
		assert!(ink > 10.0);
		assert_eq!(t.fit("abcdef", 11.0, 1000.0), "abcdef");
		assert!(t.fit("abcdefghijkl", 11.0, 30.0).ends_with('\u{2026}'));
	}
}
