//! The GPU wave engine on wgpu compute (docs/spec/30-engine.md, GPU engine; decision 0003).
//!
//! Same rules as the CPU engine (engine/cpu.rs), the reference; host-side shared parts are in engine/shared.rs.
//!
//! - Buffers cover the padded grid ([`SceneData::stride`], [`PAD`](super::stencil::PAD) rings), all f32.
//! - Kernels are WGSL under `shaders/` (kernels.rs); the `CUTS` override is compiled in only for scenes with
//!   mirrors. Grid kernels run in 32 x 8 work groups with bounds checks, the CPML rim's in 64-wide groups.
//! - Source values are computed on the host in f64 as on the CPU (engine/forcing.rs), one set per sub-step; the
//!   kernels only add them in the CPU's order.
//! - Steps are recorded into one command encoder (batch.rs), submitted every [`BATCH_STEPS`] steps or when
//!   something is read; at most [`MAX_IN_FLIGHT`] batches are queued ahead of the card.
//! - Captures (captures.rs) and probe readings (probes.rs) are read back through pooled staging buffers while the
//!   host steps on; [`DeviceFrames`] (frames.rs) keeps captured planes on the device for render passes.
//! - On wasm32 nothing can wait: call [`WaveEngine::submit_pending`], then await [`PendingCapture::ready`] before
//!   `finish_capture` and [`PendingProbes::ready`] before `drain_probes`; `sample_probes` and `prepare_frame` are
//!   unavailable there, and `poll_room` holds a step back while [`MAX_IN_FLIGHT`] submissions are unfinished.
//!
//! Where the GPU differs from the CPU: fused multiply-add and subnormal flushing change the last bit of some cells
//! per step, within the tolerance of docs/physics.md (relative 1e-4 on the field after 1000 steps). The capture's
//! block mean uses the card's division; the intensity root is corrected to the correctly rounded one in integers
//! (capture.wgsl, `sqrt_rn`); f16 rounding is bit-exact (nearest even).
//!
//! Threads: one driver thread per engine, no internal locking; the device and queue are shared by every engine of the
//! process natively (`context`), or by the engines built on one [`GpuContext`].

mod batch;
mod buffers;
mod captures;
mod context;
mod forcing;
mod frames;
mod grid;
mod kernels;
mod medium;
mod probes;
mod readback;

use std::sync::Arc;
use std::task::{Context, Poll, Waker};

pub use batch::{BATCH_STEPS, MAX_IN_FLIGHT};
pub use context::{FIRST_KICK, GpuContext, GpuError, Kicks, MAX_KICK_GAP, check_buffer, storage_limit};
#[cfg(not(target_arch = "wasm32"))]
pub use context::{context, set_context};
pub use frames::{DeviceFrame, DeviceFrames, FrameTag};
pub(crate) use kernels::HALF;

use batch::Batch;
use buffers::{Caught, ResourceScope};
use captures::Captures;
use forcing::Forcing;
use grid::{Grid, Params, Passes};
use kernels::{GRID_GROUP, Kernels, LINEAR_GROUP};
use probes::Probes;
use readback::Readback;

use super::shared::Shared;
use super::stencil::{StencilCoefficients, TimeScheme};
use super::substeps::sub_steps;
use super::types::{
	Emitter, EngineOptions, FrameShape, PendingCapture, PendingProbes, ProbeCells, ProbeReadings, SceneData, Snapshot,
};
use super::{EngineError, WaveEngine};
#[cfg(target_arch = "wasm32")]
use crate::sync::Readiness;

/// The wgpu engine. See the module docs for layouts and differences from the CPU engine.
pub struct GpuEngine {
	ctx: Arc<GpuContext>,
	shared: Shared,
	kernels: Kernels,
	/// What the uniform of `grid` holds, or is about to.
	params: Params,
	grid: Grid,
	passes: Passes,
	forcing: Forcing,
	probes: Probes,
	captures: Captures,
	batch: Batch,
	/// The first failure of a call that returns no `Result`, returned by the next one that does.
	failed: Option<GpuError>,
	/// Resource scopes closed whose errors the device has not reported yet (wasm32 hears of them later).
	caught: Vec<Caught>,
}

impl GpuEngine {
	/// An engine at rest on `scene` on the process's device; blocks until the pipelines are built.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn new(scene: impl Into<Arc<SceneData>>, options: EngineOptions) -> Result<Self, GpuError> {
		pollster::block_on(Self::create(context()?, scene, options))
	}

	/// An engine at rest on `scene` on `ctx`; refused on a lost device.
	pub async fn create(
		ctx: Arc<GpuContext>,
		scene: impl Into<Arc<SceneData>>,
		options: EngineOptions,
	) -> Result<Self, GpuError> {
		if let Some(e) = ctx.lost() {
			return Err(e);
		}
		let scene = scene.into();
		grid::check_grid(&scene, &ctx.limits)?;
		let n = sub_steps(options.stencil, scene.max_c2());
		let medium = medium::build(&scene, n)?;
		let shared = Shared::new(scene, options, n);
		let device = &ctx.device;

		let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
		let coefficients = StencilCoefficients::of(shared.options.stencil);
		let kernels = Kernels::new(device, coefficients, shared.scheme, medium.mirrors());
		if let Some(e) = scope.pop().await {
			return Err(GpuError::Pipeline(e.to_string()));
		}

		let resources = ResourceScope::open(device);
		let grid = Grid::new(device, &shared.scene, shared.scheme);
		let passes = Passes::new(device, &kernels, &grid, &shared, &medium);
		let mut params = Params::of(&shared.scene, &shared.options);
		(params.mur_count, params.d_base) = (passes.mur_count, medium.d_base);
		let forcing = Forcing::new(&ctx, &kernels.force, &grid, &shared, medium.d);
		let captures = Captures::new(&ctx);
		let mut engine = Self {
			shared,
			kernels,
			params,
			grid,
			passes,
			forcing,
			probes: Probes::default(),
			captures,
			batch: Batch::default(),
			failed: None,
			caught: Vec::new(),
			ctx: ctx.clone(),
		};
		engine.upload_source_params();
		engine.reset();
		if let Some(e) = resources.close().await {
			return Err(GpuError::Resources(e.to_string()));
		}
		engine.check()?;
		let scene = &engine.shared.scene;
		tracing::info!(
			grid = %format!("{} x {}", scene.grid_width(), scene.grid_height()),
			canvas = %format!("{} x {}", scene.canvas_width, scene.canvas_height),
			rim = scene.rim,
			edges = ?scene.edges,
			stride = scene.stride(),
			stencil = ?engine.shared.options.stencil,
			scheme = ?engine.shared.scheme,
			mirror_cuts = engine.kernels.cuts(),
			sub_steps = n,
			work_groups = %format!(
				"{} x {} of {} x {}",
				scene.grid_width().div_ceil(GRID_GROUP.0),
				scene.grid_height().div_ceil(GRID_GROUP.1),
				GRID_GROUP.0,
				GRID_GROUP.1
			),
			"GPU engine built"
		);
		Ok(engine)
	}

	/// Sets the field now and one sub-step before, each grid-sized (no padding), row-major; the ring
	/// keeps its values. For analytic tests that start from a given state.
	pub fn set_field(&mut self, now: &[f32], before: &[f32]) {
		let gw = self.shared.scene.grid_width() as usize;
		assert_eq!(now.len(), gw * self.shared.scene.grid_height() as usize);
		assert_eq!(before.len(), now.len());
		let flushed = self.flush();
		self.note(flushed);
		let (queue, cur) = (&self.ctx.queue, self.grid.cur);
		for (gy, (a, b)) in now.chunks_exact(gw).zip(before.chunks_exact(gw)).enumerate() {
			let at = (self.shared.scene.padded_index(0, gy) * 4) as u64;
			queue.write_buffer(&self.grid.field[cur], at, bytemuck::cast_slice(a));
			queue.write_buffer(&self.grid.field[1 - cur], at, bytemuck::cast_slice(b));
		}
	}

	/// Submits the recorded steps and waits until the card has run them (timing, tests).
	pub fn synchronize(&mut self) -> Result<(), GpuError> {
		self.check()?;
		let index = self.submit()?;
		readback::wait(&self.ctx.device, index)?;
		self.batch.settled();
		Ok(())
	}

	/// The kept failure or the device's loss.
	fn check(&self) -> Result<(), GpuError> {
		match self.failed.clone().or_else(|| self.ctx.lost()) {
			Some(e) => Err(e),
			None => Ok(()),
		}
	}

	/// Keeps the first error the resource scopes caught that the device has reported.
	fn settle(&mut self) {
		let mut cx = Context::from_waker(Waker::noop());
		let mut first = None;
		self.caught.retain_mut(|caught| match caught.as_mut().poll(&mut cx) {
			Poll::Ready(e) => {
				first = first.take().or(e);
				false
			}
			Poll::Pending => true,
		});
		if let Some(e) = first {
			self.failed.get_or_insert(GpuError::Resources(e.to_string()));
		}
	}

	/// Keeps the first failure of a call that returns no `Result`.
	fn note(&mut self, result: Result<(), GpuError>) {
		if let Err(e) = result {
			self.failed.get_or_insert(e);
		}
	}

	/// Uploads the parameters with the count of the source groups and the EMA weights of the intensity, which
	/// follow the sources.
	fn upload_source_params(&mut self) {
		self.params.group_count = self.forcing.group_count;
		self.params.alpha = self.shared.alpha();
		self.params.rest = 1.0 - self.params.alpha;
		self.write_params();
	}

	fn write_params(&self) {
		self.ctx.queue.write_buffer(&self.grid.params, 0, bytemuck::bytes_of(&self.params));
	}

	/// Submits the commands recorded, even none, and requests the mappings of the captures and probe batches
	/// among them.
	fn submit(&mut self) -> Result<wgpu::SubmissionIndex, GpuError> {
		let index = self.batch.submit(&self.ctx);
		self.captures.submitted(&self.ctx, &index);
		self.probes.submitted(&self.ctx, &index);
		self.batch.pace(&self.ctx, index.clone())?;
		Ok(index)
	}

	/// Submits the commands recorded, if any.
	fn flush(&mut self) -> Result<(), GpuError> {
		if self.batch.is_recorded() {
			self.submit()?;
		}
		Ok(())
	}
}

impl WaveEngine for GpuEngine {
	fn name(&self) -> &'static str {
		"GPU"
	}

	fn is_gpu(&self) -> bool {
		true
	}

	fn scene(&self) -> &SceneData {
		&self.shared.scene
	}

	fn options(&self) -> &EngineOptions {
		&self.shared.options
	}

	fn time(&self) -> u32 {
		self.shared.time
	}

	fn sub_steps(&self) -> u32 {
		self.shared.sub_steps
	}

	fn reset(&mut self) {
		self.grid.clear(self.batch.encoder(&self.ctx.device));
		let submitted = self.submit();
		self.note(submitted.map(drop));
		self.grid.cur = 0;
		self.shared.reset();
		self.probes.drop_readings();
	}

	fn set_emitters(&mut self, emitters: &[Emitter]) {
		let flushed = self.flush();
		self.note(flushed);
		self.shared.set_emitters(emitters);
		self.forcing.regroup(&self.ctx, &self.kernels.force, &self.grid, &self.shared);
		self.upload_source_params();
	}

	fn set_probes(&mut self, probes: &[ProbeCells]) {
		let flushed = self.flush();
		self.note(flushed);
		self.probes.set(&self.ctx.device, &self.kernels.probes, &self.grid, &self.shared.scene, probes);
		self.params.probe_count = self.probes.count();
		self.write_params();
	}

	fn try_replace_scene(&mut self, scene: &Arc<SceneData>) -> bool {
		let Some(n) = self.shared.replacement(scene) else { return false };
		let medium = match medium::build(scene, n) {
			Ok(medium) => medium,
			Err(e) => {
				tracing::debug!("the replaced scene's medium needs a new GPU engine: {e}");
				return false;
			}
		};
		let flushed = self.flush();
		self.note(flushed);
		self.shared.replace_scene(scene.clone(), n);
		let device = &self.ctx.device;
		let resources = ResourceScope::open(device);
		self.kernels.set_cuts(device, medium.mirrors());
		self.passes = Passes::new(device, &self.kernels, &self.grid, &self.shared, &medium);
		(self.params.mur_count, self.params.d_base) = (self.passes.mur_count, medium.d_base);
		self.forcing = Forcing::new(&self.ctx, &self.kernels.force, &self.grid, &self.shared, medium.d);
		self.upload_source_params();
		self.caught.push(resources.close());
		self.settle();
		tracing::debug!(sub_steps = self.shared.sub_steps, "GPU engine scene replaced");
		true
	}

	/// The stages are allocated either way; the flag picks the kernels and the captured planes, and a change
	/// clears the stages behind the steps recorded, as the CPU engine's start from zero.
	fn set_track_intensity(&mut self, on: bool) {
		if self.shared.options.track_intensity != on {
			self.shared.options.track_intensity = on;
			self.grid.clear_stages(self.batch.encoder(&self.ctx.device));
		}
	}

	/// Pending while [`MAX_IN_FLIGHT`] submissions are on the card; ready once the engine failed or the device is
	/// lost, so the step reports it.
	#[cfg(target_arch = "wasm32")]
	fn poll_room(&mut self, cx: &mut Context<'_>) -> Poll<()> {
		let sound = self.check().is_ok();
		match self.batch.oldest_when_full() {
			Some(oldest) if sound => {
				if oldest.poll_ready(cx).is_ready() {
					Poll::Ready(())
				} else {
					self.ctx.lost_notify().poll_ready(cx)
				}
			}
			_ => Poll::Ready(()),
		}
	}

	fn advance_one_step(&mut self) -> Result<(), EngineError> {
		self.guarded(Self::step)
	}

	fn sample_probes(&mut self, out: &mut ProbeReadings) -> Result<(), EngineError> {
		self.guarded(|e| e.read_probes(out))
	}

	fn queue_probes(&mut self) -> Result<(), EngineError> {
		self.guarded(Self::queue_probe_reading)
	}

	fn pending_probes(&mut self) -> Result<PendingProbes, EngineError> {
		self.settle();
		self.check()?;
		self.probes.record_copy(&self.ctx.device, &mut self.batch);
		Ok(self.probes.pending(&self.ctx))
	}

	fn drain_probes(&mut self, pending: PendingProbes, f: &mut dyn FnMut(&ProbeReadings)) -> Result<(), EngineError> {
		self.guarded(|e| e.collect_probe_batches(pending.end()))?;
		self.probes.drain(pending.end(), f);
		Ok(())
	}

	fn begin_capture(&mut self, shape: &FrameShape, into: Snapshot) -> Result<PendingCapture, EngineError> {
		self.guarded(|e| e.begin(shape, into))
	}

	fn submit_pending(&mut self) -> Result<(), EngineError> {
		self.guarded(Self::flush)
	}

	fn capture_capacity(&self, shape: &FrameShape) -> usize {
		captures::capacity(shape, self.shared.options.track_intensity)
	}

	fn finish_capture(&mut self, pending: PendingCapture) -> Result<Snapshot, EngineError> {
		self.guarded(|e| e.finish(pending))
	}

	fn prepare_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), EngineError> {
		self.guarded(Self::read_frame)
	}

	fn device_frames(&mut self) -> Option<Arc<DeviceFrames>> {
		Some(self.captures.frames(&self.ctx))
	}
}

/// The bodies of the fallible [`WaveEngine`] calls, run through [`GpuEngine::guarded`].
impl GpuEngine {
	/// Runs `f` unless the engine failed before; a failure is kept.
	fn guarded<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, GpuError>) -> Result<T, EngineError> {
		self.settle();
		self.check()?;
		f(self).map_err(|e| {
			self.failed.get_or_insert(e.clone());
			e.into()
		})
	}

	/// Records a step: per sub-step the grid passes of the scheme, each followed on a rim by the CPML's four
	/// stages, the forcing where the scheme takes it, and Mur's condition on open edges without a rim.
	fn step(&mut self) -> Result<(), GpuError> {
		let first = self.forcing.upload(&self.ctx.queue, &mut self.shared, self.batch.steps);
		let (kernels, passes, forcing, params) = (&self.kernels, &self.passes, &self.forcing, &self.params);
		let cells = (params.gw.div_ceil(GRID_GROUP.0), params.gh.div_ceil(GRID_GROUP.1));
		let max = self.ctx.limits.max_compute_workgroups_per_dimension;
		let linear = |threads: u32| {
			let groups = threads.div_ceil(LINEAR_GROUP);
			(groups.min(max), groups.div_ceil(max))
		};
		let y_len = params.pml_len - params.pml_x_len;
		let (x_strips, y_strips) = (linear(params.pml_x_len), linear(y_len));
		let rim_cells = linear(params.rim_cells);
		let chains = (params.mur_count.div_ceil(LINEAR_GROUP), 1);
		let dispatch = |pass: &mut wgpu::ComputePass<'_>,
		                pipeline: &wgpu::ComputePipeline,
		                group: &wgpu::BindGroup,
		                (x, y): (u32, u32)| {
			pass.set_pipeline(pipeline);
			pass.set_bind_group(0, group, &[]);
			pass.dispatch_workgroups(x, y, 1);
		};
		let mut pass = self.batch.encoder(&self.ctx.device).begin_compute_pass(&Default::default());
		for k in 0..self.shared.sub_steps {
			let intensity = usize::from(self.shared.accumulates(k));
			let c = self.grid.cur;
			let step = &passes.step[c];
			let pml = &kernels.pml.pipelines;
			let apply = |pass: &mut wgpu::ComputePass<'_>, set: usize, correction: usize| {
				if let Some(groups) = passes.pml[c].get(set) {
					dispatch(pass, &pml[0], &groups.memory, y_strips);
					dispatch(pass, &pml[1], &groups.memory, y_strips);
					dispatch(pass, &pml[2], &groups.memory, x_strips);
					dispatch(pass, &pml[correction], &groups.correct, rim_cells);
				}
			};
			match self.shared.scheme {
				TimeScheme::Leapfrog => {
					dispatch(&mut pass, &kernels.step.pipelines[intensity], step, cells);
					apply(&mut pass, 0, 3);
					forcing.record(&mut pass, &kernels.force.pipelines[intensity], c, first, k);
				}
				TimeScheme::ModifiedEquation => {
					dispatch(&mut pass, &kernels.step.pipelines[0], step, cells);
					apply(&mut pass, 0, 3);
					forcing.record(&mut pass, &kernels.force.pipelines[0], c, first, k);
					dispatch(&mut pass, &kernels.step.pipelines[1 + intensity], step, cells);
					apply(&mut pass, 1, 4);
				}
			}
			if let Some(edges) = &passes.edges {
				dispatch(&mut pass, kernels.edges.pipeline(), &edges[c], chains);
			}
			self.grid.cur = 1 - c;
		}
		drop(pass);
		self.batch.steps += 1;
		self.shared.time += 1;
		if self.batch.steps == BATCH_STEPS {
			self.flush()?;
		}
		Ok(())
	}

	fn queue_probe_reading(&mut self) -> Result<(), GpuError> {
		let (device, cur) = (&self.ctx.device, self.grid.cur);
		if self.probes.queue_reading(device, &mut self.batch, &self.kernels.probes, cur) {
			self.probes.record_copy(device, &mut self.batch);
			// wasm32 cannot wait.
			self.collect_probe_batches(if cfg!(target_arch = "wasm32") { 0 } else { u64::MAX })?;
		}
		Ok(())
	}

	/// Moves the read-back batches into the host queue, oldest first: those mapped already, and those holding
	/// readings numbered below `until`, submitted and waited for.
	fn collect_probe_batches(&mut self, until: u64) -> Result<(), GpuError> {
		while let Some(recorded) = self.probes.next_batch(until) {
			if recorded {
				self.flush()?;
			}
			self.probes.collect(&self.ctx.device)?;
		}
		Ok(())
	}

	fn read_probes(&mut self, out: &mut ProbeReadings) -> Result<(), GpuError> {
		let (device, cur) = (&self.ctx.device, self.grid.cur);
		let mut readback = self.probes.record_sample(device, &mut self.batch, &self.kernels.probes, cur);
		if let Some(readback) = &mut readback {
			let index = self.submit()?;
			readback.map(&self.ctx, index);
			readback.wait(&self.ctx.device)?;
		}
		self.probes.sample(readback, out)
	}

	fn begin(&mut self, shape: &FrameShape, mut into: Snapshot) -> Result<PendingCapture, GpuError> {
		let scene = &self.shared.scene;
		assert_eq!((shape.canvas_width, shape.canvas_height), (scene.canvas_width, scene.canvas_height));
		let intensity = self.shared.options.track_intensity;
		if !self.captures.fits(shape, intensity) {
			self.flush()?;
			self.captures.grow(&self.ctx.device, &self.kernels.capture, &self.grid, shape, intensity);
		}
		let kernel = &self.kernels.capture;
		let (slot, mapped) = self.captures.record(&self.ctx, &mut self.batch, kernel, self.grid.cur, shape, intensity);
		into.shape = *shape;
		into.step = self.shared.time.saturating_sub(1);
		Ok(PendingCapture { snapshot: into, slot, mapped: Some(mapped) })
	}

	fn finish(&mut self, pending: PendingCapture) -> Result<Snapshot, GpuError> {
		let PendingCapture { mut snapshot, slot, .. } = pending;
		if self.captures.is_recorded(slot) {
			self.flush()?;
		}
		self.captures.collect(&self.ctx.device, slot, &mut snapshot, self.shared.options.track_intensity)?;
		Ok(snapshot)
	}

	fn read_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), GpuError> {
		let cells = self.shared.scene.padded_cells();
		let n = (cells * 4) as u64;
		let mut readback = Readback::new(&self.ctx.device, "readback", 3 * n);
		let encoder = self.batch.encoder(&self.ctx.device);
		for (k, plane) in [self.grid.current(), &self.grid.s1, &self.grid.s2].into_iter().enumerate() {
			encoder.copy_buffer_to_buffer(plane, 0, &readback.buffer, k as u64 * n, n);
		}
		let index = self.submit()?;
		readback.map(&self.ctx, index);
		readback.wait(&self.ctx.device)?;
		let bytes = readback.buffer.slice(..).get_mapped_range().map_err(GpuError::MapRange)?.to_vec();
		readback.buffer.unmap();
		let all: &[f32] = bytemuck::cast_slice(&bytes);
		let (u, rest) = all.split_at(cells);
		let (s1, s2) = rest.split_at(cells);
		Ok(self.shared.canvas_frame(u, Some((s1, s2))))
	}
}

impl Drop for GpuEngine {
	fn drop(&mut self) {
		let _ = self.flush();
	}
}

#[cfg(test)]
mod tests {
	use std::collections::VecDeque;

	use super::*;
	use crate::engine::types::SampleFormat;
	use crate::model::{Layer, PulseShape, Scene, Stencil};

	const SIZE: (u32, u32) = (64, 48);

	/// An engine with one emitter on the process's device; `None` without an adapter.
	fn engine() -> Option<GpuEngine> {
		match context() {
			Ok(ctx) => Some(engine_on(ctx)),
			Err(e) => {
				eprintln!("skipped: {e}");
				None
			}
		}
	}

	/// An engine with one emitter on `ctx`.
	fn engine_on(ctx: Arc<GpuContext>) -> GpuEngine {
		let scene = SceneData::blank(SIZE.0, SIZE.1, &Scene { damping_edge: 0, ..Scene::default() });
		let options = EngineOptions {
			stencil: Stencil::NinePoint,
			source_frequency_scale: 2.0,
			image_source_scale: 1.0,
			time_decay: 10.0,
			intensity_scale: 1.0,
			track_intensity: true,
		};
		let mut e = pollster::block_on(GpuEngine::create(ctx, scene, options)).expect("GPU engine");
		e.set_emitters(&[Emitter {
			x: 32.0,
			y: 24.0,
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
		}]);
		e
	}

	fn shape(divisor: u32, format: SampleFormat, brightness: f32) -> FrameShape {
		FrameShape {
			canvas_width: SIZE.0,
			canvas_height: SIZE.1,
			divisor,
			format,
			field_brightness: brightness,
			intensity_brightness: brightness,
			intensity_root: format == SampleFormat::Half,
		}
	}

	/// Two probes, one of each layer.
	fn probes() -> [ProbeCells; 2] {
		[
			ProbeCells { layer: Layer::Field, cells: vec![(30, 24), (10, 5), (33, 20)] },
			ProbeCells { layer: Layer::Intensity, cells: vec![(31, 25)] },
		]
	}

	/// Hands out the readings `pending` covers, appended to `into`.
	fn drain(e: &mut GpuEngine, pending: PendingProbes, into: &mut Vec<ProbeReadings>) {
		e.drain_probes(pending, &mut |r| into.push(r.clone())).unwrap();
	}

	fn steps(e: &mut GpuEngine, n: u32) {
		for _ in 0..n {
			e.advance_one_step().unwrap();
		}
	}

	#[test]
	fn a_scene_whose_medium_tables_overflow_leaves_the_engine_as_it_was() {
		let ctx = match context() {
			Ok(ctx) => ctx,
			Err(e) => return eprintln!("skipped: {e}"),
		};
		let edges = Scene { damping_edge: 0, ..Scene::default() };
		let uniform = SceneData::blank(300, 300, &edges);
		let mut varied = uniform.clone();
		// more distinct values than a table indexes
		for (i, c) in varied.c2.iter_mut().enumerate() {
			*c = 0.25 + i as f32 * 1e-6;
		}
		let options = EngineOptions {
			stencil: Stencil::NinePoint,
			source_frequency_scale: 1.0,
			image_source_scale: 1.0,
			time_decay: 10.0,
			intensity_scale: 1.0,
			track_intensity: false,
		};
		let mut e = pollster::block_on(GpuEngine::create(ctx, uniform, options)).expect("GPU engine");
		let (scene, n) = (e.shared.scene.clone(), e.shared.sub_steps);
		assert!(!e.try_replace_scene(&Arc::new(varied)));
		assert!(Arc::ptr_eq(&e.shared.scene, &scene) && e.shared.sub_steps == n, "the engine keeps its scene");
		e.advance_one_step().unwrap();
	}

	#[test]
	fn captures_ride_the_batch_of_their_steps() {
		let Some(mut e) = engine() else { return };
		let shape = shape(1, SampleFormat::Index, 1.0);
		let capacity = e.capture_capacity(&shape);
		let before = e.batch.submissions;
		let mut pending = VecDeque::new();
		let total = 4 * BATCH_STEPS;
		for i in 0..total {
			e.advance_one_step().unwrap();
			if i % 2 == 0 {
				if pending.len() == capacity {
					e.finish_capture(pending.pop_front().unwrap()).unwrap();
				}
				pending.push_back(e.begin_capture(&shape, Snapshot::new(shape)).unwrap());
			}
		}
		let submissions = e.batch.submissions - before;
		assert!(submissions <= (total / BATCH_STEPS + 1) as usize, "{submissions} submissions for {total} steps");
		for p in pending {
			e.finish_capture(p).unwrap();
		}
	}

	#[test]
	fn probe_readings_ride_the_batch_of_their_steps() {
		let Some(mut e) = engine() else { return };
		e.set_probes(&probes());
		let shape = shape(1, SampleFormat::Index, 1.0);
		let capacity = e.capture_capacity(&shape);
		let before = e.batch.submissions;
		let (mut pending, mut tokens) = (VecDeque::new(), Vec::new());
		let total = 4 * BATCH_STEPS;
		for i in 0..total {
			e.advance_one_step().unwrap();
			e.queue_probes().unwrap();
			if i % 2 == 0 {
				if pending.len() == capacity {
					e.finish_capture(pending.pop_front().unwrap()).unwrap();
				}
				pending.push_back(e.begin_capture(&shape, Snapshot::new(shape)).unwrap());
				tokens.push(e.pending_probes().unwrap());
			}
		}
		let submissions = e.batch.submissions - before;
		// Natively a full probe batch is also submitted, to be waited for.
		let most = 2 * total / BATCH_STEPS + 1;
		assert!(submissions <= most as usize, "{submissions} submissions for {total} steps");
		for p in pending {
			e.finish_capture(p).unwrap();
		}
		let mut readings = Vec::new();
		for token in tokens {
			drain(&mut e, token, &mut readings);
		}
		assert_eq!(readings.len(), total as usize - 1);
	}

	#[test]
	fn probe_readings_drained_in_pieces_are_those_drained_whole_and_those_sampled() {
		let (Some(mut pieces), Some(mut whole), Some(mut sampled)) = (engine(), engine(), engine()) else { return };
		for e in [&mut pieces, &mut whole, &mut sampled] {
			e.set_probes(&probes());
		}
		let total = 3 * BATCH_STEPS + 7;
		let (mut tokens, mut drained) = (VecDeque::new(), Vec::new());
		for i in 0..total {
			pieces.advance_one_step().unwrap();
			pieces.queue_probes().unwrap();
			if i % 3 == 0 {
				tokens.push_back(pieces.pending_probes().unwrap());
			}
			if i % 7 == 0
				&& let Some(token) = tokens.pop_front()
			{
				drain(&mut pieces, token, &mut drained);
			}
			if i % 11 == 0 {
				pieces.submit_pending().unwrap();
			}
		}
		tokens.push_back(pieces.pending_probes().unwrap());
		for token in tokens {
			drain(&mut pieces, token, &mut drained);
		}
		let mut at_once = Vec::new();
		for _ in 0..total {
			whole.advance_one_step().unwrap();
			whole.queue_probes().unwrap();
		}
		let token = whole.pending_probes().unwrap();
		drain(&mut whole, token, &mut at_once);
		let each: Vec<_> = (0..total)
			.map(|_| {
				sampled.advance_one_step().unwrap();
				let mut r = ProbeReadings::default();
				sampled.sample_probes(&mut r).unwrap();
				r
			})
			.collect();
		assert_eq!(each.len(), total as usize);
		assert!(each.iter().any(|r| r.probe(0).iter().any(|&v| v != 0.0)), "the readings are blank");
		assert!(drained == each, "the readings drained in pieces differ from those sampled");
		assert!(at_once == each, "the readings drained whole differ from those sampled");
	}

	#[test]
	fn captures_of_one_batch_read_their_own_parameters() {
		let (Some(mut together), Some(mut apart)) = (engine(), engine()) else { return };
		let shapes = [shape(1, SampleFormat::Index, 1.0), shape(2, SampleFormat::Half, 0.5)];
		let take = |e: &mut GpuEngine, submit_between: bool| {
			steps(e, 5);
			let first = e.begin_capture(&shapes[0], Snapshot::new(shapes[0])).unwrap();
			if submit_between {
				e.submit_pending().unwrap();
			}
			steps(e, 3);
			let submissions = e.batch.submissions;
			let second = e.begin_capture(&shapes[1], Snapshot::new(shapes[1])).unwrap();
			assert_eq!(e.batch.submissions, submissions, "a capture submitted");
			[first, second].map(|p| e.finish_capture(p).unwrap())
		};
		let (one, two) = (take(&mut together, false), take(&mut apart, true));
		for (a, b) in one.iter().zip(&two) {
			assert_eq!((a.shape, a.step), (b.shape, b.step));
			assert!(a.field == b.field && a.intensity == b.intensity, "{:?} differs", a.shape);
			assert!(a.field.iter().any(|&v| v != 0), "{:?} is blank", a.shape);
		}
	}

	#[test]
	fn the_context_awaits_each_completion_until_it_is_reported_and_nothing_after() {
		// A context of its own: the other tests submit on the process's.
		let ctx = match pollster::block_on(GpuContext::create()) {
			Ok(ctx) => ctx,
			Err(e) => {
				eprintln!("skipped: {e}");
				return;
			}
		};
		let land = || ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
		let mut e = engine_on(ctx.clone());
		land();
		assert!(!ctx.awaits_completions(), "an idle engine awaits nothing");
		steps(&mut e, 3);
		assert!(!ctx.awaits_completions(), "recorded steps are not submitted");
		// right after a submit the flag may be down already: a fast card's work is reported done within the submit
		e.submit_pending().unwrap();
		land();
		assert!(!ctx.awaits_completions(), "a batch run");
		let shape = shape(1, SampleFormat::Index, 1.0);
		steps(&mut e, 2);
		let capture = e.begin_capture(&shape, Snapshot::new(shape)).unwrap();
		e.submit_pending().unwrap();
		land();
		assert!(!ctx.awaits_completions(), "a mapping landed");
		e.finish_capture(capture).unwrap();
		ctx.kick();
		land();
		assert!(!ctx.awaits_completions(), "a kick awaits nothing");
		let (first, second) = (ctx.awaiting(), ctx.awaiting());
		assert!(ctx.awaits_completions(), "an outstanding completion");
		drop(first);
		assert!(ctx.awaits_completions(), "one more outstanding");
		drop(second);
		assert!(!ctx.awaits_completions(), "every completion reported");
	}
}
