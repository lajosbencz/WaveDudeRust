//! What the preview draws, without a UI toolkit: the overlay of handles and labels over a picture, the probe graph
//! box's layout, and the draw list both the desktop and the browser execute.
//!
//! Pure data and arithmetic over the core's model; builds for `wasm32-unknown-unknown`.

pub mod draw;
pub mod graph;
pub mod overlay;

pub use draw::{DeviceGrid, Rgba, Shape};
