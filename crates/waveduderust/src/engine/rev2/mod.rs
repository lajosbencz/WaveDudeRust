//! The CPU_REV2 engine: the scheme, the grid and the features of the reference CPU engine (engine/cpu.rs), its passes
//! fused into blocked steps and its hot loops written as row kernels of a backend (backend/mod.rs): the generic one in
//! portable Rust, or one for an instruction set (AVX2 and FMA; simd128 in wasm). This module and blocked.rs, the
//! orchestration, build for every target; only the backends are target-specific.
//!
//! The grid is the CPU engine's: padded by [`PAD`] rings, `stride = grid width + 2 PAD`, the rings 0 unless Mur's
//! condition sets them in `u` on open edges without a rim, mirror cells at c2 = 0 with the taps across a mirror dropped
//! (pml.rs, `cuts`). The planes, the medium, the forcing, the probes, the captures, the damage rectangle, Mur's chains
//! and the CPML's state are the CPU engine's too (host.rs); this module owns the passes over the grid and how they
//! split across threads.
//!
//! Passes: the 9-point sub-step is one leapfrog pass, `next = u + ((u - prev) d + c2 L u)`; the 4th-order (DRP)
//! stencil two, `w = c2 L u` into the plane `w`, then `next = u + ((u - prev) d + (w + c2 L w / 12))` from it. `next`
//! is written over `prev`. Each pass runs on the damage grown by its reach, one grid row per task on the rayon pool;
//! a row's cells with no mirror cut go through a kernel in spans, the cut ones through the CPU engine's per-cell
//! stencils. On the last sub-step of a step the leapfrog and the outer kernels advance the intensity stages of the
//! cells they write in the same loop. With a rim, the CPML corrects each row right after the pass writes it (pml.rs,
//! `XShared::row`), after the stages, as in the CPU engine.
//!
//! Every pass takes the CPU engine's uniform runs (engine/cpu.rs, Uniform medium): a run's cells go to the kernel
//! with its `c2` and `d` as constants, the rest with the planes' values. The 4th order's two passes go through the
//! plane `w`; the CPU engine's fusion of the outer pass with the inner (a ring of 7 `w` rows per block of rows) gives
//! way here to the blocked steps below.
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
//! Fused steps: a sweep may take several whole steps too ([`CpuRev2Engine::advance_steps`], at most
//! [`FUSED_STEPS`] by default, the 9-point only; `WaveEngine::advance_steps`), so a row is read from memory once per
//! `k` steps. The sweep's levels are the `k n` sub-steps one after another, and what happens between two steps happens
//! between their levels, row by row:
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
//! other spans. A sweep of a single level (one sub-step, one step) runs the passes; the 4th order takes one step a
//! sweep.
//!
//! Backends: an engine runs on one backend, chosen when it is built ([`Backend`]: the best one this CPU runs, or one
//! named); the passes and the blocked steps are generic over the backend's type (`RowKernels`), which `on_backend!`
//! picks once per step. Each row's work goes through the backend's entry (`run_row`), so the Rust around the kernels
//! (the CPML's rows, the cut cells) is compiled for the backend's instruction set, with no feature test on the way.
//! The generic backend is safe Rust in the CPU engine's expressions and order, and Rust contracts no multiply-add, so
//! on it the results are the CPU engine's bit for bit ([`Backend::Generic`]). Another backend may round differently
//! (fused multiply-adds); the engines then agree within a tolerance (tests/rev2.rs).

#![allow(unsafe_code)]

mod backend;
mod blocked;

use std::ops::Range;
use std::sync::Arc;

use rayon::iter::Either;
use rayon::prelude::*;

pub use self::backend::Backend;
use self::backend::{Intensity, RowKernels, RowMedium, advance, on_backend};
use self::blocked::{Line, MurRows, ProbeLog, Rim, RimSet, Rings, Shared, Step, Tiling};
use super::cpu::{Rows, UniformRuns, chunks_or_none, cut_spans, drp, medium_spans, nine_point, plane_rows, row_cuts};
use super::damage::{Damage, extent};
use super::forcing::live;
use super::host::{Host, Stages, host_engine};
use super::pml::{Medium, XPart, XPass, XShared};
use super::stencil::{INV12, PAD, TimeScheme, centred};
use super::substeps::sub_steps;
use super::types::{EngineOptions, SceneData};

/// The CPU_REV2 engine on one of its backends. See the module docs.
pub struct CpuRev2Engine {
	/// The planes, the medium, the forcing, the probes and the damage (host.rs).
	host: Host,
	/// The backend the passes run on; it runs on this CPU.
	backend: Backend,
	/// The runs of each grid row where `c2` and `d` hold one value (engine/cpu.rs, Uniform medium).
	uniform: UniformRuns,
	/// Mur's chains by the grid row that owns them (blocked.rs).
	mur: MurRows,
	/// A blocked step's source table of each sub-step, and its driven cells with their runs of table entries.
	tables: Vec<Vec<f32>>,
	driven: Vec<(usize, u32, u32)>,
	/// The tilings of the last blocked steps by their levels, kept for the next.
	tilings: Vec<Tiling>,
	/// The most steps a blocked sweep fuses (module docs, Fused steps).
	fused: u32,
	/// The probe cells of a fused sweep by padded cell, with their place in a reading and whether they read the
	/// intensity; and its readings, one after another.
	probe_cells: Vec<(usize, u32, bool)>,
	readings: Vec<f32>,
	/// The 4th order's rings of `w` rows of a blocked step's bands, kept for the next (blocked.rs).
	rings: Rings,
}

impl CpuRev2Engine {
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
			fused: FUSED_STEPS,
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

	/// Fuses at most `k` steps into one sweep from now on ([`FUSED_STEPS`] by default); for measurements.
	///
	/// # Panics
	///
	/// When `k` is 0.
	#[doc(hidden)]
	pub fn set_fused_steps(&mut self, k: u32) {
		assert!(k >= 1, "at least one step a sweep");
		self.fused = k;
	}

	/// [`WaveEngine::fused_steps`](super::WaveEngine::fused_steps): the most steps a sweep fuses; 1 for the 4th order,
	/// whose steps are not fused.
	pub fn fused_steps(&self) -> u32 {
		match self.host.shared.scheme {
			TimeScheme::Leapfrog => self.fused,
			TimeScheme::ModifiedEquation => 1,
		}
	}

	/// [`WaveEngine::advance_steps`](super::WaveEngine::advance_steps), which cannot fail here: `n` steps, in sweeps of
	/// up to [`fused_steps`](Self::fused_steps) steps each (module docs, Fused steps).
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
		grid_rows(kernels, &mut h.prev, stages, s, area, rim.as_mut(), |r, next, stages| {
			line.row(r, &u[(r - 1) * s..(r + 2) * s], next, stages);
		});
	}

	/// The modified equation's first pass: `w = c2 L u` on the damaged cells.
	fn inner_pass<B: RowKernels>(&mut self, kernels: B) {
		let h = &mut self.host;
		let (s, u, area) = (h.stride, &h.u, &h.damage);
		let line = DrpLine::of((h.stride, &h.c2, &h.d, &h.cuts, h.mirrors), h.axis, &self.uniform, kernels, &area.x);
		let medium = Medium { cuts: &h.cuts, c2: &h.c2 };
		let mut rim = h.pml.as_mut().and_then(|p| p.begin(0, u, medium, 1.0, area));
		grid_rows(kernels, &mut h.w, None, s, area, rim.as_mut(), |r, w, _| line.lap(r, &plane_rows(u, r, s), w));
	}

	/// The modified equation's second pass: writes the next field over `prev` from `u` and the plane `w`, and the
	/// intensity stages when `intensity`.
	fn outer_pass<B: RowKernels>(&mut self, kernels: B, intensity: bool) {
		let h = &mut self.host;
		let (s, gw, u, w, area) = (h.stride, h.gw(), &h.u, &h.w, &h.damage);
		let line = DrpLine::of((h.stride, &h.c2, &h.d, &h.cuts, h.mirrors), h.axis, &self.uniform, kernels, &area.x);
		let stages = Host::stages(&mut h.s1, &mut h.s2, h.alpha, intensity);
		let mut rim = h.pml.as_mut().and_then(|p| p.begin(1, w, Medium { cuts: &h.cuts, c2: &h.c2 }, INV12, area));
		grid_rows(kernels, &mut h.prev, stages, s, area, rim.as_mut(), |r, next, stages| {
			line.outer(r, &u[r * s + PAD..][..gw], &plane_rows(w, r, s), next, stages);
		});
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
		if levels < 2 {
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
		let damage = extent(&h.u, s, &area, known);
		h.damage = extent(&h.prev, s, &area, damage);
	}

	/// One sub-step on the engine's backend ([`sub_step_on`](Self::sub_step_on)).
	fn sub_step(&mut self, intensity: bool) {
		on_backend!(self.backend, B => self.sub_step_on(self.kernels::<B>(), intensity));
	}

	/// One sub-step; the next field ends in `u`. Each pass runs on the damage grown by its reach; the damage then
	/// takes in what the pass and the forcing made non-zero (engine/cpu.rs, Damage).
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
				self.host.force_w(&Damage::NONE, &mut Vec::new());
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

host_engine!(
	CpuRev2Engine,
	|engine| engine.backend.engine_name(),
	CpuRev2Engine::fused_steps,
	CpuRev2Engine::advance_steps
);

/// The most steps a blocked sweep fuses by default (module docs, Fused steps; docs/measurements.md).
pub const FUSED_STEPS: u32 = 4;

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
	#[cfg_attr(not(target_arch = "wasm32"), inline(always))]
	fn row(&self, r: usize, u: &[f32], next: &mut [f32], mut stages: Option<Intensity<'_>>) {
		let Self { s, gw, kernels, uniform, c2, d, cuts, mirrors, xs } = *self;
		let (at, runs) = (r * s + PAD, uniform.row(r - PAD));
		let (c2, d) = (&c2[at..][..gw], &d[at..][..gw]);
		// Grid column x of the rows above, of and below is at x + 1 of these; the row's own at `s + PAD`.
		let rows: [&[f32]; 3] = std::array::from_fn(|k| &u[k * s + PAD - 1..][..gw + 2]);
		let centre = s + PAD;
		// The cells of `span` with no cut, a uniform run's with its constants.
		let uncut = |span: Range<usize>, next: &mut [f32], stages: &mut Option<Intensity<'_>>| {
			medium_spans(runs, c2, d, span, |piece, medium| {
				let taps = rows.map(|row| &row[piece.start..piece.end + 2]);
				let medium = match medium {
					Some((c2, d)) => RowMedium::Uniform { c2, d },
					None => RowMedium::Planes { c2: &c2[piece.clone()], d: &d[piece.clone()] },
				};
				let stages = stages.as_mut().map(|st| st.span(piece.start, piece.len()));
				kernels.nine(taps, medium, &mut next[piece], stages);
			});
		};
		let Some(cuts) = row_cuts(cuts, mirrors, at, gw, xs) else {
			return uncut(xs.clone(), next, &mut stages);
		};
		cut_spans(cuts, xs.clone(), |span, cut| {
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
		});
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
	#[cfg_attr(not(target_arch = "wasm32"), inline(always))]
	fn pieces(&self, r: usize, span: Range<usize>, mut f: impl FnMut(Range<usize>, RowMedium<'_>)) {
		let at = r * self.s + PAD;
		let (c2, d) = (&self.c2[at..][..self.gw], &self.d[at..][..self.gw]);
		medium_spans(self.uniform.row(r - PAD), c2, d, span, |piece, medium| {
			let medium = match medium {
				Some((c2, d)) => RowMedium::Uniform { c2, d },
				None => RowMedium::Planes { c2: &c2[piece.clone()], d: &d[piece.clone()] },
			};
			f(piece, medium);
		});
	}

	/// Writes `w = c2 L u` over `w`, the `gw` grid cells of padded row `r`, on the columns `xs`, from the 7 padded rows
	/// `u` around it.
	#[cfg_attr(not(target_arch = "wasm32"), inline(always))]
	fn lap(&self, r: usize, u: &Rows<'_>, w: &mut [f32]) {
		let (at, gw, kernels, axis) = (r * self.s + PAD, self.gw, self.kernels, &self.axis);
		let uncut = |span: Range<usize>, w: &mut [f32]| {
			self.pieces(r, span, |piece, medium| kernels.lap(span_rows(u, &piece), axis, medium, &mut w[piece]));
		};
		let Some(cuts) = row_cuts(self.cuts, self.mirrors, at, gw, self.xs) else { return uncut(self.xs.clone(), w) };
		let c2 = &self.c2[at..][..gw];
		cut_spans(cuts, self.xs.clone(), |span, cut| {
			if !cut {
				return uncut(span, w);
			}
			for x in span {
				w[x] = c2[x] * drp(u, x, axis, cuts[x]);
			}
		});
	}

	/// Writes the next field over `next`, the `gw` grid cells of padded row `r` holding the field one sub-step before,
	/// on the columns `xs`, from the row's field `u` (its `gw` grid cells) and the 7 padded rows `w` around it; advances
	/// `stages` (the row's), when given, by the values written.
	#[cfg_attr(not(target_arch = "wasm32"), inline(always))]
	fn outer(&self, r: usize, u: &[f32], w: &Rows<'_>, next: &mut [f32], mut stages: Option<Intensity<'_>>) {
		let (at, gw, kernels, axis) = (r * self.s + PAD, self.gw, self.kernels, &self.axis);
		let uncut = |span: Range<usize>, next: &mut [f32], stages: &mut Option<Intensity<'_>>| {
			self.pieces(r, span, |piece, medium| {
				let stages = stages.as_mut().map(|st| st.span(piece.start, piece.len()));
				kernels.outer(&u[piece.clone()], span_rows(w, &piece), axis, medium, &mut next[piece], stages);
			});
		};
		let Some(cuts) = row_cuts(self.cuts, self.mirrors, at, gw, self.xs) else {
			return uncut(self.xs.clone(), next, &mut stages);
		};
		let (c2, d, wc) = (&self.c2[at..][..gw], &self.d[at..][..gw], &w[3][PAD..][..gw]);
		cut_spans(cuts, self.xs.clone(), |span, cut| {
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
		});
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
					#[cfg_attr(not(target_arch = "wasm32"), inline(always))]
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

/// The seven rows a DRP kernel reads for the grid cells `span`: from 3 cells left of it to 3 right of it.
#[inline]
fn span_rows<'a>(rows: &Rows<'a>, span: &Range<usize>) -> Rows<'a> {
	// Grid column x is at PAD + x, so 3 cells left of the span's first is at its own index.
	rows.map(|row| &row[span.start..PAD + span.end + 3])
}
