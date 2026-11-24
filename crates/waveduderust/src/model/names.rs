//! Item names: numbering, copies, the lens/mirror rename and the adders that place
//! a new item at the centre of the canvas.
//!
//! Names are compared case-insensitively within one list.

use super::items::{Identified, insert, position};
use super::normalize::Normalize;
use super::{
	ColorScheme, FresnelLens, Part, PartId, PartKind, Probe, ProbeDetector, ProbeId, Project, SchemeId, Source,
	SourceId,
};

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

/// Sets a lens or Fresnel lens part's mirror flag as the editor does, renaming it by [`mirror_name`].
/// Returns false for a part that is neither.
pub fn set_lens_mirror(part: &mut Part, mirror: bool) -> bool {
	let (PartKind::Lens(lens) | PartKind::FresnelLens(FresnelLens { lens, .. })) = &mut part.kind else {
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

	/// Appends a source named `"Source n"` at the canvas centre.
	pub fn add_source(&mut self) -> SourceId {
		let (x, y) = self.canvas_centre();
		self.add_source_at(x, y)
	}

	pub fn add_source_at(&mut self, x: i32, y: i32) -> SourceId {
		let name = numbered_name(Source::NAME_STEM, names(&self.sources));
		let source = Source { name, x: f64::from(x), y: f64::from(y), ..Source::default() };
		insert(&mut self.sources, usize::MAX, source)
	}

	/// Appends a part of `kind` named `"<stem> n"` at the canvas centre.
	pub fn add_part(&mut self, kind: PartKind) -> PartId {
		let (x, y) = self.canvas_centre();
		self.add_part_at(kind, x, y)
	}

	pub fn add_part_at(&mut self, kind: PartKind, x: i32, y: i32) -> PartId {
		let mut part = Part::new(kind, x, y);
		part.base.name = numbered_name(part.kind.name_stem(), names(&self.parts));
		insert(&mut self.parts, usize::MAX, part)
	}

	/// Appends a probe named `"Probe n"` at the canvas centre, its line end 100 px to the right.
	pub fn add_probe(&mut self) -> ProbeId {
		let (x, y) = self.canvas_centre();
		self.add_probe_at(x, y)
	}

	pub fn add_probe_at(&mut self, x: i32, y: i32) -> ProbeId {
		self.add_detector_at(x, y, ProbeDetector::Standard)
	}

	/// Appends a probe of `detector` at (`x`, `y`), its line end 100 px to the right: `"Probe n"`, or
	/// `"Detector n"` for a probabilistic one.
	pub fn add_detector_at(&mut self, x: i32, y: i32, detector: ProbeDetector) -> ProbeId {
		let stem = if detector == ProbeDetector::Standard { Probe::NAME_STEM } else { Probe::DETECTOR_STEM };
		let name = numbered_name(stem, names(&self.probes));
		let mut probe = Probe { name, ax: x, ay: y, bx: x + 100, by: y, detector, ..Probe::default() };
		// a detector is a line probe of the integrated intensity
		probe.normalize();
		insert(&mut self.probes, usize::MAX, probe)
	}

	/// Appends a grey scheme named `"Scheme n"`.
	pub fn add_new_color_scheme(&mut self) -> SchemeId {
		let name = numbered_name(SCHEME_STEM, names(&self.color_schemes));
		insert(&mut self.color_schemes, usize::MAX, ColorScheme { name, ..ColorScheme::grey() })
	}

	/// Appends a scheme under a fresh id, renamed to a copy name when its name is taken.
	pub fn add_color_scheme(&mut self, mut scheme: ColorScheme) -> SchemeId {
		if is_taken(&scheme.name, names(&self.color_schemes)) {
			scheme.name = copy_name(&scheme.name, names(&self.color_schemes));
		}
		insert(&mut self.color_schemes, usize::MAX, scheme)
	}

	/// Inserts a copy of the source behind it under a copy name and a fresh id; `None` when it is not here.
	pub fn duplicate_source(&mut self, id: SourceId) -> Option<SourceId> {
		duplicate(&mut self.sources, id)
	}

	pub fn duplicate_part(&mut self, id: PartId) -> Option<PartId> {
		duplicate(&mut self.parts, id)
	}

	pub fn duplicate_probe(&mut self, id: ProbeId) -> Option<ProbeId> {
		duplicate(&mut self.probes, id)
	}

	pub fn duplicate_color_scheme(&mut self, id: SchemeId) -> Option<SchemeId> {
		duplicate(&mut self.color_schemes, id)
	}

	/// Inserts a pasted source at `at` under a fresh id, renamed to a copy name when its name is taken.
	pub fn paste_source(&mut self, at: usize, source: Source) -> SourceId {
		paste(&mut self.sources, at, source)
	}

	pub fn paste_part(&mut self, at: usize, part: Part) -> PartId {
		paste(&mut self.parts, at, part)
	}

	pub fn paste_probe(&mut self, at: usize, probe: Probe) -> ProbeId {
		paste(&mut self.probes, at, probe)
	}

	pub fn paste_color_scheme(&mut self, at: usize, scheme: ColorScheme) -> SchemeId {
		paste(&mut self.color_schemes, at, scheme)
	}
}

fn paste<T: Identified>(items: &mut Vec<T>, at: usize, mut item: T) -> T::Id {
	if is_taken(item.name(), names(items)) {
		item.set_name(copy_name(item.name(), names(items)));
	}
	insert(items, at, item)
}

fn duplicate<T: Identified>(items: &mut Vec<T>, id: T::Id) -> Option<T::Id> {
	let i = position(items, id)?;
	let mut copy = items[i].clone();
	copy.set_name(copy_name(items[i].name(), names(items)));
	Some(insert(items, i + 1, copy))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Layer, Lens, Material, Measurement, ProbeKind, Rectangle};

	#[test]
	fn numbered_takes_lowest_free_case_insensitive() {
		assert_eq!(numbered_name("Source", ["source 1", "Source 3"]), "Source 2");
		assert_eq!(numbered_name("Probe", []), "Probe 1");
	}

	#[test]
	fn a_paste_renames_a_taken_name_only() {
		let mut p = Project::default();
		let a = p.add_source();
		let copy = p.source(a).unwrap().clone();
		p.paste_source(1, copy.clone());
		assert_eq!(p.sources[1].name, format!("{} copy", p.sources[0].name));
		assert_ne!(p.sources[1].id, p.sources[0].id);
		p.sources.clear();
		p.paste_source(0, copy.clone());
		assert_eq!(p.sources[0].name, copy.name);
	}

	#[test]
	fn a_detector_is_named_detector() {
		let mut p = Project::default();
		p.add_probe_at(0, 0);
		let d = p.add_detector_at(0, 0, ProbeDetector::Counting);
		assert_eq!(p.probe(d).unwrap().name, "Detector 1");
		let d = p.probe(d).unwrap();
		assert_eq!(d.detector, ProbeDetector::Counting);
		assert_eq!((d.kind, d.layer, d.measurement), (ProbeKind::Line, Layer::Intensity, Measurement::IntegratedValue));
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
		p.add_part(PartKind::Lens(Lens::default()));
		assert_eq!(p.parts[0].base.name, "Lens 1");
		assert!(set_lens_mirror(&mut p.parts[0], true));
		assert_eq!(p.parts[0].base.name, "Mirror 1");
		assert!(set_lens_mirror(&mut p.parts[0], true));
		assert_eq!(p.parts[0].base.name, "Mirror 1");
		let j = p.add_part(PartKind::Rectangle(Rectangle::default()));
		assert!(!set_lens_mirror(p.part_mut(j).unwrap(), true));
	}

	#[test]
	fn adders_centre_and_number() {
		let mut p = Project::default();
		p.scene.width = 801;
		p.scene.height = 501;
		let a = p.add_source();
		let b = p.add_source();
		let sa = p.source(a).unwrap();
		assert_eq!((sa.x, sa.y), (400.0, 250.0));
		assert_eq!(p.source(b).unwrap().name, "Source 2");
		let q = p.add_probe();
		assert_eq!((p.probes[0].bx, p.probes[0].by), (500, 250));
		assert_eq!(p.probes[0].id(), q);
		let mirror =
			PartKind::Lens(Lens { material: Material { mirror: true, ..Material::default() }, ..Lens::default() });
		let m = p.add_part(mirror);
		assert_eq!(p.part(m).unwrap().base.name, "Mirror 1");
		let d = p.duplicate_source(a).unwrap();
		assert_ne!(d, a, "a copy gets a fresh id");
		assert_eq!(p.index_of(d), Some(1));
		assert_eq!(p.sources[1].name, "Source 1 copy");
		assert_eq!(p.source(a).unwrap().name, "Source 1");
		let s = p.add_color_scheme(ColorScheme { name: "x".into(), ..ColorScheme::grey() });
		let t = p.add_color_scheme(ColorScheme { name: "X".into(), ..ColorScheme::grey() });
		assert_eq!(p.color_scheme(s).unwrap().name, "x");
		assert_eq!(p.color_scheme(t).unwrap().name, "X copy");
		let n = p.add_new_color_scheme();
		assert_eq!(p.color_scheme(n).unwrap().name, "Scheme 1");
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
