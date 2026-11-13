//! Every language holds the keys of the English string table.

use std::collections::HashSet;

use wavedude_app_state::strings::{Language, keys};

#[test]
fn every_language_is_complete() {
	let english: HashSet<&str> = keys(Language::English).collect();
	for language in Language::ALL {
		let table: HashSet<&str> = keys(language).collect();
		let missing: Vec<&&str> = english.iter().filter(|k| !table.contains(*k)).collect();
		let extra: Vec<&&str> = table.iter().filter(|k| !english.contains(*k)).collect();
		assert!(missing.is_empty() && extra.is_empty(), "{language:?}: missing {missing:?}, extra {extra:?}");
	}
}
