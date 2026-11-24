//! A project's export on the desktop: its own full-quality run feeding the export job, which waits for the frames it
//! needs. Shared by the CLI and the app.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::Poll;
use std::time::Instant;

use waveduderust::io::schemes::SchemeStore;
use waveduderust::model::{Layer, Project, View};
use waveduderust::quality::PreviewQuality;
use waveduderust::run::store::StoreError;
use waveduderust::run::{FrameStore, Run, RunOptions};
use waveduderust::unstable::engine::types::FrameShape;
use waveduderust::unstable::export::h264::{H264Factory, Profile};
use waveduderust::unstable::export::{ExportFormat, ExportLayers, ExportPlan, ExportReport, FrameSource, naming};
use waveduderust::{Backend, Simulation};

/// What to export and where.
pub struct Request {
	pub project: Project,
	pub view: View,
	pub format: ExportFormat,
	/// The chosen path; each layer's output is named from it.
	pub out: PathBuf,
	pub layers: ExportLayers,
	/// A still of this timeline frame instead of the in-out range.
	pub still: Option<usize>,
	pub backend: Backend,
	pub schemes: SchemeStore,
	pub h264: Option<Arc<dyn H264Factory>>,
	pub h264_profile: Profile,
	/// Keep the outputs of a failed or cancelled export.
	pub keep_partial: bool,
}

/// Why an export could not start.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("a still needs an image format, not a video")]
	StillOfVideo,
	#[error(transparent)]
	Simulation(#[from] waveduderust::SimulationError),
	#[error("the run failed: {0}")]
	Run(String),
	#[error(transparent)]
	Export(#[from] waveduderust::unstable::export::ExportError),
}

/// The run's frames as the job reads them, waiting for those not made yet.
struct LiveFrames<'a> {
	run: &'a Run,
	store: Arc<FrameStore>,
	cancel: &'a AtomicBool,
	/// Time spent waiting for the run, nanoseconds.
	waited: AtomicU64,
}

impl FrameSource for LiveFrames<'_> {
	fn frame_count(&self) -> usize {
		self.store.count()
	}

	fn shape(&self) -> FrameShape {
		*self.store.shape()
	}

	fn poll_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> Result<Poll<()>, StoreError> {
		self.store.poll_layer(frame, layer, out)
	}

	fn wait_for(&self, frame: usize) -> Result<bool, StoreError> {
		let started = Instant::now();
		self.run.wait_until(|_| frame < self.store.count() || self.cancel.load(Ordering::Relaxed));
		self.waited.fetch_add(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX), Ordering::Relaxed);
		Ok(frame < self.store.count())
	}
}

/// The report and the time spent waiting for the run.
pub struct Done {
	pub report: ExportReport,
	pub waited: std::time::Duration,
	pub shape: FrameShape,
}

/// Runs `request`'s project at full quality and exports it; `progress` hears (frames done, frames in all).
pub fn export(request: Request, progress: impl FnMut(usize, usize), cancel: &AtomicBool) -> Result<Done, Error> {
	if request.still.is_some() && !request.format.supports_still() {
		return Err(Error::StillOfVideo);
	}
	let quality = PreviewQuality { intensity_root: request.format.intensity_root(), ..PreviewQuality::FULL };
	let track_intensity = request.layers != ExportLayers::Field || request.view.intensity.preview;
	let run = Simulation::builder(&request.project)
		.backend(request.backend)
		.track_intensity(track_intensity)
		.spawn(RunOptions::default().quality(quality).brightness((&request.view).into()))?;
	let graphs = request.view.probe_graphs_in_export && request.project.probes.iter().any(|p| p.active);
	if graphs {
		run.wait_until(|_| cancel.load(Ordering::Relaxed));
	}
	let traces = if graphs { run.traces() } else { Vec::new() };
	let store = run.wait_for_store().map_err(|e| Error::Run(e.to_string()))?;
	let schedule = run.schedule();
	let plan = ExportPlan {
		project: &request.project,
		view: &request.view,
		format: request.format,
		fs: Arc::new(waveduderust::unstable::export::fs::StdFs),
		outputs: naming::layer_outputs(&request.out, request.layers),
		still: request.still,
		frame_count: schedule.frame_count() as usize,
		traces: &traces,
		total_steps: u64::from(schedule.steps),
		keep_partial: request.keep_partial,
		h264: request.h264.clone(),
		h264_profile: request.h264_profile,
	};
	#[allow(clippy::arc_with_non_send_sync, reason = "the job shares the source on this thread")]
	let source = Arc::new(LiveFrames { run: &run, store: store.clone(), cancel, waited: AtomicU64::new(0) });
	let job = plan.build_job(source.clone(), &request.schemes)?;
	let report = job.run(progress, cancel);
	if cancel.load(Ordering::Relaxed) {
		run.cancel();
	}
	Ok(Done {
		report,
		waited: std::time::Duration::from_nanos(source.waited.load(Ordering::Relaxed)),
		shape: *store.shape(),
	})
}
