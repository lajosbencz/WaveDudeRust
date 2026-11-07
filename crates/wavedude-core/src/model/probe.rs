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
		}
	}
}

impl Probe {
	pub const NAME_STEM: &'static str = "Probe";

	pub fn uses_intensity(&self) -> bool {
		self.measurement != Measurement::Amplitude
	}

	/// Box size in pixels before fitting into the canvas.
	pub fn box_pixels(&self) -> (u32, u32) {
		self.box_size.nominal().unwrap_or((self.box_width, self.box_height))
	}
}
