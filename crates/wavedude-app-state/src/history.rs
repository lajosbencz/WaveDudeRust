//! Unlimited undo and redo: one clone of the project per step; the scene image's bytes are shared between the
//! clones (docs/ui/60-live-editing-and-runs.md, Undo and redo).
//!
//! A step ends after [`QUIET`] without an edit while no mouse button is held, so a drag, a number-box scrub or a
//! held arrow key is one step. Undo and redo end a pending step first and write the step back into the same
//! project object. Threads: plain data on the UI thread; time comes from the caller.

use wavedude_core::model::Project;

use super::platform::{Duration, Instant};

/// Quiet time that ends a step.
pub const QUIET: Duration = Duration::from_millis(500);

/// The steps of one project's editing session.
#[derive(Clone, Debug)]
pub struct History {
	steps: Vec<Project>,
	current: usize,
	/// The step the file holds; `None` once no step does.
	saved: Option<usize>,
	pending_since: Option<Instant>,
	mouse_held: bool,
}

impl History {
	/// A history whose first step is `project` as it is, saved.
	pub fn new(project: &Project) -> Self {
		let mut h = Self { steps: Vec::new(), current: 0, saved: Some(0), pending_since: None, mouse_held: false };
		h.steps.push(project.clone());
		h
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
		assert!(!h.tick(start + Duration::from_secs(2), &p), "a held button keeps the step open");
		h.mouse_up(start + Duration::from_secs(2));
		assert!(!h.tick(start + Duration::from_millis(2400), &p));
		assert!(h.tick(start + Duration::from_millis(2500), &p));
		assert_eq!(h.len(), 2);
		p.seed = 3;
		h.edit_started(start + Duration::from_secs(3));
		p.seed = 4;
		h.edit_started(start + Duration::from_millis(3400));
		assert!(!h.tick(start + Duration::from_millis(3800), &p));
		assert!(h.tick(start + Duration::from_millis(3900), &p));
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

	#[test]
	fn an_edit_that_changes_nothing_records_nothing() {
		let p = Project::default();
		let mut h = History::new(&p);
		h.edit_started(t0());
		assert!(!h.commit(&p));
		assert!(!h.can_undo());
	}
}
