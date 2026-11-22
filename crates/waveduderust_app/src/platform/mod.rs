//! What differs between the desktop and the browser, chosen at compile time: `native` on every target but
//! wasm32, `web` there. Both expose the same items; building both targets proves it.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(any(target_arch = "wasm32", test))]
mod opfs;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(any(target_arch = "wasm32", test))]
mod webcodecs;

#[cfg(not(target_arch = "wasm32"))]
use native as imp;
#[cfg(target_arch = "wasm32")]
use web as imp;

pub use imp::{
	FileWatch, HAS_PATHS, LogGuard, Store, init_log, install_panic_hook, machine_facts, open, take_crash_report,
	touch_first, write,
};

/// Selects `path` in its folder; a failure is logged.
#[cfg(not(target_arch = "wasm32"))]
pub fn show_in_folder(path: &std::path::Path) {
	if let Err(e) = open(Opened::Reveal(path)) {
		tracing::warn!("cannot show {} in its folder: {e}", path.display());
	}
}

/// What [`open`] hands to the system: a web address, or natively a file or folder, opened or revealed in its folder.
#[derive(Clone, Copy, Debug)]
pub enum Opened<'a> {
	Url(&'a str),
	#[cfg(not(target_arch = "wasm32"))]
	Path(&'a std::path::Path),
	/// A file or folder selected in its folder.
	#[cfg(not(target_arch = "wasm32"))]
	Reveal(&'a std::path::Path),
}
#[cfg(not(target_arch = "wasm32"))]
pub use native::memory_readings;
/// The browser's spill of the frames past memory, natively the core's spill file; the browser's store of an
/// export's files and a file ready for its download.
#[cfg(target_arch = "wasm32")]
pub use opfs::{Opfs, Spool, SpoolReader};
#[cfg(target_arch = "wasm32")]
pub use web::{Download, device_memory};
/// `std`'s clocks on native targets, the browser's on wasm32, where `std`'s panic.
pub use web_time::{Duration, Instant, SystemTime};
/// The browser's H.264 encoder for MP4 exports.
#[cfg(target_arch = "wasm32")]
pub use webcodecs::{Support as H264Support, WebCodecsFactory, support as h264_support};
