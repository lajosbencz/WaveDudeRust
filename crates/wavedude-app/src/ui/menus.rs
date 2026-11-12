//! Every menu of docs/ui/10-menus-and-shortcuts.md: the menu bar's model (built from the state, rebuilt only
//! when what it shows changes), the context menus of the tree rows and the part kinds, and one dispatcher for
//! the [`MenuAction`]s that the menu bar, the context menus and the shortcuts share.

use freya::prelude::*;
use wavedude_core::engine::SampleFormat;
use wavedude_core::quality::PreviewQuality;
use wavedude_widgets::icon::icons;
use wavedude_widgets::{Check, MenuEntry, MenuItem, MenuModel, Submenu};

use wavedude_app_state::clipboard::{Copyable, PasteTarget};
use wavedude_app_state::commands::{Command, NewPart};
use wavedude_app_state::selection::Row;
use wavedude_app_state::settings::{DITHER_BITS, Theme as ThemeSetting};
use wavedude_app_state::strings::{Language, Strings};
use wavedude_app_state::{Event as AppEvent, Guarded};
use wavedude_core::model::Word;
use wavedude_core::model::{ItemKind, ItemRef, ProbeDetector};

use super::app::{Cx, Ui};
use super::dialogs::{self, Modal};
use super::export_dialog::{self, ExportKindChoice};
use super::shared::{App, Shared, Topics};
use super::window::{self, PANES, PREVIEW};

/// Menu icons in the 16 px stroked style of the widget crate.
pub mod glyphs {
	use wavedude_widgets::Icon;

	pub const NEW: Icon = Icon::path("M3.5 0.5 L9.5 0.5 L12.5 3.5 L12.5 14.5 L3.5 14.5 Z M9.5 0.5 L9.5 3.5 L12.5 3.5");
	pub const OPEN: Icon = Icon::path("M0.5 2.5 L5.5 2.5 L7.5 4.5 L13.5 4.5 L13.5 13.5 L0.5 13.5 Z M0.5 6.5 L13.5 6.5");
	pub const RECENT: Icon = Icon::path("M7.5 1.5 A6 6 0 1 1 7.49 1.5 Z M7.5 4.5 L7.5 7.5 L10.5 9.5");
	pub const EXAMPLES: Icon =
		Icon::path("M1.5 1.5 L13.5 1.5 L13.5 13.5 L1.5 13.5 Z M4.5 5.5 L10.5 5.5 M4.5 8.5 L10.5 8.5");
	pub const SAVE: Icon =
		Icon::path("M1.5 1.5 L11.5 1.5 L13.5 3.5 L13.5 13.5 L1.5 13.5 Z M4.5 1.5 L4.5 5.5 L10.5 5.5 L10.5 1.5");
	pub const EXPORT: Icon =
		Icon::path("M7.5 9.5 L7.5 0.5 M4.5 3.5 L7.5 0.5 L10.5 3.5 M2.5 7.5 L2.5 13.5 L12.5 13.5 L12.5 7.5");
	pub const FRAME: Icon =
		Icon::path("M1.5 2.5 L13.5 2.5 L13.5 12.5 L1.5 12.5 Z M1.5 10.5 L5.5 6.5 L8.5 9.5 L10.5 7.5 L13.5 10.5");
	pub const UNDO: Icon = Icon::path("M4.5 2.5 L1.5 5.5 L4.5 8.5 M1.5 5.5 L9.5 5.5 A4 4 0 0 1 9.5 13.5 L5.5 13.5");
	pub const REDO: Icon = Icon::path("M10.5 2.5 L13.5 5.5 L10.5 8.5 M13.5 5.5 L5.5 5.5 A4 4 0 0 0 5.5 13.5 L9.5 13.5");
	pub const CUT: Icon = Icon::path(
		"M4.5 0.5 L10.5 10.5 M10.5 0.5 L4.5 10.5 M4.5 12.5 A2 2 0 1 1 4.49 12.5 Z M10.5 12.5 A2 2 0 1 1 10.49 12.5 Z",
	);
	pub const COPY: Icon = Icon::path("M4.5 4.5 L13.5 4.5 L13.5 14.5 L4.5 14.5 Z M1.5 11.5 L1.5 0.5 L10.5 0.5");
	pub const PASTE: Icon =
		Icon::path("M2.5 2.5 L12.5 2.5 L12.5 14.5 L2.5 14.5 Z M5.5 0.5 L9.5 0.5 L9.5 3.5 L5.5 3.5 Z");
	pub const DELETE: Icon =
		Icon::path("M2.5 3.5 L12.5 3.5 M5.5 3.5 L5.5 1.5 L9.5 1.5 L9.5 3.5 M3.5 3.5 L4.5 14.5 L10.5 14.5 L11.5 3.5");
	pub const SELECT_ALL: Icon = Icon::path("M1.5 1.5 L13.5 1.5 L13.5 13.5 L1.5 13.5 Z M4.5 7.5 L6.5 9.5 L10.5 5.5");
	pub const PANES: Icon = Icon::path("M0.5 1.5 L14.5 1.5 L14.5 13.5 L0.5 13.5 Z M5.5 1.5 L5.5 13.5");
	pub const THEME: Icon = Icon::path("M7.5 1.5 A6 6 0 1 1 7.49 1.5 Z M7.5 1.5 L7.5 13.5");
	pub const ZOOM: Icon =
		Icon::path("M6.5 1.5 A5 5 0 1 1 6.49 1.5 Z M10 10 L14.5 14.5 M4.5 6.5 L8.5 6.5 M6.5 4.5 L6.5 8.5");
	pub const LANGUAGE: Icon =
		Icon::path("M7.5 1.5 A6 6 0 1 1 7.49 1.5 Z M1.5 7.5 L13.5 7.5 M7.5 1.5 A3 6 0 0 0 7.5 13.5 A3 6 0 0 0 7.5 1.5");
	pub const RESOLUTION: Icon =
		Icon::path("M1.5 1.5 L13.5 1.5 L13.5 13.5 L1.5 13.5 Z M1.5 7.5 L13.5 7.5 M7.5 1.5 L7.5 13.5");
	pub const FRAMES: Icon = Icon::path(
		"M0.5 3.5 L4.5 3.5 L4.5 11.5 L0.5 11.5 Z M5.5 3.5 L9.5 3.5 L9.5 11.5 L5.5 11.5 Z M10.5 3.5 L14.5 3.5 L14.5 11.5 L10.5 11.5 Z",
	);
	pub const SAMPLES: Icon = Icon::path("M0.5 13.5 L3.5 13.5 L3.5 9.5 L6.5 9.5 L6.5 5.5 L9.5 5.5 L9.5 2.5 L14.5 2.5");
	pub const DITHER: Icon = Icon::filled(
		"M1 1 L4 1 L4 4 L1 4 Z M7 1 L10 1 L10 4 L7 4 Z M4 4 L7 4 L7 7 L4 7 Z M10 4 L13 4 L13 7 L10 7 Z M1 7 L4 7 L4 10 L1 10 Z M7 7 L10 7 L10 10 L7 10 Z M4 10 L7 10 L7 13 L4 13 Z M10 10 L13 10 L13 13 L10 13 Z",
	);
	pub const RUN: Icon = Icon::filled("M4 2 L12 7.5 L4 13 Z");
	pub const PAUSE: Icon = Icon::filled("M4 2 L6.5 2 L6.5 13 L4 13 Z M8.5 2 L11 2 L11 13 L8.5 13 Z");
	pub const STOP: Icon = Icon::filled("M3 3 L12 3 L12 12 L3 12 Z");
	pub const MEMORY: Icon = Icon::path(
		"M1.5 3.5 L13.5 3.5 L13.5 11.5 L1.5 11.5 Z M3.5 11.5 L3.5 14.5 M7.5 11.5 L7.5 14.5 M11.5 11.5 L11.5 14.5 M3.5 5.5 L3.5 9.5 M6.5 5.5 L6.5 9.5",
	);
	pub const ABOUT: Icon = Icon::path("M7.5 1.5 A6 6 0 1 1 7.49 1.5 Z M7.5 6.5 L7.5 11.5 M7.5 4 L7.5 4.5");
	pub const MANUAL: Icon = Icon::path(
		"M1.5 2.5 L6.5 2.5 L7.5 3.5 L8.5 2.5 L13.5 2.5 L13.5 12.5 L8.5 12.5 L7.5 13.5 L6.5 12.5 L1.5 12.5 Z M7.5 3.5 L7.5 13.5",
	);
	pub const CLOSE_PROJECT: Icon = Icon::path("M3.5 3.5 L11.5 11.5 M11.5 3.5 L3.5 11.5");
	pub const EXIT: Icon =
		Icon::path("M9.5 1.5 L1.5 1.5 L1.5 13.5 L9.5 13.5 M5.5 7.5 L14.5 7.5 M11.5 4.5 L14.5 7.5 L11.5 10.5");
	pub const TOGGLE: Icon =
		Icon::path("M1.5 1.5 L13.5 1.5 L13.5 13.5 L1.5 13.5 Z M4.5 7.5 L10.5 7.5 M7.5 4.5 L7.5 10.5");
	pub const ENABLED: Icon = Icon::path("M2.5 7.5 L5.5 10.5 L12.5 3.5");
	pub const DUPLICATE: Icon =
		Icon::path("M4.5 4.5 L13.5 4.5 L13.5 13.5 L4.5 13.5 Z M1.5 10.5 L1.5 1.5 L10.5 1.5 M9 7 L9 11 M7 9 L11 9");
}

/// The UI zoom entries of View > UI zoom: factor and string key.
pub const ZOOMS: [(f64, &str); 8] = [
	(0.5, "MenuViewUiZoom50"),
	(0.75, "MenuViewUiZoom75"),
	(1.0, "MenuViewUiZoom100"),
	(1.25, "MenuViewUiZoom125"),
	(1.5, "MenuViewUiZoom150"),
	(2.0, "MenuViewUiZoom200"),
	(2.5, "MenuViewUiZoom250"),
	(3.0, "MenuViewUiZoom300"),
];

/// The Add part menu's entries: kind, string key, test id.
pub const PART_KINDS: [(NewPart, &str, &str); 21] = [
	(NewPart::Lens, "MenuAddLens", "Lens"),
	(NewPart::FresnelLens, "MenuAddFresnelLens", "FresnelLens"),
	(NewPart::Fiber, "MenuAddFiber", "Fiber"),
	(NewPart::BraggReflector, "MenuAddBraggReflector", "BraggReflector"),
	(NewPart::GrinLens, "MenuAddGrinLens", "GrinLens"),
	(NewPart::ZonePlate, "MenuAddZonePlate", "ZonePlate"),
	(NewPart::Diffuser, "MenuAddDiffuser", "Diffuser"),
	(NewPart::InterferenceFilter, "MenuAddInterferenceFilter", "InterferenceFilter"),
	(NewPart::SimpleGrating, "MenuAddSimpleGrating", "SimpleGrating"),
	(NewPart::FacettedGrating, "MenuAddFacettedGrating", "FacettedGrating"),
	(NewPart::MetaLens, "MenuAddMetaLens", "MetaLens"),
	(NewPart::MetaPrism, "MenuAddMetaPrism", "MetaPrism"),
	(NewPart::MetaAxicon, "MenuAddMetaAxicon", "MetaAxicon"),
	(NewPart::Polygon, "MenuAddPolygon", "Polygon"),
	(NewPart::Rectangle, "MenuAddRectangle", "Rectangle"),
	(NewPart::Ellipse, "MenuAddEllipse", "Ellipse"),
	(NewPart::Triangle, "MenuAddTriangle", "Triangle"),
	(NewPart::Outline, "MenuAddOutline", "Outline"),
	(NewPart::XorGate, "MenuAddXorGate", "XorGate"),
	(NewPart::AndGate, "MenuAddAndGate", "AndGate"),
	(NewPart::OrGate, "MenuAddOrGate", "OrGate"),
];

/// What a menu entry or a shortcut does. The entries keep their string ids as test ids.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
	New,
	Open,
	/// Entry `n` of the recent files.
	Recent(usize),
	/// The Examples dialog.
	Examples,
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
	/// A pane id of [`PANES`] or [`PREVIEW`].
	Pane(&'static str),
	Theme(ThemeSetting),
	Zoom(f64),
	/// `None` follows the system.
	Language(Option<Language>),
	Resolution(u32),
	Frames(u32),
	Samples(SampleFormat),
	/// One of [`DITHER_BITS`].
	Dither(u32),
	Run,
	PauseRun,
	Stop,
	Play,
	UseGpu,
	AutoRerun,
	WarnMemory,
	CheckForUpdates,
	CheckAtLaunch,
	/// 0 is Auto.
	FrameMemory(i32),
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
}

/// What an item action acts on: the current item (menu bar, shortcuts) or the row a tree menu opened on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum On {
	Current,
	Row(Option<Row>),
}

type Entry = MenuEntry<MenuAction>;

fn item(action: MenuAction, id: &str, s: &Strings, key: &str) -> MenuItem<MenuAction> {
	MenuItem::with_action(id, s.get(key), action)
}

fn radio(action: MenuAction, id: impl Into<String>, text: impl Into<String>, on: bool) -> Entry {
	MenuItem::with_action(id, text, action).check(Check::Radio(on)).into()
}

fn checked(action: MenuAction, id: &str, s: &Strings, key: &str, on: bool) -> Entry {
	item(action, id, s, key).check(Check::Checked(on)).into()
}

/// The Add part entries, each with its kind's description as tooltip; test ids `<prefix><Kind>`.
pub fn part_entries(s: &Strings, prefix: &str) -> Vec<Entry> {
	let entry = |(kind, key, test): &(NewPart, &str, &str)| -> Entry {
		MenuItem::with_action(format!("{prefix}{test}"), s.get(key), MenuAction::AddPart(*kind))
			.icon(super::tree::kind_icon(*kind))
			.tooltip(s.kind(kind.type_name()))
			.into()
	};
	let entries = |from: NewPart, to: NewPart| -> Vec<Entry> {
		let at = |kind| PART_KINDS.iter().position(|(k, ..)| *k == kind).expect("a kind of the menu");
		PART_KINDS[at(from)..=at(to)].iter().map(entry).collect()
	};
	let submenu = |id: &str, key: &str, icon: NewPart, from, to| -> Entry {
		Submenu::new(format!("{prefix}{id}"), s.get(key), entries(from, to)).icon(super::tree::kind_icon(icon)).into()
	};
	let mut v = entries(NewPart::Lens, NewPart::InterferenceFilter);
	v.push(submenu(
		"DiffractionGratings",
		"MenuAddDiffractionGratings",
		NewPart::SimpleGrating,
		NewPart::SimpleGrating,
		NewPart::FacettedGrating,
	));
	v.push(submenu("MetaSurfaces", "MenuAddMetaSurfaces", NewPart::MetaLens, NewPart::MetaLens, NewPart::MetaAxicon));
	v.push(MenuEntry::Separator);
	v.extend(entries(NewPart::Polygon, NewPart::Outline));
	v.push(MenuEntry::Separator);
	v.push(submenu("OpticalLogic", "MenuAddOpticalLogic", NewPart::XorGate, NewPart::XorGate, NewPart::OrGate));
	v
}

/// The Add probe entries: a standard probe, and the detectors in a Probabilistic submenu, each with its
/// description as tooltip; test ids `<prefix><Kind>`.
pub fn probe_entries(s: &Strings, prefix: &str) -> Vec<Entry> {
	let entry = |detector: ProbeDetector, key: &str, tip: &str, test: &str| -> Entry {
		MenuItem::with_action(format!("{prefix}{test}"), s.get(key), MenuAction::AddProbe(detector))
			.icon(icons::PROBE)
			.tooltip(s.get(tip))
			.into()
	};
	vec![
		entry(ProbeDetector::Standard, "MenuAddStandardProbe", "Detector.Standard", "Standard"),
		Submenu::new(
			format!("{prefix}Probabilistic"),
			s.get("MenuAddProbabilistic"),
			vec![
				entry(ProbeDetector::Counting, "MenuAddCountingDetector", "Detector.Counting", "Counting"),
				entry(ProbeDetector::Imaging, "MenuAddImagingDetector", "Detector.Imaging", "Imaging"),
			],
		)
		.icon(icons::PROBE)
		.into(),
	]
}

fn export_entries(s: &Strings, enabled: bool) -> Vec<Entry> {
	let export = |choice, id, key, shortcut, icon| -> Entry {
		item(MenuAction::Export(choice), id, s, key).shortcut(shortcut).icon(icon).enabled(enabled).into()
	};
	vec![
		export(ExportKindChoice::Field, "file:export:field", "MenuFileExportField", "Ctrl+Shift+E", glyphs::EXPORT),
		export(
			ExportKindChoice::Intensity,
			"file:export:intensity",
			"MenuFileExportIntensity",
			"Ctrl+Shift+I",
			glyphs::EXPORT,
		),
		export(ExportKindChoice::Both, "file:export:both", "MenuFileExportBoth", "Ctrl+Shift+B", glyphs::EXPORT),
		MenuEntry::Separator,
		export(
			ExportKindChoice::FieldFrame,
			"file:export:field_frame",
			"MenuFileExportFieldFrame",
			"Ctrl+Shift+F",
			glyphs::FRAME,
		),
		export(
			ExportKindChoice::IntensityFrame,
			"file:export:intensity_frame",
			"MenuFileExportIntensityFrame",
			"Ctrl+Shift+G",
			glyphs::FRAME,
		),
	]
}

/// Whether there is something to export: no export running.
fn can_export(app: &App) -> bool {
	!app.live.export_busy()
}

fn file_menu(app: &App, ui: &Ui) -> Submenu<MenuAction> {
	let s = &app.strings;
	let recent: Vec<Entry> = app
		.settings
		.recent_files
		.iter()
		.enumerate()
		.map(|(i, p)| {
			MenuItem::with_action(
				format!("file:recent:{i}"),
				p.display().to_string().replace('_', "__"),
				MenuAction::Recent(i),
			)
			.into()
		})
		.collect();
	let export_last = export_dialog::has_last(ui, app);
	Submenu::new(
		"MenuFile",
		s.get("MenuFile"),
		vec![
			item(MenuAction::New, "file:new", s, "MenuFileNew").shortcut("Ctrl+N").icon(glyphs::NEW).into(),
			item(MenuAction::Open, "file:open", s, "MenuFileOpen").shortcut("Ctrl+O").icon(glyphs::OPEN).into(),
			Submenu::new("file:recent", s.get("MenuFileRecent"), recent).icon(glyphs::RECENT).into(),
			item(MenuAction::Examples, "file:examples", s, "MenuFileExamples").icon(glyphs::EXAMPLES).into(),
			MenuEntry::Separator,
			item(MenuAction::Save, "file:save", s, "MenuFileSave").shortcut("Ctrl+S").icon(glyphs::SAVE).into(),
			item(MenuAction::SaveAs, "file:saveas", s, "MenuFileSaveAs")
				.shortcut("Ctrl+Shift+S")
				.icon(glyphs::SAVE)
				.into(),
			MenuEntry::Separator,
			Submenu::new("file:export", s.get("MenuFileExport"), export_entries(s, can_export(app)))
				.icon(glyphs::EXPORT)
				.into(),
			item(MenuAction::ExportLast, "file:export_last", s, "MenuFileExportLast")
				.shortcut("Ctrl+E")
				.icon(glyphs::EXPORT)
				.enabled(export_last && !app.live.export_busy())
				.into(),
			MenuEntry::Separator,
			item(MenuAction::Close, "file:close", s, "MenuFileClose")
				.icon(glyphs::CLOSE_PROJECT)
				.enabled(app.document.path.is_some())
				.into(),
			MenuEntry::Separator,
			item(MenuAction::Exit, "file:exit", s, "MenuFileExit").shortcut("Alt+F4").icon(glyphs::EXIT).into(),
		],
	)
}

/// Whether the clipboard's text is a fragment the paste would take.
pub fn clipboard_holds_objects() -> bool {
	#[cfg(test)]
	super::shared::count_run("clipboard");
	Clipboard::get().is_ok_and(|t| wavedude_app_state::clipboard::read_fragment(&t).is_ok())
}

fn has_items(app: &App) -> bool {
	let p = &app.document.project;
	!(p.sources.is_empty() && p.parts.is_empty() && p.probes.is_empty())
}

fn edit_menu(app: &App, pastable: bool) -> Submenu<MenuAction> {
	let s = &app.strings;
	let h = &app.document.history;
	let items = !app.selection.items().is_empty();
	let copyable = items || app.selection.current().and_then(Copyable::of_row).is_some();
	Submenu::new(
		"MenuEdit",
		s.get("MenuEdit"),
		vec![
			item(MenuAction::Undo, "edit:undo", s, "MenuEditUndo")
				.shortcut("Ctrl+Z")
				.icon(glyphs::UNDO)
				.enabled(h.can_undo() || h.has_pending())
				.into(),
			item(MenuAction::Redo, "edit:redo", s, "MenuEditRedo")
				.shortcut("Ctrl+Y")
				.icon(glyphs::REDO)
				.enabled(h.can_redo())
				.into(),
			MenuEntry::Separator,
			item(MenuAction::Cut, "edit:cut", s, "MenuEditCut")
				.shortcut("Ctrl+X")
				.icon(glyphs::CUT)
				.enabled(items)
				.into(),
			item(MenuAction::Copy, "edit:copy", s, "MenuEditCopy")
				.shortcut("Ctrl+C")
				.icon(glyphs::COPY)
				.enabled(copyable)
				.into(),
			item(MenuAction::Paste, "edit:paste", s, "MenuEditPaste")
				.shortcut("Ctrl+V")
				.icon(glyphs::PASTE)
				.enabled(pastable)
				.into(),
			item(MenuAction::Delete, "edit:delete", s, "MenuEditDelete")
				.shortcut("Del")
				.icon(glyphs::DELETE)
				.enabled(items)
				.into(),
			MenuEntry::Separator,
			item(MenuAction::SelectAll, "edit:select_all", s, "MenuProjectSelectAll")
				.shortcut("Ctrl+A")
				.icon(glyphs::SELECT_ALL)
				.enabled(has_items(app))
				.into(),
		],
	)
}

fn view_menu(app: &App, ui: &Ui) -> Submenu<MenuAction> {
	let s = &app.strings;
	let st = &app.settings;
	let layout = ui.layout.read();
	let pane = |id: &'static str, key: &str, shown: bool| -> Entry {
		MenuItem::with_action(format!("view:pane:{id}"), s.get(key), MenuAction::Pane(id))
			.check(Check::Checked(shown))
			.into()
	};
	let mut panes = vec![pane(PREVIEW, "PanePreview", *ui.preview_shown.read())];
	for (id, key) in PANES.iter().zip(["PaneProject", "PaneProperties", "PaneMessages"]) {
		panes.push(pane(id, key, layout.is_shown(id)));
	}
	let themes = ThemeSetting::MEMBERS
		.iter()
		.map(|&t| {
			radio(
				MenuAction::Theme(t),
				format!("view:theme:{}", t.name()),
				s.enum_word(ThemeSetting::TYPE_NAME, t.name()),
				st.theme == t,
			)
		})
		.collect();
	let zooms = ZOOMS
		.iter()
		.map(|&(z, key)| {
			radio(MenuAction::Zoom(z), format!("view:zoom:{z}"), s.get(key), (st.ui_zoom - z).abs() < 1e-6)
		})
		.collect();
	let mut languages =
		vec![radio(MenuAction::Language(None), "view:lang:", s.get("LanguageSystem"), st.language.is_empty())];
	languages.extend(Language::ALL.map(|l| {
		radio(
			MenuAction::Language(Some(l)),
			format!("view:lang:{}", l.code()),
			l.native_name(),
			st.language == l.code(),
		)
	}));
	let q = st.preview_quality();
	let divisors = |prefix: &str, action: fn(u32) -> MenuAction, value: u32, keys: [&str; 3]| -> Vec<Entry> {
		[1, 2, 4]
			.into_iter()
			.zip(keys)
			.map(|(d, k)| radio(action(d), format!("{prefix}{d}"), s.get(k), value == d))
			.collect()
	};
	let half = q.format == SampleFormat::Half;
	let dither = DITHER_BITS
		.iter()
		.zip(["MenuViewDitherOff", "MenuViewDither10", "MenuViewDither12", "MenuViewDither14", "MenuViewDither16"])
		.map(|(&b, k)| radio(MenuAction::Dither(b), format!("view:dither:{b}"), s.get(k), st.preview_dither_bits == b))
		.collect();
	Submenu::new(
		"MenuView",
		s.get("MenuView"),
		vec![
			Submenu::new("view:panes", s.get("MenuViewPanes"), panes).icon(glyphs::PANES).into(),
			Submenu::new("view:theme", s.get("MenuViewTheme"), themes).icon(glyphs::THEME).into(),
			Submenu::new("view:zoom", s.get("MenuViewUiZoom"), zooms).icon(glyphs::ZOOM).into(),
			Submenu::new("view:lang", s.get("MenuViewLanguage"), languages).icon(glyphs::LANGUAGE).into(),
			MenuEntry::Separator,
			Submenu::new(
				"view:res",
				s.get("MenuViewResolution"),
				divisors(
					"view:res:",
					MenuAction::Resolution,
					q.resolution_divisor,
					["MenuViewResolutionFull", "MenuViewResolutionHalf", "MenuViewResolutionQuarter"],
				),
			)
			.icon(glyphs::RESOLUTION)
			.into(),
			Submenu::new(
				"view:frames",
				s.get("MenuViewFrames"),
				divisors(
					"view:frames:",
					MenuAction::Frames,
					q.frame_divisor,
					["MenuViewFramesFull", "MenuViewFramesHalf", "MenuViewFramesQuarter"],
				),
			)
			.icon(glyphs::FRAMES)
			.into(),
			Submenu::new(
				"view:samples",
				s.get("MenuViewSamples"),
				vec![
					radio(MenuAction::Samples(SampleFormat::Half), "view:samples:2", s.get("MenuViewSamples16"), half),
					radio(MenuAction::Samples(SampleFormat::Index), "view:samples:1", s.get("MenuViewSamples8"), !half),
				],
			)
			.icon(glyphs::SAMPLES)
			.into(),
			Submenu::new("view:dither", s.get("MenuViewDither"), dither).icon(glyphs::DITHER).enabled(half).into(),
		],
	)
}

fn simulation_menu(app: &App) -> Submenu<MenuAction> {
	let s = &app.strings;
	let st = &app.settings;
	let exporting = app.live.export_busy();
	let active = app.live.run_is_active();
	let playing = app.live.playback.is_playing();
	let paused = app.live.run_paused_by_user();
	let mut ram = vec![radio(
		MenuAction::FrameMemory(0),
		"sim:ram:0",
		s.get("MenuSimulationFrameMemoryAuto"),
		st.frame_memory_percent <= 0,
	)];
	ram.extend((1..=10).map(|i| {
		let pct = i * 10;
		radio(
			MenuAction::FrameMemory(pct),
			format!("sim:ram:{pct}"),
			format!("{pct} %"),
			st.frame_memory_percent == pct,
		)
	}));
	Submenu::new(
		"MenuSimulation",
		s.get("MenuSimulation"),
		vec![
			item(MenuAction::Run, "sim:run", s, "MenuSimulationRun")
				.shortcut("F5")
				.icon(glyphs::RUN)
				.enabled(!active && !exporting)
				.into(),
			item(
				MenuAction::PauseRun,
				"sim:pause",
				s,
				if paused { "MenuSimulationResume" } else { "MenuSimulationPause" },
			)
			.shortcut("F6")
			.icon(if paused { glyphs::RUN } else { glyphs::PAUSE })
			.enabled(active)
			.into(),
			item(MenuAction::Stop, "sim:stop", s, "MenuSimulationStop")
				.shortcut("Shift+F5")
				.icon(glyphs::STOP)
				.enabled(active || exporting)
				.into(),
			MenuEntry::Separator,
			item(
				MenuAction::Play,
				"sim:play",
				s,
				if playing { "MenuSimulationPausePlayback" } else { "MenuSimulationPlay" },
			)
			.shortcut("F7")
			.icon(if playing { glyphs::PAUSE } else { glyphs::RUN })
			.into(),
			MenuEntry::Separator,
			checked(MenuAction::UseGpu, "sim:gpu", s, "MenuSimulationUseGpu", st.use_gpu),
			checked(MenuAction::AutoRerun, "sim:autorerun", s, "MenuSimulationAutoRerun", st.auto_rerun),
			checked(MenuAction::WarnMemory, "sim:warn", s, "MenuSimulationWarnMemory", st.warn_when_ram_insufficient),
			Submenu::new("sim:ram", s.get("MenuSimulationFrameMemory"), ram).icon(glyphs::MEMORY).into(),
		],
	)
}

fn help_menu(app: &App) -> Submenu<MenuAction> {
	let s = &app.strings;
	let mut entries: Vec<Entry> = vec![
		item(MenuAction::About, "help:about", s, "MenuHelpAbout").icon(glyphs::ABOUT).into(),
		item(MenuAction::Manual, "help:manual", s, "MenuHelpManual").shortcut("F1").icon(glyphs::MANUAL).into(),
	];
	if super::update::AVAILABLE {
		entries.push(item(MenuAction::CheckForUpdates, "help:update", s, "MenuHelpCheckNow").into());
		entries.push(checked(
			MenuAction::CheckAtLaunch,
			"help:update-at-launch",
			s,
			"MenuHelpCheckAtLaunch",
			app.settings.check_for_updates == Some(true),
		));
	}
	Submenu::new("MenuHelp", s.get("MenuHelp"), entries)
}

/// The menu bar's model, rebuilt only when what it shows changes.
pub fn use_menu_bar() -> Memo<MenuModel<MenuAction>> {
	let cx = Cx::get();
	use_memo(move || {
		#[cfg(test)]
		super::shared::count_run("menu");
		let topics = Topics::DOCUMENT | Topics::SELECTION | Topics::RUN | Topics::SETTINGS;
		let pastable = *cx.ui.pastable.read();
		cx.shared.read(topics, |app| {
			MenuModel::new(vec![
				file_menu(app, &cx.ui).into(),
				edit_menu(app, pastable).into(),
				view_menu(app, &cx.ui).into(),
				simulation_menu(app).into(),
				help_menu(app).into(),
			])
		})
	})
}

/// The context menu of a tree row; `row` is the row it opened on, `pastable` the clipboard's state.
pub fn tree_row_menu(app: &App, row: Option<Row>, pastable: bool) -> MenuModel<MenuAction> {
	let s = &app.strings;
	let item_ref = match row {
		Some(Row::Item(i)) => Some(i),
		_ => None,
	};
	let is_item = item_ref.is_some();
	let toggles = item_ref.is_some_and(|i| i.kind != ItemKind::Scheme);
	let can_up = item_ref.is_some_and(|i| can_move(app, i, true));
	let can_down = item_ref.is_some_and(|i| can_move(app, i, false));
	let copyable = is_item || row.and_then(Copyable::of_row).is_some();
	let mut entries: Vec<Entry> = vec![
		item(MenuAction::AddSource, "tree:add_source", s, "MenuProjectAddSource").icon(icons::PLUS).into(),
		Submenu::new("tree:add_part", s.get("MenuProjectAddPart"), part_entries(s, "tree:add_part:"))
			.icon(icons::PLUS)
			.into(),
		Submenu::new("tree:add_probe", s.get("MenuProjectAddProbe"), probe_entries(s, "tree:add_probe:"))
			.icon(icons::PLUS)
			.into(),
		MenuEntry::Separator,
		item(MenuAction::ToggleSelected, "tree:toggle_selected", s, "MenuProjectToggleSelected")
			.shortcut("Ins")
			.icon(glyphs::TOGGLE)
			.enabled(is_item)
			.into(),
		item(MenuAction::SelectAll, "tree:select_all", s, "MenuProjectSelectAll")
			.shortcut("Ctrl+A")
			.icon(glyphs::SELECT_ALL)
			.into(),
		MenuEntry::Separator,
		item(MenuAction::ToggleEnabled, "tree:enabled", s, "MenuProjectToggleEnabled")
			.shortcut("Space")
			.icon(glyphs::ENABLED)
			.enabled(toggles)
			.into(),
		item(MenuAction::Duplicate, "tree:duplicate", s, "MenuProjectDuplicate")
			.shortcut("Ctrl+D")
			.icon(glyphs::DUPLICATE)
			.enabled(is_item)
			.into(),
		item(MenuAction::Cut, "tree:cut", s, "MenuEditCut").icon(glyphs::CUT).enabled(is_item).into(),
		item(MenuAction::Copy, "tree:copy", s, "MenuEditCopy").icon(glyphs::COPY).enabled(copyable).into(),
		item(MenuAction::Paste, "tree:paste", s, "MenuEditPaste").icon(glyphs::PASTE).enabled(pastable).into(),
		item(MenuAction::Delete, "tree:remove", s, "MenuProjectRemove")
			.shortcut("Del")
			.icon(glyphs::DELETE)
			.enabled(is_item)
			.into(),
		MenuEntry::Separator,
		item(MenuAction::MoveUp, "tree:up", s, "MenuProjectMoveUp")
			.shortcut("Alt+Up")
			.icon(icons::CHEVRON_UP)
			.enabled(can_up)
			.into(),
		item(MenuAction::MoveDown, "tree:down", s, "MenuProjectMoveDown")
			.shortcut("Alt+Down")
			.icon(icons::CHEVRON_DOWN)
			.enabled(can_down)
			.into(),
	];
	if let Some(i) = item_ref.filter(|i| i.kind == ItemKind::Probe) {
		let active = app.document.project.probes[i.index].active;
		entries.push(MenuEntry::Separator);
		entries.push(
			item(MenuAction::ExportProbeCsv, "tree:csv", s, "MenuExportProbeCsv")
				.icon(glyphs::EXPORT)
				.enabled(active && !app.live.export_busy())
				.into(),
		);
	}
	MenuModel::new(entries)
}

/// Moves the target (or each selected item of its list) one place up or down.
pub fn move_item(cx: &Cx, target: ItemRef, up: bool) {
	let moves = cx.shared.peek(|a| moves_of(a, target, up));
	for (index, to) in moves {
		cx.send(AppEvent::Command(Command::Reorder { target: ItemRef::new(target.kind, index), to }));
	}
}

/// Whether Move up (`up`) or Move down shifts `target` and the items moving with it.
pub fn can_move(app: &App, target: ItemRef, up: bool) -> bool {
	!moves_of(app, target, up).is_empty()
}

fn moves_of(app: &App, target: ItemRef, up: bool) -> Vec<(usize, usize)> {
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

/// Ctrl+A: every source, part and probe.
pub fn select_all(cx: &Cx) {
	cx.shared.update(|a| {
		let project = a.document.project.clone();
		a.selection.select_all(&project);
	});
}

/// Paste of the clipboard's text behind the current item (or `row`).
pub fn paste(cx: &Cx, row: Option<Row>, before: bool) {
	let text = match Clipboard::get() {
		Ok(t) => t,
		Err(e) => {
			tracing::debug!("nothing to paste: {e:?}");
			return;
		}
	};
	let current = row.or_else(|| cx.shared.peek(|a| a.selection.current()));
	cx.send(AppEvent::Paste { text, target: PasteTarget { current, before }, place_at: None });
}

fn set_quality(cx: &Cx, f: impl FnOnce(&mut PreviewQuality)) {
	let mut q = cx.shared.peek(|a| a.settings.preview_quality());
	f(&mut q);
	cx.send(AppEvent::SetPreviewQuality(q));
}

/// Runs a menu bar action or a shortcut on the current item.
pub fn dispatch(cx: &Cx, action: MenuAction) {
	act(cx, action, On::Current);
}

/// Runs `action`; item actions apply to `on`.
pub fn act(cx: &Cx, action: MenuAction, on: On) {
	let shared: &Shared = &cx.shared;
	shared.flush_edit();
	let target = || match on {
		On::Current => shared.peek(|a| a.selection.current_item()),
		On::Row(Some(Row::Item(i))) => Some(i),
		On::Row(_) => None,
	};
	let on_target = |f: &dyn Fn(ItemRef)| {
		if let Some(t) = target() {
			f(t);
		}
	};
	let command = |c: Command| cx.send(AppEvent::Command(c));
	match action {
		MenuAction::New => cx.send(AppEvent::Guarded(Guarded::New)),
		MenuAction::Open => dialogs::open_project(cx),
		MenuAction::Recent(i) => {
			let path = shared.peek(|a| a.settings.recent_files[i].clone());
			cx.send(AppEvent::Guarded(Guarded::Open(path)));
		}
		MenuAction::Examples => cx.ui.show_modal(Modal::Examples),
		MenuAction::Save => cx.send(AppEvent::Save),
		MenuAction::SaveAs => dialogs::save_as(cx, None),
		MenuAction::Export(choice) => export_dialog::start(cx, choice),
		MenuAction::ExportLast => export_dialog::export_last(cx),
		MenuAction::Close => cx.send(AppEvent::Guarded(Guarded::Close)),
		MenuAction::Exit => cx.send(AppEvent::Guarded(Guarded::Exit)),
		MenuAction::Undo => cx.send(AppEvent::Undo),
		MenuAction::Redo => cx.send(AppEvent::Redo),
		MenuAction::Cut => on_target(&|target| cx.send(AppEvent::Cut { target })),
		MenuAction::Copy => {
			if let On::Row(Some(r)) = on {
				let selected = shared.peek(|a| matches!(r, Row::Item(i) if a.selection.contains(i)));
				if !selected {
					shared.update(|a| a.selection.select(r));
				}
			}
			cx.send(AppEvent::Copy);
		}
		MenuAction::Paste => paste(cx, if let On::Row(r) = on { r } else { None }, false),
		MenuAction::Delete => on_target(&|target| command(Command::Remove { target })),
		MenuAction::SelectAll => select_all(cx),
		MenuAction::Pane(id) => window::toggle_pane(&cx.ui, id),
		MenuAction::Theme(theme) => cx.send(AppEvent::SetTheme(theme)),
		MenuAction::Zoom(z) => cx.send(AppEvent::SetMenuZoom(z)),
		MenuAction::Language(l) => cx.send(AppEvent::SetLanguage(l.map_or_else(String::new, |l| l.code().to_owned()))),
		MenuAction::Resolution(d) => set_quality(cx, |q| q.resolution_divisor = d),
		MenuAction::Frames(d) => set_quality(cx, |q| q.frame_divisor = d),
		MenuAction::Samples(format) => set_quality(cx, |q| q.format = format),
		MenuAction::Dither(bits) => cx.send(AppEvent::SetDitherBits(bits)),
		MenuAction::Run => cx.send(AppEvent::Run),
		MenuAction::PauseRun => cx.send(AppEvent::TogglePauseRun),
		MenuAction::Stop => {
			if !export_dialog::cancel(cx) {
				cx.send(AppEvent::StopRun);
			}
		}
		MenuAction::Play => {
			let playing = shared.peek(|a| a.live.playback.is_playing());
			cx.send(if playing { AppEvent::PausePlayback } else { AppEvent::Play });
		}
		MenuAction::UseGpu => cx.send(AppEvent::SetUseGpu(!shared.peek(|a| a.settings.use_gpu))),
		MenuAction::AutoRerun => cx.send(AppEvent::SetAutoRerun(!shared.peek(|a| a.settings.auto_rerun))),
		MenuAction::WarnMemory => {
			cx.send(AppEvent::SetWarnWhenRamInsufficient(!shared.peek(|a| a.settings.warn_when_ram_insufficient)));
		}
		MenuAction::FrameMemory(pct) => cx.send(AppEvent::SetFrameMemoryPercent(pct)),
		MenuAction::About => cx.ui.show_modal(Modal::About),
		MenuAction::CheckForUpdates => super::update::check(cx, false),
		MenuAction::CheckAtLaunch => {
			cx.send(AppEvent::SetCheckForUpdates(shared.peek(|a| a.settings.check_for_updates) != Some(true)));
		}
		MenuAction::Manual => {
			let mut r = cx.ui.manual_request;
			*r.write() += 1;
		}
		MenuAction::AddSource => super::tree::add_then_edit(cx, Command::AddSource { at: None }),
		MenuAction::AddPart(kind) => super::tree::add_then_edit(cx, Command::AddPart { kind, at: None }),
		MenuAction::AddProbe(detector) => super::tree::add_then_edit(cx, Command::AddProbe { at: None, detector }),
		MenuAction::ToggleSelected => on_target(&|target| shared.update(|a| a.selection.toggle(target))),
		MenuAction::ToggleEnabled => on_target(&|target| command(Command::ToggleEnabled { target })),
		MenuAction::Duplicate => on_target(&|target| command(Command::Duplicate { target })),
		MenuAction::MoveUp => on_target(&|target| move_item(cx, target, true)),
		MenuAction::MoveDown => on_target(&|target| move_item(cx, target, false)),
		MenuAction::ExportProbeCsv => on_target(&|target| export_dialog::export_probe_csv(cx, target.index)),
	}
}

#[cfg(test)]
mod tests {
	use super::block_moves;

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
