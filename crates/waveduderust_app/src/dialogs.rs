//! The filters of the string tables, for the platform's file dialogs and the browser's uploads; the browser's file
//! chooser.

use std::path::{Path, PathBuf};

use crate::strings::Strings;

/// The filters of a string table's `Name (*.a)|*.a;*.b|...` list for a file dialog; the tables are ours, so a malformed
/// list is a bug.
pub fn filters(spec: &str) -> Vec<(String, Vec<String>)> {
	let parts: Vec<&str> = spec.split('|').collect();
	parts
		.chunks(2)
		.map(|pair| {
			let [name, patterns] = pair else { panic!("the filter list '{spec}' pairs each name with patterns") };
			let ext = |p: &str| {
				let (_, e) = p.trim().rsplit_once('.').unwrap_or_else(|| panic!("the filter '{p}' has no extension"));
				e.to_owned()
			};
			((*name).to_owned(), patterns.split(';').map(ext).collect())
		})
		.collect()
}

/// Whether a file named `name` is an image for the scene, by its extension.
pub fn is_scene_image(name: &Path) -> bool {
	let ext = name.extension().map_or_default(|e| e.to_string_lossy().to_ascii_lowercase());
	waveduderust::io::image_extensions().contains(&ext.as_str())
}

/// A scene image read from its file.
pub struct SceneImageFile {
	pub name: String,
	/// The path as shown.
	pub path: String,
	pub bytes: Vec<u8>,
}

/// The scene image at `path`, read; its failure's message line, logged with the cause.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_scene_image(s: &Strings, path: &Path) -> Result<SceneImageFile, String> {
	match std::fs::read(path) {
		Ok(bytes) => Ok(SceneImageFile {
			name: path.file_name().map_or_default(|n| n.to_string_lossy().into_owned()),
			path: path.display().to_string(),
			bytes,
		}),
		Err(e) => {
			let line = s.format("OpenError", &[&path.display()]);
			tracing::warn!("{line} {e}");
			Err(line)
		}
	}
}

/// The scene image filter: the string table's name over the extensions this build decodes.
pub fn scene_image_filter(s: &Strings) -> String {
	let exts: Vec<String> = waveduderust::io::image_extensions().iter().map(|e| format!("*.{e}")).collect();
	let (name, rest) = s.get("SceneImageFilter").split_once('|').expect("the filter names the images first");
	let all = rest.split_once('|').map_or("", |(_, all)| all);
	format!("{name}|{}|{all}", exts.join(";"))
}

/// The file name to save the scene image `image` as: its own, else Untitled; `.png` when it has none.
pub fn scene_image_save_name(s: &Strings, image: &str) -> PathBuf {
	let mut name = PathBuf::from(if image.is_empty() { s.get("Untitled") } else { image });
	if name.extension().is_none() {
		name.set_extension("png");
	}
	name
}

/// Saving the embedded scene image: its bytes, the dialog's filter and the file proposed.
#[cfg(not(target_arch = "wasm32"))]
pub struct SceneImageSave {
	pub bytes: std::sync::Arc<[u8]>,
	pub filter: String,
	pub proposed: Option<PathBuf>,
}

/// The save of `doc`'s embedded scene image beside its file or in the project's folder; none without an image.
#[cfg(not(target_arch = "wasm32"))]
pub fn scene_image_save(s: &Strings, doc: &crate::document::Document) -> Option<SceneImageSave> {
	let scene = &doc.project.scene;
	let bytes = scene.image_data.clone()?;
	let name = scene_image_save_name(s, &scene.image);
	let ext = format!(".{}", name.extension().unwrap_or_default().to_string_lossy());
	let dir = Path::new(&scene.image_path).parent().filter(|p| p.is_dir()).or(doc.folder());
	Some(SceneImageSave {
		bytes,
		filter: s.format("SceneImageSaveFilterFormat", &[&ext]),
		proposed: dir.map(|d| d.join(&name)),
	})
}

/// Writes the scene image `bytes` to `target`; its failure's message line, logged with the cause.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_scene_image(s: &Strings, target: &Path, bytes: &[u8]) -> Result<(), String> {
	waveduderust::io::write_atomic(target, bytes).map_err(|e| {
		let line = s.format("SceneImageSaveError", &[&target.display()]);
		tracing::warn!("{line}: {e}");
		line
	})
}

#[cfg(target_arch = "wasm32")]
mod chooser {
	use std::cell::RefCell;

	use wasm_bindgen_futures::JsFuture;
	use web_sys::wasm_bindgen::closure::Closure;
	use web_sys::wasm_bindgen::{JsCast as _, JsValue};
	use web_sys::{HtmlInputElement, js_sys};

	use super::filters;

	/// The `accept` list of a filter list: its extensions, empty (any file) when it offers all files.
	pub fn accept(spec: &str) -> String {
		let exts: Vec<String> = filters(spec).into_iter().flat_map(|(_, exts)| exts).collect();
		if exts.iter().any(|e| e == "*") {
			String::new()
		} else {
			exts.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(",")
		}
	}

	thread_local! {
		/// The last chooser's change handler, with its input; the next chooser drops it, so a cancelled pick holds no
		/// more.
		static CHOOSER: RefCell<Option<Closure<dyn FnMut()>>> = const { RefCell::new(None) };
	}

	/// Opens the browser's file chooser for `accept` and gives `then` the chosen file's name and bytes; nothing when the
	/// user cancels. The chooser needs the user's recent input, which the frame after a click or key still has.
	pub fn upload(accept: &str, then: impl FnOnce(String, Result<Vec<u8>, String>) + 'static) -> Result<(), JsValue> {
		let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
		let input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
		input.set_type("file");
		input.set_accept(accept);
		let chooser = input.clone();
		let changed = Closure::<dyn FnMut()>::once(move || {
			let Some(file) = chooser.files().and_then(|f| f.get(0)) else { return };
			wasm_bindgen_futures::spawn_local(async move {
				let name = file.name();
				let read = JsFuture::from(file.array_buffer()).await;
				then(name, read.map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec()).map_err(|e| format!("{e:?}")));
			});
		});
		input.set_onchange(Some(changed.as_ref().unchecked_ref()));
		CHOOSER.with_borrow_mut(|c| *c = Some(changed));
		input.click();
		Ok(())
	}
}

#[cfg(target_arch = "wasm32")]
pub use chooser::{accept, upload};

#[cfg(test)]
mod tests {
	use crate::strings::Language;

	use super::*;

	#[test]
	fn a_scene_image_saves_as_its_name_or_untitled_with_png_when_it_has_no_extension() {
		let s = Strings::new(Language::English);
		assert_eq!(scene_image_save_name(&s, "wall.jpg"), PathBuf::from("wall.jpg"));
		assert_eq!(scene_image_save_name(&s, "wall"), PathBuf::from("wall.png"));
		assert_eq!(scene_image_save_name(&s, ""), PathBuf::from("Untitled.png"));
	}
}
