//! The shell's modals over the window (docs/ui/70), drawn one at a time as the kit's dialog windows
//! ([`crate::kit::dialog`]), the secondary windows among them; the focus they took goes back after the last.

use eframe::egui::{self, Id};
use egui_mineral::runtime::Core;
use waveduderust_app::menus::MenuAction;
use waveduderust_app::modal::{Modal, Modals, Reply, Window};
#[cfg(not(target_arch = "wasm32"))]
use waveduderust_app::platform::show_in_folder;
use waveduderust_app::strings::{Strings, strip_mnemonic};
use waveduderust_app::{App, Event, SaveChoice};

use super::about::{self, About};
use super::examples::{self, Examples};
use super::export;
use super::memory_warning;
use super::properties;
use super::settings::{self, Settings};
use super::startup::{self, Startup};
use crate::kit::Palette;
use crate::kit::dialog::{Choice, Disc, ITEM_SPACING, MESSAGE_WIDTH, message, window};

/// What the window does once a modal was answered.
pub(super) enum Then {
	Reply(Reply),
	/// A menu entry, as if picked.
	Act(MenuAction),
}

impl Then {
	pub(super) fn send(event: Event) -> Self {
		Self::Reply(Reply::Send(event))
	}
}

/// A modal's answer: it closes, and the window does `Some` [`Then`].
pub(super) type Answer = Option<Then>;

/// A secondary window's state while it shows.
enum Open {
	About(About),
	Examples(Examples),
	Startup(Startup),
	Settings(Settings),
}

impl Open {
	fn new(window: Window, app: &App) -> Self {
		match window {
			Window::About => Self::About(About::new(app)),
			Window::Examples => Self::Examples(Examples::default()),
			Window::Welcome => Self::Startup(Startup::new()),
			Window::Settings => Self::Settings(Settings::new()),
		}
	}
}

fn id(modal: &Modal) -> Id {
	Id::unique(match modal {
		Modal::AskSave(_) => "AskSave",
		Modal::Error(_) => "Error",
		Modal::Crash { .. } => "Crash",
		#[cfg(not(target_arch = "wasm32"))]
		Modal::Info(_) => "Info",
		Modal::Confirm { .. } => "Confirm",
		Modal::MemoryWarning(_) => "MemoryWarning",
		Modal::SceneImage(_) => "SceneImage",
		Modal::Export(_) => "Export",
		Modal::Shortcuts(_) => "Shortcuts",
		Modal::Window(Window::About) => "About",
		Modal::Window(Window::Examples) => "Examples",
		Modal::Window(Window::Welcome) => "Startup",
		Modal::Window(Window::Settings) => "SettingsDialog",
		#[cfg(target_arch = "wasm32")]
		Modal::Download(_) => "Download",
	})
}

/// The window's title and width.
fn frame_of(modal: &Modal, s: &Strings) -> (String, f32) {
	match modal {
		Modal::MemoryWarning(_) => (s.get("Title").to_owned(), memory_warning::WIDTH),
		Modal::Export(form) => (s.get(form.title_key()).to_owned(), export::WIDTH),
		Modal::Window(Window::About) => (strip_mnemonic(s.get("MenuHelpAbout")), about::WIDTH),
		Modal::Window(Window::Examples) => (s.get("ExamplesTitle").to_owned(), examples::WIDTH),
		Modal::Window(Window::Welcome) => (s.get("StartupTitle").to_owned(), startup::WIDTH),
		Modal::Window(Window::Settings) => (s.get("SettingsTitle").to_owned(), settings::WIDTH),
		_ => (s.get("Title").to_owned(), MESSAGE_WIDTH),
	}
}

/// What the window keeps beside the shell's modals: the focus they took and the secondary window shown.
#[derive(Default)]
pub(super) struct ModalWindows {
	/// The focus before the first modal took it.
	focus: Option<Option<Id>>,
	open: Option<Open>,
}

impl ModalWindows {
	/// Draws the shown modal of `modals`; once answered it closes, events its answer sends go to `out`, and what the
	/// window does then is returned. A shown modal's default button takes the focus on its first frame; after the
	/// last, the focus goes back.
	pub(super) fn ui(&mut self, core: &Core, modals: &mut Modals, app: &App, out: &mut Vec<Event>) -> Option<Then> {
		let ctx = core.ctx();
		let fresh = modals.take_fresh();
		if fresh {
			self.open = None;
		}
		let Some(modal) = modals.shown_mut() else {
			self.give_focus_back(ctx);
			return None;
		};
		if fresh && self.focus.is_none() {
			self.focus = Some(ctx.memory(egui::Memory::focused));
		}
		if let Modal::Window(window) = modal
			&& self.open.is_none()
		{
			self.open = Some(Open::new(*window, app));
		}
		let palette = Palette::of(ctx.theme());
		let s = &app.strings;
		let (title, width) = frame_of(modal, s);
		let open = &mut self.open;
		// what the window does while the modal stays
		let mut stays = None;
		let answer = window(core, id(modal), &title, width, |ui| {
			properties::style(ui, palette);
			ui.spacing_mut().item_spacing = ITEM_SPACING;
			match modal {
				Modal::AskSave(text) => {
					let choices = [
						Choice::default(s.get("DialogSave")),
						Choice::plain(s.get("DialogDiscard")),
						Choice::cancel(s.get("DialogCancel")),
					];
					let answer = [SaveChoice::Save, SaveChoice::DontSave, SaveChoice::Cancel];
					message(ui, fresh, Disc::Question, text, &choices)
						.map(|i| Some(Then::send(Event::SaveChoice(answer[i]))))
				}
				Modal::Error(text) => {
					message(ui, fresh, Disc::Error, text, &[Choice::both(s.get("DialogOk"))]).map(|_| None)
				}
				#[cfg(not(target_arch = "wasm32"))]
				Modal::Info(text) => message(ui, fresh, Disc::Info, text, &[Choice::both(s.get("DialogOk"))]).map(|_| None),
				Modal::Confirm { text, yes, destroys } => {
					let (yes_key, no_key) = (s.get("DialogYes"), s.get("DialogNo"));
					let choices = if *destroys {
						[Choice::plain(yes_key), Choice::both(no_key)]
					} else {
						[Choice::default(yes_key), Choice::cancel(no_key)]
					};
					message(ui, fresh, Disc::Question, text, &choices)
						.map(|i| (i == 0).then(|| Then::Reply(yes.clone())))
				}
				Modal::Crash { text, copy } => {
					let (copy_key, copied_key) = if waveduderust_app::platform::HAS_PATHS {
						("CrashCopyPath", "CrashPathCopied")
					} else {
						("CrashCopyReport", "CrashReportCopied")
					};
					let mut choices = vec![Choice::plain(s.get(copy_key))];
					if waveduderust_app::platform::HAS_PATHS {
						choices.push(Choice::plain(s.get("CrashShowFolder")));
					}
					choices.push(Choice::both(s.get("DialogOk")));
					match message(ui, fresh, Disc::Error, text, &choices) {
						Some(0) => {
							ui.ctx().copy_text(copy.clone());
							tracing::info!("{}", s.get(copied_key));
							None
						}
						#[cfg(not(target_arch = "wasm32"))]
						Some(1) => {
							show_in_folder(std::path::Path::new(copy));
							None
						}
						answered => answered.map(|_| None),
					}
				}
				Modal::MemoryWarning(warning) => memory_warning::show(warning, ui, fresh, app),
				Modal::SceneImage(text) => {
					let choices = [Choice::default(s.get("SceneImageTake")), Choice::cancel(s.get("SceneImageKeep"))];
					message(ui, fresh, Disc::Question, text, &choices)
						.map(|i| Some(Then::send(Event::SceneImageOffer(i == 0))))
				}
				Modal::Export(form) => export::dialog(ui, fresh, form, app, out, &mut stays),
				// this window's key table is its menus'
				Modal::Shortcuts(_) => Some(None),
				Modal::Window(_) => match open.as_mut()? {
					Open::About(about) => about.ui(ui, fresh, app),
					Open::Examples(examples) => examples.ui(ui, fresh, app),
					Open::Startup(startup) => startup.ui(ui, fresh, app, out),
					Open::Settings(settings) => settings.ui(ui, fresh, app, out),
				},
				#[cfg(target_arch = "wasm32")]
				Modal::Download(download) => {
					let text = s.format("ExportReadyFormat", &[&download.name()]);
					let choices = [Choice::default(s.get("DialogDownload")), Choice::cancel(s.get("DialogClose"))];
					let answered = message(ui, fresh, Disc::Info, &text, &choices);
					if answered == Some(0)
						&& let Err(e) = download.start()
					{
						tracing::error!("{} could not be downloaded: {e}", download.name());
					}
					answered.map(|_| None)
				}
			}
		});
		let Some(answer) = answer else { return stays };
		modals.close();
		if !modals.is_open() {
			self.give_focus_back(ctx);
		}
		answer
	}

	/// The focus goes back to where it was before the first modal.
	fn give_focus_back(&mut self, ctx: &egui::Context) {
		if let Some(focus) = self.focus.take() {
			ctx.memory_mut(|m| match (focus, m.focused()) {
				(Some(id), _) => m.request_focus(id),
				(None, Some(now)) => m.surrender_focus(now),
				(None, None) => {}
			});
		}
	}
}
