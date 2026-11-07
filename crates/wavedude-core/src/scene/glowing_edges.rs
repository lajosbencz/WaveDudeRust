//! Glowing edges: the Sobel magnitude per colour channel of the composed scene pixels, drawn over a picture.

use rayon::prelude::*;

/// Factor on the Sobel magnitude before the clamp at 255.
pub const SCALE: f64 = 0.25;

/// The edge picture of RGBA8 scene pixels (`width * height * 4` bytes), RGBA8 of the same size: per channel R, G, B
/// `floor(min(255, sqrt(gx^2 + gy^2) * 0.25))` of the 3 x 3 Sobel, the outer ring black, alpha 255. An image smaller
/// than 3 x 3 is all black. The input's alpha is ignored.
pub fn compute(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
	assert_eq!(rgba.len(), width * height * 4, "RGBA8 pixels of the given size");
	let mut out = vec![0u8; rgba.len()];
	for pixel in out.as_chunks_mut::<4>().0 {
		pixel[3] = 255;
	}
	if width < 3 || height < 3 {
		return out;
	}
	let n = width * 4;
	out.par_chunks_mut(n).enumerate().skip(1).take(height - 2).for_each_init(
		|| (vec![0i32; n], vec![0i32; n]),
		|(sum, diff), (y, row)| {
			let up = &rgba[(y - 1) * n..y * n];
			let mid = &rgba[y * n..(y + 1) * n];
			let down = &rgba[(y + 1) * n..(y + 2) * n];
			for i in 0..n {
				let (u, m, d) = (up[i] as i32, mid[i] as i32, down[i] as i32);
				sum[i] = u + 2 * m + d;
				diff[i] = d - u;
			}
			for x in 1..width - 1 {
				for c in 0..3 {
					let i = x * 4 + c;
					let gx = (sum[i + 4] - sum[i - 4]) as f64;
					let gy = (diff[i - 4] + 2 * diff[i] + diff[i + 4]) as f64;
					row[i] = ((gx * gx + gy * gy).sqrt() * SCALE).min(255.0).floor() as u8;
				}
			}
		},
	);
	out
}

/// Adds `edges` times `gain` onto an RGBA8 `frame` of the same size: per channel R, G, B
/// `min(255, frame + trunc(edge * gain))`, alpha kept; a gain not above zero changes nothing.
pub fn blend_additive(frame: &mut [u8], edges: &[u8], gain: f64) {
	assert_eq!(frame.len(), edges.len(), "frame and edges of one size");
	if gain.is_nan() || gain <= 0.0 {
		return;
	}
	const CHUNK: usize = 64 * 1024;
	let added: [u32; 256] = std::array::from_fn(|e| (e as f64 * gain) as u32);
	frame.par_chunks_mut(CHUNK).zip(edges.par_chunks(CHUNK)).for_each(|(frame, edges)| {
		for (f, e) in frame.as_chunks_mut::<4>().0.iter_mut().zip(edges.as_chunks::<4>().0) {
			for c in 0..3 {
				f[c] = (f[c] as u32 + added[e[c] as usize]).min(255) as u8;
			}
		}
	});
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The rule as written, pixel by pixel.
	fn reference(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
		let mut out = vec![0u8; rgba.len()];
		for p in out.as_chunks_mut::<4>().0 {
			p[3] = 255;
		}
		if width < 3 || height < 3 {
			return out;
		}
		let at = |x: usize, y: usize, c: usize| rgba[(y * width + x) * 4 + c] as f64;
		for y in 1..height - 1 {
			for x in 1..width - 1 {
				for c in 0..3 {
					let sum = |x| at(x, y - 1, c) + 2.0 * at(x, y, c) + at(x, y + 1, c);
					let diff = |x| at(x, y + 1, c) - at(x, y - 1, c);
					let gx = sum(x + 1) - sum(x - 1);
					let gy = diff(x - 1) + 2.0 * diff(x) + diff(x + 1);
					out[(y * width + x) * 4 + c] = (255f64.min((gx * gx + gy * gy).sqrt() * SCALE)).floor() as u8;
				}
			}
		}
		out
	}

	fn noise(len: usize, mut state: u32) -> Vec<u8> {
		(0..len)
			.map(|_| {
				state ^= state << 13;
				state ^= state >> 17;
				state ^= state << 5;
				(state >> 24) as u8
			})
			.collect()
	}

	#[test]
	fn a_step_lights_the_two_columns_beside_it() {
		let (w, h) = (8, 6);
		let mut rgba = vec![0u8; w * h * 4];
		for y in 0..h {
			for x in 4..w {
				rgba[(y * w + x) * 4..][..4].copy_from_slice(&[200, 40, 0, 255]);
			}
		}
		let edges = compute(&rgba, w, h);
		for y in 0..h {
			for x in 0..w {
				let p = &edges[(y * w + x) * 4..][..4];
				let lit = (1..h - 1).contains(&y) && (x == 3 || x == 4);
				let expected = if lit { [200, 40, 0, 255] } else { [0, 0, 0, 255] };
				assert_eq!(p, expected, "({x}, {y})");
			}
		}
	}

	#[test]
	fn a_strong_step_clamps_at_255() {
		let (w, h) = (4, 3);
		let rgba: Vec<u8> = (0..w * h).flat_map(|i| if i % w >= 2 { [255; 4] } else { [0, 0, 0, 255] }).collect();
		let edges = compute(&rgba, w, h);
		assert_eq!(&edges[(w + 1) * 4..][..4], &[255, 255, 255, 255]);
	}

	#[test]
	fn small_images_are_black() {
		let rgba = vec![255u8; 2 * 5 * 4];
		assert!(compute(&rgba, 2, 5).as_chunks::<4>().0.iter().all(|p| *p == [0, 0, 0, 255]));
	}

	#[test]
	fn rows_match_the_rule_on_noise() {
		let (w, h) = (97, 61);
		let rgba = noise(w * h * 4, 0x2545_f491);
		let edges = compute(&rgba, w, h);
		assert_eq!(edges, reference(&rgba, w, h));
		assert_eq!(edges, compute(&rgba, w, h));
	}

	#[test]
	fn blend_adds_and_clamps() {
		let mut frame = vec![10, 250, 0, 255, 0, 0, 0, 255];
		let edges = vec![5, 5, 0, 255, 1, 2, 3, 255];
		blend_additive(&mut frame, &edges, 1.5);
		assert_eq!(frame, [17, 255, 0, 255, 1, 3, 4, 255]);
		let before = frame.clone();
		blend_additive(&mut frame, &edges, 0.0);
		assert_eq!(frame, before);
	}
}
