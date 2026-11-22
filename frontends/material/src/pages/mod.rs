//! The pages over the workspace (DESIGN.md, Overlay pages): each reads the state and pushes intents.

mod about;
mod gallery;
mod settings;
mod thumbs;

use egui_mineral_material::adaptive::WindowClass;

use crate::route::Route;
use crate::shell::Intent;
use crate::{App, Mui};

/// What the pages keep between passes.
pub(crate) struct Pages {
	shown: Option<Route>,
	machine: settings::Machine,
	settings_query: String,
	/// The gallery's topic filter: 0 is All, then `Topic::ALL` in order.
	topic: usize,
	examples_query: String,
	/// The About page's system lines, read as it opens.
	system: String,
	about: about::State,
	/// The gallery cards' painted scenes.
	thumbs: thumbs::Thumbs,
}

impl Default for Pages {
	fn default() -> Self {
		Self {
			shown: None,
			machine: settings::Machine::read(),
			settings_query: String::new(),
			topic: 0,
			examples_query: String::new(),
			system: String::new(),
			about: about::State::default(),
			thumbs: thumbs::Thumbs::default(),
		}
	}
}

impl Pages {
	/// Called each pass with the page shown; entering another one reads the machine again.
	pub(crate) fn enter(&mut self, route: Route, app: &App) {
		if self.shown != Some(route) {
			self.shown = Some(route);
			self.machine = settings::Machine::read();
			if route == Route::About {
				self.system = waveduderust_app::about::system_lines(app);
			}
		}
	}
}

pub(crate) fn show(
	ui: &mut Mui<'_>,
	route: Route,
	class: WindowClass,
	app: &App,
	pages: &mut Pages,
	out: &mut Vec<Intent>,
) {
	match route {
		Route::Workspace => {}
		Route::Examples => gallery::show(ui, app, pages, out),
		Route::Settings { section } => settings::show(ui, class, app, pages, section, out),
		Route::About => about::show(ui, app, &pages.system, &mut pages.about, out),
	}
}
