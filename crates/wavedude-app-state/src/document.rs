//! The open project: its file (or the name an import or example suggests), the saved state, the undo history,
//! and the title text (docs/ui/00-overview.md, 70-dialogs-and-windows.md).
//!
//! Threads: plain data on the UI thread; loading and saving block the caller.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::Watcher;
use wavedude_core::sync::lock;

use wavedude_core::io::{self, FormatError, IoError, legacy, wdproj};
use wavedude_core::model::Project;
use wavedude_core::model::normalize::Normalize;
use wavedude_examples::Example;

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
#[derive(Debug)]
pub struct Document {
	pub project: Project,
	/// The file it was opened from or saved to.
	pub path: Option<PathBuf>,
	/// For an unsaved import or example: the file name Save As proposes and the title shows.
	pub suggested_file_name: Option<PathBuf>,
	/// Edits since the saved step (the title's asterisk).
	pub dirty: bool,
	pub history: History,
	/// The empty project at start or after Close: the title shows the app name alone until it is edited.
	placeholder: bool,
	/// The scene image file watched while the project names one.
	image_watch: Option<ImageWatch>,
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
/// A changed scene image file is re-embedded this long after its last change (docs/ui/60): a writer may still
/// hold it.
const IMAGE_DEBOUNCE: Duration = Duration::from_millis(400);

/// The scene image's file, watched while the project names it.
struct ImageWatch {
	path: PathBuf,
	_watcher: notify::RecommendedWatcher,
	/// When the file last changed on disk; taken once re-embedded.
	changed: Arc<Mutex<Option<Instant>>>,
}

impl ImageWatch {
	/// Watches `path`'s folder for changes to the file (the file itself would be lost to an editor that replaces
	/// it). None, with a warning, where the platform refuses.
	fn start(path: PathBuf) -> Option<Self> {
		let changed = Arc::new(Mutex::new(None));
		let name = path.file_name()?.to_owned();
		let mark = changed.clone();
		let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
			if let Ok(event) = event
				&& event.paths.iter().any(|p| p.file_name() == Some(&name))
			{
				*lock(&mark) = Some(Instant::now());
			}
		});
		let folder = path.parent()?.to_owned();
		let mut watcher = match watcher {
			Ok(w) => w,
			Err(e) => {
				tracing::warn!("the scene image file '{}' is not watched: {e}", path.display());
				return None;
			}
		};
		if let Err(e) = watcher.watch(&folder, notify::RecursiveMode::NonRecursive) {
			tracing::warn!("the scene image file '{}' is not watched: {e}", path.display());
			return None;
		}
		Some(Self { path, _watcher: watcher, changed })
	}
}

impl std::fmt::Debug for ImageWatch {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ImageWatch").field("path", &self.path).finish_non_exhaustive()
	}
}

impl Document {
	fn with(mut project: Project, path: Option<PathBuf>, suggested: Option<PathBuf>, placeholder: bool) -> Self {
		project.normalize();
		let history = History::new(&project);
		Self { project, path, suggested_file_name: suggested, dirty: false, history, placeholder, image_watch: None }
	}

	/// The empty project at start and after Close.
	pub fn empty() -> Self {
		Self::with(Project::new_random_seed(), None, None, true)
	}

	/// File > New: the default project with a fresh seed.
	pub fn new_project() -> Self {
		Self::with(Project::new_random_seed(), None, None, false)
	}

	/// Opens a project file by its extension: `.wdproj` (or `.xml`), a Wave Dude 2D `.json` (imported as
	/// an unsaved project that suggests its file name), or a `.wdscheme` colour scheme for the open project.
	pub fn open(path: &Path) -> Result<Opened, DocumentError> {
		let doc = match extension(path).as_str() {
			"json" => {
				let imported = legacy::import(path)?;
				for note in &imported.notes {
					tracing::info!("import of '{}': {note}", path.display());
				}
				let suggested = legacy::suggested_file_name(path, &imported.name);
				Self::with(imported.project, None, Some(suggested), false)
			}
			io::wdscheme::EXTENSION => {
				return Ok(Opened::Scheme(io::wdscheme::load(path)?));
			}
			"xml" => Self::with(wdproj::load(path)?, Some(path.to_owned()), None, false),
			_ => Self::with(io::load_project(path)?, Some(path.to_owned()), None, false),
		};
		let mut doc = doc;
		doc.adopt_scene_image();
		tracing::info!("opened '{}'", path.display());
		Ok(Opened::Project(Box::new(doc)))
	}

	/// An example as an unsaved project named after its file.
	pub fn from_example(example: &Example) -> Result<Self, DocumentError> {
		let mut project = example.load().map_err(|e| DocumentError::Example(example.file_name.to_owned(), e))?;
		io::read_embedded_image_size(&mut project.scene).expect("an embedded example's image decodes");
		tracing::info!("opened example '{}'", example.file_name);
		Ok(Self::with(project, None, Some(PathBuf::from(example.file_name)), false))
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
		let mut located = false;
		if scene.image_path.is_empty()
			&& let Some(folder) = &folder
		{
			let found = [folder.join(&scene.image), folder.join("scene_images").join(&scene.image)]
				.into_iter()
				.find(|p| p.is_file());
			if let Some(found) = found {
				self.project.scene.image_path = io::stored_image_path(Some(folder), &found.to_string_lossy());
				path = found;
				located = true;
			}
		}
		let taken = self.changed_image(&path);
		let adopted = taken.is_some();
		if let Some((bytes, size)) = taken {
			self.project.scene.image_data = Some(bytes.into());
			self.project.scene.image_size = size;
		}
		if located || adopted {
			// Undo goes back to the project as opened, with what was adopted.
			self.history = History::new(&self.project);
		}
		if !adopted {
			return false;
		}
		self.history.forget_saved();
		self.refresh_dirty();
		tracing::info!("the scene image was re-embedded from '{}'", path.display());
		true
	}

	/// The image file's bytes and size when they decode and differ from the embedded ones.
	fn changed_image(&self, path: &Path) -> Option<(Vec<u8>, (u32, u32))> {
		let bytes = std::fs::read(path).ok()?;
		if self.project.scene.image_data.as_deref() == Some(&bytes[..]) {
			return None;
		}
		match io::image_size(&bytes) {
			Ok(size) => Some((bytes, size)),
			Err(_) => {
				tracing::warn!("the scene image file '{}' does not decode; the embedded image stays", path.display());
				None
			}
		}
	}

	/// The scene image's file, when the project names one that resolves.
	fn image_file(&self) -> Option<PathBuf> {
		let path = io::resolve_image_path(self.folder(), &self.project.scene.image_path);
		(!path.as_os_str().is_empty()).then_some(path)
	}

	/// Follows the scene image's file while the project names one: a change on disk is re-embedded as an edit
	/// (undoable, the run starts over) once `IMAGE_DEBOUNCE` has passed since the last change, and tried again
	/// while the writer still holds the file. Returns whether the image changed.
	pub fn follow_image_file(&mut self, now: Instant) -> bool {
		let path = self.image_file();
		if self.image_watch.as_ref().map(|w| &w.path) != path.as_ref() {
			self.image_watch = path.and_then(ImageWatch::start);
		}
		let Some(watch) = &self.image_watch else { return false };
		let Some(at) = *lock(&watch.changed) else { return false };
		if now.saturating_duration_since(at) < IMAGE_DEBOUNCE {
			return false;
		}
		let path = watch.path.clone();
		let Ok(bytes) = std::fs::read(&path) else {
			*lock(&watch.changed) = Some(now);
			return false;
		};
		*lock(&watch.changed) = None;
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
		self.dirty = !self.history.at_saved();
	}

	/// Saves to the project's file; [`DocumentError::NoPath`] when it has none (Save As then).
	pub fn save(&mut self) -> Result<(), DocumentError> {
		let path = self.path.clone().ok_or(DocumentError::NoPath)?;
		self.save_as(&path)
	}

	/// Saves to `path` as `WaveDude` Next XML: `.wdproj` or `.xml`.
	pub fn save_as(&mut self, path: &Path) -> Result<(), DocumentError> {
		self.history.commit(&self.project);
		match extension(path).as_str() {
			"xml" => wdproj::save(&self.project, path)?,
			_ => io::save_project(&self.project, path)?,
		}
		self.path = Some(path.to_owned());
		self.suggested_file_name = None;
		self.placeholder = false;
		self.history.mark_saved();
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
	use wavedude_core::model::{ColorScheme, Rgb, Stop};

	fn png(value: u8) -> Vec<u8> {
		let mut out = Vec::new();
		let mut encoder = png::Encoder::new(&mut out, 2, 2);
		encoder.set_color(png::ColorType::Rgba);
		encoder.set_depth(png::BitDepth::Eight);
		encoder.write_header().unwrap().write_image_data(&[value; 16]).unwrap();
		out
	}

	#[test]
	fn a_changed_scene_image_file_is_re_embedded_after_the_debounce() {
		let dir = tempfile::tempdir().unwrap();
		let file = dir.path().join("scene.png");
		std::fs::write(&file, png(10)).unwrap();
		let mut d = Document::new_project();
		d.project.scene.image_path = file.to_string_lossy().into_owned();
		assert!(!d.follow_image_file(Instant::now()), "nothing changed yet; the watch is armed");
		std::fs::write(&file, png(200)).unwrap();
		let deadline = Instant::now() + Duration::from_secs(5);
		let mut changed = false;
		while !changed && Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(50));
			// a tick past the debounce
			changed = d.follow_image_file(Instant::now() + IMAGE_DEBOUNCE);
		}
		assert!(changed, "the change on disk reached the document");
		assert_eq!(d.project.scene.image_data.as_deref(), Some(&png(200)[..]));
		assert!(d.dirty, "the re-embed is an edit");
	}

	/// A project saved in `dir` that embeds `embedded` as `pic.png`, its image path `image_path`.
	fn saved_with_image(dir: &Path, embedded: &[u8], image_path: &str) -> PathBuf {
		let mut d = Document::new_project();
		let scene = &mut d.project.scene;
		scene.image = "pic.png".into();
		scene.image_path = image_path.into();
		scene.image_data = Some(embedded.into());
		scene.image_size = io::image_size(embedded).unwrap();
		let path = dir.join("p.wdproj");
		d.save_as(&path).unwrap();
		path
	}

	fn opened(path: &Path) -> Document {
		let Opened::Project(d) = Document::open(path).unwrap() else { panic!("a project") };
		*d
	}

	#[test]
	fn opening_re_embeds_a_scene_image_file_that_differs() {
		let dir = tempfile::tempdir().unwrap();
		let path = saved_with_image(dir.path(), &png(10), "pic.png");
		let d = opened(&path);
		assert!(!d.dirty, "a missing file changes nothing");
		assert_eq!(d.project.scene.image_data.as_deref(), Some(&png(10)[..]));
		std::fs::write(dir.path().join("pic.png"), png(10)).unwrap();
		assert!(!opened(&path).dirty, "the same bytes change nothing");
		std::fs::write(dir.path().join("pic.png"), png(200)).unwrap();
		let mut d = opened(&path);
		assert_eq!(d.project.scene.image_data.as_deref(), Some(&png(200)[..]));
		assert!(d.dirty, "the re-embed is unsaved");
		assert!(!d.undo(), "the history starts at the re-embedded project");
	}

	#[test]
	fn opening_finds_a_named_scene_image_in_scene_images() {
		let dir = tempfile::tempdir().unwrap();
		let path = saved_with_image(dir.path(), &png(10), "");
		std::fs::create_dir(dir.path().join("scene_images")).unwrap();
		std::fs::write(dir.path().join("scene_images").join("pic.png"), png(200)).unwrap();
		let d = opened(&path);
		assert_eq!(Path::new(&d.project.scene.image_path), Path::new("scene_images").join("pic.png"));
		assert_eq!(d.project.scene.image_data.as_deref(), Some(&png(200)[..]));
		assert!(d.dirty);
	}

	#[test]
	fn undo_keeps_the_image_path_found_on_opening() {
		let dir = tempfile::tempdir().unwrap();
		let path = saved_with_image(dir.path(), &png(10), "");
		std::fs::write(dir.path().join("pic.png"), png(10)).unwrap();
		let mut d = opened(&path);
		assert_eq!(d.project.scene.image_path, "pic.png");
		assert!(!d.dirty, "the same bytes change nothing");
		d.project.scene.width += 1;
		d.edited(Instant::now());
		d.commit_step();
		assert!(d.undo());
		assert_eq!(d.project.scene.image_path, "pic.png", "the opened project is the first step");
		assert!(!d.dirty);
	}

	#[test]
	fn a_scene_image_file_that_does_not_decode_leaves_the_embedded_one() {
		let dir = tempfile::tempdir().unwrap();
		let path = saved_with_image(dir.path(), &png(10), "pic.png");
		std::fs::write(dir.path().join("pic.png"), b"not an image").unwrap();
		let d = opened(&path);
		assert_eq!(d.project.scene.image_data.as_deref(), Some(&png(10)[..]));
		assert!(!d.dirty);
	}

	#[test]
	fn open_goes_by_the_extension() {
		let dir = tempfile::tempdir().unwrap();
		let scheme_file = dir.path().join("Sunset.wdscheme");
		let stop = |at, b| Stop { at, color: Rgb { r: 255 - b, g: 0, b } };
		let scheme =
			ColorScheme { name: String::new(), stops: vec![stop(0.0, 255), stop(1.0, 0)], ..ColorScheme::default() };
		io::wdscheme::save(&scheme, &scheme_file).unwrap();
		let Opened::Scheme(s) = Document::open(&scheme_file).unwrap() else { panic!("a scheme") };
		assert_eq!(s, ColorScheme { name: "Sunset".into(), ..scheme }, "an unnamed scheme takes the file's name");

		let legacy = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../wavedude-core/tests/fixtures/legacy/Example3_Spherical_mirror/configuration.json");
		let d = opened(&legacy);
		assert_eq!(d.path, None, "an import is unsaved");
		assert_eq!(d.suggested_file_name.unwrap().extension().unwrap(), "wdproj");
		assert!(!d.project.sources.is_empty());

		let mut p = Document::new_project();
		p.project.seed = 7;
		let xml = dir.path().join("p.xml");
		p.save_as(&xml).unwrap();
		let d = opened(&xml);
		assert_eq!((d.path.as_deref(), d.project.seed), (Some(xml.as_path()), 7));
	}

	#[test]
	fn titles() {
		let mut d = Document::empty();
		assert_eq!(d.title("App", "Untitled", false), "App");
		d.project.add_source();
		d.edited(Instant::now());
		assert_eq!(d.title("App", "Untitled", false), "App - Untitled*");
		let mut e = Document::from_example(&wavedude_examples::all()[0]).unwrap();
		assert_eq!(e.title("App", "Untitled", false), "App - Example01_Source_Mirror_Probe.wdproj");
		assert_eq!(e.proposed_save_path().unwrap(), Path::new("Example01_Source_Mirror_Probe.wdproj"));
		e.path = Some(PathBuf::from("/x/y/p.wdproj"));
		assert_eq!(e.title("App", "Untitled", true), "App - /x/y/p.wdproj");
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
		let path = dir.path().join("a.wdproj");
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

	#[test]
	fn an_edit_after_undo_is_dirty_where_the_saved_step_was() {
		let dir = tempfile::tempdir().unwrap();
		let t = Instant::now();
		let mut d = Document::new_project();
		for seed in [11, 12] {
			d.project.seed = seed;
			d.edited(t);
			d.commit_step();
		}
		d.save_as(&dir.path().join("a.wdproj")).unwrap();
		assert!(d.undo());
		d.project.seed = 13;
		d.edited(t);
		d.commit_step();
		assert!(d.dirty, "the file holds seed 12");
	}
}
