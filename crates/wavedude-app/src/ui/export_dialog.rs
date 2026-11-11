//! Exports (docs/ui/70, Export Save dialog): an in-window dialog picks the format (the platform dialog cannot
//! report its filter) and the path; a full-quality run is waited for when the current one does not serve; the
//! job then runs on a worker thread, its progress in the status bar, Esc or Stop cancelling it.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use wavedude_widgets::Button;

use freya::prelude::*;
use futures_channel::mpsc::{self, UnboundedSender};
use futures_util::StreamExt;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::model::{Layer, Project};
use wavedude_core::run::store::FrameStore;
use wavedude_export::{ExportFormat, ExportLayers, ExportPlan, ExportState, FrameSource, h264, naming};
use wavedude_widgets::{Progress, use_wave_theme};

use wavedude_app_state::ExportKind;
use wavedude_app_state::live::ExportGate;

use super::app::{Cx, Ui};
use super::dialogs::{self, Modal, button, button_row, filters, frame};
use super::downloads::{self, Download};
use super::properties::{check_box, combo_box, text_box};
use super::shared::App;

/// What an export command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKindChoice {
	Field,
	Intensity,
	Both,
	FieldFrame,
	IntensityFrame,
}

impl ExportKindChoice {
	fn layers(self) -> ExportLayers {
		match self {
			Self::Field | Self::FieldFrame => ExportLayers::Field,
			Self::Intensity | Self::IntensityFrame => ExportLayers::Intensity,
			Self::Both => ExportLayers::Both,
		}
	}

	fn is_still(self) -> bool {
		matches!(self, Self::FieldFrame | Self::IntensityFrame)
	}

	fn title_key(self) -> &'static str {
		match self {
			Self::Field => "ExportDialogTitleField",
			Self::Intensity => "ExportDialogTitleIntensity",
			Self::Both => "ExportDialogTitleBoth",
			Self::FieldFrame | Self::IntensityFrame => "ExportDialogTitleFrame",
		}
	}

	fn formats(self) -> &'static [ExportFormat] {
		const STILL: [ExportFormat; 4] =
			[ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Exr, ExportFormat::ExrRoot];
		if self.is_still() { &STILL } else { &ExportFormat::ALL }
	}

	fn filter_key(self) -> &'static str {
		if self.is_still() { "ExportDialogFilterStill" } else { "ExportDialogFilter" }
	}
}

/// The dialog's start: the command, the proposed path and the format preselected.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportRequest {
	pub choice: ExportKindChoice,
	pub path: PathBuf,
	pub format: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Plan {
	Frames { choice: ExportKindChoice, format: ExportFormat, path: PathBuf, frame: usize, repeat: bool },
	Csv { probe: usize, path: PathBuf },
}

enum Msg {
	Progress(usize, usize),
	Done(Result<String, String>),
}

/// A running export, shown in the status bar.
pub struct ExportProgress {
	pub text: String,
	pub done: usize,
	pub total: usize,
	pub waiting: bool,
	cancel: Arc<AtomicBool>,
	kind: ExportKind,
	path: PathBuf,
}

impl ExportProgress {
	pub fn progress(&self) -> Progress {
		if self.waiting || self.total == 0 {
			Progress::Indeterminate
		} else {
			Progress::Determinate(self.done as f32 / self.total as f32)
		}
	}
}

thread_local! {
	static PENDING: RefCell<Option<Plan>> = const { RefCell::new(None) };
	/// The last run export and the project file it was made for.
	static LAST: RefCell<Option<(Plan, Option<PathBuf>)>> = const { RefCell::new(None) };
	/// What [`stop_then`] runs once the export has ended.
	static THEN: RefCell<Option<Then>> = const { RefCell::new(None) };
}

type Then = Box<dyn FnOnce(&Cx)>;

/// Cancels the export and calls `then` once it has ended, its partial file removed.
pub fn stop_then(cx: &Cx, then: impl FnOnce(&Cx) + 'static) {
	THEN.with_borrow_mut(|t| *t = Some(Box::new(then)));
	cancel(cx);
	if cx.ui.export.peek().is_none() {
		run_then(cx);
	}
}

fn run_then(cx: &Cx) {
	if let Some(then) = THEN.with_borrow_mut(Option::take) {
		then(cx);
	}
}

/// Whether File > Export last has something to repeat for the open project.
pub fn has_last(_ui: &Ui, app: &App) -> bool {
	LAST.with_borrow(|l| l.as_ref().is_some_and(|(_, p)| *p == app.document.path))
}

/// Cancels a running or waiting export; false when there is none.
pub fn cancel(cx: &Cx) -> bool {
	let export = cx.ui.export.peek().as_ref().map(|e| (e.cancel.clone(), e.waiting));
	match export {
		Some((_, true)) => {
			cx.shared.update(|a| a.live.cancel_export());
			run_stopped(cx);
			true
		}
		Some((flag, false)) => {
			flag.store(true, Ordering::Relaxed);
			true
		}
		None => false,
	}
}

/// Opens the export dialog for a command.
pub fn start(cx: &Cx, choice: ExportKindChoice) {
	if cx.shared.peek(|a| a.live.export_busy()) {
		return;
	}
	let (path, remembered, project_name, frame) = cx.shared.peek(|a| {
		let kind = if choice.is_still() { ExportKind::Still } else { ExportKind::Run };
		let stem = a.document.display_name("").trim_end_matches(".wdproj").to_owned();
		(a.export_proposal(kind), a.settings.export_filter_index as usize, stem, a.live.playback.position() as usize)
	});
	let formats = choice.formats();
	// `remembered` indexes `ExportFormat::ALL` (Settings::normalize), the run formats.
	let remembered_format = ExportFormat::ALL[remembered];
	let format = if !choice.is_still() {
		remembered
	} else if remembered_format.supports_still() {
		formats.iter().position(|x| *x == remembered_format).expect("a still format is listed")
	} else {
		0
	};
	let ext = formats[format].default_extension();
	let path = if choice.is_still()
		&& path.file_name().is_none_or(|n| n.is_empty() || Path::new(n) == Path::new(&project_name))
	{
		let layer = if choice == ExportKindChoice::FieldFrame { Layer::Field } else { Layer::Intensity };
		path.with_file_name(naming::still_file_name(&project_name, Some(layer), frame, ext))
	} else {
		path.with_extension(ext)
	};
	cx.ui.show_modal(Modal::Export(ExportRequest { choice, path, format }));
}

/// File > Export last: the last run export again, no questions.
pub fn export_last(cx: &Cx) {
	if cx.shared.peek(|a| a.live.export_busy()) {
		return;
	}
	let last = LAST.with_borrow(|l| l.as_ref().map(|(p, _)| p.clone()));
	if let Some(Plan::Frames { choice, format, path, frame, .. }) = last {
		begin(cx, Plan::Frames { choice, format, path, frame, repeat: true });
	}
}

/// The tree's or the graph box's "Export probe data as CSV".
pub fn export_probe_csv(cx: &Cx, probe: usize) {
	if cx.shared.peek(|a| a.live.export_busy()) {
		return;
	}
	let (proposal, name) = cx.shared.peek(|a| {
		let project = a.document.display_name("").trim_end_matches(".wdproj").to_owned();
		let probe_name = a.document.project.probes[probe].name.clone();
		(a.export_proposal(ExportKind::Probe), naming::probe_csv_file_name(&project, &probe_name))
	});
	let start = if proposal.extension().is_some_and(|e| e == "csv") { proposal } else { proposal.with_file_name(name) };
	let s = cx.strings();
	let cx = cx.clone();
	dialogs::save_file(s.get("ExportDialogFilterCsv"), Some(start), move |path| {
		if let Some(path) = path {
			begin(&cx, Plan::Csv { probe, path });
		}
	});
}

/// What an export would replace: a sequence folder holding files, or an existing video or still; `None` when
/// nothing is in the way or the export repeats the last one (which replaces on purpose).
fn would_replace(plan: &Plan) -> Option<(&'static str, PathBuf)> {
	let Plan::Frames { format, path, repeat: false, choice, .. } = plan else { return None };
	if !choice.is_still() && !format.is_video() {
		let folder = naming::sequence_folder(path);
		let holds_files = std::fs::read_dir(&folder).is_ok_and(|mut d| d.next().is_some());
		return holds_files.then_some(("ExportFolderExistsFormat", folder));
	}
	let both = choice == &ExportKindChoice::Both;
	choice
		.layers()
		.layers()
		.iter()
		.map(|&layer| naming::layer_path(path, layer, both))
		.find(|p| p.is_file())
		.map(|p| ("ExportFileExistsFormat", p))
}

fn begin(cx: &Cx, plan: Plan) {
	if let Some((key, target)) = would_replace(&plan) {
		let s = cx.strings();
		let text = s.format(key, &[&target.display()]);
		let (cx2, plan2) = (cx.clone(), plan.clone());
		let yes = std::rc::Rc::new(move || gate(&cx2, plan2.clone()));
		cx.ui.show_modal(Modal::Confirm { text, yes });
		return;
	}
	gate(cx, plan);
}

fn needs_intensity(plan: &Plan) -> bool {
	match plan {
		Plan::Frames { choice, .. } => choice.layers().layers().contains(&Layer::Intensity),
		Plan::Csv { .. } => false,
	}
}

fn gate(cx: &Cx, plan: Plan) {
	let needs = needs_intensity(&plan);
	let gate = cx.shared.update(|a| {
		let project = a.document.project.clone();
		a.live.request_export(&project, needs, Instant::now())
	});
	let s = cx.strings();
	match gate {
		ExportGate::Ready => run(cx, plan),
		ExportGate::Waiting => {
			let (kind, path) = plan_kind(&plan);
			PENDING.with_borrow_mut(|p| *p = Some(plan));
			set_progress(
				cx,
				ExportProgress {
					text: s.get("ExportStatusWaiting").to_owned(),
					done: 0,
					total: 0,
					waiting: true,
					cancel: Arc::new(AtomicBool::new(false)),
					kind,
					path,
				},
			);
		}
		ExportGate::Failed => cx.ui.show_modal(Modal::Error(s.get("ExportError").to_owned())),
		ExportGate::Busy => tracing::debug!("export refused: another export is busy"),
	}
}

fn plan_kind(plan: &Plan) -> (ExportKind, PathBuf) {
	match plan {
		Plan::Frames { choice, path, .. } => {
			(if choice.is_still() { ExportKind::Still } else { ExportKind::Run }, path.clone())
		}
		Plan::Csv { path, .. } => (ExportKind::Probe, path.clone()),
	}
}

fn set_progress(cx: &Cx, p: ExportProgress) {
	let mut e = cx.ui.export;
	e.set(Some(p));
}

/// The waiting export's run will not complete: the export is dropped.
pub fn run_stopped(cx: &Cx) {
	PENDING.with_borrow_mut(Option::take);
	let mut e = cx.ui.export;
	e.set(None);
}

/// The live controller's run for a waiting export has completed.
pub fn run_ready(cx: &Cx) {
	match PENDING.with_borrow_mut(Option::take) {
		Some(plan) => run(cx, plan),
		None => cx.shared.update(|a| a.live.end_export()),
	}
}

fn traces(app: &App) -> Vec<ProbeTrace> {
	let mut out = Vec::new();
	app.live.with_traces(&mut |t| out = t.to_vec());
	out
}

/// Everything the worker needs, taken on the UI thread.
struct Work {
	store: Arc<FrameStore>,
	project: Project,
	traces: Vec<ProbeTrace>,
	total_steps: u64,
	schemes: SchemeStore,
	use_openh264: bool,
	codec_dir: Option<PathBuf>,
	/// The status bar's entry for the codec's download, taken by it.
	codec_download: Cell<Option<Download>>,
}

fn run(cx: &Cx, plan: Plan) {
	let s = cx.strings();
	let work = cx.shared.peek(|a| {
		a.live.run_store().map(|store| Work {
			store,
			project: a.document.project.clone(),
			traces: traces(a),
			total_steps: u64::from(a.live.run_request().expect("a run with a store has a request").schedule().steps),
			schemes: a.schemes.clone(),
			use_openh264: a.settings.use_openh264 == Some(true),
			codec_dir: a.codec_dir.clone(),
			codec_download: Cell::new(None),
		})
	});
	let Some(work) = work else {
		cx.shared.update(|a| a.live.end_export());
		cx.ui.show_modal(Modal::Error(s.get("ExportError").to_owned()));
		return;
	};
	if let Plan::Frames { .. } = &plan {
		let owner = cx.shared.peek(|a| a.document.path.clone());
		let last = match &plan {
			Plan::Frames { choice, .. } if !choice.is_still() => Some(plan.clone()),
			_ => None,
		};
		if let Some(last) = last {
			LAST.with_borrow_mut(|l| *l = Some((last, owner)));
		}
	}
	let (kind, path) = plan_kind(&plan);
	let fetches_codec = matches!(&plan, Plan::Frames { format: ExportFormat::Mp4, .. })
		&& work.use_openh264
		&& !h264::BUILT_IN
		&& !super::codec::downloaded(cx);
	if fetches_codec {
		work.codec_download.set(Some(downloads::start(cx, s.get("StatusDownloadingCodec").to_owned())));
	}
	let cancel = Arc::new(AtomicBool::new(false));
	let (tx, mut rx) = mpsc::unbounded();
	let flag = cancel.clone();
	let spawned = std::thread::Builder::new().name("export".into()).spawn(move || {
		let result = match plan {
			Plan::Csv { probe, path } => write_csv(&work, probe, &path),
			Plan::Frames { choice, format, path, frame, .. } => {
				export_frames(&work, choice, format, &path, frame, &tx, &flag)
			}
		};
		let _ = tx.unbounded_send(Msg::Done(result));
	});
	if let Err(e) = spawned {
		tracing::error!("the export thread could not start: {e}");
		cx.shared.update(|a| a.live.end_export());
		return;
	}
	set_progress(
		cx,
		ExportProgress {
			text: s.format("ExportStatusFormat", &[&0, &0]),
			done: 0,
			total: 0,
			waiting: false,
			cancel,
			kind,
			path,
		},
	);
	let cx = cx.clone();
	spawn_in_window(async move {
		let result = loop {
			match rx.next().await {
				Some(Msg::Progress(done, total)) => progressed(&cx, done, total),
				Some(Msg::Done(result)) => break result,
				None => break Err("the export thread ended".into()),
			}
		};
		finished(&cx, result);
	});
}

fn write_csv(work: &Work, probe: usize, path: &Path) -> Result<String, String> {
	let p = work.project.probes.get(probe).ok_or("no such probe")?;
	let index = work
		.project
		.probes
		.iter()
		.filter(|q| q.active)
		.position(|q| q.name == p.name)
		.ok_or("the probe is not active")?;
	let trace = work.traces.get(index).ok_or("the run has no data for the probe")?;
	let ve = work.project.simulation.visualize_every;
	let mut file = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
	let rows = wavedude_export::csv::write_probe_csv(&mut file, p, trace, ve, 1).map_err(|e| e.to_string())?;
	std::io::Write::flush(&mut file).map_err(|e| e.to_string())?;
	Ok(format!("{rows} rows of {} written to {}", p.name, path.display()))
}

fn export_frames(
	work: &Work,
	choice: ExportKindChoice,
	format: ExportFormat,
	path: &Path,
	frame: usize,
	tx: &UnboundedSender<Msg>,
	cancel: &AtomicBool,
) -> Result<String, String> {
	let store = &*work.store;
	let count = FrameSource::frame_count(store);
	let h264 = if format == ExportFormat::Mp4 { codec(work)? } else { None };
	let plan = ExportPlan {
		project: &work.project,
		format,
		layers: choice.layers(),
		path: path.to_path_buf(),
		still: choice.is_still().then(|| frame.min(count.saturating_sub(1))),
		frame_count: count,
		traces: &work.traces,
		total_steps: work.total_steps,
		keep_partial: false,
		h264,
	};
	let job = plan.build_job(store, &work.schemes).map_err(|e| e.to_string())?;
	let report = job.run(|done, total| drop(tx.unbounded_send(Msg::Progress(done, total))), cancel);
	match report.state {
		ExportState::Completed => Ok(format!("{} frames exported to {}", report.frames_done, path.display())),
		ExportState::Cancelled => Ok(format!("export cancelled after {} frames", report.frames_done)),
		ExportState::Failed(e) => Err(e.to_string()),
	}
}

/// The OpenH264 library for an MP4 export, downloaded at its first use.
fn codec(work: &Work) -> Result<Option<PathBuf>, String> {
	let dir = work.codec_dir.as_deref().ok_or("the app has no codec folder")?;
	let download = work.codec_download.take();
	let on_chunk = |bytes, total| {
		if let Some(d) = &download {
			d.chunk(bytes, total);
		}
	};
	h264::library(dir, work.use_openh264, on_chunk).map_err(|e| e.to_string())
}

/// The worker's progress, shown in the status bar.
fn progressed(cx: &Cx, done: usize, total: usize) {
	let s = cx.strings();
	let mut state = cx.ui.export;
	let mut w = state.write();
	if let Some(p) = w.as_mut() {
		p.done = done;
		p.total = total;
		p.text = s.format("ExportStatusFormat", &[&done, &total]);
	}
}

/// The worker's end: the run is released, the result told, and what was waiting for the end runs.
fn finished(cx: &Cx, result: Result<String, String>) {
	let mut state = cx.ui.export;
	let ended = state.write().take();
	super::codec::refresh(cx);
	cx.shared.update(|a| a.live.end_export());
	match result {
		Ok(line) => {
			tracing::info!("{line}");
			if let Some(p) = ended {
				cx.shared.update(|a| a.export_done(p.kind, &p.path));
			}
		}
		Err(e) => {
			tracing::error!("export failed: {e}");
			cx.ui.show_modal(Modal::Error(format!("{}\n{e}", cx.strings().get("ExportError"))));
		}
	}
	run_then(cx);
}

/// The export dialog: format and path, then the export.
#[derive(PartialEq)]
pub struct ExportDialog {
	pub request: ExportRequest,
}

impl Component for ExportDialog {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let theme = use_wave_theme();
		let s = cx.strings();
		let request = self.request.clone();
		let format = use_state(move || request.format);
		let initial = self.request.path.display().to_string();
		let mut path = use_state(move || initial);
		let choice = self.request.choice;
		let formats = choice.formats();
		let names: Vec<(String, Vec<String>)> = filters(s.get(choice.filter_key()));
		let items: Vec<(String, String)> =
			names.iter().enumerate().map(|(i, (n, _))| (i.to_string(), n.clone())).collect();
		let pick_format = move |v: String| {
			let (mut format, mut path) = (format, path);
			let i: usize = v.parse().expect("a format entry's value is its index");
			format.set(i);
			let p = PathBuf::from(path.peek().clone()).with_extension(formats[i].default_extension());
			path.set(p.display().to_string());
		};
		let browse = {
			let (name, exts) = &names[*format.peek()];
			let spec = format!("{name}|*.{}", exts.join(";*."));
			move |_: Event<PressEventData>| {
				let start = Some(PathBuf::from(path.peek().clone()));
				dialogs::save_file(&spec, start, move |p| {
					if let Some(p) = p {
						path.set(p.display().to_string());
					}
				});
			}
		};
		let ok = {
			let cx = cx.clone();
			move || {
				let f = formats[*format.peek()];
				let chosen = PathBuf::from(path.peek().clone());
				let chosen =
					if chosen.extension().is_none() { chosen.with_extension(f.default_extension()) } else { chosen };
				if !choice.is_still() {
					let index = ExportFormat::ALL.iter().position(|x| *x == f).expect("a run format is listed") as u32;
					cx.shared.update(|a| a.settings.export_filter_index = index);
				}
				let frame = cx.shared.peek(|a| a.live.playback.position() as usize);
				cx.ui.close_modal();
				begin(&cx, Plan::Frames { choice, format: f, path: chosen, frame, repeat: false });
			}
		};
		let mp4 = formats[*format.read()] == ExportFormat::Mp4 && !h264::BUILT_IN;
		let enabled =
			super::codec::enabled(cx.shared.read(super::shared::Topics::SETTINGS, |a| a.settings.use_openh264));
		let downloaded = *cx.ui.codec_downloaded.read();
		let ready = !mp4 || enabled;
		let codec = mp4.then(|| {
			let (toggle, remove) = (cx.clone(), cx.clone());
			rect()
				.spacing(4.)
				.child(label().text(h264::NOTICE).color(theme.text_dim))
				.child(check_box(s.get("ExportUseOpenH264").to_owned(), enabled, move |on| {
					super::codec::set(&toggle, on)
				}))
				.maybe_child(
					downloaded
						.then(|| button(s.get("ExportDeleteOpenH264"), true, move |_| super::codec::delete(&remove))),
				)
		});
		let ui = cx.ui;
		let ok_key = move || {
			if ready {
				ok()
			}
		};
		let ok = ok_key.clone();
		frame(560.)
			.on_global_key_down(move |e: Event<KeyboardEventData>| match e.key {
				Key::Named(NamedKey::Escape) => {
					e.stop_propagation();
					ui.close_modal();
				}
				Key::Named(NamedKey::Enter) => {
					e.stop_propagation();
					ok_key();
				}
				_ => {}
			})
			.child(
				label()
					.text(s.get(choice.title_key()).to_owned())
					.font_weight(FontWeight::BOLD)
					.font_size(theme.font_size * 1.2),
			)
			.child(combo_box(format.read().to_string(), items, true, "ExportFormat".into(), pick_format))
			.child(
				rect()
					.horizontal()
					.width(Size::fill())
					.content(Content::flex())
					.spacing(4.)
					.cross_align(Alignment::center())
					.child(rect().width(Size::flex(1.)).child(text_box(
						path.read().clone(),
						false,
						move |t| {
							let mut path = path;
							path.set(t)
						},
						"ExportPath".into(),
					)))
					.child(Button::new().compact().on_press(browse).child(label().text("..."))),
			)
			.maybe_child(codec)
			.child(button_row(vec![
				button(s.get("DialogOk"), ready, move |_| ok()),
				button(s.get("DialogCancel"), true, move |_| ui.close_modal()),
			]))
			.into_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_app_state::strings::{Language, Strings};

	#[test]
	fn every_language_names_each_format_once() {
		for l in Language::ALL {
			let s = Strings::new(l);
			for choice in [ExportKindChoice::Field, ExportKindChoice::FieldFrame] {
				assert_eq!(filters(s.get(choice.filter_key())).len(), choice.formats().len(), "{l:?} {choice:?}");
			}
		}
	}
}
