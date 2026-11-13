//! The filters of the string tables, for the platform's file dialogs (`rfd`) and the browser's uploads.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;

use wavedude_app_state::strings::Strings;

/// The filters of a string table's `Name (*.a)|*.a;*.b|...` list for `rfd`; the tables are ours, so a malformed
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

/// The scene image filter: the string table's name over the extensions this build decodes.
pub fn scene_image_filter(s: &Strings) -> String {
	let exts: Vec<String> = wavedude_core::io::image_extensions().iter().map(|e| format!("*.{e}")).collect();
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

/// A platform file dialog with the filters of `spec`, opened at `start`: a folder, or a file to propose.
#[cfg(not(target_arch = "wasm32"))]
pub fn file_dialog(spec: &str, start: Option<&Path>) -> rfd::AsyncFileDialog {
	let mut d = rfd::AsyncFileDialog::new();
	for (name, exts) in filters(spec) {
		d = d.add_filter(name, &exts);
	}
	if let Some(start) = start {
		if start.is_dir() {
			d = d.set_directory(start);
		} else {
			if let Some(dir) = start.parent().filter(|p| p.is_dir()) {
				d = d.set_directory(dir);
			}
			if let Some(name) = start.file_name() {
				d = d.set_file_name(name.to_string_lossy());
			}
		}
	}
	d
}

#[cfg(test)]
mod tests {
	use wavedude_app_state::strings::Language;

	use super::*;

	#[test]
	fn a_scene_image_saves_as_its_name_or_untitled_with_png_when_it_has_no_extension() {
		let s = Strings::new(Language::English);
		assert_eq!(scene_image_save_name(&s, "wall.jpg"), PathBuf::from("wall.jpg"));
		assert_eq!(scene_image_save_name(&s, "wall"), PathBuf::from("wall.png"));
		assert_eq!(scene_image_save_name(&s, ""), PathBuf::from("Untitled.png"));
	}
}
