//! The example projects of File > Examples, embedded from the core's fixtures: one entry per file, in the
//! order of the number the file name leads with.

use std::sync::OnceLock;

/// One example: its file, number and title ("Example07_Random_emitter_lattice.wdproj" is 7, "Random emitter
/// lattice").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Example {
	pub file_name: &'static str,
	pub number: u32,
	pub title: &'static str,
	pub bytes: &'static [u8],
}

impl Example {
	/// The menu entry, "7. Random emitter lattice".
	pub fn header(&self) -> String {
		format!("{}. {}", self.number, self.title.replace('_', " "))
	}

	/// The widget test id, `MenuExample07`.
	pub fn automation_id(&self) -> String {
		format!("MenuExample{:02}", self.number)
	}

	pub fn text(&self) -> &'static str {
		std::str::from_utf8(self.bytes).expect("an example is UTF-8")
	}
}

macro_rules! examples {
	($($file:literal),* $(,)?) => {
		[$(($file, include_bytes!(concat!("../../wavedude-core/tests/fixtures/examples/", $file)) as &[u8])),*]
	};
}

const FILES: [(&str, &[u8]); 24] = examples![
	"Example01_Source_Mirror_Probe.wdproj",
	"Example02_Coherent_source_gaussian_beam.wdproj",
	"Example03_Directional_pulsed_source.wdproj",
	"Example04_3_sources_3_frequencies.wdproj",
	"Example05_Small_incoherent_source.wdproj",
	"Example06_Large_incoherent_source.wdproj",
	"Example07_Random_emitter_lattice.wdproj",
	"Example08_Resonant_wave_in_cavity.wdproj",
	"Example09_Laplacian_dispersion.wdproj",
	"Example10_Beam_expansion.wdproj",
	"Example11_Zone_plate.wdproj",
	"Example12_Pulse_splitter.wdproj",
	"Example13_Bragg_reflector.wdproj",
	"Example14_Resonant_roundabout.wdproj",
	"Example15_Top_up_the_tank.wdproj",
	"Example16_Wave_in_2_boxes.wdproj",
	"Example17_Hanbury_Brown_Twiss.wdproj",
	"Example18_XOR_AND_OR_logic.wdproj",
	"Example19_Sources_with_spherical_mirror.wdproj",
	"Example20_Test_glass.wdproj",
	"Example21_Ball_lens.wdproj",
	"Example22_Coherence_and_ball_lens.wdproj",
	"Example23_Fibres.wdproj",
	"Example24_Fibre_cross_over.wdproj",
];

/// Splits "ExampleNN_Title.wdproj" into (NN, Title); every embedded file follows the pattern.
fn parse(file: &'static str) -> (u32, &'static str) {
	file.strip_suffix(".wdproj")
		.and_then(|stem| stem.strip_prefix("Example"))
		.and_then(|rest| rest.split_once('_'))
		.and_then(|(n, title)| n.parse().ok().map(|n| (n, title)))
		.unwrap_or_else(|| panic!("the example '{file}' is not named ExampleNN_Title.wdproj"))
}

/// Every example, by number.
pub fn all() -> &'static [Example] {
	static ALL: OnceLock<Vec<Example>> = OnceLock::new();
	ALL.get_or_init(|| {
		let mut v: Vec<Example> = FILES
			.iter()
			.map(|&(file_name, bytes)| {
				let (number, title) = parse(file_name);
				Example { file_name, number, title, bytes }
			})
			.collect();
		v.sort_by(|a, b| a.number.cmp(&b.number).then(a.file_name.cmp(b.file_name)));
		v
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn twenty_four_numbered_examples() {
		let all = all();
		assert_eq!(all.len(), 24);
		for (i, e) in all.iter().enumerate() {
			let n = i as u32 + 1;
			assert_eq!(e.number, n);
			assert!(e.file_name.starts_with(&format!("Example{n:02}_")), "{}", e.file_name);
			assert!(e.header().starts_with(&format!("{n}. ")));
		}
		assert_eq!(all[0].header(), "1. Source Mirror Probe");
		assert_eq!(all[6].automation_id(), "MenuExample07");
	}

	#[test]
	fn every_example_parses() {
		for e in all() {
			wavedude_core::io::wdproj::read_str(e.text()).unwrap_or_else(|err| panic!("{}: {err}", e.file_name));
		}
	}
}
