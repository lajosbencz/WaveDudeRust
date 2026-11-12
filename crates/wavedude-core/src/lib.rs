//! `WaveDude` core: project model, scene painting, wave engines, runs, rendering.
//!
//! Coordinates: canvas pixels, origin top-left, x right, y down; a pixel (x, y)
//! spans [x, x+1) x [y, y+1). Angles in degrees, counter-clockwise on screen.
//! Time in steps, space in cells; dx = dt = 1.
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

// The derives' generated paths (`::wavedude_core::...`) resolve inside this crate too.
extern crate self as wavedude_core;

pub mod engine;
pub mod io;
pub mod model;
pub mod quality;
pub mod render;
pub mod rng;
#[cfg(feature = "run")]
pub mod run;
pub mod scene;
pub mod spec;
pub mod sync;

pub use half::f16;
