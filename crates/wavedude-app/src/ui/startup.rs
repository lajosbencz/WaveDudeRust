//! The welcome page (docs/ui/70, Welcome page; decision 0027): the RAM row and the presets of Settings with a line
//! naming the levers, Settings... and Examples..., and whether the page opens at each launch. A change applies and
//! is saved at once.

use eframe::egui::{self, Id, Key, Modifiers, RichText, Ui};
use wavedude_app_state::Event;
use wavedude_app_state::settings::Settings as UserSettings;
use wavedude_app_state::strings::Strings;
use wavedude_core::engine::SampleFormat;

use super::modal::{self, Choice, Reply, Then, buttons, mnemonic_pressed};
use super::properties::check_box;
use super::settings::{self, Machine};
use super::theme::{FONT_SIZE, Palette};
use crate::App;

const WIDTH: f32 = 560.;

/// The lever line's words: resolution divisor and string key.
const RESOLUTIONS: [(u32, &str); 3] = [(1, "StartupResolution1"), (2, "StartupResolution2"), (4, "StartupResolution4")];

/// The lever line's words: frame divisor and string key.
const FRAME_STEPS: [(u32, &str); 3] = [(1, "StartupFrames1"), (2, "StartupFrames2"), (4, "StartupFrames4")];

/// The page opens at this launch.
pub(super) fn due(app: &App) -> bool {
	app.first_start || app.settings.show_startup
}

/// The levers as they are, e.g. "Full resolution, every frame, 8-bit samples".
fn levers_line(s: &Strings, st: &UserSettings) -> String {
	let q = st.preview_quality();
	let word =
		|table: &[(u32, &'static str)], v: u32| table.iter().find(|&&(d, _)| d == v).map(|&(_, k)| s.get(k).to_owned());
	let samples = match q.format {
		SampleFormat::Half => "StartupSamples16",
		SampleFormat::Index => "StartupSamples8",
	};
	let dither = (q.format == SampleFormat::Half && st.preview_dither_bits > 0)
		.then(|| s.format("StartupDitherFormat", &[&st.preview_dither_bits]));
	[
		word(&RESOLUTIONS, q.resolution_divisor),
		word(&FRAME_STEPS, q.frame_divisor),
		Some(s.get(samples).to_owned()),
		dither,
	]
	.into_iter()
	.flatten()
	.collect::<Vec<_>>()
	.join(", ")
}

/// The page's state.
pub(super) struct Startup {
	machine: Machine,
}

impl Startup {
	pub(super) fn new() -> Self {
		Self { machine: Machine::read() }
	}

	pub(super) fn ui(&self, ui: &mut Ui, fresh: bool, app: &App, out: &mut Vec<Event>) -> Option<Reply> {
		let s = &app.strings;
		let dim = Palette::of(ui.ctx().theme()).text_dim;
		ui.set_width(WIDTH);
		ui.label(RichText::new(s.get("StartupTitle")).size(FONT_SIZE * 1.4).family(wavedude_view::fonts::bold()));
		ui.label(RichText::new(s.get("StartupPreview")).family(wavedude_view::fonts::bold()));
		ui.add(egui::Label::new(s.get("StartupPreviewText")).wrap());
		settings::rows(ui, app, out, self.machine, &["SettingsRam", "SettingsPreset"]);
		ui.label(RichText::new(levers_line(s, &app.settings)).color(dim));
		let (settings_key, examples_key) = (s.get("MenuFileSettings"), s.get("MenuFileExamples"));
		let then = ui
			.horizontal(|ui| {
				let settings = modal::button(ui, settings_key, true).clicked() || mnemonic_pressed(ui, settings_key);
				let examples = modal::button(ui, examples_key, true).clicked() || mnemonic_pressed(ui, examples_key);
				(settings.then_some(Then::Settings)).or(examples.then_some(Then::Examples))
			})
			.inner;
		let show = app.settings.show_startup;
		if let Some(on) = check_box(ui, Id::unique("StartupShow"), s.get("StartupShowAtLaunch"), show) {
			out.push(Event::SetShowStartup(on));
		}
		// Enter and Esc in the RAM row's open drop-down are the drop-down's
		if egui::Popup::is_any_open(ui.ctx()) {
			ui.input_mut(|i| {
				i.consume_key(Modifiers::NONE, Key::Enter);
				i.consume_key(Modifiers::NONE, Key::Escape);
			});
		}
		let closed = buttons(ui, fresh, &[Choice::both(s.get("DialogClose"))]);
		then.map_or_else(|| closed.map(|_| None), |t| Some(Some(t)))
	}
}

#[cfg(test)]
mod tests {
	use wavedude_app_state::strings::Language;
	use wavedude_core::quality::PreviewTier;

	use super::*;

	fn at(tier: PreviewTier) -> UserSettings {
		let mut st = UserSettings::for_device(false);
		st.apply_preset(tier);
		st
	}

	#[test]
	fn the_line_names_the_levers() {
		let s = Strings::new(Language::English);
		assert_eq!(levers_line(&s, &at(PreviewTier::Medium)), "Full resolution, every frame, 8-bit samples");
		assert_eq!(levers_line(&s, &at(PreviewTier::Low)), "Half resolution, every second frame, 8-bit samples");
		assert_eq!(
			levers_line(&s, &at(PreviewTier::High)),
			"Full resolution, every frame, 16-bit samples, dithering to 16 bits"
		);
	}

	#[test]
	fn every_word_is_in_every_language() {
		let words = RESOLUTIONS.iter().chain(&FRAME_STEPS).map(|&(_, k)| k);
		let keys: Vec<_> = words.chain(["StartupSamples16", "StartupSamples8", "StartupDitherFormat"]).collect();
		for language in Language::ALL {
			let have: Vec<_> = wavedude_app_state::strings::keys(language).collect();
			for key in &keys {
				assert!(have.contains(key), "{key} in {language:?}");
			}
		}
	}
}
