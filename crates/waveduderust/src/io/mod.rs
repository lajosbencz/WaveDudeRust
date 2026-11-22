//! Project and colour scheme files: the `WaveDude` Next `.wdproj` and `.wdscheme` XML, and the Wave Dude 2D `configuration.json` and JSON schemes.
//!
//! Every reader normalises what it returns. A file whose content cannot be read fails with an
//! [`UnreadableFile`] that names the problem and quotes the file's text for the log.

mod fields;
pub mod legacy;
pub mod schemes;
pub mod wdproj;
pub mod wdscheme;
mod xml;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::model::{Project, Scene, Simulation, Stencil, View};

/// What a reader found wrong in a text, with its 1-based line and column when known.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FormatError {
	pub message: String,
	pub at: Option<(usize, usize)>,
}

impl FormatError {
	pub fn new(message: impl Into<String>) -> Self {
		Self { message: message.into(), at: None }
	}
}

impl fmt::Display for FormatError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.at {
			Some((line, column)) => write!(f, "{} (line {line}, position {column})", self.message),
			None => f.write_str(&self.message),
		}
	}
}

impl std::error::Error for FormatError {}

/// A file whose content could not be read. `Display` is the one-line message; [`UnreadableFile::report`]
/// appends the file's text for the log (the first 64 K characters, and the reported line if beyond).
#[derive(Debug, thiserror::Error)]
#[error("the file '{}' could not be read: {error}", path.display())]
#[non_exhaustive]
pub struct UnreadableFile {
	pub path: PathBuf,
	pub error: FormatError,
	/// The quotation: the file's text, cut as described above, or why it could not be read.
	pub text: String,
}

/// Characters of a file quoted in full.
pub const QUOTE_CHARS: usize = 64 * 1024;
const LINE_CHARS: usize = 500;

impl UnreadableFile {
	pub fn new(path: &Path, text: &str, error: FormatError) -> Self {
		let text = excerpt(text, error.at);
		Self { path: path.to_owned(), error, text }
	}

	/// The message followed by the quoted text, for the log.
	pub fn report(&self) -> String {
		format!("{self}\n--- the text of '{}' ---\n{}", self.path.display(), self.text)
	}
}

/// The first [`QUOTE_CHARS`] characters of `text`, plus the line `at` names when it lies beyond them.
pub fn excerpt(text: &str, at: Option<(usize, usize)>) -> String {
	let total = text.chars().count();
	if total <= QUOTE_CHARS {
		return text.to_owned();
	}
	let cut = text.char_indices().nth(QUOTE_CHARS).map_or(text.len(), |(i, _)| i);
	let mut out = format!("{}\n... {} more characters", &text[..cut], total - QUOTE_CHARS);
	if let Some((line, column)) = at {
		let start: usize = text.split_inclusive('\n').take(line.saturating_sub(1)).map(str::len).sum();
		if start >= cut && start < text.len() {
			let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
			let chars: Vec<char> = text[start..end].chars().collect();
			let at = (column.max(1) - 1).min(chars.len());
			let from = at.saturating_sub(LINE_CHARS);
			let to = (at + LINE_CHARS).min(chars.len());
			let cut_note = if from > 0 || to < chars.len() { " (cut)" } else { "" };
			let quoted: String = chars[from..to].iter().collect();
			out.push_str(&format!("\n--- line {line}, position {column}{cut_note} ---\n{quoted}"));
		}
	}
	out
}

/// Errors of loading and saving projects and schemes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IoError {
	#[error("cannot access '{}': {source}", path.display())]
	Io {
		path: PathBuf,
		#[source]
		source: std::io::Error,
	},
	#[error(transparent)]
	Unreadable(#[from] UnreadableFile),
	#[error("the scene image of '{}' cannot be decoded: {message}", path.display())]
	Image { path: PathBuf, message: String },
	#[error("'{}' is not a project file (.wdproj or .json expected)", .0.display())]
	UnknownType(PathBuf),
}

pub type Result<T> = std::result::Result<T, IoError>;

pub(crate) fn read_text(path: &Path) -> Result<String> {
	std::fs::read_to_string(path).map_err(|source| IoError::Io { path: path.to_owned(), source })
}

pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
	#[cfg(not(target_arch = "wasm32"))]
	let written = write_atomic(path, bytes);
	#[cfg(target_arch = "wasm32")]
	let written = std::fs::write(path, bytes);
	written.map_err(|source| IoError::Io { path: path.to_owned(), source })
}

/// Writes `bytes` to a new file beside `path`'s target and renames it over: a failed write leaves the old file
/// whole. The file keeps the old one's permissions, a new one gets those of a plain write (0o666 less the umask); a
/// read-only file is refused, as an in-place write would be.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	use std::io::Write as _;

	let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
	let old = std::fs::metadata(&path).ok().map(|m| m.permissions());
	if old.as_ref().is_some_and(std::fs::Permissions::readonly) {
		return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "the file is read-only"));
	}
	let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
	#[cfg_attr(not(unix), expect(unused_mut))]
	let mut builder = tempfile::Builder::new();
	#[cfg(unix)]
	if old.is_none() {
		builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
	}
	let mut file = builder.tempfile_in(dir)?;
	if let Some(old) = old {
		// the umask narrowed the temporary file's
		file.as_file().set_permissions(old)?;
	}
	file.write_all(bytes)?;
	file.as_file().sync_all()?;
	file.persist(&path).map_err(|e| e.error)?;
	// the rename is durable once the folder is; some file systems refuse a folder's sync
	#[cfg(unix)]
	if let Err(e) = std::fs::File::open(dir).and_then(|d| d.sync_all()) {
		tracing::warn!("syncing {} after saving {}: {e}", dir.display(), path.display());
	}
	Ok(())
}

/// Phase speed of the original's 9-point kernel (0.066 diagonals, 0.184 axes, -1 centre) relative to
/// the true Laplacian: its long-wave expansion is (axis + 2 diagonal) = 0.316 times the Laplacian,
/// so c = sqrt(0.316) = 0.5621, the constant its Mur condition carried. A 9-point project's
/// frequency scale is divided by it on import so every wavelength stays what the original showed.
pub fn nine_point_speed() -> f64 {
	0.316f64.sqrt()
}

/// Divides a 9-point simulation's source frequency scale by the original kernel's speed (levels stay as typed, so
/// the 255 cap is never hit).
pub(crate) fn scale_nine_point_frequencies(simulation: &mut Simulation) {
	if simulation.stencil == Stencil::NinePoint {
		simulation.source_frequency_scale /= nine_point_speed();
		tracing::info!(
			"9-point project: the frequency scale is divided by the original kernel's speed {:.4} so the wavelengths stay",
			nine_point_speed()
		);
	}
}

/// The file extensions of the image formats a scene image may come in, as this build decodes them.
pub fn image_extensions() -> Vec<&'static str> {
	image::ImageFormat::all()
		.filter(image::ImageFormat::reading_enabled)
		.flat_map(|f| f.extensions_str().iter().copied())
		.collect()
}

/// Width and height of encoded image bytes, read from the header.
pub fn image_size(bytes: &[u8]) -> std::result::Result<(u32, u32), String> {
	image::ImageReader::new(std::io::Cursor::new(bytes))
		.with_guessed_format()
		.map_err(|e| e.to_string())?
		.into_dimensions()
		.map_err(|e| e.to_string())
}

/// Sets `scene.image_size` from the embedded bytes; unchanged without bytes.
pub fn read_embedded_image_size(scene: &mut Scene) -> std::result::Result<(), String> {
	if let Some(bytes) = &scene.image_data {
		scene.image_size = image_size(bytes)?;
	}
	Ok(())
}

/// Sets `scene.image_size` from the embedded bytes; (0, 0) without bytes.
pub(crate) fn decode_image_size(project: &mut Project, path: &Path) -> Result<()> {
	project.scene.image_size = (0, 0);
	read_embedded_image_size(&mut project.scene).map_err(|message| IoError::Image { path: path.to_owned(), message })
}

/// The image path as a project in `folder` stores it: relative when the image lies inside the folder.
pub fn stored_image_path(folder: Option<&Path>, image_path: &str) -> String {
	let path = Path::new(image_path);
	match folder {
		Some(folder) if path.is_absolute() => {
			path.strip_prefix(folder).map_or_else(|_| image_path.to_owned(), |rel| rel.to_string_lossy().into_owned())
		}
		_ => image_path.to_owned(),
	}
}

/// The image path resolved against the project's folder; empty when relative without a folder.
pub fn resolve_image_path(folder: Option<&Path>, image_path: &str) -> PathBuf {
	let path = Path::new(image_path);
	match folder {
		_ if image_path.is_empty() => PathBuf::new(),
		_ if path.is_absolute() => path.to_owned(),
		Some(folder) => folder.join(path),
		None => PathBuf::new(),
	}
}

fn extension(path: &Path) -> String {
	path.extension().map_or_default(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// Loads a project and its view by its extension: `.wdproj`, or a Wave Dude 2D `.json` (notes are logged).
pub fn load_project(path: &Path) -> Result<(Project, View)> {
	match extension(path).as_str() {
		wdproj::EXTENSION => wdproj::load(path),
		"json" => {
			let imported = legacy::import(path)?;
			for note in &imported.notes {
				tracing::info!("import of '{}': {note}", path.display());
			}
			Ok((imported.project, imported.view))
		}
		_ => Err(IoError::UnknownType(path.to_owned())),
	}
}

/// Saves a project and its view as `.wdproj`.
pub fn save_project(project: &Project, view: &View, path: &Path) -> Result<()> {
	match extension(path).as_str() {
		wdproj::EXTENSION => wdproj::save(project, view, path),
		_ => Err(IoError::UnknownType(path.to_owned())),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn excerpt_cuts_and_quotes_the_line() {
		let short = "a\nb";
		assert_eq!(excerpt(short, Some((2, 1))), short);
		let long = format!("{}\nline two <bad>\n", "x".repeat(QUOTE_CHARS + 10));
		let e = excerpt(&long, Some((2, 10)));
		assert!(e.contains("... 26 more characters"), "{}", &e[e.len() - 80..]);
		assert!(e.ends_with("--- line 2, position 10 ---\nline two <bad>"));
	}

	#[test]
	fn image_paths() {
		let root = std::env::temp_dir();
		let folder = root.join("p");
		let inside = folder.join("img").join("a.png");
		let outside = root.join("elsewhere").join("a.png");
		let rel = Path::new("img").join("a.png");
		assert_eq!(stored_image_path(Some(&folder), &inside.to_string_lossy()), rel.to_string_lossy());
		assert_eq!(stored_image_path(Some(&folder), &outside.to_string_lossy()), outside.to_string_lossy());
		assert_eq!(resolve_image_path(Some(&folder), &rel.to_string_lossy()), inside);
		assert_eq!(resolve_image_path(None, "img/a.png"), PathBuf::new());
	}

	/// The names in `dir`, sorted.
	fn names(dir: &Path) -> Vec<String> {
		let mut names: Vec<String> =
			std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
		names.sort();
		names
	}

	#[test]
	fn a_write_replaces_the_file_whole_and_leaves_the_old_one_untouched() {
		let dir = tempfile::tempdir().unwrap();
		let (path, link) = (dir.path().join("p.wdproj"), dir.path().join("link"));
		std::fs::write(&path, "old").unwrap();
		std::fs::hard_link(&path, &link).unwrap();
		write_file(&path, b"new").unwrap();
		assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
		assert_eq!(std::fs::read_to_string(&link).unwrap(), "old", "the old file was written in place");
		assert_eq!(names(dir.path()), ["link", "p.wdproj"], "no temporary file is left");
		write_file(&dir.path().join("fresh.wdproj"), b"first").unwrap();
		assert_eq!(std::fs::read_to_string(dir.path().join("fresh.wdproj")).unwrap(), "first");
	}

	#[test]
	fn a_write_keeps_the_permissions_and_refuses_a_read_only_file() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("p.wdproj");
		std::fs::write(&path, "old").unwrap();
		let mut read_only = std::fs::metadata(&path).unwrap().permissions();
		read_only.set_readonly(true);
		std::fs::set_permissions(&path, read_only).unwrap();
		assert!(write_file(&path, b"new").is_err());
		assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
		assert_eq!(names(dir.path()), ["p.wdproj"]);
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
			std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
			write_file(&path, b"new").unwrap();
			assert_eq!(mode(&path), 0o640);
			let (fresh, plain) = (dir.path().join("fresh.wdproj"), dir.path().join("plain"));
			write_file(&fresh, b"new").unwrap();
			std::fs::write(&plain, "new").unwrap();
			assert_eq!(mode(&fresh), mode(&plain), "a new file as a plain write makes it");
		}
	}
}
