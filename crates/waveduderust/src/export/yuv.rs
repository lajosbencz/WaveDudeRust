//! RGBA8 to planar 4:2:0 Y'`CbCr`, studio range BT.601, padded by repeating the last column and row.

use rayon::prelude::*;

/// Planar Y, Cb, Cr of a padded picture: Y `width x height`, each chroma plane `width/2 x height/2`, row-major.
#[derive(Clone, Debug)]
pub struct Yuv420 {
	pub width: usize,
	pub height: usize,
	pub y: Vec<u8>,
	pub cb: Vec<u8>,
	pub cr: Vec<u8>,
}

fn byte(v: f32) -> u8 {
	v.round_ties_even().clamp(0.0, 255.0) as u8
}

impl Yuv420 {
	/// Planes of `width x height` (both even) for pictures up to that size.
	pub(crate) fn new(width: usize, height: usize) -> Self {
		debug_assert!(width.is_multiple_of(2) && height.is_multiple_of(2));
		let chroma = width / 2 * (height / 2);
		Self { width, height, y: vec![0; width * height], cb: vec![0; chroma], cr: vec![0; chroma] }
	}

	/// Converts an RGBA8 picture of `w x h` (at most the planes' size):
	/// `Y = 16 + 0.257R + 0.504G + 0.098B`, `Cb = 128 - 0.148R - 0.291G + 0.439B`,
	/// `Cr = 128 + 0.439R - 0.368G - 0.071B` in f32; chroma the 2 x 2 mean of the unclamped values; every byte
	/// `clamp(round_even(v), 0, 255)`. Pixels beyond the picture repeat its last column and row.
	pub(crate) fn convert(&mut self, rgba: &[u8], w: usize, h: usize) {
		assert!(w <= self.width && h <= self.height && rgba.len() == w * h * 4);
		let (pw, cw) = (self.width, self.width / 2);
		let pixel = |x: usize, y: usize| {
			let i = (y.min(h - 1) * w + x.min(w - 1)) * 4;
			(rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32)
		};
		self.y
			.par_chunks_mut(pw * 2)
			.zip(self.cb.par_chunks_mut(cw).zip(self.cr.par_chunks_mut(cw)))
			.enumerate()
			.for_each(|(row2, (luma, (cb, cr)))| {
				let chroma = |x: usize, y: usize| {
					let (r, g, b) = pixel(x, y);
					(128.0 - 0.148 * r - 0.291 * g + 0.439 * b, 128.0 + 0.439 * r - 0.368 * g - 0.071 * b)
				};
				for dy in 0..2 {
					for x in 0..pw {
						let (r, g, b) = pixel(x, row2 * 2 + dy);
						luma[dy * pw + x] = byte(16.0 + 0.257 * r + 0.504 * g + 0.098 * b);
					}
				}
				for cx in 0..cw {
					let (mut sb, mut sr) = (0f32, 0f32);
					for dy in 0..2 {
						for dx in 0..2 {
							let (b, r) = chroma(cx * 2 + dx, row2 * 2 + dy);
							sb += b;
							sr += r;
						}
					}
					cb[cx] = byte(sb * 0.25);
					cr[cx] = byte(sr * 0.25);
				}
			});
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn studio_range_and_padding() {
		let rgba = [255, 255, 255, 255, 0, 0, 0, 255];
		let mut p = Yuv420::new(4, 2);
		p.convert(&rgba, 2, 1);
		assert_eq!(p.y, vec![235, 16, 16, 16, 235, 16, 16, 16]);
		assert_eq!(p.cb, vec![128, 128]);
		assert_eq!(p.cr, vec![128, 128]);
	}
}
