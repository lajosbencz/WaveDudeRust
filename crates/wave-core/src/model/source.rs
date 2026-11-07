use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SourceMode {
	#[default]
	Continuous,
	Pulsed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PulseShape {
	#[default]
	Gaussian,
	SmoothedSquare,
}

/// A grid of elements around the source's position; present = composite source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct SourceArray {
	pub size_x: u32,
	pub size_y: u32,
	/// Pixels.
	pub spacing_x: i32,
	pub spacing_y: i32,
	/// Degrees.
	pub angle: f64,
	/// Random spread in levels.
	pub random_frequency: u32,
	/// Random spread in degrees.
	pub random_phase: u32,
	/// Random spread of the gap in cycles, per pulse.
	pub random_off_width: u32,
	/// Random spread in pixels.
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
	pub frequency: f64,
	/// Percent.
	pub amplitude: f64,
	/// Degrees.
	pub phase: f64,
	pub mode: SourceMode,
	pub pulse_shape: PulseShape,
	/// Cycles.
	pub pulse_width: u32,
	/// Cycles.
	pub off_width: u32,
	/// Step after which the source fades out over 30 steps; None = never.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub detach_after: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
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
