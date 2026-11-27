//! The row kernels of the AVX2 engine: each updates a span of one grid row with no mirror cut, on contiguous slices
//! of the span's length (plus the taps' margin), so a kernel is a loop over 8-lane blocks of pointers and a length.
//!
//! The reference kernels are safe Rust, in the expressions and order of the CPU engine's loops (engine/cpu.rs),
//! compiled for AVX2 and FMA through `isa::wide`; Rust contracts no multiply-add, so they compute the CPU engine's
//! values bit for bit. They stay as the reference and as the tails of the hand-written kernels.
//!
//! The 9-point kernel ([`nine_native`]) is inline assembly, 8 cells per iteration (one ymm register), not unrolled:
//! the pass is bound by memory on grids beyond the caches, and out-of-order execution overlaps the iterations. A
//! general register `i` runs over the bytes of the span's whole 8-cell blocks; the rows above, of and below are read
//! at `i`, `i + 4` and `i + 8` from one cell left of the span, with unaligned loads (memory operands of VEX
//! instructions need no alignment). The registers:
//!
//! - ymm0: the centre `v`; ymm1 to ymm4: the differences and sums, then the new value (ymm1) and the stages (ymm3).
//! - ymm10, ymm11: `-K9_DIAGONAL`, `-K9_AXIS`. The differences are `v - tap`, so the taps come straight from memory;
//!   the negated sums are the reference's sums negated, bit for bit, and the negated weights turn them back.
//! - ymm12, ymm13: on a uniform run `k = c2 (1 + d) / 2` and `d`, taken once per span; on planes ymm12 holds 0.5
//!   and `c2`, `d` are read per cell, `(1 + d) / 2` as `d 0.5 + 0.5` in one multiply-add.
//! - ymm14, ymm15: the intensity's `alpha` and `1 - alpha`, in the variant that advances the stages.
//!
//! Per block: `L u = -K9_DIAGONAL (-diagonals) + -K9_AXIS (-axes)` (one multiply, one multiply-add);
//! `next = v + ((v - next) d + L u k)`, or `+ (L u c2) ((1 + d) / 2)` on planes; then
//! `s1 = s1 alpha + v v (1 - alpha)` and `s2 = s2 alpha + s1 (1 - alpha)`, the stage loaded in the multiply-add.
//! Four variants: uniform or planes, with or without the stages. The last `len % 8` cells go through the reference.
//! The fused multiply-adds round differently from the reference, so the engine agrees with the CPU engine within a
//! tolerance (tests/avx.rs); the unit test below holds the kernel to a few ulps of the reference's.
//!
//! The DRP kernels ([`lap_native`], [`outer_native`]) are inline assembly in the same shape, 8 cells per iteration.
//! They read seven rows; a plane's rows, and a blocked step's, lie one stride apart, so the kernels address them from
//! two pointers and the stride (rows 0 to 3 from row 1, rows 4 to 6 from row 4, the stride and its negation in general
//! registers), which leaves registers for the outer update's planes and stages; rows not equally far apart go through
//! the reference. The registers:
//!
//! - ymm0: the centre `v` of the Laplacian's rows (the outer update's `w`), then the field `c`; ymm1: `L`, then the
//!   new value; ymm2 to ymm4: the differences and sums.
//! - ymm8 to ymm10: `-a1`, `-a2`, `-a3`, against the differences `v - tap` as in the 9-point kernel.
//! - The inner kernel: `w = L c2`, `c2` in ymm11 on a uniform run, per cell from memory on planes.
//! - The outer kernel: on a uniform run `c2 / 12`, `(1 + d) / 2` and `d` in ymm11 to ymm13, taken once per span;
//!   on planes `1 / 12` and 0.5 in ymm11 and ymm12. `next = c + ((c - next) d + (L k + w) h)`, the bracket in one
//!   multiply-add; on planes `(L c2) / 12 + w` and `h = d 0.5 + 0.5`. The stages as the 9-point kernel's, ymm14 and
//!   ymm15.
//!
//! Per block: `L = -a1 (-near) + -a2 (-mid) + -a3 (-far)` (one multiply, two multiply-adds), each ring's sum in the
//! reference's order. The last `len % 8` cells go through the reference.

use std::arch::asm;
use std::arch::x86_64::_mm256_set1_ps;

use super::super::isa;
use super::super::stencil::{INV12, K9_AXIS, K9_DIAGONAL, centred};

/// The 9-point leapfrog row kernel ([`nine_row`]).
pub(super) type NineRow = fn([&[f32]; 3], RowMedium<'_>, &mut [f32], Option<Intensity<'_>>);
/// The modified equation's inner row kernel ([`lap_row`]).
pub(super) type LapRow = fn([&[f32]; 7], &[f32; 4], RowMedium<'_>, &mut [f32]);
/// The modified equation's outer row kernel ([`outer_row`]).
pub(super) type OuterRow = fn(&[f32], [&[f32]; 7], &[f32; 4], RowMedium<'_>, &mut [f32], Option<Intensity<'_>>);

/// The row kernels an engine runs, picked once when it is built.
#[derive(Clone, Copy)]
pub(super) struct RowKernels {
	pub(super) nine: NineRow,
	pub(super) lap: LapRow,
	pub(super) outer: OuterRow,
}

impl RowKernels {
	/// The safe Rust kernels of this module: the CPU engine's values bit for bit.
	pub(super) const REFERENCE: Self = Self { nine: nine_row, lap: lap_row, outer: outer_row };

	/// The fastest kernels for this CPU, in assembly ([`nine_native`], [`lap_native`], [`outer_native`]). Only for a
	/// CPU with AVX2 and FMA, which it asserts.
	pub(super) fn native() -> Self {
		assert!(super::available(), "the native row kernels need AVX2 and FMA");
		Self { nine: nine_native, lap: lap_native, outer: outer_native }
	}
}

/// The intensity's two EMA stages of the cells a kernel writes, advanced by the values written, and their weight.
pub(super) struct Intensity<'a> {
	pub(super) s1: &'a mut [f32],
	pub(super) s2: &'a mut [f32],
	pub(super) alpha: f32,
}

impl Intensity<'_> {
	/// The stages of the cells `from..from + len` of these.
	#[inline(always)]
	pub(super) fn span(&mut self, from: usize, len: usize) -> Intensity<'_> {
		Intensity { s1: &mut self.s1[from..][..len], s2: &mut self.s2[from..][..len], alpha: self.alpha }
	}
}

/// Advances the two EMA stages `s1`, `s2` of a cell by its new value `v`, `rest` being `1 - alpha`; the CPU engine's
/// `accumulate` per cell.
#[inline(always)]
pub(super) fn advance(s1: &mut f32, s2: &mut f32, v: f32, alpha: f32, rest: f32) {
	*s1 = *s1 * alpha + v * v * rest;
	*s2 = *s2 * alpha + *s1 * rest;
}

/// The medium of a span's cells: one value of `c2` and `d` for all of them (a uniform run, engine/cpu.rs, Uniform
/// medium), or a value per cell from the planes, each at least as long as the span.
#[derive(Clone, Copy)]
pub(super) enum RowMedium<'a> {
	Uniform { c2: f32, d: f32 },
	Planes { c2: &'a [f32], d: &'a [f32] },
}

impl<'a> RowMedium<'a> {
	/// The medium of the span's cells from its `k`th on.
	#[inline(always)]
	fn from(self, k: usize) -> Self {
		match self {
			Self::Uniform { .. } => self,
			Self::Planes { c2, d } => Self::Planes { c2: &c2[k..], d: &d[k..] },
		}
	}
}

/// The 9-point leapfrog update of the `next.len()` cells of a span, `next = u + ((u - next) d + L u c2 (1 + d) / 2)`,
/// `next` holding the field one sub-step before on entry. `u` holds the rows above, of and below the span, each from
/// one cell left of the span to one right of it (`len + 2` values); `medium` the span's `c2` and `d`. Advances
/// `stages` by the values written, when given.
pub(super) fn nine_row(u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
	isa::wide(
		#[inline(always)]
		|| nine_body(u, medium, next, stages),
	);
}

#[inline(always)]
fn nine_body(u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
	match medium {
		RowMedium::Uniform { c2, d } => nine_cells(u, |_| c2, |_| d, next, stages),
		RowMedium::Planes { c2, d } => {
			let (c2, d) = (&c2[..next.len()], &d[..next.len()]);
			nine_cells(u, |x| c2[x], |x| d[x], next, stages);
		}
	}
}

/// [`nine_row`] with the medium of cell `x` of the span at `c2(x)` and `d(x)`.
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

/// The Laplacian of the 8 cells at byte `{i}` of the rows `{r0}`, `{r1}`, `{r2}` ([`nine_native`]): the centre `v`
/// into ymm0, `L u` into ymm2, ymm1 to ymm4 clobbered. The differences are taken as `v - tap`, the negated
/// `tap - v` of the reference, so the taps come from memory; the sums of the negated differences are the negated
/// sums, bit for bit, and the negated weights in ymm10 and ymm11 turn them back.
macro_rules! nine_lap {
	() => {
		concat!(
			"vmovups ymm0, ymmword ptr [{r1} + {i} + 4]\n",
			// -diagonals = ((v - nw) + (v - ne)) + ((v - sw) + (v - se))
			"vsubps ymm1, ymm0, ymmword ptr [{r0} + {i}]\n",
			"vsubps ymm2, ymm0, ymmword ptr [{r0} + {i} + 8]\n",
			"vaddps ymm1, ymm1, ymm2\n",
			"vsubps ymm2, ymm0, ymmword ptr [{r2} + {i}]\n",
			"vsubps ymm3, ymm0, ymmword ptr [{r2} + {i} + 8]\n",
			"vaddps ymm2, ymm2, ymm3\n",
			"vaddps ymm1, ymm1, ymm2\n",
			// -axes = ((v - n) + (v - s)) + ((v - w) + (v - e))
			"vsubps ymm2, ymm0, ymmword ptr [{r0} + {i} + 4]\n",
			"vsubps ymm3, ymm0, ymmword ptr [{r2} + {i} + 4]\n",
			"vaddps ymm2, ymm2, ymm3\n",
			"vsubps ymm3, ymm0, ymmword ptr [{r1} + {i}]\n",
			"vsubps ymm4, ymm0, ymmword ptr [{r1} + {i} + 8]\n",
			"vaddps ymm3, ymm3, ymm4\n",
			"vaddps ymm2, ymm2, ymm3\n",
			// L u = -K9_DIAGONAL (-diagonals) + -K9_AXIS (-axes)
			"vmulps ymm2, ymm2, ymm11\n",
			"vfmadd231ps ymm2, ymm1, ymm10\n",
		)
	};
}

/// The update of the 8 cells at byte `{i}` of `{next}` in a uniform run, from [`nine_lap`]'s `v` and `L u`:
/// `next = v + ((v - next) d + L u k)`, `d` in ymm13 and `k = c2 (1 + d) / 2` in ymm12; the new value stays in ymm1.
macro_rules! nine_uniform {
	() => {
		concat!(
			"vsubps ymm1, ymm0, ymmword ptr [{next} + {i}]\n",
			"vmulps ymm1, ymm1, ymm13\n",
			"vfmadd231ps ymm1, ymm2, ymm12\n",
			"vaddps ymm1, ymm1, ymm0\n",
			"vmovups ymmword ptr [{next} + {i}], ymm1\n",
		)
	};
}

/// [`nine_uniform`] with `c2` and `d` per cell from `{c2}` and `{d}`, 0.5 in ymm12:
/// `next = v + ((v - next) d + (L u c2) (d 0.5 + 0.5))`.
macro_rules! nine_planes {
	() => {
		concat!(
			"vmulps ymm2, ymm2, ymmword ptr [{c2} + {i}]\n",
			"vmovups ymm3, ymmword ptr [{d} + {i}]\n",
			"vsubps ymm1, ymm0, ymmword ptr [{next} + {i}]\n",
			"vmulps ymm1, ymm1, ymm3\n",
			"vfmadd213ps ymm3, ymm12, ymm12\n",
			"vfmadd231ps ymm1, ymm2, ymm3\n",
			"vaddps ymm1, ymm1, ymm0\n",
			"vmovups ymmword ptr [{next} + {i}], ymm1\n",
		)
	};
}

/// The intensity stages of the 8 cells at byte `{i}` of `{s1}` and `{s2}`, advanced by the new values in ymm1,
/// `alpha` in ymm14 and `1 - alpha` in ymm15: `s1 = s1 alpha + v v (1 - alpha)`, `s2 = s2 alpha + s1 (1 - alpha)`.
macro_rules! nine_stages {
	() => {
		concat!(
			"vmulps ymm3, ymm1, ymm1\n",
			"vmulps ymm3, ymm3, ymm15\n",
			"vfmadd231ps ymm3, ymm14, ymmword ptr [{s1} + {i}]\n",
			"vmovups ymmword ptr [{s1} + {i}], ymm3\n",
			"vmulps ymm3, ymm3, ymm15\n",
			"vfmadd231ps ymm3, ymm14, ymmword ptr [{s2} + {i}]\n",
			"vmovups ymmword ptr [{s2} + {i}], ymm3\n",
		)
	};
}

/// The loop over 8-cell blocks: `{i}` from 0 by 32 bytes up to `{end}`, at least one block.
macro_rules! nine_loop {
	($($body:expr),*) => {
		concat!("2:\n", $($body,)* "add {i}, 32\n", "cmp {i}, {end}\n", "jb 2b\n")
	};
}

/// [`nine_row`] in AVX2 and FMA assembly (module docs, The 9-point kernel); the last `len % 8` cells go through the
/// reference. Rounds differently: fused multiply-adds, and a uniform run's `c2 (1 + d) / 2` taken once.
fn nine_native(u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
	// SAFETY: `RowKernels::native`, the only way to this kernel, asserts the CPU has AVX2 and FMA.
	unsafe { nine_avx(u, medium, next, stages) }
}

#[target_feature(enable = "avx2,fma")]
fn nine_avx(u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], mut stages: Option<Intensity<'_>>) {
	let len = next.len();
	let body = len - len % 8;
	if body > 0 {
		// The checked slices bound every access below: the taps of cell x read x..x + 2 of each row.
		let [r0, r1, r2] = u.map(|row| row[..len + 2].as_ptr());
		let (o, end) = (next[..body].as_mut_ptr(), body * 4);
		let (nkd, nka) = (_mm256_set1_ps(-K9_DIAGONAL), _mm256_set1_ps(-K9_AXIS));
		let stages = stages.as_mut().map(|st| {
			let alpha = st.alpha;
			(st.s1[..body].as_mut_ptr(), st.s2[..body].as_mut_ptr(), _mm256_set1_ps(alpha), _mm256_set1_ps(1.0 - alpha))
		});
		match (medium, stages) {
			(RowMedium::Uniform { c2, d }, None) => {
				let (k, d) = (_mm256_set1_ps(c2 * centred(d)), _mm256_set1_ps(d));
				// SAFETY: AVX2 and FMA are enabled on this function. Each block of 8 cells reads bytes i..i + 40 of
				// the three rows and i..i + 32 of `next`, and writes i..i + 32 of `next`, for i < end = body * 4: within
				// the slices checked above (rows of len + 2 values, `next` of body), unaligned loads and stores
				// (vmovups and memory operands of VEX instructions need no alignment). No stack, the declared registers.
				unsafe {
					asm!(
						nine_loop!(nine_lap!(), nine_uniform!()),
						r0 = in(reg) r0, r1 = in(reg) r1, r2 = in(reg) r2, next = in(reg) o,
						i = inout(reg) 0usize => _, end = in(reg) end,
						in("ymm10") nkd, in("ymm11") nka, in("ymm12") k, in("ymm13") d,
						out("ymm0") _, out("ymm1") _, out("ymm2") _, out("ymm3") _, out("ymm4") _,
						options(nostack),
					);
				}
			}
			(RowMedium::Uniform { c2, d }, Some((s1, s2, alpha, rest))) => {
				let (k, d) = (_mm256_set1_ps(c2 * centred(d)), _mm256_set1_ps(d));
				// SAFETY: as above, and bytes i..i + 32 of the stages read and written, within their slices of body.
				unsafe {
					asm!(
						nine_loop!(nine_lap!(), nine_uniform!(), nine_stages!()),
						r0 = in(reg) r0, r1 = in(reg) r1, r2 = in(reg) r2, next = in(reg) o,
						s1 = in(reg) s1, s2 = in(reg) s2,
						i = inout(reg) 0usize => _, end = in(reg) end,
						in("ymm10") nkd, in("ymm11") nka, in("ymm12") k, in("ymm13") d,
						in("ymm14") alpha, in("ymm15") rest,
						out("ymm0") _, out("ymm1") _, out("ymm2") _, out("ymm3") _, out("ymm4") _,
						options(nostack),
					);
				}
			}
			(RowMedium::Planes { c2, d }, stages) => {
				let (c2, d, half) = (c2[..body].as_ptr(), d[..body].as_ptr(), _mm256_set1_ps(0.5));
				match stages {
					None => {
						// SAFETY: as above, and bytes i..i + 32 of `c2` and `d` read, within their slices of body.
						unsafe {
							asm!(
								nine_loop!(nine_lap!(), nine_planes!()),
								r0 = in(reg) r0, r1 = in(reg) r1, r2 = in(reg) r2, next = in(reg) o,
								c2 = in(reg) c2, d = in(reg) d,
								i = inout(reg) 0usize => _, end = in(reg) end,
								in("ymm10") nkd, in("ymm11") nka, in("ymm12") half,
								out("ymm0") _, out("ymm1") _, out("ymm2") _, out("ymm3") _, out("ymm4") _,
								options(nostack),
							);
						}
					}
					Some((s1, s2, alpha, rest)) => {
						// SAFETY: as above, `c2`, `d` and the stages within their slices of body.
						unsafe {
							asm!(
								nine_loop!(nine_lap!(), nine_planes!(), nine_stages!()),
								r0 = in(reg) r0, r1 = in(reg) r1, r2 = in(reg) r2, next = in(reg) o,
								c2 = in(reg) c2, d = in(reg) d, s1 = in(reg) s1, s2 = in(reg) s2,
								i = inout(reg) 0usize => _, end = in(reg) end,
								in("ymm10") nkd, in("ymm11") nka, in("ymm12") half,
								in("ymm14") alpha, in("ymm15") rest,
								out("ymm0") _, out("ymm1") _, out("ymm2") _, out("ymm3") _, out("ymm4") _,
								options(nostack),
							);
						}
					}
				}
			}
		}
	}
	if body < len {
		let stages = stages.as_mut().map(|st| st.span(body, len - body));
		nine_body(u.map(|row| &row[body..]), medium.from(body), &mut next[body..], stages);
	}
}

/// The DRP Laplacian `sum over m of a_m sum over ring m of (f_k - f)` at cell `x` of a span from the seven rows `f`
/// around it, each from 3 cells left of the span ([`lap_row`]).
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
fn cut_rows(f: [&[f32]; 7], len: usize) -> [&[f32]; 7] {
	f.map(|row| &row[..len + 6])
}

/// The modified equation's inner update of the `w.len()` cells of a span, `w = c2 L u` with the DRP Laplacian of
/// weights `a` (a1..a3; a0 is implied). `u` holds the seven rows from 3 above to 3 below the span, each from 3 cells
/// left of the span to 3 right of it (`len + 6` values); `medium` the span's `c2` (its `d` unused).
pub(super) fn lap_row(u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
	isa::wide(
		#[inline(always)]
		|| lap_body(u, a, medium, w),
	);
}

#[inline(always)]
fn lap_body(u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
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

/// The modified equation's outer update of the `next.len()` cells of a span,
/// `next = u + ((u - next) d + (w + c2 L w / 12) (1 + d) / 2)`, `next` holding the field one sub-step before on
/// entry. `u` holds the span's field; `w` the seven rows of `c2 L u` from 3 above to 3 below the span, each from 3
/// cells left of it to 3 right (`len + 6` values); `medium` the span's `c2` and `d`. Advances `stages` by the values
/// written, when given.
pub(super) fn outer_row(
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

#[inline(always)]
fn outer_body(
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

/// [`outer_row`] with the medium of cell `x` of the span at `c2(x)` and `d(x)`.
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

/// The DRP Laplacian of the 8 cells at the current block ([`lap_native`], [`outer_native`]): the rows from 3 above to
/// 3 below at `[{a} + {ns}]`, `[{a}]`, `[{a} + {s}]`, `[{a} + {s}*2]`, `[{b}]`, `[{b} + {s}]`, `[{b} + {s}*2]`, each
/// from 3 cells left of the block, `{a}` and `{b}` advanced block by block. The centre `v` into ymm0, `L` into ymm1,
/// ymm2 to ymm4 clobbered. As in [`nine_lap`], the differences are `v - tap`, the sums of a ring the reference's
/// negated bit for bit, and the negated weights `-a1`, `-a2`, `-a3` in ymm8 to ymm10 turn them back:
/// `L = -a1 (-near) + -a2 (-mid) + -a3 (-far)`, one multiply and two multiply-adds.
macro_rules! drp_lap {
	() => {
		concat!(
			"vmovups ymm0, ymmword ptr [{a} + {s}*2 + 12]\n",
			// -near = ((v - w1) + (v - e1)) + ((v - n1) + (v - s1))
			"vsubps ymm1, ymm0, ymmword ptr [{a} + {s}*2 + 8]\n",
			"vsubps ymm2, ymm0, ymmword ptr [{a} + {s}*2 + 16]\n",
			"vaddps ymm1, ymm1, ymm2\n",
			"vsubps ymm2, ymm0, ymmword ptr [{a} + {s} + 12]\n",
			"vsubps ymm3, ymm0, ymmword ptr [{b} + 12]\n",
			"vaddps ymm2, ymm2, ymm3\n",
			"vaddps ymm1, ymm1, ymm2\n",
			"vmulps ymm1, ymm1, ymm8\n",
			// -mid = ((v - w2) + (v - e2)) + ((v - n2) + (v - s2))
			"vsubps ymm2, ymm0, ymmword ptr [{a} + {s}*2 + 4]\n",
			"vsubps ymm3, ymm0, ymmword ptr [{a} + {s}*2 + 20]\n",
			"vaddps ymm2, ymm2, ymm3\n",
			"vsubps ymm3, ymm0, ymmword ptr [{a} + 12]\n",
			"vsubps ymm4, ymm0, ymmword ptr [{b} + {s} + 12]\n",
			"vaddps ymm3, ymm3, ymm4\n",
			"vaddps ymm2, ymm2, ymm3\n",
			"vfmadd231ps ymm1, ymm2, ymm9\n",
			// -far = ((v - w3) + (v - e3)) + ((v - n3) + (v - s3))
			"vsubps ymm2, ymm0, ymmword ptr [{a} + {s}*2]\n",
			"vsubps ymm3, ymm0, ymmword ptr [{a} + {s}*2 + 24]\n",
			"vaddps ymm2, ymm2, ymm3\n",
			"vsubps ymm3, ymm0, ymmword ptr [{a} + {ns} + 12]\n",
			"vsubps ymm4, ymm0, ymmword ptr [{b} + {s}*2 + 12]\n",
			"vaddps ymm3, ymm3, ymm4\n",
			"vaddps ymm2, ymm2, ymm3\n",
			"vfmadd231ps ymm1, ymm2, ymm10\n",
		)
	};
}

/// The update of the 8 cells at byte `{i}` of `{next}` in a uniform run, from [`drp_lap`]'s `w` (ymm0) and `L w`
/// (ymm1): `next = c + ((c - next) d + (L w k + w) h)`, `c` from `{u}`, `k = c2 / 12` in ymm11, `h = (1 + d) / 2` in
/// ymm12 and `d` in ymm13; the new value stays in ymm1.
macro_rules! outer_uniform {
	() => {
		concat!(
			"vfmadd213ps ymm1, ymm11, ymm0\n",
			"vmovups ymm0, ymmword ptr [{u} + {i}]\n",
			"vsubps ymm2, ymm0, ymmword ptr [{next} + {i}]\n",
			"vmulps ymm2, ymm2, ymm13\n",
			"vfmadd231ps ymm2, ymm1, ymm12\n",
			"vaddps ymm1, ymm2, ymm0\n",
			"vmovups ymmword ptr [{next} + {i}], ymm1\n",
		)
	};
}

/// [`outer_uniform`] with `c2` and `d` per cell from `{c2}` and `{d}`, `1 / 12` and 0.5 in ymm11 and ymm12:
/// `next = c + ((c - next) d + ((L w c2) / 12 + w) (d 0.5 + 0.5))`.
macro_rules! outer_planes {
	() => {
		concat!(
			"vmulps ymm1, ymm1, ymmword ptr [{c2} + {i}]\n",
			"vfmadd213ps ymm1, ymm11, ymm0\n",
			"vmovups ymm3, ymmword ptr [{d} + {i}]\n",
			"vmovups ymm0, ymmword ptr [{u} + {i}]\n",
			"vsubps ymm2, ymm0, ymmword ptr [{next} + {i}]\n",
			"vmulps ymm2, ymm2, ymm3\n",
			"vfmadd213ps ymm3, ymm12, ymm12\n",
			"vfmadd231ps ymm2, ymm1, ymm3\n",
			"vaddps ymm1, ymm2, ymm0\n",
			"vmovups ymmword ptr [{next} + {i}], ymm1\n",
		)
	};
}

/// The loop over 8-cell blocks of the DRP kernels: `{i}` from 0 by 32 bytes up to `{end}`, at least one block, the row
/// pointers `{a}` and `{b}` with it.
macro_rules! drp_loop {
	($($body:expr),*) => {
		concat!("2:\n", $($body,)* "add {a}, 32\n", "add {b}, 32\n", "add {i}, 32\n", "cmp {i}, {end}\n", "jb 2b\n")
	};
}

/// The DRP kernels' view of seven rows cut to `len + 6` values: the rows 1 and 4 and the distance between rows, in
/// bytes, when the rows lie equally far apart, as a plane's rows and a blocked step's do ([`drp_lap`] addresses them
/// so); None otherwise.
fn spaced(rows: &[&[f32]; 7]) -> Option<(*const f32, *const f32, isize)> {
	let at = rows.map(|row| row.as_ptr() as isize);
	let s = at[1].wrapping_sub(at[0]);
	(1..7).all(|k| at[k].wrapping_sub(at[k - 1]) == s).then_some((rows[1].as_ptr(), rows[4].as_ptr(), s))
}

/// [`lap_row`] in AVX2 and FMA assembly (module docs, The DRP kernels); the last `len % 8` cells, and rows not equally
/// far apart, go through the reference. Rounds differently: multiply-adds.
fn lap_native(u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
	// SAFETY: `RowKernels::native`, the only way to this kernel, asserts the CPU has AVX2 and FMA.
	unsafe { lap_avx(u, a, medium, w) }
}

#[target_feature(enable = "avx2,fma")]
fn lap_avx(u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
	let len = w.len();
	let rows = cut_rows(u, len);
	let body = match spaced(&rows) {
		Some(_) => len - len % 8,
		None => 0,
	};
	if let (Some((r1, r4, s)), true) = (spaced(&rows), body > 0) {
		let (o, end) = (w[..body].as_mut_ptr(), body * 4);
		let [n1, n2, n3] = [1, 2, 3].map(|m| _mm256_set1_ps(-a[m]));
		match medium {
			RowMedium::Uniform { c2, .. } => {
				// SAFETY: AVX2 and FMA are enabled on this function. The rows lie `s` bytes apart (`spaced`), so
				// `{a} + {ns}` to `{b} + {s}*2` address rows 0 to 6 of the checked slices of len + 6 values; each block
				// of 8 cells at byte i < end = body * 4 reads bytes i..i + 56 of each row and writes bytes i..i + 32 of
				// `w`, within those slices and `w`'s first body values. Unaligned accesses (memory operands of VEX
				// instructions need no alignment). No stack, the declared registers.
				unsafe {
					asm!(
						drp_loop!(drp_lap!(), "vmulps ymm1, ymm1, ymm11\n", "vmovups ymmword ptr [{w} + {i}], ymm1\n"),
						a = inout(reg) r1 => _, b = inout(reg) r4 => _, s = in(reg) s, ns = in(reg) -s,
						w = in(reg) o, i = inout(reg) 0usize => _, end = in(reg) end,
						in("ymm8") n1, in("ymm9") n2, in("ymm10") n3, in("ymm11") _mm256_set1_ps(c2),
						out("ymm0") _, out("ymm1") _, out("ymm2") _, out("ymm3") _, out("ymm4") _,
						options(nostack),
					);
				}
			}
			RowMedium::Planes { c2, .. } => {
				let c2 = c2[..body].as_ptr();
				// SAFETY: as above, and bytes i..i + 32 of `c2` read, within its first body values.
				unsafe {
					asm!(
						drp_loop!(
							drp_lap!(),
							"vmulps ymm1, ymm1, ymmword ptr [{c2} + {i}]\n",
							"vmovups ymmword ptr [{w} + {i}], ymm1\n"
						),
						a = inout(reg) r1 => _, b = inout(reg) r4 => _, s = in(reg) s, ns = in(reg) -s,
						w = in(reg) o, c2 = in(reg) c2, i = inout(reg) 0usize => _, end = in(reg) end,
						in("ymm8") n1, in("ymm9") n2, in("ymm10") n3,
						out("ymm0") _, out("ymm1") _, out("ymm2") _, out("ymm3") _, out("ymm4") _,
						options(nostack),
					);
				}
			}
		}
	}
	if body < len {
		lap_body(rows.map(|row| &row[body..]), a, medium.from(body), &mut w[body..]);
	}
}

/// [`outer_row`] in AVX2 and FMA assembly (module docs, The DRP kernels); the last `len % 8` cells, and rows not
/// equally far apart, go through the reference. Rounds differently: multiply-adds, and a uniform run's `c2 / 12` and
/// `(1 + d) / 2` taken once.
fn outer_native(
	u: &[f32],
	w: [&[f32]; 7],
	a: &[f32; 4],
	medium: RowMedium<'_>,
	next: &mut [f32],
	stages: Option<Intensity<'_>>,
) {
	// SAFETY: `RowKernels::native`, the only way to this kernel, asserts the CPU has AVX2 and FMA.
	unsafe { outer_avx(u, w, a, medium, next, stages) }
}

#[target_feature(enable = "avx2,fma")]
fn outer_avx(
	u: &[f32],
	w: [&[f32]; 7],
	a: &[f32; 4],
	medium: RowMedium<'_>,
	next: &mut [f32],
	mut stages: Option<Intensity<'_>>,
) {
	let len = next.len();
	let rows = cut_rows(w, len);
	let body = match spaced(&rows) {
		Some(_) => len - len % 8,
		None => 0,
	};
	if let (Some((r1, r4, s)), true) = (spaced(&rows), body > 0) {
		let (c, o, end) = (u[..body].as_ptr(), next[..body].as_mut_ptr(), body * 4);
		let [n1, n2, n3] = [1, 2, 3].map(|m| _mm256_set1_ps(-a[m]));
		let stages = stages.as_mut().map(|st| {
			let alpha = st.alpha;
			(st.s1[..body].as_mut_ptr(), st.s2[..body].as_mut_ptr(), _mm256_set1_ps(alpha), _mm256_set1_ps(1.0 - alpha))
		});
		// One loop per medium and stages: `$update` the update's macro, then its named and its register operands.
		macro_rules! run {
			($update:expr, [$($named:tt)*], [$($regs:tt)*]) => {
				match stages {
					// SAFETY: AVX2 and FMA are enabled on this function. The rows lie `s` bytes apart (`spaced`), so
					// `{a} + {ns}` to `{b} + {s}*2` address rows 0 to 6 of the checked slices of len + 6 values; each
					// block of 8 cells at byte i < end = body * 4 reads bytes i..i + 56 of each row and i..i + 32 of
					// `u`, `next` and the medium's planes, and writes i..i + 32 of `next`, within those slices' first
					// body values. Unaligned accesses (memory operands of VEX instructions need no alignment). No
					// stack, the declared registers.
					None => unsafe {
						asm!(
							drp_loop!(drp_lap!(), $update),
							a = inout(reg) r1 => _, b = inout(reg) r4 => _, s = in(reg) s, ns = in(reg) -s,
							u = in(reg) c, next = in(reg) o, i = inout(reg) 0usize => _, end = in(reg) end, $($named)*
							in("ymm8") n1, in("ymm9") n2, in("ymm10") n3, $($regs)*
							out("ymm0") _, out("ymm1") _, out("ymm2") _, out("ymm3") _, out("ymm4") _,
							options(nostack),
						);
					},
					// SAFETY: as above, and bytes i..i + 32 of the stages read and written, within their first body
					// values.
					Some((s1, s2, alpha, rest)) => unsafe {
						asm!(
							drp_loop!(drp_lap!(), $update, nine_stages!()),
							a = inout(reg) r1 => _, b = inout(reg) r4 => _, s = in(reg) s, ns = in(reg) -s,
							u = in(reg) c, next = in(reg) o, s1 = in(reg) s1, s2 = in(reg) s2,
							i = inout(reg) 0usize => _, end = in(reg) end, $($named)*
							in("ymm8") n1, in("ymm9") n2, in("ymm10") n3, $($regs)*
							in("ymm14") alpha, in("ymm15") rest,
							out("ymm0") _, out("ymm1") _, out("ymm2") _, out("ymm3") _, out("ymm4") _,
							options(nostack),
						);
					},
				}
			};
		}
		match medium {
			RowMedium::Uniform { c2, d } => {
				let (k, h, d) = (_mm256_set1_ps(c2 * INV12), _mm256_set1_ps(centred(d)), _mm256_set1_ps(d));
				run!(outer_uniform!(), [], [in("ymm11") k, in("ymm12") h, in("ymm13") d,]);
			}
			RowMedium::Planes { c2, d } => {
				let (c2, d) = (c2[..body].as_ptr(), d[..body].as_ptr());
				let (inv12, half) = (_mm256_set1_ps(INV12), _mm256_set1_ps(0.5));
				run!(outer_planes!(), [c2 = in(reg) c2, d = in(reg) d,], [in("ymm11") inv12, in("ymm12") half,]);
			}
		}
	}
	if body < len {
		let stages = stages.as_mut().map(|st| st.span(body, len - body));
		outer_body(&u[body..], rows.map(|row| &row[body..]), a, medium.from(body), &mut next[body..], stages);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A xorshift generator of values in [-1, 1).
	struct Values(u32);

	impl Values {
		fn next(&mut self) -> f32 {
			self.0 ^= self.0 << 13;
			self.0 ^= self.0 >> 17;
			self.0 ^= self.0 << 5;
			(self.0 >> 8) as f32 / (1 << 23) as f32 - 1.0
		}

		fn take(&mut self, n: usize, f: impl Fn(f32) -> f32) -> Vec<f32> {
			(0..n).map(|_| f(self.next())).collect()
		}
	}

	/// Written around the span; no kernel may touch it.
	const GUARD: f32 = 1234.5;
	/// Guard cells on each side of the outputs.
	const SIDE: usize = 9;

	/// `values` between two runs of [`GUARD`] cells.
	fn guarded(values: &[f32]) -> Vec<f32> {
		[&[GUARD; SIDE][..], values, &[GUARD; SIDE]].concat()
	}

	/// The outputs of a 9-point kernel: the next field and the stages, each guarded.
	struct Outputs {
		next: Vec<f32>,
		s1: Vec<f32>,
		s2: Vec<f32>,
	}

	struct Case {
		rows: [Vec<f32>; 3],
		c2: Vec<f32>,
		d: Vec<f32>,
		before: Vec<f32>,
		s1: Vec<f32>,
		s2: Vec<f32>,
		uniform: bool,
		stages: bool,
	}

	impl Case {
		fn new(values: &mut Values, len: usize, uniform: bool, stages: bool) -> Self {
			let rows = std::array::from_fn(|_| values.take(len + 2, |v| v));
			let (c2, d) = if uniform {
				(vec![0.3 + 0.1 * values.next(); len.max(1)], vec![0.9 + 0.05 * values.next(); len.max(1)])
			} else {
				(values.take(len, |v| 0.3 + 0.2 * v), values.take(len, |v| 0.9 + 0.1 * v))
			};
			let before = values.take(len, |v| v);
			let (s1, s2) = (values.take(len, f32::abs), values.take(len, f32::abs));
			Self { rows, c2, d, before, s1, s2, uniform, stages }
		}

		fn run(&self, kernel: NineRow) -> Outputs {
			let len = self.before.len();
			let mut out = Outputs { next: guarded(&self.before), s1: guarded(&self.s1), s2: guarded(&self.s2) };
			let medium = if self.uniform {
				RowMedium::Uniform { c2: self.c2[0], d: self.d[0] }
			} else {
				RowMedium::Planes { c2: &self.c2, d: &self.d }
			};
			let stages = self.stages.then(|| Intensity {
				s1: &mut out.s1[SIDE..][..len],
				s2: &mut out.s2[SIDE..][..len],
				alpha: 0.95,
			});
			let rows = [&self.rows[0][..], &self.rows[1][..], &self.rows[2][..]];
			kernel(rows, medium, &mut out.next[SIDE..][..len], stages);
			out
		}

		/// Checks `got` against the reference kernel's `want`: within a few ulps of the magnitudes each value is
		/// computed from, and the guards untouched.
		fn check(&self, got: &Outputs, want: &Outputs, what: &str) {
			let len = self.before.len();
			for (name, g) in [("next", &got.next), ("s1", &got.s1), ("s2", &got.s2)] {
				assert_eq!(g.len(), len + 2 * SIDE);
				let guards = g[..SIDE].iter().chain(&g[SIDE + len..]);
				assert!(guards.into_iter().all(|&v| v == GUARD), "{what}: {name} written outside the span");
			}
			let ulps = 8.0 * f32::EPSILON;
			for x in 0..len {
				let field = (0..3)
					.flat_map(|r| self.rows[r][x..x + 3].iter())
					.fold(self.before[x].abs(), |m, v| m.max(v.abs()));
				let (next, s1) = (field, self.s1[x] + field * field);
				let s2 = self.s2[x] + s1;
				let at = SIDE + x;
				for (name, g, w, scale) in [
					("next", got.next[at], want.next[at], next),
					("s1", got.s1[at], want.s1[at], s1),
					("s2", got.s2[at], want.s2[at], s2),
				] {
					assert!((g - w).abs() <= ulps * scale, "{what}: {name} at {x} is {g}, the reference's {w}");
				}
			}
		}
	}

	#[test]
	fn the_assembly_kernel_computes_the_reference_within_ulps_and_only_its_span() {
		if !super::super::available() {
			eprintln!("skipped: the CPU has no AVX2 and FMA");
			return;
		}
		let mut values = Values(0x9e37_79b9);
		for len in (0..40).chain([1000, 1003]) {
			for (uniform, stages) in [(false, false), (false, true), (true, false), (true, true)] {
				let case = Case::new(&mut values, len, uniform, stages);
				let what = format!("{len} cells, uniform {uniform}, stages {stages}");
				case.check(&case.run(nine_native), &case.run(nine_row), &what);
			}
		}
	}

	/// The DRP weights a0..a3 of the 4th-order stencil.
	fn weights() -> [f32; 4] {
		crate::engine::stencil::StencilCoefficients::of(crate::model::Stencil::FourthOrder).axis.map(|a| a as f32)
	}

	/// The inputs of the DRP kernels on a span of `len` cells: seven rows `spacing` values apart in one buffer (each
	/// `len + 6` values long), the span's field, medium, previous values and stages.
	struct DrpCase {
		buf: Vec<f32>,
		spacing: usize,
		u: Vec<f32>,
		c2: Vec<f32>,
		d: Vec<f32>,
		before: Vec<f32>,
		s1: Vec<f32>,
		s2: Vec<f32>,
		uniform: bool,
		stages: bool,
	}

	impl DrpCase {
		fn new(values: &mut Values, len: usize, spacing: usize, uniform: bool, stages: bool) -> Self {
			let buf = values.take(6 * spacing + len + 6, |v| v);
			let (c2, d) = if uniform {
				(vec![0.3 + 0.1 * values.next(); len.max(1)], vec![0.9 + 0.05 * values.next(); len.max(1)])
			} else {
				(values.take(len, |v| 0.3 + 0.2 * v), values.take(len, |v| 0.9 + 0.1 * v))
			};
			let (u, before) = (values.take(len, |v| v), values.take(len, |v| v));
			let (s1, s2) = (values.take(len, f32::abs), values.take(len, f32::abs));
			Self { buf, spacing, u, c2, d, before, s1, s2, uniform, stages }
		}

		fn rows(&self) -> [&[f32]; 7] {
			let len = self.u.len();
			std::array::from_fn(|k| &self.buf[k * self.spacing..][..len + 6])
		}

		fn medium(&self) -> RowMedium<'_> {
			if self.uniform {
				RowMedium::Uniform { c2: self.c2[0], d: self.d[0] }
			} else {
				RowMedium::Planes { c2: &self.c2, d: &self.d }
			}
		}

		/// The largest magnitude a value of `L` of cell `x` is computed from: the taps times the weights' sum.
		fn lap_scale(&self, x: usize) -> f32 {
			let a = weights();
			let taps = self.rows().iter().flat_map(|row| row[x..x + 7].iter()).fold(0f32, |m, v| m.max(v.abs()));
			8.0 * (a[1].abs() + a[2].abs() + a[3].abs()) * taps
		}

		fn lap(&self, kernel: LapRow) -> Vec<f32> {
			let len = self.u.len();
			let mut w = guarded(&vec![0.0; len]);
			kernel(self.rows(), &weights(), self.medium(), &mut w[SIDE..][..len]);
			w
		}

		fn outer(&self, kernel: OuterRow) -> Outputs {
			let len = self.u.len();
			let mut out = Outputs { next: guarded(&self.before), s1: guarded(&self.s1), s2: guarded(&self.s2) };
			let stages = self.stages.then(|| Intensity {
				s1: &mut out.s1[SIDE..][..len],
				s2: &mut out.s2[SIDE..][..len],
				alpha: 0.95,
			});
			kernel(&self.u, self.rows(), &weights(), self.medium(), &mut out.next[SIDE..][..len], stages);
			out
		}
	}

	/// Asserts no value of `got` outside its `len` span differs from [`GUARD`].
	fn guards_kept(got: &[f32], len: usize, what: &str) {
		assert_eq!(got.len(), len + 2 * SIDE);
		let guards = got[..SIDE].iter().chain(&got[SIDE + len..]);
		assert!(guards.into_iter().all(|&v| v == GUARD), "{what}: written outside the span");
	}

	#[test]
	fn the_assembly_drp_kernels_compute_the_reference_within_ulps_and_only_their_span() {
		if !super::super::available() {
			eprintln!("skipped: the CPU has no AVX2 and FMA");
			return;
		}
		let mut values = Values(0x2545_f491);
		let ulps = 8.0 * f32::EPSILON;
		for len in (0..40).chain([1000, 1003]) {
			// Rows a plane's stride apart, at two strides; both run the assembly. Rows not equally far apart, which
			// the engine never hands over (its rows come from one plane), would go through the reference.
			for spacing in [len + 6, len + 17] {
				for (uniform, stages) in [(false, false), (false, true), (true, false), (true, true)] {
					let case = DrpCase::new(&mut values, len, spacing, uniform, stages);
					let what = format!("{len} cells {spacing} apart, uniform {uniform}, stages {stages}");
					let (got, want) = (case.lap(lap_native), case.lap(lap_row));
					guards_kept(&got, len, &format!("lap, {what}"));
					for x in 0..len {
						let (g, w, scale) =
							(got[SIDE + x], want[SIDE + x], case.lap_scale(x) * case.c2[x.min(case.c2.len() - 1)]);
						assert!((g - w).abs() <= ulps * scale, "lap, {what}: {g} at {x}, the reference's {w}");
					}
					let (got, want) = (case.outer(outer_native), case.outer(outer_row));
					for (name, g) in [("next", &got.next), ("s1", &got.s1), ("s2", &got.s2)] {
						guards_kept(g, len, &format!("outer {name}, {what}"));
					}
					for x in 0..len {
						let c2 = case.c2[x.min(case.c2.len() - 1)];
						let wc = case.rows()[3][x + 3].abs();
						let next = case.u[x].abs() + case.before[x].abs() + wc + c2 * case.lap_scale(x);
						let s1 = case.s1[x] + next * next;
						let s2 = case.s2[x] + s1;
						let at = SIDE + x;
						for (name, g, w, scale) in [
							("next", got.next[at], want.next[at], next),
							("s1", got.s1[at], want.s1[at], s1),
							("s2", got.s2[at], want.s2[at], s2),
						] {
							assert!(
								(g - w).abs() <= ulps * scale,
								"outer, {what}: {name} at {x} is {g}, the reference's {w}"
							);
						}
					}
				}
			}
		}
	}
}
