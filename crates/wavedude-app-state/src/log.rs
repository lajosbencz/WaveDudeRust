//! Logging: a rolling log file in the config dir plus an in-memory ring of lines for the Messages pane and the
//! status bar (Info and up), the panic hook and its crash report, and the helpers that keep the log diagnosable
//! (engine facts, edits naming the newest change, every change at Trace as old -> new).
//!
//! Threads: the ring is shared (`Arc<Mutex>`); any thread may log, the UI thread reads.

use std::backtrace::Backtrace;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use wavedude_core::sync::lock;

use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use wavedude_core::engine::{EngineOptions, SceneData};

/// Lines the Messages pane keeps; the oldest are dropped.
pub const RING_LINES: usize = 5000;

/// One logged event.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
	/// Increases by one per line, so a reader can ask for what came after the last it saw.
	pub seq: u64,
	pub time: SystemTime,
	pub level: Level,
	/// The module path that logged it.
	pub target: String,
	/// The message followed by the event's other fields as `name=value`.
	pub message: String,
}

impl LogLine {
	/// Info and up reach the status bar.
	pub fn reaches_status_bar(&self) -> bool {
		self.level <= Level::INFO
	}
}

#[derive(Default)]
struct Ring {
	lines: VecDeque<LogLine>,
	next: u64,
	status: Option<LogLine>,
}

/// The shared in-memory ring.
#[derive(Clone, Default)]
pub struct LogRing(Arc<Mutex<Ring>>);

impl LogRing {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn push(&self, level: Level, target: &str, message: String) {
		let mut ring = lock(&self.0);
		let line = LogLine { seq: ring.next, time: SystemTime::now(), level, target: target.to_owned(), message };
		ring.next += 1;
		if line.reaches_status_bar() {
			ring.status = Some(line.clone());
		}
		if ring.lines.len() == RING_LINES {
			ring.lines.pop_front();
		}
		ring.lines.push_back(line);
	}

	/// The lines after `seq` (all with `None`), oldest first.
	pub fn since(&self, seq: Option<u64>) -> Vec<LogLine> {
		let ring = lock(&self.0);
		ring.lines.iter().filter(|l| seq.is_none_or(|s| l.seq > s)).cloned().collect()
	}

	/// The newest line of level Info and up: the status bar's text and colour (warnings and errors tint it).
	pub fn status(&self) -> Option<LogLine> {
		lock(&self.0).status.clone()
	}

	pub fn len(&self) -> usize {
		lock(&self.0).lines.len()
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// The Messages pane's Clear; the status bar keeps its line.
	pub fn clear(&self) {
		lock(&self.0).lines.clear();
	}
}

#[derive(Default)]
struct Message {
	text: String,
	fields: String,
}

impl Visit for Message {
	fn record_str(&mut self, field: &Field, value: &str) {
		if field.name() == "message" {
			self.text.push_str(value);
		} else {
			let _ = write!(self.fields, " {}={value}", field.name());
		}
	}

	fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
		if field.name() == "message" {
			let _ = write!(self.text, "{value:?}");
		} else {
			let _ = write!(self.fields, " {}={value:?}", field.name());
		}
	}
}

/// The `tracing` layer that fills a [`LogRing`].
pub struct RingLayer(pub LogRing);

impl<S: Subscriber> Layer<S> for RingLayer {
	fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
		let mut m = Message::default();
		event.record(&mut m);
		let meta = event.metadata();
		self.0.push(*meta.level(), meta.target(), m.text + &m.fields);
	}
}

/// Keeps the file writer's worker alive; drop it last, at exit, so the file gets every line.
pub struct LogGuard {
	_worker: tracing_appender::non_blocking::WorkerGuard,
}

/// The most detailed level logged: Trace only in debug builds.
pub fn max_level() -> LevelFilter {
	if cfg!(debug_assertions) { LevelFilter::TRACE } else { LevelFilter::DEBUG }
}

/// Installs the global subscriber: a daily rolling `wavedude.*.log` in `dir` (seven kept), `ring`, and stderr for
/// warnings and errors, so a terminal sees what the log file and the status bar see (where the process has no
/// console, as a Windows GUI program, the writes go nowhere and nothing is lost: the file holds every line).
pub fn init(dir: &Path, ring: LogRing) -> anyhow::Result<LogGuard> {
	std::fs::create_dir_all(dir)?;
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

/// The report of a panic not yet shown to the user.
const CRASH_FILE: &str = "crash.txt";
/// The newest report once shown.
const SHOWN_CRASH_FILE: &str = "last-crash.txt";

/// Logs every panic with a backtrace before the default hook prints it to stderr. A panic of the main thread, which
/// ends the program, is also appended to `dir`'s crash report at once, before the log's writer could flush; a worker's
/// panic is caught by its owner or ends that thread alone.
pub fn install_panic_hook(dir: &Path) {
	let file = dir.join(CRASH_FILE);
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

/// The crash report a panic of an earlier session left in `dir`, renamed so it is shown once.
pub fn take_crash_report(dir: &Path) -> Option<PathBuf> {
	let pending = dir.join(CRASH_FILE);
	if !pending.is_file() {
		return None;
	}
	let shown = dir.join(SHOWN_CRASH_FILE);
	match std::fs::rename(&pending, &shown) {
		Ok(()) => Some(shown),
		Err(e) => {
			tracing::warn!("the crash report '{}' could not be renamed: {e}", pending.display());
			None
		}
	}
}

/// Logs a built engine's grid at Info: grid, canvas, rim, edges, stride, sub-steps, stencil.
pub fn engine_built(engine: &str, scene: &SceneData, options: &EngineOptions, sub_steps: u32) {
	tracing::info!(
		"{engine} engine built: grid {}x{}, canvas {}x{}, rim {}, edges {:?}, stride {}, {} sub-steps, {:?}, \
		 intensity {}",
		scene.grid_width(),
		scene.grid_height(),
		scene.canvas_width,
		scene.canvas_height,
		scene.rim,
		scene.edges,
		scene.stride(),
		sub_steps,
		options.stencil,
		if options.track_intensity { "tracked" } else { "off" }
	);
}

/// Logs a model change at Trace as `what: old -> new`.
pub fn change(what: &str, old: &str, new: &str) {
	tracing::trace!("{what}: {old} -> {new}");
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ring_keeps_order_status_and_bound() {
		let ring = LogRing::new();
		ring.push(Level::DEBUG, "a", "one".into());
		assert!(ring.status().is_none());
		ring.push(Level::WARN, "a", "two".into());
		ring.push(Level::TRACE, "a", "three".into());
		assert_eq!(ring.status().unwrap().message, "two");
		let all = ring.since(None);
		assert_eq!(all.len(), 3);
		assert_eq!(ring.since(Some(all[0].seq)).len(), 2);
		for i in 0..RING_LINES {
			ring.push(Level::INFO, "a", i.to_string());
		}
		assert_eq!(ring.len(), RING_LINES);
		assert_eq!(ring.since(None)[0].message, "0");
	}

	#[test]
	fn a_crash_report_is_taken_once() {
		let dir = tempfile::tempdir().unwrap();
		assert_eq!(take_crash_report(dir.path()), None);
		std::fs::write(dir.path().join(CRASH_FILE), "boom").unwrap();
		let shown = take_crash_report(dir.path()).expect("the report is taken");
		assert_eq!(std::fs::read_to_string(&shown).unwrap(), "boom");
		assert_eq!(take_crash_report(dir.path()), None, "shown once");
	}

	#[test]
	fn a_main_thread_panic_leaves_a_crash_report_with_its_backtrace() {
		let dir = tempfile::tempdir().unwrap();
		install_panic_hook(dir.path());
		let joined = std::thread::Builder::new().name("main".into()).spawn(|| panic!("boom")).unwrap().join();
		drop(std::panic::take_hook());
		assert!(joined.is_err());
		let report = std::fs::read_to_string(dir.path().join(CRASH_FILE)).unwrap();
		assert!(report.starts_with(&format!("WaveDude Rust {}", env!("CARGO_PKG_VERSION"))), "{report}");
		assert!(report.contains("thread 'main' panicked at") && report.contains("boom"), "{report}");
		// With line tables only (CI) the hook frame is a bare `{closure#0}`; its source line is always there.
		assert!(report.matches("log.rs:").count() > 1, "a symbolised backtrace: {report}");
	}

	#[test]
	fn layer_formats_message_and_fields() {
		let ring = LogRing::new();
		let subscriber = tracing_subscriber::registry().with(RingLayer(ring.clone()));
		tracing::subscriber::with_default(subscriber, || {
			tracing::info!(frame = 3, "frame request");
		});
		let line = &ring.since(None)[0];
		assert_eq!(line.message, "frame request frame=3");
		assert!(line.reaches_status_bar());
	}
}
