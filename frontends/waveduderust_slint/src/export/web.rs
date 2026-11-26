//! The browser's exports (decision 0025): the dialog takes file names alone; the job is stepped on the page between
//! frames over the live run's store, its files in memory (a still) or in the spool ([`Spool`]), several videos packed
//! into one ZIP archive afterwards; the file is then made ready for the download the user starts.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wasm_bindgen_futures::spawn_local;
use waveduderust::io::schemes::SchemeStore;
use waveduderust::model::{Layer, Project, View};
use waveduderust::run::Token;
use waveduderust::unstable::export::fs::zip::UtcDateTime;
use waveduderust::unstable::export::fs::{MemFs, OutputFs, ZipFs};
use waveduderust::unstable::export::h264::Profile;
use waveduderust::unstable::export::naming::layer_outputs;
use waveduderust::unstable::export::{ExportFormat, ExportJob, ExportPlan, ExportState, Step};
use waveduderust_platform::settings::Settings;
use waveduderust_remote::opfs::Spool;
use web_sys::Blob;
use web_time::Duration;

use super::webcodecs::{Support, WebCodecsFactory, support};
use super::{Kind, Written, outputs_line, profile_index, with_format_extension};
use crate::document::Document;
use crate::files::Download;
use crate::live::{ExportFrames, Needs};
use crate::ui::{ExportData, ExportFileFormat, ExportProgress, ExportReason, Words};

/// How long a step writes frames before the page draws again.
const BUDGET: Duration = Duration::from_millis(8);
/// Bytes of a spooled video read and written into the archive at a time.
const CHUNK: u64 = 4 << 20;

thread_local! {
	/// The spool of the last completed export with one, which holds its download until the next such export
	/// completes: a download in progress outlives the next export's start.
	static KEPT: RefCell<Option<Arc<Spool>>> = const { RefCell::new(None) };
}

/// Where an export's files go and what is downloaded; each path is also the download's name.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Delivery {
	/// A still written to memory.
	Still(PathBuf),
	/// The image sequences in this ZIP archive of the spool.
	Archive(PathBuf),
	/// One video in the spool.
	Video(PathBuf),
	/// The videos in the spool, then packed into the ZIP archive `archive`.
	Videos { videos: Vec<PathBuf>, archive: PathBuf },
}

impl Delivery {
	/// The file downloaded.
	fn download(&self) -> &Path {
		match self {
			Self::Still(path) | Self::Archive(path) | Self::Video(path) => path,
			Self::Videos { archive, .. } => archive,
		}
	}
}

/// What an export writes: the dialog's answer, repeated by Export last.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
	pub(crate) kind: Kind,
	format: ExportFormat,
	/// Each layer's output, a file name.
	outputs: Vec<(Layer, PathBuf)>,
	delivery: Delivery,
}

impl Plan {
	/// What the export needs of its run.
	pub(crate) fn needs(&self, view: &View) -> Needs {
		self.kind.needs(self.format, view)
	}

	/// None: a download replaces nothing.
	pub(crate) fn existing(&self) -> Vec<(PathBuf, bool)> {
		Vec::new()
	}
}

/// The file name of what is typed in a name box: its last component, `format`'s extension replacing an export
/// format's and appended to any other suffix; `None` without a name.
fn typed_name(typed: &str, format: ExportFormat) -> Option<PathBuf> {
	let name = Path::new(typed.trim()).file_name()?;
	Some(with_format_extension(Path::new(name), format))
}

/// The file name of what is typed, `.<ext>` appended unless it ends so; `None` without a name.
fn typed_with(typed: &str, ext: &str) -> Option<PathBuf> {
	let mut name = PathBuf::from(Path::new(typed.trim()).file_name()?);
	if !name.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
		name.as_mut_os_string().push(format!(".{ext}"));
	}
	Some(name)
}

/// The picture size and rate of `project`'s videos in `view`: its canvas, which an export's full-quality run keeps.
fn video_format(project: &Project, view: &View) -> (u32, u32, u32) {
	let (width, height) = project.canvas_size();
	(width, height, view.video.fps)
}

/// Whether a run's export can be spooled: the browser writes files to its private storage and the session is made.
fn spool_available() -> bool {
	crate::web::start_spill();
	Spool::available().is_ok()
}

/// The export dialog while it is open.
pub(crate) enum Dialog {
	Export(Exports),
	/// A probe's recording as CSV, downloaded under the name typed.
	Csv(Vec<u8>),
}

impl Dialog {
	/// The dialog of `kind` for `doc` named `name`, the shown timeline frame `frame`.
	pub(crate) fn open(
		kind: Kind,
		doc: &Document,
		name: &str,
		frame: u32,
		settings: &Settings,
		words: &Words<'_>,
	) -> Option<(Self, ExportData)> {
		let (dialog, data) = Exports::open(kind, doc, name, frame, settings, words);
		Some((Self::Export(dialog), data))
	}

	/// The dialog of the probe named `probe`, whose recording is `csv`.
	pub(crate) fn csv(probe: &str, csv: Vec<u8>, words: &Words<'_>) -> (Self, ExportData) {
		let data = ExportData {
			title: words.get_export_probe_data(),
			formats: slint::ModelRc::new(slint::VecModel::from(vec![
				format!("{} (*.csv)", words.get_csv_files()).into(),
			])),
			path: format!("{probe}.csv").into(),
			codec_built_in: true,
			browser: true,
			..ExportData::default()
		};
		(Self::Csv(csv), data)
	}

	/// The format's flags, the outputs the name gives and why the export cannot start.
	pub(crate) fn follow(&self, data: &mut ExportData) {
		if let Self::Export(dialog) = self {
			dialog.follow(data);
		}
	}

	/// Follows the browser's late answers; whether `data` changed.
	pub(crate) fn refresh(&self, data: &mut ExportData) -> bool {
		match self {
			Self::Export(dialog) => dialog.refresh(data),
			Self::Csv(_) => false,
		}
	}

	/// Picks format `index`: the name takes its extension.
	pub(crate) fn format(&self, data: &mut ExportData, index: i32) {
		if let Self::Export(dialog) = self {
			dialog.format(data, index);
		}
	}

	/// No platform dialog: the box takes a file name.
	pub(crate) fn browse(&self, _: &mut ExportData, _: &Words<'_>) {}

	/// No platform dialog: the box takes a file name.
	pub(crate) fn browse_archive(&self, _: &mut ExportData, _: &Words<'_>) {}

	/// The plan of the dialog's answer, the settings it leaves remembered; the reason when a box names no file.
	pub(crate) fn plan(&self, data: &ExportData, settings: &mut Settings) -> Result<Plan, String> {
		match self {
			Self::Export(dialog) => dialog.plan(data, settings),
			Self::Csv(_) => Err("the probe data is no run's export".into()),
		}
	}

	/// The download of a CSV dialog's answer, or why there is none; `None` for a run's or a frame's export.
	pub(crate) fn csv_download(&self, data: &ExportData) -> Option<Result<Download, String>> {
		let Self::Csv(csv) = self else { return None };
		Some(
			typed_with(data.path.as_str(), "csv")
				.ok_or_else(|| "Enter a file name.".to_owned())
				.and_then(|name| Download::of_bytes(csv, &name.to_string_lossy())),
		)
	}
}

/// The dialog of a run's or a frame's export.
pub(crate) struct Exports {
	kind: Kind,
	formats: Vec<(ExportFormat, ExportFileFormat)>,
	/// The picture size and rate of a video.
	video: (u32, u32, u32),
}

impl Exports {
	/// The dialog of `kind` for `doc` named `name`, the shown timeline frame `frame`.
	fn open(
		kind: Kind,
		doc: &Document,
		name: &str,
		frame: u32,
		settings: &Settings,
		words: &Words<'_>,
	) -> (Self, ExportData) {
		let formats = kind.formats();
		let (format, index) = kind.first_format(&formats, settings);
		let video = video_format(&doc.project, &doc.view);
		// asked as the dialog opens, whatever the format
		support(video.0, video.1, video.2);
		let mut data = ExportData {
			title: kind.title(words),
			formats: kind.format_names(&formats, words),
			format: index,
			path: kind.proposed(name, frame, format.default_extension()).into(),
			codec_built_in: true,
			archive: true,
			archive_path: format!("{name}.zip").into(),
			profile: profile_index(settings),
			browser: true,
			..ExportData::default()
		};
		let dialog = Self { kind, formats, video };
		dialog.follow(&mut data);
		(dialog, data)
	}

	fn format_of(&self, data: &ExportData) -> ExportFormat {
		usize::try_from(data.format).ok().and_then(|i| self.formats.get(i)).map_or(ExportFormat::Png, |(f, _)| *f)
	}

	/// The format's flags, the outputs the name gives and why the export cannot start.
	fn follow(&self, data: &mut ExportData) {
		let format = self.format_of(data);
		data.mp4 = format == ExportFormat::Mp4;
		data.several = self.kind.several(format);
		data.outputs = outputs_line(self.kind, Path::new(data.path.as_str()), format);
		let mut supported = Vec::new();
		data.reason = match self.kind {
			Kind::Run(_) if !spool_available() => ExportReason::NoStorage,
			Kind::Run(_) if data.mp4 => match support(self.video.0, self.video.1, self.video.2) {
				Support::Unavailable => ExportReason::NoWebcodecs,
				Support::Probing => ExportReason::Probing,
				Support::Profiles(profiles) if profiles.is_empty() => ExportReason::NoProfile,
				Support::Profiles(profiles) => {
					supported = Profile::ALL.iter().map(|p| profiles.contains(p)).collect();
					ExportReason::None
				}
			},
			_ => ExportReason::None,
		};
		data.profile_supported = slint::ModelRc::new(slint::VecModel::from(supported));
		let mp4 = match self.kind {
			Kind::Run(_) => match support(self.video.0, self.video.1, self.video.2) {
				Support::Unavailable => false,
				Support::Profiles(profiles) => !profiles.is_empty(),
				Support::Probing => true,
			},
			Kind::Frame(_) => true,
		};
		let enabled: Vec<bool> = self.formats.iter().map(|(f, _)| *f != ExportFormat::Mp4 || mp4).collect();
		data.format_enabled = slint::ModelRc::new(slint::VecModel::from(enabled));
	}

	/// [`follow`](Self::follow) while the export cannot start, as the browser answers which profiles it encodes or
	/// the session is made; whether `data` changed.
	fn refresh(&self, data: &mut ExportData) -> bool {
		let reason = data.reason;
		if reason == ExportReason::None {
			return false;
		}
		self.follow(data);
		data.reason != reason
	}

	/// Picks format `index`: the name takes its extension.
	fn format(&self, data: &mut ExportData, index: i32) {
		data.format = index;
		if let Some(name) = typed_name(data.path.as_str(), self.format_of(data)) {
			data.path = name.to_string_lossy().as_ref().into();
		}
		self.follow(data);
	}

	fn plan(&self, data: &ExportData, settings: &mut Settings) -> Result<Plan, String> {
		let format = self.format_of(data);
		let out = typed_name(data.path.as_str(), format).ok_or("Enter a file name.")?;
		if let Some(i) = ExportFormat::ALL.iter().position(|f| *f == format) {
			settings.export_filter_index = i as u32;
		}
		if data.mp4 {
			settings.h264_profile =
				Profile::ALL.get(usize::try_from(data.profile).unwrap_or(0)).copied().unwrap_or_default();
		}
		let outputs = layer_outputs(&out, self.kind.layers());
		let delivery = match self.kind {
			Kind::Frame(_) => Delivery::Still(outputs[0].1.clone()),
			Kind::Run(_) if !self.kind.several(format) => Delivery::Video(outputs[0].1.clone()),
			Kind::Run(_) => {
				let archive = typed_with(data.archive_path.as_str(), "zip").ok_or("Enter an archive name.")?;
				if format.is_video() {
					Delivery::Videos { videos: outputs.iter().map(|(_, video)| video.clone()).collect(), archive }
				} else {
					Delivery::Archive(archive)
				}
			}
		};
		Ok(Plan { kind: self.kind, format, outputs, delivery })
	}
}

/// The dialog's answer is remembered as it is planned: a download has no folder.
pub(crate) fn remember(_: &Plan, _: Option<&Path>, _: &mut Settings) {}

/// The browser encodes through WebCodecs: no codec is downloaded.
pub(crate) fn delete_codec(_: &mut ExportData) {}

/// An export waiting for its run, or writing.
pub(crate) struct Exporting {
	cancel: Arc<AtomicBool>,
	/// The plan waiting for its run and the timeline frame of its still.
	waiting: Option<(Plan, u32)>,
	state: State,
}

enum State {
	Waiting,
	/// Has its frames; waits for the browser to say which H.264 profiles it encodes.
	Probing(Box<Inputs>),
	Running(Runner),
	/// Failed before its first frame.
	Failed(String),
	Ended,
}

impl Exporting {
	/// `plan` waits for its run; a still is of the timeline frame `position`.
	pub(crate) fn waiting(plan: &Plan, position: u32) -> Self {
		Self { cancel: Arc::default(), waiting: Some((plan.clone(), position)), state: State::Waiting }
	}

	/// The plan of an export waiting for its run, with its still's timeline frame.
	pub(crate) fn waits(&self) -> Option<&(Plan, u32)> {
		self.waiting.as_ref()
	}

	pub(crate) fn cancel(&self) {
		self.cancel.store(true, Ordering::Relaxed);
	}

	/// Cancels it; its spool and the files in it are removed.
	pub(crate) fn stop(&mut self) {
		self.cancel();
		self.state = State::Ended;
	}

	/// Steps it: the progress while it waits or writes; its outcome once it ended.
	pub(crate) fn poll(&mut self) -> Result<ExportProgress, Result<Written, String>> {
		let progress = |done: usize, total: usize| ExportProgress {
			active: true,
			done: i32::try_from(done).unwrap_or(i32::MAX),
			total: i32::try_from(total).unwrap_or(i32::MAX),
			..ExportProgress::default()
		};
		match std::mem::replace(&mut self.state, State::Ended) {
			State::Waiting => {
				self.state = State::Waiting;
				Ok(ExportProgress { active: true, waiting: true, ..ExportProgress::default() })
			}
			State::Probing(inputs) if inputs.probing() => {
				self.state = State::Probing(inputs);
				Ok(ExportProgress { active: true, waiting: true, ..ExportProgress::default() })
			}
			State::Probing(inputs) => {
				self.state = inputs.start(self.cancel.clone());
				self.poll()
			}
			State::Running(mut runner) => {
				let shown = match runner.poll() {
					Status::Working(done, total) => progress(done, total),
					Status::Packing => ExportProgress { active: true, packing: true, ..ExportProgress::default() },
					Status::Ended(outcome) => {
						if let (Ok(_), Some(spool)) = (&outcome, runner.spool.take()) {
							KEPT.set(Some(spool));
						}
						return Err(outcome);
					}
				};
				self.state = State::Running(runner);
				Ok(shown)
			}
			State::Failed(e) => Err(Err(e)),
			State::Ended => Err(Err("the export ended".into())),
		}
	}
}

/// Starts writing `plan` from the run's `frames`, a still at the timeline frame `position`.
pub(crate) fn start(
	plan: &Plan,
	frames: ExportFrames,
	project: &Project,
	view: &View,
	position: u32,
	settings: &Settings,
	schemes: &SchemeStore,
) -> Exporting {
	let cancel = Arc::new(AtomicBool::new(false));
	let inputs = Inputs {
		plan: plan.clone(),
		frames,
		project: project.clone(),
		view: view.clone(),
		position,
		settings: settings.clone(),
		schemes: schemes.clone(),
	};
	let state = if inputs.probing() { State::Probing(Box::new(inputs)) } else { inputs.start(cancel.clone()) };
	Exporting { cancel, waiting: None, state }
}

/// What an export starts from.
struct Inputs {
	plan: Plan,
	frames: ExportFrames,
	project: Project,
	view: View,
	position: u32,
	settings: Settings,
	schemes: SchemeStore,
}

impl Inputs {
	/// An MP4 export whose H.264 profiles the browser is still asked for (Export last skips the dialog that waits).
	fn probing(&self) -> bool {
		self.plan.format == ExportFormat::Mp4 && {
			let (width, height, fps) = video_format(&self.project, &self.view);
			matches!(support(width, height, fps), Support::Probing)
		}
	}

	fn start(self, cancel: Arc<AtomicBool>) -> State {
		let Self { plan, frames, project, view, position, settings, schemes } = self;
		match Runner::start(&plan, &frames, &project, &view, position, &settings, &schemes, cancel) {
			Ok(runner) => State::Running(runner),
			Err(e) => State::Failed(format!("the export failed: {e}")),
		}
	}
}

/// Where a running export is.
enum Status {
	/// `(done, total)` frames.
	Working(usize, usize),
	/// The videos are packed into the archive.
	Packing,
	/// How it ended.
	Ended(Result<Written, String>),
}

/// Where the job's files go.
enum Out {
	Memory(Arc<MemFs>),
	/// A ZIP archive in the spool.
	Archive(Arc<ZipFs>),
	Spool,
}

enum Phase {
	Stepping(Box<ExportJob<'static>>, Out),
	Packing(Token<()>),
	/// The spooled file on its way to its download.
	Preparing(Token<Blob>),
	Ended,
}

/// What follows a phase.
enum Next {
	Phase(Phase),
	/// The file is ready for its download.
	Ready(Download),
}

/// A browser export from its first frame to its file ready for download. The spool is removed when the export ends
/// other than completed, else kept for its download until the next export with a spool completes.
struct Runner {
	delivery: Delivery,
	phase: Phase,
	spool: Option<Arc<Spool>>,
	cancel: Arc<AtomicBool>,
	frames: (usize, usize),
}

/// Now, the archive entries' time.
fn now() -> UtcDateTime {
	UtcDateTime::from_unix((js_sys::Date::now() / 1000.0) as i64)
}

fn new_spool() -> io::Result<Arc<Spool>> {
	Spool::new().map(Arc::new)
}

impl Runner {
	#[expect(clippy::too_many_arguments, reason = "the export's inputs, as the native start takes them")]
	#[expect(
		clippy::arc_with_non_send_sync,
		reason = "the browser's file stores are not Send; the Arc is the job's type"
	)]
	fn start(
		plan: &Plan,
		frames: &ExportFrames,
		project: &Project,
		view: &View,
		position: u32,
		settings: &Settings,
		schemes: &SchemeStore,
		cancel: Arc<AtomicBool>,
	) -> Result<Self, String> {
		let mut spool = None;
		let (fs, out): (Arc<dyn OutputFs>, Out) = match &plan.delivery {
			Delivery::Still(_) => {
				let memory = Arc::new(MemFs::new());
				(memory.clone(), Out::Memory(memory))
			}
			Delivery::Archive(archive) => {
				let store = spool.insert(new_spool().map_err(|e| e.to_string())?).clone();
				let zip = Arc::new(ZipFs::in_store(store, archive, now()).map_err(|e| e.to_string())?);
				(zip.clone(), Out::Archive(zip))
			}
			Delivery::Video(_) | Delivery::Videos { .. } => {
				(spool.insert(new_spool().map_err(|e| e.to_string())?).clone(), Out::Spool)
			}
		};
		let graphs = view.probe_graphs_in_export && project.probes.iter().any(|p| p.active);
		let h264 = (plan.format == ExportFormat::Mp4).then(|| {
			let (width, height, fps) = video_format(project, view);
			Arc::new(WebCodecsFactory::new(width, height, fps)) as _
		});
		let export = ExportPlan {
			project,
			view,
			format: plan.format,
			fs,
			outputs: plan.outputs.clone(),
			still: matches!(plan.kind, Kind::Frame(_)).then_some(position as usize),
			frame_count: frames.frame_count,
			traces: if graphs { &frames.traces } else { &[] },
			total_steps: frames.total_steps,
			keep_partial: false,
			h264,
			h264_profile: settings.h264_profile,
		};
		let job = export.build_job(frames.store.clone(), schemes).map_err(|e| e.to_string())?;
		let frames = job.progress();
		Ok(Self { delivery: plan.delivery.clone(), phase: Phase::Stepping(Box::new(job), out), spool, cancel, frames })
	}

	/// Takes the next step: some frames, or a look at the packing or the file's preparation.
	fn poll(&mut self) -> Status {
		let phase = std::mem::replace(&mut self.phase, Phase::Ended);
		let next = match phase {
			Phase::Stepping(mut job, out) => {
				let step = job.step(BUDGET, &self.cancel);
				self.frames = job.progress();
				let (done, total) = self.frames;
				match step {
					Step::Working | Step::Waiting(_) => {
						self.phase = Phase::Stepping(job, out);
						return Status::Working(done, total);
					}
					Step::Ended(report) => match report.state {
						ExportState::Completed => self.deliver(out),
						ExportState::Cancelled => return self.end(Err(cancelled(done))),
						ExportState::Failed(e) => Err(e.to_string()),
					},
				}
			}
			Phase::Packing(_) | Phase::Preparing(_) if self.cancel.load(Ordering::Relaxed) => {
				return self.end(Err(cancelled(self.frames.0)));
			}
			Phase::Packing(packed) => match packed.take() {
				None => {
					self.phase = Phase::Packing(packed);
					return Status::Packing;
				}
				Some(packed) => packed.map_err(|e| e.to_string()).map(|()| self.prepare_spooled()),
			},
			Phase::Preparing(prepared) => match prepared.take() {
				None => {
					self.phase = Phase::Preparing(prepared);
					return Status::Working(self.frames.0, self.frames.1);
				}
				Some(blob) => blob.map_err(|e| e.to_string()).and_then(|blob| {
					Download::of_blob(&blob, &self.delivery.download().display().to_string()).map(Next::Ready)
				}),
			},
			Phase::Ended => return Status::Ended(Err("the export ended".into())),
		};
		match next {
			Ok(Next::Phase(phase)) => {
				self.phase = phase;
				self.poll()
			}
			Ok(Next::Ready(download)) => {
				let line = format!("{} frames exported as {}", self.frames.0, download.name());
				self.end(Ok(Written { line, download }))
			}
			Err(e) => self.end(Err(format!("the export failed after {} frames: {e}", self.frames.0))),
		}
	}

	/// What follows the job's completion.
	fn deliver(&mut self, out: Out) -> Result<Next, String> {
		match (out, &self.delivery) {
			(Out::Memory(memory), delivery) => {
				let path = delivery.download();
				let bytes = memory.take(path).ok_or_else(|| format!("{} was not written", path.display()))?;
				Download::of_bytes(&bytes, &path.display().to_string()).map(Next::Ready)
			}
			(Out::Archive(zip), _) => {
				zip.finish().map_err(|e| e.to_string())?;
				Ok(self.prepare_spooled())
			}
			(Out::Spool, Delivery::Videos { videos, archive }) => {
				let spool = self.spool.clone().ok_or("the export has no spool")?;
				let packed = Token::default();
				let (task, videos, archive, cancel) =
					(packed.clone(), videos.clone(), archive.clone(), self.cancel.clone());
				spawn_local(async move {
					task.complete(package(&spool, &videos, &archive, &cancel).await);
					crate::wake::wake();
				});
				Ok(Next::Phase(Phase::Packing(packed)))
			}
			(Out::Spool, _) => Ok(self.prepare_spooled()),
		}
	}

	fn prepare_spooled(&self) -> Next {
		let file = match &self.spool {
			Some(spool) => spool.file(self.delivery.download()),
			None => {
				let token = Token::default();
				token.complete(Err(io::Error::other("the export has no spool")));
				token
			}
		};
		Next::Phase(Phase::Preparing(file))
	}

	fn end(&mut self, outcome: Result<Written, String>) -> Status {
		self.phase = Phase::Ended;
		if outcome.is_err() {
			self.spool = None;
		}
		Status::Ended(outcome)
	}
}

fn cancelled(done: usize) -> String {
	format!("the export was stopped after {done} frames")
}

/// Packs the spooled `videos` into the ZIP archive `archive` of `spool`, chunk by chunk as the spool takes them,
/// each removed once packed.
async fn package(spool: &Arc<Spool>, videos: &[PathBuf], archive: &Path, cancel: &AtomicBool) -> io::Result<()> {
	let mut zip = ZipFs::in_store(spool.clone(), archive, now())?;
	for video in videos {
		let reader = spool.reader(video);
		reader.done().await;
		let reader = reader.take().ok_or_else(|| io::Error::other("the spool's reader was taken"))??;
		let (size, mut at) = (reader.size(), 0);
		zip.write_entry(video, async || {
			if cancel.load(Ordering::Relaxed) {
				return Err(io::Error::new(io::ErrorKind::Interrupted, "the export was cancelled"));
			}
			if at >= size {
				return Ok(None);
			}
			spool.writable().await?;
			let end = (at + CHUNK).min(size);
			let read = reader.read(at..end);
			read.done().await;
			at = end;
			read.take().ok_or_else(|| io::Error::other("the read was taken"))?.map(Some)
		})
		.await?;
		spool.remove_file(video)?;
	}
	zip.finish()
}
