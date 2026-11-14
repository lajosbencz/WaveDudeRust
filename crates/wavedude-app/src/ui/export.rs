//! Exports (docs/ui/70, Export Save dialog): the dialog picks the format (the platform dialog cannot report its
//! filter) and the path; a full-quality run is waited for when the current one does not serve; the job then runs
//! on a worker thread, its progress in the status bar, Esc or Stop cancelling it.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{self, Align, Id, Key, Layout, RichText, Ui};
use wavedude_app_state::live::ExportGate;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::{Event, ExportKind};
use wavedude_export::{ExportFormat, h264};

use super::files::on_thread;
use super::modal::{Choice, Modal, Modals, Reply, Then, buttons};
use super::properties::{self, Choices, check_box, combo};
use super::text_box::{Kind, text_box};
use super::theme::{FONT_SIZE, INPUT_HEIGHT, Palette};
use crate::App;
use crate::dialogs::{file_dialog, filters};
use crate::export::{self, Outcome, Plan, Work};
use crate::menus::ExportKindChoice;
use crate::{codec, downloads};

const WIDTH: f32 = 528.;
const BROWSE: f32 = 28.;

/// What the window does once the export has ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum After {
	Exit,
	Restart,
}

/// An export's answered modal.
#[derive(Clone)]
pub(super) enum Step {
	/// The dialog's OK.
	Chosen(Plan),
	/// Replacing the first `usize` of what is in the way was confirmed.
	Confirmed(Plan, usize),
	/// Stop the export, then do `After`.
	Stop(After),
}

/// A run export and the project file it was made for.
type Last = (Plan, Option<PathBuf>);

enum Msg {
	Progress(usize, usize),
	Done(Outcome),
	/// The CSV's save dialog answered for the document `document`.
	Csv {
		document: u64,
		probe: String,
		path: PathBuf,
	},
}

/// A running or waiting export, shown in the status bar.
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

/// The export, the one waiting for its run, the last one and what waits for the end.
pub(super) struct Exports {
	progress: Option<Progress>,
	pending: Option<Plan>,
	last: Option<Last>,
	after: Option<After>,
	tx: Sender<Msg>,
	rx: Receiver<Msg>,
}

impl Default for Exports {
	fn default() -> Self {
		let (tx, rx) = channel();
		Self { progress: None, pending: None, last: None, after: None, tx, rx }
	}
}

impl Exports {
	/// An export runs or waits for its run.
	pub(super) fn exporting(&self) -> bool {
		self.progress.is_some()
	}

	/// The status bar's text and share done, `None` while unknown.
	pub(super) fn status(&self) -> Option<(&str, Option<f32>)> {
		self.progress.as_ref().map(|p| {
			let share = (!p.waiting && p.total != 0).then(|| p.done as f32 / p.total as f32);
			(p.text.as_str(), share)
		})
	}

	/// File > Export last has something to repeat for the open project.
	pub(super) fn has_last(&self, app: &App) -> bool {
		self.last.as_ref().is_some_and(|(_, p)| *p == app.document.path)
	}

	/// The dialog of an export command; `None` while an export is busy.
	pub(super) fn dialog(app: &App, choice: ExportKindChoice) -> Option<Modal> {
		(!app.live.export_busy()).then(|| Modal::Export(ExportDialog::new(app, choice)))
	}

	/// File > Export last: the last run export again, no questions.
	pub(super) fn export_last(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals) {
		if app.live.export_busy() {
			return;
		}
		if let Some((Plan::Frames { choice, format, path, frame, .. }, _)) = self.last.clone() {
			self.ask(ctx, app, modals, Plan::Frames { choice, format, path, frame, repeat: true }, 0);
		}
	}

	/// "Export probe data as CSV" of probe `probe`: the save dialog, then the export.
	pub(super) fn probe_csv(&self, ctx: &egui::Context, app: &App, probe: usize) {
		if app.live.export_busy() {
			return;
		}
		let (start, probe) = export::probe_csv_request(app, probe);
		let dialog = file_dialog(app.strings.get("ExportDialogFilterCsv"), Some(&start)).save_file();
		let (tx, document) = (self.tx.clone(), app.document_id());
		on_thread(ctx, dialog, move |path| {
			if let Some(path) = path {
				// the window may have closed meanwhile
				let _ = tx.send(Msg::Csv { document, probe, path });
			}
		});
	}

	/// Does what an export's modal answered.
	pub(super) fn step(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals, step: Step) {
		match step {
			Step::Chosen(plan) => {
				if let Plan::Frames { choice, format, .. } = &plan
					&& !choice.is_still()
				{
					let index = ExportFormat::ALL.iter().position(|f| f == format).expect("a run format is listed");
					app.settings.export_filter_index = index as u32;
				}
				self.ask(ctx, app, modals, plan, 0);
			}
			Step::Confirmed(plan, confirmed) => self.ask(ctx, app, modals, plan, confirmed),
			Step::Stop(after) => {
				self.after = Some(after);
				self.cancel(app);
			}
		}
	}

	/// Asks before replacing each thing in the way, one at a time, from the `confirmed`th on.
	fn ask(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals, plan: Plan, confirmed: usize) {
		if let Some((key, target)) = export::would_replace(&plan).into_iter().nth(confirmed) {
			let text = app.strings.format(key, &[&target.display()]);
			let yes = Then::Export(Step::Confirmed(plan, confirmed + 1));
			modals.show(Modal::Confirm { text, yes, destroys: false });
		} else {
			self.gate(ctx, app, modals, plan);
		}
	}

	fn gate(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals, plan: Plan) {
		let project = app.document.project.clone();
		match app.live.request_export(&project, export::needs_intensity(&plan), Instant::now()) {
			ExportGate::Ready => self.run(ctx, app, modals, plan),
			ExportGate::Waiting => {
				let (kind, path) = export::plan_kind(&plan);
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
			}
			ExportGate::Busy => tracing::debug!("export refused: another export is busy"),
		}
	}

	/// Starts the worker on `plan`; an export without frames ends as failed.
	fn run(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals, plan: Plan) {
		let s = app.strings;
		let Some(work) = Work::of(app) else {
			self.finished(app, modals, Outcome::Failed("the run has no frames".into()));
			return;
		};
		let last = match &plan {
			Plan::Frames { choice, .. } if !choice.is_still() => Some((plan.clone(), app.document.path.clone())),
			_ => None,
		};
		let (kind, path) = export::plan_kind(&plan);
		if export::fetches_codec(&plan, &work, app) {
			work.codec_download.set(Some(downloads::start(s.get("StatusDownloadingCodec").to_owned())));
		}
		let cancel = Arc::new(AtomicBool::new(false));
		let flag = cancel.clone();
		let mut done = Done { tx: self.tx.clone(), ctx: ctx.clone(), outcome: None };
		let spawned = std::thread::Builder::new().name("export".into()).spawn(move || {
			let tx = done.tx.clone();
			let outcome = export::run(&work, plan, |d, total| drop(tx.send(Msg::Progress(d, total))), &flag);
			done.outcome.replace(outcome);
		});
		self.progress = Some(Progress {
			text: s.format("ExportStatusFormat", &[&0, &0]),
			done: 0,
			total: 0,
			waiting: false,
			cancel,
			kind,
			path,
			last,
		});
		// the dropped worker's guard tells the end
		if let Err(e) = spawned {
			tracing::error!("the export thread could not start: {e}");
		}
	}

	/// Cancels a running or waiting export; false when there is none.
	pub(super) fn cancel(&mut self, app: &mut App) -> bool {
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

	/// The run of the waiting export has completed ([`wavedude_app_state::live::Notice::ExportReady`]).
	pub(super) fn ready(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals) {
		match self.pending.take() {
			Some(plan) => self.run(ctx, app, modals, plan),
			None => app.live.end_export(),
		}
	}

	/// The waiting export's run will not complete: the export is dropped.
	pub(super) fn stopped(&mut self) {
		self.pending = None;
		self.progress = None;
	}

	/// Takes the worker's progress and end, and the CSV dialog's answer.
	pub(super) fn poll(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals) {
		while let Ok(msg) = self.rx.try_recv() {
			match msg {
				Msg::Progress(done, total) => {
					if let Some(p) = &mut self.progress {
						(p.done, p.total) = (done, total);
						p.text = app.strings.format("ExportStatusFormat", &[&done, &total]);
					}
				}
				Msg::Done(outcome) => self.finished(app, modals, outcome),
				Msg::Csv { document, probe, path } => {
					if document == app.document_id() && app.document.project.probes.iter().any(|p| p.name == probe) {
						self.ask(ctx, app, modals, Plan::Csv { probe, path }, 0);
					}
				}
			}
		}
	}

	/// What waited for the export's end, once it has ended.
	pub(super) fn take_after(&mut self) -> Option<After> {
		if self.progress.is_none() { self.after.take() } else { None }
	}

	/// The export's end, the only one but a waiting export's drop: the run is released and the result told. Only a completed export becomes the export hint
	/// and, a run's, File > Export last.
	fn finished(&mut self, app: &mut App, modals: &mut Modals, outcome: Outcome) {
		let ended = self.progress.take();
		app.live.end_export();
		match outcome {
			Outcome::Completed(line) => {
				tracing::info!("{line}");
				if let Some(p) = ended {
					app.export_done(p.kind, &p.path);
					if p.last.is_some() {
						self.last = p.last;
					}
				}
			}
			Outcome::Cancelled(line) => tracing::info!("{line}"),
			Outcome::Failed(e) => {
				tracing::error!("export failed: {e}");
				modals.show(Modal::Error(format!("{}\n{e}", app.strings.get("ExportError"))));
			}
		}
	}
}

/// Sends the worker's outcome when the worker ends, a failure when it unwinds without one.
struct Done {
	tx: Sender<Msg>,
	ctx: egui::Context,
	outcome: Option<Outcome>,
}

impl Drop for Done {
	fn drop(&mut self) {
		let outcome = self.outcome.take().unwrap_or_else(|| Outcome::Failed("the export thread ended".into()));
		// the window may have closed meanwhile
		let _ = self.tx.send(Msg::Done(outcome));
		self.ctx.request_repaint();
	}
}

/// The export dialog: format and path, then the export.
pub(super) struct ExportDialog {
	choice: ExportKindChoice,
	format: usize,
	path: String,
	/// The formats' names and extensions, as the filter list gives them.
	names: Vec<(String, Vec<String>)>,
	codec_downloaded: bool,
	/// The browse dialog's answer.
	browsed: Option<Receiver<PathBuf>>,
}

impl ExportDialog {
	fn new(app: &App, choice: ExportKindChoice) -> Self {
		let request = export::request(app, choice);
		Self {
			choice,
			format: request.format,
			path: request.path.display().to_string(),
			names: filters(app.strings.get(choice.filter_key())),
			codec_downloaded: codec::downloaded(app),
			browsed: None,
		}
	}

	/// The save dialog for the chosen format, at the path typed.
	fn browse(&mut self, ctx: &egui::Context) {
		let (name, exts) = &self.names[self.format];
		let spec = format!("{name}|*.{}", exts.join(";*."));
		let dialog = file_dialog(&spec, Some(&PathBuf::from(&self.path))).save_file();
		let (tx, rx) = channel();
		self.browsed = Some(rx);
		on_thread(ctx, dialog, move |path| {
			if let Some(path) = path {
				// the dialog may have closed meanwhile
				let _ = tx.send(path);
			}
		});
	}

	fn plan(&self, app: &App) -> Plan {
		let format = self.choice.formats()[self.format];
		let path = PathBuf::from(&self.path);
		let path = if path.extension().is_none() { path.with_extension(format.default_extension()) } else { path };
		let frame = app.live.playback.position() as usize;
		Plan::Frames { choice: self.choice, format, path, frame, repeat: false }
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App, out: &mut Vec<Event>) -> Option<Reply> {
		if let Some(path) = self.browsed.as_ref().and_then(|rx| rx.try_recv().ok()) {
			self.path = path.display().to_string();
			self.browsed = None;
		}
		let s = &app.strings;
		let title = s.get(self.choice.title_key());
		let formats = self.choice.formats();
		// Enter in the path box, which takes the key
		let enter = ui.input(|i| i.key_pressed(Key::Enter));
		ui.set_width(WIDTH);
		ui.label(RichText::new(title).family(wavedude_view::fonts::bold()).size(FONT_SIZE * 1.2));
		let word = |i: usize| Cow::Borrowed(self.names[i].0.as_str());
		let choices = Choices {
			len: formats.len(),
			current: Some(self.format),
			shown: &self.names[self.format].0,
			word: &word,
			swatch: None,
			actions: &[],
		};
		if let Some(i) = combo(ui, Id::unique("ExportFormat"), title, &choices) {
			self.format = i;
			self.path = PathBuf::from(&self.path).with_extension(formats[i].default_extension()).display().to_string();
		}
		let mut typing = false;
		let row = egui::vec2(ui.available_width(), INPUT_HEIGHT);
		ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
			ui.spacing_mut().item_spacing.x = 4.;
			if ui.add(egui::Button::new("...").min_size(egui::vec2(BROWSE, 0.))).clicked() {
				self.browse(ui.ctx());
			}
			let kind = Kind { live: true, ..Kind::default() };
			let edited = text_box(ui, Id::unique("ExportPath"), &self.path, ui.available_width(), kind, title);
			if let Some(path) = edited.applied {
				self.path = path;
			}
			typing = edited.response.has_focus();
		});
		let mp4 = formats[self.format] == ExportFormat::Mp4 && !h264::BUILT_IN;
		let enabled = codec::enabled(app.settings.use_openh264);
		if mp4 {
			let dim = Palette::of(ui.ctx().theme()).text_dim;
			ui.add(egui::Label::new(RichText::new(h264::NOTICE).color(dim)).wrap());
			if let Some(on) = check_box(ui, Id::unique("ExportUseOpenH264"), s.get("ExportUseOpenH264"), enabled) {
				out.push(Event::SetUseOpenH264(on));
			}
			if self.codec_downloaded && properties::button(ui, s.get("ExportDeleteOpenH264"), true).clicked() {
				codec::delete(app);
				self.codec_downloaded = codec::downloaded(app);
			}
		}
		let ready = !mp4 || enabled;
		let choices =
			[Choice { enabled: ready, ..Choice::default(s.get("DialogOk")) }, Choice::cancel(s.get("DialogCancel"))];
		let pressed = buttons(ui, fresh, &choices).or((enter && typing && ready).then_some(0));
		pressed.map(|i| (i == 0).then(|| Then::Export(Step::Chosen(self.plan(app)))))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_app_state::AppState;
	use wavedude_app_state::settings::AppDirs;

	#[test]
	fn a_worker_that_unwinds_ends_its_export_as_failed() {
		let (tx, rx) = channel();
		let done = Done { tx, ctx: egui::Context::default(), outcome: None };
		let worker = std::thread::spawn(move || {
			let _done = done;
			panic!("the worker failed");
		});
		assert!(worker.join().is_err());
		assert!(matches!(rx.try_recv(), Ok(Msg::Done(Outcome::Failed(_)))), "no failure was sent");
	}

	#[test]
	fn an_export_without_frames_ends_with_its_error() {
		let dir = tempfile::tempdir().unwrap();
		let mut app = AppState::start(&AppDirs::under(dir.path()), []).expect("the state starts");
		let (ctx, mut modals, mut exports) = (egui::Context::default(), Modals::default(), Exports::default());
		let plan = Plan::Csv { probe: "Probe 1".into(), path: dir.path().join("probe.csv") };
		assert!(app.live.run_store().is_none());
		exports.run(&ctx, &mut app, &mut modals, plan);
		assert!(!exports.exporting() && !app.live.export_busy(), "the export did not end");
		assert!(modals.is_open(), "the error was not shown");
	}
}
