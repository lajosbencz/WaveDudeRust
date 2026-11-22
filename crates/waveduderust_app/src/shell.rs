//! A window's course without a toolkit (DESIGN.md, State wiring): the events sent and their responses, the menu
//! actions both frontends run alike, the live run's notices, the modal queue, the exports' course, the updater's
//! answers, the close and exit flow and the tick. The frontend lends a [`Host`] for the clipboard and the tasks,
//! draws the shown modal and routes what [`Acted::Show`] leaves to it.

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use waveduderust::model::ItemRef;
use waveduderust::run::RunState;

#[cfg(not(target_arch = "wasm32"))]
use crate::export::session;
use crate::export::session::{After, Effect as ExportEffect, ExportForm, Exports, Step};
use crate::jobs::{Done, Job};
use crate::live::{Notice, RunFailure};
use crate::log::LogRing;
use crate::memory_warning::MemoryWarning;
use crate::menus::{self, MenuAction, On, ZoomStep};
use crate::modal::{Modal, Modals, Reply};
use crate::platform::{Duration, Instant, Opened, Store, open, take_crash_report};
use crate::settings_table::zoom_step;
use crate::strings::Strings;
use crate::window::TICK;
use crate::{App, Event, Guarded, Response, wake};

/// What a window starts with beside the state.
pub struct Boot {
	/// Where the settings are saved at exit.
	pub store: Store,
	/// The log's lines, the status line's among them.
	pub log: LogRing,
	/// The files the command line and the OS ask to open.
	pub open: crate::open_requests::OpenRequests,
	/// The downloaded update that installs at exit.
	#[cfg(not(target_arch = "wasm32"))]
	pub update: crate::update::Pending,
}

/// What the frontend lends the shell to act.
pub trait Host {
	/// Puts `text` on the clipboard.
	fn copy(&self, text: String);
	/// Runs `job` for the document `document` with the strings `s`; its answer comes back as a [`Done`].
	fn start(&self, job: Job, document: u64, s: Strings);
	/// Runs an export job on a thread of its own; its reports come back as [`Done::Export`].
	#[cfg(not(target_arch = "wasm32"))]
	fn export(&self, job: Box<session::Job>);
}

/// What [`ShellState::act`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acted {
	/// Disabled, or with nothing to act on.
	Refused,
	Ran,
	/// An Edit action ran ([`menus::edit`]).
	Edited,
	/// An action the frontend shows itself: Examples, Welcome, Settings, About, a pane.
	Show(MenuAction),
}

/// What an action does.
enum Effect {
	Send(Event),
	Start(Job),
	Zoom(ZoomStep),
	/// Asks the string `question`, whose yes destroys something, and sends the event on yes.
	Destroy(&'static str, Event),
	CheckUpdates,
	OpenUrl(&'static str),
	Edit,
	/// Its dialog, Export last or the probe CSV.
	Export,
	/// Cancels the export, else stops the run.
	Stop,
	Show,
}

/// What `action` does; `None` while it has nothing to act on.
fn effect(app: &App, action: MenuAction) -> Option<Effect> {
	let doc = &app.document;
	Some(match action {
		MenuAction::CheckForUpdates => Effect::CheckUpdates,
		MenuAction::New => Effect::Send(Event::Guarded(Guarded::New)),
		MenuAction::Open => Effect::Start(Job::Open { start: doc.folder().map(Into::into) }),
		MenuAction::Recent(i) => Effect::Send(Event::Guarded(Guarded::Open(crate::document::Source::Path(
			app.settings.recent_files.get(i)?.clone(),
		)))),
		MenuAction::Save => Effect::Send(Event::Save),
		MenuAction::SaveAs => Effect::Start(Job::SaveAs { proposed: doc.proposed_save_path() }),
		MenuAction::Close => Effect::Send(Event::Guarded(Guarded::Close)),
		MenuAction::Exit => Effect::Send(Event::Guarded(Guarded::Exit)),
		MenuAction::Examples | MenuAction::Welcome | MenuAction::Settings | MenuAction::About | MenuAction::Pane(_) => {
			Effect::Show
		}
		MenuAction::Zoom(step) => Effect::Zoom(step),
		MenuAction::Run => Effect::Send(Event::Run),
		MenuAction::PauseRun => Effect::Send(Event::TogglePauseRun),
		MenuAction::Stop => Effect::Stop,
		MenuAction::DiscardFrames => Effect::Destroy("DiscardFramesQuestion", Event::DiscardFrames),
		MenuAction::Play => Effect::Send(Event::TogglePlayback),
		MenuAction::ProjectPage => Effect::OpenUrl(env!("CARGO_PKG_REPOSITORY")),
		MenuAction::Export(_) | MenuAction::ExportLast | MenuAction::ExportProbeCsv => Effect::Export,
		MenuAction::Undo
		| MenuAction::Redo
		| MenuAction::Cut
		| MenuAction::Copy
		| MenuAction::Paste
		| MenuAction::Delete
		| MenuAction::SelectAll
		| MenuAction::AddSource
		| MenuAction::AddPart(_)
		| MenuAction::AddProbe(_)
		| MenuAction::ToggleSelected
		| MenuAction::ToggleEnabled
		| MenuAction::Duplicate
		| MenuAction::MoveUp
		| MenuAction::MoveDown
		| MenuAction::ToggleUserScheme => Effect::Edit,
	})
}

/// The window's state beside [`App`]: its modals, the exports, the updater and the close flow.
pub struct ShellState {
	pub modals: Modals,
	exports: Exports,
	/// The finished export's first output, for the frontend to offer showing in its folder.
	#[cfg(not(target_arch = "wasm32"))]
	exported: Option<PathBuf>,
	#[cfg(not(target_arch = "wasm32"))]
	updater: crate::update::Updater,
	/// The close box was pressed: Exit is asked for once no modal shows.
	close_asked: bool,
	/// Exit was answered: the window saves its place and closes at the end of the pass.
	exiting: bool,
	/// The window is closing; its close request goes through.
	closing: bool,
	/// The document shown.
	document: u64,
	/// The run's state at the last [`Self::run_ended`].
	run_state: Option<RunState>,
}

impl ShellState {
	/// Queues the crash report an earlier session left and, as the settings ask, the launch's update check.
	pub fn new(app: &App, boot: &Boot) -> Self {
		let mut modals = Modals::default();
		if let Some(report) = take_crash_report(&boot.store) {
			modals.show(Modal::crash(&app.strings, report));
		}
		let shell = Self {
			modals,
			exports: Exports::default(),
			#[cfg(not(target_arch = "wasm32"))]
			exported: None,
			#[cfg(not(target_arch = "wasm32"))]
			updater: crate::update::Updater::new(boot.update.clone()),
			close_asked: false,
			exiting: false,
			closing: false,
			document: app.document_id(),
			run_state: None,
		};
		if app.settings.check_for_updates == Some(true) {
			shell.check_updates(true);
		}
		shell
	}

	/// Sends `event` to the state and acts on its response.
	pub fn send(&mut self, app: &mut App, host: &impl Host, event: Event) {
		match app.handle(event, Instant::now()) {
			Response::Exit => self.quit(app, After::Exit),
			Response::Restart => self.quit(app, After::Restart),
			Response::Clipboard { text, then } => {
				host.copy(text);
				if let Some(command) = then {
					self.send(app, host, Event::Command(command));
				}
			}
			Response::AskSave { name } => self.modals.show(Modal::ask_save(&app.strings, &name)),
			Response::AskSavePath { proposed } => {
				let proposed = proposed.or_else(|| app.document.proposed_save_path());
				host.start(Job::SaveAs { proposed }, app.document_id(), app.strings);
			}
			Response::MemoryWarning => self.ask_memory_warning(app),
			Response::Error(line) => self.modals.show(Modal::Error(line)),
			Response::None => {}
		}
	}

	/// Runs `action` on `on` unless it is disabled; `read` reads the clipboard for Paste. Whether this frontend
	/// offers `action` is its own check, before.
	pub fn act(
		&mut self,
		app: &mut App,
		host: &impl Host,
		action: MenuAction,
		on: On,
		read: impl FnOnce() -> Option<String>,
	) -> Acted {
		if action == MenuAction::ExportLast && !self.exports.has_last(app) {
			tracing::debug!("{action:?} refused: no last export");
			return Acted::Refused;
		}
		if !menus::enabled(app, action, on) {
			tracing::debug!("{action:?} refused");
			return Acted::Refused;
		}
		let Some(effect) = effect(app, action) else { return Acted::Refused };
		match effect {
			Effect::Edit => {
				for event in menus::edit(app, action, on, read).unwrap_or_default() {
					self.send(app, host, event);
				}
				return Acted::Edited;
			}
			Effect::Send(event) => self.send(app, host, event),
			Effect::Start(job) => host.start(job, app.document_id(), app.strings),
			Effect::Zoom(step) => {
				if let Some(zoom) = zoom_step(&app.settings, step) {
					self.send(app, host, Event::SetMenuZoom(zoom));
				}
			}
			Effect::Destroy(question, yes) => {
				let text = app.strings.get(question).to_owned();
				self.modals.show(Modal::Confirm { text, yes: Reply::Send(yes), destroys: true });
			}
			Effect::Export => self.export(app, host, action, on),
			Effect::Stop => {
				if !self.exports.cancel(app) {
					self.send(app, host, Event::StopRun);
				}
			}
			Effect::CheckUpdates => self.check_updates(false),
			Effect::OpenUrl(url) => {
				if let Err(e) = open(Opened::Url(url)) {
					tracing::error!("{url} could not be opened: {e}");
					self.modals.show(Modal::Error(format!("{}\n{e}", app.strings.format("OpenError", &[&url]))));
				}
			}
			Effect::Show => return Acted::Show(action),
		}
		Acted::Ran
	}

	/// Does what an answered modal asks for; the string telling of a copy, for the frontend to show.
	pub fn reply(&mut self, app: &mut App, host: &impl Host, reply: Reply) -> Option<&'static str> {
		match reply {
			Reply::Send(event) => self.send(app, host, event),
			Reply::Export(step) => {
				let effects = self.exports.step(app, step);
				self.apply(effects, host);
			}
			#[cfg(not(target_arch = "wasm32"))]
			Reply::DownloadUpdate(offered) => self.updater.download(wake::wake, &app.strings, *offered.0),
			#[cfg(not(target_arch = "wasm32"))]
			Reply::Browse(target) => {
				if let Some(job) = self.modals.browse(target) {
					host.start(job, app.document_id(), app.strings);
				}
			}
			Reply::Copy { text, note } => {
				host.copy(text);
				return Some(note);
			}
			#[cfg(target_arch = "wasm32")]
			Reply::LocateSceneImage => {
				let path = app.document.project.scene.image_path.clone();
				host.start(Job::LocateSceneImage { path }, app.document_id(), app.strings);
			}
		}
		None
	}

	/// Acts on a task's answer; the event it asks the state for, which the caller sends in turn.
	#[cfg_attr(target_arch = "wasm32", expect(unused_variables, reason = "the browser's exports answer no task"))]
	pub fn answered(&mut self, app: &mut App, host: &impl Host, done: Done) -> Option<Event> {
		match done {
			Done::Send(event) => return Some(event),
			Done::Error(line) => self.modals.show(Modal::Error(line)),
			#[cfg(not(target_arch = "wasm32"))]
			Done::Export(msg) => {
				let effects = self.exports.take(app, msg);
				self.apply(effects, host);
			}
			#[cfg(not(target_arch = "wasm32"))]
			Done::Csv { probe, path } => {
				let effects = self.exports.csv_chosen(app, app.document_id(), probe, path);
				self.apply(effects, host);
			}
			#[cfg(not(target_arch = "wasm32"))]
			Done::Browsed(target, path) => self.modals.browsed(target, &path),
		}
		None
	}

	/// Before the state's tick: the browser's export steps, an exit waiting for the export goes on, the updater's
	/// answers queue as modals.
	#[cfg_attr(not(target_arch = "wasm32"), expect(unused_variables, reason = "native exports run on threads"))]
	pub fn poll(&mut self, app: &mut App, host: &impl Host) {
		#[cfg(target_arch = "wasm32")]
		{
			let effects = self.exports.poll(app);
			self.apply(effects, host);
		}
		if let Some(after) = self.exports.take_after() {
			self.leave(after);
		}
		#[cfg(not(target_arch = "wasm32"))]
		for notice in self.updater.take(&app.strings) {
			use crate::update::Notice as Update;
			self.modals.show(match notice {
				Update::Info(text) => Modal::Info(text),
				Update::Error(text) => Modal::Error(text),
				Update::Offer { text, update } => {
					Modal::Confirm { text, yes: Reply::DownloadUpdate(crate::modal::Offered(update)), destroys: false }
				}
				Update::Ready { text } => {
					Modal::Confirm { text, yes: Reply::Send(Event::Guarded(Guarded::Restart)), destroys: false }
				}
			});
		}
	}

	/// Ticks the state at `now`, then shows the live run's notices and the scene image's offer.
	pub fn tick(&mut self, app: &mut App, host: &impl Host, now: Instant) {
		app.tick(now);
		self.notices(app, host);
		self.scene_image_offer(app);
	}

	/// The close box was pressed: the modals but the exit's questions go, and Exit is asked for once none shows.
	/// False while the window is closing, whose close goes through.
	pub fn close_pressed(&mut self) -> bool {
		if self.closing {
			return false;
		}
		self.close_asked = true;
		self.modals.drop_for_exit();
		true
	}

	/// The close box's Exit, once no modal shows.
	pub fn exit_due(&mut self) -> Option<Event> {
		let due = self.close_asked && !self.modals.is_open();
		if due {
			self.close_asked = false;
		}
		due.then_some(Event::Guarded(Guarded::Exit))
	}

	/// Exit was answered: true once, when the window saves its place and closes; its close goes through from then.
	pub fn take_exit(&mut self) -> bool {
		let exit = std::mem::take(&mut self.exiting);
		self.closing |= exit;
		exit
	}

	/// Whether another document shows since the last call.
	pub fn document_changed(&mut self, app: &App) -> bool {
		let id = app.document_id();
		let changed = id != self.document;
		self.document = id;
		changed
	}

	/// Whether the run completed or failed since the last call.
	pub fn run_ended(&mut self, app: &App) -> bool {
		let now = app.live.run_status().map(|(state, ..)| state);
		let ended = matches!(now, Some(RunState::Completed | RunState::Failed(_))) && now != self.run_state;
		self.run_state = now;
		ended
	}

	/// The wait before the next tick: short while something moves, else none.
	pub fn next_tick(&self, app: &App) -> Option<Duration> {
		let live = &app.live;
		#[cfg(not(target_arch = "wasm32"))]
		let exporting = self.exports.exporting() || crate::downloads::running();
		// the browser's export repaints while it works and is woken while it waits
		#[cfg(target_arch = "wasm32")]
		let exporting = false;
		let moving = exporting
			|| live.run_is_progressing()
			|| live.is_working()
			|| live.fit_pending()
			|| live.playback.is_playing()
			|| app.document.history.has_pending()
			|| app.document.image_change_pending();
		moving.then_some(TICK)
	}

	/// File > Export last has something to repeat for the open project.
	pub fn has_last_export(&self, app: &App) -> bool {
		self.exports.has_last(app)
	}

	/// The export's status line text and share done ([`Exports::status`]).
	pub fn export_status(&self) -> Option<(&str, Option<f32>)> {
		self.exports.status()
	}

	/// The first output of the finished export, until taken or another export starts.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn exported(&self) -> Option<&Path> {
		self.exported.as_deref()
	}

	/// The first output of the export finished since the last call.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn take_exported(&mut self) -> Option<PathBuf> {
		self.exported.take()
	}

	/// Looks for an update; `quiet` says nothing unless there is one.
	#[cfg_attr(
		target_arch = "wasm32",
		expect(clippy::unused_self, unused_variables, reason = "the browser has no updater")
	)]
	fn check_updates(&self, quiet: bool) {
		#[cfg(not(target_arch = "wasm32"))]
		self.updater.check(wake::wake, quiet);
	}

	/// Ends the program, `after` saying whether it restarts into the downloaded update; a running export is stopped
	/// first on the user's word, and the end waits for it.
	fn quit(&mut self, app: &App, after: After) {
		if self.exports.exporting() {
			let text = app.strings.get("ExitStopsExport").to_owned();
			self.modals.show(Modal::Confirm { text, yes: Reply::Export(Step::Stop(after)), destroys: false });
		} else {
			self.leave(after);
		}
	}

	fn leave(&mut self, after: After) {
		#[cfg(not(target_arch = "wasm32"))]
		if after == After::Restart {
			self.updater.restart();
		}
		#[cfg(target_arch = "wasm32")]
		let _ = after;
		self.exiting = true;
	}

	fn ask_memory_warning(&mut self, app: &App) {
		if let Some(warning) = MemoryWarning::of(app) {
			self.modals.show(Modal::MemoryWarning(warning));
		}
	}

	/// Asks about the scene image file that differs from the embedded one, while it does.
	fn scene_image_offer(&mut self, app: &App) {
		match app.document.scene_image_offer() {
			Some(path) => {
				let name = path.file_name().map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy());
				self.modals.ask_scene_image(app.strings.format("SceneImageChangedFormat", &[&name]));
			}
			None => self.modals.drop_scene_image(),
		}
	}

	/// The live run's notices a window shows.
	fn notices(&mut self, app: &mut App, host: &impl Host) {
		for notice in app.live.take_notices() {
			match notice {
				Notice::RunFailed(failure) => self.run_failed(app, failure),
				Notice::AskMemoryWarning => self.ask_memory_warning(app),
				Notice::WarningDropped => self.modals.drop_memory_warning(),
				Notice::ExportReady => {
					let effects = self.exports.ready(app);
					self.apply(effects, host);
				}
				Notice::ExportStopped => self.exports.stopped(),
				Notice::ShowFrame(_) | Notice::Info(_) | Notice::Error(_) => {}
			}
		}
	}

	/// A failed run's message; in the browser, a run that lacks the scene image asks for its upload instead.
	fn run_failed(&mut self, app: &App, failure: RunFailure) {
		#[cfg(target_arch = "wasm32")]
		if failure == RunFailure::NoImage
			&& let Some(name) = app.document.missing_scene_image()
		{
			let text = app.strings.format("LocateSceneImageFormat", &[&name]);
			self.modals.show(Modal::Confirm { text, yes: Reply::LocateSceneImage, destroys: false });
			return;
		}
		self.modals.show(Modal::Error(failure.message(&app.strings)));
	}

	/// An export command on `on`: its dialog, while no export is busy, Export last, or the probe CSV (natively the
	/// save dialog, in the browser the export dialog).
	fn export(&mut self, app: &mut App, host: &impl Host, action: MenuAction, on: On) {
		match action {
			MenuAction::Export(choice) => {
				if let Some(form) = ExportForm::open(app, choice) {
					self.modals.show(Modal::Export(form));
				}
			}
			MenuAction::ExportLast => {
				let effects = self.exports.export_last(app);
				self.apply(effects, host);
			}
			MenuAction::ExportProbeCsv => {
				if let Some(ItemRef::Probe(probe)) = menus::target(app, on) {
					#[cfg(not(target_arch = "wasm32"))]
					if !app.live.export_busy() {
						let (start, probe) = crate::export::probe_csv_request(app, probe);
						host.start(Job::ProbeCsv { start, probe }, app.document_id(), app.strings);
					}
					#[cfg(target_arch = "wasm32")]
					if let Some(form) = ExportForm::csv(app, probe) {
						self.modals.show(Modal::Export(form));
					}
				}
			}
			_ => {}
		}
	}

	/// Shows the exports' questions, errors and downloads; runs their jobs.
	#[cfg_attr(target_arch = "wasm32", expect(unused_variables, reason = "the browser's export steps on the page"))]
	fn apply(&mut self, effects: Vec<ExportEffect>, host: &impl Host) {
		for effect in effects {
			match effect {
				#[cfg(not(target_arch = "wasm32"))]
				ExportEffect::Confirm { text, yes } => {
					self.modals.show(Modal::Confirm { text, yes: Reply::Export(yes), destroys: false });
				}
				#[cfg(not(target_arch = "wasm32"))]
				ExportEffect::Exported(path) => self.exported = Some(path),
				ExportEffect::Error(text) => self.modals.show(Modal::Error(text)),
				#[cfg(not(target_arch = "wasm32"))]
				ExportEffect::Spawn(job) => {
					self.exported = None;
					host.export(job);
				}
				#[cfg(target_arch = "wasm32")]
				ExportEffect::Download(download) => self.modals.show(Modal::Download(download)),
				#[cfg(target_arch = "wasm32")]
				ExportEffect::Repaint => wake::wake(),
			}
		}
	}
}
