//! Wave engines: the scalar wave equation on a padded grid.
//!
//! One driver thread per engine, no internal locking. The CPU engine is the
//! reference; the GPU engine must agree within the tolerance in docs/physics.md.

pub mod capture;
pub mod cpu;
pub mod emitters;
pub mod envelope;
pub mod forcing;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod green;
pub mod pml;
pub mod poisson;
pub mod probes;
pub mod stencil;
pub mod substeps;
pub mod types;

pub use types::*;

use std::sync::Arc;
use std::task::{Context, Poll};

use crate::sync::MaybeSend;

/// Why an engine call failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EngineError {
	#[cfg(feature = "gpu")]
	#[error(transparent)]
	Gpu(#[from] gpu::GpuError),
}

/// What every engine does. Positions and sizes are in grid cells, times in steps.
///
/// A GPU failure in a call that returns no `Result` is kept and returned by the next call that does.
pub trait WaveEngine: MaybeSend {
	/// "CPU" or "GPU", for logs.
	fn name(&self) -> &'static str;
	/// Whether the GPU runs the steps.
	fn is_gpu(&self) -> bool {
		false
	}
	fn scene(&self) -> &SceneData;
	fn options(&self) -> &EngineOptions;
	/// Steps simulated so far.
	fn time(&self) -> u32;
	fn sub_steps(&self) -> u32;
	/// Field, intensity, time and pulse states to zero.
	fn reset(&mut self);
	/// Replaces the object emitters (outside-canvas ones are dropped).
	fn set_emitters(&mut self, emitters: &[Emitter]);
	fn set_probes(&mut self, probes: &[ProbeCells]);
	/// Swaps in another scene of the same grid, edges and image-source count; false = rebuild needed.
	fn try_replace_scene(&mut self, scene: &Arc<SceneData>) -> bool;
	/// Tracks the intensity from the next reset on, or stops.
	fn set_track_intensity(&mut self, on: bool);
	/// Ready when a step can be taken without waiting for the device; otherwise `cx`'s waker is woken once it
	/// can. A step taken before may wait (natively) or queue past the engine's pacing (wasm32).
	fn poll_room(&mut self, _cx: &mut Context<'_>) -> Poll<()> {
		Poll::Ready(())
	}
	fn advance_one_step(&mut self) -> Result<(), EngineError>;
	/// Reads the probe cells after the last step, at once.
	fn sample_probes(&mut self, out: &mut ProbeReadings) -> Result<(), EngineError>;
	/// Queues a reading of the probe cells after the last step; the way to record every step, as an engine
	/// that batches its steps reads a batch's readings back together.
	fn queue_probes(&mut self) -> Result<(), EngineError>;
	/// Starts reading back the readings queued so far, which the result covers; several readbacks may be in
	/// flight.
	fn pending_probes(&mut self) -> Result<PendingProbes, EngineError>;
	/// Calls `f` with each reading `pending` covers not handed out yet, oldest first; one queued after `pending`
	/// was taken stays for a later token's drain, one dropped by `reset` or `set_probes` is not handed out.
	/// Natively it waits for the readings; on wasm32 the GPU engine fails with `GpuError::NotReady` if `pending`'s
	/// `ready` has not resolved, which only a native wait could make impossible.
	fn drain_probes(&mut self, pending: PendingProbes, f: &mut dyn FnMut(&ProbeReadings)) -> Result<(), EngineError>;
	/// Starts converting the canvas (rim excluded) after the last step into planes of `shape`, written into
	/// `into`'s planes (reused when large enough). The engine may step on before `finish_capture`
	/// (`Self::finish_capture`) collects it; at most [`capture_capacity`](Self::capture_capacity) captures may be
	/// pending.
	fn begin_capture(&mut self, shape: &FrameShape, into: Snapshot) -> Result<PendingCapture, EngineError>;
	/// How many captures of `shape` may be pending at once; at least two.
	fn capture_capacity(&self, _shape: &FrameShape) -> usize {
		2
	}
	/// The frame of a capture this engine began.
	fn finish_capture(&mut self, pending: PendingCapture) -> Result<Snapshot, EngineError>;
	/// A capture now, into new planes.
	fn capture_frame(&mut self, shape: &FrameShape) -> Result<Snapshot, EngineError> {
		let pending = self.begin_capture(shape, Snapshot::new(*shape))?;
		self.finish_capture(pending)
	}
	/// Host copies of the canvas field and blended intensity (tests).
	fn prepare_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), EngineError>;
	/// Where the captures from now on keep their planes on the engine's device, for render passes there; `None`
	/// for an engine on the host.
	#[cfg(feature = "gpu")]
	fn device_frames(&mut self) -> Option<Arc<gpu::DeviceFrames>> {
		None
	}
}

/// The GPU a built engine may run on.
#[cfg(feature = "gpu")]
pub type GpuHandle = Arc<gpu::GpuContext>;

/// Without the `gpu` feature there is no GPU to hand over.
#[cfg(not(feature = "gpu"))]
pub enum GpuHandle {}

/// Why the CPU engine runs where the GPU's was asked for.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Fallback {
	#[error("not built in")]
	NotBuiltIn,
	#[cfg(feature = "gpu")]
	#[error(transparent)]
	Gpu(#[from] gpu::GpuError),
}

/// A built engine and, when it is the CPU's in place of the GPU's asked for, why.
pub struct Built {
	pub engine: Box<dyn WaveEngine>,
	pub fallback: Option<Fallback>,
}

/// The CPU engine in place of the GPU's, the fallback logged.
#[cfg(any(feature = "gpu", not(target_arch = "wasm32")))]
fn cpu_instead(scene: Arc<SceneData>, options: EngineOptions, why: Fallback) -> Built {
	tracing::warn!("the GPU engine is not available ({why}); the CPU engine runs instead");
	Built { engine: Box::new(cpu::CpuEngine::new(scene, options)), fallback: Some(why) }
}

/// The engine for `scene`: the GPU's on `gpu` when given and it builds, else the CPU's.
pub async fn build_on(scene: Arc<SceneData>, options: EngineOptions, gpu: Option<GpuHandle>) -> Built {
	#[cfg(feature = "gpu")]
	if let Some(ctx) = gpu {
		return match gpu::GpuEngine::create(ctx, scene.clone(), options.clone(), gpu::MediumLayout::Compact).await {
			Ok(e) => Built { engine: Box::new(e), fallback: None },
			Err(e) => cpu_instead(scene, options, e.into()),
		};
	}
	#[cfg(not(feature = "gpu"))]
	if let Some(never) = gpu {
		match never {}
	}
	Built { engine: Box::new(cpu::CpuEngine::new(scene, options)), fallback: None }
}

/// The engine for `scene`, blocking: the GPU's when `prefer_gpu` and one is available, else the CPU's.
#[cfg(not(target_arch = "wasm32"))]
pub fn build(scene: Arc<SceneData>, options: EngineOptions, prefer_gpu: bool) -> Built {
	if !prefer_gpu {
		return Built { engine: Box::new(cpu::CpuEngine::new(scene, options)), fallback: None };
	}
	#[cfg(feature = "gpu")]
	{
		match gpu::context() {
			Ok(context) => pollster::block_on(build_on(scene, options, Some(context))),
			Err(e) => cpu_instead(scene, options, e.into()),
		}
	}
	#[cfg(not(feature = "gpu"))]
	{
		cpu_instead(scene, options, Fallback::NotBuiltIn)
	}
}

/// The GPU adapter's description, or why there is none; `None` when the GPU engine is not built in.
#[cfg(not(target_arch = "wasm32"))]
pub fn gpu_adapter() -> Option<Result<String, String>> {
	#[cfg(feature = "gpu")]
	return Some(match gpu::context() {
		Ok(c) => Ok(format!("{} ({:?}, {:?})", c.info.name, c.info.backend, c.info.device_type)),
		Err(e) => Err(e.to_string()),
	});
	#[cfg(not(feature = "gpu"))]
	None
}
