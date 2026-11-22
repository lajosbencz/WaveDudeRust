//! The Project pane (docs/ui/20): its toolbar and the project tree on the kit's [`Tree`], with the rows' check
//! boxes and buttons, the keys, the context menu, the inline rename and the drags. The selection is the state's, read
//! and written in place; a dragged item carries the kit's [`Dragged`](kit_tree::Dragged) row for the pictures to take.

use std::sync::Arc;

use eframe::egui::{
	Atom, Atoms, Button, IdSalt, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Response, Sense, Shape, Stroke,
	StrokeKind, Ui, Vec2, vec2,
};
use egui_material_icons::icons::{
	ICON_ADD, ICON_BOOKMARK_ADD, ICON_BOOKMARK_REMOVE, ICON_CHECK_BOX, ICON_CONTENT_COPY, ICON_CONTENT_CUT,
	ICON_CONTENT_PASTE, ICON_DELETE, ICON_FILE_COPY, ICON_IOS_SHARE, ICON_SELECT_ALL, ICON_TOGGLE_ON,
};
use egui_mineral::expand::Expansion;
use std::fmt::Write as _;

use egui_mineral::semantic::KeySalt;
use egui_mineral::tree::{DropPosition, DropZones, TreeData, TreeMove};
use waveduderust::model::{ItemKind, ItemRef, ProbeDetector, Project};
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::commands::{Command, NewPart};
use waveduderust_app::menus::{
	self, MenuAction, On, PART_MENU, PROBABILISTIC, PROBE_KINDS, PartEntry, moves_of, part_kinds,
};
use waveduderust_app::outline::{DropAt, drop_at, enabled_of, enabled_tip, folder_key, press, reorder_index};
use waveduderust_app::selection::Row;
use waveduderust_app::strings::Strings;
use waveduderust_egui_platform::keys::TREE_KEYS;
use waveduderust_egui_preview::gradient_editor::gradient_mesh;

use super::icons::{self, Icon};
use crate::kit::menu::{self, Entry, Lead, MenuUi};
use crate::kit::tree::{self as kit_tree, Drawing, Glyph, Rename, RowLook, Selected, Tree, TreeOutput};
use crate::kit::{Palette, TREE};

/// A row as the tree's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct RowKey(pub Row);

impl KeySalt for RowKey {
	fn write_key(&self, out: &mut String) {
		let _ = write!(out, "{:?}", self.0);
	}
}

/// An item dragged out of the tree.
pub(super) type DraggedItem = kit_tree::Dragged<RowKey>;

const ICON_SIZE: f32 = 16.;
const TOOLBAR_PADDING: f32 = 2.;
/// A tool button's padding, height, margin at each side and gap between its icon, word and caret.
const TOOL_PADDING: Vec2 = Vec2::new(4., 1.);
const TOOL_HEIGHT: f32 = 23.;
const TOOL_MARGIN: f32 = 1.;
const TOOL_GAP: f32 = 4.;
const SEPARATOR: Vec2 = Vec2::new(1., 16.);

/// A row of the tree model.
struct Node {
	key: RowKey,
	label: String,
	children: Vec<Node>,
}

impl TreeData for Node {
	type Key = RowKey;

	fn key(&self) -> &RowKey {
		&self.key
	}

	fn label(&self) -> &str {
		&self.label
	}

	fn children(&self) -> &[Self] {
		&self.children
	}

	fn accepts_children(&self) -> bool {
		matches!(self.key.0, Row::Folder(_))
	}

	/// Next's slots: a folder takes the whole drop and appends, other rows split by half.
	fn drop_zones(&self) -> DropZones {
		if self.accepts_children() { DropZones::Inside } else { DropZones::Halves }
	}
}

/// A folder's string key and icon.
pub(super) fn folder_of(kind: ItemKind) -> (&'static str, Icon) {
	let icon = match kind {
		ItemKind::Source => icons::SOURCE,
		ItemKind::Part => icons::PART,
		ItemKind::Probe => icons::PROBE,
		ItemKind::Scheme => icons::SCHEME,
		_ => icons::BLANK,
	};
	(folder_key(kind), icon)
}

/// The tree of `project`: the root over the settings rows and the folders of the item lists.
fn nodes(project: &Project, s: &Strings) -> Vec<Node> {
	let node = |row, label: &str, children| Node { key: RowKey(row), label: label.to_owned(), children };
	let mut rows = vec![
		node(Row::Scene, s.get("TreeScene"), Vec::new()),
		node(Row::Simulation, s.get("TreeSimulation"), Vec::new()),
		node(Row::Display, s.get("TreeDisplay"), Vec::new()),
	];
	for kind in ItemKind::ALL {
		let items = kind.items(project).into_iter();
		let items = items.map(|item| node(Row::Item(item), item.name(project).unwrap_or_default(), Vec::new()));
		rows.push(node(Row::Folder(kind), s.get(folder_key(kind)), items.collect()));
	}
	vec![node(Row::Project, s.get("TreeProject"), rows)]
}

/// The look of `row`'s drawing and check box.
fn drawing_of(project: &Project, row: Row) -> Drawing<'_> {
	let icon = match row {
		Row::Project => icons::PROJECT,
		Row::Scene => icons::SCENE,
		Row::Simulation => icons::SIMULATION,
		Row::Display => icons::DISPLAY,
		Row::Folder(kind) => folder_of(kind).1,
		Row::Item(ItemRef::Part(id)) => project.part(id).map_or(icons::PART, icons::part_icon),
		Row::Item(ItemRef::Scheme(id)) => match project.color_scheme(id) {
			Some(scheme) => {
				return Drawing::Paint(
					TREE.strip,
					Box::new(move |painter, rect| {
						painter.add(Shape::Mesh(Arc::new(gradient_mesh(rect, &scheme.stops))));
						let border = Palette::of(painter.ctx().theme()).hairline;
						painter.rect_stroke(rect, 0., Stroke::new(0.5, border), StrokeKind::Outside);
					}),
				);
			}
			None => icons::SCHEME,
		},
		Row::Item(item) => folder_of(item.kind()).1,
	};
	Drawing::Icon(icon)
}

/// What a press on a row's own button asks for.
#[derive(Clone, Copy)]
enum RowButton {
	Up,
	Down,
	Remove,
	Add,
}

/// The pane's state between frames.
pub(super) struct ProjectPane {
	expansion: Expansion<RowKey>,
	/// The tree, and the document revision and strings it was built for.
	nodes: Vec<Node>,
	built: Option<(u64, Strings)>,
	renaming: Option<Rename<RowKey>>,
	/// The reveal the tree took the focus for last.
	revealed: u64,
	/// The open context menu.
	menu: Option<MenuAt>,
	/// The open Add menu of a folder's plus, and where.
	add_menu: Option<(ItemKind, Pos2)>,
	/// Enter on a row or an Add asked for the form's first editor.
	activated: bool,
}

impl Default for ProjectPane {
	fn default() -> Self {
		let folders = ItemKind::ALL.into_iter().map(|kind| RowKey(Row::Folder(kind)));
		Self {
			expansion: std::iter::once(RowKey(Row::Project)).chain(folders).collect(),
			nodes: Vec::new(),
			built: None,
			renaming: None,
			revealed: 0,
			menu: None,
			add_menu: None,
			activated: false,
		}
	}
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
		let built = (app.revisions().document, app.strings);
		if self.built != Some(built) {
			self.nodes = nodes(&app.document.project, &app.strings);
			self.built = Some(built);
		}
		let reveal = app.reveals() > self.revealed;
		self.revealed = app.reveals();
		let shown = self.tree(ui, app, reveal);
		picked = picked.or(self.apply(app, &shown, out));
		let tree = &shown.response;
		if tree.has_focus() && self.renaming.is_none() {
			picked = picked.or(self.keys(ui, app, tree.rect, shown.current_rect));
		}
		if tree.secondary_clicked() {
			self.open_menu(None, tree.interact_pointer_pos().unwrap_or(tree.rect.min));
		}
		picked.map(|action| (action, On::Current)).or(self.context_menu(ui, app, tree))
	}

	/// The tree in the rest of `ui`'s room.
	fn tree(&mut self, ui: &mut Ui, app: &App, reveal: bool) -> TreeOutput<RowKey> {
		let s = &app.strings;
		let project = &app.document.project;
		let sel = &app.selection;
		let draggable = !app.settings.touch_first;
		let look = |node: &Node| {
			let row = node.key.0;
			let item = match row {
				Row::Item(item) => Some(item),
				_ => None,
			};
			let enabled = item.and_then(|i| enabled_of(project, i));
			let selected = match row {
				_ if sel.current() == Some(row) => Selected::Current,
				Row::Item(i) if sel.contains(i) => Selected::Other,
				_ => Selected::No,
			};
			let button = |glyph, tip, enabled| kit_tree::Button { glyph, tip: s.get(tip), enabled };
			let buttons = match row {
				Row::Item(i) => vec![
					button(Glyph::Up, "TipMoveUp", !moves_of(app, i, true).is_empty()),
					button(Glyph::Down, "TipMoveDown", !moves_of(app, i, false).is_empty()),
					button(Glyph::Minus, "TipRemove", true),
				],
				Row::Folder(_) => vec![button(Glyph::Plus, "TipAdd", true)],
				_ => Vec::new(),
			};
			RowLook {
				drawing: drawing_of(project, row),
				check: enabled.zip(item).map(|(on, i)| (on, s.get(enabled_tip(i.kind())))),
				dimmed: enabled == Some(false),
				selected,
				buttons,
				draggable: draggable && item.is_some(),
			}
		};
		let id = ui.scope_id().with("ProjectTree");
		Tree::new(id, s.get("PaneProject"), &self.nodes, &mut self.expansion)
			.current(sel.current().map(RowKey))
			.focus(reveal)
			.rename(&mut self.renaming)
			.drops(|m| accepts(project, m))
			.show(ui, look)
	}

	/// Opens the context menu on `row` at `at`.
	fn open_menu(&mut self, row: Option<Row>, at: Pos2) {
		self.menu = Some(MenuAt { row, at, pastable: waveduderust_egui_platform::pastable() });
	}

	/// What the rows asked for: selection, check boxes, buttons, drops, the rename.
	fn apply(&mut self, app: &mut App, shown: &TreeOutput<RowKey>, out: &mut Vec<Event>) -> Option<MenuAction> {
		let project = &app.document.project;
		if let Some((RowKey(Row::Item(target)), Some(name))) = &shown.renamed
			&& target.name(project) != Some(name.as_str())
		{
			out.push(Event::Command(Command::Rename { target: *target, name: name.clone() }));
		}
		if let Some(m) = &shown.moved
			&& let Row::Item(item) = m.node.0
			&& let Some(from) = project.index_of(item)
			&& let Some(to) = drop_of(project, m)
		{
			let to = reorder_index(from, to, item.kind().len(project));
			out.push(Event::Command(Command::Reorder { target: item, to }));
		}
		if let Some(RowKey(row)) = shown.pressed {
			let m = shown.response.ctx.input(|i| i.modifiers);
			press(&mut app.selection, &app.document.project, row, m.command, m.shift);
		}
		if let Some((RowKey(row), extend)) = shown.walked {
			match row {
				Row::Item(i) if extend => app.selection.extend_to(i, &app.document.project),
				row => app.selection.select(row),
			}
		}
		if let Some((RowKey(row), at)) = shown.context {
			let sel = &mut app.selection;
			if sel.current() != Some(row) && !matches!(row, Row::Item(i) if sel.contains(i)) {
				sel.select(row);
			}
			self.open_menu(Some(row), at);
		}
		if let Some(RowKey(Row::Item(target))) = shown.checked {
			out.push(Event::Command(Command::ToggleEnabled { target }));
		}
		let (RowKey(row), i, rect) = shown.button?;
		let button = match row {
			Row::Item(_) => [RowButton::Up, RowButton::Down, RowButton::Remove][i.min(2)],
			_ => RowButton::Add,
		};
		match (row, button) {
			(Row::Item(target), RowButton::Up | RowButton::Down) => {
				for (target, to) in moves_of(app, target, matches!(button, RowButton::Up)) {
					out.push(Event::Command(Command::Reorder { target, to }));
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

	/// The tree's keys (docs/ui/20, Selection model and Commands on the selection) beside the kit's walking ones,
	/// taken while it has the focus; `current` is the current row on screen.
	fn keys(&mut self, ui: &Ui, app: &mut App, room: Rect, current: Option<Rect>) -> Option<MenuAction> {
		let mut keys = Vec::new();
		ui.input_mut(|i| {
			i.events.retain(|e| {
				let eframe::egui::Event::Key { key, pressed: true, modifiers, .. } = e else { return true };
				let Some(taken) = tree_key(*key, *modifiers) else { return true };
				keys.push(taken);
				false
			});
		});
		let mut picked = None;
		for key in keys {
			picked = picked.or(self.key(app, room, current, key));
		}
		picked
	}

	fn key(&mut self, app: &mut App, room: Rect, current: Option<Rect>, key: TreeKey) -> Option<MenuAction> {
		let sel = &mut app.selection;
		match key {
			TreeKey::ToggleSelected => match sel.current()? {
				Row::Item(i) => sel.toggle(i),
				row => sel.select(row),
			},
			TreeKey::Activate => self.activated = sel.current().is_some(),
			TreeKey::Rename => {
				let item = sel.current_item()?;
				let text = item.name(&app.document.project)?.to_owned();
				self.renaming = Some(Rename::new(RowKey(Row::Item(item)), text));
			}
			TreeKey::Action(action) => return Some(action),
			TreeKey::Menu => {
				let at = current.map_or(room.min, |r| r.left_bottom()) + vec2(24., 0.);
				self.open_menu(sel.current(), at);
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
			let shown = menu::popup(tree, id, menu.at, &mut open, |m| row_menu(m, app, menu.row, menu.pastable));
			picked = shown.flatten().map(|action| (action, On::Row(menu.row)));
			if !open {
				self.menu = None;
			}
		}
		if let Some((kind, at)) = self.add_menu {
			let mut open = true;
			let id = ui.scope_id().with("ProjectTreeAdd");
			let shown = menu::popup(tree, id, at, &mut open, |m| add_menu(m, kind, &app.strings));
			picked = shown.flatten().map(|action| (action, On::Current)).or(picked);
			if !open {
				self.add_menu = None;
			}
		}
		picked
	}
}

/// Whether the outline's rule (docs/ui/20, Drag and drop) takes the move `m`: within the item's own list.
fn accepts(project: &Project, m: &TreeMove<RowKey>) -> bool {
	let Row::Item(item) = m.node.0 else { return false };
	drop_at(project, item, m.target.0, m.position == DropPosition::Before).is_some()
}

/// The outline's drop of an aimed move.
fn drop_of(project: &Project, m: &TreeMove<RowKey>) -> Option<DropAt> {
	let at = |key: RowKey| match key.0 {
		Row::Item(i) => project.index_of(i),
		_ => None,
	};
	Some(match m.position {
		DropPosition::Before => DropAt::Before(at(m.target)?),
		DropPosition::After => DropAt::After(at(m.target)?),
		DropPosition::Inside => DropAt::Append,
	})
}

/// A key of the tree beside the walking ones.
#[derive(Clone, Copy)]
enum TreeKey {
	ToggleSelected,
	Rename,
	/// Enter: the form's first editor takes the focus.
	Activate,
	Menu,
	/// An action on the current item.
	Action(MenuAction),
}

/// What `key` with exactly `m` does in the tree.
fn tree_key(key: Key, m: Modifiers) -> Option<TreeKey> {
	if let Some(&(_, action)) = TREE_KEYS.iter().find(|(k, _)| k.logical_key == key && m.matches_exact(k.modifiers)) {
		return Some(match action {
			MenuAction::ToggleSelected => TreeKey::ToggleSelected,
			action => TreeKey::Action(action),
		});
	}
	let none = m.is_none();
	Some(match key {
		Key::F2 if none => TreeKey::Rename,
		Key::Enter if none => TreeKey::Activate,
		Key::F10 if m.matches_exact(Modifiers::SHIFT) => TreeKey::Menu,
		_ => return None,
	})
}

/// The Add probe menu: a standard probe, and the detectors under Probabilistic, each with its description as tip.
fn probe_menu(m: &mut MenuUi<'_, '_>, s: &Strings) -> Option<ProbeDetector> {
	let mut picked = None;
	let mut entry = |m: &mut MenuUi<'_, '_>, (detector, key, tip): (ProbeDetector, &str, &str)| {
		if m.item(Entry::new(s.get(key)).icon(icons::PROBE).tip(s.get(tip))).picked {
			picked = Some(detector);
		}
	};
	let (standard, probabilistic) = PROBE_KINDS.split_at(1);
	entry(m, standard[0]);
	m.submenu(Entry::new(s.get(PROBABILISTIC)).icon(icons::PROBE), |m| {
		for &kind in probabilistic {
			entry(m, kind);
		}
	});
	picked
}

/// The Add part menu: the kinds with their drawings and descriptions, gratings, meta surfaces and logic gates in
/// submenus of their own.
fn part_menu(m: &mut MenuUi<'_, '_>, s: &Strings) -> Option<NewPart> {
	let mut picked = None;
	let mut entries = |m: &mut MenuUi<'_, '_>, from: NewPart, to: NewPart| {
		for &(kind, key) in part_kinds(from, to) {
			let tip = s.kind(kind.type_name());
			if m.item(Entry::new(s.get(key)).icon(icons::kind_icon(kind)).tip(tip)).picked {
				picked = Some(kind);
			}
		}
	};
	for entry in PART_MENU {
		match entry {
			PartEntry::Kinds(from, to) => entries(m, from, to),
			PartEntry::Sub(key, from, to) => {
				m.submenu(Entry::new(s.get(key)).icon(icons::kind_icon(from)), |m| entries(m, from, to));
			}
			PartEntry::Separator => m.separator(),
		}
	}
	picked
}

/// The Add menu of the folder of `kind`, probes or parts.
fn add_menu(m: &mut MenuUi<'_, '_>, kind: ItemKind, s: &Strings) -> Option<MenuAction> {
	if kind == ItemKind::Probe {
		probe_menu(m, s).map(MenuAction::AddProbe)
	} else {
		part_menu(m, s).map(MenuAction::AddPart)
	}
}

/// The context menu of `row` (none: the empty tree); `pastable` tells whether the clipboard holds objects.
fn row_menu(m: &mut MenuUi<'_, '_>, app: &App, row: Option<Row>, pastable: bool) -> Option<MenuAction> {
	let s = &app.strings;
	let item = match row {
		Some(Row::Item(i)) => Some(i),
		_ => None,
	};
	let ctx = m.ctx().clone();
	let shortcut = |modifiers, key| Some(ctx.format_shortcut(&KeyboardShortcut::new(modifiers, key)));
	let entry = |m: &mut MenuUi<'_, '_>, action, icon: Lead, key: &str, shortcut: Option<String>| {
		let enabled = menus::enabled(app, action, On::Row(row)) && (action != MenuAction::Paste || pastable);
		m.item(Entry::new(s.get(key)).icon(icon).shortcut(shortcut).enabled(enabled)).picked.then_some(action)
	};
	let mut picked = entry(m, MenuAction::AddSource, ICON_ADD.into(), "MenuProjectAddSource", None);
	m.submenu(Entry::new(s.get("MenuProjectAddPart")).icon(ICON_ADD), |m| {
		picked = part_menu(m, s).map(MenuAction::AddPart).or(picked);
	});
	m.submenu(Entry::new(s.get("MenuProjectAddProbe")).icon(ICON_ADD), |m| {
		picked = probe_menu(m, s).map(MenuAction::AddProbe).or(picked);
	});
	m.separator();
	let words = |text: &str| Some(text.to_owned());
	let entries = [
		(MenuAction::ToggleSelected, ICON_CHECK_BOX.into(), "MenuProjectToggleSelected", words("Ins")),
		(MenuAction::SelectAll, ICON_SELECT_ALL.into(), "MenuProjectSelectAll", shortcut(Modifiers::COMMAND, Key::A)),
	];
	for (action, icon, key, keys) in entries {
		picked = picked.or(entry(m, action, icon, key, keys));
	}
	m.separator();
	let entries = [
		(MenuAction::ToggleEnabled, ICON_TOGGLE_ON.into(), "MenuProjectToggleEnabled", words("Space")),
		(MenuAction::Duplicate, ICON_FILE_COPY.into(), "MenuProjectDuplicate", shortcut(Modifiers::COMMAND, Key::D)),
		(MenuAction::Cut, ICON_CONTENT_CUT.into(), "MenuEditCut", None),
		(MenuAction::Copy, ICON_CONTENT_COPY.into(), "MenuEditCopy", None),
		(MenuAction::Paste, ICON_CONTENT_PASTE.into(), "MenuEditPaste", None),
		(MenuAction::Delete, ICON_DELETE.into(), "MenuProjectRemove", words("Del")),
	];
	for (action, icon, key, keys) in entries {
		picked = picked.or(entry(m, action, icon, key, keys));
	}
	m.separator();
	let entries = [
		(MenuAction::MoveUp, icons::CHEVRON_UP.into(), "MenuProjectMoveUp", shortcut(Modifiers::ALT, Key::ArrowUp)),
		(
			MenuAction::MoveDown,
			icons::CHEVRON_DOWN.into(),
			"MenuProjectMoveDown",
			shortcut(Modifiers::ALT, Key::ArrowDown),
		),
	];
	for (action, icon, key, keys) in entries {
		picked = picked.or(entry(m, action, icon, key, keys));
	}
	if let Some(ItemRef::Scheme(scheme)) = item {
		m.separator();
		let name = app.document.project.color_scheme(scheme).map_or("", |c| c.name.as_str());
		let (icon, key) = if app.is_user_scheme(name) {
			(ICON_BOOKMARK_REMOVE, "MenuSchemeRemoveFromMine")
		} else {
			(ICON_BOOKMARK_ADD, "MenuSchemeAddToMine")
		};
		picked = picked.or(entry(m, MenuAction::ToggleUserScheme, icon.into(), key, None));
	}
	if item.is_some_and(|i| i.kind() == ItemKind::Probe) {
		m.separator();
		let csv = entry(m, MenuAction::ExportProbeCsv, ICON_IOS_SHARE.into(), "MenuExportProbeCsv", None);
		picked = picked.or(csv);
	}
	picked
}

/// The toolbar: Source, Probe and Part (the latter two open their Add menu), then Move up, Move down and Remove of
/// the current item.
fn toolbar(ui: &mut Ui, app: &App) -> Option<MenuAction> {
	let s = &app.strings;
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
			picked = menu::below(&response, 0., |m| add_menu(m, kind, s)).flatten().or(picked);
		}
		let (sep, _) = ui.allocate_exact_size(SEPARATOR, Sense::hover());
		ui.painter().rect_filled(sep, 0., Palette::of(ui.ctx().theme()).border);
		let tools = [
			(icons::CHEVRON_UP, "TipMoveUp", MenuAction::MoveUp),
			(icons::CHEVRON_DOWN, "TipMoveDown", MenuAction::MoveDown),
			(icons::MINUS, "TipRemove", MenuAction::Delete),
		];
		for (icon, tip, action) in tools {
			if button(ui, icon, None, tip, menus::enabled(app, action, On::Current)).clicked() {
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
			icons::paint(ui.painter(), rect, drawn, color);
		}
	}
	shown.response
}
