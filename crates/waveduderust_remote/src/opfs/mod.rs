//! The frames past a store's memory ([`SpillBackend`]) in the spill worker (decision 0020), which keeps each store
//! generation in one file of this tab's session folder, `wavedude-spill/<session>/<store>-<generation>`. The page
//! posts each write with the frame's planes, each read and each removal ([`crate::spill`] has the messages), and
//! completes an operation's [`Token`] as the worker answers it.
//!
//! A tab holds the Web Lock of its session while it lives. At start the sessions whose lock is free (their tab is
//! gone) are removed. The free storage is asked for as each store opens, other tabs write too; a store's spill is
//! refused when its frames do not fit. The start, each write and each read call the page's wake as they complete.
//!
//! A session that the page cannot make or the worker cannot open, or a spill worker that does not start, stops or
//! speaks another protocol version, ends the worker and fails every operation in flight with the reason; the stores opened after drop their frames
//! past memory, said once as a warning.

#[cfg(target_arch = "wasm32")]
mod client;
#[cfg(any(test, target_arch = "wasm32"))]
mod requests;
#[cfg(target_arch = "wasm32")]
mod session;

#[cfg(target_arch = "wasm32")]
pub use client::Opfs;
