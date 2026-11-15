//! The startup page (docs/ui/70): shown at each launch unless turned off, and while a question it asks is unanswered. The preview
//! quality (three tiers that set four levers, pre-set from the settings), Cisco's `OpenH264` for MP4 export
//! (decision 0005) and the launch's update check (decision 0011); OK saves every answer and Examples... does too, then opens the Examples window.

use std::borrow::Cow;

use eframe::egui::{self, Align, Id, Layout, RichText, Ui, vec2};
use wavedude_app_state::Event;
use wavedude_app_state::strings::{Strings, strip_mnemonic};
use wavedude_core::engine::SampleFormat;
use wavedude_core::quality::{PreviewDefaults, PreviewQuality, PreviewTier};

use super::modal::{Choice, Reply, Then, buttons};
use super::properties::{Choices, check_box, combo};
#[cfg(not(target_arch = "wasm32"))]
use super::theme::Palette;
use super::theme::{FONT_SIZE, INPUT_HEIGHT};
use crate::App;
use crate::menus::{DITHERS, FRAME_STEPS, RESOLUTIONS, SAMPLES};

const WIDTH: f32 = 560.;
const LABEL_WIDTH: f32 = 180.;

/// MP4 export asks for the user's word on Cisco's `OpenH264`.
#[cfg(not(target_arch = "wasm32"))]
fn asks_codec() -> bool {
	!wavedude_export::h264::BUILT_IN
}

#[cfg(target_arch = "wasm32")]
fn asks_codec() -> bool {
	false
}

#[cfg(not(target_arch = "wasm32"))]
const ASKS_UPDATES: bool = crate::update::AVAILABLE;
#[cfg(target_arch = "wasm32")]
const ASKS_UPDATES: bool = false;

/// The page opens at this launch.
pub(super) fn due(app: &App) -> bool {
	let st = &app.settings;
	app.first_start
		|| st.show_startup
		|| (asks_codec() && st.use_openh264.is_none())
		|| (ASKS_UPDATES && st.check_for_updates.is_none())
}

/// The page's answers so far.
pub(super) struct Startup {
	quality: PreviewQuality,
	dither: u32,
	openh264: bool,
	updates: bool,
	show: bool,
}

impl Startup {
	pub(super) fn new(app: &App) -> Self {
		let st = &app.settings;
		Self {
			quality: st.preview_quality(),
			dither: st.preview_dither_bits,
			openh264: st.use_openh264 == Some(true),
			updates: st.check_for_updates.unwrap_or(true),
			show: st.show_startup,
		}
	}

	fn set(&mut self, f: impl FnOnce(&mut PreviewQuality)) {
		f(&mut self.quality);
		self.quality = self.quality.validated();
		if self.quality.format == SampleFormat::Index {
			self.dither = 0;
		}
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App, out: &mut Vec<Event>) -> Option<Reply> {
		let s = &app.strings;
		ui.set_width(WIDTH);
		ui.label(RichText::new(s.get("StartupTitle")).size(FONT_SIZE * 1.4).family(wavedude_view::fonts::bold()));
		heading(ui, s.get("StartupPreview"));
		ui.add(egui::Label::new(s.get("StartupPreviewText")).wrap());
		ui.horizontal(|ui| {
			for tier in PreviewTier::ALL {
				if super::modal::button(ui, s.get(tier.title_key()), true).clicked() {
					self.quality = PreviewDefaults::quality_of(tier);
					self.dither = PreviewDefaults::dither_bits_of(tier);
				}
			}
		});
		let q = self.quality;
		if let Some(d) = lever(ui, s, "MenuViewResolution", &RESOLUTIONS, q.resolution_divisor) {
			self.set(|q| q.resolution_divisor = d);
		}
		if let Some(d) = lever(ui, s, "MenuViewFrames", &FRAME_STEPS, q.frame_divisor) {
			self.set(|q| q.frame_divisor = d);
		}
		if let Some(format) = lever(ui, s, "MenuViewSamples", &SAMPLES, q.format) {
			self.set(|q| q.format = format);
		}
		if q.format == SampleFormat::Half
			&& let Some(bits) = lever(ui, s, "MenuViewDither", &DITHERS, self.dither)
		{
			self.dither = bits;
		}
		#[cfg(not(target_arch = "wasm32"))]
		if asks_codec() {
			heading(ui, s.get("StartupCodec"));
			ui.add(egui::Label::new(s.get("StartupCodecText")).wrap());
			let dim = Palette::of(ui.ctx().theme()).text_dim;
			ui.label(RichText::new(wavedude_export::h264::NOTICE).color(dim));
			if let Some(on) = check_box(ui, Id::unique("StartupOpenH264"), s.get("ExportUseOpenH264"), self.openh264) {
				self.openh264 = on;
			}
		}
		if ASKS_UPDATES {
			heading(ui, s.get("StartupUpdates"));
			let text = s.get("StartupCheckForUpdates");
			if let Some(on) = check_box(ui, Id::unique("StartupUpdates"), text, self.updates) {
				self.updates = on;
			}
		}
		ui.add_space(8.);
		if let Some(on) = check_box(ui, Id::unique("StartupShow"), s.get("StartupShowAtLaunch"), self.show) {
			self.show = on;
		}
		let choices = [Choice::plain(s.get("MenuFileExamples")), Choice::both(s.get("DialogOk"))];
		let examples = buttons(ui, fresh, &choices)? == 0;
		out.push(Event::SetPreviewQuality(self.quality));
		out.push(Event::SetDitherBits(self.dither));
		out.push(Event::SetShowStartup(self.show));
		if asks_codec() {
			out.push(Event::SetUseOpenH264(self.openh264));
		}
		if ASKS_UPDATES {
			out.push(Event::SetCheckForUpdates(self.updates));
		}
		let mut then = Vec::new();
		if ASKS_UPDATES && self.updates {
			then.push(Then::CheckForUpdates);
		}
		if examples {
			then.push(Then::Examples);
		}
		Some(match then.len() {
			0 => None,
			1 => then.pop(),
			_ => Some(Then::All(then)),
		})
	}
}

fn heading(ui: &mut Ui, text: &str) {
	ui.label(RichText::new(text).family(wavedude_view::fonts::bold()));
}

/// A drop-down named by `label_key` over the `(value, key)` words of `table`, `current` shown; the value picked.
fn lever<T: Copy + PartialEq>(ui: &mut Ui, s: &Strings, label_key: &str, table: &[(T, &str)], current: T) -> Option<T> {
	let word = |i: usize| -> Cow<'_, str> { Cow::Owned(strip_mnemonic(s.get(table[i].1))) };
	let label = strip_mnemonic(s.get(label_key));
	let current = table.iter().position(|&(v, _)| v == current);
	let shown = current.map(word).unwrap_or_default();
	let choices =
		Choices { len: table.len(), current, shown: &shown, word: &word, swatch: None, disabled: None, actions: &[] };
	ui.horizontal(|ui| {
		let column = vec2(LABEL_WIDTH, INPUT_HEIGHT);
		ui.allocate_ui_with_layout(column, Layout::left_to_right(Align::Center), |ui| {
			ui.set_min_width(LABEL_WIDTH);
			ui.label(label.as_str());
		});
		combo(ui, Id::unique(("Startup", label_key)), &label, &choices)
	})
	.inner
	.map(|i| table[i].0)
}
