//! Scratch frames: "frame k of the project as it is now" for the live preview (spec/30-engine.md, Scratch frames).
//!
//! Threads: one worker thread owns the kept engine and computes the newest request; [`ScratchRunner::request`]
//! may be called from any thread. A request superseded while stepping is abandoned at the next check, every
//! [`CHECK_INTERVAL`] steps. Events are delivered on the worker thread.

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
		let worker = Worker { shared: shared.clone(), factory, sink, engine: None };
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
	engine: Option<(Box<dyn WaveEngine>, RunSpec)>,
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
					self.engine = None;
					(self.sink)(ScratchEvent::Failed { id, error: Arc::new(e.into()) });
				}
				Err(panic) => {
					self.engine = None;
					let error = Arc::new(RunError::panic("the engine", panic.as_ref()));
					(self.sink)(ScratchEvent::Failed { id, error });
				}
			}
		}
	}

	/// The frame of the request; a failed engine is dropped by the caller, so the next request builds anew.
	fn compute(&mut self, Request { id, spec, frame }: Request) -> Result<ScratchEvent, EngineError> {
		let started = Instant::now();
		let steps = frame * spec.visualize_every + 1;
		self.prepare_engine(&spec);
		let engine = &mut self.engine.as_mut().expect("engine prepared").0;
		let mut collector = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene));
		engine.set_probes(collector.as_ref().map(ProbeCollector::cells).unwrap_or_default());
		let mut readings = ProbeReadings::default();
		for i in 0..steps {
			if i % CHECK_INTERVAL == 0 && self.shared.latest.load(Ordering::Acquire) != id {
				tracing::debug!(frame, done = i, steps, "preview frame abandoned for a newer request");
				return Ok(ScratchEvent::Abandoned { id, frame, steps_done: i, steps });
			}
			engine.advance_one_step()?;
			if let Some(c) = collector.as_mut() {
				engine.sample_probes(&mut readings)?;
				c.record_step(&readings, i as u64 + 1);
			}
		}
		let snapshot = engine.capture_frame(&spec.shape)?;
		if let Some(c) = collector.as_mut() {
			c.record_frame(&readings, steps as u64);
		}
		let traces = collector.map(|c| c.traces).unwrap_or_default();
		let elapsed = started.elapsed();
		tracing::debug!(frame, steps, ms = elapsed.as_secs_f64() * 1e3, "preview frame");
		Ok(ScratchEvent::Done(Box::new(ScratchResult { id, spec, frame, snapshot, traces, elapsed })))
	}

	/// Keeps the engine reset for `spec`: as it is for the same scene, with the scene swapped in for the
	/// same grid, else a new one.
	fn prepare_engine(&mut self, spec: &RunSpec) {
		let kept = self.engine.take().and_then(|(mut engine, old)| {
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
		self.engine = Some((engine, spec.clone()));
	}
}
