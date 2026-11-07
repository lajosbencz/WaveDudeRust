//! The frame store: a run's captured frames, in memory up to a budget, the rest in a spill file
//! (spec/40-frames-store-render.md, Frame store and Memory budget).
//!
//! Layout: frame `f` holds one plane per layer in the order of [`FrameStore::layers`] (the field, then the
//! intensity when tracked), each `shape.plane_bytes()` bytes, rows top to bottom without padding. Frames
//! `0..first_spilled` live in memory as the snapshot written, its planes kept; the rest live in the spill file
//! after its 64-byte header at `64 + (frame - first)*frame_bytes + layer_index*plane_bytes`.
//!
//! Threads: one writer appends frames in order ([`FrameStore::write`]); any thread may read a frame below
//! [`FrameStore::count`] meanwhile. The capturing thread asks for the buffer of each next frame
//! ([`FrameStore::next_buffer`]) and the engine captures into it, so a frame in memory is never copied. The
//! memory frames are allocated and their pages touched ahead of the capture by a producer thread on a small
//! rayon pool, after the memory the budget counts on has been released. A frame that cannot be allocated sends
//! that frame and every later one to the file, whose frames reuse a few buffers. The file is deleted on drop.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use rayon::prelude::*;
use tempfile::NamedTempFile;

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
	#[error("frame {frame} is not stored yet ({count} are)")]
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
	file: NamedTempFile,
	first: AtomicUsize,
}

/// Waited for on the producer thread before the first block is allocated.
pub type Release = Box<dyn FnOnce() + Send>;

/// See the module docs for layout and threading.
pub struct FrameStore {
	layout: StoreLayout,
	ram_frames: usize,
	blocks: Box<[OnceLock<Snapshot>]>,
	/// Frames from here on go to the file: `ram_frames`, or the first frame the producer could not allocate.
	memory_end: AtomicUsize,
	/// Buffers handed out by [`next_buffer`](Self::next_buffer).
	handed: AtomicUsize,
	recycled: Mutex<Vec<Snapshot>>,
	first_spilled: AtomicUsize,
	spill: OnceLock<Spill>,
	spill_dir: PathBuf,
	count: AtomicUsize,
	/// The capturing thread's end of the producer.
	prepared: Mutex<Option<Receiver<Snapshot>>>,
	/// Serialises writers.
	writing: Mutex<()>,
	cancel: Arc<AtomicBool>,
	producer: Mutex<Option<JoinHandle<()>>>,
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
			blocks: (0..ram_frames).map(|_| OnceLock::new()).collect(),
			memory_end: AtomicUsize::new(ram_frames),
			handed: AtomicUsize::new(0),
			recycled: Mutex::new(Vec::new()),
			first_spilled: AtomicUsize::new(layout.capacity),
			spill: OnceLock::new(),
			spill_dir: dir.to_path_buf(),
			count: AtomicUsize::new(0),
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
			*store.prepared.lock().unwrap() = Some(rx);
			let handle = std::thread::Builder::new()
				.name("frame-blocks".into())
				.spawn(move || prepare_blocks(ram_frames, layout, release, tx, &cancel))?;
			*store.producer.lock().unwrap() = Some(handle);
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

	/// Frames stored so far; frames below it are readable from any thread.
	pub fn count(&self) -> usize {
		self.count.load(Ordering::Acquire)
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
		self.count().min(self.first_spilled())
	}

	pub fn frames_on_disk(&self) -> usize {
		self.count().saturating_sub(self.first_spilled())
	}

	/// Bytes of memory the store holds or plans to hold.
	pub fn ram_bytes(&self) -> u64 {
		self.first_spilled().min(self.ram_frames) as u64 * self.frame_bytes() as u64
	}

	/// The spill file's path, when the store has one.
	pub fn spill_path(&self) -> Option<&Path> {
		self.spill.get().map(|s| s.file.path())
	}

	/// The step index stored frame `frame` holds the state after.
	pub fn step_of(&self, frame: usize) -> u64 {
		frame as u64 * self.layout.capture_every as u64
	}

	fn layer_index(&self, layer: Layer) -> Result<usize, StoreError> {
		self.layout.layers.iter().position(|&l| l == layer).ok_or(StoreError::NoLayer(layer))
	}

	/// Makes frame `first` the first of the file: created at the budget's frame count, moved down to an
	/// earlier frame when its memory could not be allocated, which happens before any frame was spilled.
	fn start_spill(&self, first: usize) -> io::Result<()> {
		debug_assert!(self.count() <= first, "the file is empty when its first frame moves");
		let layout = &self.layout;
		let spill = match self.spill.get() {
			Some(spill) => spill,
			None => {
				std::fs::create_dir_all(&self.spill_dir)?;
				let file = tempfile::Builder::new().prefix("frames-").suffix(".wdfs").tempfile_in(&self.spill_dir)?;
				assert!(
					self.spill.set(Spill { file, first: AtomicUsize::new(first) }).is_ok(),
					"the spill is created once"
				);
				self.spill.get().expect("set above")
			}
		};
		spill.file.as_file().set_len(layout.file_bytes(layout.capacity - first))?;
		write_at(spill.file.as_file(), &header(layout, first), 0)?;
		spill.first.store(first, Ordering::Release);
		self.first_spilled.store(first, Ordering::Release);
		Ok(())
	}

	/// The buffer to capture the next frame into, in frame order: the next prepared memory frame while the
	/// frames go to memory (waiting for the producer), else a recycled or new one. Called once per frame by the
	/// one capturing thread, each buffer then written with [`write`](Self::write).
	pub fn next_buffer(&self) -> Snapshot {
		let frame = self.handed.fetch_add(1, Ordering::Relaxed);
		if frame < self.memory_end.load(Ordering::Acquire) {
			let mut prepared = self.prepared.lock().unwrap();
			if let Some(block) = prepared.as_ref().and_then(|rx| rx.recv().ok()) {
				return block;
			}
			*prepared = None;
			self.memory_end.fetch_min(frame, Ordering::AcqRel);
		}
		self.recycled.lock().unwrap().pop().unwrap_or_else(|| Snapshot::new(self.layout.shape))
	}

	/// Appends `snapshot` (its planes in the store's shape) as the next frame: kept as it is while the frames
	/// go to memory, else written to the file and its buffer recycled.
	pub fn write(&self, mut snapshot: Snapshot) -> Result<usize, StoreError> {
		let _writing = self.writing.lock().unwrap();
		let frame = self.count.load(Ordering::Relaxed);
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
		if !self.layout.layers.contains(&Layer::Intensity) {
			snapshot.intensity = Vec::new();
		}
		if frame < self.memory_end.load(Ordering::Acquire) {
			assert!(self.blocks[frame].set(snapshot).is_ok(), "each frame is written once");
		} else {
			if frame < self.first_spilled() {
				tracing::debug!(frame, "frame memory exhausted, the rest goes to the file");
				self.start_spill(frame)?;
			}
			let spill = self.spill.get().expect("spill file exists past the first spilled frame");
			let mut at = self.file_offset(spill, frame);
			for &layer in self.layout.layers {
				write_at(spill.file.as_file(), snapshot.plane(layer), at)?;
				at += plane as u64;
			}
			let mut recycled = self.recycled.lock().unwrap();
			if recycled.len() < RECYCLED {
				recycled.push(snapshot);
			}
		}
		self.count.store(frame + 1, Ordering::Release);
		Ok(frame)
	}

	fn file_offset(&self, spill: &Spill, frame: usize) -> u64 {
		HEADER_BYTES + (frame - spill.first.load(Ordering::Acquire)) as u64 * self.frame_bytes() as u64
	}

	/// Calls `f` with the plane of `layer` of stored frame `frame`.
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
		if let Some(block) = self.blocks.get(frame).and_then(OnceLock::get) {
			return Ok(f(block.plane(self.layout.layers[index])));
		}
		buf.resize(plane, 0);
		self.read_spilled(frame, index, buf)?;
		Ok(f(buf))
	}

	/// Copies the plane of `layer` of stored frame `frame` into `out`, which must hold one plane.
	pub fn read_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> Result<(), StoreError> {
		let (index, plane) = self.locate(frame, layer)?;
		assert_eq!(out.len(), plane, "the buffer holds one plane");
		match self.blocks.get(frame).and_then(OnceLock::get) {
			Some(block) => out.copy_from_slice(block.plane(self.layout.layers[index])),
			None => self.read_spilled(frame, index, out)?,
		}
		Ok(())
	}

	/// A copy of the plane of `layer` of stored frame `frame`.
	pub fn copy_layer(&self, frame: usize, layer: Layer) -> Result<Vec<u8>, StoreError> {
		let mut out = vec![0; self.layout.shape.plane_bytes()];
		self.read_layer(frame, layer, &mut out)?;
		Ok(out)
	}

	/// The layer's index in a frame and the plane's bytes, for a stored frame.
	fn locate(&self, frame: usize, layer: Layer) -> Result<(usize, usize), StoreError> {
		let count = self.count();
		if frame >= count {
			return Err(StoreError::NotStored { frame, count });
		}
		Ok((self.layer_index(layer)?, self.layout.shape.plane_bytes()))
	}

	fn read_spilled(&self, frame: usize, index: usize, out: &mut [u8]) -> io::Result<()> {
		let spill = self.spill.get().expect("a stored frame outside memory is in the file");
		read_at(spill.file.as_file(), out, self.file_offset(spill, frame) + (index * out.len()) as u64)
	}
}

impl Drop for FrameStore {
	fn drop(&mut self) {
		self.cancel.store(true, Ordering::Relaxed);
		// The producer may wait on a full channel: dropping the receiver ends it.
		self.prepared.get_mut().unwrap().take();
		if let Some(handle) = self.producer.get_mut().unwrap().take() {
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
