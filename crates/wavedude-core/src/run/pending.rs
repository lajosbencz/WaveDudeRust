//! A spill whose reads and writes complete later (the browser's origin private file system): the store starts
//! them, gets a [`Token`] for each and settles them as they complete (store.rs has the store's side).

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::sync::{MaybeSend, lock};

use super::store::StoreLayout;

/// Bytes of the writes in flight a spill allows ([`StoreLayout::writes_in_flight`]): the browser's frames posted to
/// the page and not acknowledged. As many as the GPU engine's capture ring stages
/// (docs/measurements.md, The browser's live run in a worker).
pub const WRITE_BYTES_IN_FLIGHT: u64 = 64 << 20;
/// Writes in flight at most: the GPU engine's largest capture ring.
pub(super) const MOST_WRITES_IN_FLIGHT: usize = 16;
/// Frames read back and kept: the frame asked for last and the frames read ahead for playback
/// ([`PendingSpill::prefetch`]), or for a scrub to and fro.
const LOADED: usize = 5;
/// Reads in flight at most; a frame asked for beyond them waits in one slot, the newest asked for.
const READS_IN_FLIGHT: usize = 2;
/// Frames read ahead, the playhead's and the 3 after it: the room [`LOADED`] leaves beside the frame asked for last.
pub(super) const READ_AHEAD: usize = LOADED - 1;

/// An operation in flight: completed once by its backend, polled by the store.
pub struct Token<T>(Arc<Mutex<Slot<T>>>);

struct Slot<T> {
	result: Option<io::Result<T>>,
	done: bool,
	waker: Option<Waker>,
}

impl<T> Clone for Token<T> {
	fn clone(&self) -> Self {
		Self(self.0.clone())
	}
}

impl<T> Default for Token<T> {
	fn default() -> Self {
		Self(Arc::new(Mutex::new(Slot { result: None, done: false, waker: None })))
	}
}

impl<T> Token<T> {
	/// Sets the outcome; a second call is ignored.
	pub fn complete(&self, result: io::Result<T>) {
		let mut slot = lock(&self.0);
		if slot.done {
			return;
		}
		slot.result = Some(result);
		slot.done = true;
		if let Some(waker) = slot.waker.take() {
			waker.wake();
		}
	}

	pub fn is_done(&self) -> bool {
		lock(&self.0).done
	}

	/// The outcome, once: `None` while in flight or after it was taken.
	pub fn take(&self) -> Option<io::Result<T>> {
		lock(&self.0).result.take()
	}

	/// Ready once the operation completed, its outcome left in place; otherwise `cx`'s waker is woken then.
	pub fn poll_done(&self, cx: &mut Context<'_>) -> Poll<()> {
		let mut slot = lock(&self.0);
		if slot.done {
			return Poll::Ready(());
		}
		slot.waker = Some(cx.waker().clone());
		Poll::Pending
	}

	/// Resolves when the operation completed, its outcome left in place.
	pub fn done(&self) -> impl Future<Output = ()> + use<T> {
		let token = self.clone();
		std::future::poll_fn(move |cx| token.poll_done(cx))
	}
}

/// One store's frames outside its memory. Dropping it removes them, after the writes in flight.
pub trait SpillBackend: Send + Sync {
	/// Starts writing frame `frame` of `generation`: `planes` in layer order, copied before the call returns.
	fn write(&self, generation: u64, frame: usize, planes: &[&[u8]]) -> Token<()>;
	/// Starts reading frame `frame` of `generation`, its planes in layer order.
	fn read(&self, generation: u64, frame: usize) -> Token<Vec<u8>>;
	/// Removes the frames of `generation`, after its writes in flight.
	fn drop_generation(&self, generation: u64);
}

/// The pumped worker's spills.
pub trait SpillFactory: MaybeSend {
	/// Ready once [`open`](Self::open) can answer: the free storage is known; otherwise `cx`'s waker is woken then.
	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()>;
	/// A spill for `bytes` of frames of store `store` (its [`FrameStore::id`](super::FrameStore::id)) laid out
	/// as `layout`; refused when they do not fit the free storage or there is none.
	fn open(&self, store: u64, layout: &StoreLayout, bytes: u64) -> io::Result<Box<dyn SpillBackend>>;
}

/// The store's side of a [`SpillBackend`]: the operations in flight and the frames read back.
pub(super) struct PendingSpill {
	backend: Box<dyn SpillBackend>,
	/// Writes in flight at which the spill is busy.
	most_writes: usize,
	state: Mutex<State>,
}

#[derive(Default)]
struct State {
	/// (generation, frame, write).
	writes: Vec<(u64, usize, Token<()>)>,
	/// A written frame kept in memory while it is the frame written last.
	held: Option<usize>,
	/// (generation, frame, read), at most [`READS_IN_FLIGHT`].
	reads: Vec<(u64, usize, Token<Vec<u8>>)>,
	/// The frame asked for while the reads were all in flight: (generation, frame).
	queued: Option<(u64, usize)>,
	/// The frame asked for last: (generation, frame).
	asked: Option<(u64, usize)>,
	/// The failed read of a frame while it was the one asked for last: (generation, frame), the error.
	failed: Option<((u64, usize), io::Error)>,
	/// The frames to read ahead, soonest first, at most [`READ_AHEAD`]: (generation, frame).
	ahead: Vec<(u64, usize)>,
	/// (generation, frame, planes), newest last, at most [`LOADED`].
	loaded: VecDeque<(u64, usize, Arc<Vec<u8>>)>,
}

impl PendingSpill {
	pub(super) fn new(backend: Box<dyn SpillBackend>, most_writes: usize) -> Self {
		Self { backend, most_writes, state: Mutex::default() }
	}

	pub(super) fn write(&self, generation: u64, frame: usize, planes: &[&[u8]]) {
		let token = self.backend.write(generation, frame, planes);
		lock(&self.state).writes.push((generation, frame, token));
	}

	pub(super) fn busy(&self) -> bool {
		lock(&self.state).writes.len() >= self.most_writes
	}

	pub(super) fn writing(&self) -> bool {
		!lock(&self.state).writes.is_empty()
	}

	/// Ready once a write in flight completed, or none is; otherwise `cx`'s waker is woken when one completes.
	pub(super) fn poll_write(&self, cx: &mut Context<'_>) -> Poll<()> {
		let state = lock(&self.state);
		let mut done = state.writes.is_empty();
		for (.., token) in &state.writes {
			done |= token.poll_done(cx).is_ready();
		}
		if done { Poll::Ready(()) } else { Poll::Pending }
	}

	/// Takes the completed writes; the frames of `generation` written whose memory is free now, all but `last`
	/// (the frame written last). A failed write of `generation` is the error; an older generation's is dropped.
	pub(super) fn settle(&self, generation: u64, last: usize) -> io::Result<Vec<usize>> {
		let mut state = lock(&self.state);
		let mut freed = Vec::new();
		let mut failed = None;
		let mut held = state.held.take();
		state.writes.retain(|(g, frame, token)| {
			let Some(result) = token.take() else { return true };
			match result {
				Err(e) if *g == generation => {
					failed.get_or_insert(e);
				}
				Err(e) => tracing::debug!(generation = g, frame, "a write of a cleared generation failed: {e}"),
				Ok(()) if *g != generation => {}
				Ok(()) if *frame == last => freed.extend(held.replace(*frame)),
				Ok(()) => freed.push(*frame),
			}
			false
		});
		if let Some(h) = held.filter(|&h| h != last) {
			freed.push(h);
			held = None;
		}
		state.held = held;
		match failed {
			Some(e) => Err(e),
			None => Ok(freed),
		}
	}

	/// Frame `frame` of `generation` read back: `None` while its read is in flight or queued (started or queued by
	/// the first call, a queued frame superseded by a newer one asked for).
	pub(super) fn read(&self, generation: u64, frame: usize) -> Option<io::Result<Arc<Vec<u8>>>> {
		let mut state = lock(&self.state);
		let wanted = (generation, frame);
		state.asked = Some(wanted);
		state.land();
		if let Some((key, e)) = state.failed.take()
			&& key == wanted
		{
			return Some(Err(e));
		}
		if let Some((.., planes)) = state.loaded.iter().find(|(g, f, _)| (*g, *f) == wanted) {
			return Some(Ok(planes.clone()));
		}
		if !state.reads.iter().any(|(g, f, _)| (*g, *f) == wanted) {
			state.queued = Some(wanted);
		}
		self.start_reads(&mut state);
		None
	}

	/// Reads the first [`READ_AHEAD`] of `frames` of `generation` ahead, soonest first, after a frame asked for;
	/// they are kept while ahead, as is the frame asked for last.
	pub(super) fn prefetch(&self, generation: u64, frames: &[usize]) {
		let mut state = lock(&self.state);
		state.ahead = frames.iter().take(READ_AHEAD).map(|&f| (generation, f)).collect();
		state.land();
		self.start_reads(&mut state);
	}

	/// Starts reads up to [`READS_IN_FLIGHT`]: the frame queued first, then the frames ahead not kept or in flight.
	fn start_reads(&self, state: &mut State) {
		while state.reads.len() < READS_IN_FLIGHT {
			let next = state.queued.take().or_else(|| {
				state.ahead.iter().copied().find(|&key| {
					!state.reads.iter().any(|(g, f, _)| (*g, *f) == key)
						&& !state.loaded.iter().any(|(g, f, _)| (*g, *f) == key)
				})
			});
			let Some((g, f)) = next else { return };
			let token = self.backend.read(g, f);
			state.reads.push((g, f, token));
		}
	}

	/// Reads in flight or queued, and frames kept.
	#[cfg(test)]
	pub(super) fn held(&self) -> (usize, usize) {
		let state = lock(&self.state);
		(state.reads.len() + usize::from(state.queued.is_some()), state.loaded.len())
	}

	/// The store was cleared: `generation`'s frames go, the frames read back with them.
	pub(super) fn cleared(&self, generation: u64) {
		let mut state = lock(&self.state);
		state.held = None;
		state.reads.clear();
		state.queued = None;
		state.asked = None;
		state.failed = None;
		state.ahead.clear();
		state.loaded.clear();
		drop(state);
		self.backend.drop_generation(generation);
	}
}

impl State {
	/// Keeps the reads completed, each pushing out the oldest frame kept that is neither asked for last nor
	/// ahead; the failure of the frame asked for last is kept for its next read, others are logged.
	fn land(&mut self) {
		let mut at = 0;
		while at < self.reads.len() {
			let Some(result) = self.reads[at].2.take() else {
				at += 1;
				continue;
			};
			let (g, f, _) = self.reads.swap_remove(at);
			match result {
				Ok(planes) => {
					self.loaded.push_back((g, f, Arc::new(planes)));
					if self.loaded.len() > LOADED {
						let kept = |key: &(u64, usize)| self.asked == Some(*key) || self.ahead.contains(key);
						let out = self
							.loaded
							.iter()
							.position(|(g, f, _)| !kept(&(*g, *f)))
							.expect("the frame asked for last and the frames ahead leave a frame to push out");
						self.loaded.remove(out);
					}
				}
				Err(e) if self.asked == Some((g, f)) => self.failed = Some(((g, f), e)),
				Err(e) => tracing::debug!(frame = f, "a spilled frame not asked for was not read back: {e}"),
			}
		}
	}
}

#[cfg(test)]
pub(crate) mod fake {
	//! A backend whose operations complete when the test says.

	use super::*;
	use std::collections::HashMap;

	/// The frames, the operations in flight and the generations dropped, shared with the test.
	#[derive(Default)]
	pub(crate) struct Disk {
		pub(crate) frames: HashMap<(u64, usize), Vec<u8>>,
		pub(crate) writes: Vec<(u64, usize, Vec<u8>, Token<()>)>,
		pub(crate) reads: Vec<(u64, usize, Token<Vec<u8>>)>,
		pub(crate) dropped: Vec<u64>,
		/// Writes fail with this kind.
		pub(crate) fail: Option<io::ErrorKind>,
	}

	impl Disk {
		/// Completes every operation in flight.
		pub(crate) fn complete(&mut self) {
			for (g, frame, planes, token) in self.writes.drain(..) {
				match self.fail {
					Some(kind) => token.complete(Err(kind.into())),
					None => {
						self.frames.insert((g, frame), planes);
						token.complete(Ok(()));
					}
				}
			}
			for (g, frame, token) in self.reads.drain(..) {
				let found = self.frames.get(&(g, frame)).cloned();
				token.complete(found.ok_or_else(|| io::ErrorKind::NotFound.into()));
			}
		}
	}

	pub(crate) struct Backend(pub(crate) Arc<Mutex<Disk>>);

	impl SpillBackend for Backend {
		fn write(&self, generation: u64, frame: usize, planes: &[&[u8]]) -> Token<()> {
			let token = Token::default();
			lock(&self.0).writes.push((generation, frame, planes.concat(), token.clone()));
			token
		}

		fn read(&self, generation: u64, frame: usize) -> Token<Vec<u8>> {
			let token = Token::default();
			lock(&self.0).reads.push((generation, frame, token.clone()));
			token
		}

		fn drop_generation(&self, generation: u64) {
			let mut disk = lock(&self.0);
			disk.frames.retain(|(g, _), _| *g != generation);
			disk.dropped.push(generation);
		}
	}

	/// Spills into one shared [`Disk`] holding at most `free` bytes; ready once `ready` is set, which wakes no one.
	pub(crate) struct Factory {
		pub(crate) disk: Arc<Mutex<Disk>>,
		pub(crate) free: u64,
		pub(crate) ready: Arc<std::sync::atomic::AtomicBool>,
	}

	impl Factory {
		pub(crate) fn ample() -> Self {
			Self { disk: Arc::default(), free: u64::MAX, ready: Arc::new(true.into()) }
		}
	}

	impl SpillFactory for Factory {
		fn poll_ready(&self, _: &mut Context<'_>) -> Poll<()> {
			if self.ready.load(std::sync::atomic::Ordering::Acquire) { Poll::Ready(()) } else { Poll::Pending }
		}

		fn open(&self, _store: u64, _layout: &StoreLayout, bytes: u64) -> io::Result<Box<dyn SpillBackend>> {
			if bytes > self.free {
				return Err(io::Error::new(io::ErrorKind::StorageFull, format!("{bytes} > {}", self.free)));
			}
			Ok(Box::new(Backend(self.disk.clone())))
		}
	}
}
