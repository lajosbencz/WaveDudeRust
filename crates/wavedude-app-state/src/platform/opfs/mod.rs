//! The origin private file system, written and read from the main thread: this tab's session folder
//! `wavedude-spill/<session>/` holds the frames past a store's memory ([`Opfs`]) and the exports' files
//! ([`Spool`]).
//!
//! A tab holds the Web Lock of its session while it lives. At start the sessions whose lock is free (their tab
//! is gone) are removed, then the free storage is estimated once.

mod queue;
#[cfg(target_arch = "wasm32")]
mod session;
#[cfg(target_arch = "wasm32")]
mod spill;
#[cfg(target_arch = "wasm32")]
mod spool;

#[cfg(target_arch = "wasm32")]
pub use spill::Opfs;
#[cfg(target_arch = "wasm32")]
pub use spool::{Spool, SpoolReader};
