//! The export job: frames of a [`FrameSource`] through colours, glowing edges and overlays
//! ([`Rasteriser`]) into sinks.

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
use web_time::{Duration, Instant};

use half::f16;
use wavedude_core::engine::types::{FrameShape, SampleFormat, SampleKind};
use wavedude_core::model::{ColorScheme, DisplayLayer, Layer};
use wavedude_core::render::{HalfTable, render_half_into};
use wavedude_core::scene::glowing_edges;
use wavedude_core::scene::pixels::ScenePixels;

use crate::fs::OutputFs;
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
	/// Files left in the job's file store: every output when completed or kept partial, none otherwise.
	pub written: Vec<PathBuf>,
}

/// An export of timeline frames to every target, frame by frame, in [steps](Self::step) on the caller's thread.
pub struct ExportJob<'a> {
	source: Arc<dyn FrameSource + 'a>,
	/// Where the targets' files go.
	fs: Arc<dyn OutputFs>,
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
	phase: Phase,
	halves: Vec<f16>,
	rgba: Vec<u8>,
	raster: Option<Rasteriser>,
}

/// Where a [step](ExportJob::step) left the job.
#[derive(Debug)]
pub enum Step {
	/// Frames remain.
	Working,
	/// Step again once the event loop ran.
	Waiting(Wait),
	/// Every sink is finished and its files committed; the job is not stepped again.
	Ended(ExportReport),
}

/// What the job does at its next step.
enum Phase {
	Writing,
	/// Every sink is finished; the outcome waits for the files to be committed.
	Flushing(ExportState),
	Ended,
}

/// What a waiting job waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
	/// The timeline frame, pending at [`FrameSource::poll_layer`].
	Frame(usize),
	/// The file store to take more ([`OutputFs::poll_ready`]).
	Ready,
	/// The file store to commit the closed files ([`OutputFs::poll_flushed`]).
	Flushed,
}

/// What one frame's turn did.
enum Advance {
	Wrote,
	Waiting(Wait),
	Done,
	Cancelled,
}

/// Timeline frames named to [`FrameSource::prefetch`] from the one being written on.
const PREFETCH: usize = 4;

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
	/// A job writing `frames` of `source` to every target, whose files are in `fs`; `keep_partial` keeps the
	/// outputs of a failed or cancelled job (finished, so the frames written stay readable).
	pub fn new(
		source: Arc<dyn FrameSource + 'a>,
		fs: Arc<dyn OutputFs>,
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
			fs,
			shape,
			frames,
			targets,
			overlays,
			keep_partial,
			done: 0,
			written: 0,
			started: Instant::now(),
			phase: Phase::Writing,
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
	/// complete source. At the end every sink is finished whatever the outcome and the job waits for the file store
	/// to commit the files, a cancel turning a completed job into a cancelled one; the outputs of a job that did not
	/// complete are then removed unless kept partial. A job dropped before it ended leaves its outputs as they are:
	/// step it with `cancel` set to clean up.
	pub fn step(&mut self, budget: Duration, cancel: &AtomicBool) -> Step {
		if let Phase::Writing = self.phase {
			let turn = Instant::now();
			let state = loop {
				match self.advance(cancel) {
					Ok(Advance::Wrote) if turn.elapsed() < budget => {}
					Ok(Advance::Wrote) => return Step::Working,
					Ok(Advance::Waiting(wait)) => return Step::Waiting(wait),
					Ok(Advance::Done) => break ExportState::Completed,
					Ok(Advance::Cancelled) => break ExportState::Cancelled,
					Err(e) => break ExportState::Failed(e),
				}
			};
			self.phase = Phase::Flushing(self.finish(state));
		}
		let Phase::Flushing(state) = std::mem::replace(&mut self.phase, Phase::Ended) else {
			panic!("an ended export job is not stepped");
		};
		let state = match (self.fs.poll_flushed(), state) {
			(Err(e), state) if !matches!(state, ExportState::Failed(_)) => ExportState::Failed(e.into()),
			(Ok(Poll::Pending), state) => {
				let cancelled = matches!(state, ExportState::Completed) && cancel.load(Ordering::Relaxed);
				self.phase = Phase::Flushing(if cancelled { ExportState::Cancelled } else { state });
				return Step::Waiting(Wait::Flushed);
			}
			(_, state) => state,
		};
		Step::Ended(self.conclude(state))
	}

	/// Steps a frame at a time to the end, `progress(done, total)` following every frame. A wait fails the job:
	/// nothing settles a pending frame or a busy file store while the caller's thread waits here.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn run(mut self, mut progress: impl FnMut(usize, usize), cancel: &AtomicBool) -> ExportReport {
		loop {
			match self.step(Duration::ZERO, cancel) {
				Step::Working => {
					let (done, total) = self.progress();
					progress(done, total);
				}
				Step::Waiting(wait) => {
					const BLOCKING: &str = "which a blocking export cannot wait for";
					let busy = |what| ExportError::Io(std::io::Error::new(std::io::ErrorKind::WouldBlock, what));
					let failed = ExportState::Failed(match wait {
						Wait::Frame(frame) => {
							ExportError::Source(anyhow::anyhow!("frame {frame} is pending, {BLOCKING}"))
						}
						Wait::Ready => busy(format!("the file store takes no more yet, {BLOCKING}")),
						Wait::Flushed => busy(format!("the file store is still writing, {BLOCKING}")),
					});
					let state = match std::mem::replace(&mut self.phase, Phase::Ended) {
						Phase::Writing => self.finish(failed),
						_ => failed,
					};
					return self.conclude(state);
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
		if self.fs.poll_ready()?.is_pending() {
			return Ok(Advance::Waiting(Wait::Ready));
		}
		let held = self.source.wait_for(frame).map_err(ExportError::Source)?;
		if cancel.load(Ordering::Relaxed) {
			return Ok(Advance::Cancelled);
		}
		if !held {
			return Err(ExportError::FrameMissing { frame, count: self.source.frame_count() });
		}
		if self.written == 0 {
			self.source.prefetch(frame..frame.saturating_add(PREFETCH).min(self.frames.end() + 1));
		}
		let (w, h) = (self.shape.width(), self.shape.height());
		while let Some(target) = self.targets.get_mut(self.written) {
			let out = bytemuck::cast_slice_mut(&mut self.halves);
			if self.source.poll_layer(frame, target.layer, out).map_err(ExportError::Source)?.is_pending() {
				return Ok(Advance::Waiting(Wait::Frame(frame)));
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

	/// Finishes every sink; `state`, or the failure to finish when it was not failed already.
	fn finish(&mut self, state: ExportState) -> ExportState {
		match (finish_all(self.targets.iter_mut().map(|t| &mut t.sink)), state) {
			(Err(e), state) if !matches!(state, ExportState::Failed(_)) => ExportState::Failed(e),
			(_, state) => state,
		}
	}

	/// Ends the job: removes the outputs of a job that did not complete unless kept partial.
	fn conclude(&mut self, state: ExportState) -> ExportReport {
		self.phase = Phase::Ended;
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

#[cfg(test)]
mod tests {
	use std::io;
	use std::path::Path;
	use std::sync::Mutex;
	use std::sync::atomic::AtomicUsize;

	use wavedude_core::model::{ColorScheme, DisplayLayer};

	use std::ops::Range;

	use super::*;
	use crate::fs::{MemFs, OutputFile};
	use crate::sinks::{self, EncoderPool, SinkSpec};
	use crate::{ExportFormat, MemoryFrames};

	const W: u32 = 4;
	const H: u32 = 3;
	const FRAMES: usize = 6;

	/// Files in memory behind a store that is busy `busy` times before each frame, still writing at the
	/// `unflushed` first flush polls, and fails its flush with `flush`.
	#[derive(Debug, Default)]
	struct Gated {
		mem: MemFs,
		busy: usize,
		polled: AtomicUsize,
		unflushed: AtomicUsize,
		flush: Mutex<Option<io::Error>>,
	}

	impl OutputFs for Gated {
		fn create(&self, path: &Path) -> io::Result<Box<dyn OutputFile>> {
			self.mem.create(path)
		}

		fn create_dir_all(&self, path: &Path) -> io::Result<bool> {
			self.mem.create_dir_all(path)
		}

		fn exists(&self, path: &Path) -> bool {
			self.mem.exists(path)
		}

		fn remove_file(&self, path: &Path) -> io::Result<()> {
			self.mem.remove_file(path)
		}

		fn remove_dir(&self, path: &Path) -> io::Result<()> {
			self.mem.remove_dir(path)
		}

		fn poll_ready(&self) -> io::Result<Poll<()>> {
			let polled = self.polled.fetch_add(1, Ordering::Relaxed);
			Ok(if polled % (self.busy + 1) < self.busy { Poll::Pending } else { Poll::Ready(()) })
		}

		fn poll_flushed(&self) -> io::Result<Poll<()>> {
			let unflushed = self.unflushed.load(Ordering::Relaxed);
			if unflushed > 0 {
				self.unflushed.store(unflushed - 1, Ordering::Relaxed);
				return Ok(Poll::Pending);
			}
			self.flush.lock().unwrap().take().map_or(Ok(Poll::Ready(())), Err)
		}
	}

	fn frames() -> MemoryFrames {
		let shape = FrameShape {
			canvas_width: W,
			canvas_height: H,
			divisor: 1,
			format: SampleFormat::Half,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: true,
		};
		let plane = |k: usize| (0..W * H).map(|i| f16::from_f32((i as f32 + k as f32) * 0.01)).collect::<Vec<_>>();
		MemoryFrames { shape, frames: (0..FRAMES).map(|k| (plane(k), plane(k))).collect() }
	}

	/// A job writing every frame of `source` as a PNG sequence `seq/seq_NNNN.png` of `fs`.
	fn job(source: Arc<dyn FrameSource>, fs: Arc<dyn OutputFs>) -> ExportJob<'static> {
		let spec = SinkSpec { width: W, height: H, fps: 30, quality: 90, first_frame: 0, frame_count: FRAMES };
		let sink =
			sinks::open(ExportFormat::Png, &fs, Path::new("seq.png"), spec, false, &EncoderPool::default(), None)
				.unwrap();
		let look =
			ExportLayerLook::new(&ColorScheme::grey(), &DisplayLayer::field_default(), SampleKind::Field, None, (W, H))
				.unwrap();
		ExportJob::new(source, fs, 0..=FRAMES - 1, vec![ExportTarget { layer: Layer::Field, look, sink }], None, false)
	}

	/// Steps `job` to its end; the report and the steps that waited.
	fn step_through(job: &mut ExportJob<'_>, cancel: &AtomicBool) -> (ExportReport, usize) {
		let mut waited = 0;
		loop {
			match job.step(Duration::MAX, cancel) {
				Step::Working => {}
				Step::Waiting(_) => waited += 1,
				Step::Ended(report) => return (report, waited),
			}
		}
	}

	#[test]
	fn a_busy_file_store_makes_the_job_wait_before_each_frame() {
		let fs = Arc::new(Gated { busy: 2, ..Gated::default() });
		let (report, waited) = step_through(&mut job(Arc::new(frames()), fs.clone()), &AtomicBool::new(false));
		assert!(matches!(report.state, ExportState::Completed), "{:?}", report.state);
		assert_eq!((report.frames_done, waited), (FRAMES, 2 * FRAMES));
		assert_eq!(fs.mem.paths().len(), FRAMES);
	}

	#[test]
	fn a_failed_flush_fails_the_job_and_removes_its_outputs() {
		let fs = Arc::new(Gated::default());
		*fs.flush.lock().unwrap() = Some(io::Error::other("disk full"));
		let (report, _) = step_through(&mut job(Arc::new(frames()), fs.clone()), &AtomicBool::new(false));
		match report.state {
			ExportState::Failed(ExportError::Io(e)) => assert_eq!(e.to_string(), "disk full"),
			other => panic!("{other:?}"),
		}
		assert!(report.written.is_empty());
		assert!(fs.mem.paths().is_empty(), "the frames written are removed");
	}

	#[test]
	fn a_job_ends_once_its_files_are_committed_and_a_cancel_meanwhile_removes_them() {
		for cancelled in [false, true] {
			let fs = Arc::new(Gated { unflushed: AtomicUsize::new(2), ..Gated::default() });
			let mut job = job(Arc::new(frames()), fs.clone());
			let cancel = AtomicBool::new(false);
			assert!(matches!(job.step(Duration::MAX, &cancel), Step::Waiting(Wait::Flushed)));
			cancel.store(cancelled, Ordering::Relaxed);
			assert!(matches!(job.step(Duration::MAX, &cancel), Step::Waiting(Wait::Flushed)));
			let Step::Ended(report) = job.step(Duration::MAX, &cancel) else { panic!("ended") };
			assert_eq!(report.frames_done, FRAMES);
			if cancelled {
				assert!(matches!(report.state, ExportState::Cancelled), "{:?}", report.state);
				assert!(fs.mem.paths().is_empty(), "the outputs are removed");
			} else {
				assert!(matches!(report.state, ExportState::Completed), "{:?}", report.state);
				assert_eq!(fs.mem.paths().len(), FRAMES);
			}
		}
	}

	#[test]
	fn a_cancel_while_the_file_store_is_busy_ends_the_job() {
		let fs = Arc::new(Gated { busy: 1, ..Gated::default() });
		let mut job = job(Arc::new(frames()), fs.clone());
		let cancel = AtomicBool::new(false);
		assert!(matches!(job.step(Duration::ZERO, &cancel), Step::Waiting(Wait::Ready)));
		assert!(matches!(job.step(Duration::ZERO, &cancel), Step::Working));
		assert!(matches!(job.step(Duration::ZERO, &cancel), Step::Waiting(Wait::Ready)));
		cancel.store(true, Ordering::Relaxed);
		let Step::Ended(report) = job.step(Duration::ZERO, &cancel) else { panic!("ended") };
		assert!(matches!(report.state, ExportState::Cancelled), "{:?}", report.state);
		assert_eq!(report.frames_done, 1);
		assert!(fs.mem.paths().is_empty(), "the outputs are removed");
	}

	#[test]
	fn a_blocking_run_fails_on_a_busy_file_store() {
		let fs = Arc::new(Gated { busy: 1, ..Gated::default() });
		let report = job(Arc::new(frames()), fs.clone()).run(|_, _| {}, &AtomicBool::new(false));
		match report.state {
			ExportState::Failed(ExportError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::WouldBlock, "{e}"),
			other => panic!("{other:?}"),
		}
		assert!(fs.mem.paths().is_empty());
	}

	/// Frames pending at their first read, recording the frames named to read ahead.
	#[derive(Default)]
	struct Spilled {
		frames: Option<MemoryFrames>,
		asked: Mutex<Vec<usize>>,
		prefetched: Mutex<Vec<Range<usize>>>,
	}

	impl FrameSource for Spilled {
		fn frame_count(&self) -> usize {
			FRAMES
		}

		fn shape(&self) -> FrameShape {
			self.frames.as_ref().unwrap().shape
		}

		fn poll_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> anyhow::Result<Poll<()>> {
			let mut asked = self.asked.lock().unwrap();
			if !asked.contains(&frame) {
				asked.push(frame);
				return Ok(Poll::Pending);
			}
			self.frames.as_ref().unwrap().poll_layer(frame, layer, out)
		}

		fn prefetch(&self, frames: Range<usize>) {
			self.prefetched.lock().unwrap().push(frames);
		}
	}

	#[test]
	fn each_frame_names_the_next_four_to_read_ahead() {
		let src = Arc::new(Spilled { frames: Some(frames()), ..Spilled::default() });
		let (report, waited) = step_through(&mut job(src.clone(), Arc::new(MemFs::new())), &AtomicBool::new(false));
		assert!(matches!(report.state, ExportState::Completed), "{:?}", report.state);
		assert_eq!(waited, FRAMES, "each frame pending once");
		let expected: Vec<_> = (0..FRAMES).flat_map(|k| vec![k..(k + 4).min(FRAMES); 2]).collect();
		assert_eq!(*src.prefetched.lock().unwrap(), expected, "named again when a pending frame is asked again");
	}
}
