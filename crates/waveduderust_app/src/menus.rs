//! The tables of the menus of docs/ui/10-menus-and-shortcuts.md: the menu bar, the part kinds, the [`MenuAction`]s
//! that the menu bar, the context menus and the shortcuts share, when each is enabled, and Move up and Move down.

use crate::clipboard::{Copyable, PasteTarget};
use crate::commands::{Command, NewPart};
use crate::live::Backend;
use crate::platform::HAS_PATHS;
use crate::selection::Row;
use crate::{AppState, Event};
use waveduderust::model::{ItemKind, ItemRef, Point, ProbeDetector};

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

/// An entry of the Add part menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartEntry {
	/// The kinds from the first to the last, in [`PART_KINDS`] order.
	Kinds(NewPart, NewPart),
	/// A submenu with its string key and the kinds from the first to the last.
	Sub(&'static str, NewPart, NewPart),
	Separator,
}

/// The Add part menu: the optics, gratings and meta surfaces in submenus, the shapes, and the logic gates.
pub const PART_MENU: [PartEntry; 7] = [
	PartEntry::Kinds(NewPart::Lens, NewPart::InterferenceFilter),
	PartEntry::Sub("MenuAddDiffractionGratings", NewPart::SimpleGrating, NewPart::FacettedGrating),
	PartEntry::Sub("MenuAddMetaSurfaces", NewPart::MetaLens, NewPart::MetaAxicon),
	PartEntry::Separator,
	PartEntry::Kinds(NewPart::Polygon, NewPart::Outline),
	PartEntry::Separator,
	PartEntry::Sub("MenuAddOpticalLogic", NewPart::XorGate, NewPart::OrGate),
];

/// The kinds of [`PART_KINDS`] from `from` to `to`.
pub fn part_kinds(from: NewPart, to: NewPart) -> &'static [(NewPart, &'static str)] {
	let at = |kind| PART_KINDS.iter().position(|(k, _)| *k == kind).expect("a kind of the menu");
	&PART_KINDS[at(from)..=at(to)]
}

/// The Add probe menu's detectors: detector, string key, description key; the first is the standard probe, the
/// others go under [`PROBABILISTIC`].
pub const PROBE_KINDS: [(ProbeDetector, &str, &str); 3] = [
	(ProbeDetector::Standard, "MenuAddStandardProbe", "Detector.Standard"),
	(ProbeDetector::Counting, "MenuAddCountingDetector", "Detector.Counting"),
	(ProbeDetector::Imaging, "MenuAddImagingDetector", "Detector.Imaging"),
];

/// The submenu key of the probabilistic detectors.
pub const PROBABILISTIC: &str = "MenuAddProbabilistic";

/// What an export command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExportKindChoice {
	Field,
	Intensity,
	Both,
	FieldFrame,
	IntensityFrame,
}

/// A step of the UI zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
	ProjectPage,
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

impl MenuAction {
	/// The command's id, `file.new`, which every entry point of a frontend fires.
	pub fn command_id(self) -> String {
		use MenuAction as A;
		let fixed = match self {
			A::New => "file.new",
			A::Open => "file.open",
			A::Recent(i) => return format!("file.recent.{i}"),
			A::Examples => "file.examples",
			A::Welcome => "file.welcome",
			A::Settings => "file.settings",
			A::Save => "file.save",
			A::SaveAs => "file.save_as",
			A::Export(kind) => return key("file.export", export_key(kind)),
			A::ExportLast => "file.export_last",
			A::Close => "file.close",
			A::Exit => "file.exit",
			A::Undo => "edit.undo",
			A::Redo => "edit.redo",
			A::Cut => "edit.cut",
			A::Copy => "edit.copy",
			A::Paste => "edit.paste",
			A::Delete => "edit.delete",
			A::SelectAll => "edit.select_all",
			A::Pane(pane) => return key("view.pane", pane_key(pane)),
			A::Zoom(step) => return key("view.zoom", zoom_key(step)),
			A::Run => "run.run",
			A::PauseRun => "run.pause",
			A::Stop => "run.stop",
			A::DiscardFrames => "run.discard_frames",
			A::Play => "run.play",
			A::CheckForUpdates => "help.check_for_updates",
			A::About => "help.about",
			A::ProjectPage => "help.project_page",
			A::AddSource => "item.add_source",
			A::AddPart(kind) => return key("item.add_part", new_part_key(kind)),
			A::AddProbe(detector) => return key("item.add_probe", detector_key(detector)),
			A::ToggleSelected => "item.toggle_selected",
			A::ToggleEnabled => "item.toggle_enabled",
			A::Duplicate => "item.duplicate",
			A::MoveUp => "item.move_up",
			A::MoveDown => "item.move_down",
			A::ExportProbeCsv => "item.export_probe_csv",
			A::ToggleUserScheme => "item.toggle_user_scheme",
		};
		fixed.to_owned()
	}
}

/// `prefix` joined with a fixed `leaf`.
fn key(prefix: &str, leaf: &str) -> String {
	format!("{prefix}.{leaf}")
}

fn export_key(kind: ExportKindChoice) -> &'static str {
	match kind {
		ExportKindChoice::Field => "field",
		ExportKindChoice::Intensity => "intensity",
		ExportKindChoice::Both => "both",
		ExportKindChoice::FieldFrame => "field_frame",
		ExportKindChoice::IntensityFrame => "intensity_frame",
	}
}

fn pane_key(pane: Pane) -> &'static str {
	match pane {
		Pane::Preview => "preview",
		Pane::Project => "project",
		Pane::Properties => "properties",
		Pane::Messages => "messages",
	}
}

fn zoom_key(step: ZoomStep) -> &'static str {
	match step {
		ZoomStep::In => "in",
		ZoomStep::Out => "out",
		ZoomStep::Reset => "reset",
	}
}

fn detector_key(detector: ProbeDetector) -> &'static str {
	match detector {
		ProbeDetector::Standard => "standard",
		ProbeDetector::Counting => "counting",
		ProbeDetector::Imaging => "imaging",
		_ => "unknown",
	}
}

fn new_part_key(kind: NewPart) -> &'static str {
	match kind {
		NewPart::Lens => "lens",
		NewPart::FresnelLens => "fresnel_lens",
		NewPart::Polygon => "polygon",
		NewPart::Rectangle => "rectangle",
		NewPart::Ellipse => "ellipse",
		NewPart::Triangle => "triangle",
		NewPart::Outline => "outline",
		NewPart::Diffuser => "diffuser",
		NewPart::InterferenceFilter => "interference_filter",
		NewPart::SimpleGrating => "simple_grating",
		NewPart::FacettedGrating => "facetted_grating",
		NewPart::MetaLens => "meta_lens",
		NewPart::MetaPrism => "meta_prism",
		NewPart::MetaAxicon => "meta_axicon",
		NewPart::Fiber => "fiber",
		NewPart::BraggReflector => "bragg_reflector",
		NewPart::GrinLens => "grin_lens",
		NewPart::XorGate => "xor_gate",
		NewPart::AndGate => "and_gate",
		NewPart::OrGate => "or_gate",
		NewPart::ZonePlate => "zone_plate",
	}
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

/// A menu entry's icon; each window draws it from its own icon set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
	Close,
	Copy,
	Cut,
	Paste,
	Delete,
	DeleteSweep,
	FolderOpen,
	History,
	Image,
	Info,
	Share,
	ListAlt,
	Logout,
	NoteAdd,
	OpenInNew,
	Pause,
	Play,
	Redo,
	Save,
	SaveAs,
	SelectAll,
	Settings,
	Stop,
	Undo,
	Sidebar,
	WavingHand,
}

/// The label an entry shows in place of its own while `when` holds.
#[derive(Clone, Copy)]
pub struct Alt {
	pub key: &'static str,
	pub icon: Icon,
	pub when: fn(&App) -> bool,
}

/// An entry of the menu bar: its action, the string key of its label (with its mnemonic) and its icon. Its shortcut
/// is the action's in `waveduderust_egui_platform::keys`.
#[derive(Clone, Copy)]
pub struct Item {
	pub action: MenuAction,
	pub key: &'static str,
	pub icon: Option<Icon>,
	pub show: Show,
	pub needs: Needs,
	pub alt: Option<Alt>,
}

impl Item {
	const fn new(action: MenuAction, key: &'static str, icon: Option<Icon>) -> Self {
		Self { action, key, icon, show: Show::Always, needs: Needs::Nothing, alt: None }
	}

	const fn show(self, show: Show) -> Self {
		Self { show, ..self }
	}

	const fn needs(self, needs: Needs) -> Self {
		Self { needs, ..self }
	}

	const fn alt(self, key: &'static str, icon: Icon, when: fn(&App) -> bool) -> Self {
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
		icon: Icon,
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

const fn item(action: MenuAction, key: &'static str, icon: Icon) -> Entry {
	Entry::Item(Item::new(action, key, Some(icon)))
}

const fn export(choice: ExportKindChoice, key: &'static str, icon: Icon) -> Entry {
	item(MenuAction::Export(choice), key, icon)
}

fn run_paused(app: &App) -> bool {
	app.live.run_paused_by_user()
}

fn playing(app: &App) -> bool {
	app.live.playback.is_playing()
}

const FILE: &[Entry] = &[
	item(MenuAction::New, "MenuFileNew", Icon::NoteAdd),
	item(MenuAction::Open, "MenuFileOpen", Icon::FolderOpen),
	Entry::Submenu { key: "MenuFileRecent", icon: Icon::History, show: Show::Paths, entries: &[Entry::Recent] },
	item(MenuAction::Examples, "MenuFileExamples", Icon::ListAlt),
	item(MenuAction::Welcome, "MenuFileWelcome", Icon::WavingHand),
	item(MenuAction::Settings, "MenuFileSettings", Icon::Settings),
	Entry::Separator,
	item(MenuAction::Save, "MenuFileSave", Icon::Save),
	item(MenuAction::SaveAs, "MenuFileSaveAs", Icon::SaveAs),
	Entry::Separator,
	Entry::Submenu {
		key: "MenuFileExport",
		icon: Icon::Share,
		show: Show::Always,
		entries: &[
			export(ExportKindChoice::Field, "MenuFileExportField", Icon::Share),
			export(ExportKindChoice::Intensity, "MenuFileExportIntensity", Icon::Share),
			export(ExportKindChoice::Both, "MenuFileExportBoth", Icon::Share),
			Entry::Separator,
			export(ExportKindChoice::FieldFrame, "MenuFileExportFieldFrame", Icon::Image),
			export(ExportKindChoice::IntensityFrame, "MenuFileExportIntensityFrame", Icon::Image),
		],
	},
	Entry::Item(Item::new(MenuAction::ExportLast, "MenuFileExportLast", Some(Icon::Share)).needs(Needs::LastExport)),
	Entry::Separator,
	item(MenuAction::Close, "MenuFileClose", Icon::Close),
	Entry::Separator,
	Entry::Item(Item::new(MenuAction::Exit, "MenuFileExit", Some(Icon::Logout)).show(Show::Native)),
];

const EDIT: &[Entry] = &[
	item(MenuAction::Undo, "MenuEditUndo", Icon::Undo),
	item(MenuAction::Redo, "MenuEditRedo", Icon::Redo),
	Entry::Separator,
	item(MenuAction::Cut, "MenuEditCut", Icon::Cut),
	item(MenuAction::Copy, "MenuEditCopy", Icon::Copy),
	Entry::Item(Item::new(MenuAction::Paste, "MenuEditPaste", Some(Icon::Paste)).needs(Needs::Pastable)),
	item(MenuAction::Delete, "MenuEditDelete", Icon::Delete),
	Entry::Separator,
	item(MenuAction::SelectAll, "MenuProjectSelectAll", Icon::SelectAll),
];

const VIEW: &[Entry] =
	&[Entry::Submenu { key: "MenuViewPanes", icon: Icon::Sidebar, show: Show::Always, entries: &[Entry::Panes] }];

const SIMULATION: &[Entry] = &[
	item(MenuAction::Run, "MenuSimulationRun", Icon::Play),
	Entry::Item(Item::new(MenuAction::PauseRun, "MenuSimulationPause", Some(Icon::Pause)).alt(
		"MenuSimulationResume",
		Icon::Play,
		run_paused,
	)),
	item(MenuAction::Stop, "MenuSimulationStop", Icon::Stop),
	item(MenuAction::DiscardFrames, "MenuSimulationDiscardFrames", Icon::DeleteSweep),
	Entry::Separator,
	Entry::Item(Item::new(MenuAction::Play, "MenuSimulationPlay", Some(Icon::Play)).alt(
		"MenuSimulationPausePlayback",
		Icon::Pause,
		playing,
	)),
];

const HELP: &[Entry] = &[
	item(MenuAction::About, "MenuHelpAbout", Icon::Info),
	item(MenuAction::ProjectPage, "MenuHelpProjectPage", Icon::OpenInNew),
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
		MenuAction::Run => !live.run_is_active() && !live.export_busy() && project.sources.iter().any(|s| s.enabled),
		MenuAction::PauseRun => live.run_is_active(),
		MenuAction::Stop => live.run_is_active() || live.export_busy(),
		MenuAction::DiscardFrames => live.has_frames() && !live.export_busy(),
		MenuAction::ToggleEnabled => target.is_some_and(|t| t.kind() != ItemKind::Scheme),
		MenuAction::ToggleUserScheme => target.is_some_and(|t| t.kind() == ItemKind::Scheme),
		MenuAction::MoveUp | MenuAction::MoveDown => {
			target.is_some_and(|t| can_move(app, t, action == MenuAction::MoveUp))
		}
		MenuAction::ExportProbeCsv => {
			let probe = match target {
				Some(ItemRef::Probe(id)) => project.probe(id),
				_ => None,
			};
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
		| MenuAction::ProjectPage
		| MenuAction::AddSource
		| MenuAction::AddPart(_)
		| MenuAction::AddProbe(_) => true,
	}
}

/// Whether `action` on `on` puts the focus in the form: an add, or a duplicate of something.
pub fn focuses_form<B: Backend>(app: &AppState<B>, action: MenuAction, on: On) -> bool {
	matches!(action, MenuAction::AddSource | MenuAction::AddPart(_) | MenuAction::AddProbe(_))
		|| (action == MenuAction::Duplicate && target(app, on).is_some())
}

/// The events of an Edit action on `on`, in order, after its changes of the selection; `None` for another action.
/// `pasted` reads the clipboard for Paste. The caller checks [`enabled`] first.
pub fn edit<B: Backend>(
	app: &mut AppState<B>,
	action: MenuAction,
	on: On,
	pasted: impl FnOnce() -> Option<String>,
) -> Option<Vec<Event>> {
	if action == MenuAction::Copy
		&& let On::Row(Some(row)) = on
		&& !matches!(row, Row::Item(i) if app.selection.contains(i))
	{
		app.selection.select(row);
	}
	let target = target(app, on);
	let on_target = |f: fn(ItemRef) -> Command| target.map(|t| Event::Command(f(t)));
	let event = match action {
		MenuAction::Undo => Some(Event::Undo),
		MenuAction::Redo => Some(Event::Redo),
		MenuAction::Cut => target.map(|target| Event::Cut { target }),
		MenuAction::Copy => Some(Event::Copy),
		MenuAction::Paste => pasted().map(|text| {
			let row = if let On::Row(row) = on { row } else { None };
			let target = PasteTarget { current: row.or(app.selection.current()), before: false };
			Event::Paste { text, target, place_at: None }
		}),
		MenuAction::Delete => on_target(|target| Command::Remove { target }),
		MenuAction::SelectAll => {
			app.selection.select_all(&app.document.project);
			None
		}
		MenuAction::AddSource | MenuAction::AddPart(_) | MenuAction::AddProbe(_) => {
			add_command(action, None).map(Event::Command)
		}
		MenuAction::ToggleSelected => {
			if let Some(target) = target {
				app.selection.toggle(target);
			}
			None
		}
		MenuAction::ToggleEnabled => on_target(|target| Command::ToggleEnabled { target }),
		MenuAction::ToggleUserScheme => match target {
			Some(ItemRef::Scheme(id)) => Some(Event::ToggleUserScheme(id)),
			_ => None,
		},
		MenuAction::Duplicate => on_target(|target| Command::Duplicate { target }),
		MenuAction::MoveUp | MenuAction::MoveDown => {
			let Some(t) = target else { return Some(Vec::new()) };
			let reorder = |(target, to)| Event::Command(Command::Reorder { target, to });
			return Some(moves_of(app, t, action == MenuAction::MoveUp).into_iter().map(reorder).collect());
		}
		_ => return None,
	};
	Some(event.into_iter().collect())
}

/// The command of an add action placing its item at `at` (`None`: the canvas centre); `None` for another action.
pub fn add_command(action: MenuAction, at: Option<Point>) -> Option<Command> {
	match action {
		MenuAction::AddSource => Some(Command::AddSource { at }),
		MenuAction::AddPart(kind) => Some(Command::AddPart { kind, at }),
		MenuAction::AddProbe(detector) => Some(Command::AddProbe { at, detector }),
		_ => None,
	}
}

/// Whether Move up (`up`) or Move down shifts `target` and the items moving with it.
pub fn can_move<B: Backend>(app: &AppState<B>, target: ItemRef, up: bool) -> bool {
	!moves_of(app, target, up).is_empty()
}

/// The reorders `(item, to)` of Move up (`up`) or Move down on `target` and the selected items of its list, to be
/// applied in order.
pub fn moves_of<B: Backend>(app: &AppState<B>, target: ItemRef, up: bool) -> Vec<(ItemRef, usize)> {
	let project = &app.document.project;
	let kind = target.kind();
	let indices = app.selection.targets(target, project).into_iter().filter(|t| t.kind() == kind);
	let moves = block_moves(indices.filter_map(|t| project.index_of(t)).collect(), kind.len(project), up);
	moves.into_iter().filter_map(|(from, to)| Some((ItemRef::at(kind, from, project)?, to))).collect()
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

	use crate::Event;
	use crate::commands::Command;
	use crate::live::RunNeeds;
	use crate::live::fake::FakeBackend;
	use crate::platform::Instant;
	use crate::settings::Settings;
	use crate::strings::{Language, Strings, mnemonic};
	use waveduderust::unstable::export::ExportFormat;

	use super::*;

	fn app() -> AppState<FakeBackend> {
		AppState::with_backend(Settings::default(), None, FakeBackend::default(), Arc::new(AtomicBool::new(false)))
	}

	#[test]
	fn an_item_action_needs_its_item() {
		let mut app = app();
		let scene = On::Row(Some(Row::Scene));
		let elsewhere = waveduderust::model::Project::default().add_new_color_scheme();
		let scheme = On::Row(Some(Row::Item(ItemRef::Scheme(elsewhere))));
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
		assert!(!enabled(&app, MenuAction::Run, On::Current), "an empty project has nothing to run");
		app.handle(Event::Command(Command::AddSource { at: None }), Instant::now());
		app.handle(Event::StopRun, Instant::now());
		let project = app.document.project.clone();
		assert!(enabled(&app, MenuAction::Run, On::Current) && !enabled(&app, MenuAction::Stop, On::Current));
		let view = app.document.view.clone();
		app.live.request_export(&project, &view, RunNeeds::new(ExportFormat::Png, false), Instant::now());
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
