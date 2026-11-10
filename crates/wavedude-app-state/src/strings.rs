//! The four string tables (ported from the original's resource dictionaries) and a lookup with a live
//! language switch and an English fallback.
//!
//! Keys as in the original: menu entries (`MenuFileNew`, the mnemonic marked by `_`), property labels and hints
//! (`Label.<Type>.<Member>`, `Hint.<Type>.<Member>`), enum words (`Enum.<Type>.<Member>`) and part kind
//! descriptions (`Kind.<Type>`). Placeholders are `{0}`, `{1}`, ...
//!
//! Threads: a `Strings` is plain data; the parsed tables are shared process-wide and immutable.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{Display, Write};
use std::sync::OnceLock;

/// A language of the string tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
	English,
	German,
	French,
	Dutch,
}

impl Language {
	pub const ALL: [Self; 4] = [Self::English, Self::German, Self::French, Self::Dutch];

	/// The settings' code: `en`, `de`, `fr`, `nl`.
	pub fn code(self) -> &'static str {
		match self {
			Self::English => "en",
			Self::German => "de",
			Self::French => "fr",
			Self::Dutch => "nl",
		}
	}

	/// The language's own name, as the View > Language menu lists it.
	pub fn native_name(self) -> &'static str {
		match self {
			Self::English => "English",
			Self::German => "Deutsch",
			Self::French => "Fran\u{e7}ais",
			Self::Dutch => "Nederlands",
		}
	}

	/// A language by code or locale (`de`, `de_AT.UTF-8`, `fr-CA`), case-insensitive.
	pub fn from_code(code: &str) -> Option<Self> {
		let lower = code.trim().to_ascii_lowercase();
		let primary = lower.split(['_', '-', '.', '@']).next()?;
		Self::ALL.into_iter().find(|l| l.code() == primary)
	}

	/// The operating system's display language: the user's preferred locales, else the POSIX locale variables
	/// (`LC_ALL`, `LC_MESSAGES`, `LANG`); English when none names one of the four.
	pub fn system() -> Self {
		sys_locale::get_locales()
			.find_map(|l| Self::from_code(&l))
			.or_else(|| {
				["LC_ALL", "LC_MESSAGES", "LANG"]
					.into_iter()
					.filter_map(|var| std::env::var(var).ok())
					.find(|v| !v.is_empty())
					.and_then(|v| Self::from_code(&v))
			})
			.unwrap_or(Self::English)
	}

	/// The language a setting names; `""` (or anything unknown) is the system's.
	pub fn resolve(setting: &str) -> Self {
		Self::from_code(setting).unwrap_or_else(Self::system)
	}

	fn index(self) -> usize {
		self as usize
	}
}

type Table = HashMap<String, String>;

const SOURCES: [&str; 4] = [
	include_str!("../assets/strings/en.json"),
	include_str!("../assets/strings/de.json"),
	include_str!("../assets/strings/fr.json"),
	include_str!("../assets/strings/nl.json"),
];

fn tables() -> &'static [Table; 4] {
	static TABLES: OnceLock<[Table; 4]> = OnceLock::new();
	TABLES.get_or_init(|| SOURCES.map(|s| serde_json::from_str(s).expect("an embedded string table is valid JSON")))
}

/// `template` with `{0}`, `{1}`, ... replaced by `args` in one pass, so an argument's own braces stay as they are;
/// a placeholder without an argument stays too.
fn substitute(template: &str, args: &[&dyn Display]) -> String {
	let mut out = String::with_capacity(template.len());
	let mut rest = template;
	while let Some(open) = rest.find('{') {
		out.push_str(&rest[..open]);
		let after = &rest[open + 1..];
		let arg = after.find('}').and_then(|close| {
			let digits = &after[..close];
			let i: usize = digits.bytes().all(|b| b.is_ascii_digit()).then(|| digits.parse().ok()).flatten()?;
			Some((args.get(i)?, close))
		});
		match arg {
			Some((arg, close)) => {
				let _ = write!(out, "{arg}");
				rest = &after[close + 1..];
			}
			None => {
				out.push('{');
				rest = after;
			}
		}
	}
	out.push_str(rest);
	out
}

/// The keys of one language's table (tests, tools).
pub fn keys(language: Language) -> impl Iterator<Item = &'static str> {
	tables()[language.index()].keys().map(String::as_str)
}

/// The text with the mnemonic marker removed (`_` before a letter; `__` is a literal underscore).
pub fn strip_mnemonic(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut chars = text.chars().peekable();
	while let Some(c) = chars.next() {
		if c == '_' {
			if let Some(&next) = chars.peek() {
				out.push(next);
				chars.next();
			}
		} else {
			out.push(c);
		}
	}
	out
}

/// The mnemonic letter of a menu text, lower-cased.
pub fn mnemonic(text: &str) -> Option<char> {
	let mut chars = text.chars();
	while let Some(c) = chars.next() {
		if c == '_' {
			match chars.next() {
				Some('_') => {}
				Some(m) => return m.to_lowercase().next(),
				None => return None,
			}
		}
	}
	None
}

/// The string lookup in the current language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strings {
	language: Language,
}

impl Strings {
	pub fn new(language: Language) -> Self {
		Self { language }
	}

	pub fn language(&self) -> Language {
		self.language
	}

	/// The live switch: every lookup afterwards is in `language`.
	pub fn set_language(&mut self, language: Language) {
		self.language = language;
	}

	/// The text of `key`, in English when the language lacks it.
	pub fn try_get(&self, key: &str) -> Option<&'static str> {
		let t = tables();
		t[self.language.index()].get(key).or_else(|| t[Language::English.index()].get(key)).map(String::as_str)
	}

	/// The text of `key`. Every key the code names is in the English table, so a miss is a bug.
	pub fn get(&self, key: &str) -> &'static str {
		self.try_get(key).unwrap_or_else(|| panic!("the English string table lacks '{key}'"))
	}

	/// The text of `key` with `{0}`, `{1}`, ... replaced by `args`.
	pub fn format(&self, key: &str, args: &[&dyn Display]) -> String {
		substitute(self.get(key), args)
	}

	/// The text of the key made of `parts`, composed in a reused buffer, as [`Self::get`].
	fn composed(&self, parts: &[&str]) -> &'static str {
		thread_local!(static KEY: RefCell<String> = const { RefCell::new(String::new()) });
		KEY.with_borrow_mut(|key| {
			key.clear();
			parts.iter().for_each(|p| key.push_str(p));
			self.get(key)
		})
	}

	pub fn label(&self, ty: &str, member: &str) -> &'static str {
		self.composed(&["Label.", ty, ".", member])
	}

	pub fn hint(&self, ty: &str, member: &str) -> &'static str {
		self.composed(&["Hint.", ty, ".", member])
	}

	pub fn enum_word(&self, ty: &str, member: &str) -> &'static str {
		self.composed(&["Enum.", ty, ".", member])
	}

	/// The description of a part kind (`LensPart`, `XorGatePart`, ...).
	pub fn kind(&self, ty: &str) -> &'static str {
		self.composed(&["Kind.", ty])
	}
}

impl Default for Strings {
	fn default() -> Self {
		Self::new(Language::English)
	}
}

/// The original's type names of a model property key of `wavedude_core::model::ranges` (`Part.X` is
/// `OpticalPart.X`, `Material.RefractiveIndex` belongs to the lens and the shapes, ...).
pub fn original_keys(key: &str) -> Vec<String> {
	let (ty, member) = key.split_once('.').expect("a key is Type.Member");
	let types: &[&str] = match ty {
		"Part" => &["OpticalPart"],
		"Point" => &["PartPoint"],
		"Stop" => &["ColorStop"],
		"Material" => &["LensPart", "ShapePart"],
		"Coating" => &["LensPart", "ShapePart", "FiberPart", "GrinLensPart"],
		"Lens" => &["LensPart"],
		"FresnelLens" => &["FresnelLensPart"],
		"Diffuser" => &["DiffuserPart"],
		"InterferenceFilter" => &["InterferenceFilterPart"],
		"SimpleGrating" => &["SimpleGratingPart"],
		"FacettedGrating" => &["FacettedGratingPart"],
		"Polygon" => &["PolygonPart"],
		"Rectangle" => &["RectanglePart"],
		"Ellipse" => &["EllipsePart"],
		"Triangle" => &["TrianglePart"],
		"Fiber" => &["FiberPart"],
		"BraggReflector" => &["BraggReflectorPart"],
		"GrinLens" => &["GrinLensPart"],
		"LogicGate" => &["LogicGatePart"],
		"ZonePlate" => &["ZonePlatePart"],
		other => return vec![format!("{other}.{}", original_member(ty, member))],
	};
	types.iter().map(|t| format!("{t}.{}", original_member(ty, member))).collect()
}

fn original_member<'a>(ty: &str, member: &'a str) -> &'a str {
	match (ty, member) {
		("Stop", "At") => "Position",
		_ => member,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::BTreeSet;

	#[test]
	fn four_tables_share_one_key_set() {
		let en: BTreeSet<&str> = keys(Language::English).collect();
		assert!(en.len() > 700);
		for l in Language::ALL {
			let other: BTreeSet<&str> = keys(l).collect();
			assert_eq!(en.symmetric_difference(&other).collect::<Vec<_>>(), Vec::<&&str>::new(), "{l:?}");
		}
	}

	#[test]
	fn every_model_property_has_label_and_hint() {
		use wavedude_core::model::ranges;
		let all = ranges::ALL.iter().map(|(k, _)| *k).chain(ranges::UNBOUNDED.iter().copied());
		let all: Vec<&str> = all.chain(ranges::DEPENDENT.iter().map(|(k, _)| *k)).collect();
		for l in Language::ALL {
			for key in &all {
				for k in original_keys(key) {
					for prefix in ["Label", "Hint"] {
						let full = format!("{prefix}.{k}");
						let text = tables()[l.index()].get(&full);
						assert!(text.is_some_and(|t| !t.is_empty()), "{l:?} lacks {full} (model key {key})");
					}
				}
			}
		}
	}

	#[test]
	fn lookup_switch_and_fallback() {
		let mut s = Strings::new(Language::English);
		assert_eq!(s.get("MenuFile"), "_File");
		s.set_language(Language::German);
		assert_eq!(s.get("MenuFile"), "_Datei");
		assert_eq!(s.try_get("No.Such.Key"), None);
		assert_eq!(strip_mnemonic("Save _As..."), "Save As...");
		assert_eq!(mnemonic("Save _As..."), Some('a'));
		assert_eq!(mnemonic("a__b"), None);
		assert_eq!(Language::from_code("de_AT.UTF-8"), Some(Language::German));
		assert_eq!(Language::from_code("pt_BR"), None);
		assert_eq!(Language::resolve("nl"), Language::Dutch);
		assert!(!s.kind("LensPart").is_empty());
		assert!(!s.enum_word("SourceMode", "Pulsed").is_empty());
	}

	#[test]
	#[should_panic(expected = "lacks 'No.Such.Key'")]
	fn a_key_missing_from_english_panics() {
		Strings::new(Language::German).get("No.Such.Key");
	}

	#[test]
	fn placeholders_are_replaced_in_one_pass() {
		assert_eq!(substitute("{0} and {1}", &[&"{1}", &7]), "{1} and 7");
		assert_eq!(substitute("{2} {x} {", &[&1]), "{2} {x} {");
		assert_eq!(substitute("a{0}{0}b", &[&"-"]), "a--b");
		let s = Strings::new(Language::English);
		assert_eq!(s.format("OpenError", &[&"{0}.wdproj"]).matches("{0}").count(), 1);
	}
}
