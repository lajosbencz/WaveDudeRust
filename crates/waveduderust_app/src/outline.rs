//! The project tree's rules (docs/ui/20): what a row says, how a press selects, where a dragged item lands.

use crate::selection::{Row, Selection};
use waveduderust::model::{ItemKind, ItemRef, Project};

/// A folder's string key.
pub fn folder_key(kind: ItemKind) -> &'static str {
	match kind {
		ItemKind::Source => "TreeSources",
		ItemKind::Part => "TreeParts",
		ItemKind::Probe => "TreeProbes",
		ItemKind::Scheme => "TreeColorSchemes",
		_ => "Untitled",
	}
}

/// The item's Enabled (or Active) flag; `None` for schemes.
pub fn enabled_of(project: &Project, item: ItemRef) -> Option<bool> {
	match item {
		ItemRef::Source(id) => project.source(id).map(|s| s.enabled),
		ItemRef::Part(id) => project.part(id).map(|p| p.base.enabled),
		ItemRef::Probe(id) => project.probe(id).map(|p| p.active),
		_ => None,
	}
}

/// The check box's tooltip key of an item kind.
pub fn enabled_tip(kind: ItemKind) -> &'static str {
	match kind {
		ItemKind::Source => "TipSourceEnabled",
		ItemKind::Part => "TipPartEnabled",
		ItemKind::Probe | ItemKind::Scheme => "TipProbeActive",
		_ => "Untitled",
	}
}

/// A press on `row`: Ctrl toggles an item in or out (another row is taken alone), Shift ranges from the anchor to an
/// item, a press on one of several selected items makes it current, else the row alone.
pub fn press(sel: &mut Selection, project: &Project, row: Row, ctrl: bool, shift: bool) {
	match row {
		Row::Item(i) if ctrl => sel.toggle(i),
		Row::Item(i) if shift => sel.extend_to(i, project),
		Row::Item(i) if sel.items().len() > 1 && sel.contains(i) => make_current(sel, row),
		_ => sel.select(row),
	}
}

/// `row` current, the selected items kept.
fn make_current(sel: &mut Selection, row: Row) {
	sel.assign(sel.items().to_vec(), Some(row), sel.anchor());
}

/// Where a dragged item lands among its list's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropAt {
	Before(usize),
	After(usize),
	/// At the end of its list, dropped on its folder.
	Append,
}

/// Where `dragged` lands with the pointer over `row` (its upper half when `upper`) in its list; `None` when the row
/// takes no drop of it or the drop would move nothing.
pub fn drop_at(project: &Project, dragged: ItemRef, row: Row, upper: bool) -> Option<DropAt> {
	let at = project.index_of(dragged)?;
	let len = dragged.kind().len(project);
	match row {
		Row::Item(i) if i.kind() == dragged.kind() => {
			let t = project.index_of(i)?;
			if upper {
				(t != at && t != at + 1).then_some(DropAt::Before(t))
			} else {
				(t != at && t + 1 != at).then_some(DropAt::After(t))
			}
		}
		Row::Folder(kind) if kind == dragged.kind() => (at + 1 != len).then_some(DropAt::Append),
		_ => None,
	}
}

/// The index the item at `from` ends at after a drop at `to` in its list of `len`.
pub fn reorder_index(from: usize, to: DropAt, len: usize) -> usize {
	let insert = match to {
		DropAt::Before(t) => t,
		DropAt::After(t) => t + 1,
		DropAt::Append => len,
	};
	if insert > from { insert - 1 } else { insert }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_drop_lands_by_half_and_moves_something() {
		let mut p = Project::default();
		let [s0, _, s2] = [p.add_source(), p.add_source(), p.add_source()].map(ItemRef::Source);
		let item = |i| Row::Item(ItemRef::at(ItemKind::Source, i, &p).unwrap());
		let drop = |dragged, row, upper| drop_at(&p, dragged, row, upper);
		assert_eq!(drop(s0, item(2), true), Some(DropAt::Before(2)));
		assert_eq!(drop(s0, item(1), true), None, "before the next moves nothing");
		assert_eq!(drop(s0, item(0), false), None);
		assert_eq!(drop(s2, item(0), false), Some(DropAt::After(0)));
		assert_eq!(drop(s0, Row::Folder(ItemKind::Source), true), Some(DropAt::Append));
		assert_eq!(drop(s2, Row::Folder(ItemKind::Source), true), None, "the last one stays");
		assert_eq!(drop(s0, Row::Folder(ItemKind::Probe), true), None, "another list");
		assert_eq!(drop(s0, Row::Scene, true), None);
		assert_eq!(reorder_index(0, DropAt::Before(2), 3), 1);
		assert_eq!(reorder_index(2, DropAt::After(0), 3), 1);
		assert_eq!(reorder_index(0, DropAt::Append, 3), 2);
	}
}
