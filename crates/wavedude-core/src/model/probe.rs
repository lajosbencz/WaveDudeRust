use serde::{Deserialize, Serialize};

use super::Layer;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Measurement {
	/// Field value (the field layer's only measurement).
	#[default]
	Amplitude,
	/// Intensity value.
	Value,
	/// Running sum of the intensity from `start_after`.
	IntegratedValue,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ProbeKind {
	#[default]
	Point,
	Line,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BoxSize {
	Tiny,
	Small,
	#[default]
	Medium,
	Large,
	ExtraLarge,
	Custom,
}

impl BoxSize {
	/// Nominal box pixels; Custom takes the probe's own size.
	pub fn nominal(self) -> Option<(u32, u32)> {
		Some(match self {
			BoxSize::Tiny => (200, 100),
			BoxSize::Small => (300, 150),
			BoxSize::Medium => (400, 200),
			BoxSize::Large => (500, 250),
			BoxSize::ExtraLarge => (700, 350),
			BoxSize::Custom => return None,
		})
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ScaleMode {
	/// Last `window` steps.
	#[default]
	Running,
	/// The whole run.
	Full,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum YScale {
	#[default]
	Linear,
	Logarithmic,
}

/// What a probe is: a standard probe, or a probabilistic detector that turns the intensity along its line into
/// quantised detections, Poisson-distributed about the intensity times the sensitivity, drawn from the
/// project's seed so a project gives the same detections every time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ProbeDetector {
	#[default]
	Standard,
	/// The detections of the whole line, step by step, as a photomultiplier's pulses: a series over time.
	Counting,
	/// The detections at every point of the line added up over the run, as a SPAD array builds an image: a
	/// profile along the line.
	Imaging,
}

/// A point or line measurement with a graph box.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Probe {
	pub name: String,
	pub active: bool,
	pub layer: Layer,
	pub measurement: Measurement,
	pub kind: ProbeKind,
	pub ax: i32,
	pub ay: i32,
	pub bx: i32,
	pub by: i32,
	pub box_size: BoxSize,
	pub box_x: i32,
	pub box_y: i32,
	pub box_width: u32,
	pub box_height: u32,
	pub scale_mode: ScaleMode,
	pub auto_scale_x: bool,
	pub auto_scale_y: bool,
	pub y_min: f64,
	pub y_max: f64,
	pub y_scale: YScale,
	pub decades: u32,
	/// Steps shown by a running graph.
	pub window: u32,
	/// Step from which an integrated value sums.
	pub start_after: u32,
	/// A detector is a line probe of the integrated intensity, whatever else is set.
	pub detector: ProbeDetector,
	/// Expected detections per unit of intensity per step at a point of the line, 0..MAX_SENSITIVITY.
	pub sensitivity: f64,
	/// Pixels of the line per bin of an imaging detector's histogram, 1..MAX_BIN_SIZE.
	pub bin_size: u32,
}

impl Default for Probe {
	fn default() -> Self {
		Self {
			name: String::new(),
			active: true,
			layer: Layer::Field,
			measurement: Measurement::Amplitude,
			kind: ProbeKind::Point,
			ax: 0,
			ay: 0,
			bx: 0,
			by: 0,
			box_size: BoxSize::Medium,
			box_x: 0,
			box_y: 0,
			box_width: 400,
			box_height: 200,
			scale_mode: ScaleMode::Running,
			auto_scale_x: true,
			auto_scale_y: true,
			y_min: -1.0,
			y_max: 1.0,
			y_scale: YScale::Linear,
			decades: 6,
			window: 500,
			start_after: 0,
			detector: ProbeDetector::Standard,
			sensitivity: 20.0,
			bin_size: 4,
		}
	}
}

impl Probe {
	pub const NAME_STEM: &'static str = "Probe";

	pub const MAX_SENSITIVITY: f64 = 1_000_000.0;
	pub const MAX_BIN_SIZE: u32 = 20;

	pub fn uses_intensity(&self) -> bool {
		self.measurement != Measurement::Amplitude
	}

	pub fn is_detector(&self) -> bool {
		self.detector != ProbeDetector::Standard
	}

	/// Whether the graph shows a profile along the line (a line probe, an imaging detector) rather than a
	/// series over time (a point probe, a counting detector, whose line gives one count a step).
	pub fn shows_profile(&self) -> bool {
		self.kind == ProbeKind::Line && self.detector != ProbeDetector::Counting
	}

	/// Whether a run records `other` as it records `self`: they differ in the graph box and its axes at most.
	pub fn records_like(&self, other: &Self) -> bool {
		self.name == other.name
			&& self.active == other.active
			&& self.layer == other.layer
			&& self.measurement == other.measurement
			&& self.kind == other.kind
			&& (self.ax, self.ay, self.bx, self.by) == (other.ax, other.ay, other.bx, other.by)
			&& self.start_after == other.start_after
			&& (self.detector, self.sensitivity, self.bin_size) == (other.detector, other.sensitivity, other.bin_size)
	}

	/// Box size in pixels before fitting into the canvas.
	pub fn box_pixels(&self) -> (u32, u32) {
		self.box_size.nominal().unwrap_or((self.box_width, self.box_height))
	}
}
