//! Exports (docs/ui/70, Export Save dialog). Natively (`native`) the output is a path and the job runs on a worker
//! thread; in the browser (`web`) the output is a file name, offered for download once the job, stepped on the page,
//! has ended.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
use native as imp;
#[cfg(target_arch = "wasm32")]
use web as imp;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Align, Id, Key, Layout, RichText, Ui};
use wavedude_app_state::live::ExportGate;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::strings::Strings;
use wavedude_app_state::{Event, ExportKind};
use wavedude_core::model::{Layer, Word};
use wavedude_export::h264::Profile;
use wavedude_export::{ExportFormat, naming};

use super::modal::{Choice, Modal, Modals, Reply, Then, buttons};
use super::properties::{Choices, check_box, combo};
use super::text_box::{Kind, text_box};
use super::theme::{FONT_SIZE, INPUT_HEIGHT, Palette};
use crate::App;
use crate::dialogs::filters;
use crate::export::{self, Outcome, Plan};
use crate::menus::ExportKindChoice;

const WIDTH: f32 = 528.;
/// Width of a name box's label.
const LABEL: f32 = 80.;

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
	#[cfg(not(target_arch = "wasm32"))]
	Confirmed(Plan, usize),
	/// Stop the export, then do `After`.
	Stop(After),
}

/// A run export and the project file it was made for.
type Last = (Plan, Option<PathBuf>);

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
#[derive(Default)]
pub(super) struct Exports {
	progress: Option<Progress>,
	pending: Option<Plan>,
	last: Option<Last>,
	after: Option<After>,
	/// What the platform's export keeps.
	running: imp::Running,
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

	/// File > Export last: the last run export of the open project again, no questions.
	pub(super) fn export_last(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals) {
		if app.live.export_busy() || !self.has_last(app) {
			return;
		}
		if let Some((Plan::Frames { choice, format, outputs, archive, frame, .. }, _)) = self.last.clone() {
			self.ask(ctx, app, modals, Plan::Frames { choice, format, outputs, archive, frame, repeat: true }, 0);
		}
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
			#[cfg(not(target_arch = "wasm32"))]
			Step::Confirmed(plan, confirmed) => self.ask(ctx, app, modals, plan, confirmed),
			Step::Stop(after) => {
				self.after = Some(after);
				self.cancel(app);
			}
		}
	}

	fn gate(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals, plan: Plan) {
		match app.live.request_export(&app.document.project, export::run_needs(&plan), Instant::now()) {
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

	/// The export of `plan` starts, its progress shown; the flag that cancels it.
	fn started(&mut self, app: &App, plan: &Plan) -> Arc<AtomicBool> {
		let last = match plan {
			Plan::Frames { choice, .. } if !choice.is_still() => Some((plan.clone(), app.document.path.clone())),
			_ => None,
		};
		let (kind, path) = export::plan_kind(plan);
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

	/// The running export did `(done, total)` frames.
	fn progressed(&mut self, app: &App, done: usize, total: usize) {
		if let Some(p) = &mut self.progress {
			(p.done, p.total) = (done, total);
			p.text = app.strings.format("ExportStatusFormat", &[&done, &total]);
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

	/// What waited for the export's end, once it has ended.
	pub(super) fn take_after(&mut self) -> Option<After> {
		if self.progress.is_none() { self.after.take() } else { None }
	}

	/// The export's end, the only one but a waiting export's drop: the run is released and the result told. Only a
	/// completed export becomes the export hint and, a run's, File > Export last.
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
enum Target {
	/// The output of this index.
	Output(usize),
	Archive,
}

/// The export dialog: format and outputs, then the export.
pub(super) struct ExportDialog {
	subject: Subject,
	format: usize,
	/// Each output as typed, in layer order: a path natively, a file name in the browser.
	outputs: Vec<String>,
	/// The archive as typed.
	archive: String,
	/// The formats' names and extensions, as the filter list gives them.
	names: Vec<(String, Vec<String>)>,
	#[cfg(not(target_arch = "wasm32"))]
	codec_downloaded: bool,
	/// The browse dialog's answer for a box.
	#[cfg(not(target_arch = "wasm32"))]
	browsed: super::inbox::Inbox<(Target, PathBuf)>,
	/// The browse dialog's filter of the archive.
	#[cfg(not(target_arch = "wasm32"))]
	archive_filter: &'static str,
}

impl ExportDialog {
	fn new(app: &App, choice: ExportKindChoice) -> Self {
		let request = export::request(app, choice);
		Self {
			subject: Subject::Frames(choice),
			format: request.format,
			outputs: request.outputs.iter().map(|(_, path)| imp::shown(path)).collect(),
			archive: imp::shown(&request.archive),
			names: filters(app.strings.get(choice.filter_key())),
			#[cfg(not(target_arch = "wasm32"))]
			codec_downloaded: crate::codec::downloaded(app),
			#[cfg(not(target_arch = "wasm32"))]
			browsed: super::inbox::Inbox::default(),
			#[cfg(not(target_arch = "wasm32"))]
			archive_filter: app.strings.get("ExportDialogFilterZip"),
		}
	}

	/// The format chosen; `None` for the probe CSV.
	fn format(&self) -> Option<ExportFormat> {
		match &self.subject {
			Subject::Frames(choice) => Some(choice.formats()[self.format]),
			Subject::Csv(_) => None,
		}
	}

	/// The extension the output gets when typed without one.
	fn extension(&self) -> &'static str {
		self.format().map_or("csv", ExportFormat::default_extension)
	}

	fn title_key(&self) -> &'static str {
		match &self.subject {
			Subject::Frames(choice) => choice.title_key(),
			Subject::Csv(_) => "ExportDialogTitleCsv",
		}
	}

	/// The layers written, one per output; none for the probe CSV.
	fn layers(&self) -> &'static [Layer] {
		match &self.subject {
			Subject::Frames(choice) => choice.layers().layers(),
			Subject::Csv(_) => &[],
		}
	}

	/// The export writes several files, so the archive is offered.
	fn writes_many(&self) -> bool {
		matches!(&self.subject, Subject::Frames(choice) if choice.writes_many(choice.formats()[self.format]))
	}

	/// The outputs go into one archive.
	fn archived(&self, app: &App) -> bool {
		self.writes_many() && imp::archived(app)
	}

	fn text_mut(&mut self, target: Target) -> &mut String {
		match target {
			Target::Output(i) => &mut self.outputs[i],
			Target::Archive => &mut self.archive,
		}
	}

	/// The plan of what is typed; `None` while a name is missing or two outputs share one.
	fn plan(&self, app: &App) -> Option<Plan> {
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

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App, out: &mut Vec<Event>) -> Option<Reply> {
		let s = &app.strings;
		let title = s.get(self.title_key());
		// Enter in a name box, which takes the key
		let enter = ui.input(|i| i.key_pressed(Key::Enter));
		ui.set_width(WIDTH);
		ui.label(RichText::new(title).family(wavedude_view::fonts::bold()).size(FONT_SIZE * 1.2));
		if let Subject::Frames(choice) = &self.subject {
			let formats = choice.formats();
			let word = |i: usize| Cow::Borrowed(self.names[i].0.as_str());
			let disabled = |i: usize| imp::unavailable(app, formats[i]).map(|key| s.get(key));
			let choices = Choices {
				len: formats.len(),
				current: Some(self.format),
				shown: &self.names[self.format].0,
				word: &word,
				swatch: None,
				disabled: Some(&disabled),
				actions: &[],
			};
			if let Some(i) = combo(ui, Id::unique("ExportFormat"), title, &choices) {
				self.format = i;
				let ext = formats[i].default_extension();
				for typed in &mut self.outputs {
					*typed = PathBuf::from(&*typed).with_extension(ext).display().to_string();
				}
			}
		}
		let typing = self.name_boxes(ui, app, title, out);
		let plan = self.plan(app);
		let ready = self.notes(ui, app, out) && plan.is_some();
		let choices =
			[Choice { enabled: ready, ..Choice::default(s.get("DialogOk")) }, Choice::cancel(s.get("DialogCancel"))];
		let pressed = buttons(ui, fresh, &choices).or((enter && typing && ready).then_some(0));
		pressed.map(|i| plan.filter(|_| i == 0).map(|plan| Then::Export(Step::Chosen(plan))))
	}

	/// The name boxes: each output's, labelled by its layer while there are several boxes, then, for an export of
	/// several files, the archive's check box and, while checked, the archive's box; whether a box has the focus.
	fn name_boxes(&mut self, ui: &mut Ui, app: &App, title: &str, out: &mut Vec<Event>) -> bool {
		let s = &app.strings;
		self.take_browsed();
		let archived = self.archived(app);
		let labelled = self.outputs.len() > 1 || archived;
		let mut typing = false;
		for i in 0..self.outputs.len() {
			let label =
				self.layers().get(i).filter(|_| labelled).map(|layer| s.enum_word(Layer::TYPE_NAME, layer.name()));
			typing |= self.name_row(ui, Target::Output(i), label, label.unwrap_or(title));
		}
		if self.writes_many() {
			let text = s.get("ExportArchive");
			let picked = ui
				.add_enabled_ui(imp::ARCHIVE_CHOSEN, |ui| check_box(ui, Id::unique("ExportArchive"), text, archived))
				.inner;
			if let Some(on) = picked {
				out.push(Event::SetExportArchive(on));
			}
			if archived {
				let label = s.get("ExportArchiveName");
				typing |= self.name_row(ui, Target::Archive, Some(label), label);
			}
		}
		typing
	}

	/// The row of the name box of `target`: `label` left of it when shown, the platform's browse button right of it;
	/// `name` names the box for assistive technology. Whether the box has the focus.
	fn name_row(&mut self, ui: &mut Ui, target: Target, label: Option<&str>, name: &str) -> bool {
		let row = egui::vec2(ui.available_width(), INPUT_HEIGHT);
		ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
			ui.spacing_mut().item_spacing.x = 4.;
			self.browse_button(ui, target);
			let width = ui.available_width() - label.map_or(0., |_| LABEL + ui.spacing().item_spacing.x);
			let kind = Kind { live: true, ..Kind::default() };
			let edited = text_box(ui, Id::unique("ExportPath").with(target), self.text_mut(target), width, kind, name);
			if let Some(typed) = edited.applied {
				*self.text_mut(target) = typed;
			}
			if let Some(label) = label {
				ui.with_layout(Layout::left_to_right(Align::Center), |ui| ui.label(label));
			}
			edited.response.has_focus()
		})
		.inner
	}
}

/// The H.264 profile row: every profile, those not in `supported` greyed with `reason`, which also shows below while
/// the current one is among them; the profile picked.
fn profile_row(ui: &mut Ui, s: &Strings, current: Profile, supported: &[Profile], reason: &str) -> Option<Profile> {
	let label = s.get("ExportH264Profile");
	let name = |p: Profile| {
		s.get(match p {
			Profile::Baseline => "ExportH264ProfileBaseline",
			Profile::Main => "ExportH264ProfileMain",
			Profile::High => "ExportH264ProfileHigh",
		})
	};
	let word = |i: usize| Cow::Borrowed(name(Profile::ALL[i]));
	let disabled = |i: usize| (!supported.contains(&Profile::ALL[i])).then_some(reason);
	let picked = ui
		.horizontal(|ui| {
			ui.label(label);
			let choices = Choices {
				len: Profile::ALL.len(),
				current: Profile::ALL.iter().position(|&p| p == current),
				shown: name(current),
				word: &word,
				swatch: None,
				disabled: Some(&disabled),
				actions: &[],
			};
			combo(ui, Id::unique("ExportH264Profile"), label, &choices).map(|i| Profile::ALL[i])
		})
		.inner;
	if !supported.contains(&current) {
		let dim = Palette::of(ui.ctx().theme()).text_dim;
		ui.add(egui::Label::new(RichText::new(reason).color(dim)).wrap());
	}
	picked
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_app_state::AppState;
	use wavedude_app_state::settings::AppDirs;

	#[test]
	fn export_last_repeats_no_export_of_another_project() {
		let dir = tempfile::tempdir().unwrap();
		let mut app = AppState::start(&AppDirs::under(dir.path()), []).expect("the state starts");
		let (ctx, mut modals, mut exports) = (egui::Context::default(), Modals::default(), Exports::default());
		let outputs = vec![(Layer::Field, dir.path().join("field.png"))];
		let (choice, format) = (ExportKindChoice::Field, ExportFormat::Png);
		let plan = Plan::Frames { choice, format, outputs, archive: None, frame: 0, repeat: false };
		exports.last = Some((plan, Some(dir.path().join("other.wdproj"))));
		exports.export_last(&ctx, &mut app, &mut modals);
		assert!(!exports.exporting() && !app.live.export_busy(), "the other project's export was repeated");
	}

	#[test]
	fn the_dialog_names_each_output_and_archives_them_when_asked() {
		use imp::shown;
		let dir = tempfile::tempdir().unwrap();
		let mut app = AppState::start(&AppDirs::under(dir.path()), []).expect("the state starts");
		let avi = ExportFormat::ALL.iter().position(|&f| f == ExportFormat::Avi).unwrap();
		let at = |name: &str| dir.path().join(name);
		let mut dialog = ExportDialog::new(&app, ExportKindChoice::Both);
		assert_eq!(dialog.outputs.len(), 2, "a box per layer");
		dialog.format = avi;
		dialog.outputs = vec![shown(&at("a")), shown(&at("b.avi"))];
		let Some(Plan::Frames { outputs, archive: None, .. }) = dialog.plan(&app) else { panic!("a plan of files") };
		assert_eq!(outputs, [(Layer::Field, at("a.avi")), (Layer::Intensity, at("b.avi"))]);
		dialog.outputs[1] = shown(&at("a.avi"));
		assert!(dialog.plan(&app).is_none(), "two outputs of one name");
		dialog.outputs[1] = shown(&at("b.avi"));
		app.settings.export_archive = true;
		dialog.archive = shown(&at("both"));
		let Some(Plan::Frames { outputs, archive, .. }) = dialog.plan(&app) else { panic!("an archived plan") };
		assert_eq!(outputs, [(Layer::Field, "a.avi".into()), (Layer::Intensity, "b.avi".into())], "names in it");
		assert_eq!(archive, Some(at("both.zip")));
		let mut field = ExportDialog::new(&app, ExportKindChoice::Field);
		field.format = avi;
		assert!(matches!(field.plan(&app), Some(Plan::Frames { archive: None, .. })), "one video is no archive");
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
