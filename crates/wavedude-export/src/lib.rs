//! Image, video, `OpenEXR` and CSV export: the formats, their sinks, the burned-in overlays and the export job.
//!
//! Every rendered frame handed to a sink is RGBA8, row-major, top row first, no padding, alpha 255, at canvas size.
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::path::Path;

use half::f16;
use wavedude_core::engine::types::{FrameShape, SampleKind};
use wavedude_core::model::Layer;

mod bits;
pub mod csv;
pub mod graph;
#[cfg(feature = "mp4")]
pub mod h264;
pub mod job;
pub mod naming;
pub mod overlays;
pub mod plan;
pub mod range;
pub mod raster;
pub mod sinks;
mod yuv;

pub use job::{ExportJob, ExportLayerLook, ExportReport, ExportState, ExportTarget};
pub use plan::ExportPlan;
pub use sinks::{EncoderPool, FrameSink, SinkSpec};

/// Export formats in the order of the save dialog's filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum ExportFormat {
	/// H.264 in MP4 (openh264).
	Mp4,
	/// MPEG-1 program stream, I-frames only.
	Mpeg1,
	/// Uncompressed 24-bit `OpenDML` AVI.
	Avi,
	/// `QuickTime` Animation (`rle `).
	Mov,
	/// `QuickTime` with PNG samples (`png `).
	#[cfg_attr(feature = "clap", value(name = "movpng"))]
	MovPng,
	/// 24-bit RGB PNG.
	Png,
	/// Baseline JPEG, 4:4:4.
	Jpeg,
	/// `OpenEXR` of the stored halves, the intensity as I.
	Exr,
	/// `OpenEXR` of the stored halves, the intensity as sqrt(I).
	#[cfg_attr(feature = "clap", value(name = "exrroot"))]
	ExrRoot,
}

impl ExportFormat {
	pub const ALL: [ExportFormat; 9] = [
		ExportFormat::Mp4,
		ExportFormat::Mpeg1,
		ExportFormat::Avi,
		ExportFormat::Mov,
		ExportFormat::MovPng,
		ExportFormat::Png,
		ExportFormat::Jpeg,
		ExportFormat::Exr,
		ExportFormat::ExrRoot,
	];

	/// Lower-case extensions without the dot, the default first.
	pub fn extensions(self) -> &'static [&'static str] {
		match self {
			ExportFormat::Mp4 => &["mp4", "m4v"],
			ExportFormat::Mpeg1 => &["mpg", "mpeg", "m1v"],
			ExportFormat::Avi => &["avi"],
			ExportFormat::Mov | ExportFormat::MovPng => &["mov", "qt"],
			ExportFormat::Png => &["png"],
			ExportFormat::Jpeg => &["jpg", "jpeg"],
			ExportFormat::Exr | ExportFormat::ExrRoot => &["exr"],
		}
	}

	pub fn default_extension(self) -> &'static str {
		self.extensions()[0]
	}

	/// One file holding every frame.
	pub fn is_video(self) -> bool {
		matches!(
			self,
			ExportFormat::Mp4 | ExportFormat::Mpeg1 | ExportFormat::Avi | ExportFormat::Mov | ExportFormat::MovPng
		)
	}

	/// Can write a single frame as a still.
	pub fn supports_still(self) -> bool {
		!self.is_video()
	}

	/// The sink takes the stored halves, not rendered pixels.
	pub fn wants_halves(self) -> bool {
		matches!(self, ExportFormat::Exr | ExportFormat::ExrRoot)
	}

	/// The intensity plane the export's run must store: sqrt(I) for every format but [`ExportFormat::Exr`].
	pub fn intensity_root(self) -> bool {
		self != ExportFormat::Exr
	}

	/// The first format in dialog order whose extensions hold the path's (case-insensitive).
	pub fn from_path(path: &Path) -> Option<Self> {
		let ext = path.extension()?.to_str()?.to_ascii_lowercase();
		Self::ALL.into_iter().find(|f| f.extensions().contains(&ext.as_str()))
	}
}

/// Which layers an export writes; Both writes one output per layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum ExportLayers {
	#[default]
	Field,
	Intensity,
	Both,
}

impl ExportLayers {
	pub fn layers(self) -> &'static [Layer] {
		match self {
			ExportLayers::Field => &[Layer::Field],
			ExportLayers::Intensity => &[Layer::Intensity],
			ExportLayers::Both => &[Layer::Field, Layer::Intensity],
		}
	}
}

/// Stored frames an export reads; the frame store implements it.
pub trait FrameSource {
	/// Frames held, stored frame k being the state after step `k * CaptureEvery` (every frame at full quality).
	fn frame_count(&self) -> usize;

	/// Geometry and sample format of every plane.
	fn shape(&self) -> FrameShape;

	/// Copies the plane of `layer` at `frame` into `out` (`shape().plane_bytes()` bytes; native-endian f16 for a
	/// Half store, palette indices for an Index store).
	fn copy_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> anyhow::Result<()>;

	/// Blocks until `frame` is held; false when it never will be (the source is complete or its producer ended).
	/// A complete source answers at once.
	fn wait_for(&self, frame: usize) -> anyhow::Result<bool> {
		Ok(frame < self.frame_count())
	}

	/// How the values of a layer's plane are meant.
	fn sample_kind(&self, layer: Layer) -> SampleKind {
		self.shape().sample_kind(layer)
	}
}

/// An in-memory [`FrameSource`]: one f16 plane per layer and frame.
#[derive(Clone, Debug)]
pub struct MemoryFrames {
	pub shape: FrameShape,
	/// `[frame] -> (field, intensity)` halves of `shape.plane_samples()` each.
	pub frames: Vec<(Vec<f16>, Vec<f16>)>,
}

impl FrameSource for MemoryFrames {
	fn frame_count(&self) -> usize {
		self.frames.len()
	}

	fn shape(&self) -> FrameShape {
		self.shape
	}

	fn copy_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> anyhow::Result<()> {
		let (field, intensity) = self
			.frames
			.get(frame)
			.ok_or_else(|| anyhow::anyhow!("frame {frame} is not held ({})", self.frames.len()))?;
		let plane = match layer {
			Layer::Field => field,
			Layer::Intensity => intensity,
		};
		out.copy_from_slice(bytemuck::cast_slice(plane));
		Ok(())
	}
}

/// Why an export or a sink failed.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
	#[error("{0}")]
	Io(#[from] std::io::Error),
	#[error("PNG encoding failed: {0}")]
	Png(#[from] png::EncodingError),
	#[error("JPEG encoding failed: {0}")]
	Jpeg(#[from] jpeg_encoder::EncodingError),
	#[error("OpenEXR writing failed: {0}")]
	Exr(#[from] exr::error::Error),
	#[error("H.264 encoding failed: {0}")]
	H264(String),
	#[error("OpenH264: {0}")]
	Codec(String),
	#[error("MP4 export needs Cisco's OpenH264 codec, which is not enabled")]
	H264Missing,
	#[error("{format} cannot carry a {width} x {height} picture ({limit})")]
	Size { format: &'static str, width: u32, height: u32, limit: &'static str },
	#[error("the frame is {got_width} x {got_height}, the output {width} x {height}")]
	FrameSize { width: u32, height: u32, got_width: u32, got_height: u32 },
	#[error("frame {frame} is not in the store (it holds {count})")]
	FrameMissing { frame: usize, count: usize },
	#[error("frame {frame} is beyond the run's {count} frames")]
	StillBeyond { frame: usize, count: usize },
	#[error("the run holds no frames to export")]
	NoFrames,
	#[error("an export needs a 16-bit run; this store holds palette indices")]
	IndexedStore,
	#[error("this sink takes {0}")]
	WrongInput(&'static str),
	#[error("{0}")]
	Source(anyhow::Error),
	#[error("the output is already finished")]
	Finished,
	#[error("an encoder thread panicked: {0}")]
	Panicked(String),
}

pub type Result<T, E = ExportError> = std::result::Result<T, E>;

impl FrameSource for wavedude_core::run::store::FrameStore {
	fn frame_count(&self) -> usize {
		self.count()
	}

	fn shape(&self) -> FrameShape {
		*wavedude_core::run::store::FrameStore::shape(self)
	}

	fn copy_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> anyhow::Result<()> {
		self.read_layer(frame, layer, out)?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_by_extension_take_the_first_in_dialog_order() {
		assert_eq!(ExportFormat::from_path(Path::new("a/b.MOV")), Some(ExportFormat::Mov));
		assert_eq!(ExportFormat::from_path(Path::new("x.exr")), Some(ExportFormat::Exr));
		assert_eq!(ExportFormat::from_path(Path::new("x.m1v")), Some(ExportFormat::Mpeg1));
		assert_eq!(ExportFormat::from_path(Path::new("x.jpeg")), Some(ExportFormat::Jpeg));
		assert_eq!(ExportFormat::from_path(Path::new("x.m4v")), Some(ExportFormat::Mp4));
		assert_eq!(ExportFormat::from_path(Path::new("x.gif")), None);
		assert_eq!(ExportFormat::from_path(Path::new("x")), None);
		assert!(ExportFormat::Png.supports_still() && !ExportFormat::Avi.supports_still());
		assert!(ExportFormat::ALL.iter().all(|f| f.is_video() != f.supports_still()));
		assert!(!ExportFormat::Exr.intensity_root() && ExportFormat::ExrRoot.intensity_root());
	}
}
