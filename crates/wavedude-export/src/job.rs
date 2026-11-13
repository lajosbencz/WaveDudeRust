//! The export job: frames of a [`FrameSource`] through colours, glowing edges and overlays
//! ([`Rasteriser`]) into sinks.

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use web_time::{Duration, Instant};

use half::f16;
use wavedude_core::engine::types::{FrameShape, SampleFormat, SampleKind};
use wavedude_core::model::{ColorScheme, DisplayLayer, Layer};
use wavedude_core::render::{HalfTable, render_half_into};
use wavedude_core::scene::glowing_edges;
use wavedude_core::scene::pixels::ScenePixels;

use crate::overlays::Overlays;
use crate::raster::Rasteriser;
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
	/// Wall time since the job was made, idle time between steps included.
	pub elapsed: Duration,
	/// Files left on disk: every output when completed or kept partial, none otherwise.
	pub written: Vec<PathBuf>,
}

/// An export of timeline frames to every target, frame by frame, in [steps](Self::step) on the caller's thread.
pub struct ExportJob<'a> {
	source: Arc<dyn FrameSource + 'a>,
	shape: FrameShape,
	frames: RangeInclusive<usize>,
	targets: Vec<ExportTarget>,
	overlays: Option<Overlays>,
	keep_partial: bool,
	/// Frames written to every target.
	done: usize,
	/// The targets of the frame after `done` written so far.
	written: usize,
	started: Instant,
	ended: bool,
	halves: Vec<f16>,
	rgba: Vec<u8>,
	raster: Option<Rasteriser>,
}

/// Where a [step](ExportJob::step) left the job.
#[derive(Debug)]
pub enum Step {
	/// Frames remain.
	Working,
	/// A frame is pending ([`FrameSource::poll_layer`]): step again once the event loop ran.
	Waiting,
	/// Every sink is finished; the job is not stepped again.
	Ended(ExportReport),
}

/// What one frame's turn did.
enum Advance {
	Wrote,
	Pending,
	Done,
	Cancelled,
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

impl<'a> ExportJob<'a> {
	/// A job writing `frames` of `source` to every target; `keep_partial` keeps the outputs of a failed or cancelled
	/// job (finished, so the frames written stay readable).
	pub fn new(
		source: Arc<dyn FrameSource + 'a>,
		frames: RangeInclusive<usize>,
		targets: Vec<ExportTarget>,
		overlays: Option<Overlays>,
		keep_partial: bool,
	) -> Self {
		let shape = source.shape();
		let samples = shape.plane_samples();
		let raster = overlays.as_ref().map(Overlays::rasteriser);
		Self {
			source,
			shape,
			frames,
			targets,
			overlays,
			keep_partial,
			done: 0,
			written: 0,
			started: Instant::now(),
			ended: false,
			halves: vec![f16::ZERO; samples],
			rgba: vec![0u8; samples * 4],
			raster,
		}
	}

	/// `(done, total)` frames.
	pub fn progress(&self) -> (usize, usize) {
		(self.done, (self.frames.end() + 1).saturating_sub(*self.frames.start()))
	}

	/// Writes frames until `budget` has passed, at least one or the part of one not pending. `cancel` is read
	/// before each frame and while waiting for one; [`FrameSource::wait_for`] blocks, so a step is short only on a
	/// complete source. At the end every sink is finished whatever the outcome; the outputs of a job that did not
	/// complete are then removed unless kept partial. A job dropped before it ended leaves its outputs as they are:
	/// step it with `cancel` set to clean up.
	pub fn step(&mut self, budget: Duration, cancel: &AtomicBool) -> Step {
		assert!(!self.ended, "an ended export job is not stepped");
		let turn = Instant::now();
		let state = loop {
			match self.advance(cancel) {
				Ok(Advance::Wrote) if turn.elapsed() < budget => {}
				Ok(Advance::Wrote) => return Step::Working,
				Ok(Advance::Pending) => return Step::Waiting,
				Ok(Advance::Done) => break ExportState::Completed,
				Ok(Advance::Cancelled) => break ExportState::Cancelled,
				Err(e) => break ExportState::Failed(e),
			}
		};
		Step::Ended(self.end(state))
	}

	/// Steps a frame at a time to the end, `progress(done, total)` following every frame. A pending frame fails
	/// the job: nothing settles it while the caller's thread waits here.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn run(mut self, mut progress: impl FnMut(usize, usize), cancel: &AtomicBool) -> ExportReport {
		loop {
			match self.step(Duration::ZERO, cancel) {
				Step::Working => {
					let (done, total) = self.progress();
					progress(done, total);
				}
				Step::Waiting => {
					let frame = *self.frames.start() + self.done;
					let pending = anyhow::anyhow!("frame {frame} is pending, which a blocking export cannot wait for");
					return self.end(ExportState::Failed(ExportError::Source(pending)));
				}
				Step::Ended(report) => return report,
			}
		}
	}

	/// Writes the targets of the next frame not yet written to them.
	fn advance(&mut self, cancel: &AtomicBool) -> crate::Result<Advance> {
		if self.shape.format != SampleFormat::Half {
			return Err(ExportError::IndexedStore);
		}
		let frame = *self.frames.start() + self.done;
		if !self.frames.contains(&frame) {
			return Ok(Advance::Done);
		}
		if cancel.load(Ordering::Relaxed) {
			return Ok(Advance::Cancelled);
		}
		let held = self.source.wait_for(frame).map_err(ExportError::Source)?;
		if cancel.load(Ordering::Relaxed) {
			return Ok(Advance::Cancelled);
		}
		if !held {
			return Err(ExportError::FrameMissing { frame, count: self.source.frame_count() });
		}
		let (w, h) = (self.shape.width(), self.shape.height());
		while let Some(target) = self.targets.get_mut(self.written) {
			let out = bytemuck::cast_slice_mut(&mut self.halves);
			if self.source.poll_layer(frame, target.layer, out).map_err(ExportError::Source)?.is_pending() {
				return Ok(Advance::Pending);
			}
			if target.sink.wants_halves() {
				target.sink.write_halves(&self.halves, w, h)?;
			} else {
				render_half_into(&self.halves, &target.look.table, &mut self.rgba);
				if let Some(edges) = &target.look.edges {
					glowing_edges::blend_additive(&mut self.rgba, edges, target.look.edge_gain);
				}
				if let (Some(overlays), Some(raster)) = (&self.overlays, self.raster.as_mut()) {
					overlays.draw(raster, &mut self.rgba, target.layer, frame)?;
				}
				target.sink.write_frame(&self.rgba, w, h)?;
			}
			self.written += 1;
		}
		self.written = 0;
		self.done += 1;
		Ok(Advance::Wrote)
	}

	/// Finishes every sink and removes the outputs of a job that did not complete unless kept partial.
	fn end(&mut self, mut state: ExportState) -> ExportReport {
		self.ended = true;
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
		let done = self.done;
		if let ExportState::Failed(e) = &state {
			tracing::error!(error = %e, frames = done, "export failed");
		} else {
			tracing::debug!(?state, frames = done, elapsed = ?self.started.elapsed(), "export ended");
		}
		ExportReport { state, frames_done: done, elapsed: self.started.elapsed(), written }
	}
}
