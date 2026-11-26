//! The project tree's rows and selection (docs/ui/20, Tree structure, Selection model).

use waveduderust::model::{
	BraggReflector, Diffuser, Ellipse, FacettedGrating, Fiber, FresnelLens, GateKind, GrinLens, InterferenceFilter,
	Lens, LogicGate, MetaAxiconDesign, MetaDesign, MetaLensDesign, MetaPrismDesign, MetaSurface, Outline, PartKind,
	Polygon, ProbeDetector, Project, Rectangle, SimpleGrating, Triangle, ZonePlate,
};

use crate::selection::Selection;
use crate::ui::{DropPlace, FolderAdd, NewItem, TreeKey, TreeKind, TreeRow};

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
	pub(crate) fn kind(self) -> TreeKind {
		match self {
			Self::Project => TreeKind::Project,
			Self::Scene => TreeKind::Scene,
			Self::Simulation => TreeKind::Simulation,
			Self::Display => TreeKind::Display,
			Self::Sources => TreeKind::Sources,
			Self::Parts => TreeKind::Parts,
			Self::Probes => TreeKind::Probes,
			Self::Schemes => TreeKind::Schemes,
			Self::Source(_) => TreeKind::Source,
			Self::Part(_) => TreeKind::Part,
			Self::Probe(_) => TreeKind::Probe,
			Self::Scheme(_) => TreeKind::Scheme,
		}
	}
}

/// The rows of `project` in tree order, with their nodes; the children of `collapsed` rows are left out.
pub(crate) fn rows(project: &Project, selection: &Selection, collapsed: &[Node]) -> (Vec<Node>, Vec<TreeRow>) {
	let mut nodes = Vec::new();
	let mut rows = Vec::new();
	let shut = |node: Node| collapsed.contains(&node) || (node != Node::Project && collapsed.contains(&Node::Project));
	let mut push = |node: Node, depth: i32, label: &str, enabled: Option<bool>| {
		if parent(node).is_some_and(shut) {
			return;
		}
		let children = children(project, node);
		nodes.push(node);
		rows.push(TreeRow {
			kind: node.kind(),
			depth,
			label: label.into(),
			checkable: enabled.is_some(),
			checked: enabled.unwrap_or(true),
			selected: selection.contains(node),
			icon: crate::icons::tree(project, node),
			expandable: children > 0,
			expanded: !collapsed.contains(&node),
			strip: match node {
				Node::Scheme(i) => crate::icons::strip(i, &project.color_schemes[i]),
				_ => slint::Image::default(),
			},
			add: match node {
				Node::Sources => FolderAdd::Source,
				Node::Parts => FolderAdd::Part,
				Node::Probes => FolderAdd::Probe,
				Node::Schemes => FolderAdd::Scheme,
				_ => FolderAdd::None,
			},
		});
	};
	push(Node::Project, 0, "", None);
	push(Node::Scene, 1, "", None);
	push(Node::Simulation, 1, "", None);
	push(Node::Display, 1, "", None);
	push(Node::Sources, 1, "", None);
	for (i, s) in project.sources.iter().enumerate() {
		push(Node::Source(i), 2, &s.name, Some(s.enabled));
	}
	push(Node::Parts, 1, "", None);
	for (i, p) in project.parts.iter().enumerate() {
		push(Node::Part(i), 2, &p.base.name, Some(p.base.enabled));
	}
	push(Node::Probes, 1, "", None);
	for (i, p) in project.probes.iter().enumerate() {
		push(Node::Probe(i), 2, &p.name, Some(p.active));
	}
	push(Node::Schemes, 1, "", None);
	for (i, s) in project.color_schemes.iter().enumerate() {
		push(Node::Scheme(i), 2, &s.name, None);
	}
	(nodes, rows)
}

/// Flips the enabled (or active) flag of an item row.
pub(crate) fn toggle(project: &mut Project, node: Node) {
	match node {
		Node::Source(i) => {
			if let Some(s) = project.sources.get_mut(i) {
				s.enabled ^= true;
			}
		}
		Node::Part(i) => {
			if let Some(p) = project.parts.get_mut(i) {
				p.base.enabled ^= true;
			}
		}
		Node::Probe(i) => {
			if let Some(p) = project.probes.get_mut(i) {
				p.active ^= true;
			}
		}
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

/// Removes the items of `nodes`; the row the user is kept at (Next's `ProjectItems.Remove`): the slot of the last
/// one in its list, else that list's folder.
pub(crate) fn remove_all(project: &mut Project, nodes: &[Node]) -> Option<Node> {
	let last = *nodes.last()?;
	let mut sorted = nodes.to_vec();
	// from the end, so the indices left stay valid
	sorted.sort_by_key(|&n| std::cmp::Reverse(index(n)));
	for node in sorted {
		remove(project, node);
	}
	let (i, len, item, folder): (usize, usize, fn(usize) -> Node, Node) = match last {
		Node::Source(i) => (i, project.sources.len(), Node::Source, Node::Sources),
		Node::Part(i) => (i, project.parts.len(), Node::Part, Node::Parts),
		Node::Probe(i) => (i, project.probes.len(), Node::Probe, Node::Probes),
		Node::Scheme(i) => (i, project.color_schemes.len(), Node::Scheme, Node::Schemes),
		_ => return None,
	};
	Some(if len > 0 { item(i.min(len - 1)) } else { folder })
}

/// How many rows `node` holds.
fn children(project: &Project, node: Node) -> usize {
	match node {
		Node::Project => 1,
		Node::Sources => project.sources.len(),
		Node::Parts => project.parts.len(),
		Node::Probes => project.probes.len(),
		Node::Schemes => project.color_schemes.len(),
		_ => 0,
	}
}

/// What a tree key does (WPF's TreeView keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
	/// Moves the current row there.
	To(Node),
	/// Expands (`true`) or collapses the row.
	Expand(Node, bool),
}

/// What `key` does with `current` among the shown `nodes`.
pub(crate) fn navigate(
	project: &Project,
	nodes: &[Node],
	collapsed: &[Node],
	current: Node,
	key: TreeKey,
) -> Option<Step> {
	let i = nodes.iter().position(|&n| n == current).unwrap_or(0);
	let (holds, shut) = (children(project, current) > 0, collapsed.contains(&current));
	let to = match key {
		TreeKey::Up => i.checked_sub(1),
		TreeKey::Down => Some(i + 1),
		TreeKey::Home => Some(0),
		TreeKey::End => nodes.len().checked_sub(1),
		TreeKey::Left if holds && !shut => return Some(Step::Expand(current, false)),
		TreeKey::Left => return parent(current).map(Step::To),
		TreeKey::Right if holds && shut => return Some(Step::Expand(current, true)),
		TreeKey::Right if holds => Some(i + 1),
		TreeKey::Right => None,
	};
	to.and_then(|k| nodes.get(k)).map(|&n| Step::To(n))
}

/// Where dropping the item `dragged` on `target`'s row (its upper half when `before`) moves it (Next's
/// `FindDropTarget`): onto a sibling before or after it, onto its own folder at the end. The place and the item's index
/// after the move; `None` where it would not move.
pub(crate) fn drop_target(project: &Project, dragged: Node, target: Node, before: bool) -> Option<(DropPlace, usize)> {
	let folder = parent(dragged).filter(|_| is_item(dragged))?;
	let (current, last) = (index(dragged), children(project, folder).checked_sub(1)?);
	if target == folder {
		return (current != last).then_some((DropPlace::Into, last));
	}
	if target == dragged || !is_item(target) || parent(target) != Some(folder) {
		return None;
	}
	let insert = index(target) + usize::from(!before);
	let to = insert - usize::from(current < insert);
	(to != current).then_some((if before { DropPlace::Before } else { DropPlace::After }, to))
}

/// Moves the item `node` to index `to` of its list; the node it ends at.
pub(crate) fn move_to(project: &mut Project, node: Node, to: usize) -> Node {
	fn go<T>(v: &mut Vec<T>, i: usize, to: usize) -> bool {
		let ok = i < v.len() && to < v.len();
		if ok {
			let item = v.remove(i);
			v.insert(to, item);
		}
		ok
	}
	match node {
		Node::Source(i) if go(&mut project.sources, i, to) => Node::Source(to),
		Node::Part(i) if go(&mut project.parts, i, to) => Node::Part(to),
		Node::Probe(i) if go(&mut project.probes, i, to) => Node::Probe(to),
		Node::Scheme(i) if go(&mut project.color_schemes, i, to) => Node::Scheme(to),
		_ => node,
	}
}

/// The row `node` sits under.
pub(crate) fn parent(node: Node) -> Option<Node> {
	match node {
		Node::Project => None,
		Node::Source(_) => Some(Node::Sources),
		Node::Part(_) => Some(Node::Parts),
		Node::Probe(_) => Some(Node::Probes),
		Node::Scheme(_) => Some(Node::Schemes),
		_ => Some(Node::Project),
	}
}

/// Whether `node` is `ancestor` or sits under it.
pub(crate) fn within(node: Node, ancestor: Node) -> bool {
	std::iter::successors(Some(node), |&n| parent(n)).any(|n| n == ancestor)
}

/// An item row's name; empty for the other rows.
pub(crate) fn name(project: &Project, node: Node) -> &str {
	match node {
		Node::Source(i) => project.sources.get(i).map(|s| s.name.as_str()),
		Node::Part(i) => project.parts.get(i).map(|p| p.base.name.as_str()),
		Node::Probe(i) => project.probes.get(i).map(|p| p.name.as_str()),
		Node::Scheme(i) => project.color_schemes.get(i).map(|s| s.name.as_str()),
		_ => None,
	}
	.unwrap_or_default()
}

/// A folder row's item count.
pub(crate) fn folder(project: &Project, node: Node) -> Option<usize> {
	Some(match node {
		Node::Sources => project.sources.len(),
		Node::Parts => project.parts.len(),
		Node::Probes => project.probes.len(),
		Node::Schemes => project.color_schemes.len(),
		_ => return None,
	})
}

/// Whether `node` is a list item's row.
pub(crate) fn is_item(node: Node) -> bool {
	matches!(node, Node::Source(_) | Node::Part(_) | Node::Probe(_) | Node::Scheme(_))
}

/// An item row's list, for ordering.
fn list(node: Node) -> u8 {
	match node {
		Node::Source(_) => 0,
		Node::Part(_) => 1,
		Node::Probe(_) => 2,
		_ => 3,
	}
}

/// An item row's index in its list.
fn index(node: Node) -> usize {
	match node {
		Node::Source(i) | Node::Part(i) | Node::Probe(i) | Node::Scheme(i) => i,
		_ => 0,
	}
}

/// A copy after each item of `nodes` (Next's `ProjectItems.Duplicate`); the copies' rows in the order of `nodes`.
pub(crate) fn duplicate_all(project: &mut Project, nodes: &[Node]) -> Vec<Node> {
	let mut sorted = nodes.to_vec();
	sorted.sort_by_key(|&n| (list(n), index(n)));
	sorted.dedup();
	// from the end, then each copy is shifted by the copies inserted before it in its list
	let mut copies: Vec<Option<Node>> = sorted.iter().rev().map(|&n| duplicate(project, n)).collect();
	copies.reverse();
	let mut before = [0; 4];
	for copy in copies.iter_mut().flatten() {
		let list = usize::from(list(*copy));
		*copy = shift_index(*copy, before[list]);
		before[list] += 1;
	}
	let mut seen = Vec::new();
	nodes
		.iter()
		.filter_map(|n| {
			let k = sorted.iter().position(|s| s == n)?;
			if seen.contains(&k) {
				return None;
			}
			seen.push(k);
			copies[k]
		})
		.collect()
}

fn shift_index(node: Node, by: usize) -> Node {
	match node {
		Node::Source(i) => Node::Source(i + by),
		Node::Part(i) => Node::Part(i + by),
		Node::Probe(i) => Node::Probe(i + by),
		Node::Scheme(i) => Node::Scheme(i + by),
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

/// Appends `item` at (`x`, `y`) as Next's add menus prepare it; its row.
pub(crate) fn add(project: &mut Project, item: NewItem, (x, y): (i32, i32)) -> Node {
	let last = |len: usize| len - 1;
	let detector = |project: &mut Project, detector| {
		project.add_detector_at(x, y, detector);
		Node::Probe(last(project.probes.len()))
	};
	let gate = |kind: GateKind| {
		let mut gate = LogicGate::default();
		gate.kind = kind;
		PartKind::LogicGate(gate)
	};
	let meta = |design| PartKind::MetaSurface(MetaSurface::new_part(design));
	let part = match item {
		NewItem::Source => {
			project.add_source_at(x, y);
			return Node::Source(last(project.sources.len()));
		}
		NewItem::Scheme => {
			project.add_new_color_scheme();
			return Node::Scheme(last(project.color_schemes.len()));
		}
		NewItem::StandardProbe => return detector(project, ProbeDetector::Standard),
		NewItem::CountingDetector => return detector(project, ProbeDetector::Counting),
		NewItem::ImagingDetector => return detector(project, ProbeDetector::Imaging),
		NewItem::Lens => PartKind::Lens(Lens::default()),
		NewItem::FresnelLens => PartKind::FresnelLens(FresnelLens::new_plate()),
		NewItem::Fiber => PartKind::Fiber(Fiber::default()),
		NewItem::BraggReflector => PartKind::BraggReflector(BraggReflector::default()),
		NewItem::GrinLens => PartKind::GrinLens(GrinLens::default()),
		NewItem::ZonePlate => PartKind::ZonePlate(ZonePlate::default()),
		NewItem::Diffuser => PartKind::Diffuser(Diffuser::new_part()),
		NewItem::InterferenceFilter => PartKind::InterferenceFilter(InterferenceFilter::new_part()),
		NewItem::SimpleGrating => PartKind::SimpleGrating(SimpleGrating::new_part()),
		NewItem::FacettedGrating => PartKind::FacettedGrating(FacettedGrating::default()),
		NewItem::MetaLens => meta(MetaDesign::Lens(MetaLensDesign::default())),
		NewItem::MetaPrism => meta(MetaDesign::Prism(MetaPrismDesign::default())),
		NewItem::MetaAxicon => meta(MetaDesign::Axicon(MetaAxiconDesign::default())),
		NewItem::Polygon => PartKind::Polygon(Polygon::default()),
		NewItem::Rectangle => PartKind::Rectangle(Rectangle::default()),
		NewItem::Ellipse => PartKind::Ellipse(Ellipse::default()),
		NewItem::Triangle => PartKind::Triangle(Triangle::default()),
		NewItem::Outline => PartKind::Outline(Outline::default()),
		NewItem::XorGate => gate(GateKind::Xor),
		NewItem::AndGate => gate(GateKind::And),
		NewItem::OrGate => gate(GateKind::Or),
	};
	project.add_part_at(part, x, y);
	Node::Part(last(project.parts.len()))
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

	#[test]
	fn a_drop_moves_within_the_list_by_the_half_or_to_the_folder_end() {
		let mut p = Project::default();
		for _ in 0..3 {
			p.add_source();
		}
		p.add_probe();
		let s = |i| Node::Source(i);
		assert_eq!(drop_target(&p, s(0), s(2), false), Some((DropPlace::After, 2)));
		assert_eq!(drop_target(&p, s(0), s(2), true), Some((DropPlace::Before, 1)));
		assert_eq!(drop_target(&p, s(2), s(0), true), Some((DropPlace::Before, 0)));
		// no-op positions and other lists are no target
		assert_eq!(drop_target(&p, s(0), s(1), true), None);
		assert_eq!(drop_target(&p, s(0), s(0), false), None);
		assert_eq!(drop_target(&p, s(0), Node::Probe(0), true), None);
		assert_eq!(drop_target(&p, s(0), Node::Sources, false), Some((DropPlace::Into, 2)));
		assert_eq!(drop_target(&p, s(2), Node::Sources, false), None);
		assert_eq!(drop_target(&p, Node::Scene, Node::Project, false), None);
		let name = p.sources[0].name.clone();
		assert_eq!(move_to(&mut p, s(0), 2), s(2));
		assert_eq!(p.sources[2].name, name);
	}

	#[test]
	fn keys_walk_the_shown_rows_and_fold_folders() {
		let mut p = Project::default();
		p.add_source();
		let (nodes, _) = rows(&p, &Selection::default(), &[]);
		let go = |current, key, collapsed: &[Node]| navigate(&p, &nodes, collapsed, current, key);
		assert_eq!(go(Node::Project, TreeKey::Down, &[]), Some(Step::To(nodes[1])));
		assert_eq!(go(Node::Project, TreeKey::Up, &[]), None);
		assert_eq!(go(Node::Project, TreeKey::End, &[]), nodes.last().map(|&n| Step::To(n)));
		assert_eq!(go(Node::Sources, TreeKey::Left, &[]), Some(Step::Expand(Node::Sources, false)));
		assert_eq!(go(Node::Sources, TreeKey::Left, &[Node::Sources]), Some(Step::To(Node::Project)));
		assert_eq!(go(Node::Sources, TreeKey::Right, &[Node::Sources]), Some(Step::Expand(Node::Sources, true)));
		assert_eq!(go(Node::Source(0), TreeKey::Left, &[]), Some(Step::To(Node::Sources)));
		assert_eq!(go(Node::Source(0), TreeKey::Right, &[]), None);
	}

	use super::*;

	#[test]
	fn a_group_duplicates_after_each_and_removes_to_the_last_slot() {
		let mut p = Project::default();
		for _ in 0..3 {
			p.add_source();
		}
		let copies = duplicate_all(&mut p, &[Node::Source(2), Node::Source(0)]);
		assert_eq!(copies, [Node::Source(4), Node::Source(1)]);
		assert_eq!(p.sources[1].name, format!("{} copy", p.sources[0].name));
		assert_eq!(p.sources[4].name, format!("{} copy", p.sources[3].name));
		assert_eq!(remove_all(&mut p, &[Node::Source(0), Node::Source(4)]), Some(Node::Source(2)));
		assert_eq!(p.sources.len(), 3);
		assert_eq!(remove_all(&mut p, &[Node::Source(0), Node::Source(1), Node::Source(2)]), Some(Node::Sources));
	}

	#[test]
	fn every_add_entry_adds_its_kind_and_selects_it() {
		use NewItem as N;
		let mut p = Project::default();
		let parts = [
			N::Lens,
			N::FresnelLens,
			N::Fiber,
			N::BraggReflector,
			N::GrinLens,
			N::ZonePlate,
			N::Diffuser,
			N::InterferenceFilter,
			N::SimpleGrating,
			N::FacettedGrating,
			N::MetaLens,
			N::MetaPrism,
			N::MetaAxicon,
			N::Polygon,
			N::Rectangle,
			N::Ellipse,
			N::Triangle,
			N::Outline,
			N::XorGate,
			N::AndGate,
			N::OrGate,
		];
		for kind in parts {
			assert_eq!(add(&mut p, kind, (10, 20)), Node::Part(p.parts.len() - 1), "{kind:?}");
		}
		assert_eq!(p.parts[1].base.name, "Fresnel lens 1");
		assert_eq!((p.parts[0].base.x, p.parts[0].base.y), (10, 20));
		assert_eq!(add(&mut p, N::ImagingDetector, (0, 0)), Node::Probe(0));
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
