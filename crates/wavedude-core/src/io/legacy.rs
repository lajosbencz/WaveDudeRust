//! Import of a Wave Dude 2D `configuration.json`, key by key with the original's fallbacks
//! (which differ from the model's defaults on purpose).
//!
//! The project gets a fresh seed, an existing scene image is embedded, a 9-point project's source
//! frequency scale is divided by [`super::nine_point_speed`], and `normalize` runs at the end. Notes for
//! the user (a missing image, sources without a picked position, ...) are collected, not fatal.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{FormatError, Result, UnreadableFile};
use crate::model::normalize::Normalize;
use crate::model::*;

/// The newest `format_version` understood.
pub const SUPPORTED_FORMAT_VERSION: i32 = 1;

/// An imported project with what the user should know about it.
#[derive(Clone, Debug)]
pub struct Imported {
	pub project: Project,
	/// `project_name`, the suggested file name; may be empty.
	pub name: String,
	pub notes: Vec<String>,
}

type Obj = Map<String, Value>;

fn get<'a>(o: Option<&'a Obj>, key: &str) -> Option<&'a Value> {
	o?.get(key).filter(|v| !v.is_null())
}

fn text(o: Option<&Obj>, key: &str, fallback: &str) -> String {
	match get(o, key) {
		Some(Value::String(s)) => s.clone(),
		Some(Value::Bool(b)) => b.to_string(),
		Some(Value::Number(n)) => n.to_string(),
		_ => fallback.to_owned(),
	}
}

fn flag(o: Option<&Obj>, key: &str, fallback: bool) -> bool {
	get(o, key).and_then(Value::as_bool).unwrap_or(fallback)
}

/// An int from a number (rounded, ties to even) or numeric text.
fn int(o: Option<&Obj>, key: &str, fallback: i32) -> i32 {
	match get(o, key) {
		Some(Value::Number(n)) => match n.as_i64() {
			Some(i) if i32::try_from(i).is_ok() => i as i32,
			_ => n.as_f64().filter(|f| f.abs() < i32::MAX as f64).map_or(fallback, |f| f.round_ties_even() as i32),
		},
		Some(Value::String(s)) => s.trim().parse().unwrap_or(fallback),
		_ => fallback,
	}
}

fn uint(o: Option<&Obj>, key: &str, fallback: i32) -> u32 {
	int(o, key, fallback).max(0) as u32
}

fn double(o: Option<&Obj>, key: &str, fallback: f64) -> f64 {
	match get(o, key) {
		Some(Value::Number(n)) => n.as_f64().unwrap_or(fallback),
		Some(Value::String(s)) => s.trim().parse().unwrap_or(fallback),
		_ => fallback,
	}
}

fn section<'a>(o: &'a Obj, key: &str) -> Option<&'a Obj> {
	o.get(key)?.as_object()
}

fn items(o: Option<&Obj>) -> impl Iterator<Item = &Obj> {
	o.and_then(|s| s.get("items")).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object)
}

fn import_scene(s: Option<&Obj>, scene: &mut Scene, folder: &Path, notes: &mut Vec<String>) {
	scene.mode = if text(s, "mode", "user_defined") == "image" { SceneMode::Image } else { SceneMode::Canvas };
	scene.width = uint(s, "user_width", 1920);
	scene.height = uint(s, "user_height", 1080);
	scene.edges = Edges::Reflective;
	scene.damping_edge =
		if flag(s, "damping_edge_enabled", false) { int(s, "damping_edge_size_px", 64).abs() } else { 0 };
	scene.image_sources = flag(s, "use_sources", false);
	scene.image_source_scale = double(s, "image_source_scale", 1.0);
	scene.object_sources = flag(s, "use_object_sources", false);
	scene.refractive_index = double(s, "background_refractive_index", 1.0);
	let image_file = text(s, "image_file", "");
	if image_file.is_empty() {
		return;
	}
	let image_path: PathBuf = folder.join(image_file.split('/').collect::<PathBuf>());
	match std::fs::read(&image_path) {
		Ok(bytes) => {
			scene.image_data = Some(bytes.into());
			scene.image = image_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
			scene.image_path = std::path::absolute(&image_path).unwrap_or(image_path).to_string_lossy().into_owned();
		}
		Err(e) => {
			scene.image = image_file.clone();
			let problem = if e.kind() == std::io::ErrorKind::NotFound {
				"was not found next to the project".to_owned()
			} else {
				format!("could not be read ({e})")
			};
			notes.push(format!(
				"The scene image '{image_file}' {problem}; its path is kept, but the scene has no image until one is chosen."
			));
		}
	}
}

fn import_simulation(s: Option<&Obj>, project: &mut Project) {
	let sim = &mut project.simulation;
	sim.steps = uint(s, "steps", 2500);
	sim.visualize_every = uint(s, "visualize_every", 2);
	sim.stencil = if flag(s, "use_4th_order_laplacian", false) { Stencil::FourthOrder } else { Stencil::NinePoint };
	sim.wave_speed = double(s, "wave_speed_c", 0.563);
	sim.time_decay = double(s, "time_decay_factor", 10.0);
	sim.source_frequency_scale = double(s, "source_frequency_scale", 2.0);
	sim.intensity_scale = double(s, "intensity_scale", 1.0);
	let d = &mut project.display;
	d.field.preview = flag(s, "preview_field", true);
	d.intensity.preview = flag(s, "preview_intensity", true);
	d.field.color_scheme = text(s, "field_color_scheme", "Default field");
	d.intensity.color_scheme = text(s, "intensity_color_scheme", "Default intensity");
	d.field.brightness = double(s, "field_brightness", 30.0);
	d.intensity.brightness = double(s, "intensity_brightness", 30.0);
	d.field.black_level = double(s, "field_black_level", -0.05);
	d.intensity.black_level = double(s, "intensity_black_level", 0.0);
	d.video.fps = uint(s, "video_fps", 60);
	d.video.quality = uint(s, "video_quality", 85);
}

fn amount(item: &Obj, randomize: bool, key: &str) -> u32 {
	if randomize && flag(Some(item), key, false) { uint(Some(item), &format!("{key}_amount"), 0) } else { 0 }
}

fn import_sources(s: Option<&Obj>, project: &mut Project, notes: &mut Vec<String>) {
	for item in items(s) {
		let it = Some(item);
		let mut name = text(it, "name", "");
		if name.is_empty() {
			name = format!("Source {}", project.sources.len() + 1);
		}
		let mut source = Source {
			x: double(it, "x", 0.0),
			y: double(it, "y", 0.0),
			enabled: flag(it, "source_on", true),
			detach_after: flag(it, "detach_source", false)
				.then(|| u32::try_from(int(it, "detach_after_steps", 1000)).ok())
				.flatten(),
			frequency: double(it, "frequency", 100.0),
			amplitude: double(it, "amplitude", 100.0),
			phase: double(it, "phase_deg", 0.0),
			mode: if text(it, "mode", "continuous") == "pulsed" { SourceMode::Pulsed } else { SourceMode::Continuous },
			pulse_shape: if text(it, "pulse_shape", "gaussian") == "smoothed_square" {
				PulseShape::SmoothedSquare
			} else {
				PulseShape::Gaussian
			},
			pulse_width: uint(it, "pulse_width_cycles", 10),
			off_width: uint(it, "off_state_cycles", 10),
			..Source::default()
		};
		if !flag(it, "has_position", true) {
			notes.push(format!(
				"Source '{name}' had no picked position in the original project; it ran at ({}, {}) there and does here.",
				source.x, source.y
			));
		}
		if text(it, "main_type", "single") == "composite" {
			let randomize = text(it, "composite_mode", "inherit") == "randomize";
			let array = SourceArray {
				size_x: uint(it, "array_size_x", 1),
				size_y: uint(it, "array_size_y", 1),
				spacing_x: int(it, "spacing_x", 10),
				spacing_y: int(it, "spacing_y", 10),
				angle: 0.0,
				random_frequency: amount(item, randomize, "randomize_frequency"),
				random_phase: amount(item, randomize, "randomize_phase"),
				random_off_width: amount(item, randomize, "randomize_off_width"),
				random_position: amount(item, randomize, "randomize_position"),
			};
			if array.random_frequency > 0
				|| array.random_phase > 0
				|| array.random_off_width > 0
				|| array.random_position > 0
			{
				notes.push(format!(
					"Source '{name}' uses random offsets; its emitters are regenerated from this project's seed and differ from the original's."
				));
			}
			source.array = Some(array);
		}
		source.name = name;
		project.sources.push(source);
	}
}

fn import_probes(s: Option<&Obj>, scene: Option<&Obj>, project: &mut Project, notes: &mut Vec<String>) {
	let use_probes = flag(scene, "use_probes", true);
	for item in items(s) {
		let it = Some(item);
		let layer = if text(it, "place_on", "field") == "intensity" { Layer::Intensity } else { Layer::Field };
		let probe = Probe {
			name: format!("Probe {}", project.probes.len() + 1),
			active: flag(it, "active", true) && use_probes,
			layer,
			measurement: match text(it, "measurement", "amplitude").as_str() {
				"intensity" => Measurement::Value,
				"integrated_intensity" => Measurement::IntegratedValue,
				_ => Measurement::Amplitude,
			},
			kind: if text(it, "probe_type", "point") == "line" { ProbeKind::Line } else { ProbeKind::Point },
			ax: int(it, "ax", 100),
			ay: int(it, "ay", 100),
			bx: int(it, "bx", 300),
			by: int(it, "by", 100),
			box_size: match text(it, "box_size", "medium").as_str() {
				"tiny" => BoxSize::Tiny,
				"small" => BoxSize::Small,
				"large" => BoxSize::Large,
				"extra_large" => BoxSize::ExtraLarge,
				_ => BoxSize::Medium,
			},
			box_x: int(it, "box_x", 450),
			box_y: int(it, "box_y", 20),
			scale_mode: if text(it, "scale_mode", "running") == "full" { ScaleMode::Full } else { ScaleMode::Running },
			auto_scale_x: flag(it, "auto_scale_x", true),
			auto_scale_y: flag(it, "auto_scale_y", true),
			y_min: double(it, "y_min", -1.0),
			y_max: double(it, "y_max", 1.0),
			window: uint(it, "running_window", 500),
			start_after: if flag(it, "use_start_after_step", false) { uint(it, "start_after_step", 0) } else { 0 },
			..Probe::default()
		};
		project.probes.push(probe);
	}
	if !use_probes && !project.probes.is_empty() {
		notes.push("The original project had its probes switched off; they are imported inactive.".into());
	}
}

/// Imports from the JSON text of a `configuration.json` in `folder` (where its image is looked for).
/// The scene's `image_size` is left at (0, 0); [`import`] decodes it.
pub fn import_str(json: &str, folder: &Path) -> std::result::Result<Imported, FormatError> {
	// The original reads with comments and trailing commas allowed; JSON5 covers both.
	let root: Value = json5::from_str(json).map_err(|e| FormatError {
		message: format!("not valid JSON: {e}"),
		at: e.position().map(|p| (p.line, p.column)),
	})?;
	let root =
		root.as_object().ok_or_else(|| FormatError::new("not a project configuration (a JSON object was expected)"))?;
	let version = int(Some(root), "format_version", SUPPORTED_FORMAT_VERSION);
	if version > SUPPORTED_FORMAT_VERSION {
		return Err(FormatError::new(format!("made by a newer version of the program (format {version})")));
	}
	let mut notes = Vec::new();
	let mut project = Project { description: text(Some(root), "description", ""), ..Project::new_random_seed() };
	let scene = section(root, "scene");
	import_scene(scene, &mut project.scene, folder, &mut notes);
	import_simulation(section(root, "simulation"), &mut project);
	import_sources(section(root, "object_sources"), &mut project, &mut notes);
	import_probes(section(root, "probes"), scene, &mut project, &mut notes);
	if super::scale_nine_point_frequencies(&mut project, super::nine_point_speed()) {
		tracing::info!(
			"9-point project: the frequency scale is divided by the original kernel's speed {:.4} so the wavelengths stay",
			super::nine_point_speed()
		);
	}
	project.normalize();
	Ok(Imported { project, name: text(Some(root), "project_name", ""), notes })
}

/// Imports a Wave Dude 2D `configuration.json`.
pub fn import(path: &Path) -> Result<Imported> {
	let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
	let json = super::read_text(&path)?;
	let folder = path.parent().expect("a file that was read has a parent folder");
	let mut imported = import_str(&json, folder).map_err(|e| UnreadableFile::new(&path, &json, e))?;
	super::decode_image_size(&mut imported.project, &path)?;
	tracing::info!(
		"imported legacy project '{}' from '{}': {} sources, {} probes",
		imported.name,
		path.display(),
		imported.project.sources.len(),
		imported.project.probes.len()
	);
	Ok(imported)
}

/// The `.wdproj` file name an import suggests: `project_name`, else the folder's name, next to the file.
pub fn suggested_file_name(path: &Path, project_name: &str) -> PathBuf {
	let folder = path.parent().unwrap_or(Path::new(""));
	let name = if project_name.is_empty() {
		folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
	} else {
		project_name.to_owned()
	};
	let name: String =
		name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
	folder.join(format!("{}.wdproj", if name.is_empty() { "project" } else { &name }))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tolerant_readers_and_fallbacks() {
		let json = r#"{"description":"d","scene":{"user_width":"640","damping_edge_enabled":true,"damping_edge_size_px":-16},
			"simulation":{"steps":100.5,"use_4th_order_laplacian":true},
			"object_sources":{"items":[{"frequency":"50"},{"name":"B","main_type":"composite","composite_mode":"randomize","randomize_phase":true,"randomize_phase_amount":20}]},
			"probes":{"items":[{"place_on":"intensity","measurement":"intensity"}]}}"#;
		let i = import_str(json, Path::new("/nowhere")).unwrap();
		let p = &i.project;
		assert_eq!(p.description, "d");
		assert_eq!(
			(p.scene.width, p.scene.height, p.scene.damping_edge, p.scene.edges),
			(640, 1080, 16, Edges::Reflective)
		);
		assert!(!p.scene.object_sources);
		assert_eq!(p.simulation.steps, 100);
		assert_eq!(p.simulation.wave_speed, 0.563);
		assert_eq!(p.sources[0].name, "Source 1");
		assert_eq!(p.sources[0].frequency, 50.0);
		assert_eq!(p.sources[1].array.as_ref().unwrap().random_phase, 20);
		assert_eq!(i.notes.len(), 1);
		let q = &p.probes[0];
		assert_eq!(
			(q.layer, q.measurement, q.ax, q.box_x, q.box_y),
			(Layer::Intensity, Measurement::Value, 100, 450, 20)
		);
	}

	#[test]
	fn newer_format_is_refused() {
		let err = import_str(r#"{"format_version":2}"#, Path::new(".")).unwrap_err();
		assert!(err.message.contains("newer"));
		assert!(import_str("[]", Path::new(".")).is_err());
		assert!(import_str("{", Path::new(".")).unwrap_err().at.is_some());
	}

	#[test]
	fn nine_point_scaling() {
		let i = import_str(r#"{"object_sources":{"items":[{"frequency":100}]}}"#, Path::new(".")).unwrap();
		assert_eq!(i.project.simulation.stencil, Stencil::NinePoint);
		assert_eq!(i.project.sources[0].frequency, 100.0);
		assert!((i.project.simulation.source_frequency_scale - 2.0 / 0.316f64.sqrt()).abs() < 1e-12);
	}

	#[test]
	fn suggested_name() {
		assert_eq!(suggested_file_name(Path::new("/a/b/configuration.json"), "x:y"), Path::new("/a/b/x_y.wdproj"));
		assert_eq!(suggested_file_name(Path::new("/a/b/configuration.json"), ""), Path::new("/a/b/b.wdproj"));
	}
}
