//! A docking layout without a toolkit: panes in tab groups, the groups in a tree of weighted splits around one
//! fixed centre area. [`Dock`] holds the tree and computes its geometry ([`Dock::layout`]), resizes it by its
//! splitters ([`Dock::resize`]), finds where a dragged pane or group lands ([`Dock::drop_target`]) and moves,
//! hides and shows panes. The tree round-trips through [`Arrangement`], serde-able with the `serde` feature.
//!
//! ```
//! use slint_dock::{Arrangement, Axis, Dock, Metrics, Point, Rect, Side, Size, Subject, Target};
//!
//! let mut dock = Dock::new(Metrics { splitter: 4., title_bar: 20., centre_min: Size::new(100., 100.) });
//! dock.register("a", Side::Left, Size::new(50., 50.));
//! dock.register("b", Side::Left, Size::new(50., 50.));
//! dock.register("log", Side::Bottom, Size::new(50., 50.));
//! let left = Arrangement::split(Axis::Vertical, [(1., Arrangement::tabs(["a"])), (2., Arrangement::tabs(["b"]))]);
//! dock.arrange(&Arrangement::split(Axis::Horizontal, [(1., left), (3., Arrangement::Centre)]))?;
//!
//! let area = Rect::new(0., 0., 1004., 604.);
//! assert_eq!(dock.layout(area).groups.len(), 3);
//! let a = Subject::Pane("a".into());
//! let drop = dock.drop_target(&a, Point::new(1000., 300.), area).expect("the right edge");
//! assert_eq!(drop.target, Target::Edge(Side::Right));
//! dock.move_to(&a, drop.target);
//! # Ok::<(), slint_dock::ArrangeError>(())
//! ```

mod arrangement;
mod geometry;
mod tree;

#[cfg(test)]
mod tests;

use std::collections::HashSet;

pub use arrangement::{ArrangeError, Arrangement, Tab};
pub use geometry::{Axis, Point, Rect, Side, Size};

use arrangement::Builder;
use geometry::distribute;
use tree::{Child, Group, Node, Split};

/// Depth of the strips along the dock's outer edges that drop at that edge.
pub const EDGE_STRIP: f32 = 40.;
/// Share of the dock a drop at an outer edge takes.
pub const EDGE_SHARE: f32 = 0.25;
/// Share of a group's width or height at each side that drops beside it; its middle tabs into it.
pub const SIDE_ZONE: f32 = 0.3;
/// Share of a group a drop beside it takes.
pub const BESIDE_SHARE: f32 = 1. / 3.;
/// The least weight a resize leaves a child, so a child shrunk to nothing keeps a share.
const MIN_WEIGHT: f32 = 1e-3;

/// A tab group, or the centre area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(u32);

impl GroupId {
	/// The centre area: never removed, never tabbed into, only split beside.
	pub const CENTRE: Self = Self(0);
}

/// A split node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SplitId(u32);

/// Sizes of the dock's chrome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
	/// Thickness of a splitter bar.
	pub splitter: f32,
	/// Height of a tab group's title bar, along its top; the centre area has none.
	pub title_bar: f32,
	/// Minimum size of the centre area.
	pub centre_min: Size,
}

/// What a drag or a move acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
	/// One pane, dragged by its tab.
	Pane(String),
	/// A whole group, dragged by its title bar.
	Group(GroupId),
}

/// Where a pane or group lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
	/// A group of its own along an outer edge of the dock, taking [`EDGE_SHARE`] of it.
	Edge(Side),
	/// Tabs of the group.
	Tab(GroupId),
	/// A group of its own at a side of the group, taking [`BESIDE_SHARE`] of it.
	Beside(GroupId, Side),
}

/// A target under the pointer and the room it would take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropTarget {
	/// Where the drop lands.
	pub target: Target,
	/// The room it takes, for the drag's preview.
	pub preview: Rect,
}

/// A shown group's place.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupLayout {
	/// The group.
	pub id: GroupId,
	/// Its whole rectangle, title bar included.
	pub rect: Rect,
	/// The rectangle below the title bar.
	pub content: Rect,
	/// Its shown panes in tab order; empty for the centre.
	pub tabs: Vec<String>,
	/// Index of the active pane into `tabs`.
	pub active: Option<usize>,
}

/// A splitter bar between two shown children of a split.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splitter {
	/// The split it resizes.
	pub split: SplitId,
	/// Index of the child before the bar among the split's children.
	pub index: usize,
	/// The split's axis; the bar moves along it.
	pub axis: Axis,
	/// The bar.
	pub rect: Rect,
	next: usize,
	start: f32,
	end: f32,
	min_before: f32,
	min_after: f32,
}

/// The geometry of the shown groups and splitters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
	/// Shown groups, the centre included, in tree order.
	pub groups: Vec<GroupLayout>,
	/// Splitter bars.
	pub splitters: Vec<Splitter>,
}

#[derive(Clone, Debug)]
struct Registered {
	id: String,
	side: Side,
	min: Size,
}

/// The docking layout: registered panes and their tree.
#[derive(Clone, Debug)]
pub struct Dock {
	metrics: Metrics,
	panes: Vec<Registered>,
	root: Node,
	next_id: u32,
}

impl Dock {
	/// A dock holding the centre area alone.
	pub fn new(metrics: Metrics) -> Self {
		Self { metrics, panes: Vec::new(), root: Node::Centre, next_id: 0 }
	}

	/// Registers pane `id` with the side [`Dock::show`] puts it at without a remembered place, and its minimum
	/// content size; registering again updates both. A registered pane is hidden until arranged or shown.
	pub fn register(&mut self, id: impl Into<String>, side: Side, min: Size) {
		let id = id.into();
		match self.panes.iter_mut().find(|p| p.id == id) {
			Some(p) => (p.side, p.min) = (side, min),
			None => self.panes.push(Registered { id, side, min }),
		}
	}

	/// The registered panes' ids in registration order.
	pub fn panes(&self) -> impl Iterator<Item = &str> {
		self.panes.iter().map(|p| p.id.as_str())
	}

	/// Replaces the tree by `arrangement`, normalised: panes not registered and repeated ones are left out, and
	/// registered panes it lacks are hidden without a place.
	pub fn arrange(&mut self, arrangement: &Arrangement) -> Result<(), ArrangeError> {
		let panes = &self.panes;
		let mut next_id = 0;
		let mut builder = Builder {
			known: |id: &str| panes.iter().any(|p| p.id == id),
			next_id: &mut next_id,
			seen: HashSet::new(),
			centres: 0,
		};
		let root = builder.build(arrangement);
		match builder.centres {
			0 => return Err(ArrangeError::NoCentre),
			1 => {}
			_ => return Err(ArrangeError::ManyCentres),
		}
		self.root = root.and_then(Node::normalise).unwrap_or(Node::Centre);
		self.next_id = next_id;
		Ok(())
	}

	/// The tree as data, for saving.
	pub fn arrangement(&self) -> Arrangement {
		Arrangement::of(&self.root)
	}

	/// Whether pane `id` is shown.
	pub fn is_shown(&self, id: &str) -> bool {
		self.group_of(id).is_some()
	}

	/// The group pane `id` is shown in.
	pub fn group_of(&self, id: &str) -> Option<GroupId> {
		self.root.group(&|g| g.tabs.iter().any(|t| t.shown && t.id == id)).map(|g| g.id)
	}

	/// The geometry of the tree in `area`.
	pub fn layout(&self, area: Rect) -> Layout {
		let mut out = Layout::default();
		self.place(&self.root, area, &mut out);
		out
	}

	/// Moves `splitter`'s bar so its middle is at `pointer` along its axis, as far as the minimum sizes of its two
	/// neighbours allow, by setting their weights. Whether the splitter is in the layout of `area`.
	pub fn resize(&mut self, splitter: &Splitter, pointer: Point, area: Rect) -> bool {
		let p = pointer.along(splitter.axis);
		if !p.is_finite() {
			return false;
		}
		let bars: Vec<Splitter> =
			self.layout(area).splitters.into_iter().filter(|s| s.split == splitter.split).collect();
		let (Some(&s), Some(last)) = (bars.iter().find(|s| s.index == splitter.index), bars.last()) else {
			return false;
		};
		let Some(split) = self.root.split_mut(s.split) else { return false };
		let bar = self.metrics.splitter;
		let room = s.end - s.start - bar;
		if room <= 0. {
			return true;
		}
		// The shown children's sizes become their weights, so only the pair moves; hidden ones scale along.
		let mut sizes: Vec<(usize, f32)> = bars.iter().map(|b| (b.index, b.rect.span(b.axis).0 - b.start)).collect();
		sizes.push((last.next, last.end - last.rect.span(last.axis).0 - bar));
		let shown: f32 = sizes.iter().map(|&(i, _)| split.children[i].weight).sum();
		let scale = sizes.iter().map(|&(_, size)| size).sum::<f32>() / shown;
		for child in &mut split.children {
			child.weight = (child.weight * scale).max(MIN_WEIGHT);
		}
		let hi = (room - s.min_after).max(0.);
		let before = (p - s.start - bar / 2.).clamp(s.min_before.min(hi), hi);
		sizes.extend([(s.index, before), (s.next, room - before)]);
		for (i, size) in sizes {
			split.children[i].weight = size.max(MIN_WEIGHT);
		}
		true
	}

	/// Where `subject` lands with the pointer at `pointer` in `area`: a strip along an outer edge drops there,
	/// else over a group a side zone splits beside it and the middle tabs into it; the centre takes sides only,
	/// the subject's own group nothing.
	pub fn drop_target(&self, subject: &Subject, pointer: Point, area: Rect) -> Option<DropTarget> {
		let own = self.subject_group(subject)?;
		if !area.contains(pointer) {
			return None;
		}
		let (side, depth) = nearest_side(area, pointer, |_, d| d);
		if depth < EDGE_STRIP {
			return Some(DropTarget { target: Target::Edge(side), preview: area.part(side, EDGE_SHARE) });
		}
		let group = self.layout(area).groups.into_iter().find(|g| g.rect.contains(pointer))?;
		if group.id == own {
			return None;
		}
		let rect = group.rect;
		let (side, share) =
			nearest_side(rect, pointer, |s, d| d / if s.axis() == Axis::Horizontal { rect.width } else { rect.height });
		if share < SIDE_ZONE {
			Some(DropTarget { target: Target::Beside(group.id, side), preview: rect.part(side, BESIDE_SHARE) })
		} else if group.id == GroupId::CENTRE {
			None
		} else {
			Some(DropTarget { target: Target::Tab(group.id), preview: rect })
		}
	}

	/// Moves `subject` to `target`, the rules of [`Dock::drop_target`] applying. Whether it moved.
	pub fn move_to(&mut self, subject: &Subject, target: Target) -> bool {
		let Some(own) = self.subject_group(subject) else { return false };
		let valid = match target {
			Target::Edge(_) => true,
			Target::Tab(g) => g != GroupId::CENTRE && g != own && self.is_visible_group(g),
			Target::Beside(g, _) => g != own && (g == GroupId::CENTRE || self.is_visible_group(g)),
		};
		if !valid {
			return false;
		}
		let moving = match subject {
			Subject::Pane(id) => {
				let Some(group) = self.root.group_mut(&|g| g.id == own) else { return false };
				let Some(i) = group.position(id) else { return false };
				let tab = group.tabs.remove(i);
				if i < group.active {
					group.active -= 1;
				}
				group.fix_active();
				Node::Group(Group { id: GroupId(self.fresh()), tabs: vec![tab], active: 0 })
			}
			Subject::Group(id) => {
				let Some(node) = self.root.take_group(*id) else { return false };
				node
			}
		};
		match target {
			Target::Edge(side) => {
				let split = SplitId(self.fresh());
				wrap(&mut self.root, moving, split, side, EDGE_SHARE);
			}
			Target::Beside(g, side) => {
				let split = SplitId(self.fresh());
				if let Some(node) = self.root.node_mut(g) {
					wrap(node, moving, split, side, BESIDE_SHARE);
				}
			}
			Target::Tab(g) => {
				if let (Node::Group(moved), Some(group)) = (moving, self.root.group_mut(&|x| x.id == g)) {
					let active = moved.tabs[moved.active].id.clone();
					group.tabs.extend(moved.tabs);
					group.active = group.position(&active).unwrap_or(group.active);
				}
			}
		}
		self.normalise();
		true
	}

	/// Hides pane `id`, keeping its place for [`Dock::show`]. Whether it was shown.
	pub fn close(&mut self, id: &str) -> bool {
		let Some(group) = self.root.group_mut(&|g| g.tabs.iter().any(|t| t.shown && t.id == id)) else {
			return false;
		};
		if let Some(i) = group.position(id) {
			group.tabs[i].shown = false;
		}
		group.fix_active();
		true
	}

	/// Shows pane `id` as the active tab of its place, or when it has none in a group of its own at its
	/// registered side, as [`Target::Edge`]. Whether it is registered.
	pub fn show(&mut self, id: &str) -> bool {
		if let Some(group) = self.root.group_mut(&|g| g.position(id).is_some()) {
			if let Some(i) = group.position(id) {
				group.tabs[i].shown = true;
				group.active = i;
			}
			return true;
		}
		let Some(side) = self.panes.iter().find(|p| p.id == id).map(|p| p.side) else { return false };
		let group = Group { id: GroupId(self.fresh()), tabs: vec![Tab { id: id.to_owned(), shown: true }], active: 0 };
		let split = SplitId(self.fresh());
		wrap(&mut self.root, Node::Group(group), split, side, EDGE_SHARE);
		self.normalise();
		true
	}

	/// Makes shown pane `id` its group's active tab. Whether it is shown.
	pub fn activate(&mut self, id: &str) -> bool {
		let Some(group) = self.root.group_mut(&|g| g.tabs.iter().any(|t| t.shown && t.id == id)) else {
			return false;
		};
		group.active = group.position(id).unwrap_or(group.active);
		true
	}

	fn fresh(&mut self) -> u32 {
		self.next_id += 1;
		self.next_id
	}

	fn normalise(&mut self) {
		self.root = std::mem::replace(&mut self.root, Node::Centre).normalise().unwrap_or(Node::Centre);
	}

	fn is_visible_group(&self, id: GroupId) -> bool {
		self.root.group(&|g| g.id == id).is_some_and(Group::is_visible)
	}

	/// The group a valid subject is dragged out of.
	fn subject_group(&self, subject: &Subject) -> Option<GroupId> {
		match subject {
			Subject::Pane(id) => self.group_of(id),
			Subject::Group(id) => (*id != GroupId::CENTRE && self.is_visible_group(*id)).then_some(*id),
		}
	}

	fn min_size(&self, node: &Node) -> Size {
		match node {
			Node::Centre => self.metrics.centre_min,
			Node::Group(g) => {
				let content = g
					.tabs
					.iter()
					.filter(|t| t.shown)
					.filter_map(|t| self.panes.iter().find(|p| p.id == t.id))
					.fold(Size::ZERO, |m, p| m.max(p.min));
				Size::new(content.width, content.height + self.metrics.title_bar)
			}
			Node::Split(s) => {
				let mins: Vec<Size> =
					s.children.iter().filter(|c| c.node.is_visible()).map(|c| self.min_size(&c.node)).collect();
				let bars = self.metrics.splitter * mins.len().saturating_sub(1) as f32;
				let along = mins.iter().map(|m| m.along(s.axis)).sum::<f32>() + bars;
				let across = mins.iter().fold(Size::ZERO, |a, &m| a.max(m));
				match s.axis {
					Axis::Horizontal => Size::new(along, across.height),
					Axis::Vertical => Size::new(across.width, along),
				}
			}
		}
	}

	fn place(&self, node: &Node, rect: Rect, out: &mut Layout) {
		match node {
			Node::Centre => out.groups.push(GroupLayout {
				id: GroupId::CENTRE,
				rect,
				content: rect,
				tabs: Vec::new(),
				active: None,
			}),
			Node::Group(g) if g.is_visible() => {
				let title = self.metrics.title_bar.min(rect.height);
				let shown: Vec<&Tab> = g.tabs.iter().filter(|t| t.shown).collect();
				out.groups.push(GroupLayout {
					id: g.id,
					rect,
					content: Rect::new(rect.x, rect.y + title, rect.width, rect.height - title),
					tabs: shown.iter().map(|t| t.id.clone()).collect(),
					active: shown.iter().position(|t| t.id == g.tabs[g.active].id),
				});
			}
			Node::Group(_) => {}
			Node::Split(s) => self.place_split(s, rect, out),
		}
	}

	fn place_split(&self, s: &Split, rect: Rect, out: &mut Layout) {
		let shown: Vec<(usize, &Child, f32)> = s
			.children
			.iter()
			.enumerate()
			.filter(|(_, c)| c.node.is_visible())
			.map(|(i, c)| (i, c, self.min_size(&c.node).along(s.axis)))
			.collect();
		let bar = self.metrics.splitter;
		let (start, extent) = rect.span(s.axis);
		let room = extent - bar * shown.len().saturating_sub(1) as f32;
		let items: Vec<(f32, f32)> = shown.iter().map(|&(_, c, min)| (c.weight, min)).collect();
		let sizes = distribute(room, &items);
		let mut pos = start;
		for (k, (&(i, child, min), &size)) in shown.iter().zip(&sizes).enumerate() {
			self.place(&child.node, rect.with_span(s.axis, pos, size), out);
			if let (Some(&(next, _, min_after)), Some(&after)) = (shown.get(k + 1), sizes.get(k + 1)) {
				out.splitters.push(Splitter {
					split: s.id,
					index: i,
					axis: s.axis,
					rect: rect.with_span(s.axis, pos + size, bar),
					next,
					start: pos,
					end: pos + size + bar + after,
					min_before: min,
					min_after,
				});
			}
			pos += size + bar;
		}
	}
}

/// Replaces `node` by a split of it and `new` at `side`, `new` taking `share`.
fn wrap(node: &mut Node, new: Node, id: SplitId, side: Side, share: f32) {
	let old = std::mem::replace(node, Node::Centre);
	let new = Child { weight: share, node: new };
	let old = Child { weight: 1. - share, node: old };
	let children = if side.leads() { vec![new, old] } else { vec![old, new] };
	*node = Node::Split(Split { id, axis: side.axis(), children });
}

/// The side of `rect` nearest `p` by `measure` of its depth, and that measure.
fn nearest_side(rect: Rect, p: Point, measure: impl Fn(Side, f32) -> f32) -> (Side, f32) {
	let [first, rest @ ..] = Side::ALL;
	rest.into_iter().fold((first, measure(first, rect.depth(first, p))), |best, s| {
		let d = measure(s, rect.depth(s, p));
		if d < best.1 { (s, d) } else { best }
	})
}
