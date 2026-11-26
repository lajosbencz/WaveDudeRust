//! The layout tree. Hidden panes keep their tab, so a group whose panes are all hidden stays in the tree, takes
//! no room and brings its pane back where it was.

use crate::{Axis, GroupId, SplitId, Tab};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
	Centre,
	Group(Group),
	Split(Split),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Group {
	pub(crate) id: GroupId,
	pub(crate) tabs: Vec<Tab>,
	/// Index into `tabs`, of a shown one while any is.
	pub(crate) active: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Split {
	pub(crate) id: SplitId,
	pub(crate) axis: Axis,
	pub(crate) children: Vec<Child>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Child {
	pub(crate) weight: f32,
	pub(crate) node: Node,
}

impl Group {
	pub(crate) fn is_visible(&self) -> bool {
		self.tabs.iter().any(|t| t.shown)
	}

	pub(crate) fn position(&self, pane: &str) -> Option<usize> {
		self.tabs.iter().position(|t| t.id == pane)
	}

	/// Moves `active` to the nearest shown tab, the following ones first.
	pub(crate) fn fix_active(&mut self) {
		let start = self.active.min(self.tabs.len().saturating_sub(1));
		self.active = (start..self.tabs.len()).chain((0..start).rev()).find(|&i| self.tabs[i].shown).unwrap_or(start);
	}
}

impl Node {
	pub(crate) fn is_visible(&self) -> bool {
		match self {
			Self::Centre => true,
			Self::Group(g) => g.is_visible(),
			Self::Split(s) => s.children.iter().any(|c| c.node.is_visible()),
		}
	}

	pub(crate) fn group(&self, pred: &impl Fn(&Group) -> bool) -> Option<&Group> {
		match self {
			Self::Centre => None,
			Self::Group(g) => pred(g).then_some(g),
			Self::Split(s) => s.children.iter().find_map(|c| c.node.group(pred)),
		}
	}

	pub(crate) fn group_mut(&mut self, pred: &impl Fn(&Group) -> bool) -> Option<&mut Group> {
		match self {
			Self::Centre => None,
			Self::Group(g) => pred(g).then_some(g),
			Self::Split(s) => s.children.iter_mut().find_map(|c| c.node.group_mut(pred)),
		}
	}

	pub(crate) fn split_mut(&mut self, id: SplitId) -> Option<&mut Split> {
		match self {
			Self::Split(s) => {
				if s.id == id {
					Some(s)
				} else {
					s.children.iter_mut().find_map(|c| c.node.split_mut(id))
				}
			}
			Self::Centre | Self::Group(_) => None,
		}
	}

	/// The node of group `id`, the centre for [`GroupId::CENTRE`].
	pub(crate) fn node_mut(&mut self, id: GroupId) -> Option<&mut Self> {
		let hit = match self {
			Self::Centre => id == GroupId::CENTRE,
			Self::Group(g) => g.id == id,
			Self::Split(_) => false,
		};
		if hit {
			return Some(self);
		}
		match self {
			Self::Split(s) => s.children.iter_mut().find_map(|c| c.node.node_mut(id)),
			_ => None,
		}
	}

	/// Removes group `id` from its split; the split may be left empty or with one child.
	pub(crate) fn take_group(&mut self, id: GroupId) -> Option<Self> {
		let Self::Split(s) = self else { return None };
		match s.children.iter().position(|c| matches!(&c.node, Self::Group(g) if g.id == id)) {
			Some(i) => Some(s.children.remove(i).node),
			None => s.children.iter_mut().find_map(|c| c.node.take_group(id)),
		}
	}

	/// Drops groups without tabs and empty splits, replaces one-child splits by their child and merges a split
	/// into its parent along the same axis, keeping the shown children's sizes.
	pub(crate) fn normalise(self) -> Option<Self> {
		match self {
			Self::Centre => Some(self),
			Self::Group(mut g) => {
				if g.tabs.is_empty() {
					return None;
				}
				g.fix_active();
				Some(Self::Group(g))
			}
			Self::Split(s) => {
				let mut children = Vec::with_capacity(s.children.len());
				for child in s.children {
					let weight = if child.weight.is_finite() && child.weight > 0. { child.weight } else { 1. };
					match child.node.normalise() {
						None => {}
						Some(Self::Split(inner)) if inner.axis == s.axis => {
							let visible: f32 =
								inner.children.iter().filter(|c| c.node.is_visible()).map(|c| c.weight).sum();
							let total =
								if visible > 0. { visible } else { inner.children.iter().map(|c| c.weight).sum() };
							children.extend(
								inner.children.into_iter().map(|c| Child { weight: c.weight * weight / total, ..c }),
							);
						}
						Some(node) => children.push(Child { weight, node }),
					}
				}
				match children.len() {
					0 => None,
					1 => children.pop().map(|c| c.node),
					_ => Some(Self::Split(Split { children, ..s })),
				}
			}
		}
	}
}
