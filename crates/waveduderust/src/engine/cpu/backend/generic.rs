//! The generic backend: the row kernels in safe, portable Rust, for every target. It is the reference of the
//! interface (backend/mod.rs): its kernels are the per-cell stencils' expressions in their order (engine/cpu/rows.rs),
//! and Rust contracts no multiply-add, so the CPU engine on this backend computes the same values bit for bit, whatever
//! instruction set the code is compiled for.
//!
//! A row runs through the CPU dispatch of engine/isa.rs (`wide`): compiled for AVX2 and FMA on an x86-64
//! CPU that has them, a plain call elsewhere (SSE2 on x86-64, simd128 or scalar in wasm, NEON or VFP on ARM), which
//! changes the speed and not the values. The kernels' bodies are also the other backends' tails, the cells a vector
//! loop leaves over ([`nine_body`], [`lap_body`], [`outer_body`]).
//!
//! Vectors: the kernels are written for the compiler to vectorise, with no intrinsics. Each loop reads its taps as
//! slices of the span's length indexed in step (no offsets, no bounds checks left in the loop), and a cell's value
//! is a function inlined by force, not a closure (one called from two loops stayed out of line, and its loops
//! scalar). So every kernel, on either medium, with and without the stages, has a vector body: 4 lanes at the
//! x86-64 baseline (SSE2) and 8 in `wide`'s AVX2 copy, the cells a vector leaves over scalar, the same expressions
//! per lane, so the same bits (docs/measurements.md, the generic backend vectorised; the unit test
//! below runs the AVX2 copy against the baseline's). The orchestration inlines the kernels into the row entry by
//! force for that (engine/cpu/mod.rs, `forced`).

use super::super::super::isa;
use super::super::super::stencil::{INV12, K9_AXIS, K9_DIAGONAL, centred};
use super::{Intensity, RowKernels, RowMedium, advance};

/// The generic backend. See the module docs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Generic;

impl RowKernels for Generic {
	const NAME: &'static str = "generic";
	const ENGINE_NAME: &'static str = "CPU (generic)";

	fn new() -> Option<Self> {
		Some(Self)
	}

	#[inline(always)]
	fn run_row<R>(self, f: impl FnOnce() -> R) -> R {
		isa::wide(f)
	}

	#[inline(always)]
	fn nine(self, u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
		nine_body(u, medium, next, stages);
	}

	#[inline(always)]
	fn lap(self, u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
		lap_body(u, a, medium, w);
	}

	#[inline(always)]
	fn outer(
		self,
		u: &[f32],
		w: [&[f32]; 7],
		a: &[f32; 4],
		medium: RowMedium<'_>,
		next: &mut [f32],
		stages: Option<Intensity<'_>>,
	) {
		outer_body(u, w, a, medium, next, stages);
	}
}

/// [`RowKernels::nine`] in the per-cell stencil's expressions and order.
#[inline(always)]
pub(crate) fn nine_body(u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
	match medium {
		RowMedium::Uniform { c2, d } => nine_cells(u, |_| c2, |_| d, next, stages),
		RowMedium::Planes { c2, d } => {
			let (c2, d) = (&c2[..next.len()], &d[..next.len()]);
			nine_cells(u, |x| c2[x], |x| d[x], next, stages);
		}
	}
}

/// [`nine_body`] with the medium of cell `x` of the span at `c2(x)` and `d(x)`.
#[inline(always)]
fn nine_cells(
	u: [&[f32]; 3],
	c2: impl Fn(usize) -> f32,
	d: impl Fn(usize) -> f32,
	next: &mut [f32],
	stages: Option<Intensity<'_>>,
) {
	let len = next.len();
	let tap = |row: usize, dx: usize| &u[row][dx..][..len];
	// nw, n, ne, w, c, e, sw, s, se
	let taps: [&[f32]; 9] = std::array::from_fn(|k| tap(k / 3, k % 3));
	let cell = |x: usize, before: f32| nine_cell(&taps, x, before, c2(x), d(x));
	match stages {
		None => {
			for (x, next) in next.iter_mut().enumerate() {
				*next = cell(x, *next);
			}
		}
		Some(Intensity { s1, s2, alpha }) => {
			let (s1, s2, rest) = (&mut s1[..len], &mut s2[..len], 1.0 - alpha);
			for x in 0..len {
				let v = cell(x, next[x]);
				next[x] = v;
				advance(&mut s1[x], &mut s2[x], v, alpha, rest);
			}
		}
	}
}

/// The new value of cell `x` of a span from its `taps` (rows nw n ne, w c e, sw s se), the value `before` and the
/// medium. A function inlined by force: as a closure called from two loops it stayed out of line, and neither loop
/// vectorised.
#[inline(always)]
fn nine_cell(taps: &[&[f32]; 9], x: usize, before: f32, c2: f32, d: f32) -> f32 {
	let [nw, n, ne, w, c, e, sw, so, se] = taps;
	let v = c[x];
	let diagonals = ((nw[x] - v) + (ne[x] - v)) + ((sw[x] - v) + (se[x] - v));
	let axes = ((n[x] - v) + (so[x] - v)) + ((w[x] - v) + (e[x] - v));
	let lap = K9_DIAGONAL * diagonals + K9_AXIS * axes;
	v + ((v - before) * d + lap * c2 * centred(d))
}

/// The 13 taps of the DRP Laplacian of a span's `len` cells from the seven rows `f` around it, each from 3 cells left
/// of the span and at least `len + 6` long, `len` values each: the centre, then for each ring m = 1, 2, 3 the cells m
/// left, m right, m above and m below. The loops index them in step, so they vectorise (no bounds check, no offset).
#[inline(always)]
fn drp_taps(f: [&[f32]; 7], len: usize) -> [&[f32]; 13] {
	let tap = |row: usize, dx: usize| &f[row][dx..][..len];
	let ring = |m: usize| [tap(3, 3 - m), tap(3, 3 + m), tap(3 - m, 3), tap(3 + m, 3)];
	let ([w1, e1, n1, s1], [w2, e2, n2, s2], [w3, e3, n3, s3]) = (ring(1), ring(2), ring(3));
	[tap(3, 3), w1, e1, n1, s1, w2, e2, n2, s2, w3, e3, n3, s3]
}

/// The DRP Laplacian `sum over m of a_m sum over ring m of (f_k - f)` at cell `x` of a span from its [`drp_taps`]. A
/// function inlined by force, as [`nine_cell`].
#[inline(always)]
fn drp_cell(taps: &[&[f32]; 13], x: usize, a: &[f32; 4]) -> f32 {
	let [c, w1, e1, n1, s1, w2, e2, n2, s2, w3, e3, n3, s3] = taps;
	let v = c[x];
	let near = ((w1[x] - v) + (e1[x] - v)) + ((n1[x] - v) + (s1[x] - v));
	let mid = ((w2[x] - v) + (e2[x] - v)) + ((n2[x] - v) + (s2[x] - v));
	let far = ((w3[x] - v) + (e3[x] - v)) + ((n3[x] - v) + (s3[x] - v));
	a[1] * near + a[2] * mid + a[3] * far
}

/// The seven rows of `f` cut to `len + 6` values, so the loops index them without bounds checks.
#[inline(always)]
pub(crate) fn cut_rows(f: [&[f32]; 7], len: usize) -> [&[f32]; 7] {
	f.map(|row| &row[..len + 6])
}

/// [`RowKernels::lap`] in the per-cell stencil's expressions and order.
#[inline(always)]
pub(crate) fn lap_body(u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
	let len = w.len();
	let taps = drp_taps(u, len);
	match medium {
		RowMedium::Uniform { c2, .. } => {
			for (x, w) in w.iter_mut().enumerate() {
				*w = c2 * drp_cell(&taps, x, a);
			}
		}
		RowMedium::Planes { c2, .. } => {
			for (x, (w, &c2)) in w.iter_mut().zip(&c2[..len]).enumerate() {
				*w = c2 * drp_cell(&taps, x, a);
			}
		}
	}
}

/// [`RowKernels::outer`] in the per-cell stencil's expressions and order.
#[inline(always)]
pub(crate) fn outer_body(
	u: &[f32],
	w: [&[f32]; 7],
	a: &[f32; 4],
	medium: RowMedium<'_>,
	next: &mut [f32],
	stages: Option<Intensity<'_>>,
) {
	match medium {
		RowMedium::Uniform { c2, d } => outer_cells(u, w, a, |_| c2, |_| d, next, stages),
		RowMedium::Planes { c2, d } => {
			let (c2, d) = (&c2[..next.len()], &d[..next.len()]);
			outer_cells(u, w, a, |x| c2[x], |x| d[x], next, stages);
		}
	}
}

/// [`outer_body`] with the medium of cell `x` of the span at `c2(x)` and `d(x)`.
#[inline(always)]
fn outer_cells(
	u: &[f32],
	w: [&[f32]; 7],
	a: &[f32; 4],
	c2: impl Fn(usize) -> f32,
	d: impl Fn(usize) -> f32,
	next: &mut [f32],
	stages: Option<Intensity<'_>>,
) {
	let len = next.len();
	let (u, taps) = (&u[..len], drp_taps(w, len));
	match stages {
		None => {
			for (x, next) in next.iter_mut().enumerate() {
				*next = outer_cell(&taps, a, u[x], *next, c2(x), d(x), x);
			}
		}
		Some(Intensity { s1, s2, alpha }) => {
			let (s1, s2, rest) = (&mut s1[..len], &mut s2[..len], 1.0 - alpha);
			for x in 0..len {
				let v = outer_cell(&taps, a, u[x], next[x], c2(x), d(x), x);
				next[x] = v;
				advance(&mut s1[x], &mut s2[x], v, alpha, rest);
			}
		}
	}
}

/// The new value of cell `x` of a span from the [`drp_taps`] of `w` around it (the centre tap the cell's `w`), its
/// field `c`, the value `before` and the medium. A function, not a closure: called from two loops, the closure stayed
/// out of line, and neither loop vectorised.
#[inline(always)]
fn outer_cell(taps: &[&[f32]; 13], a: &[f32; 4], c: f32, before: f32, c2: f32, d: f32, x: usize) -> f32 {
	let lw = drp_cell(taps, x, a);
	c + ((c - before) * d + (taps[0][x] + c2 * lw * INV12) * centred(d))
}

#[cfg(test)]
mod tests {
	use super::super::super::super::isa;
	use super::super::super::super::stencil::{INV12, PAD, centred};
	use super::super::super::rows::{drp, nine_point};
	use super::super::advance;
	use super::super::{Intensity, RowKernels, RowMedium, harness};
	use super::{Generic, lap_body, nine_body, outer_body};

	/// The generic bodies run as the engine runs them, through engine/isa.rs's dispatch: on a CPU with AVX2 and FMA
	/// compiled for those (8 lanes), elsewhere a plain call; the harness's reference calls them plainly, compiled for
	/// the baseline (SSE2 on x86-64).
	#[derive(Clone, Copy, Debug)]
	struct Wide;

	impl RowKernels for Wide {
		const NAME: &'static str = "wide";
		const ENGINE_NAME: &'static str = "CPU (generic, wide)";

		fn new() -> Option<Self> {
			Some(Self)
		}

		fn run_row<R>(self, f: impl FnOnce() -> R) -> R {
			f()
		}

		fn nine(self, u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
			isa::wide(
				#[inline(always)]
				|| nine_body(u, medium, next, stages),
			);
		}

		fn lap(self, u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
			isa::wide(
				#[inline(always)]
				|| lap_body(u, a, medium, w),
			);
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
			isa::wide(
				#[inline(always)]
				|| outer_body(u, w, a, medium, next, stages),
			);
		}
	}

	#[test]
	fn the_wide_copy_computes_the_baseline_copys_values_bit_for_bit() {
		harness::nine_is_generic(Wide);
		harness::drp_is_generic(Wide);
	}

	/// The generic kernels against the per-cell stencils (engine/cpu/rows.rs, `nine_point` and `drp`) and the update
	/// formulas written cell by cell, bit for bit: uncut spans of 1 to 40 cells and 1003, uniform and per-cell media,
	/// with and without the intensity stages. Pins the kernels' arithmetic, which the engine's tests compare only with
	/// itself.
	#[test]
	fn the_kernels_compute_the_per_cell_stencils_bit_for_bit() {
		/// Padded row `k` (`s` long) of `p`.
		fn row(p: &[f32], s: usize, k: usize) -> &[f32] {
			&p[k * s..][..s]
		}
		let a = [0.0, 1.5, -0.15, 1.0 / 90.0];
		let alpha = 0.97;
		for len in (1..=40).chain([1003]) {
			let s = len + 2 * PAD;
			let mut seed = 0x2545_f491_u32 ^ len as u32;
			let mut noise = || {
				seed ^= seed << 13;
				seed ^= seed >> 17;
				seed ^= seed << 5;
				f32::from_bits(0x3f80_0000 | (seed >> 9)) - 1.5
			};
			let plane: Vec<f32> = (0..9 * s).map(|_| noise()).collect();
			let wp: Vec<f32> = (0..9 * s).map(|_| noise()).collect();
			let before: Vec<f32> = (0..len).map(|_| noise()).collect();
			let (c2p, dp): (Vec<f32>, Vec<f32>) = (0..len).map(|_| (0.3 + 0.1 * noise(), 0.9 + 0.05 * noise())).unzip();
			let (s1p, s2p): (Vec<f32>, Vec<f32>) = (0..len).map(|_| (noise().abs(), noise().abs())).unzip();
			let r = 4;
			let rows: [&[f32]; 7] = std::array::from_fn(|k| row(&plane, s, r - 3 + k));
			let wrows: [&[f32]; 7] = std::array::from_fn(|k| row(&wp, s, r - 3 + k));
			for uniform in [true, false] {
				let medium = |x: usize| if uniform { (0.31, 0.93) } else { (c2p[x], dp[x]) };
				let row_medium = if uniform {
					RowMedium::Uniform { c2: 0.31, d: 0.93 }
				} else {
					RowMedium::Planes { c2: &c2p, d: &dp }
				};
				for intensity in [false, true] {
					let stages = |s1: &mut Vec<f32>, s2: &mut Vec<f32>, v: &[f32]| {
						for x in 0..len {
							advance(&mut s1[x], &mut s2[x], v[x], alpha, 1.0 - alpha);
						}
					};
					let what = format!("{len} cells, uniform {uniform}, intensity {intensity}");
					let bits = |v: &[f32]| v.iter().map(|v| v.to_bits()).collect::<Vec<_>>();

					// The 9-point update.
					let want: Vec<f32> = (0..len)
						.map(|x| {
							let (c2, d) = medium(x);
							let v = plane[r * s + PAD + x];
							let lap = nine_point(&plane, r * s + PAD + x, s, 0);
							v + ((v - before[x]) * d + lap * c2 * centred(d))
						})
						.collect();
					let u: [&[f32]; 3] = std::array::from_fn(|k| &row(&plane, s, r - 1 + k)[PAD - 1..]);
					let (mut next, mut s1, mut s2) = (before.clone(), s1p.clone(), s2p.clone());
					let given = intensity.then(|| Intensity { s1: &mut s1, s2: &mut s2, alpha });
					Generic.nine(u, row_medium, &mut next, given);
					assert_eq!(bits(&next), bits(&want), "nine, {what}");
					if intensity {
						let (mut w1, mut w2) = (s1p.clone(), s2p.clone());
						stages(&mut w1, &mut w2, &want);
						assert_eq!((bits(&s1), bits(&s2)), (bits(&w1), bits(&w2)), "nine stages, {what}");
					}

					// The 4th order's inner update.
					let want: Vec<f32> = (0..len).map(|x| medium(x).0 * drp(&rows, x, &a, 0)).collect();
					let mut w = vec![0.0; len];
					Generic.lap(rows.map(|row| &row[PAD - 3..]), &a, row_medium, &mut w);
					assert_eq!(bits(&w), bits(&want), "lap, {what}");

					// The 4th order's outer update.
					let want: Vec<f32> = (0..len)
						.map(|x| {
							let (c2, d) = medium(x);
							let (c, wc) = (plane[r * s + PAD + x], wrows[3][PAD + x]);
							let lw = drp(&wrows, x, &a, 0);
							c + ((c - before[x]) * d + (wc + c2 * lw * INV12) * centred(d))
						})
						.collect();
					let (mut next, mut s1, mut s2) = (before.clone(), s1p.clone(), s2p.clone());
					let given = intensity.then(|| Intensity { s1: &mut s1, s2: &mut s2, alpha });
					let field = &row(&plane, s, r)[PAD..][..len];
					Generic.outer(field, wrows.map(|row| &row[PAD - 3..]), &a, row_medium, &mut next, given);
					assert_eq!(bits(&next), bits(&want), "outer, {what}");
					if intensity {
						let (mut w1, mut w2) = (s1p.clone(), s2p.clone());
						stages(&mut w1, &mut w2, &want);
						assert_eq!((bits(&s1), bits(&s2)), (bits(&w1), bits(&w2)), "outer stages, {what}");
					}
				}
			}
		}
	}
}
