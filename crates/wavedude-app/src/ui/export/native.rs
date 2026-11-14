//! The desktop's exports: the output is a path, typed or browsed; what an export would replace is asked about
//! first; the job runs on a worker thread; the probe CSV's path comes from the platform's save dialog.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{self, Align, Id, Layout, RichText, Ui};
use wavedude_app_state::Event;
use wavedude_export::{ExportFormat, h264};

use super::super::files::on_thread;
use super::super::modal::{Modal, Modals, Then};
use super::super::properties::{self, check_box};
use super::super::text_box::{Kind, text_box};
use super::super::theme::{INPUT_HEIGHT, Palette};
use super::{ExportDialog, Exports, Step};
use crate::App;
use crate::dialogs::file_dialog;
use crate::export::{self, Outcome, Plan, Work};
use crate::{codec, downloads};

const BROWSE: f32 = 28.;

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

/// The worker's and the CSV dialog's channel.
pub(super) struct Running {
	tx: Sender<Msg>,
	rx: Receiver<Msg>,
}

impl Default for Running {
	fn default() -> Self {
		let (tx, rx) = channel();
		Self { tx, rx }
	}
}

/// The dialog's text of the proposed output.
pub(super) fn shown(path: &Path) -> String {
	path.display().to_string()
}

impl Exports {
	/// "Export probe data as CSV" of probe `probe`: the save dialog, then the export.
	pub(in super::super) fn probe_csv(&self, ctx: &egui::Context, app: &App, _modals: &mut Modals, probe: usize) {
		if app.live.export_busy() {
			return;
		}
		let (start, probe) = export::probe_csv_request(app, probe);
		let dialog = file_dialog(app.strings.get("ExportDialogFilterCsv"), Some(&start)).save_file();
		let (tx, document) = (self.running.tx.clone(), app.document_id());
		on_thread(ctx, dialog, move |path| {
			if let Some(path) = path {
				// the window may have closed meanwhile
				let _ = tx.send(Msg::Csv { document, probe, path });
			}
		});
	}

	/// Asks before replacing each thing in the way, one at a time, from the `confirmed`th on.
	pub(super) fn ask(
		&mut self,
		ctx: &egui::Context,
		app: &mut App,
		modals: &mut Modals,
		plan: Plan,
		confirmed: usize,
	) {
		if let Some((key, target)) = export::would_replace(&plan).into_iter().nth(confirmed) {
			let text = app.strings.format(key, &[&target.display()]);
			let yes = Then::Export(Step::Confirmed(plan, confirmed + 1));
			modals.show(Modal::Confirm { text, yes, destroys: false });
		} else {
			self.gate(ctx, app, modals, plan);
		}
	}

	/// Starts the worker on `plan`; an export without frames ends as failed.
	pub(super) fn run(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals, plan: Plan) {
		let Some(work) = Work::of(app) else {
			self.finished(app, modals, Outcome::Failed("the run has no frames".into()));
			return;
		};
		if export::fetches_codec(&plan, &work, app) {
			work.codec_download.set(Some(downloads::start(app.strings.get("StatusDownloadingCodec").to_owned())));
		}
		let flag = self.started(app, &plan);
		let mut done = Done { tx: self.running.tx.clone(), ctx: ctx.clone(), outcome: None };
		let spawned = std::thread::Builder::new().name("export".into()).spawn(move || {
			let tx = done.tx.clone();
			let outcome = export::run(&work, plan, |d, total| drop(tx.send(Msg::Progress(d, total))), &flag);
			done.outcome.replace(outcome);
		});
		// the dropped worker's guard tells the end
		if let Err(e) = spawned {
			tracing::error!("the export thread could not start: {e}");
		}
	}

	/// Takes the worker's progress and end, and the CSV dialog's answer.
	pub(in super::super) fn poll(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals) {
		while let Ok(msg) = self.running.rx.try_recv() {
			match msg {
				Msg::Progress(done, total) => self.progressed(app, done, total),
				Msg::Done(outcome) => self.finished(app, modals, outcome),
				Msg::Csv { document, probe, path } => {
					if document == app.document_id() && app.document.project.probes.iter().any(|p| p.name == probe) {
						self.ask(ctx, app, modals, Plan::Csv { probe, path }, 0);
					}
				}
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

impl ExportDialog {
	/// The path box and its browse button; whether the box has the focus.
	pub(super) fn output_row(&mut self, ui: &mut Ui, title: &str) -> bool {
		if let Some(path) = self.browsed.as_ref().and_then(|rx| rx.try_recv().ok()) {
			self.path = path.display().to_string();
			self.browsed = None;
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
		typing
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

	/// The path typed, with the format's extension when it has none.
	pub(super) fn output(&self) -> Option<PathBuf> {
		let path = PathBuf::from(&self.path);
		Some(if path.extension().is_none() { path.with_extension(self.extension()) } else { path })
	}

	/// MP4's profile, and the codec's notice and choice; whether the export can start.
	pub(super) fn notes(&mut self, ui: &mut Ui, app: &App, out: &mut Vec<Event>) -> bool {
		let s = &app.strings;
		if self.format() == Some(ExportFormat::Mp4) {
			let (current, supported) = (app.settings.h264_profile, h264::OpenH264Factory::PROFILES);
			if let Some(profile) = super::profile_row(ui, s, current, supported, s.get("ExportH264BaselineOnly")) {
				out.push(Event::SetH264Profile(profile));
			}
		}
		let mp4 = self.format() == Some(ExportFormat::Mp4) && !h264::BUILT_IN;
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
		!mp4 || enabled
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
}
