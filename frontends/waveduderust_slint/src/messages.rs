//! The Messages pane's model (docs/ui/70, Messages pane): the ring's lines filtered by level and search, with counts.

use slint::{ModelRc, VecModel};
use tracing::Level;

use crate::log::{Line, Ring};
use crate::ui::{AppWindow, LevelToggle, LogLevel, LogRow};

const LEVELS: [(Level, LogLevel); 5] = [
	(Level::TRACE, LogLevel::Trace),
	(Level::DEBUG, LogLevel::Debug),
	(Level::INFO, LogLevel::Info),
	(Level::WARN, LogLevel::Warn),
	(Level::ERROR, LogLevel::Error),
];

/// What the pane shows of the ring.
pub(crate) struct Messages {
	ring: Ring,
	seen: u64,
	lines: Vec<Line>,
	/// Shown per level, in [`LEVELS`] order: Info and up, Trace and Debug in debug builds.
	shown: [bool; 5],
	search: String,
}

impl Messages {
	pub(crate) fn new(ring: Ring) -> Self {
		let debug = cfg!(debug_assertions);
		Self { ring, seen: u64::MAX, lines: Vec::new(), shown: [debug, debug, true, true, true], search: String::new() }
	}

	pub(crate) fn toggle(&mut self, level: LogLevel) {
		if let Some(i) = LEVELS.iter().position(|(_, l)| *l == level) {
			self.shown[i] ^= true;
		}
	}

	pub(crate) fn search(&mut self, text: &str) {
		self.search = text.to_lowercase();
	}

	pub(crate) fn clear(&mut self) {
		self.ring.clear();
		self.lines.clear();
	}

	/// Takes new lines from the ring; `true` when there were any.
	pub(crate) fn take(&mut self) -> bool {
		self.ring.since(self.seen).map(|(added, lines)| (self.seen, self.lines) = (added, lines)).is_some()
	}

	/// Pushes the filtered lines, the level counts and the newest Info and up line to the window.
	pub(crate) fn show(&self, app: &AppWindow) {
		let index = |level: Level| LEVELS.iter().position(|(l, _)| *l == level).unwrap_or(2);
		let rows: Vec<LogRow> = self
			.lines
			.iter()
			.filter(|l| self.shown[index(l.level)])
			.filter(|l| {
				self.search.is_empty() || format!("{} {}", l.source, l.message).to_lowercase().contains(&self.search)
			})
			.map(|l| LogRow {
				time: l.time.as_str().into(),
				level: LEVELS[index(l.level)].1,
				source: l.source.as_str().into(),
				message: l.message.as_str().into(),
			})
			.collect();
		let levels: Vec<LevelToggle> = LEVELS
			.iter()
			.enumerate()
			.map(|(i, (level, log))| LevelToggle {
				level: *log,
				label: format!("{level} ({})", self.lines.iter().filter(|l| l.level == *level).count()).into(),
				shown: self.shown[i],
			})
			.collect();
		app.set_log_rows(ModelRc::new(VecModel::from(rows)));
		app.set_log_levels(ModelRc::new(VecModel::from(levels)));
		if let Some(last) = self.lines.iter().rev().find(|l| l.level <= Level::INFO) {
			app.set_status_text(last.message.as_str().into());
		}
	}
}
