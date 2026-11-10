//! Runs: the snapshot a run works from, the frame store, the batch stepping loop, the live run that follows a
//! goal, preview quality and the memory warning's arithmetic (spec/40-frames-store-render.md, spec/30-engine.md
//! The live run).

pub mod quality;
pub mod simulation;
pub mod spec;
pub mod store;
pub mod warning;
pub mod worker;

pub use quality::{MachineFacts, PreviewDefaults, PreviewQuality, PreviewTier};
pub use simulation::{EventSink, RunError, RunEvent, RunState, SimulationRun};
pub use spec::{RunRequest, RunSpec, RunSpecError, SceneCache, Schedule};
pub use store::{Budget, FrameStore, Release, StoreError, StoreLayout};
pub use warning::{Levers, MemoryWarning};
pub use worker::{CHECK_INTERVAL, EngineFactory, FitFrame, FitRequest, Goal, RunWorker, Status, Wanted, WorkState};
