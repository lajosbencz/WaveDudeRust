//! The desktop: files, threads and the machine's own facts.

use std::backtrace::Backtrace;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use notify::Watcher;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wavedude_core::quality::MachineFacts;
use wavedude_core::sync::lock;

use super::Instant;
use crate::document::Target;
use crate::log::{CRASH_KEY, LogRing, RingLayer, SHOWN_CRASH_KEY, max_level};
use crate::settings::AppDirs;

/// Files have paths: recent files, the log folder, saving in place.
pub const HAS_PATHS: bool = true;
/// The program looks for and installs its releases.
pub const HAS_UPDATER: bool = true;
/// Exports of videos, image sequences, stills and probe data to files.
pub const HAS_EXPORTS: bool = true;

/// Memory and threads of this machine; the GPU and the drive unknown.
pub fn machine_facts() -> MachineFacts {
	wavedude_core::run::machine_facts()
}

/// A file watched for changes through its folder (the file itself would be lost to an editor that replaces it).
pub struct FileWatch {
	path: PathBuf,
	_watcher: notify::RecommendedWatcher,
	changed: Arc<Mutex<Option<Instant>>>,
}

impl FileWatch {
	/// Watches `path`; None, with a warning, where the platform refuses.
	pub fn start(path: PathBuf) -> Option<Self> {
		let changed = Arc::new(Mutex::new(None));
		let name = path.file_name()?.to_owned();
		let mark = changed.clone();
		let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
			if let Ok(event) = event
				&& event.paths.iter().any(|p| p.file_name() == Some(&name))
			{
				*lock(&mark) = Some(Instant::now());
				crate::wake::wake();
			}
		});
		let folder = path.parent()?.to_owned();
		let mut watcher = match watcher {
			Ok(w) => w,
			Err(e) => {
				tracing::warn!("the file '{}' is not watched: {e}", path.display());
				return None;
			}
		};
		if let Err(e) = watcher.watch(&folder, notify::RecursiveMode::NonRecursive) {
			tracing::warn!("the file '{}' is not watched: {e}", path.display());
			return None;
		}
		Some(Self { path, _watcher: watcher, changed })
	}

	pub fn path(&self) -> &Path {
		&self.path
	}

	/// When the file last changed, unless cleared since.
	pub fn changed_at(&self) -> Option<Instant> {
		*lock(&self.changed)
	}

	/// Clears the change (`None`) or dates it to `at`, so it is looked at again then.
	pub fn set_changed_at(&self, at: Option<Instant>) {
		*lock(&self.changed) = at;
	}
}

impl std::fmt::Debug for FileWatch {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FileWatch").field("path", &self.path).finish_non_exhaustive()
	}
}

/// The store's folder of the log files, beside the crash report.
const LOG_FOLDER: &str = "logs";

/// Keys are paths under the config folder, `/` separating folders; a write replaces its file whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
	dir: PathBuf,
}

impl Store {
	/// The platform's config folder ([`AppDirs::platform`]); `None` when the platform names no home.
	pub fn open() -> Option<Self> {
		AppDirs::platform().map(|d| d.store())
	}

	/// The keys under `dir`.
	pub fn in_dir(dir: PathBuf) -> Self {
		Self { dir }
	}

	/// The file of `key`.
	pub fn path(&self, key: &str) -> PathBuf {
		key.split('/').fold(self.dir.clone(), |p, part| p.join(part))
	}

	/// Where `key` is kept, for messages.
	pub fn location(&self, key: &str) -> String {
		self.path(key).display().to_string()
	}

	/// The text of `key`; `None` when there is none.
	pub fn read(&self, key: &str) -> std::io::Result<Option<String>> {
		match std::fs::read_to_string(self.path(key)) {
			Ok(text) => Ok(Some(text)),
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
			Err(e) => Err(e),
		}
	}

	pub fn write(&self, key: &str, text: &str) -> std::io::Result<()> {
		let path = self.path(key);
		if let Some(dir) = path.parent() {
			std::fs::create_dir_all(dir)?;
		}
		// Written beside and renamed over: a crash mid-write leaves the old file whole.
		let mut temp = path.as_os_str().to_owned();
		temp.push(".tmp");
		let temp = PathBuf::from(temp);
		std::fs::write(&temp, text)?;
		std::fs::rename(&temp, path)
	}

	/// Removes `key`; none is no error.
	pub fn remove(&self, key: &str) -> std::io::Result<()> {
		match std::fs::remove_file(self.path(key)) {
			Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
			_ => Ok(()),
		}
	}

	/// The keys of the files directly in folder `prefix`, sorted; none without the folder.
	pub fn keys(&self, prefix: &str) -> std::io::Result<Vec<String>> {
		let entries = match std::fs::read_dir(self.path(prefix)) {
			Ok(d) => d,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
			Err(e) => return Err(e),
		};
		let mut keys = Vec::new();
		for entry in entries {
			let entry = entry?;
			if entry.file_type()?.is_file() {
				keys.push(format!("{prefix}/{}", entry.file_name().to_string_lossy()));
			}
		}
		keys.sort();
		Ok(keys)
	}
}

/// Writes `bytes` to the target's file; the desktop has no downloads.
pub fn write(target: &Target, bytes: &[u8]) -> std::io::Result<()> {
	match target {
		Target::Path(path) => std::fs::write(path, bytes),
		Target::Download(_) => Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no downloads on the desktop")),
	}
}

/// Keeps the file writer's worker alive; drop it last, at exit, so the file gets every line.
pub struct LogGuard {
	_worker: tracing_appender::non_blocking::WorkerGuard,
}

/// Installs the global subscriber: a daily rolling `wavedude.*.log` in the store's log folder (seven kept), `ring`,
/// and stderr for warnings and errors, so a terminal sees what the log file and the status bar see (where the
/// process has no console, as a Windows GUI program, the writes go nowhere and nothing is lost: the file holds
/// every line).
pub fn init_log(store: &Store, ring: LogRing) -> anyhow::Result<LogGuard> {
	let dir = store.path(LOG_FOLDER);
	std::fs::create_dir_all(&dir)?;
	let appender = tracing_appender::rolling::Builder::new()
		.rotation(tracing_appender::rolling::Rotation::DAILY)
		.filename_prefix("wavedude")
		.filename_suffix("log")
		.max_log_files(7)
		.build(dir)?;
	let (writer, guard) = tracing_appender::non_blocking(appender);
	tracing_subscriber::registry()
		.with(max_level())
		.with(tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false))
		.with(
			tracing_subscriber::fmt::layer()
				.with_writer(std::io::stderr)
				.with_ansi(false)
				.with_filter(LevelFilter::WARN),
		)
		.with(RingLayer(ring))
		.try_init()?;
	Ok(LogGuard { _worker: guard })
}

/// Logs every panic with a backtrace before the default hook prints it to stderr. A panic of the main thread, which
/// ends the program, is also appended to the store's crash report at once, before the log's writer could flush; a
/// worker's panic is caught by its owner or ends that thread alone.
pub fn install_panic_hook(store: &Store) {
	let file = store.path(CRASH_KEY);
	let default = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |info| {
		let thread = std::thread::current();
		let report = format!(
			"WaveDude Rust {} on {} {}\nthread '{}' {info}\n\n{}",
			env!("CARGO_PKG_VERSION"),
			std::env::consts::OS,
			std::env::consts::ARCH,
			thread.name().unwrap_or("<unnamed>"),
			Backtrace::force_capture(),
		);
		if thread.name() == Some("main") {
			let written = std::fs::OpenOptions::new()
				.create(true)
				.append(true)
				.open(&file)
				.and_then(|mut f| std::io::Write::write_all(&mut f, format!("{report}\n").as_bytes()));
			if let Err(e) = written {
				eprintln!("the crash report could not be written to '{}': {e}", file.display());
			}
		}
		tracing::error!("{report}");
		default(info);
	}));
}

/// The crash report a panic of an earlier session left, renamed so it is shown once: its file's path.
pub fn take_crash_report(store: &Store) -> Option<String> {
	let pending = store.path(CRASH_KEY);
	if !pending.is_file() {
		return None;
	}
	let shown = store.path(SHOWN_CRASH_KEY);
	match std::fs::rename(&pending, &shown) {
		Ok(()) => Some(shown.display().to_string()),
		Err(e) => {
			tracing::warn!("the crash report '{}' could not be renamed: {e}", pending.display());
			None
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_store_writes_whole_files_and_lists_a_folder() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		assert_eq!(store.read("a/b.txt").unwrap(), None);
		assert_eq!(store.keys("a").unwrap(), Vec::<String>::new());
		store.write("a/b.txt", "one").unwrap();
		store.write("a/b.txt", "two").unwrap();
		store.write("a/c.txt", "three").unwrap();
		assert_eq!(store.read("a/b.txt").unwrap().as_deref(), Some("two"));
		assert_eq!(store.keys("a").unwrap(), ["a/b.txt", "a/c.txt"], "no temporary file is left");
		assert_eq!(store.path("a/b.txt"), dir.path().join("a").join("b.txt"));
	}

	#[test]
	fn a_crash_report_is_taken_once() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		assert_eq!(take_crash_report(&store), None);
		std::fs::create_dir_all(store.path(LOG_FOLDER)).unwrap();
		std::fs::write(store.path(CRASH_KEY), "boom").unwrap();
		let shown = take_crash_report(&store).expect("the report is taken");
		assert_eq!(std::fs::read_to_string(shown).unwrap(), "boom");
		assert_eq!(take_crash_report(&store), None, "shown once");
	}

	#[test]
	fn a_main_thread_panic_leaves_a_crash_report_with_its_backtrace() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		std::fs::create_dir_all(store.path(LOG_FOLDER)).unwrap();
		install_panic_hook(&store);
		let joined = std::thread::Builder::new().name("main".into()).spawn(|| panic!("boom")).unwrap().join();
		drop(std::panic::take_hook());
		assert!(joined.is_err());
		let report = std::fs::read_to_string(store.path(CRASH_KEY)).unwrap();
		assert!(report.starts_with(&format!("WaveDude Rust {}", env!("CARGO_PKG_VERSION"))), "{report}");
		assert!(report.contains("thread 'main' panicked at") && report.contains("boom"), "{report}");
		// With line tables only (CI) the hook frame is a bare `{closure#0}`; its source line is always there.
		assert!(report.matches("native.rs:").count() > 1, "a symbolised backtrace: {report}");
	}
}
