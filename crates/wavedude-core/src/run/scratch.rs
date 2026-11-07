//! Scratch frames: "frame k of the project as it is now" for the live preview (spec/30-engine.md, Scratch frames).
//!
//! The kept engine steps on from where the previous request left it (finished or abandoned) when the project's
//! steps are the same and it has not passed the frame; recent frames come from a cache.
//!
//! Threads: one worker thread owns the kept engine and computes the newest request; [`ScratchRunner::request`]
//! may be called from any thread. A request superseded while stepping is abandoned at the next check, every
//! [`CHECK_INTERVAL`] steps. Events are delivered on the worker thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::engine::cpu::CpuEngine;
use crate::engine::probes::{ProbeCollector, ProbeTrace};
use crate::engine::{EngineError, EngineOptions, ProbeReadings, SceneData, Snapshot, WaveEngine};

use super::simulation::RunError;
use super::spec::RunSpec;

/// Steps between two looks for a newer request.
pub const CHECK_INTERVAL: u32 = 16;
/// Bytes of planes the recent frames keep.
pub const CACHE_BYTES: usize = 256 << 20;

/// A computed scratch frame.
#[derive(Clone, Debug)]
pub struct ScratchResult {
	pub id: u64,
	pub spec: RunSpec,
	/// Timeline frame.
	pub frame: u32,
	/// Holds the state after step index `frame * visualize_every`.
	pub snapshot: Snapshot,
	/// Every series up to the frame's step; each line probe one profile, the frame's.
	pub traces: Vec<ProbeTrace>,
	pub elapsed: Duration,
}

#[derive(Debug)]
pub enum ScratchEvent {
	Done(Box<ScratchResult>),
	/// Left for a newer request after `steps_done` of `steps`.
	Abandoned {
		id: u64,
		frame: u32,
		steps_done: u32,
		steps: u32,
	},
	Failed {
		id: u64,
		error: Arc<RunError>,
	},
}

/// Builds an engine for a scene and options.
pub type EngineFactory = Box<dyn Fn(Arc<SceneData>, EngineOptions) -> Box<dyn WaveEngine> + Send>;
pub type ScratchSink = Box<dyn FnMut(ScratchEvent) + Send>;

struct Request {
	id: u64,
	spec: RunSpec,
	frame: u32,
}

#[derive(Default)]
struct Inbox {
	request: Option<Request>,
	shutdown: bool,
}

struct Shared {
	inbox: Mutex<Inbox>,
	wake: Condvar,
	/// The newest request's id; the computation of any other is abandoned.
	latest: AtomicU64,
}

/// The kept engine and its worker. Dropping it stops the worker after its current check.
pub struct ScratchRunner {
	shared: Arc<Shared>,
	thread: Option<JoinHandle<()>>,
}

impl ScratchRunner {
	/// A runner whose engines `factory` builds, reporting to `sink`.
	pub fn new(factory: EngineFactory, sink: ScratchSink) -> std::io::Result<Self> {
		let shared = Arc::new(Shared { inbox: Mutex::default(), wake: Condvar::new(), latest: AtomicU64::new(0) });
		let worker = Worker { shared: shared.clone(), factory, sink, kept: None, cache: VecDeque::new() };
		let thread = std::thread::Builder::new().name("scratch-frames".into()).spawn(move || worker.run())?;
		Ok(Self { shared, thread: Some(thread) })
	}

	/// A runner on the CPU engine.
	pub fn cpu(sink: ScratchSink) -> std::io::Result<Self> {
		Self::new(Box::new(|scene, options| Box::new(CpuEngine::new(scene, options))), sink)
	}

	/// Asks for timeline frame `frame` of `spec`; returns the request's id. Supersedes any earlier request.
	pub fn request(&self, spec: RunSpec, frame: u32) -> u64 {
		let mut inbox = self.shared.inbox.lock().unwrap();
		let id = self.shared.latest.fetch_add(1, Ordering::AcqRel) + 1;
		inbox.request = Some(Request { id, spec, frame });
		self.shared.wake.notify_one();
		id
	}

	/// Abandons the request being computed, if any, and drops a waiting one.
	pub fn cancel(&self) {
		let mut inbox = self.shared.inbox.lock().unwrap();
		inbox.request = None;
		self.shared.latest.fetch_add(1, Ordering::AcqRel);
	}
}

impl Drop for ScratchRunner {
	fn drop(&mut self) {
		self.shared.inbox.lock().unwrap().shutdown = true;
		self.shared.latest.fetch_add(1, Ordering::AcqRel);
		self.shared.wake.notify_one();
		if let Some(t) = self.thread.take() {
			let _ = t.join();
		}
	}
}

struct Worker {
	shared: Arc<Shared>,
	factory: EngineFactory,
	sink: ScratchSink,
	kept: Option<Kept>,
	/// Recent frames of the kept engine's project, oldest first, within [`CACHE_BYTES`].
	cache: VecDeque<ScratchResult>,
}

/// The kept engine, the spec it was prepared for and how far it has stepped since its reset.
struct Kept {
	engine: Box<dyn WaveEngine>,
	spec: RunSpec,
	steps_done: u32,
	collector: Option<ProbeCollector>,
	readings: ProbeReadings,
}

/// Same scene, emitters and probes: one more step of either is the same computation.
fn same_steps(a: &RunSpec, b: &RunSpec) -> bool {
	a.same_scene_as(b) && a.seed == b.seed && a.emitters == b.emitters && a.probes == b.probes
}

impl Worker {
	fn run(mut self) {
		loop {
			let request = {
				let mut inbox = self.shared.inbox.lock().unwrap();
				loop {
					if inbox.shutdown {
						return;
					}
					if let Some(r) = inbox.request.take() {
						break r;
					}
					inbox = self.shared.wake.wait(inbox).unwrap();
				}
			};
			let id = request.id;
			match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.compute(request))) {
				Ok(Ok(event)) => (self.sink)(event),
				Ok(Err(e)) => {
					tracing::warn!("preview frame failed: {e}");
					self.forget();
					(self.sink)(ScratchEvent::Failed { id, error: Arc::new(e.into()) });
				}
				Err(panic) => {
					self.forget();
					let error = Arc::new(RunError::panic("the engine", panic.as_ref()));
					(self.sink)(ScratchEvent::Failed { id, error });
				}
			}
		}
	}

	/// Drops a failed engine and its frames, so the next request builds anew.
	fn forget(&mut self) {
		self.kept = None;
		self.cache.clear();
	}

	/// The frame of the request: from the cache, else stepped on from the kept engine's state when it has not
	/// passed the frame, else from step 0.
	fn compute(&mut self, Request { id, spec, frame }: Request) -> Result<ScratchEvent, EngineError> {
		let started = Instant::now();
		let steps = frame * spec.visualize_every + 1;
		if !self.kept.as_ref().is_some_and(|k| same_steps(&k.spec, &spec)) {
			self.cache.clear();
		}
		if let Some(hit) = self.cache.iter().find(|r| r.frame == frame && r.snapshot.shape == spec.shape) {
			tracing::debug!(frame, "preview frame from the cache");
			let elapsed = started.elapsed();
			return Ok(ScratchEvent::Done(Box::new(ScratchResult { id, spec, elapsed, ..hit.clone() })));
		}
		self.prepare(&spec, steps);
		let kept = self.kept.as_mut().expect("engine prepared");
		let from = kept.steps_done;
		for i in from..steps {
			if (i - from).is_multiple_of(CHECK_INTERVAL) && self.shared.latest.load(Ordering::Acquire) != id {
				tracing::debug!(frame, done = i, steps, "preview frame abandoned for a newer request");
				return Ok(ScratchEvent::Abandoned { id, frame, steps_done: i, steps });
			}
			kept.engine.advance_one_step()?;
			if let Some(c) = kept.collector.as_mut() {
				kept.engine.sample_probes(&mut kept.readings)?;
				c.record_step(&kept.readings, u64::from(i) + 1);
			}
			kept.steps_done = i + 1;
		}
		let snapshot = kept.engine.capture_frame(&spec.shape)?;
		let traces = kept.collector.as_ref().map_or_else(Vec::new, |c| {
			let mut frame_end = c.clone();
			frame_end.record_frame(&kept.readings, u64::from(steps));
			frame_end.traces
		});
		let elapsed = started.elapsed();
		tracing::debug!(frame, steps, from, ms = elapsed.as_secs_f64() * 1e3, "preview frame");
		let result = ScratchResult { id, spec, frame, snapshot, traces, elapsed };
		self.remember(result.clone());
		Ok(ScratchEvent::Done(Box::new(result)))
	}

	fn remember(&mut self, result: ScratchResult) {
		let bytes = |r: &ScratchResult| r.snapshot.field.len() + r.snapshot.intensity.len();
		let mut total = self.cache.iter().map(bytes).sum::<usize>() + bytes(&result);
		while total > CACHE_BYTES
			&& let Some(old) = self.cache.pop_front()
		{
			total -= bytes(&old);
		}
		if total <= CACHE_BYTES {
			self.cache.push_back(result);
		}
	}

	/// The kept engine ready to step on to `steps` for `spec`: as it stands when it ran the same steps and has
	/// not passed them; else reset, with the scene swapped in for the same grid, or a new one.
	fn prepare(&mut self, spec: &RunSpec, steps: u32) {
		let continues = self.kept.as_ref().is_some_and(|k| same_steps(&k.spec, spec) && k.steps_done <= steps);
		if !continues {
			let engine = self.reset_engine(spec);
			let collector = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene));
			let mut kept =
				Kept { engine, spec: spec.clone(), steps_done: 0, collector, readings: ProbeReadings::default() };
			kept.engine.set_probes(kept.collector.as_ref().map(ProbeCollector::cells).unwrap_or_default());
			self.kept = Some(kept);
		}
		if let Some(kept) = self.kept.as_mut() {
			kept.spec = spec.clone();
		}
	}

	/// The kept engine reset for `spec`: as it is for the same scene, with the scene swapped in for the same
	/// grid, else a new one.
	fn reset_engine(&mut self, spec: &RunSpec) -> Box<dyn WaveEngine> {
		let kept = self.kept.take().and_then(|Kept { mut engine, spec: old, .. }| {
			if old.same_scene_as(spec) {
				if old.emitters != spec.emitters {
					engine.set_emitters(&spec.emitters);
				}
				Some(engine)
			} else if old.same_grid_as(spec) && engine.try_replace_scene(&spec.scene) {
				tracing::debug!("preview engine: scene replaced");
				engine.set_emitters(&spec.emitters);
				Some(engine)
			} else {
				None
			}
		});
		let mut engine = kept.unwrap_or_else(|| {
			let started = Instant::now();
			let mut e = (self.factory)(spec.scene.clone(), spec.options.clone());
			e.set_emitters(&spec.emitters);
			tracing::debug!(ms = started.elapsed().as_secs_f64() * 1e3, "preview engine built");
			e
		});
		engine.reset();
		engine
	}
}
