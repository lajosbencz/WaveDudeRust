//! The 16-bit preview path: an f16 plane resampled in value space to the picture's size on screen, then coloured.

use half::f16;
use rayon::prelude::*;

use super::lut::{DeepHalfTable, HalfTable, unpack};
use crate::engine::types::SampleKind;

/// Weights of one axis from a window `[lo, hi)` of `source` samples to `target` pixels: a box mean when shrinking
/// (each sample weighs its overlap with the pixel's footprint), linear between the two samples nearest the pixel's
/// centre when enlarging (the ends held), a copy at the same size over the whole axis.
#[derive(Clone, Debug)]
pub struct ResampleAxis {
	source: usize,
	target: usize,
	first: Vec<usize>,
	offsets: Vec<usize>,
	weights: Vec<f32>,
	/// The samples any pixel weighs, `[start, end)`.
	used: (usize, usize),
}

impl ResampleAxis {
	/// The whole axis. Panics when either size is 0.
	pub fn new(source: usize, target: usize) -> Self {
		Self::window(source, (0.0, source as f64), target)
	}

	/// The window `[lo, hi)` of the axis, in samples, clamped to it. Panics when a size is 0 or the window empty.
	pub fn window(source: usize, (lo, hi): (f64, f64), target: usize) -> Self {
		assert!(source >= 1 && target >= 1, "resampling needs at least one sample and one pixel");
		let (lo, hi) = (lo.clamp(0.0, source as f64), hi.clamp(0.0, source as f64));
		assert!(hi > lo, "resampling needs a window of some width");
		let whole = lo == 0.0 && hi == source as f64;
		let mut first = Vec::with_capacity(target);
		let mut offsets = Vec::with_capacity(target + 1);
		let mut weights = Vec::with_capacity(target * 2);
		let scale = (hi - lo) / target as f64;
		for t in 0..target {
			offsets.push(weights.len());
			if whole && source == target {
				first.push(t);
				weights.push(1.0);
			} else if scale > 1.0 {
				let a = lo + t as f64 * scale;
				let b = lo + (t + 1) as f64 * scale;
				let start = (a.floor() as usize).min(source - 1);
				let last = (source - 1).min((b.ceil() as usize).max(1) - 1).max(start);
				first.push(start);
				weights.extend(
					(start..=last).map(|i| ((b.min(i as f64 + 1.0) - a.max(i as f64)).max(0.0) / scale) as f32),
				);
			} else {
				let u = lo + (t as f64 + 0.5) * scale - 0.5;
				if u <= 0.0 || source == 1 {
					first.push(0);
					weights.push(1.0);
				} else if u >= (source - 1) as f64 {
					first.push(source - 1);
					weights.push(1.0);
				} else {
					let i = u.floor();
					let f = (u - i) as f32;
					first.push(i as usize);
					weights.extend([1.0 - f, f]);
				}
			}
		}
		offsets.push(weights.len());
		let end = (0..target).map(|t| first[t] + offsets[t + 1] - offsets[t]).max().unwrap_or(0);
		let used = (first.iter().copied().min().unwrap_or(0), end);
		Self { source, target, first, offsets, weights, used }
	}

	pub fn source(&self) -> usize {
		self.source
	}

	pub fn target(&self) -> usize {
		self.target
	}

	/// The samples any pixel weighs, `[start, end)`.
	pub fn used(&self) -> (usize, usize) {
		self.used
	}

	/// The first sample of pixel `t` and the weights of it and the samples after it.
	pub fn weights_of(&self, t: usize) -> (usize, &[f32]) {
		(self.first[t], &self.weights[self.offsets[t]..self.offsets[t + 1]])
	}
}

/// Ordered dithering depth of the preview: the 16-bit colours brought to 8 bits through a Bayer matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DitherDepth {
	/// 2 x 2 matrix.
	Bits10,
	/// 4 x 4 matrix.
	Bits12,
	/// 8 x 8 matrix.
	Bits14,
	/// 16 x 16 matrix.
	Bits16,
}

impl DitherDepth {
	pub const ALL: [Self; 4] = [Self::Bits10, Self::Bits12, Self::Bits14, Self::Bits16];

	/// 10, 12, 14 or 16; anything else is none.
	pub fn from_bits(bits: u32) -> Option<Self> {
		Self::ALL.into_iter().find(|d| d.bits() == bits)
	}

	pub fn bits(self) -> u32 {
		match self {
			Self::Bits10 => 10,
			Self::Bits12 => 12,
			Self::Bits14 => 14,
			Self::Bits16 => 16,
		}
	}

	/// Side of the matrix: 2^((bits - 8) / 2).
	pub fn side(self) -> usize {
		1 << ((self.bits() - 8) / 2)
	}

	/// Row-major `[y * side + x]`: `(2 * Bayer[y][x] + 1) << (8 - e)`, e = bits - 8, compared against twice the
	/// low byte of a 16-bit channel.
	pub fn thresholds(self) -> Vec<u16> {
		let side = self.side();
		let mut matrix = vec![0u16];
		let mut n = 1;
		while n < side {
			let mut next = vec![0u16; 4 * n * n];
			for y in 0..n {
				for x in 0..n {
					let m = 4 * matrix[y * n + x];
					next[y * 2 * n + x] = m;
					next[y * 2 * n + x + n] = m + 2;
					next[(y + n) * 2 * n + x] = m + 3;
					next[(y + n) * 2 * n + x + n] = m + 1;
				}
			}
			matrix = next;
			n *= 2;
		}
		let shift = 8 - (self.bits() - 8);
		matrix.into_iter().map(|m| (2 * m + 1) << shift).collect()
	}
}

/// The colours the screen renderer looks a resampled value up in.
#[derive(Clone, Copy, Debug)]
pub enum ScreenColors<'a> {
	Plain(&'a HalfTable),
	Dithered(&'a DeepHalfTable, DitherDepth),
}

impl ScreenColors<'_> {
	fn kind(&self) -> SampleKind {
		match self {
			Self::Plain(t) => t.kind(),
			Self::Dithered(t, _) => t.kind(),
		}
	}
}

/// A rectangle of the source plane in samples: x, y, width, height.
pub type Window = (f64, f64, f64, f64);

/// Resamples f16 planes of one size, or a window of them, to one picture size and colours them. Reads only the
/// samples the window weighs; keeps its scratch rows between calls and views.
#[derive(Clone, Debug)]
pub struct ScreenRenderer {
	x: ResampleAxis,
	y: ResampleAxis,
	window: Option<Window>,
	/// The x pass of the rows the y axis uses, `target width` values each.
	rows: Vec<f32>,
}

impl ScreenRenderer {
	/// Sizes in samples and in device pixels; panics when one is 0.
	pub fn new(source_width: usize, source_height: usize, target_width: usize, target_height: usize) -> Self {
		Self::windowed(source_width, source_height, None, target_width, target_height)
	}

	/// A `window` of the source (the zoomed picture's part) to the target size; None is the whole plane.
	pub fn windowed(
		source_width: usize,
		source_height: usize,
		window: Option<Window>,
		target_width: usize,
		target_height: usize,
	) -> Self {
		let (x, y) = axes(source_width, source_height, window, target_width, target_height);
		Self { x, y, window, rows: Vec::new() }
	}

	/// Changes the sizes or the window, keeping the scratch rows.
	pub fn set_view(
		&mut self,
		source_width: usize,
		source_height: usize,
		window: Option<Window>,
		target_width: usize,
		target_height: usize,
	) {
		if self.source_size() == (source_width, source_height)
			&& self.target_size() == (target_width, target_height)
			&& self.window == window
		{
			return;
		}
		(self.x, self.y) = axes(source_width, source_height, window, target_width, target_height);
		self.window = window;
	}

	pub fn window(&self) -> Option<Window> {
		self.window
	}

	pub fn source_size(&self) -> (usize, usize) {
		(self.x.source(), self.y.source())
	}

	pub fn target_size(&self) -> (usize, usize) {
		(self.x.target(), self.y.target())
	}

	/// Colours `plane` (source size, row-major) into `out`, RGBA8 at the target size. Rows first, then columns, in f32;
	/// an `IntensityRoot` plane is squared before and rooted after (an energy blend); the result rounds to f16 for the
	/// lookup. A dither matrix is fixed to the output pixels.
	pub fn render(&mut self, plane: &[f16], colors: ScreenColors<'_>, out: &mut [u8]) {
		let (sw, sh) = self.source_size();
		let (tw, th) = self.target_size();
		assert_eq!(plane.len(), sw * sh, "plane of the source size");
		assert_eq!(out.len(), tw * th * 4, "RGBA8 output of the target size");
		let energy = colors.kind() == SampleKind::IntensityRoot;
		let (x0, x1) = self.x.used();
		let (y0, y1) = self.y.used();
		self.rows.resize((y1 - y0) * tw, 0.0);
		let x_axis = &self.x;
		self.rows.par_chunks_mut(tw).zip(plane[y0 * sw..y1 * sw].par_chunks(sw)).for_each_init(
			|| vec![0f32; x1 - x0],
			|line, (target, samples)| {
				for (l, s) in line.iter_mut().zip(&samples[x0..x1]) {
					let v = s.to_f32();
					*l = if energy { v * v } else { v };
				}
				for (t, value) in target.iter_mut().enumerate() {
					let (first, weights) = x_axis.weights_of(t);
					*value = weights.iter().zip(&line[first - x0..]).map(|(w, v)| w * v).sum();
				}
			},
		);
		let rows = &self.rows;
		let y_axis = &self.y;
		let thresholds = match colors {
			ScreenColors::Dithered(_, depth) => depth.thresholds(),
			ScreenColors::Plain(_) => Vec::new(),
		};
		let pixels: &mut [[u8; 4]] = bytemuck::cast_slice_mut(out);
		pixels.par_chunks_mut(tw).enumerate().for_each_init(
			|| vec![0f32; tw],
			|values, (y, target)| {
				let (first, weights) = y_axis.weights_of(y);
				values.fill(0.0);
				for (k, w) in weights.iter().enumerate() {
					let r = first + k - y0;
					for (v, s) in values.iter_mut().zip(&rows[r * tw..(r + 1) * tw]) {
						*v += w * s;
					}
				}
				let half = |v: f32| f16::from_f32(if energy { v.sqrt() } else { v });
				match colors {
					ScreenColors::Plain(table) => {
						for (p, &v) in target.iter_mut().zip(values.iter()) {
							*p = table.color(half(v));
						}
					}
					ScreenColors::Dithered(table, depth) => {
						let side = depth.side();
						let row = &thresholds[(y & (side - 1)) * side..][..side];
						for (x, (p, &v)) in target.iter_mut().zip(values.iter()).enumerate() {
							let threshold = row[x & (side - 1)];
							let [r, g, b] = unpack(table.entries()[half(v).to_bits() as usize]);
							*p = [dither(r, threshold), dither(g, threshold), dither(b, threshold), 255];
						}
					}
				}
			},
		);
	}

	/// [`ScreenRenderer::render`] into a new buffer.
	pub fn render_to_vec(&mut self, plane: &[f16], colors: ScreenColors<'_>) -> Vec<u8> {
		let (tw, th) = self.target_size();
		let mut out = vec![0; tw * th * 4];
		self.render(plane, colors, &mut out);
		out
	}
}

/// The axes of a view: the `window` of the source, or all of it, to the target size.
pub(crate) fn axes(sw: usize, sh: usize, window: Option<Window>, tw: usize, th: usize) -> (ResampleAxis, ResampleAxis) {
	match window {
		None => (ResampleAxis::new(sw, tw), ResampleAxis::new(sh, th)),
		Some((x, y, w, h)) => (ResampleAxis::window(sw, (x, x + w), tw), ResampleAxis::window(sh, (y, y + h), th)),
	}
}

/// A 16-bit channel level (0..=65280) to 8 bits: the high byte, plus one where twice the low byte reaches the threshold.
fn dither(level: u16, threshold: u16) -> u8 {
	((level >> 8) + u16::from((level & 0xFF) * 2 >= threshold)) as u8
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn axes_weigh_every_sample_alike_and_sum_to_one() {
		for (source, target) in [(1920, 960), (1920, 1000), (37, 5), (64, 640), (64, 97), (5, 5), (1, 7), (7, 1)] {
			let axis = ResampleAxis::new(source, target);
			let mut coverage = vec![0f64; source];
			for t in 0..target {
				let (first, weights) = axis.weights_of(t);
				assert!(first + weights.len() <= source);
				let sum: f64 = weights.iter().map(|&w| w as f64).sum();
				assert!((sum - 1.0).abs() < 1e-5, "{source} -> {target}: pixel {t} sums to {sum}");
				for (k, &w) in weights.iter().enumerate() {
					coverage[first + k] += w as f64;
				}
			}
			if target <= source {
				let share = target as f64 / source as f64;
				assert!(coverage.iter().all(|c| (c - share).abs() < 1e-4), "{source} -> {target}");
			}
		}
	}

	#[test]
	fn a_window_resamples_its_part_of_the_axis() {
		// The right half of 8 samples onto 2 pixels: box means of samples 4-5 and 6-7.
		let half = ResampleAxis::window(8, (4.0, 8.0), 2);
		assert_eq!(half.weights_of(0), (4, &[0.5f32, 0.5][..]));
		assert_eq!(half.weights_of(1), (6, &[0.5f32, 0.5][..]));
		// Two samples onto 4 pixels, linear between their centres with the ends held.
		let zoom = ResampleAxis::window(8, (3.0, 5.0), 4);
		assert_eq!(zoom.weights_of(0), (2, &[0.25f32, 0.75][..]));
		assert_eq!(zoom.weights_of(3), (4, &[0.75f32, 0.25][..]));
		// The whole axis is the plain constructor.
		let whole = ResampleAxis::window(8, (0.0, 8.0), 8);
		assert_eq!(whole.weights_of(5), (5, &[1.0f32][..]));
		let r = ScreenRenderer::windowed(8, 8, Some((1.0, 2.0, 4.0, 4.0)), 2, 2);
		assert_eq!(r.window(), Some((1.0, 2.0, 4.0, 4.0)));
	}

	#[test]
	fn axes_copy_halve_and_double() {
		assert_eq!(ResampleAxis::new(9, 9).weights_of(4), (4, &[1.0][..]));
		assert_eq!(ResampleAxis::new(8, 4).weights_of(1), (2, &[0.5, 0.5][..]));
		let twice = ResampleAxis::new(2, 4);
		assert_eq!(twice.weights_of(1), (0, &[0.75, 0.25][..]));
		assert_eq!(twice.weights_of(0), (0, &[1.0][..]));
	}

	/// The closed form of the recursive Bayer matrix: bit k of x ^ y and of y, least significant first, make the
	/// base-4 digit of weight 4^(n-1-k).
	fn bayer_closed_form(x: usize, y: usize, n: u32) -> u16 {
		(0..n).map(|k| ((((x ^ y) >> k & 1) * 2 + (y >> k & 1)) as u16) << (2 * (n - 1 - k))).sum()
	}

	#[test]
	fn bayer_thresholds_follow_the_formula() {
		assert_eq!(DitherDepth::Bits10.thresholds(), [64, 320, 448, 192]);
		assert_eq!(
			DitherDepth::Bits12.thresholds(),
			[0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5].map(|m: u16| (2 * m + 1) << 4)
		);
		for depth in DitherDepth::ALL {
			let e = depth.bits() - 8;
			let side = depth.side();
			assert_eq!(side, 1 << (e / 2));
			let thresholds = depth.thresholds();
			for y in 0..side {
				for x in 0..side {
					let m = bayer_closed_form(x, y, e / 2);
					assert_eq!(thresholds[y * side + x], (2 * m + 1) << (8 - e), "{depth:?} ({x}, {y})");
				}
			}
			let mut sorted = thresholds.clone();
			sorted.sort_unstable();
			sorted.dedup();
			assert_eq!(sorted.len(), side * side);
			assert!(thresholds.iter().all(|&t| t > 0 && t < 512));
		}
		assert_eq!(DitherDepth::from_bits(11), None);
		assert_eq!(DitherDepth::from_bits(14), Some(DitherDepth::Bits14));
	}

	#[test]
	fn dither_rounds_at_the_threshold() {
		assert_eq!(dither(65280, 1), 255);
		assert_eq!(dither(0x1280, 0x100), 0x13);
		assert_eq!(dither(0x127F, 0x100), 0x12);
	}
}
