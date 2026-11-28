//! The application's headless state: the open document with its history, the selection, the live controller
//! with playback, the user settings, the strings and the log. The views send [`Event`]s and call
//! [`AppState::tick`] about 60 times a second; everything here is testable without a window.
//!
//! Beside it, the windows' logic without a UI toolkit: menus and their actions, forms, the preview's models, exports,
//! dialogs, the settings table, messages and the updater. File dialogs and the updater are native only.
//!
//! Threads: `AppState` lives on the UI thread; natively the live run works on a thread of its own and is polled,
//! on wasm32 it works in the browser's run worker and each tick takes what the worker sent ([`remote`]).

pub mod about;
pub mod clipboard;
#[cfg(not(target_arch = "wasm32"))]
pub mod codec;
pub mod commands;
pub mod dialogs;
pub mod document;
#[cfg(not(target_arch = "wasm32"))]
pub mod downloads;
pub mod export;
pub mod form;
pub mod gradient;
pub mod history;
pub mod jobs;
pub mod live;
pub mod log;
pub mod memory_warning;
pub mod menus;
pub mod messages;
pub mod modal;
pub mod number;
pub mod open_requests;
pub mod outline;
pub mod platform;
pub mod playback;
pub mod preview;
pub mod properties;
pub mod remote;
pub mod selection;
pub mod settings;
pub mod settings_table;
pub mod shell;
pub mod spill;
pub mod status;
pub mod strings;
#[cfg(not(target_arch = "wasm32"))]
pub mod update;
pub mod user_schemes;
pub mod wake;
pub mod window;

use std::path::{Path, PathBuf};

use waveduderust::io::schemes::SchemeStore;
use waveduderust::model::{ItemRef, Point, SchemeId};
use waveduderust::quality::{MachineFacts, PreviewQuality};
use waveduderust::run::{Levers, RamCache};
use waveduderust::unstable::engine::EngineChoice;

use clipboard::{Copyable, PasteTarget};
use commands::{Change, ChangeKind, Command, CommandError};
use document::{Document, DocumentError, Opened, Source, Target};
use live::{Backend, CoreBackend, LiveController, LiveSettings, SharedEngine};
use platform::{Instant, Store};
use selection::{Row, Selection};
use settings::{Engine, ExportHint, Settings, Theme};
use strings::{Language, Strings};

/// This build: the version and, after a `+`, the build `tools/build-web.sh` names in `WAVEDUDE_BUILD` (its commit),
/// so the browser's page and run worker of one build tell each other apart from those of another.
pub fn build_id() -> String {
	match option_env!("WAVEDUDE_BUILD") {
		Some(build) => format!("{}+{build}", env!("CARGO_PKG_VERSION")),
		None => env!("CARGO_PKG_VERSION").to_owned(),
	}
}

/// An action that replaces the open project, asked about first when there are unsaved edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Guarded {
	New,
	Open(Source),
	OpenExample(usize),
	Close,
	Exit,
	/// Exit to install a downloaded update and start it.
	Restart,
}

/// The answer to "Save the changes to ...?".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
	DropFile(Source),
	Save,
	SaveAs(Target),
	/// The Save As dialog was cancelled.
	SaveAsCancelled,
	SaveChoice(SaveChoice),
	/// The answer to [`Document::scene_image_offer`]: true takes the file's image, false keeps the embedded one.
	SceneImageOffer(bool),
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
	/// Drops the run's frames; the project and its history stay.
	DiscardFrames,
	Play,
	PausePlayback,
	/// Play while paused, else Pause playback.
	TogglePlayback,
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
	/// The engine of the live runs, one of `EngineChoice::available`.
	SetEngine(EngineChoice),
	SetRamCache(RamCache),
	SetWarnWhenRamInsufficient(bool),
	SetTitleFullPath(bool),
	/// The user's answer whether MP4 export may use Cisco's `OpenH264`.
	SetUseOpenH264(bool),
	/// The H.264 profile of MP4 export.
	SetH264Profile(waveduderust::unstable::export::h264::Profile),
	/// Whether a run export writing several files archives them.
	SetExportArchive(bool),
	/// Whether to look for a new release at launch.
	SetCheckForUpdates(bool),
	/// Whether the welcome page opens at each launch.
	SetShowStartup(bool),
	/// The steps that can be undone; the oldest beyond them go as the next step is recorded.
	SetUndoSteps(u32),
	SetPreviewOrientation(settings::Orientation),
	/// The dock's arrangement, as [`Settings::layout`](settings::Settings::layout).
	SetLayout(String),
	/// Keeps the project's colour scheme among the user's, or removes the user's of its name.
	ToggleUserScheme(SchemeId),
	/// Removes the user's colour scheme of this name.
	RemoveUserScheme(String),
}

/// What the views must do after an event.
#[derive(Clone, Debug, PartialEq)]
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
	/// Put this text on the clipboard (app format and plain text), then apply `then` (a cut's removal).
	Clipboard {
		text: String,
		then: Option<Command>,
	},
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
	pub selection: u64,
	/// The position, the frame shown, the frame count.
	pub playback: u64,
	/// A run's progress: its steps done and the frames it published; moves every tick of a run.
	pub progress: u64,
	/// The live run's work: stepping or not, and towards which frame.
	pub work: u64,
	/// The slower live state: the run's state, playing, loop, speed, range, follow.
	pub run: u64,
	/// The user settings, the language among them, and the user's colour schemes.
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
	(Option<(waveduderust::run::RunState, u32)>, (bool, bool, bool, bool), (bool, u64, bool, (u32, u32), bool));

/// See the module docs.
pub struct AppState<B: Backend> {
	pub settings: Settings,
	settings_store: Option<Store>,
	pub strings: Strings,
	pub document: Document,
	pub selection: Selection,
	pub live: LiveController<B>,
	/// The user's colour schemes beside the project's and the built-in ones.
	pub schemes: SchemeStore,
	/// No settings file existed at the start: the welcome page shows.
	pub first_start: bool,
	/// Cisco's `OpenH264` folder (`AppDirs::codec`); `None` without one.
	pub codec_dir: Option<PathBuf>,
	revisions: Revisions,
	observed: Option<Observed>,
	/// Bumped when the tree is to give the current row the keyboard focus: an opened project, a remove, a move,
	/// an undo that left nothing selected. It moves with the document revision.
	reveals: u64,
	/// Bumped when another document replaces the open one (New, Open, Close); a save keeps it.
	documents: u64,
	engine: SharedEngine,
	pending: Option<Guarded>,
	/// The export hint of a project without a file: this session's alone.
	session_hint: ExportHint,
}

/// The application state with the core's engines.
pub type App = AppState<CoreBackend>;

fn live_settings(s: &Settings) -> LiveSettings {
	LiveSettings {
		auto_rerun: s.auto_rerun,
		warn_when_ram_insufficient: s.warn_when_ram_insufficient,
		quality: s.preview_quality(),
		ram_cache: s.ram_cache(),
	}
}

impl AppState<CoreBackend> {
	/// The app's state on the core's engines: settings from `dirs` (the first start chooses the preview
	/// quality from the machine), the empty project, the language and zoom of the settings and `args`.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn start<'a>(dirs: &settings::AppDirs, args: impl IntoIterator<Item = &'a str>) -> std::io::Result<Self> {
		Self::start_with_memory(dirs, args, waveduderust::run::Memory::System)
	}

	/// As [`start`](Self::start), the frame budgets on `memory`'s readings.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn start_with_memory<'a>(
		dirs: &settings::AppDirs,
		args: impl IntoIterator<Item = &'a str>,
		memory: waveduderust::run::Memory,
	) -> std::io::Result<Self> {
		let store = dirs.store();
		let touch_first = platform::touch_first();
		let loaded =
			load_settings(&store, touch_first, || Some(waveduderust::unstable::engine::gpu::context().is_ok()), args);
		let engine = SharedEngine::new(loaded.settings.engine.choice());
		let backend = CoreBackend::new(engine.clone(), dirs.cache.join("frames"), memory)?;
		let mut state = Self::started(loaded, store, backend, engine);
		state.codec_dir = Some(dirs.codec());
		Ok(state)
	}

	/// The app's state in the browser: settings from `store` (the first start chooses the preview quality from
	/// the device, its GPU unknown until the run worker started), the run worker started, the empty project, the
	/// language and zoom of the settings and `args`.
	#[cfg(target_arch = "wasm32")]
	pub fn start<'a>(store: Store, args: impl IntoIterator<Item = &'a str>) -> Self {
		let touch_first = platform::touch_first();
		let loaded = load_settings(&store, touch_first, || None, args);
		let engine = SharedEngine::new(loaded.settings.engine.choice());
		let backend = CoreBackend::new(engine.clone());
		Self::started(loaded, store, backend, engine)
	}

	fn started(loaded: settings::Loaded, store: Store, backend: CoreBackend, engine: SharedEngine) -> Self {
		let mut state = Self::with_backend(loaded.settings, Some(store.clone()), backend, engine);
		state.schemes = user_schemes::load(&store);
		state.first_start = loaded.first_start;
		state
	}
}

/// The settings of `store`, the first start's chosen from the machine with `has_gpu` (asked then alone; `None`
/// when unknown), at the zoom of `args`; `touch_first` sets the device's defaults.
fn load_settings<'a>(
	store: &Store,
	touch_first: bool,
	has_gpu: impl FnOnce() -> Option<bool>,
	args: impl IntoIterator<Item = &'a str>,
) -> settings::Loaded {
	let facts = || MachineFacts { has_gpu: has_gpu(), ..platform::machine_facts() };
	let mut loaded = Settings::load(store, touch_first, facts);
	if let Some(zoom) = settings::zoom_switch(args) {
		loaded.settings.ui_zoom = zoom;
	}
	loaded
}

impl<B: Backend> AppState<B> {
	/// The state on `backend`, which builds the engine `engine` names; `settings_store` is where setting changes are
	/// saved (`None`: not saved).
	pub fn with_backend(settings: Settings, settings_store: Option<Store>, backend: B, engine: SharedEngine) -> Self {
		let strings = Strings::new(Language::resolve(&settings.language));
		let mut document = Document::empty();
		document.history.set_limit(settings.undo_steps as usize);
		let mut live = LiveController::new(backend, live_settings(&settings));
		live.project_replaced(&document.project, &document.view, Instant::now());
		let mut state = Self {
			settings,
			settings_store,
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
			documents: 0,
			engine,
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

	/// The project changed in a way the cheap observation misses.
	fn document_changed(&mut self) {
		self.revisions.document += 1;
	}

	/// The window title.
	pub fn title(&self) -> String {
		self.document.title(self.strings.get("Title"), self.strings.get("Untitled"), self.settings.title_full_path)
	}

	/// The status bar's run line: `<State>: <done> / <total> steps, <engine>[, edited since]`; empty without a run.
	pub fn run_status(&self) -> String {
		let Some((state, done, total)) = self.live.run_status() else {
			return String::new();
		};
		let key = match state {
			waveduderust::run::RunState::Running => "RunStateRunning",
			waveduderust::run::RunState::Paused => "RunStatePaused",
			waveduderust::run::RunState::Completed => "RunStateCompleted",
			waveduderust::run::RunState::Cancelled => "RunStateCancelled",
			waveduderust::run::RunState::Failed(_) => "RunStateFailed",
		};
		let s = &self.strings;
		s.format("RunStatusFormat", &[&s.get(key), &done, &total, &self.live.engine().text(s)])
	}

	/// Advances the history's quiet time and the live controller to `now`; true when something a view shows
	/// changed (a frame, the position, a run's state, a history step), so the views can skip a render on quiet
	/// ticks.
	pub fn tick(&mut self, now: Instant) -> bool {
		self.document.tick(now);
		let offered = self.document.follow_image_file(now);
		self.live.tick(now, &self.document.project, &self.document.view);
		self.sync_revisions() || offered
	}

	fn save_settings(&self) {
		if let Some(store) = &self.settings_store
			&& let Err(e) = self.settings.save(store)
		{
			tracing::warn!("{e}");
		}
	}

	fn settings_changed(&mut self, preview_changed: bool, now: Instant) {
		self.engine.set(self.settings.engine.choice());
		self.live.set_settings(live_settings(&self.settings));
		if preview_changed {
			let (project, view) = (&self.document.project, &self.document.view);
			self.live.on_change(ChangeKind::Simulation, project, view, Some("preview quality".into()), now);
		}
		self.save_settings();
	}

	fn record(&mut self, change: Change, now: Instant) {
		if change.kind != ChangeKind::None {
			self.document_changed();
			self.document.edited(now);
			if change.own_step {
				self.document.commit_step();
			}
		}
		self.live.on_change(change.kind, &self.document.project, &self.document.view, change.note, now);
	}

	/// Applies a model command as one edit.
	pub fn apply(&mut self, command: Command, now: Instant) -> Result<ChangeKind, CommandError> {
		let reveal = matches!(command, Command::Remove { .. } | Command::Reorder { .. });
		let d = &mut self.document;
		let change = commands::apply(&mut d.project, &mut d.view, &mut self.selection, command)?;
		let kind = change.kind;
		self.record(change, now);
		self.reveals += u64::from(reveal);
		Ok(kind)
	}

	fn replace_document(&mut self, document: Document, now: Instant) {
		if let Some(path) = &document.path {
			self.settings.push_recent(path);
			self.save_settings();
		}
		self.document = document;
		self.document.history.set_limit(self.settings.undo_steps as usize);
		self.schemes.check(&self.document.project.color_schemes, &self.document.view);
		self.documents += 1;
		self.document_changed();
		self.selection = Selection::root();
		self.reveals += 1;
		self.session_hint = ExportHint::default();
		self.live.project_replaced(&self.document.project, &self.document.view, now);
	}

	fn perform(&mut self, action: Guarded, now: Instant) -> Response {
		match action {
			Guarded::New => self.replace_document(Document::new_project(), now),
			Guarded::Close => self.replace_document(Document::empty(), now),
			Guarded::Exit => return Response::Exit,
			Guarded::Restart => return Response::Restart,
			Guarded::OpenExample(i) => {
				let example = &waveduderust_examples::all()[i];
				match Document::from_example(example) {
					Ok(d) => self.replace_document(d, now),
					Err(e) => return self.error("OpenError", example.file_name, &e),
				}
			}
			Guarded::Open(source) => {
				let opened = match &source {
					Source::Path(path) => Document::open(path),
					Source::Bytes { name, bytes } => Document::from_bytes(name, bytes, None),
				};
				match opened {
					Ok(Opened::Project(d)) => self.replace_document(*d, now),
					Ok(Opened::Scheme(scheme)) => self.add_scheme(scheme, now),
					Err(e) => return self.error("OpenError", &source.label(), &e),
				}
			}
		}
		Response::None
	}

	fn add_scheme(&mut self, scheme: waveduderust::model::ColorScheme, now: Instant) {
		let i = self.document.project.add_color_scheme(scheme);
		self.selection.set_items(vec![ItemRef::Scheme(i)]);
		let mut change = Change { kind: ChangeKind::Display, own_step: true, note: None };
		let name = ItemRef::Scheme(i).name(&self.document.project).unwrap_or_default();
		change.note = Some(format!("colour scheme {name} added"));
		self.record(change, now);
	}

	fn error(&self, key: &str, what: &str, e: &DocumentError) -> Response {
		match e {
			DocumentError::Io(waveduderust::io::IoError::Unreadable(u)) => tracing::error!("{}", u.report()),
			e => tracing::error!("{e}"),
		}
		Response::Error(self.strings.format(key, &[&what]))
	}

	/// An action that replaces the project: asked about first when there are unsaved edits.
	fn guarded(&mut self, action: Guarded, now: Instant) -> Response {
		let replaces = matches!(action, Guarded::New | Guarded::Close | Guarded::Open(_) | Guarded::OpenExample(_));
		if replaces && self.live.export_busy() && !matches!(&action, Guarded::Open(s) if is_scheme(s)) {
			tracing::warn!("The project stays while an export runs; finish or cancel the export first.");
			return Response::None;
		}
		if self.document.dirty && !matches!(&action, Guarded::Open(s) if is_scheme(s)) {
			let name = self.document.display_name(self.strings.get("Untitled"));
			self.pending = Some(action);
			return Response::AskSave { name };
		}
		self.perform(action, now)
	}

	fn save_then_continue(&mut self, target: Option<Target>, now: Instant) -> Response {
		let result = match &target {
			Some(t) => self.document.save_to(t),
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
				let name = target
					.map(|t| t.label())
					.or_else(|| self.document.path.as_ref().map(|p| p.display().to_string()))
					.expect("a save without a path is NoPath");
				self.error("SaveError", &name, &e)
			}
		}
	}

	fn copy_text(&self) -> Option<String> {
		let mut objects: Vec<Copyable> = self.selection.items().iter().map(|&i| Copyable::Item(i)).collect();
		if objects.is_empty() {
			objects.extend(self.selection.current().and_then(Copyable::of_row));
		}
		(!objects.is_empty()).then(|| clipboard::write_fragment(&self.document.project, &self.document.view, &objects))
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
				let (before, before_view) = (self.document.project.clone(), self.document.view.clone());
				let done = if event == Event::Undo { self.document.undo() } else { self.document.redo() };
				if done {
					self.document_changed();
					self.selection = self.selection.follow(&before, &self.document.project);
					if self.selection.current().is_none() {
						self.selection = Selection::root();
						self.reveals += 1;
					}
					let (project, view) = (&self.document.project, &self.document.view);
					let kind = commands::classify(&before, &before_view, project, view);
					self.live.on_change(kind, project, view, Some("undo or redo".into()), now);
				}
				Response::None
			}
			Event::Guarded(action) => self.guarded(action, now),
			Event::DropFile(source) => {
				if ["wdproj", "json", "xml", "wdscheme"].contains(&source.extension().as_str()) {
					self.guarded(Guarded::Open(source), now)
				} else {
					let label = source.label();
					tracing::warn!("'{label}' is not a project or colour scheme file");
					Response::Error(self.strings.format("OpenError", &[&label]))
				}
			}
			Event::Save => self.save_then_continue(None, now),
			Event::SaveAs(target) => self.save_then_continue(Some(target), now),
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
			Event::SceneImageOffer(take) => {
				if self.document.answer_scene_image_offer(take, now) {
					self.document_changed();
					let note = Some("the scene image file was taken".into());
					self.live.on_change(ChangeKind::Simulation, &self.document.project, &self.document.view, note, now);
				}
				Response::None
			}
			Event::Copy => self.copy_text().map_or(Response::None, |text| Response::Clipboard { text, then: None }),
			Event::Cut { target } => {
				let targets = self.selection.targets(target, &self.document.project);
				let objects: Vec<Copyable> = targets.iter().map(|&i| Copyable::Item(i)).collect();
				let text = clipboard::write_fragment(&self.document.project, &self.document.view, &objects);
				Response::Clipboard { text, then: Some(Command::Remove { target }) }
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
			Event::Run => self.live_start(LiveController::run_user, now),
			Event::PausePlayback => {
				self.live.pause_playback(now);
				Response::None
			}
			Event::TogglePlayback if self.live.playback.is_playing() => {
				self.live.pause_playback(now);
				Response::None
			}
			Event::Play | Event::TogglePlayback => self.live_start(
				|live, p, v, now| {
					live.play(p, v, now);
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
			Event::DiscardFrames => {
				self.live.discard_frames();
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
				// the file's 0 is "to the end"
				let (from, to) = (from.0, to.map_or(0, |step| step.0));
				let display = &mut self.document.view;
				if (display.export_from, display.export_to) != (from, to) {
					log::change(
						"Display.export range",
						&format!("{}..{}", display.export_from, display.export_to),
						&format!("{from}..{to}"),
					);
					(display.export_from, display.export_to) = (from, to);
					self.record(Change { kind: ChangeKind::Display, own_step: false, note: None }, now);
				}
				Response::None
			}
			Event::MemoryWarningRun { levers, mute } => {
				if let Some(choice) = self.live.confirm_warning(levers, mute, now) {
					self.settings.set_preview_quality(choice.quality);
					if choice.cache_auto {
						self.settings.set_ram_cache(RamCache::Auto);
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
			Event::SetEngine(choice) => {
				let changed = self.settings.engine.choice() != choice;
				self.settings.engine = Engine::of(choice);
				self.settings_changed(false, now);
				if changed {
					self.live.backend_mut().engine_changed();
				}
				Response::None
			}
			Event::SetRamCache(cache) => {
				self.settings.set_ram_cache(cache);
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
			Event::SetShowStartup(on) => {
				self.settings.show_startup = on;
				self.save_settings();
				Response::None
			}
			Event::SetUndoSteps(steps) => {
				self.settings.undo_steps = steps;
				self.settings.normalize();
				self.document.history.set_limit(self.settings.undo_steps as usize);
				self.save_settings();
				Response::None
			}
			Event::SetUseOpenH264(on) => {
				self.settings.use_openh264 = Some(on);
				self.save_settings();
				Response::None
			}
			Event::SetH264Profile(profile) => {
				self.settings.h264_profile = profile;
				self.save_settings();
				Response::None
			}
			Event::SetExportArchive(on) => {
				self.settings.export_archive = on;
				self.save_settings();
				Response::None
			}
			Event::SetPreviewOrientation(o) => {
				self.settings.preview_orientation = o;
				self.save_settings();
				Response::None
			}
			Event::SetLayout(layout) => {
				self.settings.layout = Some(layout);
				self.save_settings();
				Response::None
			}
			Event::ToggleUserScheme(index) => {
				let Some(scheme) = self.document.project.color_scheme(index) else { return Response::None };
				if self.is_user_scheme(&scheme.name) {
					let name = scheme.name.clone();
					self.change_user_schemes("RemoveError", |store| user_schemes::remove(store, &name))
				} else {
					let scheme = scheme.clone();
					self.change_user_schemes("SaveError", |store| user_schemes::add(store, &scheme))
				}
			}
			Event::RemoveUserScheme(name) => {
				self.change_user_schemes("RemoveError", |store| user_schemes::remove(store, &name))
			}
		}
	}

	/// Whether `name` is one of the user's colour schemes.
	pub fn is_user_scheme(&self, name: &str) -> bool {
		user_schemes::has(&self.schemes, name)
	}

	/// Applies `change` to the user's colour schemes and reads them again; its failure is the message `key` of where.
	fn change_user_schemes(&mut self, key: &str, change: impl FnOnce(&Store) -> Result<(), String>) -> Response {
		let Some(store) = &self.settings_store else {
			tracing::warn!("the user's colour schemes are not kept");
			return Response::None;
		};
		let result = change(store);
		self.schemes = user_schemes::load(store);
		self.schemes.check(&self.document.project.color_schemes, &self.document.view);
		self.revisions.settings += 1;
		match result {
			Ok(()) => Response::None,
			Err(at) => Response::Error(self.strings.format(key, &[&at])),
		}
	}

	fn live_start(
		&mut self,
		start: impl FnOnce(
			&mut LiveController<B>,
			&waveduderust::model::Project,
			&waveduderust::model::View,
			Instant,
		) -> bool,
		now: Instant,
	) -> Response {
		start(&mut self.live, &self.document.project, &self.document.view, now);
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
		let folder = self.document.folder().map_or_else(|| self.settings.last_export_folder.clone(), Path::to_owned);
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

	/// Which document is open: it changes when another replaces it, not when it is saved under a new path.
	pub fn document_id(&self) -> u64 {
		self.documents
	}

	/// The row the properties pane shows.
	pub fn current_row(&self) -> Option<Row> {
		self.selection.current()
	}
}

fn is_scheme(source: &Source) -> bool {
	source.extension() == waveduderust::io::wdscheme::EXTENSION
}
