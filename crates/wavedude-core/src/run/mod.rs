//! Runs: the frame store, the live run that follows a goal and the memory warning's arithmetic
//! (spec/40-frames-store-render.md, spec/30-engine.md The live run).
//!
//! On wasm32 there is no spill file and no machine query: the live run is pumped by its caller and its stores hold
//! their frames in memory up to the caller's budget, the rest in a pending spill the caller provides
//! (`SpillFactory`). Natively a live run is pumped or works on a thread of its own.

mod error;
mod pending;
#[cfg(not(target_arch = "wasm32"))]
mod spill;
mod state;
pub mod store;
pub mod warning;
pub mod worker;

pub use error::RunError;
pub use pending::{SpillBackend, SpillFactory, Token, WRITE_BYTES_IN_FLIGHT};
pub use state::RunState;
pub use store::{Budget, FrameStore, RamCache, StoreError, StoreLayout};
#[cfg(not(target_arch = "wasm32"))]
pub use store::{Memory, Release, sweep_leftovers};
pub use warning::{Levers, MemoryWarning, engine_fits};
#[cfg(not(target_arch = "wasm32"))]
pub use worker::{BuildEngine, EngineFactory};
pub use worker::{CHECK_INTERVAL, FitFrame, FitRequest, Goal, Pump, RunWorker, Status, Wanted, WorkState};
pub use worker::{EngineFuture, PumpedFactory, StartEngine};

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
