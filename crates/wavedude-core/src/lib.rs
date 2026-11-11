//! `WaveDude` core: project model, scene painting, wave engines, runs, rendering.
//!
//! Coordinates: canvas pixels, origin top-left, x right, y down; a pixel (x, y)
//! spans [x, x+1) x [y, y+1). Angles in degrees, counter-clockwise on screen.
//! Time in steps, space in cells; dx = dt = 1.

pub mod engine;
pub mod io;
pub mod model;
pub mod render;
pub mod rng;
pub mod run;
pub mod scene;

pub use half::f16;
