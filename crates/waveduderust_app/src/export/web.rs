//! The browser's export: the job stepped on the page between frames, its files in memory (a still) or in the spool
//! ([`Spool`]), then packed when several videos go into one archive, and made ready for the download the user starts
//! ([`Delivery`], [`Download`]).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::platform::{self, Download, Duration, H264Support, Spool, WebCodecsFactory};
use wasm_bindgen_futures::spawn_local;
use waveduderust::model::{Project, View};
use waveduderust::run::Token;
use waveduderust::unstable::export::fs::{MemFs, OutputFs, ZipFs};
use waveduderust::unstable::export::{ExportFormat, ExportJob, ExportState, Step};

use super::delivery::Delivery;
use super::{Failure, Outcome, Plan, Work, now};

/// The H.264 profiles the browser encodes for the project's MP4, every one while that is unknown, and the string key
/// of why the others cannot be picked.
pub fn h264_profiles(app: &crate::App) -> (Vec<waveduderust::unstable::export::h264::Profile>, &'static str) {
	let profiles = match h264_support(&app.document.project, &app.document.view) {
		H264Support::Profiles(profiles) if !profiles.is_empty() => profiles,
		_ => waveduderust::unstable::export::h264::Profile::ALL.to_vec(),
	};
	(profiles, "ExportH264NotInBrowser")
}

/// An export of several files is always archived, as it leaves as one download.
pub(super) const ARCHIVE_CHOSEN: bool = false;

/// The dialog's text of a proposed output: its file name.
pub(super) fn shown(path: &Path) -> String {
	path.file_name().map_or_default(|n| n.to_string_lossy().into_owned())
}

/// The file name typed, with extension `ext` when it has none; `None` without a name.
pub(super) fn typed(text: &str, ext: &str) -> Option<PathBuf> {
	super::delivery::typed_name(text, ext)
}

/// An export of several files goes into one archive: always.
pub(super) fn archived(_: &crate::App) -> bool {
	true
}

/// Why MP4 cannot be exported with `support`.
pub(super) fn mp4_unavailable(support: &H264Support) -> Option<&'static str> {
	match support {
		H264Support::Unavailable => Some("ExportNoWebCodecs"),
		H264Support::Profiles(profiles) if profiles.is_empty() => Some("ExportH264NoProfile"),
		H264Support::Probing | H264Support::Profiles(_) => None,
	}
}

/// Why `format` cannot be picked, a string key: MP4 when the browser has no H.264 encoder for the project's videos.
pub(super) fn unavailable(app: &crate::App, format: ExportFormat) -> Option<&'static str> {
	(format == ExportFormat::Mp4)
		.then(|| mp4_unavailable(&h264_support(&app.document.project, &app.document.view)))
		.flatten()
}

/// How long a step writes frames before the page draws again.
const BUDGET: Duration = Duration::from_millis(8);
/// Bytes of a spooled video read and written into the archive at a time.
const CHUNK: u64 = 4 << 20;

/// An export started: ended at once (a probe's CSV), with its file when completed, or running.
pub enum Started {
	Done(Outcome, Option<Download>),
	Running(Runner),
}

/// Where a running export is.
pub enum Status {
	/// `(done, total)` frames; poll again at once.
	Working(usize, usize),
	/// `(done, total)` frames; poll again once the window is woken.
	Waiting(usize, usize),
	/// The videos are packed into the archive; poll again once the window is woken.
	Packaging,
	/// How it ended, with its file when completed.
	Ended(Outcome, Option<Download>),
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
	Packaging(Token<()>),
	/// The spooled file on its way to its download.
	Preparing(Token<Download>),
	Ended,
}

/// What follows a phase.
enum Next {
	Phase(Phase),
	/// The file is ready for its download.
	Ready(Download),
}

/// A browser export from its first frame to its file ready for download. The spool is removed when the export ends
/// other than completed, else kept for its download until the next export.
pub struct Runner {
	delivery: Delivery,
	phase: Phase,
	spool: Option<Arc<Spool>>,
	cancel: Arc<AtomicBool>,
	frames: (usize, usize),
}

/// Starts `plan`, `cancel` stopping it.
#[expect(clippy::arc_with_non_send_sync, reason = "the browser's file stores are not Send; the Arc is the job's type")]
pub fn start(work: &Work, plan: Plan, cancel: Arc<AtomicBool>) -> Result<Started, Failure> {
	let delivery = Delivery::of(&plan);
	let (choice, format, outputs, frame) = match plan {
		Plan::Csv { probe, path } => {
			let mut bytes = Vec::new();
			let written = work.probe_csv(&probe, &mut bytes)?;
			let download = Download::of_bytes(&bytes, &path.display().to_string()).map_err(Failure::from)?;
			let line = format!("{written} exported as {}", path.display());
			return Ok(Started::Done(Outcome::Completed(line), Some(download)));
		}
		Plan::Frames { choice, format, outputs, frame, .. } => (choice, format, outputs, frame),
	};
	let mut spool = None;
	let (fs, out): (Arc<dyn OutputFs>, Out) = match &delivery {
		Delivery::Still(_) | Delivery::Csv(_) => {
			let memory = Arc::new(MemFs::new());
			(memory.clone(), Out::Memory(memory))
		}
		Delivery::Archive(archive) => {
			let store = spool.insert(new_spool()?).clone();
			let zip = Arc::new(ZipFs::in_store(store, archive, now()).map_err(Failure::from)?);
			(zip.clone(), Out::Archive(zip))
		}
		Delivery::Video(_) | Delivery::Videos { .. } => (spool.insert(new_spool()?).clone(), Out::Spool),
	};
	let mut plan = work.export_plan(choice, format, outputs, frame, fs);
	if format == ExportFormat::Mp4 {
		let (width, height, fps) = video_format(&work.project, &work.view);
		plan.h264 = Some(Arc::new(WebCodecsFactory::new(width, height, fps)));
	}
	let job = plan.build_job(work.store.clone(), &work.schemes).map_err(Failure::from)?;
	let frames = job.progress();
	Ok(Started::Running(Runner { delivery, phase: Phase::Stepping(Box::new(job), out), spool, cancel, frames }))
}

/// The picture size and rate of `project`'s videos in `view`: its canvas, which an export's full-quality run keeps.
fn video_format(project: &Project, view: &View) -> (u32, u32, u32) {
	let (width, height) = project.canvas_size();
	(width, height, view.video.fps)
}

/// The H.264 profiles the browser encodes for `project`'s MP4 in `view`, asked the first time.
pub fn h264_support(project: &Project, view: &View) -> H264Support {
	let (width, height, fps) = video_format(project, view);
	platform::h264_support(width, height, fps)
}

fn new_spool() -> Result<Arc<Spool>, Failure> {
	Spool::new().map(Arc::new).map_err(Failure::from)
}

impl Runner {
	/// Takes the next step: some frames, or a look at the packing or the file's preparation.
	pub fn poll(&mut self) -> Status {
		let phase = std::mem::replace(&mut self.phase, Phase::Ended);
		let next = match phase {
			Phase::Stepping(mut job, out) => {
				let step = job.step(BUDGET, &self.cancel);
				self.frames = job.progress();
				let (done, total) = self.frames;
				match step {
					Step::Working => {
						self.phase = Phase::Stepping(job, out);
						return Status::Working(done, total);
					}
					Step::Waiting(_) => {
						self.phase = Phase::Stepping(job, out);
						return Status::Waiting(done, total);
					}
					Step::Ended(report) => match report.state {
						ExportState::Completed => self.deliver(out),
						ExportState::Cancelled => return self.end(Outcome::cancelled(done), None),
						ExportState::Failed(e) => Err(e.into()),
					},
				}
			}
			Phase::Packaging(_) | Phase::Preparing(_) if self.cancel.load(Ordering::Relaxed) => {
				return self.end(Outcome::cancelled(self.frames.0), None);
			}
			Phase::Packaging(packed) => match packed.take() {
				None => {
					self.phase = Phase::Packaging(packed);
					return Status::Packaging;
				}
				Some(packed) => packed.map_err(Failure::from).map(|()| self.prepare_spooled()),
			},
			Phase::Preparing(prepared) => match prepared.take() {
				None => {
					self.phase = Phase::Preparing(prepared);
					return Status::Waiting(self.frames.0, self.frames.1);
				}
				Some(prepared) => prepared.map_err(Failure::from).map(Next::Ready),
			},
			Phase::Ended => panic!("an ended export is not polled"),
		};
		match next {
			Ok(Next::Phase(phase)) => {
				self.phase = phase;
				self.poll()
			}
			Ok(Next::Ready(download)) => {
				let line = format!("{} frames exported as {}", self.frames.0, download.name());
				self.end(Outcome::Completed(line), Some(download))
			}
			Err(e) => self.end(Outcome::Failed(e), None),
		}
	}

	/// The spool of a completed export, holding its download.
	pub fn take_spool(&mut self) -> Option<Arc<Spool>> {
		self.spool.take()
	}

	/// What follows the job's completion.
	fn deliver(&mut self, out: Out) -> Result<Next, Failure> {
		match (out, &self.delivery) {
			(Out::Memory(memory), delivery) => {
				let path = delivery.download();
				let bytes = memory.take(path).ok_or_else(|| format!("{} was not written", path.display()))?;
				Download::of_bytes(&bytes, &path.display().to_string()).map(Next::Ready).map_err(Failure::from)
			}
			(Out::Archive(zip), _) => {
				zip.finish().map_err(Failure::from)?;
				Ok(self.prepare_spooled())
			}
			(Out::Spool, Delivery::Videos { videos, archive }) => {
				let spool = self.spool.clone().expect("a spooled export has a spool");
				let packed = Token::default();
				let (task, videos, archive, cancel) =
					(packed.clone(), videos.clone(), archive.clone(), self.cancel.clone());
				spawn_local(async move {
					task.complete(package(&spool, &videos, &archive, &cancel).await);
					crate::wake::wake();
				});
				Ok(Next::Phase(Phase::Packaging(packed)))
			}
			(Out::Spool, _) => Ok(self.prepare_spooled()),
		}
	}

	fn prepare_spooled(&self) -> Next {
		let spool = self.spool.as_ref().expect("a spooled export has a spool");
		let path = self.delivery.download();
		Next::Phase(Phase::Preparing(spool.download(path, &path.display().to_string())))
	}

	fn end(&mut self, outcome: Outcome, download: Option<Download>) -> Status {
		self.phase = Phase::Ended;
		if !matches!(outcome, Outcome::Completed(_)) {
			self.spool = None;
		}
		Status::Ended(outcome, download)
	}
}

/// Packs the spooled `videos` into the ZIP archive `archive` of `spool`, chunk by chunk as the spool takes them,
/// each removed once packed.
async fn package(spool: &Arc<Spool>, videos: &[PathBuf], archive: &Path, cancel: &AtomicBool) -> io::Result<()> {
	let mut zip = ZipFs::in_store(spool.clone(), archive, now())?;
	for video in videos {
		let reader = spool.reader(video);
		reader.done().await;
		let reader = reader.take().expect("done")?;
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
			read.take().expect("done").map(Some)
		})
		.await?;
		spool.remove_file(video)?;
	}
	zip.finish()
}
