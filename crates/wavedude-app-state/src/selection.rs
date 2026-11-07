//! The selection shared by the tree, the picture handles and the commands (docs/ui/20-project-tree.md): a list
//! of selected items plus one current row, the tree's rules, and the group move.
//!
//! Items are addressed by list and index ([`ItemRef`]); a command that shifts indices sets the selection it
//! leaves. Threads: plain data on the UI thread.

use wavedude_core::model::names::Named;
use wavedude_core::model::{Part, Point, Probe, ProbeKind, Project, Source};

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

/// A row of the project tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Row {
	Project,
	Scene,
	Simulation,
	Display,
	Folder(ItemKind),
	Item(ItemRef),
}

/// Selected items plus the current row (the one the properties pane shows). A current item is always a member.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
	items: Vec<ItemRef>,
	current: Option<Row>,
	anchor: Option<ItemRef>,
}

/// The order a range runs in: sources, parts and probes as one sequence; schemes on their own.
fn range_sequence(project: &Project, scheme: bool) -> Vec<ItemRef> {
	let kinds: &[ItemKind] =
		if scheme { &[ItemKind::Scheme] } else { &[ItemKind::Source, ItemKind::Part, ItemKind::Probe] };
	kinds.iter().flat_map(|&k| (0..k.len(project)).map(move |i| ItemRef::new(k, i))).collect()
}

impl Selection {
	pub fn items(&self) -> &[ItemRef] {
		&self.items
	}

	pub fn current(&self) -> Option<Row> {
		self.current
	}

	pub fn current_item(&self) -> Option<ItemRef> {
		match self.current {
			Some(Row::Item(i)) => Some(i),
			_ => None,
		}
	}

	pub fn contains(&self, item: ItemRef) -> bool {
		self.items.contains(&item)
	}

	pub fn is_empty(&self) -> bool {
		self.items.is_empty() && self.current.is_none()
	}

	/// A click or the arrows: the row alone, the anchor of later ranges.
	pub fn select(&mut self, row: Row) {
		self.current = Some(row);
		match row {
			Row::Item(i) => {
				self.items = vec![i];
				self.anchor = Some(i);
			}
			_ => self.items.clear(),
		}
	}

	/// The anchor of later ranges.
	pub fn anchor(&self) -> Option<ItemRef> {
		self.anchor
	}

	/// Takes a selection made elsewhere (the tree view); a current item is added to the items when missing.
	pub fn assign(&mut self, mut items: Vec<ItemRef>, current: Option<Row>, anchor: Option<ItemRef>) {
		if let Some(Row::Item(i)) = current
			&& !items.contains(&i)
		{
			items.push(i);
		}
		*self = Self { items, current, anchor };
	}

	/// A click on empty picture space: nothing selected.
	pub fn clear(&mut self) {
		*self = Self::default();
	}

	/// Sets the items in order, the last one current (a paste, a duplicate, an add).
	pub fn set_items(&mut self, items: Vec<ItemRef>) {
		self.current = items.last().map(|&i| Row::Item(i));
		self.anchor = items.last().copied();
		self.items = items;
	}

	/// Ctrl+click, Insert, Ctrl+Space: the item in or out; one taken in becomes current, and when the current
	/// item is taken out the current moves to the last one left.
	pub fn toggle(&mut self, item: ItemRef) {
		if let Some(at) = self.items.iter().position(|&i| i == item) {
			self.items.remove(at);
			if self.current == Some(Row::Item(item)) {
				self.current = self.items.last().map(|&i| Row::Item(i));
			}
		} else {
			self.items.push(item);
			self.current = Some(Row::Item(item));
		}
	}

	/// Shift+click, Shift+arrows: the range from the anchor to `item` in tree order; `item` becomes current.
	/// A scheme ranges over the schemes only, the other items over sources, parts and probes.
	pub fn extend_to(&mut self, item: ItemRef, project: &Project) {
		let scheme = item.kind == ItemKind::Scheme;
		let anchor = self.anchor.filter(|a| (a.kind == ItemKind::Scheme) == scheme && a.exists(project));
		let Some(anchor) = anchor else {
			self.select(Row::Item(item));
			return;
		};
		let seq = range_sequence(project, scheme);
		let at = |x: ItemRef| seq.iter().position(|&i| i == x).expect("an existing item of the anchor's family");
		let (a, b) = (at(anchor), at(item));
		let (lo, hi) = (a.min(b), a.max(b));
		self.items = seq[lo..=hi].to_vec();
		self.current = Some(Row::Item(item));
	}

	/// Ctrl+A: every source, part and probe; the last becomes current.
	pub fn select_all(&mut self, project: &Project) {
		let all = range_sequence(project, false);
		self.anchor = all.first().copied();
		self.current = all.last().map(|&i| Row::Item(i));
		self.items = all;
	}

	/// The Explorer rule: a command on one of several selected items acts on all of them (in tree order), else
	/// on `target` alone.
	pub fn targets(&self, target: ItemRef) -> Vec<ItemRef> {
		if self.items.len() > 1 && self.contains(target) {
			let mut all = self.items.clone();
			all.sort();
			all
		} else {
			vec![target]
		}
	}

	/// Drops references that no longer exist (after a list shrank).
	pub fn retain_existing(&mut self, project: &Project) {
		self.items.retain(|i| i.exists(project));
		if let Some(Row::Item(i)) = self.current
			&& !i.exists(project)
		{
			self.current = self.items.last().map(|&i| Row::Item(i));
		}
		self.anchor = self.anchor.filter(|a| a.exists(project));
	}

	/// The selection carried from `old` into `new` (an undo or redo): each item by its name, else by its index in
	/// the same list; settings rows and folders by place.
	pub fn follow(&self, old: &Project, new: &Project) -> Self {
		let map = |i: ItemRef| -> Option<ItemRef> {
			i.name(old).and_then(|n| ItemRef::by_name(i.kind, n, new)).or_else(|| i.exists(new).then_some(i))
		};
		let mut items: Vec<ItemRef> = Vec::new();
		for i in self.items.iter().filter_map(|&i| map(i)) {
			if !items.contains(&i) {
				items.push(i);
			}
		}
		let current = match self.current {
			Some(Row::Item(i)) => map(i).map(Row::Item).or_else(|| items.last().map(|&i| Row::Item(i))),
			other => other,
		};
		Self { items, current, anchor: self.anchor.and_then(map) }
	}
}

/// Something with canvas positions that a group move shifts.
pub trait Movable {
	/// The positions that must stay on the canvas (canvas pixels).
	fn anchors(&self) -> Vec<Point>;
	fn move_by(&mut self, dx: i32, dy: i32);
}

impl Movable for Source {
	/// The cell the position lies in; a move keeps the fraction.
	fn anchors(&self) -> Vec<Point> {
		vec![Point::new(self.x.floor() as i32, self.y.floor() as i32)]
	}

	fn move_by(&mut self, dx: i32, dy: i32) {
		self.x += f64::from(dx);
		self.y += f64::from(dy);
	}
}

impl Movable for Probe {
	fn anchors(&self) -> Vec<Point> {
		let a = Point::new(self.ax, self.ay);
		if self.kind == ProbeKind::Line { vec![a, Point::new(self.bx, self.by)] } else { vec![a] }
	}

	/// Moves A (and B of a line probe); the graph box follows while it sits on A.
	fn move_by(&mut self, dx: i32, dy: i32) {
		if (self.box_x, self.box_y) == (self.ax, self.ay) {
			self.box_x += dx;
			self.box_y += dy;
		}
		self.ax += dx;
		self.ay += dy;
		if self.kind == ProbeKind::Line {
			self.bx += dx;
			self.by += dy;
		}
	}
}

impl Movable for Part {
	fn anchors(&self) -> Vec<Point> {
		vec![Point::new(self.base.x, self.base.y)]
	}

	fn move_by(&mut self, dx: i32, dy: i32) {
		self.base.x += dx;
		self.base.y += dy;
	}
}

/// The part of a wanted move that keeps every anchor on a `width` x `height` canvas, so a group pushed against
/// an edge stops as a whole and keeps its formation. An anchor already outside can only move back in.
pub fn limit_move(anchors: impl IntoIterator<Item = Point>, dx: i32, dy: i32, width: u32, height: u32) -> (i32, i32) {
	let mut bounds: Option<(i32, i32, i32, i32)> = None;
	for p in anchors {
		let b = bounds.get_or_insert((p.x, p.y, p.x, p.y));
		*b = (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y));
	}
	let Some((min_x, min_y, max_x, max_y)) = bounds else {
		return (dx, dy);
	};
	let right = (width - 1) as i32;
	let bottom = (height - 1) as i32;
	(dx.clamp((-min_x).min(0), (right - max_x).max(0)), dy.clamp((-min_y).min(0), (bottom - max_y).max(0)))
}

/// Moves `items` of `project` as a group by up to (dx, dy), held inside the canvas; schemes are ignored.
/// Returns the applied delta.
pub fn move_group(project: &mut Project, items: &[ItemRef], dx: i32, dy: i32) -> (i32, i32) {
	let (w, h) = project.canvas_size();
	let anchors: Vec<Point> = items
		.iter()
		.flat_map(|i| match i.kind {
			ItemKind::Source => project.sources.get(i.index).map(Movable::anchors),
			ItemKind::Part => project.parts.get(i.index).map(Movable::anchors),
			ItemKind::Probe => project.probes.get(i.index).map(Movable::anchors),
			ItemKind::Scheme => None,
		})
		.flatten()
		.collect();
	let (dx, dy) = limit_move(anchors, dx, dy, w, h);
	if (dx, dy) == (0, 0) {
		return (0, 0);
	}
	for i in items {
		let item: Option<&mut dyn Movable> = match i.kind {
			ItemKind::Source => project.sources.get_mut(i.index).map(|s| s as &mut dyn Movable),
			ItemKind::Part => project.parts.get_mut(i.index).map(|p| p as &mut dyn Movable),
			ItemKind::Probe => project.probes.get_mut(i.index).map(|p| p as &mut dyn Movable),
			ItemKind::Scheme => None,
		};
		if let Some(item) = item {
			item.move_by(dx, dy);
		}
	}
	(dx, dy)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 100;
		p.scene.height = 80;
		for _ in 0..3 {
			p.add_source();
			p.add_probe();
		}
		p.add_part(wavedude_core::model::PartKind::Rectangle(Default::default()));
		p.add_new_color_scheme();
		p.add_new_color_scheme();
		p
	}

	const S: ItemKind = ItemKind::Source;

	#[test]
	fn click_toggle_and_range() {
		let p = project();
		let mut sel = Selection::default();
		sel.select(Row::Item(ItemRef::new(S, 1)));
		sel.extend_to(ItemRef::new(ItemKind::Probe, 0), &p);
		assert_eq!(
			sel.items(),
			[ItemRef::new(S, 1), ItemRef::new(S, 2), ItemRef::new(ItemKind::Part, 0), ItemRef::new(ItemKind::Probe, 0)]
		);
		assert_eq!(sel.current(), Some(Row::Item(ItemRef::new(ItemKind::Probe, 0))));
		sel.toggle(ItemRef::new(ItemKind::Probe, 0));
		assert_eq!(sel.current(), Some(Row::Item(ItemRef::new(ItemKind::Part, 0))), "current moves to the last left");
		sel.toggle(ItemRef::new(ItemKind::Scheme, 1));
		assert_eq!(sel.items().len(), 4);
		sel.extend_to(ItemRef::new(ItemKind::Scheme, 0), &p);
		assert_eq!(sel.items(), [ItemRef::new(ItemKind::Scheme, 0)], "a scheme range needs a scheme anchor");
		sel.select_all(&p);
		assert_eq!(sel.items().len(), 7);
		assert_eq!(sel.current(), Some(Row::Item(ItemRef::new(ItemKind::Probe, 2))));
	}

	#[test]
	fn explorer_rule() {
		let mut sel = Selection::default();
		sel.select(Row::Item(ItemRef::new(S, 2)));
		sel.toggle(ItemRef::new(S, 0));
		assert_eq!(sel.targets(ItemRef::new(S, 2)), [ItemRef::new(S, 0), ItemRef::new(S, 2)]);
		assert_eq!(sel.targets(ItemRef::new(S, 1)), [ItemRef::new(S, 1)]);
	}

	#[test]
	fn group_move_holds_the_formation() {
		let mut p = project();
		p.sources[0].x = 10.0;
		p.sources[1].x = 90.0;
		let items = [ItemRef::new(S, 0), ItemRef::new(S, 1)];
		assert_eq!(move_group(&mut p, &items, 50, 0), (9, 0));
		assert_eq!((p.sources[0].x, p.sources[1].x), (19.0, 99.0));
		assert_eq!(move_group(&mut p, &items, -100, -100).0, -19);
		assert_eq!((p.sources[0].x, p.sources[1].x), (0.0, 80.0));
	}

	#[test]
	fn follows_by_name_then_index() {
		let old = project();
		let mut new = old.clone();
		new.sources.remove(0);
		new.probes[1].name = "Renamed".into();
		let mut sel = Selection::default();
		sel.select(Row::Item(ItemRef::new(S, 1)));
		sel.toggle(ItemRef::new(ItemKind::Probe, 1));
		let f = sel.follow(&old, &new);
		assert_eq!(f.items(), [ItemRef::new(S, 0), ItemRef::new(ItemKind::Probe, 1)]);
	}
}
