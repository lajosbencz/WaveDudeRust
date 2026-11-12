//! Runs: the frame store, the batch stepping loop, the live run that follows a goal and the memory
//! warning's arithmetic (spec/40-frames-store-render.md, spec/30-engine.md The live run).

pub mod simulation;
pub mod store;
pub mod warning;
pub mod worker;

pub use simulation::{EventSink, RunError, RunEvent, RunState, SimulationRun};
pub use store::{Budget, FrameStore, Release, StoreError, StoreLayout, sweep_leftovers};
pub use warning::{Levers, MemoryWarning};
pub use worker::{CHECK_INTERVAL, EngineFactory, FitFrame, FitRequest, Goal, RunWorker, Status, Wanted, WorkState};

use crate::quality::MachineFacts;

/// Memory and threads of this machine. The GPU and the drive are left unknown: the GPU is the
/// caller's to fill (it owns the GPU engine), and no platform's seek-penalty query is wired yet.
pub fn machine_facts() -> MachineFacts {
	let mut system = sysinfo::System::new();
	system.refresh_memory();
	let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
	MachineFacts { physical_bytes: system.total_memory(), threads, has_gpu: None, cache_drive_seek_penalty: None }
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
