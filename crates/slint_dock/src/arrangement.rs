//! The arrangement as data: what an app builds its first layout from, and what it saves and restores.

use std::collections::HashSet;
use std::fmt;

use crate::tree::{Child, Group, Node, Split};
use crate::{Axis, GroupId, MIN_WEIGHT, SplitId};

/// A pane in a tab group.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tab {
	/// The pane's id.
	pub id: String,
	/// Whether it is shown; a hidden one keeps its place for [`Dock::show`](crate::Dock::show).
	pub shown: bool,
}

/// A layout tree as data.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Arrangement {
	/// The fixed centre area; exactly one per arrangement.
	Centre,
	/// A tab group.
	Tabs {
		/// The panes in tab order.
		tabs: Vec<Tab>,
		/// Index of the active one into `tabs`.
		active: usize,
	},
	/// Children side by side along `axis`, each sized by its weight against its shown siblings'.
	Split {
		/// The direction of the children.
		axis: Axis,
		/// Weight and node of each child.
		children: Vec<(f32, Arrangement)>,
	},
}

impl Arrangement {
	/// A group of shown panes, the first active.
	pub fn tabs<I: IntoIterator<Item: Into<String>>>(ids: I) -> Self {
		let tabs = ids.into_iter().map(|id| Tab { id: id.into(), shown: true }).collect();
		Self::Tabs { tabs, active: 0 }
	}

	/// A split of weighted children.
	pub fn split(axis: Axis, children: impl IntoIterator<Item = (f32, Self)>) -> Self {
		Self::Split { axis, children: children.into_iter().collect() }
	}

	pub(crate) fn of(node: &Node) -> Self {
		match node {
			Node::Centre => Self::Centre,
			Node::Group(g) => Self::Tabs { tabs: g.tabs.clone(), active: g.active },
			Node::Split(s) => Self::Split {
				axis: s.axis,
				children: s.children.iter().map(|c| (c.weight, Self::of(&c.node))).collect(),
			},
		}
	}
}

/// Why [`Dock::arrange`](crate::Dock::arrange) refused an arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrangeError {
	/// It has no [`Arrangement::Centre`].
	NoCentre,
	/// It has more than one [`Arrangement::Centre`].
	ManyCentres,
}

impl fmt::Display for ArrangeError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::NoCentre => "the arrangement has no centre",
			Self::ManyCentres => "the arrangement has more than one centre",
		})
	}
}

impl std::error::Error for ArrangeError {}

/// Turns an arrangement into a tree with fresh ids, keeping the first tab of each `known` pane; a weight below
/// [`MIN_WEIGHT`] takes that, one not finite 1.
pub(crate) struct Builder<'a, F: Fn(&str) -> bool> {
	pub(crate) known: F,
	pub(crate) next_id: &'a mut u32,
	pub(crate) seen: HashSet<String>,
	pub(crate) centres: usize,
}

impl<F: Fn(&str) -> bool> Builder<'_, F> {
	pub(crate) fn build(&mut self, a: &Arrangement) -> Option<Node> {
		match a {
			Arrangement::Centre => {
				self.centres += 1;
				Some(Node::Centre)
			}
			Arrangement::Tabs { tabs: given, active } => {
				let tabs: Vec<Tab> =
					given.iter().filter(|t| (self.known)(&t.id) && self.seen.insert(t.id.clone())).cloned().collect();
				if tabs.is_empty() {
					return None;
				}
				let active = given.get(*active).and_then(|t| tabs.iter().position(|k| k.id == t.id)).unwrap_or(0);
				let mut group = Group { id: GroupId(self.fresh()), tabs, active };
				group.fix_active();
				Some(Node::Group(group))
			}
			Arrangement::Split { axis, children } => {
				let children = children
					.iter()
					.filter_map(|(weight, child)| {
						let weight = if weight.is_finite() { weight.max(MIN_WEIGHT) } else { 1. };
						Some(Child { weight, node: self.build(child)? })
					})
					.collect();
				Some(Node::Split(Split { id: SplitId(self.fresh()), axis: *axis, children }))
			}
		}
	}

	fn fresh(&mut self) -> u32 {
		*self.next_id += 1;
		*self.next_id
	}
}
