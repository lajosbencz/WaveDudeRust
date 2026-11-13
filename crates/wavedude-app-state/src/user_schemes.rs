//! The user's colour schemes, kept in the store's scheme folder on both targets: `.wdscheme` files the program
//! writes and the `.wdscheme` and Wave Dude 2D `.json` files the user places there. A scheme is named by its text;
//! the file's stem names one whose text does not.

use std::path::Path;

use wavedude_core::io::schemes::{SchemeStore, parse_author_json};
use wavedude_core::io::{FormatError, wdscheme};
use wavedude_core::model::ColorScheme;

use crate::platform::Store;
use crate::settings::SCHEME_FOLDER;

/// The characters some platform's file names cannot carry, and `%`, which starts an encoded one.
const UNSAFE: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|', '%'];

/// Windows' device names, which no file name may start with before its first dot.
const DEVICES: &[&str] = &[
	"CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
	"LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn escape(c: char, into: &mut String) {
	let mut utf8 = [0; 4];
	for b in c.encode_utf8(&mut utf8).bytes() {
		into.push_str(&format!("%{b:02X}"));
	}
}

/// The stem of the empty name, which no other name encodes to: an encoded `%` is never bare.
const EMPTY: &str = "%";

/// The file stem of scheme `name`: the characters a file name cannot carry, `%`, trailing dots and spaces and the
/// first letter of a device name percent-encoded; a plain name is its own stem.
pub fn file_stem(name: &str) -> String {
	if name.is_empty() {
		return EMPTY.to_owned();
	}
	let kept = name.trim_end_matches(['.', ' ']).len();
	let device = DEVICES.iter().any(|d| name.split('.').next().is_some_and(|base| base.eq_ignore_ascii_case(d)));
	let mut stem = String::with_capacity(name.len());
	for (i, c) in name.char_indices() {
		if c.is_control() || UNSAFE.contains(&c) || i >= kept || (device && i == 0) {
			escape(c, &mut stem);
		} else {
			stem.push(c);
		}
	}
	stem
}

/// The scheme name of file stem `stem`, as [`file_stem`] encodes it; a stem that decodes to no text is its own.
pub fn name_of_stem(stem: &str) -> String {
	if stem == EMPTY {
		return String::new();
	}
	let bytes = stem.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		let hex = bytes.get(i + 1..i + 3).filter(|h| bytes[i] == b'%' && h.iter().all(u8::is_ascii_hexdigit));
		match hex.and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()) {
			Some(b) => {
				out.push(b);
				i += 3;
			}
			None => {
				out.push(bytes[i]);
				i += 1;
			}
		}
	}
	String::from_utf8(out).unwrap_or_else(|_| stem.to_owned())
}

/// Whether two scheme names are one, as the name lookup compares them.
fn same_name(a: &str, b: &str) -> bool {
	a.to_lowercase() == b.to_lowercase()
}

/// Whether `name` is one of the user's schemes.
pub fn has(schemes: &SchemeStore, name: &str) -> bool {
	schemes.user.iter().any(|s| same_name(&s.name, name))
}

/// The scheme of the store's `key`; `None` for a file of another type.
fn read(store: &Store, key: &str) -> Option<Result<ColorScheme, String>> {
	let path = Path::new(key);
	let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
	let parse: fn(&str, &str) -> Result<ColorScheme, FormatError> = match ext.as_str() {
		wdscheme::EXTENSION => wdscheme::read_str,
		"json" => parse_author_json,
		_ => return None,
	};
	let stem = name_of_stem(&path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default());
	Some(
		store
			.read(key)
			.map_err(|e| e.to_string())
			.and_then(|text| parse(text.as_deref().unwrap_or_default(), &stem).map_err(|e| e.to_string())),
	)
}

/// The keys of the scheme folder, in the order of their names regardless of case.
fn keys(store: &Store) -> Vec<String> {
	let mut keys = store.keys(SCHEME_FOLDER).unwrap_or_else(|e| {
		tracing::warn!("colour scheme folder '{}' not read: {e}", store.location(SCHEME_FOLDER));
		Vec::new()
	});
	keys.sort_by_key(|k| k.to_lowercase());
	keys
}

/// The user's colour schemes; files that fail to read are logged and left out.
pub fn load(store: &Store) -> SchemeStore {
	let mut user = Vec::new();
	for key in keys(store) {
		match read(store, &key) {
			Some(Ok(s)) => user.push(s),
			Some(Err(e)) => tracing::warn!("colour scheme '{}' not loaded: {e}", store.location(&key)),
			None => {}
		}
	}
	SchemeStore { user }
}

/// Keeps `scheme` among the user's as `<name>.wdscheme`; where it failed to go, for messages.
pub fn add(store: &Store, scheme: &ColorScheme) -> Result<(), String> {
	let key = format!("{SCHEME_FOLDER}/{}.{}", file_stem(&scheme.name), wdscheme::EXTENSION);
	store.write(&key, &wdscheme::write_string(scheme)).map_err(|e| {
		tracing::warn!("colour scheme '{}' not saved: {e}", store.location(&key));
		store.location(&key)
	})
}

/// Removes every file of the user's that holds a scheme named `name`; the first that stays, for messages.
pub fn remove(store: &Store, name: &str) -> Result<(), String> {
	for key in keys(store) {
		if let Some(Ok(s)) = read(store, &key)
			&& same_name(&s.name, name)
		{
			store.remove(&key).map_err(|e| {
				tracing::warn!("colour scheme '{}' not removed: {e}", store.location(&key));
				store.location(&key)
			})?;
		}
	}
	Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
	use wavedude_core::model::SchemeKind;

	use super::*;

	#[test]
	fn a_plain_name_is_its_own_stem_and_every_name_comes_back() {
		assert_eq!(file_stem("Ice and fire"), "Ice and fire");
		assert_eq!(file_stem("a/b"), "a%2Fb");
		assert_eq!(file_stem("100%"), "100%25");
		assert_eq!(file_stem("end. "), "end%2E%20");
		assert_eq!(file_stem("con.x"), "%63on.x");
		for name in ["", "%", "Ice and fire", "a/b\\c:d*e?f\"g<h>i|j%k", "tab\there", "end. .", "NUL", "Grün", "50%41"]
		{
			assert_eq!(name_of_stem(&file_stem(name)), name, "{name}");
		}
		assert_eq!(name_of_stem("100%"), "100%", "a hand-named file keeps a lone %");
		assert_eq!(name_of_stem("%+F"), "%+F", "a sign is no hex digit");
	}

	fn scheme(name: &str) -> ColorScheme {
		let mut s = wavedude_core::io::schemes::built_in()[0].clone();
		s.name = name.to_owned();
		s.kind = SchemeKind::Field;
		s
	}

	#[test]
	fn a_scheme_of_any_name_is_kept_found_and_removed_by_its_name() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		let odd = "a/b\\c:d*e?f\"g<h>i|j%k. ";
		for name in [odd, "Copper", "", "%"] {
			add(&store, &scheme(name)).unwrap();
		}
		let loaded = load(&store);
		let names = |s: &SchemeStore| s.user.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
		assert_eq!(names(&loaded), ["", "%", odd, "Copper"]);
		assert!(has(&loaded, "COPPER"), "names compare regardless of case");
		for name in [odd.to_uppercase().as_str(), "", "%"] {
			remove(&store, name).unwrap();
		}
		assert_eq!(names(&load(&store)), ["Copper"]);
	}

	#[test]
	fn a_hand_placed_file_is_removed_by_the_name_in_its_text() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		let mut text = wdscheme::write_string(&scheme("Inside"));
		store.write(&format!("{SCHEME_FOLDER}/outside.wdscheme"), &text).unwrap();
		text = text.replace("Name=\"Inside\"", "Name=\"\"");
		store.write(&format!("{SCHEME_FOLDER}/a%2Fb.wdscheme"), &text).unwrap();
		let names: Vec<_> = load(&store).user.into_iter().map(|s| s.name).collect();
		assert_eq!(names, ["a/b", "Inside"], "an unnamed scheme takes its decoded stem");
		remove(&store, "Inside").unwrap();
		remove(&store, "a/b").unwrap();
		assert!(load(&store).user.is_empty());
	}
}
