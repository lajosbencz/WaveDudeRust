//! The open project: its file (or the name an import or example suggests), the saved state, the undo history,
//! and the title text (docs/ui/00-overview.md, 70-dialogs-and-windows.md).
//!
//! Threads: plain data on the UI thread; loading and saving block the caller.

use std::path::{Path, PathBuf};
use std::time::Instant;

use wavedude_core::io::{self, FormatError, IoError, legacy, wdproj};
use wavedude_core::model::normalize::Normalize;
use wavedude_core::model::{Layer, Project};

use super::examples::{Example, Loaded};
use super::history::History;

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
	#[error(transparent)]
	Io(#[from] IoError),
	#[error("the example '{0}' cannot be read: {1}")]
	Example(String, FormatError),
	#[error("the project has no file yet")]
	NoPath,
}

/// The project being edited and its file.
#[derive(Clone, Debug)]
pub struct Document {
	pub project: Project,
	/// The file it was opened from or saved to.
	pub path: Option<PathBuf>,
	/// For an unsaved import or example: the file name Save As proposes and the title shows.
	pub suggested_file_name: Option<PathBuf>,
	/// Edits since the saved step (the title's asterisk).
	pub dirty: bool,
	/// The history step the file holds; `None` when no step is saved.
	pub saved_step: Option<usize>,
	pub history: History,
	/// Read from a format written for the original's blended source (`.wdproj`, Wave Dude 2D json, an example):
	/// its brightness is fitted once to the first frame with content. Not persisted.
	pub needs_brightness_fit: bool,
	/// The empty project at start or after Close: the title shows the app name alone until it is edited.
	placeholder: bool,
	/// The scene image file's modification time last seen and when it was last checked.
	image_watch: Option<(std::time::SystemTime, Instant)>,
}

/// What a file opened as: a project, or a colour scheme for the open project.
#[derive(Debug)]
pub enum Opened {
	Project(Box<Document>),
	Scheme(wavedude_core::model::ColorScheme),
}

fn extension(path: &Path) -> String {
	path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// How often the scene image's file is checked for a change.
const IMAGE_WATCH: std::time::Duration = std::time::Duration::from_secs(1);

impl Document {
	fn with(mut project: Project, path: Option<PathBuf>, suggested: Option<PathBuf>, placeholder: bool) -> Self {
		project.normalize();
		let history = History::new(&project);
		Self {
			project,
			path,
			suggested_file_name: suggested,
			dirty: false,
			saved_step: Some(0),
			history,
			needs_brightness_fit: false,
			placeholder,
			image_watch: None,
		}
	}

	/// The empty project at start and after Close.
	pub fn empty() -> Self {
		Self::with(Project::new_random_seed(), None, None, true)
	}

	/// File > New: the default project with a fresh seed.
	pub fn new_project() -> Self {
		Self::with(Project::new_random_seed(), None, None, false)
	}

	/// Opens a project file by its extension: `.wdr`, `.wdproj` (or `.xml`), a Wave Dude 2D `.json` (imported as
	/// an unsaved project that suggests its file name), or a `.wdscheme` colour scheme for the open project.
	pub fn open(path: &Path) -> Result<Opened, DocumentError> {
		let doc = match extension(path).as_str() {
			"json" => {
				let imported = legacy::import(path)?;
				for note in &imported.notes {
					tracing::info!("import of '{}': {note}", path.display());
				}
				let suggested = legacy::suggested_file_name(path, &imported.name);
				let mut doc = Self::with(imported.project, None, Some(suggested), false);
				doc.needs_brightness_fit = true;
				doc
			}
			wavedude_core::io::wdscheme::EXTENSION => {
				return Ok(Opened::Scheme(wavedude_core::io::wdscheme::load(path)?));
			}
			"xml" | wdproj::EXTENSION => {
				let read = wdproj::load_read(path)?;
				let mut doc = Self::with(read.project, Some(path.to_owned()), None, false);
				doc.needs_brightness_fit = !read.own;
				doc
			}
			_ => Self::with(io::load_project(path)?, Some(path.to_owned()), None, false),
		};
		let mut doc = doc;
		doc.adopt_scene_image();
		tracing::info!("opened '{}'", path.display());
		Ok(Opened::Project(Box::new(doc)))
	}

	/// An example as an unsaved project named after its file.
	pub fn from_example(example: &Example) -> Result<Self, DocumentError> {
		let Loaded { mut project, needs_brightness_fit } =
			example.load().map_err(|e| DocumentError::Example(example.file_name.to_owned(), e))?;
		if let Some(bytes) = &project.scene.image_data {
			project.scene.image_size = io::image_size(bytes).expect("an embedded example's image decodes");
		}
		tracing::info!("opened example '{}'", example.file_name);
		let mut doc = Self::with(project, None, Some(PathBuf::from(example.file_name)), false);
		doc.needs_brightness_fit = needs_brightness_fit;
		Ok(doc)
	}

	/// The folder relative image paths resolve against.
	pub fn folder(&self) -> Option<&Path> {
		self.path.as_deref().and_then(Path::parent)
	}

	/// Takes the scene image from its file when the file's bytes differ from the embedded ones (the project
	/// turns dirty); a project without an image path looks for its image by name beside the project file and
	/// in a `scene_images` folder there. A missing file changes nothing. Returns whether bytes were taken.
	pub fn adopt_scene_image(&mut self) -> bool {
		let scene = &self.project.scene;
		if scene.image.is_empty() && scene.image_path.is_empty() {
			return false;
		}
		let folder = self.folder().map(Path::to_owned);
		let mut path = io::resolve_image_path(folder.as_deref(), &scene.image_path);
		if scene.image_path.is_empty()
			&& let Some(folder) = &folder
		{
			let found = [folder.join(&scene.image), folder.join("scene_images").join(&scene.image)]
				.into_iter()
				.find(|p| p.is_file());
			if let Some(found) = found {
				self.project.scene.image_path = io::stored_image_path(Some(folder), &found.to_string_lossy());
				path = found;
			}
		}
		let Ok(bytes) = std::fs::read(&path) else {
			return false;
		};
		if self.project.scene.image_data.as_deref() == Some(&bytes[..]) {
			return false;
		}
		let Ok(size) = io::image_size(&bytes) else {
			tracing::warn!("the scene image file '{}' does not decode; the embedded image stays", path.display());
			return false;
		};
		self.project.scene.image_data = Some(bytes.into());
		self.project.scene.image_size = size;
		self.saved_step = None;
		self.history = History::new(&self.project);
		self.refresh_dirty();
		tracing::info!("the scene image was re-embedded from '{}'", path.display());
		true
	}

	/// The scene image's file, when the project names one that resolves.
	fn image_file(&self) -> Option<PathBuf> {
		let path = io::resolve_image_path(self.folder(), &self.project.scene.image_path);
		(!path.as_os_str().is_empty()).then_some(path)
	}

	/// Follows the scene image's file while the project is open: checked every [`IMAGE_WATCH`], a changed file
	/// is re-embedded as an edit (undoable, the run starts over). Returns whether the image changed.
	pub fn poll_image_file(&mut self, now: Instant) -> bool {
		let Some(path) = self.image_file() else {
			self.image_watch = None;
			return false;
		};
		if self.image_watch.is_some_and(|(_, checked)| now.saturating_duration_since(checked) < IMAGE_WATCH) {
			return false;
		}
		let Some(modified) = std::fs::metadata(&path).and_then(|m| m.modified()).ok() else {
			self.image_watch = None;
			return false;
		};
		let changed = self.image_watch.replace((modified, now)).is_some_and(|(seen, _)| seen != modified);
		if !changed {
			return false;
		}
		let Ok(bytes) = std::fs::read(&path) else { return false };
		if self.project.scene.image_data.as_deref() == Some(&bytes[..]) {
			return false;
		}
		let Ok(size) = io::image_size(&bytes) else {
			tracing::warn!("the scene image file '{}' does not decode; the embedded image stays", path.display());
			return false;
		};
		self.project.scene.image_data = Some(bytes.into());
		self.project.scene.image_size = size;
		self.edited(now);
		tracing::info!("the scene image was re-embedded from '{}': the file changed", path.display());
		true
	}

	/// An edit happened: the history's step stays open, the project turns dirty.
	pub fn edited(&mut self, now: Instant) {
		self.history.edit_started(now);
		self.placeholder = false;
		self.refresh_dirty();
	}

	/// Ends the history's step when quiet long enough.
	pub fn tick(&mut self, now: Instant) {
		if self.history.tick(now, &self.project) {
			self.refresh_dirty();
		}
	}

	/// Writes `brightness` (field, intensity; `None` keeps a layer's) into the project and every history step, so
	/// neither the dirty state nor the undo history sees it.
	pub fn set_brightness_unrecorded(&mut self, brightness: [Option<f64>; 2]) {
		let apply = |p: &mut Project| {
			for (layer, value) in [Layer::Field, Layer::Intensity].into_iter().zip(brightness) {
				if let Some(v) = value {
					p.display.layer_mut(layer).brightness = v;
				}
			}
		};
		apply(&mut self.project);
		self.history.amend(apply);
		self.refresh_dirty();
	}

	/// Ends the pending step now (an edit that is one step by itself: paste, cut, remove, duplicate).
	pub fn commit_step(&mut self) {
		self.history.commit(&self.project);
		self.refresh_dirty();
	}

	pub fn undo(&mut self) -> bool {
		let done = self.history.undo(&mut self.project);
		self.refresh_dirty();
		done
	}

	pub fn redo(&mut self) -> bool {
		let done = self.history.redo(&mut self.project);
		self.refresh_dirty();
		done
	}

	pub fn refresh_dirty(&mut self) {
		self.dirty = self.history.has_pending() || self.saved_step != Some(self.history.current());
	}

	/// Saves to the project's file; [`DocumentError::NoPath`] when it has none (Save As then).
	pub fn save(&mut self) -> Result<(), DocumentError> {
		let path = self.path.clone().ok_or(DocumentError::NoPath)?;
		self.save_as(&path)
	}

	/// Saves to `path` by its extension: `.wdproj` or `.xml` (WaveDude Next XML), `.wdr` (zip of JSON).
	pub fn save_as(&mut self, path: &Path) -> Result<(), DocumentError> {
		self.history.commit(&self.project);
		match extension(path).as_str() {
			"xml" => wdproj::save(&self.project, path)?,
			_ => io::save_project(&self.project, path)?,
		}
		self.path = Some(path.to_owned());
		self.suggested_file_name = None;
		self.needs_brightness_fit = false;
		self.placeholder = false;
		self.saved_step = Some(self.history.current());
		self.refresh_dirty();
		Ok(())
	}

	/// The path Save As proposes: the file, else the suggested name as a `.wdproj` beside its origin.
	pub fn proposed_save_path(&self) -> Option<PathBuf> {
		self.path.clone().or_else(|| self.suggested_file_name.as_ref().map(|p| p.with_extension(wdproj::EXTENSION)))
	}

	/// The name the unsaved-edits question uses: the file name, the suggested one, or `untitled`.
	pub fn display_name(&self, untitled: &str) -> String {
		self.path
			.as_deref()
			.or(self.suggested_file_name.as_deref())
			.and_then(Path::file_name)
			.map_or_else(|| untitled.to_owned(), |n| n.to_string_lossy().into_owned())
	}

	/// The window title: `<app> - <file>` with `*` for unsaved edits, the file's full path with `full_path`;
	/// the suggested name for an unsaved import or example; `untitled` for a new project; the app name alone
	/// for the empty project at start or after Close.
	pub fn title(&self, app: &str, untitled: &str, full_path: bool) -> String {
		let star = if self.dirty { "*" } else { "" };
		let name = match (&self.path, &self.suggested_file_name) {
			(Some(p), _) if full_path => p.display().to_string(),
			(Some(p), _) | (None, Some(p)) => {
				p.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
			}
			(None, None) if self.placeholder && !self.dirty => return app.to_owned(),
			(None, None) => untitled.to_owned(),
		};
		format!("{app} - {name}{star}")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	#[test]
	fn titles() {
		let mut d = Document::empty();
		assert_eq!(d.title("App", "Untitled", false), "App");
		d.project.add_source();
		d.edited(Instant::now());
		assert_eq!(d.title("App", "Untitled", false), "App - Untitled*");
		let mut e = Document::from_example(&crate::examples::all()[0]).unwrap();
		assert!(e.needs_brightness_fit, "an example is written for the original's source");
		assert_eq!(e.title("App", "Untitled", false), "App - Example01_Source_Mirror_Probe.wdproj");
		assert_eq!(e.proposed_save_path().unwrap(), Path::new("Example01_Source_Mirror_Probe.wdproj"));
		e.path = Some(PathBuf::from("/x/y/p.wdr"));
		assert_eq!(e.title("App", "Untitled", true), "App - /x/y/p.wdr");
	}

	#[test]
	fn save_open_and_dirty_at_the_saved_step() {
		let dir = tempfile::tempdir().unwrap();
		let mut d = Document::new_project();
		assert!(matches!(d.save(), Err(DocumentError::NoPath)));
		let t = Instant::now();
		d.project.add_source();
		d.edited(t);
		assert!(d.dirty);
		let path = dir.path().join("a.wdr");
		d.save_as(&path).unwrap();
		assert!(!d.dirty);
		d.project.seed = 5;
		d.edited(t);
		d.tick(t + Duration::from_secs(1));
		assert!(d.dirty);
		d.undo();
		assert!(!d.dirty, "the dirty flag clears at the saved step");
		d.redo();
		assert!(d.dirty);
		let Opened::Project(o) = Document::open(&path).unwrap() else { panic!("a project") };
		assert_eq!(o.project.sources.len(), 1);
		let xml = dir.path().join("b.wdproj");
		d.save_as(&xml).unwrap();
		let Opened::Project(o) = Document::open(&xml).unwrap() else { panic!("a project") };
		assert_eq!(o.project.seed, 5);
	}
}
