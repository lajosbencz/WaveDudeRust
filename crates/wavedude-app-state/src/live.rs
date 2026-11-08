//! The live controller: what a model change does to runs, scratch frames, playback and the pictures
//! (docs/ui/60-live-editing-and-runs.md, 50-timeline-transport.md).
//!
//! A state machine driven by events and [`LiveController::tick`] with the caller's clock, so the timings
//! (scratch debounce, automatic re-run after quiet) are testable with a fake clock. Runs, scratch frames and
//! memory budgets sit behind [`Backend`]; [`CoreBackend`] wires the core's `SimulationRun` and `ScratchRunner`
//! on the CPU or GPU engine, [`fake::FakeBackend`] scripts them for tests.
//! Threads: the controller lives on the UI thread; the core's runs and scratch runner work on their own
//! threads and are polled here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::engine::{EngineOptions, SceneData, WaveEngine};
use wavedude_core::engine::{FrameShape, SampleFormat};
use wavedude_core::model::{Layer, Project};
use wavedude_core::run::{
	Budget, FrameStore, Levers, MemoryWarning, PreviewQuality, RunSpec, RunState, SceneCache, ScratchEvent,
	ScratchResult, ScratchRunner, SimulationRun,
};

use super::commands::ChangeKind;
use super::playback::{Available, Limit, Playback};

/// Wait after the last edit or scrub before a scratch frame is requested.
pub const SCRATCH_DEBOUNCE: Duration = Duration::from_millis(120);
/// Quiet after a simulation edit before the automatic re-run.
pub const RERUN_QUIET: Duration = Duration::from_millis(1500);

/// A run as the controller sees it. Called on the UI thread; implementations publish across threads.
pub trait RunHandle {
	fn state(&self) -> RunState;
	fn steps_done(&self) -> u32;
	/// Timeline frames the stored frames cover.
	fn frame_count(&self) -> u32;
	fn is_finished(&self) -> bool;
	fn pause(&self);
	fn resume(&self);
	fn cancel(&self);
	/// The frames, for the presenter and the export.
	fn store(&self) -> Option<Arc<FrameStore>> {
		None
	}
	/// Calls `f` with the probe traces recorded so far, in the spec's probe order.
	fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		f(&[]);
	}
}

/// A finished scratch request.
#[derive(Debug)]
pub enum ScratchOutcome {
	Done { id: u64, frame: u32, result: Option<Box<ScratchResult>> },
	Abandoned { id: u64 },
	Failed { id: u64, message: String },
}

/// Where runs and scratch frames come from.
pub trait Backend {
	/// The snapshot of `project` a run or scratch frame works from.
	fn spec(&mut self, project: &Project, quality: PreviewQuality, track_intensity: bool) -> Result<RunSpec, String>;
	/// Starts a run of `spec` with the RAM cache at `ram_percent` (0 = Auto).
	fn start_run(&mut self, spec: RunSpec, ram_percent: u32) -> Result<Box<dyn RunHandle>, String>;
	/// Asks for timeline frame `frame` of `spec`; the newest request wins. Returns its id.
	fn request_scratch(&mut self, spec: RunSpec, frame: u32) -> u64;
	fn cancel_scratch(&mut self);
	fn poll_scratch(&mut self) -> Vec<ScratchOutcome>;
	/// The store budget at `ram_percent` and at Auto, counting `reclaimed` bytes as free.
	fn budgets(&self, ram_percent: u32, reclaimed: u64) -> (u64, u64);
	/// "CPU (16 threads)" or the GPU's name.
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

/// Why a run starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
	/// F5 or Run: from frame 0, followed (Live on).
	User,
	/// The re-run after edits: never moves the preview, never followed.
	Automatic,
	/// Play on a stale or missing run: plays as frames arrive.
	Play,
	/// An export's full-quality run.
	Export,
}

/// What the UI should do or show; drained with [`LiveController::take_notices`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
	/// Show the run's frame (timeline frame).
	ShowRunFrame(u32),
	/// Show the scratch frame just computed ([`LiveController::latest_scratch`]).
	ShowScratch(u32),
	/// Computing a scratch frame: dim the pictures after 150 ms.
	Computing(u32),
	RunStarted(RunKind),
	RunEnded(RunState),
	/// The memory warning asks ([`LiveController::warning`]).
	AskMemoryWarning,
	/// The export's run is complete and fresh.
	ExportReady,
	/// The waiting export's run ended before completing or was replaced: the export is dropped.
	ExportStopped,
	Info(String),
	Error(String),
	/// A run could not start: `no_image` when the scene's image is missing.
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

struct ActiveRun {
	handle: Box<dyn RunHandle>,
	spec: RunSpec,
	kind: RunKind,
	stale: bool,
	probes_stale: bool,
	/// Names of the active probes the run records.
	recorded: Vec<String>,
	user_paused: bool,
	scratch_paused: bool,
	ended: bool,
}

impl ActiveRun {
	fn frames(&self) -> u32 {
		self.handle.frame_count()
	}

	fn is_active(&self) -> bool {
		!self.handle.is_finished()
	}

	fn is_full_for_export(&self, needs_intensity: bool) -> bool {
		self.spec.quality.is_full()
			&& (!needs_intensity || self.spec.layers().contains(&Layer::Intensity))
			&& self.handle.state() == RunState::Completed
			&& !self.stale
	}
}

struct PendingWarning {
	spec: RunSpec,
	budget: u64,
	auto_budget: u64,
	kind: RunKind,
}

/// The frame the pictures show: a run's stored frame or the newest scratch frame, by timeline frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
	Run(u32),
	Scratch(u32),
}

/// What a scratch frame is computed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScratchUse {
	/// The pictures: the run cannot show the position.
	Picture,
	/// The probe graphs only: the run shows the position but its recordings are stale.
	Graphs,
	/// Auto brightness: the position in 16-bit samples, kept for the fit and never shown.
	Fit,
	/// An opened project's brightness fit: the same, for its own taker.
	OpenFit,
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
	run: Option<ActiveRun>,
	live: bool,
	scratch_due: Option<Instant>,
	scratch_in_flight: Option<(u64, u32, ScratchUse)>,
	rerun_due: Option<Instant>,
	shown: Option<Shown>,
	warning: Option<PendingWarning>,
	export: Export,
	last_edit: Option<String>,
	notices: Vec<Notice>,
	/// The newest scratch frame shown, kept for the presenter and the graphs.
	latest_scratch: Option<Arc<ScratchResult>>,
	/// A brightness fit's 16-bit frame, until its taker takes it.
	fit_frame: Option<(ScratchUse, Arc<ScratchResult>)>,
}

impl<B: Backend> LiveController<B> {
	pub fn new(backend: B, settings: LiveSettings) -> Self {
		Self {
			backend,
			settings,
			playback: Playback::default(),
			run: None,
			live: false,
			scratch_due: None,
			scratch_in_flight: None,
			rerun_due: None,
			shown: None,
			warning: None,
			export: Export::Idle,
			last_edit: None,
			notices: Vec::new(),
			latest_scratch: None,
			fit_frame: None,
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

	/// New settings; a changed preview quality is a simulation edit, scheduled by the caller through
	/// [`on_change`](Self::on_change).
	pub fn set_settings(&mut self, settings: LiveSettings) {
		self.settings = settings;
		if !settings.auto_rerun {
			self.rerun_due = None;
		}
	}

	pub fn take_notices(&mut self) -> Vec<Notice> {
		std::mem::take(&mut self.notices)
	}

	/// What the pictures show; `None` before the first frame of a project.
	pub fn shown(&self) -> Option<Shown> {
		self.shown
	}

	/// The newest scratch frame shown for the position.
	pub fn latest_scratch(&self) -> Option<&Arc<ScratchResult>> {
		self.latest_scratch.as_ref()
	}

	/// The scratch frame whose probe traces the graphs show: the one shown, or the one for the run's frame shown
	/// while the run's recordings are stale.
	pub fn graph_scratch(&self) -> Option<&Arc<ScratchResult>> {
		let r = self.latest_scratch.as_ref()?;
		match self.shown? {
			Shown::Scratch(_) => Some(r),
			Shown::Run(k) => (r.frame == k && self.run.as_ref().is_some_and(|run| run.probes_stale)).then_some(r),
		}
	}

	/// Calls `f` with the shown frame's shape and its plane of `layer`; `None` without a shown frame or such a plane.
	pub fn with_shown_plane<R>(&self, layer: Layer, f: impl FnOnce(&FrameShape, &[u8]) -> R) -> Option<R> {
		match self.shown? {
			Shown::Run(k) => {
				let store = self.run_store()?;
				let stored = self.run_spec()?.stored_index_of(k) as usize;
				store.with_layer(stored, layer, |plane| f(store.shape(), plane)).ok()
			}
			Shown::Scratch(_) => {
				let r = self.latest_scratch.as_ref()?;
				let plane = r.snapshot.plane(layer);
				(!plane.is_empty()).then(|| f(&r.snapshot.shape, plane))
			}
		}
	}

	/// The timeline frame a scratch request is computing.
	pub fn computing_frame(&self) -> Option<u32> {
		self.scratch_in_flight.map(|(_, frame, _)| frame)
	}

	/// The timeline frame a scratch request computes for the pictures (not for the probe graphs alone).
	pub fn computing_picture(&self) -> Option<u32> {
		self.scratch_in_flight.and_then(|(_, frame, used)| (used == ScratchUse::Picture).then_some(frame))
	}

	/// Calls `f` with the current run's probe traces (empty without a run).
	pub fn with_run_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		match &self.run {
			Some(run) => run.handle.with_traces(f),
			None => f(&[]),
		}
	}

	/// Live follow of an active run.
	pub fn is_live(&self) -> bool {
		self.live
	}

	pub fn run_is_active(&self) -> bool {
		self.run.as_ref().is_some_and(ActiveRun::is_active)
	}

	/// Whether frames keep arriving: a run is active and the user did not pause it.
	pub fn run_is_progressing(&self) -> bool {
		self.run.as_ref().is_some_and(|r| r.is_active() && !r.user_paused)
	}

	pub fn run_is_stale(&self) -> bool {
		self.run.as_ref().is_some_and(|r| r.stale)
	}

	pub fn run_kind(&self) -> Option<RunKind> {
		self.run.as_ref().map(|r| r.kind)
	}

	/// (state, steps done, total steps, stale) of the run, for the status bar.
	pub fn run_status(&self) -> Option<(RunState, u32, u32, bool)> {
		self.run.as_ref().map(|r| (r.handle.state(), r.handle.steps_done(), r.spec.steps, r.stale))
	}

	pub fn run_store(&self) -> Option<Arc<FrameStore>> {
		self.run.as_ref().and_then(|r| r.handle.store())
	}

	pub fn run_spec(&self) -> Option<&RunSpec> {
		self.run.as_ref().map(|r| &r.spec)
	}

	pub fn scratch_pending(&self) -> bool {
		self.scratch_due.is_some() || self.scratch_in_flight.is_some()
	}

	pub fn rerun_due(&self) -> Option<Instant> {
		self.rerun_due
	}

	pub fn engine_name(&self) -> String {
		self.backend.engine_name()
	}

	/// The memory warning waiting for an answer.
	pub fn warning(&self) -> Option<MemoryWarning<'_>> {
		self.warning.as_ref().map(|w| MemoryWarning { spec: &w.spec, budget: w.budget, auto_budget: w.auto_budget })
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

	fn run_can_show(&self, frame: u32) -> bool {
		self.run.as_ref().is_some_and(|r| !r.stale && r.frames() > frame)
	}

	fn needs_scratch(&self, frame: u32) -> bool {
		!self.run_can_show(frame) || self.run.as_ref().is_some_and(|r| r.probes_stale)
	}

	fn schedule_scratch(&mut self, at: Instant) {
		self.scratch_due = Some(at);
	}

	/// A new, opened, imported or closed project: the run is discarded, playback stops, the position goes to
	/// frame 0 and the first scratch frame warms the engine at once.
	pub fn project_replaced(&mut self, project: &Project, now: Instant) {
		self.discard_run();
		self.warning = None;
		self.export = Export::Idle;
		self.rerun_due = None;
		self.live = false;
		self.shown = None;
		self.latest_scratch = None;
		self.fit_frame = None;
		self.last_edit = None;
		self.playback.pause();
		self.playback.set_position(0);
		self.sync_timeline(project);
		self.playback.set_position(0);
		self.backend.cancel_scratch();
		self.scratch_in_flight = None;
		self.schedule_scratch(now);
	}

	fn discard_run(&mut self) {
		if let Some(run) = self.run.take() {
			run.handle.cancel();
		}
		self.playback.set_available(None);
		self.drop_waiting_export();
	}

	fn drop_waiting_export(&mut self) {
		if let Export::Waiting { .. } = self.export {
			self.export = Export::Idle;
			tracing::warn!("The export was dropped: its run stopped before the last frame.");
			self.notices.push(Notice::ExportStopped);
		}
	}

	/// A model change of class `kind` at `now`; `note` names the newest edit with its value.
	pub fn on_change(&mut self, kind: ChangeKind, project: &Project, note: Option<String>, now: Instant) {
		let kind = match kind {
			ChangeKind::Brightness if self.settings.quality.samples_at_lowest() => ChangeKind::Simulation,
			ChangeKind::Brightness => ChangeKind::Display,
			ChangeKind::ProbePresentation if self.unrecorded_probe(project) => ChangeKind::Simulation,
			k => k,
		};
		if note.is_some() {
			self.last_edit = note;
		}
		self.sync_timeline(project);
		match kind {
			ChangeKind::Simulation => {
				if let Some(run) = &mut self.run {
					run.stale = true;
					if run.is_active() {
						run.handle.cancel();
						tracing::debug!("run stopped by an edit");
					}
				}
				self.live = false;
				self.playback.pause();
				self.schedule_scratch(now + SCRATCH_DEBOUNCE);
				self.rerun_due = self.settings.auto_rerun.then(|| now + RERUN_QUIET);
			}
			ChangeKind::ProbePresentation => {
				if let Some(run) = &mut self.run {
					run.probes_stale = true;
				}
				if !self.playback.is_playing() {
					self.schedule_scratch(now + SCRATCH_DEBOUNCE);
				}
			}
			ChangeKind::Display | ChangeKind::Metadata | ChangeKind::None | ChangeKind::Brightness => {}
		}
	}

	/// An active probe the run does not record (switched on, added).
	fn unrecorded_probe(&self, project: &Project) -> bool {
		self.run.as_ref().is_some_and(|r| {
			project
				.probes
				.iter()
				.filter(|p| p.active)
				.any(|p| !r.recorded.iter().any(|n| n.eq_ignore_ascii_case(&p.name)))
		})
	}

	/// A recorded probe was renamed: it stays recorded.
	pub fn probe_renamed(&mut self, old: &str, new: &str) {
		if let Some(run) = &mut self.run {
			for n in run.recorded.iter_mut().filter(|n| n.eq_ignore_ascii_case(old)) {
				*n = new.to_owned();
			}
		}
	}

	/// Advances the machine to `now`: run progress and its end, playback, scratch results and requests, the
	/// automatic re-run.
	pub fn tick(&mut self, now: Instant, project: &Project) {
		self.poll_run();
		self.advance_playback(now);
		self.poll_scratch();
		self.show_run_frame();
		self.request_due_scratch(now, project);
		self.maybe_rerun(now, project);
	}

	fn poll_run(&mut self) {
		let Some(run) = &mut self.run else {
			return;
		};
		let frames = run.frames();
		self.playback.set_available(Some(Available { frames, stale: run.stale }));
		if self.live && run.kind == RunKind::User && frames > 0 {
			self.playback.set_position(frames - 1);
		}
		if run.handle.is_finished() && !run.ended {
			run.ended = true;
			let state = run.handle.state();
			if self.live && frames > 0 {
				self.playback.set_position(frames - 1);
			}
			self.live = false;
			let export_ready = match self.export {
				Export::Waiting { needs_intensity } => Some(run.is_full_for_export(needs_intensity)),
				_ => None,
			};
			if let RunState::Failed(error) = &state {
				self.notices.push(Notice::Error(error.to_string()));
			}
			self.notices.push(Notice::RunEnded(state));
			match export_ready {
				Some(true) => {
					self.export = Export::Reading;
					self.notices.push(Notice::ExportReady);
				}
				Some(false) => self.drop_waiting_export(),
				None => {}
			}
		}
	}

	fn advance_playback(&mut self, now: Instant) {
		if !self.playback.is_playing() {
			return;
		}
		let limit = self.play_limit();
		self.playback.advance(now, Some(limit));
	}

	/// The newest frame playback may reach: the run's newest while it runs, its last once it ended.
	fn play_limit(&self) -> Limit {
		match &self.run {
			Some(r) if r.is_active() => Limit::Hold(r.frames().saturating_sub(1)),
			Some(r) => Limit::End(r.frames().saturating_sub(1)),
			None => Limit::Hold(self.playback.position()),
		}
	}

	fn poll_scratch(&mut self) {
		for outcome in self.backend.poll_scratch() {
			let Some((id, frame, used)) = self.scratch_in_flight else {
				continue;
			};
			match outcome {
				ScratchOutcome::Done { id: done, result, .. } if done == id => {
					self.scratch_in_flight = None;
					let result = result.map(Arc::from);
					match used {
						ScratchUse::Fit | ScratchUse::OpenFit => self.fit_frame = result.map(|r| (used, r)),
						ScratchUse::Picture | ScratchUse::Graphs if frame == self.playback.position() => {
							self.latest_scratch = result;
							if used == ScratchUse::Picture && !self.run_can_show(frame) {
								self.shown = Some(Shown::Scratch(frame));
								self.notices.push(Notice::ShowScratch(frame));
							}
						}
						ScratchUse::Picture | ScratchUse::Graphs => {}
					}
					self.resume_after_scratch();
				}
				ScratchOutcome::Failed { id: failed, message } if failed == id => {
					self.scratch_in_flight = None;
					tracing::warn!("preview frame {frame} failed: {message}");
					self.notices.push(Notice::Error(message));
					self.resume_after_scratch();
				}
				ScratchOutcome::Abandoned { id: abandoned } if abandoned == id => {
					self.scratch_in_flight = None;
					self.resume_after_scratch();
				}
				_ => {}
			}
		}
	}

	fn resume_after_scratch(&mut self) {
		if let Some(run) = &mut self.run
			&& run.scratch_paused
		{
			run.scratch_paused = false;
			if !run.user_paused {
				run.handle.resume();
			}
		}
	}

	/// A picture changes only when a frame for its position arrives.
	fn show_run_frame(&mut self) {
		let position = self.playback.position();
		if self.run_can_show(position) && self.shown != Some(Shown::Run(position)) {
			self.shown = Some(Shown::Run(position));
			self.notices.push(Notice::ShowRunFrame(position));
		}
	}

	fn request_due_scratch(&mut self, now: Instant, project: &Project) {
		let Some(due) = self.scratch_due else {
			return;
		};
		if now < due || self.playback.is_playing() {
			return;
		}
		self.scratch_due = None;
		let frame = self.playback.position();
		if !self.needs_scratch(frame) {
			return;
		}
		let spec = match self.backend.spec(project, self.settings.quality, false) {
			Ok(spec) => spec,
			Err(e) => {
				tracing::warn!("no preview frame: {e}");
				self.notices.push(Notice::Error(e));
				return;
			}
		};
		let used = if self.run_can_show(frame) { ScratchUse::Graphs } else { ScratchUse::Picture };
		self.request_scratch(spec, frame, used);
	}

	/// Pauses an active run (not one the user paused) and asks for `frame` of `spec`.
	fn request_scratch(&mut self, spec: RunSpec, frame: u32, used: ScratchUse) {
		if let Some(run) = &mut self.run
			&& run.is_active()
			&& !run.user_paused
			&& !run.scratch_paused
		{
			run.handle.pause();
			run.scratch_paused = true;
		}
		let id = self.backend.request_scratch(spec, frame);
		tracing::debug!(
			"preview frame {frame} requested (id {id}); newest edit: {}",
			self.last_edit.take().as_deref().unwrap_or("none")
		);
		self.scratch_in_flight = Some((id, frame, used));
		self.notices.push(Notice::Computing(frame));
	}

	/// Auto brightness with 8-bit samples: the frame at the position once more with 16-bit samples, for
	/// [`take_half_frame`](Self::take_half_frame) alone. Returns whether it was requested.
	pub fn request_half_frame(&mut self, project: &Project) -> bool {
		self.request_16_bit(project, ScratchUse::Fit)
	}

	/// The frame [`request_half_frame`](Self::request_half_frame) asked for, once it arrived.
	pub fn take_half_frame(&mut self) -> Option<Arc<ScratchResult>> {
		self.take_fit_frame(ScratchUse::Fit)
	}

	/// An opened project's brightness fit with 8-bit samples: the frame at the position with 16-bit samples, for
	/// [`take_open_fit`](Self::take_open_fit) alone. Returns whether it was requested.
	pub fn request_open_fit(&mut self, project: &Project) -> bool {
		self.request_16_bit(project, ScratchUse::OpenFit)
	}

	/// The frame [`request_open_fit`](Self::request_open_fit) asked for, once it arrived.
	pub fn take_open_fit(&mut self) -> Option<Arc<ScratchResult>> {
		self.take_fit_frame(ScratchUse::OpenFit)
	}

	fn take_fit_frame(&mut self, used: ScratchUse) -> Option<Arc<ScratchResult>> {
		self.fit_frame.take_if(|(u, _)| *u == used).map(|(_, r)| r)
	}

	fn request_16_bit(&mut self, project: &Project, used: ScratchUse) -> bool {
		let quality = PreviewQuality { format: SampleFormat::Half, ..self.settings.quality };
		match self.backend.spec(project, quality, false) {
			Ok(spec) => {
				self.scratch_due = None;
				self.request_scratch(spec, self.playback.position(), used);
				true
			}
			Err(e) => {
				tracing::warn!("no 16-bit frame for the brightness fit: {e}");
				false
			}
		}
	}

	fn maybe_rerun(&mut self, now: Instant, project: &Project) {
		let due = self.rerun_due.is_some_and(|d| now >= d);
		if !due
			|| self.scratch_pending()
			|| self.warning.is_some()
			|| self.export == Export::Reading
			|| self.run_is_active()
		{
			return;
		}
		self.rerun_due = None;
		self.start(RunKind::Automatic, project, now);
	}

	/// Starts a preview run of `kind` (not Export), asking first when its frames do not fit in the RAM cache.
	/// Returns whether it started (false: failed or the warning asks).
	pub fn start(&mut self, kind: RunKind, project: &Project, now: Instant) -> bool {
		let spec = match self.backend.spec(project, self.settings.quality, false) {
			Ok(spec) => spec,
			Err(e) => {
				tracing::error!("the simulation could not start: {e}");
				self.notices.push(Notice::RunFailed { no_image: e.contains("no image"), message: e });
				return false;
			}
		};
		if self.settings.warn_when_ram_insufficient {
			let reclaimed = self.run_store().map_or(0, |s| s.ram_bytes());
			let (budget, auto_budget) = self.backend.budgets(self.settings.ram_percent, reclaimed);
			if MemoryWarning::check(&spec, budget, auto_budget).is_some() {
				tracing::debug!(need = spec.store_bytes(), budget, "memory warning asks before the run");
				self.warning = Some(PendingWarning { spec, budget, auto_budget, kind });
				self.notices.push(Notice::AskMemoryWarning);
				return false;
			}
		}
		self.launch(spec, kind, now)
	}

	fn launch(&mut self, spec: RunSpec, kind: RunKind, now: Instant) -> bool {
		self.discard_run();
		self.rerun_due = None;
		let recorded = spec.probes.iter().map(|p| p.name.clone()).collect();
		let handle = match self.backend.start_run(spec.clone(), self.settings.ram_percent) {
			Ok(h) => h,
			Err(e) => {
				tracing::error!("the simulation could not start: {e}");
				self.notices.push(Notice::RunFailed { message: e, no_image: false });
				return false;
			}
		};
		tracing::debug!(?kind, steps = spec.steps, frames = spec.frame_count(), "run started");
		self.run = Some(ActiveRun {
			handle,
			spec,
			kind,
			stale: false,
			probes_stale: false,
			recorded,
			user_paused: false,
			scratch_paused: false,
			ended: false,
		});
		match kind {
			RunKind::User => {
				self.playback.pause();
				self.playback.set_position(0);
				self.live = true;
			}
			RunKind::Play => {
				self.live = false;
				self.playback.play(now, None);
			}
			RunKind::Automatic | RunKind::Export => {}
		}
		self.notices.push(Notice::RunStarted(kind));
		true
	}

	/// The memory warning's Run: the levers' quality and cache for this and later runs (the caller persists
	/// the returned choice), then the run.
	pub fn confirm_warning(&mut self, levers: Levers, mute: bool, now: Instant) -> Option<WarningChoice> {
		let w = self.warning.take()?;
		let warning = MemoryWarning { spec: &w.spec, budget: w.budget, auto_budget: w.auto_budget };
		let quality = warning.quality_of(levers);
		let choice = WarningChoice { quality, cache_auto: levers.cache_auto, mute };
		self.settings.quality = quality;
		if levers.cache_auto {
			self.settings.ram_percent = 0;
		}
		if mute {
			self.settings.warn_when_ram_insufficient = false;
		}
		let spec = w.spec.with_quality(quality);
		self.launch(spec, w.kind, now);
		Some(choice)
	}

	/// The memory warning's Cancel: nothing changes, the stale run stays, playback is off.
	pub fn cancel_warning(&mut self) {
		if self.warning.take().is_some() {
			self.playback.pause();
			self.rerun_due = None;
			tracing::info!("The run was not started; the preview keeps the frames it has.");
			self.notices.push(Notice::Info("The run was not started.".into()));
		}
	}

	/// F5 / Run: a full run from step 0, followed.
	pub fn run_user(&mut self, project: &Project, now: Instant) -> bool {
		self.start(RunKind::User, project, now)
	}

	/// Stop / Esc: the frames so far stay, the preview on the newest one, Live off.
	pub fn stop_run(&mut self) {
		if let Some(run) = &self.run
			&& run.is_active()
		{
			run.handle.cancel();
			let frames = run.frames();
			if self.live && frames > 0 {
				self.playback.set_position(frames - 1);
			}
			self.live = false;
		}
	}

	/// F6: pauses or resumes the run (a run the user paused stays paused through scratch frames).
	pub fn toggle_pause_run(&mut self) {
		if let Some(run) = &mut self.run
			&& run.is_active()
		{
			run.user_paused = !run.user_paused;
			if run.user_paused {
				run.handle.pause();
			} else if !run.scratch_paused {
				run.handle.resume();
			}
		}
	}

	pub fn run_paused_by_user(&self) -> bool {
		self.run.as_ref().is_some_and(|r| r.user_paused)
	}

	/// F or the Live button: only while a run is active.
	pub fn set_live(&mut self, on: bool) {
		self.live = on && self.run_is_active();
	}

	/// A click or drag on the timeline: the position only; playback pauses, Live turns off. A drag asks for a
	/// scratch frame every [`SCRATCH_DEBOUNCE`] instead of after it, so the picture follows the drag.
	pub fn scrub(&mut self, frame: u32, now: Instant) {
		self.playback.pause();
		self.live = false;
		self.playback.set_position(frame);
		if self.needs_scratch(self.playback.position()) {
			self.scratch_due.get_or_insert(now + SCRATCH_DEBOUNCE);
		}
	}

	/// Arrows, PageUp/PageDown, Home/End as a frame delta; scrub rules.
	pub fn step_frames(&mut self, delta: i64, now: Instant) {
		let target = (self.playback.position() as i64 + delta).max(0) as u32;
		self.scrub(target, now);
	}

	/// Play: "show me the simulation". Starts an automatic run when there is none or it is stale, and plays
	/// as its frames arrive.
	pub fn play(&mut self, project: &Project, now: Instant) {
		if self.playback.is_playing() {
			return;
		}
		self.live = false;
		let needs_run = self.run.as_ref().is_none_or(|r| r.stale);
		if needs_run {
			self.start(RunKind::Play, project, now);
		} else {
			let end = match self.play_limit() {
				Limit::End(end) => Some(end),
				Limit::Hold(_) => None,
			};
			self.playback.play(now, end);
		}
	}

	/// Pause: a position the store cannot show gets a scratch frame.
	pub fn pause_playback(&mut self, now: Instant) {
		self.playback.pause();
		if self.needs_scratch(self.playback.position()) {
			self.schedule_scratch(now);
		}
	}

	/// An export asks for its run: ready when the current one is fresh, complete and full quality (with the
	/// intensity when `needs_intensity`), else a full-quality run starts (never with the memory warning).
	pub fn request_export(&mut self, project: &Project, needs_intensity: bool, now: Instant) -> ExportGate {
		if self.export_busy() {
			return ExportGate::Busy;
		}
		if self.run.as_ref().is_some_and(|r| r.is_full_for_export(needs_intensity)) {
			self.export = Export::Reading;
			return ExportGate::Ready;
		}
		let spec = match self.backend.spec(project, PreviewQuality::FULL, needs_intensity) {
			Ok(spec) => spec,
			Err(e) => {
				self.notices.push(Notice::Error(e));
				return ExportGate::Failed;
			}
		};
		if self.launch(spec, RunKind::Export, now) {
			self.export = Export::Waiting { needs_intensity };
			ExportGate::Waiting
		} else {
			ExportGate::Failed
		}
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
			if let Some(run) = &self.run
				&& run.kind == RunKind::Export
			{
				run.handle.cancel();
			}
		}
	}
}

impl<B: Backend> Drop for LiveController<B> {
	fn drop(&mut self) {
		self.discard_run();
		self.backend.cancel_scratch();
	}
}

/// The core's runs and scratch frames on the CPU or GPU engine, by the Use the GPU setting.
pub struct CoreBackend {
	use_gpu: Arc<AtomicBool>,
	cache_dir: PathBuf,
	scratch: ScratchRunner,
	scratch_rx: Receiver<ScratchEvent>,
	scene: Arc<Mutex<SceneCache>>,
}

/// Builds the engine for a scene: the GPU's when asked for and available (a fallback to the CPU is logged).
pub fn build_engine(scene: Arc<SceneData>, options: EngineOptions, use_gpu: bool) -> Box<dyn WaveEngine> {
	let e = wavedude_core::engine::build(scene, options, use_gpu);
	super::log::engine_built(e.name(), e.scene(), e.options(), e.sub_steps());
	e
}

struct CoreRun(SimulationRun);

impl RunHandle for CoreRun {
	fn state(&self) -> RunState {
		self.0.state()
	}

	fn steps_done(&self) -> u32 {
		self.0.steps_done()
	}

	fn frame_count(&self) -> u32 {
		self.0.frame_count()
	}

	fn is_finished(&self) -> bool {
		self.0.is_finished()
	}

	fn pause(&self) {
		self.0.pause();
	}

	fn resume(&self) {
		self.0.resume();
	}

	fn cancel(&self) {
		self.0.cancel();
	}

	fn store(&self) -> Option<Arc<FrameStore>> {
		Some(self.0.store().clone())
	}

	fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		self.0.with_traces(|t| f(t));
	}
}

impl CoreBackend {
	/// A backend with its spill files in `cache_dir`; the engine follows `use_gpu`, shared with the settings.
	pub fn new(use_gpu: Arc<AtomicBool>, cache_dir: PathBuf) -> std::io::Result<Self> {
		let (tx, scratch_rx) = channel();
		let tx = Mutex::new(tx);
		let gpu = use_gpu.clone();
		let scratch = ScratchRunner::new(
			Box::new(move |scene, options| build_engine(scene, options, gpu.load(Ordering::Relaxed))),
			Box::new(move |event| {
				let _ = tx.lock().unwrap().send(event);
			}),
		)?;
		Ok(Self { use_gpu, cache_dir, scratch, scratch_rx, scene: Arc::default() })
	}

	/// The scene pixels and medium the specs are built from, shared with whatever else shows the scene.
	pub fn scene_cache(&self) -> Arc<Mutex<SceneCache>> {
		self.scene.clone()
	}
}

impl Backend for CoreBackend {
	fn spec(&mut self, project: &Project, quality: PreviewQuality, track_intensity: bool) -> Result<RunSpec, String> {
		let mut scene = self.scene.lock().unwrap_or_else(PoisonError::into_inner);
		scene.spec(project, quality, track_intensity).map_err(|e| e.to_string())
	}

	fn start_run(&mut self, spec: RunSpec, ram_percent: u32) -> Result<Box<dyn RunHandle>, String> {
		let budget = Budget::compute(0, ram_percent);
		let store =
			FrameStore::in_dir(spec.store_layout(), budget, None, &self.cache_dir).map_err(|e| e.to_string())?;
		let engine = build_engine(spec.scene.clone(), spec.options.clone(), self.use_gpu.load(Ordering::Relaxed));
		let run = SimulationRun::start(spec, Arc::new(store), engine, None).map_err(|e| e.to_string())?;
		Ok(Box::new(CoreRun(run)))
	}

	fn request_scratch(&mut self, spec: RunSpec, frame: u32) -> u64 {
		self.scratch.request(spec, frame)
	}

	fn cancel_scratch(&mut self) {
		self.scratch.cancel();
	}

	fn poll_scratch(&mut self) -> Vec<ScratchOutcome> {
		self.scratch_rx
			.try_iter()
			.map(|event| match event {
				ScratchEvent::Done(result) => {
					ScratchOutcome::Done { id: result.id, frame: result.frame, result: Some(result) }
				}
				ScratchEvent::Abandoned { id, .. } => ScratchOutcome::Abandoned { id },
				ScratchEvent::Failed { id, error } => ScratchOutcome::Failed { id, message: error.to_string() },
			})
			.collect()
	}

	fn budgets(&self, ram_percent: u32, reclaimed: u64) -> (u64, u64) {
		(Budget::compute(reclaimed, ram_percent), Budget::compute(reclaimed, 0))
	}

	fn engine_name(&self) -> String {
		if self.use_gpu.load(Ordering::Relaxed)
			&& let Ok(ctx) = wavedude_core::engine::gpu::context()
		{
			return ctx.info.name.clone();
		}
		let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
		format!("CPU ({threads} threads)")
	}
}

/// A scripted backend: runs and scratch frames that the caller advances by hand. For tests and headless tools.
pub mod fake {
	use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
	use std::sync::{Arc, Mutex};

	use std::time::Duration;

	use wavedude_core::engine::{SampleFormat, Snapshot};
	use wavedude_core::f16;
	use wavedude_core::model::Project;
	use wavedude_core::render::{field_index, intensity_root_index};
	use wavedude_core::run::{PreviewQuality, RunSpec, RunState, ScratchResult};

	use super::{Backend, RunHandle, ScratchOutcome};

	/// The shared state of a fake run.
	#[derive(Debug)]
	pub struct FakeRun {
		pub spec: RunSpec,
		pub frames: AtomicU32,
		pub state: Mutex<RunState>,
		pub paused: AtomicBool,
	}

	impl FakeRun {
		/// Publishes `frames` timeline frames.
		pub fn publish(&self, frames: u32) {
			self.frames.store(frames.min(self.spec.frame_count()), Ordering::Release);
		}

		/// Completes the run with every frame.
		pub fn complete(&self) {
			self.publish(self.spec.frame_count());
			*self.state.lock().unwrap() = RunState::Completed;
		}

		pub fn is_paused(&self) -> bool {
			self.paused.load(Ordering::Acquire)
		}
	}

	struct Handle(Arc<FakeRun>);

	impl RunHandle for Handle {
		fn state(&self) -> RunState {
			self.0.state.lock().unwrap().clone()
		}

		fn steps_done(&self) -> u32 {
			self.0.frames.load(Ordering::Acquire) * self.0.spec.visualize_every
		}

		fn frame_count(&self) -> u32 {
			self.0.frames.load(Ordering::Acquire)
		}

		fn is_finished(&self) -> bool {
			self.state().is_finished()
		}

		fn pause(&self) {
			self.0.paused.store(true, Ordering::Release);
		}

		fn resume(&self) {
			self.0.paused.store(false, Ordering::Release);
		}

		fn cancel(&self) {
			let mut state = self.0.state.lock().unwrap();
			if !state.is_finished() {
				*state = RunState::Cancelled;
			}
		}
	}

	/// Records every run and scratch request; the caller completes them.
	#[derive(Default)]
	pub struct FakeBackend {
		pub runs: Vec<Arc<FakeRun>>,
		/// (id, frame) of every scratch request.
		pub scratch_requests: Vec<(u64, u32)>,
		/// Outcomes handed out at the next poll.
		pub outcomes: Vec<ScratchOutcome>,
		/// (budget, budget at Auto); `u64::MAX` when unset.
		pub budgets: Option<(u64, u64)>,
		pub scratch_cancels: u32,
		/// The spec of the newest scratch request.
		pub scratch_spec: Option<RunSpec>,
	}

	impl FakeBackend {
		pub fn last_run(&self) -> &Arc<FakeRun> {
			self.runs.last().expect("a run was started")
		}

		/// Completes the newest scratch request.
		pub fn finish_scratch(&mut self) {
			if let Some(&(id, frame)) = self.scratch_requests.last() {
				self.outcomes.push(ScratchOutcome::Done { id, frame, result: None });
			}
		}

		/// Completes the newest scratch request with a frame of uniform samples in its spec's shape: the field
		/// `u` and the intensity `i` as halves (sqrt(I) when the shape says so) or as palette indices.
		pub fn finish_scratch_uniform(&mut self, u: f32, i: f32) {
			let (Some(&(id, frame)), Some(spec)) = (self.scratch_requests.last(), self.scratch_spec.clone()) else {
				return;
			};
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
			let snapshot = Snapshot { shape, step: spec.step_of(frame), field, intensity };
			let result = ScratchResult {
				id,
				spec,
				frame,
				snapshot: Arc::new(snapshot),
				traces: Arc::new([]),
				elapsed: Duration::ZERO,
			};
			self.outcomes.push(ScratchOutcome::Done { id, frame, result: Some(Box::new(result)) });
		}
	}

	impl Backend for FakeBackend {
		fn spec(
			&mut self,
			project: &Project,
			quality: PreviewQuality,
			track_intensity: bool,
		) -> Result<RunSpec, String> {
			RunSpec::create(project, None, quality, track_intensity).map_err(|e| e.to_string())
		}

		fn start_run(&mut self, spec: RunSpec, _ram_percent: u32) -> Result<Box<dyn RunHandle>, String> {
			let run = Arc::new(FakeRun {
				spec,
				frames: AtomicU32::new(0),
				state: Mutex::new(RunState::Running),
				paused: AtomicBool::new(false),
			});
			self.runs.push(run.clone());
			Ok(Box::new(Handle(run)))
		}

		fn request_scratch(&mut self, spec: RunSpec, frame: u32) -> u64 {
			self.scratch_spec = Some(spec);
			let id = self.scratch_requests.len() as u64 + 1;
			self.scratch_requests.push((id, frame));
			id
		}

		fn cancel_scratch(&mut self) {
			self.scratch_cancels += 1;
		}

		fn poll_scratch(&mut self) -> Vec<ScratchOutcome> {
			std::mem::take(&mut self.outcomes)
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
		c.backend_mut().finish_scratch();
		c.tick(t, p);
		c.take_notices();
		c
	}

	fn ms(t: Instant, ms: u64) -> Instant {
		t + Duration::from_millis(ms)
	}

	#[test]
	fn a_replaced_project_warms_the_engine_at_frame_zero() {
		let t = Instant::now();
		let p = project();
		let mut c = LiveController::new(FakeBackend::default(), LiveSettings::default());
		c.project_replaced(&p, t);
		c.tick(t, &p);
		assert_eq!(c.backend().scratch_requests, [(1, 0)]);
		assert!(c.backend().runs.is_empty(), "opening never runs");
	}

	#[test]
	fn debounce_then_rerun_after_quiet() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.on_change(ChangeKind::Simulation, &p, Some("Source 1.frequency = 120".into()), ms(t, 1000));
		c.tick(ms(t, 1100), &p);
		assert_eq!(c.backend().scratch_requests.len(), 1, "not before 120 ms");
		c.on_change(ChangeKind::Simulation, &p, None, ms(t, 1100));
		c.tick(ms(t, 1219), &p);
		assert_eq!(c.backend().scratch_requests.len(), 1, "the newest edit restarts the debounce");
		c.tick(ms(t, 1220), &p);
		assert_eq!(c.backend().scratch_requests.len(), 2);
		c.backend_mut().finish_scratch();
		c.tick(ms(t, 2599), &p);
		assert!(c.backend().runs.is_empty(), "not before 1.5 s of quiet");
		c.tick(ms(t, 2600), &p);
		assert_eq!(c.backend().runs.len(), 1);
		assert_eq!(c.run_kind(), Some(RunKind::Automatic));
		assert_eq!(c.playback.position(), 0, "an automatic run never moves the preview");
		c.on_change(ChangeKind::Simulation, &p, None, ms(t, 3000));
		c.tick(ms(t, 4600), &p);
		assert_eq!(c.backend().runs.len(), 1, "the re-run waits for the scratch frame");
		c.backend_mut().finish_scratch();
		c.tick(ms(t, 4601), &p);
		assert_eq!(c.backend().runs.len(), 2);
	}

	#[test]
	fn an_edit_stops_the_run_and_marks_it_stale() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		assert!(c.run_user(&p, t));
		assert!(c.is_live());
		c.backend().last_run().publish(30);
		c.tick(ms(t, 10), &p);
		assert_eq!(c.playback.position(), 29, "Live follows the newest frame");
		c.on_change(ChangeKind::Simulation, &p, None, ms(t, 20));
		assert!(c.run_is_stale() && !c.is_live());
		assert_eq!(*c.backend().last_run().state.lock().unwrap(), RunState::Cancelled);
		c.tick(ms(t, 200), &p);
		assert_eq!(c.backend().scratch_requests.last().unwrap().1, 29, "the stale run cannot show the position");
	}

	#[test]
	fn probe_edit_refreshes_but_never_reruns() {
		let t = Instant::now();
		let mut p = project();
		p.add_probe();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend().last_run().complete();
		c.tick(t, &p);
		let requests = c.backend().scratch_requests.len();
		c.on_change(ChangeKind::ProbePresentation, &p, None, ms(t, 100));
		c.tick(ms(t, 300), &p);
		assert_eq!(c.backend().scratch_requests.len(), requests + 1, "graphs refresh from a scratch frame");
		c.backend_mut().finish_scratch();
		c.tick(ms(t, 5000), &p);
		assert_eq!(c.backend().runs.len(), 1, "a probe edit never starts a run");
		assert!(!c.run_is_stale());
		p.add_probe();
		c.on_change(ChangeKind::ProbePresentation, &p, None, ms(t, 6000));
		assert!(c.run_is_stale(), "an active probe the run does not record is a simulation edit");
	}

	#[test]
	fn a_scratch_frame_for_the_graphs_leaves_the_run_frame_shown() {
		let t = Instant::now();
		let mut p = project();
		p.add_probe();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend().last_run().complete();
		c.scrub(40, t);
		c.tick(ms(t, 10), &p);
		assert_eq!(c.shown(), Some(Shown::Run(40)));
		c.on_change(ChangeKind::ProbePresentation, &p, None, ms(t, 20));
		c.tick(ms(t, 200), &p);
		assert_eq!((c.computing_frame(), c.computing_picture()), (Some(40), None), "the pictures do not wait");
		c.backend_mut().finish_scratch_uniform(0.5, 0.25);
		c.tick(ms(t, 210), &p);
		assert_eq!(c.shown(), Some(Shown::Run(40)), "the run's frame stays: no second picture");
		assert!(c.graph_scratch().is_some_and(|r| r.frame == 40), "the graphs take the scratch frame's traces");
		assert!(!c.take_notices().contains(&Notice::ShowScratch(40)));
	}

	#[test]
	fn the_half_frame_reaches_the_fit_over_a_shown_run_frame() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend().last_run().complete();
		c.scrub(40, t);
		c.tick(ms(t, 10), &p);
		assert!(c.request_half_frame(&p));
		assert_eq!(c.computing_picture(), None, "the pictures keep the run's frame");
		c.backend_mut().finish_scratch_uniform(0.5, 0.25);
		c.tick(ms(t, 20), &p);
		assert_eq!(c.shown(), Some(Shown::Run(40)));
		assert!(c.take_open_fit().is_none(), "only its own taker gets it");
		let frame = c.take_half_frame().expect("the 16-bit frame");
		assert_eq!((frame.frame, frame.snapshot.shape.format), (40, SampleFormat::Half));
		assert!(c.take_half_frame().is_none());
	}

	#[test]
	fn play_ends_at_the_last_frame_of_a_stopped_run() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend().last_run().publish(10);
		*c.backend().last_run().state.lock().unwrap() = RunState::Cancelled;
		c.tick(ms(t, 10), &p);
		c.play(&p, ms(t, 20));
		for i in 1..=60 {
			c.tick(ms(t, 20 + i * 17), &p);
		}
		assert_eq!((c.playback.position(), c.playback.is_playing()), (9, false));
	}

	#[test]
	fn no_rerun_is_due_with_auto_rerun_off() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.on_change(ChangeKind::Simulation, &p, None, t);
		assert!(c.rerun_due().is_some());
		c.set_settings(LiveSettings { auto_rerun: false, ..c.settings() });
		assert_eq!(c.rerun_due(), None, "turning it off drops the pending re-run");
		c.on_change(ChangeKind::Simulation, &p, None, t);
		assert_eq!(c.rerun_due(), None);
		c.set_settings(LiveSettings { auto_rerun: true, ..c.settings() });
		c.tick(ms(t, 2000), &p);
		assert_eq!(c.backend().runs.len(), 0, "turning it on fires no stale re-run");
	}

	#[test]
	fn a_second_export_is_refused_while_one_is_busy() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		let runs = c.backend().runs.len();
		assert_eq!(c.request_export(&p, false, t), ExportGate::Busy);
		assert_eq!(c.backend().runs.len(), runs, "no second run");
		assert!(c.export_busy());
	}

	#[test]
	fn a_scratch_frame_the_run_overtook_is_not_shown() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend().last_run().publish(10);
		c.scrub(50, t);
		c.tick(ms(t, 130), &p);
		assert_eq!(c.computing_picture(), Some(50));
		c.backend().last_run().publish(60);
		c.backend_mut().finish_scratch_uniform(0.5, 0.25);
		c.tick(ms(t, 140), &p);
		assert_eq!(c.shown(), Some(Shown::Run(50)));
		assert!(!c.take_notices().contains(&Notice::ShowScratch(50)));
	}

	#[test]
	fn scrub_pauses_and_turns_live_off() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend().last_run().publish(10);
		c.playback.play(t, None);
		c.scrub(50, ms(t, 5));
		assert!(!c.is_live() && !c.playback.is_playing());
		assert_eq!(c.playback.position(), 50);
		c.tick(ms(t, 130), &p);
		assert_eq!(c.backend().scratch_requests.last().unwrap().1, 50);
		assert!(c.backend().last_run().is_paused(), "the run is paused while a scratch frame computes");
		c.backend_mut().finish_scratch();
		c.tick(ms(t, 140), &p);
		assert!(!c.backend().last_run().is_paused());
		assert!(c.take_notices().contains(&Notice::ShowScratch(50)));
	}

	#[test]
	fn play_starts_a_run_and_holds_at_the_newest_frame() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.play(&p, t);
		assert_eq!(c.run_kind(), Some(RunKind::Play));
		assert!(c.playback.is_playing());
		c.backend().last_run().publish(5);
		c.tick(ms(t, 1000), &p);
		assert_eq!(c.playback.position(), 4);
		c.backend().last_run().complete();
		c.tick(ms(t, 1100), &p);
		assert!(c.playback.position() > 4);
	}

	#[test]
	fn memory_warning_run_and_cancel() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		let need = c.backend_mut().spec(&p, PreviewQuality::DEFAULT, false).unwrap().store_bytes();
		c.backend_mut().budgets = Some((need / 2, need / 2));
		assert!(!c.run_user(&p, t));
		assert!(c.take_notices().contains(&Notice::AskMemoryWarning));
		let proposal = c.warning().unwrap().proposal();
		assert!(proposal.frames || proposal.resolution);
		let choice = c.confirm_warning(proposal, false, t).unwrap();
		assert_ne!(choice.quality, PreviewQuality::DEFAULT);
		assert_eq!(c.backend().runs.len(), 1);
		assert_eq!(c.settings().quality, choice.quality);
		c.on_change(ChangeKind::Simulation, &p, None, t);
		c.backend_mut().budgets = Some((0, 0));
		c.play(&p, ms(t, 10));
		c.cancel_warning();
		assert!(!c.playback.is_playing(), "a cancelled warning turns playback off");
		assert!(c.run_is_stale(), "the stale run stays");
	}

	#[test]
	fn export_needs_a_full_quality_run() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);
		c.run_user(&p, t);
		c.backend().last_run().complete();
		c.tick(t, &p);
		assert_eq!(c.request_export(&p, true, t), ExportGate::Waiting, "an 8-bit preview run does not do");
		assert!(c.backend().last_run().spec.quality.is_full());
		c.backend().last_run().complete();
		c.tick(ms(t, 10), &p);
		assert!(c.take_notices().contains(&Notice::ExportReady));
		c.end_export();
		assert_eq!(c.request_export(&p, true, t), ExportGate::Ready);
	}

	#[test]
	fn a_waiting_export_never_outlives_its_run() {
		let t = Instant::now();
		let p = project();
		let mut c = controller(&p, t);

		assert_eq!(c.request_export(&p, false, t), ExportGate::Waiting);
		c.on_change(ChangeKind::Simulation, &p, None, ms(t, 10));
		c.tick(ms(t, 20), &p);
		assert!(c.take_notices().contains(&Notice::ExportStopped), "an edit stopped the export's run");
		assert!(!c.export_busy());

		assert_eq!(c.request_export(&p, false, ms(t, 30)), ExportGate::Waiting);
		c.run_user(&p, ms(t, 40));
		assert!(c.take_notices().contains(&Notice::ExportStopped), "F5 replaced the export's run");
		assert!(!c.export_busy());

		assert_eq!(c.request_export(&p, false, ms(t, 50)), ExportGate::Waiting);
		let run = c.backend().last_run().clone();
		c.cancel_export();
		assert!(!c.export_busy());
		assert_eq!(*run.state.lock().unwrap(), RunState::Cancelled, "a cancelled export stops its run");
		c.tick(ms(t, 60), &p);
		let notices = c.take_notices();
		assert!(!notices.contains(&Notice::ExportReady) && !notices.contains(&Notice::ExportStopped));
		c.on_change(ChangeKind::Simulation, &p, None, ms(t, 70));
		c.tick(ms(t, 70 + RERUN_QUIET.as_millis() as u64), &p);
		c.backend_mut().finish_scratch();
		c.tick(ms(t, 80 + RERUN_QUIET.as_millis() as u64), &p);
		assert_eq!(c.run_kind(), Some(RunKind::Automatic), "nothing holds the re-run back");
	}
}
