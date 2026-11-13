//! The four string tables (ported from the original's resource dictionaries) and a lookup with a live
//! language switch and an English fallback.
//!
//! Keys as in the original: menu entries (`MenuFileNew`, the mnemonic marked by `_`), property labels and hints
//! (`Label.<Type>.<Member>`, `Hint.<Type>.<Member>`), enum words (`Enum.<Type>.<Member>`) and part kind
//! descriptions (`Kind.<Type>`). Placeholders are `{0}`, `{1}`, ...
//!
//! Threads: a `Strings` is plain data; the parsed tables are shared process-wide and immutable.

use std::any::TypeId;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Write};
use std::sync::OnceLock;

use wavedude_core::model::{Field, Fields, Project, Source, Visitor, Word};

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

/// A menu text without its mnemonic marker (`_` before the letter; `__` is a literal underscore) and the byte offset
/// of the mnemonic letter in it.
pub fn split_mnemonic(text: &str) -> (String, Option<usize>) {
	let mut out = String::with_capacity(text.len());
	let mut at = None;
	let mut chars = text.chars();
	while let Some(c) = chars.next() {
		if c != '_' {
			out.push(c);
			continue;
		}
		match chars.next() {
			Some('_') => out.push('_'),
			Some(m) => {
				at.get_or_insert(out.len());
				out.push(m);
			}
			None => {}
		}
	}
	(out, at)
}

/// The text with the mnemonic marker removed.
pub fn strip_mnemonic(text: &str) -> String {
	split_mnemonic(text).0
}

/// The mnemonic letter of a menu text, lower-cased.
pub fn mnemonic(text: &str) -> Option<char> {
	let (text, at) = split_mnemonic(text);
	at.and_then(|i| text[i..].chars().next()).and_then(|c| c.to_lowercase().next())
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

	/// `n` with the language's thousands separator: WPF's `N0`.
	pub fn grouped(&self, n: u64) -> String {
		let digits = n.to_string();
		let separator = self.get("ThousandsSeparator");
		let mut out = String::with_capacity(digits.len() + digits.len() / 3 * separator.len());
		for (i, c) in digits.chars().enumerate() {
			if i > 0 && (digits.len() - i).is_multiple_of(3) {
				out.push_str(separator);
			}
			out.push(c);
		}
		out
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

/// A field (`Type.Member`), every field of a type or every member of an enum (`Type`) no row or combo texts,
/// with the reason.
const NO_TEXT: [(&str, &str); 9] = [
	("LogicGate.Kind", "the element name carries the gate's kind"),
	("InterferenceFilter.Layers", "a list with its own heading and hint"),
	("Outline.Points", "a list with its own heading and hint"),
	("Fiber.Nodes", "a list with its own heading and hint"),
	("ColorScheme.Stops", "a list with its own heading and hint"),
	("Stop.At", "the stop editor's tips name it"),
	("Rgb", "the colour picker edits a colour whole"),
	("Stencil", "a check box"),
	("GateKind", "the gate's kind is its element name"),
];

fn exempt(name: &str) -> bool {
	NO_TEXT.iter().any(|(n, _)| *n == name)
}

/// The keys the types reachable from `Project`, and the settings' `Theme`, need: `Label.` and `Hint.` per field
/// of a type, `Enum.` per member of a `Word`; `reached` holds every `Type.Member` and `Type` the walk met.
#[derive(Default)]
struct ModelKeys {
	seen: HashSet<TypeId>,
	keys: Vec<String>,
	reached: HashSet<String>,
}

impl ModelKeys {
	fn walk() -> Self {
		let mut walk = Self::default();
		Project::visit(&mut walk);
		walk.word::<crate::settings::Theme>();
		// the composite box edits the array as a whole
		walk.field::<Source>(Source::SET_ARRAY.field);
		walk
	}

	fn field<T: Fields>(&mut self, field: T::Field) {
		let name = format!("{}.{}", T::TYPE_NAME, field.name());
		if !exempt(&name) && !exempt(T::TYPE_NAME) {
			self.keys.extend(["Label", "Hint"].map(|prefix| format!("{prefix}.{name}")));
		}
		self.reached.insert(name);
	}
}

impl Visitor for ModelKeys {
	fn fields<T: Fields>(&mut self) {
		if self.seen.insert(TypeId::of::<T>()) {
			self.reached.insert(T::TYPE_NAME.to_owned());
			for field in T::Field::all().into_iter().filter(|f| !f.path().contains('.')) {
				self.field::<T>(field);
			}
		}
	}

	fn word<W: Word>(&mut self) {
		if self.seen.insert(TypeId::of::<W>()) {
			if !exempt(W::TYPE_NAME) {
				self.keys.extend(W::NAMES.iter().map(|name| format!("Enum.{}.{name}", W::TYPE_NAME)));
			}
			self.reached.insert(W::TYPE_NAME.to_owned());
		}
	}
}

/// The `Label.`, `Hint.` and `Enum.` keys of every field and word the model has.
pub fn model_keys() -> Vec<String> {
	ModelKeys::walk().keys
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_model_field_and_word_has_its_texts_in_every_language() {
		let walk = ModelKeys::walk();
		for (name, _) in NO_TEXT {
			assert!(walk.reached.contains(name), "'{name}' is exempt but names no field or word the walk reached");
		}
		assert!(walk.keys.iter().any(|k| k == "Hint.Probe.BoxWidth"));
		assert!(walk.keys.iter().any(|k| k == "Enum.BoxSize.ExtraLarge"));
		for l in Language::ALL {
			for key in &walk.keys {
				let text = tables()[l.index()].get(key);
				assert!(text.is_some_and(|t| !t.is_empty()), "{l:?} lacks {key}");
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
		assert_eq!(split_mnemonic("a__b_c"), ("a_bc".into(), Some(3)));
		assert_eq!(split_mnemonic("Projekt _\u{f6}ffnen..."), ("Projekt \u{f6}ffnen...".into(), Some(8)));
		assert_eq!(mnemonic("_\u{d6}ffnen"), Some('\u{f6}'));
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
	fn thousands_are_grouped_by_the_language() {
		let en = Strings::new(Language::English);
		assert_eq!((en.grouped(0), en.grouped(999), en.grouped(1000)), ("0".into(), "999".into(), "1,000".into()));
		assert_eq!(en.grouped(1234567), "1,234,567");
		assert_eq!(Strings::new(Language::German).grouped(1234567), "1.234.567");
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
