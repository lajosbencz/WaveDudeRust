//! Logging (docs/ui/70, Messages pane, Status bar): stderr plus a ring of the last lines the window shows.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

/// Lines kept, the oldest dropped.
const KEPT: usize = 5000;

/// A logged line.
#[derive(Clone, Debug)]
pub(crate) struct Line {
	pub(crate) time: String,
	pub(crate) level: Level,
	pub(crate) source: String,
	/// The logger's full name.
	pub(crate) target: String,
	pub(crate) message: String,
	/// The line's number among all ever added.
	pub(crate) id: u64,
}

#[derive(Debug, Default)]
struct Lines {
	lines: VecDeque<Line>,
	/// Counts every line ever added, so a reader sees whether it changed.
	added: u64,
}

/// The ring of lines, shared by the logging layer and the window.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ring(Arc<Mutex<Lines>>);

impl Ring {
	/// The lines and their count when more than `seen` were added; `None` otherwise.
	pub(crate) fn since(&self, seen: u64) -> Option<(u64, Vec<Line>)> {
		let l = self.0.lock().unwrap_or_else(PoisonError::into_inner);
		(l.added != seen).then(|| (l.added, l.lines.iter().cloned().collect()))
	}

	pub(crate) fn clear(&self) {
		self.0.lock().unwrap_or_else(PoisonError::into_inner).lines.clear();
	}
}

struct Message(String);

impl Visit for Message {
	fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
		if field.name() == "message" {
			let _ = write!(self.0, "{value:?}");
		} else {
			let _ = write!(self.0, " {}={value:?}", field.name());
		}
	}
}

impl<S: Subscriber> Layer<S> for Ring {
	fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
		let mut message = Message(String::new());
		event.record(&mut message);
		let meta = event.metadata();
		let source = meta.target().rsplit("::").next().unwrap_or_default().to_owned();
		let since = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default();
		let (secs, ms) = (since.as_secs(), since.subsec_millis());
		let time = format!("{:02}:{:02}:{:02}.{ms:03}", secs / 3600 % 24, secs / 60 % 60, secs % 60);
		let mut l = self.0.lock().unwrap_or_else(PoisonError::into_inner);
		if l.lines.len() == KEPT {
			l.lines.pop_front();
		}
		let (target, id) = (meta.target().to_owned(), l.added);
		// Debug and Trace wait for the next tick, as do other crates' lines
		let wakes = *meta.level() <= Level::INFO && target.starts_with("wavedude");
		l.lines.push_back(Line { time, level: *meta.level(), source, target, message: message.0, id });
		l.added += 1;
		drop(l);
		if wakes {
			crate::wake::wake();
		}
	}
}

#[cfg(not(target_arch = "wasm32"))]
fn console<S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>>() -> impl Layer<S> {
	tracing_subscriber::fmt::layer().with_writer(std::io::stderr)
}

#[cfg(target_arch = "wasm32")]
fn console<S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>>() -> impl Layer<S> {
	tracing_subscriber::fmt::layer()
		.with_ansi(false)
		.without_time()
		.with_writer(tracing_web::MakeWebConsoleWriter::new())
}

/// Logs to stderr (the browser's console) and into the returned ring; Debug and up in debug builds, Info and up in release.
pub(crate) fn init() -> Ring {
	let ring = Ring::default();
	let level = if cfg!(debug_assertions) { Level::DEBUG } else { Level::INFO };
	let filter = tracing_subscriber::filter::filter_fn(move |m| {
		*m.level() <= level && (m.target().starts_with("waveduderust") || *m.level() <= Level::WARN)
	});
	tracing_subscriber::registry()
		.with(console().with_filter(filter.clone()))
		.with(ring.clone().with_filter(filter))
		.init();
	ring
}
