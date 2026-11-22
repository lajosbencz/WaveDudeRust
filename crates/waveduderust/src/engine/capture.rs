//! Canvas samples to stored planes (spec/40-frames-store-render.md, Frame conversion).
//!
//! Input values are addressed by canvas pixel (x, y), rim excluded. A block `(bx, by)` covers canvas
//! x in `[bx*div, min((bx+1)*div, W))`, the same for y; its value is the f32 mean in row-major order
//! over the clipped block. Output rows run top to bottom without padding.

use half::{f16, slice::HalfFloatSliceExt};
use rayon::prelude::*;

use super::types::{FrameShape, SampleFormat};
use crate::model::Layer;

// The palette indices of stored halves. capture.wgsl computes the same on the card; these are the
// reference its test (tests/gpu.rs) compares against. A NaN half maps to the palette's zero (127 for
// the field, 0 for the intensity); brightness is finite and positive (`ranges::LAYER_BRIGHTNESS`), so
// `h * b` is NaN only for a NaN half.

/// Palette index of a field half at brightness `b`: `trunc(clamp(h * b, -1, 1) * 127 + 127)`, 127 is zero,
/// 0 and 254 the clipped ends.
pub fn field_index(h: f16, b: f32) -> u8 {
	let h = h.to_f32();
	if h.is_nan() { 127 } else { ((h * b).clamp(-1.0, 1.0) * 127.0 + 127.0) as u8 }
}

/// Palette index of a half holding sqrt(I) at brightness `b`: `trunc(min(h * b, 1) * 254)`, 0 when `h` is
/// not above zero.
pub fn intensity_root_index(h: f16, b: f32) -> u8 {
	let h = h.to_f32();
	if h > 0.0 { ((h * b).min(1.0) * 254.0) as u8 } else { 0 }
}

/// Palette index of an intensity `i` (not its root, no half) at brightness `b`:
/// `trunc(min(sqrt(i) * b, 1) * 254)`, 0 when `i` is not above zero.
pub fn intensity_index(i: f32, b: f32) -> u8 {
	if i > 0.0 { ((i.sqrt() * b).min(1.0) * 254.0) as u8 } else { 0 }
}

/// Output rows per rayon task.
const ROWS_PER_TASK: usize = 8;

/// One plane of `layer` in the shape's format from canvas rows into `out` (resized to the plane, its allocation
/// reused): `fill(y, row)` writes the `canvas_width` values of canvas row `y` (for the intensity I, not its
/// root). Rows run in parallel in chunks, the block means, the root and the conversion share one pass over
/// per-task scratch rows.
pub fn plane_rows(shape: &FrameShape, layer: Layer, out: &mut Vec<u8>, fill: impl Fn(usize, &mut [f32]) + Sync) {
	let (w, h, div) = (shape.canvas_width as usize, shape.canvas_height as usize, shape.divisor as usize);
	let (ow, oh) = (shape.width() as usize, shape.height() as usize);
	let bytes = match shape.format {
		SampleFormat::Half => 2,
		SampleFormat::Index => 1,
	};
	out.resize(ow * oh * bytes, 0);
	let root = shape.intensity_root && layer == Layer::Intensity;
	out.par_chunks_mut(ow * bytes * ROWS_PER_TASK).enumerate().for_each_init(
		|| (vec![0f32; w * div], vec![0f32; ow]),
		|(rows, means), (chunk, dst)| {
			for (r, dst_row) in dst.chunks_exact_mut(ow * bytes).enumerate() {
				let by = chunk * ROWS_PER_TASK + r;
				let (y0, y1) = (by * div, ((by + 1) * div).min(h));
				if div == 1 {
					fill(y0, means);
				} else {
					for (k, y) in (y0..y1).enumerate() {
						fill(y, &mut rows[k * w..(k + 1) * w]);
					}
					for (bx, v) in means.iter_mut().enumerate() {
						let (x0, x1) = (bx * div, ((bx + 1) * div).min(w));
						let mut sum = 0f32;
						for k in 0..y1 - y0 {
							for &s in &rows[k * w + x0..k * w + x1] {
								sum += s;
							}
						}
						*v = sum / ((y1 - y0) * (x1 - x0)) as f32;
					}
				}
				if root {
					for v in means.iter_mut() {
						*v = v.sqrt();
					}
				}
				store_row(shape, layer, means, dst_row);
			}
		},
	);
}

/// Converts one row of f32 samples (already rooted for a root intensity) into the shape's format.
fn store_row(shape: &FrameShape, layer: Layer, means: &[f32], dst: &mut [u8]) {
	match shape.format {
		SampleFormat::Half => {
			let halves: &mut [f16] = bytemuck::try_cast_slice_mut(dst).expect("a Half plane is 2-byte aligned");
			halves.convert_from_f32_slice(means);
		}
		SampleFormat::Index => match layer {
			Layer::Field => {
				let b = shape.field_brightness;
				for (d, &v) in dst.iter_mut().zip(means) {
					*d = field_index(f16::from_f32(v), b);
				}
			}
			Layer::Intensity => {
				let b = shape.intensity_brightness;
				if shape.intensity_root {
					for (d, &v) in dst.iter_mut().zip(means) {
						*d = intensity_root_index(f16::from_f32(v), b);
					}
				} else {
					for (d, &i) in dst.iter_mut().zip(means) {
						*d = intensity_index(i, b);
					}
				}
			}
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn shape(w: u32, h: u32, divisor: u32, format: SampleFormat, root: bool) -> FrameShape {
		FrameShape {
			canvas_width: w,
			canvas_height: h,
			divisor,
			format,
			field_brightness: 3.0,
			intensity_brightness: 5.0,
			intensity_root: root,
		}
	}

	fn wave(x: usize, y: usize) -> f32 {
		((x as f32 * 0.37).sin() + (y as f32 * 0.21).cos()) * 0.4
	}

	fn energy(x: usize, y: usize) -> f32 {
		wave(x, y).powi(2) * 0.01
	}

	fn plane(shape: &FrameShape, layer: Layer, sample: fn(usize, usize) -> f32) -> Vec<u8> {
		let mut out = Vec::new();
		plane_rows(shape, layer, &mut out, |y, row| row.iter_mut().enumerate().for_each(|(x, v)| *v = sample(x, y)));
		out
	}

	fn halves(bytes: &[u8]) -> &[f16] {
		bytemuck::cast_slice(bytes)
	}

	#[test]
	fn means_of_even_blocks() {
		let s = shape(64, 48, 2, SampleFormat::Half, true);
		assert_eq!((s.width(), s.height()), (32, 24));
		let p = plane(&s, Layer::Field, wave);
		let m = halves(&p);
		assert_eq!(m.len(), 32 * 24);
		let (bx, by) = (5, 7);
		let expected = (wave(10, 14) + wave(11, 14) + wave(10, 15) + wave(11, 15)) / 4.0;
		assert_eq!(m[by * 32 + bx], f16::from_f32(expected));
	}

	#[test]
	fn means_of_clipped_blocks() {
		let s = shape(67, 45, 2, SampleFormat::Half, true);
		assert_eq!((s.width(), s.height()), (34, 23));
		let p = plane(&s, Layer::Field, wave);
		let m = halves(&p);
		let h = |v: f32| f16::from_f32(v);
		assert_eq!(m[33], h((wave(66, 0) + wave(66, 1)) / 2.0), "right column of one pixel");
		assert_eq!(m[22 * 34], h((wave(0, 44) + wave(1, 44)) / 2.0), "bottom row of one pixel");
		assert_eq!(m[22 * 34 + 33], h(wave(66, 44)), "corner of one pixel");
	}

	#[test]
	fn divisor_one_copies() {
		let s = shape(9, 5, 1, SampleFormat::Half, false);
		let p = plane(&s, Layer::Field, wave);
		assert_eq!(halves(&p)[2 * 9 + 4], f16::from_f32(wave(4, 2)));
		let i = plane(&s, Layer::Intensity, energy);
		assert_eq!(halves(&i)[3], f16::from_f32(energy(3, 0)));
	}

	#[test]
	fn index_equals_the_palette_index_of_the_stored_half() {
		for (w, h, div) in [(64, 48, 2), (67, 45, 2), (67, 45, 4), (30, 20, 1)] {
			let half = shape(w, h, div, SampleFormat::Half, true);
			let index = shape(w, h, div, SampleFormat::Index, true);
			let hf = plane(&half, Layer::Field, wave);
			let xf = plane(&index, Layer::Field, wave);
			let hi = plane(&half, Layer::Intensity, energy);
			let xi = plane(&index, Layer::Intensity, energy);
			assert_eq!(xf.len(), index.plane_samples());
			for (k, h) in halves(&hf).iter().enumerate() {
				assert_eq!(xf[k], field_index(*h, 3.0));
			}
			for (k, h) in halves(&hi).iter().enumerate() {
				assert_eq!(xi[k], intensity_root_index(*h, 5.0));
			}
			assert!(xf.iter().any(|&v| v != 127) && xi.iter().any(|&v| v != 0));
		}
		let i = plane(&shape(30, 20, 1, SampleFormat::Index, false), Layer::Intensity, energy);
		assert_eq!(i[3], intensity_index(energy(3, 0), 5.0));
	}

	#[test]
	fn index_rules() {
		assert_eq!(field_index(f16::NAN, 1.0), 127);
		assert_eq!(field_index(f16::from_f32(0.0), 1.0), 127);
		assert_eq!(field_index(f16::from_f32(-0.5), 1.0), 63);
		assert_eq!(field_index(f16::from_f32(5.0), 1.0), 254);
		assert_eq!(field_index(f16::from_f32(-5.0), 1.0), 0);
		assert_eq!(field_index(f16::INFINITY, 1.0), 254);
		assert_eq!(intensity_root_index(f16::NAN, 1.0), 0);
		assert_eq!(intensity_root_index(f16::from_f32(-1.0), 1.0), 0);
		assert_eq!(intensity_root_index(f16::from_f32(0.5), 1.0), 127);
		assert_eq!(intensity_root_index(f16::from_f32(2.0), 1.0), 254);
		assert_eq!(intensity_index(0.25, 1.0), 127);
		assert_eq!(intensity_index(9.0, 1.0), 254);
		assert_eq!(intensity_index(0.0, 1.0), 0);
		assert_eq!(intensity_index(f32::NAN, 1.0), 0);
	}
}
