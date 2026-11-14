//! Exports (docs/ui/70, Export Save dialog): the dialog picks the format (the platform dialog cannot report its
//! filter) and the output; a full-quality run is waited for when the current one does not serve; the job then runs,
//! its progress in the status bar, Esc or Stop cancelling it. Natively (`native`) the output is a path, the job runs
//! on a worker thread and the probe CSV's path comes from the platform's save dialog; in the browser (`web`) the
//! output is a file name, downloaded once the job, stepped on the page, has ended.

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

use eframe::egui::{self, Id, Key, RichText, Ui};
use wavedude_app_state::live::ExportGate;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::strings::Strings;
use wavedude_app_state::{Event, ExportKind};
use wavedude_export::ExportFormat;
use wavedude_export::h264::Profile;

use super::modal::{Choice, Modal, Modals, Reply, Then, buttons};
use super::properties::{Choices, combo};
use super::theme::{FONT_SIZE, Palette};
use crate::App;
use crate::dialogs::filters;
use crate::export::{self, Outcome, Plan};
use crate::menus::ExportKindChoice;

const WIDTH: f32 = 528.;

/// What the window does once the export has ended.
#[cfg(not(target_arch = "wasm32"))]
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
	#[cfg(not(target_arch = "wasm32"))]
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
	#[cfg(not(target_arch = "wasm32"))]
	after: Option<After>,
	/// What the platform's export keeps.
	running: imp::Running,
}

impl Exports {
	/// An export runs or waits for its run.
	#[cfg(not(target_arch = "wasm32"))]
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
			#[cfg(not(target_arch = "wasm32"))]
			Step::Stop(after) => {
				self.after = Some(after);
				self.cancel(app);
			}
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
	#[cfg(not(target_arch = "wasm32"))]
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

/// The export dialog: format and output, then the export.
pub(super) struct ExportDialog {
	subject: Subject,
	format: usize,
	/// The output as typed: a path natively, a file name in the browser.
	path: String,
	/// The formats' names and extensions, as the filter list gives them.
	names: Vec<(String, Vec<String>)>,
	#[cfg(not(target_arch = "wasm32"))]
	codec_downloaded: bool,
	/// The browse dialog's answer.
	#[cfg(not(target_arch = "wasm32"))]
	browsed: Option<std::sync::mpsc::Receiver<PathBuf>>,
}

impl ExportDialog {
	fn new(app: &App, choice: ExportKindChoice) -> Self {
		let request = export::request(app, choice);
		Self {
			subject: Subject::Frames(choice),
			format: request.format,
			path: imp::shown(&request.path),
			names: filters(app.strings.get(choice.filter_key())),
			#[cfg(not(target_arch = "wasm32"))]
			codec_downloaded: crate::codec::downloaded(app),
			#[cfg(not(target_arch = "wasm32"))]
			browsed: None,
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

	fn plan(&self, app: &App, path: PathBuf) -> Plan {
		match &self.subject {
			Subject::Frames(choice) => {
				let format = choice.formats()[self.format];
				let frame = app.live.playback.position() as usize;
				Plan::Frames { choice: *choice, format, path, frame, repeat: false }
			}
			Subject::Csv(probe) => Plan::Csv { probe: probe.clone(), path },
		}
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App, out: &mut Vec<Event>) -> Option<Reply> {
		let s = &app.strings;
		let title = s.get(self.title_key());
		// Enter in the path box, which takes the key
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
				self.path =
					PathBuf::from(&self.path).with_extension(formats[i].default_extension()).display().to_string();
			}
		}
		let typing = self.output_row(ui, title);
		let path = self.output();
		let ready = self.notes(ui, app, out) && path.is_some();
		let choices =
			[Choice { enabled: ready, ..Choice::default(s.get("DialogOk")) }, Choice::cancel(s.get("DialogCancel"))];
		let pressed = buttons(ui, fresh, &choices).or((enter && typing && ready).then_some(0));
		pressed.map(|i| path.filter(|_| i == 0).map(|path| Then::Export(Step::Chosen(self.plan(app, path)))))
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

	#[test]
	fn a_profile_the_encoder_lacks_is_greyed_with_the_reason() {
		use eframe::egui::accesskit::Role;
		use egui_kittest::kittest::{NodeT as _, Queryable as _};
		use wavedude_app_state::strings::Language;

		let s = Strings::new(Language::English);
		let reason = s.get("ExportH264BaselineOnly");
		let builder = egui_kittest::Harness::builder().with_context_setup(|ctx| {
			super::super::theme::install(ctx);
			egui_material_icons::initialize(ctx);
		});
		let mut harness = builder.build_ui_state(
			|ui, picked: &mut Vec<Profile>| {
				picked.extend(profile_row(ui, &s, Profile::High, &[Profile::Baseline], reason));
			},
			Vec::new(),
		);
		harness.get_by_label(reason);
		harness.get_by_role_and_label(Role::ComboBox, s.get("ExportH264Profile")).click();
		harness.run();
		assert!(harness.get_by_role_and_label(Role::MenuItemCheckBox, "Main").accesskit_node().is_disabled());
		assert!(harness.get_by_role_and_label(Role::MenuItemCheckBox, "High").accesskit_node().is_disabled());
		harness.get_by_role_and_label(Role::MenuItemCheckBox, "Main").click();
		harness.run();
		assert!(harness.state().is_empty(), "a greyed profile was picked");
		harness.get_by_role_and_label(Role::ComboBox, s.get("ExportH264Profile")).click();
		harness.run();
		harness.get_by_role_and_label(Role::MenuItemCheckBox, "Baseline").click();
		harness.run();
		assert_eq!(harness.state(), &[Profile::Baseline]);
	}
}
