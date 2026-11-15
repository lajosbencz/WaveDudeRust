//! The page's end: a live-run [`Backend`] whose frames land in a mirror of the worker's store, which holds them in
//! memory up to the budget and spills the rest through the page's [`SpillFactory`]. A frame is acknowledged once the
//! mirror holds it: at once in memory, when its spill write completed otherwise, and at once for a spilled frame of a
//! store or generation the worker left. A frame the page's memory refuses lets go of the mirror, whose frames fail
//! from then on. A worker lost, or of another protocol version or build, fails every run until the page is reloaded.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use wavedude_core::engine::Snapshot;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::run::{
	Budget, FitFrame, FrameStore, Goal, RamCache, RunError, SpillBackend, SpillFactory, Status, StoreError,
	StoreLayout, Token, WorkState,
};
use wavedude_core::scene::compose::image_key;
use wavedude_core::sync::lock;

use super::protocol::{Layout, Lost, RunStatus, State, ToPage, ToWorker, Trace, VERSION};
use super::{Buffer, Port, WorkerGone, decode, post, version_of};
use crate::live::{Backend, EngineLabel};

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
	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
		self.spill.poll_ready(cx)
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
	store: Result<Arc<FrameStore>, NoStore>,
}

/// Why a mirror holds no store.
#[derive(Clone, Debug, thiserror::Error)]
enum NoStore {
	#[error("{0}")]
	NotOpened(String),
	/// The page's memory refused a frame.
	#[error("{0}")]
	NoMemory(String),
}

/// Where the page is with its worker.
enum Worker {
	/// It has not said [`ToPage::Ready`] yet.
	Starting,
	/// Ready: its GPU adapter's name, or why it has none.
	Ready(Result<String, String>),
	/// Gone for good: why, and the failure every run reads.
	Gone(WorkerGone, Arc<RunError>),
}

/// The live run in the browser's run worker, behind a [`Port`].
pub struct Remote<P: Port> {
	port: P,
	use_gpu: Arc<AtomicBool>,
	/// The memory the mirror's frames take at Auto, the RAM cache size's share of it or a size in its place.
	frame_memory: u64,
	spill: Watching,
	worker: Worker,
	/// The label of the engine built last and why the CPU's runs in place of the GPU's.
	engine: Option<(EngineLabel, Option<String>)>,
	/// The key of the scene image posted last.
	image: Option<u64>,
	ram_cache: RamCache,
	mirror: Option<Mirror>,
	status: RunStatus,
	/// The worker's failure in force, by its number.
	failure: Option<(u64, Arc<RunError>)>,
	traces: Vec<ProbeTrace>,
	held: Vec<ProbeTrace>,
	fit: Option<FitFrame>,
	/// A store opened before the spill could answer, and what came after it.
	waiting: Option<(ToPage, Option<Buffer>)>,
}

impl<P: Port> Remote<P> {
	/// A run behind `port` on the GPU while `use_gpu` holds (shared with the settings), its frames in memory by the
	/// RAM cache size, `frame_memory` bytes at Auto, and the rest in a spill of `spill`; posts [`ToWorker::Init`].
	pub fn new(port: P, use_gpu: Arc<AtomicBool>, frame_memory: u64, spill: Box<dyn SpillFactory>) -> Self {
		let init =
			ToWorker::Init { version: VERSION, build: crate::build_id(), use_gpu: use_gpu.load(Ordering::Relaxed) };
		post(&port, &init, None);
		Self {
			port,
			use_gpu,
			frame_memory,
			spill: Watching { spill, writes: Watches::default() },
			worker: Worker::Starting,
			engine: None,
			image: None,
			ram_cache: RamCache::Auto,
			mirror: None,
			status: RunStatus::default(),
			failure: None,
			traces: Vec::new(),
			held: Vec::new(),
			fit: None,
			waiting: None,
		}
	}

	/// Why the worker runs nothing until the page is reloaded.
	pub fn worker_gone(&self) -> Option<WorkerGone> {
		match &self.worker {
			Worker::Gone(gone, _) => Some(gone.clone()),
			_ => None,
		}
	}

	/// Why the page's memory refused a frame of the mirror's store.
	pub fn memory_refused(&self) -> Option<String> {
		match self.mirror.as_ref().map(|m| &m.store) {
			Some(Err(NoStore::NoMemory(why))) => Some(why.clone()),
			_ => None,
		}
	}

	/// Posts to the worker, unless it is gone.
	fn send(&self, message: &ToWorker, buffer: Option<Vec<u8>>) {
		if !matches!(self.worker, Worker::Gone(..)) {
			post(&self.port, message, buffer);
		}
	}

	/// Handles the messages waiting; a store's opening waits for the spill to answer.
	fn receive(&mut self) {
		while !matches!(self.worker, Worker::Gone(..)) {
			let (message, buffer) = match self.waiting.take() {
				Some(waiting) => waiting,
				None => {
					let Some(envelope) = self.port.receive() else { return };
					if matches!(self.worker, Worker::Starting)
						&& let Some(worker) = version_of(&envelope).filter(|&v| v != VERSION)
					{
						return self.gone(WorkerGone::OtherVersion { page: VERSION, worker });
					}
					match decode(&envelope) {
						Ok(message) => (message, envelope.buffer),
						Err(e) => return self.lose(Lost::Broken(format!("an unreadable message: {e}"))),
					}
				}
			};
			// A spill not ready yet wakes the window once it is (`crate::wake`); the next pump takes the message.
			if matches!(message, ToPage::StoreOpened { .. })
				&& self.spill.poll_ready(&mut Context::from_waker(Waker::noop())).is_pending()
			{
				self.waiting = Some((message, buffer));
				return;
			}
			self.handle(message, buffer);
		}
	}

	fn handle(&mut self, message: ToPage, buffer: Option<Buffer>) {
		match message {
			ToPage::Ready { version: _, build, gpu } => {
				let page = crate::build_id();
				if build != page {
					return self.gone(WorkerGone::OtherBuild { page, worker: build });
				}
				match &gpu {
					Ok(name) => tracing::info!("the run worker started, {}", EngineLabel::of(Some(name))),
					Err(why) => tracing::info!("the run worker started without a GPU: {why}"),
				}
				self.worker = Worker::Ready(gpu);
			}
			ToPage::EngineBuilt { engine, fallback } => {
				let before = self.engine.as_ref().and_then(|(_, why)| why.as_ref());
				if let Some(why) = fallback.as_ref().filter(|&why| before != Some(why)) {
					tracing::warn!("the GPU engine is not available ({why}); the CPU engine runs instead");
				}
				self.engine = Some((engine, fallback));
			}
			ToPage::StoreOpened { id, layout } => self.open(id, layout),
			ToPage::Frame { store, generation, frame } => {
				let Some(planes) = buffer else {
					return self.lose(Lost::Broken(format!("frame {frame} came without planes")));
				};
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
				let Some(planes) = buffer else {
					return self.lose(Lost::Broken(format!("fit frame {frame} came without planes")));
				};
				let split = shape.plane_bytes().min(planes.len());
				let (field, intensity) = (planes.to_vec(0..split), planes.to_vec(split..planes.len()));
				let snapshot = Arc::new(Snapshot { shape, step, field, intensity });
				self.fit = Some(FitFrame { id, frame, snapshot });
			}
			ToPage::Lost(lost) => self.lose(lost),
		}
	}

	/// The mirror of the worker's store `id`: the frames that fit the budget in memory, the rest spilled. The
	/// mirror lets go of the store before first; like the native run's outgoing store, its memory counts as free.
	fn open(&mut self, id: u64, layout: Layout) {
		let layout = StoreLayout::from(layout);
		self.mirror = None;
		let budget = Budget::of_memory(self.frame_memory, self.ram_cache);
		let store = FrameStore::open(layout, budget, &self.spill).map(Arc::new).map_err(|e| {
			tracing::debug!("the frames of the run do not fit: {e}");
			NoStore::NotOpened(e.to_string())
		});
		self.mirror = Some(Mirror { id, generation: 0, store });
	}

	/// Writes a frame of the worker's into the mirror and acknowledges it, once its spill write completed when
	/// it is spilled. A frame of a store or generation gone is acknowledged unwritten.
	fn land(&mut self, store: u64, generation: u64, frame: usize, planes: Buffer) {
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
		self.send(&ToWorker::Ack { store, generation, frame, result }, None);
	}

	/// The worker is lost: it did not start, or stopped once ready.
	fn lose(&mut self, lost: Lost) {
		self.gone(match self.worker {
			Worker::Starting => WorkerGone::NotStarted(lost),
			_ => WorkerGone::Stopped(lost),
		});
	}

	/// No run starts again.
	fn gone(&mut self, gone: WorkerGone) {
		tracing::error!("{gone}");
		let failure = Arc::new(RunError::Worker(gone.to_string()));
		self.worker = Worker::Gone(gone, failure);
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

	/// Copies `planes`, the frame's in layer order, into the store's buffer for frame `frame` and stores it; the
	/// mirror's frame written. A buffer the memory refuses lets go of the store: its frames fail from then on.
	fn write(&mut self, frame: usize, planes: Buffer) -> Result<FrameKey, String> {
		let store = self.store.as_ref().map_err(ToString::to_string)?;
		let plane = store.shape().plane_bytes();
		let mut snapshot = store.buffer_for(frame);
		let reserved = store.layers().iter().try_for_each(|&layer| {
			let out = snapshot.plane_mut(layer);
			out.try_reserve_exact(plane.saturating_sub(out.len()))
		});
		if let Err(e) = reserved {
			let why = format!("the browser has no memory for frame {frame} of the run ({plane} bytes a layer: {e})");
			tracing::error!("{why}; a smaller RAM cache size keeps fewer frames in memory");
			self.store = Err(NoStore::NoMemory(why.clone()));
			return Err(why);
		}
		if planes.len() != store.frame_bytes() {
			return Err(StoreError::ShapeMismatch.to_string());
		}
		snapshot.step = store.step_of(frame) as u32;
		for (index, &layer) in store.layers().iter().enumerate() {
			let out = snapshot.plane_mut(layer);
			out.resize(plane, 0);
			planes.copy_to(index * plane, out);
		}
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
		self.ram_cache = goal.ram_cache;
		let image = goal.run.as_ref().and_then(|r| image_key(&r.project));
		if let Some(key) = image.filter(|&k| self.image != Some(k)) {
			let project = &goal.run.as_ref().expect("an image key comes from a run").project;
			let bytes = project.scene.image_data.as_deref().expect("an image key has its bytes");
			self.send(&ToWorker::Image { key }, Some(bytes.to_vec()));
			self.image = Some(key);
		}
		self.send(&ToWorker::Goal { goal, image }, None);
	}

	fn status(&self) -> Status {
		let s = &self.status;
		let state = match (&self.worker, &s.state) {
			(Worker::Gone(_, e), _) => WorkState::Failed(e.clone()),
			(_, State::Idle) => WorkState::Idle,
			(_, State::Stepping { target }) => WorkState::Stepping { target: *target },
			(_, State::Fitting) => WorkState::Fitting,
			(_, State::Paused) => WorkState::Paused,
			(_, State::Failed { .. }) => {
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
		self.send(&ToWorker::Rebuild { use_gpu: self.use_gpu.load(Ordering::Relaxed) }, None);
	}

	fn discard_frames(&mut self) {
		self.send(&ToWorker::Discard, None);
	}

	fn budgets(&self, cache: RamCache, _reclaimed: u64) -> (u64, u64) {
		(Budget::of_memory(self.frame_memory, cache), Budget::of_memory(self.frame_memory, RamCache::Auto))
	}

	/// The engine built last; before the first, the one asked for, once the worker is ready.
	fn engine(&self) -> EngineLabel {
		if let Some((engine, _)) = &self.engine {
			return engine.clone();
		}
		match &self.worker {
			Worker::Starting => EngineLabel::Starting,
			Worker::Ready(gpu) => {
				let gpu = gpu.as_ref().ok().filter(|_| self.use_gpu.load(Ordering::Relaxed));
				EngineLabel::of(gpu.map(String::as_str))
			}
			Worker::Gone(..) => EngineLabel::Missing,
		}
	}

	fn worker_gone(&self) -> Option<WorkerGone> {
		Remote::worker_gone(self)
	}

	fn memory_refused(&self) -> Option<String> {
		Remote::memory_refused(self)
	}

	/// Handles the messages waiting and the mirror's spill writes done.
	fn pump(&mut self) {
		self.receive();
		self.release();
		self.settle();
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use wavedude_core::engine::{FrameShape, SampleFormat};
	use wavedude_core::model::Layer;

	use super::super::Loopback;
	use super::*;

	const GIB: u64 = 1 << 30;

	/// No spill; each refused open notes whether the store it watches is still alive.
	#[derive(Clone, Default)]
	struct NoSpill(Arc<Mutex<(std::sync::Weak<FrameStore>, Option<bool>)>>);

	impl SpillFactory for NoSpill {
		fn poll_ready(&self, _: &mut Context<'_>) -> Poll<()> {
			Poll::Ready(())
		}

		fn open(&self, _: u64, _: &StoreLayout, _: u64) -> io::Result<Box<dyn SpillBackend>> {
			let mut probe = lock(&self.0);
			probe.1 = Some(probe.0.strong_count() > 0);
			Err(io::Error::other("no spill in the tests"))
		}
	}

	fn layout() -> StoreLayout {
		let shape = FrameShape {
			canvas_width: 4,
			canvas_height: 2,
			divisor: 1,
			format: SampleFormat::Index,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: false,
		};
		StoreLayout { shape, layers: &[Layer::Field, Layer::Intensity], capacity: 4, capture_every: 1 }
	}

	/// A mirror in memory and its store.
	fn mirror() -> (Mirror, Arc<FrameStore>) {
		let store = Arc::new(FrameStore::in_memory(layout(), GIB));
		(Mirror { id: 1, generation: 0, store: Ok(store.clone()) }, store)
	}

	/// A frame's planes as the worker posts them: the field of `value`, the intensity of `value + 1`.
	fn planes(value: u8) -> Buffer {
		let plane = layout().shape.plane_bytes();
		[vec![value; plane], vec![value + 1; plane]].concat().into()
	}

	#[test]
	fn a_mirrored_frame_holds_each_plane_in_an_allocation_of_its_size() {
		let (mut mirror, store) = mirror();
		mirror.write(0, planes(1)).unwrap();
		store.clear();
		let (_, _, s) = store.latest().unwrap();
		let plane = store.shape().plane_bytes();
		assert_eq!((s.field.len(), s.field.capacity()), (plane, plane));
		assert_eq!((s.intensity.len(), s.intensity.capacity()), (plane, plane));
		assert_eq!((s.field[0], s.intensity[0]), (1, 2));
	}

	#[test]
	fn a_mirror_refilled_after_each_clear_writes_into_the_buffers_it_had() {
		let (mut mirror, store) = mirror();
		let mut seen = HashSet::new();
		for generation in 0..5 {
			mirror.follow(generation);
			for frame in 0..store.capacity() {
				mirror.write(frame, planes(frame as u8)).unwrap();
				seen.insert(store.with_layer(frame, Layer::Field, |b| b.as_ptr() as usize).unwrap());
			}
		}
		assert!(seen.len() <= store.capacity() + 1, "{} buffers, the latest kept beside the frames", seen.len());
	}

	#[test]
	fn a_frame_the_memory_refuses_lets_go_of_the_mirror_and_fails_its_frames() {
		let mut huge = layout();
		(huge.shape.canvas_width, huge.shape.canvas_height) = (1 << 31, 1 << 31);
		(huge.layers, huge.capacity) = (&[Layer::Field], 1);
		let store = Arc::new(FrameStore::in_memory(huge, u64::MAX));
		let mut mirror = Mirror { id: 1, generation: 0, store: Ok(store.clone()) };
		let refused = mirror.write(0, Vec::new().into()).unwrap_err();
		assert!(refused.contains("no memory for frame 0"), "{refused}");
		assert!(
			matches!(mirror.store, Err(NoStore::NoMemory(_))) && Arc::strong_count(&store) == 1,
			"the mirror let go of its store"
		);
		assert_eq!(mirror.write(0, planes(1)), Err(refused), "the frames after fail alike");
	}

	#[test]
	fn a_new_mirror_takes_the_whole_budget_and_lets_go_of_the_one_before_first() {
		let spill = NoSpill::default();
		let mut remote =
			Remote::new(Loopback::pair().0, Arc::new(AtomicBool::new(false)), GIB, Box::new(spill.clone()));
		let store = |remote: &Remote<Loopback>| match &remote.mirror {
			Some(Mirror { store: Ok(store), .. }) => store.clone(),
			_ => unreachable!("the frames fit"),
		};
		remote.open(1, Layout::from(&layout()));
		let held = store(&remote);
		remote.open(2, Layout::from(&layout()));
		assert_eq!(store(&remote).ram_budget(), GIB, "a reader holding the store before takes nothing from it");
		drop(held);

		lock(&spill.0).0 = Arc::downgrade(&store(&remote));
		let mut past_memory = Layout::from(&layout());
		past_memory.capacity = (GIB / layout().frame_bytes() as u64) as usize + 1;
		remote.open(3, past_memory);
		assert_eq!(lock(&spill.0).1, Some(false), "the store before was let go of before the next opened");
	}

	#[test]
	fn a_ram_cache_size_replaces_the_frame_memory_and_auto_keeps_it() {
		let mut remote =
			Remote::new(Loopback::pair().0, Arc::new(AtomicBool::new(false)), GIB / 8, Box::new(NoSpill::default()));
		let budget = |remote: &mut Remote<Loopback>, ram_cache| {
			remote.set_goal(Goal { ram_cache, ..Goal::default() });
			remote.open(1, Layout::from(&layout()));
			let store = remote.mirror.as_ref().and_then(|m| m.store.clone().ok()).expect("the frames fit");
			(store.ram_budget(), remote.budgets(ram_cache, 0))
		};
		assert_eq!(budget(&mut remote, RamCache::Bytes(2 * GIB)), (2 * GIB, (2 * GIB, GIB / 8)));
		assert_eq!(budget(&mut remote, RamCache::Auto), (GIB / 8, (GIB / 8, GIB / 8)));
	}
}
