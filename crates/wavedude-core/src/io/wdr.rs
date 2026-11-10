//! The native `.wdr` project: a zip holding `project.json` (the model as sparse JSON under a `format` number,
//! every value at its default left out) and, for a scene with an image, `scene.<ext>` with the embedded bytes
//! verbatim.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use zip::write::SimpleFileOptions;

use super::{FormatError, IoError, Result, UnreadableFile};
use crate::model::normalize::Normalize;
use crate::model::{PartKind, Project};

/// File extension of a native project.
pub const EXTENSION: &str = "wdr";
/// The `format` number this build writes and the newest it reads.
pub const FORMAT: u32 = 1;
const PROJECT_ENTRY: &str = "project.json";
const SCENE_STEM: &str = "scene.";

/// Entry `i` turns the `project` value of format `i + 1` into format `i + 2`.
const MIGRATIONS: [fn(&mut Value); FORMAT as usize - 1] = [];

#[derive(Serialize)]
struct Wrapper {
	format: u32,
	project: Value,
}

/// A read container: the normalised project and what was left out of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
	pub project: Project,
	pub notes: Vec<String>,
}

/// File extension of encoded image bytes, from their signature.
fn image_extension(bytes: &[u8]) -> &'static str {
	match image::guess_format(bytes) {
		Ok(image::ImageFormat::Png) => "png",
		Ok(image::ImageFormat::Jpeg) => "jpg",
		Ok(image::ImageFormat::Bmp) => "bmp",
		Ok(image::ImageFormat::Gif) => "gif",
		Ok(image::ImageFormat::Tiff) => "tif",
		Ok(image::ImageFormat::WebP) => "webp",
		_ => "bin",
	}
}

/// The container's bytes. `folder` makes an image path inside it relative.
pub fn write_bytes(project: &Project, folder: Option<&Path>) -> std::result::Result<Vec<u8>, FormatError> {
	let mut stored = project.without_image();
	stored.scene.image_path = super::stored_image_path(folder, &project.scene.image_path);
	let json = serde_json::to_vec_pretty(&Wrapper { format: FORMAT, project: super::sparse::project(&stored) })
		.map_err(|e| FormatError::new(format!("cannot serialise the project: {e}")))?;
	let zip_err = |e: zip::result::ZipError| FormatError::new(format!("cannot write the container: {e}"));
	let io_err = |e: std::io::Error| FormatError::new(format!("cannot write the container: {e}"));
	let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
	let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
	zip.start_file(PROJECT_ENTRY, deflated).map_err(zip_err)?;
	zip.write_all(&json).map_err(io_err)?;
	if let Some(bytes) = &project.scene.image_data {
		let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
		zip.start_file(format!("{SCENE_STEM}{}", image_extension(bytes)), stored).map_err(zip_err)?;
		zip.write_all(bytes).map_err(io_err)?;
	}
	Ok(zip.finish().map_err(zip_err)?.into_inner())
}

/// Reads a container into a normalised project; the image's size is left at (0, 0). Parts of a kind this
/// build does not know are left out, each with a note.
pub fn read_bytes(bytes: &[u8]) -> std::result::Result<Loaded, FormatError> {
	let bad = |e: zip::result::ZipError| FormatError::new(format!("not a WaveDude project container: {e}"));
	let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(bad)?;
	let mut json = String::new();
	zip.by_name(PROJECT_ENTRY)
		.map_err(bad)?
		.read_to_string(&mut json)
		.map_err(|e| FormatError::new(format!("cannot read {PROJECT_ENTRY}: {e}")))?;
	let mut root: Value = serde_json::from_str(&json)
		.map_err(|e| FormatError { message: format!("{PROJECT_ENTRY}: {e}"), at: Some((e.line(), e.column())) })?;
	let format = root
		.get("format")
		.and_then(Value::as_u64)
		.ok_or_else(|| FormatError::new(format!("{PROJECT_ENTRY} has no format number")))?;
	if format > FORMAT as u64 {
		return Err(FormatError::new(format!("made by a newer version of the program (format {format})")));
	}
	if format == 0 {
		return Err(FormatError::new(format!("{PROJECT_ENTRY} has the format number 0, which no version writes")));
	}
	let mut value = root
		.get_mut("project")
		.map(Value::take)
		.ok_or_else(|| FormatError::new(format!("{PROJECT_ENTRY} has no project")))?;
	migrate(&mut value, format as u32);
	let notes = drop_unknown_parts(&mut value);
	let mut project: Project =
		serde_json::from_value(value).map_err(|e| FormatError::new(format!("{PROJECT_ENTRY}: {e}")))?;
	let scene_entry = zip
		.file_names()
		.find(|n| n.as_ref().map_or(true, |n| n.starts_with(SCENE_STEM)))
		.transpose()
		.map_err(bad)?
		.map(|n| n.into_owned());
	if let Some(name) = scene_entry {
		let mut data = Vec::new();
		zip.by_name(&name)
			.map_err(bad)?
			.read_to_end(&mut data)
			.map_err(|e| FormatError::new(format!("cannot read {name}: {e}")))?;
		project.scene.image_data = Some(data.into());
	}
	project.normalize();
	Ok(Loaded { project, notes })
}

/// Brings the `project` value of a container in format `from` (1 to [`FORMAT`]) to [`FORMAT`].
fn migrate(project: &mut Value, from: u32) {
	for step in &MIGRATIONS[from as usize - 1..] {
		step(project);
	}
}

/// Removes the parts whose kind this build does not know; a note per part.
fn drop_unknown_parts(project: &mut Value) -> Vec<String> {
	let mut notes = Vec::new();
	let Some(parts) = project.get_mut("parts").and_then(Value::as_array_mut) else {
		return notes;
	};
	parts.retain(|part| {
		let Some((kind, _)) = part.get("kind").and_then(Value::as_object).and_then(|k| k.iter().next()) else {
			return true;
		};
		// Every kind's fields have defaults, so an empty body parses for a known kind.
		let probe = Value::Object([(kind.clone(), Value::Object(Default::default()))].into_iter().collect());
		if serde_json::from_value::<PartKind>(probe).is_ok() {
			return true;
		}
		let name = part.get("name").and_then(Value::as_str).unwrap_or_default();
		notes.push(format!("the part '{name}' of the unknown kind '{kind}' was left out"));
		false
	});
	notes
}

/// The JSON of a container for the error quotation, or why there is none.
fn quotable(bytes: &[u8]) -> String {
	zip::ZipArchive::new(Cursor::new(bytes))
		.ok()
		.and_then(|mut z| {
			let mut s = String::new();
			z.by_name(PROJECT_ENTRY).ok()?.read_to_string(&mut s).ok()?;
			Some(s)
		})
		.unwrap_or_else(|| "(not a readable zip container with a project.json)".into())
}

pub fn load(path: &Path) -> Result<Loaded> {
	let bytes = std::fs::read(path).map_err(|source| IoError::Io { path: path.to_owned(), source })?;
	tracing::info!("loading project from '{}'", path.display());
	let mut loaded = read_bytes(&bytes).map_err(|e| UnreadableFile::new(path, &quotable(&bytes), e))?;
	super::decode_image_size(&mut loaded.project, path)?;
	Ok(loaded)
}

pub fn save(project: &Project, path: &Path) -> Result<()> {
	let bytes = write_bytes(project, path.parent()).map_err(|e| UnreadableFile::new(path, "", e))?;
	super::write_file(path, &bytes)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Lens, SceneMode};

	fn png(w: u32, h: u32) -> Vec<u8> {
		let mut out = Vec::new();
		image::RgbImage::new(w, h).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
		out
	}

	#[test]
	fn round_trip_with_image() {
		let mut p = Project { seed: 42, description: "d".into(), ..Project::default() };
		p.scene.mode = SceneMode::Image;
		p.scene.image = "a.png".into();
		p.scene.image_data = Some(png(17, 9).into());
		p.add_source();
		p.add_part(PartKind::Lens(Lens::default()));
		p.add_probe();
		p.normalize();
		let bytes = write_bytes(&p, None).unwrap();
		let back = read_bytes(&bytes).unwrap();
		assert_eq!(back, Loaded { project: p.clone(), notes: Vec::new() });
		let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
		assert!(zip.by_name("scene.png").is_ok());
		let mut json = String::new();
		zip.by_name(PROJECT_ENTRY).unwrap().read_to_string(&mut json).unwrap();
		assert!(json.contains("\"format\": 1"));
		assert!(!json.contains("image_data"));
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("p.wdr");
		save(&p, &path).unwrap();
		assert_eq!(load(&path).unwrap().project.scene.image_size, (17, 9));
	}

	/// The container `p` makes with its JSON passed through `edit`.
	fn edited(p: &Project, edit: impl FnOnce(&mut Value)) -> Vec<u8> {
		let bytes = write_bytes(p, None).unwrap();
		let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
		let mut json = String::new();
		zip.by_name(PROJECT_ENTRY).unwrap().read_to_string(&mut json).unwrap();
		let mut root: Value = serde_json::from_str(&json).unwrap();
		edit(&mut root);
		let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
		w.start_file(PROJECT_ENTRY, SimpleFileOptions::default()).unwrap();
		w.write_all(root.to_string().as_bytes()).unwrap();
		w.finish().unwrap().into_inner()
	}

	#[test]
	fn garbage_and_newer_are_refused() {
		assert!(read_bytes(b"nope").is_err());
		let newer = edited(&Project::default(), |v| v["format"] = (FORMAT + 1).into());
		assert!(read_bytes(&newer).unwrap_err().message.contains("newer"));
		let zero = edited(&Project::default(), |v| v["format"] = 0.into());
		assert!(read_bytes(&zero).unwrap_err().message.contains("format number 0"));
	}

	#[test]
	fn the_current_format_passes_through_the_migration() {
		let mut p = Project::default();
		p.add_source();
		p.normalize();
		let mut value = serde_json::to_value(&p).unwrap();
		let before = value.clone();
		migrate(&mut value, FORMAT);
		assert_eq!(value, before);
	}

	#[test]
	fn an_unknown_part_kind_is_left_out_with_a_note() {
		let mut p = Project::default();
		p.add_part(PartKind::Lens(Lens::default()));
		p.add_part(PartKind::Lens(Lens::default()));
		p.normalize();
		let bytes = edited(&p, |v| {
			v["project"]["parts"][0]["kind"] = serde_json::json!({ "Prism": { "apex": 60 } });
		});
		let loaded = read_bytes(&bytes).unwrap();
		assert_eq!(loaded.project.parts, p.parts[1..]);
		assert_eq!(loaded.notes.len(), 1);
		assert!(
			loaded.notes[0].contains("'Prism'") && loaded.notes[0].contains(&p.parts[0].base.name),
			"{:?}",
			loaded.notes
		);
	}
}
