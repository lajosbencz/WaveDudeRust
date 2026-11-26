use super::{ColorScheme, Fields, Part, Probe, Scene, Simulation, Source};
use derive_setters::Setters;

/// Current project format version (the `.wdproj` number).
pub const PROJECT_VERSION: u32 = 4;

/// A whole project: settings, items in their list order, project-local colour schemes.
#[derive(Clone, Debug, PartialEq, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
#[derive(Setters)]
#[setters(prefix = "with_")]
pub struct Project {
	/// Seeds every random stream (composite sources, emitter gaps).
	pub seed: i32,
	pub description: String,
	#[field(nested)]
	pub scene: Scene,
	#[field(nested)]
	pub simulation: Simulation,
	/// Emitter order.
	#[field(skip)]
	pub sources: Vec<Source>,
	/// Paint order.
	#[field(skip)]
	pub parts: Vec<Part>,
	/// Marker numbering order.
	#[field(skip)]
	pub probes: Vec<Probe>,
	#[field(skip)]
	pub color_schemes: Vec<ColorScheme>,
}

impl Default for Project {
	fn default() -> Self {
		Self {
			seed: 1,
			description: String::new(),
			scene: Scene::default(),
			simulation: Simulation::default(),
			sources: Vec::new(),
			parts: Vec::new(),
			probes: Vec::new(),
			color_schemes: Vec::new(),
		}
	}
}

impl Project {
	/// Whether a run has a wave to make: an enabled source.
	pub fn has_enabled_source(&self) -> bool {
		self.sources.iter().any(|s| s.enabled)
	}

	/// A new project with a random seed in [1, `i32::MAX`).
	pub fn new_random_seed() -> Self {
		use web_time::{SystemTime, UNIX_EPOCH};
		let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() as u64 ^ d.as_secs());
		#[cfg(not(target_arch = "wasm32"))]
		let stream = std::process::id() as u64;
		#[cfg(target_arch = "wasm32")]
		let stream = 0;
		let mut rng = crate::rng::Pcg32::new(nanos, stream);
		Self { seed: rng.next_range(1, i32::MAX - 1), ..Self::default() }
	}

	/// Canvas size the scene simulates: the image's size for an image scene, else the settings'.
	pub fn canvas_size(&self) -> (u32, u32) {
		self.scene.canvas_size()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Source;

	#[test]
	fn only_an_enabled_source_makes_a_project_runnable() {
		let mut project = Project::default();
		assert!(!project.has_enabled_source(), "no source");
		project.sources.push(Source { enabled: false, ..Source::default() });
		assert!(!project.has_enabled_source(), "a disabled source");
		project.sources.push(Source { enabled: true, ..Source::default() });
		assert!(project.has_enabled_source());
	}
}
