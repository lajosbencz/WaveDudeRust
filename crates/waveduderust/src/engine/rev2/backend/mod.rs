//! The backends of the CPU_REV2 engine: the row kernels its passes run, and the entry its rows' work goes through, one
//! implementation of [`RowKernels`] per instruction set. The orchestration (engine/rev2: the passes, the blocked
//! steps, the CPML, the forcing, Mur's condition, the damage) is target-independent and generic over the backend;
//! only the backends are target-specific.
//!
//! - `generic` (generic.rs): safe, portable Rust, every target. The reference: bit for bit the CPU engine's values.
//! - `avx2` (avx2.rs, x86-64): inline assembly on AVX2 and FMA, when the CPU has them.
//!
//! # A trait, not a table of function pointers
//!
//! The entry [`RowKernels::run_row`] takes the row's work as a closure and runs it inlined into a function compiled for
//! the backend's instruction set (`#[target_feature]`), so the Rust around the kernels (the cut cells, the CPML's rows,
//! the forcing) takes that instruction set too. Only a generic method can do that: through a function pointer the
//! closure would be a `dyn FnMut` called out of line, compiled for the baseline. So the backend is a trait, the
//! orchestration is generic over it and monomorphised per backend, and the kernels are direct calls the compiler may
//! inline (no indirect call per span). The engine stays one type, [`CpuRev2Engine`](super::CpuRev2Engine): it keeps
//! the [`Backend`] it was built on, and [`on_backend`] turns it into the backend's type once per step or sub-step,
//! the code below that point being the backend's own copy. The price is one copy of the orchestration per backend
//! built for the target.
//!
//! # The interface
//!
//! A backend is a zero-sized `Copy` type; [`RowKernels::new`] makes one only where it runs (a run-time feature test,
//! or `Some` always when the target guarantees the instruction set), so holding a value proves the instruction set is
//! there and its `unsafe` entries may rely on it. The engine calls the kernels from any thread of the rayon pool, on
//! disjoint outputs, so a backend keeps no state.
//!
//! The kernels update a span of `len` cells of one grid row, all of them uncut (no mirror cut; the engine runs the cut
//! cells itself), `len` being the output's length, any value from 0 up. Every slice is plain `f32`s with no alignment
//! beyond `f32`'s: spans start at any column (the uniform runs and the cuts split a row anywhere). The inputs are at
//! least as long as stated below (longer is allowed; a kernel reads only the stated prefix); outputs never alias inputs
//! (Rust's borrows). A kernel writes the cells `0..len` of its outputs and nothing else, and reads nothing beyond the
//! stated prefixes.
//!
//! - [`RowKernels::nine`]: the 9-point leapfrog update, `next = v + ((v - next) d + L c2 (1 + d) / 2)`, `v` the centre
//!   tap, `next` holding the field one sub-step before on entry and the new field on return, `L` the 9-point
//!   Laplacian `K9_DIAGONAL diagonals + K9_AXIS axes` (engine/stencil.rs). `u` holds the rows above, of and below the
//!   span, each from one cell left of the span (index 0) to one right of it: `len + 2` values.
//! - [`RowKernels::lap`]: the 4th order's inner update, `w = c2 L u` with the DRP Laplacian of axis weights `a`
//!   (a1..a3 used; a0 is implied by the differences). `u` holds the seven rows from 3 above to 3 below the span, each
//!   from 3 cells left of the span (index 0) to 3 right of it: `len + 6` values. The medium's `d` is unused.
//! - [`RowKernels::outer`]: the 4th order's outer update, `next = c + ((c - next) d + (w + c2 L w / 12) (1 + d) / 2)`,
//!   `c` the span's field `u` (`len` values), `w` seven rows as `lap`'s `u` (the centre row's `w` at index 3 + x),
//!   `next` as in `nine`.
//!
//! The medium ([`RowMedium`]) is one `c2` and `d` for the whole span (a uniform run) or a value per cell from the
//! planes (`len` values at least, index 0 the span's first cell). With `stages` given (`len` values each), `nine` and
//! `outer` advance the intensity's EMA stages of each cell by its new value `v`: `s1 = s1 alpha + v v (1 - alpha)`,
//! then `s2 = s2 alpha + s1 (1 - alpha)` from the new `s1` ([`advance`]).
//!
//! Row spacing: the seven rows of `lap` and `outer` are in practice one plane's (or one buffer's) consecutive rows,
//! equally far apart in memory, and a backend may address them from one pointer and the distance (avx2.rs does), but
//! only after checking it: rows anywhere must still give the right values (a fallback to the generic body will do).
//!
//! Rounding: the generic backend computes each value in the CPU engine's expressions and order, and Rust contracts no
//! multiply-add, so it is the CPU engine bit for bit; it stays so. Another backend may round differently: fuse
//! multiply-adds, take a uniform run's products of constants once (`c2 (1 + d) / 2`), negate the differences and the
//! weights; it keeps the sums' grouping, so each value lies within a few ulps of the generic backend's on the
//! magnitudes it is computed from (backend/harness.rs, the unit tests), and the engine within 1e-4 of the CPU engine's
//! peak (tests/rev2.rs). It may compute a span's cells in any order, with the cells a vector loop leaves over through
//! the generic bodies (generic.rs, [`generic::nine_body`] and its peers). Subnormals must be kept, not flushed to
//! zero: a backend on an instruction set that flushes them (32-bit ARM NEON) runs those values through the generic
//! bodies or sets the mode that keeps them, as a flushed value fails the bounds above and the engine's non-zero cells.
//!
//! The entry: the engine calls the kernels only inside [`RowKernels::run_row`] of the same backend value, which wraps
//! a whole row's work (the kernels, the cut cells, the CPML's row, the forcing, Mur's condition) with closures marked
//! `#[inline(always)]`. A backend with a `#[target_feature]` instruction set runs the closure inlined into such a
//! function (avx2.rs); the generic backend through the CPU engine's dispatch (engine/isa.rs, a plain call off x86-64).
//! The kernels must also be right when called outside it (the unit tests do).
//!
//! # Adding a backend
//!
//! One file implementing [`RowKernels`] (its module gated by `#[cfg]` on its target), and its entries here: its module,
//! a [`Backend`] variant, its place in [`Backend::ALL`] (best first) and an arm in [`on_backend`]. Nothing in the
//! orchestration changes. Its unit tests call the harness (backend/harness.rs) with a value of it, as avx2.rs's do;
//! tests/rev2.rs runs every available backend of [`Backend::ALL`] against the CPU engine. Planned:
//!
//! - `sse2` (x86 and x86-64): 4 lanes, no FMA; `std::arch` intrinsics or inline assembly, `run_row` a plain call (SSE2
//!   is the x86-64 baseline). Without fused multiply-adds, and in the generic backend's order, it could even stay bit
//!   for bit the CPU engine's.
//! - `simd128` (wasm32 with `target_feature = "simd128"`): `core::arch::wasm32` intrinsics (no inline assembly in
//!   wasm), chosen at compile time, as wasm has no run-time feature test: `new` is `Some` always, `run_row` a plain
//!   call. The engine would then also be wired into the web worker, which does not build it yet.
//! - `neon` (aarch64, and 32-bit ARM with NEON): `core::arch::aarch64` intrinsics or inline assembly, 4 lanes, `vfmaq`
//!   for the multiply-adds. Plain VFP (32-bit ARM without NEON) is scalar: the generic backend already compiles to it,
//!   and a `vfp` backend would pay only with hand-scheduled code.

#[cfg(target_arch = "x86_64")]
pub(crate) mod avx2;
pub(crate) mod generic;
#[cfg(test)]
mod harness;

/// The intensity's two EMA stages of the cells a kernel writes, advanced by the values written, and their weight.
pub(crate) struct Intensity<'a> {
	pub(crate) s1: &'a mut [f32],
	pub(crate) s2: &'a mut [f32],
	pub(crate) alpha: f32,
}

impl Intensity<'_> {
	/// The stages of the cells `from..from + len` of these.
	#[inline(always)]
	pub(crate) fn span(&mut self, from: usize, len: usize) -> Intensity<'_> {
		Intensity { s1: &mut self.s1[from..][..len], s2: &mut self.s2[from..][..len], alpha: self.alpha }
	}
}

/// Advances the two EMA stages `s1`, `s2` of a cell by its new value `v`, `rest` being `1 - alpha`; the CPU engine's
/// `accumulate` per cell.
#[inline(always)]
pub(crate) fn advance(s1: &mut f32, s2: &mut f32, v: f32, alpha: f32, rest: f32) {
	*s1 = *s1 * alpha + v * v * rest;
	*s2 = *s2 * alpha + *s1 * rest;
}

/// The medium of a span's cells: one value of `c2` and `d` for all of them (a uniform run, engine/cpu.rs, Uniform
/// medium), or a value per cell from the planes, each at least as long as the span.
#[derive(Clone, Copy)]
pub(crate) enum RowMedium<'a> {
	Uniform { c2: f32, d: f32 },
	Planes { c2: &'a [f32], d: &'a [f32] },
}

impl RowMedium<'_> {
	/// The medium of the span's cells from its `k`th on.
	#[inline(always)]
	pub(crate) fn from(self, k: usize) -> Self {
		match self {
			Self::Uniform { .. } => self,
			Self::Planes { c2, d } => Self::Planes { c2: &c2[k..], d: &d[k..] },
		}
	}
}

/// A backend: the row kernels and the row entry of one instruction set. The contract is the module docs' (The
/// interface).
pub(crate) trait RowKernels: Copy + Send + Sync + 'static {
	/// The backend's name in [`Backend::named`] and `WAVEDUDE_CPU_ENGINE=rev2-<name>`: lower case.
	const NAME: &'static str;
	/// The engine's name on this backend, for logs ([`WaveEngine::name`](crate::engine::WaveEngine::name)).
	const ENGINE_NAME: &'static str;

	/// The backend, where it runs: on this CPU (a run-time feature test) or this target; None elsewhere.
	fn new() -> Option<Self>;

	/// Runs `f`, a row's work, compiled where the backend can for its instruction set (module docs, The entry).
	fn run_row<R>(self, f: impl FnOnce() -> R) -> R;

	/// The 9-point leapfrog update of the `next.len()` cells of a span from the 3 rows `u` around it; advances
	/// `stages` by the values written, when given (module docs, The interface).
	fn nine(self, u: [&[f32]; 3], medium: RowMedium<'_>, next: &mut [f32], stages: Option<Intensity<'_>>);

	/// The 4th order's inner update `w = c2 L u` of the `w.len()` cells of a span from the 7 rows `u` around it, `a`
	/// the stencil's axis weights (module docs, The interface).
	fn lap(self, u: [&[f32]; 7], a: &[f32; 4], medium: RowMedium<'_>, w: &mut [f32]);

	/// The 4th order's outer update of the `next.len()` cells of a span from its field `u` and the 7 rows `w` around
	/// it; advances `stages` by the values written, when given (module docs, The interface).
	fn outer(
		self,
		u: &[f32],
		w: [&[f32]; 7],
		a: &[f32; 4],
		medium: RowMedium<'_>,
		next: &mut [f32],
		stages: Option<Intensity<'_>>,
	);
}

/// Evaluates `$body` with `$B` the type of the backend `$backend` (a [`Backend`]): the one place the backends' types
/// meet their variants.
macro_rules! on_backend {
	($backend:expr, $B:ident => $body:expr) => {
		match $backend {
			$crate::engine::rev2::Backend::Generic => {
				type $B = $crate::engine::rev2::backend::generic::Generic;
				$body
			}
			#[cfg(target_arch = "x86_64")]
			$crate::engine::rev2::Backend::Avx2 => {
				type $B = $crate::engine::rev2::backend::avx2::Avx2;
				$body
			}
		}
	};
}

pub(crate) use on_backend;

/// A backend of the CPU_REV2 engine ([`CpuRev2Engine::with_backend`](super::CpuRev2Engine::with_backend)); the
/// variants are the backends built for this target, which may still not run on this CPU ([`Backend::available`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Backend {
	/// Safe, portable Rust: bit for bit the CPU engine's values (module docs).
	Generic,
	/// Inline assembly on AVX2 and FMA (x86-64).
	#[cfg(target_arch = "x86_64")]
	Avx2,
}

impl Backend {
	/// The backends built for this target, best first.
	pub const ALL: &'static [Self] = &[
		#[cfg(target_arch = "x86_64")]
		Self::Avx2,
		Self::Generic,
	];

	/// The best backend that runs here: the first available of [`Backend::ALL`].
	pub fn best() -> Self {
		Self::ALL.iter().copied().find(|b| b.available()).unwrap_or(Self::Generic)
	}

	/// Whether the backend runs on this CPU.
	pub fn available(self) -> bool {
		on_backend!(self, B => B::new().is_some())
	}

	/// The backend's name: "generic", "avx2".
	pub fn name(self) -> &'static str {
		on_backend!(self, B => B::NAME)
	}

	/// The backend of [`name`](Self::name) `name` built for this target, if any.
	pub fn named(name: &str) -> Option<Self> {
		Self::ALL.iter().copied().find(|b| b.name() == name)
	}

	/// The engine's name on the backend, for logs: "CPU_REV2 (generic)", "CPU_REV2 (AVX2)".
	pub fn engine_name(self) -> &'static str {
		on_backend!(self, B => B::ENGINE_NAME)
	}
}
