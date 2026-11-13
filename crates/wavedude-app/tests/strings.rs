//! Every key of the English string table is looked up somewhere in the app, the state, the examples or the core
//! crate, so a key that nothing shows cannot linger; and every other language holds every English key.

use std::collections::HashSet;
use std::path::Path;

use wavedude_app_state::strings::{Language, keys, model_keys};

/// Keys built at run time from a part's type name.
const DYNAMIC_PREFIXES: [&str; 1] = ["Kind."];

/// The gate's output words, named by the gate form's outputs.
const GATE_OUTPUT: [&str; 4] =
	["Enum.GateOutput.Xor", "Enum.GateOutput.And", "Enum.GateOutput.Or", "Enum.GateOutput.Remainder"];

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
	sources(&root.join("wavedude-examples/src"), &mut text);
	sources(&root.join("wavedude-core/src"), &mut text);
	let english: HashSet<&str> = keys(Language::English).collect();
	let named: HashSet<String> = model_keys().into_iter().collect();
	let unused: Vec<&str> = english
		.iter()
		.copied()
		.filter(|k| !DYNAMIC_PREFIXES.iter().any(|p| k.starts_with(p)))
		// a field or a word names its key at run time: the type's name and the member's
		.filter(|k| !named.contains(*k) && !GATE_OUTPUT.contains(k))
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
