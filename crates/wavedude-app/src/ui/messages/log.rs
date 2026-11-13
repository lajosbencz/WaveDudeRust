//! The Messages pane's model: its copy of the log ring's lines with counts per level, the filter by level and
//! search (plain or regex), and the selection.

use std::collections::{BTreeSet, VecDeque};

use eframe::egui::Color32;
use regex::{Regex, RegexBuilder};
use tracing::Level;
use wavedude_app_state::log::{self, RING_LINES};
use wavedude_app_state::platform::SystemTime;

use crate::ui::theme::Palette;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LogLevel {
	Trace,
	Debug,
	Info,
	Warn,
	Error,
	Fatal,
}

impl LogLevel {
	pub(super) const ALL: [LogLevel; 6] =
		[LogLevel::Trace, LogLevel::Debug, LogLevel::Info, LogLevel::Warn, LogLevel::Error, LogLevel::Fatal];

	pub(super) fn name(self) -> &'static str {
		match self {
			LogLevel::Trace => "Trace",
			LogLevel::Debug => "Debug",
			LogLevel::Info => "Info",
			LogLevel::Warn => "Warn",
			LogLevel::Error => "Error",
			LogLevel::Fatal => "Fatal",
		}
	}

	fn index(self) -> usize {
		self as usize
	}

	pub(super) fn color(self, palette: &Palette) -> Color32 {
		match self {
			LogLevel::Trace => palette.level_trace,
			LogLevel::Debug => palette.level_debug,
			LogLevel::Info => palette.level_info,
			LogLevel::Warn => palette.level_warn,
			LogLevel::Error | LogLevel::Fatal => palette.level_error,
		}
	}
}

impl From<Level> for LogLevel {
	fn from(level: Level) -> Self {
		match level {
			Level::TRACE => LogLevel::Trace,
			Level::DEBUG => LogLevel::Debug,
			Level::INFO => LogLevel::Info,
			Level::WARN => LogLevel::Warn,
			_ => LogLevel::Error,
		}
	}
}

/// One line. `id` is assigned by [`MessageLog::push`] and never reused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LogLine {
	pub id: u64,
	/// `HH:MM:SS.mmm` of the UTC day.
	pub time: String,
	pub level: LogLevel,
	/// The full logger name.
	pub source: String,
	pub text: String,
	/// A multi-line message's whole text; the row's tooltip.
	pub detail: Option<String>,
}

impl LogLine {
	pub(super) fn new(
		time: impl Into<String>,
		level: LogLevel,
		source: impl Into<String>,
		text: impl Into<String>,
	) -> Self {
		Self { id: 0, time: time.into(), level, source: source.into(), text: text.into(), detail: None }
	}

	#[cfg(test)]
	fn detail(mut self, detail: impl Into<String>) -> Self {
		self.detail = Some(detail.into());
		self
	}

	/// A ring line: a multi-line message keeps its first line, the whole text as the detail.
	pub(super) fn of(l: &log::LogLine) -> Self {
		let first = l.message.lines().next().unwrap_or_default();
		let mut line = Self::new(time_text(l.time), l.level.into(), l.target.clone(), first);
		if l.message.contains('\n') {
			line.detail = Some(l.message.clone());
		}
		line
	}

	/// The logger name without its namespace (after the last `.` or `::`).
	pub(super) fn source_short(&self) -> &str {
		let s = self.source.rsplit("::").next().unwrap_or(&self.source);
		s.rsplit('.').next().unwrap_or(s)
	}

	/// The copied form: time, level, source, message, tab-separated.
	pub(super) fn copy_text(&self) -> String {
		format!("{}\t{}\t{}\t{}", self.time, self.level.name(), self.source_short(), self.text)
	}
}

fn time_text(t: SystemTime) -> String {
	let ms = t.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis()) % 86_400_000;
	format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// The newest `capacity` lines and their counts per level.
#[derive(Debug)]
pub(super) struct MessageLog {
	lines: VecDeque<LogLine>,
	capacity: usize,
	counts: [usize; 6],
	next_id: u64,
	/// Bumped by every change.
	pub revision: u64,
}

impl Default for MessageLog {
	fn default() -> Self {
		Self::with_capacity(RING_LINES)
	}
}

impl MessageLog {
	pub(super) fn with_capacity(capacity: usize) -> Self {
		Self { lines: VecDeque::new(), capacity: capacity.max(1), counts: [0; 6], next_id: 0, revision: 0 }
	}

	pub(super) fn push(&mut self, mut line: LogLine) {
		line.id = self.next_id;
		self.next_id += 1;
		self.counts[line.level.index()] += 1;
		self.lines.push_back(line);
		while self.lines.len() > self.capacity {
			if let Some(old) = self.lines.pop_front() {
				self.counts[old.level.index()] -= 1;
			}
		}
		self.revision += 1;
	}

	/// Appends a batch; one revision for all of it.
	pub(super) fn extend(&mut self, lines: impl IntoIterator<Item = LogLine>) {
		let revision = self.revision;
		for line in lines {
			self.push(line);
		}
		if self.revision != revision {
			self.revision = revision + 1;
		}
	}

	pub(super) fn clear(&mut self) {
		self.lines.clear();
		self.counts = [0; 6];
		self.revision += 1;
	}

	#[cfg(test)]
	fn len(&self) -> usize {
		self.lines.len()
	}

	pub(super) fn count(&self, level: LogLevel) -> usize {
		self.counts[level.index()]
	}

	/// The line `id`, while held.
	pub(super) fn get(&self, id: u64) -> Option<&LogLine> {
		// ids are consecutive within the ring
		let first = self.lines.front()?.id;
		self.lines.get(usize::try_from(id.checked_sub(first)?).ok()?)
	}

	/// The ids of the lines the filter lets through, oldest first, into `out`.
	pub(super) fn visible_ids(&self, filter: &MessageFilter, out: &mut Vec<u64>) {
		let matcher = filter.matcher();
		out.clear();
		out.extend(self.lines.iter().filter(|l| filter.shown[l.level.index()] && matcher.matches(l)).map(|l| l.id));
	}

	/// The lines of `ids`, as copied text.
	pub(super) fn copy_text<'a>(&self, ids: impl Iterator<Item = &'a u64>) -> String {
		ids.filter_map(|&id| self.get(id)).map(LogLine::copy_text).collect::<Vec<_>>().join("\n")
	}
}

/// Which lines the pane shows.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct MessageFilter {
	pub shown: [bool; 6],
	pub search: String,
	pub regex: bool,
}

impl MessageFilter {
	/// Info and up shown; Trace and Debug only when `verbose`.
	pub(super) fn new(verbose: bool) -> Self {
		Self { shown: [verbose, verbose, true, true, true, true], search: String::new(), regex: false }
	}

	pub(super) fn is_shown(&self, level: LogLevel) -> bool {
		self.shown[level.index()]
	}

	pub(super) fn toggle(&mut self, level: LogLevel) {
		self.shown[level.index()] ^= true;
	}

	/// Case-insensitive; an unfinished pattern matches everything until it parses.
	fn matcher(&self) -> Matcher {
		let search = self.search.trim();
		if search.is_empty() {
			Matcher::All
		} else if self.regex {
			RegexBuilder::new(search).case_insensitive(true).build().map_or(Matcher::All, Matcher::Regex)
		} else {
			Matcher::Text(search.to_lowercase())
		}
	}
}

/// A compiled search over message, logger and detail.
enum Matcher {
	All,
	Text(String),
	Regex(Regex),
}

impl Matcher {
	fn matches(&self, line: &LogLine) -> bool {
		let fields = [Some(line.text.as_str()), Some(line.source.as_str()), line.detail.as_deref()];
		match self {
			Matcher::All => true,
			Matcher::Text(t) => fields.iter().flatten().any(|f| f.to_lowercase().contains(t)),
			Matcher::Regex(r) => fields.iter().flatten().any(|f| r.is_match(f)),
		}
	}
}

/// Selected line ids and the range anchor.
#[derive(Clone, Debug, PartialEq, Default)]
pub(super) struct MessageSelection {
	pub ids: BTreeSet<u64>,
	anchor: Option<u64>,
	focus: Option<u64>,
}

impl MessageSelection {
	/// A click: plain selects one, Ctrl toggles, Shift selects from the anchor over `visible` (ids in order).
	pub(super) fn click(&mut self, id: u64, ctrl: bool, shift: bool, visible: &[u64]) {
		match (shift, self.anchor) {
			(true, Some(anchor)) => {
				let range = range_ids(visible, anchor, id);
				if !ctrl {
					self.ids.clear();
				}
				self.ids.extend(range);
			}
			_ if ctrl => {
				if !self.ids.remove(&id) {
					self.ids.insert(id);
				}
				self.anchor = Some(id);
			}
			_ => {
				self.ids = BTreeSet::from([id]);
				self.anchor = Some(id);
			}
		}
		self.focus = Some(id);
	}

	/// Up or Down: the next line from the newest selected one; Shift extends from the anchor.
	pub(super) fn step(&mut self, visible: &[u64], forward: bool, extend: bool) -> Option<u64> {
		let current = self.focus.or(self.anchor).and_then(|f| visible.iter().position(|&v| v == f));
		let next = match current {
			Some(i) if forward => (i + 1).min(visible.len().checked_sub(1)?),
			Some(i) => i.saturating_sub(1),
			None if forward => 0,
			None => visible.len().checked_sub(1)?,
		};
		let id = *visible.get(next)?;
		match self.anchor.filter(|_| extend) {
			Some(anchor) => {
				self.ids = range_ids(visible, anchor, id).collect();
				self.focus = Some(id);
			}
			None => self.click(id, false, false, visible),
		}
		Some(id)
	}

	pub(super) fn select_all(&mut self, visible: &[u64]) {
		self.ids = visible.iter().copied().collect();
	}
}

fn range_ids(visible: &[u64], a: u64, b: u64) -> impl Iterator<Item = u64> + '_ {
	let ia = visible.iter().position(|&v| v == a);
	let ib = visible.iter().position(|&v| v == b);
	let (lo, hi) = match (ia, ib) {
		(Some(x), Some(y)) => (x.min(y), x.max(y)),
		(None, Some(y)) => (y, y),
		_ => (1, 0),
	};
	visible.iter().copied().skip(lo).take((hi + 1).saturating_sub(lo))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn log() -> MessageLog {
		let mut log = MessageLog::with_capacity(4);
		log.extend([
			LogLine::new("00:00:01.000", LogLevel::Debug, "WaveDude.Engine", "grid 1920 x 1080"),
			LogLine::new("00:00:02.000", LogLevel::Info, "WaveDude.App", "Opened tiny.wdproj"),
			LogLine::new("00:00:03.000", LogLevel::Warn, "wavedude_app::store", "spilled to disk")
				.detail("budget 6 GB"),
			LogLine::new("00:00:04.000", LogLevel::Error, "WaveDude.Export", "The export failed."),
		]);
		log
	}

	fn visible(log: &MessageLog, filter: &MessageFilter) -> Vec<u64> {
		let mut ids = Vec::new();
		log.visible_ids(filter, &mut ids);
		ids
	}

	#[test]
	fn ring_buffer_drops_the_oldest() {
		let mut log = log();
		assert_eq!(log.revision, 1);
		log.push(LogLine::new("00:00:05.000", LogLevel::Info, "A", "five"));
		assert_eq!(log.len(), 4);
		assert_eq!(log.count(LogLevel::Debug), 0);
		assert_eq!(log.count(LogLevel::Info), 2);
		assert_eq!(log.get(0), None, "dropped by the ring");
		assert_eq!(log.get(4).map(|l| l.text.as_str()), Some("five"));
		assert_eq!(log.get(5), None);
		log.clear();
		assert!(log.len() == 0 && log.count(LogLevel::Info) == 0 && log.get(4).is_none());
	}

	#[test]
	fn filter_by_level_text_and_regex() {
		let log = log();
		let mut f = MessageFilter::new(false);
		assert_eq!(visible(&log, &f).len(), 3);
		f.toggle(LogLevel::Debug);
		assert_eq!(visible(&log, &f).len(), 4);
		f.search = "EXPORT".into();
		assert_eq!(visible(&log, &f), [3]);
		f.search = "budget".into();
		assert_eq!(visible(&log, &f), [2]);
		f.regex = true;
		f.search = r"^(opened|the)\b".into();
		assert_eq!(visible(&log, &f), [1, 3]);
		f.search = "(unfinished".into();
		assert_eq!(visible(&log, &f).len(), 4);
	}

	#[test]
	fn a_ring_line_keeps_its_first_line_and_the_whole_text_as_detail() {
		let ring = log::LogRing::new();
		ring.push(Level::WARN, "wavedude_app::store", "spilled\nbudget 6 GB".into());
		let line = LogLine::of(&ring.since(None)[0]);
		assert_eq!((line.text.as_str(), line.level, line.source_short()), ("spilled", LogLevel::Warn, "store"));
		assert_eq!(line.detail.as_deref(), Some("spilled\nbudget 6 GB"));
		assert_eq!(line.time.len(), "00:00:00.000".len());
	}

	#[test]
	fn selection_and_copy() {
		let log = log();
		let ids = visible(&log, &MessageFilter::new(true));
		let mut s = MessageSelection::default();
		s.click(1, false, false, &ids);
		s.click(3, false, true, &ids);
		assert_eq!(s.ids, BTreeSet::from([1, 2, 3]));
		s.click(2, true, false, &ids);
		assert_eq!(s.ids, BTreeSet::from([1, 3]));
		assert_eq!(log.copy_text(s.ids.iter()).lines().count(), 2);
		assert_eq!(log.get(2).unwrap().copy_text(), "00:00:03.000\tWarn\tstore\tspilled to disk");
		let mut s = MessageSelection::default();
		assert_eq!(s.step(&ids, true, false), Some(0));
		assert_eq!(s.step(&ids, true, true), Some(1));
		assert_eq!(s.step(&ids, true, true), Some(2));
		assert_eq!(s.ids, BTreeSet::from([0, 1, 2]));
		assert_eq!(s.step(&ids, false, false), Some(1));
		assert_eq!(s.ids, BTreeSet::from([1]));
		s.select_all(&ids);
		assert_eq!(log.copy_text(ids.iter()).lines().count(), 4);
	}
}
