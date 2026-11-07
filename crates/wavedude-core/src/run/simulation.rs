//! A simulation run: the stepping loop on a thread of its own, captures into the store on a writer thread
//! (spec/40-frames-store-render.md, Run schedule).
//!
//! Threads: [`SimulationRun::start`] spawns the driver thread, which owns the engine and the probe collector,
//! and a writer thread, which appends the captured snapshots to the store (at most two queued: the back
//! pressure when the writer lags). Each capture goes into a buffer the store hands out and is collected after
//! the next capture has begun, so the engine's readback overlaps the steps in between. Control (pause, resume, cancel) and the published state may be used from
//! any thread. Events are delivered on the driver thread; progress at most 30 times a second.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::engine::probes::{ProbeCollector, ProbeTrace};
use crate::engine::{EngineError, PendingCapture, Snapshot, WaveEngine};

use super::spec::RunSpec;
use super::store::{FrameStore, StoreError};

const HEARTBEAT: Duration = Duration::from_millis(33);
const WRITE_QUEUE: usize = 2;

/// Why a run or a scratch frame failed.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
	#[error("frame store: {0}")]
	Store(#[from] StoreError),
	#[error("engine: {0}")]
	Engine(#[from] EngineError),
	#[error("could not start a thread: {0}")]
	Thread(#[source] std::io::Error),
	#[error("{0} panicked: {1}")]
	Panic(&'static str, String),
	#[error("the run was cancelled")]
	Cancelled,
	#[error(transparent)]
	Failed(Arc<RunError>),
}

impl RunError {
	/// The panic of `what`, its payload's message when it has one.
	pub fn panic(what: &'static str, payload: &(dyn std::any::Any + Send)) -> Self {
		let message = payload
			.downcast_ref::<String>()
			.cloned()
			.or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
			.unwrap_or_default();
		Self::Panic(what, message)
	}
}

/// Equality compares the variant; two failures are equal when they share the error.
#[derive(Clone, Debug)]
pub enum RunState {
	Created,
	Running,
	Paused,
	Completed,
	Cancelled,
	Failed(Arc<RunError>),
}

impl PartialEq for RunState {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Failed(a), Self::Failed(b)) => Arc::ptr_eq(a, b),
			_ => std::mem::discriminant(self) == std::mem::discriminant(other),
		}
	}
}

impl Eq for RunState {}

impl RunState {
	pub fn is_finished(&self) -> bool {
		matches!(self, Self::Completed | Self::Cancelled | Self::Failed(_))
	}

	pub fn is_active(&self) -> bool {
		matches!(self, Self::Running | Self::Paused)
	}
}

/// What a run reports while it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunEvent {
	/// Steps done and timeline frames the store covers; paced, and once more at the end.
	Progress { steps_done: u32, frames: u32 },
	/// Every change of state; a finished state is the last event.
	State(RunState),
}

/// Called on the driver thread.
pub type EventSink = Box<dyn Fn(RunEvent) + Send + Sync>;

#[derive(Default)]
struct Control {
	paused: bool,
	cancelled: bool,
}

struct Shared {
	control: Mutex<Control>,
	wake: Condvar,
	state: Mutex<RunState>,
	steps_done: AtomicU32,
	frames: AtomicU32,
	finished: AtomicBool,
	traces: Mutex<Vec<ProbeTrace>>,
	sink: Option<EventSink>,
}

impl Shared {
	fn emit(&self, event: RunEvent) {
		if let Some(sink) = &self.sink {
			sink(event);
		}
	}

	fn set_state(&self, state: RunState) {
		*self.state.lock().unwrap_or_else(PoisonError::into_inner) = state.clone();
		self.emit(RunEvent::State(state));
	}

	/// Blocks while paused; false once cancelled.
	fn gate(&self) -> bool {
		let mut control = self.control.lock().unwrap_or_else(PoisonError::into_inner);
		if control.paused && !control.cancelled {
			drop(control);
			self.set_state(RunState::Paused);
			control = self.control.lock().unwrap_or_else(PoisonError::into_inner);
			while control.paused && !control.cancelled {
				control = self.wake.wait(control).unwrap_or_else(PoisonError::into_inner);
			}
			if !control.cancelled {
				drop(control);
				self.set_state(RunState::Running);
				return true;
			}
		}
		!control.cancelled
	}
}

/// A run of a [`RunSpec`] into a [`FrameStore`]. Dropping it cancels and joins it.
pub struct SimulationRun {
	spec: RunSpec,
	store: Arc<FrameStore>,
	shared: Arc<Shared>,
	thread: Option<JoinHandle<Box<dyn WaveEngine>>>,
	engine: Option<Box<dyn WaveEngine>>,
}

impl SimulationRun {
	/// Starts `spec` on `engine` (built for the spec's scene and options; it is reset and given the spec's
	/// emitters and probes) into `store`, which must have the spec's shape, layers and at least its stored frames.
	pub fn start(
		spec: RunSpec,
		store: Arc<FrameStore>,
		engine: Box<dyn WaveEngine>,
		sink: Option<EventSink>,
	) -> std::io::Result<Self> {
		let layout = store.layout();
		assert!(
			layout.shape == spec.shape
				&& layout.layers == spec.layers()
				&& layout.capacity >= spec.stored_frames() as usize,
			"the frame store does not fit the run"
		);
		let shared = Arc::new(Shared {
			control: Mutex::default(),
			wake: Condvar::new(),
			state: Mutex::new(RunState::Created),
			steps_done: AtomicU32::new(0),
			frames: AtomicU32::new(0),
			finished: AtomicBool::new(false),
			traces: Mutex::new(Vec::new()),
			sink,
		});
		let thread = {
			let (spec, store, shared) = (spec.clone(), store.clone(), shared.clone());
			std::thread::Builder::new()
				.name("simulation-run".into())
				.spawn(move || drive(spec, store, engine, shared))?
		};
		Ok(Self { spec, store, shared, thread: Some(thread), engine: None })
	}

	pub fn spec(&self) -> &RunSpec {
		&self.spec
	}

	pub fn store(&self) -> &Arc<FrameStore> {
		&self.store
	}

	pub fn state(&self) -> RunState {
		self.shared.state.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	pub fn steps_done(&self) -> u32 {
		self.shared.steps_done.load(Ordering::Acquire)
	}

	/// Timeline frames the stored frames cover.
	pub fn frame_count(&self) -> u32 {
		self.shared.frames.load(Ordering::Acquire)
	}

	/// The run ended (completed, cancelled or failed) and its final state is published.
	pub fn is_finished(&self) -> bool {
		self.shared.finished.load(Ordering::Acquire)
	}

	/// The probe traces in the spec's probe order, up to the last heartbeat (all of them once finished).
	pub fn traces(&self) -> Vec<ProbeTrace> {
		self.shared.traces.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	pub fn with_traces<R>(&self, f: impl FnOnce(&[ProbeTrace]) -> R) -> R {
		f(&self.shared.traces.lock().unwrap_or_else(PoisonError::into_inner))
	}

	pub fn pause(&self) {
		self.shared.control.lock().unwrap_or_else(PoisonError::into_inner).paused = true;
	}

	pub fn resume(&self) {
		self.shared.control.lock().unwrap_or_else(PoisonError::into_inner).paused = false;
		self.shared.wake.notify_all();
	}

	pub fn is_pause_requested(&self) -> bool {
		self.shared.control.lock().unwrap_or_else(PoisonError::into_inner).paused
	}

	pub fn cancel(&self) {
		self.shared.control.lock().unwrap_or_else(PoisonError::into_inner).cancelled = true;
		self.shared.wake.notify_all();
	}

	/// Waits for the end of the run and returns its final state.
	pub fn wait(&mut self) -> RunState {
		self.join();
		self.state()
	}

	/// Waits for the end of the run; an error unless it completed.
	pub fn wait_completed(&mut self) -> Result<(), RunError> {
		match self.wait() {
			RunState::Completed => Ok(()),
			RunState::Cancelled => Err(RunError::Cancelled),
			RunState::Failed(e) => Err(RunError::Failed(e)),
			s @ (RunState::Created | RunState::Running | RunState::Paused) => unreachable!("a joined run is {s:?}"),
		}
	}

	/// Joins the driver; a panic of its own (outside the engine's, which `drive` catches) fails the run.
	fn join(&mut self) {
		if let Some(t) = self.thread.take() {
			match t.join() {
				Ok(engine) => self.engine = Some(engine),
				Err(panic) => {
					let error = RunError::panic("the run's driver", panic.as_ref());
					tracing::error!("{error}");
					*self.shared.state.lock().unwrap_or_else(PoisonError::into_inner) =
						RunState::Failed(Arc::new(error));
					self.shared.finished.store(true, Ordering::Release);
				}
			}
		}
	}

	/// Waits for the end of the run and hands back its engine for reuse (as the run left it: reset it).
	/// `None` when the driver thread itself panicked.
	pub fn into_engine(mut self) -> Option<Box<dyn WaveEngine>> {
		self.join();
		self.engine.take()
	}
}

impl Drop for SimulationRun {
	fn drop(&mut self) {
		if self.thread.is_some() {
			self.cancel();
			self.join();
		}
	}
}

fn drive(
	spec: RunSpec,
	store: Arc<FrameStore>,
	mut engine: Box<dyn WaveEngine>,
	shared: Arc<Shared>,
) -> Box<dyn WaveEngine> {
	let started = Instant::now();
	let result = catch_unwind(AssertUnwindSafe(|| step_all(&spec, &store, engine.as_mut(), &shared)));
	let state = match result {
		Ok(Ok(true)) => RunState::Completed,
		Ok(Ok(false)) => RunState::Cancelled,
		Ok(Err(e)) => RunState::Failed(Arc::new(e)),
		Err(panic) => RunState::Failed(Arc::new(RunError::panic("the engine", panic.as_ref()))),
	};
	let frames = spec.covered_frames(store.count() as u32);
	shared.frames.store(frames, Ordering::Release);
	let steps_done = shared.steps_done.load(Ordering::Acquire);
	tracing::info!(
		state = ?state,
		steps_done,
		steps = spec.steps,
		frames,
		in_memory = store.frames_in_memory(),
		on_disk = store.frames_on_disk(),
		seconds = started.elapsed().as_secs_f64(),
		"run finished"
	);
	shared.emit(RunEvent::Progress { steps_done, frames });
	*shared.state.lock().unwrap_or_else(PoisonError::into_inner) = state.clone();
	shared.finished.store(true, Ordering::Release);
	shared.emit(RunEvent::State(state));
	engine
}

/// The stepping loop; Ok(false) when cancelled.
fn step_all(
	spec: &RunSpec,
	store: &Arc<FrameStore>,
	engine: &mut dyn WaveEngine,
	shared: &Shared,
) -> Result<bool, RunError> {
	let mut collector = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene));
	*shared.traces.lock().unwrap_or_else(PoisonError::into_inner) =
		collector.as_ref().map(|c| c.traces.clone()).unwrap_or_default();
	engine.set_emitters(&spec.emitters);
	engine.set_probes(collector.as_ref().map(ProbeCollector::cells).unwrap_or_default());
	engine.reset();
	shared.set_state(RunState::Running);
	let (tx, rx) = sync_channel::<Snapshot>(WRITE_QUEUE);
	let writer = {
		let store = store.clone();
		let frame_divisor = spec.frame_divisor() as u64;
		let frame_count = spec.frame_count() as u64;
		move || -> Result<(), RunError> {
			for snapshot in rx {
				let frame = store.write(snapshot)?;
				let covered = ((frame as u64 + 1) * frame_divisor).min(frame_count);
				shared.frames.store(covered as u32, Ordering::Release);
			}
			Ok(())
		}
	};
	std::thread::scope(|scope| {
		let writer = std::thread::Builder::new()
			.name("frame-writer".into())
			.spawn_scoped(scope, writer)
			.map_err(RunError::Thread)?;
		let stepped = step_loop(spec, store, engine, shared, collector.as_mut(), &tx);
		drop(tx);
		let written = writer.join().map_err(|p| RunError::panic("the frame writer", p.as_ref()))?;
		publish(shared, collector.as_mut(), shared.steps_done.load(Ordering::Acquire));
		let completed = stepped?;
		written.map(|()| completed)
	})
}

/// Steps the run and hands its captures to `tx`; Ok(false) when cancelled or the writer is gone. A capture
/// begun is always finished, so the engine can be reused.
fn step_loop(
	spec: &RunSpec,
	store: &FrameStore,
	engine: &mut dyn WaveEngine,
	shared: &Shared,
	mut collector: Option<&mut ProbeCollector>,
	tx: &SyncSender<Snapshot>,
) -> Result<bool, RunError> {
	let every = spec.visualize_every;
	let capture_every = spec.capture_every();
	let mut captured = 0usize;
	let mut pending: Option<PendingCapture> = None;
	let mut recorded = 0u32;
	let mut last_beat = Instant::now();
	let mut completed = true;
	for i in 0..spec.steps {
		// A paused run shows its last capture and its probes so far.
		if shared.control.lock().unwrap_or_else(PoisonError::into_inner).paused {
			if let Some(c) = collector.as_deref_mut() {
				record(engine, c, &mut recorded, every)?;
			}
			publish(shared, collector.as_deref_mut(), i);
			if let Some(last) = pending.take()
				&& tx.send(engine.finish_capture(last)?).is_err()
			{
				completed = false;
				break;
			}
		}
		if !shared.gate() {
			completed = false;
			break;
		}
		engine.advance_one_step()?;
		let done = i + 1;
		if collector.is_some() {
			engine.queue_probes()?;
		}
		shared.steps_done.store(done, Ordering::Release);
		if i % capture_every == 0 {
			debug_assert!(captured < store.capacity(), "more captures than the store holds");
			let next = engine.begin_capture(&spec.shape, store.next_buffer())?;
			captured += 1;
			if let Some(previous) = pending.replace(next)
				&& tx.send(engine.finish_capture(previous)?).is_err()
			{
				completed = false;
				break;
			}
		}
		if last_beat.elapsed() >= HEARTBEAT {
			last_beat = Instant::now();
			if let Some(c) = collector.as_deref_mut() {
				record(engine, c, &mut recorded, every)?;
			}
			publish(shared, collector.as_deref_mut(), done);
		}
	}
	if let Some(c) = collector {
		record(engine, c, &mut recorded, every)?;
	}
	if let Some(last) = pending {
		let _ = tx.send(engine.finish_capture(last)?);
	}
	Ok(completed)
}

/// Records the engine's queued probe readings, the steps after `recorded`, into `collector`: every step's
/// values, and the line profiles of each frame's step.
pub(crate) fn record(
	engine: &mut dyn WaveEngine,
	collector: &mut ProbeCollector,
	recorded: &mut u32,
	every: u32,
) -> Result<(), EngineError> {
	engine.drain_probes(&mut |readings| {
		let step = u64::from(*recorded) + 1;
		collector.record_step(readings, step);
		if (*recorded).is_multiple_of(every) {
			collector.record_frame(readings, step);
		}
		*recorded += 1;
	})
}

/// Moves the samples and profiles recorded since the last heartbeat into the shared traces.
fn publish(shared: &Shared, collector: Option<&mut ProbeCollector>, steps_done: u32) {
	if let Some(c) = collector {
		let mut traces = shared.traces.lock().unwrap_or_else(PoisonError::into_inner);
		for (out, t) in traces.iter_mut().zip(&mut c.traces) {
			out.series.append(&mut t.series);
			out.profiles.append(&mut t.profiles);
			out.steps_sampled = t.steps_sampled;
		}
	}
	shared.emit(RunEvent::Progress { steps_done, frames: shared.frames.load(Ordering::Acquire) });
}
