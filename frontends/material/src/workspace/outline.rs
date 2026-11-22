//! The outline (DESIGN.md, Outline and inspector): Scene, Simulation and Display, then a section per item list with
//! its count and items, each item with its kind icon and an eye toggling Enabled. The rows mirror `AppState.selection`; presses,
//! keys and drags go through [`waveduderust_app::outline`], so they select and reorder as the other window's tree.

use std::sync::Arc;

use eframe::egui::{Key, Modifiers, Rect, Shape, Stroke, StrokeKind, Vec2};
use egui_mineral::expand::Expansion;
use egui_mineral::select::Selection;
use egui_mineral::semantic::KeySalt;
use egui_mineral::shortcut::{self, Scope};
use egui_mineral::tree::{DropPosition, TreeData};
use egui_mineral_material::prelude::{
	Density, IconAction, LeadingPaint, ListSlot, MaterialUiExt as _, Menu, TreeView, TypeScale, icons,
};
use waveduderust::model::{ItemKind, ItemRef, PartKind, Project};
use waveduderust_app::Event;
use waveduderust_app::commands::Command;
use waveduderust_app::menus::{MenuAction, On};
use waveduderust_app::outline::{drop_at, enabled_of, enabled_tip, folder_key, press, reorder_index};
use waveduderust_app::selection::Row;
use waveduderust_app::strings::strip_mnemonic;
use waveduderust_egui_platform::keys::TREE_KEYS;
use waveduderust_egui_preview::drawn;
use waveduderust_egui_preview::gradient_editor::gradient_mesh;

use crate::commands::Cmd;
use crate::menus::{self, row_icon};
use crate::shell::Intent;
use crate::{App, Mui, Salt};

/// A row of the tree.
struct Node {
	key: Salt<Row>,
	label: String,
	children: Vec<Node>,
}

impl TreeData for Node {
	type Key = Salt<Row>;

	fn key(&self) -> &Salt<Row> {
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
}

/// The rows of `app`'s project.
fn nodes(app: &App) -> Vec<Node> {
	let s = &app.strings;
	let p = &app.document.project;
	let leaf = |key: Row, label: String| Node { key: Salt(key), label, children: Vec::new() };
	let mut out = vec![
		leaf(Row::Scene, s.get("TreeScene").to_owned()),
		leaf(Row::Simulation, s.get("TreeSimulation").to_owned()),
		leaf(Row::Display, s.get("TreeDisplay").to_owned()),
	];
	for kind in ItemKind::ALL {
		let len = kind.len(p);
		let children = (kind.items(p).into_iter())
			.map(|item| leaf(Row::Item(item), item.name(p).unwrap_or_default().to_owned()))
			.collect();
		out.push(Node {
			key: Salt(Row::Folder(kind)),
			label: format!("{} ({len})", s.get(folder_key(kind))),
			children,
		});
	}
	out
}

/// The outline's state between passes.
pub(super) struct Outline {
	expansion: Expansion<Salt<Row>>,
	/// The row that had the focus last pass.
	focused: Option<Row>,
	/// The open Add menu: its folder and its `+`.
	adding: Option<(ItemKind, Rect)>,
}

impl Default for Outline {
	fn default() -> Self {
		let mut expansion = Expansion::default();
		for kind in ItemKind::ALL {
			expansion.expand(Salt(Row::Folder(kind)));
		}
		Self { expansion, focused: None, adding: None }
	}
}

/// What the outline asks the inspector for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Asked {
	/// Enter on a row: the form's first editor.
	pub(super) form: bool,
	/// F2 on an item: its name.
	pub(super) name: bool,
}

impl Outline {
	/// The tree in `ui`; selection changes go to `app`, edits to `out`, actions on a row to `intents`.
	pub(super) fn show(
		&mut self,
		ui: &mut Mui<'_>,
		app: &mut App,
		out: &mut Vec<Event>,
		intents: &mut Vec<Intent>,
	) -> Asked {
		let s = app.strings;
		let rt = ui.rt();
		let roots = nodes(app);
		let mut selection = Selection::multiple().with(app.selection.items().iter().map(|&i| Salt(Row::Item(i))));
		if let Some(current) = app.selection.current() {
			selection.select(Salt(current));
		}
		let selected = selection.len();
		let project = &app.document.project;
		let mut asked = Asked::default();
		let (scope, r) = ui
			.push_id("outline", |ui| {
				let tree =
					TreeView::new("outline", strip_mnemonic(s.get("MenuViewOutline")), &roots, &mut self.expansion)
						.selection(&mut selection)
						.leading(|n: &Node| leading(project, n.key.0))
						.actions(|n: &Node| match n.key.0 {
							Row::Folder(kind) => {
								vec![(RowAction::Add, IconAction::new(icons::ICON_ADD, s.get(add_tip(kind))))]
							}
							Row::Item(i) => enabled_of(project, i).map_or_else(Vec::new, |on| {
								let icon = if on { icons::ICON_VISIBILITY } else { icons::ICON_VISIBILITY_OFF };
								vec![(
									RowAction::Toggle,
									IconAction::new(icon, s.get(enabled_tip(i.kind()))).selected(on),
								)]
							}),
							_ => Vec::new(),
						})
						.node_text_style(|n: &Node| match n.key.0 {
							Row::Folder(_) => TypeScale::TitleSmall,
							_ => TypeScale::BodyMedium,
						})
						.reorderable(!app.settings.touch_first);
				let ui = &mut rt.ui(ui);
				// one-line rows reach the 32 dp desktop row only at the densest step
				let r = if app.settings.touch_first {
					tree.show(ui)
				} else {
					ui.with_density(Density::MIN, |ui| tree.show(ui))
				};
				(ui.unique_id(), r)
			})
			.inner;
		let modifiers = ui.input(|i| i.modifiers);
		let pointer = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
		// the tree takes Ctrl+A into its own selection, which is the window's Select all
		if r.changed && !pointer && !modifiers.shift && selection.len() > selected {
			intents.push(Intent::Cmd(Cmd::Act(MenuAction::SelectAll), None));
		}
		let mut focused = None;
		for (&Salt(row), response) in r.rows.iter() {
			if Menu::context_requested(rt, response) {
				let sel = &mut app.selection;
				if sel.current() != Some(row) && !matches!(row, Row::Item(i) if sel.contains(i)) {
					sel.select(row);
				}
				if let Some(action) = menus::picked(context_menu(app, row).show_context(rt, response).clicked) {
					intents.push(Intent::On(action, On::Row(Some(row))));
				}
			}
			let on_control = r.clicked.as_ref().is_some_and(|a| a.node == Salt(row));
			if response.clicked() {
				// the row's keys need the focus, which a click does not give
				response.request_focus();
				if !on_control && !matches!(row, Row::Folder(_)) {
					press(&mut app.selection, &app.document.project, row, modifiers.command, modifiers.shift);
				}
			}
			if response.has_focus() {
				focused = Some(row);
				if response.ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
					asked.form = true;
				}
				if matches!(row, Row::Item(_)) && response.ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F2)) {
					asked.name = true;
				}
			}
		}
		// the arrows, Home and End move the focus: the row it lands on is selected
		if let Some(row) = focused.filter(|r| self.focused != Some(*r) && !pointer && !matches!(r, Row::Folder(_))) {
			press(&mut app.selection, &app.document.project, row, false, modifiers.shift);
		}
		self.focused = focused;
		match r.clicked.map(|a| (a.node.0, a.rect)) {
			Some((Row::Item(target), _)) => out.push(Event::Command(Command::ToggleEnabled { target })),
			Some((Row::Folder(ItemKind::Source), _)) => intents.push(Intent::On(MenuAction::AddSource, On::Current)),
			Some((Row::Folder(ItemKind::Scheme), _)) => {
				out.push(Event::Command(Command::AddScheme));
				asked.form = true;
			}
			// a second press on the open menu's `+` closes it
			Some((Row::Folder(kind), rect)) => {
				self.adding = (self.adding.map(|(k, _)| k) != Some(kind)).then_some((kind, rect));
			}
			_ => {}
		}
		if let Some(action) = self.add_menu(ui, app) {
			intents.push(Intent::On(action, On::Current));
		}
		if let Some(m) = r.moved
			&& let Row::Item(item) = m.node.0
		{
			let project = &app.document.project;
			let upper = m.position == DropPosition::Before;
			if let Some(to) = drop_at(project, item, m.target.0, upper)
				&& let Some(from) = project.index_of(item)
			{
				let to = reorder_index(from, to, item.kind().len(project));
				out.push(Event::Command(Command::Reorder { target: item, to }));
			}
		}
		if focused.is_some() {
			for (key, action) in TREE_KEYS {
				shortcut::bind(ui.ctx(), Scope::Focus(scope), key, Cmd::Act(action).id());
			}
		}
		asked
	}

	/// The open Add menu of parts or probes at its `+`; the action picked.
	fn add_menu(&mut self, ui: &Mui<'_>, app: &App) -> Option<MenuAction> {
		let (kind, anchor) = self.adding?;
		let s = &app.strings;
		let rows = if kind == ItemKind::Probe { menus::probe_rows(s) } else { menus::part_rows(s) };
		let menu = Menu::new(("outline-add", Salt(Row::Folder(kind))), s.get(add_tip(kind)));
		let menu = menus::action_menu(app, menu, &rows, On::Current);
		let mut open = true;
		let picked = menus::picked(menu.show_at(ui.rt(), anchor, &mut open).clicked);
		if !open {
			self.adding = None;
		}
		picked
	}
}

/// A row's trailing action: a folder's `+`, an item's Enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, KeySalt)]
enum RowAction {
	Add,
	Toggle,
}

/// The tip of a folder's `+`.
fn add_tip(kind: ItemKind) -> &'static str {
	match kind {
		ItemKind::Source => "TipAddSource",
		ItemKind::Part => "TipAddPart",
		ItemKind::Probe => "TipAddProbe",
		ItemKind::Scheme => "TipAdd",
		_ => "TipAdd",
	}
}

/// Every command a key of the outline may fire; Enter and F2 are read by [`Outline::show`].
pub(crate) fn keyed() -> impl Iterator<Item = Cmd> {
	TREE_KEYS.iter().map(|&(_, a)| Cmd::Act(a))
}

/// The row's context menu (docs/ui/10 Project tree row).
fn context_menu(app: &App, row: Row) -> Menu<menus::MenuKey> {
	let menu = Menu::new(("outline-menu", Salt(row)), app.strings.get("NavMore"));
	menus::action_menu(app, menu, &menus::row_menu(app, row), On::Row(Some(row)))
}

/// A row's leading slot: a lens's own profile, a scheme's gradient swatch, else the row's Material icon.
fn leading(project: &Project, row: Row) -> ListSlot {
	let item = match row {
		Row::Item(i) => Some(i),
		_ => None,
	};
	match item {
		Some(ItemRef::Part(id)) => match project.part(id).map(|p| &p.kind) {
			Some(PartKind::Lens(lens)) => {
				let icon = drawn::lens_icon(lens);
				ListSlot::new().paint(LeadingPaint::new(move |p, rect, color| drawn::paint(p, rect, icon, color)))
			}
			_ => ListSlot::new().icon(row_icon(project, row)),
		},
		Some(ItemRef::Scheme(id)) => match project.color_scheme(id) {
			Some(scheme) => {
				let stops = scheme.stops.clone();
				ListSlot::new().paint(LeadingPaint::new(move |p, rect, color| {
					let swatch = rect.shrink2(Vec2::new(0., rect.height() / 4.));
					p.add(Shape::Mesh(Arc::new(gradient_mesh(swatch, &stops))));
					p.rect_stroke(swatch, 0., Stroke::new(0.5, color), StrokeKind::Outside);
				}))
			}
			None => ListSlot::new().icon(row_icon(project, row)),
		},
		_ => ListSlot::new().icon(row_icon(project, row)),
	}
}
