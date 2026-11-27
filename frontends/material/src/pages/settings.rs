//! The Settings page (docs/ui/80, DESIGN.md Settings page): list-detail from Expanded on, else the section list and a
//! section as pages of their own; a search over the rows replaces the list. A change applies and is saved at once.

use eframe::egui::{Align, Layout, vec2};
use egui_mineral::widget::SizeExt as _;
use egui_mineral_material::adaptive::WindowClass;
use egui_mineral_material::prelude::{
	ButtonGroup, ButtonGroupItem, ContainerVariant, EmptyPlaceholder, ListItem, NumberField, PushButton, SearchBar,
	Select, SelectOption, Switch, icons,
};
use waveduderust::model::Word;
use waveduderust::quality::{PreviewDefaults, PreviewTier};
use waveduderust::unstable::engine::SampleFormat;
use waveduderust::unstable::export::h264::Profile;
use waveduderust_app::Event;
use waveduderust_app::export::{h264_profiles, profile_key};
#[cfg(target_arch = "wasm32")]
use waveduderust_app::live::{WEB_RAM_CACHE_SIZES, detected_web_frame_memory};
use waveduderust_app::number::NumberSpec;
use waveduderust_app::platform::machine_facts;
use waveduderust_app::settings::{Settings as UserSettings, Theme as ThemeSetting, UNDO_STEPS};
use waveduderust_app::settings_table::{
	BUILD, DITHERS, FRAME_STEPS, RESOLUTIONS, ROWS, Row, SAMPLES, Section, Setting, ZOOMS, bytes_text, found,
	shown_rows, shown_sections,
};
use waveduderust_app::strings::{Strings, strip_mnemonic};
use waveduderust_egui_platform::number::range_of;

use super::Pages;
use crate::route::Route;
use crate::shell::Intent;
use crate::{App, Mui, Salt, form};

const LIST_W: f32 = 240.;
const GAP: f32 = 24.;
const ROW_GAP: f32 = 12.;

/// What the rows read of the machine as their page opens.
#[derive(Clone, Copy)]
pub(crate) struct Machine {
	/// (available, physical) bytes of memory.
	#[cfg(not(target_arch = "wasm32"))]
	memory: (u64, u64),
}

impl Machine {
	pub(crate) fn read() -> Self {
		Self {
			#[cfg(not(target_arch = "wasm32"))]
			memory: waveduderust_app::platform::memory_readings(),
		}
	}
}

pub(super) fn show(
	ui: &mut Mui<'_>,
	class: WindowClass,
	app: &App,
	pages: &mut Pages,
	section: Option<usize>,
	out: &mut Vec<Intent>,
) {
	let s = &app.strings;
	let sections = shown_sections(BUILD);
	let beside = class >= WindowClass::Expanded;
	let search = SearchBar::new("settings-search", s.get("ExamplesSearchHint"), &mut pages.settings_query)
		.placeholder(s.get("ExamplesSearchHint"));
	let w = ui.available_width();
	ui.mineral(search.preferred_width(w));
	ui.add_space(ROW_GAP);
	if !pages.settings_query.trim().is_empty() {
		results(ui, app, pages.machine, &found(&pages.settings_query, s, BUILD), out);
		return;
	}
	let open = section.filter(|&i| i < sections.len());
	if beside {
		let open = open.unwrap_or(0);
		let width = ui.available_width();
		let rt = ui.rt();
		ui.allocate_ui_with_layout(vec2(width, 0.), Layout::left_to_right(Align::Min), |ui| {
			ui.allocate_ui_with_layout(vec2(LIST_W, 0.), Layout::top_down(Align::Min), |ui| {
				ui.set_width(LIST_W);
				list(&mut rt.ui(ui), s, &sections, Some(open), |i| {
					out.push(Intent::Go(Route::Settings { section: Some(i) }));
				});
			});
			ui.add_space(GAP);
			let w = width - LIST_W - GAP - 2. * ui.spacing().item_spacing.x;
			ui.allocate_ui_with_layout(vec2(w, 0.), Layout::top_down(Align::Min), |ui| {
				ui.set_width(w);
				rows(&mut rt.ui(ui), app, pages.machine, sections[open], out);
			});
		});
	} else if let Some(open) = open {
		form::heading(ui, &strip_mnemonic(s.get(sections[open].title_key())));
		ui.add_space(ROW_GAP);
		rows(ui, app, pages.machine, sections[open], out);
	} else {
		list(ui, s, &sections, None, |i| out.push(Intent::Push(Route::Settings { section: Some(i) })));
	}
}

/// The sections as list items, `open` selected; `pick` gets the one activated.
fn list(ui: &mut Mui<'_>, s: &Strings, sections: &[Section], open: Option<usize>, mut pick: impl FnMut(usize)) {
	for (i, section) in sections.iter().enumerate() {
		let item = ListItem::new(("settings-section", i), strip_mnemonic(s.get(section.title_key())))
			.leading(icon(*section))
			.selected(open == Some(i))
			.clickable(true);
		if ui.mineral(item).output().activated {
			pick(i);
		}
	}
}

fn icon(section: Section) -> egui_mineral_material::MaterialIcon {
	match section {
		Section::General => icons::ICON_TUNE,
		Section::Preview => icons::ICON_IMAGE,
		Section::Simulation => icons::ICON_SCIENCE,
		Section::Export => icons::ICON_UPLOAD,
		Section::Updates => icons::ICON_UPDATE,
	}
}

fn rows(ui: &mut Mui<'_>, app: &App, machine: Machine, section: Section, out: &mut Vec<Intent>) {
	for i in shown_rows(BUILD).filter(|&i| ROWS[i].section == section) {
		row(ui, app, machine, &ROWS[i], out);
	}
}

/// The rows the search found, under their sections' headings.
fn results(ui: &mut Mui<'_>, app: &App, machine: Machine, found: &[usize], out: &mut Vec<Intent>) {
	let s = &app.strings;
	if found.is_empty() {
		EmptyPlaceholder::new("settings-none", s.get("SettingsNone")).icon(icons::ICON_SEARCH_OFF).show(ui);
		return;
	}
	let mut heading = None;
	for &i in found {
		let row = &ROWS[i];
		if heading != Some(row.section) {
			heading = Some(row.section);
			form::heading(ui, &strip_mnemonic(s.get(row.section.title_key())));
			ui.add_space(ROW_GAP / 2.);
		}
		self::row(ui, app, machine, row, out);
	}
}

/// One setting's row; Language stays out (English only).
pub(crate) fn row(ui: &mut Mui<'_>, app: &App, machine: Machine, row: &Row, out: &mut Vec<Intent>) {
	if row.setting == Setting::Language {
		return;
	}
	let s = &app.strings;
	let label = strip_mnemonic(s.get(row.label));
	let mut events = Vec::new();
	let cx = Cx { app, label: &label, id: row.label };
	cx.control(ui, row.setting, machine, &mut events, out);
	out.extend(events.into_iter().map(Intent::Send));
}

/// What a row's control reads.
struct Cx<'a> {
	app: &'a App,
	label: &'a str,
	id: &'static str,
}

impl Cx<'_> {
	fn settings(&self) -> &UserSettings {
		&self.app.settings
	}

	fn s(&self) -> &Strings {
		&self.app.strings
	}

	fn control(
		&self,
		ui: &mut Mui<'_>,
		setting: Setting,
		machine: Machine,
		ev: &mut Vec<Event>,
		out: &mut Vec<Intent>,
	) {
		let st = self.settings();
		match setting {
			Setting::Language => {}
			Setting::Theme => {
				let members = ThemeSetting::MEMBERS;
				let words = members.iter().map(|t| self.s().enum_word(ThemeSetting::TYPE_NAME, t.name()).to_owned());
				let at = members.iter().position(|&t| t == st.theme);
				if let Some(i) = self.select(ui, words.collect(), at, None) {
					ev.push(Event::SetTheme(members[i]));
				}
			}
			Setting::Zoom => {
				let at = ZOOMS.iter().position(|&(z, _)| (z - st.ui_zoom).abs() < 1e-6);
				let other = at.is_none().then(|| format!("{}x", st.ui_zoom));
				if let Some(i) = self.select(ui, self.words(&ZOOMS), at, other) {
					ev.push(Event::SetMenuZoom(ZOOMS[i].0));
				}
			}
			Setting::TitleFullPath => self.switch(ui, st.title_full_path, Event::SetTitleFullPath, ev),
			Setting::ShowStartup => self.switch(ui, st.show_startup, Event::SetShowStartup, ev),
			Setting::UndoSteps => self.undo_steps(ui, ev),
			Setting::Ram => self.ram(ui, machine, ev),
			Setting::Preset => self.preset(ui, ev),
			Setting::Resolution => {
				let mut q = st.preview_quality();
				if let Some(d) = self.lever(ui, &RESOLUTIONS, q.resolution_divisor, false) {
					q.resolution_divisor = d;
					ev.push(Event::SetPreviewQuality(q));
				}
			}
			Setting::Frames => {
				let mut q = st.preview_quality();
				if let Some(d) = self.lever(ui, &FRAME_STEPS, q.frame_divisor, false) {
					q.frame_divisor = d;
					ev.push(Event::SetPreviewQuality(q));
				}
			}
			Setting::Samples => {
				let mut q = st.preview_quality();
				if let Some(f) = self.lever(ui, &SAMPLES, q.format, false) {
					q.format = f;
					ev.push(Event::SetPreviewQuality(q));
				}
			}
			Setting::Dither => {
				let half = st.preview_quality().format == SampleFormat::Half;
				if let Some(bits) = self.lever(ui, &DITHERS, st.preview_dither_bits, !half) {
					ev.push(Event::SetDitherBits(bits));
				}
			}
			Setting::UseGpu => self.switch(ui, st.use_gpu, Event::SetUseGpu, ev),
			Setting::LegacyCpuEngine => self.switch(ui, st.legacy_cpu_engine, Event::SetLegacyCpuEngine, ev),
			Setting::AutoRerun => self.switch(ui, st.auto_rerun, Event::SetAutoRerun, ev),
			Setting::WarnMemory => {
				self.switch(ui, st.warn_when_ram_insufficient, Event::SetWarnWhenRamInsufficient, ev);
			}
			Setting::OpenH264 => {
				let on = st.use_openh264 == Some(true);
				#[cfg(not(target_arch = "wasm32"))]
				let notice = Some(waveduderust::unstable::export::h264::NOTICE);
				#[cfg(target_arch = "wasm32")]
				let notice = None;
				if let Some(on) = form::row(ui, self.id, self.label, notice, |ui, _| self.toggle(ui, on)) {
					ev.push(Event::SetUseOpenH264(on));
				}
			}
			Setting::H264Profile => self.h264_profile(ui, ev),
			Setting::Archive => self.switch(ui, st.export_archive, Event::SetExportArchive, ev),
			Setting::CheckAtLaunch => {
				self.switch(ui, st.check_for_updates == Some(true), Event::SetCheckForUpdates, ev);
			}
			Setting::CheckNow => {
				let runs = crate::commands::runs(
					self.app,
					crate::commands::Cmd::Act(waveduderust_app::menus::MenuAction::CheckForUpdates),
				);
				let clicked = form::row(ui, self.id, self.label, None, |ui, _| {
					ui.mineral(PushButton::new(self.id, self.label).disabled(!runs)).clicked()
				});
				if clicked {
					out.push(Intent::Act(waveduderust_app::menus::MenuAction::CheckForUpdates));
				}
			}
		}
	}

	fn words<T>(&self, table: &[(T, &str)]) -> Vec<String> {
		table.iter().map(|&(_, key)| strip_mnemonic(self.s().get(key))).collect()
	}

	/// The memory the frames take from: what is available of the physical memory.
	#[cfg(not(target_arch = "wasm32"))]
	fn ram(&self, ui: &mut Mui<'_>, machine: Machine, _: &mut Vec<Event>) {
		let (available, physical) = machine.memory;
		let text = self.s().format("SettingsRamFormat", &[&bytes_text(available), &bytes_text(physical)]);
		form::value(ui, self.id, self.label, &text);
	}

	/// The RAM cache size: Auto, the detected memory, then sizes in its place.
	#[cfg(target_arch = "wasm32")]
	fn ram(&self, ui: &mut Mui<'_>, _: Machine, ev: &mut Vec<Event>) {
		use waveduderust::run::RamCache;
		let auto = self.s().format("MenuSimulationFrameMemoryAutoFormat", &[&bytes_text(detected_web_frame_memory())]);
		let words = std::iter::once(strip_mnemonic(&auto)).chain(WEB_RAM_CACHE_SIZES.iter().map(|&b| bytes_text(b)));
		let at = match self.app.settings.ram_cache() {
			RamCache::Auto => Some(0),
			RamCache::Bytes(bytes) => WEB_RAM_CACHE_SIZES.iter().position(|&b| b == bytes).map(|i| i + 1),
			RamCache::Percent(_) => None,
		};
		if let Some(i) = self.select(ui, words.collect(), at, None).filter(|&i| Some(i) != at) {
			let cache = i.checked_sub(1).map_or(RamCache::Auto, |i| RamCache::Bytes(WEB_RAM_CACHE_SIZES[i]));
			ev.push(Event::SetRamCache(cache));
		}
	}

	fn select(
		&self,
		ui: &mut Mui<'_>,
		words: Vec<String>,
		at: Option<usize>,
		supporting: Option<String>,
	) -> Option<usize> {
		let options = words.into_iter().map(SelectOption::new).collect();
		self.select_in(ui, options, at, supporting, false)
	}

	/// A drop-down over `options`, `at` selected; the index newly picked.
	fn select_in(
		&self,
		ui: &mut Mui<'_>,
		options: Vec<SelectOption>,
		at: Option<usize>,
		supporting: Option<String>,
		disabled: bool,
	) -> Option<usize> {
		form::row(ui, self.id, self.label, supporting.as_deref(), |ui, w| {
			let mut picked = at;
			let select = Select::optional(self.id, self.label, &mut picked)
				.items(options.into_iter().enumerate())
				.label_visible(false);
			ui.mineral(select.disabled(disabled).preferred_width(w));
			picked.filter(|_| picked != at)
		})
	}

	/// A drop-down over the `(value, key)` words of `table`; the value newly picked.
	fn lever<T: Copy + PartialEq>(&self, ui: &mut Mui<'_>, table: &[(T, &str)], now: T, disabled: bool) -> Option<T> {
		let at = table.iter().position(|&(v, _)| v == now);
		let options = self.words(table).into_iter().map(SelectOption::new).collect();
		self.select_in(ui, options, at, None, disabled).map(|i| table[i].0)
	}

	/// The row's switch, `Some` with the new value when flipped.
	fn toggle(&self, ui: &mut Mui<'_>, on: bool) -> Option<bool> {
		let mut now = on;
		ui.mineral(Switch::new(self.id, self.label, &mut now).label_visible(false));
		(now != on).then_some(now)
	}

	fn switch(&self, ui: &mut Mui<'_>, on: bool, event: fn(bool) -> Event, ev: &mut Vec<Event>) {
		if let Some(on) = form::row(ui, self.id, self.label, None, |ui, _| self.toggle(ui, on)) {
			ev.push(event(on));
		}
	}

	/// A whole number over [`UNDO_STEPS`], two percent a step, set on Enter or focus loss.
	fn undo_steps(&self, ui: &mut Mui<'_>, ev: &mut Vec<Event>) {
		let now = f64::from(self.settings().undo_steps);
		let supporting = self.s().get("TipSettingsUndoSteps");
		let value = form::row(ui, self.id, self.label, Some(supporting), |ui, w| {
			let mut value = now;
			let spec = NumberSpec::logarithmic_whole(f64::from(UNDO_STEPS.0), f64::from(UNDO_STEPS.1));
			let field = NumberField::new(self.id, self.label, range_of(&spec), &mut value)
				.commit_on_submit(true)
				.snap_typed(false)
				.format(|v| format!("{v:.0}"));
			ui.mineral(field.preferred_width(w));
			value
		});
		if value.round() != now {
			ev.push(Event::SetUndoSteps(value.round() as u32));
		}
	}

	/// Low, Medium and High fill the four levers with their tier's; Auto with the machine's tier as read now.
	fn preset(&self, ui: &mut Mui<'_>, ev: &mut Vec<Event>) {
		let s = self.s();
		let picked = form::stacked(ui, self.label, None, |ui| {
			let group = PreviewTier::ALL
				.iter()
				.fold(ButtonGroup::new(self.id, self.label).button_variant(ContainerVariant::Tonal), |g, t| {
					g.item(Some(Salt(*t)), ButtonGroupItem::new(s.get(t.title_key())))
				})
				.item(None, ButtonGroupItem::new(strip_mnemonic(s.get("MenuSimulationFrameMemoryAuto"))));
			group.show(ui).clicked
		});
		// Auto: the machine's tier as read now.
		let tier = picked.map(|t| t.map_or_else(|| self.settings().auto_preset(machine_facts()), |t| t.0));
		if let Some(tier) = tier {
			ev.push(Event::SetPreviewQuality(PreviewDefaults::quality_of(tier)));
			ev.push(Event::SetDitherBits(PreviewDefaults::dither_bits_of(tier)));
		}
	}

	/// A profile the encoder lacks is greyed, and the supporting text says why.
	fn h264_profile(&self, ui: &mut Mui<'_>, ev: &mut Vec<Event>) {
		let s = self.s();
		let now = self.settings().h264_profile;
		let (supported, reason) = h264_profiles(self.app);
		let options = Profile::ALL
			.iter()
			.map(|p| SelectOption::new(s.get(profile_key(*p))).disabled(!supported.contains(p)))
			.collect();
		let at = Profile::ALL.iter().position(|&p| p == now);
		let lacking = Profile::ALL.iter().any(|p| !supported.contains(p)).then(|| s.get(reason).to_owned());
		if let Some(i) = self.select_in(ui, options, at, lacking, false) {
			ev.push(Event::SetH264Profile(Profile::ALL[i]));
		}
	}
}
