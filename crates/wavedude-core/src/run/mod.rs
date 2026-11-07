//! Runs: the snapshot a run works from, the frame store, the stepping loop, scratch frames, preview quality
//! and the memory warning's arithmetic (spec/40-frames-store-render.md, spec/30-engine.md Scratch frames).

pub mod quality;
pub mod scratch;
pub mod simulation;
pub mod spec;
pub mod store;
pub mod warning;

pub use quality::{MachineFacts, PreviewDefaults, PreviewQuality, PreviewTier};
pub use scratch::{CHECK_INTERVAL, EngineFactory, ScratchEvent, ScratchResult, ScratchRunner, ScratchSink};
pub use simulation::{EventSink, RunError, RunEvent, RunState, SimulationRun};
pub use spec::{RunSpec, RunSpecError, SceneCache};
pub use store::{Budget, FrameStore, Release, StoreError, StoreLayout};
pub use warning::{Levers, MemoryWarning};
