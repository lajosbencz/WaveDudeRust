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

use crate::model::{Project, Stencil};

/// What a reader found wrong in a text, with its 1-based line and column when known.
#[derive(Clone, Debug, PartialEq, Eq)]
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
	std::fs::write(path, bytes).map_err(|source| IoError::Io { path: path.to_owned(), source })
}

/// Phase speed of the original's 9-point kernel (0.066 diagonals, 0.184 axes, -1 centre) relative to
/// the true Laplacian: its long-wave expansion is (axis + 2 diagonal) = 0.316 times the Laplacian,
/// so c = sqrt(0.316) = 0.5621, the constant its Mur condition carried. A 9-point project's
/// frequency scale is divided by it on import so every wavelength stays what the original showed.
pub fn nine_point_speed() -> f64 {
	0.316f64.sqrt()
}

/// Divides the simulation's source frequency scale by the original 9-point kernel's speed when the
/// project uses that stencil (levels stay as typed, so the 255 cap is never hit). Returns whether it did.
pub(crate) fn scale_nine_point_frequencies(project: &mut Project, speed: f64) -> bool {
	if project.simulation.stencil != Stencil::NinePoint {
		return false;
	}
	project.simulation.source_frequency_scale /= speed;
	true
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

/// Sets `scene.image_size` from the embedded bytes; (0, 0) without bytes.
pub(crate) fn decode_image_size(project: &mut Project, path: &Path) -> Result<()> {
	project.scene.image_size = match &project.scene.image_data {
		Some(bytes) => image_size(bytes).map_err(|message| IoError::Image { path: path.to_owned(), message })?,
		None => (0, 0),
	};
	Ok(())
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
	path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Loads a project by its extension: `.wdproj`, or a Wave Dude 2D `.json` (notes are logged).
pub fn load_project(path: &Path) -> Result<Project> {
	match extension(path).as_str() {
		wdproj::EXTENSION => wdproj::load(path),
		"json" => {
			let imported = legacy::import(path)?;
			for note in &imported.notes {
				tracing::info!("import of '{}': {note}", path.display());
			}
			Ok(imported.project)
		}
		_ => Err(IoError::UnknownType(path.to_owned())),
	}
}

/// Saves a project as `.wdproj`.
pub fn save_project(project: &Project, path: &Path) -> Result<()> {
	match extension(path).as_str() {
		wdproj::EXTENSION => wdproj::save(project, path),
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
}
