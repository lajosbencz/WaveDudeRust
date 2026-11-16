//! The Settings window (docs/ui/70, Settings; decision 0027): every user setting as a row of a section, a search over
//! the rows' labels in the current language and in English and over the sections' names. A change applies and is
//! saved at once; Close is the only button.

use std::borrow::Cow;

use eframe::egui::{
	self, Align, Align2, Color32, FontId, Id, Key, Layout, Modifiers, Response, RichText, Role, ScrollArea, Sense,
	Stroke, StrokeKind, TextEdit, Ui, WidgetInfo, vec2,
};
use wavedude_app_state::Event;
#[cfg(target_arch = "wasm32")]
use wavedude_app_state::live::{WEB_RAM_CACHE_SIZES, detected_web_frame_memory};
use wavedude_app_state::platform::machine_facts;
use wavedude_app_state::settings::{DITHER_BITS, Settings as UserSettings, Theme as ThemeSetting, UNDO_STEPS};
use wavedude_app_state::strings::{Language, Strings, strip_mnemonic};
use wavedude_core::engine::SampleFormat;
use wavedude_core::model::Word;
use wavedude_core::quality::{PreviewDefaults, PreviewTier};
#[cfg(target_arch = "wasm32")]
use wavedude_core::run::RamCache;

use super::export::{h264_profiles, profile_combo, profile_note};
use super::memory_warning::bytes_text;
use super::modal::{self, Choice, Reply, Then, buttons};
use super::number_box::{NumberSpec, number_box};
use super::properties::{Choices, check_box, combo};
use super::theme::{FONT_SIZE, INPUT_HEIGHT, Palette};
use crate::App;
use crate::menus::ZoomStep;

/// The UI zooms: factor and string key.
const ZOOMS: [(f64, &str); 8] = [
	(0.5, "MenuViewUiZoom50"),
	(0.75, "MenuViewUiZoom75"),
	(1.0, "MenuViewUiZoom100"),
	(1.25, "MenuViewUiZoom125"),
	(1.5, "MenuViewUiZoom150"),
	(2.0, "MenuViewUiZoom200"),
	(2.5, "MenuViewUiZoom250"),
	(3.0, "MenuViewUiZoom300"),
];

/// The preview's resolution lever: divisor and string key.
const RESOLUTIONS: [(u32, &str); 3] =
	[(1, "MenuViewResolutionFull"), (2, "MenuViewResolutionHalf"), (4, "MenuViewResolutionQuarter")];

/// The preview's frames lever: divisor and string key.
const FRAME_STEPS: [(u32, &str); 3] =
	[(1, "MenuViewFramesFull"), (2, "MenuViewFramesHalf"), (4, "MenuViewFramesQuarter")];

/// The preview's samples lever: format and string key.
const SAMPLES: [(SampleFormat, &str); 2] =
	[(SampleFormat::Half, "MenuViewSamples16"), (SampleFormat::Index, "MenuViewSamples8")];

/// The preview's dither lever: bits and string key.
const DITHERS: [(u32, &str); DITHER_BITS.len()] = [
	(DITHER_BITS[0], "MenuViewDitherOff"),
	(DITHER_BITS[1], "MenuViewDither10"),
	(DITHER_BITS[2], "MenuViewDither12"),
	(DITHER_BITS[3], "MenuViewDither14"),
	(DITHER_BITS[4], "MenuViewDither16"),
];

/// The UI zoom a zoom key sets from `st`'s, if it changes: the next of [`ZOOMS`] up or down, or the device's default.
pub(super) fn zoom_step(st: &UserSettings, step: ZoomStep) -> Option<f64> {
	let now = st.ui_zoom;
	let zooms = ZOOMS.iter().map(|&(z, _)| z);
	let zoom = match step {
		ZoomStep::In => zooms.filter(|&z| z > now + 1e-6).reduce(f64::min),
		ZoomStep::Out => zooms.filter(|&z| z < now - 1e-6).reduce(f64::max),
		ZoomStep::Reset => Some(UserSettings::for_device(st.touch_first).ui_zoom),
	};
	zoom.filter(|&z| (z - now).abs() > 1e-6)
}

const WIDTH: f32 = 760.;
const LABEL_WIDTH: f32 = 180.;
const SECTIONS_WIDTH: f32 = 170.;
const SECTION_HEIGHT: f32 = 26.;
const BODY_HEIGHT: f32 = 380.;
const NUMBER_WIDTH: f32 = 100.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
	General,
	Preview,
	Simulation,
	Export,
	Updates,
}

impl Section {
	const ALL: [Self; 5] = [Self::General, Self::Preview, Self::Simulation, Self::Export, Self::Updates];

	const fn title_key(self) -> &'static str {
		match self {
			Self::General => "SettingsGeneral",
			Self::Preview => "SettingsPreview",
			Self::Simulation => "MenuSimulation",
			Self::Export => "SettingsExport",
			Self::Updates => "StartupUpdates",
		}
	}
}

/// What the build offers that rows depend on.
#[derive(Clone, Copy, Debug)]
struct Build {
	web: bool,
	/// `OpenH264` is compiled in.
	codec_built_in: bool,
	updater: bool,
}

#[cfg(not(target_arch = "wasm32"))]
const BUILD: Build =
	Build { web: false, codec_built_in: wavedude_export::h264::BUILT_IN, updater: crate::update::AVAILABLE };
#[cfg(target_arch = "wasm32")]
const BUILD: Build = Build { web: true, codec_built_in: false, updater: false };

/// Which builds show a row.
#[derive(Clone, Copy)]
enum Need {
	Always,
	Native,
	/// Natively, while `OpenH264` is downloaded from Cisco.
	CodecDownload,
	Updater,
}

impl Need {
	fn met(self, b: Build) -> bool {
		match self {
			Self::Always => true,
			Self::Native => !b.web,
			Self::CodecDownload => !b.web && !b.codec_built_in,
			Self::Updater => !b.web && b.updater,
		}
	}
}

/// A row's label: its string key and its text in the current language.
#[derive(Clone, Copy)]
struct Name<'a> {
	key: &'static str,
	text: &'a str,
}

impl Name<'_> {
	fn id(self) -> Id {
		Id::unique(("Settings", self.key))
	}
}

/// A row: its section, label key, the builds showing it and its control.
struct Row {
	section: Section,
	label: &'static str,
	need: Need,
	control: fn(&mut Ui, &mut Cx<'_>, Name<'_>),
}

const fn row(section: Section, label: &'static str, need: Need, control: fn(&mut Ui, &mut Cx<'_>, Name<'_>)) -> Row {
	Row { section, label, need, control }
}

const ROWS: [Row; 20] = [
	row(Section::General, "MenuViewTheme", Need::Always, theme),
	row(Section::General, "MenuViewLanguage", Need::Always, language),
	row(Section::General, "MenuViewUiZoom", Need::Always, zoom),
	row(Section::General, "SettingsTitleFullPath", Need::Always, title_full_path),
	row(Section::General, "SettingsShowStartup", Need::Always, show_startup),
	row(Section::General, "SettingsUndoSteps", Need::Always, undo_steps),
	row(Section::Preview, "SettingsRam", Need::Always, ram),
	row(Section::Preview, "SettingsPreset", Need::Always, preset),
	row(Section::Preview, "MenuViewResolution", Need::Always, resolution),
	row(Section::Preview, "MenuViewFrames", Need::Always, frames),
	row(Section::Preview, "MenuViewSamples", Need::Always, samples),
	row(Section::Preview, "MenuViewDither", Need::Always, dither),
	row(Section::Preview, "MenuSimulationUseGpu", Need::Always, use_gpu),
	row(Section::Simulation, "MenuSimulationAutoRerun", Need::Always, auto_rerun),
	row(Section::Simulation, "MenuSimulationWarnMemory", Need::Always, warn_memory),
	row(Section::Export, "ExportUseOpenH264", Need::CodecDownload, openh264),
	row(Section::Export, "ExportH264Profile", Need::Always, h264_profile),
	row(Section::Export, "SettingsExportArchive", Need::Native, archive),
	row(Section::Updates, "SettingsCheckForUpdates", Need::Updater, check_at_launch),
	row(Section::Updates, "SettingsCheckNow", Need::Updater, check_now),
];

/// The indices into [`ROWS`] that `build` shows.
fn shown_rows(build: Build) -> impl Iterator<Item = usize> {
	(0..ROWS.len()).filter(move |&i| ROWS[i].need.met(build))
}

/// The sections with a row `build` shows.
fn shown_sections(build: Build) -> Vec<Section> {
	Section::ALL.into_iter().filter(|&sec| shown_rows(build).any(|i| ROWS[i].section == sec)).collect()
}

/// Whether `query`, trimmed and in any case, is part of one of `texts`; an empty query is part of every text.
fn matches(query: &str, texts: &[&str]) -> bool {
	let query = query.trim().to_lowercase();
	query.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&query))
}

/// The indices into [`ROWS`] shown by `build` that `query` matches: the label in `s` or in English, or the section's
/// name in `s`.
fn found(query: &str, s: &Strings, build: Build) -> Vec<usize> {
	let en = Strings::new(Language::English);
	shown_rows(build)
		.filter(|&i| {
			let row = &ROWS[i];
			let texts = [s.get(row.label), en.get(row.label), s.get(row.section.title_key())].map(strip_mnemonic);
			matches(query, &texts.each_ref().map(String::as_str))
		})
		.collect()
}

/// What the rows read of the machine as their window opens.
#[derive(Clone, Copy)]
pub(super) struct Machine {
	/// (available, physical) bytes of memory.
	#[cfg(not(target_arch = "wasm32"))]
	memory: (u64, u64),
}

impl Machine {
	pub(super) fn read() -> Self {
		Self {
			#[cfg(not(target_arch = "wasm32"))]
			memory: wavedude_app_state::platform::memory_readings(),
		}
	}
}

/// What the rows' controls read and send.
struct Cx<'a> {
	app: &'a App,
	out: &'a mut Vec<Event>,
	/// What the window does as the dialog closes for a row.
	then: Option<Then>,
	#[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "the browser's RAM row reads no memory"))]
	machine: Machine,
}

impl Cx<'_> {
	fn settings(&self) -> &UserSettings {
		&self.app.settings
	}

	fn strings(&self) -> &Strings {
		&self.app.strings
	}
}

/// The dialog's state.
pub(super) struct Settings {
	section: Section,
	query: String,
	machine: Machine,
}

impl Settings {
	pub(super) fn new() -> Self {
		Self { section: Section::General, query: String::new(), machine: Machine::read() }
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App, out: &mut Vec<Event>) -> Option<Reply> {
		let s = &app.strings;
		let ctx = ui.ctx().clone();
		let popup = egui::Popup::is_any_open(&ctx);
		let focused = ctx.memory(egui::Memory::focused);
		ui.set_width(WIDTH);
		ui.label(RichText::new(s.get("SettingsTitle")).size(FONT_SIZE * 1.2).family(wavedude_view::fonts::bold()));
		let mut cx = Cx { app, out, then: None, machine: self.machine };
		let body = ui.scope(|ui| {
			let search =
				TextEdit::singleline(&mut self.query).id_salt("SettingsSearch").hint_text(s.get("ExamplesSearchHint"));
			let search = ui.add(search.desired_width(f32::INFINITY));
			if fresh {
				search.request_focus();
			}
			if self.query.trim().is_empty() {
				self.sections(ui, &mut cx, popup);
			} else {
				results(ui, &mut cx, &found(&self.query, s, BUILD));
			}
		});
		// Enter in a control is the control's, Enter and Esc in an open drop-down the drop-down's
		let in_control =
			focused.and_then(|id| ctx.read_response(id)).is_some_and(|r| body.response.rect.contains_rect(r.rect));
		ui.input_mut(|i| {
			if popup || in_control {
				i.consume_key(Modifiers::NONE, Key::Enter);
			}
			if popup {
				i.consume_key(Modifiers::NONE, Key::Escape);
			}
		});
		let closed = buttons(ui, false, &[Choice::both(s.get("DialogClose"))]);
		match cx.then {
			Some(then) => Some(Some(then)),
			None => closed.map(|_| None),
		}
	}

	/// The section list beside the shown section's rows; Up and Down walk the list.
	fn sections(&mut self, ui: &mut Ui, cx: &mut Cx<'_>, popup: bool) {
		let s = cx.app.strings;
		let shown = shown_sections(BUILD);
		let item_id = |sec: Section| Id::unique(("SettingsSection", sec.title_key()));
		let at = shown.iter().position(|&sec| sec == self.section).unwrap_or(0);
		self.section = shown[at];
		if !popup {
			let step = ui.input_mut(|i| {
				isize::from(i.consume_key(Modifiers::NONE, Key::ArrowDown))
					- isize::from(i.consume_key(Modifiers::NONE, Key::ArrowUp))
			});
			if step != 0 {
				let held = ui.memory(|m| m.has_focus(item_id(self.section)));
				self.section = shown[at.saturating_add_signed(step).min(shown.len() - 1)];
				if held {
					ui.memory_mut(|m| m.request_focus(item_id(self.section)));
				}
			}
		}
		ui.horizontal_top(|ui| {
			ui.vertical(|ui| {
				ui.set_width(SECTIONS_WIDTH);
				ui.spacing_mut().item_spacing.y = 0.;
				for &sec in &shown {
					let active = sec == self.section;
					if section_item(ui, item_id(sec), &strip_mnemonic(s.get(sec.title_key())), active).clicked() {
						self.section = sec;
					}
				}
			});
			body_area(ui, "SettingsRows", |ui| {
				for i in shown_rows(BUILD).filter(|&i| ROWS[i].section == self.section) {
					show_row(ui, cx, i);
				}
			});
		});
	}
}

/// The rows of `found` under their sections' names, or the line saying none matches.
fn results(ui: &mut Ui, cx: &mut Cx<'_>, found: &[usize]) {
	let s = cx.app.strings;
	body_area(ui, "SettingsFound", |ui| {
		if found.is_empty() {
			ui.label(RichText::new(s.get("SettingsNone")).color(Palette::of(ui.ctx().theme()).text_dim));
		}
		let mut last = None;
		for &i in found {
			let section = ROWS[i].section;
			if last != Some(section) {
				ui.label(
					RichText::new(strip_mnemonic(s.get(section.title_key()))).family(wavedude_view::fonts::bold()),
				);
				last = Some(section);
			}
			show_row(ui, cx, i);
		}
	});
}

/// The scrolled area of the rows, of one height whatever it shows.
fn body_area(ui: &mut Ui, salt: &str, content: impl FnOnce(&mut Ui)) {
	let area = ScrollArea::vertical().id_salt(salt).auto_shrink(false);
	// a scroll area's content takes its parent's layout, here the horizontal row beside the sections
	area.max_height(BODY_HEIGHT).min_scrolled_height(BODY_HEIGHT).show(ui, |ui| ui.vertical(content));
}

/// The rows labelled `labels` as this window draws them, for another window; a row's [`Then`] is dropped.
pub(super) fn rows(ui: &mut Ui, app: &App, out: &mut Vec<Event>, machine: Machine, labels: &[&str]) {
	let mut cx = Cx { app, out, then: None, machine };
	for i in labels.iter().filter_map(|&l| ROWS.iter().position(|r| r.label == l)) {
		show_row(ui, &mut cx, i);
	}
}

fn show_row(ui: &mut Ui, cx: &mut Cx<'_>, i: usize) {
	let row = &ROWS[i];
	let text = strip_mnemonic(cx.strings().get(row.label));
	(row.control)(ui, cx, Name { key: row.label, text: &text });
}

/// A section's entry in the list, `active` the one shown.
fn section_item(ui: &mut Ui, id: Id, text: &str, active: bool) -> Response {
	let palette = Palette::of(ui.ctx().theme());
	let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), SECTION_HEIGHT), Sense::hover());
	let response = ui.interact(rect, id, Sense::click());
	response.widget_info(|| WidgetInfo::selected(Role::Tab, true, active, text));
	let (fill, ink) = match (active, response.hovered()) {
		(true, _) => (palette.selection, palette.selection_text),
		(false, true) => (palette.hover_background, palette.text),
		(false, false) => (Color32::TRANSPARENT, palette.text),
	};
	let painter = ui.painter();
	painter.rect_filled(rect, 0., fill);
	if response.has_focus() {
		painter.rect_stroke(rect, 0., Stroke::new(1., palette.accent), StrokeKind::Inside);
	}
	painter.text(rect.left_center() + vec2(8., 0.), Align2::LEFT_CENTER, text, FontId::proportional(FONT_SIZE), ink);
	response
}

/// `label` in its column, `control` right of it.
fn labelled<R>(ui: &mut Ui, label: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
	ui.horizontal(|ui| {
		let column = vec2(LABEL_WIDTH, INPUT_HEIGHT);
		ui.allocate_ui_with_layout(column, Layout::left_to_right(Align::Center), |ui| {
			ui.set_min_width(LABEL_WIDTH);
			ui.label(label);
		});
		control(ui)
	})
	.inner
}

/// A drop-down row named `label` over `words`, `current` marked and shown, `other` shown when none is; the index
/// picked.
fn choose(ui: &mut Ui, id: Id, label: &str, words: &[String], current: Option<usize>, other: &str) -> Option<usize> {
	let word = |i: usize| Cow::Borrowed(words[i].as_str());
	let shown = current.map_or(other, |i| words[i].as_str());
	let choices = Choices { len: words.len(), current, shown, word: &word, swatch: None, disabled: None, actions: &[] };
	labelled(ui, label, |ui| combo(ui, id, label, &choices))
}

/// A drop-down row over the `(value, key)` words of `table`, `current` shown; the value picked.
fn lever<T: Copy + PartialEq>(ui: &mut Ui, s: &Strings, name: Name<'_>, table: &[(T, &str)], current: T) -> Option<T> {
	let words: Vec<String> = table.iter().map(|&(_, key)| strip_mnemonic(s.get(key))).collect();
	let at = table.iter().position(|&(v, _)| v == current);
	choose(ui, name.id(), name.text, &words, at, "").map(|i| table[i].0)
}

/// A check box row ticked as `on` reads the settings; `event` sends a change.
fn toggle(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>, on: fn(&UserSettings) -> bool, event: fn(bool) -> Event) {
	if let Some(on) = check_box(ui, name.id(), name.text, on(cx.settings())) {
		cx.out.push(event(on));
	}
}

fn theme(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let (s, now) = (cx.strings(), cx.settings().theme);
	let members = ThemeSetting::MEMBERS;
	let words: Vec<String> =
		members.iter().map(|t| s.enum_word(ThemeSetting::TYPE_NAME, t.name()).to_owned()).collect();
	let at = members.iter().position(|&t| t == now);
	if let Some(i) = choose(ui, name.id(), name.text, &words, at, "").filter(|&i| Some(i) != at) {
		cx.out.push(Event::SetTheme(members[i]));
	}
}

/// System first, then each language by its own name.
fn language(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let (s, code) = (cx.strings(), cx.settings().language.as_str());
	let system = strip_mnemonic(s.get("LanguageSystem"));
	let words: Vec<String> =
		std::iter::once(system).chain(Language::ALL.iter().map(|l| l.native_name().to_owned())).collect();
	let at = if code.is_empty() { Some(0) } else { Language::ALL.iter().position(|l| l.code() == code).map(|i| i + 1) };
	if let Some(i) = choose(ui, name.id(), name.text, &words, at, code).filter(|&i| Some(i) != at) {
		let code = i.checked_sub(1).map_or_else(String::new, |i| Language::ALL[i].code().to_owned());
		cx.out.push(Event::SetLanguage(code));
	}
}

/// The menu's zooms; one from `--zoom=` outside them shows as its factor.
fn zoom(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let (s, now) = (cx.strings(), cx.settings().ui_zoom);
	let words: Vec<String> = ZOOMS.iter().map(|&(_, key)| strip_mnemonic(s.get(key))).collect();
	let at = ZOOMS.iter().position(|&(z, _)| (z - now).abs() < 1e-6);
	if let Some(i) = choose(ui, name.id(), name.text, &words, at, &format!("{now}x")).filter(|&i| Some(i) != at) {
		cx.out.push(Event::SetMenuZoom(ZOOMS[i].0));
	}
}

fn title_full_path(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	toggle(ui, cx, name, |st| st.title_full_path, Event::SetTitleFullPath);
}

fn show_startup(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	toggle(ui, cx, name, |st| st.show_startup, Event::SetShowStartup);
}

/// A number box over [`UNDO_STEPS`], two percent a step.
fn undo_steps(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let now = f64::from(cx.settings().undo_steps);
	let spec = NumberSpec::logarithmic_whole(f64::from(UNDO_STEPS.0), f64::from(UNDO_STEPS.1));
	let id = name.id();
	let edited = labelled(ui, name.text, |ui| {
		let (rect, _) = ui.allocate_exact_size(vec2(NUMBER_WIDTH, INPUT_HEIGHT), Sense::hover());
		number_box(ui, rect, id, now, &spec, false, Some(name.text))
	});
	let event = |v: f64| Event::SetUndoSteps(v.round() as u32);
	edited.offer(ui.ctx(), id, event);
	edited.response.on_hover_text(cx.strings().get("TipSettingsUndoSteps"));
	if let Some(v) = edited.applied.filter(|&v| v != now) {
		cx.out.push(event(v));
	}
}

/// The memory the frames take from: what is available of the physical memory.
#[cfg(not(target_arch = "wasm32"))]
fn ram(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let (available, physical) = cx.machine.memory;
	let text = cx.strings().format("SettingsRamFormat", &[&bytes_text(available), &bytes_text(physical)]);
	labelled(ui, name.text, |ui| ui.label(text));
}

/// The RAM cache size: Auto, the detected memory, then sizes in its place.
#[cfg(target_arch = "wasm32")]
fn ram(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let s = cx.strings();
	let auto = s.format("MenuSimulationFrameMemoryAutoFormat", &[&bytes_text(detected_web_frame_memory())]);
	let words: Vec<String> =
		std::iter::once(strip_mnemonic(&auto)).chain(WEB_RAM_CACHE_SIZES.iter().map(|&b| bytes_text(b))).collect();
	let at = match cx.settings().ram_cache() {
		RamCache::Auto => Some(0),
		RamCache::Bytes(bytes) => WEB_RAM_CACHE_SIZES.iter().position(|&b| b == bytes).map(|i| i + 1),
		RamCache::Percent(_) => None,
	};
	if let Some(i) = choose(ui, name.id(), name.text, &words, at, "").filter(|&i| Some(i) != at) {
		let cache = i.checked_sub(1).map_or(RamCache::Auto, |i| RamCache::Bytes(WEB_RAM_CACHE_SIZES[i]));
		cx.out.push(Event::SetRamCache(cache));
	}
}

/// Low, Medium and High fill the four levers with their tier's; Auto with the machine's tier as read now.
fn preset(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let (s, st) = (cx.strings(), cx.settings());
	let picked = labelled(ui, name.text, |ui| {
		let mut picked = None;
		for tier in PreviewTier::ALL {
			if modal::button(ui, s.get(tier.title_key()), true).clicked() {
				picked = Some(tier);
			}
		}
		if modal::button(ui, s.get("MenuSimulationFrameMemoryAuto"), true).clicked() {
			picked = Some(st.auto_preset(machine_facts()));
		}
		picked
	});
	if let Some(tier) = picked {
		cx.out.push(Event::SetPreviewQuality(PreviewDefaults::quality_of(tier)));
		cx.out.push(Event::SetDitherBits(PreviewDefaults::dither_bits_of(tier)));
	}
}

fn resolution(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let mut q = cx.settings().preview_quality();
	if let Some(d) =
		lever(ui, cx.strings(), name, &RESOLUTIONS, q.resolution_divisor).filter(|&d| d != q.resolution_divisor)
	{
		q.resolution_divisor = d;
		cx.out.push(Event::SetPreviewQuality(q));
	}
}

fn frames(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let mut q = cx.settings().preview_quality();
	if let Some(d) = lever(ui, cx.strings(), name, &FRAME_STEPS, q.frame_divisor).filter(|&d| d != q.frame_divisor) {
		q.frame_divisor = d;
		cx.out.push(Event::SetPreviewQuality(q));
	}
}

fn samples(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let mut q = cx.settings().preview_quality();
	if let Some(format) = lever(ui, cx.strings(), name, &SAMPLES, q.format).filter(|&f| f != q.format) {
		q.format = format;
		cx.out.push(Event::SetPreviewQuality(q));
	}
}

/// Disabled unless the samples are 16-bit.
fn dither(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let (s, st) = (cx.strings(), cx.settings());
	let (half, now) = (st.preview_quality().format == SampleFormat::Half, st.preview_dither_bits);
	let picked = ui.add_enabled_ui(half, |ui| lever(ui, s, name, &DITHERS, now)).inner;
	if let Some(bits) = picked.filter(|&b| b != now) {
		cx.out.push(Event::SetDitherBits(bits));
	}
}

fn use_gpu(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	toggle(ui, cx, name, |st| st.use_gpu, Event::SetUseGpu);
}

fn auto_rerun(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	toggle(ui, cx, name, |st| st.auto_rerun, Event::SetAutoRerun);
}

fn warn_memory(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	toggle(ui, cx, name, |st| st.warn_when_ram_insufficient, Event::SetWarnWhenRamInsufficient);
}

/// The check box with Cisco's notice below it.
fn openh264(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	toggle(ui, cx, name, |st| st.use_openh264 == Some(true), Event::SetUseOpenH264);
	#[cfg(not(target_arch = "wasm32"))]
	{
		let dim = Palette::of(ui.ctx().theme()).text_dim;
		ui.add(egui::Label::new(RichText::new(wavedude_export::h264::NOTICE).color(dim)).wrap());
	}
}

/// The profiles the encoder lacks greyed, as in the export dialog.
fn h264_profile(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let (s, now) = (cx.strings(), cx.settings().h264_profile);
	let (supported, reason) = h264_profiles(cx.app);
	let reason = s.get(reason);
	let picked = labelled(ui, name.text, |ui| profile_combo(ui, name.id(), s, now, &supported, reason));
	profile_note(ui, now, &supported, reason);
	if let Some(profile) = picked.filter(|&p| p != now) {
		cx.out.push(Event::SetH264Profile(profile));
	}
}

fn archive(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	toggle(ui, cx, name, |st| st.export_archive, Event::SetExportArchive);
}

fn check_at_launch(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	toggle(ui, cx, name, |st| st.check_for_updates == Some(true), Event::SetCheckForUpdates);
}

/// Help > Check for updates; the dialog closes for the check's answer.
fn check_now(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	if modal::button(ui, name.text, true).clicked() {
		cx.then = Some(Then::CheckForUpdates { quiet: false });
	}
}

#[cfg(test)]
mod tests {
	use wavedude_app_state::settings::TOUCH_ZOOM;

	use super::*;

	const NATIVE: Build = Build { web: false, codec_built_in: false, updater: true };

	fn labels(rows: &[usize]) -> Vec<&'static str> {
		rows.iter().map(|&i| ROWS[i].label).collect()
	}

	#[test]
	fn a_query_matches_in_any_case_and_an_empty_one_everything() {
		assert!(matches("", &["Theme"]) && matches("  ", &[]));
		assert!(matches(" gPu ", &["Use the GPU"]));
		assert!(matches("lang", &["Sprache", "Language"]));
		assert!(!matches("zoom", &["Theme", "General"]));
	}

	#[test]
	fn the_zoom_keys_step_through_the_zooms_and_reset_to_the_devices() {
		let at = |ui_zoom, touch_first| UserSettings { ui_zoom, ..UserSettings::for_device(touch_first) };
		assert_eq!(zoom_step(&at(1.0, false), ZoomStep::In), Some(1.25));
		assert_eq!(zoom_step(&at(1.0, false), ZoomStep::Out), Some(0.75));
		assert_eq!(zoom_step(&at(1.1, false), ZoomStep::Out), Some(1.0), "off the table");
		assert_eq!(zoom_step(&at(3.0, false), ZoomStep::In), None);
		assert_eq!(zoom_step(&at(0.5, false), ZoomStep::Out), None);
		assert_eq!(zoom_step(&at(5.0, false), ZoomStep::Out), Some(3.0), "a --zoom= beyond the table");
		assert_eq!(zoom_step(&at(1.5, false), ZoomStep::Reset), Some(1.0));
		assert_eq!(zoom_step(&at(1.0, true), ZoomStep::Reset), Some(TOUCH_ZOOM));
		assert_eq!(zoom_step(&at(1.0, false), ZoomStep::Reset), None);
	}

	#[test]
	fn rows_follow_the_build() {
		let shown = |b: Build| labels(&shown_rows(b).collect::<Vec<_>>());
		let web = Build { web: true, codec_built_in: false, updater: false };
		for label in ["ExportUseOpenH264", "SettingsExportArchive", "SettingsCheckForUpdates", "SettingsCheckNow"] {
			assert!(!shown(web).contains(&label), "{label} in the browser");
			assert!(shown(NATIVE).contains(&label), "{label} natively");
		}
		assert!(!shown(Build { codec_built_in: true, ..NATIVE }).contains(&"ExportUseOpenH264"));
		assert!(!shown_sections(web).contains(&Section::Updates));
		assert!(!shown_sections(Build { updater: false, ..NATIVE }).contains(&Section::Updates));
		assert_eq!(shown_sections(NATIVE), Section::ALL);
	}

	#[test]
	fn the_search_reads_labels_in_both_languages_and_section_names() {
		let (en, de) = (Strings::new(Language::English), Strings::new(Language::German));
		assert_eq!(labels(&found("gpu", &en, NATIVE)), ["MenuSimulationUseGpu"]);
		assert_eq!(labels(&found("undo", &en, NATIVE)), ["SettingsUndoSteps"]);
		assert_eq!(labels(&found("undo", &de, NATIVE)), ["SettingsUndoSteps"], "English in German");
		assert_eq!(labels(&found("preview res", &en, NATIVE)), ["MenuViewResolution"], "mnemonics are no text");
		assert_eq!(labels(&found("sprache", &de, NATIVE)), ["MenuViewLanguage"]);
		assert_eq!(labels(&found("language", &de, NATIVE)), ["MenuViewLanguage"], "English in German");
		assert_eq!(labels(&found("simulation", &en, NATIVE)), ["MenuSimulationAutoRerun", "MenuSimulationWarnMemory"]);
		assert!(found("no such setting", &en, NATIVE).is_empty());
		assert_eq!(found("", &en, NATIVE).len(), ROWS.len());
	}

	#[test]
	fn every_label_is_in_every_language() {
		for language in Language::ALL {
			let keys: Vec<_> = wavedude_app_state::strings::keys(language).collect();
			for key in ROWS.iter().map(|r| r.label).chain(Section::ALL.map(Section::title_key)) {
				assert!(keys.contains(&key), "{key} in {language:?}");
			}
		}
	}
}
