//! The tree's selection (docs/ui/20, Selection model; Next's `Selection`): the current row, whose form Properties
//! shows, and a group of sources, parts and probes that commands act on together.

use waveduderust::model::Project;

use crate::tree::Node;

/// How a row click selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
	/// The row alone.
	Alone,
	/// Into the group or out of it (Ctrl+click, Insert, Ctrl+Space).
	Toggle,
	/// The rows from the current one to it, in tree order (Shift+click).
	Range,
	/// Current, the group kept when it is in it (a right click).
	Focus,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Selection {
	current: Node,
	/// In the order they joined; the current row need not be one of them.
	items: Vec<Node>,
	/// Where a Shift+click range starts: the row of the last plain click or Ctrl+click that added.
	anchor: Node,
}

/// Whether `node` can join a group: a source, a part or a probe.
pub(crate) fn groupable(node: Node) -> bool {
	matches!(node, Node::Source(_) | Node::Part(_) | Node::Probe(_))
}

/// Every source, part and probe of `project` in tree order.
fn all(project: &Project) -> Vec<Node> {
	let sources = (0..project.sources.len()).map(Node::Source);
	let parts = (0..project.parts.len()).map(Node::Part);
	sources.chain(parts).chain((0..project.probes.len()).map(Node::Probe)).collect()
}

impl Selection {
	pub(crate) fn current(&self) -> Node {
		self.current
	}

	/// Selects `node` alone.
	pub(crate) fn set(&mut self, node: Node) {
		self.current = node;
		self.anchor = node;
		self.items.clear();
	}

	/// Makes `items` the group, the last one current; the root when there is none.
	pub(crate) fn set_group(&mut self, items: Vec<Node>) {
		self.current = items.last().copied().unwrap_or_default();
		self.anchor = self.current;
		self.items = if items.len() > 1 { items } else { Vec::new() };
	}

	/// Whether `node`'s row shows as selected.
	pub(crate) fn contains(&self, node: Node) -> bool {
		node == self.current || self.items.contains(&node)
	}

	/// Selects `node` as a click with `pick` does.
	pub(crate) fn pick(&mut self, project: &Project, node: Node, pick: Pick) {
		match pick {
			Pick::Toggle if groupable(node) => self.toggle(node),
			Pick::Range if groupable(node) && groupable(self.anchor) => {
				let all = all(project);
				let (Some(a), Some(b)) =
					(all.iter().position(|&n| n == self.anchor), all.iter().position(|&n| n == node))
				else {
					return self.set(node);
				};
				self.items = all[a.min(b)..=a.max(b)].to_vec();
				self.current = node;
			}
			Pick::Focus if self.items.contains(&node) => self.current = node,
			_ => self.set(node),
		}
	}

	/// Takes `node` into the group and makes it current, or out of it, the current row then moving to the last one
	/// left; out of a group of one, nothing stays selected but the root.
	pub(crate) fn toggle(&mut self, node: Node) {
		if !groupable(node) {
			return;
		}
		// a lone selected row is a group of one
		if self.items.is_empty() && groupable(self.current) {
			self.items.push(self.current);
		}
		if let Some(i) = self.items.iter().position(|&n| n == node) {
			self.items.remove(i);
			if self.current == node {
				self.current = self.items.last().copied().unwrap_or_default();
			}
		} else {
			self.items.push(node);
			self.current = node;
			self.anchor = node;
		}
	}

	/// Every source, part and probe, the last one current.
	pub(crate) fn select_all(&mut self, project: &Project) {
		self.set_group(all(project));
	}

	/// What a command acts on: the whole group when the current row is one of several selected (Explorer's rule),
	/// else the current row alone.
	pub(crate) fn group(&self) -> Vec<Node> {
		self.group_of(self.current)
	}

	/// What a command on `target`'s row acts on: the whole group when `target` is one of several selected, else
	/// `target` alone.
	pub(crate) fn group_of(&self, target: Node) -> Vec<Node> {
		if self.items.len() > 1 && self.items.contains(&target) { self.items.clone() } else { vec![target] }
	}

	/// Drops the rows `project` no longer has, after an undo or another edit.
	pub(crate) fn retain(&mut self, project: &Project) {
		let all = all(project);
		self.items.retain(|n| all.contains(n));
		if groupable(self.current) && !all.contains(&self.current) {
			self.set(Node::Project);
		}
	}

	/// Keeps the current row alone: an undo or redo may have moved the group's rows to other objects.
	pub(crate) fn collapse(&mut self) {
		let current = self.current;
		self.set(current);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn project() -> Project {
		let mut p = Project::default();
		for _ in 0..3 {
			p.add_source();
		}
		p.add_probe();
		p
	}

	#[test]
	fn ctrl_click_toggles_and_the_current_row_follows() {
		let p = project();
		let mut s = Selection::default();
		s.pick(&p, Node::Source(0), Pick::Alone);
		s.pick(&p, Node::Source(2), Pick::Toggle);
		assert_eq!(s.group(), [Node::Source(0), Node::Source(2)]);
		s.pick(&p, Node::Source(2), Pick::Toggle);
		assert_eq!((s.current(), s.group()), (Node::Source(0), vec![Node::Source(0)]));
		// the last one out: nothing but the root
		s.pick(&p, Node::Source(0), Pick::Toggle);
		assert_eq!(s.current(), Node::Project);
		// not groupable: alone
		s.pick(&p, Node::Scene, Pick::Toggle);
		assert_eq!(s.group(), [Node::Scene]);
	}

	#[test]
	fn shift_click_takes_the_range_in_tree_order_across_lists() {
		let p = project();
		let mut s = Selection::default();
		s.pick(&p, Node::Source(1), Pick::Alone);
		s.pick(&p, Node::Probe(0), Pick::Range);
		assert_eq!(s.group(), [Node::Source(1), Node::Source(2), Node::Probe(0)]);
		assert_eq!(s.current(), Node::Probe(0));
		// the anchor stays where the range started
		s.pick(&p, Node::Source(2), Pick::Range);
		assert_eq!(s.group(), [Node::Source(1), Node::Source(2)]);
		s.select_all(&p);
		assert_eq!(s.group().len(), 4);
		// a right click in the group moves only the current row
		s.pick(&p, Node::Source(0), Pick::Focus);
		assert_eq!((s.current(), s.group().len()), (Node::Source(0), 4));
	}
}
