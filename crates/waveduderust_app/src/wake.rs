//! Wakes the window when news arrives from another thread (a log line, a watched file, the run's state), so it
//! sleeps while nothing moves.

use std::sync::OnceLock;

static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Sets what wakes the window; the first one set stays.
pub fn set(wake: impl Fn() + Send + Sync + 'static) {
	let _ = WAKE.set(Box::new(wake));
}

/// Wakes the window, once [`set`] said how.
pub fn wake() {
	if let Some(wake) = WAKE.get() {
		wake();
	}
}
