//! The CPU rasteriser of the burned-in overlays: epaint shapes tessellated as the window's are, their triangles
//! blended over a frame's RGBA8 pixels in place.
//!
//! One pixel per point. A pixel is covered when its centre lies in a triangle, edges in 1/256 pixel fixed point with
//! a top-left rule, so a pixel on an edge two triangles share is blended once; inside the clip rect rounded to whole
//! pixels, as egui-wgpu's scissor is. Colours are premultiplied sRGB, interpolated and multiplied by the font atlas,
//! sampled bilinearly with clamped edges, in gamma space as the window's renderer does, then blended:
//! `out = min(255, src + (dst*(255 - a) + 127)/255)`. Integers and IEEE f32 without fused operations only, so every
//! machine draws the same bytes. Pixels no triangle covers are not touched. Bands of rows are drawn in parallel,
//! each with the triangles reaching it in drawing order.

use std::cmp::Ordering;

use rayon::prelude::*;
use wavedude_view::draw::Outlines;
use wavedude_view::epaint::text::{Fonts, FontsView, TextOptions, ViewportKey};
use wavedude_view::epaint::{
	ClippedPrimitive, ClippedShape, Color32, ImageData, ImageDelta, Pos2, Primitive, Rect, TessellationOptions,
	Tessellator, Vertex, WHITE_UV,
};

use crate::{ExportError, Result};

/// Fixed-point steps of a pixel.
const ONE: i64 = 256;

/// Rows of a band drawn by one thread.
const BAND: usize = 16;

/// The font atlas as the shapes see it: premultiplied texels, row by row.
#[derive(Default)]
struct Atlas {
	texels: Vec<Color32>,
	size: [usize; 2],
}

impl Atlas {
	/// Takes an update: the whole image, then true, or a patch of it.
	fn apply(&mut self, delta: &ImageDelta) -> bool {
		let ImageData::Color(image) = &delta.image;
		let [w, h] = image.size;
		match delta.pos {
			None => {
				self.texels.clone_from(&image.pixels);
				self.size = image.size;
				true
			}
			Some([x, y]) => {
				for (row, src) in image.pixels.chunks_exact(w).take(h).enumerate() {
					let at = (y + row) * self.size[0] + x;
					self.texels[at..at + w].copy_from_slice(src);
				}
				false
			}
		}
	}

	/// The texel at texture coordinates `(u, v)`, bilinear, clamped to the edges; premultiplied, 0 to 255.
	fn sample(&self, u: f32, v: f32) -> [f32; 4] {
		let [w, h] = self.size;
		let (fx, fy) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
		let (x0, y0) = (fx.floor(), fy.floor());
		let (tx, ty) = (fx - x0, fy - y0);
		let at = |x: f32, y: f32| {
			let (x, y) = ((x.max(0.) as usize).min(w - 1), (y.max(0.) as usize).min(h - 1));
			self.texels[y * w + x].to_array().map(f32::from)
		};
		let (a, b, c, d) = (at(x0, y0), at(x0 + 1., y0), at(x0, y0 + 1.), at(x0 + 1., y0 + 1.));
		let lerp = |p: f32, q: f32, t: f32| p + (q - p) * t;
		std::array::from_fn(|i| lerp(lerp(a[i], b[i], tx), lerp(c[i], d[i], tx), ty))
	}
}

/// A triangle to fill: its vertices, its corners in fixed point, clockwise on screen (y down), and the pixels inside
/// its clip that its bounds reach, `x0..x1` by `y0..y1`.
struct Triangle {
	v: [Vertex; 3],
	p: [(i64, i64); 3],
	area: i64,
	x0: i64,
	x1: i64,
	y0: i64,
	y1: i64,
}

impl Triangle {
	/// `None` when it is degenerate or covers nothing of `[x0, y0, x1, y1]`.
	fn new(v: [Vertex; 3], [cx0, cy0, cx1, cy1]: [u32; 4]) -> Option<Self> {
		let fixed = |p: Pos2| ((p.x * ONE as f32).round() as i64, (p.y * ONE as f32).round() as i64);
		let (mut v, mut p) = (v, v.map(|v| fixed(v.pos)));
		let mut area = edge(p[0], p[1], p[2]);
		if area < 0 {
			p.swap(1, 2);
			v.swap(1, 2);
			area = -area;
		}
		// the pixels `k` whose centre `k + 1/2` can lie between the corners, inside the clip
		let span = |c: [i64; 3], c0: u32, c1: u32| {
			let (lo, hi) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
			let first = (lo - ONE / 2).div_euclid(ONE).max(i64::from(c0));
			(first, ((hi - ONE / 2).div_euclid(ONE) + 1).min(i64::from(c1)))
		};
		let (x0, x1) = span(p.map(|q| q.0), cx0, cx1);
		let (y0, y1) = span(p.map(|q| q.1), cy0, cy1);
		(area > 0 && x0 < x1 && y0 < y1).then_some(Self { v, p, area, x0, x1, y0, y1 })
	}

	/// Blends the triangle's pixels of rows `first..first + rows.len() / stride` into `rows`.
	fn fill(&self, atlas: &Atlas, rows: &mut [u8], stride: usize, first: i64) {
		let v = &self.v;
		let edges = [(self.p[1], self.p[2]), (self.p[2], self.p[0]), (self.p[0], self.p[1])];
		let white = v.iter().all(|v| v.uv == WHITE_UV);
		let flat = white && v.iter().all(|u| u.color == v[0].color);
		let colours = v.map(|v| v.color.to_array().map(f32::from));
		let area = self.area as f32;
		let last = first + (rows.len() / stride) as i64;
		for y in self.y0.max(first)..self.y1.min(last) {
			let py = y * ONE + ONE / 2;
			let (mut from, mut to) = (self.x0, self.x1);
			for &(a, b) in &edges {
				// edge(a, b, centre of pixel x) = m*x + q; a centre on the edge counts where the edge is owned
				let m = -(b.1 - a.1) * ONE;
				let q = edge(a, b, (ONE / 2, py)) - i64::from(!owns(a, b));
				match m.cmp(&0) {
					Ordering::Greater => from = from.max(-q.div_euclid(m)),
					Ordering::Less => to = to.min(q.div_euclid(-m) + 1),
					Ordering::Equal if q < 0 => to = from,
					Ordering::Equal => {}
				}
			}
			let row = &mut rows[(y - first) as usize * stride..][..stride];
			for x in from..to {
				let src = if flat {
					v[0].color.to_array()
				} else {
					let px = x * ONE + ONE / 2;
					let l = edges.map(|(a, b)| edge(a, b, (px, py)) as f32 / area);
					let texel = if white {
						[255.; 4]
					} else {
						let uv = |c: fn(&Pos2) -> f32| l[0] * c(&v[0].uv) + l[1] * c(&v[1].uv) + l[2] * c(&v[2].uv);
						atlas.sample(uv(|q| q.x), uv(|q| q.y))
					};
					std::array::from_fn(|c| {
						let colour = l[0] * colours[0][c] + l[1] * colours[1][c] + l[2] * colours[2][c];
						(colour * texel[c] / 255. + 0.5).clamp(0., 255.) as u8
					})
				};
				blend(&mut row[x as usize * 4..][..4], src);
			}
		}
	}
}

/// Triangles in drawing order and, per band of rows, the indices of those reaching it.
struct Banded {
	triangles: Vec<Triangle>,
	bands: Vec<Vec<u32>>,
}

impl Banded {
	/// Empty, for a frame of `height` rows.
	fn new(height: u32) -> Self {
		Self { triangles: Vec::new(), bands: vec![Vec::new(); (height as usize).div_ceil(BAND)] }
	}

	fn clear(&mut self) {
		self.triangles.clear();
		self.bands.iter_mut().for_each(Vec::clear);
	}

	fn push(&mut self, t: Triangle) {
		let i = self.triangles.len() as u32;
		for band in &mut self.bands[t.y0 as usize / BAND..=(t.y1 as usize - 1) / BAND] {
			band.push(i);
		}
		self.triangles.push(t);
	}

	/// The triangles reaching band `i`.
	fn band(&self, i: usize) -> impl Iterator<Item = &Triangle> {
		self.bands[i].iter().map(|&t| &self.triangles[t as usize])
	}
}

/// Shapes for the fonts and their outlines.
type Draw<'a> = dyn FnMut(&mut FontsView<'_>, &mut Outlines) -> Vec<ClippedShape> + 'a;

/// The bundled fonts and their outlines, the font atlas as the shapes see it, a frame size, and fixed shapes drawn
/// under every frame's, tessellated once.
pub struct Rasteriser {
	fonts: Fonts,
	outlines: Outlines,
	atlas: Atlas,
	width: u32,
	height: u32,
	fixed: Option<Box<Draw<'static>>>,
	/// The fixed shapes' triangles; `None` until laid out, and again once the atlas is rebuilt.
	fixed_triangles: Option<Banded>,
	/// A frame's triangles, kept for their allocation.
	triangles: Banded,
}

impl Rasteriser {
	/// A rasteriser of `width x height` frames.
	pub fn new(width: u32, height: u32) -> Self {
		Self {
			fonts: Fonts::new(TextOptions::default(), wavedude_view::fonts::definitions()),
			outlines: Outlines::new(1.),
			atlas: Atlas::default(),
			width,
			height,
			fixed: None,
			fixed_triangles: None,
			triangles: Banded::new(height),
		}
	}

	/// Draws the shapes `draw` makes under every frame's; it is called again when the font atlas is rebuilt.
	pub fn with_fixed(
		mut self,
		draw: impl FnMut(&mut FontsView<'_>, &mut Outlines) -> Vec<ClippedShape> + 'static,
	) -> Self {
		self.fixed = Some(Box::new(draw));
		self
	}

	/// Draws the fixed shapes, then the shapes `draw` makes with the fonts and their outlines (at one pixel per
	/// point), over the RGBA8 frame `rgba` of the rasteriser's size, in place.
	pub fn render(
		&mut self,
		rgba: &mut [u8],
		draw: impl FnOnce(&mut FontsView<'_>, &mut Outlines) -> Vec<ClippedShape>,
	) -> Result<()> {
		let (w, h) = (self.width, self.height);
		if rgba.len() != w as usize * h as usize * 4 {
			return Err(ExportError::WrongInput("RGBA8 pixels of the rasteriser's size"));
		}
		self.fonts.begin_pass(*self.fonts.options(), ViewportKey::default());
		let (shapes, rebuilt) = lay_out(&mut self.fonts, &mut self.outlines, &mut self.atlas, draw);
		if rebuilt {
			self.fixed_triangles = None;
		}
		if self.fixed_triangles.is_none()
			&& let Some(fixed) = self.fixed.as_mut()
		{
			let (fixed, _) = lay_out(&mut self.fonts, &mut self.outlines, &mut self.atlas, |f, o| fixed(f, o));
			let mut triangles = Banded::new(h);
			self.tessellate(fixed, &mut triangles);
			self.fixed_triangles = Some(triangles);
		}
		let mut triangles = std::mem::replace(&mut self.triangles, Banded::new(0));
		triangles.clear();
		self.tessellate(shapes, &mut triangles);
		let (stride, atlas) = (w as usize * 4, &self.atlas);
		let fixed = self.fixed_triangles.as_ref();
		rgba.par_chunks_mut(stride * BAND).enumerate().for_each(|(i, rows)| {
			for t in fixed.into_iter().flat_map(|f| f.band(i)).chain(triangles.band(i)) {
				t.fill(atlas, rows, stride, (i * BAND) as i64);
			}
		});
		self.triangles = triangles;
		Ok(())
	}

	/// Appends the triangles of `shapes` inside the frame to `out`, in drawing order.
	fn tessellate(&self, shapes: Vec<ClippedShape>, out: &mut Banded) {
		let (w, h) = (self.width, self.height);
		let mut tessellator = Tessellator::new(
			1.,
			TessellationOptions::default(),
			self.fonts.font_image_size(),
			self.fonts.texture_atlas().prepared_discs(),
		);
		for ClippedPrimitive { clip_rect, primitive } in tessellator.tessellate_shapes(shapes) {
			let Primitive::Mesh(mesh) = primitive else { continue };
			let clip = scissor(clip_rect, w, h);
			for t in mesh.indices.as_chunks::<3>().0 {
				if let Some(t) = Triangle::new(t.map(|i| mesh.vertices[i as usize]), clip) {
					out.push(t);
				}
			}
		}
	}
}

/// `draw`'s shapes and the atlas updated for their glyphs; true when it was rebuilt, which moves the glyphs of shapes
/// laid out before.
fn lay_out(
	fonts: &mut Fonts,
	outlines: &mut Outlines,
	atlas: &mut Atlas,
	draw: impl FnOnce(&mut FontsView<'_>, &mut Outlines) -> Vec<ClippedShape>,
) -> (Vec<ClippedShape>, bool) {
	let shapes = draw(&mut fonts.with_pixels_per_point(1.), outlines);
	let rebuilt = fonts.font_image_delta().is_some_and(|delta| atlas.apply(&delta));
	(shapes, rebuilt)
}

/// `out = min(255, src + (dst*(255 - a) + 127)/255)` per channel.
fn blend(dst: &mut [u8], src: [u8; 4]) {
	let keep = 255 - u32::from(src[3]);
	for (d, s) in dst.iter_mut().zip(src) {
		*d = (u32::from(s) + (u32::from(*d) * keep + 127) / 255).min(255) as u8;
	}
}

/// Twice the signed area of `(a, b, p)`: positive with `p` right of `a -> b` on screen (y down).
fn edge(a: (i64, i64), b: (i64, i64), p: (i64, i64)) -> i64 {
	(b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// Whether the pixel centres on the edge `a -> b` belong to its triangle; of an edge's two directions exactly one
/// does, so the two triangles sharing it never both blend those pixels.
fn owns(a: (i64, i64), b: (i64, i64)) -> bool {
	let (dx, dy) = (b.0 - a.0, b.1 - a.1);
	dy < 0 || (dy == 0 && dx > 0)
}

/// `clip` in whole pixels of a `width x height` frame, rounded as egui-wgpu's scissor: `[x0, y0, x1, y1]`.
fn scissor(clip: Rect, width: u32, height: u32) -> [u32; 4] {
	let round = |v: f32, size: u32| (v.round().max(0.) as u32).min(size);
	let (x0, y0) = (round(clip.min.x, width), round(clip.min.y, height));
	[x0, y0, round(clip.max.x, width).max(x0), round(clip.max.y, height).max(y0)]
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_view::epaint::{Mesh, Shape, pos2, vec2};

	const W: u32 = 16;
	const H: u32 = 8;

	fn grey() -> Vec<u8> {
		[100, 100, 100, 255].repeat((W * H) as usize)
	}

	fn everything() -> Rect {
		Rect::from_min_size(pos2(0., 0.), vec2(W as f32, H as f32))
	}

	fn at(px: &[u8], x: u32, y: u32) -> [u8; 4] {
		let i = ((y * W + x) * 4) as usize;
		[px[i], px[i + 1], px[i + 2], px[i + 3]]
	}

	#[test]
	fn a_rectangle_on_whole_pixels_covers_exactly_its_pixels_once() {
		let mut r = Rasteriser::new(W, H);
		let mut px = grey();
		let rect = Rect::from_min_max(pos2(2., 1.), pos2(6., 4.));
		let half = Color32::from_rgba_unmultiplied(255, 255, 255, 128);
		let mut mesh = Mesh::default();
		mesh.add_colored_rect(rect, half);
		r.render(&mut px, |_, _| vec![ClippedShape::new(everything(), Shape::mesh(mesh))]).unwrap();
		let blended = (128 + (100 * 127 + 127) / 255) as u8;
		for y in 0..H {
			for x in 0..W {
				let inside = (2..6).contains(&x) && (1..4).contains(&y);
				let want = if inside { [blended, blended, blended, 255] } else { [100, 100, 100, 255] };
				assert_eq!(at(&px, x, y), want, "({x}, {y}): two triangles, the diagonal blended once");
			}
		}
	}

	#[test]
	fn the_clip_rect_rounds_as_the_window_s_scissor() {
		assert_eq!(scissor(Rect::from_min_max(pos2(1.4, -3.), pos2(5.5, 20.)), W, H), [1, 0, 6, H]);
		assert_eq!(scissor(Rect::from_min_max(pos2(20., 2.), pos2(30., 3.)), W, H), [W, 2, W, 3]);
		let mut r = Rasteriser::new(W, H);
		let mut px = grey();
		let clip = Rect::from_min_max(pos2(3.4, 0.), pos2(5.6, H as f32));
		let shape = Shape::rect_filled(everything(), 0, Color32::WHITE);
		r.render(&mut px, |_, _| vec![ClippedShape::new(clip, shape)]).unwrap();
		let white: Vec<u32> = (0..W).filter(|&x| at(&px, x, 3) == [255; 4]).collect();
		assert_eq!(white, [3, 4, 5]);
	}

	#[test]
	fn a_pixel_centre_on_an_edge_two_triangles_share_belongs_to_one() {
		let (a, b, c, d) = ((0, 0), (256, 0), (256, 256), (0, 256));
		assert_ne!(owns(a, c), owns(c, a));
		assert_ne!(owns(a, b), owns(b, a));
		assert_ne!(owns(b, c), owns(c, b));
		assert_ne!(owns(d, a), owns(a, d));
	}
}
