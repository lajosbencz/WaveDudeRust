//! The project tree's rows and selection (docs/ui/20, Tree structure, Selection model).

use waveduderust::model::{
	BraggReflector, Diffuser, Ellipse, FacettedGrating, Fiber, FresnelLens, GateKind, GrinLens, InterferenceFilter,
	Lens, LogicGate, MetaAxiconDesign, MetaDesign, MetaLensDesign, MetaPrismDesign, MetaSurface, Outline, PartKind,
	Polygon, ProbeDetector, Project, Rectangle, SimpleGrating, Triangle, ZonePlate,
};

use crate::ui::{TreeKind, TreeRow};

/// What a tree row stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Node {
	#[default]
	Project,
	Scene,
	Simulation,
	Display,
	Sources,
	Parts,
	Probes,
	Schemes,
	Source(usize),
	Part(usize),
	Probe(usize),
	Scheme(usize),
}

impl Node {
	/// The row's kind as the UI draws it.
	fn kind(self) -> TreeKind {
		match self {
			Self::Project => TreeKind::Project,
			Self::Scene | Self::Simulation | Self::Display => TreeKind::Settings,
			Self::Sources | Self::Parts | Self::Probes | Self::Schemes => TreeKind::Folder,
			Self::Source(_) => TreeKind::Source,
			Self::Part(_) => TreeKind::Part,
			Self::Probe(_) => TreeKind::Probe,
			Self::Scheme(_) => TreeKind::Scheme,
		}
	}
}

/// The rows of `project` in tree order, with their nodes.
pub(crate) fn rows(project: &Project, selected: Node) -> (Vec<Node>, Vec<TreeRow>) {
	let mut nodes = Vec::new();
	let mut rows = Vec::new();
	let mut push = |node: Node, depth: i32, label: &str, enabled: Option<bool>| {
		nodes.push(node);
		rows.push(TreeRow {
			kind: node.kind(),
			depth,
			label: label.into(),
			checkable: enabled.is_some(),
			checked: enabled.unwrap_or(true),
			selected: node == selected,
			icon: crate::icons::tree(project, node),
			add: match node {
				Node::Sources => "source",
				Node::Parts => "part",
				Node::Probes => "probe",
				Node::Schemes => "scheme",
				_ => "",
			}
			.into(),
		});
	};
	push(Node::Project, 0, "Project", None);
	push(Node::Scene, 1, "Scene", None);
	push(Node::Simulation, 1, "Simulation", None);
	push(Node::Display, 1, "Display", None);
	push(Node::Sources, 1, "Sources", None);
	for (i, s) in project.sources.iter().enumerate() {
		push(Node::Source(i), 2, &s.name, Some(s.enabled));
	}
	push(Node::Parts, 1, "Parts", None);
	for (i, p) in project.parts.iter().enumerate() {
		push(Node::Part(i), 2, &p.base.name, Some(p.base.enabled));
	}
	push(Node::Probes, 1, "Probes", None);
	for (i, p) in project.probes.iter().enumerate() {
		push(Node::Probe(i), 2, &p.name, Some(p.active));
	}
	push(Node::Schemes, 1, "Colour schemes", None);
	for (i, s) in project.color_schemes.iter().enumerate() {
		push(Node::Scheme(i), 2, &s.name, None);
	}
	(nodes, rows)
}

/// Flips the enabled (or active) flag of an item row.
pub(crate) fn toggle(project: &mut Project, node: Node) {
	match node {
		Node::Source(i) => project.sources[i].enabled ^= true,
		Node::Part(i) => project.parts[i].base.enabled ^= true,
		Node::Probe(i) => project.probes[i].active ^= true,
		_ => {}
	}
}

/// Removes an item row's item; the row the selection lands on: the one that took its place, else the one above,
/// else the folder.
pub(crate) fn remove(project: &mut Project, node: Node) -> Node {
	fn go<T>(v: &mut Vec<T>, i: usize, item: fn(usize) -> Node, folder: Node) -> Node {
		if i >= v.len() {
			return item(i);
		}
		v.remove(i);
		if i < v.len() { item(i) } else { i.checked_sub(1).map_or(folder, item) }
	}
	match node {
		Node::Source(i) => go(&mut project.sources, i, Node::Source, Node::Sources),
		Node::Part(i) => go(&mut project.parts, i, Node::Part, Node::Parts),
		Node::Probe(i) => go(&mut project.probes, i, Node::Probe, Node::Probes),
		Node::Scheme(i) => go(&mut project.color_schemes, i, Node::Scheme, Node::Schemes),
		_ => node,
	}
}

/// Whether an item row's item can move by `by` places within its list.
pub(crate) fn can_shift(project: &Project, node: Node, by: isize) -> bool {
	let (i, len) = match node {
		Node::Source(i) => (i, project.sources.len()),
		Node::Part(i) => (i, project.parts.len()),
		Node::Probe(i) => (i, project.probes.len()),
		Node::Scheme(i) => (i, project.color_schemes.len()),
		_ => return false,
	};
	i < len && i.checked_add_signed(by).is_some_and(|j| j < len)
}

/// Appends the item an add command names at (`x`, `y`): `source`, `scheme`, `probe:<detector>` or `part:<kind>`
/// (Next's add menus); its row.
pub(crate) fn add(project: &mut Project, what: &str, (x, y): (i32, i32)) -> Option<Node> {
	let last = |len: usize| len - 1;
	match what.split_once(':').unwrap_or((what, "")) {
		("source", _) => {
			project.add_source_at(x, y);
			Some(Node::Source(last(project.sources.len())))
		}
		("scheme", _) => {
			project.add_new_color_scheme();
			Some(Node::Scheme(last(project.color_schemes.len())))
		}
		("probe", detector) => {
			let detector = match detector {
				"counting" => ProbeDetector::Counting,
				"imaging" => ProbeDetector::Imaging,
				_ => ProbeDetector::Standard,
			};
			project.add_detector_at(x, y, detector);
			Some(Node::Probe(last(project.probes.len())))
		}
		("part", kind) => {
			project.add_part_at(new_part(kind)?, x, y);
			Some(Node::Part(last(project.parts.len())))
		}
		_ => None,
	}
}

/// A new part of the kind an add menu entry names, as Next prepares it.
fn new_part(kind: &str) -> Option<PartKind> {
	let gate = |kind: GateKind| {
		let mut gate = LogicGate::default();
		gate.kind = kind;
		PartKind::LogicGate(gate)
	};
	Some(match kind {
		"lens" => PartKind::Lens(Lens::default()),
		"fresnel-lens" => PartKind::FresnelLens(FresnelLens::new_plate()),
		"fiber" => PartKind::Fiber(Fiber::default()),
		"bragg-reflector" => PartKind::BraggReflector(BraggReflector::default()),
		"grin-lens" => PartKind::GrinLens(GrinLens::default()),
		"zone-plate" => PartKind::ZonePlate(ZonePlate::default()),
		"diffuser" => PartKind::Diffuser(Diffuser::new_part()),
		"interference-filter" => PartKind::InterferenceFilter(InterferenceFilter::new_part()),
		"simple-grating" => PartKind::SimpleGrating(SimpleGrating::new_part()),
		"facetted-grating" => PartKind::FacettedGrating(FacettedGrating::default()),
		"meta-lens" => PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Lens(MetaLensDesign::default()))),
		"meta-prism" => PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Prism(MetaPrismDesign::default()))),
		"meta-axicon" => PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Axicon(MetaAxiconDesign::default()))),
		"polygon" => PartKind::Polygon(Polygon::default()),
		"rectangle" => PartKind::Rectangle(Rectangle::default()),
		"ellipse" => PartKind::Ellipse(Ellipse::default()),
		"triangle" => PartKind::Triangle(Triangle::default()),
		"outline" => PartKind::Outline(Outline::default()),
		"xor-gate" => gate(GateKind::Xor),
		"and-gate" => gate(GateKind::And),
		"or-gate" => gate(GateKind::Or),
		_ => return None,
	})
}

/// Inserts a copy of an item row's item after it under a copy name; its row.
pub(crate) fn duplicate(project: &mut Project, node: Node) -> Option<Node> {
	match node {
		Node::Source(i) => project.duplicate_source(project.sources.get(i)?.id()).map(|_| Node::Source(i + 1)),
		Node::Part(i) => project.duplicate_part(project.parts.get(i)?.id()).map(|_| Node::Part(i + 1)),
		Node::Probe(i) => project.duplicate_probe(project.probes.get(i)?.id()).map(|_| Node::Probe(i + 1)),
		Node::Scheme(i) => {
			project.duplicate_color_scheme(project.color_schemes.get(i)?.id()).map(|_| Node::Scheme(i + 1))
		}
		_ => None,
	}
}

/// Moves an item row's item by `by` places within its list, when it can; the node it lands on.
pub(crate) fn shift(project: &mut Project, node: Node, by: isize) -> Node {
	fn go<T>(v: &mut [T], i: usize, by: isize) -> Option<usize> {
		let j = i.checked_add_signed(by).filter(|&j| j < v.len() && i < v.len())?;
		v.swap(i, j);
		Some(j)
	}
	match node {
		Node::Source(i) => go(&mut project.sources, i, by).map_or(node, Node::Source),
		Node::Part(i) => go(&mut project.parts, i, by).map_or(node, Node::Part),
		Node::Probe(i) => go(&mut project.probes, i, by).map_or(node, Node::Probe),
		Node::Scheme(i) => go(&mut project.color_schemes, i, by).map_or(node, Node::Scheme),
		_ => node,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_add_entry_adds_its_kind_and_selects_it() {
		let mut p = Project::default();
		for kind in [
			"lens",
			"fresnel-lens",
			"fiber",
			"bragg-reflector",
			"grin-lens",
			"zone-plate",
			"diffuser",
			"interference-filter",
			"simple-grating",
			"facetted-grating",
			"meta-lens",
			"meta-prism",
			"meta-axicon",
			"polygon",
			"rectangle",
			"ellipse",
			"triangle",
			"outline",
			"xor-gate",
			"and-gate",
			"or-gate",
		] {
			let node = add(&mut p, &format!("part:{kind}"), (10, 20));
			assert_eq!(node, Some(Node::Part(p.parts.len() - 1)), "{kind}");
		}
		assert_eq!(p.parts[1].base.name, "Fresnel lens 1");
		assert_eq!((p.parts[0].base.x, p.parts[0].base.y), (10, 20));
		assert_eq!(add(&mut p, "part:nothing", (0, 0)), None);
		assert_eq!(add(&mut p, "probe:imaging", (0, 0)), Some(Node::Probe(0)));
		assert_eq!(p.probes[0].name, "Detector 1");
	}

	#[test]
	fn a_removal_lands_on_the_slot_then_above_then_the_folder() {
		let mut p = Project::default();
		for _ in 0..3 {
			p.add_source();
		}
		assert_eq!(remove(&mut p, Node::Source(1)), Node::Source(1));
		assert_eq!(remove(&mut p, Node::Source(1)), Node::Source(0));
		assert_eq!(remove(&mut p, Node::Source(0)), Node::Sources);
		assert!(!can_shift(&p, Node::Source(0), 1));
	}
}
