//! The frame store: a run's captured frames, in memory up to a budget, the rest in a spill file
//! (spec/40-frames-store-render.md, Frame store and Memory budget).
//!
//! Layout: frame `f` holds one plane per layer in the order of [`FrameStore::layers`] (the field, then the
//! intensity when tracked), each `shape.plane_bytes()` bytes, rows top to bottom without padding. Frames
//! `0..first_spilled` live in memory as the snapshot written, its planes kept; the rest live in the spill file
//! after its 64-byte header at `64 + (frame - first)*frame_bytes + layer_index*plane_bytes`.
//!
//! Frames are written by index, in any order ([`FrameStore::write_at`]); which are present is tracked per frame
//! and as the contiguous prefix [`FrameStore::count`]. [`FrameStore::clear`] forgets every frame, keeps the
//! buffers for the next generation and moves [`FrameStore::generation`], so a reader tells the frames of one
//! project state from another's; the frame written last stays readable as [`FrameStore::latest`], so a burst of
//! edits shows each frame made even when the next edit's generation has begun.
//!
//! Threads: one writer at a time; any thread may read a present frame meanwhile. The capturing thread asks for
//! the buffer of a frame ([`FrameStore::buffer_for`]) and the engine captures into it, so a frame in memory is
//! never copied. The memory frames are allocated and their pages touched ahead of the capture by a producer
//! thread on a small rayon pool, after the memory the budget counts on has been released; cleared frames'
//! buffers come back first. A frame that cannot be allocated sends that frame and every later one to the file
//! while the file is empty, else it stays in memory; the file's frames reuse a few buffers. The file is deleted
//! on drop.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::JoinHandle;

use rayon::prelude::*;

use crate::engine::{FrameShape, SampleFormat, Snapshot};
use crate::model::Layer;

/// Bytes of the spill file's header.
pub const HEADER_BYTES: u64 = 64;
const MAGIC: i32 = 0x5346_4457;
const VERSION: i32 = 3;
/// Blocks the producer prepares ahead of the writer.
const PREPARE_DEPTH: usize = 4;
/// Buffers kept for the frames that go to the file.
const RECYCLED: usize = 4;
/// Threads that touch a fresh block's pages.
const TOUCH_THREADS: usize = 4;
/// A block under this size is touched on the producer thread alone.
const TOUCH_PARALLEL_BYTES: usize = 4 << 20;
const PAGE: usize = 4096;

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
	#[error("frame file: {0}")]
	Io(#[from] io::Error),
}

/// The bytes frames may take in memory.
pub struct Budget;

impl Budget {
	/// Kept free for everything else.
	pub const MARGIN: u64 = 2 << 30;

	/// The budget on this machine now: `max(0, available + reclaimed - 2 GiB)`, capped by `percent` of the
	/// physical memory when `percent > 0` (Auto at 0). `reclaimed` is the outgoing run's store, which counts as
	/// free. Available is the OS's estimate of memory a process can take without swapping (sysinfo).
	pub fn compute(reclaimed_bytes: u64, percent: u32) -> u64 {
		let mut system = sysinfo::System::new();
		system.refresh_memory();
		Self::of(system.available_memory(), system.total_memory(), percent, reclaimed_bytes)
	}

	/// The arithmetic of [`compute`](Self::compute) on given readings.
	pub fn of(available: u64, physical: u64, percent: u32, reclaimed: u64) -> u64 {
		let by_available = (available + reclaimed).saturating_sub(Self::MARGIN);
		if percent == 0 {
			return by_available;
		}
		let by_share = (physical as f64 * percent.min(100) as f64 / 100.0) as u64;
		by_share.min(by_available)
	}

	/// Frames of `frame_bytes` (not 0) each that fit into `budget`.
	pub fn frames_in(budget: u64, frame_bytes: u64) -> usize {
		(budget / frame_bytes).min(usize::MAX as u64) as usize
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
	pub fn file_bytes(&self, frames: usize) -> u64 {
		HEADER_BYTES + frames as u64 * self.frame_bytes() as u64
	}
}

/// The spill file: frames `first..capacity`, `first` moved down (the file still empty) when memory runs out
/// before the budget's frame count.
struct Spill {
	/// Unlinked at creation: the OS reclaims it when the last handle closes, a killed process included.
	file: File,
	first: AtomicUsize,
}

/// Waited for on the producer thread before the first block is allocated.
pub type Release = Box<dyn FnOnce() + Send>;

/// Removes the named `.wdfs` spill files earlier builds left in `dir` when their process died; the bytes freed.
pub fn sweep_leftovers(dir: &Path) -> io::Result<u64> {
	let mut freed = 0;
	let entries = match std::fs::read_dir(dir) {
		Ok(entries) => entries,
		Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
		Err(e) => return Err(e),
	};
	for entry in entries {
		let entry = entry?;
		if entry.path().extension().is_some_and(|e| e == "wdfs") {
			freed += entry.metadata()?.len();
			std::fs::remove_file(entry.path())?;
		}
	}
	Ok(freed)
}

/// See the module docs for layout and threading.
pub struct FrameStore {
	layout: StoreLayout,
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
	spill: OnceLock<Spill>,
	spill_dir: PathBuf,
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
	/// The capturing thread's end of the producer, and the blocks taken from it: all `ram_frames` taken means
	/// the producer finished, fewer with the channel closed means its memory ran out.
	prepared: Mutex<Option<(Receiver<Snapshot>, usize)>>,
	/// Serialises writers and the clear.
	writing: Mutex<()>,
	cancel: Arc<AtomicBool>,
	producer: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FrameStore {
	/// A store in the default cache folder (`<temp>/wavedude`) with `ram_budget` bytes of memory. The file for
	/// the frames beyond the budget is created now, so a lack of disk space shows at the start.
	pub fn new(layout: StoreLayout, ram_budget: u64, release: Option<Release>) -> io::Result<Self> {
		Self::in_dir(layout, ram_budget, release, &std::env::temp_dir().join("wavedude"))
	}

	/// As [`new`](Self::new) with the spill file in `dir`.
	pub fn in_dir(layout: StoreLayout, ram_budget: u64, release: Option<Release>, dir: &Path) -> io::Result<Self> {
		assert!(!layout.layers.is_empty(), "a store needs at least one layer");
		let frame_bytes = layout.frame_bytes();
		let ram_frames = layout.capacity.min(Budget::frames_in(ram_budget, frame_bytes as u64));
		let cancel = Arc::new(AtomicBool::new(false));
		let store = Self {
			layout,
			ram_frames,
			blocks: (0..layout.capacity).map(|_| Mutex::new(None)).collect(),
			memory_end: AtomicUsize::new(ram_frames),
			recycled: Mutex::new(Vec::new()),
			file_buffers: Mutex::new(Vec::new()),
			first_spilled: AtomicUsize::new(layout.capacity),
			spill: OnceLock::new(),
			spill_dir: dir.to_path_buf(),
			present: (0..layout.capacity).map(|_| AtomicBool::new(false)).collect(),
			prefix: AtomicUsize::new(0),
			present_count: AtomicUsize::new(0),
			spilled_count: AtomicUsize::new(0),
			generation: AtomicU64::new(0),
			last_written: AtomicUsize::new(layout.capacity),
			latest: Mutex::new(None),
			prepared: Mutex::new(None),
			writing: Mutex::new(()),
			cancel: cancel.clone(),
			producer: Mutex::new(None),
		};
		if ram_frames < layout.capacity {
			store.start_spill(ram_frames)?;
		}
		if ram_frames > 0 {
			let (tx, rx) = sync_channel(PREPARE_DEPTH);
			*lock(&store.prepared) = Some((rx, 0));
			let handle = std::thread::Builder::new()
				.name("frame-blocks".into())
				.spawn(move || prepare_blocks(ram_frames, layout, release, tx, &cancel))?;
			*lock(&store.producer) = Some(handle);
		}
		Ok(store)
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

	/// Moves at every [`clear`](Self::clear): the frames present belong to this generation.
	pub fn generation(&self) -> u64 {
		self.generation.load(Ordering::Acquire)
	}

	/// Frames the budget planned for memory.
	pub fn ram_frames(&self) -> usize {
		self.ram_frames
	}

	/// The first frame in the file; the capacity when none is.
	pub fn first_spilled(&self) -> usize {
		self.first_spilled.load(Ordering::Acquire)
	}

	pub fn frames_in_memory(&self) -> usize {
		self.present_count() - self.frames_on_disk()
	}

	pub fn frames_on_disk(&self) -> usize {
		self.spilled_count.load(Ordering::Acquire)
	}

	/// Bytes of memory the store holds or plans to hold.
	pub fn ram_bytes(&self) -> u64 {
		self.first_spilled().min(self.ram_frames) as u64 * self.frame_bytes() as u64
	}

	/// The spill file's size, when the store has one.
	pub fn spill_bytes(&self) -> Option<u64> {
		self.spill.get().map(|s| s.file.metadata().map_or(0, |m| m.len()))
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
		let layout = &self.layout;
		let spill = match self.spill.get() {
			Some(spill) => spill,
			None => {
				std::fs::create_dir_all(&self.spill_dir)?;
				let file = tempfile::tempfile_in(&self.spill_dir)?;
				assert!(
					self.spill.set(Spill { file, first: AtomicUsize::new(first) }).is_ok(),
					"the spill is created once"
				);
				self.spill.get().expect("set above")
			}
		};
		spill.file.set_len(layout.file_bytes(layout.capacity - first))?;
		write_at(&spill.file, &header(layout, first), 0)?;
		spill.first.store(first, Ordering::Release);
		self.first_spilled.store(first, Ordering::Release);
		Ok(())
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
				let mut prepared = lock(&self.prepared);
				if let Some((rx, taken)) = prepared.as_mut() {
					match rx.recv() {
						Ok(block) => {
							*taken += 1;
							return block;
						}
						Err(_) => {
							let exhausted = *taken < self.ram_frames;
							*prepared = None;
							if exhausted {
								self.memory_end.fetch_min(frame, Ordering::AcqRel);
							}
						}
					}
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
	/// belongs in memory, else written to the file and its buffer recycled. A present frame is left as it is.
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
			tracing::debug!(frame, "frame memory exhausted, the rest goes to the file");
			self.start_spill(frame)?;
		}
		if frame < self.first_spilled() {
			*lock(&self.blocks[frame]) = Some(Arc::new(snapshot));
		} else {
			let spill = self.spill.get().expect("spill file exists past the first spilled frame");
			let mut at = self.file_offset(spill, frame);
			for &layer in self.layout.layers {
				write_at(&spill.file, snapshot.plane(layer), at)?;
				at += plane as u64;
			}
			self.spilled_count.fetch_add(1, Ordering::AcqRel);
			self.give_back(frame, snapshot);
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
	}

	/// The frame written last before the last clear, with its index: the picture of the previous project state.
	pub fn latest(&self) -> Option<(usize, u64, Arc<Snapshot>)> {
		lock(&self.latest).clone()
	}

	fn file_offset(&self, spill: &Spill, frame: usize) -> u64 {
		HEADER_BYTES + (frame - spill.first.load(Ordering::Acquire)) as u64 * self.frame_bytes() as u64
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

	/// Reads a spilled frame's plane, refused when the frame is not on the file or a clear ran meanwhile (the
	/// next generation may have written the same slot).
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

	fn read_spilled(&self, frame: usize, index: usize, out: &mut [u8]) -> io::Result<()> {
		let spill = self.spill.get().expect("a stored frame outside memory is in the file");
		read_at(&spill.file, out, self.file_offset(spill, frame) + (index * out.len()) as u64)
	}
}

impl Drop for FrameStore {
	fn drop(&mut self) {
		self.cancel.store(true, Ordering::Relaxed);
		// The producer may wait on a full channel: dropping the receiver ends it.
		self.prepared.get_mut().unwrap_or_else(PoisonError::into_inner).take();
		if let Some(handle) = self.producer.get_mut().unwrap_or_else(PoisonError::into_inner).take() {
			let _ = handle.join();
		}
	}
}

fn header(layout: &StoreLayout, first: usize) -> [u8; HEADER_BYTES as usize] {
	let s = &layout.shape;
	let flags = layout.layers.iter().fold(0, |f, l| {
		f | match l {
			Layer::Field => 1,
			Layer::Intensity => 2,
		}
	});
	let fields = [
		MAGIC,
		VERSION,
		s.width() as i32,
		s.height() as i32,
		flags,
		layout.capture_every as i32,
		layout.capacity as i32,
		first as i32,
		match s.format {
			SampleFormat::Half => 0,
			SampleFormat::Index => 1,
		},
		s.divisor as i32,
		s.canvas_width as i32,
		s.canvas_height as i32,
		s.intensity_root as i32,
	];
	let mut out = [0; HEADER_BYTES as usize];
	for (chunk, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(fields) {
		*chunk = v.to_le_bytes();
	}
	out
}

/// Allocates `frames` frames of `layout`, touches their pages and hands them over, after `release`.
/// Stops at a failed allocation, a cancel, or a dropped receiver.
fn prepare_blocks(
	frames: usize,
	layout: StoreLayout,
	release: Option<Release>,
	tx: SyncSender<Snapshot>,
	cancel: &AtomicBool,
) {
	if let Some(release) = release {
		release();
	}
	let pool =
		rayon::ThreadPoolBuilder::new().num_threads(TOUCH_THREADS).thread_name(|i| format!("frame-touch-{i}")).build();
	let bytes = layout.shape.plane_bytes();
	let plane = || {
		let mut plane = Vec::new();
		plane.try_reserve_exact(bytes).ok()?;
		match (&pool, bytes >= TOUCH_PARALLEL_BYTES) {
			(Ok(pool), true) => pool.install(|| {
				let chunk = bytes.div_ceil(TOUCH_THREADS).next_multiple_of(PAGE);
				plane.par_extend(rayon::iter::repeat_n(0u8, bytes).with_min_len(chunk));
			}),
			_ => plane.resize(bytes, 0),
		}
		Some(plane)
	};
	for _ in 0..frames {
		if cancel.load(Ordering::Relaxed) {
			return;
		}
		let mut block = Snapshot::new(layout.shape);
		for &layer in layout.layers {
			let Some(p) = plane() else { return };
			match layer {
				Layer::Field => block.field = p,
				Layer::Intensity => block.intensity = p,
			}
		}
		if tx.send(block).is_err() {
			return;
		}
	}
}

#[cfg(unix)]
fn write_at(file: &File, bytes: &[u8], at: u64) -> io::Result<()> {
	std::os::unix::fs::FileExt::write_all_at(file, bytes, at)
}

#[cfg(unix)]
fn read_at(file: &File, bytes: &mut [u8], at: u64) -> io::Result<()> {
	std::os::unix::fs::FileExt::read_exact_at(file, bytes, at)
}

#[cfg(windows)]
fn write_at(file: &File, mut bytes: &[u8], mut at: u64) -> io::Result<()> {
	use std::os::windows::fs::FileExt;
	while !bytes.is_empty() {
		let n = file.seek_write(bytes, at)?;
		if n == 0 {
			return Err(io::ErrorKind::WriteZero.into());
		}
		bytes = &bytes[n..];
		at += n as u64;
	}
	Ok(())
}

#[cfg(windows)]
fn read_at(file: &File, mut bytes: &mut [u8], mut at: u64) -> io::Result<()> {
	use std::os::windows::fs::FileExt;
	while !bytes.is_empty() {
		let n = file.seek_read(bytes, at)?;
		if n == 0 {
			return Err(io::ErrorKind::UnexpectedEof.into());
		}
		bytes = &mut bytes[n..];
		at += n as u64;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn budget_arithmetic() {
		const G: u64 = 1 << 30;
		assert_eq!(Budget::of(10 * G, 64 * G, 0, 0), 8 * G);
		assert_eq!(Budget::of(10 * G, 64 * G, 0, 3 * G), 11 * G, "the outgoing run counts as free");
		assert_eq!(Budget::of(10 * G, 64 * G, 10, 3 * G), (6.4 * G as f64) as u64, "the share caps it");
		assert_eq!(Budget::of(G, 64 * G, 0, 0), 0);
		assert_eq!(Budget::of(10 * G, 64 * G, 500, 0), 8 * G);
	}

	#[test]
	fn header_fields() {
		let shape = FrameShape {
			canvas_width: 67,
			canvas_height: 45,
			divisor: 2,
			format: SampleFormat::Index,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: true,
		};
		let layout = StoreLayout { shape, layers: &[Layer::Field, Layer::Intensity], capacity: 9, capture_every: 4 };
		let h = header(&layout, 3);
		let at = |i: usize| i32::from_le_bytes(h[i..i + 4].try_into().unwrap());
		assert_eq!(&h[0..4], b"WDFS");
		assert_eq!([at(4), at(8), at(12), at(16), at(20), at(24), at(28)], [3, 34, 23, 3, 4, 9, 3]);
		assert_eq!([at(32), at(36), at(40), at(44), at(48)], [1, 2, 67, 45, 1]);
		assert!(h[52..].iter().all(|&b| b == 0));
	}
}
