//! The CPU wave engine on a padded f32 grid (docs/spec/30-engine.md, deviations in docs/spec/05-deviations.md;
//! docs/physics.md for the scheme): its passes fused into blocked steps over the rows and its hot loops written as row
//! kernels of a backend (backend/mod.rs), the generic one in portable Rust or one for an instruction set (AVX2 and FMA;
//! simd128 in wasm). This module and blocked.rs, the orchestration, build for every target; only the backends are
//! target-specific. The engine is the reference; the GPU engine agrees with it within the tolerance in docs/physics.md.
//!
//! Buffers are padded by [`PAD`] rings: `stride = grid width + 2 PAD`, grid cell (gx, gy) at index
//! `(gy + PAD) * stride + gx + PAD`, canvas pixel (x, y) at grid cell (x + rim, y + rim). The rings stay 0 (the
//! wall) unless the edges are open without a rim, where Mur's first-order condition sets them in `u` (`w` stays 0,
//! docs/physics.md, Open edges). A mirror cell has c2 = 0 and stays 0; taps across a mirror are dropped (pml.rs,
//! `cuts`). The planes, the medium, the forcing, the probes, the captures, the damage rectangle, Mur's chains and the
//! CPML's state are host.rs's; this module owns the passes over the grid and how they split across threads.
//!
//! Stencils are weighted sums of differences from the centre, `K sum (u_k - u)`: each difference is exact in f32
//! for a smooth field (Sterbenz), so round-off scales with the result and the weights sum to 0 exactly. Summing
//! the taps first biases the 9-point's energy (docs/physics.md, Energy).
//!
//! Passes: the 9-point sub-step is one leapfrog pass, `next = u + ((u - prev) d + c2 L u)`; the 4th-order (DRP)
//! stencil two, `w = c2 L u` into the plane `w`, then `next = u + ((u - prev) d + (w + c2 L w / 12))` from it. `next`
//! is written over `prev`. Each pass runs on the damage grown by its reach, one grid row per task on the rayon pool;
//! a row's cells with no mirror cut go through a kernel in spans, the cut ones through the per-cell stencils
//! (rows.rs). On the last sub-step of a step the leapfrog and the outer kernels advance the intensity stages of the
//! cells they write in the same loop. With a rim, the CPML corrects each row right after the pass writes it (pml.rs,
//! `XShared::row`), after the stages.
//!
//! Uniform medium: `c2` and `d` never change during a run, and most of a scene is one medium. Each grid row keeps its
//! runs of at least 16 cells where both hold one value, bit for bit (rows.rs, `UniformRuns`); every pass hands a run's
//! cells to the kernel with its `c2` and `d` as constants, the rest with the planes' values. The values are the same,
//! so the result is the same bit for bit.
//!
//! Damage: one rectangle of grid cells (damage.rs) outside which the field, `w` and the intensity stages are +0. A
//! pass runs on the damage grown by its reach (the stencil's, the CPML's with a rim), its rows only and its columns
//! within them, and so does the CPML on the rim (pml.rs, `CpuPml::begin`); a skipped cell would compute to +0, so the
//! result is the same bit for bit. The damage then takes in the cells a source drives and the cells the pass made
//! non-zero (any bit set), found in the band it grew by. It is empty at rest and shrinks only on a reset, so every
//! cell ever written is recomputed while the run lasts. Any code that writes the field or the stages outside the
//! passes and the forcing must widen the damage; debug builds check the invariant after each sub-step and each blocked
//! step.
//!
//! Blocked steps: the passes are bound by memory once the planes leave the caches, and a step of `n` sub-steps sweeps
//! them `n` times (`2 n` for the 4th order). With two or more sub-steps (and anything to compute) the engine fuses them
//! into one sweep over the rows instead (blocked.rs): row `q` reaches level `j` (the field after `j` sub-steps) as
//! soon as the rows it reads have reached level `j - 1`, so each row is read from memory once a step and its later
//! levels come from the cache. The step is the same expressions on the same inputs as `n` sub-steps one by one, only
//! the order of the rows changes, so the results are the same bit for bit; single sub-steps keep the passes above.
//!
//! - The 4th order: a level is two units per row, `W(q, j)`, `w = c2 L u` of the field of level `j - 1` (with the
//!   rim's first application and the forcing), and `D(q, j)`, the outer update from `w` (with the rim's second
//!   application). In a band (below) `w` lives in a ring of rows per level, as many slots as rows of `w` the sweep
//!   holds at once (from `W` writing a row to the last `D` reading it: 7, 8 with a rim, derived by the tiling), so it
//!   stays in the cache and makes no trip through memory. The band also writes to the plane `w` the rows a seam's
//!   units read at the trapezoid's edge, a few rows per level and side (disjoint across levels), after the forcing
//!   and the rim's first application have completed them; the seams' units take `w` from the plane and write it there.
//!   `W(q, j)` overwrites row `q` of the plane at level `j - 1`, whose readers, the `D` units within 3 rows, have run
//!   once the field rows `W(q, j)` reads are ready, so the plane costs no lag.
//!
//! - In place: level `j` is written over level `j - 2` in the plane of `j`'s parity (`u` holds the even levels, `prev`
//!   the odd ones, and the planes swap after an odd `n`), so a row may advance only once the rows that read its older
//!   level have read it. Within one sweep a lag of one row per level does it (four with a rim, below).
//! - Threads: split tiling, without halos. The rows split into bands, one per thread; a first parallel phase runs in
//!   each band the units whose inputs and whose overwritten values' readers all lie in the band, a trapezoid that
//!   narrows by the stencil's reach per level at each side facing another band; a second parallel phase runs the
//!   inverted trapezoids around the boundaries, whose inputs the first phase left in the planes at the levels they
//!   need (a row holds its last two levels). No work is done twice, no row is copied, and a thread never waits for
//!   another inside a phase: overlapped tiling would recompute halos and copy their rows and the CPML's memory per
//!   row, and pipelined bands would stall on a preempted neighbour, which this host's noisy threads make common. A
//!   band takes at least `2 S + rho` rows (`S` the widest trapezoid side, `rho` the read reach), so two boundaries'
//!   units never meet; a grid of fewer rows runs as one band.
//! - Damage: the step runs on the damage, with every cell a source drives on any of its sub-steps, grown by `n`
//!   sub-steps' reach up front (each pass's reach, two passes a sub-step for the 4th order); a cell outside the true
//!   damage of a sub-step computes to the +0 it holds. After the step the damage takes in the non-zero cells of both
//!   planes in the band it grew by.
//! - Forcing: the step's source tables are taken before it, in order; each row adds sub-step `j - 1`'s forcing to its
//!   driven cells right after its update to level `j` (host.rs's `force_next`, row by row); the 4th order adds it to
//!   `w` right after `W(q, j)` (host.rs's `force_w`).
//! - Mur: each row runs Mur's condition on its own chains after its update to each level (the first and last rows
//!   also the rings above and below the grid), so with open edges every row of the grid takes part, the rows outside
//!   the damage with Mur's chains alone.
//! - Intensity: the stages advance in the last level's update, as in the passes.
//! - CPML: the y strips' stages split per row. psi of y of a row at level `j - 1` reads the field within 2 rows (3
//!   loaded), `delta_y` and xi of y read psi of y within 2 rows of the same strip and the field within 3; the x part
//!   reads its row's own memory and the field within 1 row. So a row's update with a rim depends on the field within
//!   4 rows, and psi of y is one more unit per row and level, run 2 rows ahead of the update (blocked.rs, `Tiling`).
//!   The 4th order applies the rim twice, to the field in `W` and to `w` in `D`, each with its own memory variables and
//!   its own psi of y unit. Each row's memory variables are its own, so the trapezoids carry them like the field.
//!
//! Fused steps: a sweep may take several whole steps too ([`CpuEngine::advance_steps`];
//! `WaveEngine::advance_steps`), for both stencils, so a row is read from memory once per `k` steps. By default `k` is
//! the most, up to [`FUSED_STEPS`], whose sweep's trapezoids (above, Threads) keep their sides within [`FUSED_SIDE`]
//! rows: a side grows by 1 row a level for the 9-point, 4 with a rim, 6 for the 4th order, 8 with a rim, and with it
//! the seams' share of the work and the rows in flight. With 2 sub-steps a step, that is 8 steps for the 9-point, 4
//! with a rim, and 2 for the 4th order; more sub-steps fuse fewer steps (docs/measurements.md). The sweep's levels are
//! the `k n` sub-steps one after another, and what happens between two steps happens between their levels, row by
//! row:
//!
//! - the forcing: the source tables of all the levels are taken before the sweep, in order, each step's at its time
//!   (an emitter that detaches within the sweep has its fade and its silence in them), and the driven cells are those
//!   of any level;
//! - the intensity: the stages advance in the update to the last level of each step (`j % n == 0`);
//! - the probes: the update of a row to the last level of step `i` records the row's probe cells (the field it wrote,
//!   the intensity of its stages) into reading `i`; the readings start as the ones before the sweep, which hold on the
//!   cells outside its area, and are queued in step order after it;
//! - Mur's condition and the CPML: per level, as within a step; the damage: the area is grown by the `k n` levels'
//!   reach up front, a superset of `k` single steps' areas, outside which every cell computes to the +0 it holds.
//!
//! Captures, checks and drains need the engine between steps, so a caller ends its batches there (run/worker.rs).
//! The values are those of `k` single steps bit for bit on the generic backend; another backend, which may round a
//! span's last cells otherwise (backend/mod.rs), within its tolerance, as the sweep's wider area splits the rows into
//! other spans. A sweep of a single level (one sub-step, one step) runs the passes, and so does every sub-step of an
//! engine set to step unblocked ([`CpuEngine::set_unblocked`]), the plainest order, which the tests take as the
//! reference.
//!
//! Backends: an engine runs on one backend, chosen when it is built ([`Backend`]: the best one this CPU runs, or one
//! named; `WAVEDUDE_CPU_BACKEND` names one for the engines `engine::build` builds); the passes and the blocked steps
//! are generic over the backend's type (`RowKernels`), which `on_backend!` picks once per step. Each row's work goes
//! through the backend's entry (`run_row`), so the Rust around the kernels (the CPML's rows, the cut cells) is compiled
//! for the backend's instruction set, with no feature test on the way. The generic backend is safe Rust in the per-cell
//! stencils' expressions and order (rows.rs), and Rust contracts no multiply-add, so its results are the same on every
//! target bit for bit ([`Backend::Generic`]). Another backend may round differently (fused multiply-adds); it then
//! agrees with the generic one within a tolerance (tests/cpu.rs).

#![allow(unsafe_code)]

mod backend;
mod blocked;
mod host;
mod rows;

use std::ops::Range;
use std::sync::Arc;

use rayon::iter::Either;
use rayon::prelude::*;

pub use self::backend::Backend;
use self::backend::{Intensity, RowKernels, RowMedium, advance, on_backend};
use self::blocked::{Line, MurRows, ProbeLog, Rim, RimSet, Rings, Shared, Step, Tiling};
use self::host::{Host, Stages};
use self::rows::{Rows, UniformRuns, chunks_or_none, cut_spans, drp, medium_spans, nine_point, plane_rows, row_cuts};
use super::damage::{Damage, extent};
use super::forcing::live;
use super::pml::{self, Medium, XPart, XPass, XShared};
use super::stencil::{INV12, PAD, TimeScheme, centred};
use super::substeps::sub_steps;
use super::types::{
	Emitter, EngineOptions, FrameShape, PendingCapture, PendingProbes, ProbeCells, ProbeReadings, SceneData, Snapshot,
};
use super::{EngineError, WaveEngine};

/// The CPU engine on one of its backends. See the module docs.
pub struct CpuEngine {
	/// The planes, the medium, the forcing, the probes and the damage (host.rs).
	host: Host,
	/// The backend the passes run on; it runs on this CPU.
	backend: Backend,
	/// The runs of each grid row where `c2` and `d` hold one value (module docs, Uniform medium).
	uniform: UniformRuns,
	/// Mur's chains by the grid row that owns them (blocked.rs).
	mur: MurRows,
	/// A blocked step's source table of each sub-step, and its driven cells with their runs of table entries.
	tables: Vec<Vec<f32>>,
	driven: Vec<(usize, u32, u32)>,
	/// The tilings of the last blocked steps by their levels, kept for the next.
	tilings: Vec<Tiling>,
	/// The most steps a blocked sweep fuses (module docs, Fused steps): set for measurements, else `auto`.
	fused: Option<u32>,
	/// The most steps a blocked sweep fuses for the scene's sub-steps, scheme and rim ([`fused_by_side`]).
	auto: u32,
	/// Every step runs its sub-steps one by one through the passes (module docs, Fused steps).
	unblocked: bool,
	/// The probe cells of a fused sweep by padded cell, with their place in a reading and whether they read the
	/// intensity; and its readings, one after another.
	probe_cells: Vec<(usize, u32, bool)>,
	readings: Vec<f32>,
	/// The 4th order's rings of `w` rows of a blocked step's bands, kept for the next (blocked.rs).
	rings: Rings,
}

/// The bytes [`CpuEngine`] allocates by the grid of `scene` with `options`: the field, medium and stage planes, the
/// mirror cuts and the CPML's auxiliary fields.
pub fn grid_bytes(scene: &SceneData, options: &EngineOptions) -> u64 {
	let cells = scene.padded_cells() as u64;
	let modified = TimeScheme::of(options.stencil) == TimeScheme::ModifiedEquation;
	let planes = 4 + u64::from(modified) + 2 * u64::from(options.track_intensity);
	let strips = pml::Strips::of(scene);
	let (psi, xi) = (strips.len() as u64, (strips.len() + strips.y_len()) as u64);
	cells * (4 * planes + 1) + 2 * 4 * (psi + xi)
}

impl CpuEngine {
	/// An engine at rest on `scene` with the sub-steps its CFL bound requires, on the best backend this CPU runs
	/// ([`Backend::best`]).
	pub fn new(scene: impl Into<Arc<SceneData>>, options: EngineOptions) -> Self {
		Self::with_backend(scene, options, Backend::best())
	}

	/// An engine at rest on `scene` with the sub-steps its CFL bound requires, on `backend`.
	///
	/// # Panics
	///
	/// When this CPU does not run `backend` ([`Backend::available`]).
	pub fn with_backend(scene: impl Into<Arc<SceneData>>, options: EngineOptions, backend: Backend) -> Self {
		let scene = scene.into();
		let n = sub_steps(options.stencil, scene.max_c2());
		Self::with_backend_and_sub_steps(scene, options, backend, n)
	}

	/// An engine on the best backend taking `n` sub-steps per step whatever the scene; below the CFL bound it diverges.
	/// For tests of the bound.
	pub fn with_sub_steps(scene: impl Into<Arc<SceneData>>, options: EngineOptions, n: u32) -> Self {
		Self::with_backend_and_sub_steps(scene, options, Backend::best(), n)
	}

	/// An engine on `backend` taking `n` sub-steps per step whatever the scene ([`with_sub_steps`](Self::with_sub_steps)).
	///
	/// # Panics
	///
	/// When this CPU does not run `backend` ([`Backend::available`]).
	pub fn with_backend_and_sub_steps(
		scene: impl Into<Arc<SceneData>>,
		options: EngineOptions,
		backend: Backend,
		n: u32,
	) -> Self {
		assert!(backend.available(), "this CPU does not run the {} backend", backend.name());
		let host = Host::new(scene.into(), options, n);
		let mut engine = Self {
			host,
			backend,
			uniform: UniformRuns::default(),
			mur: MurRows::default(),
			tables: Vec::new(),
			driven: Vec::new(),
			tilings: Vec::new(),
			fused: None,
			auto: 1,
			unblocked: false,
			probe_cells: Vec::new(),
			readings: Vec::new(),
			rings: Rings::default(),
		};
		engine.scene_loaded();
		engine
	}

	/// The backend the engine runs on.
	pub fn backend(&self) -> Backend {
		self.backend
	}

	/// Fuses at most `k` steps into one sweep from now on, whatever the scene (module docs, Fused steps); for
	/// measurements.
	///
	/// # Panics
	///
	/// When `k` is 0.
	#[doc(hidden)]
	pub fn set_fused_steps(&mut self, k: u32) {
		assert!(k >= 1, "at least one step a sweep");
		self.fused = Some(k);
	}

	/// Steps from now on one step at a time and each step's sub-steps one by one through the passes, never in a blocked
	/// sweep, while `on` (module docs, Fused steps); the plainest order, for tests to compare the sweeps with.
	#[doc(hidden)]
	pub fn set_unblocked(&mut self, on: bool) {
		self.unblocked = on;
	}

	/// [`WaveEngine::fused_steps`]: the most steps a sweep fuses, by default the most whose trapezoids' sides stay
	/// within [`FUSED_SIDE`] rows ([`FUSED_STEPS`] at most; module docs, Fused steps); 1 when unblocked.
	pub fn fused_steps(&self) -> u32 {
		if self.unblocked { 1 } else { self.fused.unwrap_or(self.auto) }
	}

	/// [`WaveEngine::advance_steps`], which cannot fail here: `n` steps, in sweeps of up to
	/// [`fused_steps`](Self::fused_steps) steps each (module docs, Fused steps).
	pub fn advance_steps(&mut self, n: u32, probes: bool) {
		let mut left = n;
		while left > 0 {
			let k = left.min(self.fused_steps());
			on_backend!(self.backend, B => self.steps_on(self.kernels::<B>(), k as usize, probes));
			self.host.shared.time += k;
			left -= k;
		}
	}

	/// Rebuilds the uniform runs from the medium the host loaded.
	fn scene_loaded(&mut self) {
		let h = &self.host;
		self.uniform = UniformRuns::of(&h.c2, &h.d, h.stride, h.gw());
		self.mur = MurRows::of(&h.mur, h.stride, h.shared.scene.grid_height() as usize);
		self.auto = fused_by_side(h.shared.sub_steps, h.shared.scheme, h.pml.is_some());
		// The passes take `w` as 0 beyond the cells they wrote in earlier steps; a blocked step writes it on an area
		// grown by its sub-steps, which a medium with fewer sub-steps would not cover.
		self.host.w.fill(0.0);
	}

	/// The leapfrog pass of the 9-point stencil: writes the next field over `prev`, and the intensity stages when
	/// `intensity`.
	fn leapfrog_pass<B: RowKernels>(&mut self, kernels: B, intensity: bool) {
		let h = &mut self.host;
		let (s, area, xs) = (h.stride, &h.damage, h.damage.x.clone());
		let line = NineLine::of((h.stride, &h.c2, &h.d, &h.cuts, h.mirrors), &self.uniform, kernels, &xs);
		let (u, c2, cuts) = (&h.u, &h.c2, &h.cuts);
		let stages = Host::stages(&mut h.s1, &mut h.s2, h.alpha, intensity);
		let mut rim = h.pml.as_mut().and_then(|p| p.begin(0, u, Medium { cuts, c2 }, 1.0, area));
		grid_rows(
			kernels,
			&mut h.prev,
			stages,
			s,
			area,
			rim.as_mut(),
			#[cfg_attr(cpu_wide, inline(always))]
			|r, next, stages| line.row(r, &u[(r - 1) * s..(r + 2) * s], next, stages),
		);
	}

	/// The modified equation's first pass: `w = c2 L u` on the damaged cells.
	fn inner_pass<B: RowKernels>(&mut self, kernels: B) {
		let h = &mut self.host;
		let (s, u, area) = (h.stride, &h.u, &h.damage);
		let line = DrpLine::of((h.stride, &h.c2, &h.d, &h.cuts, h.mirrors), h.axis, &self.uniform, kernels, &area.x);
		let medium = Medium { cuts: &h.cuts, c2: &h.c2 };
		let mut rim = h.pml.as_mut().and_then(|p| p.begin(0, u, medium, 1.0, area));
		grid_rows(
			kernels,
			&mut h.w,
			None,
			s,
			area,
			rim.as_mut(),
			#[cfg_attr(cpu_wide, inline(always))]
			|r, w, _| line.lap(r, &plane_rows(u, r, s), w),
		);
	}

	/// The modified equation's second pass: writes the next field over `prev` from `u` and the plane `w`, and the
	/// intensity stages when `intensity`.
	fn outer_pass<B: RowKernels>(&mut self, kernels: B, intensity: bool) {
		let h = &mut self.host;
		let (s, gw, u, w, area) = (h.stride, h.gw(), &h.u, &h.w, &h.damage);
		let line = DrpLine::of((h.stride, &h.c2, &h.d, &h.cuts, h.mirrors), h.axis, &self.uniform, kernels, &area.x);
		let stages = Host::stages(&mut h.s1, &mut h.s2, h.alpha, intensity);
		let mut rim = h.pml.as_mut().and_then(|p| p.begin(1, w, Medium { cuts: &h.cuts, c2: &h.c2 }, INV12, area));
		grid_rows(
			kernels,
			&mut h.prev,
			stages,
			s,
			area,
			rim.as_mut(),
			#[cfg_attr(cpu_wide, inline(always))]
			|r, next, stages| line.outer(r, &u[r * s + PAD..][..gw], &plane_rows(w, r, s), next, stages),
		);
	}

	/// The engine's backend as its type, `B` being that type: an engine is only built on a backend this CPU runs.
	fn kernels<B: RowKernels>(&self) -> B {
		B::new().expect("an engine is built on a backend this CPU runs")
	}

	/// One step (module docs, Blocked steps) on the engine's backend.
	fn step(&mut self) {
		on_backend!(self.backend, B => self.steps_on(self.kernels::<B>(), 1, false));
	}

	/// `k` steps, a probe reading queued after each when `probes`; the caller counts them. Their sub-steps fused into
	/// one sweep over the rows when there are two or more and anything to compute, else one by one.
	fn steps_on<B: RowKernels>(&mut self, kernels: B, k: usize, probes: bool) {
		let n = self.host.shared.sub_steps as usize;
		let levels = k * n;
		if levels < 2 || self.unblocked {
			debug_assert!(k == 1, "an unblocked engine fuses no steps");
			self.sub_steps_in_turn();
			if probes {
				self.host.queue_probes();
			}
			return;
		}
		let h = &mut self.host;
		let len = h.values.len();
		self.tables.resize_with(levels, Vec::new);
		for (l, table) in self.tables[..levels].iter_mut().enumerate() {
			table.clear();
			table.resize(len, 0.0);
			h.shared.source_values_ahead(table, (l / n) as u32, (l % n) as u32);
		}
		// The cells a source drives on any sub-step, ascending, and the damage with them.
		let (s, groups) = (h.stride, &h.groups);
		self.driven.clear();
		let mut known = h.damage.clone();
		for ((&cell, &(start, count)), _) in groups.cells.iter().zip(&groups.runs).zip(0..) {
			let run = &groups.entries[start as usize..(start + count) as usize];
			if self.tables[..levels].iter().any(|t| run.iter().any(|&e| live(t[e as usize]))) {
				self.driven.push((cell, start, count));
				known.include(cell, s);
			}
		}
		self.driven.sort_unstable_by_key(|&(cell, ..)| cell);
		if known.is_empty() {
			for l in 0..levels {
				self.host.values.copy_from_slice(&self.tables[l]);
				let intensity = self.host.shared.accumulates((l % n) as u32);
				self.sub_step_on(kernels, intensity);
				if probes && (l + 1) % n == 0 {
					self.host.queue_probes();
				}
			}
			return;
		}
		self.blocked_steps(kernels, k, known, probes);
	}

	/// The sub-steps of `k` steps fused into one sweep (blocked.rs) on the damage with the steps' driven cells, `known`,
	/// grown by their reach, recording the probes after each step when `probes`; then the damage takes in what they
	/// made non-zero.
	fn blocked_steps<B: RowKernels>(&mut self, kernels: B, k: usize, known: Damage, probes: bool) {
		let h = &mut self.host;
		let (s, gw, gh, scheme) = (h.stride, h.gw(), h.shared.scene.grid_height() as usize, h.shared.scheme);
		let (sub, n) = (h.shared.sub_steps as usize, k * h.shared.sub_steps as usize);
		// The 4th order's sub-step is two passes, each reaching `reach`.
		let passes = if scheme == TimeScheme::Leapfrog { 1 } else { 2 };
		let area = known.grown(n * passes * h.reach, gw, gh);
		let intensity = h.shared.options.track_intensity && !h.s1.is_empty();
		// Each step's readings start as the ones now, which hold on the rows the sweep does not update.
		let width = if probes {
			h.probe_values(&mut self.readings);
			let width = self.readings.len();
			self.readings.resize(k * width, 0.0);
			for step in 1..k {
				self.readings.copy_within(..width, step * width);
			}
			self.probe_cells.clear();
			self.probe_cells.extend(h.probe_cells().zip(0..).map(|((cell, intensity), slot)| (cell, slot, intensity)));
			self.probe_cells.sort_unstable_by_key(|&(cell, ..)| cell);
			Some(width)
		} else {
			None
		};
		let log = width.map(|width| ProbeLog {
			cells: &self.probe_cells,
			out: Shared::of(&mut self.readings),
			width,
			scale: h.shared.options.intensity_scale as f32,
		});
		let stages = intensity.then(|| (Shared::of(&mut h.s1), Shared::of(&mut h.s2), h.alpha));
		let planes = [Shared::of(&mut h.u), Shared::of(&mut h.prev)];
		let medium = (h.stride, &h.c2[..], &h.d[..], &h.cuts[..], h.mirrors);
		let line = match scheme {
			TimeScheme::Leapfrog => Line::Nine(NineLine::of(medium, &self.uniform, kernels, &area.x)),
			TimeScheme::ModifiedEquation => {
				Line::Drp(DrpLine::of(medium, h.axis, &self.uniform, kernels, &area.x), Shared::of(&mut h.w))
			}
		};
		let pass = |scale| XPass { xs: area.x.clone(), medium: Medium { cuts: &h.cuts, c2: &h.c2 }, scale };
		let rim = h.pml.as_mut().map(|pml| {
			let (tables, [first, second]) = pml.row_wise_pair();
			let z = Shared::of(first.z);
			let mut sets = vec![RimSet::of(first, pass(1.0))];
			if scheme == TimeScheme::ModifiedEquation {
				sets.push(RimSet::of(second, pass(INV12)));
			}
			Rim { tables, z, sets }
		});
		let tiling = match self.tilings.iter().position(|t| t.is(n, scheme, rim.is_some())) {
			Some(at) => self.tilings.swap_remove(at),
			None => Tiling::new(n, scheme, rim.is_some()),
		};
		let step = Step {
			kernels,
			s,
			gw,
			gh,
			n,
			sub,
			planes,
			stages,
			alpha: h.alpha,
			line,
			d: &h.d,
			cuts: &h.cuts,
			area: area.y.clone(),
			tables: &self.tables,
			driven: &self.driven,
			entries: &h.groups.entries,
			mur: &self.mur,
			rim,
			probes: log,
		};
		step.run(&tiling, BANDS_PER_THREAD * rayon::current_num_threads(), &mut self.rings);
		// A few tilings: one per batch length a caller uses, for each scheme and sub-step count; any is evicted past 7.
		self.tilings.truncate(7);
		self.tilings.push(tiling);
		if n % 2 == 1 {
			std::mem::swap(&mut h.u, &mut h.prev);
		}
		h.values.copy_from_slice(&self.tables[n - 1]);
		if let Some(width) = width {
			for step in 0..k {
				h.probe_queue().push(&self.readings[step * width..][..width]);
			}
		}
		// `w` too, as the inner pass takes it in: a -0 there is not +0 (module docs, Damage).
		let damage = extent(&h.u, s, &area, known);
		let damage = if h.w.is_empty() { damage } else { extent(&h.w, s, &area, damage) };
		h.damage = extent(&h.prev, s, &area, damage);
		debug_assert!(h.damage_holds(), "a cell outside the damage {:?} is not +0", h.damage);
	}

	/// One sub-step on the engine's backend ([`sub_step_on`](Self::sub_step_on)).
	fn sub_step(&mut self, intensity: bool) {
		on_backend!(self.backend, B => self.sub_step_on(self.kernels::<B>(), intensity));
	}

	/// One sub-step; the next field ends in `u`. Each pass runs on the damage grown by its reach; the damage then
	/// takes in what the pass and the forcing made non-zero (module docs, Damage).
	fn sub_step_on<B: RowKernels>(&mut self, kernels: B, intensity: bool) {
		let known = self.host.damage.clone();
		let area = match self.host.shared.scheme {
			TimeScheme::Leapfrog => {
				self.host.damage = self.host.reached(&known);
				self.leapfrog_pass(kernels, intensity);
				self.host.force_next(intensity);
				std::mem::replace(&mut self.host.damage, known)
			}
			TimeScheme::ModifiedEquation => {
				// `w` is written on the inner area, which a later sub-step's covers, as the damage only grows.
				self.host.damage = self.host.reached(&known);
				self.inner_pass(kernels);
				self.host.force_w();
				let inner = std::mem::replace(&mut self.host.damage, known.clone());
				let excited = extent(&self.host.w, self.host.stride, &inner, known);
				self.host.damage = self.host.reached(&excited);
				self.outer_pass(kernels, intensity);
				std::mem::replace(&mut self.host.damage, excited)
			}
		};
		self.host.end_sub_step(&area);
	}
}

// The inherent calls below take `&mut self` like the trait's, so they win method resolution on the concrete type: its
// callers get no `Result`.
impl CpuEngine {
	/// Sets the field now and one sub-step before, each grid-sized (no padding), row-major.
	/// For analytic tests that start from a given state.
	pub fn set_field(&mut self, now: &[f32], before: &[f32]) {
		self.host.set_field(now, before);
	}

	/// The padded field buffer (see [`SceneData::stride`]).
	pub fn field_padded(&self) -> &[f32] {
		&self.host.u
	}

	/// The intensity's two EMA stages over the padded grid, empty when it is not tracked. For tests.
	#[doc(hidden)]
	pub fn intensity_stages(&self) -> (&[f32], &[f32]) {
		(&self.host.s1, &self.host.s2)
	}

	/// [`WaveEngine::advance_one_step`], which cannot fail here.
	pub fn advance_one_step(&mut self) {
		self.step();
		self.host.shared.time += 1;
	}

	/// The step's sub-steps one after the other, each with its source table.
	fn sub_steps_in_turn(&mut self) {
		for k in 0..self.host.shared.sub_steps {
			self.host.shared.source_values(&mut self.host.values, k);
			let intensity = self.host.shared.accumulates(k);
			self.sub_step(intensity);
		}
	}

	/// [`WaveEngine::sample_probes`], which cannot fail here.
	pub fn sample_probes(&mut self, out: &mut ProbeReadings) {
		self.host.sample_probes(out);
	}

	/// Converts the canvas after the last step into `out`'s planes in `shape`, their allocations reused.
	pub fn capture_into(&self, shape: &FrameShape, out: &mut Snapshot) {
		self.host.capture_into(shape, out);
	}

	/// [`WaveEngine::capture_frame`], which cannot fail here.
	pub fn capture_frame(&mut self, shape: &FrameShape) -> Snapshot {
		let mut out = Snapshot::new(*shape);
		self.host.capture_into(shape, &mut out);
		out
	}

	/// [`WaveEngine::prepare_frame`], which cannot fail here.
	pub fn prepare_frame(&mut self) -> (Vec<f32>, Vec<f32>) {
		self.host.prepare_frame()
	}
}

impl WaveEngine for CpuEngine {
	fn name(&self) -> &'static str {
		self.backend.engine_name()
	}

	fn scene(&self) -> &SceneData {
		&self.host.shared.scene
	}

	fn options(&self) -> &EngineOptions {
		&self.host.shared.options
	}

	fn time(&self) -> u32 {
		self.host.shared.time
	}

	fn sub_steps(&self) -> u32 {
		self.host.shared.sub_steps
	}

	fn reset(&mut self) {
		self.host.reset();
	}

	fn set_track_intensity(&mut self, on: bool) {
		self.host.set_track_intensity(on);
	}

	fn set_emitters(&mut self, emitters: &[Emitter]) {
		self.host.set_emitters(emitters);
	}

	fn set_probes(&mut self, probes: &[ProbeCells]) {
		self.host.set_probes(probes);
	}

	fn try_replace_scene(&mut self, scene: &Arc<SceneData>) -> bool {
		let replaced = self.host.try_replace_scene(scene);
		if replaced {
			self.scene_loaded();
		}
		replaced
	}

	fn advance_one_step(&mut self) -> Result<(), EngineError> {
		Self::advance_one_step(self);
		Ok(())
	}

	fn fused_steps(&self) -> u32 {
		Self::fused_steps(self)
	}

	fn advance_steps(&mut self, n: u32, probes: bool) -> Result<(), EngineError> {
		Self::advance_steps(self, n, probes);
		Ok(())
	}

	fn sample_probes(&mut self, out: &mut ProbeReadings) -> Result<(), EngineError> {
		self.host.sample_probes(out);
		Ok(())
	}

	fn queue_probes(&mut self) -> Result<(), EngineError> {
		self.host.queue_probes();
		Ok(())
	}

	fn pending_probes(&mut self) -> Result<PendingProbes, EngineError> {
		Ok(PendingProbes::done(self.host.probe_queue().end()))
	}

	fn drain_probes(&mut self, pending: PendingProbes, f: &mut dyn FnMut(&ProbeReadings)) -> Result<(), EngineError> {
		self.host.probe_queue().drain(pending.end(), f);
		Ok(())
	}

	fn begin_capture(&mut self, shape: &FrameShape, mut into: Snapshot) -> Result<PendingCapture, EngineError> {
		self.host.capture_into(shape, &mut into);
		Ok(PendingCapture::done(into))
	}

	fn finish_capture(&mut self, pending: PendingCapture) -> Result<Snapshot, EngineError> {
		Ok(pending.snapshot)
	}

	fn prepare_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), EngineError> {
		Ok(self.host.prepare_frame())
	}
}

/// The cap on the steps a blocked sweep fuses by default, chosen by [`FUSED_SIDE`] (module docs, Fused steps;
/// docs/measurements.md).
pub const FUSED_STEPS: u32 = 8;

/// The widest side of a sweep's trapezoids, in rows, up to which the engine fuses more steps by default (module docs,
/// Fused steps; docs/measurements.md).
pub const FUSED_SIDE: usize = 32;

/// The most steps of `sub` sub-steps of `scheme`, with or without a rim, a sweep fuses by default: the most, up to
/// [`FUSED_STEPS`], whose trapezoids' sides ([`Tiling`]) stay within [`FUSED_SIDE`] rows, at least one.
fn fused_by_side(sub: u32, scheme: TimeScheme, rim: bool) -> u32 {
	let fits = |k: u32| Tiling::new((k * sub) as usize, scheme, rim).side() <= FUSED_SIDE;
	(2..=FUSED_STEPS).rev().find(|&k| fits(k)).unwrap_or(1)
}

/// The bands of a blocked step per thread of the rayon pool (blocked.rs).
const BANDS_PER_THREAD: usize = 1;

/// What the 9-point leapfrog update of a grid row reads besides the field: the medium, its uniform runs and cuts, the
/// backend, and the columns to update.
#[derive(Clone, Copy)]
struct NineLine<'a, B> {
	s: usize,
	gw: usize,
	kernels: B,
	uniform: &'a UniformRuns,
	c2: &'a [f32],
	d: &'a [f32],
	cuts: &'a [u8],
	mirrors: bool,
	xs: &'a Range<usize>,
}

impl<'a, B: RowKernels> NineLine<'a, B> {
	/// The update on a medium of rows of `s` with its `c2`, `d`, cuts and whether any cell is cut (a host's fields,
	/// borrowed one by one so its planes stay free).
	fn of(
		(s, c2, d, cuts, mirrors): (usize, &'a [f32], &'a [f32], &'a [u8], bool),
		uniform: &'a UniformRuns,
		kernels: B,
		xs: &'a Range<usize>,
	) -> Self {
		Self { s, gw: s - 2 * PAD, kernels, uniform, c2, d, cuts, mirrors, xs }
	}

	/// Writes the next field over `next`, the `gw` grid cells of padded row `r` holding the field one sub-step before,
	/// on the columns `xs`, and advances `stages` (the row's), when given, by the values written. `u` holds the
	/// padded rows `r - 1` to `r + 1` of the field.
	#[cfg_attr(cpu_wide, inline(always))]
	fn row(&self, r: usize, u: &[f32], next: &mut [f32], mut stages: Option<Intensity<'_>>) {
		let Self { s, gw, kernels, uniform, c2, d, cuts, mirrors, xs } = *self;
		let (at, runs) = (r * s + PAD, uniform.row(r - PAD));
		let (c2, d) = (&c2[at..][..gw], &d[at..][..gw]);
		// Grid column x of the rows above, of and below is at x + 1 of these; the row's own at `s + PAD`.
		let rows: [&[f32]; 3] = std::array::from_fn(|k| &u[k * s + PAD - 1..][..gw + 2]);
		let centre = s + PAD;
		// The cells of `span` with no cut, a uniform run's with its constants.
		let uncut = forced(
			#[cfg_attr(cpu_wide, inline(always))]
			|span: Range<usize>, next: &mut [f32], stages: &mut Option<Intensity<'_>>| {
				medium_spans(
					runs,
					c2,
					d,
					span,
					#[cfg_attr(cpu_wide, inline(always))]
					|piece, medium| {
						let taps = rows.map(|row| &row[piece.start..piece.end + 2]);
						let medium = match medium {
							Some((c2, d)) => RowMedium::Uniform { c2, d },
							None => RowMedium::Planes { c2: &c2[piece.clone()], d: &d[piece.clone()] },
						};
						let stages = stages.as_mut().map(|st| st.span(piece.start, piece.len()));
						kernels.nine(taps, medium, &mut next[piece], stages);
					},
				);
			},
		);
		let Some(cuts) = row_cuts(cuts, mirrors, at, gw, xs) else {
			return uncut(xs.clone(), next, &mut stages);
		};
		cut_spans(
			cuts,
			xs.clone(),
			#[cfg_attr(cpu_wide, inline(always))]
			|span, cut| {
				if !cut {
					return uncut(span, next, &mut stages);
				}
				for x in span {
					let v = u[centre + x];
					let lap = nine_point(u, centre + x, s, cuts[x]);
					next[x] = v + ((v - next[x]) * d[x] + lap * c2[x] * centred(d[x]));
					if let Some(st) = stages.as_mut() {
						advance(&mut st.s1[x], &mut st.s2[x], next[x], st.alpha, 1.0 - st.alpha);
					}
				}
			},
		);
	}
}

/// What the 4th order's two updates of a grid row read besides the fields: the medium, its uniform runs and cuts, the
/// stencil's weights, the backend, and the columns to update.
#[derive(Clone, Copy)]
struct DrpLine<'a, B> {
	s: usize,
	gw: usize,
	kernels: B,
	axis: [f32; 4],
	uniform: &'a UniformRuns,
	c2: &'a [f32],
	d: &'a [f32],
	cuts: &'a [u8],
	mirrors: bool,
	xs: &'a Range<usize>,
}

impl<'a, B: RowKernels> DrpLine<'a, B> {
	/// The updates on a medium of rows of `s` with its `c2`, `d`, cuts and whether any cell is cut, and the stencil's
	/// axis weights.
	fn of(
		(s, c2, d, cuts, mirrors): (usize, &'a [f32], &'a [f32], &'a [u8], bool),
		axis: [f32; 4],
		uniform: &'a UniformRuns,
		kernels: B,
		xs: &'a Range<usize>,
	) -> Self {
		Self { s, gw: s - 2 * PAD, kernels, axis, uniform, c2, d, cuts, mirrors, xs }
	}

	/// Hands `f` the pieces of the cells `span` of padded row `r` (all uncut), each with its medium: a uniform run's
	/// constants, or the planes' values.
	#[cfg_attr(cpu_wide, inline(always))]
	fn pieces(&self, r: usize, span: Range<usize>, mut f: impl FnMut(Range<usize>, RowMedium<'_>)) {
		let at = r * self.s + PAD;
		let (c2, d) = (&self.c2[at..][..self.gw], &self.d[at..][..self.gw]);
		medium_spans(
			self.uniform.row(r - PAD),
			c2,
			d,
			span,
			#[cfg_attr(cpu_wide, inline(always))]
			|piece, medium| {
				let medium = match medium {
					Some((c2, d)) => RowMedium::Uniform { c2, d },
					None => RowMedium::Planes { c2: &c2[piece.clone()], d: &d[piece.clone()] },
				};
				f(piece, medium);
			},
		);
	}

	/// Writes `w = c2 L u` over `w`, the `gw` grid cells of padded row `r`, on the columns `xs`, from the 7 padded rows
	/// `u` around it.
	#[cfg_attr(cpu_wide, inline(always))]
	fn lap(&self, r: usize, u: &Rows<'_>, w: &mut [f32]) {
		let (at, gw, kernels, axis) = (r * self.s + PAD, self.gw, self.kernels, &self.axis);
		let uncut = forced(
			#[cfg_attr(cpu_wide, inline(always))]
			|span: Range<usize>, w: &mut [f32]| {
				self.pieces(
					r,
					span,
					#[cfg_attr(cpu_wide, inline(always))]
					|piece, medium| kernels.lap(span_rows(u, &piece), axis, medium, &mut w[piece]),
				);
			},
		);
		let Some(cuts) = row_cuts(self.cuts, self.mirrors, at, gw, self.xs) else { return uncut(self.xs.clone(), w) };
		let c2 = &self.c2[at..][..gw];
		cut_spans(
			cuts,
			self.xs.clone(),
			#[cfg_attr(cpu_wide, inline(always))]
			|span, cut| {
				if !cut {
					return uncut(span, w);
				}
				for x in span {
					w[x] = c2[x] * drp(u, x, axis, cuts[x]);
				}
			},
		);
	}

	/// Writes the next field over `next`, the `gw` grid cells of padded row `r` holding the field one sub-step before,
	/// on the columns `xs`, from the row's field `u` (its `gw` grid cells) and the 7 padded rows `w` around it; advances
	/// `stages` (the row's), when given, by the values written.
	#[cfg_attr(cpu_wide, inline(always))]
	fn outer(&self, r: usize, u: &[f32], w: &Rows<'_>, next: &mut [f32], mut stages: Option<Intensity<'_>>) {
		let (at, gw, kernels, axis) = (r * self.s + PAD, self.gw, self.kernels, &self.axis);
		let uncut = forced(
			#[cfg_attr(cpu_wide, inline(always))]
			|span: Range<usize>, next: &mut [f32], stages: &mut Option<Intensity<'_>>| {
				self.pieces(
					r,
					span,
					#[cfg_attr(cpu_wide, inline(always))]
					|piece, medium| {
						let stages = stages.as_mut().map(|st| st.span(piece.start, piece.len()));
						kernels.outer(&u[piece.clone()], span_rows(w, &piece), axis, medium, &mut next[piece], stages);
					},
				);
			},
		);
		let Some(cuts) = row_cuts(self.cuts, self.mirrors, at, gw, self.xs) else {
			return uncut(self.xs.clone(), next, &mut stages);
		};
		let (c2, d, wc) = (&self.c2[at..][..gw], &self.d[at..][..gw], &w[3][PAD..][..gw]);
		cut_spans(
			cuts,
			self.xs.clone(),
			#[cfg_attr(cpu_wide, inline(always))]
			|span, cut| {
				if !cut {
					return uncut(span, next, &mut stages);
				}
				for x in span {
					let (lw, c) = (drp(w, x, axis, cuts[x]), u[x]);
					next[x] = c + ((c - next[x]) * d[x] + (wc[x] + c2[x] * lw * INV12) * centred(d[x]));
					if let Some(st) = stages.as_mut() {
						advance(&mut st.s1[x], &mut st.s2[x], next[x], st.alpha, 1.0 - st.alpha);
					}
				}
			},
		);
	}
}

/// Runs `row(r, cells, stages)` on the `gw` grid cells of every padded row `r` of `out` (rows of `s`) that `area`
/// covers, one row per task on the rayon pool, through the backend's row entry; `row` writes `area`'s columns and, when
/// given the row's intensity stages, advances those of the cells it writes. Then runs the rim's psi of x and
/// correction on the row, if given (pml.rs, `XShared::row`), as a pass over the grid followed by one over the rim
/// would.
fn grid_rows(
	kernels: impl RowKernels,
	out: &mut [f32],
	stages: Option<Stages<'_>>,
	s: usize,
	area: &Damage,
	rim: Option<&mut XPart<'_>>,
	row: impl Fn(usize, &mut [f32], Option<Intensity<'_>>) + Sync,
) {
	if area.is_empty() {
		return;
	}
	let (first, span, n) = (PAD + area.y.start, (PAD + area.y.start) * s..(PAD + area.y.end) * s, area.y.len());
	let gw = s - 2 * PAD;
	let (s1, s2, alpha) = match stages {
		Some(Stages { s1, s2, alpha }) => (Some(&mut s1[span.clone()]), Some(&mut s2[span.clone()]), alpha),
		None => (None, None, 0.0),
	};
	let (shared, rim_rows) = match rim {
		Some(part) => {
			let (shared, chunks) = part.chunks(1);
			(Some(shared), Either::Left(chunks.map(Some)))
		}
		None => (None, Either::Right((0..n).into_par_iter().map(|_| None))),
	};
	out[span]
		.par_chunks_mut(s)
		.zip(chunks_or_none(s1, s, n))
		.zip(chunks_or_none(s2, s, n))
		.zip(rim_rows)
		.enumerate()
		.for_each_init(
			|| shared.map(XShared::scratch),
			|scratch, (k, (((line, s1), s2), rim_rows))| {
				let (r, cells) = (first + k, &mut line[PAD..][..gw]);
				let stages = match (s1, s2) {
					(Some(s1), Some(s2)) => {
						Some(Intensity { s1: &mut s1[PAD..][..gw], s2: &mut s2[PAD..][..gw], alpha })
					}
					_ => None,
				};
				kernels.run_row(
					#[cfg_attr(cpu_wide, inline(always))]
					|| {
						row(r, cells, stages);
						if let (Some(shared), Some(mut rows), Some(scratch)) = (shared, rim_rows, scratch.as_mut()) {
							shared.row(r - PAD, &mut rows, 0, cells, scratch);
						}
					},
				);
			},
		);
}

/// `f` itself: a closure written as the argument of a call may take `#[inline(always)]`, one bound by `let` may not.
/// The row functions' closures are inlined by force where `cpu_wide` is set (build.rs), so the kernels compile into
/// the backend's row entry ([`RowKernels::run_row`]) and take its instruction set: out of line they kept the
/// baseline's.
#[inline(always)]
fn forced<F>(f: F) -> F {
	f
}

/// The seven rows a DRP kernel reads for the grid cells `span`: from 3 cells left of it to 3 right of it.
#[inline]
fn span_rows<'a>(rows: &Rows<'a>, span: &Range<usize>) -> Rows<'a> {
	// Grid column x is at PAD + x, so 3 cells left of the span's first is at its own index.
	rows.map(|row| &row[span.start..PAD + span.end + 3])
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::types::{ImageSource, Snapshot};
	use crate::model::{Edges, Layer, PulseShape, Scene, Stencil};

	fn options(stencil: Stencil) -> EngineOptions {
		EngineOptions {
			stencil,
			source_frequency_scale: 2.0,
			image_source_scale: 1.0,
			time_decay: 10.0,
			intensity_scale: 1.0,
			track_intensity: true,
		}
	}

	fn emitter(x: i32, y: i32) -> Emitter {
		Emitter {
			x: f64::from(x),
			y: f64::from(y),
			level: 100.0,
			amplitude: 1.0,
			phase_radians: 0.0,
			pulsed: false,
			shape: PulseShape::Gaussian,
			on_width_cycles: 10,
			off_width_cycles: 10,
			off_width_base: 10,
			randomize_off_width: 0,
			detach_after_step: None,
			rng_seed: std::num::NonZeroU32::MIN,
		}
	}

	fn engine(edges: Edges, rim: i32, stencil: Stencil) -> CpuEngine {
		let scene = SceneData::blank(40, 30, &Scene { damping_edge: rim, edges, ..Scene::default() });
		let mut e = CpuEngine::with_backend(scene, options(stencil), Backend::Generic);
		e.set_emitters(&[emitter(20, 15), emitter(-1, 3), emitter(40, 0)]);
		e
	}

	#[test]
	fn a_cpu_capture_is_ready_at_once() {
		use std::future::Future;
		use std::task::{Context, Poll, Waker};
		let mut e = engine(Edges::Open, 0, Stencil::NinePoint);
		e.advance_one_step();
		let shape = FrameShape {
			canvas_width: 40,
			canvas_height: 30,
			divisor: 1,
			format: crate::engine::SampleFormat::Half,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: false,
		};
		let pending = e.begin_capture(&shape, Snapshot::new(shape)).unwrap();
		let mut ready = std::pin::pin!(pending.ready());
		assert_eq!(ready.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Ready(()));
		assert_eq!(e.finish_capture(pending).unwrap().shape, shape);
	}

	#[test]
	fn outside_emitters_are_dropped() {
		assert_eq!(engine(Edges::Open, 0, Stencil::NinePoint).host.shared.sources.emitters.len(), 1);
	}

	#[test]
	fn reflective_ring_stays_zero_and_intensity_accumulates() {
		let mut e = engine(Edges::Reflective, 0, Stencil::FourthOrder);
		for _ in 0..80 {
			e.advance_one_step();
		}
		let (s, rows) = (e.host.stride, e.host.u.len() / e.host.stride);
		let padding = (0..rows * s).filter(|&i| {
			let (x, y) = (i % s, i / s);
			x < PAD || y < PAD || x >= s - PAD || y >= rows - PAD
		});
		assert!(padding.into_iter().all(|i| e.host.u[i] == 0.0 && e.host.prev[i] == 0.0 && e.host.w[i] == 0.0));
		let (field, intensity) = e.prepare_frame();
		assert!(field.iter().any(|&v| v != 0.0));
		assert!(intensity.iter().all(|&v| v >= 0.0) && intensity.iter().any(|&v| v > 0.0));
		assert_eq!(e.time(), 80);
	}

	#[test]
	fn reset_and_replay_are_identical() {
		let mut e = engine(Edges::Open, -8, Stencil::NinePoint);
		let run = |e: &mut CpuEngine| {
			for _ in 0..50 {
				e.advance_one_step();
			}
			e.prepare_frame()
		};
		let a = run(&mut e);
		e.reset();
		assert_eq!(a, run(&mut e));
	}

	#[test]
	fn probes_read_the_field_and_the_intensity() {
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		e.set_probes(&[
			ProbeCells { layer: Layer::Field, cells: vec![(22, 15)] },
			ProbeCells { layer: Layer::Intensity, cells: vec![(22, 15), (22, 15)] },
		]);
		for _ in 0..30 {
			e.advance_one_step();
		}
		let mut r = ProbeReadings::default();
		e.sample_probes(&mut r);
		let (field, intensity) = e.prepare_frame();
		assert_eq!(r.probe(0), [field[15 * 40 + 22]]);
		assert_eq!(r.probe(1), [intensity[15 * 40 + 22]; 2]);
	}

	#[test]
	fn pending_probes_are_ready_at_once_and_drain_in_step_order() {
		use std::future::Future;
		use std::task::{Context, Poll, Waker};
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		e.set_probes(&[
			ProbeCells { layer: Layer::Field, cells: vec![(22, 15)] },
			ProbeCells { layer: Layer::Intensity, cells: vec![(10, 5), (11, 5), (12, 5)] },
		]);
		let (mut sampled, mut drained, mut r) = (Vec::new(), Vec::new(), ProbeReadings::default());
		let mut held = None;
		for step in 1..=40 {
			e.advance_one_step();
			e.sample_probes(&mut r);
			sampled.push(r.clone());
			e.queue_probes().unwrap();
			if step % 15 == 0 || step == 40 {
				if let Some((pending, covered)) = held.take() {
					e.drain_probes(pending, &mut |r| drained.push(r.clone())).unwrap();
					assert_eq!(drained.len(), covered, "the readings queued after the token stay");
				}
				let pending = e.pending_probes().unwrap();
				let mut ready = std::pin::pin!(pending.ready());
				assert_eq!(ready.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Ready(()));
				held = Some((pending, step));
			}
		}
		let (pending, _) = held.unwrap();
		e.drain_probes(pending, &mut |r| drained.push(r.clone())).unwrap();
		assert_eq!(drained, sampled);
	}

	#[test]
	fn a_token_taken_before_a_reset_drains_nothing_after_it() {
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		e.set_probes(&[ProbeCells { layer: Layer::Field, cells: vec![(22, 15)] }]);
		for _ in 0..10 {
			e.advance_one_step();
			e.queue_probes().unwrap();
		}
		let stale = e.pending_probes().unwrap();
		e.reset();
		for _ in 0..5 {
			e.advance_one_step();
			e.queue_probes().unwrap();
		}
		e.drain_probes(stale, &mut |_| panic!("a reading from before the reset")).unwrap();
		let (pending, mut drained) = (e.pending_probes().unwrap(), 0);
		e.drain_probes(pending, &mut |_| drained += 1).unwrap();
		assert_eq!(drained, 5);
	}

	#[test]
	fn capture_holds_the_canvas() {
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		for _ in 0..30 {
			e.advance_one_step();
		}
		let shape = FrameShape {
			canvas_width: 40,
			canvas_height: 30,
			divisor: 1,
			format: crate::engine::SampleFormat::Half,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: true,
		};
		let snap = e.capture_frame(&shape);
		let (field, intensity) = e.prepare_frame();
		assert_eq!(snap.step, 29);
		let h = snap.halves(Layer::Field);
		assert!(h.iter().zip(&field).all(|(a, &b)| *a == half::f16::from_f32(b)));
		let i = snap.halves(Layer::Intensity);
		assert!(i.iter().zip(&intensity).all(|(a, &b)| *a == half::f16::from_f32(b.sqrt())));
	}

	#[test]
	fn replace_refuses_another_grid() {
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		let other = SceneData::blank(41, 30, &Scene { damping_edge: -4, ..Scene::default() });
		assert!(!e.try_replace_scene(&Arc::new(other)));
		let reflective =
			SceneData::blank(40, 30, &Scene { damping_edge: -4, edges: Edges::Reflective, ..Scene::default() });
		assert!(!e.try_replace_scene(&Arc::new(reflective)));
		let slower = SceneData::blank(40, 30, &Scene { damping_edge: -4, refractive_index: 2.0, ..Scene::default() });
		assert!(e.try_replace_scene(&Arc::new(slower)));
		assert_eq!(e.sub_steps(), 1);
	}

	#[test]
	fn alpha_follows_the_lowest_level() {
		let mut e = engine(Edges::Open, 0, Stencil::NinePoint);
		assert_eq!(e.host.alpha, (-1.0f64 / 10.0).exp() as f32);
		let mut low = emitter(1, 1);
		low.level = 50.0;
		e.set_emitters(&[low]);
		assert_eq!(e.host.alpha, (-1.0f64 / 20.0).exp() as f32);
		e.set_emitters(&[]);
		assert_eq!(e.host.alpha, (-1.0f64 / 10.0).exp() as f32);
		e.host.shared.sources.level_used[0] = true;
		e.host.update_alpha();
		assert_eq!(e.host.alpha, (-1.0f64 / 10.0).exp() as f32, "level 0 is not a source level");
	}

	/// Every buffer the step writes but `w`, whose plane a blocked step leaves stale where its bands keep `w` in rings
	/// (blocked.rs), as bits.
	fn state(e: &CpuEngine) -> Vec<u32> {
		[&e.host.u, &e.host.prev, &e.host.s1, &e.host.s2].into_iter().flatten().map(|v| v.to_bits()).collect()
	}

	#[test]
	fn the_damage_skips_only_cells_that_stay_zero() {
		let stencils = [Stencil::NinePoint, Stencil::FourthOrder];
		for (stencil, unblocked) in stencils.into_iter().flat_map(|s| [false, true].map(|u| (s, u))) {
			for (edges, rim) in [(Edges::Open, 0), (Edges::Reflective, 0), (Edges::Open, -6)] {
				let mut scene = SceneData::blank(60, 44, &Scene { damping_edge: rim, edges, ..Scene::default() });
				let gw = scene.grid_width() as usize;
				let r = scene.rim as usize;
				for gy in r + 20..r + 26 {
					scene.c2[gy * gw + r + 38] = 0.0;
				}
				scene.image_sources.push(ImageSource { gx: (r + 50) as u32, gy: (r + 40) as u32, level: 40 });
				let mut damaged = CpuEngine::with_backend(scene.clone(), options(stencil), Backend::Generic);
				let mut whole = CpuEngine::with_backend(scene, options(stencil), Backend::Generic);
				for e in [&mut damaged, &mut whole] {
					e.set_emitters(&[emitter(30, 22), emitter(30, 41)]);
					e.set_unblocked(unblocked);
				}
				let cells = gw * whole.host.shared.scene.grid_height() as usize;
				whole.set_field(&vec![0.0; cells], &vec![0.0; cells]);
				damaged.advance_one_step();
				whole.advance_one_step();
				let all = Damage::all(gw, cells / gw);
				assert!(damaged.host.damage != all, "{stencil:?} {edges:?} {rim} {unblocked}: nothing skipped");
				for step in 1..150 {
					damaged.advance_one_step();
					whole.advance_one_step();
					assert!(state(&damaged) == state(&whole), "{stencil:?} {edges:?} {rim} {unblocked}: step {step}");
				}
			}
		}
	}

	#[test]
	fn an_idle_scene_has_no_damage_and_a_reset_clears_it() {
		let scene = SceneData::blank(40, 30, &Scene::default());
		let mut e = CpuEngine::with_backend(scene, options(Stencil::FourthOrder), Backend::Generic);
		for _ in 0..5 {
			e.advance_one_step();
		}
		assert_eq!(e.host.damage, Damage::NONE);
		e.set_emitters(&[emitter(20, 15)]);
		e.advance_one_step();
		assert!(!e.host.damage.is_empty());
		e.reset();
		assert_eq!(e.host.damage, Damage::NONE);
	}

	#[test]
	fn the_fused_steps_follow_the_trapezoids_sides() {
		// The sides of n levels: the 9-point's n - 1 rows, 4 n - 2 with a rim; the 4th order's 6 n - 3, with a rim
		// 8 n - 2.
		let (nine, drp) = (TimeScheme::Leapfrog, TimeScheme::ModifiedEquation);
		let by_sub = |scheme, rim| [1, 2, 3, 4, 9].map(|sub| fused_by_side(sub, scheme, rim));
		assert_eq!(by_sub(nine, false), [8, 8, 8, 8, 3]);
		assert_eq!(by_sub(nine, true), [8, 4, 2, 2, 1]);
		assert_eq!(by_sub(drp, false), [5, 2, 1, 1, 1]);
		assert_eq!(by_sub(drp, true), [4, 2, 1, 1, 1]);
	}
}
