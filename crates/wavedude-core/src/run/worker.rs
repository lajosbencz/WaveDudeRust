//! The live run: one worker thread, one engine and one store that converge on a [`Goal`] (spec/30-engine.md,
//! The live run).
//!
//! The goal names the project state ([`RunRequest`], whose spec the worker builds, composing the scene off
//! the UI thread), the timeline position, how much is wanted ([`Wanted`]) and whether the user paused. Between two frames the worker reads the newest goal and derives its next target: the first
//! stored frame at or after the position that is missing, or whose probe readings are not taken yet; with
//! [`Wanted::All`], when none follows, the first such frame before it. It steps to the target (from 0 when the
//! engine has passed it), captures it when missing and, with everything wanted, the missing frames it passes,
//! records the probes on the way and publishes progress and traces. A spec whose frames differ starts a generation: the store is cleared
//! (its buffers kept), the engine reused (same scene: new emitters; same grid: the scene swapped in; else
//! rebuilt). A spec whose probes alone differ clears the traces and keeps the frames. Discarded frames go with
//! their store: the spec in force gets a new one, so the old one's memory and spill are freed once its readers
//! let go, and the engine starts over. A goal change is looked at every [`CHECK_INTERVAL`] steps: work goes on
//! only for the goal's own spec, so an edit means one run.
//!
//! Threads: [`RunWorker::set_goal`] and the readers may be called from any thread; `RunWorker::new` runs the work
//! on one thread, which owns the engine and writes the store. Of a [`RunWorker::pumped`], the only kind on wasm32,
//! the caller works the run in slices with [`RunWorker::pump`]: a slice ends at its deadline, at a pause, with as
//! many captures in flight as the engine takes, with as many probe batches in flight as it lets on its way, at probe
//! readings to be drained whole (before the run idles, pauses or resets) that are not mapped yet, while an engine is
//! on its way (the old one freed first, as on a thread), while the spill has as many writes in flight as it takes
//! and while the free storage a new store's spill is checked against is not known yet; the next goes on from the
//! steps done. Traces and progress are published at heartbeats and at every frame.

use crate::sync::lock;
use std::collections::VecDeque;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::task::{Context, Waker};
use std::time::Duration;

use web_time::Instant;

#[cfg(feature = "gpu")]
use crate::engine::gpu::{DeviceFrames, FrameTag};
use crate::engine::probes::{ProbeCollector, ProbeTrace, record};
use crate::engine::{
	EngineOptions, FrameShape, PendingCapture, PendingProbes, SampleFormat, SceneData, Snapshot, WaveEngine,
};
use crate::sync::MaybeSend;

use super::RunError;
use super::pending::SpillFactory;
use super::store::{FrameStore, StoreLayout};
use crate::spec::{RunRequest, RunSpec, SceneCache};

/// Steps between two looks at the pause and shutdown flags.
pub const CHECK_INTERVAL: u32 = 16;
/// Steps between two drains of the engine's queued probe readings.
const PROBE_DRAIN: u32 = 256;
/// Probe batches on their way to the host at which a pumped run yields at a drain on the way to its target.
const PROBE_BATCHES_IN_FLIGHT: usize = 16;
const HEARTBEAT: Duration = Duration::from_millis(33);
/// How long a new store's producer waits for the readers of the store it replaces.
#[cfg(not(target_arch = "wasm32"))]
const RELEASE_WAIT: Duration = Duration::from_secs(1);

/// Builds an engine for a scene and options.
#[cfg(not(target_arch = "wasm32"))]
pub trait BuildEngine: Fn(Arc<SceneData>, EngineOptions) -> Box<dyn WaveEngine> + MaybeSend {}

#[cfg(not(target_arch = "wasm32"))]
impl<F: Fn(Arc<SceneData>, EngineOptions) -> Box<dyn WaveEngine> + MaybeSend> BuildEngine for F {}

/// The threaded worker's engines, built at once.
#[cfg(not(target_arch = "wasm32"))]
pub type EngineFactory = Box<dyn BuildEngine>;

/// An engine on its way: on wasm32 the GPU's awaits its device.
#[cfg(target_arch = "wasm32")]
pub type EngineFuture = Pin<Box<dyn Future<Output = Box<dyn WaveEngine>>>>;
/// An engine on its way.
#[cfg(not(target_arch = "wasm32"))]
pub type EngineFuture = Pin<Box<dyn Future<Output = Box<dyn WaveEngine>> + Send>>;

/// Starts an engine for a scene and options.
pub trait StartEngine: Fn(Arc<SceneData>, EngineOptions) -> EngineFuture + MaybeSend {}

impl<F: Fn(Arc<SceneData>, EngineOptions) -> EngineFuture + MaybeSend> StartEngine for F {}

/// The pumped worker's engines, polled at each pump until built.
pub type PumpedFactory = Box<dyn StartEngine>;

/// Where a worker's engines come from.
enum Factory {
	#[cfg(not(target_arch = "wasm32"))]
	Now(EngineFactory),
	Later(PumpedFactory),
}

/// An engine the pumped worker waits for, and the spec it is for.
struct Building {
	spec: RunSpec,
	engine: EngineFuture,
	started: Instant,
}

/// How much of the run the goal wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Wanted {
	/// The frame at the position alone.
	#[default]
	Position,
	/// Every frame, the position's first.
	All,
}

/// A 16-bit frame of timeline frame `frame`, for a brightness fit; served once per `id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FitRequest {
	pub id: u64,
	pub frame: u32,
}

/// What the worker converges on. `revision` moves with every project change; [`Status::adopted`] echoes the
/// revision whose frames the store holds.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Goal {
	pub revision: u64,
	pub run: Option<RunRequest>,
	/// Timeline frame.
	pub position: u32,
	pub wanted: Wanted,
	pub paused: bool,
	pub fit: Option<FitRequest>,
	/// The RAM share for a new store, 0 = Auto.
	pub ram_percent: u32,
}

/// Equality compares the variant; two failures are equal when they share the error.
#[derive(Clone, Debug)]
pub enum WorkState {
	Idle,
	/// Stepping towards stored frame `target`.
	Stepping {
		target: u32,
	},
	/// Stepping towards a fit frame; the run's frames are as they were.
	Fitting,
	Paused,
	Failed(Arc<RunError>),
}

impl PartialEq for WorkState {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Failed(a), Self::Failed(b)) => Arc::ptr_eq(a, b),
			(Self::Stepping { target: a }, Self::Stepping { target: b }) => a == b,
			_ => std::mem::discriminant(self) == std::mem::discriminant(other),
		}
	}
}

impl Eq for WorkState {}

/// What the worker published last.
#[derive(Clone)]
pub struct Status {
	/// The goal revision whose spec is in force: the store's frames are its.
	pub adopted: u64,
	pub state: WorkState,
	/// Steps the engine has done since its last reset.
	pub steps_done: u32,
	/// Steps whose probe readings the traces hold.
	pub traces_steps: u32,
	pub store: Option<Arc<FrameStore>>,
	/// Where the GPU engine keeps the frames it captured last, tagged by their stored frame; `None` on the CPU.
	#[cfg(feature = "gpu")]
	pub device_frames: Option<Arc<DeviceFrames>>,
}

/// The frame a [`FitRequest`] asked for.
#[derive(Clone, Debug)]
pub struct FitFrame {
	pub id: u64,
	pub frame: u32,
	pub snapshot: Arc<Snapshot>,
}

struct Shared {
	goal: Mutex<(Goal, u64)>,
	wake: Condvar,
	/// The goal's number, for a cheap "did it change" while stepping.
	goal_number: AtomicU64,
	paused: AtomicBool,
	/// The engine is to be built anew (the device changed).
	rebuild: AtomicBool,
	/// The frames in force are to be dropped for a new store.
	discard: AtomicBool,
	shutdown: AtomicBool,
	/// One lock, so a reader never pairs a revision with another revision's state or store.
	published: Mutex<Status>,
	traces: Mutex<Vec<ProbeTrace>>,
	/// The traces of the generation before, while its frames are still looked at.
	held: Mutex<Vec<ProbeTrace>>,
	fit: Mutex<Option<FitFrame>>,
	/// Called when the published state changes kind, revision or store, and when a fit frame is served.
	notify: OnceLock<Notify>,
}

/// What a reader of the published state waiting on another thread is woken with.
pub type Notify = Box<dyn Fn() + Send + Sync>;

impl Shared {
	fn new() -> Self {
		Self {
			goal: Mutex::new((Goal::default(), 0)),
			wake: Condvar::new(),
			goal_number: AtomicU64::new(0),
			paused: AtomicBool::new(false),
			rebuild: AtomicBool::new(false),
			discard: AtomicBool::new(false),
			shutdown: AtomicBool::new(false),
			published: Mutex::new(Status {
				adopted: 0,
				state: WorkState::Idle,
				steps_done: 0,
				traces_steps: 0,
				store: None,
				#[cfg(feature = "gpu")]
				device_frames: None,
			}),
			traces: Mutex::new(Vec::new()),
			held: Mutex::new(Vec::new()),
			fit: Mutex::new(None),
			notify: OnceLock::new(),
		}
	}

	fn notify(&self) {
		if let Some(notify) = self.notify.get() {
			notify();
		}
	}
}

/// The handle; dropping it stops the worker after its current check and joins its thread.
pub struct RunWorker {
	shared: Arc<Shared>,
	#[cfg(not(target_arch = "wasm32"))]
	thread: Option<std::thread::JoinHandle<()>>,
	pumped: Option<Mutex<Pumped>>,
}

/// A worker without a thread, and the goal it failed at.
struct Pumped {
	worker: Worker,
	failed_at: Option<u64>,
}

impl RunWorker {
	/// A worker on a thread of its own whose engines `factory` builds, with its spill files in `cache_dir` and its
	/// scenes from `scene` (shared with whatever else composes the scene); its budgets read the system's memory.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn new(
		factory: EngineFactory,
		cache_dir: std::path::PathBuf,
		scene: Arc<Mutex<SceneCache>>,
	) -> std::io::Result<Self> {
		Self::with_memory(factory, cache_dir, scene, super::store::Memory::System)
	}

	/// As [`new`](Self::new), the budgets on `memory`'s readings.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn with_memory(
		factory: EngineFactory,
		cache_dir: std::path::PathBuf,
		scene: Arc<Mutex<SceneCache>>,
		memory: super::store::Memory,
	) -> std::io::Result<Self> {
		match super::store::sweep_leftovers(&cache_dir) {
			Ok(0) => {}
			Ok(bytes) => tracing::info!("removed {bytes} bytes of spill files left in {}", cache_dir.display()),
			Err(e) => tracing::warn!("the spill folder {} could not be swept: {e}", cache_dir.display()),
		}
		let shared = Arc::new(Shared::new());
		let worker = Worker::new(shared.clone(), Factory::Now(factory), Frames::Spill(cache_dir, memory), scene);
		let thread = std::thread::Builder::new().name("live-run".into()).spawn(move || worker.run())?;
		Ok(Self { shared, thread: Some(thread), pumped: None })
	}

	/// A worker on the CPU engine.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn cpu(cache_dir: std::path::PathBuf) -> std::io::Result<Self> {
		Self::new(
			Box::new(|scene, options| Box::new(crate::engine::cpu::CpuEngine::new(scene, options))),
			cache_dir,
			Arc::default(),
		)
	}

	/// A worker that works in [`pump`](Self::pump) alone, whose engines `factory` starts (a pump yields while one is
	/// on its way), whose stores hold their frames in `ram_bytes` of memory (the goal's RAM share of them) and the
	/// rest in a spill of `spill`, and whose scenes come from `scene`.
	pub fn pumped(
		factory: PumpedFactory,
		ram_bytes: u64,
		spill: Box<dyn SpillFactory>,
		scene: Arc<Mutex<SceneCache>>,
	) -> Self {
		#[cfg_attr(
			all(target_arch = "wasm32", feature = "gpu"),
			allow(clippy::arc_with_non_send_sync, reason = "the device frames' wgpu types are not Send there")
		)]
		let shared = Arc::new(Shared::new());
		let frames = Frames::Pending { ram_bytes, spill };
		let worker = Worker::new(shared.clone(), Factory::Later(factory), frames, scene);
		Self {
			shared,
			#[cfg(not(target_arch = "wasm32"))]
			thread: None,
			pumped: Some(Mutex::new(Pumped { worker, failed_at: None })),
		}
	}

	/// Works on the goal for about `budget` (a unit of work interrupted at the deadline has done at least
	/// [`CHECK_INTERVAL`] steps or reached its end); whether work is left, spill writes in flight included. False
	/// until the next goal: the goal is reached, paused or failed.
	pub fn pump(&self, budget: Duration) -> bool {
		let mut pumped = lock(self.pumped.as_ref().expect("a worker on a thread is not pumped"));
		let Pumped { worker, failed_at } = &mut *pumped;
		let deadline = Instant::now() + budget;
		worker.deadline = Some(deadline);
		worker.stepped = false;
		let more = loop {
			let (goal, number) = {
				let g = lock(&self.shared.goal);
				(g.0.clone(), g.1)
			};
			if *failed_at == Some(number) {
				break false;
			}
			match worker.serve_caught(&goal, number) {
				Ok(true) if Instant::now() >= deadline => break true,
				Ok(true) | Err(Interrupted::Goal) => {}
				Ok(false) | Err(Interrupted::Stop) => break false,
				Err(Interrupted::Yield) => break true,
				Err(Interrupted::Failed(e)) => {
					worker.fail(e);
					*failed_at = Some(number);
					break false;
				}
			}
		};
		worker.deadline = None;
		more || worker.current.as_ref().is_some_and(|c| c.store.spill_writing())
	}

	/// Replaces the goal; the worker takes it up at its next look.
	pub fn set_goal(&self, goal: Goal) {
		let mut g = lock(&self.shared.goal);
		self.shared.paused.store(goal.paused, Ordering::Release);
		g.1 += 1;
		self.shared.goal_number.store(g.1, Ordering::Release);
		g.0 = goal;
		self.shared.wake.notify_one();
	}

	/// The next unit of work builds the engine anew (after a device change); the frames and traces stay.
	pub fn rebuild_engine(&self) {
		self.ask(&self.shared.rebuild);
	}

	/// The next unit of work drops the frames and traces for a new store of the same spec, so their memory and
	/// spill are freed once their readers let go; the engine starts over from step 0. The goal in force then says
	/// which frames are made again: set the one that goes with the discard first.
	pub fn discard_frames(&self) {
		self.ask(&self.shared.discard);
	}

	/// Raises `flag` and wakes the worker as a goal change does.
	fn ask(&self, flag: &AtomicBool) {
		flag.store(true, Ordering::Release);
		let mut g = lock(&self.shared.goal);
		g.1 += 1;
		self.shared.goal_number.store(g.1, Ordering::Release);
		drop(g);
		self.shared.wake.notify_one();
	}

	pub fn status(&self) -> Status {
		lock(&self.shared.published).clone()
	}

	pub fn store(&self) -> Option<Arc<FrameStore>> {
		lock(&self.shared.published).store.clone()
	}

	/// Calls `f` with the probe traces of the spec in force, in its probe order.
	pub fn with_traces<R>(&self, f: impl FnOnce(&[ProbeTrace]) -> R) -> R {
		f(&lock(&self.shared.traces))
	}

	/// Calls `f` with the traces of the generation before the current one when its probes measure the same
	/// (the graphs of the frame kept through the clear, [`FrameStore::latest`]); empty otherwise.
	pub fn with_held_traces<R>(&self, f: impl FnOnce(&[ProbeTrace]) -> R) -> R {
		f(&lock(&self.shared.held))
	}

	/// The fit frame served, once.
	pub fn take_fit(&self) -> Option<FitFrame> {
		lock(&self.shared.fit).take()
	}

	/// Calls `notify` from the working thread when the state changes kind (stepping, idle, paused, failed),
	/// revision or store, and when a fit frame is served; progress within a state does not call it. The first one set stays.
	pub fn set_notify(&self, notify: Notify) {
		let _ = self.shared.notify.set(notify);
	}
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for RunWorker {
	fn drop(&mut self) {
		{
			// Under the goal lock: the worker checks `shutdown` under it before it waits.
			let _goal = lock(&self.shared.goal);
			self.shared.shutdown.store(true, Ordering::Release);
		}
		self.shared.wake.notify_one();
		if let Some(t) = self.thread.take() {
			let _ = t.join();
		}
	}
}

/// Where a worker's stores keep their frames.
enum Frames {
	/// In memory up to the budget on the readings for the goal's RAM share, the rest in a spill file in the folder.
	#[cfg(not(target_arch = "wasm32"))]
	Spill(std::path::PathBuf, super::store::Memory),
	/// In memory up to the goal's RAM share of `ram_bytes`, the rest in a spill of `spill`.
	Pending { ram_bytes: u64, spill: Box<dyn SpillFactory> },
}

impl Frames {
	/// A new store can be made now.
	fn ready(&self) -> bool {
		match self {
			#[cfg(not(target_arch = "wasm32"))]
			Self::Spill(..) => true,
			Self::Pending { spill, .. } => spill.ready(),
		}
	}

	/// A store for `layout` in place of `old`, with `ram_percent` of the memory (0 = Auto).
	fn store(
		&self,
		layout: StoreLayout,
		old: Option<Arc<FrameStore>>,
		ram_percent: u32,
	) -> std::io::Result<FrameStore> {
		match self {
			#[cfg(not(target_arch = "wasm32"))]
			Self::Spill(dir, memory) => {
				// The old store's memory counts as free once every reader let go of it; the new store's
				// producer waits for that (bounded, a reader may hold on).
				let reclaimed = old.as_ref().map_or(0, |s| s.ram_bytes());
				let release = old.map(|old| -> super::store::Release {
					let signal = old.release_signal();
					Box::new(move || {
						drop(old);
						signal.wait(RELEASE_WAIT);
					})
				});
				let budget = super::store::Budget::compute(*memory, reclaimed, ram_percent);
				FrameStore::in_dir(layout, budget, release, dir)
			}
			Self::Pending { ram_bytes, spill } => {
				use super::store::Budget;
				drop(old);
				let budget = Budget::of_memory(*ram_bytes, ram_percent);
				let frame_bytes = layout.frame_bytes() as u64;
				let ram_frames = layout.capacity.min(Budget::frames_in(budget, frame_bytes));
				if ram_frames == layout.capacity {
					return Ok(FrameStore::in_memory(layout, budget));
				}
				let backend = spill.open((layout.capacity - ram_frames) as u64 * frame_bytes)?;
				Ok(FrameStore::with_spill(layout, budget, backend))
			}
		}
	}
}

/// The spec in force and its store.
struct Current {
	spec: RunSpec,
	store: Arc<FrameStore>,
}

struct Worker {
	shared: Arc<Shared>,
	factory: Factory,
	building: Option<Building>,
	/// An engine change asked for and not done yet.
	rebuild_due: bool,
	frames: Frames,
	scene: Arc<Mutex<SceneCache>>,
	/// The spec built last and its request, so a goal's spec is built once.
	built: Option<(RunRequest, RunSpec)>,
	engine: Option<Box<dyn WaveEngine>>,
	/// The engine's frames on its device.
	#[cfg(feature = "gpu")]
	device_frames: Option<Arc<DeviceFrames>>,
	current: Option<Current>,
	/// The goal revision `current` serves; published with the next state.
	adopted: u64,
	/// The engine's steps since its reset for the spec in force.
	steps_done: u32,
	collector: Option<ProbeCollector>,
	/// Steps whose readings are in the collector.
	recorded: u32,
	/// Steps the published traces hold.
	traces_steps: u32,
	/// Captures begun, oldest first, and what each is for; landed in order, at most the engine's
	/// `capture_capacity` of them.
	pending: VecDeque<(Capture, PendingCapture)>,
	/// Probe readings whose readback was not mapped when a pump looked; drained before any later ones.
	probes: Option<PendingProbes>,
	served_fit: Option<u64>,
	last_beat: Instant,
	/// The goal number the unit of work started from.
	goal_seen: u64,
	/// Set while pumped: the worker yields at this time once it stepped, at a pause and at a readback not mapped.
	deadline: Option<Instant>,
	/// A step was done since the pump began.
	stepped: bool,
}

impl Worker {
	fn new(shared: Arc<Shared>, factory: Factory, frames: Frames, scene: Arc<Mutex<SceneCache>>) -> Self {
		Self {
			shared,
			factory,
			building: None,
			rebuild_due: false,
			frames,
			scene,
			built: None,
			engine: None,
			#[cfg(feature = "gpu")]
			device_frames: None,
			current: None,
			adopted: 0,
			steps_done: 0,
			collector: None,
			recorded: 0,
			traces_steps: 0,
			pending: VecDeque::new(),
			probes: None,
			served_fit: None,
			last_beat: Instant::now(),
			goal_seen: 0,
			deadline: None,
			stepped: false,
		}
	}

	#[cfg(not(target_arch = "wasm32"))]
	fn run(mut self) {
		let mut failed_at: Option<u64> = None;
		loop {
			let (goal, number) = {
				let mut g = lock(&self.shared.goal);
				loop {
					if self.shared.shutdown.load(Ordering::Acquire) {
						drop(g);
						let _ = self.finish_pending();
						return;
					}
					if failed_at != Some(g.1) {
						break (g.0.clone(), g.1);
					}
					g = self.shared.wake.wait(g).unwrap_or_else(PoisonError::into_inner);
				}
			};
			match self.serve_caught(&goal, number) {
				Ok(true) | Err(Interrupted::Goal | Interrupted::Yield) => {}
				Ok(false) => self.wait_for_change(number),
				Err(Interrupted::Stop) => {
					let _ = self.finish_pending();
					return;
				}
				Err(Interrupted::Failed(e)) => {
					self.fail(e);
					failed_at = Some(number);
				}
			}
		}
	}

	/// [`serve`](Self::serve) with a panic as a failure.
	fn serve_caught(&mut self, goal: &Goal, number: u64) -> Result<bool, Interrupted> {
		catch_unwind(AssertUnwindSafe(|| self.serve(goal, number)))
			.unwrap_or_else(|payload| Err(Interrupted::Failed(RunError::panic("the live run", payload.as_ref()))))
	}

	fn fail(&mut self, e: RunError) {
		tracing::warn!("the live run failed: {e}");
		self.forget();
		self.publish(WorkState::Failed(Arc::new(e)));
	}

	/// Waits until the goal is no longer `number` or the worker is stopped.
	#[cfg(not(target_arch = "wasm32"))]
	fn wait_for_change(&self, number: u64) {
		let mut g = lock(&self.shared.goal);
		while g.1 == number && !self.shared.shutdown.load(Ordering::Acquire) {
			g = self.shared.wake.wait(g).unwrap_or_else(PoisonError::into_inner);
		}
	}

	/// One step of the loop: adopts the goal's spec and does one unit of work. Ok(false) when nothing is to do.
	fn serve(&mut self, goal: &Goal, number: u64) -> Result<bool, Interrupted> {
		self.goal_seen = number;
		if let Some(c) = &self.current {
			c.store.settle().map_err(|e| Interrupted::Failed(e.into()))?;
		}
		let Some(request) = &goal.run else {
			self.finish_pending()?;
			self.publish(WorkState::Idle);
			return Ok(false);
		};
		let spec = self.spec_of(request).map_err(|e| Interrupted::Failed(e.into()))?;
		if self.engine.is_none() && self.current.as_ref().is_some_and(|c| c.spec.same_frames_as(&spec)) {
			// Pumped: the engine of the frames in force was dropped for one still on its way.
			self.restart_engine(None, &spec)?;
		}
		self.adopt(&spec, goal)?;
		if self.shared.discard.load(Ordering::Acquire) {
			if !self.frames.ready() {
				return Err(Interrupted::Yield);
			}
			self.discard(goal)?;
		}
		self.rebuild_due |= self.shared.rebuild.swap(false, Ordering::AcqRel);
		if self.rebuild_due {
			self.finish_pending()?;
			self.restart_engine(None, &spec)?;
		}
		let fit = goal.fit.filter(|f| self.served_fit != Some(f.id));
		let target = self.next_target(goal);
		if goal.paused {
			// The frame looked at is made even while paused (a scrub while paused shows its frame).
			if let Some(Target::Frame(t)) = target
				&& t == self.position_of(goal)
			{
				self.step_to(t, false)?;
				return Ok(true);
			}
			self.finish_pending()?;
			let idle = fit.is_none() && target.is_none();
			self.publish(if idle { WorkState::Idle } else { WorkState::Paused });
			return Ok(false);
		}
		if let Some(fit) = fit {
			self.serve_fit(fit)?;
			return Ok(true);
		}
		let all = goal.wanted == Wanted::All;
		match target {
			Some(Target::Frame(target)) => {
				self.step_to(target, all)?;
				Ok(true)
			}
			Some(Target::End) => {
				let (steps, last) = {
					let c = self.current.as_ref().expect("a target needs a current spec");
					(c.spec.steps, c.spec.stored_frames().saturating_sub(1))
				};
				self.advance_to(steps, Unit::End, WorkState::Stepping { target: last }, all)?;
				self.publish_traces();
				Ok(true)
			}
			None => {
				self.finish_pending()?;
				self.publish(WorkState::Idle);
				Ok(false)
			}
		}
	}

	/// The spec of `request`: the one built last when it is the same request, else built (the scene composed
	/// when the project's scene changed).
	fn spec_of(&mut self, request: &RunRequest) -> Result<RunSpec, crate::spec::RunSpecError> {
		if let Some((r, spec)) = &self.built
			&& r.same_as(request)
		{
			return Ok(spec.clone());
		}
		let spec = request.spec(&mut lock(&self.scene))?;
		self.built = Some((request.clone(), spec.clone()));
		Ok(spec)
	}

	/// Puts `spec` in force: a generation when its frames differ, new traces when its probes do. Readers learn of
	/// it with the next state published.
	fn adopt(&mut self, spec: &RunSpec, goal: &Goal) -> Result<(), Interrupted> {
		let same_frames = self.current.as_ref().is_some_and(|c| c.spec.same_frames_as(spec));
		let same_probes = self.current.as_ref().is_some_and(|c| c.spec.same_probes_as(spec));
		if same_frames {
			let current = self.current.as_mut().expect("same frames means a current spec");
			current.spec = spec.clone();
			if !same_probes {
				self.finish_pending()?;
				let engine = self.engine.as_mut().expect("an engine goes with the current spec");
				engine.reset();
				self.steps_done = 0;
				lock(&self.shared.held).clear();
				self.restart_traces(spec);
				tracing::debug!(revision = goal.revision, "live run: new probes");
			}
		} else {
			if !self.frames.ready() {
				return Err(Interrupted::Yield);
			}
			self.finish_pending()?;
			let old_spec = self.current.as_ref().map(|c| c.spec.clone());
			self.reset_engine(old_spec.as_ref(), spec)?;
			let layout = StoreLayout::of(spec);
			let previous = self.current.take();
			let store = match previous {
				Some(c) if c.store.layout().fits(&layout) => {
					c.store.clear();
					c.store
				}
				old => self.new_store(layout, old.map(|c| c.store), goal.ram_percent)?,
			};
			{
				let mut traces = lock(&self.shared.traces);
				let mut held = lock(&self.shared.held);
				*held = if same_probes { std::mem::take(&mut *traces) } else { Vec::new() };
				traces.clear();
			}
			self.current = Some(Current { spec: spec.clone(), store });
			self.restart_traces(spec);
			tracing::debug!(revision = goal.revision, steps = spec.steps, "live run: new frames");
		}
		self.adopted = goal.revision;
		Ok(())
	}

	/// A store for `layout` in place of `old`, which readers no longer get.
	fn new_store(
		&self,
		layout: StoreLayout,
		old: Option<Arc<FrameStore>>,
		ram_percent: u32,
	) -> Result<Arc<FrameStore>, Interrupted> {
		lock(&self.shared.published).store = None;
		let store =
			self.frames.store(layout, old, ram_percent).map_err(|e| Interrupted::Failed(RunError::Store(e.into())))?;
		Ok(Arc::new(store))
	}

	/// [`RunWorker::discard_frames`]: the spec in force on a new store, the engine reset, the traces anew. The ask is
	/// answered once the old store is let go of, so a yield before that leaves it to the next unit of work.
	fn discard(&mut self, goal: &Goal) -> Result<(), Interrupted> {
		if self.current.is_none() {
			self.shared.discard.store(false, Ordering::Release);
			return Ok(());
		}
		self.finish_pending()?;
		let Current { spec, store } = self.current.take().expect("checked above");
		let store = self.new_store(*store.layout(), Some(store), goal.ram_percent);
		self.shared.discard.store(false, Ordering::Release);
		let store = store?;
		self.engine.as_mut().expect("an engine goes with the current spec").reset();
		self.probes = None;
		self.steps_done = 0;
		lock(&self.shared.held).clear();
		self.restart_traces(&spec);
		self.current = Some(Current { spec, store });
		tracing::debug!(revision = goal.revision, "live run: frames discarded");
		Ok(())
	}

	/// A collector for `spec`'s probes on a reset engine; the published traces start over.
	fn restart_traces(&mut self, spec: &RunSpec) {
		let collector = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene, spec.seed));
		let engine = self.engine.as_mut().expect("an engine goes with the current spec");
		engine.set_probes(collector.as_ref().map(ProbeCollector::cells).unwrap_or_default());
		*lock(&self.shared.traces) = collector.as_ref().map(|c| c.traces.clone()).unwrap_or_default();
		self.probes = None;
		self.collector = collector;
		self.recorded = 0;
		self.traces_steps = 0;
	}

	/// The engine reset for `spec`, from `old`'s: as it is for the same scene, with the scene swapped in for
	/// the same grid, else a new one.
	fn reset_engine(&mut self, old: Option<&RunSpec>, spec: &RunSpec) -> Result<(), Interrupted> {
		let kept = match (old, self.engine.as_mut()) {
			(Some(old), Some(engine)) if old.same_scene_as(spec) => {
				if old.emitters != spec.emitters {
					engine.set_emitters(&spec.emitters);
				}
				engine.set_track_intensity(spec.options.track_intensity);
				true
			}
			(Some(old), Some(engine)) if old.same_grid_as(spec) => {
				let replaced = engine.try_replace_scene(&spec.scene);
				if replaced {
					tracing::debug!("live run: scene replaced");
					engine.set_emitters(&spec.emitters);
					engine.set_track_intensity(spec.options.track_intensity);
				}
				replaced
			}
			_ => false,
		};
		if !kept {
			let mut e = self.build(spec)?;
			e.set_emitters(&spec.emitters);
			#[cfg(feature = "gpu")]
			{
				self.device_frames = e.device_frames();
			}
			self.engine = Some(e);
		}
		self.engine.as_mut().expect("kept or built").reset();
		self.probes = None;
		self.steps_done = 0;
		Ok(())
	}

	/// A new engine for `spec`, which answers a rebuild asked for; pumped, a yield while it is on its way, the
	/// worker without an engine meanwhile.
	fn build(&mut self, spec: &RunSpec) -> Result<Box<dyn WaveEngine>, Interrupted> {
		// The old engine's memory is freed first: a GPU may not hold both.
		self.engine = None;
		#[cfg(feature = "gpu")]
		{
			self.device_frames = None;
		}
		self.probes = None;
		self.steps_done = 0;
		self.rebuild_due = false;
		match &self.factory {
			#[cfg(not(target_arch = "wasm32"))]
			Factory::Now(factory) => {
				let started = Instant::now();
				let engine = factory(spec.scene.clone(), spec.options.clone());
				tracing::debug!(ms = started.elapsed().as_secs_f64() * 1e3, "live run: engine built");
				Ok(engine)
			}
			Factory::Later(factory) => {
				let mut building = match self.building.take() {
					Some(b) if b.spec.same_scene_as(spec) => b,
					_ => Building {
						spec: spec.clone(),
						engine: factory(spec.scene.clone(), spec.options.clone()),
						started: Instant::now(),
					},
				};
				let polled = building.engine.as_mut().poll(&mut Context::from_waker(Waker::noop()));
				match polled {
					std::task::Poll::Ready(mut engine) => {
						engine.set_track_intensity(spec.options.track_intensity);
						let ms = building.started.elapsed().as_secs_f64() * 1e3;
						tracing::debug!(ms, "live run: engine built");
						Ok(engine)
					}
					std::task::Poll::Pending => {
						self.building = Some(building);
						Err(Interrupted::Yield)
					}
				}
			}
		}
	}

	/// The engine back at step 0 (kept from `old`'s spec, or built anew), the readings so far published and a
	/// collector for the steps to come: the traces grow on from where they are.
	fn restart_engine(&mut self, old: Option<&RunSpec>, spec: &RunSpec) -> Result<(), Interrupted> {
		self.publish_traces();
		self.reset_engine(old, spec)?;
		self.collector = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene, spec.seed));
		let engine = self.engine.as_mut().expect("reset_engine leaves an engine");
		engine.set_probes(self.collector.as_ref().map(ProbeCollector::cells).unwrap_or_default());
		self.recorded = 0;
		Ok(())
	}

	/// Drops a failed engine and its frames, so the next goal builds anew.
	fn forget(&mut self) {
		self.rebuild_due = false;
		self.pending.clear();
		self.probes = None;
		self.engine = None;
		#[cfg(feature = "gpu")]
		{
			self.device_frames = None;
		}
		self.current = None;
		self.collector = None;
		lock(&self.shared.traces).clear();
		lock(&self.shared.held).clear();
	}

	/// Whether stored frame `j` is there or on its way (a capture in flight).
	fn has_frame(&self, j: u32) -> bool {
		self.current.as_ref().is_some_and(|c| c.store.has(j as usize))
			|| self.pending.iter().any(|(c, _)| *c == Capture::Frame(j))
	}

	/// Steps whose probe readings are recorded or queued in the engine, on their way to be drained.
	fn probes_taken(&self) -> u32 {
		if self.collector.is_some() { self.traces_steps.max(self.steps_done) } else { self.traces_steps }
	}

	/// Stored frame `j` is wanted: missing, or its probe readings not taken.
	fn needed(&self, j: u32) -> bool {
		let Some(c) = self.current.as_ref() else { return false };
		!self.has_frame(j) || (!c.spec.probes.is_empty() && self.probes_taken() < j * c.spec.capture_every() + 1)
	}

	/// The stored frame `goal` looks at.
	fn position_of(&self, goal: &Goal) -> u32 {
		let spec = &self.current.as_ref().expect("a current spec").spec;
		spec.stored_index_of(goal.position.min(spec.frame_count().saturating_sub(1)))
	}

	/// The next target by the module's rule. With everything wanted, the frames after the position come
	/// first, then the probes' last steps while the engine stands at the readings' end (as a batch run records
	/// them), then the frames before the position still missing (the pass to the position captures them),
	/// then the probes' last steps from wherever the engine stands.
	fn next_target(&self, goal: &Goal) -> Option<Target> {
		let current = self.current.as_ref()?;
		let spec = &current.spec;
		let frames = spec.stored_frames();
		if frames == 0 {
			return None;
		}
		let position = self.position_of(goal);
		if self.needed(position) {
			return Some(Target::Frame(position));
		}
		if goal.wanted == Wanted::Position {
			return None;
		}
		if let Some(j) = (position + 1..frames).find(|&j| self.needed(j)) {
			return Some(Target::Frame(j));
		}
		let taken = self.probes_taken();
		let tail = !spec.probes.is_empty() && taken < spec.steps;
		if tail && self.steps_done == taken {
			return Some(Target::End);
		}
		if let Some(j) = (0..position).find(|&j| self.needed(j)) {
			return Some(Target::Frame(j));
		}
		tail.then_some(Target::End)
	}

	/// Steps to stored frame `target` and captures it when missing; `passed` captures the missing frames on
	/// the way too.
	fn step_to(&mut self, target: u32, passed: bool) -> Result<(), Interrupted> {
		let steps = self.current.as_ref().expect("a target needs a current spec").spec.capture_every() * target + 1;
		self.advance_to(steps, Unit::Frame(target), WorkState::Stepping { target }, passed)?;
		if self.ready_to_capture(target)? {
			self.capture(target)?;
		}
		self.publish_traces();
		self.publish(WorkState::Stepping { target });
		Ok(())
	}

	/// Whether stored frame `j` is to be captured: false when it is there or on its way. The captures read back
	/// land and the store's spill is settled first; then a capture's room is made in the engine's ring, natively
	/// by landing the oldest. Pumped, a yield until the capture can begin without waiting: the spill has fewer
	/// writes in flight than it takes and the ring has room.
	fn ready_to_capture(&mut self, j: u32) -> Result<bool, Interrupted> {
		if self.has_frame(j) {
			return Ok(false);
		}
		self.land_ready()?;
		if let Some(c) = &self.current {
			c.store.settle().map_err(|e| Interrupted::Failed(e.into()))?;
			if self.deadline.is_some() && c.store.spill_busy() {
				return Err(Interrupted::Yield);
			}
		}
		let shape = self.current.as_ref().expect("a capture needs a current spec").spec.shape;
		let capacity = self.engine.as_ref().expect("an engine goes with the current spec").capture_capacity(&shape);
		while self.pending.len() >= capacity {
			if self.deadline.is_some() {
				return Err(Interrupted::Yield);
			}
			self.land_oldest()?;
		}
		Ok(true)
	}

	/// Begins the capture of the engine's state as stored frame `j`, which [`ready_to_capture`](Self::ready_to_capture)
	/// made room for.
	fn capture(&mut self, j: u32) -> Result<(), Interrupted> {
		let (store, shape) = {
			let c = self.current.as_ref().expect("a capture needs a current spec");
			(c.store.clone(), c.spec.shape)
		};
		let engine = self.engine.as_mut().expect("an engine goes with the current spec");
		let buffer = store.buffer_for(j as usize);
		let next = engine.begin_capture(&shape, buffer).map_err(|e| Interrupted::Failed(e.into()))?;
		self.pending.push_back((Capture::Frame(j), next));
		Ok(())
	}

	/// The fit's 16-bit frame of the spec's shape at the fit's timeline frame.
	fn serve_fit(&mut self, fit: FitRequest) -> Result<(), Interrupted> {
		let (steps, shape) = {
			let c = self.current.as_ref().expect("a fit needs a current spec");
			let frame = fit.frame.min(c.spec.frame_count().saturating_sub(1));
			(c.spec.step_of(frame) + 1, c.spec.shape)
		};
		self.advance_to(steps, Unit::Fit(fit), WorkState::Fitting, false)?;
		self.publish_traces();
		if !self.pending.iter().any(|(c, _)| *c == Capture::Fit(fit)) {
			self.finish_pending()?;
			let engine = self.engine.as_mut().expect("an engine goes with the current spec");
			let half = FrameShape { format: SampleFormat::Half, ..shape };
			let pending =
				engine.begin_capture(&half, Snapshot::new(half)).map_err(|e| Interrupted::Failed(e.into()))?;
			self.pending.push_back((Capture::Fit(fit), pending));
		}
		self.finish_pending()
	}

	/// Brings the engine to `steps` steps done (from 0 when it has passed them), recording the probes and, with
	/// `passed`, capturing the missing stored frames on the way. Every [`CHECK_INTERVAL`] steps a pause is
	/// honoured, and a new goal is looked at: the unit goes on when it is still what the goal wants
	/// ([`still_wanted`](Self::still_wanted)), else it is left.
	fn advance_to(&mut self, steps: u32, unit: Unit, state: WorkState, passed: bool) -> Result<(), Interrupted> {
		if self.steps_done > steps {
			self.finish_pending()?;
			let spec = self.current.as_ref().expect("a current spec").spec.clone();
			self.restart_engine(Some(&spec), &spec)?;
		}
		let (every, capture_every) = {
			let spec = &self.current.as_ref().expect("a current spec").spec;
			(spec.visualize_every, spec.capture_every())
		};
		if self.steps_done == steps {
			return self.record(every, false);
		}
		self.publish(state.clone());
		let from = self.steps_done;
		for i in from..steps {
			if (i - from).is_multiple_of(CHECK_INTERVAL) {
				self.check_goal(unit, &state)?;
			}
			// Decided before the step: a capture left after it would cost a run from step 0.
			let capturing = passed
				&& i + 1 < steps
				&& i.is_multiple_of(capture_every)
				&& self.ready_to_capture(i / capture_every)?;
			let engine = self.engine.as_mut().expect("an engine goes with the current spec");
			engine.advance_one_step().map_err(|e| Interrupted::Failed(e.into()))?;
			if self.collector.is_some() {
				engine.queue_probes().map_err(|e| Interrupted::Failed(e.into()))?;
			}
			self.steps_done = i + 1;
			self.stepped = true;
			if capturing {
				self.capture(i / capture_every)?;
			}
			if (i + 1 - from).is_multiple_of(PROBE_DRAIN) {
				self.record(every, false)?;
			}
			if self.last_beat.elapsed() >= HEARTBEAT {
				self.last_beat = Instant::now();
				self.record(every, false)?;
				self.publish_traces();
				self.publish(state.clone());
			}
		}
		self.record(every, false)
	}

	/// Drains the engine's queued readings into the collector, `whole` all of them. Pumped, a yield while they are
	/// not read back; not `whole`, those read back are drained and it yields only with
	/// [`PROBE_BATCHES_IN_FLIGHT`] batches on their way.
	fn record(&mut self, every: u32, whole: bool) -> Result<(), Interrupted> {
		let Some(c) = self.collector.as_mut() else { return Ok(()) };
		let engine = self.engine.as_mut().expect("an engine goes with the current spec").as_mut();
		let mut pending = match self.probes.take() {
			Some(pending) => pending,
			None => engine.pending_probes().map_err(|e| Interrupted::Failed(e.into()))?,
		};
		if self.deadline.is_some() && !resolved(pending.ready()) {
			if whole || pending.batches_in_flight() >= PROBE_BATCHES_IN_FLIGHT {
				self.probes = Some(pending);
				return Err(Interrupted::Yield);
			}
			pending = pending.ready_part();
		}
		record(engine, pending, c, &mut self.recorded, every).map_err(|e| Interrupted::Failed(e.into()))
	}

	/// Whether `unit` is still worth finishing under `goal`: only work for the goal's own spec is. Of that, the
	/// frame the goal looks at always is; a frame for later while every frame is wanted and the goal's own
	/// frame is there; the probes' tail needs the same probes too; a fit while it is still asked for.
	fn still_wanted(&mut self, unit: Unit, goal: &Goal) -> bool {
		if self.shared.discard.load(Ordering::Acquire) {
			return false;
		}
		let Some(spec) = self.goal_spec(goal) else { return false };
		let Some(current) = self.current.as_ref() else { return false };
		if !current.spec.same_frames_as(&spec) {
			return false;
		}
		let position = self.position_of(goal);
		match unit {
			Unit::Frame(j) if j == position => true,
			Unit::Frame(_) => goal.wanted == Wanted::All && !self.needed(position),
			Unit::End => current.spec.same_probes_as(&spec) && goal.wanted == Wanted::All && !self.needed(position),
			Unit::Fit(f) => goal.fit == Some(f),
		}
	}

	/// The goal's spec, when it has one that can be built (a failure is reported when it is served).
	fn goal_spec(&mut self, goal: &Goal) -> Option<RunSpec> {
		goal.run.as_ref().and_then(|r| self.spec_of(r).ok())
	}

	/// The goal's spec is the one in force.
	fn same_spec(&mut self, goal: &Goal) -> bool {
		let Some(spec) = self.goal_spec(goal) else { return false };
		self.current.as_ref().is_some_and(|c| c.spec.same_frames_as(&spec) && c.spec.same_probes_as(&spec))
	}

	/// Stops, the pump's deadline, pauses and goal changes, looked at between steps.
	fn check_goal(&mut self, unit: Unit, state: &WorkState) -> Result<(), Interrupted> {
		if self.shared.shutdown.load(Ordering::Acquire) {
			return Err(Interrupted::Stop);
		}
		if self.stepped && self.deadline.is_some_and(|d| Instant::now() >= d) {
			self.publish_traces();
			self.publish(state.clone());
			return Err(Interrupted::Yield);
		}
		let changed = self.shared.goal_number.load(Ordering::Acquire) != self.goal_seen;
		if !changed && !self.shared.paused.load(Ordering::Acquire) {
			return Ok(());
		}
		let (goal, number) = {
			let g = lock(&self.shared.goal);
			(g.0.clone(), g.1)
		};
		self.goal_seen = number;
		if !self.still_wanted(unit, &goal) {
			tracing::debug!(?unit, "live run: left for the new goal");
			return Err(Interrupted::Goal);
		}
		let looked_at = matches!(unit, Unit::Frame(j) if j == self.position_of(&goal));
		if !goal.paused || looked_at {
			return Ok(());
		}
		if !self.same_spec(&goal) {
			return Err(Interrupted::Goal);
		}
		self.finish_pending()?;
		self.publish_traces();
		self.publish(WorkState::Paused);
		if self.deadline.is_some() {
			return Err(Interrupted::Yield);
		}
		let shared = self.shared.clone();
		let (goal, number) = loop {
			let (goal, number) = {
				let g = lock(&shared.goal);
				(g.0.clone(), g.1)
			};
			if shared.shutdown.load(Ordering::Acquire) {
				return Err(Interrupted::Stop);
			}
			if !goal.paused || (number != self.goal_seen && !self.same_spec(&goal)) {
				break (goal, number);
			}
			let g = lock(&shared.goal);
			if g.1 == number && !shared.shutdown.load(Ordering::Acquire) {
				drop(shared.wake.wait(g).unwrap_or_else(PoisonError::into_inner));
			}
		};
		self.goal_seen = number;
		if !self.same_spec(&goal) || !self.still_wanted(unit, &goal) {
			return Err(Interrupted::Goal);
		}
		self.publish(state.clone());
		Ok(())
	}

	/// Lands the captures in flight and records the probe readings queued, all of them, into the published
	/// traces; pumped, those read back, then a yield while any is left.
	fn finish_pending(&mut self) -> Result<(), Interrupted> {
		self.land_ready()?;
		while !self.pending.is_empty() {
			if self.deadline.is_some() {
				return Err(Interrupted::Yield);
			}
			self.land_oldest()?;
		}
		let every = match (&self.current, &self.engine) {
			(Some(c), Some(_)) => c.spec.visualize_every,
			_ => return Ok(()),
		};
		self.record(every, true)?;
		self.publish_traces();
		Ok(())
	}

	/// Lands the oldest captures for as long as they are read back.
	fn land_ready(&mut self) -> Result<(), Interrupted> {
		while self.pending.front().is_some_and(|(_, p)| resolved(p.ready())) {
			self.land_oldest()?;
		}
		Ok(())
	}

	/// Lands the oldest capture, waiting for it natively.
	fn land_oldest(&mut self) -> Result<(), Interrupted> {
		let Some((capture, pending)) = self.pending.pop_front() else { return Ok(()) };
		let Some(engine) = self.engine.as_mut() else { return Ok(()) };
		let snapshot = engine.finish_capture(pending).map_err(|e| Interrupted::Failed(e.into()))?;
		self.land(capture, snapshot)
	}

	/// A finished capture into the store, or out as the fit frame.
	fn land(&mut self, capture: Capture, snapshot: Snapshot) -> Result<(), Interrupted> {
		match capture {
			Capture::Frame(frame) => match self.current.as_ref() {
				Some(current) => {
					#[cfg(feature = "gpu")]
					if let Some(frames) = &self.device_frames {
						let store = &current.store;
						frames.publish(FrameTag {
							store: store.id(),
							generation: store.generation(),
							stored: frame as usize,
						});
					}
					current.store.write_at(frame as usize, snapshot).map_err(|e| Interrupted::Failed(e.into()))
				}
				None => Ok(()),
			},
			Capture::Fit(fit) => {
				// A fit the goal moved past while its readback was in flight is dropped unserved.
				if lock(&self.shared.goal).0.fit != Some(fit) {
					return Ok(());
				}
				*lock(&self.shared.fit) = Some(FitFrame { id: fit.id, frame: fit.frame, snapshot: Arc::new(snapshot) });
				self.shared.notify();
				self.served_fit = Some(fit.id);
				tracing::debug!(frame = fit.frame, "live run: fit frame");
				Ok(())
			}
		}
	}

	/// Appends what the collector recorded beyond the published traces.
	fn publish_traces(&mut self) {
		let Some(c) = self.collector.as_ref() else { return };
		if self.recorded <= self.traces_steps {
			return;
		}
		let mut traces = lock(&self.shared.traces);
		for (out, t) in traces.iter_mut().zip(&c.traces) {
			if t.series.len() > out.series.len() {
				out.series.extend_from_slice(&t.series[out.series.len()..]);
			}
			for profile in t.profiles.iter().skip(out.profiles.len()) {
				out.profiles.push(profile.iter().copied());
			}
			out.steps_sampled = out.steps_sampled.max(t.steps_sampled);
		}
		self.traces_steps = self.recorded;
	}

	/// Publishes `state` with the revision, progress and store it belongs to.
	fn publish(&self, state: WorkState) {
		let mut p = lock(&self.shared.published);
		let store = self.current.as_ref().map(|c| c.store.clone());
		let changed = p.adopted != self.adopted
			|| std::mem::discriminant(&p.state) != std::mem::discriminant(&state)
			|| p.store.as_ref().map(Arc::as_ptr) != store.as_ref().map(Arc::as_ptr);
		p.adopted = self.adopted;
		p.state = state;
		p.steps_done = self.steps_done;
		p.traces_steps = self.traces_steps;
		p.store = store;
		#[cfg(feature = "gpu")]
		{
			p.device_frames = self.device_frames.clone();
		}
		drop(p);
		if changed {
			self.shared.notify();
		}
	}
}

enum Target {
	/// A stored frame.
	Frame(u32),
	/// The run's last step, for the traces.
	End,
}

/// What a capture is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Capture {
	/// A stored frame.
	Frame(u32),
	Fit(FitRequest),
}

/// Whether `ready` resolved, polled once.
fn resolved(ready: impl Future<Output = ()>) -> bool {
	std::pin::pin!(ready).poll(&mut Context::from_waker(Waker::noop())).is_ready()
}

/// A unit of work, for [`Worker::still_wanted`].
#[derive(Clone, Copy, Debug)]
enum Unit {
	Frame(u32),
	End,
	Fit(FitRequest),
}

/// Why a unit of work ended early.
enum Interrupted {
	Stop,
	/// The goal changed and the unit is no longer wanted.
	Goal,
	/// Pumped: the deadline passed, the goal paused, a readback is not mapped yet, the spill is busy or the free
	/// storage not known yet; the next pump goes on.
	Yield,
	Failed(RunError),
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::cpu::CpuEngine;
	use crate::model::{Layer, Probe, Project, Source};
	use crate::quality::PreviewQuality;
	use crate::run::pending::fake;
	use crate::run::{Memory, StoreError, WRITES_IN_FLIGHT};

	fn project(probe_x: i32) -> Project {
		let mut p = Project::default();
		p.scene.width = 32;
		p.scene.height = 24;
		p.simulation.steps = 40;
		p.simulation.visualize_every = 2;
		p.sources.push(Source { name: "S".into(), x: 16.0, y: 12.0, ..Source::default() });
		p.probes.push(Probe { name: "P".into(), ax: probe_x, ay: 12, ..Probe::default() });
		p
	}

	fn goal(revision: u64, p: &Project) -> Goal {
		let run = RunRequest::new(p, PreviewQuality::FULL, false);
		Goal { revision, run: Some(run), wanted: Wanted::All, ..Goal::default() }
	}

	fn served(r: Result<bool, Interrupted>) -> bool {
		match r {
			Ok(more) => more,
			Err(Interrupted::Failed(e)) => panic!("the run failed: {e}"),
			Err(_) => panic!("the run was interrupted"),
		}
	}

	#[test]
	fn a_probe_edit_is_published_with_the_state_it_brings() {
		let dir = tempfile::tempdir().unwrap();
		let shared = Arc::new(Shared::new());
		let factory: EngineFactory = Box::new(|scene, options| Box::new(CpuEngine::new(scene, options)));
		let mut w = Worker::new(
			shared.clone(),
			Factory::Now(factory),
			Frames::Spill(dir.path().to_path_buf(), Memory::System),
			Arc::default(),
		);
		// the handle without a thread: this test is the worker
		let handle = RunWorker { shared, thread: None, pumped: None };
		let first = goal(1, &project(20));
		handle.set_goal(first.clone());
		while served(w.serve(&first, 1)) {}
		let done = handle.status();
		assert!(done.adopted == 1 && done.state == WorkState::Idle);
		assert!(done.store.is_some_and(|s| s.is_complete()));

		let moved = goal(2, &project(22));
		let spec = w.spec_of(moved.run.as_ref().unwrap()).unwrap();
		served(w.adopt(&spec, &moved).map(|()| true));
		let between = handle.status();
		assert!(
			between.adopted == 1 || between.state != WorkState::Idle,
			"the new revision with the old Idle reads as a complete run without traces"
		);
		handle.set_goal(moved.clone());
		assert!(served(w.serve(&moved, 2)));
		let after = handle.status();
		assert_eq!(after.adopted, 2);
		assert_ne!(after.state, WorkState::Idle, "the traces are recorded anew");
	}

	#[test]
	fn the_reader_is_notified_of_state_changes_not_of_progress() {
		let dir = tempfile::tempdir().unwrap();
		let shared = Arc::new(Shared::new());
		let factory: EngineFactory = Box::new(|scene, options| Box::new(CpuEngine::new(scene, options)));
		let mut w = Worker::new(
			shared.clone(),
			Factory::Now(factory),
			Frames::Spill(dir.path().to_path_buf(), Memory::System),
			Arc::default(),
		);
		let handle = RunWorker { shared, thread: None, pumped: None };
		let notified = Arc::new(AtomicU64::new(0));
		let count = notified.clone();
		handle.set_notify(Box::new(move || {
			count.fetch_add(1, Ordering::Relaxed);
		}));
		let g = goal(1, &project(20));
		handle.set_goal(g.clone());
		while served(w.serve(&g, 1)) {}
		let frames = handle.status().store.unwrap().count() as u64;
		let calls = notified.load(Ordering::Relaxed);
		assert!(calls >= 2, "the run's start and end were not notified: {calls}");
		assert!(calls < frames, "{calls} notifications for {frames} frames");
	}

	#[test]
	fn a_target_reached_by_an_interrupted_step_still_records_its_probes() {
		let dir = tempfile::tempdir().unwrap();
		let factory: EngineFactory = Box::new(|scene, options| Box::new(CpuEngine::new(scene, options)));
		let mut w = Worker::new(
			Arc::new(Shared::new()),
			Factory::Now(factory),
			Frames::Spill(dir.path().to_path_buf(), Memory::System),
			Arc::default(),
		);
		let g = goal(1, &project(20));
		let spec = w.spec_of(g.run.as_ref().unwrap()).unwrap();
		served(w.adopt(&spec, &g).map(|()| true));
		let target = 3;
		// Stepped and queued as advance_to does, left before its final record (a goal change).
		for _ in 0..spec.capture_every() * target + 1 {
			let engine = w.engine.as_mut().unwrap();
			engine.advance_one_step().unwrap();
			engine.queue_probes().unwrap();
			w.steps_done += 1;
		}
		served(w.step_to(target, false).map(|()| true));
		assert!(!w.needed(target), "the queued readings are recorded");
	}

	const GIB: u64 = 1 << 30;
	/// Long enough for any of these runs: a pump given it ends with its goal.
	const AMPLE: Duration = Duration::from_secs(60);

	fn cpu() -> PumpedFactory {
		Box::new(|scene, options| -> EngineFuture {
			Box::pin(async move { Box::new(CpuEngine::new(scene, options)) as Box<dyn WaveEngine> })
		})
	}

	/// The engines alive, and the most ever alive at once.
	#[derive(Default)]
	struct Live {
		now: AtomicU64,
		peak: AtomicU64,
	}

	/// The CPU engine counted in `live`, on its way until `gate` opens.
	fn gated_cpu(gate: Arc<Gate>, live: Arc<Live>) -> PumpedFactory {
		Box::new(move |scene, options| -> EngineFuture {
			let (gate, live) = (gate.clone(), live.clone());
			let mut args = Some((scene, options));
			Box::pin(std::future::poll_fn(move |_| {
				if !gate.0.load(Ordering::Acquire) {
					return std::task::Poll::Pending;
				}
				let (scene, options) = args.take().expect("polled once ready");
				let open = || Arc::new(Gate(AtomicBool::new(true)));
				let inner = Box::new(CpuEngine::new(scene, options));
				let engine = Gated::new(inner, open(), open(), Some(live.clone()));
				std::task::Poll::Ready(Box::new(engine) as Box<dyn WaveEngine>)
			}))
		})
	}

	#[test]
	fn a_pump_yields_while_its_engine_is_on_its_way_without_the_old_one() {
		let (gate, live) = (Arc::new(Gate::default()), Arc::new(Live::default()));
		let w = RunWorker::pumped(gated_cpu(gate.clone(), live.clone()), GIB, ample(), Arc::default());
		let gate_to = |open: bool| {
			gate.0.store(open, Ordering::Release);
			if open {
				while w.pump(AMPLE) {}
			}
		};
		// As natively, where the build is within one unit of work: the state published last stands (a store
		// replaced is let go of before the build).
		let seen = |s: Status| (s.adopted, s.state, s.steps_done);
		let waiting = |why: &str, goal: Goal| {
			let before = seen(w.status());
			w.set_goal(goal);
			for _ in 0..3 {
				assert!(w.pump(AMPLE), "{why} waits for its engine");
				assert_eq!(live.now.load(Ordering::Acquire), 0, "{why}: the old engine is freed first");
				assert_eq!(seen(w.status()), before, "{why}: the status stands");
			}
		};
		waiting("the first run", goal(1, &long(40)));
		assert!(w.store().is_none() && w.status().steps_done == 0, "nothing adopted without an engine");
		gate_to(true);
		assert!(w.store().unwrap().is_complete());

		gate_to(false);
		w.rebuild_engine();
		let mut edited = long(40);
		edited.simulation.visualize_every = 4;
		waiting("the rebuild", goal(2, &edited));
		gate_to(true);
		assert_eq!((w.status().adopted, w.status().state), (2, WorkState::Idle));
		assert_as_threaded(&w, &edited);

		gate_to(false);
		let mut wider = edited.clone();
		wider.scene.width = 40;
		waiting("a new grid", goal(3, &wider));
		// Back to the frames in force with another probe, paused, while the new grid's engine is on its way.
		edited.probes[0].ax = 22;
		let back = goal(4, &edited);
		waiting("the frames in force", Goal { paused: true, ..back.clone() });
		gate_to(true);
		assert_eq!((w.status().adopted, w.status().state), (4, WorkState::Paused));
		w.set_goal(back);
		while w.pump(AMPLE) {}
		assert_eq!((w.status().adopted, w.status().state), (4, WorkState::Idle));
		assert_as_threaded(&w, &edited);
		assert_eq!(live.peak.load(Ordering::Acquire), 1, "never two engines at once");
	}

	fn long(steps: u32) -> Project {
		let mut p = project(20);
		p.simulation.steps = steps;
		p
	}

	fn pumped_to_the_end(w: &RunWorker) {
		while w.pump(Duration::ZERO) {}
	}

	#[test]
	fn a_pump_yields_at_its_deadline_and_the_next_goes_on_from_the_steps_done() {
		let w = RunWorker::pumped(cpu(), GIB, ample(), Arc::default());
		let p = long(4000);
		let last = RunRequest::new(&p, PreviewQuality::FULL, false).spec(&mut SceneCache::default()).unwrap();
		let position = last.frame_count() - 1;
		w.set_goal(Goal { position, wanted: Wanted::Position, ..goal(1, &p) });
		assert!(w.pump(Duration::ZERO), "the deadline passed with the frame unmade");
		assert_eq!(w.status().steps_done, CHECK_INTERVAL, "one check interval, then the deadline");
		assert_eq!(w.status().state, WorkState::Stepping { target: last.stored_index_of(position) });
		assert!(w.pump(Duration::ZERO));
		assert_eq!(w.status().steps_done, 2 * CHECK_INTERVAL);
		assert!(!w.pump(AMPLE), "the goal is reached");
		let done = w.status();
		assert_eq!((done.state, done.steps_done), (WorkState::Idle, last.step_of(position) + 1));
		assert!(done.store.unwrap().has(last.stored_index_of(position) as usize));
	}

	#[test]
	fn a_pause_seen_between_steps_yields_and_the_resumed_run_goes_on() {
		let shared = Arc::new(Shared::new());
		let mut w = Worker::new(
			shared.clone(),
			Factory::Later(cpu()),
			Frames::Pending { ram_bytes: GIB, spill: ample() },
			Arc::default(),
		);
		let handle = RunWorker { shared, thread: None, pumped: None };
		let running = goal(1, &long(400));
		handle.set_goal(running.clone());
		for _ in 0..5 {
			assert!(served(w.serve(&running, 1)));
		}
		let before = w.steps_done;
		// The pause arrives while the unit is served, as from another thread.
		handle.set_goal(Goal { paused: true, ..running.clone() });
		w.deadline = Some(Instant::now() + AMPLE);
		assert!(matches!(w.serve(&running, 1), Err(Interrupted::Yield)), "a pumped worker does not wait");
		assert_eq!(handle.status().state, WorkState::Paused);
		assert_eq!(w.steps_done, before);
		handle.set_goal(running.clone());
		assert!(served(w.serve(&running, 3)));
		assert!(w.steps_done > before);
		assert_eq!(w.engine.as_ref().unwrap().time(), w.steps_done, "stepped on, not from 0");
	}

	#[test]
	fn a_paused_goal_ends_the_pump_until_resumed() {
		let w = RunWorker::pumped(cpu(), GIB, ample(), Arc::default());
		let running = goal(1, &long(400));
		w.set_goal(running.clone());
		assert!(w.pump(Duration::ZERO));
		w.set_goal(Goal { paused: true, ..running.clone() });
		assert!(!w.pump(AMPLE));
		let paused = w.status();
		assert_eq!(paused.state, WorkState::Paused);
		w.set_goal(running);
		assert!(w.pump(Duration::ZERO));
		assert!(w.status().steps_done > paused.steps_done);
		pumped_to_the_end(&w);
		assert!(w.store().unwrap().is_complete());
	}

	/// A threaded run of `p`, ended.
	fn threaded(p: &Project) -> (RunWorker, tempfile::TempDir) {
		let dir = tempfile::tempdir().unwrap();
		let threaded = RunWorker::cpu(dir.path().to_path_buf()).unwrap();
		threaded.set_goal(goal(1, p));
		let given_up = Instant::now() + AMPLE;
		while !(threaded.status().state == WorkState::Idle && threaded.store().is_some_and(|s| s.is_complete())) {
			assert!(Instant::now() < given_up, "the threaded run ends");
			std::thread::sleep(Duration::from_millis(1));
		}
		(threaded, dir)
	}

	/// `pumped`'s store is complete with the frames and traces a threaded run of `p` makes.
	fn assert_as_threaded(pumped: &RunWorker, p: &Project) {
		let (threaded, _dir) = threaded(p);
		let (a, b) = (threaded.store().unwrap(), pumped.store().unwrap());
		assert!(b.is_complete());
		for frame in 0..a.capacity() {
			for &layer in a.layers() {
				assert_eq!(a.copy_layer(frame, layer).unwrap(), b.copy_layer(frame, layer).unwrap(), "frame {frame}");
			}
		}
		threaded.with_traces(|t| pumped.with_traces(|u| assert_eq!(t, u)));
	}

	#[test]
	fn a_pumped_run_makes_the_threaded_runs_frames_and_traces() {
		let p = long(300);
		let pumped = RunWorker::pumped(cpu(), GIB, ample(), Arc::default());
		pumped.set_goal(goal(1, &p));
		pumped_to_the_end(&pumped);
		assert_as_threaded(&pumped, &p);
		assert_eq!(pumped.status().traces_steps, 300);
	}

	fn ample() -> Box<dyn SpillFactory> {
		Box::new(fake::Factory::ample())
	}

	/// The bytes of a stored frame of `p`.
	fn frame_bytes(p: &Project) -> u64 {
		let spec = RunRequest::new(p, PreviewQuality::FULL, false).spec(&mut SceneCache::default()).unwrap();
		StoreLayout::of(&spec).frame_bytes() as u64
	}

	/// Pumps to the end, completing the spill's operations after each pump.
	fn pumped_with_spill(w: &RunWorker, disk: &Mutex<fake::Disk>) {
		while w.pump(AMPLE) {
			lock(disk).complete();
		}
	}

	/// The plane of `frame`, its spill read completed when it is pending.
	fn spilled_layer(store: &FrameStore, disk: &Mutex<fake::Disk>, frame: usize, layer: Layer) -> Vec<u8> {
		match store.copy_layer(frame, layer) {
			Err(StoreError::Pending { .. }) => {
				lock(disk).complete();
				store.copy_layer(frame, layer).unwrap()
			}
			read => read.unwrap(),
		}
	}

	#[test]
	fn a_pumped_run_spills_the_frames_past_its_memory_and_reads_them_back() {
		let p = long(40);
		let factory = fake::Factory::ample();
		let disk = factory.disk.clone();
		let w = RunWorker::pumped(cpu(), 3 * frame_bytes(&p), Box::new(factory), Arc::default());
		w.set_goal(goal(1, &p));
		pumped_with_spill(&w, &disk);
		let store = w.store().unwrap();
		assert!(store.is_complete() && !store.spill_writing());
		assert_eq!((store.frames_in_memory(), store.frames_on_disk()), (3, store.capacity() - 3));

		let dir = tempfile::tempdir().unwrap();
		let threaded = RunWorker::cpu(dir.path().to_path_buf()).unwrap();
		threaded.set_goal(goal(1, &p));
		let given_up = Instant::now() + AMPLE;
		while !(threaded.status().state == WorkState::Idle && threaded.store().is_some_and(|s| s.is_complete())) {
			assert!(Instant::now() < given_up, "the threaded run ends");
			std::thread::sleep(Duration::from_millis(1));
		}
		let expected = threaded.store().unwrap();
		for frame in 0..expected.capacity() {
			for &layer in expected.layers() {
				let read = spilled_layer(&store, &disk, frame, layer);
				assert_eq!(expected.copy_layer(frame, layer).unwrap(), read, "frame {frame}");
			}
		}
	}

	#[test]
	fn discarded_frames_go_with_their_store_and_the_position_is_made_anew() {
		let p = long(40);
		let factory = fake::Factory::ample();
		let disk = factory.disk.clone();
		let w = RunWorker::pumped(cpu(), 3 * frame_bytes(&p), Box::new(factory), Arc::default());
		w.set_goal(goal(1, &p));
		pumped_with_spill(&w, &disk);
		let old = Arc::downgrade(&w.store().unwrap());
		w.discard_frames();
		w.set_goal(Goal { wanted: Wanted::Position, ..goal(1, &p) });
		pumped_with_spill(&w, &disk);
		assert!(old.upgrade().is_none(), "the discarded store is dropped");
		let store = w.store().unwrap();
		assert!(store.has(0) && store.present_count() == 1 && store.frames_on_disk() == 0);
		assert_eq!((w.status().adopted, w.status().state), (1, WorkState::Idle));
	}

	#[test]
	fn a_busy_spill_yields_before_the_capture_until_its_writes_complete() {
		let p = long(40);
		let factory = fake::Factory::ample();
		let disk = factory.disk.clone();
		let w = RunWorker::pumped(cpu(), 0, Box::new(factory), Arc::default());
		w.set_goal(goal(1, &p));
		assert!(w.pump(AMPLE));
		let (steps, writes) = (w.status().steps_done, lock(&disk).writes.len());
		assert!(writes >= WRITES_IN_FLIGHT, "{writes} writes in flight");
		for _ in 0..3 {
			assert!(w.pump(AMPLE), "the run waits for its writes");
			assert_eq!((w.status().steps_done, lock(&disk).writes.len()), (steps, writes));
		}
		let mut done = steps;
		while w.pump(AMPLE) {
			lock(&disk).complete();
			let now = w.status().steps_done;
			assert!(now >= done, "no capture was left behind for a run from step 0");
			done = now;
		}
		let store = w.store().unwrap();
		assert!(store.is_complete());
		assert_eq!(store.frames_on_disk(), store.capacity());
	}

	#[test]
	fn a_spill_past_the_free_storage_fails_the_run_at_its_start() {
		let p = long(40);
		let frame = frame_bytes(&p);
		let factory = fake::Factory { free: frame, ..fake::Factory::ample() };
		let w = RunWorker::pumped(cpu(), 3 * frame, Box::new(factory), Arc::default());
		w.set_goal(goal(1, &p));
		assert!(!w.pump(AMPLE));
		let WorkState::Failed(e) = w.status().state else { panic!("the frames past memory do not fit") };
		let RunError::Store(StoreError::Io(io)) = &*e else { panic!("{e}") };
		assert_eq!(io.kind(), std::io::ErrorKind::StorageFull);
		assert_eq!(w.status().steps_done, 0);
	}

	#[test]
	fn a_new_store_waits_for_the_free_storage_to_be_known() {
		let p = long(40);
		let factory = fake::Factory { ready: Arc::new(false.into()), ..fake::Factory::ample() };
		let (ready, disk) = (factory.ready.clone(), factory.disk.clone());
		let w = RunWorker::pumped(cpu(), GIB, Box::new(factory), Arc::default());
		w.set_goal(goal(1, &p));
		for _ in 0..3 {
			assert!(w.pump(AMPLE));
			assert!(w.store().is_none());
		}
		ready.store(true, Ordering::Release);
		pumped_with_spill(&w, &disk);
		assert!(w.store().unwrap().is_complete());
	}

	#[test]
	fn a_failed_spill_write_fails_the_run() {
		let p = long(40);
		let factory = fake::Factory::ample();
		let disk = factory.disk.clone();
		lock(&disk).fail = Some(std::io::ErrorKind::QuotaExceeded);
		let w = RunWorker::pumped(cpu(), 3 * frame_bytes(&p), Box::new(factory), Arc::default());
		w.set_goal(goal(1, &p));
		pumped_with_spill(&w, &disk);
		let WorkState::Failed(e) = w.status().state else { panic!("a write failed") };
		let RunError::Store(StoreError::Io(io)) = &*e else { panic!("{e}") };
		assert_eq!(io.kind(), std::io::ErrorKind::QuotaExceeded);
	}

	/// The CPU engine with readbacks that are mapped once their gate opens, as the GPU's on wasm32; counted in
	/// `live` while it exists.
	struct Gated {
		inner: Box<dyn WaveEngine>,
		probes: Arc<Gate>,
		captures: Arc<Gate>,
		live: Option<Arc<Live>>,
		/// When set, each capture gets a gate of its own, closed and pushed here, in place of `captures`.
		each_capture: Option<Arc<Mutex<Vec<Arc<Gate>>>>>,
		/// When set, the readings queued since the token before are a batch with a gate of its own, closed and
		/// pushed here, in place of `probes`.
		each_probe_batch: Option<Arc<Mutex<Vec<Arc<Gate>>>>>,
		/// The batches not drained yet and the number of the first reading after each.
		batches: Vec<(Arc<Gate>, u64)>,
		/// Number of the first reading not drained.
		drained: u64,
		capacity: usize,
	}

	impl Gated {
		fn new(inner: Box<dyn WaveEngine>, probes: Arc<Gate>, captures: Arc<Gate>, live: Option<Arc<Live>>) -> Self {
			if let Some(live) = &live {
				let now = live.now.fetch_add(1, Ordering::AcqRel) + 1;
				live.peak.fetch_max(now, Ordering::AcqRel);
			}
			Self {
				inner,
				probes,
				captures,
				live,
				each_capture: None,
				each_probe_batch: None,
				batches: Vec::new(),
				drained: 0,
				capacity: 2,
			}
		}

		/// The readings held are dropped by the engine.
		fn drop_batches(&mut self) {
			self.batches.clear();
			self.drained = self.inner.pending_probes().expect("the CPU's readings are on the host").end();
		}
	}

	impl Drop for Gated {
		fn drop(&mut self) {
			if let Some(live) = &self.live {
				live.now.fetch_sub(1, Ordering::AcqRel);
			}
		}
	}

	#[derive(Default)]
	struct Gate(AtomicBool);

	impl crate::sync::Readiness for Gate {
		fn poll_ready(&self, _: &mut Context<'_>) -> std::task::Poll<()> {
			if self.0.load(Ordering::Acquire) { std::task::Poll::Ready(()) } else { std::task::Poll::Pending }
		}
	}

	impl WaveEngine for Gated {
		fn name(&self) -> &'static str {
			"gated"
		}
		fn scene(&self) -> &SceneData {
			self.inner.scene()
		}
		fn options(&self) -> &EngineOptions {
			self.inner.options()
		}
		fn time(&self) -> u32 {
			self.inner.time()
		}
		fn sub_steps(&self) -> u32 {
			self.inner.sub_steps()
		}
		fn reset(&mut self) {
			self.inner.reset();
			self.drop_batches();
		}
		fn set_emitters(&mut self, emitters: &[crate::engine::Emitter]) {
			self.inner.set_emitters(emitters);
		}
		fn set_probes(&mut self, probes: &[crate::engine::ProbeCells]) {
			self.inner.set_probes(probes);
			self.drop_batches();
		}
		fn try_replace_scene(&mut self, scene: &Arc<SceneData>) -> bool {
			self.inner.try_replace_scene(scene)
		}
		fn set_track_intensity(&mut self, on: bool) {
			self.inner.set_track_intensity(on);
		}
		fn advance_one_step(&mut self) -> Result<(), crate::engine::EngineError> {
			self.inner.advance_one_step()
		}
		fn sample_probes(&mut self, out: &mut crate::engine::ProbeReadings) -> Result<(), crate::engine::EngineError> {
			self.inner.sample_probes(out)
		}
		fn queue_probes(&mut self) -> Result<(), crate::engine::EngineError> {
			self.inner.queue_probes()
		}
		fn pending_probes(&mut self) -> Result<PendingProbes, crate::engine::EngineError> {
			let end = self.inner.pending_probes()?.end();
			let Some(issued) = &self.each_probe_batch else {
				// The readings gated as one batch.
				return Ok(PendingProbes { batches: vec![(self.probes.clone(), end)], on_host: 0 });
			};
			if end > self.batches.last().map_or(self.drained, |&(_, end)| end) {
				let gate = Arc::new(Gate::default());
				lock(issued).push(gate.clone());
				self.batches.push((gate, end));
			}
			let batches = self.batches.iter().map(|(g, end)| (g.clone() as Arc<dyn crate::sync::Readiness>, *end));
			Ok(PendingProbes { batches: batches.collect(), on_host: self.drained })
		}
		fn drain_probes(
			&mut self,
			pending: PendingProbes,
			f: &mut dyn FnMut(&crate::engine::ProbeReadings),
		) -> Result<(), crate::engine::EngineError> {
			assert!(resolved(pending.ready()), "drained before mapped");
			let end = pending.end();
			self.inner.drain_probes(pending, f)?;
			self.drained = self.drained.max(end);
			self.batches.retain(|&(_, e)| e > end);
			Ok(())
		}
		fn begin_capture(
			&mut self,
			shape: &FrameShape,
			into: Snapshot,
		) -> Result<PendingCapture, crate::engine::EngineError> {
			let mut pending = self.inner.begin_capture(shape, into)?;
			let gate = match &self.each_capture {
				Some(issued) => {
					let gate = Arc::new(Gate::default());
					lock(issued).push(gate.clone());
					gate
				}
				None => self.captures.clone(),
			};
			pending.mapped = Some(gate);
			Ok(pending)
		}
		fn capture_capacity(&self, _: &FrameShape) -> usize {
			self.capacity
		}
		fn finish_capture(&mut self, pending: PendingCapture) -> Result<Snapshot, crate::engine::EngineError> {
			assert!(resolved(pending.ready()), "finished before mapped");
			self.inner.finish_capture(pending)
		}
		fn prepare_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), crate::engine::EngineError> {
			self.inner.prepare_frame()
		}
	}

	#[test]
	fn a_readback_not_mapped_yields_until_it_is() {
		let (probes, captures) = (Arc::new(Gate::default()), Arc::new(Gate::default()));
		let factory: PumpedFactory = {
			let (probes, captures) = (probes.clone(), captures.clone());
			Box::new(move |scene, options| -> EngineFuture {
				let inner = Box::new(CpuEngine::new(scene, options));
				let engine = Gated::new(inner, probes.clone(), captures.clone(), None);
				Box::pin(std::future::ready(Box::new(engine) as Box<dyn WaveEngine>))
			})
		};
		let w = RunWorker::pumped(factory, GIB, ample(), Arc::default());
		w.set_goal(Goal { fit: Some(FitRequest { id: 7, frame: 10 }), ..goal(1, &long(40)) });
		for gate in [&probes, &captures] {
			for _ in 0..3 {
				assert!(w.pump(AMPLE), "waits for the gate without failing");
			}
			assert!(w.take_fit().is_none());
			assert_eq!(w.store().unwrap().present_count(), 0);
			gate.0.store(true, Ordering::Release);
		}
		assert!(!w.pump(AMPLE));
		assert_eq!(w.take_fit().map(|f| (f.id, f.frame)), Some((7, 10)));
		assert!(w.store().unwrap().is_complete());
		assert_eq!(w.status().traces_steps, 40);
	}

	#[test]
	fn a_discard_waits_for_the_capture_in_flight() {
		let captures = Arc::new(Gate::default());
		let factory: PumpedFactory = {
			let captures = captures.clone();
			Box::new(move |scene, options| -> EngineFuture {
				let inner = Box::new(CpuEngine::new(scene, options));
				let engine = Gated::new(inner, Arc::new(Gate(AtomicBool::new(true))), captures.clone(), None);
				Box::pin(std::future::ready(Box::new(engine) as Box<dyn WaveEngine>))
			})
		};
		let w = RunWorker::pumped(factory, GIB, ample(), Arc::default());
		let run = goal(1, &long(40));
		w.set_goal(run.clone());
		assert!(w.pump(AMPLE) && w.pump(AMPLE), "frame 0's capture is in flight");
		let old = Arc::downgrade(&w.store().unwrap());
		w.discard_frames();
		w.set_goal(Goal { wanted: Wanted::Position, ..run });
		assert!(w.pump(AMPLE), "the discard waits for the capture");
		captures.0.store(true, Ordering::Release);
		while w.pump(AMPLE) {}
		assert!(old.upgrade().is_none(), "the discard was not lost to the yield");
		assert_eq!(w.store().unwrap().present_count(), 1);
	}

	#[test]
	fn a_fit_the_goal_moved_past_mid_readback_is_not_served() {
		let (probes, captures) = (Arc::new(Gate(AtomicBool::new(true))), Arc::new(Gate::default()));
		let factory: PumpedFactory = {
			let (probes, captures) = (probes.clone(), captures.clone());
			Box::new(move |scene, options| -> EngineFuture {
				let inner = Box::new(CpuEngine::new(scene, options));
				let engine = Gated::new(inner, probes.clone(), captures.clone(), None);
				Box::pin(std::future::ready(Box::new(engine) as Box<dyn WaveEngine>))
			})
		};
		let w = RunWorker::pumped(factory, GIB, ample(), Arc::default());
		let run = goal(1, &long(40));
		w.set_goal(Goal { fit: Some(FitRequest { id: 7, frame: 10 }), ..run.clone() });
		for _ in 0..3 {
			assert!(w.pump(AMPLE));
		}
		w.set_goal(run);
		captures.0.store(true, Ordering::Release);
		while w.pump(AMPLE) {}
		assert!(w.take_fit().is_none());
	}

	#[test]
	fn the_frames_passed_on_the_way_are_captured_with_readbacks_mapped_between_pumps() {
		let issued: Arc<Mutex<Vec<Arc<Gate>>>> = Arc::default();
		let factory: PumpedFactory = {
			let issued = issued.clone();
			Box::new(move |scene, options| -> EngineFuture {
				let inner = Box::new(CpuEngine::new(scene, options));
				let mut engine = Gated::new(inner, Arc::new(Gate(AtomicBool::new(true))), Arc::default(), None);
				engine.each_capture = Some(issued.clone());
				Box::pin(std::future::ready(Box::new(engine) as Box<dyn WaveEngine>))
			})
		};
		let w = RunWorker::pumped(factory, GIB, ample(), Arc::default());
		let mut p = long(400);
		p.probes.clear();
		let spec = RunRequest::new(&p, PreviewQuality::FULL, false).spec(&mut SceneCache::default()).unwrap();
		let position = spec.frame_count() / 2;
		let stored = spec.stored_index_of(position) as usize;
		w.set_goal(Goal { position, ..goal(1, &p) });
		let mut last = 0;
		let mut seen = false;
		while w.pump(AMPLE) {
			// As WebGPU's mapAsync: a readback maps after the pump that began it returned.
			for gate in lock(&issued).drain(..) {
				gate.0.store(true, Ordering::Release);
			}
			let steps = w.status().steps_done;
			assert!(steps >= last, "the run went back to step 0 from {last}");
			last = steps;
			if !seen && let Some(store) = w.store().filter(|s| s.has(stored)) {
				seen = true;
				let missing: Vec<usize> = (0..stored).filter(|&j| !store.has(j)).collect();
				assert!(missing.is_empty(), "frames passed on the way to {stored} are missing: {missing:?}");
			}
		}
		assert!(seen && w.store().unwrap().is_complete());
	}

	#[test]
	fn a_pumped_run_keeps_as_many_captures_in_flight_as_its_engine_takes() {
		let issued: Arc<Mutex<Vec<Arc<Gate>>>> = Arc::default();
		let factory: PumpedFactory = {
			let issued = issued.clone();
			Box::new(move |scene, options| -> EngineFuture {
				let inner = Box::new(CpuEngine::new(scene, options));
				let mut engine = Gated::new(inner, Arc::new(Gate(AtomicBool::new(true))), Arc::default(), None);
				engine.each_capture = Some(issued.clone());
				engine.capacity = 4;
				Box::pin(std::future::ready(Box::new(engine) as Box<dyn WaveEngine>))
			})
		};
		let w = RunWorker::pumped(factory, GIB, ample(), Arc::default());
		let mut p = long(400);
		p.probes.clear();
		w.set_goal(goal(1, &p));
		assert!(w.pump(AMPLE));
		assert_eq!(lock(&issued).len(), 4, "the ring is full, then the pump yields");
		assert_eq!(w.store().unwrap().present_count(), 0);
		for gate in lock(&issued).iter() {
			gate.0.store(true, Ordering::Release);
		}
		assert!(w.pump(AMPLE));
		assert_eq!(w.store().unwrap().present_count(), 4, "the captures read back land in one pump");
		assert_eq!(lock(&issued).len(), 8);
	}

	#[test]
	fn probe_drains_on_the_way_do_not_wait_for_their_readbacks() {
		let probes = Arc::new(Gate::default());
		let factory: PumpedFactory = {
			let probes = probes.clone();
			Box::new(move |scene, options| -> EngineFuture {
				let inner = Box::new(CpuEngine::new(scene, options));
				let engine = Gated::new(inner, probes.clone(), Arc::new(Gate(AtomicBool::new(true))), None);
				Box::pin(std::future::ready(Box::new(engine) as Box<dyn WaveEngine>))
			})
		};
		let w = RunWorker::pumped(factory, GIB, ample(), Arc::default());
		let p = long(4000);
		let spec = RunRequest::new(&p, PreviewQuality::FULL, false).spec(&mut SceneCache::default()).unwrap();
		let position = spec.frame_count() - 1;
		w.set_goal(Goal { position, wanted: Wanted::Position, ..goal(1, &p) });
		assert!(w.pump(AMPLE));
		let status = w.status();
		assert!(status.steps_done > PROBE_DRAIN, "stepped past a drain in one pump: {}", status.steps_done);
		assert_eq!(status.traces_steps, 0, "the target's readings are not read back yet");
		probes.0.store(true, Ordering::Release);
		assert!(!w.pump(AMPLE));
		assert_eq!(w.status().traces_steps, spec.step_of(position) + 1);
		assert!(w.store().unwrap().has(spec.stored_index_of(position) as usize));
	}

	/// A pumped CPU engine whose captures and probe batches each map once their gate, pushed to `issued`, opens.
	fn pumped_gated(issued: &Arc<Mutex<Vec<Arc<Gate>>>>) -> RunWorker {
		let issued = issued.clone();
		let factory: PumpedFactory = Box::new(move |scene, options| -> EngineFuture {
			let inner = Box::new(CpuEngine::new(scene, options));
			let mut engine = Gated::new(inner, Arc::default(), Arc::default(), None);
			engine.each_capture = Some(issued.clone());
			engine.each_probe_batch = Some(issued.clone());
			engine.capacity = 8;
			Box::pin(std::future::ready(Box::new(engine) as Box<dyn WaveEngine>))
		});
		RunWorker::pumped(factory, GIB, ample(), Arc::default())
	}

	/// Pumps up to `limit` times, the readbacks begun mapped after each pump as WebGPU's mapAsync; asserts no
	/// revision's run goes back to step 0. The pumps done.
	fn pump_mapped(w: &RunWorker, issued: &Mutex<Vec<Arc<Gate>>>, limit: usize) -> usize {
		let mut last = (w.status().adopted, w.status().steps_done);
		for pumps in 0..limit {
			let more = w.pump(AMPLE);
			for gate in lock(issued).drain(..) {
				gate.0.store(true, Ordering::Release);
			}
			let now = (w.status().adopted, w.status().steps_done);
			assert!(now.0 != last.0 || now.1 >= last.1, "the run went back from step {} to {}", last.1, now.1);
			last = now;
			if !more {
				return pumps + 1;
			}
		}
		limit
	}

	/// `w` rests with every reading recorded: its traces are `p`'s threaded run's up to the steps done.
	fn assert_readings_whole(w: &RunWorker, p: &Project) {
		let status = w.status();
		assert_eq!(status.traces_steps, status.steps_done, "readings left in flight at {:?}", status.state);
		let (threaded, _dir) = threaded(p);
		let steps = status.steps_done as usize;
		threaded.with_traces(|t| {
			w.with_traces(|u| {
				assert_eq!(t.len(), u.len());
				for (t, u) in t.iter().zip(u) {
					assert_eq!(u.series, t.series[..steps], "the series up to step {steps}");
				}
			});
		});
	}

	#[test]
	fn probe_runs_chain_their_frames_without_waiting_for_each_frames_readings() {
		let issued: Arc<Mutex<Vec<Arc<Gate>>>> = Arc::default();
		let w = pumped_gated(&issued);
		let p = long(400);
		w.set_goal(goal(1, &p));
		let pumps = pump_mapped(&w, &issued, 10_000);
		let frames = w.store().unwrap().capacity();
		assert!(pumps * 4 <= frames, "{pumps} pumps for {frames} stored frames");
		assert_eq!((w.status().state, w.status().traces_steps), (WorkState::Idle, 400));
		assert_as_threaded(&w, &p);
	}

	#[test]
	fn a_paused_or_stopped_probe_run_has_every_reading_recorded() {
		let issued: Arc<Mutex<Vec<Arc<Gate>>>> = Arc::default();
		let p = long(400);
		let running = goal(1, &p);
		for (why, rest) in [
			("paused", Goal { paused: true, ..running.clone() }),
			("stopped", Goal { wanted: Wanted::Position, ..running.clone() }),
		] {
			let w = pumped_gated(&issued);
			w.set_goal(running.clone());
			assert_eq!(pump_mapped(&w, &issued, 3), 3, "{why}: mid-run");
			assert!(w.status().steps_done > 0);
			w.set_goal(rest.clone());
			assert!(pump_mapped(&w, &issued, 100) < 100, "{why}: the run rests");
			let expected = if rest.paused { WorkState::Paused } else { WorkState::Idle };
			assert_eq!(w.status().state, expected, "{why}");
			assert_readings_whole(&w, &p);
			w.set_goal(running.clone());
			pump_mapped(&w, &issued, 10_000);
			assert_as_threaded(&w, &p);
		}
	}

	#[test]
	fn a_probe_edit_mid_run_records_the_edited_probes_whole() {
		let issued: Arc<Mutex<Vec<Arc<Gate>>>> = Arc::default();
		let w = pumped_gated(&issued);
		let p = long(400);
		w.set_goal(goal(1, &p));
		assert_eq!(pump_mapped(&w, &issued, 3), 3);
		let mut edited = p.clone();
		edited.probes[0].ax = 22;
		w.set_goal(goal(2, &edited));
		pump_mapped(&w, &issued, 10_000);
		assert_eq!((w.status().adopted, w.status().traces_steps), (2, 400));
		assert_as_threaded(&w, &edited);
	}
}
