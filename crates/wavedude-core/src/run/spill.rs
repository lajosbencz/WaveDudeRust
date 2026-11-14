//! What a native frame store keeps outside its memory frames: the spill file, the producer thread that prepares
//! the memory frames ahead of the capture, and the signal of the store's drop (store.rs has the layout).

use crate::sync::lock;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use rayon::prelude::*;
use web_time::Instant;

use super::store::StoreLayout;
use crate::engine::{SampleFormat, Snapshot};
use crate::model::Layer;

/// Bytes of the spill file's header.
pub const HEADER_BYTES: u64 = 64;
const MAGIC: i32 = 0x5346_4457;
const VERSION: i32 = 3;
/// Blocks the producer prepares ahead of the writer.
const PREPARE_DEPTH: usize = 4;
/// Threads that touch a fresh block's pages.
const TOUCH_THREADS: usize = 4;
/// A block under this size is touched on the producer thread alone.
const TOUCH_PARALLEL_BYTES: usize = 4 << 20;
const PAGE: usize = 4096;

/// Waited for on the producer thread before the first block is allocated.
pub type Release = Box<dyn FnOnce() + Send>;

/// Fires when a store is dropped (its last handle let go), so the next store's producer can wait for the
/// memory without polling. [`FrameStore::release_signal`](super::store::FrameStore::release_signal).
pub struct ReleaseSignal {
	done: Mutex<bool>,
	fired: Condvar,
}

impl ReleaseSignal {
	pub(super) fn new() -> Self {
		Self { done: Mutex::new(false), fired: Condvar::new() }
	}

	pub(super) fn fire(&self) {
		*lock(&self.done) = true;
		self.fired.notify_all();
	}

	/// Waits up to `timeout` for the drop; whether it happened.
	pub fn wait(&self, timeout: Duration) -> bool {
		let deadline = Instant::now() + timeout;
		let mut done = lock(&self.done);
		while !*done {
			let left = deadline.saturating_duration_since(Instant::now());
			if left.is_zero() {
				break;
			}
			done = self.fired.wait_timeout(done, left).unwrap_or_else(PoisonError::into_inner).0;
		}
		*done
	}
}

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

/// The spill file: frames `first..capacity`, `first` moved down (the file still empty) when memory runs out
/// before the budget's frame count.
struct SpillFile {
	/// Unlinked at creation: the OS reclaims it when the last handle closes, a killed process included.
	file: File,
	first: AtomicUsize,
}

/// The spill file, created when first needed, and the producer of the memory frames.
pub(super) struct Disk {
	dir: PathBuf,
	file: OnceLock<SpillFile>,
	/// The capturing thread's end of the producer, and the blocks taken from it.
	prepared: Mutex<Option<(Receiver<Snapshot>, usize)>>,
	cancel: Arc<AtomicBool>,
	producer: Option<JoinHandle<()>>,
}

impl Disk {
	pub(super) fn new(dir: &Path) -> Self {
		Self {
			dir: dir.to_path_buf(),
			file: OnceLock::new(),
			prepared: Mutex::new(None),
			cancel: Arc::new(AtomicBool::new(false)),
			producer: None,
		}
	}

	/// Starts the thread that allocates `frames` memory frames, after `release`.
	pub(super) fn produce(&mut self, frames: usize, layout: StoreLayout, release: Option<Release>) -> io::Result<()> {
		let (tx, rx) = sync_channel(PREPARE_DEPTH);
		*lock(&self.prepared) = Some((rx, 0));
		let cancel = self.cancel.clone();
		self.producer = Some(
			std::thread::Builder::new()
				.name("frame-blocks".into())
				.spawn(move || prepare_blocks(frames, layout, release, tx, &cancel))?,
		);
		Ok(())
	}

	/// The next prepared block, waiting for the producer; `Err` with the blocks taken once it ended, `None` after.
	pub(super) fn prepared_block(&self) -> Option<Result<Snapshot, usize>> {
		let mut prepared = lock(&self.prepared);
		let (rx, taken) = prepared.as_mut()?;
		match rx.recv() {
			Ok(block) => {
				*taken += 1;
				Some(Ok(block))
			}
			Err(_) => {
				let taken = *taken;
				*prepared = None;
				Some(Err(taken))
			}
		}
	}

	/// Ends the producer.
	pub(super) fn stop(&mut self) {
		self.cancel.store(true, Ordering::Relaxed);
		// The producer may wait on a full channel: dropping the receiver ends it.
		self.prepared.get_mut().unwrap_or_else(PoisonError::into_inner).take();
		if let Some(handle) = self.producer.take() {
			let _ = handle.join();
		}
	}

	/// Makes frame `first` the first of the file, which is created on the first call.
	pub(super) fn start(&self, layout: &StoreLayout, first: usize) -> io::Result<()> {
		let spill = match self.file.get() {
			Some(spill) => spill,
			None => {
				std::fs::create_dir_all(&self.dir)?;
				let file = tempfile::tempfile_in(&self.dir)?;
				assert!(
					self.file.set(SpillFile { file, first: AtomicUsize::new(first) }).is_ok(),
					"the spill is created once"
				);
				self.file.get().expect("set above")
			}
		};
		spill.file.set_len(layout.file_bytes(layout.capacity - first))?;
		write_at(&spill.file, &header(layout, first), 0)?;
		spill.first.store(first, Ordering::Release);
		Ok(())
	}

	/// The spill file's size, once it exists.
	pub(super) fn bytes(&self) -> Option<u64> {
		self.file.get().map(|s| s.file.metadata().map_or(0, |m| m.len()))
	}

	pub(super) fn write(&self, layout: &StoreLayout, frame: usize, snapshot: &Snapshot) -> io::Result<()> {
		let spill = self.file.get().expect("spill file exists past the first spilled frame");
		let mut at = offset(spill, layout, frame);
		for &layer in layout.layers {
			write_at(&spill.file, snapshot.plane(layer), at)?;
			at += layout.shape.plane_bytes() as u64;
		}
		Ok(())
	}

	/// Reads plane `index` of `frame` into `out`, which holds one plane.
	pub(super) fn read(&self, layout: &StoreLayout, frame: usize, index: usize, out: &mut [u8]) -> io::Result<()> {
		let spill = self.file.get().expect("a stored frame outside memory is in the file");
		read_at(&spill.file, out, offset(spill, layout, frame) + (index * out.len()) as u64)
	}
}

fn offset(spill: &SpillFile, layout: &StoreLayout, frame: usize) -> u64 {
	HEADER_BYTES + layout.bytes_of(frame - spill.first.load(Ordering::Acquire))
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
	let pool = rayon::ThreadPoolBuilder::new()
		.num_threads(TOUCH_THREADS)
		.thread_name(|i| format!("frame-touch-{i}"))
		.build()
		.inspect_err(|e| tracing::warn!("the frame-touch pool did not start, pages are touched on one thread: {e}"));
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
	use crate::engine::FrameShape;

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
