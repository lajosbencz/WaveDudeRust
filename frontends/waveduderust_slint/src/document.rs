//! The open project: its file, the saved state and the undo history (docs/ui/60, Undo and redo; decision 0030).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use waveduderust::model::{Project, View};

/// The open project with its undo and redo steps.
#[derive(Debug, Default)]
pub(crate) struct Document {
	pub(crate) project: Project,
	pub(crate) view: View,
	pub(crate) path: Option<PathBuf>,
	/// The name an example suggests while there is no file.
	pub(crate) suggested: Option<String>,
	saved: Snapshot,
	/// The project or the view differs from `saved`.
	dirty: bool,
	undo: VecDeque<Snapshot>,
	redo: Vec<Snapshot>,
	limit: usize,
	/// The key of the run of edits the last undo step holds (typing into one field).
	merging: Option<String>,
	/// What starting that step dropped: the redo steps and the oldest undo step over the limit, for a cancel.
	dropped: (Vec<Snapshot>, Option<Snapshot>),
	/// Counts the project's changes, for whoever follows it.
	pub(crate) generation: u64,
	/// Counts the view's changes.
	pub(crate) view_generation: u64,
}

/// What an undo step restores and the saved state is compared by: the Display settings belong to the project file.
#[derive(Clone, Debug, Default, PartialEq)]
struct Snapshot {
	project: Project,
	view: View,
}

impl Document {
	/// A new empty project keeping `limit` undo steps.
	pub(crate) fn new(limit: usize) -> Self {
		Self { limit, ..Self::default() }
	}

	/// Applies `f` to the project, its saved state and its history: what the project takes as it opens, neither an
	/// edit nor undone.
	pub(crate) fn settle(&mut self, f: impl Fn(&mut Project)) {
		f(&mut self.project);
		let history = self.undo.iter_mut().chain(&mut self.redo);
		for snapshot in std::iter::once(&mut self.saved).chain(history) {
			f(&mut snapshot.project);
		}
		self.generation += 1;
		self.recheck();
	}

	/// The project of `path`, replacing this one and its history.
	#[cfg(not(target_arch = "wasm32"))]
	pub(crate) fn open(&mut self, path: &Path) -> waveduderust::io::Result<()> {
		let (project, view) = waveduderust::io::load_project(path)?;
		*self = Self {
			saved: Snapshot { project: project.clone(), view: view.clone() },
			project,
			view,
			path: Some(path.to_owned()),
			limit: self.limit,
			..Self::default()
		};
		Ok(())
	}

	/// `project` with no file, `name` suggested for it, replacing this one and its history.
	pub(crate) fn adopt(&mut self, project: Project, view: View, name: &str) {
		*self = Self {
			saved: Snapshot { project: project.clone(), view: view.clone() },
			project,
			view,
			suggested: Some(name.to_owned()),
			limit: self.limit,
			generation: self.generation + 1,
			..Self::default()
		};
	}

	/// Opens a project file's `text` named `name` with no path: a `.json` is a Wave Dude 2D project.
	pub(crate) fn open_text(&mut self, name: &str, text: &str) -> Result<(), String> {
		let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
		let (project, view) = if name.to_ascii_lowercase().ends_with(".json") {
			let imported = waveduderust::io::legacy::import_str(text, Path::new("")).map_err(|e| e.to_string())?;
			for note in &imported.notes {
				tracing::info!("import of '{name}': {note}");
			}
			(imported.project, imported.view)
		} else {
			waveduderust::io::wdproj::read_str(text).map_err(|e| e.to_string())?
		};
		self.adopt(project, view, &format!("{stem}.wdproj"));
		Ok(())
	}

	/// The project as `.wdproj` text.
	#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code, reason = "the browser saves by download"))]
	pub(crate) fn text(&self) -> String {
		waveduderust::io::wdproj::write_string(&self.project, &self.view, None)
	}

	/// Writes the project to `path`, which becomes its file.
	#[cfg(not(target_arch = "wasm32"))]
	pub(crate) fn save_as(&mut self, path: &Path) -> waveduderust::io::Result<()> {
		waveduderust::io::save_project(&self.project, &self.view, path)?;
		self.path = Some(path.to_owned());
		self.mark_saved();
		Ok(())
	}

	/// Applies `edit` as one undo step; an edit that changes nothing leaves no step.
	pub(crate) fn edit(&mut self, edit: impl FnOnce(&mut Project)) {
		self.edit_merging(None, edit);
	}

	/// Applies `edit`, joining the last undo step when it was made under the same `key` and nothing came between.
	pub(crate) fn edit_merging(&mut self, key: Option<&str>, edit: impl FnOnce(&mut Project)) {
		self.change(key, |project, _| edit(project));
	}

	/// Applies `edit` to the view as [`Document::edit_merging`] does to the project.
	pub(crate) fn edit_view_merging(&mut self, key: Option<&str>, edit: impl FnOnce(&mut View)) {
		self.change(key, |_, view| edit(view));
	}

	/// Applies `edit` to the project and the view as [`Document::edit_merging`] does to the project.
	pub(crate) fn edit_both(&mut self, key: Option<&str>, edit: impl FnOnce(&mut Project, &mut View)) {
		self.change(key, edit);
	}

	fn change(&mut self, key: Option<&str>, edit: impl FnOnce(&mut Project, &mut View)) {
		let joins = key.is_some() && self.merging.as_deref() == key && !self.undo.is_empty();
		let before = self.snapshot();
		edit(&mut self.project, &mut self.view);
		if !self.counted(&before) {
			return;
		}
		// a merged step back where it began is no step
		if joins && self.undo.back().is_some_and(|s| s.project == self.project && s.view == self.view) {
			self.undo.pop_back();
			self.merging = None;
			return;
		}
		if !joins {
			self.merging = key.map(str::to_owned);
			self.undo.push_back(before);
			let evicted = if self.undo.len() > self.limit { self.undo.pop_front() } else { None };
			self.dropped = (std::mem::take(&mut self.redo), evicted);
		}
	}

	/// Takes back the merged step of `key` while it is the last one, as if it had not been made: the redo steps and
	/// the oldest step it dropped come back. An edit of another key since then leaves it as it is.
	pub(crate) fn cancel(&mut self, key: &str) {
		if self.merging.as_deref() == Some(key)
			&& let Some(before) = self.undo.pop_back()
		{
			self.restore(before);
			self.merging = None;
			let (redo, evicted) = std::mem::take(&mut self.dropped);
			self.redo = redo;
			if let Some(oldest) = evicted {
				self.undo.push_front(oldest);
			}
		}
	}

	fn snapshot(&self) -> Snapshot {
		Snapshot { project: self.project.clone(), view: self.view.clone() }
	}

	/// Counts the changes since `before`; whether there were any.
	fn counted(&mut self, before: &Snapshot) -> bool {
		let project = self.project != before.project;
		let view = self.view != before.view;
		self.generation += u64::from(project);
		self.view_generation += u64::from(view);
		if project || view {
			self.recheck();
		}
		project || view
	}

	fn recheck(&mut self) {
		self.dirty = self.project != self.saved.project || self.view != self.saved.view;
	}

	/// Replaces the project and the view with `to`, handing back what they were.
	fn restore(&mut self, to: Snapshot) -> Snapshot {
		let was = Snapshot {
			project: std::mem::replace(&mut self.project, to.project),
			view: std::mem::replace(&mut self.view, to.view),
		};
		self.counted(&was);
		was
	}

	pub(crate) fn undo(&mut self) {
		self.merging = None;
		if let Some(step) = self.undo.pop_back() {
			let was = self.restore(step);
			self.redo.push(was);
		}
	}

	pub(crate) fn redo(&mut self) {
		self.merging = None;
		if let Some(step) = self.redo.pop() {
			let was = self.restore(step);
			self.undo.push_back(was);
		}
	}

	pub(crate) fn can_undo(&self) -> bool {
		!self.undo.is_empty()
	}

	pub(crate) fn can_redo(&self) -> bool {
		!self.redo.is_empty()
	}

	/// Keeps at most `limit` undo steps from now on.
	pub(crate) fn set_limit(&mut self, limit: usize) {
		self.limit = limit;
		let over = self.undo.len().saturating_sub(limit);
		self.undo.drain(..over);
	}

	/// Takes the project and the view as saved: their edits are kept or given up.
	pub(crate) fn mark_saved(&mut self) {
		self.saved = self.snapshot();
		self.dirty = false;
	}

	pub(crate) fn dirty(&self) -> bool {
		self.dirty
	}

	/// The file's name or the suggested one; `None` for an untitled project.
	pub(crate) fn name(&self) -> Option<String> {
		self.path
			.as_deref()
			.and_then(Path::file_name)
			.map(|n| n.to_string_lossy().into_owned())
			.or_else(|| self.suggested.clone())
	}
}

#[cfg(test)]
mod tests {

	#[test]
	fn typing_into_one_field_is_one_undo_step() {
		let mut d = Document::new(10);
		for seed in 2..5 {
			d.edit_merging(Some("Project/Seed"), |p| p.seed = seed);
		}
		d.edit_merging(Some("Project/Description"), |p| p.description = "x".into());
		d.undo();
		assert_eq!(d.project.seed, 4);
		d.undo();
		assert_eq!(d.project.seed, Project::default().seed);
		assert!(!d.can_undo());
	}

	use super::*;

	#[test]
	fn a_cancelled_gesture_leaves_no_step() {
		let mut d = Document::new(10);
		let seed = d.project.seed;
		d.edit_merging(Some("g"), |p| p.seed = seed + 1);
		d.edit_merging(Some("g"), |p| p.seed = seed + 2);
		d.cancel("g");
		assert_eq!(d.project.seed, seed);
		assert!(!d.can_undo() && !d.can_redo());
		d.cancel("g");
		assert_eq!(d.project.seed, seed);
	}

	#[test]
	fn a_cancelled_gesture_keeps_the_redo_steps() {
		let mut d = Document::new(10);
		let seed = d.project.seed;
		d.edit(|p| p.seed = seed + 1);
		d.undo();
		d.edit_merging(Some("g"), |p| p.seed = seed + 5);
		d.cancel("g");
		assert!(d.can_redo());
		d.redo();
		assert_eq!(d.project.seed, seed + 1);
	}

	#[test]
	fn a_merged_step_back_where_it_began_is_dropped() {
		let mut d = Document::new(10);
		let seed = d.project.seed;
		d.edit_merging(Some("g"), |p| p.seed = seed + 1);
		d.edit_merging(Some("g"), |p| p.seed = seed);
		assert!(!d.can_undo());
		d.edit_merging(Some("g"), |p| p.seed = seed + 2);
		assert!(d.can_undo());
		d.undo();
		assert_eq!(d.project.seed, seed);
	}

	#[test]
	fn an_edit_is_undone_and_redone_and_a_no_op_leaves_no_step() {
		let mut d = Document::new(2);
		d.edit(|_| {});
		assert!(!d.can_undo());
		d.edit(|p| p.seed = 7);
		assert!(d.dirty());
		d.undo();
		assert_eq!(d.project.seed, Project::default().seed);
		d.redo();
		assert_eq!(d.project.seed, 7);
	}

	#[test]
	fn a_display_edit_is_undone_and_dirties_without_counting_as_a_project_change() {
		let mut d = Document::new(10);
		d.edit_view_merging(Some("Display"), |v| v.field.brightness = 3.0);
		d.edit_view_merging(Some("Display"), |v| v.field.brightness = 4.0);
		assert!(d.dirty());
		assert_eq!((d.generation, d.view_generation), (0, 2));
		d.edit_both(None, |p, v| {
			p.seed = 9;
			v.field.color_scheme = "Copy".into();
		});
		d.undo();
		assert_eq!(
			(d.project.seed, d.view.field.color_scheme.as_str()),
			(Project::default().seed, View::default().field.color_scheme.as_str())
		);
		d.undo();
		assert_eq!(d.view.field.brightness, View::default().field.brightness);
		assert!(!d.dirty());
	}

	#[test]
	fn the_project_is_dirty_until_it_is_back_where_it_was_saved() {
		let mut d = Document::new(10);
		let seed = d.project.seed;
		d.edit(|p| p.seed = seed + 1);
		assert!(d.dirty());
		d.undo();
		assert!(!d.dirty());
		d.redo();
		assert!(d.dirty());
		d.mark_saved();
		assert!(!d.dirty());
		d.undo();
		assert!(d.dirty());
		d.redo();
		assert!(!d.dirty());
		d.edit_merging(Some("g"), |p| p.seed = seed + 2);
		assert!(d.dirty());
		d.cancel("g");
		assert!(!d.dirty());
		d.edit_view_merging(None, |v| v.field.brightness = 3.0);
		d.settle(|p| p.seed = seed + 3);
		assert!(d.dirty());
		d.adopt(d.project.clone(), d.view.clone(), "x.wdproj");
		assert!(!d.dirty());
	}

	#[test]
	fn the_history_keeps_its_limit() {
		let mut d = Document::new(2);
		for s in 1..=3 {
			d.edit(|p| p.seed = s);
		}
		d.undo();
		d.undo();
		assert!(!d.can_undo());
		assert_eq!(d.project.seed, 1);
	}
}
