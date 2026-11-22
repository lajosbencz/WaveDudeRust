//! A project run to its end in the background: [`SimulationBuilder::spawn`](crate::SimulationBuilder::spawn).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::{FrameStore, Goal, RunError, RunWorker, Status, Wanted, WorkState};
use crate::engine::probes::ProbeTrace;
use crate::engine::{FrameShape, SampleFormat};
use crate::model::Layer;
use crate::model::{Probe, Project};
use crate::quality::{Brightness, PreviewQuality};
use crate::spec::{RunRequest, SceneCache, Schedule};
use crate::{Backend, FieldView, SimulationError};

/// The one goal a background run follows.
const REVISION: u64 = 1;

/// How a background run stores its frames.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct RunOptions {
	/// The frames' resolution and sample format; [`PreviewQuality::FULL`], every cell as a half, by default.
	pub quality: PreviewQuality,
	/// Baked into 8-bit frames; a default [`View`](crate::model::View)'s by default.
	pub brightness: Brightness,
	/// Where the frames past memory spill; `<temp>/wavedude` when `None`.
	pub cache_dir: Option<PathBuf>,
}

impl Default for RunOptions {
	fn default() -> Self {
		Self { quality: PreviewQuality::FULL, brightness: Brightness::default(), cache_dir: None }
	}
}

impl RunOptions {
	pub fn quality(mut self, quality: PreviewQuality) -> Self {
		self.quality = quality;
		self
	}

	pub fn brightness(mut self, brightness: Brightness) -> Self {
		self.brightness = brightness;
		self
	}

	pub fn cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
		self.cache_dir = Some(dir.into());
		self
	}
}

/// Where a background run is.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Stage {
	Running,
	Completed,
	Cancelled,
	Failed(Arc<RunError>),
}

/// A background run's progress.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Progress {
	pub stage: Stage,
	/// Steps done; every step once completed.
	pub steps_done: u32,
	/// The run's steps.
	pub steps: u32,
	/// Timeline frames stored so far.
	pub frames: u32,
}

impl Progress {
	/// Completed, cancelled or failed.
	pub fn ended(&self) -> bool {
		!matches!(self.stage, Stage::Running)
	}
}

/// Stops a background run from any thread.
#[derive(Clone)]
pub struct Canceller {
	cancelled: Arc<AtomicBool>,
	wake: Arc<dyn Fn() + Send + Sync>,
}

impl Canceller {
	/// Stops stepping and returns the waits; the frames stored so far stay readable.
	pub fn cancel(&self) {
		self.cancelled.store(true, Ordering::Release);
		(self.wake)();
	}
}

/// A project running to its end on a thread of its own; dropping it stops the run.
pub struct Run {
	worker: RunWorker,
	request: RunRequest,
	schedule: Schedule,
	probes: Arc<[Probe]>,
	canceller: Canceller,
	paused: Mutex<bool>,
}

impl Run {
	/// A run of the normalised `project`.
	pub(crate) fn start(
		project: &Project,
		backend: Backend,
		track_intensity: bool,
		options: RunOptions,
	) -> Result<Self, SimulationError> {
		let request = RunRequest::new(project, options.quality, options.brightness, track_intensity);
		let scene = Arc::<Mutex<SceneCache>>::default();
		let spec = request.spec(&mut crate::sync::lock(&scene))?;
		let dir = options.cache_dir.unwrap_or_else(FrameStore::default_dir);
		let worker = RunWorker::new(super::worker::engines(backend), dir, scene)
			.map_err(|e| SimulationError::Io(Arc::new(e)))?;
		worker.set_goal(Goal {
			revision: REVISION,
			run: Some(request.clone()),
			wanted: Wanted::All,
			..Goal::default()
		});
		let canceller = Canceller { cancelled: Arc::default(), wake: Arc::new(worker.waker()) };
		Ok(Self { worker, request, schedule: spec.schedule, probes: spec.probes, canceller, paused: Mutex::new(false) })
	}

	pub fn progress(&self) -> Progress {
		self.progress_of(&self.worker.status())
	}

	/// Blocks until the run ended or `stop` holds; the progress then. `stop` is asked again on every change of the
	/// run's state and progress, with the run's status locked: it must not call this run's other methods.
	pub fn wait_until(&self, mut stop: impl FnMut(&Progress) -> bool) -> Progress {
		let mut last = None;
		self.worker.wait_until(|s| {
			let p = self.progress_of(s);
			let done = p.ended() || stop(&p);
			last = Some(p);
			done
		});
		self.pause_if_cancelled();
		last.unwrap_or_else(|| self.progress())
	}

	/// Blocks until the run ended; an error when it failed.
	pub fn wait(&self) -> Result<Progress, Arc<RunError>> {
		let p = self.wait_until(|_| false);
		match p.stage {
			Stage::Failed(e) => Err(e),
			_ => Ok(p),
		}
	}

	pub fn cancel(&self) {
		self.canceller.cancel();
		self.pause_if_cancelled();
	}

	/// A handle that cancels this run from another thread or a signal handler.
	pub fn canceller(&self) -> Canceller {
		self.canceller.clone()
	}

	/// The run's frames once its store exists.
	#[cfg(feature = "unstable-run")]
	pub fn store(&self) -> Option<Arc<FrameStore>> {
		self.worker.store()
	}

	/// The frames stored so far, once the run has its store.
	pub fn frames(&self) -> Option<Frames> {
		self.worker.store().map(|store| Frames { store, schedule: self.schedule })
	}

	/// Blocks until the run can hand out its frames; an error when the run failed first.
	pub fn wait_for_frames(&self) -> Result<Frames, Arc<RunError>> {
		Ok(Frames { store: self.store_when_ready()?, schedule: self.schedule })
	}

	/// Blocks until the run's store exists; an error when the run failed first.
	#[cfg(feature = "unstable-run")]
	pub fn wait_for_store(&self) -> Result<Arc<FrameStore>, Arc<RunError>> {
		self.store_when_ready()
	}

	pub fn schedule(&self) -> Schedule {
		self.schedule
	}

	/// The active probes, in project order.
	pub fn probes(&self) -> &[Probe] {
		&self.probes
	}

	/// Each active probe's readings so far, in project order.
	pub fn traces(&self) -> Vec<ProbeTrace> {
		self.worker.with_traces(<[ProbeTrace]>::to_vec)
	}

	fn store_when_ready(&self) -> Result<Arc<FrameStore>, Arc<RunError>> {
		let s = self.worker.wait_until(|s| s.store.is_some() || matches!(s.state, WorkState::Failed(_)));
		match s.state {
			WorkState::Failed(e) => Err(e),
			_ => Ok(s.store.expect("the wait ends on a store or a failure")),
		}
	}

	fn cancelled(&self) -> bool {
		self.canceller.cancelled.load(Ordering::Acquire)
	}

	/// The worker stops stepping once cancelled; the goal is set from the caller's thread, never from a waker.
	fn pause_if_cancelled(&self) {
		let mut paused = crate::sync::lock(&self.paused);
		if self.cancelled() && !*paused {
			*paused = true;
			let run = Some(self.request.clone());
			self.worker.set_goal(Goal {
				revision: REVISION,
				run,
				wanted: Wanted::All,
				paused: true,
				..Goal::default()
			});
		}
	}

	fn progress_of(&self, s: &Status) -> Progress {
		let completed = s.state == WorkState::Idle && s.adopted == REVISION;
		let stage = match &s.state {
			WorkState::Failed(e) => Stage::Failed(e.clone()),
			_ if completed => Stage::Completed,
			_ if self.cancelled() => Stage::Cancelled,
			_ => Stage::Running,
		};
		let steps = self.schedule.steps;
		let frames = s.store.as_ref().map_or(0, |store| self.schedule.covered_frames(store.count() as u32));
		// Past the last frame only the probes are stepped: a completed run reports every step.
		let steps_done = if completed { steps } else { s.steps_done };
		Progress { stage, steps_done, steps, frames }
	}
}

/// A run's timeline frames, read while the run goes on or after it ended.
#[derive(Clone)]
pub struct Frames {
	store: Arc<FrameStore>,
	schedule: Schedule,
}

impl Frames {
	/// Timeline frames stored so far, from the first.
	pub fn len(&self) -> u32 {
		match self.store.present_through(0) {
			Some(last) => self.schedule.covered_frames(last as u32 + 1),
			None => 0,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Timeline frames of the whole run.
	pub fn total(&self) -> u32 {
		self.schedule.frame_count()
	}

	/// Steps done in the frame [`get`](Self::get) returns for timeline frame `k`: with a frame divisor, the
	/// stored frame's.
	pub fn steps_at(&self, k: u32) -> u32 {
		let s = &self.schedule;
		s.step_of(s.stored_index_of(k) * s.frame_divisor()) + 1
	}

	/// Timeline frame `k`, `None` while it is not stored. A frame stored at a resolution divisor is that much
	/// smaller than the canvas; with [`SampleFormat::Index`](crate::quality::SampleFormat::Index) its values are the 255 levels of the display's
	/// brightness, clipped there.
	pub fn get(&self, k: u32) -> Result<Option<FieldView>, RunError> {
		let stored = self.schedule.stored_index_of(k) as usize;
		if k >= self.total() || !self.store.has(stored) {
			return Ok(None);
		}
		let shape = *self.store.shape();
		let layers = self.store.layers();
		let field = decode(&shape, Layer::Field, &self.store.copy_layer(stored, Layer::Field)?);
		let intensity = match layers.contains(&Layer::Intensity) {
			true => Some(decode(&shape, Layer::Intensity, &self.store.copy_layer(stored, Layer::Intensity)?)),
			false => None,
		};
		Ok(Some(FieldView { width: shape.width() as usize, height: shape.height() as usize, field, intensity }))
	}
}

/// A stored plane of `layer` as values: the field, or the intensity (not its root).
fn decode(shape: &FrameShape, layer: Layer, plane: &[u8]) -> Vec<f32> {
	match (shape.format, layer) {
		(SampleFormat::Half, Layer::Field) => halves(plane).collect(),
		(SampleFormat::Half, Layer::Intensity) if shape.intensity_root => halves(plane).map(|r| r * r).collect(),
		(SampleFormat::Half, Layer::Intensity) => halves(plane).collect(),
		(SampleFormat::Index, Layer::Field) => {
			let b = shape.field_brightness;
			plane.iter().map(|&i| (f32::from(i) - 127.0) / 127.0 / b).collect()
		}
		(SampleFormat::Index, Layer::Intensity) => {
			let b = shape.intensity_brightness;
			plane.iter().map(|&i| (f32::from(i) / 254.0 / b).powi(2)).collect()
		}
	}
}

fn halves(plane: &[u8]) -> impl Iterator<Item = f32> + '_ {
	plane.as_chunks::<2>().0.iter().map(|b| half::f16::from_ne_bytes(*b).to_f32())
}
