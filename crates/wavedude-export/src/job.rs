//! The export job: frames of a [`FrameSource`] through colours, glowing edges and overlays into sinks.

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use half::f16;
use wavedude_core::engine::types::{SampleFormat, SampleKind};
use wavedude_core::model::{ColorScheme, DisplayLayer, Layer};
use wavedude_core::render::{HalfTable, render_half_into};
use wavedude_core::scene::glowing_edges;
use wavedude_core::scene::pixels::ScenePixels;

use crate::overlays::Overlays;
use crate::sinks::FrameSink;
use crate::{ExportError, FrameSource};

/// How a layer's frames are coloured: its half table and, when burned in, the scene's glowing edges.
#[derive(Clone, Debug)]
pub struct ExportLayerLook {
	pub table: Arc<HalfTable>,
	/// RGBA8 edge picture of the canvas's size.
	pub edges: Option<Arc<Vec<u8>>>,
	pub edge_gain: f64,
}

impl ExportLayerLook {
	/// The look of a Display layer: its scheme, black level and brightness for samples of `kind`; the glowing edges
	/// of `scene` when the layer burns them in with a gain above zero. A scene of another size than the canvas is
	/// an error: the frames and the scene come from one project.
	pub fn new(
		scheme: &ColorScheme,
		display: &DisplayLayer,
		kind: SampleKind,
		scene: Option<&ScenePixels>,
		canvas: (u32, u32),
	) -> Result<Self, ExportError> {
		let table = HalfTable::cached(scheme, display.black_level, display.brightness as f32, kind);
		let edges = scene
			.filter(|_| display.overlay_in_export && display.overlay_gain > 0.0)
			.map(|s| {
				if (s.width, s.height) != canvas {
					return Err(ExportError::WrongInput("a scene of another size than the canvas"));
				}
				Ok(Arc::new(glowing_edges::compute(&s.rgba, s.width as usize, s.height as usize)))
			})
			.transpose()?;
		Ok(Self { table, edges, edge_gain: display.overlay_gain })
	}
}

/// One output of a job.
pub struct ExportTarget {
	pub layer: Layer,
	pub look: ExportLayerLook,
	pub sink: Box<dyn FrameSink>,
}

/// How a job ended.
#[derive(Debug)]
pub enum ExportState {
	Completed,
	Cancelled,
	Failed(ExportError),
}

#[derive(Debug)]
pub struct ExportReport {
	pub state: ExportState,
	/// Frames written to every target.
	pub frames_done: usize,
	pub elapsed: Duration,
	/// Files left on disk: every output when completed or kept partial, none otherwise.
	pub written: Vec<PathBuf>,
}

/// An export of `frames` (timeline frames) to every target, frame by frame. The source is borrowed: the job runs
/// on the caller's thread, which holds the source for its length.
pub struct ExportJob<'a> {
	pub source: &'a dyn FrameSource,
	pub frames: RangeInclusive<usize>,
	pub targets: Vec<ExportTarget>,
	pub overlays: Option<Overlays>,
	/// Keep the outputs of a failed or cancelled job (finished, so the frames written stay readable).
	pub keep_partial: bool,
}

/// Finishes every sink; the first error.
pub(crate) fn finish_all<'a>(sinks: impl Iterator<Item = &'a mut Box<dyn FrameSink>>) -> crate::Result<()> {
	let mut first = Ok(());
	for sink in sinks {
		if let (Err(e), Ok(())) = (sink.finish(), &first) {
			first = Err(e);
		}
	}
	first
}

/// Removes every sink's files; the first error.
pub(crate) fn discard_all<'a>(sinks: impl Iterator<Item = &'a mut Box<dyn FrameSink>>) -> crate::Result<()> {
	let mut first = Ok(());
	for sink in sinks {
		if let (Err(e), Ok(())) = (sink.discard(), &first) {
			first = Err(e);
		}
	}
	first
}

impl ExportJob<'_> {
	/// Runs on the caller's thread. `progress(done, total)` follows every frame; `cancel` is read before each frame
	/// and while waiting for one. Every sink is finished whatever the outcome; the outputs of a job that did not
	/// complete are then removed unless `keep_partial`.
	pub fn run(mut self, mut progress: impl FnMut(usize, usize), cancel: &AtomicBool) -> ExportReport {
		let started = Instant::now();
		let total = self.frames.clone().count();
		let mut done = 0;
		let mut state = match self.render(&mut done, total, &mut progress, cancel) {
			Ok(true) => ExportState::Completed,
			Ok(false) => ExportState::Cancelled,
			Err(e) => ExportState::Failed(e),
		};
		let finished = finish_all(self.targets.iter_mut().map(|t| &mut t.sink));
		if let (Err(e), false) = (finished, matches!(state, ExportState::Failed(_))) {
			state = ExportState::Failed(e);
		}
		let completed = matches!(state, ExportState::Completed);
		let mut written = Vec::new();
		if completed || self.keep_partial {
			written = self.targets.iter().flat_map(|t| t.sink.files()).collect();
		} else if let Err(e) = discard_all(self.targets.iter_mut().map(|t| &mut t.sink)) {
			tracing::error!(error = %e, "the partial export could not be removed");
			written = self.targets.iter().flat_map(|t| t.sink.files()).collect();
		}
		if let ExportState::Failed(e) = &state {
			tracing::error!(error = %e, frames = done, "export failed");
		} else {
			tracing::debug!(?state, frames = done, elapsed = ?started.elapsed(), "export ended");
		}
		ExportReport { state, frames_done: done, elapsed: started.elapsed(), written }
	}

	/// True when every frame was written, false when cancelled.
	fn render(
		&mut self,
		done: &mut usize,
		total: usize,
		progress: &mut impl FnMut(usize, usize),
		cancel: &AtomicBool,
	) -> crate::Result<bool> {
		let shape = self.source.shape();
		if shape.format != SampleFormat::Half {
			return Err(ExportError::IndexedStore);
		}
		let (w, h) = (shape.width(), shape.height());
		let mut halves = vec![f16::ZERO; shape.plane_samples()];
		let mut rgba = vec![0u8; shape.plane_samples() * 4];
		for frame in self.frames.clone() {
			if cancel.load(Ordering::Relaxed) {
				return Ok(false);
			}
			let held = self.source.wait_for(frame).map_err(ExportError::Source)?;
			if cancel.load(Ordering::Relaxed) {
				return Ok(false);
			}
			if !held {
				return Err(ExportError::FrameMissing { frame, count: self.source.frame_count() });
			}
			for target in &mut self.targets {
				self.source
					.copy_layer(frame, target.layer, bytemuck::cast_slice_mut(&mut halves))
					.map_err(ExportError::Source)?;
				if target.sink.wants_halves() {
					target.sink.write_halves(&halves, w, h)?;
					continue;
				}
				render_half_into(&halves, &target.look.table, &mut rgba);
				if let Some(edges) = &target.look.edges {
					glowing_edges::blend_additive(&mut rgba, edges, target.look.edge_gain);
				}
				if let Some(overlays) = &self.overlays {
					overlays.draw(&mut rgba, target.layer, frame);
				}
				target.sink.write_frame(&rgba, w, h)?;
			}
			*done += 1;
			progress(*done, total);
		}
		Ok(true)
	}
}
