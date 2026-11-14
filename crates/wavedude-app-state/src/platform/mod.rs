//! What differs between the desktop and the browser, chosen at compile time: `native` on every target but
//! wasm32, `web` there. Both expose the same items; building both targets proves it.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(any(target_arch = "wasm32", test))]
mod opfs;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
use native as imp;
#[cfg(target_arch = "wasm32")]
use web as imp;

pub use imp::{
	FileWatch, HAS_EXPORTS, HAS_PATHS, HAS_UPDATER, LogGuard, Store, init_log, install_panic_hook, machine_facts,
	take_crash_report, write,
};
/// The browser's spill of the frames past memory, natively the core's spill file; the browser's store of an
/// export's files.
#[cfg(target_arch = "wasm32")]
pub use opfs::{Opfs, Spool, SpoolReader};
/// `std`'s clocks on native targets, the browser's on wasm32, where `std`'s panic.
pub use web_time::{Duration, Instant, SystemTime};
