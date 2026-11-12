//! Paints optical parts into scene pixels (spec 20 "Per-part painting").
//!
//! Red takes a refractive part's index (`clamp(round_even(n * 100), 1, 255)`) or 0 for a mirror,
//! blue an absorbing part's absorption (the larger of old and new), green is never touched. A
//! refractive part takes over a mirror pixel only where it covers at least half of it (its centre
//! inside), the mirror's own rule; a pixel's previous index is read as the engine does, at least 1.
//! Pixel (x, y) spans [x, x+1) x [y, y+1); its centre is (x + 0.5, y + 0.5) in canvas pixels.
//! Pixels are painted one by one (no settled runs), rows in parallel.
//!
//! A pixel an edge crosses takes the effective medium of the scalar wave equation, the area-weighted
//! permittivity `n_eff = sqrt(sum(share * n^2))` (Farjadpour et al. 2006, decision 0010): for kinds
//! whose depth is a signed distance the shares are the areas a straight edge at the centre's distance
//! cuts from the pixel ([`coverage`]), otherwise the 16 point samples.

use rayon::prelude::*;

use super::pixels::ScenePixels;
use super::shapes::{Frame, PartShape, Vec2, shapes_for};
use crate::model::{CoatingProfile, Part, Simulation};

/// How far a sample can lie from its pixel's centre, with a margin.
pub const SAMPLE_REACH: f64 = 0.6;
/// Samples per pixel edge for refractive parts of kinds without a signed distance.
pub const SUPERSAMPLE: usize = 4;
/// How far a straight edge can lie from a pixel's centre and still cut the pixel (sqrt(1/2)), with a margin.
pub const EDGE_REACH: f64 = 0.71;
/// Step of the forward differences that give an edge's normal, in pixels.
const NORMAL_STEP: f64 = 1e-3;

/// Paints the enabled, normalised `part` into `pixels` (if it has a shape), a part of several pieces piece by
/// piece in their order: each piece's absorption first, then its index. `seed` is the project's, for a kind
/// that draws on it.
pub fn paint_part(pixels: &mut ScenePixels, part: &Part, sim: &Simulation, seed: i32) {
	debug_assert!(part.base.enabled, "disabled parts are filtered by the caller");
	let shapes = shapes_for(part, sim, seed);
	if shapes.is_empty() {
		tracing::debug!(name = %part.base.name, "part has no shape to paint");
	}
	for shape in shapes {
		paint_shape(pixels, part, shape.as_ref());
	}
}

fn paint_shape(pixels: &mut ScenePixels, part: &Part, shape: &dyn PartShape) {
	let frame = Frame::of(&part.base);
	let Some(bounds) = Bounds::of(shape.extent(), frame.centre(), pixels.width, pixels.height) else {
		tracing::debug!(name = %part.base.name, "part lies outside the canvas");
		return;
	};
	let started = web_time::Instant::now();
	let painter = Painter { shape, frame, bounds, stride: pixels.stride() };
	let rows = &mut pixels.rgba[bounds.y0 * painter.stride..(bounds.y1 + 1) * painter.stride];
	if shape.absorption() > 0 {
		painter.absorption(rows);
	}
	if shape.paints_index() {
		if shape.is_mirror() {
			painter.mirror(rows);
		} else {
			painter.index(rows);
		}
	}
	tracing::debug!(
		name = %part.base.name, x = part.base.x, y = part.base.y, angle = part.base.angle,
		rows = ?(bounds.y0, bounds.y1), ms = started.elapsed().as_secs_f64() * 1e3, "painted part"
	);
}

/// The inclusive pixel rectangle a shape can touch: `centre +- (extent + 1)`, floor and ceil, clipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
	pub x0: usize,
	pub y0: usize,
	pub x1: usize,
	pub y1: usize,
}

impl Bounds {
	/// None when the rectangle misses the canvas.
	pub fn of(extent: f64, centre: Vec2, width: u32, height: u32) -> Option<Self> {
		let e = extent + 1.0;
		let x0 = (centre.x - e).floor().max(0.0);
		let y0 = (centre.y - e).floor().max(0.0);
		let x1 = (centre.x + e).ceil().min(width as f64 - 1.0);
		let y1 = (centre.y + e).ceil().min(height as f64 - 1.0);
		(x0 <= x1 && y0 <= y1).then_some(Self { x0: x0 as usize, y0: y0 as usize, x1: x1 as usize, y1: y1 as usize })
	}
}

/// A red level from an index: `clamp(round_even(n * 100), 1, 255)`.
fn red_of(index: f64) -> u8 {
	(index * 100.0).round_ties_even().clamp(1.0, 255.0) as u8
}

/// Share of a unit pixel on the inner side of a straight edge `e` px from its centre (> 0: the centre
/// inside), the edge's unit normal `(nx, ny)` in canvas axes. Exact for a straight edge: `clamp(0.5 + e, 0, 1)`
/// for an axis-aligned one, quadratic where the edge cuts a corner off. A curved edge of radius R
/// differs from its tangent by the segment between them, at most about `0.24 / R` of the pixel
/// (`1 / (12 R)` axis-aligned); a corner of the outline within the pixel is not accounted for.
pub fn coverage(e: f64, nx: f64, ny: f64) -> f64 {
	let (a, b) = if nx.abs() >= ny.abs() { (nx.abs(), ny.abs()) } else { (ny.abs(), nx.abs()) };
	let half = (a + b) / 2.0;
	if e >= half {
		return 1.0;
	}
	if e <= -half {
		return 0.0;
	}
	if e.abs() <= (a - b) / 2.0 {
		return 0.5 + e / a;
	}
	let corner = (half - e.abs()).powi(2) / (2.0 * a * b);
	if e > 0.0 { 1.0 - corner } else { corner }
}

struct Painter<'a> {
	shape: &'a dyn PartShape,
	frame: Frame,
	bounds: Bounds,
	stride: usize,
}

impl Painter<'_> {
	fn at(&self, x: f64, y: f64) -> Vec2 {
		self.frame.to_frame(Vec2::new(x, y))
	}

	/// The index at a part-frame point with the coating band applied, `outside` the pixel's previous index.
	fn index_at(&self, p: Vec2, outside: f64) -> Option<f64> {
		let (depth, body) = self.shape.sample(p);
		if depth < 0.0 {
			return None;
		}
		let band = self.shape.band();
		if band.width <= 0.0 || depth >= band.width || !(band.all_faces() || band.coats(self.shape.face_at(p))) {
			return Some(body);
		}
		Some(band.index(depth, body, outside))
	}

	/// Centre test only: red 0 where the centre's depth is >= 0.
	fn mirror(&self, rows: &mut [u8]) {
		let b = self.bounds;
		rows.par_chunks_mut(self.stride).enumerate().for_each(|(row, line)| {
			let cy = (b.y0 + row) as f64 + 0.5;
			for x in b.x0..=b.x1 {
				if self.shape.depth(self.at(x as f64 + 0.5, cy)) >= 0.0 {
					line[x * 4] = 0;
				}
			}
		});
	}

	/// The centre's index where no edge is within reach, else the effective medium of the pixel.
	fn index(&self, rows: &mut [u8]) {
		let b = self.bounds;
		let band = self.shape.band().width;
		let distance = self.shape.depth_is_distance();
		let reach = if distance { EDGE_REACH } else { SAMPLE_REACH };
		rows.par_chunks_mut(self.stride).enumerate().for_each(|(row, line)| {
			let y = (b.y0 + row) as f64;
			for x in b.x0..=b.x1 {
				let (fx, red) = (x as f64, &mut line[x * 4]);
				let centre = self.at(fx + 0.5, y + 0.5);
				let (depth, inner) = self.shape.depths(centre);
				if depth < -reach || (*red == 0 && depth < 0.0) {
					continue;
				}
				let existing = (*red as f64 / 100.0).max(1.0);
				let value = if depth > reach && inner > reach && (band == 0.0 || (depth - band).abs() > reach) {
					self.index_at(centre, existing)
				} else if distance {
					self.smoothed(fx + 0.5, y + 0.5, depth, inner, existing)
				} else {
					self.supersampled(fx, y, existing)
				};
				if let Some(n) = value {
					*red = red_of(n);
				}
			}
		});
	}

	/// The mean permittivity of 16 point samples, `existing` where a sample lies outside; None with none inside.
	fn supersampled(&self, x: f64, y: f64, existing: f64) -> Option<f64> {
		let step = 1.0 / SUPERSAMPLE as f64;
		let mut sum = 0.0;
		let mut inside = false;
		for sy in 0..SUPERSAMPLE {
			for sx in 0..SUPERSAMPLE {
				let p = self.at(x + step / 2.0 + sx as f64 * step, y + step / 2.0 + sy as f64 * step);
				let n = match self.index_at(p, existing) {
					Some(n) => {
						inside = true;
						n
					}
					None => existing,
				};
				sum += n * n;
			}
		}
		inside.then(|| (sum / (SUPERSAMPLE * SUPERSAMPLE) as f64).sqrt())
	}

	/// The unit normal of `field`'s level line through the canvas point (cx, cy), `value` its value there.
	fn normal(&self, cx: f64, cy: f64, value: f64, field: impl Fn(Vec2) -> f64) -> (f64, f64) {
		let gx = field(self.at(cx + NORMAL_STEP, cy)) - value;
		let gy = field(self.at(cx, cy + NORMAL_STEP)) - value;
		let length = (gx * gx + gy * gy).sqrt();
		if length > 0.0 && length.is_finite() { (gx / length, gy / length) } else { (1.0, 0.0) }
	}

	/// The effective medium of the pixel centred at canvas (cx, cy), `depth` the centre's signed distance to
	/// the outline, `inner` to an inner edge: the outline, the inner edge and a layer coating's inner edge
	/// cut the pixel as straight edges; a graded band takes its index at the centre. None when nothing
	/// of the part covers the pixel.
	fn smoothed(&self, cx: f64, cy: f64, depth: f64, inner: f64, existing: f64) -> Option<f64> {
		let centre = self.at(cx, cy);
		let band = self.shape.band();
		let layer = matches!(band.profile, CoatingProfile::Layer);
		let cut = |e: f64| e.abs() < EDGE_REACH;
		let (nx, ny) = if cut(depth) || (layer && cut(depth - band.width)) {
			self.normal(cx, cy, depth, |p| self.shape.depth(p))
		} else {
			(1.0, 0.0)
		};
		let f = coverage(depth, nx, ny);
		if f <= 0.0 {
			return None;
		}
		let coated = band.width > 0.0
			&& depth < band.width + EDGE_REACH
			&& (band.all_faces() || band.coats(self.shape.face_at(centre)));
		let deep = if coated && layer { coverage(depth - band.width, nx, ny) } else { f };
		// permittivity of the part's share of the pixel over a body of index `body`
		let part = |body: f64| {
			if !coated {
				f * body * body
			} else if layer {
				let n = band.index(0.0, body, existing);
				(f - deep) * n * n + deep * body * body
			} else {
				let n = band.index(depth.max(0.0), body, existing);
				f * n * n
			}
		};
		let near = self.shape.body_index_at(centre);
		let permittivity = if inner <= EDGE_REACH {
			let (d, far) = self.shape.inner_edge(centre);
			let (ix, iy) = if cut(d) { self.normal(cx, cy, d, |p| self.shape.inner_edge(p).0) } else { (1.0, 0.0) };
			let h = coverage(d, ix, iy);
			h * part(near) + (1.0 - h) * part(far)
		} else {
			part(near)
		};
		Some((permittivity + (1.0 - f) * existing * existing).sqrt())
	}

	/// Blue at pixel centres inside (depth stored as f32 >= 0): `max(old, round_even(A * share))`,
	/// the ramp shortened to the deepest centre.
	fn absorption(&self, rows: &mut [u8]) {
		let b = self.bounds;
		let columns = b.x1 - b.x0 + 1;
		let mut depths = vec![0f32; columns * (b.y1 - b.y0 + 1)];
		let deepest = depths
			.par_chunks_mut(columns)
			.enumerate()
			.map(|(row, line)| {
				let cy = (b.y0 + row) as f64 + 0.5;
				line.iter_mut().enumerate().fold(0.0f64, |deepest, (i, slot)| {
					let depth = self.shape.depth(self.at((b.x0 + i) as f64 + 0.5, cy));
					*slot = depth as f32;
					deepest.max(depth)
				})
			})
			.reduce(|| 0.0, f64::max);
		let ramp = self.shape.absorption_ramp().min(deepest);
		let full = self.shape.absorption() as f64;
		tracing::debug!(absorption = full, ramp, deepest, "absorption");
		rows.par_chunks_mut(self.stride).zip(depths.par_chunks(columns)).enumerate().for_each(
			|(row, (line, depths))| {
				let cy = (b.y0 + row) as f64 + 0.5;
				for (i, &depth) in depths.iter().enumerate() {
					if depth >= 0.0 {
						let x = b.x0 + i;
						let share = self.shape.absorption_share(self.at(x as f64 + 0.5, cy), depth, ramp);
						let blue = (full * share).round_ties_even().clamp(0.0, 255.0) as u8;
						line[x * 4 + 2] = line[x * 4 + 2].max(blue);
					}
				}
			},
		);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::normalize::Normalize;
	use crate::model::{PartKind, Triangle, TriangleType};

	/// A triangle of index 1.5 whose vertical base cuts `share` of pixel column 99.
	fn base_edge(share: f64) -> Part {
		let height = 2.0 * (10.0 + share);
		let angle = (height / 20.0).atan().to_degrees();
		let kind = Triangle { base: 40, kind: TriangleType::Isosceles, angle_a: angle, ..Triangle::default() };
		let mut part = Part::new(PartKind::Triangle(kind), 110, 60);
		part.normalize();
		part
	}

	/// Column 99 painted with `red`, then the triangle cutting `share` of it: the column's red at row 60.
	fn over(red: u8, share: f64) -> u8 {
		let mut p = ScenePixels::blank(200, 120, 1.0);
		for y in 0..120 {
			p.rgba[(y * 200 + 99) * 4] = red;
		}
		paint_part(&mut p, &base_edge(share), &Simulation::default(), 0);
		p.red(99, 60)
	}

	#[test]
	fn a_mirror_pixel_stays_unless_half_covered_and_low_reds_read_as_index_1() {
		let effective = |share: f64| ((share * 2.25 + (1.0 - share)).sqrt() * 100.0).round_ties_even() as u8;
		assert_eq!([over(0, 0.3), over(0, 0.45)], [0, 0], "under half: the mirror stays");
		assert_eq!([over(0, 0.5), over(0, 0.7)], [effective(0.5), effective(0.7)]);
		assert_eq!(over(50, 0.3), effective(0.3), "red 50 is index 1 to the engine");
	}
}
