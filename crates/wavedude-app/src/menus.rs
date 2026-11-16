//! The tables of the menus of docs/ui/10-menus-and-shortcuts.md: the menu bar, the part kinds, the [`MenuAction`]s
//! that the menu bar, the context menus and the shortcuts share, when each is enabled, and Move up and Move down.

use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{
	ICON_CLOSE, ICON_CONTENT_COPY, ICON_CONTENT_CUT, ICON_CONTENT_PASTE, ICON_DELETE, ICON_DELETE_SWEEP,
	ICON_FOLDER_OPEN, ICON_HISTORY, ICON_IMAGE, ICON_INFO, ICON_IOS_SHARE, ICON_LIST_ALT, ICON_LOGOUT, ICON_MENU_BOOK,
	ICON_NOTE_ADD, ICON_PAUSE, ICON_PLAY_ARROW, ICON_REDO, ICON_SAVE, ICON_SAVE_AS, ICON_SELECT_ALL, ICON_SETTINGS,
	ICON_STOP, ICON_UNDO, ICON_VIEW_SIDEBAR, ICON_WAVING_HAND,
};
use wavedude_app_state::AppState;
use wavedude_app_state::clipboard::Copyable;
use wavedude_app_state::commands::NewPart;
use wavedude_app_state::live::Backend;
use wavedude_app_state::platform::HAS_PATHS;
use wavedude_app_state::selection::Row;
use wavedude_core::model::{ItemKind, ItemRef, ProbeDetector};

use crate::App;
use crate::window::Pane;

/// The Add part menu's entries: kind, string key.
pub const PART_KINDS: [(NewPart, &str); 21] = [
	(NewPart::Lens, "MenuAddLens"),
	(NewPart::FresnelLens, "MenuAddFresnelLens"),
	(NewPart::Fiber, "MenuAddFiber"),
	(NewPart::BraggReflector, "MenuAddBraggReflector"),
	(NewPart::GrinLens, "MenuAddGrinLens"),
	(NewPart::ZonePlate, "MenuAddZonePlate"),
	(NewPart::Diffuser, "MenuAddDiffuser"),
	(NewPart::InterferenceFilter, "MenuAddInterferenceFilter"),
	(NewPart::SimpleGrating, "MenuAddSimpleGrating"),
	(NewPart::FacettedGrating, "MenuAddFacettedGrating"),
	(NewPart::MetaLens, "MenuAddMetaLens"),
	(NewPart::MetaPrism, "MenuAddMetaPrism"),
	(NewPart::MetaAxicon, "MenuAddMetaAxicon"),
	(NewPart::Polygon, "MenuAddPolygon"),
	(NewPart::Rectangle, "MenuAddRectangle"),
	(NewPart::Ellipse, "MenuAddEllipse"),
	(NewPart::Triangle, "MenuAddTriangle"),
	(NewPart::Outline, "MenuAddOutline"),
	(NewPart::XorGate, "MenuAddXorGate"),
	(NewPart::AndGate, "MenuAddAndGate"),
	(NewPart::OrGate, "MenuAddOrGate"),
];

/// What an export command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKindChoice {
	Field,
	Intensity,
	Both,
	FieldFrame,
	IntensityFrame,
}

/// A step of the UI zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomStep {
	In,
	Out,
	/// The device's default.
	Reset,
}

/// What an item action applies to: the current item, or the row a context menu opened on (none: the empty tree).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum On {
	Current,
	Row(Option<Row>),
}

/// What a menu entry or a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
	New,
	Open,
	/// Entry `n` of the recent files.
	Recent(usize),
	/// The Examples dialog.
	Examples,
	/// The welcome page.
	Welcome,
	/// The Settings window.
	Settings,
	Save,
	SaveAs,
	Export(ExportKindChoice),
	ExportLast,
	Close,
	Exit,
	Undo,
	Redo,
	Cut,
	Copy,
	Paste,
	Delete,
	SelectAll,
	/// Shows or hides a pane.
	Pane(Pane),
	/// The UI zoom, by its keys.
	Zoom(ZoomStep),
	Run,
	PauseRun,
	Stop,
	DiscardFrames,
	Play,
	CheckForUpdates,
	About,
	Manual,
	AddSource,
	AddPart(NewPart),
	AddProbe(ProbeDetector),
	ToggleSelected,
	ToggleEnabled,
	Duplicate,
	MoveUp,
	MoveDown,
	ExportProbeCsv,
	/// Adds a project colour scheme to the user's, or removes the user's of its name.
	ToggleUserScheme,
}

/// When an entry of the menu bar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
	Always,
	/// The desktop only.
	Native,
	/// Where files have paths.
	Paths,
	/// Builds with the updater.
	Updater,
}

impl Show {
	/// Whether this build shows the entry.
	pub fn now(self) -> bool {
		match self {
			Self::Always => true,
			Self::Native => cfg!(not(target_arch = "wasm32")),
			Self::Paths => HAS_PATHS,
			#[cfg(not(target_arch = "wasm32"))]
			Self::Updater => crate::update::AVAILABLE,
			#[cfg(target_arch = "wasm32")]
			Self::Updater => false,
		}
	}
}

/// What the window holds for an entry beyond [`enabled`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
	Nothing,
	/// Objects on the clipboard.
	Pastable,
	/// A last export of the open project.
	LastExport,
}

/// The label an entry shows in place of its own while `when` holds.
#[derive(Clone, Copy)]
pub struct Alt {
	pub key: &'static str,
	pub icon: MaterialIcon,
	pub when: fn(&App) -> bool,
}

/// An entry of the menu bar: its action, the string key of its label (with its mnemonic) and its icon. Its shortcut
/// is the action's in `ui::shortcuts`.
#[derive(Clone, Copy)]
pub struct Item {
	pub action: MenuAction,
	pub key: &'static str,
	pub icon: Option<MaterialIcon>,
	pub show: Show,
	pub needs: Needs,
	pub alt: Option<Alt>,
}

impl Item {
	const fn new(action: MenuAction, key: &'static str, icon: Option<MaterialIcon>) -> Self {
		Self { action, key, icon, show: Show::Always, needs: Needs::Nothing, alt: None }
	}

	const fn show(self, show: Show) -> Self {
		Self { show, ..self }
	}

	const fn needs(self, needs: Needs) -> Self {
		Self { needs, ..self }
	}

	const fn alt(self, key: &'static str, icon: MaterialIcon, when: fn(&App) -> bool) -> Self {
		Self { alt: Some(Alt { key, icon, when }), ..self }
	}
}

/// A row of a menu of the bar.
#[derive(Clone, Copy)]
pub enum Entry {
	Item(Item),
	/// Enabled while one of its items is.
	Submenu {
		key: &'static str,
		icon: MaterialIcon,
		show: Show,
		entries: &'static [Entry],
	},
	/// Dropped at either end of a menu and after another.
	Separator,
	/// The recent files, their paths without mnemonics.
	Recent,
	/// One checked entry per pane, its title without a mnemonic.
	Panes,
}

const fn item(action: MenuAction, key: &'static str, icon: MaterialIcon) -> Entry {
	Entry::Item(Item::new(action, key, Some(icon)))
}

const fn export(choice: ExportKindChoice, key: &'static str, icon: MaterialIcon) -> Entry {
	item(MenuAction::Export(choice), key, icon)
}

fn run_paused(app: &App) -> bool {
	app.live.run_paused_by_user()
}

fn playing(app: &App) -> bool {
	app.live.playback.is_playing()
}

const FILE: &[Entry] = &[
	item(MenuAction::New, "MenuFileNew", ICON_NOTE_ADD),
	item(MenuAction::Open, "MenuFileOpen", ICON_FOLDER_OPEN),
	Entry::Submenu { key: "MenuFileRecent", icon: ICON_HISTORY, show: Show::Paths, entries: &[Entry::Recent] },
	item(MenuAction::Examples, "MenuFileExamples", ICON_LIST_ALT),
	item(MenuAction::Welcome, "MenuFileWelcome", ICON_WAVING_HAND),
	item(MenuAction::Settings, "MenuFileSettings", ICON_SETTINGS),
	Entry::Separator,
	item(MenuAction::Save, "MenuFileSave", ICON_SAVE),
	item(MenuAction::SaveAs, "MenuFileSaveAs", ICON_SAVE_AS),
	Entry::Separator,
	Entry::Submenu {
		key: "MenuFileExport",
		icon: ICON_IOS_SHARE,
		show: Show::Always,
		entries: &[
			export(ExportKindChoice::Field, "MenuFileExportField", ICON_IOS_SHARE),
			export(ExportKindChoice::Intensity, "MenuFileExportIntensity", ICON_IOS_SHARE),
			export(ExportKindChoice::Both, "MenuFileExportBoth", ICON_IOS_SHARE),
			Entry::Separator,
			export(ExportKindChoice::FieldFrame, "MenuFileExportFieldFrame", ICON_IMAGE),
			export(ExportKindChoice::IntensityFrame, "MenuFileExportIntensityFrame", ICON_IMAGE),
		],
	},
	Entry::Item(Item::new(MenuAction::ExportLast, "MenuFileExportLast", Some(ICON_IOS_SHARE)).needs(Needs::LastExport)),
	Entry::Separator,
	item(MenuAction::Close, "MenuFileClose", ICON_CLOSE),
	Entry::Separator,
	Entry::Item(Item::new(MenuAction::Exit, "MenuFileExit", Some(ICON_LOGOUT)).show(Show::Native)),
];

const EDIT: &[Entry] = &[
	item(MenuAction::Undo, "MenuEditUndo", ICON_UNDO),
	item(MenuAction::Redo, "MenuEditRedo", ICON_REDO),
	Entry::Separator,
	item(MenuAction::Cut, "MenuEditCut", ICON_CONTENT_CUT),
	item(MenuAction::Copy, "MenuEditCopy", ICON_CONTENT_COPY),
	Entry::Item(Item::new(MenuAction::Paste, "MenuEditPaste", Some(ICON_CONTENT_PASTE)).needs(Needs::Pastable)),
	item(MenuAction::Delete, "MenuEditDelete", ICON_DELETE),
	Entry::Separator,
	item(MenuAction::SelectAll, "MenuProjectSelectAll", ICON_SELECT_ALL),
];

const VIEW: &[Entry] =
	&[Entry::Submenu { key: "MenuViewPanes", icon: ICON_VIEW_SIDEBAR, show: Show::Always, entries: &[Entry::Panes] }];

const SIMULATION: &[Entry] = &[
	item(MenuAction::Run, "MenuSimulationRun", ICON_PLAY_ARROW),
	Entry::Item(Item::new(MenuAction::PauseRun, "MenuSimulationPause", Some(ICON_PAUSE)).alt(
		"MenuSimulationResume",
		ICON_PLAY_ARROW,
		run_paused,
	)),
	item(MenuAction::Stop, "MenuSimulationStop", ICON_STOP),
	item(MenuAction::DiscardFrames, "MenuSimulationDiscardFrames", ICON_DELETE_SWEEP),
	Entry::Separator,
	Entry::Item(Item::new(MenuAction::Play, "MenuSimulationPlay", Some(ICON_PLAY_ARROW)).alt(
		"MenuSimulationPausePlayback",
		ICON_PAUSE,
		playing,
	)),
];

const HELP: &[Entry] = &[
	item(MenuAction::About, "MenuHelpAbout", ICON_INFO),
	item(MenuAction::Manual, "MenuHelpManual", ICON_MENU_BOOK),
	Entry::Item(Item::new(MenuAction::CheckForUpdates, "MenuHelpCheckNow", None).show(Show::Updater)),
];

/// The menu bar (docs/ui/10): each menu's header key and its entries.
pub const MENU_BAR: [(&str, &[Entry]); 5] =
	[("MenuFile", FILE), ("MenuEdit", EDIT), ("MenuView", VIEW), ("MenuSimulation", SIMULATION), ("MenuHelp", HELP)];

/// The item an item action on `on` applies to: the current item, or the context menu's row when it is an item.
pub fn target<B: Backend>(app: &AppState<B>, on: On) -> Option<ItemRef> {
	match on {
		On::Current => app.selection.current_item(),
		On::Row(Some(Row::Item(i))) => Some(i),
		On::Row(_) => None,
	}
}

/// Whether `action` on `on` can run now (the Enabled columns of docs/ui/10): its entries are greyed out and the
/// window refuses it otherwise. Paste and Export last also need the clipboard's objects and a last export of the
/// project, which their entries and their actions check.
pub fn enabled<B: Backend>(app: &AppState<B>, action: MenuAction, on: On) -> bool {
	let live = &app.live;
	let history = &app.document.history;
	let project = &app.document.project;
	let target = target(app, on);
	match action {
		MenuAction::Close => !app.document.is_placeholder(),
		MenuAction::Export(_) | MenuAction::ExportLast => !live.export_busy(),
		MenuAction::Undo => history.can_undo() || history.has_pending(),
		MenuAction::Redo => history.can_redo(),
		MenuAction::Cut | MenuAction::Delete | MenuAction::Duplicate | MenuAction::ToggleSelected => target.is_some(),
		MenuAction::Copy => {
			let row = match on {
				On::Current => app.selection.current(),
				On::Row(row) => row,
			};
			(on == On::Current && !app.selection.items().is_empty()) || row.and_then(Copyable::of_row).is_some()
		}
		MenuAction::SelectAll => !(project.sources.is_empty() && project.parts.is_empty() && project.probes.is_empty()),
		MenuAction::Run => !live.run_is_active() && !live.export_busy(),
		MenuAction::PauseRun => live.run_is_active(),
		MenuAction::Stop => live.run_is_active() || live.export_busy(),
		MenuAction::DiscardFrames => live.has_frames() && !live.export_busy(),
		MenuAction::ToggleEnabled => target.is_some_and(|t| t.kind != ItemKind::Scheme),
		MenuAction::ToggleUserScheme => target.is_some_and(|t| t.kind == ItemKind::Scheme),
		MenuAction::MoveUp | MenuAction::MoveDown => {
			target.is_some_and(|t| can_move(app, t, action == MenuAction::MoveUp))
		}
		MenuAction::ExportProbeCsv => {
			let probe = target.filter(|t| t.kind == ItemKind::Probe).and_then(|t| project.probes.get(t.index));
			probe.is_some_and(|p| p.active) && !live.export_busy()
		}
		MenuAction::New
		| MenuAction::Open
		| MenuAction::Recent(_)
		| MenuAction::Examples
		| MenuAction::Welcome
		| MenuAction::Settings
		| MenuAction::Save
		| MenuAction::SaveAs
		| MenuAction::Exit
		| MenuAction::Paste
		| MenuAction::Pane(_)
		| MenuAction::Zoom(_)
		| MenuAction::Play
		| MenuAction::CheckForUpdates
		| MenuAction::About
		| MenuAction::Manual
		| MenuAction::AddSource
		| MenuAction::AddPart(_)
		| MenuAction::AddProbe(_) => true,
	}
}

/// Whether Move up (`up`) or Move down shifts `target` and the items moving with it.
pub fn can_move<B: Backend>(app: &AppState<B>, target: ItemRef, up: bool) -> bool {
	!moves_of(app, target, up).is_empty()
}

/// The reorders `(index, to)` of Move up (`up`) or Move down on `target` and the selected items of its list.
pub fn moves_of<B: Backend>(app: &AppState<B>, target: ItemRef, up: bool) -> Vec<(usize, usize)> {
	let indices = app.selection.targets(target).into_iter().filter(|t| t.kind == target.kind).map(|t| t.index);
	block_moves(indices.collect(), target.kind.len(&app.document.project), up)
}

/// The reorders `(from, to)` shifting the items at `indices` of a list of `len` one place, the leading item first;
/// none when the leading item is at the list's edge, so the block keeps its order.
fn block_moves(mut indices: Vec<usize>, len: usize, up: bool) -> Vec<(usize, usize)> {
	indices.sort_unstable();
	if !up {
		indices.reverse();
	}
	let lead = indices.first().copied();
	if lead.is_none_or(|i| if up { i == 0 } else { i + 1 >= len }) {
		return Vec::new();
	}
	indices.into_iter().map(|i| (i, if up { i - 1 } else { i + 1 })).collect()
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;

	use wavedude_app_state::Event;
	use wavedude_app_state::commands::Command;
	use wavedude_app_state::live::RunNeeds;
	use wavedude_app_state::live::fake::FakeBackend;
	use wavedude_app_state::platform::Instant;
	use wavedude_app_state::settings::Settings;
	use wavedude_app_state::strings::{Language, Strings, mnemonic};
	use wavedude_export::ExportFormat;

	use super::*;

	fn app() -> AppState<FakeBackend> {
		AppState::with_backend(Settings::default(), None, FakeBackend::default(), Arc::new(AtomicBool::new(false)))
	}

	#[test]
	fn an_item_action_needs_its_item() {
		let mut app = app();
		let scene = On::Row(Some(Row::Scene));
		let scheme = On::Row(Some(Row::Item(ItemRef::new(ItemKind::Scheme, 0))));
		for action in [MenuAction::Cut, MenuAction::Delete, MenuAction::Duplicate, MenuAction::ToggleEnabled] {
			assert!(!enabled(&app, action, On::Current), "{action:?} on the project row");
			assert!(!enabled(&app, action, scene), "{action:?} on the Scene row");
		}
		assert!(!enabled(&app, MenuAction::Copy, On::Current), "the project row copies nothing");
		assert!(enabled(&app, MenuAction::Copy, scene));
		assert!(!enabled(&app, MenuAction::SelectAll, On::Current), "an empty project has nothing to select");
		assert!(!enabled(&app, MenuAction::Close, On::Current), "the placeholder closes to itself");
		assert!(
			!enabled(&app, MenuAction::ToggleEnabled, scheme) && enabled(&app, MenuAction::ToggleUserScheme, scheme)
		);
		app.handle(Event::Command(Command::AddSource { at: None }), Instant::now());
		for action in [MenuAction::Cut, MenuAction::Copy, MenuAction::Delete, MenuAction::ToggleEnabled] {
			assert!(enabled(&app, action, On::Current), "{action:?} on the added source");
		}
		assert!(enabled(&app, MenuAction::SelectAll, On::Current) && enabled(&app, MenuAction::Close, On::Current));
		assert!(!enabled(&app, MenuAction::MoveUp, On::Current), "a lone source stays");
		assert!(!enabled(&app, MenuAction::ExportProbeCsv, On::Current), "a source has no probe data");
	}

	#[test]
	fn an_export_refuses_a_run_and_what_needs_its_frames_but_stops() {
		let mut app = app();
		let project = app.document.project.clone();
		assert!(enabled(&app, MenuAction::Run, On::Current) && !enabled(&app, MenuAction::Stop, On::Current));
		app.live.request_export(&project, RunNeeds::new(ExportFormat::Png, false), Instant::now());
		assert!(app.live.export_busy());
		for action in [
			MenuAction::Run,
			MenuAction::DiscardFrames,
			MenuAction::Export(ExportKindChoice::Field),
			MenuAction::ExportLast,
		] {
			assert!(!enabled(&app, action, On::Current), "{action:?} during an export");
		}
		assert!(enabled(&app, MenuAction::Stop, On::Current));
	}

	/// The labels of `entries` by menu, the submenus' after: each entry's key and its alternative's.
	fn menus_of(entries: &'static [Entry], out: &mut Vec<Vec<(&'static str, Option<&'static str>)>>) {
		let mut menu = Vec::new();
		for entry in entries {
			match *entry {
				Entry::Item(item) => menu.push((item.key, item.alt.map(|a| a.key))),
				Entry::Submenu { key, entries, .. } => {
					menu.push((key, None));
					menus_of(entries, out);
				}
				Entry::Separator | Entry::Recent | Entry::Panes => {}
			}
		}
		out.push(menu);
	}

	/// Every entry of every build, whatever it shows; an alternative label may share its entry's letter.
	#[test]
	fn every_menu_entry_has_a_mnemonic_unique_in_its_menu_in_every_language() {
		let mut menus = vec![MENU_BAR.iter().map(|&(key, _)| (key, None)).collect()];
		for (_, entries) in MENU_BAR {
			menus_of(entries, &mut menus);
		}
		let mut faults = Vec::new();
		for language in Language::ALL {
			let s = Strings::new(language);
			for menu in &menus {
				let letters: Vec<Vec<(&str, Option<char>)>> = menu
					.iter()
					.map(|&(key, alt)| {
						[Some(key), alt].into_iter().flatten().map(|k| (k, mnemonic(s.get(k)))).collect()
					})
					.collect();
				for (i, labels) in letters.iter().enumerate() {
					for &(key, letter) in labels {
						let Some(letter) = letter else {
							faults.push(format!("{language:?} {key}: no mnemonic"));
							continue;
						};
						let mut later = letters[i + 1..].iter().flatten();
						let clash = later.find(|&&(_, l)| l == Some(letter));
						if let Some((other, _)) = clash {
							faults.push(format!("{language:?} '{letter}': {key} / {other}"));
						}
					}
				}
			}
		}
		assert!(faults.is_empty(), "{faults:#?}");
	}

	#[test]
	fn a_block_at_the_list_edge_keeps_its_order() {
		assert_eq!(block_moves(vec![1, 0], 3, true), []);
		assert_eq!(block_moves(vec![1, 2], 3, false), []);
		assert_eq!(block_moves(vec![0, 2], 4, true), []);
	}

	#[test]
	fn a_block_moves_with_its_leading_item_first() {
		assert_eq!(block_moves(vec![2, 1], 4, true), [(1, 0), (2, 1)]);
		assert_eq!(block_moves(vec![1, 2], 4, false), [(2, 3), (1, 2)]);
		assert_eq!(block_moves(vec![1, 3], 5, true), [(1, 0), (3, 2)]);
		assert_eq!(block_moves(Vec::new(), 4, true), []);
	}
}
