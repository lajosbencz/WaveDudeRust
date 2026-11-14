//! The writes of a spool's files, queued for the task of each file that posts them in order: the bytes not written
//! yet, the files closed and not committed yet, the first failure, which every later call returns.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::task::{Poll, Waker};

/// A write joins the one queued before it when it starts where that one ends and both stay within this many bytes.
const MERGED: usize = 4 << 20;

/// What a file's task does next.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Next {
	/// Writes `data` at `at`; [`Queue::written`] once done.
	Write { at: u64, data: Vec<u8> },
	/// Commits the file.
	Close,
	/// Drops the file uncommitted: it was discarded or the queue failed.
	Abort,
}

#[derive(Default)]
struct File {
	writes: VecDeque<(u64, Vec<u8>)>,
	closed: bool,
	/// The close was handed to the task.
	closing: bool,
	discarded: bool,
	task: Option<Waker>,
}

impl File {
	fn wake(&mut self) {
		if let Some(task) = self.task.take() {
			task.wake();
		}
	}
}

/// See the module docs.
pub(super) struct Queue {
	/// Bytes not written at which [`poll_ready`](Self::poll_ready) is Pending.
	cap: u64,
	files: BTreeMap<u64, File>,
	next_id: u64,
	in_flight: u64,
	/// Files closed whose task did not finish.
	unflushed: usize,
	failure: Option<(io::ErrorKind, String)>,
}

impl Queue {
	pub(super) fn new(cap: u64) -> Self {
		Self { cap, files: BTreeMap::new(), next_id: 0, in_flight: 0, unflushed: 0, failure: None }
	}

	fn check(&self) -> io::Result<()> {
		match &self.failure {
			Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
			None => Ok(()),
		}
	}

	fn file(&mut self, id: u64) -> io::Result<&mut File> {
		self.check()?;
		self.files
			.get_mut(&id)
			.filter(|f| !f.closed && !f.discarded)
			.ok_or_else(|| io::Error::other("the file is closed"))
	}

	/// A new file; its id.
	pub(super) fn open(&mut self) -> io::Result<u64> {
		self.check()?;
		let id = self.next_id;
		self.next_id += 1;
		self.files.insert(id, File::default());
		Ok(id)
	}

	/// Queues `data` at `at` of file `id`.
	pub(super) fn write(&mut self, id: u64, at: u64, data: &[u8]) -> io::Result<()> {
		if data.is_empty() {
			return self.check();
		}
		let file = self.file(id)?;
		match file.writes.back_mut() {
			Some((start, queued)) if *start + queued.len() as u64 == at && queued.len() + data.len() <= MERGED => {
				queued.extend_from_slice(data);
			}
			_ => file.writes.push_back((at, data.to_vec())),
		}
		file.wake();
		self.in_flight += data.len() as u64;
		Ok(())
	}

	/// Queues the commit of file `id` after its writes.
	pub(super) fn close(&mut self, id: u64) -> io::Result<()> {
		let file = self.file(id)?;
		file.closed = true;
		file.wake();
		self.unflushed += 1;
		Ok(())
	}

	/// File `id` is dropped unclosed: its writes queued go, its task aborts.
	pub(super) fn discard(&mut self, id: u64) {
		if let Some(file) = self.files.get_mut(&id) {
			file.discarded = true;
			file.wake();
		}
	}

	/// The next step of file `id`'s task; Pending until one is queued, `task` woken then.
	pub(super) fn poll_next(&mut self, id: u64, task: &Waker) -> Poll<Next> {
		let failed = self.failure.is_some();
		let Some(file) = self.files.get_mut(&id) else { return Poll::Ready(Next::Abort) };
		if failed || file.discarded {
			let dropped: u64 = file.writes.drain(..).map(|(_, data)| data.len() as u64).sum();
			self.in_flight -= dropped;
			return Poll::Ready(Next::Abort);
		}
		if let Some((at, data)) = file.writes.pop_front() {
			return Poll::Ready(Next::Write { at, data });
		}
		if file.closed && !file.closing {
			file.closing = true;
			return Poll::Ready(Next::Close);
		}
		file.task = Some(task.clone());
		Poll::Pending
	}

	/// A write of `bytes` handed out by [`poll_next`](Self::poll_next) completed, or failed.
	pub(super) fn written(&mut self, bytes: usize) {
		self.in_flight -= bytes as u64;
	}

	/// File `id`'s task ended, with `error` when it failed.
	pub(super) fn finished(&mut self, id: u64, error: Option<&io::Error>) {
		if let Some(file) = self.files.remove(&id) {
			self.in_flight -= file.writes.iter().map(|(_, data)| data.len() as u64).sum::<u64>();
			if file.closed {
				self.unflushed -= 1;
			}
		}
		if let Some(e) = error {
			self.fail(e);
		}
	}

	/// Fails the queue with `e` unless it failed already; every file's task aborts.
	pub(super) fn fail(&mut self, e: &io::Error) {
		if self.failure.is_some() {
			return;
		}
		self.failure = Some((e.kind(), e.to_string()));
		for file in self.files.values_mut() {
			file.wake();
		}
	}

	/// Ready while fewer bytes than the cap are not written.
	pub(super) fn poll_ready(&self) -> io::Result<Poll<()>> {
		self.check()?;
		Ok(if self.in_flight < self.cap { Poll::Ready(()) } else { Poll::Pending })
	}

	/// Ready once every closed file is committed.
	pub(super) fn poll_flushed(&self) -> io::Result<Poll<()>> {
		self.check()?;
		Ok(if self.unflushed == 0 { Poll::Ready(()) } else { Poll::Pending })
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::task::Wake;

	use super::*;

	#[derive(Default)]
	struct Count(AtomicUsize);

	impl Wake for Count {
		fn wake(self: Arc<Self>) {
			self.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	fn waker() -> (Arc<Count>, Waker) {
		let count = Arc::new(Count::default());
		(count.clone(), Waker::from(count))
	}

	fn next(queue: &mut Queue, id: u64) -> Poll<Next> {
		queue.poll_next(id, Waker::noop())
	}

	fn write(at: u64, data: &[u8]) -> Poll<Next> {
		Poll::Ready(Next::Write { at, data: data.to_vec() })
	}

	#[test]
	fn the_bytes_not_written_hold_the_writer_back_at_the_cap() {
		let mut queue = Queue::new(8);
		let id = queue.open().unwrap();
		queue.write(id, 0, &[1; 5]).unwrap();
		assert_eq!(queue.poll_ready().unwrap(), Poll::Ready(()));
		queue.write(id, 10, &[2; 3]).unwrap();
		assert_eq!(queue.poll_ready().unwrap(), Poll::Pending, "8 bytes queued");
		assert_eq!(next(&mut queue, id), write(0, &[1; 5]));
		assert_eq!(queue.poll_ready().unwrap(), Poll::Pending, "a write handed out counts until it completes");
		queue.written(5);
		assert_eq!(queue.poll_ready().unwrap(), Poll::Ready(()));
		assert_eq!(next(&mut queue, id), write(10, &[2; 3]));
		queue.written(3);
		assert_eq!(queue.in_flight, 0);
	}

	#[test]
	fn contiguous_writes_join_and_a_seek_starts_another() {
		let mut queue = Queue::new(u64::MAX);
		let id = queue.open().unwrap();
		queue.write(id, 0, b"ab").unwrap();
		queue.write(id, 2, b"cd").unwrap();
		queue.write(id, 1, b"X").unwrap();
		queue.write(id, 2, b"Y").unwrap();
		queue.write(id, 9, &[]).unwrap();
		assert_eq!(next(&mut queue, id), write(0, b"abcd"));
		assert_eq!(next(&mut queue, id), write(1, b"XY"), "the write after the seek back stays after the one before");
		assert_eq!(next(&mut queue, id), Poll::Pending, "no empty write");
		queue.write(id, 3, b"e").unwrap();
		assert_eq!(next(&mut queue, id), write(3, b"e"), "a write handed out takes no more");

		queue.write(id, 0, &vec![0; MERGED]).unwrap();
		queue.write(id, MERGED as u64, b"f").unwrap();
		assert!(matches!(next(&mut queue, id), Poll::Ready(Next::Write { data, .. }) if data.len() == MERGED));
		assert_eq!(next(&mut queue, id), write(MERGED as u64, b"f"), "joined writes stay within {MERGED} bytes");
	}

	#[test]
	fn a_file_is_flushed_once_its_task_committed_it() {
		let mut queue = Queue::new(u64::MAX);
		let (open, closed) = (queue.open().unwrap(), queue.open().unwrap());
		queue.write(open, 0, b"a").unwrap();
		assert_eq!(queue.poll_flushed().unwrap(), Poll::Ready(()), "an open file is not waited for");
		queue.write(closed, 0, b"b").unwrap();
		queue.close(closed).unwrap();
		assert!(queue.write(closed, 1, b"c").is_err(), "a closed file takes no more");
		assert_eq!(queue.poll_flushed().unwrap(), Poll::Pending);
		assert_eq!(next(&mut queue, closed), write(0, b"b"));
		queue.written(1);
		assert_eq!(next(&mut queue, closed), Poll::Ready(Next::Close));
		assert_eq!(next(&mut queue, closed), Poll::Pending, "the close is handed out once");
		assert_eq!(queue.poll_flushed().unwrap(), Poll::Pending, "until the task commits it");
		queue.finished(closed, None);
		assert_eq!(queue.poll_flushed().unwrap(), Poll::Ready(()));
	}

	#[test]
	fn the_first_failure_sticks_and_aborts_every_file() {
		let mut queue = Queue::new(4);
		let (failing, other) = (queue.open().unwrap(), queue.open().unwrap());
		queue.write(other, 0, b"abcd").unwrap();
		queue.close(other).unwrap();
		let (count, task) = waker();
		assert_eq!(queue.poll_next(failing, &task), Poll::Pending);
		queue.finished(failing, Some(&io::Error::new(io::ErrorKind::QuotaExceeded, "full")));
		queue.fail(&io::Error::other("later"));
		for e in [queue.poll_ready().unwrap_err(), queue.poll_flushed().unwrap_err(), queue.open().unwrap_err()] {
			assert_eq!((e.kind(), e.to_string()), (io::ErrorKind::QuotaExceeded, "full".into()), "the first failure");
		}
		assert!(queue.write(other, 4, b"e").is_err() && queue.close(other).is_err());
		assert_eq!(next(&mut queue, other), Poll::Ready(Next::Abort));
		assert_eq!(queue.in_flight, 0, "the aborted file's writes are not waited for");
		queue.finished(other, None);
		assert_eq!(queue.unflushed, 0);
		assert_eq!(count.0.load(Ordering::SeqCst), 0, "the failed file's own task ended");
	}

	#[test]
	fn a_failure_wakes_the_tasks_waiting() {
		let mut queue = Queue::new(u64::MAX);
		let id = queue.open().unwrap();
		let (count, task) = waker();
		assert_eq!(queue.poll_next(id, &task), Poll::Pending);
		queue.write(id, 0, b"a").unwrap();
		assert_eq!(count.0.load(Ordering::SeqCst), 1, "a write wakes the task");
		assert_eq!(queue.poll_next(id, &task), write(0, b"a"));
		assert_eq!(queue.poll_next(id, &task), Poll::Pending);
		queue.fail(&io::Error::other("removed"));
		assert_eq!(count.0.load(Ordering::SeqCst), 2);
		assert_eq!(queue.poll_next(id, &task), Poll::Ready(Next::Abort));
	}

	#[test]
	fn a_discarded_file_drops_its_writes() {
		let mut queue = Queue::new(u64::MAX);
		let id = queue.open().unwrap();
		queue.write(id, 0, b"abc").unwrap();
		queue.discard(id);
		assert!(queue.write(id, 3, b"d").is_err());
		assert_eq!(next(&mut queue, id), Poll::Ready(Next::Abort));
		assert_eq!(queue.in_flight, 0);
		queue.finished(id, None);
		assert_eq!(queue.poll_flushed().unwrap(), Poll::Ready(()), "a discarded file is no failure");
	}
}
