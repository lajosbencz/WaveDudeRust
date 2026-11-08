//! Dock host: one centre area with tool panes on its four sides; panes sharing a group show a tab strip;
//! title bars with a "window position" drop-down and a close button; a title bar or a tab dragged past the drag
//! threshold shows the drop targets (the four outer edges, and on each group its centre to tab into it and its
//! two ends to split beside it); draggable, keyboard-resizable splitters; show/hide by id; a serialisable
//! layout.

use std::collections::HashMap;

use freya::prelude::*;
use serde::{Deserialize, Serialize};

use crate::icon::{IconView, icons};
use crate::menu::{ContextMenuHost, ContextMenuRequest, MenuEntry, MenuItem, MenuModel};
use crate::theme::{Theme, use_wave_theme};
use crate::util::{TestIdExt, focus_border};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
	Left,
	Right,
	Top,
	Bottom,
}

impl Side {
	pub const ALL: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

	/// Left and right sides stack their groups vertically.
	pub fn is_vertical(self) -> bool {
		matches!(self, Side::Left | Side::Right)
	}

	fn word(self) -> &'static str {
		match self {
			Side::Left => "left",
			Side::Right => "right",
			Side::Top => "top",
			Side::Bottom => "bottom",
		}
	}

	fn from_word(word: &str) -> Option<Self> {
		Side::ALL.into_iter().find(|s| s.word() == word)
	}
}

/// Panes sharing one place; `active` is the one shown. `weight` is its share along the side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneGroup {
	pub panes: Vec<String>,
	pub active: usize,
	pub weight: f32,
}

impl PaneGroup {
	pub fn new(panes: Vec<String>, weight: f32) -> Self {
		Self { panes, active: 0, weight }
	}

	/// The shown pane; a group is never empty and `active` indexes it ([`DockLayout::retain_known`] at load).
	pub fn active_pane(&self) -> &str {
		&self.panes[self.active]
	}
}

/// The groups docked along one side; `size` is the side's depth in logical pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SideDock {
	pub size: f32,
	pub groups: Vec<PaneGroup>,
}

impl SideDock {
	fn new(size: f32) -> Self {
		Self { size, groups: Vec::new() }
	}
}

/// The arrangement of the dock host, saved by pane id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockLayout {
	pub left: SideDock,
	pub right: SideDock,
	pub top: SideDock,
	pub bottom: SideDock,
	/// Hidden panes with the side they left, for showing them there again.
	pub hidden: Vec<(String, Side)>,
}

impl Default for DockLayout {
	fn default() -> Self {
		Self {
			left: SideDock::new(320.),
			right: SideDock::new(320.),
			top: SideDock::new(160.),
			bottom: SideDock::new(200.),
			hidden: Vec::new(),
		}
	}
}

/// Smallest side depth and group length, logical pixels.
pub const MIN_SIZE: f32 = 60.;

impl DockLayout {
	pub fn side(&self, side: Side) -> &SideDock {
		match side {
			Side::Left => &self.left,
			Side::Right => &self.right,
			Side::Top => &self.top,
			Side::Bottom => &self.bottom,
		}
	}

	pub fn side_mut(&mut self, side: Side) -> &mut SideDock {
		match side {
			Side::Left => &mut self.left,
			Side::Right => &mut self.right,
			Side::Top => &mut self.top,
			Side::Bottom => &mut self.bottom,
		}
	}

	/// Where a shown pane is: side, group index, index in the group.
	pub fn find(&self, id: &str) -> Option<(Side, usize, usize)> {
		Side::ALL.into_iter().find_map(|side| {
			self.side(side)
				.groups
				.iter()
				.enumerate()
				.find_map(|(g, group)| group.panes.iter().position(|p| p == id).map(|i| (side, g, i)))
		})
	}

	pub fn is_shown(&self, id: &str) -> bool {
		self.find(id).is_some()
	}

	/// Shows a pane where it was last, else on `default_side`; a shown pane becomes its group's active one.
	pub fn show(&mut self, id: &str, default_side: Side) {
		if self.find(id).is_some() {
			self.activate(id);
			return;
		}
		let side = self.hidden.iter().find(|(h, _)| h == id).map_or(default_side, |(_, s)| *s);
		self.hidden.retain(|(h, _)| h != id);
		self.side_mut(side).groups.push(PaneGroup::new(vec![id.to_string()], 1.));
	}

	pub fn hide(&mut self, id: &str) {
		if let Some(side) = self.take(id) {
			self.hidden.retain(|(h, _)| h != id);
			self.hidden.push((id.to_string(), side));
		}
	}

	pub fn toggle(&mut self, id: &str, default_side: Side) {
		if self.is_shown(id) {
			self.hide(id);
		} else {
			self.show(id, default_side);
		}
	}

	/// Moves a pane into a new group at the end of `side`.
	pub fn move_to(&mut self, id: &str, side: Side) {
		if self.take(id).is_some() {
			self.side_mut(side).groups.push(PaneGroup::new(vec![id.to_string()], 1.));
		}
	}

	/// Moves a pane into a new group of `side` at `index` (the end when past the groups).
	pub fn insert_at(&mut self, id: &str, side: Side, index: usize) {
		let Some((from_side, from_group, _)) = self.find(id) else { return };
		let emptied = self.side(from_side).groups[from_group].panes.len() == 1;
		self.take(id);
		let index = if from_side == side && emptied && from_group < index { index - 1 } else { index };
		let groups = &mut self.side_mut(side).groups;
		let index = index.min(groups.len());
		groups.insert(index, PaneGroup::new(vec![id.to_string()], 1.));
	}

	/// Applies a drop target.
	pub fn drop(&mut self, id: &str, target: DropTarget) {
		match target {
			DropTarget::Edge(side) => self.move_to(id, side),
			DropTarget::Tab(side, group) => self.tab_into(id, side, group),
			DropTarget::Before(side, group) => self.insert_at(id, side, group),
			DropTarget::After(side, group) => self.insert_at(id, side, group + 1),
		}
	}

	/// Moves a pane into an existing group as a tab, active.
	pub fn tab_into(&mut self, id: &str, side: Side, group: usize) {
		let Some((from_side, from_group, _)) = self.find(id) else { return };
		if from_side == side && from_group == group {
			return;
		}
		let target_len = self.side(side).groups.len();
		if group >= target_len {
			return;
		}
		let emptied = self.side(from_side).groups[from_group].panes.len() == 1;
		self.take(id);
		let group = if from_side == side && emptied && from_group < group { group - 1 } else { group };
		let g = &mut self.side_mut(side).groups[group];
		g.panes.push(id.to_string());
		g.active = g.panes.len() - 1;
	}

	pub fn activate(&mut self, id: &str) {
		if let Some((side, g, i)) = self.find(id) {
			self.side_mut(side).groups[g].active = i;
		}
	}

	/// Changes a side's depth by `delta` px (positive grows it), kept within `MIN_SIZE..=max`.
	pub fn resize_side(&mut self, side: Side, delta: f32, max: f32) {
		let s = self.side_mut(side);
		s.size = (s.size + delta).clamp(MIN_SIZE, max.max(MIN_SIZE));
	}

	/// Moves the splitter after group `index` by `delta` px along a side `length` px long.
	pub fn resize_groups(&mut self, side: Side, index: usize, delta: f32, length: f32) {
		let groups = &mut self.side_mut(side).groups;
		if index + 1 >= groups.len() || length <= 0. {
			return;
		}
		let total: f32 = groups.iter().map(|g| g.weight).sum();
		let per_px = total / length;
		let min = MIN_SIZE * per_px;
		let pair = groups[index].weight + groups[index + 1].weight;
		let a = (groups[index].weight + delta * per_px).clamp(min.min(pair / 2.), (pair - min).max(pair / 2.));
		groups[index].weight = a;
		groups[index + 1].weight = pair - a;
	}

	/// Drops ids the app does not register (a layout saved by another version).
	pub fn retain_known(&mut self, known: &[&str]) {
		for side in Side::ALL {
			let s = self.side_mut(side);
			for g in &mut s.groups {
				g.panes.retain(|p| known.contains(&p.as_str()));
				g.active = g.active.min(g.panes.len().saturating_sub(1));
			}
			s.groups.retain(|g| !g.panes.is_empty());
		}
		self.hidden.retain(|(h, _)| known.contains(&h.as_str()));
	}

	fn take(&mut self, id: &str) -> Option<Side> {
		let (side, g, i) = self.find(id)?;
		let groups = &mut self.side_mut(side).groups;
		groups[g].panes.remove(i);
		if groups[g].panes.is_empty() {
			groups.remove(g);
		} else if groups[g].active >= i && groups[g].active > 0 {
			groups[g].active -= 1;
		}
		Some(side)
	}
}

/// Where a dragged pane may land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
	/// A new group at the end of an outer edge.
	Edge(Side),
	/// A tab of the group.
	Tab(Side, usize),
	/// A new group before the group, along its side.
	Before(Side, usize),
	After(Side, usize),
}

/// A rectangle in window logical pixels: left, top, width, height.
pub type Rect4 = (f32, f32, f32, f32);

fn contains(r: Rect4, p: (f32, f32)) -> bool {
	p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3
}

/// Depth of an outer edge's drop strip, logical pixels.
pub const EDGE_STRIP: f32 = 40.;
/// The share of a group's length at either end that splits beside it; the middle tabs into it.
const END_SHARE: f32 = 0.3;

/// The drop target under `p`, with the rectangle shown for it: a group's end or middle first, then an outer
/// edge strip of the host. A pane never targets its own group.
pub fn drop_target_at(
	host: Rect4,
	groups: &[((Side, usize), Rect4)],
	own: Option<(Side, usize)>,
	p: (f32, f32),
) -> Option<(DropTarget, Rect4)> {
	for &((side, index), r) in groups {
		if !contains(r, p) || own == Some((side, index)) {
			continue;
		}
		let (x, y, w, h) = r;
		let (along, length, start) = if side.is_vertical() { (p.1 - y, h, y) } else { (p.0 - x, w, x) };
		let end = length * END_SHARE;
		let target = if along < end {
			(DropTarget::Before(side, index), if side.is_vertical() { (x, start, w, end) } else { (start, y, end, h) })
		} else if along > length - end {
			let far = start + length - end;
			(DropTarget::After(side, index), if side.is_vertical() { (x, far, w, end) } else { (far, y, end, h) })
		} else {
			(DropTarget::Tab(side, index), r)
		};
		return Some(target);
	}
	let (hx, hy, hw, hh) = host;
	let strips = [
		(Side::Left, (hx, hy, EDGE_STRIP, hh)),
		(Side::Right, (hx + hw - EDGE_STRIP, hy, EDGE_STRIP, hh)),
		(Side::Top, (hx, hy, hw, EDGE_STRIP)),
		(Side::Bottom, (hx, hy + hh - EDGE_STRIP, hw, EDGE_STRIP)),
	];
	strips.into_iter().find(|(_, r)| contains(*r, p)).map(|(side, r)| (DropTarget::Edge(side), r))
}

/// A pane the host can show.
#[derive(Clone, PartialEq)]
pub struct DockPane {
	pub id: String,
	pub title: String,
	pub content: Element,
}

impl DockPane {
	pub fn new(id: impl Into<String>, title: impl Into<String>, content: impl IntoElement) -> Self {
		Self { id: id.into(), title: title.into(), content: content.into_element() }
	}
}

/// Words of the title bar's drop-down, so the app can localise them.
#[derive(Clone, Debug, PartialEq)]
pub struct DockLabels {
	pub position: String,
	pub move_to: [String; 4],
	pub close: String,
}

impl Default for DockLabels {
	fn default() -> Self {
		Self {
			position: "Window position".into(),
			move_to: ["Move to _Left".into(), "Move to _Right".into(), "Move to _Top".into(), "Move to _Bottom".into()],
			close: "_Close".into(),
		}
	}
}

const SPLITTER: f32 = 4.;
/// Pointer travel that turns a press on a title bar or tab into a drag, logical pixels.
const DRAG_THRESHOLD: f64 = 6.;

/// A title bar or tab being dragged.
#[derive(Clone, Debug, PartialEq)]
struct DockDrag {
	pane: String,
	from: (Side, usize),
	start: (f64, f64),
	at: (f64, f64),
	active: bool,
	target: Option<(DropTarget, Rect4)>,
}

/// The dock host around a fixed centre area.
#[derive(Clone, PartialEq)]
pub struct DockHost {
	layout: Writable<DockLayout>,
	panes: Vec<DockPane>,
	centre: Element,
	labels: DockLabels,
}

impl DockHost {
	pub fn new(layout: impl IntoWritable<DockLayout>, panes: Vec<DockPane>, centre: impl IntoElement) -> Self {
		Self { layout: layout.into_writable(), panes, centre: centre.into_element(), labels: DockLabels::default() }
	}

	pub fn labels(mut self, labels: DockLabels) -> Self {
		self.labels = labels;
		self
	}
}

impl Component for DockHost {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let mut host = use_state(|| (0f32, 0f32));
		let mut host_area = use_state(Area::default);
		let active_group = use_state(|| None::<(Side, usize)>);
		let menu = use_state(|| None::<ContextMenuRequest>);
		let drag = use_state(|| None::<DockDrag>);
		let areas = use_state(HashMap::<(Side, usize), Rect4>::new);
		let layout = self.layout.read().clone();
		let (hw, hh) = *host.read();

		let on_global_move = move |e: Event<PointerEventData>| {
			let mut d = drag;
			let Some(mut current) = d.peek().clone() else { return };
			let p = e.global_location();
			current.at = (p.x, p.y);
			if !current.active
				&& ((p.x - current.start.0).abs() > DRAG_THRESHOLD || (p.y - current.start.1).abs() > DRAG_THRESHOLD)
			{
				current.active = true;
			}
			if current.active {
				let a = *host_area.peek();
				let groups: Vec<((Side, usize), Rect4)> = areas.peek().iter().map(|(k, r)| (*k, *r)).collect();
				let host = (a.min_x(), a.min_y(), a.width(), a.height());
				current.target = drop_target_at(host, &groups, Some(current.from), (p.x as f32, p.y as f32));
			}
			d.set(Some(current));
		};
		let on_global_up = {
			let mut l = self.layout.clone();
			move |_: Event<PointerEventData>| {
				let mut d = drag;
				let Some(current) = d.peek().clone() else { return };
				d.set(None);
				if let (true, Some((target, _))) = (current.active, current.target) {
					let mut next = l.peek().clone();
					next.drop(&current.pane, target);
					*l.write() = next;
				}
			}
		};

		let on_menu = {
			let mut l = self.layout.clone();
			move |id: String| {
				let mut parts = id.splitn(3, ':');
				let (Some("dock"), Some(action), Some(pane)) = (parts.next(), parts.next(), parts.next()) else {
					return;
				};
				let mut next = l.peek().clone();
				match Side::from_word(action) {
					Some(side) => next.move_to(pane, side),
					None => next.hide(pane),
				}
				*l.write() = next;
			}
		};

		let ctx = HostContext {
			theme: theme.clone(),
			layout: self.layout.clone(),
			panes: self.panes.clone(),
			labels: self.labels.clone(),
			active_group,
			menu,
			drag,
			areas,
		};
		let drag_view = drag.read().clone();
		let overlay = drag_view.filter(|d| d.active).map(|d| {
			let a = *host_area.peek();
			let title = self.panes.iter().find(|p| p.id == d.pane).map_or(d.pane.clone(), |p| p.title.clone());
			let ghost = rect()
				.position(
					Position::new_absolute().left(d.at.0 as f32 - a.min_x() + 12.).top(d.at.1 as f32 - a.min_y() + 12.),
				)
				.padding((2., 8., 2., 8.))
				.background(theme.title_active)
				.color(theme.title_active_text)
				.child(label().text(title).max_lines(1));
			let marker = d.target.map(|(_, (x, y, w, h))| {
				rect()
					.position(Position::new_absolute().left(x - a.min_x()).top(y - a.min_y()))
					.width(Size::px(w))
					.height(Size::px(h))
					.background(theme.accent.with_a(0x50))
					.border(Border::new().fill(theme.accent).width(2.).alignment(BorderAlignment::Inner))
			});
			rect()
				.position(Position::new_absolute().left(0.).top(0.))
				.width(Size::fill())
				.height(Size::fill())
				.layer(Layer::Overlay)
				.interactive(Interactive::No)
				.test_id("DockDragOverlay")
				.maybe_child(marker)
				.child(ghost)
		});
		let side_view = |side: Side| -> Option<Element> {
			let dock = layout.side(side);
			(!dock.groups.is_empty()).then(|| side_view(&ctx, side, dock, if side.is_vertical() { hh } else { hw }))
		};
		let splitter_for = |side: Side, max: f32| {
			let mut l = self.layout.clone();
			let sign = if matches!(side, Side::Left | Side::Top) { 1. } else { -1. };
			Splitter::new(!side.is_vertical(), move |delta: f32| l.write().resize_side(side, delta * sign, max))
				.test_id(format!("DockSplitter:{}", side.word()))
		};

		let left = side_view(Side::Left);
		let right = side_view(Side::Right);
		let top = side_view(Side::Top);
		let bottom = side_view(Side::Bottom);
		let middle = rect()
			.width(Size::fill())
			.height(Size::flex(1.))
			.horizontal()
			.content(Content::flex())
			.maybe_child(left.map(|v| sized(v, layout.left.size, true)))
			.maybe_child(layout.left.groups.first().map(|_| splitter_for(Side::Left, hw - 2. * MIN_SIZE)))
			.child(rect().width(Size::flex(1.)).height(Size::fill()).child(self.centre.clone()))
			.maybe_child(layout.right.groups.first().map(|_| splitter_for(Side::Right, hw - 2. * MIN_SIZE)))
			.maybe_child(right.map(|v| sized(v, layout.right.size, true)));

		rect()
			.expanded()
			.content(Content::flex())
			.background(theme.window_background)
			.color(theme.text)
			.font_size(theme.font_size)
			.on_sized(move |e: Event<SizedEventData>| {
				host.set_if_modified((e.area.width(), e.area.height()));
				host_area.set_if_modified(e.area);
			})
			.on_global_pointer_move(on_global_move)
			.on_global_pointer_up(on_global_up)
			.maybe_child(top.map(|v| sized(v, layout.top.size, false)))
			.maybe_child(layout.top.groups.first().map(|_| splitter_for(Side::Top, hh - 2. * MIN_SIZE)))
			.child(middle)
			.maybe_child(layout.bottom.groups.first().map(|_| splitter_for(Side::Bottom, hh - 2. * MIN_SIZE)))
			.maybe_child(bottom.map(|v| sized(v, layout.bottom.size, false)))
			.maybe_child(overlay)
			.child(ContextMenuHost::new(menu, on_menu))
	}
}

fn sized(view: Element, size: f32, horizontal: bool) -> Element {
	let r = rect();
	let r = if horizontal {
		r.width(Size::px(size)).height(Size::fill())
	} else {
		r.height(Size::px(size)).width(Size::fill())
	};
	r.child(view).into_element()
}

#[derive(Clone)]
struct HostContext {
	theme: Theme,
	layout: Writable<DockLayout>,
	panes: Vec<DockPane>,
	labels: DockLabels,
	active_group: State<Option<(Side, usize)>>,
	menu: State<Option<ContextMenuRequest>>,
	drag: State<Option<DockDrag>>,
	areas: State<HashMap<(Side, usize), Rect4>>,
}

/// Starts a press that may become a drag of `pane` out of `from`.
fn press_to_drag(mut drag: State<Option<DockDrag>>, pane: String, from: (Side, usize), e: &Event<PointerEventData>) {
	if e.button() != Some(MouseButton::Left) {
		return;
	}
	let p = e.global_location();
	drag.set(Some(DockDrag { pane, from, start: (p.x, p.y), at: (p.x, p.y), active: false, target: None }));
}

fn side_view(ctx: &HostContext, side: Side, dock: &SideDock, length: f32) -> Element {
	let vertical = side.is_vertical();
	let mut children: Vec<Element> = Vec::new();
	let total = dock.groups.len();
	for (g, group) in dock.groups.iter().enumerate() {
		children.push(
			rect()
				.width(if vertical { Size::fill() } else { Size::flex(group.weight) })
				.height(if vertical { Size::flex(group.weight) } else { Size::fill() })
				.child(group_view(ctx, side, g, group))
				.into_element(),
		);
		if g + 1 < total {
			let mut l = ctx.layout.clone();
			children.push(
				Splitter::new(vertical, move |delta: f32| l.write().resize_groups(side, g, delta, length))
					.test_id(format!("DockGroupSplitter:{}:{g}", side.word()))
					.into_element(),
			);
		}
	}
	rect()
		.expanded()
		.content(Content::flex())
		.direction(if vertical { Direction::Vertical } else { Direction::Horizontal })
		.children(children)
		.into_element()
}

fn group_view(ctx: &HostContext, side: Side, index: usize, group: &PaneGroup) -> Element {
	let theme = &ctx.theme;
	let active_id = group.active_pane().to_owned();
	let pane = ctx.panes.iter().find(|p| p.id == active_id);
	let title = pane.map_or(active_id.clone(), |p| p.title.clone());
	let is_active = *ctx.active_group.read() == Some((side, index));
	let (bar_bg, bar_fg) = if is_active {
		(theme.title_active, theme.title_active_text)
	} else {
		(theme.title_inactive, theme.title_inactive_text)
	};
	let mut active_group = ctx.active_group;
	let mut menu = ctx.menu;
	let labels = ctx.labels.clone();
	let dropdown_id = active_id.clone();
	let close_id = active_id.clone();
	let mut layout_close = ctx.layout.clone();
	let drag = ctx.drag;
	let mut areas = ctx.areas;
	let drag_id = active_id.clone();

	let title_bar = rect()
		.width(Size::fill())
		.height(Size::px(theme.row_height))
		.horizontal()
		.content(Content::flex())
		.cross_align(Alignment::center())
		.padding((0., 2., 0., 6.))
		.background(bar_bg)
		.color(bar_fg)
		.on_pointer_down(move |e: Event<PointerEventData>| press_to_drag(drag, drag_id.clone(), (side, index), &e))
		.test_id(format!("DockTitle:{active_id}"))
		.child(label().text(title).max_lines(1).text_overflow(TextOverflow::Ellipsis).width(Size::flex(1.)))
		.child(title_button(icons::DROPDOWN, bar_fg, labels.position.clone(), move |at: (f64, f64)| {
			let item = |action: &str, text: &str| -> MenuEntry {
				MenuItem::new(format!("dock:{action}:{dropdown_id}"), text).into()
			};
			let mut entries: Vec<MenuEntry> =
				Side::ALL.iter().zip(labels.move_to.iter()).map(|(s, t)| item(s.word(), t)).collect();
			entries.push(MenuEntry::Separator);
			entries.push(item("close", &labels.close));
			menu.set(Some(ContextMenuRequest { at, model: MenuModel::new(entries) }));
		}))
		.child(title_button(icons::CLOSE, bar_fg, ctx.labels.close.replace('_', ""), move |_| {
			layout_close.write().hide(&close_id);
		}));

	let tabs = (group.panes.len() > 1).then(|| {
		let tabs = group.panes.iter().enumerate().map(|(i, id)| {
			let title = ctx.panes.iter().find(|p| &p.id == id).map_or(id.clone(), |p| p.title.clone());
			let selected = i == group.active;
			let mut l = ctx.layout.clone();
			let id = id.clone();
			rect()
				.height(Size::px(theme.row_height))
				.padding((0., 10., 0., 10.))
				.main_align(Alignment::center())
				.background(if selected { theme.tab_active } else { theme.tab_background })
				.color(if selected { theme.text } else { theme.text_dim })
				.test_id(format!("DockTab:{id}"))
				.on_pointer_down(move |e: Event<PointerEventData>| {
					l.write().activate(&id);
					press_to_drag(drag, id.clone(), (side, index), &e);
				})
				.child(label().text(title).max_lines(1))
				.into_element()
		});
		rect().width(Size::fill()).horizontal().background(theme.tab_background).children(tabs)
	});

	rect()
		.expanded()
		.content(Content::flex())
		.background(theme.tool_window_background)
		.border(Border::new().fill(theme.border).width(1.).alignment(BorderAlignment::Inner))
		.test_id(format!("DockPane:{active_id}"))
		.on_sized(move |e: Event<SizedEventData>| {
			let a = e.area;
			let r = (a.min_x(), a.min_y(), a.width(), a.height());
			if areas.peek().get(&(side, index)) != Some(&r) {
				areas.write().insert((side, index), r);
			}
		})
		.on_pointer_down(move |_| active_group.set_if_modified(Some((side, index))))
		.child(title_bar)
		.child(rect().width(Size::fill()).height(Size::flex(1.)).maybe_child(pane.map(|p| p.content.clone())))
		.maybe_child(tabs)
		.into_element()
}

fn title_button(
	icon: crate::icon::Icon,
	color: Color,
	tooltip: String,
	on_press: impl FnMut((f64, f64)) + 'static,
) -> Element {
	TitleButton { icon, color, tooltip: tooltip.replace('_', ""), on_press: EventHandler::new(on_press) }.into_element()
}

/// A flat glyph button reporting its bottom-left corner (window logical pixels) when pressed.
#[derive(Clone, PartialEq)]
struct TitleButton {
	icon: crate::icon::Icon,
	color: Color,
	tooltip: String,
	on_press: EventHandler<(f64, f64)>,
}

impl Component for TitleButton {
	fn render(&self) -> impl IntoElement {
		let mut area = use_state(Area::default);
		let on_press = self.on_press.clone();
		TooltipContainer::new(Tooltip::new_text(self.tooltip.clone())).child(
			Button::new()
				.flat()
				.compact()
				.on_press(move |_| {
					let a = *area.peek();
					on_press.call((f64::from(a.min_x()), f64::from(a.max_y())));
				})
				.child(
					rect()
						.on_sized(move |e: Event<SizedEventData>| area.set_if_modified(e.area))
						.child(IconView::new(self.icon.clone()).size(12.).color(self.color)),
				),
		)
	}
}

/// A 4 px splitter; drag or arrow keys (10 px) report deltas along its axis.
#[derive(Clone, PartialEq)]
pub struct Splitter {
	/// Moves along y (a horizontal bar between stacked cells).
	vertical_drag: bool,
	on_drag: EventHandler<f32>,
	test_id: Option<String>,
}

impl Splitter {
	pub fn new(vertical_drag: bool, on_drag: impl FnMut(f32) + 'static) -> Self {
		Self { vertical_drag, on_drag: EventHandler::new(on_drag), test_id: None }
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = Some(id.into());
		self
	}
}

impl Component for Splitter {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let mut last = use_state(|| None::<f64>);
		let vertical = self.vertical_drag;
		let on_drag = self.on_drag.clone();
		let on_drag_key = self.on_drag.clone();
		rect()
			.a11y_id(id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Splitter)
			.maybe_test_id(self.test_id.clone())
			.width(if vertical { Size::fill() } else { Size::px(SPLITTER) })
			.height(if vertical { Size::px(SPLITTER) } else { Size::fill() })
			.background(if focus().is_focused() { theme.accent.with_a(0x80) } else { theme.splitter })
			.border(focus_border(focus(), theme.accent))
			.cursor(if vertical { CursorIcon::RowResize } else { CursorIcon::ColResize })
			.on_pointer_down(move |e: Event<PointerEventData>| {
				id.request_focus();
				let p = e.global_location();
				last.set(Some(if vertical { p.y } else { p.x }));
			})
			.on_global_pointer_move(move |e: Event<PointerEventData>| {
				let Some(prev) = *last.peek() else { return };
				let p = e.global_location();
				let now = if vertical { p.y } else { p.x };
				if now != prev {
					last.set(Some(now));
					on_drag.call((now - prev) as f32);
				}
			})
			.on_global_pointer_up(move |_| last.set_if_modified(None))
			.on_key_down(move |e: Event<KeyboardEventData>| {
				let delta = match (&e.key, vertical) {
					(Key::Named(NamedKey::ArrowUp), true) | (Key::Named(NamedKey::ArrowLeft), false) => -10.,
					(Key::Named(NamedKey::ArrowDown), true) | (Key::Named(NamedKey::ArrowRight), false) => 10.,
					_ => return,
				};
				e.stop_propagation();
				on_drag_key.call(delta);
			})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn first_start() -> DockLayout {
		let mut l = DockLayout::default();
		l.left.groups = vec![PaneGroup::new(vec!["project".into()], 1.), PaneGroup::new(vec!["properties".into()], 2.)];
		l.hidden.push(("messages".into(), Side::Bottom));
		l
	}

	#[test]
	fn show_hide_and_remember_the_side() {
		let mut l = first_start();
		assert!(!l.is_shown("messages"));
		l.show("messages", Side::Right);
		assert_eq!(l.find("messages"), Some((Side::Bottom, 0, 0)));
		l.move_to("project", Side::Right);
		l.hide("project");
		assert_eq!(l.left.groups.len(), 1);
		assert!(l.right.groups.is_empty());
		l.show("project", Side::Left);
		assert_eq!(l.find("project"), Some((Side::Right, 0, 0)));
		l.toggle("project", Side::Left);
		assert!(!l.is_shown("project"));
	}

	#[test]
	fn tabs_and_activation() {
		let mut l = first_start();
		l.tab_into("project", Side::Left, 1);
		assert_eq!(l.left.groups.len(), 1);
		assert_eq!(l.left.groups[0].panes, vec!["properties", "project"]);
		assert_eq!(l.left.groups[0].active_pane(), "project");
		l.activate("properties");
		assert_eq!(l.left.groups[0].active_pane(), "properties");
		l.hide("properties");
		assert_eq!(l.left.groups[0].active_pane(), "project");
	}

	#[test]
	fn drops_land_as_tabs_splits_and_edges() {
		let mut l = first_start();
		l.drop("project", DropTarget::After(Side::Left, 1));
		assert_eq!(l.left.groups.iter().map(|g| g.panes[0].as_str()).collect::<Vec<_>>(), ["properties", "project"]);
		l.drop("project", DropTarget::Before(Side::Left, 0));
		assert_eq!(l.left.groups[0].panes, ["project"]);
		l.drop("project", DropTarget::Tab(Side::Left, 1));
		assert_eq!(l.left.groups.len(), 1);
		assert_eq!(l.left.groups[0].active_pane(), "project");
		l.drop("properties", DropTarget::Edge(Side::Bottom));
		assert_eq!(l.find("properties"), Some((Side::Bottom, 0, 0)));
		assert_eq!(l.left.groups[0].panes, ["project"]);
	}

	#[test]
	fn the_target_under_the_pointer_is_an_end_the_middle_or_an_edge() {
		let host = (0., 0., 1000., 800.);
		let groups = [((Side::Left, 0), (0., 0., 300., 400.)), ((Side::Left, 1), (0., 400., 300., 400.))];
		let at = |p| drop_target_at(host, &groups, Some((Side::Left, 0)), p).map(|t| t.0);
		assert_eq!(at((100., 10.)), None, "a pane never targets its own group");
		assert_eq!(at((100., 410.)), Some(DropTarget::Before(Side::Left, 1)));
		assert_eq!(at((100., 600.)), Some(DropTarget::Tab(Side::Left, 1)));
		assert_eq!(at((100., 790.)), Some(DropTarget::After(Side::Left, 1)));
		assert_eq!(at((990., 300.)), Some(DropTarget::Edge(Side::Right)));
		assert_eq!(at((500., 790.)), Some(DropTarget::Edge(Side::Bottom)));
		assert_eq!(at((500., 300.)), None);
	}

	#[test]
	fn resizing_keeps_minimums() {
		let mut l = first_start();
		l.resize_side(Side::Left, 100., 1000.);
		assert_eq!(l.left.size, 420.);
		l.resize_side(Side::Left, -1000., 1000.);
		assert_eq!(l.left.size, MIN_SIZE);
		l.resize_groups(Side::Left, 0, 100., 300.);
		assert!((l.left.groups[0].weight - 2.).abs() < 1e-5);
		assert!((l.left.groups[1].weight - 1.).abs() < 1e-5);
		l.resize_groups(Side::Left, 0, 1000., 300.);
		assert!((l.left.groups[1].weight - 0.6).abs() < 1e-5);
	}

	#[test]
	fn unknown_ids_are_dropped_and_the_layout_round_trips() {
		let mut l = first_start();
		l.left.groups[0].panes.push("gone".into());
		l.hidden.push(("old".into(), Side::Top));
		l.retain_known(&["project", "properties", "messages"]);
		assert_eq!(l, first_start());
		let json = serde_json::to_string(&l).unwrap();
		assert_eq!(serde_json::from_str::<DockLayout>(&json).unwrap(), l);
	}
}
