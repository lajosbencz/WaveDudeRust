//! What a window shows of the settings: title, UI zoom, and the look, through egui-mineral's runtime (theme preference
//! and locale) or on egui's context alone; touched on a change only.

use eframe::egui::{self, ThemePreference, ViewportCommand};
use egui_mineral::i18n::{Locale, locale};
use egui_mineral::runtime::Core;
use waveduderust_app::App;
use waveduderust_app::settings::Theme;
use waveduderust_app::strings::Language;

use crate::theme::system_theme;

/// The locale of `language`'s number formats and the library's messages.
fn locale(language: Language) -> Locale {
	match language {
		Language::English => locale!("en-US"),
		Language::German => locale!("de-DE"),
		Language::French => locale!("fr-FR"),
		Language::Dutch => locale!("nl-NL"),
	}
}

/// The settings as the window shows them.
pub struct Shown {
	title: String,
	zoom: f64,
	/// The Theme setting the runtime's preference was set for.
	setting: Option<Theme>,
	/// The OS scheme given to the runtime.
	system: Option<Option<egui::Theme>>,
	/// The Language setting the locale was set for.
	language: Option<String>,
	/// The look set on egui's context, without the runtime.
	look: Option<egui::Theme>,
}

impl Shown {
	/// What the window starts with: the zoom it was created at, a title, theme and locale still to set.
	pub fn new(ctx: &egui::Context, app: &App) -> Self {
		let zoom = app.settings.ui_zoom;
		ctx.set_zoom_factor(zoom as f32);
		Self { title: String::new(), zoom, setting: None, system: None, language: None, look: None }
	}

	/// Title and UI zoom follow the settings; a zoom change scales the window with it.
	pub fn follow_window(&mut self, ctx: &egui::Context, frame: &eframe::Frame, app: &App) {
		let title = app.title();
		if title != self.title {
			crate::set_title(ctx, &title);
			self.title = title;
		}
		let zoom = app.settings.ui_zoom;
		if zoom != self.zoom {
			ctx.set_zoom_factor(zoom as f32);
			crate::rezoom(frame, zoom / self.zoom, zoom);
			self.zoom = zoom;
		}
	}

	/// Title, UI zoom, the runtime's theme preference and locale follow the settings.
	pub fn follow(&mut self, core: &Core, frame: &eframe::Frame, app: &App) {
		self.follow_window(core.ctx(), frame, app);
		self.follow_theme(core, app);
		if self.language.as_ref() != Some(&app.settings.language) {
			let tag = locale(Language::resolve(&app.settings.language));
			if let Err(e) = core.i18n().set_locale(&tag) {
				tracing::warn!("the locale {tag} is not set: {e}");
			}
			self.language = Some(app.settings.language.clone());
		}
	}

	/// The runtime's theme preference follows the setting, its `System` the OS scheme.
	fn follow_theme(&mut self, core: &Core, app: &App) {
		let system = system_theme(core.ctx());
		if self.system != Some(system) {
			core.set_system_theme(system);
			self.system = Some(system);
		}
		if self.setting != Some(app.settings.theme) {
			core.set_preference(match app.settings.theme {
				Theme::Light => ThemePreference::Light,
				Theme::Dark => ThemePreference::Dark,
				Theme::System => ThemePreference::System,
			});
			self.setting = Some(app.settings.theme);
		}
	}

	/// The runtime's theme follows the setting and the OS scheme, and egui's context and the window show its look.
	pub fn follow_look(&mut self, core: &Core, app: &App) {
		self.follow_theme(core, app);
		let ctx = core.ctx();
		let look = core.theme();
		if self.look != Some(look) {
			ctx.set_theme(look);
			ctx.send_viewport_cmd(ViewportCommand::SetTheme(match look {
				egui::Theme::Dark => egui::SystemTheme::Dark,
				egui::Theme::Light => egui::SystemTheme::Light,
			}));
			self.look = Some(look);
		}
	}
}
