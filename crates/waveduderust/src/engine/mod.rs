//! Wave engines: the scalar wave equation on a padded grid.
//!
//! One driver thread per engine, no internal locking. The CPU engine is the
//! reference; the GPU engine must agree within the tolerance in docs/physics.md.

pub mod capture;
pub mod cpu;
mod damage;
pub mod emitters;
pub mod envelope;
pub mod forcing;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod green;
mod isa;
mod mur;
pub mod pml;
pub mod poisson;
pub mod probes;
mod shared;
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
	/// `"CPU (<backend>)"` or "GPU", for logs and the engine's label.
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
	/// Tracks the intensity or stops; a change starts it from zero.
	fn set_track_intensity(&mut self, on: bool);
	/// Ready when a step can be taken without waiting for the device; otherwise `cx`'s waker is woken once it
	/// can. A step taken before may wait (natively) or queue past the engine's pacing (wasm32).
	fn poll_room(&mut self, _cx: &mut Context<'_>) -> Poll<()> {
		Poll::Ready(())
	}
	fn advance_one_step(&mut self) -> Result<(), EngineError>;
	/// The most steps [`advance_steps`](Self::advance_steps) is worth calling with: more than 1 for an engine that
	/// fuses steps (the CPU engine, engine/cpu), 1 for one that takes them one at a time. A caller batches its steps
	/// up to it between the events that need the engine between steps (captures, probe drains, checks).
	fn fused_steps(&self) -> u32 {
		1
	}
	/// Advances `n` steps, queueing a probe reading after each when `probes`: `n` times
	/// [`advance_one_step`](Self::advance_one_step), each followed by [`queue_probes`](Self::queue_probes) when
	/// `probes`, with the same results, which an engine may compute in fewer sweeps over its planes.
	fn advance_steps(&mut self, n: u32, probes: bool) -> Result<(), EngineError> {
		for _ in 0..n {
			self.advance_one_step()?;
			if probes {
				self.queue_probes()?;
			}
		}
		Ok(())
	}
	/// Reads the probe cells after the last step, at once.
	fn sample_probes(&mut self, out: &mut ProbeReadings) -> Result<(), EngineError>;
	/// Queues a reading of the probe cells after the last step; the way to record every step, as an engine
	/// that batches its steps reads a batch's readings back together.
	fn queue_probes(&mut self) -> Result<(), EngineError>;
	/// Reads back the readings queued so far, which the result covers, with the engine's next submission
	/// ([`submit_pending`](Self::submit_pending)); several readbacks may be in flight.
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
	/// Submits the work recorded so far, so the readbacks of the captures begun and of the probe readings pending
	/// are on their way; for a caller about to await one.
	fn submit_pending(&mut self) -> Result<(), EngineError> {
		Ok(())
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
	Built { engine: cpu_engine(scene, options), fallback: Some(why) }
}

/// The CPU engine ([`cpu::CpuEngine`]) on the backend [`cpu_backend`] chooses.
pub(crate) fn cpu_engine(scene: Arc<SceneData>, options: EngineOptions) -> Box<dyn WaveEngine> {
	Box::new(cpu::CpuEngine::with_backend(scene, options, cpu_backend()))
}

/// The backend of the CPU engines built: the best one this CPU runs ([`cpu::Backend::best`]: avx2 on x86-64 with AVX2
/// and FMA, simd128 in wasm built with it, else generic), or natively the one `WAVEDUDE_CPU_BACKEND` names
/// ([`backend_choice`]), read once.
fn cpu_backend() -> cpu::Backend {
	#[cfg(not(target_arch = "wasm32"))]
	{
		static CHOICE: std::sync::OnceLock<cpu::Backend> = std::sync::OnceLock::new();
		*CHOICE.get_or_init(|| backend_choice(std::env::var("WAVEDUDE_CPU_BACKEND").ok().as_deref()))
	}
	#[cfg(target_arch = "wasm32")]
	{
		cpu::Backend::best()
	}
}

/// The CPU engine's backend for `WAVEDUDE_CPU_BACKEND=value`: unset, the best; else the backend of that name
/// ([`cpu::Backend::named`]: `generic`, `avx2`), or the best, logged, when this build or this CPU lacks it.
#[cfg(not(target_arch = "wasm32"))]
fn backend_choice(value: Option<&str>) -> cpu::Backend {
	let Some(value) = value else {
		return cpu::Backend::best();
	};
	match cpu::Backend::named(value) {
		Some(backend) if backend.available() => backend,
		_ => {
			let best = cpu::Backend::best();
			tracing::warn!("WAVEDUDE_CPU_BACKEND={value}: no such backend runs here; the {} one runs", best.name());
			best
		}
	}
}

/// The engine for `scene`: the GPU's on `gpu` when given and it builds, else the CPU's.
pub async fn build_on(scene: Arc<SceneData>, options: EngineOptions, gpu: Option<GpuHandle>) -> Built {
	#[cfg(feature = "gpu")]
	if let Some(ctx) = gpu {
		return match gpu::GpuEngine::create(ctx, scene.clone(), options.clone()).await {
			Ok(e) => Built { engine: Box::new(e), fallback: None },
			Err(e) => cpu_instead(scene, options, e.into()),
		};
	}
	#[cfg(not(feature = "gpu"))]
	if let Some(never) = gpu {
		match never {}
	}
	Built { engine: cpu_engine(scene, options), fallback: None }
}

/// The engine for `scene`, blocking: the GPU's when `prefer_gpu` and one is available, else the CPU's.
#[cfg(not(target_arch = "wasm32"))]
pub fn build(scene: Arc<SceneData>, options: EngineOptions, prefer_gpu: bool) -> Built {
	if !prefer_gpu {
		return Built { engine: cpu_engine(scene, options), fallback: None };
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
	use super::backend_choice;
	use super::cpu::Backend;

	#[test]
	fn the_cpu_backend_follows_the_environment() {
		assert_eq!(backend_choice(None), Backend::best());
		assert_eq!(backend_choice(Some("generic")), Backend::Generic);
		assert_eq!(backend_choice(Some("nonesuch")), Backend::best());
		assert_eq!(backend_choice(Some("")), Backend::best());
		for &b in Backend::ALL {
			let want = if b.available() { b } else { Backend::best() };
			assert_eq!(backend_choice(Some(b.name())), want);
		}
	}

	#[test]
	fn the_best_backend_runs_here() {
		assert!(Backend::best().available());
		assert_eq!(Backend::ALL.last(), Some(&Backend::Generic));
		assert!(Backend::Generic.available());
		for &b in Backend::ALL {
			assert_eq!(Backend::named(b.name()), Some(b));
		}
	}
}
