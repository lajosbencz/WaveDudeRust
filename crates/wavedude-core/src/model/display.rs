use super::{Fields, Word, ranges};

/// Which stored plane a value belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Word)]
pub enum Layer {
	#[default]
	Field,
	Intensity,
}

/// Presentation of one layer.
#[derive(Clone, Debug, PartialEq, Fields)]
pub struct DisplayLayer {
	pub color_scheme: String,
	/// Gain to the palette's top: |u| * b (field) or sqrt(I) * b (intensity) at 1 is the gradient's end.
	#[field(range = ranges::LAYER_BRIGHTNESS)]
	pub brightness: f64,
	#[field(range = ranges::LAYER_BLACK_LEVEL)]
	pub black_level: f64,
	/// Layer captured and shown.
	pub preview: bool,
	/// Glowing edges over the preview picture.
	pub overlay: bool,
	#[field(range = ranges::LAYER_OVERLAY_GAIN)]
	pub overlay_gain: f64,
	pub overlay_in_export: bool,
}

/// Default field brightness: a source at Amplitude 100 % swings its cell by 1 in free space and the field falls
/// as 1/sqrt(r) from there, so a gain of 1 puts the source's swing at the palette's top.
pub const FIELD_BRIGHTNESS: f64 = 1.0;

/// Default intensity brightness: the EMA of u^2 of a unit swing is 1/2 at the source cell, sqrt(I) about 0.71,
/// so a gain of 1.5 puts the source cell just past the top and the wave around it inside the gradient.
pub const INTENSITY_BRIGHTNESS: f64 = 1.5;

impl DisplayLayer {
	pub fn field_default() -> Self {
		Self {
			color_scheme: "Default field".into(),
			brightness: FIELD_BRIGHTNESS,
			black_level: -0.05,
			preview: true,
			overlay: true,
			overlay_gain: 3.0,
			overlay_in_export: true,
		}
	}

	pub fn intensity_default() -> Self {
		Self {
			color_scheme: "Default intensity".into(),
			brightness: INTENSITY_BRIGHTNESS,
			black_level: 0.0,
			..Self::field_default()
		}
	}
}

impl Default for DisplayLayer {
	fn default() -> Self {
		Self::field_default()
	}
}

#[derive(Clone, Debug, PartialEq, Fields)]
pub struct Video {
	#[field(range = ranges::VIDEO_FPS)]
	pub fps: u32,
	#[field(range = ranges::VIDEO_QUALITY)]
	pub quality: u32,
}

impl Default for Video {
	fn default() -> Self {
		Self { fps: 60, quality: 85 }
	}
}

#[derive(Clone, Debug, PartialEq, Fields)]
pub struct Display {
	#[field(nested)]
	pub field: DisplayLayer,
	#[field(nested)]
	pub intensity: DisplayLayer,
	#[field(nested)]
	pub video: Video,
	/// In point in steps.
	#[field(range = ranges::STEP_POINT)]
	pub export_from: u32,
	/// Out point in steps; 0 = to the end.
	#[field(range = ranges::STEP_POINT, dependent)]
	pub export_to: u32,
	pub probe_graphs_in_export: bool,
	pub probe_markers_in_export: bool,
	pub source_markers_in_export: bool,
	pub handles_in_preview: bool,
	pub labels_in_preview: bool,
}

impl Default for Display {
	fn default() -> Self {
		Self {
			field: DisplayLayer::field_default(),
			intensity: DisplayLayer::intensity_default(),
			video: Video::default(),
			export_from: 0,
			export_to: 0,
			probe_graphs_in_export: true,
			probe_markers_in_export: true,
			source_markers_in_export: false,
			handles_in_preview: true,
			labels_in_preview: true,
		}
	}
}

impl Display {
	pub fn layer(&self, layer: Layer) -> &DisplayLayer {
		match layer {
			Layer::Field => &self.field,
			Layer::Intensity => &self.intensity,
		}
	}

	pub fn layer_mut(&mut self, layer: Layer) -> &mut DisplayLayer {
		match layer {
			Layer::Field => &mut self.field,
			Layer::Intensity => &mut self.intensity,
		}
	}
}
