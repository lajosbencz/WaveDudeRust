//! The generic backend: the row kernels in safe, portable Rust, for every target. It is the reference of the
//! interface (backend/mod.rs): its kernels are the CPU engine's loops (engine/cpu.rs), in their expressions and order,
//! and Rust contracts no multiply-add, so a CPU_REV2 engine on this backend computes the CPU engine's values bit for
//! bit, whatever instruction set the code is compiled for.
//!
//! A row runs through the CPU engine's own dispatch (engine/isa.rs, `wide`): compiled for AVX2 and FMA on an x86-64
//! CPU that has them, a plain call elsewhere (SSE2 on x86-64, simd128 or scalar in wasm, NEON or VFP on ARM), which
//! changes the speed and not the values. The kernels' bodies are also the other backends' tails, the cells a vector
//! loop leaves over ([`nine_body`], [`lap_body`], [`outer_body`]).

use super::super::super::isa;
use super::super::super::stencil::{INV12, K9_AXIS, K9_DIAGONAL, centred};
use super::{Intensity, RowKernels, RowMedium, advance};

/// The generic backend. See the module docs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Generic;

impl RowKernels for Generic {
	const NAME: &'static str = "generic";
	const ENGINE_NAME: &'static str = "CPU_REV2 (generic)";

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

/// [`RowKernels::nine`] in the CPU engine's expressions and order.
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

/// The DRP Laplacian `sum over m of a_m sum over ring m of (f_k - f)` at cell `x` of a span from the seven rows `f`
/// around it, each from 3 cells left of the span.
#[inline(always)]
fn drp_at(f: &[&[f32]; 7], x: usize, a: &[f32; 4]) -> f32 {
	let (row, i) = (f[3], x + 3);
	let v = row[i];
	let near = ((row[i - 1] - v) + (row[i + 1] - v)) + ((f[2][i] - v) + (f[4][i] - v));
	let mid = ((row[i - 2] - v) + (row[i + 2] - v)) + ((f[1][i] - v) + (f[5][i] - v));
	let far = ((row[i - 3] - v) + (row[i + 3] - v)) + ((f[0][i] - v) + (f[6][i] - v));
	a[1] * near + a[2] * mid + a[3] * far
}

/// The seven rows of `f` cut to `len + 6` values, so the loops index them without bounds checks.
#[inline(always)]
pub(crate) fn cut_rows(f: [&[f32]; 7], len: usize) -> [&[f32]; 7] {
	f.map(|row| &row[..len + 6])
}

/// [`RowKernels::lap`] in the CPU engine's expressions and order.
#[inline(always)]
pub(crate) fn lap_body(u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
	let len = w.len();
	let u = cut_rows(u, len);
	match medium {
		RowMedium::Uniform { c2, .. } => {
			for (x, w) in w.iter_mut().enumerate() {
				*w = c2 * drp_at(&u, x, a);
			}
		}
		RowMedium::Planes { c2, .. } => {
			let c2 = &c2[..len];
			for x in 0..len {
				w[x] = c2[x] * drp_at(&u, x, a);
			}
		}
	}
}

/// [`RowKernels::outer`] in the CPU engine's expressions and order.
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
	let (u, w) = (&u[..len], cut_rows(w, len));
	let wc = &w[3][3..][..len];
	let cell = |x: usize, before: f32| {
		let lw = drp_at(&w, x, a);
		let (c, d) = (u[x], d(x));
		c + ((c - before) * d + (wc[x] + c2(x) * lw * INV12) * centred(d))
	};
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
