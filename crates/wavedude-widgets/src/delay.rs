//! A deadline that each restart pushes out: a tooltip's show delay, a pointer resting on a handle.

use std::time::Duration;

use freya::prelude::*;

/// What runs when the deadline passes; a restart or a cancel before then drops it.
#[derive(Clone, Copy)]
pub struct Delayed {
	pending: State<Option<TaskHandle>>,
}

pub fn use_delayed() -> Delayed {
	Delayed { pending: use_state(|| None) }
}

impl Delayed {
	/// Runs `then` once `delay` has passed, in place of whatever was waiting.
	pub fn restart(&mut self, delay: Duration, then: impl FnOnce() + 'static) {
		self.cancel();
		let task = spawn(async move {
			timer(delay).await;
			then();
		});
		self.pending.set(Some(task));
	}

	pub fn cancel(&mut self) {
		if let Some(task) = self.pending.write().take() {
			task.cancel();
		}
	}
}
