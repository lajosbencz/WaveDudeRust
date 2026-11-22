//! The example projects of the Examples dialog, embedded from the repository's `examples` folder, one subfolder per
//! [`Topic`].

use std::sync::OnceLock;

use waveduderust::io::wdproj;
use waveduderust::model::{Project, View};

/// The dialog's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
	Sources,
	Lenses,
	Gratings,
	Metasurfaces,
	Guides,
	Detectors,
}

impl Topic {
	pub const ALL: [Topic; 6] =
		[Topic::Sources, Topic::Lenses, Topic::Gratings, Topic::Metasurfaces, Topic::Guides, Topic::Detectors];

	/// The string table key of the tab's title.
	pub fn title_key(self) -> &'static str {
		match self {
			Topic::Sources => "ExamplesTopicSources",
			Topic::Lenses => "ExamplesTopicLenses",
			Topic::Gratings => "ExamplesTopicGratings",
			Topic::Metasurfaces => "ExamplesTopicMetasurfaces",
			Topic::Guides => "ExamplesTopicGuides",
			Topic::Detectors => "ExamplesTopicDetectors",
		}
	}
}

/// One example: its file, title (the file name without its series prefix), the project's description and its topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Example {
	pub file_name: &'static str,
	pub title: String,
	pub description: String,
	pub topic: Topic,
	bytes: &'static [u8],
}

impl Example {
	/// The file name without its extension, the widget test id's tail.
	pub fn stem(&self) -> &'static str {
		self.file_name.rsplit_once('.').map_or(self.file_name, |(stem, _)| stem)
	}

	/// The file's content.
	pub fn bytes(&self) -> &'static [u8] {
		self.bytes
	}

	/// The widget test id of the dialog's row, `Example:Example07_Random_emitter_lattice`.
	pub fn test_id(&self) -> String {
		format!("Example:{}", self.stem())
	}

	/// The project and its view.
	pub fn load(&self) -> Result<(Project, View), waveduderust::io::FormatError> {
		wdproj::read_str(std::str::from_utf8(self.bytes).expect("an example is UTF-8"))
	}
}

/// `Topic("folder") => [file, ...]`: each file of `examples/<folder>`, embedded, in dialog order.
macro_rules! embed {
	($($topic:ident($folder:literal) => [$($file:literal),* $(,)?]),* $(,)?) => {
		&[$($((Topic::$topic, $file, include_bytes!(concat!("../../../examples/", $folder, "/", $file)))),*),*]
	};
}

/// Next's copies by number, then ours by file name, per folder.
const FILES: &[(Topic, &str, &[u8])] = embed![
	Sources("sources") => [
		"Example01_Source_Mirror_Probe.wdproj",
		"Example02_Coherent_source_gaussian_beam.wdproj",
		"Example03_Directional_pulsed_source.wdproj",
		"Example04_3_sources_3_frequencies.wdproj",
		"Example05_Small_incoherent_source.wdproj",
		"Example06_Large_incoherent_source.wdproj",
		"Example07_Random_emitter_lattice.wdproj",
		"Example09_Laplacian_dispersion.wdproj",
	],
	Lenses("lenses") => [
		"Example10_Beam_expansion.wdproj",
		"Example11_Zone_plate.wdproj",
		"Example19_Sources_with_spherical_mirror.wdproj",
		"Example20_Test_glass.wdproj",
		"Example21_Ball_lens.wdproj",
		"Example22_Coherence_and_ball_lens.wdproj",
		"Example25_Lens_vs_Fresnel_lens.wdproj",
		"01-fresnel-lens-lighthouse.wdproj",
		"06-bessel-beam.wdproj",
		"07-rayleigh-criterion.wdproj",
	],
	Gratings("gratings") => [
		"Example13_Bragg_reflector.wdproj",
		"Example27_Interference_filter.wdproj",
		"Example28_Blazed_grating_spectrometer.wdproj",
		"Example29_Diffraction_on_a_crystal_lattice.wdproj",
		"02-grating-spectrometer.wdproj",
		"03-dichroic-filter.wdproj",
		"10-blazed-grating.wdproj",
	],
	Metasurfaces("metasurfaces") => [
		"Example26_Three_diffusers_in_a_row.wdproj",
		"Example30_Meta_prism.wdproj",
		"04-flat-lens.wdproj",
		"05-beam-steering.wdproj",
		"09-ground-glass.wdproj",
	],
	Guides("guides") => [
		"Example08_Resonant_wave_in_cavity.wdproj",
		"Example12_Pulse_splitter.wdproj",
		"Example14_Resonant_roundabout.wdproj",
		"Example15_Top_up_the_tank.wdproj",
		"Example16_Wave_in_2_boxes.wdproj",
		"Example18_XOR_AND_OR_logic.wdproj",
		"Example23_Fibres.wdproj",
		"Example24_Fibre_cross_over.wdproj",
		"01-single-fiber.wdproj",
		"02-coupled-fibers-full-transfer.wdproj",
		"03-coupled-fibers-close.wdproj",
		"04-crossover.wdproj",
		"05-or-plain.wdproj",
		"06-or-neat.wdproj",
		"07-xor.wdproj",
		"08-xor-gate-part.wdproj",
		"09-or-gate-part.wdproj",
		"10-and-gate-part.wdproj",
	],
	Detectors("detectors") => [
		"Example17_Hanbury_Brown_Twiss.wdproj",
		"Example31_Meta_lens_with_photomultiplier.wdproj",
		"Example32_Multi_Pixel_Photon_Counter.wdproj",
		"Example33_Fibre_splitter_with_2_PMs.wdproj",
		"08-shot-noise.wdproj",
	],
];

/// The file name without its series prefix (`ExampleNN_` or `NN-`), separators as spaces, capitalised:
/// "`Example07_Random_emitter_lattice.wdproj`" is "Random emitter lattice", "01-single-fiber.wdproj" "Single fiber".
fn title(file: &str) -> String {
	let stem = file.strip_suffix(".wdproj").unwrap_or_else(|| panic!("the example '{file}' is not a .wdproj"));
	let (_, name) = stem.split_once(['_', '-']).unwrap_or_else(|| panic!("the example '{file}' has no series prefix"));
	let words = name.replace(['_', '-'], " ");
	let mut chars = words.chars();
	chars.next().map_or_default(|c| c.to_uppercase().chain(chars).collect())
}

fn build(file_name: &'static str, bytes: &'static [u8], title: String, topic: Topic) -> Example {
	let mut example = Example { file_name, title, description: String::new(), topic, bytes };
	let loaded = example.load().unwrap_or_else(|e| panic!("the embedded example '{file_name}' parses: {e}"));
	example.description = loaded.0.description;
	example
}

/// Every example, by topic.
pub fn all() -> &'static [Example] {
	static ALL: OnceLock<Vec<Example>> = OnceLock::new();
	ALL.get_or_init(|| FILES.iter().map(|&(topic, file, bytes)| build(file, bytes, title(file), topic)).collect())
}

/// The indices into [`all`] of the examples of `topic` whose title or description holds `query` in any case, in list
/// order; an empty query matches every one.
pub fn matching(topic: Topic, query: &str) -> Vec<usize> {
	let query = query.trim().to_lowercase();
	let all = all().iter().enumerate();
	all.filter(|(_, e)| {
		e.topic == topic
			&& (query.is_empty()
				|| e.title.to_lowercase().contains(&query)
				|| e.description.to_lowercase().contains(&query))
	})
	.map(|(i, _)| i)
	.collect()
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use super::*;

	#[test]
	fn examples_carry_title_topic_and_description() {
		let all = all();
		for topic in Topic::ALL {
			assert!(all.iter().any(|e| e.topic == topic), "{topic:?} has no example");
		}
		for e in all {
			assert_eq!(e.title, title(e.file_name), "the title of {}", e.file_name);
			assert_eq!(format!("{}.wdproj", e.stem()), e.file_name);
			assert_eq!(e.test_id(), format!("Example:{}", e.stem()));
			assert!(!e.description.is_empty(), "{} describes itself", e.file_name);
		}
		assert_eq!(title("Example07_Random_emitter_lattice.wdproj"), "Random emitter lattice");
		assert_eq!(title("01-single-fiber.wdproj"), "Single fiber");
	}

	#[test]
	fn every_file_of_a_topic_folder_is_embedded_with_that_topic() {
		let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
		let mut on_disk: Vec<String> = Topic::ALL
			.iter()
			.flat_map(|t| {
				let folder = format!("{t:?}").to_lowercase();
				std::fs::read_dir(root.join(&folder))
					.unwrap()
					.map(|e| e.unwrap().file_name().into_string().unwrap())
					.filter(|f| f.ends_with(".wdproj"))
					.map(move |f| format!("{folder}/{f}"))
			})
			.collect();
		let mut embedded: Vec<String> =
			FILES.iter().map(|(t, f, _)| format!("{}/{f}", format!("{t:?}").to_lowercase())).collect();
		on_disk.sort();
		embedded.sort();
		assert_eq!(embedded, on_disk);
	}

	/// A saved example loads and saves back byte for byte.
	#[test]
	fn every_example_saves_back_byte_for_byte() {
		for e in all() {
			let (project, view) = e.load().unwrap();
			let saved = wdproj::write_string(&project, &view, None);
			let (again, again_view) = wdproj::read_str(&saved).unwrap();
			assert_eq!(wdproj::write_string(&again, &again_view, None), saved, "{}", e.file_name);
		}
	}

	#[test]
	fn file_names_are_unique() {
		let names: HashSet<_> = all().iter().map(|e| e.file_name).collect();
		assert_eq!(names.len(), all().len());
	}
}
