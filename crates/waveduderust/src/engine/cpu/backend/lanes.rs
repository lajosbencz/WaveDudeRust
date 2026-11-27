//! The row kernels written once over a vector of lanes ([`Lanes`]), for the backends whose instruction set has plain
//! vector adds, subtracts and multiplies and no fused multiply-add in use: the simd128 backend (simd128.rs) runs them
//! on wasm's `v128`, the unit tests below on a portable 4-lane array, natively.
//!
//! Each kernel takes a span's cells one vector at a time, 4 cells on simd128, and hands the last `len % N` to the
//! generic backend's bodies (generic.rs). Not 8 at a time as two vectors side by side (each operation on two adjacent
//! 4-cell groups in a row, the pattern V8's revectorization of simd128 to 256-bit instructions fuses): measured in V8
//! 12.4 that gained nothing on the 9-point kernel and lost on the DRP ones (docs/measurements.md, The CPU engine's
//! wavefront in wasm). Such a pair is a [`Lanes`] type of its own (two vectors, each operation on one then the
//! other), should a revectorizing engine want it.
//!
//! The expressions are the generic backend's, term for term and in its order, with each `f32`
//! operation done per lane: a vector add, subtract or multiply rounds each lane as the scalar operation does, so the
//! kernels compute the generic backend's values bit for bit, whatever the lane type. Nothing is fused or hoisted that
//! would round differently: a uniform run takes only `c2`, `d` and `(1 + d) / 2` once (the generic backend's values),
//! not their products.

use std::ops::{Add, Mul, Range, Sub};

use super::super::super::stencil::{INV12, K9_AXIS, K9_DIAGONAL, centred};
use super::generic::{cut_rows, lap_body, nine_body, outer_body};
use super::{Intensity, RowMedium};

/// A vector of [`N`](Lanes::N) `f32` lanes, its operators lane by lane, each rounding as the scalar `f32` operation.
pub(crate) trait Lanes: Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> {
	/// The lanes.
	const N: usize;

	/// `v` in every lane.
	fn splat(v: f32) -> Self;

	/// The values `at..at + N` of `s`, with no alignment beyond `f32`'s.
	///
	/// # Safety
	///
	/// `at + N <= s.len()`.
	unsafe fn load(s: &[f32], at: usize) -> Self;

	/// Writes the lanes to the values `at..at + N` of `s`, with no alignment beyond `f32`'s.
	///
	/// # Safety
	///
	/// `at + N <= s.len()`.
	unsafe fn store(self, s: &mut [f32], at: usize);
}

/// The cells of a span of `len` that whole vectors of `L` cover, from its start: the rest go through the generic body.
#[inline(always)]
fn whole<L: Lanes>(len: usize) -> usize {
	len / L::N * L::N
}

/// The stages of the cells `range` of `stages`, when given.
#[inline(always)]
fn stages_of<'a>(stages: &'a mut Option<Intensity<'_>>, range: Range<usize>) -> Option<Intensity<'a>> {
	stages.as_mut().map(|st| st.span(range.start, range.len()))
}

/// [`RowKernels::nine`](super::RowKernels::nine) on lanes `L` (module docs).
#[inline(always)]
pub(crate) fn nine<L: Lanes>(
	u: [&[f32]; 3],
	medium: RowMedium<'_>,
	next: &mut [f32],
	mut stages: Option<Intensity<'_>>,
) {
	let (len, blocks) = (next.len(), whole::<L>(next.len()));
	nine_blocks::<L>(u, medium, &mut next[..blocks], stages_of(&mut stages, 0..blocks));
	let (u, medium) = (u.map(|row| &row[blocks..]), medium.from(blocks));
	nine_body(u, medium, &mut next[blocks..], stages_of(&mut stages, blocks..len));
}

/// [`nine`] on whole vectors: `next.len()` is a multiple of `L::N`.
#[inline(always)]
fn nine_blocks<L: Lanes>(u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
	let len = next.len();
	match medium {
		RowMedium::Uniform { c2, d } => {
			let (c2, d, h) = (L::splat(c2), L::splat(d), L::splat(centred(d)));
			nine_cells(u, |_| (c2, d, h), next, stages);
		}
		RowMedium::Planes { c2, d } => {
			let (c2, d) = (&c2[..len], &d[..len]);
			let (one, half) = (L::splat(1.0), L::splat(0.5));
			nine_cells(
				u,
				|x| {
					// SAFETY: `nine_cells` calls this with `x + L::N <= len`, the planes' length.
					let (c2, d) = unsafe { (L::load(c2, x), L::load(d, x)) };
					(c2, d, (one + d) * half)
				},
				next,
				stages,
			);
		}
	}
}

/// [`nine_blocks`] with `c2`, `d` and `(1 + d) / 2` of the cells `x..x + L::N` of the span at `medium(x)`.
#[inline(always)]
fn nine_cells<L: Lanes>(
	u: [&[f32]; 3],
	medium: impl Fn(usize) -> (L, L, L),
	next: &mut [f32],
	stages: Option<Intensity<'_>>,
) {
	let len = next.len();
	assert_eq!(len % L::N, 0, "whole vectors only");
	// nw, n, ne, w, c, e, sw, s, se, each `len` long.
	let taps: [&[f32]; 9] = std::array::from_fn(|k| &u[k / 3][k % 3..][..len]);
	let (k9_diagonal, k9_axis) = (L::splat(K9_DIAGONAL), L::splat(K9_AXIS));
	// The new value of the cells `x..x + L::N`, `x + L::N <= len`, from their values `before`.
	let cell = |x: usize, before: L| {
		// SAFETY: `x + L::N <= len`, the taps' length.
		let [nw, n, ne, w, c, e, sw, so, se] = taps.map(|tap| unsafe { L::load(tap, x) });
		let v = c;
		let diagonals = ((nw - v) + (ne - v)) + ((sw - v) + (se - v));
		let axes = ((n - v) + (so - v)) + ((w - v) + (e - v));
		let lap = k9_diagonal * diagonals + k9_axis * axes;
		let (c2, d, h) = medium(x);
		v + ((v - before) * d + lap * c2 * h)
	};
	match stages {
		None => {
			for x in (0..len).step_by(L::N) {
				// SAFETY: `x + L::N <= len`, `len` a multiple of `L::N`.
				unsafe { cell(x, L::load(next, x)).store(next, x) };
			}
		}
		Some(Intensity { s1, s2, alpha }) => {
			let (s1, s2) = (&mut s1[..len], &mut s2[..len]);
			let (alpha, rest) = (L::splat(alpha), L::splat(1.0 - alpha));
			for x in (0..len).step_by(L::N) {
				// SAFETY: as above, `len` the stages' length too.
				unsafe {
					let v = cell(x, L::load(next, x));
					v.store(next, x);
					advance(s1, s2, x, v, alpha, rest);
				}
			}
		}
	}
}

/// The intensity stages of the cells `x..x + L::N` advanced by their new values `v` (backend/mod.rs, `advance`).
///
/// # Safety
///
/// `x + L::N` is at most the length of `s1` and of `s2`.
#[inline(always)]
unsafe fn advance<L: Lanes>(s1: &mut [f32], s2: &mut [f32], x: usize, v: L, alpha: L, rest: L) {
	// SAFETY: within both (the caller's contract).
	unsafe {
		let first = L::load(s1, x) * alpha + v * v * rest;
		first.store(s1, x);
		(L::load(s2, x) * alpha + first * rest).store(s2, x);
	}
}

/// The DRP Laplacian of the cells `x..x + L::N` of a span from the seven rows `f` around it, each from 3 cells left of
/// the span; `a` the weights in lanes (generic.rs, `drp_cell`).
///
/// # Safety
///
/// `x + L::N + 6` is at most the length of every row.
#[inline(always)]
unsafe fn drp_at<L: Lanes>(f: &[&[f32]; 7], x: usize, a: &[L; 4]) -> L {
	let (row, i) = (f[3], x + 3);
	// SAFETY: the loads read from `x` to `x + 6 + L::N` (the caller's contract).
	let at = |r: &[f32], i: usize| unsafe { L::load(r, i) };
	let v = at(row, i);
	let near = ((at(row, i - 1) - v) + (at(row, i + 1) - v)) + ((at(f[2], i) - v) + (at(f[4], i) - v));
	let mid = ((at(row, i - 2) - v) + (at(row, i + 2) - v)) + ((at(f[1], i) - v) + (at(f[5], i) - v));
	let far = ((at(row, i - 3) - v) + (at(row, i + 3) - v)) + ((at(f[0], i) - v) + (at(f[6], i) - v));
	a[1] * near + a[2] * mid + a[3] * far
}

/// [`RowKernels::lap`](super::RowKernels::lap) on lanes `L` (module docs).
#[inline(always)]
pub(crate) fn lap<L: Lanes>(u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
	let blocks = whole::<L>(w.len());
	lap_blocks::<L>(u, a, medium, &mut w[..blocks]);
	lap_body(u.map(|row| &row[blocks..]), a, medium.from(blocks), &mut w[blocks..]);
}

/// [`lap`] on whole vectors: `w.len()` is a multiple of `L::N`.
#[inline(always)]
fn lap_blocks<L: Lanes>(u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
	let len = w.len();
	assert_eq!(len % L::N, 0, "whole vectors only");
	let (u, a) = (cut_rows(u, len), a.map(L::splat));
	match medium {
		RowMedium::Uniform { c2, .. } => {
			let c2 = L::splat(c2);
			for x in (0..len).step_by(L::N) {
				// SAFETY: `x + L::N <= len`, the rows `len + 6` long.
				unsafe { (c2 * drp_at(&u, x, &a)).store(w, x) };
			}
		}
		RowMedium::Planes { c2, .. } => {
			let c2 = &c2[..len];
			for x in (0..len).step_by(L::N) {
				// SAFETY: as above, `c2` `len` long.
				unsafe { (L::load(c2, x) * drp_at(&u, x, &a)).store(w, x) };
			}
		}
	}
}

/// [`RowKernels::outer`](super::RowKernels::outer) on lanes `L` (module docs).
#[inline(always)]
pub(crate) fn outer<L: Lanes>(
	u: &[f32],
	w: [&[f32]; 7],
	a: &[f32; 4],
	medium: RowMedium<'_>,
	next: &mut [f32],
	mut stages: Option<Intensity<'_>>,
) {
	let (len, blocks) = (next.len(), whole::<L>(next.len()));
	outer_blocks::<L>(u, w, a, medium, &mut next[..blocks], stages_of(&mut stages, 0..blocks));
	let (u, w, medium) = (&u[blocks..], w.map(|row| &row[blocks..]), medium.from(blocks));
	outer_body(u, w, a, medium, &mut next[blocks..], stages_of(&mut stages, blocks..len));
}

/// [`outer`] on whole vectors: `next.len()` is a multiple of `L::N`.
#[inline(always)]
fn outer_blocks<L: Lanes>(
	u: &[f32],
	w: [&[f32]; 7],
	a: &[f32; 4],
	medium: RowMedium<'_>,
	next: &mut [f32],
	stages: Option<Intensity<'_>>,
) {
	match medium {
		RowMedium::Uniform { c2, d } => {
			let (c2, d, h) = (L::splat(c2), L::splat(d), L::splat(centred(d)));
			outer_cells(u, w, a, |_| (c2, d, h), next, stages);
		}
		RowMedium::Planes { c2, d } => {
			let (c2, d) = (&c2[..next.len()], &d[..next.len()]);
			let (one, half) = (L::splat(1.0), L::splat(0.5));
			outer_cells(
				u,
				w,
				a,
				|x| {
					// SAFETY: `outer_cells` calls this with `x + L::N <= len`, the planes' length.
					let (c2, d) = unsafe { (L::load(c2, x), L::load(d, x)) };
					(c2, d, (one + d) * half)
				},
				next,
				stages,
			);
		}
	}
}

/// [`outer_blocks`] with `c2`, `d` and `(1 + d) / 2` of the cells `x..x + L::N` of the span at `medium(x)`.
#[inline(always)]
fn outer_cells<L: Lanes>(
	u: &[f32],
	w: [&[f32]; 7],
	a: &[f32; 4],
	medium: impl Fn(usize) -> (L, L, L),
	next: &mut [f32],
	stages: Option<Intensity<'_>>,
) {
	let len = next.len();
	assert_eq!(len % L::N, 0, "whole vectors only");
	let (u, w, a, inv12) = (&u[..len], cut_rows(w, len), a.map(L::splat), L::splat(INV12));
	let wc = &w[3][3..][..len];
	// The new value of the cells `x..x + L::N`, `x + L::N <= len`, from their values `before`.
	let cell = |x: usize, before: L| {
		// SAFETY: `x + L::N <= len`, the length of `u` and `wc`, the rows `len + 6` long.
		let (lw, c, wc) = unsafe { (drp_at(&w, x, &a), L::load(u, x), L::load(wc, x)) };
		let (c2, d, h) = medium(x);
		c + ((c - before) * d + (wc + c2 * lw * inv12) * h)
	};
	match stages {
		None => {
			for x in (0..len).step_by(L::N) {
				// SAFETY: `x + L::N <= len`, `len` a multiple of `L::N`.
				unsafe { cell(x, L::load(next, x)).store(next, x) };
			}
		}
		Some(Intensity { s1, s2, alpha }) => {
			let (s1, s2) = (&mut s1[..len], &mut s2[..len]);
			let (alpha, rest) = (L::splat(alpha), L::splat(1.0 - alpha));
			for x in (0..len).step_by(L::N) {
				// SAFETY: as above, `len` the stages' length too.
				unsafe {
					let v = cell(x, L::load(next, x));
					v.store(next, x);
					advance(s1, s2, x, v, alpha, rest);
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use std::ops::{Add, Mul, Sub};

	use super::super::{Intensity, RowKernels, RowMedium, harness};
	use super::Lanes;

	/// Four lanes in portable Rust, each operation per lane in `f32`.
	#[derive(Clone, Copy)]
	struct Quad([f32; 4]);

	impl Add for Quad {
		type Output = Self;

		fn add(self, other: Self) -> Self {
			Self(std::array::from_fn(|k| self.0[k] + other.0[k]))
		}
	}

	impl Sub for Quad {
		type Output = Self;

		fn sub(self, other: Self) -> Self {
			Self(std::array::from_fn(|k| self.0[k] - other.0[k]))
		}
	}

	impl Mul for Quad {
		type Output = Self;

		fn mul(self, other: Self) -> Self {
			Self(std::array::from_fn(|k| self.0[k] * other.0[k]))
		}
	}

	impl Lanes for Quad {
		const N: usize = 4;

		fn splat(v: f32) -> Self {
			Self([v; 4])
		}

		unsafe fn load(s: &[f32], at: usize) -> Self {
			Self(s[at..at + 4].try_into().expect("4 values"))
		}

		unsafe fn store(self, s: &mut [f32], at: usize) {
			s[at..at + 4].copy_from_slice(&self.0);
		}
	}

	/// The lane kernels on [`Quad`]s, as a backend.
	#[derive(Clone, Copy, Debug)]
	struct Portable;

	impl RowKernels for Portable {
		const NAME: &'static str = "portable";
		const ENGINE_NAME: &'static str = "CPU (portable lanes)";

		fn new() -> Option<Self> {
			Some(Self)
		}

		fn run_row<R>(self, f: impl FnOnce() -> R) -> R {
			f()
		}

		fn nine(self, u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
			super::nine::<Quad>(u, medium, next, stages);
		}

		fn lap(self, u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
			super::lap::<Quad>(u, a, medium, w);
		}

		fn outer(
			self,
			u: &[f32],
			w: [&[f32]; 7],
			a: &[f32; 4],
			medium: RowMedium<'_>,
			next: &mut [f32],
			stages: Option<Intensity<'_>>,
		) {
			super::outer::<Quad>(u, w, a, medium, next, stages);
		}
	}

	#[test]
	fn the_lane_kernels_compute_the_generic_bit_for_bit_and_only_their_span() {
		harness::nine_is_generic(Portable);
		harness::drp_is_generic(Portable);
	}
}
