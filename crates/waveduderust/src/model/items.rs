//! The named items of a project: sources, parts, probes and colour schemes, addressed by their ids.

use super::names::Named;
use super::{ColorScheme, Part, PartId, Probe, ProbeId, Project, SchemeId, Source, SourceId};

/// The four lists of named items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
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

	/// The items of the list, in order.
	pub fn items(self, project: &Project) -> Vec<ItemRef> {
		match self {
			Self::Source => project.sources.iter().map(|s| ItemRef::Source(s.id())).collect(),
			Self::Part => project.parts.iter().map(|p| ItemRef::Part(p.id())).collect(),
			Self::Probe => project.probes.iter().map(|p| ItemRef::Probe(p.id())).collect(),
			Self::Scheme => project.color_schemes.iter().map(|c| ItemRef::Scheme(c.id())).collect(),
		}
	}
}

/// A source, part, probe or colour scheme by its id. `Ord` is by kind, then by id (not list order: [`Self::order`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ItemRef {
	Source(SourceId),
	Part(PartId),
	Probe(ProbeId),
	Scheme(SchemeId),
}

impl From<SourceId> for ItemRef {
	fn from(id: SourceId) -> Self {
		Self::Source(id)
	}
}

impl From<PartId> for ItemRef {
	fn from(id: PartId) -> Self {
		Self::Part(id)
	}
}

impl From<ProbeId> for ItemRef {
	fn from(id: ProbeId) -> Self {
		Self::Probe(id)
	}
}

impl From<SchemeId> for ItemRef {
	fn from(id: SchemeId) -> Self {
		Self::Scheme(id)
	}
}

impl ItemRef {
	pub fn kind(self) -> ItemKind {
		match self {
			Self::Source(_) => ItemKind::Source,
			Self::Part(_) => ItemKind::Part,
			Self::Probe(_) => ItemKind::Probe,
			Self::Scheme(_) => ItemKind::Scheme,
		}
	}

	/// Sort key in project order: by kind, then by list position; a missing item sorts last of its kind.
	pub fn order(self, project: &Project) -> (ItemKind, usize) {
		(self.kind(), project.index_of(self).unwrap_or(usize::MAX))
	}

	pub fn exists(self, project: &Project) -> bool {
		project.index_of(self).is_some()
	}

	pub fn name(self, project: &Project) -> Option<&str> {
		match self {
			Self::Source(id) => project.source(id).map(Named::name),
			Self::Part(id) => project.part(id).map(Named::name),
			Self::Probe(id) => project.probe(id).map(Named::name),
			Self::Scheme(id) => project.color_scheme(id).map(Named::name),
		}
	}

	/// The item at `index` of `kind`'s list.
	pub fn at(kind: ItemKind, index: usize, project: &Project) -> Option<Self> {
		Some(match kind {
			ItemKind::Source => Self::Source(project.sources.get(index)?.id()),
			ItemKind::Part => Self::Part(project.parts.get(index)?.id()),
			ItemKind::Probe => Self::Probe(project.probes.get(index)?.id()),
			ItemKind::Scheme => Self::Scheme(project.color_schemes.get(index)?.id()),
		})
	}

	/// The item named `name` (case-insensitive) in `kind`'s list.
	pub fn by_name(kind: ItemKind, name: &str, project: &Project) -> Option<Self> {
		let lower = name.to_lowercase();
		let index = kind.names(project).iter().position(|n| n.to_lowercase() == lower)?;
		Self::at(kind, index, project)
	}
}

/// An item of one of the lists.
pub(crate) trait Identified: Named + Clone {
	type Id: Copy + Eq;
	fn id(&self) -> Self::Id;
	/// Gives the item a fresh id.
	fn renew_id(&mut self);
}

macro_rules! identified {
	($t:ty, $id:ty) => {
		impl Identified for $t {
			type Id = $id;
			fn id(&self) -> $id {
				self.id
			}
			fn renew_id(&mut self) {
				self.id = <$id>::mint();
			}
		}
	};
}

identified!(Source, SourceId);
identified!(Part, PartId);
identified!(Probe, ProbeId);
identified!(ColorScheme, SchemeId);

pub(crate) fn position<T: Identified>(items: &[T], id: T::Id) -> Option<usize> {
	items.iter().position(|t| Identified::id(t) == id)
}

fn find<T: Identified>(items: &[T], id: T::Id) -> Option<&T> {
	items.iter().find(|t| Identified::id(*t) == id)
}

fn find_mut<T: Identified>(items: &mut [T], id: T::Id) -> Option<&mut T> {
	items.iter_mut().find(|t| Identified::id(&**t) == id)
}

/// Inserts `item` with a fresh id at `at`, clamped to the list's end.
pub(crate) fn insert<T: Identified>(items: &mut Vec<T>, at: usize, mut item: T) -> T::Id {
	item.renew_id();
	let id = item.id();
	items.insert(at.min(items.len()), item);
	id
}

fn remove<T: Identified>(items: &mut Vec<T>, id: T::Id) -> bool {
	position(items, id).map(|i| items.remove(i)).is_some()
}

fn move_to<T: Identified>(items: &mut Vec<T>, id: T::Id, to: usize) -> bool {
	let Some(from) = position(items, id) else {
		return false;
	};
	let item = items.remove(from);
	items.insert(to.min(items.len()), item);
	true
}

impl Project {
	pub fn source(&self, id: SourceId) -> Option<&Source> {
		find(&self.sources, id)
	}

	pub fn source_mut(&mut self, id: SourceId) -> Option<&mut Source> {
		find_mut(&mut self.sources, id)
	}

	pub fn part(&self, id: PartId) -> Option<&Part> {
		find(&self.parts, id)
	}

	pub fn part_mut(&mut self, id: PartId) -> Option<&mut Part> {
		find_mut(&mut self.parts, id)
	}

	pub fn probe(&self, id: ProbeId) -> Option<&Probe> {
		find(&self.probes, id)
	}

	pub fn probe_mut(&mut self, id: ProbeId) -> Option<&mut Probe> {
		find_mut(&mut self.probes, id)
	}

	pub fn color_scheme(&self, id: SchemeId) -> Option<&ColorScheme> {
		find(&self.color_schemes, id)
	}

	pub fn color_scheme_mut(&mut self, id: SchemeId) -> Option<&mut ColorScheme> {
		find_mut(&mut self.color_schemes, id)
	}

	/// The item's position in its list: emitter, paint or marker order.
	pub fn index_of(&self, item: impl Into<ItemRef>) -> Option<usize> {
		match item.into() {
			ItemRef::Source(id) => position(&self.sources, id),
			ItemRef::Part(id) => position(&self.parts, id),
			ItemRef::Probe(id) => position(&self.probes, id),
			ItemRef::Scheme(id) => position(&self.color_schemes, id),
		}
	}

	/// Inserts `source` at `at` (clamped to the end) under a fresh id; the name is kept as it is.
	pub fn insert_source(&mut self, at: usize, source: Source) -> SourceId {
		insert(&mut self.sources, at, source)
	}

	pub fn insert_part(&mut self, at: usize, part: Part) -> PartId {
		insert(&mut self.parts, at, part)
	}

	pub fn insert_probe(&mut self, at: usize, probe: Probe) -> ProbeId {
		insert(&mut self.probes, at, probe)
	}

	pub fn insert_color_scheme(&mut self, at: usize, scheme: ColorScheme) -> SchemeId {
		insert(&mut self.color_schemes, at, scheme)
	}

	/// Removes the item; false when it is not in the project.
	pub fn remove(&mut self, item: impl Into<ItemRef>) -> bool {
		match item.into() {
			ItemRef::Source(id) => remove(&mut self.sources, id),
			ItemRef::Part(id) => remove(&mut self.parts, id),
			ItemRef::Probe(id) => remove(&mut self.probes, id),
			ItemRef::Scheme(id) => remove(&mut self.color_schemes, id),
		}
	}

	/// Moves the item to position `to` of its list (clamped to the end); false when it is not in the project.
	pub fn move_item(&mut self, item: impl Into<ItemRef>, to: usize) -> bool {
		match item.into() {
			ItemRef::Source(id) => move_to(&mut self.sources, id, to),
			ItemRef::Part(id) => move_to(&mut self.parts, id, to),
			ItemRef::Probe(id) => move_to(&mut self.probes, id, to),
			ItemRef::Scheme(id) => move_to(&mut self.color_schemes, id, to),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{PartKind, Rectangle};

	#[test]
	fn ids_stay_with_their_items_across_a_reorder() {
		let mut p = Project::default();
		let [a, b, c] = [p.add_source(), p.add_source(), p.add_source()];
		assert!(p.move_item(a, 2));
		assert_eq!(ItemKind::Source.items(&p), [b.into(), c.into(), a.into()]);
		assert_eq!(p.index_of(a), Some(2));
		assert_eq!(p.source(a).unwrap().name, "Source 1");
		assert!(p.remove(b));
		assert!(!p.remove(b));
		assert!(!ItemRef::Source(b).exists(&p));
		assert_eq!(p.index_of(c), Some(0));
	}

	#[test]
	fn an_inserted_item_gets_a_fresh_id() {
		let mut p = Project::default();
		let a = p.add_part(PartKind::Rectangle(Rectangle::default()));
		let copy = p.part(a).unwrap().clone();
		assert_eq!(copy.id(), a, "a clone keeps the id");
		let b = p.insert_part(0, copy);
		assert_ne!(a, b);
		assert_eq!(p.parts[0], p.parts[1], "equality is by value");
		assert_eq!(p.index_of(b), Some(0));
	}

	#[test]
	fn lookups_by_name_and_position() {
		let mut p = Project::default();
		p.add_probe();
		let q = p.add_probe();
		assert_eq!(ItemRef::by_name(ItemKind::Probe, "probe 2", &p), Some(ItemRef::Probe(q)));
		assert_eq!(ItemRef::at(ItemKind::Probe, 1, &p), Some(ItemRef::Probe(q)));
		assert_eq!(ItemRef::at(ItemKind::Probe, 2, &p), None);
		assert_eq!(ItemRef::Probe(q).name(&p), Some("Probe 2"));
		assert_eq!(ItemRef::Probe(q).kind(), ItemKind::Probe);
	}
}
