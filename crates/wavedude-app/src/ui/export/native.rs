//! The desktop's exports: each output is a path, typed or browsed, and an export of several files goes into one ZIP
//! archive when the user asks; what an export would replace is asked about first; the job runs on a worker thread;
//! the probe CSV's path comes from the platform's save dialog.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use eframe::egui::{self, Id, RichText, Ui};
use wavedude_app_state::Event;
use wavedude_export::ExportFormat;
use wavedude_export::h264::{self, Profile};

use super::super::files::wait;
use super::super::inbox::Inbox;
use super::super::modal::{Modal, Modals, Then};
use super::super::properties::{self, check_box};
use super::super::theme::Palette;
use super::{ExportDialog, Exports, Step, Target};
use crate::App;
use crate::dialogs::file_dialog;
use crate::export::{self, Outcome, Plan, Work};
use crate::{codec, downloads};

const BROWSE: f32 = 28.;

pub(super) enum Msg {
	Progress(usize, usize),
	Done(Outcome),
	/// The CSV's save dialog answered for the document `document`.
	Csv {
		document: u64,
		probe: String,
		path: PathBuf,
	},
}

/// The worker's and the CSV dialog's answers.
pub(super) type Running = Inbox<Msg>;

/// The user picks whether an export of several files is archived.
pub(super) const ARCHIVE_CHOSEN: bool = true;

/// The dialog's text of a proposed output.
pub(super) fn shown(path: &Path) -> String {
	path.display().to_string()
}

/// The path typed, with extension `ext` when it has none; `None` without a file name.
pub(super) fn typed(text: &str, ext: &str) -> Option<PathBuf> {
	let path = PathBuf::from(text);
	path.file_name()?;
	Some(if path.extension().is_none() { path.with_extension(ext) } else { path })
}

/// An export of several files goes into one archive: the user's choice.
pub(super) fn archived(app: &App) -> bool {
	app.settings.export_archive
}

impl Exports {
	/// "Export probe data as CSV" of probe `probe`: the save dialog, then the export.
	pub(in super::super) fn probe_csv(&self, ctx: &egui::Context, app: &App, _modals: &mut Modals, probe: usize) {
		if app.live.export_busy() {
			return;
		}
		let (start, probe) = export::probe_csv_request(app, probe);
		let dialog = file_dialog(app.strings.get("ExportDialogFilterCsv"), Some(&start)).save_file();
		let document = app.document_id();
		let csv = move |_: &Sender<Msg>| wait(dialog).map(|path| Msg::Csv { document, probe, path });
		self.running.spawn(ctx, "csv-dialog", csv, |_| None);
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
			self.finished(app, modals, Outcome::no_frames());
			return;
		};
		if export::fetches_codec(&plan, &work, app) {
			work.codec_download.set(Some(downloads::start(app.strings.get("StatusDownloadingCodec").to_owned())));
		}
		let flag = self.started(app, &plan);
		let job = move |tx: &Sender<Msg>| {
			let progress = |done, total| drop(tx.send(Msg::Progress(done, total)));
			Some(Msg::Done(export::run(&work, plan, progress, &flag)))
		};
		self.running.spawn(ctx, "export", job, |why| Some(Msg::Done(Outcome::Failed(why))));
	}

	/// Takes the worker's progress and end, and the CSV dialog's answer.
	pub(in super::super) fn poll(&mut self, ctx: &egui::Context, app: &mut App, modals: &mut Modals) {
		let arrived: Vec<Msg> = self.running.try_iter().collect();
		for msg in arrived {
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

impl ExportDialog {
	/// The browse dialogs' answers, into their boxes.
	pub(super) fn take_browsed(&mut self) {
		let answers: Vec<(Target, PathBuf)> = self.browsed.try_iter().collect();
		for (target, path) in answers {
			*self.text_mut(target) = path.display().to_string();
		}
	}

	/// The browse button of the box of `target`.
	pub(super) fn browse_button(&self, ui: &mut Ui, target: Target) {
		if ui.add(egui::Button::new("...").min_size(egui::vec2(BROWSE, 0.))).clicked() {
			self.browse(ui.ctx(), target);
		}
	}

	/// The save dialog for the box of `target`, at the path typed: the chosen format's, or the archive's.
	fn browse(&self, ctx: &egui::Context, target: Target) {
		let (spec, start) = match target {
			Target::Output(i) => {
				let (name, exts) = &self.names[self.format];
				(format!("{name}|*.{}", exts.join(";*.")), &self.outputs[i])
			}
			Target::Archive => (self.archive_filter.to_owned(), &self.archive),
		};
		let dialog = file_dialog(&spec, Some(&PathBuf::from(start))).save_file();
		self.browsed.spawn(ctx, "export-browse", move |_| wait(dialog).map(|path| (target, path)), |_| None);
	}

	/// MP4's profile, and the codec's notice and choice; whether the export can start.
	pub(super) fn notes(&mut self, ui: &mut Ui, app: &App, out: &mut Vec<Event>) -> bool {
		let s = &app.strings;
		if self.format() == Some(ExportFormat::Mp4) {
			let (supported, reason) = h264_profiles(app);
			if let Some(profile) = super::profile_row(ui, s, app.settings.h264_profile, &supported, s.get(reason)) {
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

/// The H.264 profiles MP4 export encodes, and the string key of why the others cannot be picked.
pub(in crate::ui) fn h264_profiles(_: &App) -> (Vec<Profile>, &'static str) {
	(h264::OpenH264Factory::PROFILES.to_vec(), "ExportH264BaselineOnly")
}

/// Why `format` cannot be picked, a string key: never, as OpenH264's use is asked for in the dialog.
pub(super) fn unavailable(_: &App, _: ExportFormat) -> Option<&'static str> {
	None
}
