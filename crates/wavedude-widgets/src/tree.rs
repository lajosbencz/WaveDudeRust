//! Project tree: hierarchical 23 px rows with check box, icon and name; current item plus a shared
//! multi-selection; keyboard model; inline rename; drag reorder with a drop indicator.

use std::fmt::Debug;
use std::hash::Hash;

use freya::prelude::*;

use crate::icon::{Icon, IconView, icons};
use crate::text_box::TextBox;
use crate::theme::{Theme, UiFontExt, use_wave_theme};
use crate::tooltip::Tip;
use crate::util::{TestIdExt, focus_border, use_modifiers};

/// Kind of a row; only [`RowKind::Item`] rows join a multi-selection and reorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowKind {
	Root,
	Settings,
	Folder,
	Item,
}

/// One row in tree order (pre-order). `depth` 0 is the root level.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow<K> {
	pub key: K,
	pub depth: u16,
	pub label: String,
	pub icon: Icon,
	pub kind: RowKind,
	/// The Enabled/Active check box; `None` hides it.
	pub enabled: Option<bool>,
	/// The list an item reorders within, or the list a folder holds.
	pub group: Option<u32>,
	/// Folder rows: a plus button at the right.
	pub can_add: bool,
	pub test_id: Option<String>,
	/// Tooltips of the check box and the plus button.
	pub enabled_tip: Option<String>,
	pub add_tip: Option<String>,
}

impl<K> TreeRow<K> {
	pub fn new(key: K, depth: u16, kind: RowKind, label: impl Into<String>) -> Self {
		Self {
			key,
			depth,
			label: label.into(),
			icon: Icon::None,
			kind,
			enabled: None,
			group: None,
			can_add: false,
			test_id: None,
			enabled_tip: None,
			add_tip: None,
		}
	}

	pub fn enabled_tip(mut self, tip: impl Into<String>) -> Self {
		self.enabled_tip = Some(tip.into());
		self
	}

	pub fn add_tip(mut self, tip: impl Into<String>) -> Self {
		self.add_tip = Some(tip.into());
		self
	}

	pub fn icon(mut self, icon: Icon) -> Self {
		self.icon = icon;
		self
	}

	pub fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = Some(enabled);
		self
	}

	pub fn group(mut self, group: u32) -> Self {
		self.group = Some(group);
		self
	}

	pub fn can_add(mut self) -> Self {
		self.can_add = true;
		self
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = Some(id.into());
		self
	}
}

/// The current item plus the shared multi-selection; the current item is always a member.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeSelection<K> {
	pub current: Option<K>,
	pub selected: Vec<K>,
	pub anchor: Option<K>,
}

impl<K> Default for TreeSelection<K> {
	fn default() -> Self {
		Self { current: None, selected: Vec::new(), anchor: None }
	}
}

impl<K: Clone + PartialEq> TreeSelection<K> {
	pub fn is_selected(&self, key: &K) -> bool {
		self.selected.contains(key)
	}

	/// That row alone; it becomes the anchor.
	pub fn select(&mut self, key: K) {
		self.current = Some(key.clone());
		self.selected = vec![key.clone()];
		self.anchor = Some(key);
	}

	pub fn clear(&mut self) {
		*self = Self::default();
	}

	/// Ctrl+click, Insert, Ctrl+Space: toggles a multi-selectable row; others are selected alone.
	pub fn toggle(&mut self, key: K, multi: bool) {
		let current_multi = self.current.is_none() || self.selected.len() > 1 || multi;
		if !multi || !current_multi {
			self.select(key);
			return;
		}
		if let Some(i) = self.selected.iter().position(|k| *k == key) {
			self.selected.remove(i);
			if self.current.as_ref() == Some(&key) {
				self.current = self.selected.last().cloned();
			}
		} else {
			self.selected.retain(|_| true);
			self.selected.push(key.clone());
			self.current = Some(key.clone());
			self.anchor = Some(key);
		}
	}

	/// Shift+click, Shift+arrows: the range from the anchor to `key` among `items` (multi-selectable rows
	/// in tree order); a plain selection when either end is not among them.
	pub fn extend_to(&mut self, key: K, items: &[K]) {
		let (Some(anchor), Some(to)) = (self.anchor.clone(), items.iter().position(|k| *k == key)) else {
			self.select(key);
			return;
		};
		let Some(from) = items.iter().position(|k| *k == anchor) else {
			self.select(key);
			return;
		};
		let (a, b) = if from <= to { (from, to) } else { (to, from) };
		self.selected = items[a..=b].to_vec();
		self.current = Some(key);
	}

	/// Ctrl+A: every item; the last becomes current.
	pub fn select_all(&mut self, items: &[K]) {
		if items.is_empty() {
			return;
		}
		self.selected = items.to_vec();
		self.current = items.last().cloned();
		self.anchor = items.first().cloned();
	}

	/// Explorer rule: a command on one of several selected rows acts on the selection, else on the row.
	pub fn targets(&self, key: &K) -> Vec<K> {
		if self.selected.len() > 1 && self.selected.contains(key) { self.selected.clone() } else { vec![key.clone()] }
	}
}

/// Tree state the app owns: selection, collapsed rows, the row being renamed.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeState<K> {
	pub selection: TreeSelection<K>,
	pub collapsed: Vec<K>,
	pub renaming: Option<K>,
}

impl<K> Default for TreeState<K> {
	fn default() -> Self {
		Self { selection: TreeSelection::default(), collapsed: Vec::new(), renaming: None }
	}
}

/// Where a dragged item lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropPosition<K> {
	Before(K),
	After(K),
	/// At the end of the list a folder holds.
	Append(u32),
}

/// What the tree asks the app to do. Selection changes are written to the state directly.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeAction<K> {
	ToggleEnabled(Vec<K>),
	Activate(K),
	Rename(K, String),
	Delete(Vec<K>),
	Duplicate(Vec<K>),
	MoveUp(Vec<K>),
	MoveDown(Vec<K>),
	/// A folder's plus button.
	Add(K),
	Drop {
		dragged: K,
		position: DropPosition<K>,
	},
	/// A row dragged out of the tree and released at `at` (window logical pixels).
	DropOutside {
		dragged: K,
		at: (f64, f64),
	},
	/// Right click, Apps key or Shift+F10; `at` in window logical pixels.
	ContextMenu {
		key: Option<K>,
		at: (f64, f64),
	},
}

/// Indices of the rows not hidden under a collapsed ancestor.
pub fn visible_rows<K: PartialEq>(rows: &[TreeRow<K>], collapsed: &[K]) -> Vec<usize> {
	let mut out = Vec::with_capacity(rows.len());
	let mut hide_below: Option<u16> = None;
	for (i, row) in rows.iter().enumerate() {
		if let Some(depth) = hide_below {
			if row.depth > depth {
				continue;
			}
			hide_below = None;
		}
		out.push(i);
		if collapsed.contains(&row.key) {
			hide_below = Some(row.depth);
		}
	}
	out
}

fn has_children<K>(rows: &[TreeRow<K>], index: usize) -> bool {
	rows.get(index + 1).is_some_and(|next| next.depth > rows[index].depth)
}

/// The drop position for a pointer over `rows[target]` (upper half: before), or `None` when the drop is
/// not taken or would move nothing.
pub fn drop_target<K: Clone + PartialEq>(
	rows: &[TreeRow<K>],
	dragged: &K,
	target: usize,
	upper_half: bool,
) -> Option<DropPosition<K>> {
	let from = rows.iter().position(|r| r.key == *dragged)?;
	let source = &rows[from];
	let group = source.group.filter(|_| source.kind == RowKind::Item)?;
	let row = rows.get(target)?;
	let siblings: Vec<&K> =
		rows.iter().filter(|r| r.kind == RowKind::Item && r.group == Some(group)).map(|r| &r.key).collect();
	let at = siblings.iter().position(|k| *k == dragged)?;
	match row.kind {
		RowKind::Item if row.group == Some(group) => {
			let t = siblings.iter().position(|k| **k == row.key)?;
			if upper_half {
				(t != at && t != at + 1).then(|| DropPosition::Before(row.key.clone()))
			} else {
				(t != at && t + 1 != at).then(|| DropPosition::After(row.key.clone()))
			}
		}
		RowKind::Folder if row.group == Some(group) => {
			(at + 1 != siblings.len()).then_some(DropPosition::Append(group))
		}
		_ => None,
	}
}

/// The index `dragged` ends at in `list` after a drop at `position` (the list without the move applied).
pub fn reorder_index<K: PartialEq>(list: &[K], dragged: &K, position: &DropPosition<K>) -> Option<usize> {
	let from = list.iter().position(|k| k == dragged)?;
	let insert = match position {
		DropPosition::Before(k) => list.iter().position(|x| x == k)?,
		DropPosition::After(k) => list.iter().position(|x| x == k)? + 1,
		DropPosition::Append(_) => list.len(),
	};
	Some(if insert > from { insert - 1 } else { insert })
}

/// Result of a key press on the tree.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyOutcome<K> {
	Ignored,
	Handled,
	Action(TreeAction<K>),
}

/// The tree's keyboard model, pure. `at` is where a keyboard context menu opens.
pub fn handle_key<K: Clone + PartialEq>(
	state: &mut TreeState<K>,
	rows: &[TreeRow<K>],
	key: &Key,
	modifiers: Modifiers,
	at: (f64, f64),
) -> KeyOutcome<K> {
	let visible = visible_rows(rows, &state.collapsed);
	let items: Vec<K> =
		visible.iter().map(|&i| &rows[i]).filter(|r| r.kind == RowKind::Item).map(|r| r.key.clone()).collect();
	let ctrl = crate::util::command(modifiers);
	let shift = modifiers.contains(Modifiers::SHIFT);
	let alt = modifiers.contains(Modifiers::ALT);
	let cursor = state.selection.current.as_ref().and_then(|c| visible.iter().position(|&i| rows[i].key == *c));
	let row_at = |pos: usize| &rows[visible[pos]];
	let current_row = cursor.map(row_at);
	let go = |state: &mut TreeState<K>, pos: usize| {
		let row = row_at(pos);
		if shift && row.kind == RowKind::Item {
			state.selection.extend_to(row.key.clone(), &items);
		} else {
			state.selection.select(row.key.clone());
		}
		KeyOutcome::Handled
	};
	let last = visible.len().saturating_sub(1);
	match key {
		Key::Named(NamedKey::ArrowUp) if alt => current_row
			.map_or(KeyOutcome::Ignored, |r| KeyOutcome::Action(TreeAction::MoveUp(state.selection.targets(&r.key)))),
		Key::Named(NamedKey::ArrowDown) if alt => current_row
			.map_or(KeyOutcome::Ignored, |r| KeyOutcome::Action(TreeAction::MoveDown(state.selection.targets(&r.key)))),
		Key::Named(NamedKey::ArrowUp) if !visible.is_empty() => go(state, cursor.map_or(0, |c| c.saturating_sub(1))),
		Key::Named(NamedKey::ArrowDown) if !visible.is_empty() => go(state, cursor.map_or(0, |c| (c + 1).min(last))),
		Key::Named(NamedKey::Home) if !visible.is_empty() => go(state, 0),
		Key::Named(NamedKey::End) if !visible.is_empty() => go(state, last),
		Key::Named(NamedKey::ArrowLeft) => {
			let Some(c) = cursor else { return KeyOutcome::Ignored };
			let row = row_at(c);
			if has_children(rows, visible[c]) && !state.collapsed.contains(&row.key) {
				state.collapsed.push(row.key.clone());
			} else if let Some(parent) = (0..c).rev().find(|&p| row_at(p).depth < row.depth) {
				state.selection.select(row_at(parent).key.clone());
			}
			KeyOutcome::Handled
		}
		Key::Named(NamedKey::ArrowRight) => {
			let Some(c) = cursor else { return KeyOutcome::Ignored };
			let key = row_at(c).key.clone();
			state.collapsed.retain(|k| *k != key);
			KeyOutcome::Handled
		}
		Key::Character(c) if c.eq_ignore_ascii_case("a") && ctrl => {
			state.selection.select_all(&items);
			KeyOutcome::Handled
		}
		Key::Character(c) if c.eq_ignore_ascii_case("d") && ctrl => {
			current_row.filter(|r| r.kind == RowKind::Item).map_or(KeyOutcome::Ignored, |r| {
				KeyOutcome::Action(TreeAction::Duplicate(state.selection.targets(&r.key)))
			})
		}
		Key::Named(NamedKey::Insert) => toggle_cursor(state, current_row),
		Key::Character(c) if c == " " && ctrl => toggle_cursor(state, current_row),
		Key::Character(c) if c == " " => current_row.filter(|r| r.enabled.is_some()).map_or(KeyOutcome::Ignored, |r| {
			KeyOutcome::Action(TreeAction::ToggleEnabled(state.selection.targets(&r.key)))
		}),
		Key::Named(NamedKey::F2) => match current_row.filter(|r| r.kind == RowKind::Item) {
			Some(r) => {
				state.renaming = Some(r.key.clone());
				KeyOutcome::Handled
			}
			None => KeyOutcome::Ignored,
		},
		Key::Named(NamedKey::Enter) => {
			current_row.map_or(KeyOutcome::Ignored, |r| KeyOutcome::Action(TreeAction::Activate(r.key.clone())))
		}
		Key::Named(NamedKey::Delete) => current_row
			.filter(|r| r.kind == RowKind::Item)
			.map_or(KeyOutcome::Ignored, |r| KeyOutcome::Action(TreeAction::Delete(state.selection.targets(&r.key)))),
		Key::Named(NamedKey::ContextMenu) => {
			KeyOutcome::Action(TreeAction::ContextMenu { key: current_row.map(|r| r.key.clone()), at })
		}
		Key::Named(NamedKey::F10) if shift => {
			KeyOutcome::Action(TreeAction::ContextMenu { key: current_row.map(|r| r.key.clone()), at })
		}
		_ => KeyOutcome::Ignored,
	}
}

fn toggle_cursor<K: Clone + PartialEq>(state: &mut TreeState<K>, row: Option<&TreeRow<K>>) -> KeyOutcome<K> {
	match row {
		Some(r) => {
			state.selection.toggle(r.key.clone(), r.kind == RowKind::Item);
			KeyOutcome::Handled
		}
		None => KeyOutcome::Ignored,
	}
}

const DRAG_THRESHOLD: f64 = 4.;
const INDENT: f32 = 16.;

#[derive(Clone, PartialEq)]
struct DragState<K> {
	key: K,
	start: (f64, f64),
	active: bool,
	target: Option<(usize, DropPosition<K>)>,
}

/// The project tree view. Rows come in tree order; the state is shared with the app.
#[derive(Clone, PartialEq)]
pub struct TreeView<K: 'static> {
	rows: Vec<TreeRow<K>>,
	state: Writable<TreeState<K>>,
	on_action: EventHandler<TreeAction<K>>,
	test_id: Option<String>,
	a11y_id: Option<AccessibilityId>,
}

impl<K: Clone + PartialEq + 'static> TreeView<K> {
	pub fn new(
		rows: Vec<TreeRow<K>>,
		state: impl IntoWritable<TreeState<K>>,
		on_action: impl Into<EventHandler<TreeAction<K>>>,
	) -> Self {
		Self { rows, state: state.into_writable(), on_action: on_action.into(), test_id: None, a11y_id: None }
	}

	/// The tree's id, for giving it the keyboard focus from outside (`id.request_focus()`).
	pub fn a11y_id(mut self, id: AccessibilityId) -> Self {
		self.a11y_id = Some(id);
		self
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = Some(id.into());
		self
	}
}

impl<K: Clone + PartialEq + Debug + 'static> Component for TreeView<K> {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let own_id = use_a11y();
		let tree_id = self.a11y_id.unwrap_or(own_id);
		let focus = use_focus(tree_id);
		let modifiers = use_modifiers();
		let mut list_area = use_state(Area::default);
		let mut scroll = use_scroll_controller(ScrollConfig::default);
		let mut drag = use_state(|| None::<DragState<K>>);
		let mut last_current = use_state(|| None::<K>);
		let rows = self.rows.clone();
		let state = self.state.clone();
		let on_action = self.on_action.clone();
		let row_h = theme.row_height;

		let snapshot = state.read().clone();
		let visible = visible_rows(&rows, &snapshot.collapsed);

		if *last_current.peek() != snapshot.selection.current {
			last_current.set(snapshot.selection.current.clone());
			if let Some(pos) =
				snapshot.selection.current.as_ref().and_then(|c| visible.iter().position(|&i| rows[i].key == *c))
			{
				let top = pos as f32 * row_h;
				let offset = -scroll.position().y;
				let viewport = list_area.peek().height();
				if top < offset {
					scroll.scroll_to_y(-(top as i32));
				} else if viewport > 0. && top + row_h > offset + viewport {
					scroll.scroll_to_y(-((top + row_h - viewport) as i32));
				}
			}
		}

		let on_key_down = {
			let rows = rows.clone();
			let on_action = on_action.clone();
			let mut state = state.clone();
			move |e: Event<KeyboardEventData>| {
				if state.peek().renaming.is_some() {
					return;
				}
				let a = *list_area.peek();
				let mut next = state.peek().clone();
				let current_y = next
					.selection
					.current
					.as_ref()
					.and_then(|c| visible_rows(&rows, &next.collapsed).iter().position(|&i| rows[i].key == *c))
					.map_or(0., |p| p as f32 * row_h);
				let at = (f64::from(a.min_x() + 24.), f64::from(a.min_y() + current_y + scroll.position().y + row_h));
				let outcome = handle_key(&mut next, &rows, &e.key, e.modifiers, at);
				if outcome == KeyOutcome::Ignored {
					return;
				}
				e.stop_propagation();
				e.prevent_default();
				if *state.peek() != next {
					*state.write() = next;
				}
				if let KeyOutcome::Action(action) = outcome {
					on_action.call(action);
				}
			}
		};

		let on_global_move = {
			let rows = rows.clone();
			let state = state.clone();
			move |e: Event<PointerEventData>| {
				let Some(mut d) = drag.peek().clone() else { return };
				let p = e.global_location();
				if !d.active && ((p.x - d.start.0).abs() > DRAG_THRESHOLD || (p.y - d.start.1).abs() > DRAG_THRESHOLD) {
					d.active = true;
				}
				if d.active {
					let a = *list_area.peek();
					let y = (p.y as f32 - a.min_y()) - scroll.position().y;
					let visible = visible_rows(&rows, &state.peek().collapsed);
					d.target = (y >= 0.).then(|| (y / row_h) as usize).filter(|&v| v < visible.len()).and_then(|v| {
						let upper = (y / row_h).fract() < 0.5;
						drop_target(&rows, &d.key, visible[v], upper).map(|t| (v, t))
					});
				}
				drag.set(Some(d));
			}
		};

		let on_global_up = {
			let on_action = on_action.clone();
			move |e: Event<PointerEventData>| {
				let Some(d) = drag.peek().clone() else { return };
				drag.set(None);
				if !d.active {
					return;
				}
				let p = e.global_location();
				let a = list_area.peek().to_f64();
				let inside = p.x >= a.min_x() && p.x < a.max_x() && p.y >= a.min_y() && p.y < a.max_y();
				match d.target {
					Some((_, position)) => on_action.call(TreeAction::Drop { dragged: d.key, position }),
					None if !inside => on_action.call(TreeAction::DropOutside { dragged: d.key, at: (p.x, p.y) }),
					None => {}
				}
			}
		};

		let items: Vec<K> =
			visible.iter().map(|&i| &rows[i]).filter(|r| r.kind == RowKind::Item).map(|r| r.key.clone()).collect();
		let focused = focus().is_focused();
		let drag_view = drag.read().clone();

		let row_views = visible.iter().enumerate().map(|(pos, &index)| {
			let row = rows[index].clone();
			let expandable = has_children(&rows, index);
			let collapsed = snapshot.collapsed.contains(&row.key);
			let is_current = snapshot.selection.current.as_ref() == Some(&row.key);
			let is_selected = snapshot.selection.is_selected(&row.key);
			let renaming = snapshot.renaming.as_ref() == Some(&row.key);
			let indicator =
				drag_view.as_ref().and_then(|d| d.target.as_ref()).filter(|(v, _)| *v == pos).map(|(_, t)| t.clone());
			let ctx = RowContext {
				theme: theme.clone(),
				row,
				expandable,
				collapsed,
				is_current,
				is_selected,
				focused,
				renaming,
				indicator,
				items: items.clone(),
			};
			row_view(ctx, state.clone(), on_action.clone(), drag, modifiers, tree_id)
		});

		let body = rect()
			.a11y_id(tree_id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Tree)
			.maybe_test_id(self.test_id.clone())
			.expanded()
			.background(theme.tool_window_background)
			.color(theme.text)
			.ui_font(&theme)
			.border(focus_border(focus(), theme.accent))
			.on_key_down(on_key_down)
			.on_global_pointer_move(on_global_move)
			.on_global_pointer_up(on_global_up)
			.on_sized(move |e: Event<SizedEventData>| list_area.set_if_modified(e.area))
			.on_secondary_down({
				let on_action = on_action.clone();
				move |e: Event<PressEventData>| {
					if let PressEventData::Mouse(m) = e.data() {
						on_action.call(TreeAction::ContextMenu {
							key: None,
							at: (m.global_location.x, m.global_location.y),
						});
					}
				}
			})
			.child(ScrollView::new_controlled(scroll).scroll_with_arrows(false).expanded().children(row_views));
		crate::util::modifier_tracker(modifiers, body).expanded()
	}
}

struct RowContext<K> {
	theme: Theme,
	row: TreeRow<K>,
	expandable: bool,
	collapsed: bool,
	is_current: bool,
	is_selected: bool,
	focused: bool,
	renaming: bool,
	indicator: Option<DropPosition<K>>,
	items: Vec<K>,
}

fn row_view<K: Clone + PartialEq + 'static>(
	ctx: RowContext<K>,
	state: Writable<TreeState<K>>,
	on_action: EventHandler<TreeAction<K>>,
	mut drag: State<Option<DragState<K>>>,
	modifiers: State<Modifiers>,
	tree_id: AccessibilityId,
) -> Element {
	let RowContext { theme, row, expandable, collapsed, is_current, is_selected, focused, renaming, indicator, items } =
		ctx;
	let (background, color) = if is_current && focused {
		(theme.selection, theme.selection_text)
	} else if is_selected {
		(theme.inactive_selection, theme.inactive_selection_text)
	} else {
		(Color::TRANSPARENT, theme.text)
	};
	let dimmed = row.enabled == Some(false);
	let ink = if dimmed { theme.text_disabled } else { color };
	let key = row.key.clone();
	let kind = row.kind;

	let on_secondary = {
		let key = key.clone();
		let on_action = on_action.clone();
		let mut state = state.clone();
		move |e: Event<PointerEventData>| {
			e.stop_propagation();
			tree_id.request_focus();
			if !state.peek().selection.is_selected(&key) {
				state.write().selection.select(key.clone());
			}
			let p = e.global_location();
			on_action.call(TreeAction::ContextMenu { key: Some(key.clone()), at: (p.x, p.y) });
		}
	};

	let on_down = {
		let key = key.clone();
		let mut state = state.clone();
		let mut on_secondary = on_secondary;
		move |e: Event<PointerEventData>| {
			if e.button() == Some(MouseButton::Right) {
				on_secondary(e);
				return;
			}
			if e.button() != Some(MouseButton::Left) {
				return;
			}
			tree_id.request_focus();
			let m = *modifiers.peek();
			let mut next = state.peek().clone();
			next.renaming = None;
			if crate::util::command(m) {
				next.selection.toggle(key.clone(), kind == RowKind::Item);
			} else if m.contains(Modifiers::SHIFT) && kind == RowKind::Item {
				next.selection.extend_to(key.clone(), &items);
			} else if !(next.selection.selected.len() > 1 && next.selection.is_selected(&key)) {
				next.selection.select(key.clone());
			} else {
				next.selection.current = Some(key.clone());
			}
			if *state.peek() != next {
				*state.write() = next;
			}
			let p = e.global_location();
			drag.set(Some(DragState { key: key.clone(), start: (p.x, p.y), active: false, target: None }));
		}
	};

	let chevron = rect().width(Size::px(16.)).height(Size::px(16.)).center().maybe_child(expandable.then(|| {
		let key = key.clone();
		let mut state = state.clone();
		rect()
			.on_pointer_down(move |e: Event<PointerEventData>| {
				e.stop_propagation();
				let mut s = state.write();
				if let Some(i) = s.collapsed.iter().position(|k| *k == key) {
					s.collapsed.remove(i);
				} else {
					s.collapsed.push(key.clone());
				}
			})
			.child(IconView::new(if collapsed { icons::CHEVRON_RIGHT } else { icons::CHEVRON_DOWN }).color(ink))
	}));

	let check = row.enabled.map(|on| {
		let key = key.clone();
		let on_action = on_action.clone();
		let state = state.clone();
		let mark = rect()
			.width(Size::px(13.))
			.height(Size::px(13.))
			.margin((0., 4., 0., 0.))
			.center()
			.border(Border::new().fill(theme.input_border).width(1.).alignment(BorderAlignment::Inner))
			.background(theme.input_background)
			.maybe(row.test_id.is_some(), |r| r.test_id(format!("Enabled:{}", row.label)))
			.on_pointer_down(move |e: Event<PointerEventData>| {
				e.stop_propagation();
				on_action.call(TreeAction::ToggleEnabled(state.peek().selection.targets(&key)));
			})
			.maybe_child(on.then(|| IconView::new(icons::CHECK).size(11.).color(theme.text)));
		with_tip(row.enabled_tip.clone(), mark)
	});

	let name: Element = if renaming {
		let key = key.clone();
		let on_action = on_action.clone();
		let mut state = state.clone();
		let mut done_state = state.clone();
		TextBox::new(row.label.clone(), move |text: String| {
			state.write().renaming = None;
			tree_id.request_focus();
			on_action.call(TreeAction::Rename(key.clone(), text));
		})
		.auto_focus()
		.select_all_on_focus(true)
		.width(Size::flex(1.))
		.on_done(move |_| {
			done_state.write().renaming = None;
			tree_id.request_focus();
		})
		.into()
	} else {
		label()
			.text(row.label.clone())
			.color(ink)
			.max_lines(1)
			.text_overflow(TextOverflow::Ellipsis)
			.width(Size::flex(1.))
			.into()
	};

	let buttons: Vec<Element> = match row.kind {
		RowKind::Item => [(icons::CHEVRON_UP, 0u8), (icons::CHEVRON_DOWN, 1), (icons::MINUS, 2)]
			.into_iter()
			.map(|(icon, which)| {
				let key = key.clone();
				let on_action = on_action.clone();
				let state = state.clone();
				glyph_button(icon, ink, move || {
					let t = state.peek().selection.targets(&key);
					on_action.call(match which {
						0 => TreeAction::MoveUp(t),
						1 => TreeAction::MoveDown(t),
						_ => TreeAction::Delete(t),
					});
				})
			})
			.collect(),
		RowKind::Folder if row.can_add => {
			let key = key.clone();
			let on_action = on_action.clone();
			let button = glyph_button(icons::PLUS, ink, move || on_action.call(TreeAction::Add(key.clone())));
			vec![with_tip(row.add_tip.clone(), button)]
		}
		_ => Vec::new(),
	};

	let mut outer = rect()
		.width(Size::fill())
		.height(Size::px(theme.row_height))
		.horizontal()
		.content(Content::flex())
		.cross_align(Alignment::center())
		.padding((0., 2., 0., 2. + INDENT * f32::from(row.depth)))
		.background(background)
		.color(ink)
		.on_pointer_down(on_down)
		.maybe_test_id(row.test_id.clone());
	if let Some(DropPosition::Append(_)) = indicator {
		outer = outer.border(Border::new().fill(theme.drop_indicator).width(1.).alignment(BorderAlignment::Inner));
	}
	let line = match indicator {
		Some(DropPosition::Before(_)) => Some(0.),
		Some(DropPosition::After(_)) => Some(theme.row_height - 2.),
		_ => None,
	};
	outer
		.maybe_child(line.map(|top| {
			rect()
				.position(Position::new_absolute().top(top).left(0.))
				.width(Size::fill())
				.height(Size::px(2.))
				.background(theme.drop_indicator)
				.layer(Layer::Relative(1))
		}))
		.child(chevron)
		.maybe_child(check)
		.maybe_child(
			(!row.icon.is_none())
				.then(|| rect().margin((0., 4., 0., 0.)).child(IconView::new(row.icon.clone()).color(ink))),
		)
		.child(name)
		.children(buttons)
		.into()
}

fn with_tip(tip: Option<String>, child: impl IntoElement) -> Element {
	match tip {
		Some(t) => Tip::new(t).child(child).into_element(),
		None => child.into_element(),
	}
}

fn glyph_button(icon: Icon, color: Color, mut on_press: impl FnMut() + 'static) -> Element {
	rect()
		.width(Size::px(18.))
		.height(Size::px(18.))
		.center()
		.on_pointer_down(move |e: Event<PointerEventData>| {
			e.stop_propagation();
			on_press();
		})
		.child(IconView::new(icon).size(12.).color(color))
		.into()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn rows() -> Vec<TreeRow<&'static str>> {
		vec![
			TreeRow::new("project", 0, RowKind::Root, "Project"),
			TreeRow::new("scene", 1, RowKind::Settings, "Scene"),
			TreeRow::new("sources", 1, RowKind::Folder, "Sources").group(0).can_add(),
			TreeRow::new("s1", 2, RowKind::Item, "Source 1").group(0).enabled(true),
			TreeRow::new("s2", 2, RowKind::Item, "Source 2").group(0).enabled(false),
			TreeRow::new("s3", 2, RowKind::Item, "Source 3").group(0).enabled(true),
			TreeRow::new("probes", 1, RowKind::Folder, "Probes").group(1),
			TreeRow::new("p1", 2, RowKind::Item, "Probe 1").group(1).enabled(true),
		]
	}

	fn items() -> Vec<&'static str> {
		vec!["s1", "s2", "s3", "p1"]
	}

	#[test]
	fn selection_click_ctrl_shift() {
		let mut s = TreeSelection::default();
		s.select("s1");
		s.toggle("s3", true);
		assert_eq!(s.selected, vec!["s1", "s3"]);
		assert_eq!(s.current, Some("s3"));
		s.toggle("s3", true);
		assert_eq!(s.selected, vec!["s1"]);
		assert_eq!(s.current, Some("s1"));
		s.select("s2");
		s.extend_to("p1", &items());
		assert_eq!(s.selected, vec!["s2", "s3", "p1"]);
		assert_eq!(s.current, Some("p1"));
		s.extend_to("s1", &items());
		assert_eq!(s.selected, vec!["s1", "s2"]);
		s.toggle("scene", false);
		assert_eq!(s.selected, vec!["scene"]);
		s.select_all(&items());
		assert_eq!(s.current, Some("p1"));
		assert_eq!(s.targets(&"s2").len(), 4);
		s.select("s2");
		assert_eq!(s.targets(&"s3"), vec!["s3"]);
	}

	#[test]
	fn collapsed_rows_hide_descendants() {
		let r = rows();
		assert_eq!(visible_rows(&r, &[]).len(), 8);
		assert_eq!(visible_rows(&r, &["sources"]), vec![0, 1, 2, 6, 7]);
		assert_eq!(visible_rows(&r, &["project"]), vec![0]);
	}

	#[test]
	fn keys_move_extend_and_act() {
		let r = rows();
		let mut st = TreeState::default();
		st.selection.select("s1");
		let none = Modifiers::empty();
		assert_eq!(
			handle_key(&mut st, &r, &Key::Named(NamedKey::ArrowDown), Modifiers::SHIFT, (0., 0.)),
			KeyOutcome::Handled
		);
		assert_eq!(st.selection.selected, vec!["s1", "s2"]);
		handle_key(&mut st, &r, &Key::Named(NamedKey::ArrowDown), Modifiers::SHIFT, (0., 0.));
		assert_eq!(st.selection.selected, vec!["s1", "s2", "s3"]);
		assert_eq!(
			handle_key(&mut st, &r, &Key::Character(" ".into()), none, (0., 0.)),
			KeyOutcome::Action(TreeAction::ToggleEnabled(vec!["s1", "s2", "s3"]))
		);
		handle_key(&mut st, &r, &Key::Named(NamedKey::Home), none, (0., 0.));
		assert_eq!(st.selection.current, Some("project"));
		handle_key(&mut st, &r, &Key::Character("a".into()), Modifiers::CONTROL, (0., 0.));
		assert_eq!(st.selection.selected, items());
		handle_key(&mut st, &r, &Key::Named(NamedKey::Insert), none, (0., 0.));
		assert_eq!(st.selection.selected, vec!["s1", "s2", "s3"]);
		assert_eq!(st.selection.current, Some("s3"));
		handle_key(&mut st, &r, &Key::Named(NamedKey::F2), none, (0., 0.));
		assert_eq!(st.renaming, Some("s3"));
		st.selection.select("sources");
		handle_key(&mut st, &r, &Key::Named(NamedKey::ArrowLeft), none, (0., 0.));
		assert_eq!(st.collapsed, vec!["sources"]);
		handle_key(&mut st, &r, &Key::Named(NamedKey::ArrowDown), none, (0., 0.));
		assert_eq!(st.selection.current, Some("probes"));
		assert_eq!(handle_key(&mut st, &r, &Key::Named(NamedKey::Delete), none, (0., 0.)), KeyOutcome::Ignored);
		st.selection.select("p1");
		assert_eq!(
			handle_key(&mut st, &r, &Key::Named(NamedKey::ArrowUp), Modifiers::ALT, (0., 0.)),
			KeyOutcome::Action(TreeAction::MoveUp(vec!["p1"]))
		);
	}

	#[test]
	fn drop_targets_by_half_and_skip_no_moves() {
		let r = rows();
		assert_eq!(drop_target(&r, &"s1", 5, true), Some(DropPosition::Before("s3")));
		assert_eq!(drop_target(&r, &"s1", 4, true), None);
		assert_eq!(drop_target(&r, &"s1", 3, false), None);
		assert_eq!(drop_target(&r, &"s2", 3, false), None);
		assert_eq!(drop_target(&r, &"s3", 3, false), Some(DropPosition::After("s1")));
		assert_eq!(drop_target(&r, &"s1", 2, true), Some(DropPosition::Append(0)));
		assert_eq!(drop_target(&r, &"s3", 2, true), None);
		assert_eq!(drop_target(&r, &"s1", 7, true), None);
		assert_eq!(drop_target(&r, &"s1", 1, true), None);
		let list = ["s1", "s2", "s3"];
		assert_eq!(reorder_index(&list, &"s1", &DropPosition::Before("s3")), Some(1));
		assert_eq!(reorder_index(&list, &"s3", &DropPosition::After("s1")), Some(1));
		assert_eq!(reorder_index(&list, &"s1", &DropPosition::Append(0)), Some(2));
	}
}
