//! The clipboard and drag text: items and settings objects as a fragment of their `.wdproj` elements, in the
//! app's format and as plain text, so a text editor can take and give it (docs/ui/60-live-editing-and-runs.md).
//!
//! Writing goes through the core's `.wdproj` writer: a temporary project holding only the chosen objects is
//! written and its top-level elements are cut out (the element writer itself is private to the core). Reading
//! wraps the fragment's elements in a `Project` element and parses it with the core's reader; an element the
//! format does not know refuses the whole fragment. Threads: pure functions.

use quick_xml::Reader;
use quick_xml::events::Event;
use wavedude_core::io::wdproj;
use wavedude_core::model::names::copy_name;
use wavedude_core::model::{ColorScheme, Display, Part, Point, Probe, Project, Scene, Simulation, Source};

use super::selection::{ItemKind, ItemRef, Row, move_group};

/// The app's own clipboard format name.
pub const FORMAT: &str = "WaveDudeNext.Objects";

const SETTINGS: [&str; 3] = ["Scene", "Simulation", "Display"];
const ITEMS: [&str; 16] = [
	"Source",
	"Probe",
	"ColorScheme",
	"Lens",
	"Polygon",
	"Rectangle",
	"Ellipse",
	"Triangle",
	"Outline",
	"Fiber",
	"BraggReflector",
	"GrinLens",
	"XorGate",
	"AndGate",
	"OrGate",
	"ZonePlate",
];

/// What can be copied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Copyable {
	Scene,
	Simulation,
	Display,
	Item(ItemRef),
}

impl Copyable {
	/// The copyable object of a tree row, if any (folders and the root are not).
	pub fn of_row(row: Row) -> Option<Self> {
		match row {
			Row::Scene => Some(Self::Scene),
			Row::Simulation => Some(Self::Simulation),
			Row::Display => Some(Self::Display),
			Row::Item(i) => Some(Self::Item(i)),
			Row::Project | Row::Folder(_) => None,
		}
	}
}

/// Parsed objects of a fragment, normalised.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fragment {
	pub scene: Option<Scene>,
	pub simulation: Option<Simulation>,
	pub display: Option<Display>,
	pub sources: Vec<Source>,
	pub parts: Vec<Part>,
	pub probes: Vec<Probe>,
	pub schemes: Vec<ColorScheme>,
}

impl Fragment {
	pub fn is_empty(&self) -> bool {
		self.scene.is_none()
			&& self.simulation.is_none()
			&& self.display.is_none()
			&& self.sources.is_empty()
			&& self.parts.is_empty()
			&& self.probes.is_empty()
			&& self.schemes.is_empty()
	}
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FragmentError {
	#[error("the text holds no objects")]
	Empty,
	#[error("'{0}' is not an object of a project")]
	UnknownElement(String),
	#[error("text outside the objects")]
	StrayText,
	#[error("the objects cannot be read: {0}")]
	Malformed(String),
}

/// The top-level elements of `text` as (name, byte span).
fn top_elements(text: &str) -> Result<Vec<(String, std::ops::Range<usize>)>, FragmentError> {
	let mut reader = Reader::from_str(text);
	let mut depth = 0usize;
	let mut start = 0usize;
	let mut name = String::new();
	let mut out = Vec::new();
	loop {
		let before = reader.buffer_position() as usize;
		let event = reader.read_event().map_err(|e| FragmentError::Malformed(e.to_string()))?;
		let after = reader.buffer_position() as usize;
		match event {
			Event::Start(e) => {
				if depth == 0 {
					start = before;
					name = e.name().as_ref().to_owned();
				}
				depth += 1;
			}
			Event::End(_) => {
				depth = depth.checked_sub(1).ok_or_else(|| FragmentError::Malformed("unbalanced end tag".into()))?;
				if depth == 0 {
					out.push((std::mem::take(&mut name), start..after));
				}
			}
			Event::Empty(e) if depth == 0 => {
				out.push((e.name().as_ref().to_owned(), before..after));
			}
			Event::Text(t) if depth == 0 => {
				if !t.chars().all(|c| c.is_ascii_whitespace()) {
					return Err(FragmentError::StrayText);
				}
			}
			Event::CData(_) if depth == 0 => return Err(FragmentError::StrayText),
			Event::Eof => break,
			_ => {}
		}
	}
	if depth != 0 {
		return Err(FragmentError::Malformed("an element is not closed".into()));
	}
	Ok(out)
}

/// Cuts each top-level element of `text` out and removes one level of indentation.
fn dedent(span: &str) -> String {
	span.lines().map(|l| l.strip_prefix('\t').unwrap_or(l)).collect::<Vec<_>>().join("\n")
}

/// The fragment text of `objects` of `project`, in project order (settings, sources, parts, probes, schemes).
pub fn write_fragment(project: &Project, objects: &[Copyable]) -> String {
	let has = |c: Copyable| objects.contains(&c);
	let pick = |kind: ItemKind| -> Vec<usize> {
		let mut v: Vec<usize> = objects
			.iter()
			.filter_map(|o| match o {
				Copyable::Item(i) if i.kind == kind => Some(i.index),
				_ => None,
			})
			.collect();
		v.sort_unstable();
		v.dedup();
		v
	};
	let temp = Project {
		seed: project.seed,
		description: String::new(),
		scene: if has(Copyable::Scene) { project.scene.clone() } else { Scene::default() },
		simulation: if has(Copyable::Simulation) { project.simulation.clone() } else { Simulation::default() },
		display: project.display.clone(),
		sources: pick(ItemKind::Source).into_iter().map(|i| project.sources[i].clone()).collect(),
		parts: pick(ItemKind::Part).into_iter().map(|i| project.parts[i].clone()).collect(),
		probes: pick(ItemKind::Probe).into_iter().map(|i| project.probes[i].clone()).collect(),
		color_schemes: pick(ItemKind::Scheme).into_iter().map(|i| project.color_schemes[i].clone()).collect(),
	};
	let text = wdproj::write_string(&temp, None);
	let body = &text[text.find("<Project ").expect("the writer writes a Project element")..];
	let root_open = body.find('>').expect("the Project element has a start tag") + 1;
	let root_close = body.rfind("</Project>").expect("the Project element has an end tag");
	let inner = &body[root_open..root_close];
	let elements = top_elements(inner).expect("the writer's elements parse");
	let mut out: Vec<String> = Vec::new();
	for (setting, copy) in
		[("Scene", Copyable::Scene), ("Simulation", Copyable::Simulation), ("Display", Copyable::Display)]
	{
		if has(copy) {
			match elements.iter().find(|(n, _)| n == setting) {
				Some((_, span)) => out.push(dedent(&inner[span.clone()])),
				None => out.push(format!("<{setting} />")),
			}
		}
	}
	out.extend(
		elements.iter().filter(|(n, _)| !SETTINGS.contains(&n.as_str())).map(|(_, s)| dedent(&inner[s.clone()])),
	);
	out.join("\n")
}

/// Parses a fragment (the app's format or plain text). Elements other than a project's objects refuse it all.
pub fn read_fragment(text: &str) -> Result<Fragment, FragmentError> {
	let elements = top_elements(text)?;
	if elements.is_empty() {
		return Err(FragmentError::Empty);
	}
	if let Some((name, _)) =
		elements.iter().find(|(n, _)| !SETTINGS.contains(&n.as_str()) && !ITEMS.contains(&n.as_str()))
	{
		return Err(FragmentError::UnknownElement(name.clone()));
	}
	let inner: Vec<&str> = elements.iter().map(|(_, s)| &text[s.clone()]).collect();
	let wrapped = format!(
		"<Project Version=\"{}\" Seed=\"1\">\n{}\n</Project>",
		wavedude_core::model::PROJECT_VERSION,
		inner.join("\n")
	);
	let mut project = wdproj::read_str(&wrapped).map_err(|e| FragmentError::Malformed(e.to_string()))?;
	wavedude_core::io::read_embedded_image_size(&mut project.scene).map_err(FragmentError::Malformed)?;
	let named = |n: &str| elements.iter().any(|(e, _)| e == n);
	Ok(Fragment {
		scene: named("Scene").then_some(project.scene),
		simulation: named("Simulation").then_some(project.simulation),
		display: named("Display").then_some(project.display),
		sources: project.sources,
		parts: project.parts,
		probes: project.probes,
		schemes: project.color_schemes,
	})
}

/// Where a paste lands: the current row and, for a drop on a row's upper half, before it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PasteTarget {
	pub current: Option<Row>,
	pub before: bool,
}

/// What a paste changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pasted {
	/// The inserted items, in project order.
	pub items: Vec<ItemRef>,
	pub scene: bool,
	pub simulation: bool,
	pub display: bool,
}

impl Pasted {
	pub fn is_empty(&self) -> bool {
		self.items.is_empty() && !self.scene && !self.simulation && !self.display
	}
}

fn insert<T: wavedude_core::model::names::Named>(
	list: &mut Vec<T>,
	items: Vec<T>,
	kind: ItemKind,
	target: PasteTarget,
	out: &mut Vec<ItemRef>,
) {
	let start = match target.current {
		Some(Row::Item(i)) if i.kind == kind && i.index < list.len() => i.index + usize::from(!target.before),
		_ => list.len(),
	};
	for (offset, mut item) in items.into_iter().enumerate() {
		let at = start + offset;
		let lower = item.name().to_lowercase();
		if list.iter().any(|o| o.name().to_lowercase() == lower) {
			let name = copy_name(item.name(), list.iter().map(wavedude_core::model::names::Named::name));
			item.set_name(name);
		}
		list.insert(at, item);
		out.push(ItemRef::new(kind, at));
	}
}

/// Pastes `fragment` into `project`: list items behind the current item of their list (before it with
/// `before`), else at the end, renamed only on a name clash; a settings object replaces only when pasted on
/// its own row or the project root. Composite emitters follow the target's seed by themselves (they are
/// derived from it at run time).
pub fn paste(project: &mut Project, fragment: Fragment, target: PasteTarget) -> Pasted {
	let mut pasted = Pasted::default();
	let on = |row: Row| target.current == Some(row) || target.current == Some(Row::Project);
	if let Some(scene) = fragment.scene.filter(|_| on(Row::Scene)) {
		project.scene = scene;
		pasted.scene = true;
	}
	if let Some(simulation) = fragment.simulation.filter(|_| on(Row::Simulation)) {
		project.simulation = simulation;
		pasted.simulation = true;
	}
	if let Some(display) = fragment.display.filter(|_| on(Row::Display)) {
		project.display = display;
		pasted.display = true;
	}
	insert(&mut project.sources, fragment.sources, ItemKind::Source, target, &mut pasted.items);
	insert(&mut project.parts, fragment.parts, ItemKind::Part, target, &mut pasted.items);
	insert(&mut project.probes, fragment.probes, ItemKind::Probe, target, &mut pasted.items);
	insert(&mut project.color_schemes, fragment.schemes, ItemKind::Scheme, target, &mut pasted.items);
	pasted
}

/// Moves pasted items as a group so the first one's anchor lands on `at` (a drop pixel), the group held
/// inside the canvas.
pub fn place_at(project: &mut Project, items: &[ItemRef], at: Point) {
	use super::selection::Movable;
	let first = items.iter().find_map(|i| match i.kind {
		ItemKind::Source => project.sources.get(i.index).map(|s| s.anchors()[0]),
		ItemKind::Part => project.parts.get(i.index).map(|p| p.anchors()[0]),
		ItemKind::Probe => project.probes.get(i.index).map(|p| p.anchors()[0]),
		ItemKind::Scheme => None,
	});
	if let Some(anchor) = first {
		move_group(project, items, at.x - anchor.x, at.y - anchor.y);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_core::model::{PartKind, Rectangle, SourceArray};

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 400;
		p.scene.height = 300;
		let s = p.add_source();
		p.sources[s].array = Some(SourceArray { size_x: 3, ..SourceArray::default() });
		p.sources[s].frequency = 77.5;
		p.add_part(PartKind::Rectangle(Rectangle { length: 33, ..Rectangle::default() }));
		let q = p.add_probe();
		p.probes[q].kind = wavedude_core::model::ProbeKind::Line;
		p.add_new_color_scheme();
		p
	}

	#[test]
	fn round_trip_of_items_and_settings() {
		let p = project();
		let all = [
			Copyable::Simulation,
			Copyable::Item(ItemRef::new(ItemKind::Source, 0)),
			Copyable::Item(ItemRef::new(ItemKind::Part, 0)),
			Copyable::Item(ItemRef::new(ItemKind::Probe, 0)),
			Copyable::Item(ItemRef::new(ItemKind::Scheme, 0)),
		];
		let text = write_fragment(&p, &all);
		assert!(text.starts_with("<Simulation />"), "{text}");
		assert!(!text.contains("<Display"), "{text}");
		let f = read_fragment(&text).unwrap();
		assert_eq!(f.simulation, Some(Simulation::default()));
		assert_eq!(f.sources, p.sources);
		assert_eq!(f.parts, p.parts);
		assert_eq!(f.probes, p.probes);
		assert_eq!(f.schemes, p.color_schemes);
		assert!(f.scene.is_none() && f.display.is_none());
	}

	#[test]
	fn refusals() {
		assert_eq!(read_fragment("  "), Err(FragmentError::Empty));
		assert_eq!(
			read_fragment("<Source Name=\"a\" X=\"1\" Y=\"2\"/><Bogus/>"),
			Err(FragmentError::UnknownElement("Bogus".into()))
		);
		assert_eq!(read_fragment("hello <Source Name=\"a\" X=\"1\" Y=\"2\"/>"), Err(FragmentError::StrayText));
		assert!(matches!(read_fragment("<Source Name=\"a\""), Err(FragmentError::Malformed(_))));
		assert!(read_fragment("<Source Name=\"typed\" X=\"5\" Y=\"6\" Frequency=\"50\" />").is_ok());
	}

	#[test]
	fn paste_rules() {
		let mut p = project();
		p.add_source();
		let text = write_fragment(&p, &[Copyable::Item(ItemRef::new(ItemKind::Source, 0)), Copyable::Display]);
		let f = read_fragment(&text).unwrap();
		let target = PasteTarget { current: Some(Row::Item(ItemRef::new(ItemKind::Source, 0))), before: false };
		let pasted = paste(&mut p, f.clone(), target);
		assert_eq!(pasted.items, [ItemRef::new(ItemKind::Source, 1)]);
		assert_eq!(p.sources[1].name, "Source 1 copy");
		assert!(!pasted.display, "a settings object replaces only on its own row or the root");
		let pasted = paste(&mut p, f, PasteTarget { current: Some(Row::Display), before: false });
		assert!(pasted.display);
		assert_eq!(pasted.items, [ItemRef::new(ItemKind::Source, 3)], "no source current: appended");
		let unique =
			Fragment { sources: vec![Source { name: "Fresh".into(), ..Source::default() }], ..Fragment::default() };
		let pasted = paste(
			&mut p,
			unique,
			PasteTarget { current: Some(Row::Item(ItemRef::new(ItemKind::Source, 1))), before: true },
		);
		assert_eq!(pasted.items, [ItemRef::new(ItemKind::Source, 1)]);
		assert_eq!(p.sources[1].name, "Fresh", "no clash, no rename");
	}

	#[test]
	fn place_at_holds_inside() {
		let mut p = project();
		p.sources[0].x = 10.0;
		p.probes[0].kind = wavedude_core::model::ProbeKind::Point;
		p.probes[0].ax = 30;
		let items = [ItemRef::new(ItemKind::Source, 0), ItemRef::new(ItemKind::Probe, 0)];
		place_at(&mut p, &items, Point::new(390, 100));
		assert_eq!(p.sources[0].x, 379.0, "the probe 20 px right stops at the edge");
		assert_eq!(p.probes[0].ax, 399);
		assert_eq!(p.sources[0].y, 100.0);
	}
}
