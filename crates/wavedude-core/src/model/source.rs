use serde::{Deserialize, Serialize};

use super::{Fields, Word, ranges};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
#[serde(rename_all = "PascalCase")]
pub enum SourceMode {
	#[default]
	Continuous,
	Pulsed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
#[serde(rename_all = "PascalCase")]
pub enum PulseShape {
	#[default]
	Gaussian,
	SmoothedSquare,
}

/// A grid of elements around the source's position; present = composite source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct SourceArray {
	#[field(range = ranges::AT_LEAST_1)]
	pub size_x: u32,
	#[field(range = ranges::AT_LEAST_1)]
	pub size_y: u32,
	/// Pixels.
	pub spacing_x: i32,
	pub spacing_y: i32,
	/// Degrees.
	#[field(range = ranges::ANGLE)]
	pub angle: f64,
	/// Random spread in levels.
	#[field(range = ranges::AT_LEAST_0)]
	pub random_frequency: u32,
	/// Random spread in degrees.
	#[field(range = ranges::AT_LEAST_0)]
	pub random_phase: u32,
	/// Random spread of the gap in cycles, per pulse.
	#[field(range = ranges::AT_LEAST_0)]
	pub random_off_width: u32,
	/// Random spread in pixels.
	#[field(range = ranges::AT_LEAST_0)]
	pub random_position: u32,
}

impl Default for SourceArray {
	fn default() -> Self {
		Self {
			size_x: 1,
			size_y: 1,
			spacing_x: 10,
			spacing_y: 10,
			angle: 0.0,
			random_frequency: 0,
			random_phase: 0,
			random_off_width: 0,
			random_position: 0,
		}
	}
}

/// A point or composite source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Source {
	pub name: String,
	pub enabled: bool,
	/// Position in cells: a whole number is a cell's centre, a fraction lies between cells and the
	/// engines spread the drive over the cells around it.
	pub x: f64,
	pub y: f64,
	/// Frequency level 0.01..255.
	#[field(range = ranges::LEVEL)]
	pub frequency: f64,
	/// Percent.
	#[field(range = ranges::PERCENT)]
	pub amplitude: f64,
	/// Degrees.
	#[field(range = ranges::PHASE)]
	pub phase: f64,
	pub mode: SourceMode,
	pub pulse_shape: PulseShape,
	/// Cycles.
	#[field(range = ranges::AT_LEAST_1)]
	pub pulse_width: u32,
	/// Cycles.
	#[field(range = ranges::AT_LEAST_1)]
	pub off_width: u32,
	/// Step after which the source fades out over 30 steps; None = never.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[field(range = ranges::DETACH_AFTER)]
	pub detach_after: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	#[field(nested, whole)]
	pub array: Option<SourceArray>,
}

impl Default for Source {
	fn default() -> Self {
		Self {
			name: String::new(),
			enabled: true,
			x: 0.0,
			y: 0.0,
			frequency: 100.0,
			amplitude: 100.0,
			phase: 0.0,
			mode: SourceMode::Continuous,
			pulse_shape: PulseShape::Gaussian,
			pulse_width: 10,
			off_width: 10,
			detach_after: None,
			array: None,
		}
	}
}

impl Source {
	pub const NAME_STEM: &'static str = "Source";
}
