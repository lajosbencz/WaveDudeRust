//! The window over [`App`]: the frame loop that ticks the state, the title, UI zoom and look that follow the
//! settings, the menu bar and the shortcuts with the one dispatcher of their [`MenuAction`]s, the dock and the status
//! bar, the modals that the state's responses and notices ask for, the close box that asks about unsaved edits
//! first, and the renderer's GPU device that the engines share (natively and in the browser).
//! What differs between the two is in `native` and `web`, chosen at compile time.
//!
//! Threads: the UI thread; the state's workers are polled on the tick while something moves, and wake the window
//! through [`wavedude_app_state::wake`] otherwise.

mod about;
mod color_picker;
mod dock;
mod edit;
mod examples;
#[cfg(not(target_arch = "wasm32"))]
mod export;
mod files;
mod gradient_editor;
mod icons;
mod manual;
mod markdown;
mod memory_warning;
mod menu_bar;
mod menu_keys;
mod messages;
mod modal;
mod number_box;
mod preview;
mod properties;
mod shortcuts;
mod startup;
mod status_bar;
mod text_box;
pub mod theme;
mod tree;
#[cfg(not(target_arch = "wasm32"))]
mod updates;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
use native as platform;
#[cfg(target_arch = "wasm32")]
use web as platform;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use eframe::egui::{self, ViewportCommand};
#[cfg(not(target_arch = "wasm32"))]
use eframe::{egui_wgpu, wgpu};
use wavedude_app_state::clipboard::PasteTarget;
use wavedude_app_state::commands::Command;
use wavedude_app_state::document::Source;
use wavedude_app_state::live::Notice;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::{Duration, Instant, Store, take_crash_report};
use wavedude_app_state::selection::Row;
use wavedude_app_state::{Event, Guarded, Response};
#[cfg(not(target_arch = "wasm32"))]
use wavedude_core::engine::GpuHandle;
#[cfg(not(target_arch = "wasm32"))]
use wavedude_core::engine::gpu::GpuContext;
use wavedude_core::model::{ItemKind, ItemRef};
use wavedude_core::quality::PreviewQuality;

use self::about::About;
use self::dock::Dock;
use self::examples::Examples;
#[cfg(not(target_arch = "wasm32"))]
use self::export::{After, Exports, Step};
use self::manual::Manual;
use self::memory_warning::MemoryWarning;
use self::messages::MessagesPane;
use self::modal::{Modal, Modals, Then};
use self::preview::Preview;
use self::properties::{PropertiesPane, Request};
use self::shortcuts::Keyboard;
use self::startup::Startup;
use self::tree::ProjectPane;
#[cfg(not(target_arch = "wasm32"))]
use self::updates::Updates;
use crate::App;
use crate::menus::{MenuAction, On, moves_of};
use crate::window::Pane;
use crate::window::TICK;

/// The page's canvas the window draws on.
#[cfg(target_arch = "wasm32")]
pub const CANVAS: &str = "app";

/// What the window starts with beside the state.
pub struct Boot {
	/// Where the settings are saved at exit.
	pub store: Store,
	/// The log's lines, the status bar's among them.
	pub log: LogRing,
	/// The renderer's context, when the engines run on it.
	#[cfg(not(target_arch = "wasm32"))]
	pub gpu: Option<GpuHandle>,
	/// Files the command line and the OS ask to open.
	#[cfg(not(target_arch = "wasm32"))]
	pub open: crate::open_requests::OpenRequests,
	/// The downloaded update `main` installs after a restart.
	#[cfg(not(target_arch = "wasm32"))]
	pub update: crate::update::Pending,
}

/// What the window shows of the settings, so the platform is touched on a change only.
struct Shown {
	title: String,
	zoom: f64,
	theme: Option<egui::Theme>,
}

/// The [`eframe::App`] of both targets.
pub struct Shell {
	app: App,
	boot: Boot,
	shown: Shown,
	dock: Dock,
	preview: Preview,
	project: ProjectPane,
	properties: PropertiesPane,
	messages: MessagesPane,
	modals: Modals,
	manual: Option<Manual>,
	files: files::Files,
	#[cfg(not(target_arch = "wasm32"))]
	exports: Exports,
	#[cfg(not(target_arch = "wasm32"))]
	updates: Updates,
	/// The close box was pressed: Exit is asked for once no modal shows.
	close_asked: bool,
	/// Exit was answered: the window saves its place and closes at the end of the frame.
	exiting: bool,
	/// The window is closing; its close request goes through.
	closing: bool,
	leave: platform::LeaveGuard,
	/// Whether the clipboard holds objects to paste, read when the Edit menu opens.
	pastable: Option<bool>,
	cache_line: status_bar::CacheLine,
}

impl Shell {
	pub fn new(cc: &eframe::CreationContext<'_>, app: App, boot: Boot) -> Self {
		theme::install(&cc.egui_ctx);
		cc.egui_ctx.add_plugin(menu_keys::MenuKeys::default());
		egui_material_icons::initialize(&cc.egui_ctx);
		egui_extras::install_image_loaders(&cc.egui_ctx);
		#[cfg(target_arch = "wasm32")]
		web::guard_shortcuts(cc.egui_ctx.os());
		let zoom = app.settings.ui_zoom;
		cc.egui_ctx.set_zoom_factor(zoom as f32);
		let shown = Shown { title: app.title(), zoom, theme: None };
		let dock = Dock::load(app.settings.layout.as_deref());
		#[cfg(not(target_arch = "wasm32"))]
		let preview = Preview::new(cc.wgpu_render_state.clone(), boot.gpu.clone());
		// the engines run in the run worker, on a device of their own
		#[cfg(target_arch = "wasm32")]
		let preview = Preview::new(cc.wgpu_render_state.clone(), None);
		let project = ProjectPane::default();
		#[cfg(not(target_arch = "wasm32"))]
		let updates = Updates::new(boot.update.clone());
		// the launch asks one thing at most: the crash report, the startup page or (quietly) for an update
		let mut modals = Modals::default();
		if let Some(report) = take_crash_report(&boot.store) {
			modals.show(Modal::crash(&app.strings, report));
		} else if startup::due(&app) {
			modals.show(Modal::Startup(Startup::new(&app)));
		} else if app.settings.check_for_updates == Some(true) {
			#[cfg(not(target_arch = "wasm32"))]
			updates.check(&cc.egui_ctx, true);
		}
		Self {
			app,
			boot,
			shown,
			dock,
			preview,
			project,
			properties: PropertiesPane::default(),
			messages: MessagesPane::default(),
			modals,
			manual: None,
			files: files::Files::default(),
			#[cfg(not(target_arch = "wasm32"))]
			exports: Exports::default(),
			#[cfg(not(target_arch = "wasm32"))]
			updates,
			close_asked: false,
			exiting: false,
			closing: false,
			leave: platform::LeaveGuard::default(),
			pastable: None,
			cache_line: status_bar::CacheLine::default(),
		}
	}

	/// The state the window shows.
	pub fn app(&self) -> &App {
		&self.app
	}

	/// Sends `event` to the state and acts on its response.
	fn send(&mut self, ctx: &egui::Context, event: Event) {
		match self.app.handle(event, Instant::now()) {
			#[cfg(not(target_arch = "wasm32"))]
			Response::Exit => self.quit(After::Exit),
			#[cfg(target_arch = "wasm32")]
			Response::Exit => self.exiting = true,
			Response::Clipboard { text, then } => {
				ctx.copy_text(text);
				if let Some(command) = then {
					self.send(ctx, Event::Command(command));
				}
			}
			Response::AskSave { name } => self.modals.show(Modal::ask_save(&self.app.strings, &name)),
			Response::AskSavePath { proposed } => self.files.save_as(ctx, &self.app, proposed),
			Response::MemoryWarning => self.ask_memory_warning(),
			Response::Error(line) => self.modals.show(Modal::Error(line)),
			#[cfg(not(target_arch = "wasm32"))]
			Response::Restart => self.quit(After::Restart),
			#[cfg(target_arch = "wasm32")]
			Response::Restart => {}
			Response::None => {}
		}
	}

	/// Does what an answered modal asks for.
	fn then(&mut self, ctx: &egui::Context, then: Then) {
		match then {
			Then::Send(event) => self.send(ctx, event),
			Then::All(all) => all.into_iter().for_each(|then| self.then(ctx, then)),
			Then::Examples => self.modals.show(Modal::Examples(Examples::default())),
			#[cfg(not(target_arch = "wasm32"))]
			Then::Download(update) => self.updates.download(ctx, &self.app.strings, *update),
			#[cfg(not(target_arch = "wasm32"))]
			Then::CheckForUpdates => self.updates.check(ctx, true),
			#[cfg(not(target_arch = "wasm32"))]
			Then::Export(step) => self.exports.step(ctx, &mut self.app, &mut self.modals, step),
			#[cfg(target_arch = "wasm32")]
			Then::LocateSceneImage => self.files.locate_scene_image(ctx, &self.app),
		}
	}

	/// Ends the program, `after` saying whether it restarts into the downloaded update. A running export is
	/// stopped first on the user's word, and the end waits for it.
	#[cfg(not(target_arch = "wasm32"))]
	fn quit(&mut self, after: After) {
		if self.exports.exporting() {
			let text = self.app.strings.get("ExitStopsExport").to_owned();
			self.modals.show(Modal::Confirm { text, yes: Then::Export(Step::Stop(after)), destroys: false });
		} else {
			self.leave(after);
		}
	}

	#[cfg(not(target_arch = "wasm32"))]
	fn leave(&mut self, after: After) {
		if after == After::Restart {
			self.updates.restart();
		}
		self.exiting = true;
	}

	fn ask_memory_warning(&mut self) {
		if let Some(warning) = MemoryWarning::of(&self.app) {
			self.modals.show(Modal::MemoryWarning(warning));
		}
	}

	/// Acts on the live run's notices that the window shows.
	fn notices(&mut self, ctx: &egui::Context) {
		for notice in self.app.live.take_notices() {
			match notice {
				Notice::AskMemoryWarning => self.ask_memory_warning(),
				Notice::WarningDropped => self.modals.drop_memory_warning(ctx),
				Notice::RunFailed { message, no_image } => {
					// the browser asks to upload the image instead
					#[cfg(target_arch = "wasm32")]
					if no_image && self.files.ask_scene_image(&self.app, &mut self.modals) {
						continue;
					}
					let s = &self.app.strings;
					let detail = if no_image { s.get("RunErrorNoImage") } else { &message };
					self.modals.show(Modal::Error(format!("{}\n{detail}", s.get("RunError"))));
				}
				#[cfg(not(target_arch = "wasm32"))]
				Notice::ExportReady => self.exports.ready(ctx, &mut self.app, &mut self.modals),
				#[cfg(not(target_arch = "wasm32"))]
				Notice::ExportStopped => self.exports.stopped(),
				#[cfg(target_arch = "wasm32")]
				Notice::ExportReady | Notice::ExportStopped => {}
				Notice::ShowFrame(_) | Notice::Info(_) | Notice::Error(_) => {}
			}
		}
	}

	/// Runs a menu entry or a shortcut; item actions apply to `on`. `pasted` is the clipboard's text that came with
	/// a paste.
	fn act(&mut self, ctx: &egui::Context, action: MenuAction, on: On, pasted: Option<String>) {
		if matches!(on, On::Current) && self.messages.act(ctx, action) {
			return;
		}
		if action == MenuAction::Copy
			&& let On::Row(Some(row)) = on
			&& !matches!(row, Row::Item(i) if self.app.selection.contains(i))
		{
			self.app.selection.select(row);
		}
		let app = &self.app;
		let st = &app.settings;
		let target = match on {
			On::Current => app.selection.current_item(),
			On::Row(Some(Row::Item(i))) => Some(i),
			On::Row(_) => None,
		};
		let quality = |f: fn(&mut PreviewQuality, u32), v: u32| {
			let mut q = st.preview_quality();
			f(&mut q, v);
			Event::SetPreviewQuality(q)
		};
		let on_target = |f: fn(ItemRef) -> Command| target.map(|t| Event::Command(f(t)));
		// an added or duplicated item's form takes the focus
		if matches!(action, MenuAction::AddSource | MenuAction::AddPart(_) | MenuAction::AddProbe(_))
			|| (action == MenuAction::Duplicate && target.is_some())
		{
			self.properties.focus_form();
		}
		let event = match action {
			MenuAction::New => Some(Event::Guarded(Guarded::New)),
			MenuAction::Recent(i) => {
				st.recent_files.get(i).map(|p| Event::Guarded(Guarded::Open(Source::Path(p.clone()))))
			}
			MenuAction::Save => Some(Event::Save),
			MenuAction::Close => Some(Event::Guarded(Guarded::Close)),
			MenuAction::Exit => Some(Event::Guarded(Guarded::Exit)),
			MenuAction::Undo => Some(Event::Undo),
			MenuAction::Redo => Some(Event::Redo),
			MenuAction::Cut => target.map(|target| Event::Cut { target }),
			MenuAction::Copy => Some(Event::Copy),
			MenuAction::Paste => pasted.or_else(|| platform::read_clipboard(ctx)).map(|text| {
				let row = if let On::Row(row) = on { row } else { None };
				let target = PasteTarget { current: row.or(app.selection.current()), before: false };
				Event::Paste { text, target, place_at: None }
			}),
			MenuAction::Delete => on_target(|target| Command::Remove { target }),
			MenuAction::SelectAll => {
				let project = self.app.document.project.clone();
				self.app.selection.select_all(&project);
				None
			}
			MenuAction::Pane(pane) => {
				self.dock.toggle(pane);
				None
			}
			MenuAction::Theme(theme) => Some(Event::SetTheme(theme)),
			MenuAction::Zoom(z) => Some(Event::SetMenuZoom(z)),
			MenuAction::Language(l) => Some(Event::SetLanguage(l.map_or_else(String::new, |l| l.code().to_owned()))),
			MenuAction::Resolution(d) => Some(quality(|q, d| q.resolution_divisor = d, d)),
			MenuAction::Frames(d) => Some(quality(|q, d| q.frame_divisor = d, d)),
			MenuAction::Samples(format) => {
				let mut q = st.preview_quality();
				q.format = format;
				Some(Event::SetPreviewQuality(q))
			}
			MenuAction::Dither(bits) => Some(Event::SetDitherBits(bits)),
			MenuAction::Run => Some(Event::Run),
			MenuAction::PauseRun => Some(Event::TogglePauseRun),
			#[cfg(not(target_arch = "wasm32"))]
			MenuAction::Stop => (!self.exports.cancel(&mut self.app)).then_some(Event::StopRun),
			#[cfg(target_arch = "wasm32")]
			MenuAction::Stop => Some(Event::StopRun),
			MenuAction::DiscardFrames => {
				let text = app.strings.get("DiscardFramesQuestion").to_owned();
				self.modals.show(Modal::Confirm { text, yes: Then::Send(Event::DiscardFrames), destroys: true });
				None
			}
			MenuAction::Play => Some(if app.live.playback.is_playing() { Event::PausePlayback } else { Event::Play }),
			MenuAction::UseGpu => Some(Event::SetUseGpu(!st.use_gpu)),
			MenuAction::AutoRerun => Some(Event::SetAutoRerun(!st.auto_rerun)),
			MenuAction::WarnMemory => Some(Event::SetWarnWhenRamInsufficient(!st.warn_when_ram_insufficient)),
			MenuAction::FrameMemory(pct) => Some(Event::SetFrameMemoryPercent(pct)),
			MenuAction::CheckAtLaunch => Some(Event::SetCheckForUpdates(st.check_for_updates != Some(true))),
			MenuAction::AddSource => Some(Event::Command(Command::AddSource { at: None })),
			MenuAction::AddPart(kind) => Some(Event::Command(Command::AddPart { kind, at: None })),
			MenuAction::AddProbe(detector) => Some(Event::Command(Command::AddProbe { at: None, detector })),
			MenuAction::ToggleSelected => {
				if let Some(target) = target {
					self.app.selection.toggle(target);
				}
				None
			}
			MenuAction::ToggleEnabled => on_target(|target| Command::ToggleEnabled { target }),
			MenuAction::ToggleUserScheme => {
				target.filter(|t| t.kind == ItemKind::Scheme).map(|t| Event::ToggleUserScheme(t.index))
			}
			MenuAction::Duplicate => on_target(|target| Command::Duplicate { target }),
			MenuAction::MoveUp | MenuAction::MoveDown => {
				if let Some(target) = target {
					for (index, to) in moves_of(app, target, action == MenuAction::MoveUp) {
						let target = ItemRef::new(target.kind, index);
						self.send(ctx, Event::Command(Command::Reorder { target, to }));
					}
				}
				None
			}
			MenuAction::Open => {
				self.files.open_project(ctx, &self.app);
				None
			}
			MenuAction::SaveAs => {
				self.files.save_as(ctx, &self.app, None);
				None
			}
			MenuAction::Examples => {
				self.modals.show(Modal::Examples(Examples::default()));
				None
			}
			MenuAction::Welcome => {
				self.modals.show(Modal::Startup(Startup::new(app)));
				None
			}
			MenuAction::About => {
				self.modals.show(Modal::About(About::new(app)));
				None
			}
			MenuAction::Manual => {
				self.manual.get_or_insert_default().raise();
				None
			}
			#[cfg(not(target_arch = "wasm32"))]
			MenuAction::CheckForUpdates => {
				self.updates.check(ctx, false);
				None
			}
			#[cfg(target_arch = "wasm32")]
			MenuAction::CheckForUpdates => None,
			#[cfg(not(target_arch = "wasm32"))]
			MenuAction::Export(choice) => {
				if let Some(dialog) = Exports::dialog(app, choice) {
					self.modals.show(dialog);
				}
				None
			}
			#[cfg(not(target_arch = "wasm32"))]
			MenuAction::ExportLast => {
				self.exports.export_last(ctx, &mut self.app, &mut self.modals);
				None
			}
			#[cfg(not(target_arch = "wasm32"))]
			MenuAction::ExportProbeCsv => {
				if let Some(probe) = target.filter(|t| t.kind == ItemKind::Probe) {
					self.exports.probe_csv(ctx, app, probe.index);
				}
				None
			}
			// the browser has no exports
			#[cfg(target_arch = "wasm32")]
			MenuAction::Export(_) | MenuAction::ExportLast | MenuAction::ExportProbeCsv => None,
		};
		if let Some(event) = event {
			self.send(ctx, event);
		}
	}

	/// Answers a pane's `request`.
	fn answer(&mut self, ctx: &egui::Context, request: Request) {
		match request {
			Request::DescriptionOpen(open) => self.app.settings.part_description_expanded = open,
			Request::Dialog(dialog) => self.files.ask(ctx, &self.app, dialog),
		}
	}

	/// Title, UI zoom and look follow the settings. A zoom change scales the window with it.
	fn follow_settings(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
		let title = self.app.title();
		if title != self.shown.title {
			platform::set_title(ctx, &title);
			self.shown.title = title;
		}
		let zoom = self.app.settings.ui_zoom;
		if zoom != self.shown.zoom {
			ctx.set_zoom_factor(zoom as f32);
			platform::rezoom(frame, zoom / self.shown.zoom, zoom);
			self.shown.zoom = zoom;
		}
		let look = theme::resolve(self.app.settings.theme, platform::system_theme(ctx));
		if self.shown.theme != Some(look) {
			ctx.set_theme(look);
			ctx.send_viewport_cmd(ViewportCommand::SetTheme(match look {
				egui::Theme::Dark => egui::SystemTheme::Dark,
				egui::Theme::Light => egui::SystemTheme::Light,
			}));
			self.shown.theme = Some(look);
		}
	}

	/// The wait before the next tick: short while something moves, else no tick.
	fn next_tick(&self) -> Option<Duration> {
		let live = &self.app.live;
		#[cfg(not(target_arch = "wasm32"))]
		let downloading = crate::downloads::running();
		#[cfg(not(target_arch = "wasm32"))]
		let exporting = self.exports.exporting();
		#[cfg(target_arch = "wasm32")]
		let (downloading, exporting) = (false, false);
		let moving = downloading
			|| exporting
			|| live.run_is_progressing()
			|| live.is_working()
			|| live.fit_pending()
			|| live.playback.is_playing()
			|| self.app.document.history.has_pending()
			|| self.app.document.image_change_pending();
		moving.then_some(TICK)
	}
}

impl eframe::App for Shell {
	fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
		let ctx = ui.ctx().clone();
		// what the window and the panes ask the state for, sent after the typed text was applied
		let mut asked = Vec::new();
		#[cfg(not(target_arch = "wasm32"))]
		if let Some(path) = self.boot.open.take_last() {
			asked.push(Event::Guarded(Guarded::Open(Source::Path(path))));
		}
		if ctx.input(|i| i.viewport().close_requested()) && !self.closing {
			ctx.send_viewport_cmd(ViewportCommand::CancelClose);
			self.close_asked = true;
		}
		if self.close_asked && !self.modals.is_open() {
			self.close_asked = false;
			asked.push(Event::Guarded(Guarded::Exit));
		}
		self.app.tick(Instant::now());
		self.notices(&ctx);
		#[cfg(not(target_arch = "wasm32"))]
		{
			self.exports.poll(&ctx, &mut self.app, &mut self.modals);
			if let Some(after) = self.exports.take_after() {
				self.leave(after);
			}
		}
		#[cfg(not(target_arch = "wasm32"))]
		self.updates.take(&self.app.strings, &mut self.modals);
		self.follow_settings(&ctx, frame);
		// the window's keys wait while a modal shows
		let modal = self.modals.is_open();
		let keyboard = (!modal).then(|| Keyboard::before_ui(&ctx));
		edit::begin_pass(&ctx);
		#[cfg(not(target_arch = "wasm32"))]
		let (has_last, export) = (self.exports.has_last(&self.app), self.exports.status());
		#[cfg(target_arch = "wasm32")]
		let (has_last, export) = (false, None);
		let mut picked = None;
		egui::Panel::top("menu_bar").show(ui, |ui| {
			ui.add_enabled_ui(!modal, |ui| {
				picked = menu_bar::show(ui, &self.app, &self.dock, &mut self.pastable, has_last);
			});
		});
		self.messages.poll(&self.boot.log);
		self.cache_line.update(&self.app);
		if status_bar::show(ui, &self.app, &self.boot.log, export, &self.cache_line) {
			self.dock.show(Pane::Messages);
			self.messages.focus();
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
					Pane::Preview => preview.ui(ui, app, &mut asked),
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
		let then = self.modals.ui(&ctx, &self.app, &mut asked);
		if let Some(manual) = &mut self.manual
			&& !manual.show(&ctx, &self.app)
		{
			self.manual = None;
		}
		if !modal {
			self.files.drop_in(&ctx, &self.app);
		}
		self.files.take(&self.app, &mut asked, &mut self.modals);
		let keys = keyboard.map_or_else(Vec::new, |k| k.after_ui(&ctx));
		let picked = picked.map(|action| (action, On::Current)).or(from_panes);
		let pasted = platform::pasted();
		// typed text applies before anything else acts, and when its box is gone
		let acting = !asked.is_empty() || !keys.is_empty() || picked.is_some() || pasted.is_some() || request.is_some();
		let mut pending = Vec::new();
		edit::take(&ctx, acting, &mut pending);
		for event in pending.into_iter().chain(asked) {
			self.send(&ctx, event);
		}
		if let Some(then) = then {
			self.then(&ctx, then);
		}
		if let Some(request) = request {
			self.answer(&ctx, request);
		}
		for (action, pasted) in keys {
			self.act(&ctx, action, On::Current, pasted);
		}
		if let Some((action, on)) = picked {
			self.act(&ctx, action, on, None);
		}
		if let Some(text) = pasted {
			self.act(&ctx, MenuAction::Paste, On::Current, Some(text));
		}
		// A drag edits the arrangement on every frame: it is saved once the pointer lets go.
		if !ctx.input(|i| i.pointer.any_down())
			&& let Some(layout) = self.dock.take_change()
		{
			self.send(&ctx, Event::SetLayout(layout));
		}
		if std::mem::take(&mut self.exiting) {
			platform::remember_place(frame, &mut self.app.settings);
			self.app.settings.layout = self.dock.save();
			if let Err(e) = self.app.settings.save(&self.boot.store) {
				tracing::warn!("{e}");
			}
			self.closing = true;
			ctx.send_viewport_cmd(ViewportCommand::Close);
		}
		self.leave.set(self.app.document.dirty || self.app.live.export_busy());
		if let Some(tick) = self.next_tick() {
			ctx.request_repaint_after(tick);
		}
	}
}

/// Asks for the renderer's device with the adapter's own limits where it runs compute shaders, so the engines can
/// share it (large grids need its storage binding size); egui's request otherwise.
#[cfg(not(target_arch = "wasm32"))]
pub fn share_device(options: &mut egui_wgpu::WgpuConfiguration) {
	if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
		let fallback = setup.device_descriptor.clone();
		setup.device_descriptor = Arc::new(move |adapter| {
			if adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
				wgpu::DeviceDescriptor {
					label: Some("wavedude"),
					required_limits: adapter.limits(),
					..Default::default()
				}
			} else {
				fallback(adapter)
			}
		});
	}
}

/// The engines' GPU: the renderer's device when it can run them; `None` (logged) otherwise.
#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_gpu(cc: &eframe::CreationContext<'_>) -> Option<GpuHandle> {
	let rs = cc.wgpu_render_state.as_ref()?;
	GpuContext::from_device(&rs.adapter, rs.device.clone(), rs.queue.clone())
		.inspect_err(|e| tracing::warn!("the renderer's GPU device runs no engine: {e}"))
		.ok()
}
