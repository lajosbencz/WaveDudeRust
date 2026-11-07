//! Colouring of stored planes: gradients, colour tables, the frame and screen renderers, the brightness fit.
//!
//! Every picture is RGBA8, row-major, top row first, no padding, alpha 255.

pub mod brightness;
pub mod frame;
pub mod gradient;
pub mod lut;
pub mod screen;

pub use crate::engine::capture::{field_index, intensity_index, intensity_root_index};
pub use brightness::{fit_brightness, fit_brightness_f32};
pub use frame::{render_half, render_half_into, render_indexed, render_indexed_into};
pub use gradient::Gradient;
pub use lut::{DeepHalfTable, HalfTable, Palette};
pub use screen::{DitherDepth, ResampleAxis, ScreenRenderer};
