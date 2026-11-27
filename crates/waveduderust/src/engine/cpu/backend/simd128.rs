//! The simd128 backend (wasm32 built with `target_feature = "simd128"`): the row kernels on wasm's 128-bit vectors,
//! 4 `f32` lanes, through `core::arch::wasm32` (wasm has no inline assembly). Wasm has no run-time feature test: a
//! module built with simd128 runs only where the engine has it, so the backend exists in such a build and
//! [`Simd128::new`] is always `Some`; a build without simd128 has the generic backend alone.
//!
//! The kernels are the lane kernels of backend/lanes.rs on [`V`], a `v128` as 4 lanes: 4 cells per iteration, not
//! unrolled, the last `len % 4` through the generic backend; the 9-point, the inner and the outer updates, each on a
//! uniform run or on the planes, with or without the intensity stages. Loads and stores are unaligned (`v128.load` and
//! `v128.store` with alignment 1; spans start at any column). 8 cells per iteration as two `v128`s side by side gained
//! nothing in V8 (lanes.rs).
//!
//! Rounding: simd128 has no fused multiply-add (relaxed simd's `f32x4.relaxed_madd` is a separate proposal, not used),
//! `f32x4.add`, `.sub` and `.mul` round each lane as the scalar operation does, and wasm keeps subnormals; the kernels
//! keep the generic backend's expressions and order, so this backend computes the generic backend's values bit for bit.
//! Native tests cannot run it (lanes.rs's unit tests run the same kernels on a portable
//! 4-lane type); its check is a Node harness outside the repository stepping it beside the generic backend in wasm
//! (docs/measurements.md, The CPU engine's wavefront in wasm), until the web tests run.
//!
//! The row entry ([`RowKernels::run_row`]) is a plain call: the instruction set is the module's.

use std::arch::wasm32::{f32x4_add, f32x4_mul, f32x4_splat, f32x4_sub, v128, v128_load, v128_store};
use std::ops::{Add, Mul, Sub};

use super::lanes::{self, Lanes};
use super::{Intensity, RowKernels, RowMedium};

/// The simd128 backend. See the module docs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Simd128(());

impl RowKernels for Simd128 {
	const NAME: &'static str = "simd128";
	const ENGINE_NAME: &'static str = "CPU (simd128)";

	/// Always: the module is built with simd128, which the wasm engine running it then has.
	fn new() -> Option<Self> {
		Some(Self(()))
	}

	#[inline(always)]
	fn run_row<R>(self, f: impl FnOnce() -> R) -> R {
		f()
	}

	#[inline(always)]
	fn nine(self, u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>) {
		lanes::nine::<V>(u, medium, next, stages);
	}

	#[inline(always)]
	fn lap(self, u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]) {
		lanes::lap::<V>(u, a, medium, w);
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
		lanes::outer::<V>(u, w, a, medium, next, stages);
	}
}

/// A `v128` as 4 `f32` lanes.
#[derive(Clone, Copy)]
pub(crate) struct V(v128);

impl Add for V {
	type Output = Self;

	#[inline(always)]
	fn add(self, other: Self) -> Self {
		Self(f32x4_add(self.0, other.0))
	}
}

impl Sub for V {
	type Output = Self;

	#[inline(always)]
	fn sub(self, other: Self) -> Self {
		Self(f32x4_sub(self.0, other.0))
	}
}

impl Mul for V {
	type Output = Self;

	#[inline(always)]
	fn mul(self, other: Self) -> Self {
		Self(f32x4_mul(self.0, other.0))
	}
}

impl Lanes for V {
	const N: usize = 4;

	#[inline(always)]
	fn splat(v: f32) -> Self {
		Self(f32x4_splat(v))
	}

	#[inline(always)]
	unsafe fn load(s: &[f32], at: usize) -> Self {
		debug_assert!(at + 4 <= s.len());
		// SAFETY: the 16 bytes from `at` lie in `s` (the caller's contract); `v128_load` needs no alignment.
		Self(unsafe { v128_load(s.as_ptr().add(at).cast()) })
	}

	#[inline(always)]
	unsafe fn store(self, s: &mut [f32], at: usize) {
		debug_assert!(at + 4 <= s.len());
		// SAFETY: as in `load`; `v128_store` needs no alignment either.
		unsafe { v128_store(s.as_mut_ptr().add(at).cast(), self.0) };
	}
}

#[cfg(test)]
mod tests {
	use super::super::harness;
	use super::Simd128;

	/// Built for wasm with simd128 only, so not run until the web tests are (module docs).
	#[test]
	fn the_simd128_kernels_compute_the_generic_bit_for_bit_and_only_their_span() {
		harness::nine_is_generic(Simd128(()));
		harness::drp_is_generic(Simd128(()));
	}
}
