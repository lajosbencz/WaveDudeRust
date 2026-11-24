//! The open project: its file, the saved state and the undo history (docs/ui/60, Undo and redo; decision 0030).

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
	#[cfg(not(target_arch = "wasm32"))]
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

	/// `project` with no file, `name` suggested for it, replacing this one and its history.
	pub(crate) fn adopt(&mut self, project: Project, view: View, name: &str) {
		*self = Self {
			saved: project.clone(),
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

	/// Keeps at most `limit` undo steps from now on.
	pub(crate) fn set_limit(&mut self, limit: usize) {
		self.limit = limit;
		let over = self.undo.len().saturating_sub(limit);
		self.undo.drain(..over);
	}

	/// Takes the project as saved: its edits are kept or given up.
	pub(crate) fn mark_saved(&mut self) {
		self.saved = self.project.clone();
	}

	pub(crate) fn dirty(&self) -> bool {
		self.project != self.saved
	}

	/// The window title (docs/ui/00, Main window): the file name, `*` for unsaved edits, `Untitled` without a file.
	/// The file's name, the suggested one or "Untitled.wdproj".
	pub(crate) fn name(&self) -> String {
		self.path
			.as_deref()
			.and_then(Path::file_name)
			.map(|n| n.to_string_lossy().into_owned())
			.or_else(|| self.suggested.clone())
			.unwrap_or_else(|| "Untitled.wdproj".into())
	}

	pub(crate) fn title(&self) -> String {
		let name = self.name();
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
