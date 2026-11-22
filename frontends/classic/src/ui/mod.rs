//! The window over [`App`] and the shell's course ([`ShellState`]): the frame loop that ticks the state, the menu bar
//! and shortcuts whose [`MenuAction`]s the shell runs, the dock, the status bar and the modals as modal windows. What
//! differs between the desktop and the browser is in `native` and `web`, chosen at compile time.
//!
//! Threads: the UI thread; the state's workers are polled on the tick while something moves, and wake the window
//! through [`waveduderust_app::wake`] otherwise.

mod about;
mod color_picker;
mod dock;
mod edit;
mod examples;
mod export;
mod markdown;
mod memory_warning;
mod menu_bar;
mod messages;
mod modal;
mod number_box;
mod preview;
mod properties;
mod settings;
mod shortcuts;
mod startup;
mod status_bar;
#[cfg(test)]
mod tests;
mod text_box;
mod tree;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
use native as platform;
#[cfg(target_arch = "wasm32")]
use web as platform;

use std::sync::Arc;

use eframe::egui::{self, ViewportCommand};
use egui_mineral::runtime::InitError;
use waveduderust_app::App;
use waveduderust_app::document::Source;
use waveduderust_app::form::Request;
use waveduderust_app::jobs::Job;
use waveduderust_app::menus::{self, MenuAction, On};
use waveduderust_app::modal::{Modal, Window};
use waveduderust_app::platform::Instant;
#[cfg(not(target_arch = "wasm32"))]
use waveduderust_app::platform::{Opened, open};
use waveduderust_app::settings_table::welcome_due;
use waveduderust_app::shell::{Acted, Boot, Host as _, ShellState};
use waveduderust_app::status::CacheLine;
use waveduderust_app::window::Pane;
use waveduderust_app::{Event, Guarded};
use waveduderust_egui_platform::jobs::Jobs;
use waveduderust_egui_platform::shown::Shown;
use waveduderust_egui_platform::{LeaveGuard, pasted, read_clipboard};
use waveduderust_egui_preview::drawn as icons;

use crate::kit::{Classic, ClassicUi, Rt};

use self::dock::Dock;
use self::messages::MessagesPane;
use self::modal::{ModalWindows, Then};
use self::preview::Preview;
use self::properties::PropertiesPane;
use self::shortcuts::Keyboard;
use self::tree::ProjectPane;

/// The [`eframe::App`] of both targets.
pub(crate) struct Shell {
	rt: Arc<Rt>,
	app: App,
	boot: Boot,
	shown: Shown,
	dock: Dock,
	preview: Preview,
	project: ProjectPane,
	properties: PropertiesPane,
	messages: MessagesPane,
	shell: ShellState,
	jobs: Jobs,
	windows: ModalWindows,
	leave: LeaveGuard,
	/// Whether the clipboard holds objects to paste, read when the Edit menu opens.
	pastable: Option<bool>,
	cache_line: CacheLine,
}

impl Shell {
	/// # Errors
	/// When the UI runtime cannot start.
	pub(crate) fn new(cc: &eframe::CreationContext<'_>, app: App, boot: Boot) -> Result<Self, InitError> {
		let ctx = &cc.egui_ctx;
		let rt = Classic::runtime(ctx)?;
		egui_extras::install_image_loaders(ctx);
		platform::guard_shortcuts(ctx.os());
		// the launch shows the crash report or the welcome page; an update found queues behind it
		let mut shell = ShellState::new(&app, &boot);
		if !shell.modals.is_open() && welcome_due(&app) {
			shell.modals.show(Modal::Window(Window::Welcome));
		}
		Ok(Self {
			rt,
			shown: Shown::new(ctx, &app),
			dock: Dock::load(app.settings.layout.as_deref()),
			preview: Preview::new(cc.wgpu_render_state.clone()),
			project: ProjectPane::default(),
			properties: PropertiesPane::default(),
			messages: MessagesPane::default(),
			shell,
			jobs: Jobs::new(ctx),
			windows: ModalWindows::default(),
			leave: LeaveGuard::default(),
			pastable: None,
			cache_line: CacheLine::default(),
			app,
			boot,
		})
	}

	/// The state the window shows.
	#[cfg(test)]
	pub(crate) fn app(&self) -> &App {
		&self.app
	}

	fn send(&mut self, event: Event) {
		self.shell.send(&mut self.app, &self.jobs, event);
	}

	/// Does what an answered modal asks for.
	fn then(&mut self, ctx: &egui::Context, then: Then) {
		match then {
			Then::Reply(reply) => {
				if let Some(note) = self.shell.reply(&mut self.app, &self.jobs, reply) {
					tracing::info!("{}", self.app.strings.get(note));
				}
			}
			Then::Act(action) => self.act(ctx, action, On::Current, None),
		}
	}

	/// Runs a menu entry or a shortcut unless it is disabled; item actions apply to `on`. `pasted` is the clipboard's
	/// text that came with a paste.
	fn act(&mut self, ctx: &egui::Context, action: MenuAction, on: On, pasted: Option<String>) {
		if matches!(on, On::Current) && self.messages.act(ctx, action) {
			return;
		}
		let focus = menus::focuses_form(&self.app, action, on);
		let read = || pasted.or_else(|| read_clipboard(ctx, on));
		match self.shell.act(&mut self.app, &self.jobs, action, on, read) {
			Acted::Edited if focus => self.properties.focus_form(),
			Acted::Show(action) => self.show(action),
			Acted::Refused | Acted::Ran | Acted::Edited => {}
		}
	}

	/// Toggles the pane, or shows the window, of `action`.
	fn show(&mut self, action: MenuAction) {
		let window = match action {
			MenuAction::Pane(pane) => return self.dock.toggle(pane),
			MenuAction::Examples => Window::Examples,
			MenuAction::Welcome => Window::Welcome,
			MenuAction::Settings => Window::Settings,
			MenuAction::About => Window::About,
			_ => return,
		};
		self.shell.modals.show(Modal::Window(window));
	}

	/// Answers a pane's `request`.
	fn answer(&mut self, request: Request) {
		match request {
			Request::DescriptionOpen(open) => self.app.settings.part_description_expanded = open,
			Request::Dialog(dialog) => {
				if let Some(job) = Job::of_dialog(&self.app, dialog) {
					self.jobs.start(job, self.app.document_id(), self.app.strings);
				}
			}
		}
	}

	/// Shows the finished export in its folder; an error window where the system cannot.
	#[cfg(not(target_arch = "wasm32"))]
	fn reveal(&mut self) {
		let Some(path) = self.shell.take_exported() else { return };
		if let Err(e) = open(Opened::Reveal(&path)) {
			tracing::error!("{} could not be shown: {e}", path.display());
			let line = self.app.strings.format("OpenError", &[&path.display()]);
			self.shell.modals.show(Modal::Error(format!("{line}\n{e}")));
		}
	}

	/// The status bar offers to show the finished export in its folder.
	fn revealable(&self) -> bool {
		#[cfg(not(target_arch = "wasm32"))]
		return self.shell.exported().is_some();
		#[cfg(target_arch = "wasm32")]
		false
	}
}

impl eframe::App for Shell {
	fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
		let rt = Arc::clone(&self.rt);
		// before the pass, which styles its root in the runtime's look
		self.shown.follow_look(rt.core(), &self.app);
		rt.pass(ui, |ui| self.frame(ui, frame));
	}
}

impl Shell {
	/// One pass of the window; hot-patched code re-enters here.
	fn frame(&mut self, ui: &mut ClassicUi<'_>, frame: &mut eframe::Frame) {
		let ctx = ui.ctx().clone();
		// what the window and the panes ask the state for, sent after the typed text was applied
		let mut asked = Vec::new();
		if let Some(path) = self.boot.open.take_last() {
			asked.push(Event::Guarded(Guarded::Open(Source::Path(path))));
		}
		if ctx.input(|i| i.viewport().close_requested()) && self.shell.close_pressed() {
			ctx.send_viewport_cmd(ViewportCommand::CancelClose);
		}
		asked.extend(self.shell.exit_due());
		self.shell.tick(&mut self.app, &self.jobs, Instant::now());
		for done in self.jobs.take(self.app.document_id()) {
			asked.extend(self.shell.answered(&mut self.app, &self.jobs, done));
		}
		self.shell.poll(&mut self.app, &self.jobs);
		// another project drops the offer to show the export's folder
		#[cfg(not(target_arch = "wasm32"))]
		if self.shell.document_changed(&self.app) {
			self.shell.take_exported();
		}
		self.shown.follow_window(&ctx, frame, &self.app);
		// the window's keys wait while a modal shows
		let modal = self.shell.modals.is_open();
		let keyboard = (!modal).then(|| Keyboard::before_ui(&ctx));
		edit::begin_pass(&ctx);
		let (has_last, export) = (self.shell.has_last_export(&self.app), self.shell.export_status());
		let mut picked = None;
		let rt = Arc::clone(&self.rt);
		egui::Panel::top("menu_bar").frame(egui::Frame::NONE).show(ui, |ui| {
			picked = menu_bar::show(&mut rt.ui(ui), &self.app, &self.dock, &mut self.pastable, has_last, !modal);
		});
		self.messages.poll(&self.boot.log);
		self.cache_line.update(&self.app);
		let bar = status_bar::show(ui, &self.app, &self.boot.log, export, &self.cache_line, self.revealable());
		if bar.messages {
			self.dock.show(Pane::Messages);
			self.messages.focus();
		}
		#[cfg(not(target_arch = "wasm32"))]
		if bar.reveal {
			self.reveal();
		}
		let body = egui::Frame::central_panel(ui.style()).inner_margin(0);
		let strings = self.app.strings;
		let (app, preview, project, properties, messages) =
			(&mut self.app, &mut self.preview, &mut self.project, &mut self.properties, &mut self.messages);
		let log = &self.boot.log;
		let mut from_panes = None;
		let mut request = None;
		egui::CentralPanel::default().frame(body).show(ui, |ui| {
			self.dock.ui(ui, &strings, &mut |ui, pane| {
				let picked = match pane {
					Pane::Preview => preview.ui(&mut rt.ui(ui), app, &mut asked),
					Pane::Project => project.ui(ui, app, &mut asked),
					Pane::Properties => {
						request = properties.ui(ui, app, &mut asked).or(request.take());
						None
					}
					Pane::Messages => {
						messages.ui(ui, &strings, log);
						None
					}
				};
				from_panes = picked.or(from_panes);
			});
		});
		if self.project.take_activated() {
			self.properties.focus_form();
		}
		let then = self.windows.ui(self.rt.core(), &mut self.shell.modals, &self.app, &mut asked);
		// the last file dropped on the window: a project or a colour scheme opens, an image becomes the scene's
		if !modal && let Some(file) = ctx.input(|i| i.raw.dropped_files.last().cloned()) {
			self.jobs.drop_in(file, self.app.document_id(), self.app.strings);
		}
		let keys = keyboard.map_or_else(Vec::new, |k| k.after_ui(&ctx));
		let picked = picked.map(|action| (action, On::Current)).or(from_panes);
		let pasted = pasted();
		// typed text applies before anything else acts, and when its box is gone
		let acting = !asked.is_empty() || !keys.is_empty() || picked.is_some() || pasted.is_some() || request.is_some();
		let mut pending = Vec::new();
		edit::take(&ctx, acting, &mut pending);
		for event in pending.into_iter().chain(asked) {
			self.send(event);
		}
		if let Some(then) = then {
			self.then(&ctx, then);
		}
		if let Some(request) = request {
			self.answer(request);
		}
		for (action, pasted) in keys {
			self.act(&ctx, action, On::Current, pasted);
		}
		if let Some((action, on)) = picked {
			self.act(&ctx, action, on, None);
		}
		if let Some((text, on)) = pasted {
			self.act(&ctx, MenuAction::Paste, on, Some(text));
		}
		// A drag edits the arrangement on every frame: it is saved once the pointer lets go.
		if !ctx.input(|i| i.pointer.any_down())
			&& let Some(layout) = self.dock.take_change()
		{
			self.send(Event::SetLayout(layout));
		}
		if self.shell.take_exit() {
			waveduderust_egui_platform::remember_place(frame, &mut self.app.settings);
			self.app.settings.layout = self.dock.save();
			if let Err(e) = self.app.settings.save(&self.boot.store) {
				tracing::warn!("{e}");
			}
			ctx.send_viewport_cmd(ViewportCommand::Close);
		}
		self.leave.set(self.app.document.dirty || self.app.live.export_busy());
		if let Some(tick) = self.shell.next_tick(&self.app) {
			ctx.request_repaint_after(tick);
		}
	}
}
