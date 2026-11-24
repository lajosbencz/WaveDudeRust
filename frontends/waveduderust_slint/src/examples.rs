//! The embedded example projects (docs/ui/70, Examples dialog), by topic.

use std::sync::OnceLock;

/// The topics in tab order.
pub(crate) const TOPICS: [&str; 6] = [
	"Sources and coherence",
	"Lenses and imaging",
	"Gratings and filters",
	"Metasurfaces and diffusers",
	"Guides, cavities and logic",
	"Detectors and statistics",
];

/// `(topic, file name, text)` of every example.
const FILES: &[(usize, &str, &str)] = include!(concat!(env!("OUT_DIR"), "/examples.rs"));

/// An example as the dialog lists it.
#[derive(Debug)]
pub(crate) struct Example {
	pub(crate) topic: usize,
	pub(crate) file: &'static str,
	pub(crate) title: String,
	pub(crate) description: String,
	pub(crate) text: &'static str,
}

/// `Example02_Coherent_source` is `Coherent source`.
fn title(file: &str) -> String {
	let stem = file.strip_suffix(".wdproj").unwrap_or(file);
	let name = stem.split_once(['_', '-']).map_or(stem, |(_, n)| n).replace(['_', '-'], " ");
	let mut chars = name.chars();
	chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

pub(crate) fn all() -> &'static [Example] {
	static ALL: OnceLock<Vec<Example>> = OnceLock::new();
	ALL.get_or_init(|| {
		FILES
			.iter()
			.map(|&(topic, file, text)| Example {
				topic,
				file,
				title: title(file),
				description: waveduderust::io::wdproj::read_str(text)
					.map_or_else(|_| String::new(), |(p, _)| p.description),
				text,
			})
			.collect()
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_example_parses_and_every_topic_has_some() {
		for e in all() {
			assert!(waveduderust::io::wdproj::read_str(e.text).is_ok(), "{}", e.file);
		}
		assert!((0..TOPICS.len()).all(|t| all().iter().any(|e| e.topic == t)));
		assert_eq!(title("Example02_Coherent_source_gaussian_beam.wdproj"), "Coherent source gaussian beam");
	}
}
