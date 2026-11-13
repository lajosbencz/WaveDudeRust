//! The menu bar of docs/ui/10 (Menu bar): File, Edit, View, Simulation and Help, each entry a [`MenuAction`] with
//! its icon (or check mark) in a 24 px column and its shortcut on the right.

use eframe::egui::containers::menu::{MenuButton, SubMenu, SubMenuButton};
use eframe::egui::{self, Atom, AtomExt, Button, IdSalt, Rect, Role, Vec2, WidgetInfo};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::*;
use wavedude_app_state::platform::{HAS_EXPORTS, HAS_PATHS};
use wavedude_app_state::settings::{DITHER_BITS, Theme as ThemeSetting};
use wavedude_app_state::strings::{Language, Strings};
use wavedude_core::engine::SampleFormat;
use wavedude_core::model::Word;

use super::dock::Dock;
use super::{icons, menu_keys, shortcuts};
use crate::App;
use crate::menus::{ExportKindChoice, MenuAction, ZOOMS};
use crate::window::Pane;

const ICON_SLOT: f32 = 24.;
const ICON_SIZE: f32 = 16.;
const ENTRY_HEIGHT: f32 = 22.;
const MIN_PANEL_WIDTH: f32 = 200.;

/// The menu bar; the action of the entry clicked this frame. `pastable` is read when the Edit menu opens and
/// forgotten when it closes; `has_last` enables File > Export last.
pub(super) fn show(
	ui: &mut egui::Ui,
	app: &App,
	dock: &Dock,
	pastable: &mut Option<bool>,
	has_last: bool,
) -> Option<MenuAction> {
	let mut menu = Menu { s: &app.strings, picked: None };
	let mut edit_open = false;
	egui::MenuBar::new().ui(ui, |ui| {
		menu.top(ui, "MenuFile", |m, ui| {
			m.file(ui, app);
			if HAS_EXPORTS {
				m.export(ui, app, has_last);
			}
			m.exit(ui, app);
		});
		menu.top(ui, "MenuEdit", |m, ui| {
			edit_open = true;
			m.edit(ui, app, *pastable.get_or_insert_with(super::platform::pastable));
		});
		menu.top(ui, "MenuView", |m, ui| m.view(ui, app, dock));
		menu.top(ui, "MenuSimulation", |m, ui| m.simulation(ui, app));
		menu.top(ui, "MenuHelp", |m, ui| {
			m.item(ui, MenuAction::About, ICON_INFO, "MenuHelpAbout", true);
			m.item(ui, MenuAction::Manual, ICON_MENU_BOOK, "MenuHelpManual", true);
			#[cfg(not(target_arch = "wasm32"))]
			if crate::update::AVAILABLE {
				m.updater(ui, app);
			}
		});
	});
	if !edit_open {
		*pastable = None;
	}
	menu.picked
}

/// What an entry shows in its icon column.
#[derive(Clone, Copy)]
pub(super) enum Slot {
	Empty,
	Icon(MaterialIcon),
	Drawn(icons::Icon),
}

impl From<Option<MaterialIcon>> for Slot {
	fn from(icon: Option<MaterialIcon>) -> Self {
		icon.map_or(Slot::Empty, Slot::Icon)
	}
}

/// The id of a drawn icon's room in its entry.
fn drawn_id() -> IdSalt {
	IdSalt::new("drawn_icon")
}

/// Where an entry's icon goes: the icon, or room for a drawn one or for nothing in the column's width.
fn slot(icon: Slot) -> Atom<'static> {
	let size = Vec2::new(ICON_SLOT, ICON_SIZE);
	match icon {
		Slot::Icon(icon) => icon.rich_text().size(ICON_SIZE).atom_size(size),
		Slot::Drawn(_) => Atom::custom(drawn_id(), size),
		Slot::Empty => Atom::default().atom_size(size),
	}
}

/// Adds the entry `button` with `icon` in its column.
fn add_entry(ui: &mut egui::Ui, button: Button<'_>, icon: Slot) -> egui::Response {
	let shown = button.atom_ui(ui);
	if let (Slot::Drawn(drawn), Some(rect)) = (icon, shown.rect(drawn_id())) {
		let color = ui.style().interact(&shown.response).fg_stroke.color;
		icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(ICON_SIZE)), drawn, color);
	}
	shown.response
}

/// Names an entry by its text alone for assistive technology, not by its icon glyph and shortcut.
fn named(response: &egui::Response, text: &str, checked: Option<bool>) {
	let enabled = response.enabled();
	response.widget_info(|| match checked {
		Some(on) => WidgetInfo::selected(Role::MenuItemCheckBox, enabled, on, text),
		None => WidgetInfo::labeled(Role::MenuItem, enabled, text),
	});
}

/// A menu entry, `marked` its text with its mnemonic, `icon` in its column, `checked` its check mark's state for
/// one that has it; clicking it closes the menu.
pub(super) fn entry(
	ui: &mut egui::Ui,
	icon: impl Into<Slot>,
	marked: &str,
	checked: Option<bool>,
	shortcut: Option<String>,
) -> egui::Response {
	let icon = icon.into();
	let (label, text) = menu_keys::label(ui, marked);
	let mut button = Button::new((slot(icon), label)).min_size(Vec2::new(0., ENTRY_HEIGHT));
	if let Some(shortcut) = shortcut {
		button = button.shortcut_text(shortcut);
	}
	let response = add_entry(ui, button, icon);
	named(&response, &text, checked);
	menu_keys::entry(ui, &response, marked, false);
	if response.clicked() {
		ui.close();
	}
	response
}

/// A drop-down entry's swatch: its size and what paints it.
pub(super) type Swatch<'a> = (Vec2, &'a dyn Fn(&egui::Ui, Rect));

/// An entry of a drop-down: `text` as it is (no mnemonic), its check mark while `checked`, and before the text a
/// `size` room that `paint` draws into, if any; `italic` for an action. Clicking it closes the menu.
pub(super) fn choice_entry(
	ui: &mut egui::Ui,
	text: &str,
	checked: bool,
	italic: bool,
	swatch: Option<Swatch<'_>>,
) -> egui::Response {
	let room = IdSalt::new("choice_swatch");
	let icon: Slot = checked.then_some(ICON_CHECK).into();
	let word = if italic { egui::RichText::new(text).italics() } else { egui::RichText::new(text) };
	let mut atoms = egui::Atoms::new(slot(icon));
	if let Some((size, _)) = swatch {
		atoms.push_right(Atom::custom(room, size));
	}
	atoms.push_right(word);
	let shown = Button::new(atoms).min_size(Vec2::new(0., ENTRY_HEIGHT)).atom_ui(ui);
	if let (Some((_, paint)), Some(rect)) = (swatch, shown.rect(room)) {
		paint(ui, rect);
	}
	let response = shown.response;
	named(&response, text, Some(checked));
	menu_keys::entry(ui, &response, "", false);
	if response.clicked() {
		ui.close();
	}
	response
}

/// An entry with a check mark (or radio dot) while `on`, outside the menu bar.
pub(super) fn check_entry(ui: &mut egui::Ui, marked: &str, on: bool) -> egui::Response {
	entry(ui, on.then_some(ICON_CHECK), marked, Some(on), None)
}

/// A submenu's entry, `marked` its text with its mnemonic, `icon` in its column; `content` fills the submenu.
pub(super) fn submenu(
	ui: &mut egui::Ui,
	icon: Slot,
	marked: &str,
	enabled: bool,
	content: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
	ui.add_enabled_ui(enabled, |ui| {
		let (label, text) = menu_keys::label(ui, marked);
		let button = Button::new((slot(icon), label))
			.min_size(Vec2::new(0., ENTRY_HEIGHT))
			.right_text(SubMenuButton::arrow_atom(None));
		let (response, shown) = SubMenuButton::from_button(button).ui(ui, |ui| {
			ui.set_min_width(MIN_PANEL_WIDTH);
			content(ui);
		});
		if let Slot::Drawn(drawn) = icon {
			let x = response.rect.left() + ui.spacing().button_padding.x + ICON_SLOT / 2.;
			let room = Rect::from_center_size(egui::pos2(x, response.rect.center().y), Vec2::splat(ICON_SIZE));
			icons::paint(ui, room, drawn, ui.style().interact(&response).fg_stroke.color);
		}
		named(&response, &text, None);
		menu_keys::entry(ui, &response, marked, true);
		if shown.is_some() {
			menu_keys::menu(ui.ctx(), SubMenu::id_from_widget_id(response.id), response.id);
		}
		response
	})
	.inner
}

struct Menu<'a> {
	s: &'a Strings,
	picked: Option<MenuAction>,
}

impl Menu<'_> {
	fn top(&mut self, ui: &mut egui::Ui, key: &str, content: impl FnOnce(&mut Self, &mut egui::Ui)) {
		let marked = self.s.get(key);
		let button = Button::new(menu_keys::label(ui, marked).0).sense(menu_keys::header_sense(ui.ctx()));
		let (response, shown) = MenuButton::from_button(button).ui(ui, |ui| {
			ui.set_min_width(MIN_PANEL_WIDTH);
			content(self, ui);
		});
		menu_keys::header(&response, marked, shown.is_some());
	}

	/// An entry, `marked` its text with its mnemonic; `checked` is its check mark's state, for one that has it.
	fn button(
		&mut self,
		ui: &mut egui::Ui,
		action: MenuAction,
		icon: Option<MaterialIcon>,
		marked: &str,
		checked: Option<bool>,
	) {
		if entry(ui, icon, marked, checked, shortcuts::text(ui.ctx(), action)).clicked() {
			self.picked = Some(action);
		}
	}

	/// An entry named by string `key`.
	fn item(&mut self, ui: &mut egui::Ui, action: MenuAction, icon: MaterialIcon, key: &str, enabled: bool) {
		let marked = self.s.get(key);
		ui.add_enabled_ui(enabled, |ui| self.button(ui, action, Some(icon), marked, None));
	}

	/// An entry with a check mark (or radio dot) while `on`.
	fn check(&mut self, ui: &mut egui::Ui, action: MenuAction, marked: &str, on: bool) {
		self.button(ui, action, on.then_some(ICON_CHECK), marked, Some(on));
	}

	fn submenu(
		&mut self,
		ui: &mut egui::Ui,
		icon: MaterialIcon,
		key: &str,
		enabled: bool,
		content: impl FnOnce(&mut Self, &mut egui::Ui),
	) {
		submenu(ui, Slot::Icon(icon), self.s.get(key), enabled, |ui| content(self, ui));
	}

	fn file(&mut self, ui: &mut egui::Ui, app: &App) {
		self.item(ui, MenuAction::New, ICON_NOTE_ADD, "MenuFileNew", true);
		self.item(ui, MenuAction::Open, ICON_FOLDER_OPEN, "MenuFileOpen", true);
		if HAS_PATHS {
			self.submenu(ui, ICON_HISTORY, "MenuFileRecent", true, |m, ui| {
				for (i, path) in app.settings.recent_files.iter().enumerate() {
					// A path has no mnemonic: its underscores are literal.
					let text = path.display().to_string().replace('_', "__");
					m.button(ui, MenuAction::Recent(i), None, &text, None);
				}
			});
		}
		self.item(ui, MenuAction::Examples, ICON_LIST_ALT, "MenuFileExamples", true);
		ui.separator();
		self.item(ui, MenuAction::Save, ICON_SAVE, "MenuFileSave", true);
		self.item(ui, MenuAction::SaveAs, ICON_SAVE_AS, "MenuFileSaveAs", true);
	}

	/// File's exports; Export last while `has_last`.
	fn export(&mut self, ui: &mut egui::Ui, app: &App, has_last: bool) {
		let exporting = app.live.export_busy();
		ui.separator();
		self.submenu(ui, ICON_IOS_SHARE, "MenuFileExport", !exporting, |m, ui| {
			let export = |m: &mut Self, ui: &mut egui::Ui, choice, icon, key| {
				m.item(ui, MenuAction::Export(choice), icon, key, true);
			};
			export(m, ui, ExportKindChoice::Field, ICON_IOS_SHARE, "MenuFileExportField");
			export(m, ui, ExportKindChoice::Intensity, ICON_IOS_SHARE, "MenuFileExportIntensity");
			export(m, ui, ExportKindChoice::Both, ICON_IOS_SHARE, "MenuFileExportBoth");
			ui.separator();
			export(m, ui, ExportKindChoice::FieldFrame, ICON_IMAGE, "MenuFileExportFieldFrame");
			export(m, ui, ExportKindChoice::IntensityFrame, ICON_IMAGE, "MenuFileExportIntensityFrame");
		});
		self.item(ui, MenuAction::ExportLast, ICON_IOS_SHARE, "MenuFileExportLast", has_last && !exporting);
	}

	fn exit(&mut self, ui: &mut egui::Ui, app: &App) {
		ui.separator();
		self.item(ui, MenuAction::Close, ICON_CLOSE, "MenuFileClose", !app.document.is_placeholder());
		#[cfg(not(target_arch = "wasm32"))]
		{
			ui.separator();
			self.item(ui, MenuAction::Exit, ICON_LOGOUT, "MenuFileExit", true);
		}
	}

	fn edit(&mut self, ui: &mut egui::Ui, app: &App, pastable: bool) {
		let history = &app.document.history;
		let items = !app.selection.items().is_empty();
		let copyable =
			items || app.selection.current().and_then(wavedude_app_state::clipboard::Copyable::of_row).is_some();
		let project = &app.document.project;
		let any = !(project.sources.is_empty() && project.parts.is_empty() && project.probes.is_empty());
		self.item(ui, MenuAction::Undo, ICON_UNDO, "MenuEditUndo", history.can_undo() || history.has_pending());
		self.item(ui, MenuAction::Redo, ICON_REDO, "MenuEditRedo", history.can_redo());
		ui.separator();
		self.item(ui, MenuAction::Cut, ICON_CONTENT_CUT, "MenuEditCut", items);
		self.item(ui, MenuAction::Copy, ICON_CONTENT_COPY, "MenuEditCopy", copyable);
		self.item(ui, MenuAction::Paste, ICON_CONTENT_PASTE, "MenuEditPaste", pastable);
		self.item(ui, MenuAction::Delete, ICON_DELETE, "MenuEditDelete", items);
		ui.separator();
		self.item(ui, MenuAction::SelectAll, ICON_SELECT_ALL, "MenuProjectSelectAll", any);
	}

	fn view(&mut self, ui: &mut egui::Ui, app: &App, dock: &Dock) {
		let st = &app.settings;
		let s = self.s;
		self.submenu(ui, ICON_VIEW_SIDEBAR, "MenuViewPanes", true, |m, ui| {
			for pane in Pane::ALL {
				m.check(ui, MenuAction::Pane(pane), s.get(pane.title_key()), dock.is_shown(pane));
			}
		});
		self.submenu(ui, ICON_CONTRAST, "MenuViewTheme", true, |m, ui| {
			for &t in ThemeSetting::MEMBERS {
				m.check(ui, MenuAction::Theme(t), s.enum_word(ThemeSetting::TYPE_NAME, t.name()), st.theme == t);
			}
		});
		self.submenu(ui, ICON_ZOOM_IN, "MenuViewUiZoom", true, |m, ui| {
			for &(z, key) in &ZOOMS {
				m.check(ui, MenuAction::Zoom(z), s.get(key), (st.ui_zoom - z).abs() < 1e-6);
			}
		});
		self.submenu(ui, ICON_LANGUAGE, "MenuViewLanguage", true, |m, ui| {
			m.check(ui, MenuAction::Language(None), s.get("LanguageSystem"), st.language.is_empty());
			for l in Language::ALL {
				m.check(ui, MenuAction::Language(Some(l)), l.native_name(), st.language == l.code());
			}
		});
		ui.separator();
		let q = st.preview_quality();
		let divisors = |m: &mut Self, ui: &mut egui::Ui, action: fn(u32) -> MenuAction, value, keys: [&str; 3]| {
			for (d, key) in [1, 2, 4].into_iter().zip(keys) {
				m.check(ui, action(d), s.get(key), value == d);
			}
		};
		self.submenu(ui, ICON_GRID_ON, "MenuViewResolution", true, |m, ui| {
			let keys = ["MenuViewResolutionFull", "MenuViewResolutionHalf", "MenuViewResolutionQuarter"];
			divisors(m, ui, MenuAction::Resolution, q.resolution_divisor, keys);
		});
		self.submenu(ui, ICON_VIEW_WEEK, "MenuViewFrames", true, |m, ui| {
			let keys = ["MenuViewFramesFull", "MenuViewFramesHalf", "MenuViewFramesQuarter"];
			divisors(m, ui, MenuAction::Frames, q.frame_divisor, keys);
		});
		let half = q.format == SampleFormat::Half;
		self.submenu(ui, ICON_STAIRS, "MenuViewSamples", true, |m, ui| {
			m.check(ui, MenuAction::Samples(SampleFormat::Half), s.get("MenuViewSamples16"), half);
			m.check(ui, MenuAction::Samples(SampleFormat::Index), s.get("MenuViewSamples8"), !half);
		});
		self.submenu(ui, ICON_GRAIN, "MenuViewDither", half, |m, ui| {
			let keys =
				["MenuViewDitherOff", "MenuViewDither10", "MenuViewDither12", "MenuViewDither14", "MenuViewDither16"];
			for (&bits, key) in DITHER_BITS.iter().zip(keys) {
				m.check(ui, MenuAction::Dither(bits), s.get(key), st.preview_dither_bits == bits);
			}
		});
	}

	fn simulation(&mut self, ui: &mut egui::Ui, app: &App) {
		let st = &app.settings;
		let s = self.s;
		let live = &app.live;
		let (exporting, active) = (live.export_busy(), live.run_is_active());
		let (paused, playing) = (live.run_paused_by_user(), live.playback.is_playing());
		self.item(ui, MenuAction::Run, ICON_PLAY_ARROW, "MenuSimulationRun", !active && !exporting);
		let (icon, key) =
			if paused { (ICON_PLAY_ARROW, "MenuSimulationResume") } else { (ICON_PAUSE, "MenuSimulationPause") };
		self.item(ui, MenuAction::PauseRun, icon, key, active);
		self.item(ui, MenuAction::Stop, ICON_STOP, "MenuSimulationStop", active || exporting);
		let discardable = live.has_frames() && !exporting;
		self.item(ui, MenuAction::DiscardFrames, ICON_DELETE_SWEEP, "MenuSimulationDiscardFrames", discardable);
		ui.separator();
		let (icon, key) =
			if playing { (ICON_PAUSE, "MenuSimulationPausePlayback") } else { (ICON_PLAY_ARROW, "MenuSimulationPlay") };
		self.item(ui, MenuAction::Play, icon, key, true);
		ui.separator();
		self.check(ui, MenuAction::UseGpu, s.get("MenuSimulationUseGpu"), st.use_gpu);
		self.check(ui, MenuAction::AutoRerun, s.get("MenuSimulationAutoRerun"), st.auto_rerun);
		self.check(ui, MenuAction::WarnMemory, s.get("MenuSimulationWarnMemory"), st.warn_when_ram_insufficient);
		self.submenu(ui, ICON_MEMORY, "MenuSimulationFrameMemory", true, |m, ui| {
			let auto = st.frame_memory_percent <= 0;
			m.check(ui, MenuAction::FrameMemory(0), s.get("MenuSimulationFrameMemoryAuto"), auto);
			for pct in (10..=100).step_by(10) {
				m.check(ui, MenuAction::FrameMemory(pct), &format!("{pct} %"), st.frame_memory_percent == pct);
			}
		});
	}

	#[cfg(not(target_arch = "wasm32"))]
	fn updater(&mut self, ui: &mut egui::Ui, app: &App) {
		let s = self.s;
		self.button(ui, MenuAction::CheckForUpdates, None, s.get("MenuHelpCheckNow"), None);
		let on = app.settings.check_for_updates == Some(true);
		self.check(ui, MenuAction::CheckAtLaunch, s.get("MenuHelpCheckAtLaunch"), on);
	}
}
