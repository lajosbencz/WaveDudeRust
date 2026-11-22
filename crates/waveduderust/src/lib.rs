//! `WaveDude` Rust's simulation library: project model and its view, scene painting, wave engines, runs, rendering, the
//! overlays (`view`) and the exports (`export`).
//!
//! [`Simulation`] steps a project in the caller's thread; [`run`] runs one in the background into a frame store.
//!
//! Coordinates: canvas pixels, origin top-left, x right, y down; a pixel (x, y)
//! spans [x, x+1) x [y, y+1). Angles in degrees, counter-clockwise on screen.
//! Time in steps, space in cells; dx = dt = 1.
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

// The derives' generated paths (`::waveduderust::...`) resolve inside this crate too.
extern crate self as waveduderust;

pub mod io;
pub mod model;
pub mod quality;
#[cfg(feature = "run")]
pub mod run;
#[cfg(not(target_arch = "wasm32"))]
mod simulation;

/// The library's internals for first-party frontends: they may break in any release and the API diff skips them.
/// Each module is public with its `unstable-<area>` feature and crate-private without it.
#[allow(unreachable_pub, unused_imports, dead_code, reason = "public only with the module's unstable feature")]
pub mod unstable;

#[cfg(feature = "export")]
pub(crate) use unstable::export;
#[allow(unused_imports, reason = "used by runs and the native facade")]
pub(crate) use unstable::spec;
#[cfg(feature = "view")]
pub(crate) use unstable::view;
pub(crate) use unstable::{engine, render, rng, scene, sync};

#[cfg(feature = "gpu")]
pub use engine::gpu::GpuError;
pub use engine::probes::{ProbeTrace, Profiles};
pub use engine::{EngineError, Fallback};
pub use half::f16;
#[cfg(not(target_arch = "wasm32"))]
pub use simulation::{Backend, FieldView, Simulation, SimulationBuilder, SimulationError};
pub use spec::RunSpecError;
