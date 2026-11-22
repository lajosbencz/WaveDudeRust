//! The clipboard and drag text: items and settings objects as a fragment of their `.wdproj` elements
//! ([`wdproj::write_fragment`]), in the app's format and as plain text, so a text editor can take and give it
//! (docs/ui/60-live-editing-and-runs.md). Threads: pure functions.

use waveduderust::io::{FormatError, wdproj};
use waveduderust::model::names::{Named, copy_name};
use waveduderust::model::{ItemKind, ItemRef, Point, Project, View};

pub use waveduderust::io::wdproj::Fragment;

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

/// The fragment text of `objects` of `project` and its `view`, in project order (settings, sources, parts, probes,
/// schemes).
pub fn write_fragment(project: &Project, view: &View, objects: &[Copyable]) -> String {
	let has = |c: Copyable| objects.contains(&c);
	let pick = |kind: ItemKind| -> Vec<usize> {
		let mut v: Vec<usize> = objects
			.iter()
			.filter_map(|o| match o {
				Copyable::Item(i) if i.kind() == kind => project.index_of(*i),
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
		view: has(Copyable::Display).then(|| view.clone()),
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
	pub view: bool,
}

impl Pasted {
	pub fn is_empty(&self) -> bool {
		self.items.is_empty() && !self.scene && !self.simulation && !self.view
	}
}

/// Inserts `items` of `kind` with fresh ids through `put` (the project's inserter of that list).
fn insert<T: Named, I: Into<ItemRef>>(
	project: &mut Project,
	items: Vec<T>,
	kind: ItemKind,
	target: PasteTarget,
	out: &mut Vec<ItemRef>,
	put: impl Fn(&mut Project, usize, T) -> I,
) {
	let start = match target.current {
		Some(Row::Item(i)) if i.kind() == kind => project.index_of(i).map(|at| at + usize::from(!target.before)),
		_ => None,
	}
	.unwrap_or_else(|| kind.len(project));
	for (offset, mut item) in items.into_iter().enumerate() {
		let names = kind.names(project);
		let lower = item.name().to_lowercase();
		if names.iter().any(|o| o.to_lowercase() == lower) {
			let name = copy_name(item.name(), names.iter().copied());
			item.set_name(name);
		}
		out.push(put(project, start + offset, item).into());
	}
}

/// Pastes `fragment` into `project` and its `view`: list items behind the current item of their list (before it with
/// `before`), else at the end, renamed only on a name clash; a settings object replaces only when pasted on
/// its own row or the project root. Composite emitters follow the target's seed by themselves (they are
/// derived from it at run time).
pub fn paste(project: &mut Project, view: &mut View, fragment: Fragment, target: PasteTarget) -> Pasted {
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
	if let Some(pasted_view) = fragment.view.filter(|_| on(Row::Display)) {
		*view = pasted_view;
		pasted.view = true;
	}
	let out = &mut pasted.items;
	insert(project, fragment.sources, ItemKind::Source, target, out, Project::insert_source);
	insert(project, fragment.parts, ItemKind::Part, target, out, Project::insert_part);
	insert(project, fragment.probes, ItemKind::Probe, target, out, Project::insert_probe);
	insert(project, fragment.schemes, ItemKind::Scheme, target, out, Project::insert_color_scheme);
	pasted
}

/// Moves pasted items as a group so the first one's anchor lands on `at` (a drop pixel), the group held
/// inside the canvas.
pub fn place_at(project: &mut Project, items: &[ItemRef], at: Point) {
	use super::selection::Movable;
	let first = items.iter().find_map(|&i| match i {
		ItemRef::Source(id) => project.source(id).map(|s| s.anchors()[0]),
		ItemRef::Part(id) => project.part(id).map(|p| p.anchors()[0]),
		ItemRef::Probe(id) => project.probe(id).map(|p| p.anchors()[0]),
		_ => None,
	});
	if let Some(anchor) = first {
		move_group(project, items, at.x - anchor.x, at.y - anchor.y);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use waveduderust::model::{
		Diffuser, Outline, PartKind, ProbeKind, Rectangle, Simulation, Source, SourceArray, Stencil,
	};

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 400;
		p.scene.height = 300;
		let s = p.add_source();
		p.source_mut(s).unwrap().array = Some(SourceArray::default().with_size_x(3));
		p.source_mut(s).unwrap().frequency = 77.5;
		p.add_part(PartKind::Rectangle(Rectangle::default().with_length(33)));
		let q = p.add_probe();
		p.probe_mut(q).unwrap().kind = ProbeKind::Line;
		p.add_new_color_scheme();
		p
	}

	#[test]
	fn round_trip_of_items_and_settings() {
		let p = project();
		let all = [
			Copyable::Simulation,
			Copyable::Item(ItemRef::at(ItemKind::Source, 0, &p).unwrap()),
			Copyable::Item(ItemRef::at(ItemKind::Part, 0, &p).unwrap()),
			Copyable::Item(ItemRef::at(ItemKind::Probe, 0, &p).unwrap()),
			Copyable::Item(ItemRef::at(ItemKind::Scheme, 0, &p).unwrap()),
		];
		let text = write_fragment(&p, &View::default(), &all);
		assert!(text.starts_with("<Simulation />"), "{text}");
		assert!(!text.contains("<Display"), "{text}");
		let f = read_fragment(&text).unwrap();
		assert_eq!(f.simulation, Some(Simulation::default()));
		assert_eq!(f.sources, p.sources);
		assert_eq!(f.parts, p.parts);
		assert_eq!(f.probes, p.probes);
		assert_eq!(f.schemes, p.color_schemes);
		assert!(f.scene.is_none() && f.view.is_none());
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
		p.source_mut(s).unwrap().array = Some(SourceArray::default().with_size_x(3));
		p.source_mut(s).unwrap().frequency = 77.5;
		let points = vec![Point::new(1, 2), Point::new(30, 4), Point::new(5, 60)];
		p.add_part(PartKind::Outline(Outline::default().with_points(points)));
		p.add_part(PartKind::Diffuser(Diffuser::default()));
		let q = p.add_probe();
		p.probe_mut(q).unwrap().kind = ProbeKind::Line;
		p.add_new_color_scheme();
		p
	}

	#[test]
	fn the_fragment_format_is_kept() {
		let p = fixed_project();
		let all = [
			Copyable::Simulation,
			Copyable::Display,
			Copyable::Item(ItemRef::at(ItemKind::Source, 0, &p).unwrap()),
			Copyable::Item(ItemRef::at(ItemKind::Part, 0, &p).unwrap()),
			Copyable::Item(ItemRef::at(ItemKind::Part, 1, &p).unwrap()),
			Copyable::Item(ItemRef::at(ItemKind::Probe, 0, &p).unwrap()),
			Copyable::Item(ItemRef::at(ItemKind::Scheme, 0, &p).unwrap()),
		];
		assert_eq!(write_fragment(&p, &View::default(), &all), FIXED_FRAGMENT);
		for text in [FIXED_FRAGMENT.to_owned(), FIXED_FRAGMENT.replace('\n', "\r\n")] {
			let f = read_fragment(&text).unwrap();
			let simulation = f.simulation.unwrap();
			assert!(
				(simulation.source_frequency_scale - 1.5).abs() < 1e-12,
				"the 9-point scale in this program's units"
			);
			assert_eq!(simulation.with_source_frequency_scale(1.5), p.simulation);
			assert_eq!(f.view, Some(View::default()));
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
		let mut v = View::default();
		let source = Copyable::Item(ItemRef::at(ItemKind::Source, 0, &p).unwrap());
		let text = write_fragment(&p, &v.clone().with_labels_in_preview(false), &[source, Copyable::Display]);
		let f = read_fragment(&text).unwrap();
		let target =
			PasteTarget { current: Some(Row::Item(ItemRef::at(ItemKind::Source, 0, &p).unwrap())), before: false };
		let pasted = paste(&mut p, &mut v, f.clone(), target);
		assert_eq!(pasted.items, [ItemRef::at(ItemKind::Source, 1, &p).unwrap()]);
		assert_eq!(p.sources[1].name, "Source 1 copy");
		assert!(!pasted.view && v.labels_in_preview, "a settings object replaces only on its own row or the root");
		let pasted = paste(&mut p, &mut v, f, PasteTarget { current: Some(Row::Display), before: false });
		assert!(pasted.view && !v.labels_in_preview);
		assert_eq!(pasted.items, [ItemRef::at(ItemKind::Source, 3, &p).unwrap()], "no source current: appended");
		let unique = Fragment { sources: vec![Source::default().with_name("Fresh".into())], ..Fragment::default() };
		let target =
			PasteTarget { current: Some(Row::Item(ItemRef::at(ItemKind::Source, 1, &p).unwrap())), before: true };
		let pasted = paste(&mut p, &mut v, unique, target);
		assert_eq!(pasted.items, [ItemRef::at(ItemKind::Source, 1, &p).unwrap()]);
		assert_eq!(p.sources[1].name, "Fresh", "no clash, no rename");
	}

	#[test]
	fn place_at_holds_inside() {
		let mut p = project();
		p.sources[0].x = 10.0;
		p.probes[0].kind = ProbeKind::Point;
		p.probes[0].ax = 30;
		let items = [ItemRef::at(ItemKind::Source, 0, &p).unwrap(), ItemRef::at(ItemKind::Probe, 0, &p).unwrap()];
		place_at(&mut p, &items, Point::new(390, 100));
		assert_eq!(p.sources[0].x, 379.0, "the probe 20 px right stops at the edge");
		assert_eq!(p.probes[0].ax, 399);
		assert_eq!(p.sources[0].y, 100.0);
	}
}
