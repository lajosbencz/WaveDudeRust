//! Logging: a rolling log file in the config dir plus an in-memory ring of lines for the Messages pane and the
//! status bar (Info and up), and the helpers that keep the log diagnosable (engine facts, scratch requests
//! naming the newest edit, every change at Trace as old -> new).
//!
//! Threads: the ring is shared (`Arc<Mutex>`); any thread may log, the UI thread reads.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use wave_core::engine::{EngineOptions, SceneData};

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
		let mut ring = self.0.lock().unwrap();
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
		let ring = self.0.lock().unwrap();
		ring.lines.iter().filter(|l| seq.is_none_or(|s| l.seq > s)).cloned().collect()
	}

	/// The newest line of level Info and up: the status bar's text and colour (warnings and errors tint it).
	pub fn status(&self) -> Option<LogLine> {
		self.0.lock().unwrap().status.clone()
	}

	pub fn len(&self) -> usize {
		self.0.lock().unwrap().lines.len()
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// The Messages pane's Clear; the status bar keeps its line.
	pub fn clear(&self) {
		self.0.lock().unwrap().lines.clear();
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

/// Installs the global subscriber: a daily rolling `wavedude.*.log` in `dir` (seven kept) and `ring`.
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
		.with(RingLayer(ring))
		.try_init()?;
	Ok(LogGuard { _worker: guard })
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
	fn layer_formats_message_and_fields() {
		let ring = LogRing::new();
		let subscriber = tracing_subscriber::registry().with(RingLayer(ring.clone()));
		tracing::subscriber::with_default(subscriber, || {
			tracing::info!(frame = 3, "scratch request");
		});
		let line = &ring.since(None)[0];
		assert_eq!(line.message, "scratch request frame=3");
		assert!(line.reaches_status_bar());
	}
}
