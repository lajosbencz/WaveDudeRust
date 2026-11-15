//! Answers taken on a later frame: from worker threads natively, from the browser's callbacks. A worker that panics
//! or does not start answers as its owner says (docs/conventions.md, Logging); the panic hook logs the panic.
//!
//! Threads: one per `Inbox::spawn`; the window takes the answers.

use std::sync::mpsc::{Receiver, Sender, TryIter, channel};

/// The answers of the threads spawned through it.
pub(super) struct Inbox<T> {
	tx: Sender<T>,
	rx: Receiver<T>,
}

impl<T> Default for Inbox<T> {
	fn default() -> Self {
		let (tx, rx) = channel();
		Self { tx, rx }
	}
}

impl<T> Inbox<T> {
	/// A sender of answers, for callbacks.
	pub(super) fn sender(&self) -> Sender<T> {
		self.tx.clone()
	}

	/// The answers that arrived.
	pub(super) fn try_iter(&self) -> TryIter<'_, T> {
		self.rx.try_iter()
	}
}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + 'static> Inbox<T> {
	/// Runs `work` on the thread `name`; it may post through the sender it gets, and its answer, if any, arrives
	/// last. When it panics or does not start, `failed` answers with why. The window repaints after the answer.
	pub(super) fn spawn<F>(
		&self,
		ctx: &eframe::egui::Context,
		name: &str,
		work: impl FnOnce(&Sender<T>) -> Option<T> + Send + 'static,
		failed: F,
	) where
		F: FnOnce(String) -> Option<T> + Send + 'static,
	{
		let end = End { tx: self.tx.clone(), ctx: ctx.clone(), thread: name.to_owned(), failed: Some(failed) };
		let started = std::thread::Builder::new().name(name.into()).spawn(move || {
			let mut end = end;
			let answer = work(&end.tx);
			end.failed = None;
			if let Some(answer) = answer {
				// the window may have closed meanwhile
				let _ = end.tx.send(answer);
			}
		});
		// the dropped worker's `End` answers
		if let Err(e) = started {
			tracing::error!("the {name} thread could not start: {e}");
		}
	}
}

/// Repaints the window when a worker ends; answers through `failed` when it ends without its answer.
#[cfg(not(target_arch = "wasm32"))]
struct End<T, F: FnOnce(String) -> Option<T>> {
	tx: Sender<T>,
	ctx: eframe::egui::Context,
	thread: String,
	failed: Option<F>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<T, F: FnOnce(String) -> Option<T>> Drop for End<T, F> {
	fn drop(&mut self) {
		if let Some(failed) = self.failed.take() {
			let why = if std::thread::panicking() { "panicked" } else { "did not start" };
			if let Some(answer) = failed(format!("the {} thread {why}", self.thread)) {
				// the window may have closed meanwhile
				let _ = self.tx.send(answer);
			}
		}
		self.ctx.request_repaint();
	}
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use eframe::egui;

	use super::*;

	fn answer(inbox: &Inbox<String>) -> String {
		inbox.rx.recv_timeout(Duration::from_secs(10)).expect("an answer")
	}

	#[test]
	fn a_worker_answers_after_its_posts() {
		let inbox = Inbox::default();
		inbox.spawn(
			&egui::Context::default(),
			"test",
			|tx| {
				tx.send("post".into()).unwrap();
				Some("done".into())
			},
			Some,
		);
		assert_eq!((answer(&inbox), answer(&inbox)), ("post".into(), "done".into()));
	}

	#[test]
	fn a_worker_that_panics_answers_as_failed() {
		let inbox = Inbox::default();
		inbox.spawn(&egui::Context::default(), "test", |_| panic!("the worker failed"), Some);
		assert_eq!(answer(&inbox), "the test thread panicked");
	}
}
