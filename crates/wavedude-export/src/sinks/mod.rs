//! Frame sinks: one output (a video file, an image sequence or a still) taking frames in order.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use half::f16;
use wavedude_core::sync::MaybeSend;

use crate::fs::{OutputFs, SharedFile};
use crate::{ExportError, ExportFormat, Result};

pub mod avi;
mod counted;
pub mod exr;
pub mod jpeg;
pub mod mov;
#[cfg(feature = "mp4")]
pub mod mp4;
pub mod mpeg1;
pub mod png;
mod pool;
pub mod sequence;
pub mod threaded;

pub use pool::EncoderPool;
pub use threaded::Threaded;

/// Where frames go, in order. Rendered frames are RGBA8 at the output's size (alpha ignored); a sink that
/// [`wants_halves`](FrameSink::wants_halves) takes the stored f16 plane instead.
pub trait FrameSink: MaybeSend {
	fn wants_halves(&self) -> bool {
		false
	}

	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()>;

	fn write_halves(&mut self, _halves: &[f16], _width: u32, _height: u32) -> Result<()> {
		Err(ExportError::WrongInput("rendered pixels"))
	}

	/// Frames accepted so far.
	fn frames_written(&self) -> usize;

	/// Completes the output (indexes, headers, pending files) and flushes it; later calls do nothing.
	fn finish(&mut self) -> Result<()>;

	/// Files this sink created, once finished.
	fn files(&self) -> Vec<PathBuf> {
		Vec::new()
	}

	/// Removes the files this sink created, once finished.
	fn discard(&mut self) -> Result<()> {
		Ok(())
	}
}

/// A sink writing one file at `path` of `fs`, closed once the sink finished.
pub struct FileOutput<S> {
	sink: S,
	fs: Arc<dyn OutputFs>,
	path: PathBuf,
	file: SharedFile,
}

impl<S: FrameSink> FrameSink for FileOutput<S> {
	fn wants_halves(&self) -> bool {
		self.sink.wants_halves()
	}

	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		self.sink.write_frame(rgba, width, height)
	}

	fn write_halves(&mut self, halves: &[f16], width: u32, height: u32) -> Result<()> {
		self.sink.write_halves(halves, width, height)
	}

	fn frames_written(&self) -> usize {
		self.sink.frames_written()
	}

	fn finish(&mut self) -> Result<()> {
		let finished = self.sink.finish();
		let closed = self.file.close();
		finished.and(closed.map_err(Into::into))
	}

	fn files(&self) -> Vec<PathBuf> {
		vec![self.path.clone()]
	}

	fn discard(&mut self) -> Result<()> {
		// the file goes: an error flushing it is moot
		let _ = self.file.close();
		Ok(self.fs.remove_file(&self.path)?)
	}
}

/// Opens the single-file sink `make` creates on the file at `path`, which its first write creates; a file it
/// created is removed when it fails.
fn file<S: FrameSink + 'static>(
	fs: &Arc<dyn OutputFs>,
	path: &Path,
	make: impl FnOnce(SharedFile) -> Result<S>,
) -> Result<Box<dyn FrameSink>> {
	let existed = fs.exists(path);
	let file = SharedFile::new(fs.clone(), path.to_path_buf());
	match make(file.clone()) {
		Ok(sink) => Ok(Box::new(FileOutput { sink, fs: fs.clone(), path: path.to_path_buf(), file })),
		Err(e) => {
			let opened = file.opened();
			let cleanup = file.close().and_then(|()| if opened && !existed { fs.remove_file(path) } else { Ok(()) });
			if let Err(cleanup) = cleanup {
				tracing::error!(error = %cleanup, "the output that failed to open could not be removed");
			}
			Err(e)
		}
	}
}

/// What an output is opened with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SinkSpec {
	/// Canvas pixels.
	pub width: u32,
	pub height: u32,
	/// `Display.Video.Fps`, 1..240.
	pub fps: u32,
	/// `Display.Video.Quality`, 1..100.
	pub quality: u32,
	/// Timeline frame of the first frame written (numbers sequence files).
	pub first_frame: usize,
	/// Frames planned (pads sequence numbers, reserves a MOV header).
	pub frame_count: usize,
}

pub(crate) fn check_size(spec_w: u32, spec_h: u32, w: u32, h: u32, len: usize, bytes_per_pixel: usize) -> Result<()> {
	if (w, h) != (spec_w, spec_h) || len != w as usize * h as usize * bytes_per_pixel {
		return Err(ExportError::FrameSize { width: spec_w, height: spec_h, got_width: w, got_height: h });
	}
	Ok(())
}

/// Opens the sink of `format` at `path` of `fs`: a video file at the path, encoded on a thread of its own (a movie on
/// `pool`); for a sequence format a still at the path when `still`, else the folder of
/// [`crate::naming::sequence_folder`], encoded on `pool`. `h264`: Cisco's `OpenH264` binary for MP4 (`crate::h264`).
pub fn open(
	format: ExportFormat,
	fs: &Arc<dyn OutputFs>,
	path: &Path,
	spec: SinkSpec,
	still: bool,
	pool: &EncoderPool,
	h264: Option<&Path>,
) -> Result<Box<dyn FrameSink>> {
	use sequence::{ImageKind, ImageSink, Placement};
	let (w, h) = (spec.width, spec.height);
	let image = |kind| -> Result<Box<dyn FrameSink>> {
		let placement = if still {
			Placement::Still(path.to_path_buf())
		} else {
			Placement::Sequence { chosen: PathBuf::from(path), first: spec.first_frame, planned: spec.frame_count }
		};
		Ok(Box::new(ImageSink::new(kind, fs.clone(), placement, spec, pool)?))
	};
	let mov = |codec| {
		file(fs, path, |out| {
			let writer = mov::MovWriter::new(out, w, h, spec.fps, codec, Some(spec.frame_count))?;
			mov::ParallelMov::new(writer, pool)
		})
	};
	match format {
		#[cfg(feature = "mp4")]
		ExportFormat::Mp4 => file(fs, path, |out| Threaded::new(mp4::Mp4Writer::new(out, spec, h264)?, w, h)),
		#[cfg(not(feature = "mp4"))]
		ExportFormat::Mp4 => {
			let _ = h264;
			Err(ExportError::H264("built without the mp4 feature".into()))
		}
		ExportFormat::Mpeg1 => file(fs, path, |out| {
			Threaded::new(mpeg1::Mpeg1Writer::new(out, w, h, spec.fps as f64, spec.quality)?, w, h)
		}),
		ExportFormat::Avi => {
			file(fs, path, |out| Threaded::new(avi::AviWriter::new(out, w, h, spec.fps, avi::SEGMENT_LIMIT)?, w, h))
		}
		ExportFormat::Mov => mov(mov::MovCodec::Animation),
		ExportFormat::MovPng => mov(mov::MovCodec::Png),
		ExportFormat::Png => image(ImageKind::Png),
		ExportFormat::Jpeg => image(ImageKind::Jpeg { quality: spec.quality }),
		ExportFormat::Exr | ExportFormat::ExrRoot => image(ImageKind::Exr),
	}
}
