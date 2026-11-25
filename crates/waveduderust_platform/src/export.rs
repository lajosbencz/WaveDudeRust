//! A project's export on the desktop: its own full-quality run feeding the export job, which waits for the frames it
//! needs. Shared by the CLI and the app.

use std::path::{Path, PathBuf};
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
use waveduderust::unstable::export::fs::zip::UtcDateTime;
use waveduderust::unstable::export::fs::{StdFs, ZipFs};
use waveduderust::unstable::export::h264::{H264Factory, Profile};
use waveduderust::unstable::export::{
	ExportFormat, ExportLayers, ExportPlan, ExportReport, ExportState, FrameSource, naming,
};
use waveduderust::{Backend, ProbeTrace, Simulation};

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
	/// The ZIP archive the outputs go into, named in it as `out` names them; it replaces its file only once complete.
	pub archive: Option<PathBuf>,
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
	#[error("the archive: {0}")]
	Archive(std::io::Error),
	#[error("the archive: {error}; the outputs are kept in {}", kept.display())]
	ArchiveKept { error: std::io::Error, kept: PathBuf },
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

/// The folder an archive's outputs are written into before they are packed: hidden, beside it, removed with this.
struct Parts(PathBuf, bool);

impl Parts {
	/// The folder for `archive`, emptied of what an interrupted export left.
	fn new(archive: &Path) -> std::io::Result<Self> {
		let name = archive.file_name().unwrap_or_default().to_string_lossy();
		let folder = archive.with_file_name(format!(".{name}.parts"));
		match std::fs::remove_dir_all(&folder) {
			Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
			_ => {}
		}
		// the archive's folder must exist: none is made for it
		std::fs::create_dir(&folder)?;
		Ok(Self(folder, false))
	}

	/// The folder, left in place.
	fn keep(mut self) -> PathBuf {
		self.1 = true;
		std::mem::take(&mut self.0)
	}
}

impl Drop for Parts {
	fn drop(&mut self) {
		if !self.1 {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}
}

/// Packs the files under `parts` into the ZIP archive `archive`, stored, streamed in chunks; written under a
/// temporary name, then renamed over `archive`; `cancel` interrupts it.
fn pack(parts: &Path, archive: &Path, cancel: &AtomicBool) -> std::io::Result<()> {
	use std::io::Read as _;
	let name = archive.file_name().unwrap_or_default().to_string_lossy();
	let partial = archive.with_file_name(format!(".{name}.partial"));
	let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
	let time = UtcDateTime::from_unix(i64::try_from(now).unwrap_or(0));
	let packed = (|| {
		let mut zip = ZipFs::in_store(Arc::new(StdFs), &partial, time)?;
		let mut files = Vec::new();
		let mut folders = vec![parts.to_path_buf()];
		while let Some(folder) = folders.pop() {
			for entry in std::fs::read_dir(folder)? {
				let path = entry?.path();
				if path.is_dir() { folders.push(path) } else { files.push(path) }
			}
		}
		files.sort();
		for path in files {
			let mut file = std::fs::File::open(&path)?;
			let inside = path.strip_prefix(parts).map_err(std::io::Error::other)?;
			zip.write_entry_sync(inside, || {
				if cancel.load(Ordering::Relaxed) {
					return Err(std::io::Error::other("cancelled"));
				}
				let mut chunk = vec![0; 1 << 20];
				let n = file.read(&mut chunk)?;
				chunk.truncate(n);
				Ok((n > 0).then_some(chunk))
			})?;
		}
		zip.finish()
	})();
	let done = packed.and_then(|()| std::fs::rename(&partial, archive));
	if done.is_err() {
		let _ = std::fs::remove_file(&partial);
	}
	done
}

/// What a run made for an export beside its frames.
pub struct Made<'a> {
	/// The probes' traces, drawn when the view puts the graphs in the export.
	pub traces: &'a [ProbeTrace],
	pub frame_count: usize,
	pub total_steps: u64,
}

/// Exports `request` from frames already made (`source`, a live run's store); the project is not run again.
pub fn export_frames<'s>(
	request: &Request,
	source: Arc<dyn FrameSource + 's>,
	made: &Made<'_>,
	progress: impl FnMut(usize, usize),
	cancel: &AtomicBool,
) -> Result<ExportReport, Error> {
	if request.still.is_some() && !request.format.supports_still() {
		return Err(Error::StillOfVideo);
	}
	// an archive's outputs go into a folder beside it first, packed once complete
	let parts = request.archive.as_deref().map(Parts::new).transpose().map_err(Error::Archive)?;
	let out = match &parts {
		Some(parts) => parts.0.join(request.out.file_name().unwrap_or_default()),
		None => request.out.clone(),
	};
	let graphs = request.view.probe_graphs_in_export && request.project.probes.iter().any(|p| p.active);
	let plan = ExportPlan {
		project: &request.project,
		view: &request.view,
		format: request.format,
		fs: Arc::new(StdFs),
		outputs: naming::layer_outputs(&out, request.layers),
		still: request.still,
		frame_count: made.frame_count,
		traces: if graphs { made.traces } else { &[] },
		total_steps: made.total_steps,
		keep_partial: request.keep_partial,
		h264: request.h264.clone(),
		h264_profile: request.h264_profile,
	};
	let job = plan.build_job(source, &request.schemes)?;
	let mut report = job.run(progress, cancel);
	if let (Some(archive), Some(folder)) = (&request.archive, parts)
		&& matches!(report.state, ExportState::Completed)
	{
		match pack(&folder.0, archive, cancel) {
			Ok(()) => report.written = vec![archive.clone()],
			Err(_) if cancel.load(Ordering::Relaxed) => {
				report.state = ExportState::Cancelled;
				report.written.clear();
			}
			Err(error) => return Err(Error::ArchiveKept { error, kept: folder.keep() }),
		}
	}
	Ok(report)
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
	let made =
		Made { traces: &traces, frame_count: schedule.frame_count() as usize, total_steps: u64::from(schedule.steps) };
	#[allow(clippy::arc_with_non_send_sync, reason = "the job shares the source on this thread")]
	let source = Arc::new(LiveFrames { run: &run, store: store.clone(), cancel, waited: AtomicU64::new(0) });
	let report = export_frames(&request, source.clone(), &made, progress, cancel)?;
	if cancel.load(Ordering::Relaxed) {
		run.cancel();
	}
	Ok(Done {
		report,
		waited: std::time::Duration::from_nanos(source.waited.load(Ordering::Relaxed)),
		shape: *store.shape(),
	})
}
