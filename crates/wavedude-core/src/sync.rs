//! The one way to take a mutex: a lock poisoned by a panicking thread is taken anyway, since every guarded value
//! here is whole between operations and the panic is reported where it happened.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(PoisonError::into_inner)
}
