//! The live run: one worker thread, one engine and one store that converge on a [`Goal`] (spec/30-engine.md,
//! The live run).
//!
//! The goal names the project state ([`RunRequest`], whose spec the worker builds, composing the scene off
//! the UI thread), the timeline position, how much is wanted ([`Wanted`]) and whether the user paused. Between two frames the worker reads the newest goal and derives its next target: the first
//! stored frame at or after the position that is missing, or whose probe traces are not recorded yet; with
//! [`Wanted::All`], when none follows, the first such frame before it. It steps to the target (from 0 when the
//! engine has passed it), captures it when missing and, with everything wanted, the missing frames it passes,
//! records the probes on the way and publishes progress and traces. A spec whose frames differ starts a generation: the store is cleared
//! (its buffers kept), the engine reused (same scene: new emitters; same grid: the scene swapped in; else
//! rebuilt). A spec whose probes alone differ clears the traces and keeps the frames. A goal change is looked
//! at every [`CHECK_INTERVAL`] steps: work goes on only for the goal's own spec, so an edit means one run.
//!
//! Threads: [`RunWorker::set_goal`] and the readers may be called from any thread; the work runs on one
//! thread, which owns the engine and writes the store. Traces and progress are published at heartbeats and at
//! every frame.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::engine::cpu::CpuEngine;
use crate::engine::probes::{ProbeCollector, ProbeTrace};
use crate::engine::{EngineOptions, PendingCapture, SampleFormat, SceneData, Snapshot, WaveEngine};

use super::simulation::{RunError, record};
use super::spec::{RunRequest, RunSpec, SceneCache};
use super::store::{Budget, FrameStore};

/// Steps between two looks at the pause and shutdown flags.
pub const CHECK_INTERVAL: u32 = 16;
/// Steps between two drains of the engine's queued probe readings.
const PROBE_DRAIN: u32 = 256;
const HEARTBEAT: Duration = Duration::from_millis(33);
/// How long a new store's producer waits for the readers of the store it replaces.
const RELEASE_WAIT: Duration = Duration::from_secs(1);

/// Builds an engine for a scene and options.
pub type EngineFactory = Box<dyn Fn(Arc<SceneData>, EngineOptions) -> Box<dyn WaveEngine> + Send>;

/// How much of the run the goal wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wanted {
	/// The frame at the position alone.
	#[default]
	Position,
	/// Every frame, the position's first.
	All,
}

/// A 16-bit frame of timeline frame `frame`, for a brightness fit; served once per `id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FitRequest {
	pub id: u64,
	pub frame: u32,
}

/// What the worker converges on. `revision` moves with every project change; [`Status::adopted`] echoes the
/// revision whose frames the store holds.
#[derive(Clone, Debug, Default)]
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
}

/// The frame a [`FitRequest`] asked for.
#[derive(Clone, Debug)]
pub struct FitFrame {
	pub id: u64,
	pub frame: u32,
	pub snapshot: Arc<Snapshot>,
}

#[derive(Clone, Debug)]
struct Published {
	adopted: u64,
	state: WorkState,
	steps_done: u32,
	traces_steps: u32,
}

struct Shared {
	goal: Mutex<(Goal, u64)>,
	wake: Condvar,
	/// The goal's number, for a cheap "did it change" while stepping.
	goal_number: AtomicU64,
	paused: AtomicBool,
	/// The engine is to be built anew (the device changed).
	rebuild: AtomicBool,
	shutdown: AtomicBool,
	published: Mutex<Published>,
	store: Mutex<Option<Arc<FrameStore>>>,
	traces: Mutex<Vec<ProbeTrace>>,
	/// The traces of the generation before, while its frames are still looked at.
	held: Mutex<Vec<ProbeTrace>>,
	fit: Mutex<Option<FitFrame>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The handle; dropping it stops the worker after its current check and joins it.
pub struct RunWorker {
	shared: Arc<Shared>,
	thread: Option<JoinHandle<()>>,
}

impl RunWorker {
	/// A worker whose engines `factory` builds, with its spill files in `cache_dir` and its scenes from
	/// `scene` (shared with whatever else composes the scene).
	pub fn new(factory: EngineFactory, cache_dir: PathBuf, scene: Arc<Mutex<SceneCache>>) -> std::io::Result<Self> {
		let shared = Arc::new(Shared {
			goal: Mutex::new((Goal::default(), 0)),
			wake: Condvar::new(),
			goal_number: AtomicU64::new(0),
			paused: AtomicBool::new(false),
			rebuild: AtomicBool::new(false),
			shutdown: AtomicBool::new(false),
			published: Mutex::new(Published { adopted: 0, state: WorkState::Idle, steps_done: 0, traces_steps: 0 }),
			store: Mutex::new(None),
			traces: Mutex::new(Vec::new()),
			held: Mutex::new(Vec::new()),
			fit: Mutex::new(None),
		});
		let worker = Worker {
			shared: shared.clone(),
			factory,
			cache_dir,
			scene,
			built: None,
			engine: None,
			current: None,
			steps_done: 0,
			collector: None,
			recorded: 0,
			traces_steps: 0,
			pending: None,
			served_fit: None,
			last_beat: Instant::now(),
			goal_seen: 0,
		};
		let thread = std::thread::Builder::new().name("live-run".into()).spawn(move || worker.run())?;
		Ok(Self { shared, thread: Some(thread) })
	}

	/// A worker on the CPU engine.
	pub fn cpu(cache_dir: PathBuf) -> std::io::Result<Self> {
		Self::new(Box::new(|scene, options| Box::new(CpuEngine::new(scene, options))), cache_dir, Arc::default())
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
		self.shared.rebuild.store(true, Ordering::Release);
		let mut g = lock(&self.shared.goal);
		g.1 += 1;
		self.shared.goal_number.store(g.1, Ordering::Release);
		drop(g);
		self.shared.wake.notify_one();
	}

	pub fn status(&self) -> Status {
		let p = lock(&self.shared.published).clone();
		Status {
			adopted: p.adopted,
			state: p.state,
			steps_done: p.steps_done,
			traces_steps: p.traces_steps,
			store: lock(&self.shared.store).clone(),
		}
	}

	pub fn store(&self) -> Option<Arc<FrameStore>> {
		lock(&self.shared.store).clone()
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
}

impl Drop for RunWorker {
	fn drop(&mut self) {
		self.shared.shutdown.store(true, Ordering::Release);
		self.shared.wake.notify_one();
		if let Some(t) = self.thread.take() {
			let _ = t.join();
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
	factory: EngineFactory,
	cache_dir: PathBuf,
	scene: Arc<Mutex<SceneCache>>,
	/// The spec built last and its request, so a goal's spec is built once.
	built: Option<(RunRequest, RunSpec)>,
	engine: Option<Box<dyn WaveEngine>>,
	current: Option<Current>,
	/// The engine's steps since its reset for the spec in force.
	steps_done: u32,
	collector: Option<ProbeCollector>,
	/// Steps whose readings are in the collector.
	recorded: u32,
	/// Steps the published traces hold.
	traces_steps: u32,
	/// A capture begun, finished before the next one or when the engine stops; its stored frame.
	pending: Option<(u32, PendingCapture)>,
	served_fit: Option<u64>,
	last_beat: Instant,
	/// The goal number the unit of work started from.
	goal_seen: u64,
}

impl Worker {
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
			match self.serve(&goal, number) {
				Ok(true) => {}
				Ok(false) => self.wait_for_change(number),
				Err(Interrupted::Stop) => {
					let _ = self.finish_pending();
					return;
				}
				Err(Interrupted::Goal) => {}
				Err(Interrupted::Failed(e)) => {
					tracing::warn!("the live run failed: {e}");
					self.forget();
					self.publish(WorkState::Failed(Arc::new(e)));
					failed_at = Some(number);
				}
			}
		}
	}

	/// Waits until the goal is no longer `number` or the worker is stopped.
	fn wait_for_change(&self, number: u64) {
		let mut g = lock(&self.shared.goal);
		while g.1 == number && !self.shared.shutdown.load(Ordering::Acquire) {
			g = self.shared.wake.wait(g).unwrap_or_else(PoisonError::into_inner);
		}
	}

	/// One step of the loop: adopts the goal's spec and does one unit of work. Ok(false) when nothing is to do.
	fn serve(&mut self, goal: &Goal, number: u64) -> Result<bool, Interrupted> {
		self.goal_seen = number;
		let Some(request) = &goal.run else {
			self.finish_pending()?;
			self.publish(WorkState::Idle);
			return Ok(false);
		};
		let spec = self.spec_of(request).map_err(|e| Interrupted::Failed(e.into()))?;
		self.adopt(&spec, goal)?;
		if self.shared.rebuild.swap(false, Ordering::AcqRel) {
			self.finish_pending()?;
			self.restart_engine(None, &spec);
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
	fn spec_of(&mut self, request: &RunRequest) -> Result<RunSpec, super::spec::RunSpecError> {
		if let Some((r, spec)) = &self.built
			&& r.same_as(request)
		{
			return Ok(spec.clone());
		}
		let spec = request.spec(&mut lock(&self.scene))?;
		self.built = Some((request.clone(), spec.clone()));
		Ok(spec)
	}

	/// Puts `spec` in force: a generation when its frames differ, new traces when its probes do.
	fn adopt(&mut self, spec: &RunSpec, goal: &Goal) -> Result<(), Interrupted> {
		let same_frames = self.current.as_ref().is_some_and(|c| c.spec.same_frames_as(spec));
		let same_probes = self.current.as_ref().is_some_and(|c| c.spec.same_probes_as(spec));
		if !same_frames {
			self.finish_pending()?;
			let layout = spec.store_layout();
			let previous = self.current.take();
			let old_spec = previous.as_ref().map(|c| c.spec.clone());
			let store = match previous {
				Some(c) if c.store.layout().fits(&layout) => {
					c.store.clear();
					c.store
				}
				old => {
					// The old store's memory counts as free once every reader let go of it; the new store's
					// producer waits for that (bounded, a reader may hold on).
					*lock(&self.shared.store) = None;
					let old = old.map(|c| c.store);
					let reclaimed = old.as_ref().map_or(0, |s| s.ram_bytes());
					let release = old.map(|old| -> super::store::Release {
						Box::new(move || {
							let deadline = Instant::now() + RELEASE_WAIT;
							while Arc::strong_count(&old) > 1 && Instant::now() < deadline {
								std::thread::sleep(Duration::from_millis(1));
							}
							drop(old);
						})
					});
					let budget = Budget::compute(reclaimed, goal.ram_percent);
					let store = FrameStore::in_dir(layout, budget, release, &self.cache_dir)
						.map_err(|e| Interrupted::Failed(RunError::Store(e.into())))?;
					Arc::new(store)
				}
			};
			{
				let mut traces = lock(&self.shared.traces);
				let mut held = lock(&self.shared.held);
				*held = if same_probes { std::mem::take(&mut *traces) } else { Vec::new() };
				traces.clear();
				*lock(&self.shared.store) = Some(store.clone());
			}
			self.reset_engine(old_spec.as_ref(), spec);
			self.current = Some(Current { spec: spec.clone(), store });
			self.restart_traces(spec);
			tracing::debug!(revision = goal.revision, steps = spec.steps, "live run: new frames");
		} else {
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
		}
		let mut p = lock(&self.shared.published);
		p.adopted = goal.revision;
		p.steps_done = self.steps_done;
		p.traces_steps = self.traces_steps;
		Ok(())
	}

	/// A collector for `spec`'s probes on a reset engine; the published traces start over.
	fn restart_traces(&mut self, spec: &RunSpec) {
		let collector = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene, spec.seed));
		let engine = self.engine.as_mut().expect("an engine goes with the current spec");
		engine.set_probes(collector.as_ref().map(ProbeCollector::cells).unwrap_or_default());
		*lock(&self.shared.traces) = collector.as_ref().map(|c| c.traces.clone()).unwrap_or_default();
		self.collector = collector;
		self.recorded = 0;
		self.traces_steps = 0;
	}

	/// The engine reset for `spec`, from `old`'s: as it is for the same scene, with the scene swapped in for
	/// the same grid, else a new one.
	fn reset_engine(&mut self, old: Option<&RunSpec>, spec: &RunSpec) {
		let kept = self.engine.take().and_then(|mut engine| match old {
			Some(old) if old.same_scene_as(spec) => {
				if old.emitters != spec.emitters {
					engine.set_emitters(&spec.emitters);
				}
				engine.set_track_intensity(spec.options.track_intensity);
				Some(engine)
			}
			Some(old) if old.same_grid_as(spec) && engine.try_replace_scene(&spec.scene) => {
				tracing::debug!("live run: scene replaced");
				engine.set_emitters(&spec.emitters);
				engine.set_track_intensity(spec.options.track_intensity);
				Some(engine)
			}
			_ => None,
		});
		let mut engine = kept.unwrap_or_else(|| {
			let started = Instant::now();
			let mut e = (self.factory)(spec.scene.clone(), spec.options.clone());
			e.set_emitters(&spec.emitters);
			tracing::debug!(ms = started.elapsed().as_secs_f64() * 1e3, "live run: engine built");
			e
		});
		engine.reset();
		self.engine = Some(engine);
		self.steps_done = 0;
	}

	/// The engine back at step 0 (kept from `old`'s spec, or built anew), the readings so far published and a
	/// collector for the steps to come: the traces grow on from where they are.
	fn restart_engine(&mut self, old: Option<&RunSpec>, spec: &RunSpec) {
		self.publish_traces();
		self.reset_engine(old, spec);
		self.collector = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene, spec.seed));
		let engine = self.engine.as_mut().expect("reset_engine leaves an engine");
		engine.set_probes(self.collector.as_ref().map(ProbeCollector::cells).unwrap_or_default());
		self.recorded = 0;
	}

	/// Drops a failed engine and its frames, so the next goal builds anew.
	fn forget(&mut self) {
		self.pending = None;
		self.engine = None;
		self.current = None;
		self.collector = None;
		*lock(&self.shared.store) = None;
		lock(&self.shared.traces).clear();
		lock(&self.shared.held).clear();
	}

	/// Whether stored frame `j` is there or on its way (a capture in flight).
	fn has_frame(&self, j: u32) -> bool {
		self.current.as_ref().is_some_and(|c| c.store.has(j as usize))
			|| self.pending.as_ref().is_some_and(|(f, _)| *f == j)
	}

	/// Stored frame `j` is wanted: missing, or its probe traces not recorded.
	fn needed(&self, j: u32) -> bool {
		let Some(c) = self.current.as_ref() else { return false };
		!self.has_frame(j) || (!c.spec.probes.is_empty() && self.traces_steps < j * c.spec.capture_every() + 1)
	}

	/// The stored frame `goal` looks at.
	fn position_of(&self, goal: &Goal) -> u32 {
		let spec = &self.current.as_ref().expect("a current spec").spec;
		spec.stored_index_of(goal.position.min(spec.frame_count().saturating_sub(1)))
	}

	/// The next target by the module's rule. With everything wanted, the frames after the position come
	/// first, then the probes' last steps while the engine stands at the traces' end (as a batch run records
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
		let tail = !spec.probes.is_empty() && self.traces_steps < spec.steps;
		if tail && self.steps_done == self.traces_steps {
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
		self.capture(target)?;
		self.publish_traces();
		self.publish(WorkState::Stepping { target });
		Ok(())
	}

	/// Begins the capture of the engine's state as stored frame `j` unless it is there; the capture in flight
	/// before it lands in the store.
	fn capture(&mut self, j: u32) -> Result<(), Interrupted> {
		if self.has_frame(j) {
			return Ok(());
		}
		let (store, shape) = {
			let c = self.current.as_ref().expect("a capture needs a current spec");
			(c.store.clone(), c.spec.shape)
		};
		let engine = self.engine.as_mut().expect("an engine goes with the current spec");
		let buffer = store.buffer_for(j as usize);
		let next = engine.begin_capture(&shape, buffer).map_err(|e| Interrupted::Failed(e.into()))?;
		if let Some((frame, previous)) = self.pending.replace((j, next)) {
			let snapshot = engine.finish_capture(previous).map_err(|e| Interrupted::Failed(e.into()))?;
			store.write_at(frame as usize, snapshot).map_err(|e| Interrupted::Failed(e.into()))?;
		}
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
		self.finish_pending()?;
		let engine = self.engine.as_mut().expect("an engine goes with the current spec");
		let half = crate::engine::FrameShape { format: SampleFormat::Half, ..shape };
		let snapshot = engine.capture_frame(&half).map_err(|e| Interrupted::Failed(e.into()))?;
		*lock(&self.shared.fit) = Some(FitFrame { id: fit.id, frame: fit.frame, snapshot: Arc::new(snapshot) });
		self.served_fit = Some(fit.id);
		tracing::debug!(frame = fit.frame, "live run: fit frame");
		Ok(())
	}

	/// Brings the engine to `steps` steps done (from 0 when it has passed them), recording the probes and, with
	/// `passed`, capturing the missing stored frames on the way. Every [`CHECK_INTERVAL`] steps a pause is
	/// honoured, and a new goal is looked at: the unit goes on when it is still what the goal wants
	/// ([`still_wanted`](Self::still_wanted)), else it is left.
	fn advance_to(&mut self, steps: u32, unit: Unit, state: WorkState, passed: bool) -> Result<(), Interrupted> {
		if self.steps_done > steps {
			self.finish_pending()?;
			let spec = self.current.as_ref().expect("a current spec").spec.clone();
			self.restart_engine(Some(&spec), &spec);
		}
		if self.steps_done == steps {
			return Ok(());
		}
		let (every, capture_every) = {
			let spec = &self.current.as_ref().expect("a current spec").spec;
			(spec.visualize_every, spec.capture_every())
		};
		self.publish(state.clone());
		let from = self.steps_done;
		for i in from..steps {
			if (i - from).is_multiple_of(CHECK_INTERVAL) {
				self.check_goal(unit, &state)?;
			}
			let engine = self.engine.as_mut().expect("an engine goes with the current spec");
			engine.advance_one_step().map_err(|e| Interrupted::Failed(e.into()))?;
			if self.collector.is_some() {
				engine.queue_probes().map_err(|e| Interrupted::Failed(e.into()))?;
			}
			self.steps_done = i + 1;
			if passed && i + 1 < steps && i.is_multiple_of(capture_every) {
				self.capture(i / capture_every)?;
			}
			if (i + 1 - from).is_multiple_of(PROBE_DRAIN) {
				self.record(every)?;
			}
			if self.last_beat.elapsed() >= HEARTBEAT {
				self.last_beat = Instant::now();
				self.record(every)?;
				self.publish_traces();
				self.publish(state.clone());
			}
		}
		self.record(every)
	}

	/// Drains the engine's queued readings into the collector.
	fn record(&mut self, every: u32) -> Result<(), Interrupted> {
		if let Some(c) = self.collector.as_mut() {
			let engine = self.engine.as_mut().expect("an engine goes with the current spec");
			record(engine.as_mut(), c, &mut self.recorded, every).map_err(|e| Interrupted::Failed(e.into()))?;
		}
		Ok(())
	}

	/// Whether `unit` is still worth finishing under `goal`: only work for the goal's own spec is. Of that, the
	/// frame the goal looks at always is; a frame for later while every frame is wanted and the goal's own
	/// frame is there; the probes' tail needs the same probes too; a fit while it is still asked for.
	fn still_wanted(&mut self, unit: Unit, goal: &Goal) -> bool {
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

	/// Stops, pauses and goal changes, looked at between steps.
	fn check_goal(&mut self, unit: Unit, state: &WorkState) -> Result<(), Interrupted> {
		if self.shared.shutdown.load(Ordering::Acquire) {
			return Err(Interrupted::Stop);
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

	/// Writes the capture in flight into the store.
	fn finish_pending(&mut self) -> Result<(), Interrupted> {
		let Some((frame, pending)) = self.pending.take() else { return Ok(()) };
		let (Some(engine), Some(current)) = (self.engine.as_mut(), self.current.as_ref()) else { return Ok(()) };
		let snapshot = engine.finish_capture(pending).map_err(|e| Interrupted::Failed(e.into()))?;
		current.store.write_at(frame as usize, snapshot).map_err(|e| Interrupted::Failed(e.into()))
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

	fn publish(&self, state: WorkState) {
		let mut p = lock(&self.shared.published);
		p.state = state;
		p.steps_done = self.steps_done;
		p.traces_steps = self.traces_steps;
	}
}

enum Target {
	/// A stored frame.
	Frame(u32),
	/// The run's last step, for the traces.
	End,
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
	Failed(RunError),
}
