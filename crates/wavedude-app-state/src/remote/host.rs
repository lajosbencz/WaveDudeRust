//! The run worker's end: a pumped [`RunWorker`] whose stores keep no frame in memory. Their spill ([`PostSpill`])
//! queues each store opened and each frame written; after each pump the host posts the queue and completes a
//! frame's write when the page acknowledges it.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use wavedude_core::engine::{Built, GpuHandle, build_on};
use wavedude_core::model::Project;
use wavedude_core::run::{
	EngineFuture, Goal, PumpedFactory, RunError, RunWorker, SpillBackend, SpillFactory, Status, StoreLayout, Token,
	WorkState,
};
use wavedude_core::sync::lock;

use super::protocol::{Failure, Layout, RunStatus, State, ToPage, ToWorker, Trace};
use super::{Envelope, Port, decode, post};
use crate::live::engine_label;
use crate::platform::{Duration, Instant};

/// The most often progress is posted; a change of state is posted at once.
const STATUS_EVERY: Duration = Duration::from_millis(33);

/// What the run made since the host last posted, in order.
type Queue = VecDeque<Item>;

enum Item {
	Store { id: u64, layout: Layout },
	Frame { store: u64, generation: u64, frame: usize, planes: Vec<u8>, write: Token<()> },
	Engine { name: String, fallback: Option<String> },
}

/// The worker's stores' spills: each frame to the page.
struct PostSpill(Arc<Mutex<Queue>>);

impl SpillFactory for PostSpill {
	fn ready(&self) -> bool {
		true
	}

	/// Never refused: the page's mirror holds the frames, and refuses them when they do not fit.
	fn open(&self, store: u64, layout: &StoreLayout, _bytes: u64) -> io::Result<Box<dyn SpillBackend>> {
		lock(&self.0).push_back(Item::Store { id: store, layout: Layout::from(layout) });
		Ok(Box::new(Posted { queue: self.0.clone(), store }))
	}
}

/// The spill of store `store`.
struct Posted {
	queue: Arc<Mutex<Queue>>,
	store: u64,
}

impl SpillBackend for Posted {
	fn write(&self, generation: u64, frame: usize, planes: &[&[u8]]) -> Token<()> {
		let write = Token::default();
		let item = Item::Frame { store: self.store, generation, frame, planes: planes.concat(), write: write.clone() };
		lock(&self.queue).push_back(item);
		write
	}

	/// Refused: the frames are on the page. The run reads none: the frame written last stays in memory.
	fn read(&self, _generation: u64, frame: usize) -> Token<Vec<u8>> {
		let read = Token::default();
		read.complete(Err(io::Error::new(io::ErrorKind::Unsupported, format!("frame {frame} is on the page"))));
		read
	}

	/// The page clears its mirror at the generation's end.
	fn drop_generation(&self, _generation: u64) {}
}

/// The worker's end of a live run.
pub struct Host<P: Port> {
	port: P,
	worker: RunWorker,
	use_gpu: Arc<AtomicBool>,
	/// The scene image the page posted last: (key, bytes).
	image: Option<(u64, Arc<[u8]>)>,
	/// The goal's project before, kept for an equal one: the run builds a spec per project pointer.
	project: Option<Arc<Project>>,
	queue: Arc<Mutex<Queue>>,
	/// Frames posted and not acknowledged, by (store, generation, frame).
	unacked: HashMap<(u64, u64, usize), Token<()>>,
	posted: Option<(RunStatus, Instant)>,
	/// The run's failure last seen and how many there were.
	failed: Option<Arc<RunError>>,
	failures: u64,
	/// The trace readings posted, (series, profiles) per probe, and what they belong to:
	/// (revision adopted, store, generation, failures).
	traced: Vec<(usize, usize)>,
	traced_for: Option<(u64, Option<u64>, u64, u64)>,
}

impl<P: Port> Host<P> {
	/// A host posting to `port`, its GPU engines on `gpu` (or why there is none); posts
	/// [`ToPage::Ready`].
	pub fn new(port: P, gpu: Result<GpuHandle, String>) -> Self {
		let queue = Arc::<Mutex<Queue>>::default();
		let use_gpu = Arc::new(AtomicBool::new(false));
		let ready = gpu.as_ref().map(|ctx| ctx.info.name.clone()).map_err(Clone::clone);
		let factory = engines(gpu, use_gpu.clone(), queue.clone());
		let worker = RunWorker::pumped(factory, 0, Box::new(PostSpill(queue.clone())), Arc::default());
		post(&port, &ToPage::Ready { gpu: ready }, None);
		Self {
			port,
			worker,
			use_gpu,
			image: None,
			project: None,
			queue,
			unacked: HashMap::new(),
			posted: None,
			failed: None,
			failures: 0,
			traced: Vec::new(),
			traced_for: None,
		}
	}

	/// Handles the messages waiting, works on the goal for about `budget` and posts what the run made; whether
	/// work is left ([`RunWorker::pump`]).
	pub fn pump(&mut self, budget: Duration) -> bool {
		while let Some(envelope) = self.port.receive() {
			self.handle(envelope);
		}
		let more = self.worker.pump(budget);
		self.post_made(more);
		more
	}

	/// Frames written and not acknowledged by the page, posted or not.
	pub fn frames_in_flight(&self) -> usize {
		let queued = lock(&self.queue).iter().filter(|i| matches!(i, Item::Frame { .. })).count();
		self.unacked.len() + queued
	}

	fn handle(&mut self, envelope: Envelope) {
		let message: ToWorker = decode(&envelope).expect("the page posts messages of the protocol");
		match message {
			ToWorker::Init { use_gpu } => self.use_gpu.store(use_gpu, Ordering::Relaxed),
			ToWorker::Image { key } => {
				self.image = Some((key, envelope.buffer.expect("an image comes with its bytes").into()));
			}
			ToWorker::Goal { mut goal, image } => {
				self.restore(&mut goal, image);
				self.worker.set_goal(goal);
			}
			ToWorker::Rebuild { use_gpu } => {
				self.use_gpu.store(use_gpu, Ordering::Relaxed);
				self.worker.rebuild_engine();
			}
			ToWorker::Discard => self.worker.discard_frames(),
			// A frame of a store gone is acknowledged after the host forgot it.
			ToWorker::Ack { store, generation, frame, result } => {
				if let Some(write) = self.unacked.remove(&(store, generation, frame)) {
					write.complete(result.map_err(io::Error::other));
				}
			}
		}
	}

	/// Puts the image `image` back into the goal's project and keeps the project before when equal.
	fn restore(&mut self, goal: &mut Goal, image: Option<u64>) {
		let Some(run) = goal.run.as_mut() else { return };
		let project = Arc::get_mut(&mut run.project).expect("a decoded project is not shared");
		if let Some(key) = image {
			let (_, bytes) =
				self.image.as_ref().filter(|(k, _)| *k == key).expect("the page posts an image before its goals");
			project.scene.image_data = Some(bytes.clone());
		}
		match &self.project {
			Some(before) if **before == *project => run.project = before.clone(),
			_ => self.project = Some(run.project.clone()),
		}
	}

	/// Posts the queue, the fit frame and, when due, the traces and status:
	/// at a change of state at once, else at most every [`STATUS_EVERY`] while `more` work is left.
	fn post_made(&mut self, more: bool) {
		self.post_queue();
		let status = self.worker.status();
		if let Some(fit) = self.worker.take_fit() {
			let s = &fit.snapshot;
			let message = ToPage::Fit { id: fit.id, frame: fit.frame, step: s.step, shape: s.shape };
			post(&self.port, &message, Some([&s.field[..], &s.intensity[..]].concat()));
		}
		let status = self.run_status(&status);
		let urgent = self.posted.as_ref().is_none_or(|(before, _)| {
			let progress = matches!((&before.state, &status.state), (State::Stepping { .. }, State::Stepping { .. }));
			(before.adopted, before.store, before.generation) != (status.adopted, status.store, status.generation)
				|| (!progress && before.state != status.state)
		});
		let due = self
			.posted
			.as_ref()
			.is_some_and(|(before, at)| *before != status && (!more || at.elapsed() >= STATUS_EVERY));
		if urgent || due {
			self.post_traces(&status);
			post(&self.port, &ToPage::Status(status.clone()), None);
			self.posted = Some((status, Instant::now()));
		}
	}

	/// Posts the stores opened, their frames and the engines built. A store opened replaces the one before, whose
	/// frames are no longer waited for.
	fn post_queue(&mut self) {
		let items = std::mem::take(&mut *lock(&self.queue));
		for item in items {
			match item {
				Item::Store { id, layout } => {
					self.unacked.retain(|(store, ..), _| *store == id);
					post(&self.port, &ToPage::StoreOpened { id, layout }, None);
				}
				Item::Frame { store, generation, frame, planes, write } => {
					post(&self.port, &ToPage::Frame { store, generation, frame }, Some(planes));
					self.unacked.insert((store, generation, frame), write);
				}
				Item::Engine { name, fallback } => post(&self.port, &ToPage::EngineBuilt { name, fallback }, None),
			}
		}
	}

	fn run_status(&mut self, status: &Status) -> RunStatus {
		let state = match &status.state {
			WorkState::Idle => State::Idle,
			WorkState::Stepping { target } => State::Stepping { target: *target },
			WorkState::Fitting => State::Fitting,
			WorkState::Paused => State::Paused,
			WorkState::Failed(e) => {
				if self.failed.as_ref().is_none_or(|f| !Arc::ptr_eq(f, e)) {
					self.failed = Some(e.clone());
					self.failures += 1;
				}
				State::Failed { id: self.failures, failure: Failure::from(&**e) }
			}
		};
		RunStatus {
			adopted: status.adopted,
			state,
			steps_done: status.steps_done,
			traces_steps: status.traces_steps,
			store: status.store.as_ref().map(|s| s.id()),
			generation: status.store.as_ref().map_or(0, |s| s.generation()),
		}
	}

	/// Posts the readings since the last; everything, with the held traces, when the traces may have started
	/// over: a new revision, store, generation or failure.
	fn post_traces(&mut self, status: &RunStatus) {
		let owner = (status.adopted, status.store, status.generation, self.failures);
		let restart = self.traced_for != Some(owner);
		self.traced_for = Some(owner);
		let traced = &mut self.traced;
		let tails: Vec<Trace> = self.worker.with_traces(|traces| {
			if restart {
				*traced = vec![(0, 0); traces.len()];
			}
			debug_assert_eq!(traced.len(), traces.len(), "the probes change with the revision");
			traces
				.iter()
				.zip(traced.iter_mut())
				.map(|(t, (series, profiles))| {
					let tail = Trace::tail(t, *series, *profiles);
					(*series, *profiles) = (t.series.len(), t.profiles.len());
					tail
				})
				.collect()
		});
		let held = restart.then(|| self.worker.with_held_traces(|h| h.iter().map(|t| Trace::tail(t, 0, 0)).collect()));
		if held.is_some() || !tails.is_empty() {
			post(&self.port, &ToPage::Traces { held, tails }, None);
		}
	}
}

/// Engines on `gpu` while `use_gpu` holds, else the CPU's; each one built is queued for the page.
fn engines(gpu: Result<GpuHandle, String>, use_gpu: Arc<AtomicBool>, queue: Arc<Mutex<Queue>>) -> PumpedFactory {
	Box::new(move |scene, options| -> EngineFuture {
		let (gpu, use_gpu, queue) = (gpu.clone(), use_gpu.load(Ordering::Relaxed), queue.clone());
		Box::pin(async move {
			let (device, refused) = match gpu {
				Ok(ctx) if use_gpu => (Some(ctx), None),
				Err(why) if use_gpu => {
					tracing::warn!("the GPU engine is not available ({why}); the CPU engine runs instead");
					(None, Some(why))
				}
				_ => (None, None),
			};
			let name = device.as_ref().map(|ctx| ctx.info.name.clone());
			let Built { engine, fallback } = build_on(scene, options, device).await;
			crate::log::engine_built(engine.name(), engine.scene(), engine.options(), engine.sub_steps());
			let item = Item::Engine {
				name: engine_label(name.filter(|_| engine.is_gpu()).as_deref()),
				fallback: fallback.map(|f| f.to_string()).or(refused),
			};
			lock(&queue).push_back(item);
			engine
		})
	})
}
