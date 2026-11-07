//! Colouring a stored plane at its stored size.

use half::f16;
use rayon::prelude::*;

use super::lut::{HalfTable, Palette};

const CHUNK: usize = 16 * 1024;

/// An 8-bit plane (palette indices, 255 read as 254) as RGBA8, one pixel per sample.
pub fn render_indexed(plane: &[u8], palette: &Palette) -> Vec<u8> {
	let mut out = vec![0; plane.len() * 4];
	render_indexed_into(plane, palette, &mut out);
	out
}

/// [`render_indexed`] into `out`, RGBA8 of `plane.len() * 4` bytes.
pub fn render_indexed_into(plane: &[u8], palette: &Palette, out: &mut [u8]) {
	lookup(plane, out, |&i| palette.color(i));
}

/// An f16 plane as RGBA8 through its half table, one pixel per sample.
pub fn render_half(plane: &[f16], table: &HalfTable) -> Vec<u8> {
	let mut out = vec![0; plane.len() * 4];
	render_half_into(plane, table, &mut out);
	out
}

/// [`render_half`] into `out`, RGBA8 of `plane.len() * 4` bytes.
pub fn render_half_into(plane: &[f16], table: &HalfTable, out: &mut [u8]) {
	let entries = table.entries();
	lookup(plane, out, |h| entries[h.to_bits() as usize]);
}

fn lookup<S: Sync>(plane: &[S], out: &mut [u8], color: impl Fn(&S) -> [u8; 4] + Sync) {
	assert_eq!(out.len(), plane.len() * 4, "RGBA8 output of the plane's size");
	let pixels: &mut [[u8; 4]] = bytemuck::cast_slice_mut(out);
	pixels.par_chunks_mut(CHUNK).zip(plane.par_chunks(CHUNK)).for_each(|(pixels, samples)| {
		for (p, s) in pixels.iter_mut().zip(samples) {
			*p = color(s);
		}
	});
}
