//! The page's end: a live-run [`Backend`] whose frames land in a mirror of the worker's store, which holds them
//! in memory up to the budget and spills the rest through the page's [`SpillFactory`]. A frame is acknowledged
//! once the mirror holds it: at once in memory, when its spill write completed otherwise; a spilled frame of a
//! store or generation the worker left is acknowledged at once.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use wavedude_core::engine::Snapshot;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::run::{
	Budget, FitFrame, FrameStore, Goal, RunError, SpillBackend, SpillFactory, Status, StoreLayout, Token, WorkState,
};
use wavedude_core::scene::compose::image_key;
use wavedude_core::sync::lock;

use super::protocol::{Layout, RunStatus, State, ToPage, ToWorker, Trace};
use super::{Port, decode, post};
use crate::live::{Backend, engine_label};

/// (store, generation, frame).
type FrameKey = (u64, u64, usize);

/// A spill write of the mirror: the page's backend's and the one the mirror waits on, completed together.
struct Watch {
	/// The mirror's frame written.
	frame: FrameKey,
	write: Token<()>,
	mirror: Token<()>,
	/// The worker's frame it acknowledges; none once released.
	ack: Option<FrameKey>,
}

type Watches = Arc<Mutex<Vec<Watch>>>;

/// The page's spills, their writes watched.
struct Watching {
	spill: Box<dyn SpillFactory>,
	writes: Watches,
}

impl SpillFactory for Watching {
	fn ready(&self) -> bool {
		self.spill.ready()
	}

	fn open(&self, store: u64, layout: &StoreLayout, bytes: u64) -> io::Result<Box<dyn SpillBackend>> {
		Ok(Box::new(Watched { store, spill: self.spill.open(store, layout, bytes)?, writes: self.writes.clone() }))
	}
}

/// The spill of the mirror store `store`.
struct Watched {
	store: u64,
	spill: Box<dyn SpillBackend>,
	writes: Watches,
}

impl SpillBackend for Watched {
	fn write(&self, generation: u64, frame: usize, planes: &[&[u8]]) -> Token<()> {
		let mirror = Token::default();
		let write = self.spill.write(generation, frame, planes);
		let watch = Watch { frame: (self.store, generation, frame), write, mirror: mirror.clone(), ack: None };
		lock(&self.writes).push(watch);
		mirror
	}

	fn read(&self, generation: u64, frame: usize) -> Token<Vec<u8>> {
		self.spill.read(generation, frame)
	}

	fn drop_generation(&self, generation: u64) {
		self.spill.drop_generation(generation);
	}
}

/// The page's copy of the worker's store `id`, at the worker's generation `generation`; why there is none when
/// its frames do not fit.
struct Mirror {
	id: u64,
	generation: u64,
	store: Result<Arc<FrameStore>, String>,
}

/// The live run in the browser's run worker, behind a [`Port`].
pub struct Remote<P: Port> {
	port: P,
	use_gpu: Arc<AtomicBool>,
	/// The memory the mirror's frames share by the RAM cache size.
	frame_memory: u64,
	spill: Watching,
	/// The worker's GPU adapter's name, or why it has none.
	gpu: Option<Result<String, String>>,
	/// The label of the engine built last and why the CPU's runs in place of the GPU's.
	engine: Option<(String, Option<String>)>,
	/// The key of the scene image posted last.
	image: Option<u64>,
	ram_percent: u32,
	mirror: Option<Mirror>,
	status: RunStatus,
	/// The worker's failure in force, by its number.
	failure: Option<(u64, Arc<RunError>)>,
	crashed: Option<Arc<RunError>>,
	traces: Vec<ProbeTrace>,
	held: Vec<ProbeTrace>,
	fit: Option<FitFrame>,
	/// A store opened before the spill could answer, and what came after it.
	waiting: Option<(ToPage, Option<Vec<u8>>)>,
}

impl<P: Port> Remote<P> {
	/// A run behind `port` on the GPU while `use_gpu` holds (shared with the settings), its frames in
	/// `frame_memory` bytes by the RAM cache size and the rest in a spill of `spill`; posts [`ToWorker::Init`].
	pub fn new(port: P, use_gpu: Arc<AtomicBool>, frame_memory: u64, spill: Box<dyn SpillFactory>) -> Self {
		post(&port, &ToWorker::Init { use_gpu: use_gpu.load(Ordering::Relaxed) }, None);
		Self {
			port,
			use_gpu,
			frame_memory,
			spill: Watching { spill, writes: Watches::default() },
			gpu: None,
			engine: None,
			image: None,
			ram_percent: 0,
			mirror: None,
			status: RunStatus::default(),
			failure: None,
			crashed: None,
			traces: Vec::new(),
			held: Vec::new(),
			fit: None,
			waiting: None,
		}
	}

	/// Why the engine built last is the CPU's where the GPU's was asked for.
	pub fn fallback(&self) -> Option<&str> {
		self.engine.as_ref().and_then(|(_, why)| why.as_deref())
	}

	/// Handles the messages waiting; a store's opening waits for the spill to answer.
	fn receive(&mut self) {
		while self.crashed.is_none() {
			let (message, buffer) = match self.waiting.take() {
				Some(waiting) => waiting,
				None => {
					let Some(envelope) = self.port.receive() else { return };
					match decode(&envelope) {
						Ok(message) => (message, envelope.buffer),
						Err(e) => return self.crash(format!("an unreadable message: {e}")),
					}
				}
			};
			if matches!(message, ToPage::StoreOpened { .. }) && !self.spill.ready() {
				self.waiting = Some((message, buffer));
				return;
			}
			self.handle(message, buffer);
		}
	}

	fn handle(&mut self, message: ToPage, buffer: Option<Vec<u8>>) {
		match message {
			ToPage::Ready { gpu } => {
				match &gpu {
					Ok(name) => tracing::info!("the run worker started, {}", engine_label(Some(name))),
					Err(why) => tracing::info!("the run worker started without a GPU: {why}"),
				}
				self.gpu = Some(gpu);
			}
			ToPage::EngineBuilt { name, fallback } => {
				if let Some(why) = &fallback {
					tracing::warn!("the GPU engine is not available ({why}); the CPU engine runs instead");
				}
				self.engine = Some((name, fallback));
			}
			ToPage::StoreOpened { id, layout } => self.open(id, layout),
			ToPage::Frame { store, generation, frame } => {
				let Some(planes) = buffer else { return self.crash(format!("frame {frame} came without planes")) };
				self.land(store, generation, frame, planes);
			}
			ToPage::Status(status) => {
				if let State::Failed { id, failure } = &status.state
					&& self.failure.as_ref().is_none_or(|(known, _)| known != id)
				{
					self.failure = Some((*id, Arc::new(failure.clone().into())));
				}
				if let Some(m) = self.mirror.as_mut().filter(|m| status.store == Some(m.id)) {
					m.follow(status.generation);
				}
				self.status = status;
			}
			ToPage::Traces { held, tails } => {
				if let Some(held) = held {
					self.held = held.into_iter().map(trace).collect();
					self.traces = vec![ProbeTrace::default(); tails.len()];
				}
				for (tail, t) in tails.into_iter().zip(&mut self.traces) {
					tail.append_to(t);
				}
			}
			ToPage::Fit { id, frame, step, shape } => {
				let Some(mut field) = buffer else {
					return self.crash(format!("fit frame {frame} came without planes"));
				};
				let intensity = field.split_off(shape.plane_bytes().min(field.len()));
				let snapshot = Arc::new(Snapshot { shape, step, field, intensity });
				self.fit = Some(FitFrame { id, frame, snapshot });
			}
			ToPage::Crashed(why) => self.crash(why),
		}
	}

	/// The mirror of the worker's store `id`: the frames that fit the budget in memory, the rest spilled.
	fn open(&mut self, id: u64, layout: Layout) {
		let layout = StoreLayout::from(layout);
		let budget = Budget::of_memory(self.frame_memory, self.ram_percent);
		let store = FrameStore::open(layout, budget, &self.spill).map(Arc::new).map_err(|e| {
			tracing::debug!("the frames of the run do not fit: {e}");
			e.to_string()
		});
		self.mirror = Some(Mirror { id, generation: 0, store });
	}

	/// Writes a frame of the worker's into the mirror and acknowledges it, once its spill write completed when
	/// it is spilled. A frame of a store or generation gone is acknowledged unwritten.
	fn land(&mut self, store: u64, generation: u64, frame: usize, planes: Vec<u8>) {
		let key = (store, generation, frame);
		let written = match self.mirror.as_mut().filter(|m| m.id == store) {
			Some(m) if generation >= m.generation => {
				m.follow(generation);
				m.write(frame, planes).map(Some)
			}
			_ => Ok(None),
		};
		if let Ok(Some(written)) = written
			&& let Some(watch) = lock(&self.spill.writes).iter_mut().find(|w| w.frame == written)
		{
			watch.ack = Some(key);
			return;
		}
		self.ack(key, written.map(|_| ()));
	}

	/// Acknowledges the spill writes in flight of the worker's stores and generations gone: the worker waits for
	/// them no longer than the mirror does.
	fn release(&mut self) {
		let current = self.mirror.as_ref().map(|m| (m.id, m.generation));
		let gone: Vec<FrameKey> = lock(&self.spill.writes)
			.iter_mut()
			.filter(|w| w.ack.is_some_and(|(store, generation, _)| current.is_none_or(|c| c != (store, generation))))
			.filter_map(|w| w.ack.take())
			.collect();
		for key in gone {
			self.ack(key, Ok(()));
		}
	}

	/// Acknowledges the spilled frames whose writes completed and frees their memory.
	fn settle(&mut self) {
		let done: Vec<Watch> = lock(&self.spill.writes).extract_if(.., |w| w.write.is_done()).collect();
		for watch in done {
			let result = watch.write.take().expect("a write done has its outcome");
			let ack = result.as_ref().copied().map_err(ToString::to_string);
			watch.mirror.complete(result);
			if let Some(key) = watch.ack {
				self.ack(key, ack);
			}
		}
		if let Some(Ok(store)) = self.mirror.as_ref().map(|m| &m.store)
			&& let Err(e) = store.settle()
		{
			tracing::debug!("a frame of the mirror was not spilled: {e}");
		}
	}

	fn ack(&self, (store, generation, frame): FrameKey, result: Result<(), String>) {
		post(&self.port, &ToWorker::Ack { store, generation, frame, result }, None);
	}

	/// The run is over for good: the worker is gone.
	fn crash(&mut self, why: String) {
		tracing::error!("the run worker failed: {why}");
		self.crashed = Some(Arc::new(RunError::Worker(why)));
	}
}

impl Mirror {
	/// The worker's store moved to `generation`: the frames before go.
	fn follow(&mut self, generation: u64) {
		if generation > self.generation {
			if let Ok(store) = &self.store {
				store.clear();
			}
			self.generation = generation;
		}
	}

	/// Stores `planes`, the frame's in layer order, as frame `frame`; the mirror's frame written.
	fn write(&self, frame: usize, planes: Vec<u8>) -> Result<FrameKey, String> {
		let store = self.store.as_ref()?;
		let mut field = planes;
		let intensity = field.split_off(store.shape().plane_bytes().min(field.len()));
		let snapshot = Snapshot { shape: *store.shape(), step: store.step_of(frame) as u32, field, intensity };
		store.write_at(frame, snapshot).map_err(|e| e.to_string())?;
		Ok((store.id(), store.generation(), frame))
	}
}

fn trace(t: Trace) -> ProbeTrace {
	let mut out = ProbeTrace::default();
	t.append_to(&mut out);
	out
}

impl<P: Port> Backend for Remote<P> {
	fn set_goal(&mut self, goal: Goal) {
		self.ram_percent = goal.ram_percent;
		let image = goal.run.as_ref().and_then(|r| image_key(&r.project));
		if let Some(key) = image.filter(|&k| self.image != Some(k)) {
			let project = &goal.run.as_ref().expect("an image key comes from a run").project;
			let bytes = project.scene.image_data.as_deref().expect("an image key has its bytes");
			post(&self.port, &ToWorker::Image { key }, Some(bytes.to_vec()));
			self.image = Some(key);
		}
		post(&self.port, &ToWorker::Goal { goal, image }, None);
	}

	fn status(&self) -> Status {
		let s = &self.status;
		let state = match (&self.crashed, &s.state) {
			(Some(e), _) => WorkState::Failed(e.clone()),
			(None, State::Idle) => WorkState::Idle,
			(None, State::Stepping { target }) => WorkState::Stepping { target: *target },
			(None, State::Fitting) => WorkState::Fitting,
			(None, State::Paused) => WorkState::Paused,
			(None, State::Failed { .. }) => {
				WorkState::Failed(self.failure.as_ref().expect("a failure is recorded as it arrives").1.clone())
			}
		};
		let store = self.mirror.as_ref().filter(|m| s.store == Some(m.id)).and_then(|m| m.store.clone().ok());
		Status {
			adopted: s.adopted,
			state,
			steps_done: s.steps_done,
			traces_steps: s.traces_steps,
			store,
			device_frames: None,
		}
	}

	fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		f(&self.traces);
	}

	fn with_held_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		f(&self.held);
	}

	fn take_fit(&mut self) -> Option<FitFrame> {
		self.fit.take()
	}

	fn engine_changed(&mut self) {
		post(&self.port, &ToWorker::Rebuild { use_gpu: self.use_gpu.load(Ordering::Relaxed) }, None);
	}

	fn discard_frames(&mut self) {
		post(&self.port, &ToWorker::Discard, None);
	}

	fn budgets(&self, ram_percent: u32, _reclaimed: u64) -> (u64, u64) {
		(Budget::of_memory(self.frame_memory, ram_percent), Budget::of_memory(self.frame_memory, 0))
	}

	/// The engine built last; before the first, the one asked for.
	fn engine_name(&self) -> String {
		if let Some((name, _)) = &self.engine {
			return name.clone();
		}
		let gpu = self.gpu.as_ref().and_then(|g| g.as_ref().ok()).filter(|_| self.use_gpu.load(Ordering::Relaxed));
		engine_label(gpu.map(String::as_str))
	}

	/// Handles the messages waiting and the mirror's spill writes done.
	fn pump(&mut self) {
		self.receive();
		self.release();
		self.settle();
	}
}
