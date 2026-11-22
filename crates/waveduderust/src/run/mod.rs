//! Runs: the frame store, the live run that follows a goal and the memory warning's arithmetic
//! (spec/40-frames-store-render.md, spec/30-engine.md The live run).
//!
//! On wasm32 there is no spill file and no machine query: the live run is pumped by its caller and its stores hold
//! their frames in memory up to the caller's budget, the rest in a pending spill the caller provides
//! (`SpillFactory`). Natively a live run is pumped or works on a thread of its own.

#[cfg(not(target_arch = "wasm32"))]
mod batch;
mod error;
#[cfg_attr(not(feature = "unstable-run"), allow(unreachable_pub, dead_code, reason = "public only with unstable-run"))]
mod pending;
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(not(feature = "unstable-run"), allow(unreachable_pub, dead_code, reason = "public only with unstable-run"))]
mod spill;
#[cfg_attr(not(feature = "unstable-run"), allow(unreachable_pub, dead_code, reason = "public only with unstable-run"))]
mod state;
#[cfg(feature = "unstable-run")]
pub mod store;
#[cfg(not(feature = "unstable-run"))]
#[allow(unreachable_pub, unused_imports, dead_code, reason = "public only with unstable-run")]
pub(crate) mod store;
#[cfg(feature = "unstable-run")]
pub mod warning;
#[cfg(not(feature = "unstable-run"))]
#[allow(unreachable_pub, unused_imports, dead_code, reason = "public only with unstable-run")]
pub(crate) mod warning;
#[cfg(feature = "unstable-run")]
pub mod worker;
#[cfg(not(feature = "unstable-run"))]
#[allow(unreachable_pub, unused_imports, dead_code, reason = "public only with unstable-run")]
pub(crate) mod worker;

pub use crate::spec::Schedule;
#[cfg(not(target_arch = "wasm32"))]
pub use batch::{Canceller, Frames, Progress, Run, RunOptions, Stage};
pub use error::RunError;
pub use store::StoreError;

/// The live worker the editor drives, its frame store and spill.
#[cfg(feature = "unstable-run")]
mod live {
	pub use super::error::storage_full;
	pub use super::pending::{SpillBackend, SpillFactory, Token, WRITE_BYTES_IN_FLIGHT};
	pub use super::state::RunState;
	pub use super::store::{Budget, FrameStore, RamCache, StoreLayout};
	#[cfg(not(target_arch = "wasm32"))]
	pub use super::store::{Memory, Release, sweep_leftovers};
	pub use super::warning::{Levers, MemoryWarning, engine_fits};
	#[cfg(not(target_arch = "wasm32"))]
	pub use super::worker::{BuildEngine, EngineFactory, engines};
	pub use super::worker::{CHECK_INTERVAL, FitFrame, FitRequest, Goal, Pump, RunWorker, Status, Wanted, WorkState};
	pub use super::worker::{EngineFuture, PumpedFactory, StartEngine};
}
#[cfg(feature = "unstable-run")]
pub use live::*;
#[cfg(not(feature = "unstable-run"))]
#[allow(unused_imports, reason = "the crate's own paths")]
pub(crate) use {
	pending::{SpillBackend, SpillFactory},
	store::{FrameStore, StoreLayout},
	warning::engine_fits,
	worker::{Goal, RunWorker, Status, Wanted, WorkState},
};
#[cfg(all(not(feature = "unstable-run"), not(target_arch = "wasm32")))]
#[allow(unused_imports, reason = "the crate's own paths")]
pub(crate) use {store::Memory, worker::engines};

/// (available, physical) bytes; zero where the OS does not tell.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn system_memory() -> (u64, u64) {
	match sysinfo::System::new() {
		Ok(mut system) => {
			system.refresh_memory();
			(system.available_memory(), system.total_memory())
		}
		Err(e) => {
			tracing::warn!("reading the memory: {e}");
			(0, 0)
		}
	}
}

/// Memory, threads and whether `cache`'s drive is a hard disk. The GPU is left unknown: it is the caller's to fill
/// (it owns the GPU engine).
#[cfg(not(target_arch = "wasm32"))]
pub fn machine_facts(cache: &std::path::Path) -> crate::quality::MachineFacts {
	let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
	crate::quality::MachineFacts {
		physical_bytes: system_memory().1,
		threads,
		has_gpu: None,
		cache_drive_seek_penalty: seek_penalty(cache),
	}
}

/// Whether the drive holding `path` (or its nearest existing ancestor) is a hard disk; `None` when the system
/// does not tell.
#[cfg(not(target_arch = "wasm32"))]
fn seek_penalty(path: &std::path::Path) -> Option<bool> {
	let path = path.ancestors().find_map(|p| p.canonicalize().ok())?;
	let disks = sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing().with_kind())
		.inspect_err(|e| tracing::warn!("reading the disks: {e}"))
		.ok()?;
	let disk =
		disks.iter().filter(|d| path.starts_with(d.mount_point())).max_by_key(|d| d.mount_point().as_os_str().len())?;
	match disk.kind() {
		sysinfo::DiskKind::HDD => Some(true),
		sysinfo::DiskKind::SSD => Some(false),
		sysinfo::DiskKind::Unknown(_) => None,
	}
}
