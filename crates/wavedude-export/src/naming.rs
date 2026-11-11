//! Output paths: layer suffixes, sequence folders and files, stills, probe CSV files.

use std::path::{Path, PathBuf};

use wavedude_core::model::Layer;

/// The file-name suffix of a layer: `_field` or `_intensity`.
pub fn layer_suffix(layer: Layer) -> &'static str {
	match layer {
		Layer::Field => "_field",
		Layer::Intensity => "_intensity",
	}
}

/// The output of `layer` for a chosen path: unchanged for one layer; for both layers the stem without a layer
/// suffix, then the layer's suffix, the extension kept.
pub fn layer_path(chosen: &Path, layer: Layer, both: bool) -> PathBuf {
	if !both {
		return chosen.to_path_buf();
	}
	let stem = chosen.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
	let base = ["_field", "_intensity"].iter().find_map(|s| stem.strip_suffix(s)).unwrap_or(&stem);
	let mut name = format!("{base}{}", layer_suffix(layer));
	if let Some(ext) = chosen.extension() {
		name.push('.');
		name.push_str(&ext.to_string_lossy());
	}
	chosen.with_file_name(name)
}

/// Digits of a sequence's frame numbers: `max(4, len(first + planned - 1))`.
pub fn sequence_digits(first: usize, planned: usize) -> usize {
	(first + planned - 1).to_string().len().max(4)
}

/// A sequence chosen as `dir/name.ext` is written into the folder `dir/name/`.
pub fn sequence_folder(chosen: &Path) -> PathBuf {
	chosen.with_extension("")
}

/// File `frame` of a sequence chosen as `dir/name.ext`: `dir/name/name_NNNN.ext`, NNNN the timeline frame padded to
/// `digits`.
pub fn sequence_file(chosen: &Path, frame: usize, digits: usize) -> PathBuf {
	let stem = chosen.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
	let ext = chosen.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
	sequence_folder(chosen).join(format!("{stem}_{frame:0digits$}.{ext}"))
}

/// The proposed file name of a still: `<project>[_field|_intensity]_NNNN.ext`, NNNN the shown frame padded to 4.
pub fn still_file_name(project: &str, layer: Option<Layer>, frame: usize, ext: &str) -> String {
	let suffix = layer.map_or("", layer_suffix);
	format!("{project}{suffix}_{frame:04}.{ext}")
}

/// The CSV file name of a probe: `<project>_<probe>.csv`, the probe name trimmed, `probe` when then blank, the
/// characters Windows refuses in a file name (`<>:"/\|?*` and 0..31) replaced by `_`, inner spaces kept.
pub fn probe_csv_file_name(project: &str, probe: &str) -> String {
	let name: String =
		probe.trim().chars().map(|c| if c < ' ' || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
	let name = if name.is_empty() { "probe" } else { &name };
	format!("{project}_{name}.csv")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sequences_go_into_a_folder_of_their_name() {
		let chosen = Path::new("dir/chosenname.png");
		assert_eq!(sequence_folder(chosen), Path::new("dir/chosenname"));
		assert_eq!(sequence_file(chosen, 7, 4), Path::new("dir/chosenname/chosenname_0007.png"));
		assert_eq!(sequence_digits(0, 10), 4);
		assert_eq!(sequence_digits(9990, 10), 4);
		assert_eq!(sequence_digits(9990, 11), 5);
		assert_eq!(sequence_file(chosen, 12, 5), Path::new("dir/chosenname/chosenname_00012.png"));
	}

	#[test]
	fn both_layers_take_suffixes_on_a_bare_base() {
		let chosen = Path::new("out/run_field.mp4");
		assert_eq!(layer_path(chosen, Layer::Intensity, true), Path::new("out/run_intensity.mp4"));
		assert_eq!(layer_path(chosen, Layer::Field, true), Path::new("out/run_field.mp4"));
		assert_eq!(layer_path(Path::new("out/run.mp4"), Layer::Field, true), Path::new("out/run_field.mp4"));
		assert_eq!(layer_path(chosen, Layer::Intensity, false), chosen);
	}

	#[test]
	fn stills_and_csv_names() {
		assert_eq!(still_file_name("ball", Some(Layer::Field), 655, "png"), "ball_field_0655.png");
		assert_eq!(still_file_name("ball", None, 12345, "exr"), "ball_12345.exr");
		assert_eq!(probe_csv_file_name("ball", "A/B: focus"), "ball_A_B_ focus.csv");
		assert_eq!(probe_csv_file_name("ball", "   "), "ball_probe.csv");
		assert_eq!(probe_csv_file_name("ball", " a\tb "), "ball_a_b.csv");
	}
}
