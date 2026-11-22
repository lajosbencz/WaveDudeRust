//! The desktop's tasks: one OS thread each, waiting on the platform's dialogs or writing an export.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use egui_mineral::bus::{BlockingJob, BlockingSpawner, BoxFuture, Spawner, SpawnerExt as _};
use egui_mineral::platform::{
	FileFilter, FileType, NativePlatform, PlatformError, PlatformPathPickers as _, PlatformPickers as _, SaveRequest,
};
use waveduderust_app::dialogs::{filters, is_scene_image, read_scene_image, scene_image_filter, write_scene_image};
use waveduderust_app::document::{Source, Target};
use waveduderust_app::export::Outcome;
use waveduderust_app::export::session::{self, Msg};
use waveduderust_app::strings::Strings;

use waveduderust_app::jobs::{Answer, Job};

use super::Task;

/// How long a task may take to end once the window closed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// One OS thread per task, joined by [`Self::reap`] once finished. On drop, running tasks
/// get [`SHUTDOWN_GRACE`]; a task waiting on a native dialog cannot be cancelled and is detached with a warning.
#[derive(Default)]
pub(super) struct ThreadSpawner(Mutex<Vec<JoinHandle<()>>>);

impl ThreadSpawner {
	fn handles(&self) -> MutexGuard<'_, Vec<JoinHandle<()>>> {
		self.0.lock().unwrap_or_else(PoisonError::into_inner)
	}

	pub(super) fn reap(&self) {
		let (done, running): (Vec<_>, Vec<_>) = self.handles().drain(..).partition(JoinHandle::is_finished);
		self.handles().extend(running);
		for h in done {
			// the panic hook logged it; the task answered as failed (`Replied`)
			let _ = h.join();
		}
	}

	/// Runs `task` with the strings `s`; `reply` gets its answer.
	/// A panic answers as failed and ends the task's thread alone (docs/ui/70, Crash report).
	pub(super) fn start(&self, task: Task, s: Strings, reply: impl FnOnce(Option<Answer>) + Send + 'static) {
		self.spawn_future(async move {
			let replied = Replied(Some(reply));
			replied.send(run(task, s).await);
		});
	}

	/// Runs the export `job`, `report` getting its progress and end; a panic ends it as failed (the panic hook logs
	/// it).
	pub(super) fn export(&self, job: Box<session::Job>, report: impl Fn(Msg) + Send + 'static) {
		self.spawn_future(async move {
			let run = std::panic::catch_unwind(AssertUnwindSafe(|| {
				job.run(|done, total| report(Msg::Progress(done, total)))
			}));
			report(Msg::Done(run.unwrap_or_else(|_| Outcome::Failed("the export thread panicked".into()))));
		});
	}
}

/// A task's reply, sent once: with its answer, or as failed when the task panics first.
struct Replied<F: FnOnce(Option<Answer>)>(Option<F>);

impl<F: FnOnce(Option<Answer>)> Replied<F> {
	fn send(mut self, answer: Option<Answer>) {
		if let Some(reply) = self.0.take() {
			reply(answer);
		}
	}
}

impl<F: FnOnce(Option<Answer>)> Drop for Replied<F> {
	fn drop(&mut self) {
		if let Some(reply) = self.0.take() {
			reply(Some(Answer::Failed("the file dialog thread panicked".into())));
		}
	}
}

impl Spawner for ThreadSpawner {
	fn spawn(&self, fut: BoxFuture) {
		let handle = std::thread::Builder::new()
			.name("job".into())
			.spawn(move || pollster::block_on(fut))
			.expect("the OS starts a thread");
		self.handles().push(handle);
	}
}

impl Drop for ThreadSpawner {
	fn drop(&mut self) {
		let deadline = Instant::now() + SHUTDOWN_GRACE;
		for h in std::mem::take(self.0.get_mut().unwrap_or_else(PoisonError::into_inner)) {
			while !h.is_finished() && Instant::now() < deadline {
				std::thread::sleep(Duration::from_millis(5));
			}
			if !h.is_finished() {
				tracing::warn!("a task did not end within {SHUTDOWN_GRACE:?}; detached");
			} else if let Err(panic) = h.join() {
				tracing::error!(?panic, "a task panicked at shutdown");
			}
		}
	}
}

/// The picker filter of the string table's list `spec`, opened at `start`.
fn filter(spec: &str, start: Option<&Path>) -> FileFilter {
	let types = filters(spec).into_iter().map(|(name, exts)| FileType::new(name, exts));
	let f = FileFilter::default().file_types(types);
	start.map_or(f.clone(), |p| f.start(p))
}

/// The picked path; `None` when the user cancelled or the picker failed.
fn picked<T>(r: Result<Option<T>, PlatformError>) -> Option<T> {
	r.unwrap_or_else(|e| {
		tracing::warn!("the file picker failed: {e}");
		None
	})
}

async fn pick_path(spec: &str, start: Option<&Path>) -> Option<PathBuf> {
	picked(platform().pick_file_path(filter(spec, start)).await)
}

/// The save path; the dialog proposes the name of `start` when it is a file.
async fn save_path(spec: &str, start: Option<&Path>) -> Option<PathBuf> {
	let name = start
		.filter(|p| !p.is_dir())
		.and_then(Path::file_name)
		.map_or_else(String::new, |n| n.to_string_lossy().into_owned());
	picked(platform().pick_save_path(SaveRequest::new(name).filter(filter(spec, start))).await)
}

fn platform() -> NativePlatform {
	NativePlatform::new(Threads).expect("the picker platform needs no OS service")
}

/// Runs each blocking platform job on its own thread; pickers are rare and short.
struct Threads;

impl BlockingSpawner for Threads {
	fn spawn_blocking(&self, job: BlockingJob) {
		std::thread::spawn(job);
	}
}

async fn run(task: Task, s: Strings) -> Option<Answer> {
	let read = |path: &Path| read_scene_image(&s, path).map_or_else(Answer::Failed, Answer::SceneImage);
	let job = match task {
		Task::Job(job) => job,
		Task::Dropped(file) => {
			let path = file.path().to_owned();
			return Some(if is_scene_image(&path) { read(&path) } else { Answer::Dropped(Source::Path(path)) });
		}
	};
	match job {
		Job::Open { start } => {
			let f = pick_path(s.get("OpenFileDialogFilter"), start.as_deref()).await?;
			Some(Answer::Open(Source::Path(f)))
		}
		Job::SaveAs { proposed } => {
			let f = save_path(s.get("SaveFileDialogFilter"), proposed.as_deref()).await;
			Some(Answer::SaveAs(f.map(Target::Path)))
		}
		Job::PickSceneImage { start } => {
			let path = pick_path(&scene_image_filter(&s), start.as_deref()).await?;
			Some(read(&path))
		}
		Job::SaveSceneImage(save) => {
			let target = save_path(&save.filter, save.proposed.as_deref()).await?;
			write_scene_image(&s, &target, &save.bytes).err().map(Answer::Failed)
		}
		Job::PickScheme => {
			let path = pick_path(s.get("ColorSchemeFilter"), None).await?;
			Some(Answer::Open(Source::Path(path)))
		}
		Job::ProbeCsv { start, probe } => {
			let path = save_path(s.get("ExportDialogFilterCsv"), Some(&start)).await?;
			Some(Answer::Csv { probe, path })
		}
		Job::Browse { target, spec, start } => {
			let path = save_path(&spec, Some(&start)).await?;
			Some(Answer::Browsed(target, path))
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::mpsc::channel;

	use super::*;

	#[test]
	fn a_task_that_panics_answers_as_failed() {
		let (tx, rx) = channel();
		let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
			let _replied = Replied(Some(move |a| tx.send(a).unwrap()));
			panic!("in the dialog");
		}));
		assert!(matches!(rx.recv().unwrap(), Some(Answer::Failed(_))));
	}

	#[test]
	fn a_task_answers_once() {
		let (tx, rx) = channel();
		Replied(Some(move |a| tx.send(a).unwrap())).send(None);
		assert_eq!(rx.iter().count(), 1);
	}
}
