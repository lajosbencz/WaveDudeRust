//! Unlimited undo and redo: one snapshot per step, the project as JSON without the scene image plus the image
//! bytes by reference, equal bytes shared between steps (docs/ui/60-live-editing-and-runs.md, Undo and redo).
//!
//! A step ends after [`QUIET`] without an edit while no mouse button is held, so a drag, a number-box scrub or a
//! held arrow key is one step. Undo and redo end a pending step first and write the step back into the same
//! project object. Threads: plain data on the UI thread; time comes from the caller.

use std::sync::Arc;
use std::time::{Duration, Instant};

use wavedude_core::model::Project;

/// Quiet time that ends a step.
pub const QUIET: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
struct Step {
	json: Arc<str>,
	image: Option<Arc<[u8]>>,
	image_size: (u32, u32),
}

impl Step {
	fn same_as(&self, other: &Self) -> bool {
		let image = match (&self.image, &other.image) {
			(None, None) => true,
			(Some(a), Some(b)) => Arc::ptr_eq(a, b) || a[..] == b[..],
			_ => false,
		};
		image && self.json == other.json
	}
}

/// The steps of one project's editing session.
#[derive(Clone, Debug)]
pub struct History {
	steps: Vec<Step>,
	current: usize,
	pending_since: Option<Instant>,
	mouse_held: bool,
}

impl History {
	/// A history whose first step is `project` as it is.
	pub fn new(project: &Project) -> Self {
		let mut h = Self { steps: Vec::new(), current: 0, pending_since: None, mouse_held: false };
		let first = h.snapshot(project);
		h.steps.push(first);
		h
	}

	fn snapshot(&self, project: &Project) -> Step {
		let image = project.scene.image_data.clone();
		let json = serde_json::to_string(&project.without_image()).expect("a project serialises");
		Step { json: json.into(), image, image_size: project.scene.image_size }
	}

	fn restore(&self, project: &mut Project) {
		let step = &self.steps[self.current];
		let mut restored: Project = serde_json::from_str(&step.json).expect("a stored step deserialises");
		restored.scene.image_data = step.image.clone();
		restored.scene.image_size = step.image_size;
		*project = restored;
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
		let step = self.snapshot(project);
		if step.same_as(&self.steps[self.current]) {
			return false;
		}
		self.steps.truncate(self.current + 1);
		self.steps.push(step);
		self.current += 1;
		true
	}

	/// Applies `f` to every step, as if the steps had always held its result.
	pub fn amend(&mut self, f: impl Fn(&mut Project)) {
		for step in &mut self.steps {
			let mut p: Project = serde_json::from_str(&step.json).expect("a stored step deserialises");
			f(&mut p);
			step.json = serde_json::to_string(&p).expect("a project serialises").into();
		}
	}

	/// Writes the previous step into `project`; false when there is none.
	pub fn undo(&mut self, project: &mut Project) -> bool {
		self.commit(project);
		if self.current == 0 {
			return false;
		}
		self.current -= 1;
		self.restore(project);
		true
	}

	/// Writes the next step into `project`; false when there is none (a pending edit ends the redo branch).
	pub fn redo(&mut self, project: &mut Project) -> bool {
		self.commit(project);
		if self.current + 1 >= self.steps.len() {
			return false;
		}
		self.current += 1;
		self.restore(project);
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

	pub fn has_pending(&self) -> bool {
		self.pending_since.is_some()
	}

	pub fn len(&self) -> usize {
		self.steps.len()
	}

	pub fn is_empty(&self) -> bool {
		self.steps.is_empty()
	}

	/// The image bytes a step holds (shared between steps with equal bytes).
	pub fn image_of(&self, step: usize) -> Option<&Arc<[u8]>> {
		self.steps.get(step)?.image.as_ref()
	}
}

#[cfg(test)]
mod tests {
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
		assert!(Arc::ptr_eq(h.image_of(0).unwrap(), h.image_of(1).unwrap()), "equal bytes are shared");
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
	fn an_edit_that_changes_nothing_records_nothing() {
		let p = Project::default();
		let mut h = History::new(&p);
		h.edit_started(t0());
		assert!(!h.commit(&p));
		assert!(!h.can_undo());
	}
}
