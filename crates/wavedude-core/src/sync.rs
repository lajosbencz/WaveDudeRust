//! The one way to take a mutex: a lock poisoned by a panicking thread is taken anyway, since every guarded value
//! here is whole between operations and the panic is reported where it happened. Also the bounds that are `Send`
//! and `Sync` only where threads exist, and the flag a callback sets for a future to wait on.

#[cfg(feature = "gpu")]
use std::sync::OnceLock;
use std::sync::{Mutex, MutexGuard, PoisonError};
#[cfg(feature = "gpu")]
use std::task::Waker;
use std::task::{Context, Poll};

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `Send` where threads exist; nothing on wasm32, where wgpu's types are not `Send`.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}

/// `Send` where threads exist; nothing on wasm32, where wgpu's types are not `Send`.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// `Sync` where threads exist; nothing on wasm32, where the browser's types are not `Sync`.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync> MaybeSync for T {}

/// `Sync` where threads exist; nothing on wasm32, where the browser's types are not `Sync`.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSync for T {}

#[cfg(feature = "gpu")]
/// A value set once, possibly from a callback on another thread, that a future can wait for.
pub(crate) struct Notify<T> {
	value: OnceLock<T>,
	waker: Mutex<Option<Waker>>,
}

#[cfg(feature = "gpu")]
impl<T> Default for Notify<T> {
	fn default() -> Self {
		Self { value: OnceLock::new(), waker: Mutex::new(None) }
	}
}

#[cfg(feature = "gpu")]
impl<T> Notify<T> {
	/// Keeps the first value and wakes the waiter.
	pub(crate) fn set(&self, value: T) {
		let _ = self.value.set(value);
		let waker = lock(&self.waker).take();
		if let Some(waker) = waker {
			waker.wake();
		}
	}

	pub(crate) fn get(&self) -> Option<&T> {
		self.value.get()
	}
}

/// What a waiting future sees of a [`Notify`]: whether it was set, whatever it holds.
pub(crate) trait Readiness: Send + Sync {
	/// Ready once set; otherwise `cx`'s waker is woken by the `set`.
	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()>;
}

#[cfg(feature = "gpu")]
impl<T: Send + Sync> Readiness for Notify<T> {
	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
		// The waker goes in before the value is read, so a `set` in between still wakes it.
		*lock(&self.waker) = Some(cx.waker().clone());
		if self.value.get().is_some() { Poll::Ready(()) } else { Poll::Pending }
	}
}

#[cfg(all(test, feature = "gpu"))]
mod tests {
	use super::*;
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::task::Wake;

	struct Counter(AtomicUsize);

	impl Wake for Counter {
		fn wake(self: Arc<Self>) {
			self.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	#[test]
	fn a_notify_wakes_its_waiter_once_set() {
		let notify = Notify::<u32>::default();
		let counter = Arc::new(Counter(AtomicUsize::new(0)));
		let waker = Waker::from(counter.clone());
		let mut cx = Context::from_waker(&waker);
		assert!(notify.poll_ready(&mut cx).is_pending());
		notify.set(7);
		notify.set(8);
		assert_eq!(counter.0.load(Ordering::SeqCst), 1);
		assert!(notify.poll_ready(&mut cx).is_ready());
		assert_eq!(notify.get(), Some(&7));
	}
}
