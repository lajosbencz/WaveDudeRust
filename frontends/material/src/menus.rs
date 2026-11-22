//! The menus (DESIGN.md, Commands, menus, keys): `waveduderust_app::menus::MENU_BAR` with this window's View menu and
//! additions, shown as the menu bar (Expanded and up), as submenus of the top bar's menu button (below) and as the
//! command palette's entries.

use egui_mineral::action::Widen;
use egui_mineral::semantic::KeySalt;
use egui_mineral_material::prelude::{MaterialIcon, Menu, MenuItem, MenuStrip, PaletteCommand, icons};
use waveduderust::model::{ItemKind, ItemRef, ProbeDetector, Project};
use waveduderust_app::commands::NewPart;
use waveduderust_app::menus::{
	Entry, MENU_BAR, MenuAction, Needs, On, PART_MENU, PROBABILISTIC, PROBE_KINDS, PartEntry, enabled, part_kinds,
};
use waveduderust_app::preview::PaneKey;
use waveduderust_app::selection::Row as TreeRow;
use waveduderust_app::settings::Orientation;
use waveduderust_app::settings_table::{BUILD, ROWS, Setting, ZOOMS, shown_sections};
use waveduderust_app::strings::{Strings, mnemonic, strip_mnemonic};

use crate::App;
use crate::commands::{self, Cmd, Layers, ViewCommand, key_of, runs};
use crate::route::Route;
use crate::workspace::Columns;

/// A menu row; `text` keeps its mnemonic marker; `off` disables an item the window lacks the means for (File >
/// Export last without a last export); `tip` describes the entry.
pub(crate) enum Row {
	Item { cmd: Cmd, text: String, icon: Option<MaterialIcon>, selected: bool, off: bool, tip: Option<&'static str> },
	Sub { key: &'static str, text: String, icon: MaterialIcon, rows: Vec<Row> },
	Divider,
}

/// A menu of the bar.
pub(crate) struct Group {
	pub(crate) key: &'static str,
	pub(crate) text: String,
	pub(crate) rows: Vec<Row>,
}
/// The Material symbol of a menu entry's `icon`, from the shared table.
fn material(icon: waveduderust_app::menus::Icon) -> MaterialIcon {
	let symbol = waveduderust_egui_platform::icons::material(icon).codepoint;
	MaterialIcon::new(symbol.chars().next().expect("a symbol is one code point"))
}

fn item(s: &Strings, cmd: Cmd, key: &str, icon: Option<MaterialIcon>, selected: bool) -> Row {
	Row::Item { cmd, text: s.get(key).to_owned(), icon, selected, off: false, tip: None }
}

/// The rows of `entries` as this build shows them; the recent files' paths carry no mnemonic. `has_last` enables File
/// > Export last.
fn rows_of(app: &App, entries: &'static [Entry], has_last: bool) -> Vec<Row> {
	let s = &app.strings;
	let mut rows = Vec::new();
	for entry in entries {
		match *entry {
			Entry::Item(i) if i.show.now() => {
				let (key, icon) = match i.alt.filter(|a| (a.when)(app)) {
					Some(alt) => (alt.key, Some(alt.icon)),
					None => (i.key, i.icon),
				};
				let mut row = item(s, Cmd::Act(i.action), key, icon.map(material), false);
				if let Row::Item { off, .. } = &mut row {
					*off = i.needs == Needs::LastExport && !has_last;
				}
				rows.push(row);
			}
			Entry::Submenu { key, icon, show, entries } if show.now() => {
				rows.push(Row::Sub {
					key,
					text: s.get(key).to_owned(),
					icon: material(icon),
					rows: rows_of(app, entries, has_last),
				});
			}
			Entry::Separator => rows.push(Row::Divider),
			Entry::Recent => {
				for (i, path) in app.settings.recent_files.iter().enumerate() {
					let text = path.display().to_string().replace('_', "__");
					let cmd = Cmd::Act(MenuAction::Recent(i));
					rows.push(Row::Item { cmd, text, icon: None, selected: false, off: false, tip: None });
				}
			}
			Entry::Item(_) | Entry::Submenu { .. } | Entry::Panes => {}
		}
	}
	rows
}

/// The View menu: columns, pictures, layout, canvas zoom, UI zoom and the palette.
fn view_rows(app: &App, columns: Columns) -> Vec<Row> {
	let s = &app.strings;
	let v = |view, key, icon, selected| item(s, Cmd::View(view), key, Some(icon), selected);
	let layers = Layers::of(app);
	let side = app.settings.preview_orientation == Orientation::Horizontal;
	let d = &app.document.view;
	let zooms = ZOOMS
		.iter()
		.enumerate()
		.map(|(i, &(z, key))| {
			let now = (z - app.settings.ui_zoom).abs() < 1e-6;
			let cmd = Cmd::View(ViewCommand::UiZoom(i));
			Row::Item { cmd, text: s.get(key).to_owned(), icon: None, selected: now, off: false, tip: None }
		})
		.collect();
	vec![
		v(ViewCommand::Outline, "MenuViewOutline", icons::ICON_ACCOUNT_TREE, columns.outline),
		v(ViewCommand::Inspector, "MenuViewInspector", icons::ICON_TUNE, columns.inspector),
		v(ViewCommand::Messages, "MenuViewMessages", icons::ICON_NOTES, false),
		v(ViewCommand::CanvasOnly, "MenuViewCanvasOnly", icons::ICON_FULLSCREEN, columns.canvas_only()),
		Row::Divider,
		v(
			ViewCommand::Layers(Layers::Field),
			Layers::Field.label_key(),
			icons::ICON_WAVES,
			layers == Some(Layers::Field),
		),
		v(
			ViewCommand::Layers(Layers::Both),
			Layers::Both.label_key(),
			icons::ICON_VIEW_COLUMN_2,
			layers == Some(Layers::Both),
		),
		v(
			ViewCommand::Layers(Layers::Intensity),
			Layers::Intensity.label_key(),
			icons::ICON_LIGHT_MODE,
			layers == Some(Layers::Intensity),
		),
		Row::Divider,
		v(ViewCommand::Orientation(Orientation::Horizontal), "MenuViewSideBySide", icons::ICON_VIEW_COLUMN_2, side),
		v(ViewCommand::Orientation(Orientation::Vertical), "MenuViewStacked", icons::ICON_VIEW_AGENDA, !side),
		Row::Divider,
		v(ViewCommand::Handles, "MenuViewHandles", icons::ICON_DRAG_PAN, d.handles_in_preview),
		v(ViewCommand::Labels, "MenuViewLabels", icons::ICON_LABEL, d.labels_in_preview),
		Row::Divider,
		v(ViewCommand::Fit, "MenuViewFit", icons::ICON_FIT_SCREEN, false),
		v(ViewCommand::ZoomIn, "MenuViewZoomIn", icons::ICON_ZOOM_IN, false),
		v(ViewCommand::ZoomOut, "MenuViewZoomOut", icons::ICON_ZOOM_OUT, false),
		Row::Divider,
		Row::Sub {
			key: "MenuViewUiZoom",
			text: s.get("MenuViewUiZoom").to_owned(),
			icon: icons::ICON_TEXT_INCREASE,
			rows: zooms,
		},
		v(ViewCommand::Palette, "MenuViewPalette", icons::ICON_SEARCH, false),
	]
}

/// Live, Loop and the export range, after the Simulation menu's own.
fn playback_rows(app: &App) -> Vec<Row> {
	let s = &app.strings;
	let p = |key: PaneKey, text, icon, selected| item(s, Cmd::View(ViewCommand::Pane(key)), text, Some(icon), selected);
	vec![
		Row::Divider,
		p(PaneKey::ToggleLive, "MenuSimulationLive", icons::ICON_RADIO_BUTTON_CHECKED, app.live.is_live()),
		p(PaneKey::ToggleLoop, "MenuSimulationLoop", icons::ICON_REPEAT, app.live.playback.looping()),
		p(PaneKey::SetIn, "MenuSimulationSetIn", icons::ICON_LINE_START, false),
		p(PaneKey::SetOut, "MenuSimulationSetOut", icons::ICON_LINE_END, false),
		p(PaneKey::ClearRange, "MenuSimulationClearRange", icons::ICON_CLOSE, false),
	]
}

/// A part kind's icon.
pub(crate) fn part_icon(kind: NewPart) -> MaterialIcon {
	match kind {
		NewPart::Lens => icons::ICON_LENS_BLUR,
		NewPart::FresnelLens => icons::ICON_BLUR_CIRCULAR,
		NewPart::Polygon => icons::ICON_HEXAGON,
		NewPart::Rectangle => icons::ICON_RECTANGLE,
		NewPart::Ellipse => icons::ICON_CIRCLE,
		NewPart::Triangle => icons::ICON_CHANGE_HISTORY,
		NewPart::Outline => icons::ICON_POLYLINE,
		NewPart::Diffuser => icons::ICON_BLUR_ON,
		NewPart::InterferenceFilter => icons::ICON_FILTER_LIST,
		NewPart::SimpleGrating => icons::ICON_GRID_ON,
		NewPart::FacettedGrating => icons::ICON_TEXTURE,
		NewPart::MetaLens | NewPart::MetaPrism | NewPart::MetaAxicon => icons::ICON_GRAIN,
		NewPart::Fiber => icons::ICON_ROUTE,
		NewPart::BraggReflector => icons::ICON_STACKS,
		NewPart::GrinLens => icons::ICON_CONTRAST,
		NewPart::XorGate | NewPart::AndGate | NewPart::OrGate => icons::ICON_MEMORY,
		NewPart::ZonePlate => icons::ICON_ALBUM,
	}
}

/// A list's icon.
pub(crate) fn kind_icon(kind: ItemKind) -> MaterialIcon {
	match kind {
		ItemKind::Source => icons::ICON_WIFI_TETHERING,
		ItemKind::Part => icons::ICON_CATEGORY,
		ItemKind::Probe => icons::ICON_SENSORS,
		ItemKind::Scheme => icons::ICON_GRADIENT,
		_ => icons::ICON_CATEGORY,
	}
}

/// The icon of an outline row; a part's is its kind's.
pub(crate) fn row_icon(project: &Project, row: TreeRow) -> MaterialIcon {
	match row {
		TreeRow::Project => icons::ICON_DESCRIPTION,
		TreeRow::Scene => icons::ICON_LANDSCAPE,
		TreeRow::Simulation => icons::ICON_FUNCTIONS,
		TreeRow::Display => icons::ICON_PALETTE,
		TreeRow::Folder(kind) => kind_icon(kind),
		TreeRow::Item(ItemRef::Part(id)) => {
			project.part(id).and_then(|p| NewPart::of(&p.kind)).map_or(icons::ICON_CATEGORY, part_icon)
		}
		TreeRow::Item(i) => kind_icon(i.kind()),
	}
}

/// The Add part rows: the kinds, gratings, meta surfaces and logic gates in submenus.
pub(crate) fn part_rows(s: &Strings) -> Vec<Row> {
	let kinds = |from, to| -> Vec<Row> {
		part_kinds(from, to)
			.iter()
			.map(|&(kind, key)| {
				let tip = Some(s.kind(kind.type_name()));
				let cmd = Cmd::Act(MenuAction::AddPart(kind));
				Row::Item {
					cmd,
					text: s.get(key).to_owned(),
					icon: Some(part_icon(kind)),
					selected: false,
					off: false,
					tip,
				}
			})
			.collect()
	};
	let mut rows = Vec::new();
	for entry in PART_MENU {
		match entry {
			PartEntry::Kinds(from, to) => rows.extend(kinds(from, to)),
			PartEntry::Sub(key, from, to) => {
				rows.push(Row::Sub { key, text: s.get(key).to_owned(), icon: part_icon(from), rows: kinds(from, to) });
			}
			PartEntry::Separator => rows.push(Row::Divider),
		}
	}
	rows
}

/// The Add probe rows: the standard probe, then the probabilistic detectors in a submenu.
pub(crate) fn probe_rows(s: &Strings) -> Vec<Row> {
	let probe = |&(detector, key, tip): &(ProbeDetector, &'static str, &'static str)| {
		let cmd = Cmd::Act(MenuAction::AddProbe(detector));
		let text = s.get(key).to_owned();
		Row::Item { cmd, text, icon: Some(icons::ICON_SENSORS), selected: false, off: false, tip: Some(s.get(tip)) }
	};
	vec![
		probe(&PROBE_KINDS[0]),
		Row::Sub {
			key: PROBABILISTIC,
			text: s.get(PROBABILISTIC).to_owned(),
			icon: icons::ICON_RADAR,
			rows: PROBE_KINDS[1..].iter().map(probe).collect(),
		},
	]
}

/// Add source, Add part and Add probe.
pub(crate) fn add_rows(s: &Strings) -> Vec<Row> {
	vec![
		item(s, Cmd::Act(MenuAction::AddSource), "MenuProjectAddSource", Some(icons::ICON_WIFI_TETHERING), false),
		Row::Sub {
			key: "MenuProjectAddPart",
			text: s.get("MenuProjectAddPart").to_owned(),
			icon: icons::ICON_CATEGORY,
			rows: part_rows(s),
		},
		Row::Sub {
			key: "MenuProjectAddProbe",
			text: s.get("MenuProjectAddProbe").to_owned(),
			icon: icons::ICON_SENSORS,
			rows: probe_rows(s),
		},
	]
}

/// Add to or remove from my schemes, by whether `scheme` is one of the user's.
fn scheme_row(app: &App, scheme: ItemRef) -> Row {
	let s = &app.strings;
	let name = scheme.name(&app.document.project).unwrap_or_default();
	let (key, icon) = if app.is_user_scheme(name) {
		("MenuSchemeRemoveFromMine", icons::ICON_BOOKMARK_REMOVE)
	} else {
		("MenuSchemeAddToMine", icons::ICON_BOOKMARK_ADD)
	};
	item(s, Cmd::Act(MenuAction::ToggleUserScheme), key, Some(icon), false)
}

/// The Edit menu's item rows after its own: Duplicate, Enabled, the moves, the adds and my schemes.
fn edit_rows(app: &App) -> Vec<Row> {
	let s = &app.strings;
	let a = |action, key, icon| item(s, Cmd::Act(action), key, Some(icon), false);
	let mut rows = vec![
		Row::Divider,
		a(MenuAction::Duplicate, "MenuProjectDuplicate", icons::ICON_FILE_COPY),
		a(MenuAction::ToggleEnabled, "MenuProjectToggleEnabled", icons::ICON_TOGGLE_ON),
		a(MenuAction::MoveUp, "MenuProjectMoveUp", icons::ICON_ARROW_UPWARD),
		a(MenuAction::MoveDown, "MenuProjectMoveDown", icons::ICON_ARROW_DOWNWARD),
		Row::Divider,
	];
	rows.extend(add_rows(s));
	if let Some(item) = app.selection.current_item() {
		rows.extend(item_rows(app, item));
	}
	rows
}

/// An outline row's context menu (docs/ui/10 Project tree row).
pub(crate) fn row_menu(app: &App, row: TreeRow) -> Vec<Row> {
	let s = &app.strings;
	let a = |action, key, icon| item(s, Cmd::Act(action), key, Some(icon), false);
	let mut rows = add_rows(s);
	rows.extend([
		Row::Divider,
		a(MenuAction::ToggleSelected, "MenuProjectToggleSelected", icons::ICON_CHECK_BOX),
		a(MenuAction::SelectAll, "MenuProjectSelectAll", icons::ICON_SELECT_ALL),
		Row::Divider,
		a(MenuAction::ToggleEnabled, "MenuProjectToggleEnabled", icons::ICON_TOGGLE_ON),
		a(MenuAction::Duplicate, "MenuProjectDuplicate", icons::ICON_FILE_COPY),
		a(MenuAction::Cut, "MenuEditCut", icons::ICON_CONTENT_CUT),
		a(MenuAction::Copy, "MenuEditCopy", icons::ICON_CONTENT_COPY),
		a(MenuAction::Paste, "MenuEditPaste", icons::ICON_CONTENT_PASTE),
		a(MenuAction::Delete, "MenuProjectRemove", icons::ICON_DELETE),
		Row::Divider,
		a(MenuAction::MoveUp, "MenuProjectMoveUp", icons::ICON_ARROW_UPWARD),
		a(MenuAction::MoveDown, "MenuProjectMoveDown", icons::ICON_ARROW_DOWNWARD),
	]);
	if let TreeRow::Item(item) = row {
		rows.extend(item_rows(app, item));
	}
	rows
}

/// A probe's Export probe data as CSV, a scheme's my schemes entry.
fn item_rows(app: &App, of: ItemRef) -> Vec<Row> {
	let s = &app.strings;
	match of.kind() {
		ItemKind::Probe => vec![
			Row::Divider,
			item(s, Cmd::Act(MenuAction::ExportProbeCsv), "MenuExportProbeCsv", Some(icons::ICON_IOS_SHARE), false),
		],
		ItemKind::Scheme => vec![Row::Divider, scheme_row(app, of)],
		_ => Vec::new(),
	}
}

/// The inspector header's overflow: Cut, Copy, the moves, a probe's CSV and my schemes.
pub(crate) fn header_menu(app: &App) -> Vec<Row> {
	let s = &app.strings;
	let a = |action, key, icon| item(s, Cmd::Act(action), key, Some(icon), false);
	let mut rows = vec![
		a(MenuAction::Cut, "MenuEditCut", icons::ICON_CONTENT_CUT),
		a(MenuAction::Copy, "MenuEditCopy", icons::ICON_CONTENT_COPY),
		Row::Divider,
		a(MenuAction::MoveUp, "MenuProjectMoveUp", icons::ICON_ARROW_UPWARD),
		a(MenuAction::MoveDown, "MenuProjectMoveDown", icons::ICON_ARROW_DOWNWARD),
		Row::Divider,
		a(MenuAction::Duplicate, "MenuProjectDuplicate", icons::ICON_FILE_COPY),
		a(MenuAction::Delete, "MenuProjectRemove", icons::ICON_DELETE),
	];
	if let Some(item) = app.selection.current_item() {
		rows.extend(item_rows(app, item));
	}
	rows
}

/// The value of a menu entry: its command, or the key of the submenu it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum MenuKey {
	Cmd(Cmd),
	Sub(&'static str),
}

impl KeySalt for MenuKey {
	fn write_key(&self, out: &mut String) {
		match self {
			Self::Cmd(cmd) => cmd.write_key(out),
			Self::Sub(key) => out.push_str(key),
		}
	}
}

/// `menu` reporting [`MenuKey`]s; an empty `items` widens it, `Menu::widen` being crate-private.
fn keyed<A: Widen<MenuKey>>(menu: Menu<A>) -> Menu<MenuKey> {
	menu.items(None::<(MenuKey, MenuItem)>)
}

/// `menu` with `rows` acting on `on` rather than firing commands; each entry reports its [`MenuAction`]
/// ([`picked`]).
pub(crate) fn action_menu<A: Widen<MenuKey>>(app: &App, menu: Menu<A>, rows: &[Row], on: On) -> Menu<MenuKey> {
	rows.iter().fold(keyed(menu), |menu, r| match r {
		Row::Divider => menu.divider(),
		Row::Item { cmd: cmd @ Cmd::Act(action), text, icon, tip, .. } => {
			let item = MenuItem::new(strip_mnemonic(text)).disabled(!runs_on(app, *action, on));
			menu.item(MenuKey::Cmd(*cmd), decorated(item, *icon, *tip))
		}
		Row::Item { .. } => menu,
		Row::Sub { key, text, icon, rows } => {
			let item = MenuItem::new(strip_mnemonic(text)).leading(*icon);
			menu.submenu(MenuKey::Sub(key), item, |m| action_menu(app, m, rows, on))
		}
	})
}

/// The action of an [`action_menu`]'s entry clicked.
pub(crate) fn picked(clicked: Option<MenuKey>) -> Option<MenuAction> {
	match clicked? {
		MenuKey::Cmd(Cmd::Act(action)) => Some(action),
		_ => None,
	}
}

/// `item` with its leading `icon` and its `tip`.
fn decorated(mut item: MenuItem, icon: Option<MaterialIcon>, tip: Option<&str>) -> MenuItem {
	if let Some(icon) = icon {
		item = item.leading(icon);
	}
	match tip {
		Some(tip) => item.tooltip(tip),
		None => item,
	}
}

/// Whether `action` runs on `on`: offered here and enabled by the state.
pub(crate) fn runs_on(app: &App, action: MenuAction, on: On) -> bool {
	commands::offered(action) && enabled(app, action, on)
}

/// `menu` with `rows` as command items, each firing its command.
pub(crate) fn command_menu<A: Widen<MenuKey>>(app: &App, menu: Menu<A>, rows: &[Row]) -> Menu<MenuKey> {
	fill(app, menu, rows)
}

/// The menus of the bar with this window's changes; `has_last` enables File > Export last.
pub(crate) fn groups(app: &App, columns: Columns, has_last: bool) -> Vec<Group> {
	let s = &app.strings;
	MENU_BAR
		.iter()
		.map(|&(header, entries)| {
			let mut rows = match header {
				"MenuView" => view_rows(app, columns),
				_ => rows_of(app, entries, has_last),
			};
			match header {
				"MenuEdit" => rows.extend(edit_rows(app)),
				"MenuSimulation" => rows.extend(playback_rows(app)),
				"MenuHelp" => rows.push(item(
					s,
					Cmd::View(ViewCommand::Shortcuts),
					"MenuHelpShortcuts",
					Some(icons::ICON_KEYBOARD),
					false,
				)),
				_ => {}
			}
			Group { key: header, text: s.get(header).to_owned(), rows }
		})
		.collect()
}

/// Every command of `groups`, nested ones included.
pub(crate) fn commands(groups: &[Group]) -> Vec<Cmd> {
	fn walk(rows: &[Row], out: &mut Vec<Cmd>) {
		for row in rows {
			match row {
				Row::Item { cmd, .. } => out.push(*cmd),
				Row::Sub { rows, .. } => walk(rows, out),
				Row::Divider => {}
			}
		}
	}
	let mut out = Vec::new();
	for g in groups {
		walk(&g.rows, &mut out);
	}
	out
}

/// Whether one item of `rows` runs.
fn any_runs(app: &App, rows: &[Row]) -> bool {
	rows.iter().any(|r| match r {
		Row::Item { cmd, off, .. } => !off && runs(app, *cmd),
		Row::Sub { rows, .. } => any_runs(app, rows),
		Row::Divider => false,
	})
}

/// The mnemonic of `text` unless an earlier entry of its menu took it.
fn letter(text: &str, used: &mut Vec<char>) -> Option<char> {
	let c =
		mnemonic(text).map(|c| c.to_ascii_uppercase()).filter(|c| c.is_ascii_alphanumeric() && !used.contains(c))?;
	used.push(c);
	Some(c)
}

/// `menu` with `rows`, dividers trimmed at the ends and never doubled.
fn fill<A: Widen<MenuKey>>(app: &App, menu: Menu<A>, rows: &[Row]) -> Menu<MenuKey> {
	let mut menu = keyed(menu);
	let mut used = Vec::new();
	let mut divide = false;
	let mut any = false;
	for row in rows {
		let (item, text) = match row {
			Row::Divider => {
				divide = any;
				continue;
			}
			Row::Item { cmd, text, icon, selected, off, tip } => {
				let mut i = MenuItem::new(strip_mnemonic(text))
					.command(cmd.id())
					.selected(*selected)
					.disabled(*off || !runs(app, *cmd));
				if let Some(key) = key_of(*cmd) {
					i = i.trailing(key);
				}
				(decorated(i, *icon, *tip), text)
			}
			Row::Sub { text, icon, rows, .. } => {
				(MenuItem::new(strip_mnemonic(text)).leading(*icon).disabled(!any_runs(app, rows)), text)
			}
		};
		let item = match letter(text, &mut used) {
			Some(c) => item.mnemonic(c),
			None => item,
		};
		if std::mem::take(&mut divide) {
			menu = menu.divider();
		}
		menu = match row {
			Row::Item { cmd, .. } => menu.item(MenuKey::Cmd(*cmd), item),
			Row::Sub { key, rows, .. } => menu.submenu(MenuKey::Sub(key), item, |m| fill(app, m, rows)),
			Row::Divider => unreachable!("dividers continue above"),
		};
		any = true;
	}
	menu
}

/// The menu bar.
pub(crate) fn bar(app: &App, groups: &[Group]) -> MenuStrip<MenuKey> {
	let mut used = Vec::new();
	let strip = MenuStrip::new("menu-bar").menus(None::<Menu<MenuKey>>);
	groups.iter().enumerate().fold(strip, |bar, (i, g)| {
		let mut menu = Menu::new(("menu", i), strip_mnemonic(&g.text));
		if let Some(c) = letter(&g.text, &mut used) {
			menu = menu.mnemonic(c);
		}
		bar.menu(fill(app, menu, &g.rows))
	})
}

/// The top bar's menu: each menu of the bar as a submenu.
pub(crate) fn app_menu(app: &App, groups: &[Group]) -> Menu<MenuKey> {
	let mut used = Vec::new();
	groups.iter().fold(keyed(Menu::new("app-menu", app.strings.get("NavMenu"))), |menu, g| {
		let mut i = MenuItem::new(strip_mnemonic(&g.text));
		if let Some(c) = letter(&g.text, &mut used) {
			i = i.mnemonic(c);
		}
		menu.submenu(MenuKey::Sub(g.key), i, |m| fill(app, m, &g.rows))
	})
}

/// What a palette entry without a command does, its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Target {
	Go(Route),
	Example(usize),
	/// Selects the item and focuses the inspector.
	Item(ItemRef),
}

impl KeySalt for Target {
	fn write_key(&self, out: &mut String) {
		match self {
			Self::Go(route) => ("page", route.title_key()).write_key(out),
			Self::Example(i) => ("example", *i).write_key(out),
			Self::Item(item) => ("item", crate::Salt(*item)).write_key(out),
		}
	}
}

/// The command palette's entries: every menu item (greyed while it cannot run, with its key; a submenu's under the
/// submenu's name), the pages, the examples, the settings rows and "Go to" each item.
#[derive(Default)]
pub(crate) struct Palette {
	pub(crate) open: bool,
	pub(crate) query: String,
}

impl Palette {
	/// The entries of this pass; an entry without a command reports its [`Target`].
	pub(crate) fn entries(app: &App, groups: &[Group]) -> Vec<(Option<Target>, PaletteCommand)> {
		fn walk(app: &App, rows: &[Row], group: &str, out: &mut Vec<(Option<Target>, PaletteCommand)>) {
			for row in rows {
				match row {
					Row::Item { cmd, text, icon, off, .. } => {
						let mut c = PaletteCommand::new(strip_mnemonic(text))
							.group(group)
							.command(cmd.id(), key_of(*cmd))
							.disabled(*off || !runs(app, *cmd));
						if let Some(icon) = icon {
							c = c.icon(*icon);
						}
						out.push((None, c));
					}
					Row::Sub { text, rows, .. } => walk(app, rows, &strip_mnemonic(text), out),
					Row::Divider => {}
				}
			}
		}
		let s = &app.strings;
		let mut entries = Vec::new();
		for g in groups {
			walk(app, &g.rows, &strip_mnemonic(&g.text), &mut entries);
		}
		let pages = strip_mnemonic(s.get("NavPages"));
		for route in Route::PAGES {
			let c =
				PaletteCommand::new(s.get(route.title_key())).group(&pages).icon(route.icon()).salt(Target::Go(route));
			entries.push((Some(Target::Go(route)), c));
		}
		let examples = s.get("ExamplesTitle");
		for (i, example) in waveduderust_examples::all().iter().enumerate() {
			entries.push((
				Some(Target::Example(i)),
				PaletteCommand::new(&example.title).group(examples).salt(Target::Example(i)),
			));
		}
		let settings = s.get("SettingsTitle");
		let sections = shown_sections(BUILD);
		for row in ROWS.iter().filter(|r| r.need.met(BUILD) && r.setting != Setting::Language) {
			let section = sections.iter().position(|&sec| sec == row.section);
			let c = PaletteCommand::new(strip_mnemonic(s.get(row.label))).group(settings).salt(("setting", row.label));
			entries.push((Some(Target::Go(Route::Settings { section })), c));
		}
		let items = s.get("PaletteItems");
		let project = &app.document.project;
		for kind in ItemKind::ALL {
			for item in kind.items(project) {
				let text = s.format("PaletteGoTo", &[&item.name(project).unwrap_or_default()]);
				let c = PaletteCommand::new(text)
					.group(items)
					.icon(row_icon(project, TreeRow::Item(item)))
					.salt(Target::Item(item));
				entries.push((Some(Target::Item(item)), c));
			}
		}
		entries
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_menu_letter_goes_to_its_first_entry() {
		let mut used = Vec::new();
		assert_eq!(letter("_File", &mut used), Some('F'));
		assert_eq!(letter("_First", &mut used), None);
		assert_eq!(letter("no mark", &mut used), None);
		assert_eq!(letter("E_xit", &mut used), Some('X'));
	}
}
