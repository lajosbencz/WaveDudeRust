//! Downloads in the status bar: a worker thread reports its bytes through a [`Download`], and the bar shows the
//! oldest running one until its worker drops it. Threads: [`start`] and [`shown`] on the UI thread.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
struct Counts {
	done: AtomicU64,
	/// 0 while the size is unknown.
	total: AtomicU64,
}

/// A running download's reporter, for its worker; dropping it ends the entry.
pub struct Download(Arc<Counts>);

impl Download {
	/// `bytes` more arrived of `total`, when known; the shape of the updater's and the codec's callbacks.
	pub fn chunk(&self, bytes: usize, total: Option<u64>) {
		self.0.done.fetch_add(bytes as u64, Ordering::Relaxed);
		if let Some(t) = total {
			self.0.total.store(t, Ordering::Relaxed);
		}
	}
}

thread_local! {
	static RUNNING: RefCell<Vec<(String, Arc<Counts>)>> = const { RefCell::new(Vec::new()) };
}

/// Lists `text` among the running downloads until the returned reporter is dropped.
pub fn start(text: String) -> Download {
	let counts = Arc::new(Counts::default());
	RUNNING.with_borrow_mut(|r| r.push((text, counts.clone())));
	Download(counts)
}

/// Drops finished downloads; the oldest running one with its share done, `None` while its size is unknown.
pub fn shown() -> Option<(String, Option<f32>)> {
	RUNNING.with_borrow_mut(|r| {
		r.retain(|(_, c)| Arc::strong_count(c) > 1);
		r.first().map(|(text, c)| {
			let (done, total) = (c.done.load(Ordering::Relaxed), c.total.load(Ordering::Relaxed));
			let share = (total != 0).then(|| (done as f64 / total as f64).min(1.0) as f32);
			(text.clone(), share)
		})
	})
}

/// Some download is running.
pub fn running() -> bool {
	RUNNING.with_borrow(|r| r.iter().any(|(_, c)| Arc::strong_count(c) > 1))
}
