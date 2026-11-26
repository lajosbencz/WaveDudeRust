//! The run worker's end: a pumped [`RunWorker`] whose stores keep no frame in memory, as the host runs each goal at
//! [`RamCache::Auto`] on no memory; the page's mirror applies the goal's RAM cache size. Their spill ([`PostSpill`])
//! queues each store opened and each frame written; after each pump the host posts the queue and completes a frame's
//! write when the page acknowledges it. Its engines are the CPU's. A page of another protocol version or build is not
//! listened to.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use waveduderust::model::Project;
use waveduderust::run::{
	EngineFuture, Goal, Pump, RamCache, RunError, RunWorker, SpillBackend, SpillFactory, Status, StoreLayout, Token,
	WorkState,
};
use waveduderust::unstable::engine::WaveEngine;
use waveduderust::unstable::engine::cpu::CpuEngine;
use waveduderust::unstable::engine::types::{EngineOptions, SceneData};
use waveduderust::unstable::sync::lock;
use web_time::{Duration, Instant};

use super::protocol::{Failure, Layout, RunStatus, State, ToPage, ToWorker, Trace, VERSION};
use super::{Envelope, Port, decode, post, version_of};

/// The most often progress is posted; a change of state is posted at once.
const STATUS_EVERY: Duration = Duration::from_millis(33);

/// What the run made since the host last posted, in order.
type Queue = VecDeque<Item>;

enum Item {
	Store { id: u64, layout: Layout },
	Frame { store: u64, generation: u64, frame: usize, planes: Vec<u8>, write: Token<()> },
}

/// The worker's stores' spills: each frame to the page.
struct PostSpill(Arc<Mutex<Queue>>);

impl SpillFactory for PostSpill {
	fn poll_ready(&self, _: &mut Context<'_>) -> Poll<()> {
		Poll::Ready(())
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
	/// The trace readings posted, (series, profiles) per probe, and the traces' start they belong to.
	traced: Vec<(usize, usize)>,
	traced_for: Option<u64>,
	/// The page speaks another protocol version or is another build: its messages are dropped.
	deaf: bool,
}

impl<P: Port> Host<P> {
	/// A host posting to `port`, its engines within the device's `memory` bytes (the address space alone where it is
	/// unknown); posts [`ToPage::Ready`].
	pub fn new(port: P, memory: Option<u64>) -> Self {
		let queue = Arc::<Mutex<Queue>>::default();
		let worker = RunWorker::pumped(
			Box::new(engine),
			memory.unwrap_or(u64::MAX),
			0,
			Box::new(PostSpill(queue.clone())),
			Arc::default(),
		);
		post(&port, &ToPage::Ready { version: VERSION, build: crate::build_id() }, None);
		Self {
			port,
			worker,
			image: None,
			project: None,
			queue,
			unacked: HashMap::new(),
			posted: None,
			failed: None,
			failures: 0,
			traced: Vec::new(),
			traced_for: None,
			deaf: false,
		}
	}

	/// Handles the messages waiting, works on the goal for about `budget` and posts what the run made; what is
	/// left, `waker` woken once what it waits for is ready ([`RunWorker::pump`]). An acknowledgement arrives as a
	/// message.
	pub fn pump(&mut self, budget: Duration, waker: &Waker) -> Pump {
		while let Some(envelope) = self.port.receive() {
			self.handle(envelope);
		}
		let left = self.worker.pump(budget, waker);
		self.post_made(left != Pump::Idle);
		left
	}

	fn handle(&mut self, envelope: Envelope) {
		if let Some(version) = version_of(&envelope).filter(|&v| v != VERSION) {
			tracing::error!("the page speaks protocol version {version}, this worker {VERSION}: it is not listened to");
			self.deaf = true;
		}
		if self.deaf {
			return;
		}
		let message: ToWorker = decode(&envelope).expect("the page posts messages of the protocol");
		match message {
			ToWorker::Init { version: _, build } => {
				let worker = crate::build_id();
				if build != worker {
					tracing::error!("the page is build {build}, this worker {worker}: it is not listened to");
					self.deaf = true;
				}
			}
			ToWorker::Image { key } => {
				let bytes = envelope.buffer.expect("an image comes with its bytes");
				self.image = Some((key, bytes.to_vec(0..bytes.len()).into()));
			}
			ToWorker::Goal { mut goal, image } => {
				self.restore(&mut goal, image);
				goal.ram_cache = RamCache::Auto;
				self.worker.set_goal(goal);
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
			self.post_traces();
			post(&self.port, &ToPage::Status(status.clone()), None);
			self.posted = Some((status, Instant::now()));
		}
	}

	/// Posts the stores opened and their frames. A store opened replaces the one before, whose
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

	/// Posts the readings since the last; everything, marked a restart, when the traces started over.
	fn post_traces(&mut self) {
		let (traced, traced_for) = (&mut self.traced, &mut self.traced_for);
		let (restart, tails): (bool, Vec<Trace>) = self.worker.with_traces_started(|start, traces| {
			let restart = *traced_for != Some(start);
			*traced_for = Some(start);
			if restart {
				*traced = vec![(0, 0); traces.len()];
			}
			debug_assert_eq!(traced.len(), traces.len(), "the probes change as the traces start over");
			let tails = traces
				.iter()
				.zip(traced.iter_mut())
				.map(|(t, (series, profiles))| {
					let tail = Trace::tail(t, *series, *profiles);
					(*series, *profiles) = (t.series.len(), t.profiles.len());
					tail
				})
				.collect();
			(restart, tails)
		});
		if restart || !tails.is_empty() {
			post(&self.port, &ToPage::Traces { restart, tails }, None);
		}
	}
}

/// A CPU engine of `scene`, its grid logged.
fn engine(scene: Arc<SceneData>, options: EngineOptions) -> EngineFuture {
	let engine = CpuEngine::new(scene, options);
	let (scene, options) = (engine.scene(), engine.options());
	tracing::info!(
		"{} engine built: grid {}x{}, canvas {}x{}, rim {}, edges {:?}, stride {}, {} sub-steps, {:?}, intensity {}",
		engine.name(),
		scene.grid_width(),
		scene.grid_height(),
		scene.canvas_width,
		scene.canvas_height,
		scene.rim,
		scene.edges,
		scene.stride(),
		engine.sub_steps(),
		options.stencil,
		if options.track_intensity { "tracked" } else { "off" }
	);
	Box::pin(std::future::ready(Box::new(engine) as Box<dyn WaveEngine>))
}
