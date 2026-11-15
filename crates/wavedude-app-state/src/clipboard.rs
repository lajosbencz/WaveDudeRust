//! The clipboard and drag text: items and settings objects as a fragment of their `.wdproj` elements
//! ([`wdproj::write_fragment`]), in the app's format and as plain text, so a text editor can take and give it
//! (docs/ui/60-live-editing-and-runs.md). Threads: pure functions.

use wavedude_core::io::{FormatError, wdproj};
use wavedude_core::model::names::copy_name;
use wavedude_core::model::{ItemKind, ItemRef, Point, Project};

pub use wavedude_core::io::wdproj::Fragment;

use super::selection::{Row, move_group};

/// The app's own clipboard format name.
pub const FORMAT: &str = "WaveDudeNext.Objects";

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

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FragmentError {
	#[error("the text holds no objects")]
	Empty,
	#[error("the objects cannot be read: {0}")]
	Malformed(#[from] FormatError),
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
	wdproj::write_fragment(&Fragment {
		scene: has(Copyable::Scene).then(|| project.scene.clone()),
		simulation: has(Copyable::Simulation).then(|| project.simulation.clone()),
		display: has(Copyable::Display).then(|| project.display.clone()),
		sources: pick(ItemKind::Source).into_iter().map(|i| project.sources[i].clone()).collect(),
		parts: pick(ItemKind::Part).into_iter().map(|i| project.parts[i].clone()).collect(),
		probes: pick(ItemKind::Probe).into_iter().map(|i| project.probes[i].clone()).collect(),
		schemes: pick(ItemKind::Scheme).into_iter().map(|i| project.color_schemes[i].clone()).collect(),
	})
}

/// Parses a fragment (the app's format or plain text). Elements other than a project's objects refuse it all.
pub fn read_fragment(text: &str) -> Result<Fragment, FragmentError> {
	let fragment = wdproj::read_fragment(text)?;
	if fragment.is_empty() {
		return Err(FragmentError::Empty);
	}
	Ok(fragment)
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
	use wavedude_core::model::{
		Diffuser, Outline, PartKind, ProbeKind, Rectangle, Simulation, Source, SourceArray, Stencil,
	};

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 400;
		p.scene.height = 300;
		let s = p.add_source();
		p.sources[s].array = Some(SourceArray { size_x: 3, ..SourceArray::default() });
		p.sources[s].frequency = 77.5;
		p.add_part(PartKind::Rectangle(Rectangle { length: 33, ..Rectangle::default() }));
		let q = p.add_probe();
		p.probes[q].kind = ProbeKind::Line;
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

	/// The text the clipboard has always written for [`fixed_project`]; every part kind of the format reads back.
	const FIXED_FRAGMENT: &str = r##"<Simulation FourthOrderLaplacian="false" SourceFrequencyScale="0.8432081593533117" />
<Display>
	<Field ColorScheme="Default field" BlackLevel="-0.05" />
	<Intensity ColorScheme="Default intensity" />
</Display>
<Source Name="Source 1" X="200" Y="540" Frequency="77.5">
	<Array SizeX="3" />
</Source>
<Outline Name="Outline 1" X="200" Y="540">
	<Point X="1" Y="2" />
	<Point X="30" Y="4" />
	<Point X="5" Y="60" />
</Outline>
<Diffuser Name="Diffuser 1" X="200" Y="540" />
<Probe Name="Probe 1" Kind="Line" AX="200" AY="540" BX="300" BY="540" BoxX="0" BoxY="0" />
<ColorScheme Name="Scheme 1">
	<Stop At="0" Color="#000000" />
	<Stop At="1" Color="#FEFEFE" />
</ColorScheme>"##;

	fn fixed_project() -> Project {
		let mut p = Project::default();
		p.scene.width = 400;
		p.simulation.stencil = Stencil::NinePoint;
		p.simulation.source_frequency_scale = 1.5;
		let s = p.add_source();
		p.sources[s].array = Some(SourceArray { size_x: 3, ..SourceArray::default() });
		p.sources[s].frequency = 77.5;
		let points = vec![Point::new(1, 2), Point::new(30, 4), Point::new(5, 60)];
		p.add_part(PartKind::Outline(Outline { points, ..Outline::default() }));
		p.add_part(PartKind::Diffuser(Diffuser::default()));
		let q = p.add_probe();
		p.probes[q].kind = ProbeKind::Line;
		p.add_new_color_scheme();
		p
	}

	#[test]
	fn the_fragment_format_is_kept() {
		let p = fixed_project();
		let all = [
			Copyable::Simulation,
			Copyable::Display,
			Copyable::Item(ItemRef::new(ItemKind::Source, 0)),
			Copyable::Item(ItemRef::new(ItemKind::Part, 0)),
			Copyable::Item(ItemRef::new(ItemKind::Part, 1)),
			Copyable::Item(ItemRef::new(ItemKind::Probe, 0)),
			Copyable::Item(ItemRef::new(ItemKind::Scheme, 0)),
		];
		assert_eq!(write_fragment(&p, &all), FIXED_FRAGMENT);
		for text in [FIXED_FRAGMENT.to_owned(), FIXED_FRAGMENT.replace('\n', "\r\n")] {
			let f = read_fragment(&text).unwrap();
			let simulation = f.simulation.unwrap();
			assert!(
				(simulation.source_frequency_scale - 1.5).abs() < 1e-12,
				"the 9-point scale in this program's units"
			);
			assert_eq!(Simulation { source_frequency_scale: 1.5, ..simulation }, p.simulation);
			assert_eq!(f.display.as_ref(), Some(&p.display));
			assert_eq!(
				(f.sources, f.parts, f.probes, f.schemes),
				(p.sources.clone(), p.parts.clone(), p.probes.clone(), p.color_schemes.clone())
			);
			assert!(f.scene.is_none());
		}
	}

	#[test]
	fn refusals() {
		let refused = |text: &str| match read_fragment(text) {
			Err(FragmentError::Malformed(e)) => e.message,
			other => panic!("{text}: {other:?}"),
		};
		assert_eq!(read_fragment("  "), Err(FragmentError::Empty));
		assert!(refused("<Source Name=\"a\" X=\"1\" Y=\"2\"/><Bogus/>").contains("'Bogus'"));
		assert!(refused("hello <Source Name=\"a\" X=\"1\" Y=\"2\"/>").contains("text outside"));
		refused("<Source Name=\"a\"");
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
		p.probes[0].kind = ProbeKind::Point;
		p.probes[0].ax = 30;
		let items = [ItemRef::new(ItemKind::Source, 0), ItemRef::new(ItemKind::Probe, 0)];
		place_at(&mut p, &items, Point::new(390, 100));
		assert_eq!(p.sources[0].x, 379.0, "the probe 20 px right stops at the edge");
		assert_eq!(p.probes[0].ax, 399);
		assert_eq!(p.sources[0].y, 100.0);
	}
}
