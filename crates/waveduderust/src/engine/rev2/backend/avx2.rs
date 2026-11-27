//! The AVX2 backend (x86-64 only): the row kernels in inline assembly on AVX2 and FMA, 8 lanes at a time, for a CPU
//! that has both (detected at run time, [`Avx2::new`]). Each kernel updates a span of one grid row with no mirror cut,
//! on contiguous slices of the span's length (plus the taps' margin), so a kernel is a loop over 8-lane blocks of
//! pointers and a length (the interface's contract: backend/mod.rs).
//!
//! The row entry ([`RowKernels::run_row`]) runs the row's work compiled for AVX2 and FMA (a `#[target_feature]`
//! function the closure is inlined into), so the Rust around the kernels (the cut cells, the CPML's rows, the forcing)
//! takes the instruction set too, with no feature test on the way. The cells a kernel's 8-lane loop leaves over go
//! through the generic backend's bodies, compiled for AVX2 and FMA where they are inlined.
//!
//! The 9-point kernel ([`Avx2::nine`]) is 8 cells per iteration (one ymm register), not unrolled: the pass is bound by
//! memory on grids beyond the caches, and out-of-order execution overlaps the iterations. A general register `i` runs
//! over the bytes of the span's whole 8-cell blocks; the rows above, of and below are read at `i`, `i + 4` and `i + 8`
//! from one cell left of the span, with unaligned loads (memory operands of VEX instructions need no alignment). The
//! registers:
//!
//! - ymm0: the centre `v`; ymm1 to ymm4: the differences and sums, then the new value (ymm1) and the stages (ymm3).
//! - ymm10, ymm11: `-K9_DIAGONAL`, `-K9_AXIS`. The differences are `v - tap`, so the taps come straight from memory;
//!   the negated sums are the generic backend's sums negated, bit for bit, and the negated weights turn them back.
//! - ymm12, ymm13: on a uniform run `k = c2 (1 + d) / 2` and `d`, taken once per span; on planes ymm12 holds 0.5
//!   and `c2`, `d` are read per cell, `(1 + d) / 2` as `d 0.5 + 0.5` in one multiply-add.
//! - ymm14, ymm15: the intensity's `alpha` and `1 - alpha`, in the variant that advances the stages.
//!
//! Per block: `L u = -K9_DIAGONAL (-diagonals) + -K9_AXIS (-axes)` (one multiply, one multiply-add);
//! `next = v + ((v - next) d + L u k)`, or `+ (L u c2) ((1 + d) / 2)` on planes; then
//! `s1 = s1 alpha + v v (1 - alpha)` and `s2 = s2 alpha + s1 (1 - alpha)`, the stage loaded in the multiply-add.
//! Four variants: uniform or planes, with or without the stages. The last `len % 8` cells go through the generic
//! backend. The fused multiply-adds round differently from it, so the engine agrees with the CPU engine within a
//! tolerance (tests/rev2.rs); the unit tests below hold the kernels to a few ulps of the generic backend's
//! (backend/harness.rs).
//!
//! The DRP kernels ([`Avx2::lap`], [`Avx2::outer`]) are in the same shape, 8 cells per iteration. They read seven rows;
//! a plane's rows, and a blocked step's, lie one stride apart, so the kernels address them from two pointers and the
//! stride (rows 0 to 3 from row 1, rows 4 to 6 from row 4, the stride and its negation in general registers), which
//! leaves registers for the outer update's planes and stages; rows not equally far apart go through the generic
//! backend. The registers:
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
//! generic backend's order. The last `len % 8` cells go through the generic backend.

use std::arch::asm;
use std::arch::x86_64::_mm256_set1_ps;

use super::super::super::stencil::{INV12, K9_AXIS, K9_DIAGONAL, centred};
use super::generic::{cut_rows, lap_body, nine_body, outer_body};
use super::{Intensity, RowKernels, RowMedium};

/// The AVX2 backend. See the module docs. Only [`Avx2::new`] makes one, on a CPU with AVX2 and FMA, so a value is the
/// proof the instruction set is there: its entries rely on it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Avx2(());

impl RowKernels for Avx2 {
	const NAME: &'static str = "avx2";
	const ENGINE_NAME: &'static str = "CPU_REV2 (AVX2)";

	fn new() -> Option<Self> {
		let yes = std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma");
		yes.then_some(Self(()))
	}

	/// Runs `f` compiled for AVX2 and FMA; `f` takes the instruction set where it is inlined, so the engine marks its
	/// closures `#[inline(always)]`.
	#[inline(always)]
	fn run_row<R>(self, f: impl FnOnce() -> R) -> R {
		// SAFETY: `self` is an `Avx2`, which only `Avx2::new` makes, on a CPU with AVX2 and FMA, the features
		// `with_avx2` is compiled for.
		unsafe { with_avx2(f) }
	}

	/// In assembly (module docs, The 9-point kernel); the last `len % 8` cells go through the generic backend. Rounds
	/// differently: fused multiply-adds, and a uniform run's `c2 (1 + d) / 2` taken once.
	#[inline(always)]
	fn nine(self, u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
		// SAFETY: `self` is an `Avx2`, made only on a CPU with AVX2 and FMA, the features of `nine_avx`.
		unsafe { nine_avx(u, medium, next, stages) }
	}

	/// In assembly (module docs, The DRP kernels); the last `len % 8` cells, and rows not equally far apart, go through
	/// the generic backend. Rounds differently: multiply-adds.
	#[inline(always)]
	fn lap(self, u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
		// SAFETY: as in `nine`, the features of `lap_avx`.
		unsafe { lap_avx(u, a, medium, w) }
	}

	/// In assembly (module docs, The DRP kernels); the last `len % 8` cells, and rows not equally far apart, go through
	/// the generic backend. Rounds differently: multiply-adds, and a uniform run's `c2 / 12` and `(1 + d) / 2` taken
	/// once.
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
		// SAFETY: as in `nine`, the features of `outer_avx`.
		unsafe { outer_avx(u, w, a, medium, next, stages) }
	}
}

/// [`Avx2::run_row`]'s body.
#[target_feature(enable = "avx2,fma")]
#[inline]
fn with_avx2<R>(f: impl FnOnce() -> R) -> R {
	f()
}

/// The Laplacian of the 8 cells at byte `{i}` of the rows `{r0}`, `{r1}`, `{r2}` ([`Avx2::nine`]): the centre `v`
/// into ymm0, `L u` into ymm2, ymm1 to ymm4 clobbered. The differences are taken as `v - tap`, the negated
/// `tap - v` of the generic backend, so the taps come from memory; the sums of the negated differences are the negated
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

/// [`Avx2::nine`]'s body.
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

/// The DRP Laplacian of the 8 cells at the current block ([`Avx2::lap`], [`Avx2::outer`]): the rows from 3 above to
/// 3 below at `[{a} + {ns}]`, `[{a}]`, `[{a} + {s}]`, `[{a} + {s}*2]`, `[{b}]`, `[{b} + {s}]`, `[{b} + {s}*2]`, each
/// from 3 cells left of the block, `{a}` and `{b}` advanced block by block. The centre `v` into ymm0, `L` into ymm1,
/// ymm2 to ymm4 clobbered. As in [`nine_lap`], the differences are `v - tap`, the sums of a ring the generic backend's
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

/// [`Avx2::lap`]'s body.
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

/// [`Avx2::outer`]'s body.
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
	use super::super::harness;
	use super::*;

	#[test]
	fn the_assembly_kernel_computes_the_generic_within_ulps_and_only_its_span() {
		let Some(avx2) = Avx2::new() else { return eprintln!("skipped: the CPU has no AVX2 and FMA") };
		harness::nine_matches_generic(avx2);
	}

	#[test]
	fn the_assembly_drp_kernels_compute_the_generic_within_ulps_and_only_their_span() {
		let Some(avx2) = Avx2::new() else { return eprintln!("skipped: the CPU has no AVX2 and FMA") };
		harness::drp_match_generic(avx2);
	}
}
