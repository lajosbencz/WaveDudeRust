//! An export's course for any window (docs/ui/70, Export Save dialog): the dialog's form and its plan, the questions
//! before replacing, the wait for the full-quality run, the job's progress, cancel, the end and File > Export last.
//! The window draws the form and acts on the [`Effect`]s; natively it runs each [`Job`] on a thread of its own and
//! hands its [`Msg`]s back, in the browser `Exports::poll` steps the job on the page.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ExportKind;
use crate::live::ExportGate;
use crate::platform::Instant;
#[cfg(target_arch = "wasm32")]
use crate::platform::{Download, H264Support, Spool};
use waveduderust::model::Layer;
use waveduderust::unstable::export::h264::Profile;
use waveduderust::unstable::export::{ExportFormat, naming};

#[cfg(not(target_arch = "wasm32"))]
use super::native as imp;
#[cfg(target_arch = "wasm32")]
use super::web as imp;
use super::{Outcome, Plan, Work};
#[cfg(target_arch = "wasm32")]
use super::{Runner, Started, Status};
use crate::App;
use crate::dialogs::filters;
use crate::menus::ExportKindChoice;

/// What the window does once the export has ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
	Exit,
	Restart,
}

/// An export's answered modal.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
	/// The dialog's OK.
	Chosen(Plan),
	/// Replacing the first `usize` of what is in the way was confirmed.
	#[cfg(not(target_arch = "wasm32"))]
	Confirmed(Plan, usize),
	/// Stop the export, then do `After`.
	Stop(After),
}

/// What the window shows or runs for an export.
pub enum Effect {
	/// Asks `text` (it destroys nothing); yes answers with `yes`.
	#[cfg(not(target_arch = "wasm32"))]
	Confirm { text: String, yes: Step },
	/// The finished export's first output, which the window may offer to show in its folder.
	#[cfg(not(target_arch = "wasm32"))]
	Exported(PathBuf),
	/// The failure's message window.
	Error(String),
	/// Runs the job on a thread of its own, its [`Msg`]s handed to [`Exports::take`].
	#[cfg(not(target_arch = "wasm32"))]
	Spawn(Box<Job>),
	/// Offers the completed export's file for its download, started by the user's click (decision 0025).
	#[cfg(target_arch = "wasm32")]
	Download(Download),
	/// Draws again at once: the job steps on the page.
	#[cfg(target_arch = "wasm32")]
	Repaint,
}

/// A running job's report.
#[cfg(not(target_arch = "wasm32"))]
pub enum Msg {
	/// `(done, total)` frames.
	Progress(usize, usize),
	Done(Outcome),
}

/// An export job for a worker thread.
#[cfg(not(target_arch = "wasm32"))]
pub struct Job {
	work: Work,
	plan: Plan,
	cancel: Arc<AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Job {
	/// Runs the export on the calling thread, reporting `(done, total)` frames.
	pub fn run(self, progress: impl FnMut(usize, usize)) -> Outcome {
		super::run(&self.work, self.plan, progress, &self.cancel)
	}
}

/// A run export and the project file it was made for.
type Last = (Plan, Option<PathBuf>);

/// A running or waiting export.
struct Progress {
	text: String,
	done: usize,
	total: usize,
	waiting: bool,
	cancel: Arc<AtomicBool>,
	kind: ExportKind,
	path: PathBuf,
	/// File > Export last once it completes.
	last: Option<Last>,
}

/// The export, the one waiting for its run, the last one (session state) and what waits for the end.
#[derive(Default)]
pub struct Exports {
	progress: Option<Progress>,
	pending: Option<Plan>,
	last: Option<Last>,
	after: Option<After>,
	/// The running browser export.
	#[cfg(target_arch = "wasm32")]
	runner: Option<Runner>,
	/// The last browser export's spool, which holds its download until the next export starts.
	#[cfg(target_arch = "wasm32")]
	spool: Option<Arc<Spool>>,
}

impl Exports {
	/// An export runs or waits for its run.
	pub fn exporting(&self) -> bool {
		self.progress.is_some()
	}

	/// The status line's text and share done, `None` while unknown.
	pub fn status(&self) -> Option<(&str, Option<f32>)> {
		self.progress.as_ref().map(|p| {
			let share = (!p.waiting && p.total != 0).then(|| p.done as f32 / p.total as f32);
			(p.text.as_str(), share)
		})
	}

	/// File > Export last has something to repeat for the open project.
	pub fn has_last(&self, app: &App) -> bool {
		self.last.as_ref().is_some_and(|(_, p)| *p == app.document.path)
	}

	/// File > Export last: the last run export of the open project again, no questions.
	pub fn export_last(&mut self, app: &mut App) -> Vec<Effect> {
		if app.live.export_busy() || !self.has_last(app) {
			return Vec::new();
		}
		match self.last.clone() {
			Some((Plan::Frames { choice, format, outputs, archive, frame, .. }, _)) => {
				self.ask(app, Plan::Frames { choice, format, outputs, archive, frame, repeat: true }, 0)
			}
			_ => Vec::new(),
		}
	}

	/// Does what an export's modal answered.
	pub fn step(&mut self, app: &mut App, step: Step) -> Vec<Effect> {
		match step {
			Step::Chosen(plan) => {
				if let Plan::Frames { choice, format, .. } = &plan
					&& !choice.is_still()
				{
					let index = ExportFormat::ALL.iter().position(|f| f == format).expect("a run format is listed");
					app.settings.export_filter_index = index as u32;
				}
				self.ask(app, plan, 0)
			}
			#[cfg(not(target_arch = "wasm32"))]
			Step::Confirmed(plan, confirmed) => self.ask(app, plan, confirmed),
			Step::Stop(after) => {
				self.after = Some(after);
				self.cancel(app);
				Vec::new()
			}
		}
	}

	/// Natively asks before replacing each thing in the way, one at a time, from the `confirmed`th on; a download
	/// replaces nothing.
	#[cfg_attr(target_arch = "wasm32", expect(unused_variables, reason = "a download replaces nothing"))]
	fn ask(&mut self, app: &mut App, plan: Plan, confirmed: usize) -> Vec<Effect> {
		#[cfg(not(target_arch = "wasm32"))]
		if let Some((key, target)) = super::would_replace(&plan).into_iter().nth(confirmed) {
			let text = app.strings.format(key, &[&target.display()]);
			return vec![Effect::Confirm { text, yes: Step::Confirmed(plan, confirmed + 1) }];
		}
		self.gate(app, plan)
	}

	fn gate(&mut self, app: &mut App, plan: Plan) -> Vec<Effect> {
		match app.live.request_export(
			&app.document.project,
			&app.document.view,
			super::run_needs(&plan),
			Instant::now(),
		) {
			ExportGate::Ready => self.run(app, plan),
			ExportGate::Waiting => {
				let (kind, path) = super::plan_kind(&plan);
				self.pending = Some(plan);
				self.progress = Some(Progress {
					text: app.strings.get("ExportStatusWaiting").to_owned(),
					done: 0,
					total: 0,
					waiting: true,
					cancel: Arc::default(),
					kind,
					path,
					last: None,
				});
				Vec::new()
			}
			ExportGate::Busy => {
				tracing::debug!("export refused: another export is busy");
				Vec::new()
			}
		}
	}

	/// The export of `plan` starts, its progress shown; the flag that cancels it.
	fn started(&mut self, app: &App, plan: &Plan) -> Arc<AtomicBool> {
		let last = match plan {
			Plan::Frames { choice, .. } if !choice.is_still() => Some((plan.clone(), app.document.path.clone())),
			_ => None,
		};
		let (kind, path) = super::plan_kind(plan);
		let cancel = Arc::new(AtomicBool::new(false));
		self.progress = Some(Progress {
			text: app.strings.format("ExportStatusFormat", &[&0, &0]),
			done: 0,
			total: 0,
			waiting: false,
			cancel: cancel.clone(),
			kind,
			path,
			last,
		});
		cancel
	}

	/// Starts the job of `plan`; an export without frames ends as failed.
	#[cfg(not(target_arch = "wasm32"))]
	fn run(&mut self, app: &mut App, plan: Plan) -> Vec<Effect> {
		let Some(work) = Work::of(app) else {
			return self.finished(app, Outcome::no_frames());
		};
		if super::fetches_codec(&plan, &work, app) {
			work.codec_download
				.set(Some(crate::downloads::start(app.strings.get("StatusDownloadingCodec").to_owned())));
		}
		let cancel = self.started(app, &plan);
		vec![Effect::Spawn(Box::new(Job { work, plan, cancel }))]
	}

	/// Starts `plan`, the last export's spool removed first; an export without frames ends as failed.
	#[cfg(target_arch = "wasm32")]
	fn run(&mut self, app: &mut App, plan: Plan) -> Vec<Effect> {
		let Some(work) = Work::of(app) else {
			return self.finished(app, Outcome::no_frames());
		};
		self.spool = None;
		let cancel = self.started(app, &plan);
		match super::start(&work, plan, cancel) {
			Ok(Started::Done(outcome, download)) => self.ended(app, outcome, download),
			Ok(Started::Running(runner)) => {
				self.runner = Some(runner);
				vec![Effect::Repaint]
			}
			Err(e) => self.finished(app, Outcome::Failed(e)),
		}
	}

	/// The running export did `(done, total)` frames.
	fn progressed(&mut self, app: &App, done: usize, total: usize) {
		if let Some(p) = &mut self.progress {
			(p.done, p.total) = (done, total);
			p.text = app.strings.format("ExportStatusFormat", &[&done, &total]);
		}
	}

	/// A job's report.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn take(&mut self, app: &mut App, msg: Msg) -> Vec<Effect> {
		match msg {
			Msg::Progress(done, total) => {
				self.progressed(app, done, total);
				Vec::new()
			}
			Msg::Done(outcome) => self.finished(app, outcome),
		}
	}

	/// The probe CSV's save dialog answered `path` for the probe named `probe` of the document `document`: the export
	/// goes on while that document shows and has the probe.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn csv_chosen(&mut self, app: &mut App, document: u64, probe: String, path: PathBuf) -> Vec<Effect> {
		if document == app.document_id() && app.document.project.probes.iter().any(|p| p.name == probe) {
			self.ask(app, Plan::Csv { probe, path }, 0)
		} else {
			Vec::new()
		}
	}

	/// Steps the running export.
	#[cfg(target_arch = "wasm32")]
	pub fn poll(&mut self, app: &mut App) -> Vec<Effect> {
		let Some(status) = self.runner.as_mut().map(Runner::poll) else { return Vec::new() };
		match status {
			Status::Working(done, total) => {
				self.progressed(app, done, total);
				vec![Effect::Repaint]
			}
			Status::Waiting(done, total) => {
				self.progressed(app, done, total);
				Vec::new()
			}
			Status::Packaging => {
				if let Some(p) = &mut self.progress {
					app.strings.get("ExportStatusPackaging").clone_into(&mut p.text);
				}
				Vec::new()
			}
			Status::Ended(outcome, download) => {
				self.spool = self.runner.take().and_then(|mut r| r.take_spool());
				self.ended(app, outcome, download)
			}
		}
	}

	/// The export's end; a completed export's file is offered for its download.
	#[cfg(target_arch = "wasm32")]
	fn ended(&mut self, app: &mut App, outcome: Outcome, download: Option<Download>) -> Vec<Effect> {
		let mut effects = self.finished(app, outcome);
		effects.extend(download.map(Effect::Download));
		effects
	}

	/// Cancels a running or waiting export; false when there is none.
	pub fn cancel(&mut self, app: &mut App) -> bool {
		match &self.progress {
			Some(p) if p.waiting => {
				app.live.cancel_export();
				self.stopped();
				true
			}
			Some(p) => {
				p.cancel.store(true, Ordering::Relaxed);
				true
			}
			None => false,
		}
	}

	/// The run of the waiting export has completed ([`crate::live::Notice::ExportReady`]).
	pub fn ready(&mut self, app: &mut App) -> Vec<Effect> {
		match self.pending.take() {
			Some(plan) => self.run(app, plan),
			None => {
				app.live.end_export();
				Vec::new()
			}
		}
	}

	/// The waiting export's run will not complete: the export is dropped.
	pub fn stopped(&mut self) {
		self.pending = None;
		self.progress = None;
	}

	/// What waited for the export's end, once it has ended.
	pub fn take_after(&mut self) -> Option<After> {
		if self.progress.is_none() { self.after.take() } else { None }
	}

	/// The export's end, the only one but a waiting export's drop: the run is released and the result told. Only a
	/// completed export becomes the export hint and, a run's, File > Export last.
	fn finished(&mut self, app: &mut App, outcome: Outcome) -> Vec<Effect> {
		let ended = self.progress.take();
		app.live.end_export();
		match outcome {
			Outcome::Completed(line) => {
				tracing::info!("{line}");
				#[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
				let mut effects = Vec::new();
				if let Some(p) = ended {
					app.export_done(p.kind, &p.path);
					if p.last.is_some() {
						self.last = p.last;
					}
					#[cfg(not(target_arch = "wasm32"))]
					effects.push(Effect::Exported(p.path));
				}
				effects
			}
			Outcome::Cancelled(line) => {
				tracing::info!("{line}");
				Vec::new()
			}
			Outcome::Failed(e) => {
				tracing::error!("export failed: {e}");
				let message = if e.storage_full {
					format!("{}\n{}\n{e}", app.strings.get("ExportError"), app.strings.get("StorageFull"))
				} else {
					format!("{}\n{e}", app.strings.get("ExportError"))
				};
				vec![Effect::Error(message)]
			}
		}
	}
}

/// What the export dialog exports.
enum Subject {
	Frames(ExportKindChoice),
	/// The probe of this name's data as CSV.
	#[cfg_attr(
		not(target_arch = "wasm32"),
		expect(dead_code, reason = "the desktop's probe CSV takes the platform's save dialog")
	)]
	Csv(String),
}

/// A name box of the export dialog.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Target {
	/// The output of this index.
	Output(usize),
	Archive,
}

/// The H.264 profile row: the profile set, those offered and the string key of why the others are greyed.
pub struct ProfileRow {
	pub current: Profile,
	pub supported: Vec<Profile>,
	pub reason: &'static str,
}

impl ProfileRow {
	/// The profile set and the encoder's offer.
	pub fn of(app: &App) -> Self {
		let (supported, reason) = super::h264_profiles(app);
		Self { current: app.settings.h264_profile, supported, reason }
	}

	/// The reason shows below the row while the current profile is not offered.
	pub fn note(&self) -> Option<&'static str> {
		(!self.supported.contains(&self.current)).then_some(self.reason)
	}
}

/// What the dialog shows below the name boxes, in this order, and whether the export can start.
pub struct Notes {
	/// MP4's H.264 profile.
	pub profile: Option<ProfileRow>,
	/// Cisco's notice and the consent box, checked or not: MP4 without a built-in encoder.
	pub openh264: Option<bool>,
	/// The downloaded codec can be deleted.
	pub delete_codec: bool,
	/// Why the export cannot start, a string key shown dimmed.
	pub blocked: Option<&'static str>,
	pub ready: bool,
}

/// The export dialog's form: format and outputs as typed, with the plan they make.
pub struct ExportForm {
	subject: Subject,
	/// The format's index in [`Self::formats`].
	pub format: usize,
	/// Each output as typed, in layer order: a path natively, a file name in the browser.
	pub outputs: Vec<String>,
	/// The archive as typed.
	pub archive: String,
	/// The formats' names and extensions, as the filter list gives them.
	names: Vec<(String, Vec<String>)>,
	#[cfg(not(target_arch = "wasm32"))]
	codec_downloaded: bool,
	/// The browse dialog's filter of the archive.
	#[cfg(not(target_arch = "wasm32"))]
	archive_filter: &'static str,
}

impl ExportForm {
	/// The form of an export command; `None` while an export is busy.
	pub fn open(app: &App, choice: ExportKindChoice) -> Option<Self> {
		(!app.live.export_busy()).then(|| Self::new(app, choice))
	}

	fn new(app: &App, choice: ExportKindChoice) -> Self {
		let request = super::request(app, choice);
		Self {
			subject: Subject::Frames(choice),
			format: request.format,
			outputs: request.outputs.iter().map(|(_, path)| imp::shown(path)).collect(),
			archive: imp::shown(&request.archive),
			names: filters(app.strings.get(choice.filter_key())),
			#[cfg(not(target_arch = "wasm32"))]
			codec_downloaded: crate::codec::downloaded(app),
			#[cfg(not(target_arch = "wasm32"))]
			archive_filter: app.strings.get("ExportDialogFilterZip"),
		}
	}

	/// The form naming probe `probe`'s CSV; `None` while an export is busy.
	#[cfg(target_arch = "wasm32")]
	pub fn csv(app: &App, probe: waveduderust::model::ProbeId) -> Option<Self> {
		if app.live.export_busy() {
			return None;
		}
		let (start, probe) = super::probe_csv_request(app, probe);
		let outputs = vec![imp::shown(&start)];
		Some(Self { subject: Subject::Csv(probe), format: 0, outputs, archive: String::new(), names: Vec::new() })
	}

	/// The formats offered; none for the probe CSV.
	pub fn formats(&self) -> &'static [ExportFormat] {
		match &self.subject {
			Subject::Frames(choice) => choice.formats(),
			Subject::Csv(_) => &[],
		}
	}

	/// The name of the format of index `i`.
	pub fn format_name(&self, i: usize) -> &str {
		&self.names[i].0
	}

	/// Why the format of index `i` cannot be picked, a string key.
	pub fn unavailable(&self, app: &App, i: usize) -> Option<&'static str> {
		imp::unavailable(app, self.formats()[i])
	}

	/// The format chosen; `None` for the probe CSV.
	pub fn format(&self) -> Option<ExportFormat> {
		self.formats().get(self.format).copied()
	}

	/// Picks the format of index `i`; each output takes its extension.
	pub fn set_format(&mut self, i: usize) {
		self.format = i;
		let ext = self.formats()[i].default_extension();
		for typed in &mut self.outputs {
			*typed = PathBuf::from(&*typed).with_extension(ext).display().to_string();
		}
	}

	/// The extension the output gets when typed without one.
	fn extension(&self) -> &'static str {
		self.format().map_or("csv", ExportFormat::default_extension)
	}

	pub fn title_key(&self) -> &'static str {
		match &self.subject {
			Subject::Frames(choice) => choice.title_key(),
			Subject::Csv(_) => "ExportDialogTitleCsv",
		}
	}

	/// The layers written, one per output; none for the probe CSV.
	pub fn layers(&self) -> &'static [Layer] {
		match &self.subject {
			Subject::Frames(choice) => choice.layers().layers(),
			Subject::Csv(_) => &[],
		}
	}

	/// The export writes several files, so the archive is offered.
	pub fn writes_many(&self) -> bool {
		matches!(&self.subject, Subject::Frames(choice) if choice.writes_many(choice.formats()[self.format]))
	}

	/// The outputs go into one archive.
	pub fn archived(&self, app: &App) -> bool {
		self.writes_many() && imp::archived(app)
	}

	/// The user picks whether an export of several files is archived; the browser always archives it.
	pub fn archive_chosen() -> bool {
		imp::ARCHIVE_CHOSEN
	}

	pub fn text_mut(&mut self, target: Target) -> &mut String {
		match target {
			Target::Output(i) => &mut self.outputs[i],
			Target::Archive => &mut self.archive,
		}
	}

	/// The browse dialog's filter list and start of the box of `target`: the chosen format's, or the archive's.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn browse(&self, target: Target) -> (String, PathBuf) {
		match target {
			Target::Output(i) => {
				let (name, exts) = &self.names[self.format];
				(format!("{name}|*.{}", exts.join(";*.")), PathBuf::from(&self.outputs[i]))
			}
			Target::Archive => (self.archive_filter.to_owned(), PathBuf::from(&self.archive)),
		}
	}

	/// Deletes the downloaded `OpenH264`.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn delete_codec(&mut self, app: &App) {
		crate::codec::delete(app);
		self.codec_downloaded = crate::codec::downloaded(app);
	}

	/// The plan of what is typed; `None` while a name is missing or two outputs share one.
	pub fn plan(&self, app: &App) -> Option<Plan> {
		let ext = self.extension();
		let choice = match &self.subject {
			Subject::Frames(choice) => *choice,
			Subject::Csv(probe) => {
				return Some(Plan::Csv { probe: probe.clone(), path: imp::typed(&self.outputs[0], ext)? });
			}
		};
		let format = choice.formats()[self.format];
		let archive = if self.archived(app) { Some(imp::typed(&self.archive, "zip")?) } else { None };
		let outputs = choice
			.layers()
			.layers()
			.iter()
			.zip(&self.outputs)
			.map(|(&layer, typed)| {
				let path = imp::typed(typed, ext)?;
				Some((layer, if archive.is_some() { PathBuf::from(path.file_name()?) } else { path }))
			})
			.collect::<Option<Vec<_>>>()?;
		let sequence = !choice.is_still() && !format.is_video();
		let written: Vec<PathBuf> =
			outputs.iter().map(|(_, p)| if sequence { naming::sequence_folder(p) } else { p.clone() }).collect();
		if written.iter().enumerate().any(|(i, p)| written[..i].contains(p)) {
			return None;
		}
		let frame = app.live.playback.position() as usize;
		Some(Plan::Frames { choice, format, outputs, archive, frame, repeat: false })
	}

	/// The dialog's OK: the plan of what is typed, while the export can start.
	pub fn chosen(&self, app: &App) -> Option<Step> {
		self.plan(app).filter(|_| self.notes(app).ready).map(Step::Chosen)
	}

	/// MP4's profile, and the codec's notice and choice.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn notes(&self, app: &App) -> Notes {
		let mp4 = self.format() == Some(ExportFormat::Mp4);
		let profile = mp4.then(|| ProfileRow::of(app));
		let consent = mp4 && !waveduderust::unstable::export::h264::BUILT_IN;
		let enabled = crate::codec::enabled(app.settings.use_openh264);
		Notes {
			profile,
			openh264: consent.then_some(enabled),
			delete_codec: consent && self.codec_downloaded,
			blocked: None,
			ready: !consent || enabled,
		}
	}

	/// The H.264 profile row of MP4, or why the export cannot start in the browser: a run's export without the private
	/// file system to spool it in, MP4 without an encoder. The browser is asked which profiles it encodes as the
	/// dialog opens, whatever the format.
	#[cfg(target_arch = "wasm32")]
	pub fn notes(&self, app: &App) -> Notes {
		let mut profile = None;
		let blocked = match &self.subject {
			Subject::Frames(choice) if !choice.is_still() && Spool::available().is_err() => Some("ExportNeedsStorage"),
			Subject::Frames(choice) if !choice.is_still() => {
				match super::h264_support(&app.document.project, &app.document.view) {
					_ if self.format() != Some(ExportFormat::Mp4) => None,
					H264Support::Profiles(supported) if !supported.is_empty() => {
						let current = app.settings.h264_profile;
						profile = Some(ProfileRow { current, supported, reason: "ExportH264NotInBrowser" });
						None
					}
					H264Support::Probing => Some("ExportH264Probing"),
					support => imp::mp4_unavailable(&support),
				}
			}
			_ => None,
		};
		Notes { profile, openh264: None, delete_codec: false, blocked, ready: blocked.is_none() }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::AppState;
	use crate::settings::AppDirs;

	#[test]
	fn export_last_repeats_no_export_of_another_project() {
		let dir = tempfile::tempdir().unwrap();
		let mut app = AppState::start(&AppDirs::under(dir.path()), []).expect("the state starts");
		let mut exports = Exports::default();
		let outputs = vec![(Layer::Field, dir.path().join("field.png"))];
		let (choice, format) = (ExportKindChoice::Field, ExportFormat::Png);
		let plan = Plan::Frames { choice, format, outputs, archive: None, frame: 0, repeat: false };
		exports.last = Some((plan, Some(dir.path().join("other.wdproj"))));
		assert!(exports.export_last(&mut app).is_empty());
		assert!(!exports.exporting() && !app.live.export_busy(), "the other project's export was repeated");
	}

	#[test]
	fn the_dialog_names_each_output_and_archives_them_when_asked() {
		use imp::shown;
		let dir = tempfile::tempdir().unwrap();
		let mut app = AppState::start(&AppDirs::under(dir.path()), []).expect("the state starts");
		let avi = ExportFormat::ALL.iter().position(|&f| f == ExportFormat::Avi).unwrap();
		let at = |name: &str| dir.path().join(name);
		let mut form = ExportForm::new(&app, ExportKindChoice::Both);
		assert_eq!(form.outputs.len(), 2, "a box per layer");
		form.format = avi;
		form.outputs = vec![shown(&at("a")), shown(&at("b.avi"))];
		let Some(Plan::Frames { outputs, archive: None, .. }) = form.plan(&app) else { panic!("a plan of files") };
		assert_eq!(outputs, [(Layer::Field, at("a.avi")), (Layer::Intensity, at("b.avi"))]);
		form.outputs[1] = shown(&at("a.avi"));
		assert!(form.plan(&app).is_none(), "two outputs of one name");
		form.outputs[1] = shown(&at("b.avi"));
		app.settings.export_archive = true;
		form.archive = shown(&at("both"));
		let Some(Plan::Frames { outputs, archive, .. }) = form.plan(&app) else { panic!("an archived plan") };
		assert_eq!(outputs, [(Layer::Field, "a.avi".into()), (Layer::Intensity, "b.avi".into())], "names in it");
		assert_eq!(archive, Some(at("both.zip")));
		let mut field = ExportForm::new(&app, ExportKindChoice::Field);
		field.format = avi;
		assert!(matches!(field.plan(&app), Some(Plan::Frames { archive: None, .. })), "one video is no archive");
	}

	#[test]
	fn a_format_change_renames_each_output() {
		let dir = tempfile::tempdir().unwrap();
		let app = AppState::start(&AppDirs::under(dir.path()), []).expect("the state starts");
		let mut form = ExportForm::new(&app, ExportKindChoice::Both);
		let avi = ExportFormat::ALL.iter().position(|&f| f == ExportFormat::Avi).unwrap();
		form.set_format(avi);
		assert!(form.outputs.iter().all(|o| o.ends_with(".avi")), "{:?}", form.outputs);
	}

	#[test]
	fn an_export_without_frames_ends_with_its_error() {
		let dir = tempfile::tempdir().unwrap();
		let mut app = AppState::start(&AppDirs::under(dir.path()), []).expect("the state starts");
		let mut exports = Exports::default();
		let plan = Plan::Csv { probe: "Probe 1".into(), path: dir.path().join("probe.csv") };
		assert!(app.live.run_store().is_none());
		let effects = exports.run(&mut app, plan);
		assert!(!exports.exporting() && !app.live.export_busy(), "the export did not end");
		assert!(matches!(effects.as_slice(), [Effect::Error(_)]), "the error was not shown");
	}
}
