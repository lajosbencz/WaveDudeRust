//! The open project: its file, the saved state and the undo history (docs/ui/60, Undo and redo; decision 0030).

use std::path::{Path, PathBuf};

use waveduderust::model::{Project, View};

/// The open project with its undo and redo steps.
#[derive(Debug, Default)]
pub(crate) struct Document {
	pub(crate) project: Project,
	pub(crate) view: View,
	pub(crate) path: Option<PathBuf>,
	saved: Project,
	undo: Vec<Project>,
	redo: Vec<Project>,
	limit: usize,
	/// Counts the project's changes, for whoever follows it.
	pub(crate) generation: u64,
}

impl Document {
	/// A new empty project keeping `limit` undo steps.
	pub(crate) fn new(limit: usize) -> Self {
		Self { limit, ..Self::default() }
	}

	/// The project of `path`, replacing this one and its history.
	pub(crate) fn open(&mut self, path: &Path) -> waveduderust::io::Result<()> {
		let (project, view) = waveduderust::io::load_project(path)?;
		*self = Self {
			saved: project.clone(),
			project,
			view,
			path: Some(path.to_owned()),
			limit: self.limit,
			..Self::default()
		};
		Ok(())
	}

	/// Writes the project to `path`, which becomes its file.
	pub(crate) fn save_as(&mut self, path: &Path) -> waveduderust::io::Result<()> {
		waveduderust::io::save_project(&self.project, &self.view, path)?;
		self.path = Some(path.to_owned());
		self.saved = self.project.clone();
		Ok(())
	}

	/// Applies `edit` as one undo step; an edit that changes nothing leaves no step.
	pub(crate) fn edit(&mut self, edit: impl FnOnce(&mut Project)) {
		let before = self.project.clone();
		edit(&mut self.project);
		if self.project != before {
			self.generation += 1;
			self.undo.push(before);
			if self.undo.len() > self.limit {
				self.undo.remove(0);
			}
			self.redo.clear();
		}
	}

	pub(crate) fn undo(&mut self) {
		if let Some(p) = self.undo.pop() {
			self.generation += 1;
			self.redo.push(std::mem::replace(&mut self.project, p));
		}
	}

	pub(crate) fn redo(&mut self) {
		if let Some(p) = self.redo.pop() {
			self.generation += 1;
			self.undo.push(std::mem::replace(&mut self.project, p));
		}
	}

	pub(crate) fn can_undo(&self) -> bool {
		!self.undo.is_empty()
	}

	pub(crate) fn can_redo(&self) -> bool {
		!self.redo.is_empty()
	}

	pub(crate) fn dirty(&self) -> bool {
		self.project != self.saved
	}

	/// The window title (docs/ui/00, Main window): the file name, `*` for unsaved edits, `Untitled` without a file.
	pub(crate) fn title(&self) -> String {
		let name = self
			.path
			.as_deref()
			.and_then(Path::file_name)
			.map_or_else(|| "Untitled".into(), |n| n.to_string_lossy().into_owned());
		format!("WaveDude Rust - {name}{}", if self.dirty() { "*" } else { "" })
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
