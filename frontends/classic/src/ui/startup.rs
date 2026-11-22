//! The welcome page (docs/ui/70, Welcome page; decision 0027): the RAM row and the presets of Settings with a line
//! naming the levers, Settings..., whether the page opens at each launch, and Open Project..., Random example and
//! Examples... at the bottom right. A change applies and is saved at once.

use eframe::egui::{self, Id, Key, Modifiers, RichText, Ui};
use waveduderust_app::App;
use waveduderust_app::menus::MenuAction;
use waveduderust_app::settings_table::levers_line;
use waveduderust_app::{Event, Guarded};

use super::modal::{Answer, Then};
use super::properties::check_box;
use super::settings::{self, Machine};
use crate::kit::Palette;
use crate::kit::dialog::{self, Choice, buttons, escape_pressed, mnemonic_pressed};

pub(super) const WIDTH: f32 = 560.;

/// An index into the examples, picked anew at each call.
fn random_example() -> usize {
	let n = waveduderust_examples::all().len();
	waveduderust::model::Project::new_random_seed().seed as usize % n
}

/// The page's state.
pub(super) struct Startup {
	machine: Machine,
}

impl Startup {
	pub(super) fn new() -> Self {
		Self { machine: Machine::read() }
	}

	pub(super) fn ui(&self, ui: &mut Ui, fresh: bool, app: &App, out: &mut Vec<Event>) -> Option<Answer> {
		let s = &app.strings;
		let dim = Palette::of(ui.ctx().theme()).text_dim;
		ui.label(RichText::new(s.get("StartupPreview")).family(waveduderust::unstable::view::fonts::bold()));
		ui.add(egui::Label::new(s.get("StartupPreviewText")).wrap());
		settings::rows(ui, app, out, self.machine, &["SettingsRam", "SettingsPreset"]);
		ui.label(RichText::new(levers_line(s, &app.settings)).color(dim));
		let settings_key = s.get("MenuFileSettings");
		let settings = dialog::button(ui, settings_key, true).clicked() || mnemonic_pressed(ui, settings_key);
		let show = app.settings.show_startup;
		if let Some(on) = check_box(ui, Id::unique("StartupShow"), s.get("StartupShowAtLaunch"), show) {
			out.push(Event::SetShowStartup(on));
		}
		// Enter and Esc in the RAM row's open drop-down are the drop-down's
		if crate::kit::menu::any_open(ui.ctx()) {
			ui.input_mut(|i| {
				i.consume_key(Modifiers::NONE, Key::Enter);
				i.consume_key(Modifiers::NONE, Key::Escape);
			});
		}
		let choices = [
			Choice::plain(s.get("MenuFileOpen")),
			Choice::plain(s.get("StartupRandomExample")),
			Choice { primary: true, ..Choice::default(s.get("MenuFileExamples")) },
		];
		let then = match buttons(ui, fresh, &choices) {
			_ if settings => Then::Act(MenuAction::Settings),
			Some(0) => Then::Act(MenuAction::Open),
			Some(1) => Then::send(Event::Guarded(Guarded::OpenExample(random_example()))),
			Some(_) => Then::Act(MenuAction::Examples),
			None if escape_pressed(ui) => return Some(None),
			None => return None,
		};
		Some(Some(then))
	}
}
