//! Logging: an in-memory ring of lines for the Messages pane and the status bar (Info and up), beside the
//! platform's own log ([`crate::platform::init_log`]), and the helpers that keep the log diagnosable (engine facts,
//! edits naming the newest change, every change at Trace as old -> new).
//!
//! Threads: the ring is shared (`Arc<Mutex>`); any thread may log, the UI thread reads.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use wavedude_core::sync::lock;

use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Context;
use wavedude_core::engine::{EngineOptions, SceneData};

use super::platform::SystemTime;

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
		let wakes = line.reaches_status_bar();
		if wakes {
			ring.status = Some(line.clone());
		}
		if ring.lines.len() == RING_LINES {
			ring.lines.pop_front();
		}
		ring.lines.push_back(line);
		drop(ring);
		// Debug and Trace wait for the next frame: eframe traces inside its repaint callback, under egui's lock.
		if wakes {
			crate::wake::wake();
		}
	}

	/// The lines after `seq` (all with `None`), oldest first.
	pub fn since(&self, seq: Option<u64>) -> Vec<LogLine> {
		let ring = lock(&self.0);
		// seqs are consecutive within the ring
		let first = ring.lines.front().map_or(0, |l| l.seq);
		let skip = seq.map_or(0, |s| usize::try_from((s + 1).saturating_sub(first)).unwrap_or(usize::MAX));
		ring.lines.iter().skip(skip).cloned().collect()
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

/// The most detailed level logged: Trace only in debug builds.
pub fn max_level() -> LevelFilter {
	if cfg!(debug_assertions) { LevelFilter::TRACE } else { LevelFilter::DEBUG }
}

/// The report of a panic not yet shown to the user, in the store.
pub(crate) const CRASH_KEY: &str = "logs/crash.txt";
/// The newest report once shown.
pub(crate) const SHOWN_CRASH_KEY: &str = "logs/last-crash.txt";

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
	use tracing_subscriber::layer::SubscriberExt;

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
		let newest = ring.since(None).last().unwrap().seq;
		assert_eq!(ring.since(Some(newest - 2)).len(), 2);
		assert!(ring.since(Some(newest)).is_empty());
		assert_eq!(ring.since(Some(0)).len(), RING_LINES, "a seq the ring dropped reads all of it");
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
