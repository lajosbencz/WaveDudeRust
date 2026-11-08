//! Item names: numbering, copies, the lens/mirror rename and the adders that place
//! a new item at the centre of the canvas.
//!
//! Names are compared case-insensitively within one list.

use super::{ColorScheme, Part, PartKind, Probe, Project, Source};

/// An item with a user-visible name unique within its list.
pub trait Named {
	fn name(&self) -> &str;
	fn set_name(&mut self, name: String);
}

macro_rules! named {
	($t:ty, $($field:tt)+) => {
		impl Named for $t {
			fn name(&self) -> &str {
				&self.$($field)+
			}
			fn set_name(&mut self, name: String) {
				self.$($field)+ = name;
			}
		}
	};
}

named!(Source, name);
named!(Probe, name);
named!(ColorScheme, name);
named!(Part, base.name);

/// Name stem of a new colour scheme.
pub const SCHEME_STEM: &str = "Scheme";

fn is_taken<'a>(candidate: &str, taken: impl IntoIterator<Item = &'a str>) -> bool {
	let candidate = candidate.to_lowercase();
	taken.into_iter().any(|t| t.to_lowercase() == candidate)
}

/// `"<stem> <n>"` with the lowest free n >= 1.
pub fn numbered_name<'a>(stem: &str, taken: impl IntoIterator<Item = &'a str> + Clone) -> String {
	(1..).map(|n| format!("{stem} {n}")).find(|c| !is_taken(c, taken.clone())).expect("unbounded")
}

/// `"<name> copy"`, then `"<name> copy 2"`, `3`, ...
pub fn copy_name<'a>(name: &str, taken: impl IntoIterator<Item = &'a str> + Clone) -> String {
	let stem = format!("{name} copy");
	if !is_taken(&stem, taken.clone()) {
		return stem;
	}
	(2..).map(|n| format!("{stem} {n}")).find(|c| !is_taken(c, taken.clone())).expect("unbounded")
}

fn names<T: Named>(items: &[T]) -> impl Iterator<Item = &str> + Clone {
	items.iter().map(Named::name)
}

fn is_word_char(c: char) -> bool {
	c.is_alphanumeric() || c == '_'
}

/// Replaces every whole word `from` in `text` with `to`.
fn replace_word(text: &str, from: &str, to: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut last = 0;
	for (at, _) in text.match_indices(from) {
		let before = text[..at].chars().next_back();
		let after = text[at + from.len()..].chars().next();
		if !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char) {
			out.push_str(&text[last..at]);
			out.push_str(to);
			last = at + from.len();
		}
	}
	out.push_str(&text[last..]);
	out
}

/// The name a lens takes when the user toggles its mirror flag: the word "Lens" becomes
/// "Mirror" and back. Called by the editor's command, never on load.
pub fn mirror_name(name: &str, mirror: bool) -> String {
	if mirror { replace_word(name, "Lens", "Mirror") } else { replace_word(name, "Mirror", "Lens") }
}

/// Sets a lens part's mirror flag as the editor does, renaming it by [`mirror_name`].
/// Returns false for a part that is not a lens.
pub fn set_lens_mirror(part: &mut Part, mirror: bool) -> bool {
	let PartKind::Lens(lens) = &mut part.kind else {
		return false;
	};
	if lens.material.mirror != mirror {
		lens.material.mirror = mirror;
		part.base.name = mirror_name(&part.base.name, mirror);
	}
	true
}

/// Gives every empty name a numbered one and every repeated name (case-insensitive) a copy name,
/// keeping the first holder of a name.
pub fn make_unique<T: Named>(items: &mut [T], stem: impl Fn(&T) -> String) {
	for i in 0..items.len() {
		let empty = items[i].name().is_empty();
		if !empty && !is_taken(items[i].name(), names(&items[..i])) {
			continue;
		}
		let others: Vec<String> =
			items.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, t)| t.name().to_owned()).collect();
		let taken = others.iter().map(String::as_str);
		let name = if empty { numbered_name(&stem(&items[i]), taken) } else { copy_name(items[i].name(), taken) };
		if !empty {
			tracing::info!(
				"'{}' is renamed '{name}': the name was taken (a composite source's jitter follows its name)",
				items[i].name()
			);
		}
		items[i].set_name(name);
	}
}

impl Project {
	/// Centre of the canvas the preview shows, integer division.
	pub fn canvas_centre(&self) -> (i32, i32) {
		let (w, h) = self.canvas_size();
		((w / 2) as i32, (h / 2) as i32)
	}

	/// Appends a source named `"Source n"` at the canvas centre; returns its index.
	pub fn add_source(&mut self) -> usize {
		let (x, y) = self.canvas_centre();
		self.add_source_at(x, y)
	}

	pub fn add_source_at(&mut self, x: i32, y: i32) -> usize {
		let name = numbered_name(Source::NAME_STEM, names(&self.sources));
		self.sources.push(Source { name, x: f64::from(x), y: f64::from(y), ..Source::default() });
		self.sources.len() - 1
	}

	/// Appends a part of `kind` named `"<stem> n"` at the canvas centre; returns its index.
	pub fn add_part(&mut self, kind: PartKind) -> usize {
		let (x, y) = self.canvas_centre();
		self.add_part_at(kind, x, y)
	}

	pub fn add_part_at(&mut self, kind: PartKind, x: i32, y: i32) -> usize {
		let mut part = Part::new(kind, x, y);
		part.base.name = numbered_name(part.kind.name_stem(), names(&self.parts));
		self.parts.push(part);
		self.parts.len() - 1
	}

	/// Appends a probe named `"Probe n"` at the canvas centre, its line end 100 px to the right.
	pub fn add_probe(&mut self) -> usize {
		let (x, y) = self.canvas_centre();
		self.add_probe_at(x, y)
	}

	pub fn add_probe_at(&mut self, x: i32, y: i32) -> usize {
		let name = numbered_name(Probe::NAME_STEM, names(&self.probes));
		self.probes.push(Probe { name, ax: x, ay: y, bx: x + 100, by: y, ..Probe::default() });
		self.probes.len() - 1
	}

	/// Appends a grey scheme named `"Scheme n"`.
	pub fn add_new_color_scheme(&mut self) -> usize {
		let name = numbered_name(SCHEME_STEM, names(&self.color_schemes));
		self.color_schemes.push(ColorScheme { name, ..ColorScheme::grey() });
		self.color_schemes.len() - 1
	}

	/// Appends a scheme, renamed to a copy name when its name is taken.
	pub fn add_color_scheme(&mut self, mut scheme: ColorScheme) -> usize {
		if is_taken(&scheme.name, names(&self.color_schemes)) {
			scheme.name = copy_name(&scheme.name, names(&self.color_schemes));
		}
		self.color_schemes.push(scheme);
		self.color_schemes.len() - 1
	}

	/// Inserts a copy of source `i` behind it under a copy name; returns the copy's index.
	pub fn duplicate_source(&mut self, i: usize) -> usize {
		duplicate(&mut self.sources, i)
	}

	pub fn duplicate_part(&mut self, i: usize) -> usize {
		duplicate(&mut self.parts, i)
	}

	pub fn duplicate_probe(&mut self, i: usize) -> usize {
		duplicate(&mut self.probes, i)
	}

	pub fn duplicate_color_scheme(&mut self, i: usize) -> usize {
		duplicate(&mut self.color_schemes, i)
	}
}

fn duplicate<T: Named + Clone>(items: &mut Vec<T>, i: usize) -> usize {
	let mut copy = items[i].clone();
	copy.set_name(copy_name(items[i].name(), names(items)));
	items.insert(i + 1, copy);
	i + 1
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Lens, Material, Rectangle};

	#[test]
	fn numbered_takes_lowest_free_case_insensitive() {
		assert_eq!(numbered_name("Source", ["source 1", "Source 3"]), "Source 2");
		assert_eq!(numbered_name("Probe", []), "Probe 1");
	}

	#[test]
	fn copies_count_from_two() {
		assert_eq!(copy_name("Lens 1", ["Lens 1"]), "Lens 1 copy");
		assert_eq!(copy_name("Lens 1", ["Lens 1", "lens 1 COPY"]), "Lens 1 copy 2");
		assert_eq!(copy_name("Lens 1", ["Lens 1", "Lens 1 copy", "Lens 1 copy 2"]), "Lens 1 copy 3");
	}

	#[test]
	fn mirror_rename_whole_words_only() {
		assert_eq!(mirror_name("Lens 3", true), "Mirror 3");
		assert_eq!(mirror_name("Mirror 3", false), "Lens 3");
		assert_eq!(mirror_name("Lenses Lens", true), "Lenses Mirror");
		assert_eq!(mirror_name("My Lens 2 Lens", true), "My Mirror 2 Mirror");
		assert_eq!(mirror_name("GRIN lens 1", true), "GRIN lens 1");
	}

	#[test]
	fn set_mirror_renames_once() {
		let mut p = Project::default();
		let i = p.add_part(PartKind::Lens(Lens::default()));
		assert_eq!(p.parts[i].base.name, "Lens 1");
		assert!(set_lens_mirror(&mut p.parts[i], true));
		assert_eq!(p.parts[i].base.name, "Mirror 1");
		assert!(set_lens_mirror(&mut p.parts[i], true));
		assert_eq!(p.parts[i].base.name, "Mirror 1");
		let j = p.add_part(PartKind::Rectangle(Rectangle::default()));
		assert!(!set_lens_mirror(&mut p.parts[j], true));
	}

	#[test]
	fn adders_centre_and_number() {
		let mut p = Project::default();
		p.scene.width = 801;
		p.scene.height = 501;
		let a = p.add_source();
		let b = p.add_source();
		assert_eq!((p.sources[a].x, p.sources[a].y), (400.0, 250.0));
		assert_eq!(p.sources[b].name, "Source 2");
		let q = p.add_probe();
		assert_eq!((p.probes[q].bx, p.probes[q].by), (500, 250));
		let mirror =
			PartKind::Lens(Lens { material: Material { mirror: true, ..Material::default() }, ..Lens::default() });
		let m = p.add_part(mirror);
		assert_eq!(p.parts[m].base.name, "Mirror 1");
		let d = p.duplicate_source(a);
		assert_eq!(d, 1);
		assert_eq!(p.sources[1].name, "Source 1 copy");
		let s = p.add_color_scheme(ColorScheme { name: "x".into(), ..ColorScheme::grey() });
		let t = p.add_color_scheme(ColorScheme { name: "X".into(), ..ColorScheme::grey() });
		assert_eq!(p.color_schemes[s].name, "x");
		assert_eq!(p.color_schemes[t].name, "X copy");
		let n = p.add_new_color_scheme();
		assert_eq!(p.color_schemes[n].name, "Scheme 1");
	}

	#[test]
	fn unique_names() {
		let mut v: Vec<Source> =
			["A", "", "a", "A copy"].iter().map(|n| Source { name: (*n).into(), ..Source::default() }).collect();
		make_unique(&mut v, |_| Source::NAME_STEM.into());
		let got: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
		assert_eq!(got, ["A", "Source 1", "a copy 2", "A copy"]);
	}
}
