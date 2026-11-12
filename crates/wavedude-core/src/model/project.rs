use super::{ColorScheme, Display, Fields, Part, Probe, Scene, Simulation, Source};

/// Current project format version (the `.wdproj` number).
pub const PROJECT_VERSION: u32 = 4;

/// A whole project: settings, items in their list order, project-local colour schemes.
#[derive(Clone, Debug, PartialEq, Fields)]
pub struct Project {
	/// Seeds every random stream (composite sources, emitter gaps).
	pub seed: i32,
	pub description: String,
	#[field(nested)]
	pub scene: Scene,
	#[field(nested)]
	pub simulation: Simulation,
	#[field(nested)]
	pub display: Display,
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
			display: Display::default(),
			sources: Vec::new(),
			parts: Vec::new(),
			probes: Vec::new(),
			color_schemes: Vec::new(),
		}
	}
}

impl Project {
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
