//! The instruction set the CPU engine's loops run on: x86-64 machines with AVX2 and FMA run them compiled for those,
//! the rest on the baseline (SSE2 on x86-64, simd128 or scalar in wasm). Rust contracts no `a * b + c` into a fused
//! multiply-add, so both compute the same values bit for bit.

/// Runs `f`, compiled for AVX2 and FMA when the CPU has them. The loops a row's work inlines take that instruction set;
/// a call `f` leaves out of line keeps the baseline's, so the closures on the way in to a loop are inlined by force on
/// x86-64 (`#[cfg_attr(target_arch = "x86_64", inline(always))]`). Elsewhere `wide` is a plain call, and forcing
/// them only grew the wasm and slowed it. A loop's own `wide` sits where its output slices are local: one reached
/// through captured pointers stays scalar.
#[inline(always)]
pub(crate) fn wide<R>(f: impl FnOnce() -> R) -> R {
	#[cfg(target_arch = "x86_64")]
	if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
		#[allow(unsafe_code)]
		// SAFETY: the CPU has AVX2 and FMA, the features `with_avx2` is compiled for.
		return unsafe { with_avx2(f) };
	}
	f()
}

/// [`wide`] on a CPU with AVX2 and FMA.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[inline]
fn with_avx2<R>(f: impl FnOnce() -> R) -> R {
	f()
}
