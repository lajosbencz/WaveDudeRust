//! The Project pane (docs/ui/20): its toolbar and the tree of the project, with the rows' check boxes and buttons,
//! the keys, the context menu, the inline rename and the drags. The selection is the state's, read and written in
//! place; a dragged item carries [`DraggedItem`] for the pictures to take.

use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{
	self, Align2, Atom, Atoms, Button, Color32, EventFilter, Id, IdSalt, Key, KeyboardShortcut, Mesh, Modifiers, Popup,
	Pos2, Rect, Response, Role, ScrollArea, Sense, Shape, Stroke, StrokeKind, TextStyle, TextWrapMode, Ui, Vec2,
	WidgetInfo, WidgetText, pos2, vec2,
};
use egui_material_icons::icons::{
	ICON_ADD, ICON_BOOKMARK_ADD, ICON_BOOKMARK_REMOVE, ICON_CHECK_BOX, ICON_CONTENT_COPY, ICON_CONTENT_CUT,
	ICON_CONTENT_PASTE, ICON_DELETE, ICON_FILE_COPY, ICON_IOS_SHARE, ICON_SELECT_ALL, ICON_TOGGLE_ON,
};
use wavedude_app_state::Event;
use wavedude_app_state::clipboard::Copyable;
use wavedude_app_state::commands::{Command, NewPart};
use wavedude_app_state::platform::HAS_EXPORTS;
use wavedude_app_state::selection::{Row, Selection};
use wavedude_app_state::strings::Strings;
use wavedude_core::model::{ItemKind, ItemRef, ProbeDetector, Project, Stop};

use super::gradient_editor::gradient_mesh;
use super::icons::{self, Icon};
use super::menu_bar::{self, Slot};
use super::menu_keys;
use super::text_box::select_all;
use super::theme::{Palette, ROW_HEIGHT};
use crate::App;
use crate::menus::{MenuAction, On, PART_KINDS, can_move, moves_of};

/// An item dragged out of the tree.
#[derive(Clone, Copy, Debug)]
pub(super) struct DraggedItem(pub ItemRef);

const INDENT: f32 = 16.;
const ICON_SIZE: f32 = 16.;
const CHECK_BOX: f32 = 13.;
const ROW_BUTTON: f32 = 18.;
/// The room a row button's glyph leaves at each side.
const ROW_GLYPH_INSET: f32 = 3.;
/// The gap after a row's check box and drawing.
const GAP: f32 = 4.;
const ROW_PADDING: f32 = 2.;
/// A scheme row's gradient strip.
const STRIP: Vec2 = Vec2::new(36., 10.);
const TOOLBAR_PADDING: f32 = 2.;
/// A tool button's padding, height, margin at each side and gap between its icon, word and caret.
const TOOL_PADDING: Vec2 = Vec2::new(4., 1.);
const TOOL_HEIGHT: f32 = 23.;
const TOOL_MARGIN: f32 = 1.;
const TOOL_GAP: f32 = 4.;
const SEPARATOR: Vec2 = Vec2::new(1., 16.);
/// The drop line's thickness.
const DROP_LINE: f32 = 2.;

/// A row's drawing: an icon, or a scheme's gradient.
#[derive(Clone, Copy)]
enum Look<'a> {
	Icon(Icon),
	Gradient(&'a [Stop]),
}

/// A row shown, in tree order.
struct Line<'a> {
	row: Row,
	depth: u8,
	label: &'a str,
	look: Look<'a>,
	/// The item's Enabled (or Active) flag; none for the others and for schemes.
	enabled: Option<bool>,
	/// It has rows under it, shown or folded.
	parent: bool,
}

/// A folder's string key and icon.
fn folder_of(kind: ItemKind) -> (&'static str, Icon) {
	match kind {
		ItemKind::Source => ("TreeSources", icons::SOURCE),
		ItemKind::Part => ("TreeParts", icons::PART),
		ItemKind::Probe => ("TreeProbes", icons::PROBE),
		ItemKind::Scheme => ("TreeColorSchemes", icons::SCHEME),
	}
}

/// The item's Enabled (or Active) flag; `None` for schemes.
fn enabled_of(project: &Project, item: ItemRef) -> Option<bool> {
	match item.kind {
		ItemKind::Source => project.sources.get(item.index).map(|s| s.enabled),
		ItemKind::Part => project.parts.get(item.index).map(|p| p.base.enabled),
		ItemKind::Probe => project.probes.get(item.index).map(|p| p.active),
		ItemKind::Scheme => None,
	}
}

/// The check box's tooltip key of an item kind.
fn enabled_tip(kind: ItemKind) -> &'static str {
	match kind {
		ItemKind::Source => "TipSourceEnabled",
		ItemKind::Part => "TipPartEnabled",
		ItemKind::Probe | ItemKind::Scheme => "TipProbeActive",
	}
}

/// Refills `out` with the rows shown: every row but those under a folded one.
fn build(project: &Project, folded: &[Row], out: &mut Vec<Shown>) {
	out.clear();
	let shown = |row, depth, parent| Shown { row, depth, parent };
	out.push(shown(Row::Project, 0, true));
	if folded.contains(&Row::Project) {
		return;
	}
	out.extend([Row::Scene, Row::Simulation, Row::Display].map(|row| shown(row, 1, false)));
	for kind in ItemKind::ALL {
		let len = kind.len(project);
		out.push(shown(Row::Folder(kind), 1, len > 0));
		if !folded.contains(&Row::Folder(kind)) {
			out.extend((0..len).map(|index| shown(Row::Item(ItemRef::new(kind, index)), 2, false)));
		}
	}
}

/// How `shown` draws.
fn line_of<'a>(project: &'a Project, s: &'a Strings, shown: Shown) -> Line<'a> {
	let line = |label, icon| Line {
		row: shown.row,
		depth: shown.depth,
		label,
		look: Look::Icon(icon),
		enabled: None,
		parent: shown.parent,
	};
	match shown.row {
		Row::Project => line(s.get("TreeProject"), icons::PROJECT),
		Row::Scene => line(s.get("TreeScene"), icons::SCENE),
		Row::Simulation => line(s.get("TreeSimulation"), icons::SIMULATION),
		Row::Display => line(s.get("TreeDisplay"), icons::DISPLAY),
		Row::Folder(kind) => {
			let (key, icon) = folder_of(kind);
			line(s.get(key), icon)
		}
		Row::Item(item) => {
			let icon = folder_of(item.kind).1;
			let look = match item.kind {
				ItemKind::Part => Look::Icon(icons::part_icon(&project.parts[item.index])),
				ItemKind::Scheme => Look::Gradient(&project.color_schemes[item.index].stops),
				ItemKind::Source | ItemKind::Probe => Look::Icon(icon),
			};
			let label = item.name(project).unwrap_or_default();
			Line { look, enabled: enabled_of(project, item), ..line(label, icon) }
		}
	}
}

/// Where a dragged item lands among its list's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropAt {
	Before(usize),
	After(usize),
	/// At the end of its list, dropped on its folder.
	Append,
}

/// Where `dragged` lands with the pointer over `row` (its upper half when `upper`) in its list of `len`; `None` when
/// the row takes no drop of it or the drop would move nothing.
fn drop_at(dragged: ItemRef, row: Row, upper: bool, len: usize) -> Option<DropAt> {
	let at = dragged.index;
	match row {
		Row::Item(i) if i.kind == dragged.kind => {
			let t = i.index;
			if upper {
				(t != at && t != at + 1).then_some(DropAt::Before(t))
			} else {
				(t != at && t + 1 != at).then_some(DropAt::After(t))
			}
		}
		Row::Folder(kind) if kind == dragged.kind => (at + 1 != len).then_some(DropAt::Append),
		_ => None,
	}
}

/// The index the item at `from` ends at after a drop at `to` in its list of `len`.
fn reorder_index(from: usize, to: DropAt, len: usize) -> usize {
	let insert = match to {
		DropAt::Before(t) => t,
		DropAt::After(t) => t + 1,
		DropAt::Append => len,
	};
	if insert > from { insert - 1 } else { insert }
}

/// A press on `row`: Ctrl toggles an item in or out (another row is taken alone), Shift ranges from the anchor to an
/// item, a press on one of several selected items makes it current, else the row alone.
fn press(sel: &mut Selection, project: &Project, row: Row, ctrl: bool, shift: bool) {
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

/// An item row being renamed: its text so far.
struct Renaming {
	item: ItemRef,
	text: String,
	/// The box has taken the focus.
	started: bool,
}

/// What a press on a row's own button asks for.
#[derive(Clone, Copy)]
enum RowButton {
	Up,
	Down,
	Remove,
	Add,
}

/// What the rows asked for in one frame.
#[derive(Default)]
struct Asked {
	press: Option<Row>,
	secondary: Option<(Row, Pos2)>,
	fold: Option<Row>,
	toggle_enabled: Option<ItemRef>,
	button: Option<(Row, RowButton, Rect)>,
	/// The rename ended: the name to apply, none when reverted; whether the tree takes the focus back.
	renamed: Option<(Option<String>, bool)>,
	/// A row or one of its buttons was clicked: egui takes the focus from the tree under them.
	clicked: bool,
}

/// The pane's state between frames.
#[derive(Default)]
pub(super) struct ProjectPane {
	/// Rows folded by their chevron.
	folded: Vec<Row>,
	renaming: Option<Renaming>,
	/// The reveal the tree took the focus for last.
	revealed: u64,
	/// The current row the tree scrolled to last.
	scrolled_to: Option<Row>,
	/// The open context menu.
	menu: Option<MenuAt>,
	/// The open Add menu of a folder's plus, and where.
	add_menu: Option<(ItemKind, Pos2)>,
	/// The rows shown, in order.
	rows: Vec<Shown>,
	/// The document revision `rows` were built for; none after a fold.
	built: Option<u64>,
	/// The rows on screen, and the top of the first row.
	visible: (Range<usize>, f32),
	/// The schemes' gradient strips, by scheme, with where they were drawn.
	strips: Vec<Option<(Rect, Arc<Mesh>)>>,
	/// Enter on a row or an Add asked for the form's first editor.
	activated: bool,
}

/// The context menu: the row it opened on (none: the empty tree), where, and whether the clipboard held objects then.
#[derive(Clone, Copy)]
struct MenuAt {
	row: Option<Row>,
	at: Pos2,
	pastable: bool,
}

impl ProjectPane {
	/// Whether the form's first editor was asked for since the last call.
	pub(super) fn take_activated(&mut self) -> bool {
		std::mem::take(&mut self.activated)
	}

	/// The pane in `ui`'s room; the events it asks the state for go to `out`, an action for the window is returned
	/// with what it applies to.
	pub(super) fn ui(&mut self, ui: &mut Ui, app: &mut App, out: &mut Vec<Event>) -> Option<(MenuAction, On)> {
		let palette = Palette::of(ui.ctx().theme());
		let pane = ui.max_rect();
		ui.painter().rect_filled(pane, 0., palette.tool_window_background);
		let mut picked = ui
			.scope(|ui| {
				ui.set_clip_rect(pane.intersect(ui.clip_rect()));
				toolbar(ui, app)
			})
			.inner;
		let room = Rect::from_min_max(pos2(pane.left(), ui.cursor().top()), pane.max);
		let tree_id = ui.scope_id().with("ProjectTree");
		let tree = ui.interact(room, tree_id, Sense::click());
		let s = app.strings;
		tree.widget_info(|| WidgetInfo::labeled(Role::Tree, true, s.get("PaneProject")));
		if tree.clicked() {
			tree.request_focus();
		}
		if tree.secondary_clicked() {
			tree.request_focus();
			self.open_menu(None, tree.interact_pointer_pos().unwrap_or(room.min));
		}
		if app.reveals() > self.revealed {
			self.revealed = app.reveals();
			tree.request_focus();
		}
		if tree.has_focus() {
			let arrows = EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
			ui.memory_mut(|m| m.set_focus_lock_filter(tree_id, arrows));
		}
		let mut asked = Asked::default();
		self.rows(ui, room, app, tree.has_focus(), &mut asked, out);
		picked = picked.or(self.apply(app, &asked, &tree, out));
		if tree.has_focus() && self.renaming.is_none() {
			picked = picked.or(self.keys(ui, app, room));
		}
		picked.map(|action| (action, On::Current)).or(self.context_menu(ui, app, &tree))
	}

	/// The rows in `room`, those on screen drawn, a drop on them applied.
	fn rows(&mut self, ui: &mut Ui, room: Rect, app: &App, focused: bool, asked: &mut Asked, out: &mut Vec<Event>) {
		let palette = Palette::of(ui.ctx().theme());
		let project = &app.document.project;
		let revision = app.revisions().document;
		if self.built != Some(revision) {
			build(project, &self.folded, &mut self.rows);
			self.strips.clear();
			self.strips.resize(project.color_schemes.len(), None);
			self.built = Some(revision);
		}
		let dragged = egui::DragAndDrop::payload::<DraggedItem>(ui.ctx()).map(|d| d.0);
		let scroll_to = app.selection.current().filter(|c| self.scrolled_to != Some(*c));
		self.scrolled_to = app.selection.current();
		let mut drop = None;
		let mut tree_ui = ui.new_child(egui::UiBuilder::new().max_rect(room));
		// show_rows steps by the row height and the spacing at its call
		tree_ui.spacing_mut().item_spacing.y = 0.;
		let count = self.rows.len();
		ScrollArea::vertical().auto_shrink(false).show_rows(&mut tree_ui, ROW_HEIGHT, count, |ui, range| {
			let top = ui.max_rect().top() - range.start as f32 * ROW_HEIGHT;
			let at =
				|i: usize| Rect::from_min_size(pos2(room.left(), top + i as f32 * ROW_HEIGHT), vec2(1., ROW_HEIGHT));
			if let Some(i) = scroll_to.and_then(|c| self.rows.iter().position(|r| r.row == c)) {
				ui.scroll_to_rect_animation(at(i), None, egui::style::ScrollAnimation::none());
			}
			self.visible = (range.clone(), top);
			for i in range {
				let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
				let line = line_of(project, &app.strings, self.rows[i]);
				let look = RowLook { line: &line, focused, palette, s: &app.strings };
				self.row(ui, rect, &look, app, asked);
			}
			let Some(item) = dragged else { return };
			let Some(p) = ui.ctx().pointer_hover_pos().filter(|p| ui.clip_rect().contains(*p)) else { return };
			let over = self.row_at(p.y).map(|i| (self.rows[i].row, self.rect_of(i, ui.max_rect())));
			let len = item.kind.len(project);
			drop = over.and_then(|(row, r)| drop_at(item, row, p.y < r.center().y, len).map(|to| (item, to, r)));
			if let Some((_, to, r)) = drop {
				paint_drop(ui, r, to, palette.drop_indicator);
			}
		});
		if let Some((item, to, _)) = drop
			&& ui.input(|i| i.pointer.any_released())
			&& egui::DragAndDrop::take_payload::<DraggedItem>(ui.ctx()).is_some()
		{
			let to = reorder_index(item.index, to, item.kind.len(project));
			out.push(Event::Command(Command::Reorder { target: item, to }));
		}
	}

	/// The row on screen at height `y`.
	fn row_at(&self, y: f32) -> Option<usize> {
		let (range, top) = &self.visible;
		let i = ((y - top) / ROW_HEIGHT).floor();
		(i >= 0.).then_some(i as usize).filter(|i| range.contains(i))
	}

	/// Where row `i` is drawn across `span`'s width.
	fn rect_of(&self, i: usize, span: Rect) -> Rect {
		let y = self.visible.1 + i as f32 * ROW_HEIGHT;
		Rect::from_min_max(pos2(span.left(), y), pos2(span.right(), y + ROW_HEIGHT))
	}

	/// Opens the context menu on `row` at `at`.
	fn open_menu(&mut self, row: Option<Row>, at: Pos2) {
		self.menu = Some(MenuAt { row, at, pastable: super::platform::pastable() });
	}

	/// What the rows asked for: selection, folding, check boxes, buttons, the rename.
	fn apply(&mut self, app: &mut App, asked: &Asked, tree: &Response, out: &mut Vec<Event>) -> Option<MenuAction> {
		if let Some((name, refocus)) = &asked.renamed
			&& let Some(r) = self.renaming.take()
		{
			if let Some(name) = name.clone().filter(|n| r.item.name(&app.document.project) != Some(n.as_str())) {
				out.push(Event::Command(Command::Rename { target: r.item, name }));
			}
			if *refocus {
				tree.request_focus();
			}
		}
		if let Some(row) = asked.fold {
			match self.folded.iter().position(|r| *r == row) {
				Some(at) => {
					self.folded.remove(at);
				}
				None => self.folded.push(row),
			}
			self.built = None;
		}
		if asked.clicked {
			tree.request_focus();
		}
		if let Some(row) = asked.press {
			tree.request_focus();
			let m = tree.ctx.input(|i| i.modifiers);
			press(&mut app.selection, &app.document.project, row, m.command, m.shift);
		}
		if let Some((row, at)) = asked.secondary {
			tree.request_focus();
			let sel = &mut app.selection;
			if sel.current() != Some(row) && !matches!(row, Row::Item(i) if sel.contains(i)) {
				sel.select(row);
			}
			self.open_menu(Some(row), at);
		}
		if let Some(target) = asked.toggle_enabled {
			out.push(Event::Command(Command::ToggleEnabled { target }));
		}
		let (row, button, rect) = asked.button?;
		match (row, button) {
			(Row::Item(target), RowButton::Up | RowButton::Down) => {
				for (index, to) in moves_of(app, target, matches!(button, RowButton::Up)) {
					out.push(Event::Command(Command::Reorder { target: ItemRef::new(target.kind, index), to }));
				}
			}
			(Row::Item(target), RowButton::Remove) => out.push(Event::Command(Command::Remove { target })),
			(Row::Folder(ItemKind::Source), RowButton::Add) => return Some(MenuAction::AddSource),
			(Row::Folder(ItemKind::Scheme), RowButton::Add) => {
				out.push(Event::Command(Command::AddScheme));
				self.activated = true;
			}
			(Row::Folder(kind), RowButton::Add) => self.add_menu = Some((kind, rect.left_bottom())),
			_ => {}
		}
		None
	}

	/// One row at `rect`.
	fn row(&mut self, ui: &mut Ui, rect: Rect, look: &RowLook<'_>, app: &App, asked: &mut Asked) {
		let line = look.line;
		let s = look.s;
		let id = ui.scope_id().with(("TreeRow", line.row));
		let response = ui.interact(rect, id, Sense::click_and_drag());
		let sel = &app.selection;
		let current = sel.current() == Some(line.row);
		let selected = current || matches!(line.row, Row::Item(i) if sel.contains(i));
		response.widget_info(|| WidgetInfo::selected(Role::TreeItem, true, selected, line.label));
		let palette = look.palette;
		let (fill, text) = if current && look.focused {
			(palette.selection, palette.selection_text)
		} else if selected {
			(palette.inactive_selection, palette.inactive_selection_text)
		} else {
			(Color32::TRANSPARENT, palette.text)
		};
		let ink = if line.enabled == Some(false) { palette.text_disabled } else { text };
		ui.painter().rect_filled(rect, 0., fill);
		let cy = rect.center().y;
		let square = |x: f32, size: f32| Rect::from_center_size(pos2(x + size / 2., cy), Vec2::splat(size));
		let mut x = rect.left() + ROW_PADDING + INDENT * f32::from(line.depth);
		if line.parent {
			let chevron = square(x, ICON_SIZE);
			let icon = if self.folded.contains(&line.row) { icons::CHEVRON_RIGHT } else { icons::CHEVRON_DOWN };
			icons::paint(ui, chevron, icon, ink);
			if ui.interact(chevron, id.with("fold"), Sense::CLICK).clicked() {
				asked.fold = Some(line.row);
				asked.clicked = true;
			}
		}
		x += ICON_SIZE;
		if let (Some(on), Row::Item(item)) = (line.enabled, line.row) {
			let r = square(x, CHECK_BOX);
			ui.painter().rect(r, 0., palette.input_background, Stroke::new(1., palette.hairline), StrokeKind::Inside);
			if on {
				icons::paint(ui, r.shrink(1.), icons::CHECK, palette.text);
			}
			let check = ui.interact(r, id.with("enabled"), Sense::CLICK).on_hover_text(s.get(enabled_tip(item.kind)));
			check.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, on, line.label));
			if check.clicked() {
				asked.toggle_enabled = Some(item);
				asked.clicked = true;
			}
			x += CHECK_BOX + GAP;
		}
		match line.look {
			Look::Icon(icon) => {
				icons::paint(ui, square(x, ICON_SIZE), icon, ink);
				x += ICON_SIZE + GAP;
			}
			Look::Gradient(stops) => {
				let strip = Rect::from_min_size(pos2(x, cy - STRIP.y / 2.), STRIP);
				if let Row::Item(item) = line.row
					&& let Some(cached) = self.strips.get_mut(item.index)
				{
					let (_, mesh) = cached
						.take()
						.filter(|(at, _)| *at == strip)
						.unwrap_or_else(|| (strip, Arc::new(gradient_mesh(strip, stops))));
					paint_strip(ui, strip, Arc::clone(&mesh), palette.hairline);
					*cached = Some((strip, mesh));
				}
				x += STRIP.x + GAP;
			}
		}
		let buttons: &[(Icon, RowButton, &str)] = match line.row {
			Row::Item(_) => &[
				(icons::CHEVRON_UP, RowButton::Up, "TipMoveUp"),
				(icons::CHEVRON_DOWN, RowButton::Down, "TipMoveDown"),
				(icons::MINUS, RowButton::Remove, "TipRemove"),
			],
			Row::Folder(_) => &[(icons::PLUS, RowButton::Add, "TipAdd")],
			_ => &[],
		};
		let right = rect.right() - ROW_PADDING - ROW_BUTTON * buttons.len() as f32;
		for (i, &(icon, button, tip)) in buttons.iter().enumerate() {
			let min = pos2(right + ROW_BUTTON * i as f32, cy - ROW_BUTTON / 2.);
			let r = Rect::from_min_size(min, Vec2::splat(ROW_BUTTON));
			let b = ui.interact(r, id.with(("button", i)), Sense::CLICK).on_hover_text(s.get(tip));
			b.widget_info(|| WidgetInfo::labeled(Role::Button, true, s.get(tip)));
			icons::paint(ui, r.shrink(ROW_GLYPH_INSET), icon, ink);
			if b.clicked() {
				asked.button = Some((line.row, button, r));
				asked.clicked = true;
			}
		}
		let name = Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom()));
		match self.renaming.as_mut().filter(|r| Row::Item(r.item) == line.row) {
			Some(r) => rename_box(ui, name, r, asked),
			None => {
				let galley = WidgetText::from(line.label).into_galley(
					ui,
					Some(TextWrapMode::Truncate),
					name.width().max(0.),
					TextStyle::Body,
				);
				let at = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), name).min;
				ui.painter().with_clip_rect(name.intersect(ui.clip_rect())).galley(at, galley, ink);
			}
		}
		if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
			asked.press = Some(line.row);
		}
		asked.clicked |= response.clicked();
		if response.secondary_clicked() {
			asked.secondary = response.interact_pointer_pos().map(|p| (line.row, p));
		}
		if let Row::Item(item) = line.row
			&& response.drag_started()
		{
			egui::DragAndDrop::set_payload(ui.ctx(), DraggedItem(item));
		}
	}

	/// The tree's keys (docs/ui/20, Selection model and Commands on the selection), taken while it has the focus.
	fn keys(&mut self, ui: &Ui, app: &mut App, room: Rect) -> Option<MenuAction> {
		let mut keys = Vec::new();
		ui.input_mut(|i| {
			i.events.retain(|e| {
				let egui::Event::Key { key, pressed: true, modifiers, .. } = e else { return true };
				let Some(taken) = tree_key(*key, *modifiers) else { return true };
				keys.push(taken);
				false
			});
		});
		let mut picked = None;
		for key in keys {
			picked = picked.or(self.key(app, room, key));
		}
		picked
	}

	fn key(&mut self, app: &mut App, room: Rect, key: TreeKey) -> Option<MenuAction> {
		let rows = &self.rows;
		let sel = &mut app.selection;
		let project = &app.document.project;
		let current = sel.current();
		let cursor = current.and_then(|c| rows.iter().position(|r| r.row == c));
		let last = rows.len().saturating_sub(1);
		let mut go = |at: usize, extend: bool| match rows[at].row {
			Row::Item(i) if extend => sel.extend_to(i, project),
			row => sel.select(row),
		};
		match key {
			TreeKey::Up(extend) => go(cursor.map_or(0, |c| c.saturating_sub(1)), extend),
			TreeKey::Down(extend) => go(cursor.map_or(0, |c| (c + 1).min(last)), extend),
			TreeKey::Home(extend) => go(0, extend),
			TreeKey::End(extend) => go(last, extend),
			TreeKey::Fold => {
				let at = cursor?;
				let r = &rows[at];
				if r.parent && !self.folded.contains(&r.row) {
					self.folded.push(r.row);
					self.built = None;
				} else if let Some(parent) = rows[..at].iter().rev().find(|p| p.depth < r.depth) {
					sel.select(parent.row);
				}
			}
			TreeKey::Unfold => {
				let row = current?;
				self.folded.retain(|r| *r != row);
				self.built = None;
			}
			TreeKey::ToggleSelected => match current? {
				Row::Item(i) => sel.toggle(i),
				row => sel.select(row),
			},
			TreeKey::ToggleEnabled => {
				return sel
					.current_item()
					.filter(|i| enabled_of(project, *i).is_some())
					.map(|_| MenuAction::ToggleEnabled);
			}
			TreeKey::Activate => self.activated = current.is_some(),
			TreeKey::Rename => {
				let item = sel.current_item()?;
				let text = item.name(project)?.to_owned();
				self.renaming = Some(Renaming { item, text, started: false });
			}
			TreeKey::Action(action) => return sel.current_item().map(|_| action),
			TreeKey::Menu => {
				let shown = cursor.filter(|c| self.visible.0.contains(c));
				let at = shown.map_or(room.min, |c| self.rect_of(c, room).left_bottom()) + vec2(24., 0.);
				self.open_menu(current, at);
			}
		}
		None
	}

	/// The context menu, whose actions apply to its row, and a folder's Add menu, while open.
	fn context_menu(&mut self, ui: &Ui, app: &App, tree: &Response) -> Option<(MenuAction, On)> {
		let mut picked = None;
		if let Some(menu) = self.menu {
			let mut open = true;
			let id = ui.scope_id().with("ProjectTreeMenu");
			let shown = popup(tree, id, menu.at, &mut open).show(|ui| row_menu(ui, app, menu.row, menu.pastable));
			menu_keys::menu(ui.ctx(), id, tree.id);
			picked = shown.and_then(|r| r.inner).map(|action| (action, On::Row(menu.row)));
			if !open {
				self.menu = None;
			}
		}
		if let Some((kind, at)) = self.add_menu {
			let mut open = true;
			let id = ui.scope_id().with("ProjectTreeAdd");
			let shown = popup(tree, id, at, &mut open).show(|ui| add_menu(ui, kind, &app.strings));
			menu_keys::menu(ui.ctx(), id, tree.id);
			picked = shown.and_then(|r| r.inner).map(|action| (action, On::Current)).or(picked);
			if !open {
				self.add_menu = None;
			}
		}
		picked
	}
}

/// A row shown: its place in the tree.
#[derive(Clone, Copy)]
struct Shown {
	row: Row,
	depth: u8,
	/// It has rows under it, shown or folded.
	parent: bool,
}

/// How a row draws.
struct RowLook<'a> {
	line: &'a Line<'a>,
	/// The tree has the keyboard focus.
	focused: bool,
	palette: &'static Palette,
	s: &'a Strings,
}

/// A key of the tree.
#[derive(Clone, Copy)]
enum TreeKey {
	/// Up, Down, Home, End: extending the range with Shift.
	Up(bool),
	Down(bool),
	Home(bool),
	End(bool),
	Fold,
	Unfold,
	ToggleSelected,
	ToggleEnabled,
	Rename,
	/// Enter: the form's first editor takes the focus.
	Activate,
	Menu,
	/// An action on the current item.
	Action(MenuAction),
}

/// What `key` with exactly `m` does in the tree.
fn tree_key(key: Key, m: Modifiers) -> Option<TreeKey> {
	let none = m.is_none();
	let shift = m.matches_exact(Modifiers::SHIFT);
	let alt = m.matches_exact(Modifiers::ALT);
	let cmd = m.matches_exact(Modifiers::COMMAND);
	Some(match key {
		Key::ArrowUp if alt => TreeKey::Action(MenuAction::MoveUp),
		Key::ArrowDown if alt => TreeKey::Action(MenuAction::MoveDown),
		Key::ArrowUp if none || shift => TreeKey::Up(shift),
		Key::ArrowDown if none || shift => TreeKey::Down(shift),
		Key::Home if none || shift => TreeKey::Home(shift),
		Key::End if none || shift => TreeKey::End(shift),
		Key::ArrowLeft if none => TreeKey::Fold,
		Key::ArrowRight if none => TreeKey::Unfold,
		Key::D if cmd => TreeKey::Action(MenuAction::Duplicate),
		Key::Insert if none => TreeKey::ToggleSelected,
		Key::Space if cmd => TreeKey::ToggleSelected,
		Key::Space if none => TreeKey::ToggleEnabled,
		Key::F2 if none => TreeKey::Rename,
		Key::Enter if none => TreeKey::Activate,
		Key::Delete if none => TreeKey::Action(MenuAction::Delete),
		Key::F10 if shift => TreeKey::Menu,
		_ => return None,
	})
}

/// The rename box of `r` in `room`: it takes the focus with its text selected; Enter applies, Esc reverts (both
/// give the focus back to the tree), the focus leaving applies.
fn rename_box(ui: &mut Ui, room: Rect, r: &mut Renaming, asked: &mut Asked) {
	let edit =
		egui::TextEdit::singleline(&mut r.text).margin(egui::Margin::symmetric(2, 0)).desired_width(room.width());
	let response = ui.put(room, edit);
	if !r.started {
		response.request_focus();
		select_all(ui, response.id, r.text.chars().count());
		r.started = true;
	} else if response.lost_focus() {
		// taken, so the tree the focus goes back to does not take them as its own
		let (enter, esc) =
			ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
		asked.renamed = Some(((!esc).then(|| r.text.clone()), enter || esc));
	}
}

/// A menu of the tree at `at`, open while `open`.
fn popup<'a>(tree: &Response, id: Id, at: Pos2, open: &'a mut bool) -> Popup<'a> {
	Popup::menu(tree).id(id).at_position(at).open_bool(open)
}

/// The drop target's mark: a line along the row's top or bottom edge, or an outline around its folder.
fn paint_drop(ui: &Ui, row: Rect, to: DropAt, color: Color32) {
	let line = |y: f32| Rect::from_min_max(pos2(row.left(), y - DROP_LINE / 2.), pos2(row.right(), y + DROP_LINE / 2.));
	let painter = ui.painter();
	match to {
		DropAt::Before(_) => painter.rect_filled(line(row.top()), 0., color),
		DropAt::After(_) => painter.rect_filled(line(row.bottom()), 0., color),
		DropAt::Append => painter.rect_stroke(row, 0., Stroke::new(1., color), StrokeKind::Inside),
	};
}

/// The gradient `mesh` of `rect`, outlined thinly in `border`.
fn paint_strip(ui: &Ui, rect: Rect, mesh: Arc<Mesh>, border: Color32) {
	let painter = ui.painter();
	painter.add(Shape::Mesh(mesh));
	painter.rect_stroke(rect, 0., Stroke::new(0.5, border), StrokeKind::Outside);
}

/// The Add probe menu: a standard probe, and the detectors under Probabilistic, each with its description as tip.
fn probe_menu(ui: &mut Ui, s: &Strings) -> Option<ProbeDetector> {
	let mut picked = None;
	let mut entry = |ui: &mut Ui, detector, key: &str, tip: &str| {
		if menu_bar::entry(ui, Slot::Drawn(icons::PROBE), s.get(key), None, None).on_hover_text(s.get(tip)).clicked() {
			picked = Some(detector);
		}
	};
	entry(ui, ProbeDetector::Standard, "MenuAddStandardProbe", "Detector.Standard");
	menu_bar::submenu(ui, Slot::Drawn(icons::PROBE), s.get("MenuAddProbabilistic"), true, |ui| {
		entry(ui, ProbeDetector::Counting, "MenuAddCountingDetector", "Detector.Counting");
		entry(ui, ProbeDetector::Imaging, "MenuAddImagingDetector", "Detector.Imaging");
	});
	picked
}

/// The Add part menu: the kinds with their drawings and descriptions, gratings, meta surfaces and logic gates in
/// submenus of their own.
fn part_menu(ui: &mut Ui, s: &Strings) -> Option<NewPart> {
	let mut picked = None;
	let mut entries = |ui: &mut Ui, from: NewPart, to: NewPart| {
		let at = |kind| PART_KINDS.iter().position(|(k, ..)| *k == kind).expect("a kind of the menu");
		for &(kind, key, _) in &PART_KINDS[at(from)..=at(to)] {
			let icon = Slot::Drawn(icons::kind_icon(kind));
			if menu_bar::entry(ui, icon, s.get(key), None, None).on_hover_text(s.kind(kind.type_name())).clicked() {
				picked = Some(kind);
			}
		}
	};
	entries(ui, NewPart::Lens, NewPart::InterferenceFilter);
	let groups = [
		("MenuAddDiffractionGratings", NewPart::SimpleGrating, NewPart::FacettedGrating),
		("MenuAddMetaSurfaces", NewPart::MetaLens, NewPart::MetaAxicon),
	];
	for (key, from, to) in groups {
		menu_bar::submenu(ui, Slot::Drawn(icons::kind_icon(from)), s.get(key), true, |ui| entries(ui, from, to));
	}
	ui.separator();
	entries(ui, NewPart::Polygon, NewPart::Outline);
	ui.separator();
	let (from, to) = (NewPart::XorGate, NewPart::OrGate);
	menu_bar::submenu(ui, Slot::Drawn(icons::kind_icon(from)), s.get("MenuAddOpticalLogic"), true, |ui| {
		entries(ui, from, to);
	});
	picked
}

/// The Add menu of the folder of `kind`, probes or parts.
fn add_menu(ui: &mut Ui, kind: ItemKind, s: &Strings) -> Option<MenuAction> {
	if kind == ItemKind::Probe {
		probe_menu(ui, s).map(MenuAction::AddProbe)
	} else {
		part_menu(ui, s).map(MenuAction::AddPart)
	}
}

/// The context menu of `row` (none: the empty tree); `pastable` tells whether the clipboard holds objects.
fn row_menu(ui: &mut Ui, app: &App, row: Option<Row>, pastable: bool) -> Option<MenuAction> {
	let s = &app.strings;
	let item = match row {
		Some(Row::Item(i)) => Some(i),
		_ => None,
	};
	let is_item = item.is_some();
	let copyable = is_item || row.and_then(Copyable::of_row).is_some();
	let ctx = ui.ctx().clone();
	let shortcut = |modifiers, key| Some(ctx.format_shortcut(&KeyboardShortcut::new(modifiers, key)));
	let entry = |ui: &mut Ui, action, icon: Slot, key: &str, shortcut: Option<String>, enabled: bool| {
		let r = ui.add_enabled_ui(enabled, |ui| menu_bar::entry(ui, icon, s.get(key), None, shortcut)).inner;
		r.clicked().then_some(action)
	};
	let mut picked = entry(ui, MenuAction::AddSource, Slot::Icon(ICON_ADD), "MenuProjectAddSource", None, true);
	menu_bar::submenu(ui, Slot::Icon(ICON_ADD), s.get("MenuProjectAddPart"), true, |ui| {
		picked = part_menu(ui, s).map(MenuAction::AddPart).or(picked);
	});
	menu_bar::submenu(ui, Slot::Icon(ICON_ADD), s.get("MenuProjectAddProbe"), true, |ui| {
		picked = probe_menu(ui, s).map(MenuAction::AddProbe).or(picked);
	});
	ui.separator();
	let toggles = item.is_some_and(|i| i.kind != ItemKind::Scheme);
	let can = |up| item.is_some_and(|i| can_move(app, i, up));
	let words = |text: &str| Some(text.to_owned());
	let entries = [
		(MenuAction::ToggleSelected, Slot::Icon(ICON_CHECK_BOX), "MenuProjectToggleSelected", words("Ins"), is_item),
		(
			MenuAction::SelectAll,
			Slot::Icon(ICON_SELECT_ALL),
			"MenuProjectSelectAll",
			shortcut(Modifiers::COMMAND, Key::A),
			true,
		),
	];
	for (action, icon, key, keys, enabled) in entries {
		picked = picked.or(entry(ui, action, icon, key, keys, enabled));
	}
	ui.separator();
	let entries = [
		(MenuAction::ToggleEnabled, Slot::Icon(ICON_TOGGLE_ON), "MenuProjectToggleEnabled", words("Space"), toggles),
		(
			MenuAction::Duplicate,
			Slot::Icon(ICON_FILE_COPY),
			"MenuProjectDuplicate",
			shortcut(Modifiers::COMMAND, Key::D),
			is_item,
		),
		(MenuAction::Cut, Slot::Icon(ICON_CONTENT_CUT), "MenuEditCut", None, is_item),
		(MenuAction::Copy, Slot::Icon(ICON_CONTENT_COPY), "MenuEditCopy", None, copyable),
		(MenuAction::Paste, Slot::Icon(ICON_CONTENT_PASTE), "MenuEditPaste", None, pastable),
		(MenuAction::Delete, Slot::Icon(ICON_DELETE), "MenuProjectRemove", words("Del"), is_item),
	];
	for (action, icon, key, keys, enabled) in entries {
		picked = picked.or(entry(ui, action, icon, key, keys, enabled));
	}
	ui.separator();
	let entries = [
		(
			MenuAction::MoveUp,
			Slot::Drawn(icons::CHEVRON_UP),
			"MenuProjectMoveUp",
			shortcut(Modifiers::ALT, Key::ArrowUp),
			can(true),
		),
		(
			MenuAction::MoveDown,
			Slot::Drawn(icons::CHEVRON_DOWN),
			"MenuProjectMoveDown",
			shortcut(Modifiers::ALT, Key::ArrowDown),
			can(false),
		),
	];
	for (action, icon, key, keys, enabled) in entries {
		picked = picked.or(entry(ui, action, icon, key, keys, enabled));
	}
	if let Some(scheme) = item.filter(|i| i.kind == ItemKind::Scheme) {
		ui.separator();
		let (icon, key) = if app.is_user_scheme(&app.document.project.color_schemes[scheme.index].name) {
			(ICON_BOOKMARK_REMOVE, "MenuSchemeRemoveFromMine")
		} else {
			(ICON_BOOKMARK_ADD, "MenuSchemeAddToMine")
		};
		picked = picked.or(entry(ui, MenuAction::ToggleUserScheme, Slot::Icon(icon), key, None, true));
	}
	if let Some(probe) = item.filter(|i| HAS_EXPORTS && i.kind == ItemKind::Probe) {
		ui.separator();
		let active = app.document.project.probes[probe.index].active && !app.live.export_busy();
		let csv = entry(ui, MenuAction::ExportProbeCsv, Slot::Icon(ICON_IOS_SHARE), "MenuExportProbeCsv", None, active);
		picked = picked.or(csv);
	}
	picked
}

/// The toolbar: Source, Probe and Part (the latter two open their Add menu), then Move up, Move down and Remove of
/// the current item.
fn toolbar(ui: &mut Ui, app: &App) -> Option<MenuAction> {
	let s = &app.strings;
	let current = app.selection.current_item();
	let can = |up| current.is_some_and(|i| can_move(app, i, up));
	let mut picked = None;
	ui.horizontal(|ui| {
		ui.add_space(TOOLBAR_PADDING);
		ui.spacing_mut().item_spacing.x = TOOLBAR_PADDING + 2. * TOOL_MARGIN;
		ui.spacing_mut().button_padding = TOOL_PADDING;
		let button = |ui: &mut Ui, icon, caption: Option<(&str, bool)>, tip: &str, enabled| {
			ui.add_enabled_ui(enabled, |ui| tool_button(ui, icon, caption)).inner.on_hover_text(s.get(tip))
		};
		if button(ui, icons::SOURCE, Some((s.get("ToolAddSource"), false)), "TipAddSource", true).clicked() {
			picked = Some(MenuAction::AddSource);
		}
		for (kind, icon, key, tip) in [
			(ItemKind::Probe, icons::PROBE, "ToolAddProbe", "TipAddProbe"),
			(ItemKind::Part, icons::PART, "ToolAddPart", "TipAddPart"),
		] {
			let response = button(ui, icon, Some((s.get(key), true)), tip, true);
			let popup = Popup::menu(&response);
			let id = popup.get_id();
			if let Some(shown) = popup.show(|ui| add_menu(ui, kind, s)) {
				menu_keys::menu(ui.ctx(), id, response.id);
				picked = shown.inner.or(picked);
			}
		}
		let (sep, _) = ui.allocate_exact_size(SEPARATOR, Sense::hover());
		ui.painter().rect_filled(sep, 0., Palette::of(ui.ctx().theme()).border);
		let tools = [
			(icons::CHEVRON_UP, "TipMoveUp", can(true), MenuAction::MoveUp),
			(icons::CHEVRON_DOWN, "TipMoveDown", can(false), MenuAction::MoveDown),
			(icons::MINUS, "TipRemove", current.is_some(), MenuAction::Delete),
		];
		for (icon, tip, enabled, action) in tools {
			if button(ui, icon, None, tip, enabled).clicked() {
				picked = Some(action);
			}
		}
	});
	ui.add_space(TOOLBAR_PADDING);
	picked
}

/// A flat tool button: `icon`, then the caption's word and, for a button that opens a menu, the drop-down arrow.
fn tool_button(ui: &mut Ui, icon: Icon, caption: Option<(&str, bool)>) -> Response {
	let (icon_id, caret_id) = (IdSalt::new("tool_icon"), IdSalt::new("tool_caret"));
	let size = Vec2::splat(ICON_SIZE);
	let mut atoms = Atoms::new(Atom::custom(icon_id, size));
	if let Some((word, menu)) = caption {
		atoms.push_right(word);
		if menu {
			atoms.push_right(Atom::custom(caret_id, size));
		}
	}
	let shown = Button::new(atoms).frame_when_inactive(false).min_size(vec2(0., TOOL_HEIGHT)).gap(TOOL_GAP).atom_ui(ui);
	let color = ui.style().interact(&shown.response).fg_stroke.color;
	for (id, drawn) in [(icon_id, icon), (caret_id, icons::DROPDOWN)] {
		if let Some(rect) = shown.rect(id) {
			icons::paint(ui, rect, drawn, color);
		}
	}
	shown.response
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_drop_lands_by_half_and_moves_something() {
		let item = |i| Row::Item(ItemRef::new(ItemKind::Source, i));
		let s0 = ItemRef::new(ItemKind::Source, 0);
		let s2 = ItemRef::new(ItemKind::Source, 2);
		assert_eq!(drop_at(s0, item(2), true, 3), Some(DropAt::Before(2)));
		assert_eq!(drop_at(s0, item(1), true, 3), None, "before the next moves nothing");
		assert_eq!(drop_at(s0, item(0), false, 3), None);
		assert_eq!(drop_at(s2, item(0), false, 3), Some(DropAt::After(0)));
		assert_eq!(drop_at(s0, Row::Folder(ItemKind::Source), true, 3), Some(DropAt::Append));
		assert_eq!(drop_at(s2, Row::Folder(ItemKind::Source), true, 3), None, "the last one stays");
		assert_eq!(drop_at(s0, Row::Folder(ItemKind::Probe), true, 3), None, "another list");
		assert_eq!(drop_at(s0, Row::Scene, true, 3), None);
		assert_eq!(reorder_index(0, DropAt::Before(2), 3), 1);
		assert_eq!(reorder_index(2, DropAt::After(0), 3), 1);
		assert_eq!(reorder_index(0, DropAt::Append, 3), 2);
	}

	#[test]
	fn the_tree_keys_fire_with_exactly_their_modifiers() {
		assert!(matches!(tree_key(Key::ArrowUp, Modifiers::SHIFT), Some(TreeKey::Up(true))));
		assert!(matches!(tree_key(Key::ArrowUp, Modifiers::ALT), Some(TreeKey::Action(MenuAction::MoveUp))));
		assert!(matches!(tree_key(Key::Space, Modifiers::COMMAND), Some(TreeKey::ToggleSelected)));
		assert!(matches!(tree_key(Key::Space, Modifiers::NONE), Some(TreeKey::ToggleEnabled)));
		assert!(tree_key(Key::Delete, Modifiers::SHIFT).is_none(), "Shift+Del is Cut");
		assert!(tree_key(Key::A, Modifiers::COMMAND).is_none(), "Select all is the window's");
		assert!(tree_key(Key::Escape, Modifiers::NONE).is_none(), "Esc stops the run");
	}
}
