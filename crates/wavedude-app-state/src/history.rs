//! Undo and redo up to a number of steps (the setting `UndoSteps`, decision 0030): one clone of the project per step;
//! the scene image's bytes are shared between the clones (docs/ui/60-live-editing-and-runs.md, Undo and redo).
//!
//! A step ends after [`QUIET`] without an edit while no mouse button is held, so a drag, a number-box scrub or a held
//! arrow key is one step. Undo and redo end a pending step first and write the step back into the same project object.
//!
//! Threads: plain data on the UI thread; time comes from the caller.

use wavedude_core::model::Project;

use super::platform::{Duration, Instant};

/// Quiet time that ends a step.
pub const QUIET: Duration = Duration::from_millis(500);
/// The steps a new history can undo.
pub const DEFAULT_LIMIT: usize = 1000;

/// The steps of one project's editing session.
#[derive(Clone, Debug)]
pub struct History {
	steps: Vec<Project>,
	current: usize,
	/// The step the file holds; `None` once no step does.
	saved: Option<usize>,
	pending_since: Option<Instant>,
	mouse_held: bool,
	/// The steps that can be undone; the oldest beyond it are dropped.
	limit: usize,
}

impl History {
	/// A history whose first step is `project` as it is, saved, keeping [`DEFAULT_LIMIT`] steps.
	pub fn new(project: &Project) -> Self {
		Self {
			steps: vec![project.clone()],
			current: 0,
			saved: Some(0),
			pending_since: None,
			mouse_held: false,
			limit: DEFAULT_LIMIT,
		}
	}

	/// Starts over at `project`, saved, keeping the limit.
	pub fn restart(&mut self, project: &Project) {
		*self = Self { limit: self.limit, ..Self::new(project) };
	}

	/// At most `steps` (at least 1) can be undone; the steps beyond it are dropped as the next step is recorded.
	pub fn set_limit(&mut self, steps: usize) {
		self.limit = steps.max(1);
	}

	pub fn limit(&self) -> usize {
		self.limit
	}

	/// Drops the oldest steps down to the limit; the current step is the last. A dropped saved step leaves none saved.
	fn trim(&mut self) {
		let old = self.steps.len().saturating_sub(self.limit + 1);
		self.steps.drain(..old);
		self.current -= old;
		self.saved = self.saved.and_then(|s| s.checked_sub(old));
	}

	/// An edit happened at `now`; the step stays open until [`QUIET`] passes without another.
	pub fn edit_started(&mut self, now: Instant) {
		self.pending_since = Some(now);
	}

	pub fn mouse_down(&mut self) {
		self.mouse_held = true;
	}

	/// The quiet time of a pending step counts from the release.
	pub fn mouse_up(&mut self, now: Instant) {
		self.mouse_held = false;
		if self.pending_since.is_some() {
			self.pending_since = Some(now);
		}
	}

	/// Ends the pending step when it has been quiet long enough; returns whether a step was recorded.
	pub fn tick(&mut self, now: Instant, project: &Project) -> bool {
		match self.pending_since {
			Some(since) if !self.mouse_held && now.saturating_duration_since(since) >= QUIET => self.commit(project),
			_ => false,
		}
	}

	/// Ends the pending step now (a paste, a save, an undo); an edit that changed nothing records nothing.
	/// Returns whether a step was recorded.
	pub fn commit(&mut self, project: &Project) -> bool {
		if self.pending_since.take().is_none() {
			return false;
		}
		if *project == self.steps[self.current] {
			return false;
		}
		self.steps.truncate(self.current + 1);
		if self.saved > Some(self.current) {
			self.saved = None;
		}
		self.steps.push(project.clone());
		self.current += 1;
		self.trim();
		true
	}

	/// Writes the previous step into `project`; false when there is none.
	pub fn undo(&mut self, project: &mut Project) -> bool {
		self.commit(project);
		if self.current == 0 {
			return false;
		}
		self.current -= 1;
		project.clone_from(&self.steps[self.current]);
		true
	}

	/// Writes the next step into `project`; false when there is none (a pending edit ends the redo branch).
	pub fn redo(&mut self, project: &mut Project) -> bool {
		self.commit(project);
		if self.current + 1 >= self.steps.len() {
			return false;
		}
		self.current += 1;
		project.clone_from(&self.steps[self.current]);
		true
	}

	pub fn can_undo(&self) -> bool {
		self.current > 0 || self.pending_since.is_some()
	}

	pub fn can_redo(&self) -> bool {
		self.pending_since.is_none() && self.current + 1 < self.steps.len()
	}

	/// The index of the step the project is at.
	pub fn current(&self) -> usize {
		self.current
	}

	/// The project is the step the file holds.
	pub fn at_saved(&self) -> bool {
		self.pending_since.is_none() && self.saved == Some(self.current)
	}

	/// The current step is the one the file holds.
	pub fn mark_saved(&mut self) {
		self.saved = Some(self.current);
	}

	/// No step is what the file holds.
	pub fn forget_saved(&mut self) {
		self.saved = None;
	}

	pub fn has_pending(&self) -> bool {
		self.pending_since.is_some()
	}

	pub fn len(&self) -> usize {
		self.steps.len()
	}

	pub fn is_empty(&self) -> bool {
		self.steps.is_empty()
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use super::*;

	fn t0() -> Instant {
		Instant::now()
	}

	#[test]
	fn a_step_ends_after_quiet_time_without_a_held_button() {
		let start = t0();
		let mut p = Project::default();
		let mut h = History::new(&p);
		p.seed = 2;
		h.edit_started(start);
		h.mouse_down();
		let up = start + 4 * QUIET;
		assert!(!h.tick(up, &p), "a held button keeps the step open");
		h.mouse_up(up);
		let ms = Duration::from_millis(1);
		assert!(!h.tick(up + QUIET - ms, &p));
		assert!(h.tick(up + QUIET, &p));
		assert_eq!(h.len(), 2);
		let (first, last) = (up + 2 * QUIET, up + 2 * QUIET + QUIET - ms);
		p.seed = 3;
		h.edit_started(first);
		p.seed = 4;
		h.edit_started(last);
		assert!(!h.tick(last + QUIET - ms, &p));
		assert!(h.tick(last + QUIET, &p));
		assert_eq!(h.len(), 3, "edits inside the quiet time are one step");
	}

	#[test]
	fn undo_redo_round_trip() {
		let start = t0();
		let mut p = Project::default();
		p.scene.image_data = Some(vec![1, 2, 3].into());
		p.scene.image_size = (7, 9);
		let mut h = History::new(&p);
		let before = p.clone();
		p.add_source();
		h.edit_started(start);
		let after = p.clone();
		assert!(h.undo(&mut p));
		assert_eq!(p, before);
		assert_eq!(p.scene.image_size, (7, 9));
		let bytes = |step: &Project| step.scene.image_data.clone().unwrap();
		assert!(Arc::ptr_eq(&bytes(&h.steps[0]), &bytes(&h.steps[1])), "equal bytes are shared");
		assert!(h.can_redo());
		assert!(h.redo(&mut p));
		assert_eq!(p, after);
		assert!(!h.redo(&mut p));
		h.undo(&mut p);
		p.seed = 99;
		h.edit_started(start);
		assert!(!h.can_redo(), "a new edit ends the redo branch");
		h.commit(&p);
		assert!(!h.redo(&mut p));
	}

	#[test]
	fn a_new_branch_drops_the_saved_step_it_replaces() {
		let start = t0();
		let mut p = Project::default();
		let mut h = History::new(&p);
		for seed in [11, 12] {
			p.seed = seed;
			h.edit_started(start);
			h.commit(&p);
		}
		h.mark_saved();
		assert!(h.undo(&mut p));
		assert!(!h.at_saved());
		p.seed = 13;
		h.edit_started(start);
		h.commit(&p);
		assert_eq!(h.current(), 2, "the new step takes the saved step's index");
		assert!(!h.at_saved(), "the step the file held is gone");
	}

	/// A history of steps with seeds `0..=n` (seed 0 the first), at the last.
	fn seeded(n: i32, limit: usize) -> (History, Project) {
		let mut p = Project { seed: 0, ..Project::default() };
		let mut h = History::new(&p);
		h.set_limit(limit);
		for seed in 1..=n {
			p.seed = seed;
			h.edit_started(t0());
			h.commit(&p);
		}
		(h, p)
	}

	fn seeds(h: &History) -> Vec<i32> {
		h.steps.iter().map(|s| s.seed).collect()
	}

	#[test]
	fn the_oldest_steps_beyond_the_limit_are_dropped() {
		let (mut h, mut p) = seeded(5, 3);
		assert_eq!(seeds(&h), [2, 3, 4, 5]);
		assert_eq!(h.current(), 3);
		for _ in 0..3 {
			assert!(h.undo(&mut p));
		}
		assert_eq!(p.seed, 2);
		assert!(!h.undo(&mut p), "no step left beyond the limit");
		assert!(!h.can_undo());
	}

	#[test]
	fn a_dropped_saved_step_leaves_the_project_dirty_for_good() {
		let (mut h, mut p) = seeded(2, 2);
		assert!(!h.at_saved());
		assert!(h.undo(&mut p) && h.undo(&mut p));
		assert!(h.at_saved(), "the saved step is still held");
		assert!(h.redo(&mut p) && h.redo(&mut p));
		p.seed = 3;
		h.edit_started(t0());
		h.commit(&p);
		assert_eq!(seeds(&h), [1, 2, 3]);
		while h.undo(&mut p) {}
		assert!(!h.at_saved(), "the step the file holds is gone");
		h.mark_saved();
		assert!(h.at_saved());
	}

	#[test]
	fn a_saved_step_kept_by_the_trim_moves_with_it() {
		let (mut h, mut p) = seeded(3, 10);
		h.mark_saved();
		for seed in [4, 5] {
			p.seed = seed;
			h.edit_started(t0());
			h.commit(&p);
			h.set_limit(3);
		}
		assert_eq!(seeds(&h), [2, 3, 4, 5]);
		assert!(h.undo(&mut p) && h.undo(&mut p));
		assert_eq!(p.seed, 3);
		assert!(h.at_saved());
	}

	#[test]
	fn a_lower_limit_trims_as_the_next_step_is_recorded() {
		let (mut h, mut p) = seeded(20, 100);
		h.set_limit(5);
		assert_eq!(h.len(), 21, "steps are kept until the next one");
		assert!(h.undo(&mut p) && h.undo(&mut p) && h.redo(&mut p));
		p.seed = 99;
		h.edit_started(t0());
		h.commit(&p);
		assert_eq!(seeds(&h), [15, 16, 17, 18, 19, 99], "the redo branch ends first, then the oldest go");
		assert_eq!(h.current(), 5);
		assert!(!h.at_saved());
		h.restart(&p);
		assert_eq!((h.len(), h.limit(), h.at_saved()), (1, 5, true), "a restart keeps the limit");
	}

	#[test]
	fn an_edit_that_changes_nothing_records_nothing() {
		let p = Project::default();
		let mut h = History::new(&p);
		h.edit_started(t0());
		assert!(!h.commit(&p));
		assert!(!h.can_undo());
	}
}
