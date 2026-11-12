//! The named items of a project: sources, parts, probes and colour schemes, addressed by list and index.

use super::Project;
use super::names::Named;

/// The four lists of named items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemKind {
	Source,
	Part,
	Probe,
	Scheme,
}

impl ItemKind {
	pub const ALL: [Self; 4] = [Self::Source, Self::Part, Self::Probe, Self::Scheme];

	pub fn len(self, project: &Project) -> usize {
		match self {
			Self::Source => project.sources.len(),
			Self::Part => project.parts.len(),
			Self::Probe => project.probes.len(),
			Self::Scheme => project.color_schemes.len(),
		}
	}

	/// The names of the list, in order.
	pub fn names(self, project: &Project) -> Vec<&str> {
		match self {
			Self::Source => project.sources.iter().map(Named::name).collect(),
			Self::Part => project.parts.iter().map(Named::name).collect(),
			Self::Probe => project.probes.iter().map(Named::name).collect(),
			Self::Scheme => project.color_schemes.iter().map(Named::name).collect(),
		}
	}
}

/// A source, part, probe or colour scheme by its list and index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemRef {
	pub kind: ItemKind,
	pub index: usize,
}

impl ItemRef {
	pub const fn new(kind: ItemKind, index: usize) -> Self {
		Self { kind, index }
	}

	pub fn exists(self, project: &Project) -> bool {
		self.index < self.kind.len(project)
	}

	pub fn name(self, project: &Project) -> Option<&str> {
		self.kind.names(project).get(self.index).copied()
	}

	/// The item named `name` (case-insensitive) in `kind`'s list.
	pub fn by_name(kind: ItemKind, name: &str, project: &Project) -> Option<Self> {
		let lower = name.to_lowercase();
		kind.names(project).iter().position(|n| n.to_lowercase() == lower).map(|i| Self::new(kind, i))
	}
}
