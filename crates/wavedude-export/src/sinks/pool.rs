//! The encoder threads of one export job, shared by its image sequences and movies, and the bounded task lanes
//! through which each sink uses them.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::{ExportError, Result};

/// At most this many encoder threads per job.
const MAX_THREADS: usize = 16;

/// A job's encoder pool, started at its first use; clones share it.
#[derive(Clone, Default)]
pub struct EncoderPool(Arc<Mutex<Option<Arc<ThreadPool>>>>);

impl EncoderPool {
	pub(crate) fn get(&self) -> Result<Arc<ThreadPool>> {
		let mut slot = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
		if let Some(pool) = slot.as_ref() {
			return Ok(pool.clone());
		}
		let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get).min(MAX_THREADS);
		let pool = ThreadPoolBuilder::new()
			.num_threads(threads)
			.thread_name(|i| format!("export encoder {i}"))
			.build()
			.map_err(|e| ExportError::Io(std::io::Error::other(e)))?;
		tracing::debug!(threads, "export encoder pool started");
		Ok(slot.insert(Arc::new(pool)).clone())
	}
}

/// A finished task: its sequence number, its outcome and its buffer.
pub(crate) struct Done<B> {
	pub seq: usize,
	pub result: Result<()>,
	pub buf: B,
}

/// One sink's tasks on the pool, each owning a recycled buffer, at most `limit` buffers at a time.
pub(crate) struct Lanes<B> {
	pool: Arc<ThreadPool>,
	tx: Sender<Done<B>>,
	rx: Receiver<Done<B>>,
	free: Vec<B>,
	allocated: usize,
	limit: usize,
	submitted: usize,
	in_flight: usize,
}

impl<B: Send + 'static> Lanes<B> {
	pub(super) fn new(pool: Arc<ThreadPool>) -> Self {
		let limit = pool.current_num_threads() + 1;
		let (tx, rx) = channel();
		Self { pool, tx, rx, free: Vec::new(), allocated: 0, limit, submitted: 0, in_flight: 0 }
	}

	/// A free buffer, a new one from `make` while under the limit; None when every buffer is in flight.
	pub(super) fn free(&mut self, make: impl FnOnce() -> B) -> Option<B> {
		self.free.pop().or_else(|| {
			(self.allocated < self.limit).then(|| {
				self.allocated += 1;
				make()
			})
		})
	}

	pub(super) fn recycle(&mut self, buf: B) {
		self.free.push(buf);
	}

	pub(super) fn submitted(&self) -> usize {
		self.submitted
	}

	/// Runs `task` on the pool with `buf`; returns its sequence number.
	pub(super) fn spawn(&mut self, mut buf: B, task: impl FnOnce(&mut B) -> Result<()> + Send + 'static) -> usize {
		let seq = self.submitted;
		self.submitted += 1;
		self.in_flight += 1;
		let tx = self.tx.clone();
		self.pool.spawn(move || {
			let result = catch_unwind(AssertUnwindSafe(|| task(&mut buf))).unwrap_or_else(|panic| {
				let message = panic
					.downcast_ref::<&str>()
					.map(|s| (*s).to_owned())
					.or_else(|| panic.downcast_ref::<String>().cloned())
					.unwrap_or_default();
				Err(ExportError::Panicked(message))
			});
			// The receiver is gone only when the sink was dropped unfinished.
			let _ = tx.send(Done { seq, result, buf });
		});
		seq
	}

	/// The next finished task; with `wait`, blocks while tasks are in flight. None when none is (or none has
	/// finished, without `wait`).
	pub(super) fn next_done(&mut self, wait: bool) -> Option<Done<B>> {
		if self.in_flight == 0 {
			return None;
		}
		let done = if wait { self.rx.recv().ok() } else { self.rx.try_recv().ok() }?;
		self.in_flight -= 1;
		Some(done)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_panicking_task_comes_back_as_an_error() {
		let mut lanes = Lanes::new(EncoderPool::default().get().unwrap());
		let buf = lanes.free(Vec::<u8>::new).unwrap();
		lanes.spawn(buf, |_| panic!("boom"));
		let done = lanes.next_done(true).expect("the task reports");
		assert!(matches!(done.result, Err(ExportError::Panicked(m)) if m == "boom"));
		assert!(lanes.next_done(true).is_none(), "nothing left in flight");
	}
}
