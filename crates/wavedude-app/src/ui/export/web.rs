//! The browser's exports: the output is a file name, downloaded, so nothing is in the way to ask about; the job is
//! stepped on the page from [`Exports::poll`], at once again while it works, once woken while it waits; the probe
//! CSV's name comes from the export dialog.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Id, RichText, Ui};
use wavedude_app_state::Event;
use wavedude_app_state::platform::{H264Support, Spool};
use wavedude_export::ExportFormat;

use super::super::modal::{Modal, Modals};
use super::super::text_box::{Kind, text_box};
use super::super::theme::Palette;
use super::{ExportDialog, Exports, Subject};
use crate::App;
use crate::export::delivery::typed_name;
use crate::export::{self, Outcome, Plan, Runner, Started, Status, Work};

/// The running export, and the spool of the last one, which holds its download until the next export starts.
#[derive(Default)]
pub(super) struct Running {
	runner: Option<Runner>,
	spool: Option<Arc<Spool>>,
}

/// The dialog's text of the proposed output: its file name.
pub(super) fn shown(path: &Path) -> String {
	path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl Exports {
	/// "Export probe data as CSV" of probe `probe`: the export dialog asks for the file's name.
	pub(in super::super) fn probe_csv(&self, _ctx: &egui::Context, app: &App, modals: &mut Modals, probe: usize) {
		if !app.live.export_busy() {
			modals.show(Modal::Export(ExportDialog::csv(app, probe)));
		}
	}

	/// A download replaces nothing: the export goes on at once.
	pub(super) fn ask(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals, plan: Plan, _: usize) {
		self.gate(ctx, app, modals, plan);
	}

	/// Starts `plan`, the last export's spool removed first; an export without frames ends as failed.
	pub(super) fn run(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals, plan: Plan) {
		let Some(work) = Work::of(app) else {
			self.finished(app, modals, Outcome::Failed("the run has no frames".into()));
			return;
		};
		self.running.spool = None;
		let cancel = self.started(app, &plan);
		match export::start(&work, plan, cancel) {
			Ok(Started::Done(outcome)) => self.finished(app, modals, outcome),
			Ok(Started::Running(runner)) => {
				self.running.runner = Some(runner);
				ctx.request_repaint();
			}
			Err(e) => self.finished(app, modals, Outcome::Failed(e)),
		}
	}

	/// Steps the running export.
	pub(in super::super) fn poll(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals) {
		let Some(status) = self.running.runner.as_mut().map(Runner::poll) else { return };
		match status {
			Status::Working(done, total) => {
				self.progressed(app, done, total);
				ctx.request_repaint();
			}
			Status::Waiting(done, total) => self.progressed(app, done, total),
			Status::Packaging => {
				if let Some(p) = &mut self.progress {
					app.strings.get("ExportStatusPackaging").clone_into(&mut p.text);
				}
			}
			Status::Ended(outcome) => {
				self.running.spool = self.running.runner.take().and_then(|mut r| r.take_spool());
				self.finished(app, modals, outcome);
			}
		}
	}
}

impl ExportDialog {
	/// The dialog naming probe `probe`'s CSV.
	fn csv(app: &App, probe: usize) -> Self {
		let (start, probe) = export::probe_csv_request(app, probe);
		Self { subject: Subject::Csv(probe), format: 0, path: shown(&start), names: Vec::new() }
	}

	/// The file name box; whether it has the focus.
	pub(super) fn output_row(&mut self, ui: &mut Ui, title: &str) -> bool {
		let kind = Kind { live: true, ..Kind::default() };
		let edited = text_box(ui, Id::unique("ExportPath"), &self.path, ui.available_width(), kind, title);
		if let Some(path) = edited.applied {
			self.path = path;
		}
		edited.response.has_focus()
	}

	/// The file name typed, with the format's extension when it has none.
	pub(super) fn output(&self) -> Option<PathBuf> {
		typed_name(&self.path, self.extension())
	}

	/// The H.264 profile row of MP4, or why the export cannot start in the browser: a run's export without the private
	/// file system to spool it in, MP4 without an encoder; whether it can. The browser is asked which profiles it
	/// encodes as the dialog opens, whatever the format.
	pub(super) fn notes(&mut self, ui: &mut Ui, app: &App, out: &mut Vec<Event>) -> bool {
		let s = &app.strings;
		let reason = match &self.subject {
			Subject::Frames(choice) if !choice.is_still() && Spool::available().is_err() => Some("ExportNeedsStorage"),
			Subject::Frames(choice) if !choice.is_still() => match export::h264_support(&app.document.project) {
				_ if self.format() != Some(ExportFormat::Mp4) => None,
				H264Support::Profiles(profiles) if !profiles.is_empty() => {
					let (current, reason) = (app.settings.h264_profile, s.get("ExportH264NotInBrowser"));
					if let Some(profile) = super::profile_row(ui, s, current, &profiles, reason) {
						out.push(Event::SetH264Profile(profile));
					}
					None
				}
				H264Support::Probing => Some("ExportH264Probing"),
				support => mp4_unavailable(&support),
			},
			_ => None,
		};
		if let Some(key) = reason {
			let dim = Palette::of(ui.ctx().theme()).text_dim;
			ui.add(egui::Label::new(RichText::new(s.get(key)).color(dim)).wrap());
		}
		reason.is_none()
	}
}

/// Why MP4 cannot be exported with `support`.
fn mp4_unavailable(support: &H264Support) -> Option<&'static str> {
	match support {
		H264Support::Unavailable => Some("ExportNoWebCodecs"),
		H264Support::Profiles(profiles) if profiles.is_empty() => Some("ExportH264NoProfile"),
		H264Support::Probing | H264Support::Profiles(_) => None,
	}
}

/// Why `format` cannot be picked, a string key: MP4 when the browser has no H.264 encoder for the project's videos.
pub(super) fn unavailable(app: &App, format: ExportFormat) -> Option<&'static str> {
	(format == ExportFormat::Mp4).then(|| mp4_unavailable(&export::h264_support(&app.document.project))).flatten()
}
