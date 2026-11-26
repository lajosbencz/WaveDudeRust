//! The Messages pane's model (docs/ui/70, Messages pane): the ring's lines filtered by level and search, with counts.

use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::rc::Rc;

use slint::{Model, ModelNotify, ModelRc, ModelTracker, VecModel};
use tracing::Level;

use crate::log::{KEPT, Line, Ring};
use crate::ui::{AppWindow, LevelToggle, LogLevel, LogRow};

const LEVELS: [(Level, LogLevel); 5] = [
	(Level::TRACE, LogLevel::Trace),
	(Level::DEBUG, LogLevel::Debug),
	(Level::INFO, LogLevel::Info),
	(Level::WARN, LogLevel::Warn),
	(Level::ERROR, LogLevel::Error),
];

/// Which lines the pane lists.
struct Filter {
	/// Shown per level, in [`LEVELS`] order: Info and up, Trace and Debug in debug builds.
	shown: [bool; 5],
	search: String,
	/// The search is a regular expression; an unfinished one shows everything.
	regex: bool,
	/// The search compiled while `regex` is on and it parses.
	pattern: Option<regex::Regex>,
}

impl Filter {
	fn compile(&mut self) {
		self.pattern = (self.regex && !self.search.is_empty())
			.then(|| regex::RegexBuilder::new(&self.search).case_insensitive(true).build().ok())
			.flatten();
	}

	/// Of a shown level, matching the search in message, logger or source.
	fn passes(&self, l: &Line) -> bool {
		if !self.shown[index(l.level)] {
			return false;
		}
		if self.search.is_empty() {
			return true;
		}
		let needle = self.search.to_lowercase();
		let matches = |text: &str| match &self.pattern {
			Some(p) => p.is_match(text),
			// an unfinished pattern shows everything
			None if self.regex => true,
			None => text.to_lowercase().contains(&needle),
		};
		matches(&l.message) || matches(&l.target) || matches(&l.source)
	}
}

/// The pane's rows, oldest first; a deque, as the oldest go once the ring is full.
#[derive(Default)]
struct Rows {
	rows: RefCell<VecDeque<LogRow>>,
	notify: ModelNotify,
}

impl Model for Rows {
	type Data = LogRow;

	fn row_count(&self) -> usize {
		self.rows.borrow().len()
	}

	fn row_data(&self, row: usize) -> Option<LogRow> {
		self.rows.borrow().get(row).cloned()
	}

	fn model_tracker(&self) -> &dyn ModelTracker {
		&self.notify
	}
}

impl Rows {
	fn set(&self, rows: VecDeque<LogRow>) {
		*self.rows.borrow_mut() = rows;
		self.notify.reset();
	}

	fn drop_front(&self, count: usize) {
		if count > 0 {
			self.rows.borrow_mut().drain(..count);
			self.notify.row_removed(0, count);
		}
	}

	fn extend(&self, new: Vec<LogRow>) {
		if new.is_empty() {
			return;
		}
		let (at, count) = (self.row_count(), new.len());
		self.rows.borrow_mut().extend(new);
		self.notify.row_added(at, count);
	}

	/// Sets each row's `selected` to `selected(row)`.
	fn reselect(&self, selected: impl Fn(usize) -> bool) {
		let changed: Vec<usize> = self
			.rows
			.borrow_mut()
			.iter_mut()
			.enumerate()
			.filter(|(i, r)| r.selected != selected(*i))
			.map(|(i, r)| {
				r.selected ^= true;
				i
			})
			.collect();
		for i in changed {
			self.notify.row_changed(i);
		}
	}
}

/// What the pane shows of the ring.
pub(crate) struct Messages {
	ring: Ring,
	/// The ring's count of lines ever added at the last take; `None` before the first.
	seen: Option<u64>,
	/// The ring's lines as of the last take.
	lines: VecDeque<Line>,
	/// `lines` per level, in [`LEVELS`] order.
	counts: [usize; 5],
	filter: Filter,
	/// The ids of the lines `rows` lists.
	visible: VecDeque<u64>,
	rows: Rc<Rows>,
	/// The selected lines' ids, and where a Shift selection grows from.
	selected: BTreeSet<u64>,
	anchor: Option<u64>,
}

impl Messages {
	pub(crate) fn new(ring: Ring) -> Self {
		let debug = cfg!(debug_assertions);
		Self {
			ring,
			seen: None,
			lines: VecDeque::new(),
			counts: [0; 5],
			filter: Filter {
				shown: [debug, debug, true, true, true],
				search: String::new(),
				regex: false,
				pattern: None,
			},
			visible: VecDeque::new(),
			rows: Rc::default(),
			selected: BTreeSet::new(),
			anchor: None,
		}
	}

	/// The pane's rows, kept up to date.
	pub(crate) fn rows(&self) -> ModelRc<LogRow> {
		ModelRc::from(self.rows.clone())
	}

	pub(crate) fn toggle(&mut self, level: LogLevel) {
		if let Some(i) = LEVELS.iter().position(|(_, l)| *l == level) {
			self.filter.shown[i] ^= true;
			self.refilter();
		}
	}

	pub(crate) fn search(&mut self, text: &str) {
		text.clone_into(&mut self.filter.search);
		self.filter.compile();
		self.refilter();
	}

	pub(crate) fn set_regex(&mut self, on: bool) {
		self.filter.regex = on;
		self.filter.compile();
		self.refilter();
	}

	pub(crate) fn clear(&mut self) {
		self.ring.clear();
		self.lines.clear();
		self.counts = [0; 5];
		self.visible.clear();
		self.rows.set(VecDeque::new());
		self.selected.clear();
		self.anchor = None;
	}

	/// Takes new lines from the ring; `true` when there were any.
	pub(crate) fn take(&mut self) -> bool {
		let (added, new) = self.ring.since(self.seen.unwrap_or(0));
		if self.seen == Some(added) {
			return false;
		}
		self.seen = Some(added);
		self.append(new);
		true
	}

	/// Appends `new`, dropping the oldest lines past [`KEPT`] as the ring did.
	fn append(&mut self, new: Vec<Line>) {
		let mut gone = 0;
		for _ in 0..(self.lines.len() + new.len()).saturating_sub(KEPT) {
			let Some(old) = self.lines.pop_front() else { break };
			self.counts[index(old.level)] -= 1;
			if self.visible.front() == Some(&old.id) {
				self.visible.pop_front();
				gone += 1;
			}
			self.selected.remove(&old.id);
		}
		self.rows.drop_front(gone);
		let mut rows = Vec::new();
		for line in new {
			self.counts[index(line.level)] += 1;
			if self.filter.passes(&line) {
				rows.push(row(&line, self.selected.contains(&line.id)));
				self.visible.push_back(line.id);
			}
			self.lines.push_back(line);
		}
		self.rows.extend(rows);
	}

	/// Lists anew the lines the filter passes.
	fn refilter(&mut self) {
		let (visible, rows) = self
			.lines
			.iter()
			.filter(|l| self.filter.passes(l))
			.map(|l| (l.id, row(l, self.selected.contains(&l.id))))
			.unzip();
		self.visible = visible;
		self.rows.set(rows);
	}

	/// The lines listed, in order.
	fn visible(&self) -> impl Iterator<Item = &Line> {
		self.visible
			.iter()
			.filter_map(|&id| self.lines.binary_search_by_key(&id, |l| l.id).ok().map(|i| &self.lines[i]))
	}

	/// Selects visible row `row`: alone, toggled with `control`, or the run from the anchor with `shift`.
	pub(crate) fn select(&mut self, row: usize, control: bool, shift: bool) {
		let Some(&id) = self.visible.get(row) else { return };
		match (shift, self.anchor.and_then(|a| self.visible.iter().position(|&v| v == a))) {
			(true, Some(from)) => {
				if !control {
					self.selected.clear();
				}
				self.selected.extend(self.visible.range(from.min(row)..=from.max(row)).copied());
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
		self.rows.reselect(|i| self.visible.get(i).is_some_and(|id| self.selected.contains(id)));
	}

	/// The selected visible lines (`all`: every visible line) as tab-separated text, one per line.
	pub(crate) fn text(&self, all: bool) -> String {
		let mut out = String::new();
		for l in self.visible().filter(|l| all || self.selected.contains(&l.id)) {
			let _ = writeln!(out, "{}\t{}\t{}\t{}", l.time, l.level, l.source, l.message);
		}
		out
	}

	/// Pushes the level counts and the newest Info and up line to the window.
	pub(crate) fn show(&self, app: &AppWindow) {
		let levels: Vec<LevelToggle> = LEVELS
			.iter()
			.enumerate()
			.map(|(i, (level, log))| LevelToggle {
				level: *log,
				label: format!("{level} ({})", self.counts[i]).into(),
				shown: self.filter.shown[i],
			})
			.collect();
		// new lines, which Auto-scroll follows
		app.set_log_added(i32::try_from(self.seen.unwrap_or_default() % (1 << 30)).unwrap_or(0));
		app.set_log_levels(ModelRc::new(VecModel::from(levels)));
		// the app's own lines, not a dependency's
		let own = |l: &&Line| l.level <= Level::INFO && l.target.starts_with("waveduderust");
		if let Some(last) = self.lines.iter().rev().find(own) {
			app.set_status_text(last.message.clone());
			app.set_status_level(LEVELS[index(last.level)].1);
		}
	}
}

fn row(l: &Line, selected: bool) -> LogRow {
	LogRow {
		time: l.time.clone(),
		level: LEVELS[index(l.level)].1,
		source: l.source.clone(),
		logger: l.target.clone(),
		message: l.message.clone(),
		selected,
	}
}

fn index(level: Level) -> usize {
	LEVELS.iter().position(|(l, _)| *l == level).unwrap_or(2)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn line(level: Level, message: &str) -> Line {
		Line {
			time: Default::default(),
			level,
			source: "live".into(),
			target: "waveduderust::live".into(),
			message: message.into(),
			id: 0,
		}
	}

	fn messages() -> Messages {
		let ring = Ring::default();
		for message in ["Run Completed", "frame 12", "Run Cancelled"] {
			ring.push(line(Level::INFO, message));
		}
		let mut m = Messages::new(ring);
		m.take();
		m
	}

	/// The pane's rows' messages.
	fn listed(m: &Messages) -> Vec<String> {
		m.rows.rows.borrow().iter().map(|r| r.message.to_string()).collect()
	}

	#[test]
	fn an_unfinished_regex_shows_everything_and_a_finished_one_filters() {
		let mut m = messages();
		m.set_regex(true);
		m.search("run (c");
		assert_eq!(m.visible().count(), 3);
		m.search("^run c");
		assert_eq!(m.visible().count(), 2);
		assert_eq!(listed(&m), ["Run Completed", "Run Cancelled"]);
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
		let selected: Vec<bool> = m.rows.rows.borrow().iter().map(|r| r.selected).collect();
		assert_eq!(selected, [true, false, true]);
	}

	#[test]
	fn new_lines_append_to_the_filtered_rows_and_count_per_level() {
		let mut m = messages();
		m.search("run");
		assert!(!m.take());
		m.ring.push(line(Level::WARN, "Run Failed"));
		m.ring.push(line(Level::INFO, "frame 13"));
		assert!(m.take());
		assert_eq!(listed(&m), ["Run Completed", "Run Cancelled", "Run Failed"]);
		assert_eq!(m.counts, [0, 0, 4, 1, 0]);
	}

	#[test]
	fn lines_past_the_ring_drop_from_the_rows_and_the_counts() {
		let ring = Ring::default();
		let mut m = Messages::new(ring.clone());
		ring.push(line(Level::WARN, "first"));
		m.take();
		m.select(0, false, false);
		for i in 0..KEPT {
			ring.push(line(Level::INFO, &format!("line {i}")));
		}
		assert!(m.take());
		assert_eq!(m.lines.len(), KEPT);
		assert_eq!(m.counts, [0, 0, KEPT, 0, 0]);
		assert_eq!(m.rows.row_count(), KEPT);
		assert_eq!(listed(&m).first().map(String::as_str), Some("line 0"));
		assert!(m.selected.is_empty());
		assert_eq!(m.text(true).lines().count(), KEPT);
	}

	#[test]
	fn a_burst_past_the_ring_keeps_the_rows_in_step_with_it() {
		let ring = Ring::default();
		let mut m = Messages::new(ring.clone());
		for i in 0..KEPT + 10 {
			ring.push(line(Level::INFO, &format!("line {i}")));
		}
		m.take();
		assert_eq!(m.lines.len(), KEPT);
		assert_eq!(m.lines.front().map(|l| l.id), Some(10));
		assert_eq!(m.visible.len(), KEPT);
		assert_eq!(m.counts[2], KEPT);
	}

	#[test]
	fn clear_empties_the_rows_and_the_counts_and_later_lines_come_in() {
		let mut m = messages();
		m.clear();
		assert_eq!(m.rows.row_count(), 0);
		assert_eq!(m.counts, [0; 5]);
		m.ring.push(line(Level::ERROR, "after"));
		assert!(m.take());
		assert_eq!(listed(&m), ["after"]);
		assert_eq!(m.counts, [0, 0, 0, 0, 1]);
	}

	#[test]
	fn a_hidden_level_leaves_the_rows_and_comes_back() {
		let mut m = messages();
		m.ring.push(line(Level::WARN, "warned"));
		m.take();
		m.toggle(LogLevel::Info);
		assert_eq!(listed(&m), ["warned"]);
		m.toggle(LogLevel::Info);
		assert_eq!(m.rows.row_count(), 4);
	}
}
