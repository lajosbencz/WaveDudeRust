//! What the preview and the exports draw over a picture, without a UI toolkit: the overlay of handles and labels, the
//! probe graph box's layout, as epaint shapes, and the bundled fonts their text is set in.
//!
//! Data and arithmetic over the core's model and epaint; builds for `wasm32-unknown-unknown`.

pub mod draw;
pub mod fonts;
pub mod graph;
pub mod overlay;

pub use draw::{DeviceGrid, FontMetrics, TextMetrics};
pub use epaint;
