//! The browser's export: the job stepped on the page between frames, its files in memory (a still) or in the spool
//! ([`Spool`]), then packed when both layers' videos go into one archive, and downloaded ([`Delivery`]).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wasm_bindgen_futures::spawn_local;
use wavedude_app_state::document::Target;
use wavedude_app_state::platform::{self, Duration, Spool, SystemTime};
use wavedude_core::run::Token;
use wavedude_export::fs::zip::UtcDateTime;
use wavedude_export::fs::{MemFs, OutputFs, ZipFs};
use wavedude_export::{ExportJob, ExportState, Step};

use super::delivery::Delivery;
use super::{Outcome, Plan, Work};

/// How long a step writes frames before the page draws again.
const BUDGET: Duration = Duration::from_millis(8);
/// Bytes of a spooled video read and written into the archive at a time.
const CHUNK: u64 = 4 << 20;

/// An export started: ended at once (a probe's CSV), or running.
pub enum Started {
	Done(Outcome),
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
	Ended(Outcome),
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
	Downloading(Token<()>),
	Ended,
}

/// A browser export from its first frame to its download. The spool is removed when the export ends other than
/// completed, else kept for its download until the next export.
pub struct Runner {
	delivery: Delivery,
	phase: Phase,
	spool: Option<Arc<Spool>>,
	cancel: Arc<AtomicBool>,
	frames: (usize, usize),
}

/// Starts `plan`, `cancel` stopping it.
#[expect(clippy::arc_with_non_send_sync, reason = "the browser's file stores are not Send; the Arc is the job's type")]
pub fn start(work: &Work, plan: Plan, cancel: Arc<AtomicBool>) -> Result<Started, String> {
	let delivery = Delivery::of(&plan);
	let (choice, format, path, frame) = match plan {
		Plan::Csv { probe, path } => {
			let mut bytes = Vec::new();
			let written = work.probe_csv(&probe, &mut bytes)?;
			download(&path, &bytes).map_err(|e| e.to_string())?;
			return Ok(Started::Done(Outcome::Completed(format!("{written} downloaded as {}", path.display()))));
		}
		Plan::Frames { choice, format, path, frame, .. } => (choice, format, path, frame),
	};
	let mut spool = None;
	let (fs, out): (Arc<dyn OutputFs>, Out) = match &delivery {
		Delivery::Still(_) | Delivery::Csv(_) => {
			let memory = Arc::new(MemFs::new());
			(memory.clone(), Out::Memory(memory))
		}
		Delivery::Archive(archive) => {
			let store = spool.insert(new_spool()?).clone();
			let zip = Arc::new(ZipFs::in_store(store, archive, now()).map_err(|e| e.to_string())?);
			(zip.clone(), Out::Archive(zip))
		}
		Delivery::Video(_) | Delivery::Videos { .. } => (spool.insert(new_spool()?).clone(), Out::Spool),
	};
	let job = work.export_plan(choice, format, path, frame, fs).build_job(work.store.clone(), &work.schemes);
	let job = job.map_err(|e| e.to_string())?;
	let frames = job.progress();
	Ok(Started::Running(Runner { delivery, phase: Phase::Stepping(Box::new(job), out), spool, cancel, frames }))
}

fn new_spool() -> Result<Arc<Spool>, String> {
	Spool::new().map(Arc::new).map_err(|e| e.to_string())
}

/// Now, the archive entries' time.
fn now() -> UtcDateTime {
	let since = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
	UtcDateTime::from_unix(since.as_secs() as i64)
}

/// Downloads `bytes` as the file name `path`.
fn download(path: &Path, bytes: &[u8]) -> io::Result<()> {
	platform::write(&Target::Download(path.display().to_string()), bytes)
}

impl Runner {
	/// Takes the next step: some frames, or a look at the packing or the download.
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
						ExportState::Cancelled => return self.end(cancelled(done)),
						ExportState::Failed(e) => Err(e.to_string()),
					},
				}
			}
			Phase::Packaging(_) | Phase::Downloading(_) if self.cancel.load(Ordering::Relaxed) => {
				return self.end(cancelled(self.frames.0));
			}
			Phase::Packaging(packed) => match packed.take() {
				None => {
					self.phase = Phase::Packaging(packed);
					return Status::Packaging;
				}
				Some(packed) => packed.map_err(|e| e.to_string()).map(|()| Some(self.download_spooled())),
			},
			Phase::Downloading(downloaded) => match downloaded.take() {
				None => {
					self.phase = Phase::Downloading(downloaded);
					return Status::Waiting(self.frames.0, self.frames.1);
				}
				Some(downloaded) => downloaded.map_err(|e| e.to_string()).map(|()| None),
			},
			Phase::Ended => panic!("an ended export is not polled"),
		};
		match next {
			Ok(Some(phase)) => {
				self.phase = phase;
				self.poll()
			}
			Ok(None) => {
				let line =
					format!("{} frames exported, downloaded as {}", self.frames.0, self.delivery.download().display());
				self.end(Outcome::Completed(line))
			}
			Err(e) => self.end(Outcome::Failed(e)),
		}
	}

	/// The spool of a completed export, holding its download.
	pub fn take_spool(&mut self) -> Option<Arc<Spool>> {
		self.spool.take()
	}

	/// What follows the job's completion; `None` once downloaded.
	fn deliver(&mut self, out: Out) -> Result<Option<Phase>, String> {
		match (out, &self.delivery) {
			(Out::Memory(memory), delivery) => {
				let path = delivery.download();
				let bytes = memory.take(path).ok_or_else(|| format!("{} was not written", path.display()))?;
				download(path, &bytes).map_err(|e| e.to_string())?;
				Ok(None)
			}
			(Out::Archive(zip), _) => {
				zip.finish().map_err(|e| e.to_string())?;
				Ok(Some(self.download_spooled()))
			}
			(Out::Spool, Delivery::Videos { videos, archive }) => {
				let spool = self.spool.clone().expect("a spooled export has a spool");
				let packed = Token::default();
				let (task, videos, archive, cancel) =
					(packed.clone(), videos.clone(), archive.clone(), self.cancel.clone());
				spawn_local(async move {
					task.complete(package(&spool, &videos, &archive, &cancel).await);
					wavedude_app_state::wake::wake();
				});
				Ok(Some(Phase::Packaging(packed)))
			}
			(Out::Spool, _) => Ok(Some(self.download_spooled())),
		}
	}

	fn download_spooled(&self) -> Phase {
		let spool = self.spool.as_ref().expect("a spooled export has a spool");
		let path = self.delivery.download();
		Phase::Downloading(spool.download(path, &path.display().to_string()))
	}

	fn end(&mut self, outcome: Outcome) -> Status {
		self.phase = Phase::Ended;
		if !matches!(outcome, Outcome::Completed(_)) {
			self.spool = None;
		}
		Status::Ended(outcome)
	}
}

fn cancelled(frames: usize) -> Outcome {
	Outcome::Cancelled(format!("export cancelled after {frames} frames"))
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
