//! The frame store: a run's captured frames, in memory up to a budget, the rest spilled: natively to a file, in
//! the browser to the origin private file system (spec/40-frames-store-render.md, Frame store and Memory budget).
//!
//! Layout: frame `f` holds one plane per layer in the order of [`FrameStore::layers`] (the field, then the
//! intensity when tracked), each `shape.plane_bytes()` bytes, rows top to bottom without padding. Frames
//! `0..first_spilled` live in memory as the snapshot written, its planes kept; the rest live in the spill file
//! after its 64-byte header at `64 + (frame - first)*frame_bytes + layer_index*plane_bytes`, or, pending
//! (pending.rs), one frame per entry of its `SpillBackend`.
//!
//! Frames are written by index, in any order ([`FrameStore::write_at`]); which are present is tracked per frame
//! and as the contiguous prefix [`FrameStore::count`]. [`FrameStore::clear`] forgets every frame, keeps the
//! buffers for the next generation and moves [`FrameStore::generation`], so a reader tells the frames of one
//! project state from another's; the frame written last stays readable as [`FrameStore::latest`], so a burst of
//! edits shows each frame made even when the next edit's generation has begun.
//!
//! Threads: one writer at a time; any thread may read a present frame meanwhile. The capturing thread asks for
//! the buffer of a frame ([`FrameStore::buffer_for`]) and the engine captures into it, so a frame in memory is
//! never copied. Natively the memory frames are allocated and their pages touched ahead of the capture by a
//! producer thread on a small rayon pool, after the memory the budget counts on has been released; cleared frames'
//! buffers come back first. A frame that cannot be allocated sends that frame and every later one to the file
//! while the file is empty, else it stays in memory; the file's frames reuse a few buffers. The file is deleted
//! on drop. A store in memory alone ([`FrameStore::in_memory`]) allocates its frames as they are captured and
//! refuses the frames past its budget.
//!
//! A pending spill (wasm32, `FrameStore::with_spill`) allocates its memory frames as they are captured too. A
//! spilled frame stays in memory while its write is in flight and, once `FrameStore::settle` saw it written,
//! while it is the frame written last; a read of a spilled frame that is not back yet starts its read and is
//! refused with [`StoreError::Pending`] until it is.

use crate::spec::RunSpec;
use crate::sync::lock;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use crate::engine::{FrameShape, Snapshot};
use crate::model::Layer;

use super::pending::{PendingSpill, SpillBackend, SpillFactory};
#[cfg(not(target_arch = "wasm32"))]
use super::spill::Disk;
#[cfg(not(target_arch = "wasm32"))]
pub use super::spill::{HEADER_BYTES, Release, ReleaseSignal, sweep_leftovers};

/// Buffers kept for the frames that go to the file.
const RECYCLED: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
	#[error("the store is full ({0} frames)")]
	Full(usize),
	#[error("frame {frame} is not stored ({count} are)")]
	NotStored { frame: usize, count: usize },
	#[error("the store holds no {0:?} layer")]
	NoLayer(Layer),
	#[error("the snapshot does not fit the store's frames")]
	ShapeMismatch,
	/// A spilled frame on its way back; asked again, it is there once its read completed.
	#[error("frame {frame} is being read back")]
	Pending { frame: usize },
	#[error("frame file: {0}")]
	Io(#[from] io::Error),
}

/// The bytes frames may take in memory.
pub struct Budget;

impl Budget {
	/// Kept free for everything else.
	pub const MARGIN: u64 = 2 << 30;

	/// The budget on `memory`'s readings now: `max(0, available + reclaimed - 2 GiB)`, capped by `percent` of the
	/// physical memory when `percent > 0` (Auto at 0). `reclaimed` is the outgoing run's store, which counts as
	/// free.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn compute(memory: Memory, reclaimed_bytes: u64, percent: u32) -> u64 {
		let (available, physical) = memory.read();
		Self::of(available, physical, percent, reclaimed_bytes)
	}

	/// The arithmetic of [`compute`](Self::compute) on given readings.
	pub fn of(available: u64, physical: u64, percent: u32, reclaimed: u64) -> u64 {
		Self::share((available + reclaimed).saturating_sub(Self::MARGIN), physical, percent)
	}

	/// The budget where the frames have `memory` bytes to themselves (the browser): all of it at Auto (0),
	/// else `percent` of it.
	pub fn of_memory(memory: u64, percent: u32) -> u64 {
		Self::share(memory, memory, percent)
	}

	/// `free` bytes, capped by `percent` of `physical` when `percent > 0` (Auto at 0).
	fn share(free: u64, physical: u64, percent: u32) -> u64 {
		if percent == 0 {
			return free;
		}
		let by_share = (physical as f64 * percent.min(100) as f64 / 100.0) as u64;
		by_share.min(free)
	}

	/// Frames of `frame_bytes` (not 0) each that fit into `budget`.
	pub fn frames_in(budget: u64, frame_bytes: u64) -> usize {
		(budget / frame_bytes).min(usize::MAX as u64) as usize
	}
}

/// Where a budget's memory readings come from.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Memory {
	/// The OS's at each budget: the memory a process can take without swapping, and the physical (sysinfo).
	#[default]
	System,
	/// Fixed readings, so the budgets do not depend on the machine.
	Fixed { available: u64, physical: u64 },
}

#[cfg(not(target_arch = "wasm32"))]
impl Memory {
	/// (available, physical) bytes.
	pub fn read(self) -> (u64, u64) {
		match self {
			Self::System => {
				let mut system = sysinfo::System::new();
				system.refresh_memory();
				(system.available_memory(), system.total_memory())
			}
			Self::Fixed { available, physical } => (available, physical),
		}
	}
}

/// What a store is for: geometry, planes, frames and steps per stored frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoreLayout {
	pub shape: FrameShape,
	pub layers: &'static [Layer],
	pub capacity: usize,
	/// Steps between two stored frames.
	pub capture_every: u32,
}

impl StoreLayout {
	/// The layout of a run of `spec`.
	pub fn of(spec: &RunSpec) -> Self {
		Self {
			shape: spec.shape,
			layers: spec.layers(),
			capacity: spec.stored_frames() as usize,
			capture_every: spec.capture_every(),
		}
	}

	/// The same buffers serve: geometry, format, planes, frames and steps agree (the brightness baked into
	/// index samples does not count, a cleared store takes the next generation's frames).
	pub fn fits(&self, other: &Self) -> bool {
		let geometry = |s: &Self| {
			let f = s.shape;
			(
				f.canvas_width,
				f.canvas_height,
				f.divisor,
				f.format,
				f.intensity_root,
				s.layers,
				s.capacity,
				s.capture_every,
			)
		};
		geometry(self) == geometry(other)
	}

	pub fn frame_bytes(&self) -> usize {
		self.layers.len() * self.shape.plane_bytes()
	}

	/// Bytes of a spill file holding `frames` frames.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn file_bytes(&self, frames: usize) -> u64 {
		HEADER_BYTES + frames as u64 * self.frame_bytes() as u64
	}
}

/// The next store's [`FrameStore::id`].
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn next_id() -> u64 {
	NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// See the module docs for layout and threading.
pub struct FrameStore {
	id: u64,
	layout: StoreLayout,
	ram_budget: u64,
	ram_frames: usize,
	/// Shared with readers, so a reader renders without the block's lock; a buffer whose reader is gone is reused.
	blocks: Box<[Mutex<Option<Arc<Snapshot>>>]>,
	/// Frames from here on go to the file: `ram_frames`, or the first frame the producer could not allocate.
	memory_end: AtomicUsize,
	/// Cleared memory frames' buffers, for the memory frames of the next generation.
	recycled: Mutex<Vec<Snapshot>>,
	/// A few buffers for the frames that go to the file.
	file_buffers: Mutex<Vec<Snapshot>>,
	first_spilled: AtomicUsize,
	present: Box<[AtomicBool]>,
	/// Frames `0..prefix` are all present.
	prefix: AtomicUsize,
	present_count: AtomicUsize,
	/// Frames in the file, so the file's first frame moves only while none is.
	spilled_count: AtomicUsize,
	generation: AtomicU64,
	/// The frame written last; the capacity when none since the clear.
	last_written: AtomicUsize,
	/// The frame written last before the clear, with its index.
	/// (stored index, generation it was made in, the frame).
	latest: Mutex<Option<(usize, u64, Arc<Snapshot>)>>,
	/// Serialises writers and the clear.
	writing: Mutex<()>,
	/// Where the frames past memory go; none for a store in memory alone.
	spill: Option<Spill>,
	#[cfg(not(target_arch = "wasm32"))]
	released: Arc<ReleaseSignal>,
}

impl FrameStore {
	/// A store in the default cache folder (`<temp>/wavedude`) with `ram_budget` bytes of memory. The file for
	/// the frames beyond the budget is created now, so a lack of disk space shows at the start.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn new(layout: StoreLayout, ram_budget: u64, release: Option<Release>) -> io::Result<Self> {
		Self::in_dir(layout, ram_budget, release, &std::env::temp_dir().join("wavedude"))
	}

	/// As [`new`](Self::new) with the spill file in `dir`.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn in_dir(
		layout: StoreLayout,
		ram_budget: u64,
		release: Option<Release>,
		dir: &std::path::Path,
	) -> io::Result<Self> {
		let mut store = Self::made(next_id(), layout, ram_budget, Some(Spill::File(Disk::new(dir))));
		let ram_frames = store.ram_frames;
		if ram_frames < layout.capacity {
			store.start_spill(ram_frames)?;
		}
		if ram_frames > 0
			&& let Some(Spill::File(disk)) = store.spill.as_mut()
		{
			disk.produce(ram_frames, layout, release)?;
		}
		Ok(store)
	}

	/// A store holding at most `ram_budget` bytes of frames, in memory: a frame past them is refused
	/// ([`StoreError::Full`]).
	pub fn in_memory(layout: StoreLayout, ram_budget: u64) -> Self {
		Self::made(next_id(), layout, ram_budget, None)
	}

	/// A store holding `ram_budget` bytes of frames in memory, the rest in `backend`.
	pub fn with_spill(layout: StoreLayout, ram_budget: u64, backend: Box<dyn SpillBackend>) -> Self {
		Self::pending(next_id(), layout, ram_budget, backend)
	}

	/// A store holding `ram_budget` bytes of frames in memory, the rest in a spill `spill` opens for the store's
	/// [`id`](Self::id), only when some frames do not fit the memory.
	pub fn open(layout: StoreLayout, ram_budget: u64, spill: &dyn SpillFactory) -> io::Result<Self> {
		let id = next_id();
		let frame_bytes = layout.frame_bytes() as u64;
		let ram_frames = layout.capacity.min(Budget::frames_in(ram_budget, frame_bytes));
		if ram_frames == layout.capacity {
			return Ok(Self::made(id, layout, ram_budget, None));
		}
		let backend = spill.open(id, &layout, (layout.capacity - ram_frames) as u64 * frame_bytes)?;
		Ok(Self::pending(id, layout, ram_budget, backend))
	}

	fn pending(id: u64, layout: StoreLayout, ram_budget: u64, backend: Box<dyn SpillBackend>) -> Self {
		let store = Self::made(id, layout, ram_budget, Some(Spill::Pending(PendingSpill::new(backend))));
		store.first_spilled.store(store.ram_frames, Ordering::Release);
		store
	}

	fn made(id: u64, layout: StoreLayout, ram_budget: u64, spill: Option<Spill>) -> Self {
		assert!(!layout.layers.is_empty(), "a store needs at least one layer");
		let ram_frames = layout.capacity.min(Budget::frames_in(ram_budget, layout.frame_bytes() as u64));
		Self {
			id,
			layout,
			ram_budget,
			ram_frames,
			blocks: (0..layout.capacity).map(|_| Mutex::new(None)).collect(),
			memory_end: AtomicUsize::new(ram_frames),
			recycled: Mutex::new(Vec::new()),
			file_buffers: Mutex::new(Vec::new()),
			first_spilled: AtomicUsize::new(layout.capacity),
			present: (0..layout.capacity).map(|_| AtomicBool::new(false)).collect(),
			prefix: AtomicUsize::new(0),
			present_count: AtomicUsize::new(0),
			spilled_count: AtomicUsize::new(0),
			generation: AtomicU64::new(0),
			last_written: AtomicUsize::new(layout.capacity),
			latest: Mutex::new(None),
			writing: Mutex::new(()),
			spill,
			#[cfg(not(target_arch = "wasm32"))]
			released: Arc::new(ReleaseSignal::new()),
		}
	}

	/// Fires when this store is dropped.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn release_signal(&self) -> Arc<ReleaseSignal> {
		self.released.clone()
	}

	pub fn layout(&self) -> &StoreLayout {
		&self.layout
	}

	pub fn shape(&self) -> &FrameShape {
		&self.layout.shape
	}

	pub fn layers(&self) -> &'static [Layer] {
		self.layout.layers
	}

	pub fn capacity(&self) -> usize {
		self.layout.capacity
	}

	pub fn frame_bytes(&self) -> usize {
		self.layout.frame_bytes()
	}

	/// The contiguous frames from 0: every frame below it is present.
	pub fn count(&self) -> usize {
		self.prefix.load(Ordering::Acquire)
	}

	/// Frames present, wherever they are.
	pub fn present_count(&self) -> usize {
		self.present_count.load(Ordering::Acquire)
	}

	pub fn is_complete(&self) -> bool {
		self.present_count() == self.layout.capacity
	}

	/// Whether `frame` is present (readable from any thread).
	pub fn has(&self, frame: usize) -> bool {
		self.present.get(frame).is_some_and(|p| p.load(Ordering::Acquire))
	}

	/// The last frame of the present run starting at `frame`; `None` when `frame` is absent.
	pub fn present_through(&self, frame: usize) -> Option<usize> {
		if !self.has(frame) {
			return None;
		}
		let mut last = frame;
		while self.has(last + 1) {
			last += 1;
		}
		Some(last)
	}

	/// The first absent frame at or after `frame`, if any.
	pub fn first_missing_from(&self, frame: usize) -> Option<usize> {
		(frame..self.layout.capacity).find(|&f| !self.has(f))
	}

	/// Unique among the process's stores.
	pub fn id(&self) -> u64 {
		self.id
	}

	/// Moves at every [`clear`](Self::clear): the frames present belong to this generation.
	pub fn generation(&self) -> u64 {
		self.generation.load(Ordering::Acquire)
	}

	/// The bytes of memory the store was given for its frames.
	pub fn ram_budget(&self) -> u64 {
		self.ram_budget
	}

	/// Frames the budget planned for memory.
	pub fn ram_frames(&self) -> usize {
		self.ram_frames
	}

	/// The first frame in the file; the capacity when none is.
	pub fn first_spilled(&self) -> usize {
		self.first_spilled.load(Ordering::Acquire)
	}

	/// The two counters are loaded apart, and the producer moves them one after the other.
	pub fn frames_in_memory(&self) -> usize {
		self.present_count().saturating_sub(self.frames_on_disk())
	}

	pub fn frames_on_disk(&self) -> usize {
		self.spilled_count.load(Ordering::Acquire)
	}

	/// Bytes of memory the store holds or plans to hold.
	pub fn ram_bytes(&self) -> u64 {
		self.first_spilled().min(self.ram_frames) as u64 * self.frame_bytes() as u64
	}

	/// The spill's size, when the store has one: the file's, or the pending spill's frames.
	pub fn spill_bytes(&self) -> Option<u64> {
		match self.spill.as_ref()? {
			#[cfg(not(target_arch = "wasm32"))]
			Spill::File(disk) => disk.bytes(),
			Spill::Pending(_) => Some(self.frames_on_disk() as u64 * self.frame_bytes() as u64),
		}
	}

	/// The step index stored frame `frame` holds the state after.
	pub fn step_of(&self, frame: usize) -> u64 {
		frame as u64 * self.layout.capture_every as u64
	}

	fn layer_index(&self, layer: Layer) -> Result<usize, StoreError> {
		self.layout.layers.iter().position(|&l| l == layer).ok_or(StoreError::NoLayer(layer))
	}

	/// Makes frame `first` the first of the file: created at the budget's frame count, moved down to an
	/// earlier frame when its memory could not be allocated, which happens while the file is empty.
	fn start_spill(&self, first: usize) -> io::Result<()> {
		debug_assert!(self.frames_on_disk() == 0, "the file is empty when its first frame moves");
		match self.spill.as_ref().expect("a store with a spill") {
			#[cfg(not(target_arch = "wasm32"))]
			Spill::File(disk) => disk.start(&self.layout, first)?,
			Spill::Pending(_) => {}
		}
		self.first_spilled.store(first, Ordering::Release);
		Ok(())
	}

	/// The producer's next memory frame (store.rs's Threads); none without a producer.
	fn prepared_block(&self) -> Option<Result<Snapshot, usize>> {
		match self.spill.as_ref()? {
			#[cfg(not(target_arch = "wasm32"))]
			Spill::File(disk) => disk.prepared_block(),
			Spill::Pending(_) => None,
		}
	}

	/// The buffer to capture frame `frame` into: a cleared frame's buffer or the next prepared memory frame
	/// while the frames go to memory (waiting for the producer), else a recycled or new one. Each buffer is
	/// then written with [`write_at`](Self::write_at).
	pub fn buffer_for(&self, frame: usize) -> Snapshot {
		if frame < self.first_spilled() {
			if let Some(block) = lock(&self.recycled).pop() {
				return block;
			}
			if frame < self.memory_end.load(Ordering::Acquire) {
				// All `ram_frames` taken means the producer finished, fewer means its memory ran out.
				match self.prepared_block() {
					Some(Ok(block)) => return block,
					Some(Err(taken)) if taken < self.ram_frames => {
						self.memory_end.fetch_min(frame, Ordering::AcqRel);
					}
					_ => {}
				}
				if frame < self.memory_end.load(Ordering::Acquire) {
					return Snapshot::new(self.layout.shape);
				}
			}
		}
		lock(&self.file_buffers).pop().unwrap_or_else(|| Snapshot::new(self.layout.shape))
	}

	/// [`buffer_for`](Self::buffer_for) the frame an append ([`write`](Self::write)) lands on.
	pub fn next_buffer(&self) -> Snapshot {
		self.buffer_for(self.count())
	}

	/// Appends `snapshot` as the frame after the contiguous prefix; returns its index.
	pub fn write(&self, snapshot: Snapshot) -> Result<usize, StoreError> {
		let frame = self.count();
		self.write_at(frame, snapshot)?;
		Ok(frame)
	}

	/// Stores `snapshot` (its planes in the store's shape) as frame `frame`: kept as it is while the frame
	/// belongs in memory, else written to the file and its buffer recycled. A present frame is left as it is. A
	/// store in memory alone refuses a frame past its memory.
	pub fn write_at(&self, frame: usize, mut snapshot: Snapshot) -> Result<(), StoreError> {
		let _writing = lock(&self.writing);
		if frame >= self.layout.capacity {
			return Err(StoreError::Full(self.layout.capacity));
		}
		let shape = &self.layout.shape;
		let s = &snapshot.shape;
		let plane = shape.plane_bytes();
		let same = (s.canvas_width, s.canvas_height, s.divisor, s.format, s.intensity_root)
			== (shape.canvas_width, shape.canvas_height, shape.divisor, shape.format, shape.intensity_root);
		if !same || self.layout.layers.iter().any(|&l| snapshot.plane(l).len() != plane) {
			return Err(StoreError::ShapeMismatch);
		}
		if self.has(frame) {
			self.give_back(frame, snapshot);
			return Ok(());
		}
		if !self.layout.layers.contains(&Layer::Intensity) {
			snapshot.intensity = Vec::new();
		}
		let memory_end = self.memory_end.load(Ordering::Acquire);
		if frame >= memory_end && frame < self.first_spilled() && self.frames_on_disk() == 0 {
			if self.spill.is_none() {
				return Err(StoreError::Full(memory_end));
			}
			tracing::debug!(frame, "frame memory exhausted, the rest goes to the file");
			self.start_spill(frame)?;
		}
		if frame < self.first_spilled() {
			*lock(&self.blocks[frame]) = Some(Arc::new(snapshot));
		} else {
			match self.spill.as_ref().expect("a spilled frame has a spill") {
				#[cfg(not(target_arch = "wasm32"))]
				Spill::File(disk) => {
					disk.write(&self.layout, frame, &snapshot)?;
					self.give_back(frame, snapshot);
				}
				Spill::Pending(pending) => {
					let planes: Vec<&[u8]> = self.layout.layers.iter().map(|&l| snapshot.plane(l)).collect();
					pending.write(self.generation(), frame, &planes);
					*lock(&self.blocks[frame]) = Some(Arc::new(snapshot));
				}
			}
			self.spilled_count.fetch_add(1, Ordering::AcqRel);
		}
		self.present[frame].store(true, Ordering::Release);
		self.present_count.fetch_add(1, Ordering::AcqRel);
		self.last_written.store(frame, Ordering::Release);
		let mut prefix = self.prefix.load(Ordering::Acquire);
		while prefix < self.layout.capacity && self.has(prefix) {
			prefix += 1;
		}
		self.prefix.store(prefix, Ordering::Release);
		Ok(())
	}

	/// Takes the pending spill's completed writes: a frame written leaves memory unless it is the frame written
	/// last. A failed write is the error.
	pub fn settle(&self) -> Result<(), StoreError> {
		let Some(Spill::Pending(pending)) = &self.spill else { return Ok(()) };
		let _writing = lock(&self.writing);
		for frame in pending.settle(self.generation(), self.last_written.load(Ordering::Acquire))? {
			if let Some(snapshot) = lock(&self.blocks[frame]).take()
				&& let Ok(snapshot) = Arc::try_unwrap(snapshot)
			{
				self.give_back(frame, snapshot);
			}
		}
		Ok(())
	}

	/// The pending spill has as many writes in flight as it takes.
	pub fn spill_busy(&self) -> bool {
		matches!(&self.spill, Some(Spill::Pending(p)) if p.busy())
	}

	/// The pending spill has writes in flight.
	pub fn spill_writing(&self) -> bool {
		matches!(&self.spill, Some(Spill::Pending(p)) if p.writing())
	}

	/// Ready once a write of the pending spill in flight completed, or none is; otherwise `cx`'s waker is woken
	/// when one completes.
	pub fn poll_spill_write(&self, cx: &mut Context<'_>) -> Poll<()> {
		match &self.spill {
			Some(Spill::Pending(p)) => p.poll_write(cx),
			_ => Poll::Ready(()),
		}
	}

	/// Returns a buffer [`buffer_for`](Self::buffer_for) handed out for `frame` that was not kept, to the pool
	/// it came from: the memory frames' below `memory_end`, the file's few above.
	fn give_back(&self, frame: usize, snapshot: Snapshot) {
		if frame < self.memory_end.load(Ordering::Acquire) {
			lock(&self.recycled).push(snapshot);
			return;
		}
		let mut buffers = lock(&self.file_buffers);
		if buffers.len() < RECYCLED {
			buffers.push(snapshot);
		}
	}

	/// Forgets every frame and moves the generation; the memory frames' buffers serve the next writes, but the
	/// frame written last stays as [`latest`](Self::latest).
	pub fn clear(&self) {
		let _writing = lock(&self.writing);
		let mut recycled = lock(&self.recycled);
		let last = self.last_written.swap(self.layout.capacity, Ordering::AcqRel);
		let kept = self.present.get(last).is_some_and(|p| p.swap(false, Ordering::AcqRel)).then(|| {
			lock(&self.blocks[last]).take().or_else(|| {
				let mut snapshot = Snapshot::new(self.layout.shape);
				for (index, &layer) in self.layout.layers.iter().enumerate() {
					let plane = match layer {
						Layer::Field => &mut snapshot.field,
						Layer::Intensity => &mut snapshot.intensity,
					};
					plane.resize(self.layout.shape.plane_bytes(), 0);
					if self.read_spilled(last, index, plane).is_err() {
						return None;
					}
				}
				snapshot.step = self.step_of(last) as u32;
				Some(Arc::new(snapshot))
			})
		});
		let from = self.generation.load(Ordering::Acquire);
		let mut latest = lock(&self.latest);
		if let Some((_, _, old)) = latest.take()
			&& let Ok(snapshot) = Arc::try_unwrap(old)
		{
			recycled.push(snapshot);
		}
		*latest = kept.flatten().map(|s| (last, from, s));
		for (frame, block) in self.blocks.iter().enumerate() {
			self.present[frame].store(false, Ordering::Release);
			if let Some(snapshot) = lock(block).take()
				&& let Ok(snapshot) = Arc::try_unwrap(snapshot)
			{
				recycled.push(snapshot);
			}
		}
		self.prefix.store(0, Ordering::Release);
		self.present_count.store(0, Ordering::Release);
		self.spilled_count.store(0, Ordering::Release);
		self.generation.fetch_add(1, Ordering::AcqRel);
		if let Some(Spill::Pending(pending)) = &self.spill {
			pending.cleared(from);
		}
	}

	/// The frame written last before the last clear, with its index: the picture of the previous project state.
	pub fn latest(&self) -> Option<(usize, u64, Arc<Snapshot>)> {
		lock(&self.latest).clone()
	}

	/// Calls `f` with the plane of `layer` of present frame `frame`.
	pub fn with_layer<R>(&self, frame: usize, layer: Layer, f: impl FnOnce(&[u8]) -> R) -> Result<R, StoreError> {
		self.with_layer_into(frame, layer, &mut Vec::new(), f)
	}

	/// [`Self::with_layer`] reading a spilled frame into `buf`, whose allocation is reused across calls.
	pub fn with_layer_into<R>(
		&self,
		frame: usize,
		layer: Layer,
		buf: &mut Vec<u8>,
		f: impl FnOnce(&[u8]) -> R,
	) -> Result<R, StoreError> {
		let (index, plane) = self.locate(frame, layer)?;
		if let Some(block) = lock(&self.blocks[frame]).clone() {
			return Ok(f(block.plane(self.layout.layers[index])));
		}
		buf.resize(plane, 0);
		self.read_spilled_frame(frame, index, buf)?;
		Ok(f(buf))
	}

	/// Reads a spilled frame's plane, refused when the frame is not spilled or a clear ran meanwhile (the next
	/// generation may have written the same slot).
	fn read_spilled_frame(&self, frame: usize, index: usize, out: &mut [u8]) -> Result<(), StoreError> {
		let not_stored = || StoreError::NotStored { frame, count: self.present_count() };
		let generation = self.generation();
		if !self.has(frame) || frame < self.first_spilled() {
			return Err(not_stored());
		}
		self.read_spilled(frame, index, out)?;
		if !self.has(frame) || self.generation() != generation {
			return Err(not_stored());
		}
		Ok(())
	}

	/// Copies the plane of `layer` of present frame `frame` into `out`, which must hold one plane.
	pub fn read_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> Result<(), StoreError> {
		let (index, plane) = self.locate(frame, layer)?;
		assert_eq!(out.len(), plane, "the buffer holds one plane");
		match lock(&self.blocks[frame]).clone() {
			Some(block) => out.copy_from_slice(block.plane(self.layout.layers[index])),
			None => self.read_spilled_frame(frame, index, out)?,
		}
		Ok(())
	}

	/// A copy of the plane of `layer` of present frame `frame`.
	pub fn copy_layer(&self, frame: usize, layer: Layer) -> Result<Vec<u8>, StoreError> {
		let mut out = vec![0; self.layout.shape.plane_bytes()];
		self.read_layer(frame, layer, &mut out)?;
		Ok(out)
	}

	/// The layer's index in a frame and the plane's bytes, for a present frame.
	fn locate(&self, frame: usize, layer: Layer) -> Result<(usize, usize), StoreError> {
		if !self.has(frame) {
			return Err(StoreError::NotStored { frame, count: self.present_count() });
		}
		Ok((self.layer_index(layer)?, self.layout.shape.plane_bytes()))
	}

	fn read_spilled(&self, frame: usize, index: usize, out: &mut [u8]) -> Result<(), StoreError> {
		match self.spill.as_ref().expect("a stored frame outside memory is spilled") {
			#[cfg(not(target_arch = "wasm32"))]
			Spill::File(disk) => disk.read(&self.layout, frame, index, out)?,
			Spill::Pending(pending) => {
				let planes = pending.read(self.generation(), frame).ok_or(StoreError::Pending { frame })??;
				let plane = out.len();
				out.copy_from_slice(&planes[index * plane..][..plane]);
			}
		}
		Ok(())
	}
}

/// Where a store's frames past its memory go.
enum Spill {
	#[cfg(not(target_arch = "wasm32"))]
	File(Disk),
	Pending(PendingSpill),
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for FrameStore {
	fn drop(&mut self) {
		if let Some(Spill::File(disk)) = self.spill.as_mut() {
			disk.stop();
		}
		self.released.fire();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::SampleFormat;
	use crate::run::pending::fake;

	#[test]
	fn a_scrub_across_spilled_frames_holds_two_reads_and_two_frames_at_most() {
		let shape = FrameShape {
			canvas_width: 4,
			canvas_height: 2,
			divisor: 1,
			format: SampleFormat::Index,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: false,
		};
		let layout = StoreLayout { shape, layers: &[Layer::Field], capacity: 40, capture_every: 1 };
		let disk = Arc::<Mutex<fake::Disk>>::default();
		let store = FrameStore::with_spill(layout, 0, Box::new(fake::Backend(disk.clone())));
		for k in 0..40 {
			let mut s = Snapshot::new(shape);
			s.field = vec![k; shape.plane_bytes()];
			store.write(s).unwrap();
		}
		lock(&disk).complete();
		store.settle().unwrap();
		let held = || match &store.spill {
			Some(Spill::Pending(p)) => p.held(),
			_ => unreachable!("a pending spill"),
		};
		for (i, frame) in (0..39).chain((0..39).rev()).enumerate() {
			match store.copy_layer(frame, Layer::Field) {
				Ok(plane) => assert_eq!(plane, vec![frame as u8; shape.plane_bytes()]),
				Err(e) => assert!(matches!(e, StoreError::Pending { .. }), "{e}"),
			}
			let (reads, loaded) = held();
			assert!(reads <= 3 && loaded <= 2, "frame {frame}: {reads} reads, {loaded} kept");
			assert!(lock(&disk).reads.len() <= 2, "two reads in flight at most");
			if i % 5 == 4 {
				lock(&disk).complete();
			}
		}
		lock(&disk).complete();
		let landed = store.copy_layer(0, Layer::Field);
		assert!(matches!(landed, Err(StoreError::Pending { frame: 0 })), "the newest frame asked for is read next");
		lock(&disk).complete();
		assert_eq!(store.copy_layer(0, Layer::Field).unwrap(), vec![0; shape.plane_bytes()]);
		assert!(held().1 <= 2);
	}

	#[test]
	fn a_pending_spill_holds_its_frames_until_written_and_reads_them_back_later() {
		let shape = FrameShape {
			canvas_width: 4,
			canvas_height: 2,
			divisor: 1,
			format: SampleFormat::Index,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: false,
		};
		let layout = StoreLayout { shape, layers: &[Layer::Field], capacity: 6, capture_every: 1 };
		let disk = Arc::<Mutex<fake::Disk>>::default();
		let store =
			FrameStore::with_spill(layout, 2 * layout.frame_bytes() as u64, Box::new(fake::Backend(disk.clone())));
		let plane = |k: u8| vec![k; shape.plane_bytes()];
		let frame = |k: u8| {
			let mut s = Snapshot::new(shape);
			s.field = plane(k);
			s
		};
		for k in 0..4 {
			store.write(frame(k)).unwrap();
		}
		assert_eq!((store.first_spilled(), store.frames_in_memory(), store.frames_on_disk()), (2, 2, 2));
		assert_eq!(store.copy_layer(2, Layer::Field).unwrap(), plane(2), "a write in flight reads from memory");
		assert!(store.spill_writing());

		lock(&disk).complete();
		store.settle().unwrap();
		assert!(!store.spill_writing());
		assert_eq!(store.copy_layer(3, Layer::Field).unwrap(), plane(3), "the frame written last stays in memory");
		assert!(lock(&disk).reads.is_empty());
		for _ in 0..2 {
			assert!(matches!(store.copy_layer(2, Layer::Field), Err(StoreError::Pending { frame: 2 })));
		}
		assert_eq!(lock(&disk).reads.len(), 1, "one read in flight per frame");
		lock(&disk).complete();
		assert_eq!(store.copy_layer(2, Layer::Field).unwrap(), plane(2));
		assert_eq!(store.with_layer(2, Layer::Field, |b| b[0]).unwrap(), 2, "kept once read back");

		store.write(frame(4)).unwrap();
		lock(&disk).complete();
		store.settle().unwrap();
		assert!(matches!(store.copy_layer(3, Layer::Field), Err(StoreError::Pending { frame: 3 })), "no longer last");

		store.write(frame(5)).unwrap();
		store.clear();
		let (index, _, latest) = store.latest().unwrap();
		assert_eq!((index, &latest.field), (5, &plane(5)), "the frame in flight is the latest");
		assert_eq!(lock(&disk).dropped, [0]);
		lock(&disk).complete();
		store.settle().unwrap();

		store.write_at(3, frame(7)).unwrap();
		lock(&disk).fail = Some(io::ErrorKind::QuotaExceeded);
		lock(&disk).complete();
		assert!(matches!(store.settle(), Err(StoreError::Io(e)) if e.kind() == io::ErrorKind::QuotaExceeded));
	}

	#[test]
	fn budget_arithmetic() {
		const G: u64 = 1 << 30;
		assert_eq!(Budget::of(10 * G, 64 * G, 0, 0), 8 * G);
		assert_eq!(Budget::of(10 * G, 64 * G, 0, 3 * G), 11 * G, "the outgoing run counts as free");
		assert_eq!(Budget::of(10 * G, 64 * G, 10, 3 * G), (6.4 * G as f64) as u64, "the share caps it");
		assert_eq!(Budget::of(G, 64 * G, 0, 0), 0);
		assert_eq!(Budget::of(10 * G, 64 * G, 500, 0), 8 * G);
		assert_eq!(Budget::of_memory(2 * G, 0), 2 * G, "Auto takes the memory");
		assert_eq!(Budget::of_memory(2 * G, 25), G / 2);
		assert_eq!(Budget::of_memory(2 * G, 500), 2 * G);
	}
}
