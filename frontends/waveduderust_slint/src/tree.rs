//! The project tree's rows and selection (docs/ui/20, Tree structure, Selection model).

use waveduderust::model::Project;

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

/// Removes an item row's item.
pub(crate) fn remove(project: &mut Project, node: Node) {
	match node {
		Node::Source(i) => drop(project.sources.remove(i)),
		Node::Part(i) => drop(project.parts.remove(i)),
		Node::Probe(i) => drop(project.probes.remove(i)),
		Node::Scheme(i) => drop(project.color_schemes.remove(i)),
		_ => {}
	}
}

/// Moves an item row's item by `by` places within its list, when it can; the node it lands on.
pub(crate) fn shift(project: &mut Project, node: Node, by: isize) -> Node {
	fn go<T>(v: &mut [T], i: usize, by: isize) -> Option<usize> {
		let j = i.checked_add_signed(by).filter(|&j| j < v.len())?;
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
