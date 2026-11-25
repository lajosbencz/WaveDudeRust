//! The Messages pane's model (docs/ui/70, Messages pane): the ring's lines filtered by level and search, with counts.

use std::collections::BTreeSet;
use std::fmt::Write as _;

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
	/// The search is a regular expression; an unfinished one shows everything.
	regex: bool,
	/// The selected lines' ids, and where a Shift selection grows from.
	selected: BTreeSet<u64>,
	anchor: Option<u64>,
}

impl Messages {
	pub(crate) fn new(ring: Ring) -> Self {
		let debug = cfg!(debug_assertions);
		Self {
			ring,
			seen: u64::MAX,
			lines: Vec::new(),
			shown: [debug, debug, true, true, true],
			search: String::new(),
			regex: false,
			selected: BTreeSet::new(),
			anchor: None,
		}
	}

	pub(crate) fn toggle(&mut self, level: LogLevel) {
		if let Some(i) = LEVELS.iter().position(|(_, l)| *l == level) {
			self.shown[i] ^= true;
		}
	}

	pub(crate) fn search(&mut self, text: &str) {
		self.search = text.to_owned();
	}

	pub(crate) fn set_regex(&mut self, on: bool) {
		self.regex = on;
	}

	pub(crate) fn clear(&mut self) {
		self.ring.clear();
		self.lines.clear();
		self.selected.clear();
	}

	/// Takes new lines from the ring; `true` when there were any.
	pub(crate) fn take(&mut self) -> bool {
		self.ring.since(self.seen).map(|(added, lines)| (self.seen, self.lines) = (added, lines)).is_some()
	}

	/// The lines shown: of a shown level, matching the search in message, logger or source.
	fn visible(&self) -> Vec<&Line> {
		let pattern = (self.regex && !self.search.is_empty())
			.then(|| regex::RegexBuilder::new(&self.search).case_insensitive(true).build().ok())
			.flatten();
		let needle = self.search.to_lowercase();
		let matches = |text: &str| match &pattern {
			Some(p) => p.is_match(text),
			// an unfinished pattern shows everything
			None if self.regex => true,
			None => text.to_lowercase().contains(&needle),
		};
		self.lines
			.iter()
			.filter(|l| self.shown[index(l.level)])
			.filter(|l| self.search.is_empty() || matches(&l.message) || matches(&l.target) || matches(&l.source))
			.collect()
	}

	/// Selects visible row `row`: alone, toggled with `control`, or the run from the anchor with `shift`.
	pub(crate) fn select(&mut self, row: usize, control: bool, shift: bool) {
		let visible = self.visible();
		let Some(line) = visible.get(row) else { return };
		let id = line.id;
		match (shift, self.anchor.and_then(|a| visible.iter().position(|l| l.id == a))) {
			(true, Some(from)) => {
				let ids: Vec<u64> = visible[from.min(row)..=from.max(row)].iter().map(|l| l.id).collect();
				if !control {
					self.selected.clear();
				}
				self.selected.extend(ids);
			}
			_ if control => {
				if !self.selected.remove(&id) {
					self.selected.insert(id);
				}
				self.anchor = Some(id);
			}
			_ => {
				self.selected = BTreeSet::from([id]);
				self.anchor = Some(id);
			}
		}
	}

	/// The selected visible lines (`all`: every visible line) as tab-separated text, one per line.
	pub(crate) fn text(&self, all: bool) -> String {
		let mut out = String::new();
		for l in self.visible().into_iter().filter(|l| all || self.selected.contains(&l.id)) {
			let _ = writeln!(out, "{}\t{}\t{}\t{}", l.time, l.level, l.source, l.message);
		}
		out
	}

	/// Pushes the filtered lines, the level counts and the newest Info and up line to the window.
	pub(crate) fn show(&self, app: &AppWindow) {
		let rows: Vec<LogRow> = self
			.visible()
			.into_iter()
			.map(|l| LogRow {
				time: l.time.as_str().into(),
				level: LEVELS[index(l.level)].1,
				source: l.source.as_str().into(),
				logger: l.target.as_str().into(),
				message: l.message.as_str().into(),
				selected: self.selected.contains(&l.id),
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

fn index(level: Level) -> usize {
	LEVELS.iter().position(|(l, _)| *l == level).unwrap_or(2)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn line(id: u64, message: &str) -> Line {
		Line {
			time: String::new(),
			level: Level::INFO,
			source: "live".into(),
			target: "waveduderust::live".into(),
			message: message.into(),
			id,
		}
	}

	fn messages() -> Messages {
		let mut m = Messages::new(Ring::default());
		m.lines = vec![line(0, "Run Completed"), line(1, "frame 12"), line(2, "Run Cancelled")];
		m
	}

	#[test]
	fn an_unfinished_regex_shows_everything_and_a_finished_one_filters() {
		let mut m = messages();
		m.set_regex(true);
		m.search("run (c");
		assert_eq!(m.visible().len(), 3);
		m.search("^run c");
		assert_eq!(m.visible().len(), 2);
	}

	#[test]
	fn shift_selects_a_run_and_copy_takes_it() {
		let mut m = messages();
		m.select(0, false, false);
		m.select(2, false, true);
		assert_eq!(m.text(false).lines().count(), 3);
		m.select(1, true, false);
		assert_eq!(m.text(false).lines().count(), 2);
		assert!(m.text(true).starts_with("\tINFO\tlive\tRun Completed"));
	}
}
