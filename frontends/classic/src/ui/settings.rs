//! The Settings window (docs/ui/70, Settings; decision 0027): every user setting as a row of a section, a search over
//! the rows' labels in the current language and in English and over the sections' names. A change applies and is
//! saved at once; Close is the only button.

use std::borrow::Cow;

use eframe::egui::{
	self, Align2, Color32, FontId, Id, Key, Modifiers, Response, RichText, Role, ScrollArea, Sense, Stroke, StrokeKind,
	TextEdit, Ui, WidgetInfo, vec2,
};
use waveduderust::model::Word;
use waveduderust::quality::{PreviewDefaults, PreviewTier};
#[cfg(target_arch = "wasm32")]
use waveduderust::run::RamCache;
use waveduderust::unstable::engine::SampleFormat;
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::export::session::ProfileRow;
#[cfg(target_arch = "wasm32")]
use waveduderust_app::live::{WEB_RAM_CACHE_SIZES, detected_web_frame_memory};
use waveduderust_app::menus::MenuAction;
use waveduderust_app::number::NumberSpec;
use waveduderust_app::platform::machine_facts;
use waveduderust_app::settings::{Settings as UserSettings, Theme as ThemeSetting, UNDO_STEPS};
use waveduderust_app::settings_table::{
	BUILD, DITHERS, FRAME_STEPS, RESOLUTIONS, ROWS, SAMPLES, Section, Setting, ZOOMS, bytes_text, found, shown_rows,
	shown_sections,
};
use waveduderust_app::strings::{Language, Strings, strip_mnemonic};

use super::export::{profile_combo, profile_note};
use super::modal::{Answer, Then};
use super::number_box::number_box;
use super::properties::{Choices, check_box, combo};
use crate::kit::dialog::{self, Choice, buttons};
use crate::kit::{FONT_SIZE, INPUT_HEIGHT, Palette};

pub(super) const WIDTH: f32 = 760.;
const LABEL_WIDTH: f32 = 180.;
const SECTIONS_WIDTH: f32 = 170.;
const SECTION_HEIGHT: f32 = 26.;
const BODY_HEIGHT: f32 = 380.;
const NUMBER_WIDTH: f32 = 100.;

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
			memory: waveduderust_app::platform::memory_readings(),
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

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App, out: &mut Vec<Event>) -> Option<Answer> {
		let s = &app.strings;
		let ctx = ui.ctx().clone();
		let popup = crate::kit::menu::any_open(&ctx);
		let focused = ctx.memory(egui::Memory::focused);
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
					RichText::new(strip_mnemonic(s.get(section.title_key())))
						.family(waveduderust::unstable::view::fonts::bold()),
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
	control(row.setting)(ui, cx, Name { key: row.label, text: &text });
}

/// The control drawing `setting`.
fn control(setting: Setting) -> fn(&mut Ui, &mut Cx<'_>, Name<'_>) {
	match setting {
		Setting::Theme => theme,
		Setting::Language => language,
		Setting::Zoom => zoom,
		Setting::TitleFullPath => title_full_path,
		Setting::ShowStartup => show_startup,
		Setting::UndoSteps => undo_steps,
		Setting::Ram => ram,
		Setting::Preset => preset,
		Setting::Resolution => resolution,
		Setting::Frames => frames,
		Setting::Samples => samples,
		Setting::Dither => dither,
		Setting::UseGpu => use_gpu,
		Setting::AutoRerun => auto_rerun,
		Setting::WarnMemory => warn_memory,
		Setting::OpenH264 => openh264,
		Setting::H264Profile => h264_profile,
		Setting::Archive => archive,
		Setting::CheckAtLaunch => check_at_launch,
		Setting::CheckNow => check_now,
	}
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

/// `label` in its column, naming `names`, `control` right of it.
fn labelled<R>(ui: &mut Ui, label: &str, names: Option<Id>, control: impl FnOnce(&mut Ui) -> R) -> R {
	crate::kit::form::labelled(ui, label, LABEL_WIDTH, names, control)
}

/// A drop-down row named `label` over `words`, `current` marked and shown, `other` shown when none is; the index
/// picked.
fn choose(ui: &mut Ui, id: Id, label: &str, words: &[String], current: Option<usize>, other: &str) -> Option<usize> {
	let word = |i: usize| Cow::Borrowed(words[i].as_str());
	let shown = current.map_or(other, |i| words[i].as_str());
	let choices = Choices { len: words.len(), current, shown, word: &word, swatch: None, disabled: None, actions: &[] };
	labelled(ui, label, Some(id), |ui| combo(ui, id, label, &choices))
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
	let edited = labelled(ui, name.text, Some(id), |ui| {
		let (rect, _) = ui.allocate_exact_size(vec2(NUMBER_WIDTH, INPUT_HEIGHT), Sense::hover());
		number_box(ui, rect, id, now, &spec, false, name.text)
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
	labelled(ui, name.text, None, |ui| ui.label(text));
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
	let picked = labelled(ui, name.text, None, |ui| {
		let mut picked = None;
		for tier in PreviewTier::ALL {
			if dialog::button(ui, s.get(tier.title_key()), true).clicked() {
				picked = Some(tier);
			}
		}
		if dialog::button(ui, s.get("MenuSimulationFrameMemoryAuto"), true).clicked() {
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
		ui.add(egui::Label::new(RichText::new(waveduderust::unstable::export::h264::NOTICE).color(dim)).wrap());
	}
}

/// The profiles the encoder lacks greyed, as in the export dialog.
fn h264_profile(ui: &mut Ui, cx: &mut Cx<'_>, name: Name<'_>) {
	let (s, row) = (cx.strings(), ProfileRow::of(cx.app));
	let reason = s.get(row.reason);
	let picked = labelled(ui, name.text, Some(name.id()), |ui| {
		profile_combo(ui, name.id(), s, row.current, &row.supported, reason)
	});
	profile_note(ui, s, &row);
	if let Some(profile) = picked.filter(|&p| p != row.current) {
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
	if dialog::button(ui, name.text, true).clicked() {
		cx.then = Some(Then::Act(MenuAction::CheckForUpdates));
	}
}
