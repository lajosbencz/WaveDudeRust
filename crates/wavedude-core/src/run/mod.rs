//! Runs: the frame store, the batch stepping loop, the live run that follows a goal and the memory
//! warning's arithmetic (spec/40-frames-store-render.md, spec/30-engine.md The live run).
//!
//! On wasm32 there is no batch run, no spill file and no machine query: the live run is pumped by its caller and
//! its stores hold their frames in memory up to the caller's budget, the rest in a pending spill the caller
//! provides (`SpillFactory`). Natively a live run is pumped or works on a thread of its own.

mod error;
mod pending;
#[cfg(not(target_arch = "wasm32"))]
pub mod simulation;
#[cfg(not(target_arch = "wasm32"))]
mod spill;
mod state;
pub mod store;
pub mod warning;
pub mod worker;

pub use error::RunError;
pub use pending::{SpillBackend, SpillFactory, Token, WRITES_IN_FLIGHT};
#[cfg(not(target_arch = "wasm32"))]
pub use simulation::{EventSink, RunEvent, SimulationRun};
pub use state::RunState;
pub use store::{Budget, FrameStore, StoreError, StoreLayout};
#[cfg(not(target_arch = "wasm32"))]
pub use store::{Memory, Release, sweep_leftovers};
pub use warning::{Levers, MemoryWarning};
#[cfg(not(target_arch = "wasm32"))]
pub use worker::{BuildEngine, EngineFactory};
pub use worker::{CHECK_INTERVAL, FitFrame, FitRequest, Goal, RunWorker, Status, Wanted, WorkState};
pub use worker::{EngineFuture, PumpedFactory, StartEngine};

/// Memory and threads of this machine. The GPU and the drive are left unknown: the GPU is the
/// caller's to fill (it owns the GPU engine), and no platform's seek-penalty query is wired yet.
#[cfg(not(target_arch = "wasm32"))]
pub fn machine_facts() -> crate::quality::MachineFacts {
	let mut system = sysinfo::System::new();
	system.refresh_memory();
	let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
	crate::quality::MachineFacts {
		physical_bytes: system.total_memory(),
		threads,
		has_gpu: None,
		cache_drive_seek_penalty: None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reads_this_machine() {
		let f = machine_facts();
		assert!(f.physical_bytes > 0 && f.threads >= 1);
		assert_eq!((f.has_gpu, f.cache_drive_seek_penalty), (None, None));
	}
}
