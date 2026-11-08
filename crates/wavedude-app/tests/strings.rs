//! Every key of the English string table is looked up somewhere in the app or the state crate, so a key that
//! nothing shows cannot linger; and every other language holds the same keys.

use std::collections::HashSet;
use std::path::Path;

use wavedude_app_state::strings::{Language, keys};

/// Keys built at run time from a prefix and a member name.
const DYNAMIC_PREFIXES: [&str; 4] = ["Label.", "Hint.", "Enum.", "Kind."];

fn sources(dir: &Path, out: &mut String) {
	for entry in std::fs::read_dir(dir).expect("a source folder") {
		let path = entry.unwrap().path();
		if path.is_dir() {
			sources(&path, out);
		} else if path.extension().is_some_and(|e| e == "rs") {
			out.push_str(&std::fs::read_to_string(&path).unwrap());
		}
	}
}

#[test]
fn every_english_key_is_used_and_every_language_is_complete() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
	let mut text = String::new();
	sources(&root.join("wavedude-app/src"), &mut text);
	sources(&root.join("wavedude-app-state/src"), &mut text);
	let english: HashSet<&str> = keys(Language::English).collect();
	let unused: Vec<&str> = english
		.iter()
		.copied()
		.filter(|k| !DYNAMIC_PREFIXES.iter().any(|p| k.starts_with(p)))
		.filter(|k| !text.contains(&format!("\"{k}\"")))
		.collect();
	assert!(unused.is_empty(), "string keys nothing looks up: {unused:?}");
	for language in Language::ALL {
		let table: HashSet<&str> = keys(language).collect();
		let missing: Vec<&&str> = english.iter().filter(|k| !table.contains(*k)).collect();
		let extra: Vec<&&str> = table.iter().filter(|k| !english.contains(*k)).collect();
		assert!(missing.is_empty() && extra.is_empty(), "{language:?}: missing {missing:?}, extra {extra:?}");
	}
}
