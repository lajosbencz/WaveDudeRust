//! Downloads in the status bar: a worker thread reports its bytes through a [`Download`], and the bar shows the
//! oldest running one until its worker drops it. Threads: [`start`] and [`poll`] on the UI thread.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use freya::prelude::*;
use wavedude_widgets::Progress;

use super::app::Cx;

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

/// Shows `text` in the status bar until the returned reporter is dropped.
pub fn start(cx: &Cx, text: String) -> Download {
	let counts = Arc::new(Counts::default());
	RUNNING.with_borrow_mut(|r| r.push((text, counts.clone())));
	poll(cx);
	Download(counts)
}

/// Drops finished downloads and shows the oldest running one; called every tick.
pub fn poll(cx: &Cx) {
	let shown = RUNNING.with_borrow_mut(|r| {
		r.retain(|(_, c)| Arc::strong_count(c) > 1);
		r.first().map(|(text, c)| {
			let (done, total) = (c.done.load(Ordering::Relaxed), c.total.load(Ordering::Relaxed));
			let progress = if total == 0 {
				Progress::Indeterminate
			} else {
				Progress::Determinate((done as f64 / total as f64).min(1.0) as f32)
			};
			(text.clone(), progress)
		})
	});
	let mut state = cx.ui.download;
	state.set_if_modified(shown);
}
