//! The live controller: what a model change, the transport and the export ask of the live run, and what the
//! pictures show (docs/ui/60-live-editing-and-runs.md, 50-timeline-transport.md).
//!
//! No timers: the controller publishes a [`Goal`] (the project's spec, the position, how much is wanted, a
//! pause, a fit) and reads back what the run has ([`Status`], the store's frames, the traces). The run sits
//! behind [`Backend`]; [`CoreBackend`] is the core's [`RunWorker`], [`fake::FakeBackend`] a scripted one for
//! tests. Threads: the controller lives on the UI thread; the run works on its own thread and is polled here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::engine::{EngineOptions, FrameShape, SceneData, Snapshot, WaveEngine};
use wavedude_core::model::{Layer, Project};
use wavedude_core::run::{
	Budget, FitFrame, FitRequest, FrameStore, Goal, Levers, MemoryWarning, PreviewQuality, RunError, RunRequest,
	RunSpecError, RunState, RunWorker, SceneCache, Status, Wanted, WorkState,
};

use super::commands::ChangeKind;
use super::playback::{Limit, Playback};

/// Where the live run is. Called on the UI thread; implementations publish across threads.
pub trait Backend {
	fn set_goal(&mut self, goal: Goal);
	fn status(&self) -> Status;
	/// Calls `f` with the probe traces recorded so far, in the spec's probe order.
	fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace]));
	/// Calls `f` with the traces that go with the frame kept through the last clear (empty when none fit).
	fn with_held_traces(&self, f: &mut dyn FnMut(&[ProbeTrace]));
	fn take_fit(&mut self) -> Option<FitFrame>;
	/// The engine's device changed: the next work builds the engine anew.
	fn engine_changed(&mut self);
	/// The store budget at `ram_percent` and at Auto, counting `reclaimed` bytes as free.
	fn budgets(&self, ram_percent: u32, reclaimed: u64) -> (u64, u64);
	/// "CPU (16 threads)" or "GPU: <name>".
	fn engine_name(&self) -> String;
}

/// The user settings the controller follows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveSettings {
	pub auto_rerun: bool,
	pub warn_when_ram_insufficient: bool,
	pub quality: PreviewQuality,
	/// 0 = Auto.
	pub ram_percent: u32,
}

impl Default for LiveSettings {
	fn default() -> Self {
		Self { auto_rerun: true, warn_when_ram_insufficient: true, quality: PreviewQuality::DEFAULT, ram_percent: 0 }
	}
}

/// What the UI should do or show; drained with [`LiveController::take_notices`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
	/// Show the frame at this timeline position: the store has it now.
	ShowFrame(u32),
	/// The memory warning asks ([`LiveController::warning`]).
	AskMemoryWarning,
	/// The export's run is complete and fresh.
	ExportReady,
	/// The waiting export's run was replaced or stopped: the export is dropped.
	ExportStopped,
	Info(String),
	Error(String),
	/// The memory warning asked is void: its edit was replaced; its dialog closes.
	WarningDropped,
	/// A run could not start or failed: `no_image` when the scene's image is missing.
	RunFailed {
		message: String,
		no_image: bool,
	},
}

/// The choice the memory warning's Run applies to the settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WarningChoice {
	pub quality: PreviewQuality,
	/// The RAM cache size to Auto.
	pub cache_auto: bool,
	/// "Do not warn me again".
	pub mute: bool,
}

/// The export's view of the current run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportGate {
	/// The run is fresh, complete and full quality: export now (the run is held until `end_export`).
	Ready,
	/// A full-quality run was started; [`Notice::ExportReady`] follows.
	Waiting,
	/// A memory or start failure; nothing to export.
	Failed,
	/// Another export holds or waits for the run; nothing changed.
	Busy,
}

/// What asked for every frame, applied once the memory warning lets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Then {
	/// The re-run after an edit: nothing else moves.
	Rerun,
	/// F5: from frame 0, followed.
	Run,
	/// Play: plays as the frames arrive.
	Play,
}

struct PendingWarning {
	budget: u64,
	auto_budget: u64,
	then: Then,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Export {
	Idle,
	Waiting { needs_intensity: bool },
	Reading,
}

/// See the module docs.
pub struct LiveController<B: Backend> {
	backend: B,
	settings: LiveSettings,
	pub playback: Playback,
	/// Moves with every spec published.
	revision: u64,
	/// The request of each revision from the adopted one on; the last is this project state's.
	requests: Vec<(u64, RunRequest)>,
	/// The quality in force: the setting's, or Full for an export until the next simulation edit.
	quality: PreviewQuality,
	/// The export's intensity, tracked until the next simulation edit.
	track_intensity: bool,
	wanted: Wanted,
	/// Every frame was asked for at least once for this project: the status bar has a run to describe.
	ran: bool,
	user_paused: bool,
	live: bool,
	/// The timeline frame the pictures show.
	shown: Option<u32>,
	warning: Option<PendingWarning>,
	export: Export,
	fit: Option<FitRequest>,
	fit_frames: Option<FitFrame>,
	next_fit: u64,
	last_edit: Option<String>,
	/// The failure reported last.
	failed: Option<Arc<RunError>>,
	/// An edit arrived while an export read the frames; applied when the export ends.
	edited_during_export: Option<ChangeKind>,
	published: Option<Goal>,
	/// The first tick came: the UI is up, so the run may build its engine (a GPU context made beside the
	/// window's own graphics start crashes the driver).
	started: bool,
	/// When every frame was last asked for; cleared once the run's end is reported.
	run_started: Option<Instant>,
	status: Status,
	notices: Vec<Notice>,
}

/// The frame the pictures show.
#[derive(Clone)]
pub enum ShownSource {
	/// Stored frame `stored` of the run's store.
	Stored(Arc<FrameStore>, usize),
	/// The frame the store kept through its last clear ([`FrameStore::latest`]), of `generation`.
	/// The frame made last for the position, of the project state before the edits in flight.
	Snapshot { store: Arc<FrameStore>, generation: u64, snapshot: Arc<Snapshot> },
}

impl<B: Backend> LiveController<B> {
	pub fn new(backend: B, settings: LiveSettings) -> Self {
		let status = backend.status();
		Self {
			backend,
			settings,
			playback: Playback::default(),
			revision: 0,
			requests: Vec::new(),
			quality: settings.quality,
			track_intensity: false,
			wanted: Wanted::Position,
			ran: false,
			user_paused: false,
			live: false,
			shown: None,
			warning: None,
			export: Export::Idle,
			fit: None,
			fit_frames: None,
			next_fit: 0,
			last_edit: None,
			failed: None,
			edited_during_export: None,
			published: None,
			started: false,
			run_started: None,
			status,
			notices: Vec::new(),
		}
	}

	pub fn backend(&self) -> &B {
		&self.backend
	}

	pub fn backend_mut(&mut self) -> &mut B {
		&mut self.backend
	}

	pub fn settings(&self) -> LiveSettings {
		self.settings
	}

	/// New settings; a changed preview quality is a simulation edit, published by the caller through
	/// [`on_change`](Self::on_change).
	pub fn set_settings(&mut self, settings: LiveSettings) {
		self.settings = settings;
		self.publish();
	}

	pub fn take_notices(&mut self) -> Vec<Notice> {
		std::mem::take(&mut self.notices)
	}

	/// The timeline frame the pictures show; `None` before the first frame of a project.
	pub fn shown(&self) -> Option<u32> {
		self.shown
	}

	/// The frames of the spec the run holds: the newest one it adopted, which trails this project state by the
	/// edits still in flight during a burst. The pictures, the band and the graphs show them as they land; what
	/// needs this project state exactly asks [`is_current`](Self::is_current) too.
	fn current_store(&self) -> Option<&Arc<FrameStore>> {
		self.adopted_request()?;
		self.status.store.as_ref()
	}

	/// The request of this project state.
	fn request(&self) -> Option<&RunRequest> {
		self.requests.last().map(|(_, r)| r)
	}

	/// The request whose frames the store holds.
	fn adopted_request(&self) -> Option<&RunRequest> {
		self.requests.iter().find(|(revision, _)| *revision == self.status.adopted).map(|(_, r)| r)
	}

	/// The run holds this project state's spec.
	fn is_current(&self) -> bool {
		self.status.adopted == self.revision && self.request().is_some()
	}

	/// The store's index of timeline frame `frame`.
	fn stored_index(&self, frame: u32) -> Option<usize> {
		self.adopted_request().map(|r| r.schedule().stored_index_of(frame) as usize)
	}

	fn store_has(&self, frame: u32) -> bool {
		match (self.current_store(), self.stored_index(frame)) {
			(Some(store), Some(index)) => store.has(index),
			_ => false,
		}
	}

	/// The newest timeline frame of the present run that holds `frame`.
	fn present_through(&self, frame: u32) -> Option<u32> {
		let (store, index) = (self.current_store()?, self.stored_index(frame)?);
		let schedule = self.adopted_request()?.schedule();
		let last = store.present_through(index)? as u32;
		Some(schedule.covered_frames(last + 1).saturating_sub(1).max(frame))
	}

	/// Every frame of this project state is stored and the run has nothing left to do.
	fn complete(&self) -> bool {
		self.is_current()
			&& self.current_store().is_some_and(|s| s.is_complete())
			&& matches!(self.status.state, WorkState::Idle | WorkState::Fitting)
	}

	/// The frame shown, for the presenter: the position's stored frame, else the frame made last for the
	/// position (the state before the edits in flight, so a drag shows every frame made).
	pub fn shown_source(&self) -> Option<ShownSource> {
		let frame = self.shown?;
		let (store, index) = (self.current_store()?, self.stored_index(frame)?);
		if store.has(index) {
			return Some(ShownSource::Stored(store.clone(), index));
		}
		let (_, generation, snapshot) = store.latest().filter(|(i, ..)| *i == index)?;
		Some(ShownSource::Snapshot { store: store.clone(), generation, snapshot })
	}

	/// Calls `f` with the shown frame's shape and its plane of `layer`; `None` without a shown frame or such a plane.
	pub fn with_shown_plane<R>(&self, layer: Layer, f: impl FnOnce(&FrameShape, &[u8]) -> R) -> Option<R> {
		match self.shown_source()? {
			ShownSource::Stored(store, index) => store.with_layer(index, layer, |plane| f(store.shape(), plane)).ok(),
			ShownSource::Snapshot { snapshot: s, .. } => {
				let plane = s.plane(layer);
				(!plane.is_empty()).then(|| f(&s.shape, plane))
			}
		}
	}

	/// The run is stepping (towards any frame or a fit).
	pub fn is_working(&self) -> bool {
		matches!(self.status.state, WorkState::Stepping { .. } | WorkState::Fitting)
	}

	/// The timeline frame the run is stepping towards.
	pub fn computing_frame(&self) -> Option<u32> {
		let (WorkState::Stepping { target }, Some(request)) = (&self.status.state, self.adopted_request()) else {
			return None;
		};
		let schedule = request.schedule();
		let position = self.playback.position();
		Some(if schedule.stored_index_of(position) == *target { position } else { target * schedule.frame_divisor() })
	}

	/// The position, while the run is stepping towards its frame and the pictures wait for it.
	pub fn computing_picture(&self) -> Option<u32> {
		let position = self.playback.position();
		let target = self.stored_index(position)?;
		match self.status.state {
			WorkState::Stepping { target: t } if t as usize == target && !self.store_has(position) => Some(position),
			_ => None,
		}
	}

	/// Calls `f` with the traces of the frame shown: the run's while the position's frame is stored, the
	/// previous generation's while its frame is still shown (so the graphs hold with the picture), else none.
	pub fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		match self.shown_source() {
			Some(ShownSource::Stored(..)) => self.backend.with_traces(f),
			Some(ShownSource::Snapshot { .. }) => self.backend.with_held_traces(f),
			None => f(&[]),
		}
	}

	/// Live follow of a run.
	pub fn is_live(&self) -> bool {
		self.live
	}

	/// Every frame is wanted and not every frame is there yet: a run is in progress (paused or not).
	pub fn run_is_active(&self) -> bool {
		self.wanted == Wanted::All
			&& !self.complete()
			&& self.warning.is_none()
			&& !matches!(self.status.state, WorkState::Failed(_))
	}

	/// Whether frames keep arriving: a run is active and the user did not pause it.
	pub fn run_is_progressing(&self) -> bool {
		self.run_is_active() && !self.user_paused
	}

	pub fn run_paused_by_user(&self) -> bool {
		self.user_paused
	}

	/// (state, steps done, total steps) of the run, for the status bar; `None` before the first run.
	pub fn run_status(&self) -> Option<(RunState, u32, u32)> {
		let steps = self.request()?.schedule().steps;
		if !self.ran {
			return None;
		}
		let state = match &self.status.state {
			WorkState::Failed(e) => RunState::Failed(e.clone()),
			WorkState::Paused => RunState::Paused,
			WorkState::Stepping { .. } => RunState::Running,
			WorkState::Idle | WorkState::Fitting if self.complete() => RunState::Completed,
			// Every frame wanted and the run not yet at it: between the goal and the worker's wake-up.
			WorkState::Idle | WorkState::Fitting if self.wanted == Wanted::All => RunState::Running,
			WorkState::Idle | WorkState::Fitting => RunState::Cancelled,
		};
		let done = if state == RunState::Completed { steps } else { self.status.steps_done.min(steps) };
		Some((state, done, steps))
	}

	/// Logs the summary of a run that completed or was stopped since it started, once.
	fn report_run_end(&mut self, now: Instant) {
		let Some(started) = self.run_started else { return };
		match self.run_status() {
			Some((RunState::Completed | RunState::Cancelled, ..)) => {
				self.run_started = None;
				if let Some(line) = self.run_summary(started, now) {
					tracing::info!("{line}");
				}
			}
			Some((RunState::Failed(_), ..)) => self.run_started = None,
			_ => {}
		}
	}

	/// "Run <state>: <done> of <steps> steps, ..." for a run started at `started` and ended by `now`; `None`
	/// while it is under way or without one.
	fn run_summary(&self, started: Instant, now: Instant) -> Option<String> {
		let (state, done, steps) = self.run_status()?;
		let word = match state {
			RunState::Completed => "Completed",
			RunState::Cancelled => "Cancelled",
			_ => return None,
		};
		let (store, request) = (self.current_store()?, self.adopted_request()?);
		let frames = request.schedule().covered_frames(store.present_count() as u32);
		let (in_memory, on_disk) = (store.frames_in_memory(), store.frames_on_disk());
		let gb = |n: usize| (n * store.frame_bytes()) as f64 / f64::from(1u32 << 30);
		Some(format!(
			"Run {word}: {done} of {steps} steps, {frames} frames, {:.1} s; {} stored, {in_memory} in memory ({:.1} GB) and {on_disk} on disk ({:.1} GB)",
			now.saturating_duration_since(started).as_secs_f64(),
			in_memory + on_disk,
			gb(in_memory),
			gb(on_disk),
		))
	}

	/// What the run is made from: the project state, quality and tracking the worker builds its spec from.
	pub fn run_request(&self) -> Option<&RunRequest> {
		self.request()
	}

	/// The frames of this project state, for the export.
	pub fn run_store(&self) -> Option<Arc<FrameStore>> {
		self.current_store().cloned()
	}

	pub fn engine_name(&self) -> String {
		self.backend.engine_name()
	}

	/// The memory warning waiting for an answer.
	pub fn warning(&self) -> Option<MemoryWarning<'_>> {
		let request = self.request()?;
		self.warning.as_ref().map(|w| MemoryWarning { request, budget: w.budget, auto_budget: w.auto_budget })
	}

	/// Keeps the timeline in step with the project (planned frames, steps per frame, frame rate, range).
	pub fn sync_timeline(&mut self, project: &Project) {
		let d = &project.display;
		self.playback.set_timeline(
			project.simulation.frame_count(),
			project.simulation.visualize_every,
			d.video.fps,
			d.export_from,
			d.export_to,
		);
	}

	/// Publishes the goal when it differs from the one published; nothing before the first tick.
	fn publish(&mut self) {
		if !self.started {
			return;
		}
		let fit = self.fit;
		let position = self.playback.position();
		let same = self.published.as_ref().is_some_and(|p| {
			(p.revision, p.position, p.wanted, p.paused, p.fit, p.ram_percent)
				== (self.revision, position, self.wanted, self.user_paused, fit, self.settings.ram_percent)
		});
		if same {
			return;
		}
		let goal = Goal {
			revision: self.revision,
			run: self.request().cloned(),
			position,
			wanted: self.wanted,
			paused: self.user_paused,
			fit,
			ram_percent: self.settings.ram_percent,
		};
		self.backend.set_goal(goal.clone());
		self.published = Some(goal);
	}

	/// A new request for `project` at the quality in force; the worker builds the spec and reports a failure.
	fn respec(&mut self, project: &Project) {
		self.revise(RunRequest::new(project, self.quality, self.track_intensity));
	}

	/// A new revision made from `request`.
	fn revise(&mut self, request: RunRequest) {
		self.revision += 1;
		self.requests.push((self.revision, request));
	}

	/// A new, opened, imported or closed project: nothing is wanted but frame 0, which warms the engine up.
	pub fn project_replaced(&mut self, project: &Project, _now: Instant) {
		self.quality = self.settings.quality;
		self.track_intensity = false;
		self.wanted = Wanted::Position;
		self.ran = false;
		self.run_started = None;
		self.user_paused = false;
		self.live = false;
		self.shown = None;
		self.warning = None;
		self.drop_waiting_export();
		self.export = Export::Idle;
		self.edited_during_export = None;
		self.fit = None;
		self.fit_frames = None;
		self.last_edit = None;
		self.playback.pause();
		self.sync_timeline(project);
		self.playback.set_position(0);
		self.respec(project);
		self.publish();
	}

	fn drop_waiting_export(&mut self) {
		if let Export::Waiting { .. } = self.export {
			self.export = Export::Idle;
			tracing::warn!("The export was dropped: its run stopped before the last frame.");
			self.notices.push(Notice::ExportStopped);
		}
	}

	/// A model change of class `kind`; `note` names the newest edit with its value.
	pub fn on_change(&mut self, kind: ChangeKind, project: &Project, note: Option<String>, now: Instant) {
		let kind = match kind {
			ChangeKind::Brightness if self.settings.quality.samples_at_lowest() => ChangeKind::Simulation,
			ChangeKind::Brightness => ChangeKind::Display,
			ChangeKind::ProbesAndBrightness if self.settings.quality.samples_at_lowest() => ChangeKind::Simulation,
			ChangeKind::ProbesAndBrightness => ChangeKind::ProbePresentation,
			k => k,
		};
		if note.is_some() {
			self.last_edit = note;
		}
		self.sync_timeline(project);
		if matches!(kind, ChangeKind::Simulation | ChangeKind::ProbePresentation)
			&& matches!(self.export, Export::Reading)
		{
			self.edited_during_export = Some(self.edited_during_export.map_or(kind, |k| k.merge(kind)));
			return;
		}
		match kind {
			ChangeKind::Simulation => {
				self.quality = self.settings.quality;
				self.track_intensity = false;
				self.live = false;
				self.user_paused = false;
				self.playback.pause();
				self.drop_waiting_export();
				if self.warning.take().is_some() {
					self.notices.push(Notice::WarningDropped);
				}
				self.respec(project);
				tracing::debug!(
					revision = self.revision,
					"edit: {}",
					self.last_edit.take().as_deref().unwrap_or("unnamed")
				);
				self.wanted = Wanted::Position;
				if self.settings.auto_rerun {
					self.want_all(Then::Rerun, now);
				}
			}
			ChangeKind::ProbePresentation => self.respec(project),
			ChangeKind::Display
			| ChangeKind::Metadata
			| ChangeKind::None
			| ChangeKind::Brightness
			| ChangeKind::ProbesAndBrightness => {}
		}
		self.publish();
	}

	/// Asks for every frame, after the memory warning when the frames do not fit; false when the warning asks or
	/// there is no spec. An export's run never asks: its frames are wanted already.
	fn want_all(&mut self, then: Then, now: Instant) -> bool {
		let Some(request) = self.request().cloned() else { return false };
		if self.settings.warn_when_ram_insufficient && !self.export_busy() {
			let reclaimed = self.status.store.as_ref().map_or(0, |s| s.ram_bytes());
			let (budget, auto_budget) = self.backend.budgets(self.settings.ram_percent, reclaimed);
			if MemoryWarning::check(&request, budget, auto_budget).is_some() {
				tracing::debug!(need = request.store_bytes(), budget, "memory warning asks before the run");
				let asked = self.warning.is_some();
				self.warning = Some(PendingWarning { budget, auto_budget, then });
				if !asked {
					self.notices.push(Notice::AskMemoryWarning);
				}
				return false;
			}
		}
		self.start(then, now);
		true
	}

	/// Every frame wanted, with what `then` asks beside.
	fn start(&mut self, then: Then, now: Instant) {
		self.wanted = Wanted::All;
		self.ran = true;
		self.run_started = Some(now);
		self.user_paused = false;
		match then {
			Then::Run => {
				self.playback.pause();
				self.playback.set_position(0);
				self.live = true;
			}
			Then::Play => {
				self.live = false;
				self.playback.play(now, None);
			}
			Then::Rerun => {}
		}
		tracing::debug!(?then, "every frame wanted");
	}

	/// Moves whenever [`present_spans`](Self::present_spans) would: a store only gains frames within a generation.
	pub fn progress_key(&self) -> Option<(u64, usize)> {
		self.current_store().map(|s| (s.generation(), s.present_count()))
	}

	/// The present frames as timeline spans, for the timeline's band.
	pub fn present_spans(&self) -> Vec<(u32, u32)> {
		let (Some(store), Some(request)) = (self.current_store(), self.adopted_request()) else { return Vec::new() };
		let spec = request.schedule();
		let mut spans = Vec::new();
		let mut j = 0;
		while j < spec.stored_frames() {
			if let Some(last) = store.present_through(j as usize) {
				let last = last as u32;
				spans.push((j * spec.frame_divisor(), spec.covered_frames(last + 1) - 1));
				j = last + 1;
			} else {
				j += 1;
			}
		}
		spans
	}

	/// Advances the machine: what the run has, playback, the frame shown, the fit, the export.
	pub fn tick(&mut self, now: Instant, project: &Project) {
		self.started = true;
		if !matches!(self.export, Export::Reading)
			&& let Some(kind) = self.edited_during_export.take()
		{
			self.on_change(kind, project, None, now);
		}
		self.status = self.backend.status();
		let adopted = self.status.adopted;
		if let Some(first) = self.requests.iter().position(|(revision, _)| *revision >= adopted) {
			self.requests.drain(..first);
		}
		if let WorkState::Failed(e) = &self.status.state
			&& self.failed.as_ref().is_none_or(|f| !Arc::ptr_eq(f, e))
		{
			self.failed = Some(e.clone());
			let no_image = matches!(**e, RunError::Spec(RunSpecError::NoImage));
			self.notices.push(Notice::RunFailed { message: e.to_string(), no_image });
		}
		if self.fit.is_some() && (self.request().is_none() || matches!(self.status.state, WorkState::Failed(_))) {
			self.fit = None;
		}
		// the store fills from another thread, so a run can complete between the follow's look and this check:
		// a completed run ends on its last frame, never on the frame the follow last saw
		if self.live && self.complete() {
			self.playback.set_position(self.playback.last_frame());
			self.live = false;
		} else if self.live
			&& let Some(newest) = self.present_through(self.playback.position())
		{
			self.playback.set_position(newest);
		}
		self.report_run_end(now);
		self.advance_playback(now);
		self.show_frame();
		self.poll_fit();
		self.poll_export();
		self.publish();
	}

	fn advance_playback(&mut self, now: Instant) {
		if !self.playback.is_playing() {
			return;
		}
		let limit = self.play_limit();
		self.playback.advance(now, Some(limit));
	}

	/// The newest frame playback may reach: the present run from the position while frames keep coming, its end
	/// once nothing more comes.
	fn play_limit(&self) -> Limit {
		let position = self.playback.position();
		let through = self.present_through(position).unwrap_or(position);
		if self.run_is_active() { Limit::Hold(through) } else { Limit::End(through) }
	}

	/// A picture changes only when a frame for its position arrives.
	fn show_frame(&mut self) {
		let position = self.playback.position();
		if self.store_has(position) && self.shown != Some(position) {
			self.shown = Some(position);
			self.notices.push(Notice::ShowFrame(position));
		}
	}

	fn poll_fit(&mut self) {
		let Some(frame) = self.backend.take_fit() else { return };
		if let Some(request) = self.fit
			&& request.id == frame.id
		{
			self.fit = None;
			self.fit_frames = Some(frame);
		}
	}

	fn poll_export(&mut self) {
		let Export::Waiting { needs_intensity } = self.export else { return };
		if self.is_full_for_export(needs_intensity) {
			self.export = Export::Reading;
			self.notices.push(Notice::ExportReady);
		}
	}

	/// Every frame of a full-quality spec (with the intensity when needed) is stored and recorded.
	fn is_full_for_export(&self, needs_intensity: bool) -> bool {
		self.request()
			.is_some_and(|r| r.quality.is_full() && (!needs_intensity || r.layers().contains(&Layer::Intensity)))
			&& self.complete()
	}

	/// Auto brightness with 8-bit samples: the frame at the position once more with 16-bit samples, for
	/// [`take_half_frame`](Self::take_half_frame) alone. Returns whether it was requested.
	pub fn request_half_frame(&mut self, _project: &Project) -> bool {
		self.request_fit()
	}

	/// The frame [`request_half_frame`](Self::request_half_frame) asked for, once it arrived.
	pub fn take_half_frame(&mut self) -> Option<FitFrame> {
		self.fit_frames.take()
	}

	/// Steps whose probe readings the traces hold.
	pub fn traces_steps(&self) -> u32 {
		self.status.traces_steps
	}

	/// A fit frame is on its way.
	pub fn fit_pending(&self) -> bool {
		self.fit.is_some()
	}

	fn request_fit(&mut self) -> bool {
		if self.request().is_none() {
			tracing::warn!("no 16-bit frame for the brightness fit: no run");
			return false;
		}
		self.next_fit += 1;
		self.fit = Some(FitRequest { id: self.next_fit, frame: self.playback.position() });
		self.publish();
		true
	}

	/// The memory warning's Run: the levers' quality and cache for this and later runs (the caller persists
	/// the returned choice), then the run.
	pub fn confirm_warning(&mut self, levers: Levers, mute: bool, now: Instant) -> Option<WarningChoice> {
		let w = self.warning.take()?;
		let request = self.request()?;
		let warning = MemoryWarning { request, budget: w.budget, auto_budget: w.auto_budget };
		let quality = warning.quality_of(levers);
		let next = request.with_quality(quality);
		let choice = WarningChoice { quality, cache_auto: levers.cache_auto, mute };
		self.settings.quality = quality;
		self.quality = quality;
		if levers.cache_auto {
			self.settings.ram_percent = 0;
		}
		if mute {
			self.settings.warn_when_ram_insufficient = false;
		}
		self.revise(next);
		self.start(w.then, now);
		self.publish();
		Some(choice)
	}

	/// The memory warning's Cancel: nothing changes, the frames stay, playback is off.
	pub fn cancel_warning(&mut self) {
		if self.warning.take().is_some() {
			self.playback.pause();
			tracing::info!("The run was not started; the preview keeps the frames it has.");
			self.notices.push(Notice::Info("The run was not started.".into()));
			self.publish();
		}
	}

	/// F5 / Run: every frame from frame 0, followed.
	pub fn run_user(&mut self, project: &Project, now: Instant) -> bool {
		if self.request().is_none() {
			self.respec(project);
		}
		let started = self.want_all(Then::Run, now);
		self.publish();
		started
	}

	/// Stop / Esc: the frames so far stay, the preview on the newest one, Live off; a waiting export is dropped.
	pub fn stop_run(&mut self) {
		self.drop_waiting_export();
		if self.run_is_active() {
			if self.live
				&& let Some(newest) = self.present_through(self.playback.position())
			{
				self.playback.set_position(newest);
			}
			self.wanted = Wanted::Position;
			self.live = false;
			self.user_paused = false;
			self.publish();
		}
	}

	/// F6: pauses or resumes the run.
	pub fn toggle_pause_run(&mut self) {
		if self.run_is_active() {
			self.user_paused = !self.user_paused;
			self.publish();
		}
	}

	/// F or the Live button: only while a run is active.
	pub fn set_live(&mut self, on: bool) {
		self.live = on && self.run_is_active();
		if self.live {
			self.playback.pause();
		}
	}

	/// A click or drag on the timeline: the position only; playback pauses, Live turns off. The run follows.
	pub fn scrub(&mut self, frame: u32, _now: Instant) {
		self.playback.pause();
		self.live = false;
		self.playback.set_position(frame);
		self.publish();
	}

	/// Arrows, PageUp/PageDown, Home/End as a frame delta; scrub rules.
	pub fn step_frames(&mut self, delta: i64, now: Instant) {
		let target = (self.playback.position() as i64 + delta).max(0) as u32;
		self.scrub(target, now);
	}

	/// Play: "show me the simulation". Asks for every frame when some are missing, and plays as they arrive.
	pub fn play(&mut self, project: &Project, now: Instant) {
		if self.playback.is_playing() {
			return;
		}
		self.live = false;
		if self.request().is_none() {
			self.respec(project);
		}
		if self.complete() {
			let end = match self.play_limit() {
				Limit::End(end) => Some(end),
				Limit::Hold(_) => None,
			};
			self.playback.play(now, end);
		} else if self.wanted == Wanted::All {
			self.playback.play(now, None);
		} else {
			self.want_all(Then::Play, now);
		}
		self.publish();
	}

	/// Pause: the position's frame is wanted either way.
	pub fn pause_playback(&mut self, _now: Instant) {
		self.playback.pause();
		self.publish();
	}

	/// An export asks for its run: ready when the current one is fresh, complete and full quality (with the
	/// intensity when `needs_intensity`), else a full-quality run starts (never with the memory warning).
	pub fn request_export(&mut self, project: &Project, needs_intensity: bool, now: Instant) -> ExportGate {
		if self.export_busy() {
			return ExportGate::Busy;
		}
		if self.warning.take().is_some() {
			self.notices.push(Notice::WarningDropped);
		}
		if self.is_full_for_export(needs_intensity) {
			self.export = Export::Reading;
			return ExportGate::Ready;
		}
		self.quality = PreviewQuality::FULL;
		self.track_intensity = needs_intensity;
		self.respec(project);
		if self.request().is_none() {
			return ExportGate::Failed;
		}
		self.start(Then::Rerun, now);
		self.export = Export::Waiting { needs_intensity };
		self.publish();
		ExportGate::Waiting
	}

	/// An export holds or waits for the run.
	pub fn export_busy(&self) -> bool {
		!matches!(self.export, Export::Idle)
	}

	/// The export finished or was cancelled; the run may be replaced again.
	pub fn end_export(&mut self) {
		self.export = Export::Idle;
	}

	/// Cancels an export waiting for its run; the run stops, its frames so far stay.
	pub fn cancel_export(&mut self) {
		if let Export::Waiting { .. } = self.export {
			self.export = Export::Idle;
			self.wanted = Wanted::Position;
			self.publish();
		}
	}
}

/// The core's live run on the CPU or GPU engine, by the Use the GPU setting.
pub struct CoreBackend {
	use_gpu: Arc<AtomicBool>,
	worker: RunWorker,
	scene: Arc<Mutex<SceneCache>>,
}

/// Builds the engine for a scene: the GPU's when asked for and available (a fallback to the CPU is logged).
pub fn build_engine(scene: Arc<SceneData>, options: EngineOptions, use_gpu: bool) -> Box<dyn WaveEngine> {
	let e = wavedude_core::engine::build(scene, options, use_gpu);
	super::log::engine_built(e.name(), e.scene(), e.options(), e.sub_steps());
	e
}

impl CoreBackend {
	/// A backend with its spill files in `cache_dir`; the engine follows `use_gpu`, shared with the settings.
	pub fn new(use_gpu: Arc<AtomicBool>, cache_dir: PathBuf) -> std::io::Result<Self> {
		let gpu = use_gpu.clone();
		let scene: Arc<Mutex<SceneCache>> = Arc::default();
		let worker = RunWorker::new(
			Box::new(move |scene, options| build_engine(scene, options, gpu.load(Ordering::Relaxed))),
			cache_dir,
			scene.clone(),
		)?;
		Ok(Self { use_gpu, worker, scene })
	}

	/// The scene pixels and medium the specs are built from, shared with whatever else shows the scene.
	pub fn scene_cache(&self) -> Arc<Mutex<SceneCache>> {
		self.scene.clone()
	}
}

impl Backend for CoreBackend {
	fn set_goal(&mut self, goal: Goal) {
		self.worker.set_goal(goal);
	}

	fn status(&self) -> Status {
		self.worker.status()
	}

	fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		self.worker.with_traces(|t| f(t));
	}

	fn with_held_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		self.worker.with_held_traces(|t| f(t));
	}

	fn take_fit(&mut self) -> Option<FitFrame> {
		self.worker.take_fit()
	}

	fn engine_changed(&mut self) {
		self.worker.rebuild_engine();
	}

	fn budgets(&self, ram_percent: u32, reclaimed: u64) -> (u64, u64) {
		(Budget::compute(reclaimed, ram_percent), Budget::compute(reclaimed, 0))
	}

	fn engine_name(&self) -> String {
		if self.use_gpu.load(Ordering::Relaxed)
			&& let Ok(ctx) = wavedude_core::engine::gpu::context()
		{
			return format!("GPU: {}", ctx.info.name);
		}
		let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
		format!("CPU ({threads} threads)")
	}
}

/// A scripted run: the goals are recorded, the frames come when the caller writes them. For tests and headless
/// tools.
pub mod fake {
	use std::sync::Arc;

	use wavedude_core::engine::probes::ProbeTrace;
	use wavedude_core::engine::{SampleFormat, Snapshot};
	use wavedude_core::f16;
	use wavedude_core::render::{field_index, intensity_root_index};
	use wavedude_core::run::{FitFrame, FrameStore, Goal, RunSpec, Status, Wanted, WorkState};

	use super::Backend;

	/// Records every goal; adopts a spec at once, with a real (small) store whose frames the caller writes.
	#[derive(Default)]
	pub struct FakeBackend {
		pub goals: Vec<Goal>,
		store: Option<Arc<FrameStore>>,
		spec: Option<RunSpec>,
		adopted: u64,
		/// Set by the caller: the run stepped this far.
		pub steps_done: u32,
		/// (budget, budget at Auto); unlimited when unset.
		pub budgets: Option<(u64, u64)>,
		pub fits_served: Vec<u64>,
		pub engine_changes: usize,
		fit: Option<FitFrame>,
		/// The run failed, until the next spec.
		failure: Option<Arc<wavedude_core::run::RunError>>,
		/// Set by the caller: the run's traces; a spec with other frames moves them to `held`.
		pub traces: Vec<ProbeTrace>,
		held: Vec<ProbeTrace>,
	}

	impl FakeBackend {
		/// The run fails with `message` until the next spec.
		pub fn fail(&mut self, message: &str) {
			self.failure = Some(Arc::new(wavedude_core::run::RunError::Panic("fake", message.into())));
		}

		pub fn goal(&self) -> &Goal {
			self.goals.last().expect("a goal was published")
		}

		pub fn wanted(&self) -> Wanted {
			self.goals.last().map_or(Wanted::Position, |g| g.wanted)
		}

		/// Goals published that wanted every frame.
		pub fn fills(&self) -> usize {
			self.goals.iter().filter(|g| g.wanted == Wanted::All).count()
		}

		pub fn store(&self) -> &Arc<FrameStore> {
			self.store.as_ref().expect("a spec was adopted")
		}

		pub fn adopted_spec(&self) -> &RunSpec {
			self.spec.as_ref().expect("a spec was adopted")
		}

		fn uniform(spec: &RunSpec, step: u32, u: f32, i: f32) -> Snapshot {
			let shape = spec.shape;
			let n = shape.plane_samples();
			let (field, intensity) = match shape.format {
				SampleFormat::Half => {
					let root = if shape.intensity_root { i.sqrt() } else { i };
					let plane = |v: f32| f16::from_f32(v).to_ne_bytes().repeat(n);
					(plane(u), plane(root))
				}
				SampleFormat::Index => (
					vec![field_index(f16::from_f32(u), shape.field_brightness); n],
					vec![intensity_root_index(f16::from_f32(i.sqrt()), shape.intensity_brightness); n],
				),
			};
			Snapshot { shape, step, field, intensity }
		}

		/// Writes timeline frame `frame` as uniform samples: the field `u`, the intensity `i`.
		pub fn finish_frame_uniform(&mut self, frame: u32, u: f32, i: f32) {
			let spec = self.adopted_spec().clone();
			let stored = spec.stored_index_of(frame);
			let snapshot = Self::uniform(&spec, stored * spec.capture_every(), u, i);
			self.store().write_at(stored as usize, snapshot).expect("the frame fits the store");
			self.steps_done = self.steps_done.max(stored * spec.capture_every() + 1);
		}

		/// Writes the goal's position frame.
		pub fn finish_position(&mut self) {
			let position = self.goal().position;
			self.finish_frame_uniform(position, 0.0, 0.0);
		}

		/// Writes the timeline frames below `frames`.
		pub fn publish(&mut self, frames: u32) {
			let spec = self.adopted_spec().clone();
			for stored in 0..spec.stored_frames().min(frames.div_ceil(spec.frame_divisor())) {
				self.finish_frame_uniform(stored * spec.frame_divisor(), 0.0, 0.0);
			}
		}

		/// Writes every frame.
		pub fn complete(&mut self) {
			let frames = self.adopted_spec().frame_count();
			self.publish(frames);
			self.steps_done = self.adopted_spec().steps;
		}

		/// Serves the goal's fit with uniform samples in halves.
		pub fn finish_fit_uniform(&mut self, u: f32, i: f32) {
			let Some(fit) = self.goal().fit else { return };
			let spec = self.adopted_spec().clone();
			let shape = wavedude_core::engine::FrameShape { format: SampleFormat::Half, ..spec.shape };
			let half = RunSpec { shape, ..spec.clone() };
			let snapshot = Self::uniform(&half, spec.step_of(fit.frame), u, i);
			self.fits_served.push(fit.id);
			self.fit = Some(FitFrame { id: fit.id, frame: fit.frame, snapshot: Arc::new(snapshot) });
		}
	}

	impl Backend for FakeBackend {
		fn set_goal(&mut self, goal: Goal) {
			if let Some(request) = &goal.run {
				let spec = RunSpec::create(&request.project, None, request.quality, request.track_intensity)
					.expect("the fake builds every spec");
				let spec = &spec;
				let same_frames = self.spec.as_ref().is_some_and(|s| s.same_frames_as(spec));
				let same_probes = self.spec.as_ref().is_some_and(|s| s.same_probes_as(spec));
				if !same_frames {
					let layout = spec.store_layout();
					match &self.store {
						Some(store) if store.layout().fits(&layout) => store.clear(),
						_ => self.store = Some(Arc::new(FrameStore::new(layout, 1 << 30, None).unwrap())),
					}
					self.steps_done = 0;
					self.failure = None;
					self.held = if same_probes { std::mem::take(&mut self.traces) } else { Vec::new() };
				} else if !same_probes {
					self.traces.clear();
					self.held.clear();
				}
				self.spec = Some(spec.clone());
				self.adopted = goal.revision;
			}
			self.goals.push(goal);
		}

		fn status(&self) -> Status {
			let state = match (&self.failure, self.goals.last(), self.spec.as_ref(), self.store.as_ref()) {
				(Some(e), ..) => WorkState::Failed(e.clone()),
				(None, Some(goal), Some(spec), Some(store)) => {
					let position = spec.stored_index_of(goal.position.min(spec.frame_count().saturating_sub(1)));
					let next = if !store.has(position as usize) {
						Some(position)
					} else if goal.wanted == Wanted::All {
						store.first_missing_from(0).map(|j| j as u32)
					} else {
						None
					};
					let fitting = goal.fit.is_some_and(|f| !self.fits_served.contains(&f.id));
					match next {
						_ if fitting => WorkState::Fitting,
						Some(_) if goal.paused => WorkState::Paused,
						Some(target) => WorkState::Stepping { target },
						None => WorkState::Idle,
					}
				}
				_ => WorkState::Idle,
			};
			Status {
				adopted: self.adopted,
				state,
				steps_done: self.steps_done,
				traces_steps: 0,
				store: self.store.clone(),
			}
		}

		fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
			f(&self.traces);
		}

		fn with_held_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
			f(&self.held);
		}

		fn take_fit(&mut self) -> Option<FitFrame> {
			self.fit.take()
		}

		fn engine_changed(&mut self) {
			self.engine_changes += 1;
		}

		fn budgets(&self, _ram_percent: u32, _reclaimed: u64) -> (u64, u64) {
			self.budgets.unwrap_or((u64::MAX, u64::MAX))
		}

		fn engine_name(&self) -> String {
			"fake".into()
		}
	}
}

#[cfg(test)]
mod tests {
	use super::fake::FakeBackend;
	use super::*;
	use wavedude_core::engine::SampleFormat;

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 64;
		p.scene.height = 48;
		p.simulation.steps = 200;
		p.simulation.visualize_every = 2;
		p.add_source();
		p
	}

	fn controller(p: &Project, t: Instant) -> LiveController<FakeBackend> {
		let mut c = LiveController::new(FakeBackend::default(), LiveSettings::default());
		c.project_replaced(p, t);
		c.tick(t, p);
		c.backend_mut().finish_position();
		c.tick(t, p);
		c.take_notices();
		c
	}

	#[test]
	fn a_replaced_project_wants_frame_zero_alone() {
		let t = Instant::now();
		let p = project();
		let mut c = LiveController::new(FakeBackend::default(), LiveSettings::default());
		c.project_replaced(&p, t);
		assert!(c.backend().goals.is_empty(), "nothing before the first tick: the UI is not up");
		c.tick(t, &p);
		assert_eq!((c.backend().goal().position, c.backend().wanted()), (0, Wanted::Position));
		assert_eq!(c.run_status(), None, "opening never runs");
		c.tick(t, &p);
		assert_eq!(c.computing_picture(), Some(0), "the run is stepping towards it");
		c.backend_mut().finish_position();
		c.tick(t, &p);
		assert_eq!(c.shown(), Some(0));
		assert!(c.take_notices().contains(&Notice::ShowFrame(0)));
		assert!(!c.is_working());
	}

	#[test]
	fn an_edit_wants_every_frame_and_the_picture_waits_for_its_own() {
		let t = Instant::now();
		let mut p = project();
		let mut c = controller(&p, t);
		c.scrub(10, t);
		c.tick(t, &p);
		c.backend_mut().finish_position();
		c.tick(t, &p);
		assert_eq!(c.shown(), Some(10));
		c.backend_mut().traces.push(ProbeTrace::default());
		let revision = c.backend().goal().revision;
		p.sources[0].frequency = 120.0;
		c.on_change(ChangeKind::Simulation, &p, Some("Source 1.frequency = 120".into()), t);
		let goal = c.backend().goal();
		assert_eq!((goal.wanted, goal.position), (Wanted::All, 10));
		assert!(goal.revision > revision, "a new spec");
		c.tick(t, &p);
		assert_eq!(c.shown(), Some(10), "the last picture stays");
		assert!(
			matches!(c.shown_source(), Some(ShownSource::Snapshot { .. })),
			"the frame made last stays until the new state's frame lands"
		);
		c.with_traces(&mut |t| assert_eq!(t.len(), 1, "the graphs hold with the picture"));
		assert_eq!(c.computing_picture(), Some(10));
		assert!(c.run_is_active());
		c.backend_mut().finish_position();
		c.tick(t, &p);
		assert!(matches!(c.shown_source(), Some(ShownSource::Stored(_, 10))));
		c.with_traces(&mut |t| assert!(t.is_empty(), "the new state's traces with its frame"));
		assert!(c.take_notices().contains(&Notice::ShowFrame(10)));
		assert_eq!(c.run_status().map(|s| s.0), Some(RunState::Running));
		c.backend_mut().complete();
		c.tick(t, &p);
		assert_eq!(c.run_status().map(|s| (s.0, s.1)), Some((RunState::Completed, 200)));
		assert!(!c.run_is_active());
	}

	#[test]
	fn with_auto_rerun_off_only_the_position_is_wanted() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.set_settings(LiveSettings { auto_rerun: false, ..c.settings() });
		c.on_change(ChangeKind::Simulation, &p, None, t);
		assert_eq!(c.backend().wanted(), Wanted::Position);
		assert_eq!(c.run_status(), None, "no run to describe");
		c.set_settings(LiveSettings { auto_rerun: true, ..c.settings() });
		c.tick(t, &p);
		assert_eq!(c.backend().wanted(), Wanted::Position, "turning it on asks for nothing by itself");
	}

	/// The summary is checked through `run_summary`: a test subscriber can miss the log line, since tracing caches
	/// each callsite's interest process-wide while other tests run.
	#[test]
	fn a_finished_run_is_summed_up_once() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.tick(t, &p);
		assert_eq!(c.run_started, Some(t));
		assert_eq!(c.run_summary(t, t), None, "under way");
		c.backend_mut().complete();
		c.tick(t + std::time::Duration::from_millis(400), &p);
		assert_eq!(c.run_started, None, "reported on the tick that completed it");
		let line = c.run_summary(t, t + std::time::Duration::from_millis(400)).expect("a completed run");
		assert!(line.starts_with("Run Completed: 200 of 200 steps, "), "{line}");
		assert!(line.contains(", 0.4 s; ") && line.contains(" in memory ("), "{line}");
	}

	#[test]
	fn a_probe_edit_keeps_the_frames() {
		let t = Instant::now();
		let mut p = project();
		p.add_probe();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend_mut().complete();
		c.tick(t, &p);
		let store = c.backend().store().clone();
		c.on_change(ChangeKind::ProbePresentation, &p, None, t);
		c.tick(t, &p);
		assert!(Arc::ptr_eq(&store, c.backend().store()) && store.is_complete(), "the frames stay");
		assert!(c.shown_source().is_some(), "the picture is a frame of the current state");
		assert_eq!(c.backend().wanted(), Wanted::All, "a run in force stays in force");
	}

	#[test]
	fn the_fit_frame_is_served_once() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend_mut().complete();
		c.scrub(40, t);
		c.tick(t, &p);
		assert!(c.request_half_frame(&p));
		assert_eq!(c.backend().goal().fit.map(|f| f.frame), Some(40));
		assert_eq!(c.computing_picture(), None, "the pictures keep the run's frame");
		c.backend_mut().finish_fit_uniform(0.5, 0.25);
		c.tick(t, &p);
		assert_eq!(c.shown(), Some(40));
		let frame = c.take_half_frame().expect("the 16-bit frame");
		assert_eq!((frame.frame, frame.snapshot.shape.format, frame.snapshot.step), (40, SampleFormat::Half, 80));
		assert!(c.take_half_frame().is_none());
		assert!(c.backend().goal().fit.is_none(), "served, so no longer asked for");
	}

	#[test]
	fn a_user_run_follows_the_newest_frame_and_a_scrub_turns_live_off() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.scrub(50, t);
		assert!(c.run_user(&p, t));
		assert!(c.is_live() && c.playback.position() == 0);
		assert_eq!(c.run_status().map(|s| s.0), Some(RunState::Running));
		c.backend_mut().publish(30);
		c.tick(t, &p);
		assert_eq!(c.playback.position(), 29, "Live follows the newest frame");
		assert_eq!(c.shown(), Some(29));
		c.scrub(5, t);
		assert!(!c.is_live() && !c.playback.is_playing());
		c.tick(t, &p);
		assert_eq!(c.shown(), Some(5), "the store can show frame 5");
		c.stop_run();
		assert!(!c.run_is_active());
		assert_eq!(c.backend().wanted(), Wanted::Position);
		c.tick(t, &p);
		assert_eq!(c.run_status().map(|s| s.0), Some(RunState::Cancelled), "stopped, the frames so far stay");
	}

	#[test]
	fn a_run_completing_between_ticks_ends_on_its_last_frame() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		assert!(c.run_user(&p, t));
		c.backend_mut().publish(30);
		c.tick(t, &p);
		assert_eq!(c.playback.position(), 29);
		// the rest of the run lands before the next tick
		c.backend_mut().complete();
		c.tick(t, &p);
		assert_eq!(c.playback.position(), c.playback.last_frame(), "a followed run ends on its last frame");
		assert!(!c.is_live());
	}

	/// Half frames over an odd frame count: 101 timeline frames, 51 stored, the last stored frame covering one.
	fn half_frames_controller(t: Instant) -> (Project, LiveController<FakeBackend>) {
		let mut p = project();
		p.simulation.steps = 202;
		let settings = LiveSettings { quality: PreviewQuality::DEFAULT.halve_frames(), ..LiveSettings::default() };
		let mut c = LiveController::new(FakeBackend::default(), settings);
		c.project_replaced(&p, t);
		c.tick(t, &p);
		c.backend_mut().finish_position();
		c.tick(t, &p);
		c.take_notices();
		assert_eq!((c.playback.last_frame(), c.backend().adopted_spec().stored_frames()), (100, 51));
		(p, c)
	}

	#[test]
	fn a_followed_run_with_half_frames_stays_live_until_its_last_frame() {
		let t = Instant::now();
		let (p, mut c) = half_frames_controller(t);
		assert!(c.run_user(&p, t));
		c.tick(t, &p);
		let mut followed = vec![c.playback.position()];
		// frames land out of order: a gap holds the follow at the newest contiguous frame
		for frame in [60, 2, 4, 6, 30, 8, 10] {
			c.backend_mut().finish_frame_uniform(frame, 0.0, 0.0);
			c.tick(t, &p);
			assert!(c.is_live(), "Live holds while frames are missing (after frame {frame})");
			followed.push(c.playback.position());
		}
		assert_eq!(followed, [1, 1, 3, 5, 7, 7, 9, 11], "the newest contiguous frame, both halves of a stored frame");
		assert_eq!(c.shown(), Some(11));
		c.backend_mut().complete();
		c.tick(t, &p);
		assert_eq!(c.playback.position(), 100, "the last frame, not the end of the newest contiguous span seen");
		assert_eq!(c.shown(), Some(100));
		assert!(!c.is_live() && !c.run_is_active());
		assert_eq!(c.run_status().map(|s| s.0), Some(RunState::Completed));
	}

	#[test]
	fn a_followed_run_that_completes_before_its_first_tick_ends_on_its_last_frame() {
		let t = Instant::now();
		let (p, mut c) = half_frames_controller(t);
		assert!(c.run_user(&p, t));
		c.backend_mut().complete();
		c.tick(t, &p);
		assert_eq!((c.playback.position(), c.shown()), (100, Some(100)));
		assert!(!c.is_live());
		assert!(c.take_notices().contains(&Notice::ShowFrame(100)));
	}

	#[test]
	fn play_starts_a_run_and_holds_at_the_newest_frame() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.play(&p, t);
		assert_eq!(c.backend().wanted(), Wanted::All);
		assert!(c.playback.is_playing());
		c.backend_mut().publish(5);
		c.tick(t + std::time::Duration::from_secs(1), &p);
		assert_eq!(c.playback.position(), 4);
		c.backend_mut().complete();
		c.tick(t + std::time::Duration::from_millis(1100), &p);
		assert!(c.playback.position() > 4);
	}

	#[test]
	fn pause_and_resume_reach_the_goal() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.toggle_pause_run();
		assert!(c.run_paused_by_user() && c.backend().goal().paused);
		c.tick(t, &p);
		assert_eq!(c.run_status().map(|s| s.0), Some(RunState::Paused));
		c.toggle_pause_run();
		assert!(!c.backend().goal().paused);
		c.toggle_pause_run();
		c.on_change(ChangeKind::Simulation, &p, None, t);
		assert!(!c.backend().goal().paused, "an edit's run is not paused");
	}

	#[test]
	fn memory_warning_run_and_cancel() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		let need = RunRequest::new(&p, PreviewQuality::DEFAULT, false).store_bytes();
		c.backend_mut().budgets = Some((need / 2, need / 2));
		assert!(!c.run_user(&p, t));
		assert!(c.take_notices().contains(&Notice::AskMemoryWarning));
		assert_eq!(c.backend().wanted(), Wanted::Position, "nothing more until the answer");
		let proposal = c.warning().unwrap().proposal();
		assert!(proposal.frames || proposal.resolution);
		let choice = c.confirm_warning(proposal, false, t).unwrap();
		assert_ne!(choice.quality, PreviewQuality::DEFAULT);
		assert_eq!(c.backend().wanted(), Wanted::All);
		assert!(c.is_live(), "the warning's Run is still F5's run");
		assert_eq!(c.settings().quality, choice.quality);
		c.backend_mut().budgets = Some((0, 0));
		c.on_change(ChangeKind::Simulation, &p, None, t);
		assert!(c.warning().is_some(), "the re-run asks too");
		c.play(&p, t);
		c.cancel_warning();
		assert!(!c.playback.is_playing(), "a cancelled warning turns playback off");
		assert_eq!(c.backend().wanted(), Wanted::Position);
	}

	#[test]
	fn a_replaced_project_ends_what_an_export_waited_for() {
		let t = Instant::now();
		let mut p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend_mut().complete();
		c.tick(t, &p);
		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		c.take_notices();
		c.project_replaced(&p, t);
		assert!(c.take_notices().contains(&Notice::ExportStopped), "the waiting export is told");
		assert!(!c.export_busy());
		c.run_user(&p, t);
		c.backend_mut().complete();
		c.tick(t, &p);
		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		c.backend_mut().complete();
		c.tick(t, &p);
		c.end_export();
		assert_eq!(c.request_export(&p, false, t), ExportGate::Ready);
		p.sources[0].frequency = 120.0;
		c.on_change(ChangeKind::Simulation, &p, None, t);
		let revision = c.backend().goal().revision;
		c.project_replaced(&p, t);
		c.tick(t, &p);
		assert_eq!(c.backend().wanted(), Wanted::Position, "the deferred edit died with the project");
		assert!(c.backend().goal().revision > revision);
	}

	#[test]
	fn the_memory_warning_runs_the_project_as_it_is_now() {
		let t = Instant::now();
		let mut p = project();
		let mut c = controller(&p, t);
		let need = RunRequest::new(&p, PreviewQuality::DEFAULT, false).store_bytes();
		c.backend_mut().budgets = Some((need / 2, need / 2));
		assert!(!c.run_user(&p, t));
		p.add_probe();
		c.on_change(ChangeKind::ProbePresentation, &p, None, t);
		let proposal = c.warning().unwrap().proposal();
		c.confirm_warning(proposal, false, t).unwrap();
		assert_eq!(c.backend().adopted_spec().probes.len(), 1, "the run has the probe added meanwhile");
	}

	#[test]
	fn export_needs_a_full_quality_run() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend_mut().complete();
		c.tick(t, &p);
		assert_eq!(c.request_export(&p, true, t), ExportGate::Waiting, "an 8-bit preview run does not do");
		assert!(c.backend().adopted_spec().quality.is_full() && c.backend().adopted_spec().track_intensity());
		c.backend_mut().complete();
		c.tick(t, &p);
		assert!(c.take_notices().contains(&Notice::ExportReady));
		assert_eq!(c.request_export(&p, true, t), ExportGate::Busy);
		c.end_export();
		assert_eq!(c.request_export(&p, true, t), ExportGate::Ready);
		c.end_export();
		c.on_change(ChangeKind::Simulation, &p, None, t);
		assert!(!c.backend().adopted_spec().quality.is_full(), "the next edit's frames are the preview's again");
	}

	#[test]
	fn a_waiting_export_never_outlives_its_run() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		c.on_change(ChangeKind::Simulation, &p, None, t);
		assert!(c.take_notices().contains(&Notice::ExportStopped), "an edit replaced the export's run");
		assert!(!c.export_busy());
		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		c.cancel_export();
		assert!(!c.export_busy());
		assert_eq!(c.backend().wanted(), Wanted::Position, "a cancelled export stops its run");
		c.tick(t, &p);
		let notices = c.take_notices();
		assert!(!notices.contains(&Notice::ExportReady) && !notices.contains(&Notice::ExportStopped));
	}

	#[test]
	fn a_failed_run_is_reported_once() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.backend_mut().fail("boom");
		c.tick(t, &p);
		c.tick(t, &p);
		let errors = c.take_notices().into_iter().filter(|n| matches!(n, Notice::RunFailed { .. })).count();
		assert_eq!(errors, 1);
		assert!(c.run_status().is_none(), "no run was asked for");
		c.run_user(&p, t);
		c.tick(t, &p);
		assert!(matches!(c.run_status().map(|s| s.0), Some(RunState::Failed(_))));
		assert!(!c.run_is_active() && !c.run_is_progressing(), "a failed run is over");
		c.request_half_frame(&p);
		c.tick(t, &p);
		assert!(!c.fit_pending(), "no fit comes from a failed run");
	}

	#[test]
	fn an_edit_while_an_export_reads_waits_for_it() {
		let t = Instant::now();
		let mut p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend_mut().complete();
		c.tick(t, &p);
		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		c.backend_mut().complete();
		c.tick(t, &p);
		c.end_export();
		assert_eq!(c.request_export(&p, false, t), ExportGate::Ready);
		let revision = c.backend().goal().revision;
		let generation = c.backend().store().generation();
		p.sources[0].frequency = 120.0;
		c.on_change(ChangeKind::Simulation, &p, None, t);
		c.tick(t, &p);
		assert_eq!(c.backend().goal().revision, revision, "the frames stay while the export reads them");
		assert_eq!(c.backend().store().generation(), generation);
		c.end_export();
		c.tick(t, &p);
		assert!(c.backend().goal().revision > revision, "the edit is taken up when the export ends");
		assert_eq!(c.backend().wanted(), Wanted::All);
	}

	#[test]
	fn a_probe_and_brightness_edit_keeps_16_bit_frames() {
		let t = Instant::now();
		let mut p = project();
		p.add_probe();
		let settings = LiveSettings { quality: PreviewQuality::FULL, ..LiveSettings::default() };
		let mut c = LiveController::new(FakeBackend::default(), settings);
		c.project_replaced(&p, t);
		c.tick(t, &p);
		c.backend_mut().complete();
		c.tick(t, &p);
		let generation = c.backend().store().generation();
		c.on_change(ChangeKind::ProbesAndBrightness, &p, None, t);
		c.tick(t, &p);
		assert_eq!(c.backend().store().generation(), generation, "the brightness is not baked into the frames");
		assert!(c.backend().store().is_complete());
	}

	#[test]
	fn stop_drops_the_export_waiting_for_its_run() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		c.tick(t, &p);
		c.take_notices();
		c.stop_run();
		assert!(c.take_notices().contains(&Notice::ExportStopped), "the waiting export is told");
		assert!(!c.export_busy());
		assert_eq!(c.backend().wanted(), Wanted::Position, "nobody wants the export's run any more");
	}

	#[test]
	fn an_export_drops_the_memory_warning_and_a_run_during_its_read_keeps_the_frames() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.backend_mut().budgets = Some((1, 1));
		assert!(!c.run_user(&p, t));
		c.take_notices();
		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		assert!(c.take_notices().contains(&Notice::WarningDropped), "the warning's dialog closes");
		assert!(c.warning().is_none());
		c.backend_mut().complete();
		c.tick(t, &p);
		assert!(c.take_notices().contains(&Notice::ExportReady));
		let revision = c.backend().goal().revision;
		let generation = c.backend().store().generation();
		assert!(c.run_user(&p, t), "the export's frames are all there: Run does not ask");
		c.tick(t, &p);
		assert!(!c.take_notices().contains(&Notice::AskMemoryWarning));
		assert!(c.export_busy());
		assert_eq!(c.backend().goal().revision, revision, "the frames stay while the export reads them");
		assert_eq!(c.backend().store().generation(), generation);
	}

	/// Holds every goal until [`deliver`](Self::deliver): a worker that has not adopted yet.
	struct Delayed {
		inner: FakeBackend,
		queued: Option<Goal>,
	}

	impl Delayed {
		fn deliver(&mut self) {
			if let Some(goal) = self.queued.take() {
				self.inner.set_goal(goal);
			}
		}
	}

	impl Backend for Delayed {
		fn set_goal(&mut self, goal: Goal) {
			self.queued = Some(goal);
		}
		fn status(&self) -> Status {
			self.inner.status()
		}
		fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
			self.inner.with_traces(f);
		}
		fn with_held_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
			self.inner.with_held_traces(f);
		}
		fn take_fit(&mut self) -> Option<FitFrame> {
			self.inner.take_fit()
		}
		fn engine_changed(&mut self) {}
		fn budgets(&self, ram_percent: u32, reclaimed: u64) -> (u64, u64) {
			self.inner.budgets(ram_percent, reclaimed)
		}
		fn engine_name(&self) -> String {
			"delayed".into()
		}
	}

	#[test]
	fn the_store_is_indexed_by_the_schedule_of_the_revision_it_holds() {
		let t = Instant::now();
		let p = project();
		let mut settings = LiveSettings { quality: PreviewQuality::FULL, ..LiveSettings::default() };
		let mut c = LiveController::new(Delayed { inner: FakeBackend::default(), queued: None }, settings);
		c.project_replaced(&p, t);
		c.tick(t, &p);
		c.backend_mut().deliver();
		c.backend_mut().inner.complete();
		c.scrub(80, t);
		c.backend_mut().deliver();
		c.tick(t, &p);
		assert!(matches!(c.shown_source(), Some(ShownSource::Stored(_, 80))));
		settings.quality = PreviewQuality { frame_divisor: 2, ..PreviewQuality::FULL };
		c.set_settings(settings);
		c.on_change(ChangeKind::Simulation, &p, None, t);
		c.tick(t, &p);
		assert!(
			matches!(c.shown_source(), Some(ShownSource::Stored(_, 80))),
			"the old store's frame 80 until the half frames are adopted"
		);
		assert_eq!(c.present_spans(), [(0, 99)]);
		c.backend_mut().deliver();
		c.tick(t, &p);
		assert_eq!(c.present_spans(), [], "the new store is empty");
	}
}
