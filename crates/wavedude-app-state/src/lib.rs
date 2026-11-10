//! The application's headless state: the open document with its history, the selection, the live controller
//! with playback, the user settings, the strings and the log. The views send [`Event`]s and call
//! [`AppState::tick`] about 60 times a second; everything here is testable without a window.
//!
//! Threads: `AppState` lives on the UI thread; the live run and the playback clock it drives work
//! on threads of their own and are polled.

pub mod clipboard;
pub mod commands;
pub mod document;
pub mod examples;
pub mod history;
pub mod live;
pub mod log;
pub mod playback;
pub mod selection;
pub mod settings;
pub mod strings;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::model::Point;
use wavedude_core::run::{Levers, MachineFacts, PreviewQuality};

use clipboard::{Copyable, PasteTarget};
use commands::{Change, ChangeKind, Command, CommandError, Target};
use document::{Document, DocumentError, Opened};
use live::{Backend, CoreBackend, LiveController, LiveSettings};
use selection::{ItemKind, ItemRef, Row, Selection};
use settings::{AppDirs, ExportHint, Settings, Theme};
use strings::{Language, Strings};

/// An action that replaces the open project, asked about first when there are unsaved edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Guarded {
	New,
	Open(PathBuf),
	OpenExample(usize),
	Close,
	Exit,
	/// Exit to install a downloaded update and start it.
	Restart,
}

/// The answer to "Save the changes to ...?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveChoice {
	Save,
	DontSave,
	Cancel,
}

/// The kinds of export with a start folder of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
	Run,
	Still,
	Probe,
}

/// What the views send.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
	Command(Command),
	MouseDown,
	MouseUp,
	Undo,
	Redo,
	Guarded(Guarded),
	/// A file dropped on the window (not claimed by the tree or the pictures).
	DropFile(PathBuf),
	Save,
	SaveAs(PathBuf),
	/// The Save As dialog was cancelled.
	SaveAsCancelled,
	SaveChoice(SaveChoice),
	/// Copy of the selection (or the current settings row).
	Copy,
	Cut {
		target: ItemRef,
	},
	/// Paste of clipboard or drag text.
	Paste {
		text: String,
		target: PasteTarget,
		place_at: Option<Point>,
	},
	Run,
	StopRun,
	TogglePauseRun,
	Play,
	PausePlayback,
	Scrub(u32),
	StepFrames(i64),
	SetLive(bool),
	SetSpeed(f64),
	SetLoop(bool),
	SetInPoint,
	SetOutPoint,
	ClearRange,
	MemoryWarningRun {
		levers: Levers,
		mute: bool,
	},
	MemoryWarningCancel,
	SetLanguage(String),
	SetMenuZoom(f64),
	SetTheme(Theme),
	SetPreviewQuality(PreviewQuality),
	SetDitherBits(u32),
	SetAutoRerun(bool),
	SetUseGpu(bool),
	SetFrameMemoryPercent(i32),
	SetWarnWhenRamInsufficient(bool),
	SetTitleFullPath(bool),
	/// The user's answer whether MP4 export may use Cisco's OpenH264.
	SetUseOpenH264(bool),
	/// Whether to look for a new release at launch.
	SetCheckForUpdates(bool),
	SetPreviewOrientation(settings::Orientation),
}

/// What the views must do after an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
	None,
	/// Ask "Save the changes to {name}?" and answer with [`Event::SaveChoice`].
	AskSave {
		name: String,
	},
	/// Show the Save As dialog; answer with [`Event::SaveAs`] or [`Event::SaveAsCancelled`].
	AskSavePath {
		proposed: Option<PathBuf>,
	},
	/// Put this text on the clipboard (app format and plain text).
	Clipboard(String),
	/// Show the memory warning ([`AppState::live`]`.warning()`).
	MemoryWarning,
	/// A one-line error for the message window (the detail is in the log).
	Error(String),
	/// The app may close now.
	Exit,
	/// The app may close now and start again.
	Restart,
}

/// Change counters, one per part of the state a view shows; each moves when its part may have changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revisions {
	/// The project and its file: content, path, dirty flag, undo and redo.
	pub document: u64,
	/// The scene and the parts: what the scene picture and the outline are built from.
	pub scene: u64,
	pub selection: u64,
	/// The position, the frame shown, the frame count.
	pub playback: u64,
	/// A run's progress: its steps done and the frames it published; moves every tick of a run.
	pub progress: u64,
	/// The live run's work: stepping or not, and towards which frame.
	pub work: u64,
	/// The slower live state: the run's state, playing, loop, speed, range, follow.
	pub run: u64,
	/// The user settings, the language among them.
	pub settings: u64,
}

/// The observable part of each revision, compared after every event and tick: the copyable part as read,
/// the rest held as a copy that is renewed when it differs.
#[derive(Clone, PartialEq)]
struct Observed {
	document: (bool, bool, bool),
	path: Option<PathBuf>,
	selection: Selection,
	playback: (u32, Option<u32>, u32),
	progress: (Option<u32>, Option<(u64, usize)>),
	work: (bool, Option<u32>, bool),
	run: RunObserved,
	settings: Settings,
}

type RunObserved =
	(Option<(wavedude_core::run::RunState, u32)>, (bool, bool, bool, bool), (bool, u64, bool, (u32, u32), bool));

/// See the module docs.
pub struct AppState<B: Backend> {
	pub settings: Settings,
	settings_path: Option<PathBuf>,
	pub strings: Strings,
	pub document: Document,
	pub selection: Selection,
	pub live: LiveController<B>,
	/// The user's colour schemes beside the project's and the built-in ones.
	pub schemes: SchemeStore,
	/// No settings file existed at the start: the startup page shows.
	pub first_start: bool,
	/// Cisco's OpenH264 folder (`AppDirs::codec`); `None` without one.
	pub codec_dir: Option<PathBuf>,
	revisions: Revisions,
	observed: Option<Observed>,
	/// Bumped when the tree is to give the current row the keyboard focus: an opened project, a remove, a move,
	/// an undo that left nothing selected. It moves with the document revision.
	reveals: u64,
	use_gpu: Arc<AtomicBool>,
	pending: Option<Guarded>,
	/// The export hint of a project without a file: this session's alone.
	session_hint: ExportHint,
}

fn live_settings(s: &Settings) -> LiveSettings {
	LiveSettings {
		auto_rerun: s.auto_rerun,
		warn_when_ram_insufficient: s.warn_when_ram_insufficient,
		quality: s.preview_quality(),
		ram_percent: s.frame_memory_share(),
	}
}

impl AppState<CoreBackend> {
	/// The app's state on the core's engines: settings from `dirs` (the first start chooses the preview
	/// quality from the machine), the empty project, the language and zoom of the settings and `args`.
	pub fn start<'a>(dirs: &AppDirs, args: impl IntoIterator<Item = &'a str>) -> std::io::Result<Self> {
		let path = dirs.settings_file();
		let loaded = Settings::load(&path, || MachineFacts {
			has_gpu: Some(wavedude_core::engine::gpu::context().is_ok()),
			..MachineFacts::read()
		});
		let mut settings = loaded.settings;
		if let Some(zoom) = settings::zoom_switch(args) {
			settings.ui_zoom = zoom;
		}
		let use_gpu = Arc::new(AtomicBool::new(settings.use_gpu));
		let backend = CoreBackend::new(use_gpu.clone(), dirs.cache.join("frames"))?;
		let mut state = Self::with_backend(settings, Some(path), backend, use_gpu);
		state.schemes = SchemeStore::with_user_folder(&dirs.schemes()).0;
		state.first_start = loaded.first_start;
		state.codec_dir = Some(dirs.codec());
		Ok(state)
	}
}

impl<B: Backend> AppState<B> {
	/// The state on `backend`; `settings_path` is where setting changes are saved (`None`: not saved).
	pub fn with_backend(
		settings: Settings,
		settings_path: Option<PathBuf>,
		backend: B,
		use_gpu: Arc<AtomicBool>,
	) -> Self {
		let strings = Strings::new(Language::resolve(&settings.language));
		let document = Document::empty();
		let mut live = LiveController::new(backend, live_settings(&settings));
		live.project_replaced(&document.project, Instant::now());
		let mut state = Self {
			settings,
			settings_path,
			strings,
			document,
			selection: Selection::root(),
			live,
			schemes: SchemeStore::default(),
			first_start: false,
			codec_dir: None,
			revisions: Revisions::default(),
			observed: None,
			reveals: 0,
			use_gpu,
			pending: None,
			session_hint: ExportHint::default(),
		};
		state.sync_revisions();
		state
	}

	/// The change counters as of the last event or tick.
	pub fn revisions(&self) -> Revisions {
		self.revisions
	}

	fn observe(&self) -> Observed {
		let live = &self.live;
		let p = &live.playback;
		let status = live.run_status();
		Observed {
			document: (self.document.dirty, self.document.history.can_undo(), self.document.history.can_redo()),
			path: self.document.path.clone(),
			selection: self.selection.clone(),
			playback: (p.position(), live.shown(), p.frame_count()),
			progress: (status.as_ref().map(|s| s.1), live.progress_key()),
			work: (live.is_working(), live.computing_frame(), live.fit_pending()),
			run: (
				status.map(|(state, _, total)| (state, total)),
				(live.run_is_active(), live.is_live(), live.warning().is_some(), live.export_busy()),
				(p.is_playing(), p.speed().to_bits(), p.looping(), p.range_frames(), live.run_paused_by_user()),
			),
			settings: self.settings.clone(),
		}
	}

	/// Moves the counters whose observable part changed since the last call; true when any moved.
	pub fn sync_revisions(&mut self) -> bool {
		let before = self.revisions;
		let Some(old) = self.observed.as_mut() else {
			self.observed = Some(self.observe());
			return false;
		};
		let live = &self.live;
		let p = &live.playback;
		let status = live.run_status();
		let r = &mut self.revisions;
		let moved = |changed: bool| u64::from(changed);
		let document = (self.document.dirty, self.document.history.can_undo(), self.document.history.can_redo());
		let path_changed = old.path != self.document.path;
		if path_changed {
			old.path.clone_from(&self.document.path);
		}
		r.document += moved(std::mem::replace(&mut old.document, document) != document || path_changed);
		if old.selection != self.selection {
			old.selection.clone_from(&self.selection);
			r.selection += 1;
		}
		let playback = (p.position(), live.shown(), p.frame_count());
		r.playback += moved(std::mem::replace(&mut old.playback, playback) != playback);
		let progress = (status.as_ref().map(|s| s.1), live.progress_key());
		r.progress += moved(std::mem::replace(&mut old.progress, progress) != progress);
		let work = (live.is_working(), live.computing_frame(), live.fit_pending());
		r.work += moved(std::mem::replace(&mut old.work, work) != work);
		let run = (
			status.map(|(state, _, total)| (state, total)),
			(live.run_is_active(), live.is_live(), live.warning().is_some(), live.export_busy()),
			(p.is_playing(), p.speed().to_bits(), p.looping(), p.range_frames(), live.run_paused_by_user()),
		);
		if old.run != run {
			old.run = run;
			r.run += 1;
		}
		if old.settings != self.settings {
			old.settings.clone_from(&self.settings);
			r.settings += 1;
		}
		before != self.revisions
	}

	/// The project changed in a way the cheap observation misses; `scene` when the scene or the parts did.
	fn document_changed(&mut self, scene: bool) {
		self.revisions.document += 1;
		self.revisions.scene += u64::from(scene);
	}

	/// The window title.
	pub fn title(&self) -> String {
		self.document.title(self.strings.get("Title"), self.strings.get("Untitled"), self.settings.title_full_path)
	}

	/// The status bar's run line: "<State>: <done> / <total> steps, <engine>[, edited since]"; empty without a run.
	pub fn run_status(&self) -> String {
		let Some((state, done, total)) = self.live.run_status() else {
			return String::new();
		};
		let key = match state {
			wavedude_core::run::RunState::Created => "RunStateCreated",
			wavedude_core::run::RunState::Running => "RunStateRunning",
			wavedude_core::run::RunState::Paused => "RunStatePaused",
			wavedude_core::run::RunState::Completed => "RunStateCompleted",
			wavedude_core::run::RunState::Cancelled => "RunStateCancelled",
			wavedude_core::run::RunState::Failed(_) => "RunStateFailed",
		};
		let s = &self.strings;
		s.format("RunStatusFormat", &[&s.get(key), &done, &total, &self.live.engine_name()])
	}

	/// Advances the history's quiet time and the live controller to `now`; true when something a view shows
	/// changed (a frame, the position, a run's state, a history step), so the views can skip a render on quiet
	/// ticks.
	pub fn tick(&mut self, now: Instant) -> bool {
		self.document.tick(now);
		if self.document.poll_image_file(now) {
			self.document_changed(true);
			let note = Some("the scene image file changed".into());
			self.live.on_change(ChangeKind::Simulation, &self.document.project, note, now);
		}
		self.live.tick(now, &self.document.project);
		self.sync_revisions()
	}

	fn save_settings(&self) {
		if let Some(path) = &self.settings_path
			&& let Err(e) = self.settings.save(path)
		{
			tracing::warn!("{e}");
		}
	}

	fn settings_changed(&mut self, preview_changed: bool, now: Instant) {
		self.use_gpu.store(self.settings.use_gpu, Ordering::Relaxed);
		self.live.set_settings(live_settings(&self.settings));
		if preview_changed {
			self.live.on_change(ChangeKind::Simulation, &self.document.project, Some("preview quality".into()), now);
		}
		self.save_settings();
	}

	fn record(&mut self, change: Change, scene: bool, now: Instant) {
		if change.kind != ChangeKind::None {
			self.document_changed(scene);
			self.document.edited(now);
			if change.own_step {
				self.document.commit_step();
			}
		}
		self.live.on_change(change.kind, &self.document.project, change.note, now);
	}

	/// Applies a model command as one edit.
	pub fn apply(&mut self, command: Command, now: Instant) -> Result<ChangeKind, CommandError> {
		let scene = self.touches_scene(&command);
		let reveal = matches!(command, Command::Remove { .. } | Command::Reorder { .. });
		let change = commands::apply(&mut self.document.project, &mut self.selection, command)?;
		let kind = change.kind;
		self.record(change, scene, now);
		self.reveals += u64::from(reveal);
		Ok(kind)
	}

	/// Whether `command` may change the scene or the parts.
	fn touches_scene(&self, command: &Command) -> bool {
		let parts = |t: ItemRef| self.selection.targets(t).iter().any(|i| i.kind == ItemKind::Part);
		match command {
			Command::AddPart { .. }
			| Command::SetSceneImage { .. }
			| Command::RemoveSceneImage
			| Command::Paste { .. }
			| Command::Set { target: Target::Scene, .. }
			| Command::Set { target: Target::Simulation, .. } => true,
			Command::Set { target: Target::Item(i), .. } | Command::Reorder { target: i, .. } => {
				i.kind == ItemKind::Part
			}
			Command::Remove { target }
			| Command::Duplicate { target }
			| Command::ToggleEnabled { target }
			| Command::MoveBy { target, .. } => parts(*target),
			_ => false,
		}
	}

	fn replace_document(&mut self, document: Document, now: Instant) {
		if let Some(path) = &document.path {
			self.settings.push_recent(path);
			self.save_settings();
		}
		self.document = document;
		self.document_changed(true);
		self.selection = Selection::root();
		self.reveals += 1;
		self.session_hint = ExportHint::default();
		self.live.project_replaced(&self.document.project, now);
	}

	fn perform(&mut self, action: Guarded, now: Instant) -> Response {
		match action {
			Guarded::New => self.replace_document(Document::new_project(), now),
			Guarded::Close => self.replace_document(Document::empty(), now),
			Guarded::Exit => return Response::Exit,
			Guarded::Restart => return Response::Restart,
			Guarded::OpenExample(i) => {
				let example = &examples::all()[i];
				match Document::from_example(example) {
					Ok(d) => self.replace_document(d, now),
					Err(e) => return self.error("OpenError", example.file_name, &e),
				}
			}
			Guarded::Open(path) => match Document::open(&path) {
				Ok(Opened::Project(d)) => self.replace_document(*d, now),
				Ok(Opened::Scheme(scheme)) => self.add_scheme(scheme, now),
				Err(e) => return self.error("OpenError", &path.display().to_string(), &e),
			},
		}
		Response::None
	}

	fn add_scheme(&mut self, scheme: wavedude_core::model::ColorScheme, now: Instant) {
		let i = self.document.project.add_color_scheme(scheme);
		self.selection.set_items(vec![ItemRef::new(ItemKind::Scheme, i)]);
		let mut change = Change { kind: ChangeKind::Display, own_step: true, note: None };
		change.note = Some(format!("colour scheme {} added", self.document.project.color_schemes[i].name));
		self.record(change, false, now);
	}

	fn error(&self, key: &str, what: &str, e: &DocumentError) -> Response {
		match e {
			DocumentError::Io(wavedude_core::io::IoError::Unreadable(u)) => tracing::error!("{}", u.report()),
			e => tracing::error!("{e}"),
		}
		Response::Error(self.strings.format(key, &[&what]))
	}

	/// An action that replaces the project: asked about first when there are unsaved edits.
	fn guarded(&mut self, action: Guarded, now: Instant) -> Response {
		let replaces = matches!(action, Guarded::New | Guarded::Close | Guarded::Open(_) | Guarded::OpenExample(_));
		if replaces && self.live.export_busy() && !matches!(&action, Guarded::Open(p) if is_scheme(p)) {
			tracing::warn!("The project stays while an export runs; finish or cancel the export first.");
			return Response::None;
		}
		if self.document.dirty && !matches!(&action, Guarded::Open(p) if is_scheme(p)) {
			let name = self.document.display_name(self.strings.get("Untitled"));
			self.pending = Some(action);
			return Response::AskSave { name };
		}
		self.perform(action, now)
	}

	fn save_then_continue(&mut self, path: Option<PathBuf>, now: Instant) -> Response {
		let result = match &path {
			Some(p) => self.document.save_as(p),
			None => self.document.save(),
		};
		match result {
			Ok(()) => {
				if let Some(p) = &self.document.path {
					self.settings.push_recent(p);
					self.save_settings();
				}
				match self.pending.take() {
					Some(action) => self.perform(action, now),
					None => Response::None,
				}
			}
			Err(DocumentError::NoPath) => Response::AskSavePath { proposed: self.document.proposed_save_path() },
			Err(e) => {
				self.pending = None;
				let name = path.or_else(|| self.document.path.clone()).expect("a save without a path is NoPath");
				self.error("SaveError", &name.display().to_string(), &e)
			}
		}
	}

	fn copy_text(&self) -> Option<String> {
		let mut objects: Vec<Copyable> = self.selection.items().iter().map(|&i| Copyable::Item(i)).collect();
		if objects.is_empty() {
			objects.extend(self.selection.current().and_then(Copyable::of_row));
		}
		(!objects.is_empty()).then(|| clipboard::write_fragment(&self.document.project, &objects))
	}

	/// Handles one event at `now`.
	pub fn handle(&mut self, event: Event, now: Instant) -> Response {
		let response = self.handle_event(event, now);
		self.sync_revisions();
		response
	}

	fn handle_event(&mut self, event: Event, now: Instant) -> Response {
		match event {
			Event::Command(c) => match self.apply(c, now) {
				Ok(_) => Response::None,
				Err(e) => {
					tracing::warn!("{e}");
					Response::Error(e.to_string())
				}
			},
			Event::MouseDown => {
				self.document.history.mouse_down();
				Response::None
			}
			Event::MouseUp => {
				self.document.history.mouse_up(now);
				Response::None
			}
			Event::Undo | Event::Redo => {
				let before = self.document.project.clone();
				let done = if event == Event::Undo { self.document.undo() } else { self.document.redo() };
				if done {
					let p = &self.document.project;
					self.document_changed(before.scene != p.scene || before.parts != p.parts);
					self.selection = self.selection.follow(&before, &self.document.project);
					if self.selection.current().is_none() {
						self.selection = Selection::root();
						self.reveals += 1;
					}
					let kind = commands::classify(&before, &self.document.project);
					self.live.on_change(kind, &self.document.project, Some("undo or redo".into()), now);
				}
				Response::None
			}
			Event::Guarded(action) => self.guarded(action, now),
			Event::DropFile(path) => {
				let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
				if ["wdproj", "json", "xml", "wdscheme"].contains(&ext.as_str()) {
					self.guarded(Guarded::Open(path), now)
				} else {
					tracing::warn!("'{}' is not a project or colour scheme file", path.display());
					Response::Error(self.strings.format("OpenError", &[&path.display()]))
				}
			}
			Event::Save => self.save_then_continue(None, now),
			Event::SaveAs(path) => self.save_then_continue(Some(path), now),
			Event::SaveAsCancelled => {
				self.pending = None;
				Response::None
			}
			Event::SaveChoice(choice) => match choice {
				SaveChoice::Save => self.save_then_continue(None, now),
				SaveChoice::DontSave => match self.pending.take() {
					Some(action) => self.perform(action, now),
					None => Response::None,
				},
				SaveChoice::Cancel => {
					self.pending = None;
					Response::None
				}
			},
			Event::Copy => self.copy_text().map_or(Response::None, Response::Clipboard),
			Event::Cut { target } => {
				let targets = self.selection.targets(target);
				let objects: Vec<Copyable> = targets.iter().map(|&i| Copyable::Item(i)).collect();
				let text = clipboard::write_fragment(&self.document.project, &objects);
				match self.apply(Command::Remove { target }, now) {
					Ok(_) => Response::Clipboard(text),
					Err(e) => Response::Error(e.to_string()),
				}
			}
			Event::Paste { text, target, place_at } => match clipboard::read_fragment(&text) {
				Ok(fragment) => {
					match self.apply(Command::Paste { fragment: Box::new(fragment), target, place_at }, now) {
						Ok(_) => Response::None,
						Err(e) => Response::Error(e.to_string()),
					}
				}
				Err(e) => {
					tracing::debug!("paste refused: {e}");
					Response::None
				}
			},
			Event::Run => self.live_start(|live, p, now| live.run_user(p, now), now),
			Event::Play => self.live_start(
				|live, p, now| {
					live.play(p, now);
					true
				},
				now,
			),
			Event::StopRun => {
				self.live.stop_run();
				Response::None
			}
			Event::TogglePauseRun => {
				self.live.toggle_pause_run();
				Response::None
			}
			Event::PausePlayback => {
				self.live.pause_playback(now);
				Response::None
			}
			Event::Scrub(frame) => {
				self.live.scrub(frame, now);
				Response::None
			}
			Event::StepFrames(delta) => {
				self.live.step_frames(delta, now);
				Response::None
			}
			Event::SetLive(on) => {
				self.live.set_live(on);
				Response::None
			}
			Event::SetSpeed(speed) => {
				self.live.playback.set_speed(speed);
				Response::None
			}
			Event::SetLoop(on) => {
				self.live.playback.set_looping(on);
				Response::None
			}
			Event::SetInPoint | Event::SetOutPoint | Event::ClearRange => {
				let p = &mut self.live.playback;
				match event {
					Event::SetInPoint => p.set_in_here(),
					Event::SetOutPoint => p.set_out_here(),
					_ => p.clear_range(),
				}
				let (from, to) = p.range_steps();
				let display = &mut self.document.project.display;
				if (display.export_from, display.export_to) != (from, to) {
					log::change(
						"Display.export range",
						&format!("{}..{}", display.export_from, display.export_to),
						&format!("{from}..{to}"),
					);
					(display.export_from, display.export_to) = (from, to);
					self.record(Change { kind: ChangeKind::Display, own_step: false, note: None }, false, now);
				}
				Response::None
			}
			Event::MemoryWarningRun { levers, mute } => {
				if let Some(choice) = self.live.confirm_warning(levers, mute, now) {
					self.settings.set_preview_quality(choice.quality);
					if choice.cache_auto {
						self.settings.frame_memory_percent = 0;
					}
					if choice.mute {
						self.settings.warn_when_ram_insufficient = false;
					}
					self.save_settings();
				}
				Response::None
			}
			Event::MemoryWarningCancel => {
				self.live.cancel_warning();
				Response::None
			}
			Event::SetLanguage(code) => {
				self.strings.set_language(Language::resolve(&code));
				self.settings.language = code;
				self.save_settings();
				Response::None
			}
			Event::SetMenuZoom(zoom) => {
				self.settings.set_menu_zoom(zoom);
				self.save_settings();
				Response::None
			}
			Event::SetTheme(theme) => {
				self.settings.theme = theme;
				self.save_settings();
				Response::None
			}
			Event::SetPreviewQuality(q) => {
				let changed = self.settings.preview_quality() != q.validated();
				self.settings.set_preview_quality(q);
				self.settings_changed(changed, now);
				Response::None
			}
			Event::SetDitherBits(bits) => {
				self.settings.preview_dither_bits = bits;
				self.settings.normalize();
				self.save_settings();
				Response::None
			}
			Event::SetAutoRerun(on) => {
				self.settings.auto_rerun = on;
				self.settings_changed(false, now);
				Response::None
			}
			Event::SetUseGpu(on) => {
				let changed = self.settings.use_gpu != on;
				self.settings.use_gpu = on;
				self.settings_changed(false, now);
				if changed {
					self.live.backend_mut().engine_changed();
				}
				Response::None
			}
			Event::SetFrameMemoryPercent(pct) => {
				self.settings.frame_memory_percent = pct;
				self.settings.normalize();
				self.settings_changed(false, now);
				Response::None
			}
			Event::SetWarnWhenRamInsufficient(on) => {
				self.settings.warn_when_ram_insufficient = on;
				self.settings_changed(false, now);
				Response::None
			}
			Event::SetTitleFullPath(on) => {
				self.settings.title_full_path = on;
				self.save_settings();
				Response::None
			}
			Event::SetCheckForUpdates(on) => {
				self.settings.check_for_updates = Some(on);
				self.save_settings();
				Response::None
			}
			Event::SetUseOpenH264(on) => {
				self.settings.use_openh264 = Some(on);
				self.save_settings();
				Response::None
			}
			Event::SetPreviewOrientation(o) => {
				self.settings.preview_orientation = o;
				self.save_settings();
				Response::None
			}
		}
	}

	fn live_start(
		&mut self,
		start: impl FnOnce(&mut LiveController<B>, &wavedude_core::model::Project, Instant) -> bool,
		now: Instant,
	) -> Response {
		start(&mut self.live, &self.document.project, now);
		if self.live.warning().is_some() { Response::MemoryWarning } else { Response::None }
	}

	/// The path an export's Save dialog starts at: the session's last export of the kind, else the project's
	/// folder under its name (`<stem>_field` for a run), else the last export folder.
	pub fn export_proposal(&self, kind: ExportKind) -> PathBuf {
		let hint = match &self.document.path {
			Some(p) => self.settings.export_hint(p).cloned().unwrap_or_default(),
			None => self.session_hint.clone(),
		};
		let last = match kind {
			ExportKind::Run => hint.run,
			ExportKind::Still => hint.still,
			ExportKind::Probe => hint.probe,
		};
		if let Some(last) = last {
			return last;
		}
		let named = self.document.path.as_deref().or(self.document.suggested_file_name.as_deref());
		let stem = named.and_then(Path::file_stem).map(|s| s.to_string_lossy().into_owned());
		let folder =
			self.document.folder().map(Path::to_owned).unwrap_or_else(|| self.settings.last_export_folder.clone());
		let name = match (kind, stem) {
			(ExportKind::Run, Some(s)) => format!("{s}_field"),
			(_, Some(s)) => s,
			(_, None) => String::new(),
		};
		folder.join(name)
	}

	/// Records where an export went, for the next export's start folder.
	pub fn export_done(&mut self, kind: ExportKind, path: &Path) {
		let mut hint = match &self.document.path {
			Some(p) => self
				.settings
				.export_hint(p)
				.cloned()
				.unwrap_or(ExportHint { project: p.clone(), ..ExportHint::default() }),
			None => self.session_hint.clone(),
		};
		let slot = match kind {
			ExportKind::Run => &mut hint.run,
			ExportKind::Still => &mut hint.still,
			ExportKind::Probe => &mut hint.probe,
		};
		*slot = Some(path.to_owned());
		if let Some(folder) = path.parent() {
			self.settings.last_export_folder = folder.to_owned();
		}
		if self.document.path.is_some() {
			self.settings.set_export_hint(hint);
		} else {
			self.session_hint = hint;
		}
		self.save_settings();
	}

	/// Selection helper for the pictures: a click on empty space.
	pub fn deselect(&mut self) {
		self.selection.clear();
	}

	/// See the field; read with the document topic.
	pub fn reveals(&self) -> u64 {
		self.reveals
	}

	/// The row the properties pane shows.
	pub fn current_row(&self) -> Option<Row> {
		self.selection.current()
	}
}

fn is_scheme(path: &Path) -> bool {
	path.extension().is_some_and(|e| e.eq_ignore_ascii_case(wavedude_core::io::wdscheme::EXTENSION))
}
