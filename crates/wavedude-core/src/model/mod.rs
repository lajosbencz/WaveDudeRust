//! The project model: plain data with serde derives, format-agnostic.
//!
//! Every numeric property has a default and a clamp applied by `normalize`;
//! the bounds live in `ranges` so tests can prove each clamp.

mod display;
pub mod lens_geometry;
pub mod meta;
pub mod names;
pub mod normalize;
mod parts;
mod probe;
pub mod profiled;
mod project;
pub mod ranges;
mod scene;
mod scheme;
mod simulation;
mod source;

pub use display::*;
pub use parts::*;
pub use probe::*;
pub use project::*;
pub use scene::*;
pub use scheme::*;
pub use simulation::*;
pub use source::*;

/// Integer canvas position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Point {
	pub x: i32,
	pub y: i32,
}

impl Point {
	pub const fn new(x: i32, y: i32) -> Self {
		Self { x, y }
	}
}

/// Normalises an angle in degrees into (-180, 180]; non-finite becomes 0.
pub fn normalize_angle(a: f64) -> f64 {
	if !a.is_finite() {
		return 0.0;
	}
	if a > -180.0 && a <= 180.0 {
		return a;
	}
	a - 360.0 * ((a - 180.0) / 360.0).ceil()
}

/// Wavelength in pixels of a frequency level at speed `c` and frequency scale `scale`.
/// `omega = level/255 * 0.5 * scale` radians per step.
pub fn wavelength(level: f64, c: f64, scale: f64) -> f64 {
	let omega = level / 255.0 * 0.5 * scale;
	if omega <= 0.0 {
		return f64::INFINITY;
	}
	c / (omega / std::f64::consts::TAU)
}
