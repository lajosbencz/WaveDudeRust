use serde::{Deserialize, Serialize};

use super::{ColorScheme, Display, Part, Probe, Scene, Simulation, Source};

/// Current project format version (the `.wdproj` number; the `.wdr` container carries its own).
pub const PROJECT_VERSION: u32 = 3;

/// A whole project: settings, items in their list order, project-local colour schemes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Project {
	/// Seeds every random stream (composite sources, emitter gaps).
	pub seed: i32,
	pub description: String,
	pub scene: Scene,
	pub simulation: Simulation,
	pub display: Display,
	/// Emitter order.
	pub sources: Vec<Source>,
	/// Paint order.
	pub parts: Vec<Part>,
	/// Marker numbering order.
	pub probes: Vec<Probe>,
	pub color_schemes: Vec<ColorScheme>,
}

impl Default for Project {
	fn default() -> Self {
		Self {
			seed: 1,
			description: String::new(),
			scene: Scene::default(),
			simulation: Simulation::default(),
			display: Display::default(),
			sources: Vec::new(),
			parts: Vec::new(),
			probes: Vec::new(),
			color_schemes: Vec::new(),
		}
	}
}

impl Project {
	/// A new project with a random seed in [1, i32::MAX).
	pub fn new_random_seed() -> Self {
		let nanos = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
			.unwrap_or(1);
		let mut rng = crate::rng::Pcg32::new(nanos, std::process::id() as u64);
		Self { seed: rng.next_range(1, i32::MAX - 1), ..Self::default() }
	}

	/// A copy without the scene's image bytes, which are not cloned.
	pub fn without_image(&self) -> Self {
		let Self { seed, description, scene, simulation, display, sources, parts, probes, color_schemes } = self;
		Self {
			seed: *seed,
			description: description.clone(),
			scene: scene.without_image(),
			simulation: simulation.clone(),
			display: display.clone(),
			sources: sources.clone(),
			parts: parts.clone(),
			probes: probes.clone(),
			color_schemes: color_schemes.clone(),
		}
	}

	/// Canvas size the scene simulates: the image's size for an image scene, else the settings'.
	pub fn canvas_size(&self) -> (u32, u32) {
		self.scene.canvas_size()
	}
}
