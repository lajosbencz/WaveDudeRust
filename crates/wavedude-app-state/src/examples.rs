//! The example projects of the Examples dialog, embedded: WaveDude Next's from the core's fixtures and our own
//! from the repository's `examples` folder; one entry per file, grouped by [`Topic`].

use std::sync::OnceLock;

use wavedude_core::io::{wdproj, wdr};
use wavedude_core::model::Project;

/// The dialog's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
	Sources,
	Lenses,
	Gratings,
	Guides,
	Detectors,
}

impl Topic {
	pub const ALL: [Topic; 5] = [Topic::Sources, Topic::Lenses, Topic::Gratings, Topic::Guides, Topic::Detectors];

	/// The string table key of the tab's title.
	pub fn title_key(self) -> &'static str {
		match self {
			Topic::Sources => "ExamplesTopicSources",
			Topic::Lenses => "ExamplesTopicLenses",
			Topic::Gratings => "ExamplesTopicGratings",
			Topic::Guides => "ExamplesTopicGuides",
			Topic::Detectors => "ExamplesTopicDetectors",
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
	Wdproj,
	Wdr,
}

/// One example: its file, title ("Example07_Random_emitter_lattice.wdproj" is "Random emitter lattice",
/// "01-single-fiber.wdr" is "Single fiber"), the project's description and its topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Example {
	pub file_name: &'static str,
	pub title: String,
	pub description: String,
	pub topic: Topic,
	bytes: &'static [u8],
	format: Format,
}

/// What an example opens as.
pub struct Loaded {
	pub project: Project,
	/// Written for the original's blended source: the brightness needs a fit.
	pub needs_brightness_fit: bool,
}

impl Example {
	/// The file name without its extension, the widget test id's tail.
	pub fn stem(&self) -> &'static str {
		self.file_name.rsplit_once('.').map_or(self.file_name, |(stem, _)| stem)
	}

	/// The widget test id of the dialog's row, `Example:Example07_Random_emitter_lattice`.
	pub fn test_id(&self) -> String {
		format!("Example:{}", self.stem())
	}

	pub fn load(&self) -> Result<Loaded, wavedude_core::io::FormatError> {
		match self.format {
			Format::Wdproj => {
				let read = wdproj::read(self.text())?;
				Ok(Loaded { project: read.project, needs_brightness_fit: !read.own })
			}
			Format::Wdr => Ok(Loaded { project: wdr::read_bytes(self.bytes)?.project, needs_brightness_fit: false }),
		}
	}

	fn text(&self) -> &'static str {
		std::str::from_utf8(self.bytes).expect("an example is UTF-8")
	}
}

const NEXT: [(&str, &[u8]); 24] = [
	(
		"Example01_Source_Mirror_Probe.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj"),
	),
	(
		"Example02_Coherent_source_gaussian_beam.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example02_Coherent_source_gaussian_beam.wdproj"),
	),
	(
		"Example03_Directional_pulsed_source.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example03_Directional_pulsed_source.wdproj"),
	),
	(
		"Example04_3_sources_3_frequencies.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example04_3_sources_3_frequencies.wdproj"),
	),
	(
		"Example05_Small_incoherent_source.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example05_Small_incoherent_source.wdproj"),
	),
	(
		"Example06_Large_incoherent_source.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example06_Large_incoherent_source.wdproj"),
	),
	(
		"Example07_Random_emitter_lattice.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example07_Random_emitter_lattice.wdproj"),
	),
	(
		"Example08_Resonant_wave_in_cavity.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example08_Resonant_wave_in_cavity.wdproj"),
	),
	(
		"Example09_Laplacian_dispersion.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example09_Laplacian_dispersion.wdproj"),
	),
	(
		"Example10_Beam_expansion.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example10_Beam_expansion.wdproj"),
	),
	(
		"Example11_Zone_plate.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example11_Zone_plate.wdproj"),
	),
	(
		"Example12_Pulse_splitter.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example12_Pulse_splitter.wdproj"),
	),
	(
		"Example13_Bragg_reflector.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example13_Bragg_reflector.wdproj"),
	),
	(
		"Example14_Resonant_roundabout.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example14_Resonant_roundabout.wdproj"),
	),
	(
		"Example15_Top_up_the_tank.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example15_Top_up_the_tank.wdproj"),
	),
	(
		"Example16_Wave_in_2_boxes.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj"),
	),
	(
		"Example17_Hanbury_Brown_Twiss.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example17_Hanbury_Brown_Twiss.wdproj"),
	),
	(
		"Example18_XOR_AND_OR_logic.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example18_XOR_AND_OR_logic.wdproj"),
	),
	(
		"Example19_Sources_with_spherical_mirror.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example19_Sources_with_spherical_mirror.wdproj"),
	),
	(
		"Example20_Test_glass.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example20_Test_glass.wdproj"),
	),
	(
		"Example21_Ball_lens.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example21_Ball_lens.wdproj"),
	),
	(
		"Example22_Coherence_and_ball_lens.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example22_Coherence_and_ball_lens.wdproj"),
	),
	("Example23_Fibres.wdproj", include_bytes!("../../wavedude-core/tests/fixtures/examples/Example23_Fibres.wdproj")),
	(
		"Example24_Fibre_cross_over.wdproj",
		include_bytes!("../../wavedude-core/tests/fixtures/examples/Example24_Fibre_cross_over.wdproj"),
	),
];

const OPTICS: [(&str, &[u8]); 10] = [
	("01-fresnel-lens-lighthouse.wdproj", include_bytes!("../../../examples/optics/01-fresnel-lens-lighthouse.wdproj")),
	("02-grating-spectrometer.wdproj", include_bytes!("../../../examples/optics/02-grating-spectrometer.wdproj")),
	("03-dichroic-filter.wdproj", include_bytes!("../../../examples/optics/03-dichroic-filter.wdproj")),
	("04-flat-lens.wdproj", include_bytes!("../../../examples/optics/04-flat-lens.wdproj")),
	("05-beam-steering.wdproj", include_bytes!("../../../examples/optics/05-beam-steering.wdproj")),
	("06-bessel-beam.wdproj", include_bytes!("../../../examples/optics/06-bessel-beam.wdproj")),
	("07-rayleigh-criterion.wdproj", include_bytes!("../../../examples/optics/07-rayleigh-criterion.wdproj")),
	("08-shot-noise.wdproj", include_bytes!("../../../examples/optics/08-shot-noise.wdproj")),
	("09-ground-glass.wdproj", include_bytes!("../../../examples/optics/09-ground-glass.wdproj")),
	("10-blazed-grating.wdproj", include_bytes!("../../../examples/optics/10-blazed-grating.wdproj")),
];

const GATES: [(&str, &[u8]); 10] = [
	("01-single-fiber.wdr", include_bytes!("../../../examples/gates/01-single-fiber.wdr")),
	(
		"02-coupled-fibers-full-transfer.wdr",
		include_bytes!("../../../examples/gates/02-coupled-fibers-full-transfer.wdr"),
	),
	("03-coupled-fibers-close.wdr", include_bytes!("../../../examples/gates/03-coupled-fibers-close.wdr")),
	("04-crossover.wdr", include_bytes!("../../../examples/gates/04-crossover.wdr")),
	("05-or-plain.wdr", include_bytes!("../../../examples/gates/05-or-plain.wdr")),
	("06-or-neat.wdr", include_bytes!("../../../examples/gates/06-or-neat.wdr")),
	("07-xor.wdr", include_bytes!("../../../examples/gates/07-xor.wdr")),
	("08-xor-gate-part.wdr", include_bytes!("../../../examples/gates/08-xor-gate-part.wdr")),
	("09-or-gate-part.wdr", include_bytes!("../../../examples/gates/09-or-gate-part.wdr")),
	("10-and-gate-part.wdr", include_bytes!("../../../examples/gates/10-and-gate-part.wdr")),
];

/// The topic of a Next example by its number.
fn next_topic(number: u32) -> Topic {
	match number {
		1..=7 | 9 => Topic::Sources,
		10 | 11 | 19..=22 => Topic::Lenses,
		13 => Topic::Gratings,
		17 => Topic::Detectors,
		_ => Topic::Guides,
	}
}

/// The topic of an optics example by its slug.
fn optics_topic(slug: &str) -> Topic {
	match slug {
		"fresnel-lens-lighthouse" | "flat-lens" | "bessel-beam" | "rayleigh-criterion" => Topic::Lenses,
		"shot-noise" => Topic::Detectors,
		_ => Topic::Gratings,
	}
}

/// "01-single-fiber.wdr" to ("single-fiber", "Single fiber").
fn slug_title(file: &str) -> (&str, String) {
	let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
	let slug = stem.split_once('-').map_or(stem, |(_, slug)| slug);
	let words = slug.replace('-', " ");
	let mut chars = words.chars();
	let title = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
	(slug, title)
}

/// Splits "ExampleNN_Title.wdproj" into (NN, Title); every embedded file follows the pattern.
fn parse(file: &str) -> (u32, &str) {
	file.strip_suffix(".wdproj")
		.and_then(|stem| stem.strip_prefix("Example"))
		.and_then(|rest| rest.split_once('_'))
		.and_then(|(n, title)| n.parse().ok().map(|n| (n, title)))
		.unwrap_or_else(|| panic!("the example '{file}' is not named ExampleNN_Title.wdproj"))
}

fn build(file_name: &'static str, bytes: &'static [u8], format: Format, title: String, topic: Topic) -> Example {
	let mut example = Example { file_name, title, description: String::new(), topic, bytes, format };
	let loaded = example.load().unwrap_or_else(|e| panic!("the embedded example '{file_name}' parses: {e}"));
	example.description = loaded.project.description;
	example
}

/// Every example: Next's by number, then the optics, then the gates, each by file name.
pub fn all() -> &'static [Example] {
	static ALL: OnceLock<Vec<Example>> = OnceLock::new();
	ALL.get_or_init(|| {
		let mut next: Vec<Example> = NEXT
			.iter()
			.map(|&(file, bytes)| {
				let (number, title) = parse(file);
				build(file, bytes, Format::Wdproj, title.replace('_', " "), next_topic(number))
			})
			.collect();
		next.sort_by_key(|e| parse(e.file_name).0);
		let optics = OPTICS.iter().map(|&(file, bytes)| {
			let (slug, title) = slug_title(file);
			build(file, bytes, Format::Wdproj, title, optics_topic(slug))
		});
		let gates =
			GATES.iter().map(|&(file, bytes)| build(file, bytes, Format::Wdr, slug_title(file).1, Topic::Guides));
		next.into_iter().chain(optics).chain(gates).collect()
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn forty_four_examples_in_five_topics() {
		let all = all();
		assert_eq!(all.len(), 44);
		let counts: Vec<usize> = Topic::ALL.iter().map(|&t| all.iter().filter(|e| e.topic == t).count()).collect();
		assert_eq!(counts, [8, 10, 6, 18, 2]);
		assert_eq!(all[0].title, "Source Mirror Probe");
		assert_eq!(all[6].test_id(), "Example:Example07_Random_emitter_lattice");
		assert!(all.iter().any(|e| e.title == "Fresnel lens lighthouse" && e.topic == Topic::Lenses));
		assert!(all.iter().any(|e| e.title == "Single fiber" && e.topic == Topic::Guides));
		assert!(all.iter().all(|e| !e.description.is_empty()), "every example describes itself");
	}
}
