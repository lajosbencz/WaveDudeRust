//! What the two CPU engines (engine/cpu.rs, engine/avx) keep alike: the padded planes, the medium, the forcing, the
//! probes, Mur's chains, the CPML's state and the damage, with the calls that do not depend on how the passes run;
//! and [`host_engine`], which gives an engine on a [`Host`] its public calls and its [`WaveEngine`] impl.
//!
//! The layout and the scheme are engine/cpu.rs's (module docs).
//!
//! [`WaveEngine`]: super::WaveEngine

use std::sync::Arc;

use super::capture;
use super::cpu::{drp, plane_rows};
use super::damage::{Damage, extent};
use super::forcing::{IMAGE_LEVELS, SourceGroups, live};
use super::mur::{MurChain, mur_chains};
use super::pml::{self, CpuPml};
use super::shared::{Shared, blend};
use super::stencil::{PAD, StencilCoefficients, TimeScheme, centred};
use super::types::{EngineOptions, FrameShape, ProbeCells, ProbeQueue, ProbeReadings, SceneData, Snapshot};
use crate::model::Layer;

/// The state of a CPU engine on the host. See the module docs.
pub(super) struct Host {
	pub(super) shared: Shared,
	pub(super) stride: usize,
	/// The axis weights a0..a3 of the stencil.
	pub(super) axis: [f32; 4],
	/// c2 / n^2 per padded cell.
	pub(super) c2: Vec<f32>,
	/// d^(1/n) per padded cell.
	pub(super) d: Vec<f32>,
	/// Cut bytes per padded cell (pml.rs, `cuts`); all 0 without mirrors.
	pub(super) cuts: Vec<u8>,
	pub(super) mirrors: bool,
	pub(super) u: Vec<f32>,
	pub(super) prev: Vec<f32>,
	/// The modified equation's `c2 L u`, 0 on the padding; an engine that fuses its passes may leave stale values
	/// where it computes `w` itself (engine/cpu.rs, Fused passes).
	pub(super) w: Vec<f32>,
	/// The intensity's EMA stages; empty unless the intensity is tracked.
	pub(super) s1: Vec<f32>,
	pub(super) s2: Vec<f32>,
	pub(super) mur: Vec<MurChain>,
	pub(super) pml: Option<CpuPml>,
	pub(super) groups: SourceGroups,
	/// The source table of the current sub-step (forcing.rs).
	pub(super) values: Vec<f32>,
	pub(super) alpha: f32,
	/// Per probe its layer and padded cells.
	probes: Vec<(Layer, Vec<usize>)>,
	probe_queue: ProbeQueue,
	probe_reading: ProbeReadings,
	/// The cells that hold a value (engine/cpu.rs, Damage).
	pub(super) damage: Damage,
	/// Cells one pass reaches along an axis: the stencil's, the CPML's when there is a rim.
	pub(super) reach: usize,
}

/// The intensity stages a pass advances, and the EMA weight.
pub(super) struct Stages<'a> {
	pub(super) s1: &'a mut [f32],
	pub(super) s2: &'a mut [f32],
	pub(super) alpha: f32,
}

impl Host {
	/// At rest on `scene` with `n` sub-steps a step, the scene loaded.
	pub(super) fn new(scene: Arc<SceneData>, options: EngineOptions, n: u32) -> Self {
		assert!(n >= 1, "at least one sub-step");
		let stride = scene.stride();
		let cells = scene.padded_cells();
		let scheme = TimeScheme::of(options.stencil);
		let stages = if options.track_intensity { cells } else { 0 };
		let mut host = Self {
			stride,
			axis: StencilCoefficients::of(options.stencil).axis.map(|a| a as f32),
			c2: vec![0.0; cells],
			d: vec![1.0; cells],
			cuts: Vec::new(),
			mirrors: false,
			u: vec![0.0; cells],
			prev: vec![0.0; cells],
			w: if scheme == TimeScheme::ModifiedEquation { vec![0.0; cells] } else { Vec::new() },
			s1: vec![0.0; stages],
			s2: vec![0.0; stages],
			mur: Vec::new(),
			pml: None,
			groups: SourceGroups::default(),
			values: vec![0.0; IMAGE_LEVELS],
			alpha: 0.0,
			probes: Vec::new(),
			probe_queue: ProbeQueue::default(),
			probe_reading: ProbeReadings::default(),
			damage: Damage::NONE,
			reach: 0,
			shared: Shared::new(scene, options, n),
		};
		host.load_scene();
		host.update_alpha();
		host
	}

	/// Copies the scene's maps into the padded buffers at the sub-step scale and builds the cuts, the Mur
	/// chains, the PML and the source groups.
	fn load_scene(&mut self) {
		let (scene, n) = (&self.shared.scene, self.shared.sub_steps);
		scene.fill_sub_step_medium(n, &mut self.c2, &mut self.d);
		self.cuts = pml::cuts(scene);
		self.mirrors = self.cuts.iter().any(|&c| c != 0);
		self.mur = mur_chains(scene, &self.c2, &self.d);
		let coefficients = StencilCoefficients::of(self.shared.options.stencil);
		self.pml = CpuPml::new(scene, n, self.axis, coefficients.cross as f32);
		let pml = if self.pml.is_some() { pml::REACH } else { 0 };
		self.reach = coefficients.reach().max(pml);
		self.group_sources();
	}

	pub(super) fn update_alpha(&mut self) {
		self.alpha = self.shared.alpha();
	}

	fn group_sources(&mut self) {
		self.groups = self.shared.groups();
		self.values.resize(self.shared.sources.table_len(), 0.0);
	}

	/// The grid width.
	pub(super) fn gw(&self) -> usize {
		self.shared.scene.grid_width() as usize
	}

	/// Sets the field now and one sub-step before, each grid-sized (no padding), row-major.
	pub(super) fn set_field(&mut self, now: &[f32], before: &[f32]) {
		let scene = &self.shared.scene;
		let gw = scene.grid_width() as usize;
		assert_eq!(now.len(), gw * scene.grid_height() as usize);
		assert_eq!(before.len(), now.len());
		for (gy, (a, b)) in now.chunks_exact(gw).zip(before.chunks_exact(gw)).enumerate() {
			let at = scene.padded_index(0, gy);
			self.u[at..at + gw].copy_from_slice(a);
			self.prev[at..at + gw].copy_from_slice(b);
		}
		self.damage = Damage::all(gw, scene.grid_height() as usize);
	}

	pub(super) fn sample_probes(&self, out: &mut ProbeReadings) {
		out.clear();
		for (layer, cells) in &self.probes {
			match layer {
				Layer::Field => out.push(cells.iter().map(|&c| self.u[c])),
				Layer::Intensity => out.push(cells.iter().map(|&c| self.blended(c))),
			}
		}
	}

	pub(super) fn queue_probes(&mut self) {
		let mut reading = std::mem::take(&mut self.probe_reading);
		self.sample_probes(&mut reading);
		self.probe_queue.push(reading.values());
		self.probe_reading = reading;
	}

	pub(super) fn probe_queue(&mut self) -> &mut ProbeQueue {
		&mut self.probe_queue
	}

	pub(super) fn capture_into(&self, shape: &FrameShape, out: &mut Snapshot) {
		let (scene, options) = (&self.shared.scene, &self.shared.options);
		assert_eq!((shape.canvas_width, shape.canvas_height), (scene.canvas_width, scene.canvas_height));
		capture::plane_rows(shape, Layer::Field, &mut out.field, |y, row| {
			let at = scene.canvas_index(0, y);
			row.copy_from_slice(&self.u[at..at + row.len()]);
		});
		if options.track_intensity {
			let scale = options.intensity_scale as f32;
			capture::plane_rows(shape, Layer::Intensity, &mut out.intensity, |y, row| {
				let at = scene.canvas_index(0, y);
				let n = row.len();
				for ((r, &a), &b) in row.iter_mut().zip(&self.s1[at..at + n]).zip(&self.s2[at..at + n]) {
					*r = blend(a, b, scale);
				}
			});
		} else {
			out.intensity.clear();
		}
		out.shape = *shape;
		out.step = self.shared.time.saturating_sub(1);
	}

	pub(super) fn prepare_frame(&self) -> (Vec<f32>, Vec<f32>) {
		let stages = (!self.s1.is_empty()).then_some((&self.s1[..], &self.s2[..]));
		self.shared.canvas_frame(&self.u, stages)
	}

	/// Adds the sub-step's forcing `F` to `w` at the driven cells, where the modified equation takes `A u`
	/// (forcing.rs). A driven cell of `fresh` (engine/cpu.rs, Fused passes) first takes `w = c2 L u`, which the inner
	/// pass left to the outer, and joins `driven`, from which the outer pass reads it; `driven` ends ascending.
	pub(super) fn force_w(&mut self, fresh: &Damage, driven: &mut Vec<usize>) {
		let s = self.stride;
		driven.clear();
		for (cell, run) in self.groups.iter() {
			let mut live_values = run.iter().map(|&e| self.values[e as usize]).filter(|&f| live(f)).peekable();
			if live_values.peek().is_none() {
				continue;
			}
			let (gx, gy) = (cell % s - PAD, cell / s - PAD);
			if fresh.x.contains(&gx) && fresh.y.contains(&gy) {
				let cut = if self.mirrors { self.cuts[cell] } else { 0 };
				self.w[cell] = self.c2[cell] * drp(&plane_rows(&self.u, cell / s, s), gx, &self.axis, cut);
				driven.push(cell);
			}
			let mut v = self.w[cell];
			for f in live_values {
				v += f;
			}
			self.w[cell] = v;
			self.damage.include(cell, s);
		}
		driven.sort_unstable();
		driven.dedup();
	}

	/// Adds `h F` to the next field at the driven cells, where leapfrog takes `h A u`, and moves the
	/// intensity stages of the last sub-step to the forced value.
	pub(super) fn force_next(&mut self, intensity: bool) {
		let rest = 1.0 - self.alpha;
		for (cell, run) in self.groups.iter() {
			let (before, h) = (self.prev[cell], centred(self.d[cell]));
			let mut v = before;
			let mut driven = false;
			for &e in run {
				let f = self.values[e as usize];
				if live(f) {
					v += h * f;
					driven = true;
				}
			}
			if !driven {
				continue;
			}
			self.prev[cell] = v;
			self.damage.include(cell, self.stride);
			if intensity {
				let s1 = self.s1[cell] + (v * v - before * before) * rest;
				self.s2[cell] += (s1 - self.s1[cell]) * rest;
				self.s1[cell] = s1;
			}
		}
	}

	/// The intensity stages and their weight when `intensity` (and tracked).
	pub(super) fn stages<'a>(s1: &'a mut [f32], s2: &'a mut [f32], alpha: f32, intensity: bool) -> Option<Stages<'a>> {
		(intensity && !s1.is_empty()).then_some(Stages { s1, s2, alpha })
	}

	/// The damage grown by one pass's reach: every cell the pass and the CPML after it can change.
	pub(super) fn reached(&self, damage: &Damage) -> Damage {
		let scene = &self.shared.scene;
		damage.grown(self.reach, scene.grid_width() as usize, scene.grid_height() as usize)
	}

	/// Ends a sub-step whose passes wrote the next field over `prev` on `area`: the damage takes in what they made
	/// non-zero, Mur's condition sets the open edges, and the next field becomes `u`.
	pub(super) fn end_sub_step(&mut self, area: &Damage) {
		self.damage = extent(&self.prev, self.stride, area, self.damage.clone());
		self.apply_open_edges();
		std::mem::swap(&mut self.u, &mut self.prev);
	}

	/// Mur's condition on the first ring (mur.rs, `mur_weights`); the second and third rings, which only the DRP stencil
	/// reads, continue the field linearly from the edge cell through the first ring. Chaining Mur ring by ring grows a
	/// mode where two open edges meet at Courant numbers above about 0.75 (docs/physics.md, Open edges).
	fn apply_open_edges(&mut self) {
		for m in &self.mur {
			let [a, b, c] = m.weights;
			let [r1, r2, r3] = m.rings;
			let (edge, first) = (self.prev[m.inner], a * self.u[m.inner] + b * self.prev[m.inner] + c * self.u[r1]);
			self.prev[r1] = first;
			self.prev[r2] = 2.0 * first - edge;
			self.prev[r3] = 3.0 * first - 2.0 * edge;
		}
	}

	/// The intensity at padded cell `cell`, 0 when it is not tracked.
	fn blended(&self, cell: usize) -> f32 {
		if self.s1.is_empty() {
			return 0.0;
		}
		blend(self.s1[cell], self.s2[cell], self.shared.options.intensity_scale as f32)
	}

	pub(super) fn reset(&mut self) {
		for b in [&mut self.u, &mut self.prev, &mut self.w, &mut self.s1, &mut self.s2] {
			b.fill(0.0);
		}
		self.damage = Damage::NONE;
		if let Some(p) = &mut self.pml {
			p.reset();
		}
		self.shared.reset();
		self.probe_queue.clear();
	}

	pub(super) fn set_track_intensity(&mut self, on: bool) {
		if self.shared.options.track_intensity != on {
			self.shared.options.track_intensity = on;
			let stages = if on { self.u.len() } else { 0 };
			self.s1 = vec![0.0; stages];
			self.s2 = vec![0.0; stages];
		}
	}

	pub(super) fn set_emitters(&mut self, emitters: &[super::types::Emitter]) {
		self.shared.set_emitters(emitters);
		self.group_sources();
		self.update_alpha();
	}

	pub(super) fn set_probes(&mut self, probes: &[ProbeCells]) {
		let scene = &self.shared.scene;
		self.probes = probes
			.iter()
			.map(|p| (p.layer, p.cells.iter().map(|&(x, y)| scene.canvas_index(x as usize, y as usize)).collect()))
			.collect();
		self.probe_queue.set_lengths(probes.iter().map(|p| p.cells.len()).collect());
	}

	/// Swaps in `scene` when it can replace the current one (`WaveEngine::try_replace_scene`).
	pub(super) fn try_replace_scene(&mut self, scene: &Arc<SceneData>) -> bool {
		let Some(n) = self.shared.replacement(scene) else { return false };
		self.shared.replace_scene(scene.clone(), n);
		self.load_scene();
		self.update_alpha();
		true
	}
}

/// The public calls and the [`WaveEngine`](super::WaveEngine) impl of an engine type on a [`Host`] in its field
/// `host`, named `$name` for logs. The type has `fn step(&mut self)`, the sub-steps of one step with the next field
/// ending in `u` (`sub_steps_in_turn`, which the macro adds, runs them one by one through
/// `fn sub_step(&mut self, intensity: bool)`), and `fn scene_loaded(&mut self)`, which rebuilds what it derives from
/// the scene.
macro_rules! host_engine {
	($engine:ty, $name:literal) => {
		impl $engine {
			/// Sets the field now and one sub-step before, each grid-sized (no padding), row-major.
			/// For analytic tests that start from a given state.
			pub fn set_field(&mut self, now: &[f32], before: &[f32]) {
				self.host.set_field(now, before);
			}

			/// The padded field buffer (see [`SceneData::stride`](crate::engine::SceneData::stride)).
			pub fn field_padded(&self) -> &[f32] {
				&self.host.u
			}

			/// The intensity's two EMA stages over the padded grid, empty when it is not tracked. For tests.
			#[doc(hidden)]
			pub fn intensity_stages(&self) -> (&[f32], &[f32]) {
				(&self.host.s1, &self.host.s2)
			}

			// The inherent calls below take `&mut self` like the trait's, so they win method resolution on the
			// concrete type: its callers get no `Result`.

			/// [`WaveEngine::advance_one_step`](crate::engine::WaveEngine::advance_one_step), which cannot fail here.
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

			/// [`WaveEngine::sample_probes`](crate::engine::WaveEngine::sample_probes), which cannot fail here.
			pub fn sample_probes(&mut self, out: &mut crate::engine::ProbeReadings) {
				self.host.sample_probes(out);
			}

			/// Converts the canvas after the last step into `out`'s planes in `shape`, their allocations reused.
			pub fn capture_into(&self, shape: &crate::engine::FrameShape, out: &mut crate::engine::Snapshot) {
				self.host.capture_into(shape, out);
			}

			/// [`WaveEngine::capture_frame`](crate::engine::WaveEngine::capture_frame), which cannot fail here.
			pub fn capture_frame(&mut self, shape: &crate::engine::FrameShape) -> crate::engine::Snapshot {
				let mut out = crate::engine::Snapshot::new(*shape);
				self.host.capture_into(shape, &mut out);
				out
			}

			/// [`WaveEngine::prepare_frame`](crate::engine::WaveEngine::prepare_frame), which cannot fail here.
			pub fn prepare_frame(&mut self) -> (Vec<f32>, Vec<f32>) {
				self.host.prepare_frame()
			}
		}

		impl crate::engine::WaveEngine for $engine {
			fn name(&self) -> &'static str {
				$name
			}

			fn scene(&self) -> &crate::engine::SceneData {
				&self.host.shared.scene
			}

			fn options(&self) -> &crate::engine::EngineOptions {
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

			fn set_emitters(&mut self, emitters: &[crate::engine::Emitter]) {
				self.host.set_emitters(emitters);
			}

			fn set_probes(&mut self, probes: &[crate::engine::ProbeCells]) {
				self.host.set_probes(probes);
			}

			fn try_replace_scene(&mut self, scene: &std::sync::Arc<crate::engine::SceneData>) -> bool {
				let replaced = self.host.try_replace_scene(scene);
				if replaced {
					self.scene_loaded();
				}
				replaced
			}

			fn advance_one_step(&mut self) -> Result<(), crate::engine::EngineError> {
				<$engine>::advance_one_step(self);
				Ok(())
			}

			fn sample_probes(
				&mut self,
				out: &mut crate::engine::ProbeReadings,
			) -> Result<(), crate::engine::EngineError> {
				self.host.sample_probes(out);
				Ok(())
			}

			fn queue_probes(&mut self) -> Result<(), crate::engine::EngineError> {
				self.host.queue_probes();
				Ok(())
			}

			fn pending_probes(&mut self) -> Result<crate::engine::PendingProbes, crate::engine::EngineError> {
				Ok(crate::engine::PendingProbes::done(self.host.probe_queue().end()))
			}

			fn drain_probes(
				&mut self,
				pending: crate::engine::PendingProbes,
				f: &mut dyn FnMut(&crate::engine::ProbeReadings),
			) -> Result<(), crate::engine::EngineError> {
				self.host.probe_queue().drain(pending.end(), f);
				Ok(())
			}

			fn begin_capture(
				&mut self,
				shape: &crate::engine::FrameShape,
				mut into: crate::engine::Snapshot,
			) -> Result<crate::engine::PendingCapture, crate::engine::EngineError> {
				self.host.capture_into(shape, &mut into);
				Ok(crate::engine::PendingCapture::done(into))
			}

			fn finish_capture(
				&mut self,
				pending: crate::engine::PendingCapture,
			) -> Result<crate::engine::Snapshot, crate::engine::EngineError> {
				Ok(pending.snapshot)
			}

			fn prepare_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), crate::engine::EngineError> {
				Ok(self.host.prepare_frame())
			}
		}
	};
}

pub(super) use host_engine;
