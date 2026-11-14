use super::{Fields, Layer, Word, ranges};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Measurement {
	/// Field value (the field layer's only measurement).
	#[default]
	Amplitude,
	/// Intensity value.
	Value,
	/// Running sum of the intensity from `start_after`.
	IntegratedValue,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProbeKind {
	#[default]
	Point,
	Line,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ScaleMode {
	/// Last `window` steps.
	#[default]
	Running,
	/// The whole run.
	Full,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum YScale {
	#[default]
	Linear,
	Logarithmic,
}

/// What a probe is: a standard probe, or a probabilistic detector that turns the intensity along its line into
/// quantised detections, Poisson-distributed about the intensity times the sensitivity, drawn from the
/// project's seed so a project gives the same detections every time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[derive(Clone, Debug, PartialEq, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Probe {
	pub name: String,
	pub active: bool,
	pub layer: Layer,
	pub measurement: Measurement,
	pub kind: ProbeKind,
	/// A detector is a line probe of the integrated intensity, whatever else is set.
	pub detector: ProbeDetector,
	/// Expected detections per unit of intensity per step at a point of the line.
	#[field(range = ranges::PROBE_SENSITIVITY)]
	pub sensitivity: f64,
	/// Pixels of the line per bin of an imaging detector's histogram, `1..MAX_BIN_SIZE`.
	#[field(range = ranges::PROBE_BIN_SIZE)]
	pub bin_size: u32,
	pub ax: i32,
	pub ay: i32,
	pub bx: i32,
	pub by: i32,
	pub box_size: BoxSize,
	pub box_x: i32,
	pub box_y: i32,
	#[field(range = ranges::PROBE_BOX_WIDTH)]
	pub box_width: u32,
	#[field(range = ranges::PROBE_BOX_HEIGHT)]
	pub box_height: u32,
	pub scale_mode: ScaleMode,
	pub auto_scale_x: bool,
	pub auto_scale_y: bool,
	#[field(dependent)]
	pub y_min: f64,
	#[field(dependent)]
	pub y_max: f64,
	pub y_scale: YScale,
	#[field(range = ranges::PROBE_DECADES)]
	pub decades: u32,
	/// Steps shown by a running graph.
	#[field(range = ranges::PROBE_WINDOW)]
	pub window: u32,
	/// Step from which an integrated value sums.
	#[field(range = ranges::AT_LEAST_0)]
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
			detector: ProbeDetector::Standard,
			// WaveDude Next's 20 (io::wdproj::SENSITIVITY_GAIN)
			sensitivity: 0.3125,
			bin_size: 4,
		}
	}
}

impl Probe {
	pub const NAME_STEM: &'static str = "Probe";

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

	/// What a run records of the probe: equal keys give the same traces, so a run keeps them across the edit.
	/// A detector's name counts, its Poisson stream being seeded by the name; a graph box and its axes never do.
	pub fn recording_key(&self) -> RecordingKey {
		RecordingKey {
			stream: self.is_detector().then(|| self.name.clone()),
			active: self.active,
			layer: self.layer,
			measurement: self.measurement,
			kind: self.kind,
			line: (self.ax, self.ay, self.bx, self.by),
			start_after: self.start_after,
			detector: self.detector,
			sensitivity: self.sensitivity.to_bits(),
			bin_size: self.bin_size,
		}
	}

	/// Box size in pixels before fitting into the canvas.
	pub fn box_pixels(&self) -> (u32, u32) {
		self.box_size.nominal().unwrap_or((self.box_width, self.box_height))
	}
}

/// See [`Probe::recording_key`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RecordingKey {
	stream: Option<String>,
	active: bool,
	layer: Layer,
	measurement: Measurement,
	kind: ProbeKind,
	line: (i32, i32, i32, i32),
	start_after: u32,
	detector: ProbeDetector,
	sensitivity: u64,
	bin_size: u32,
}
