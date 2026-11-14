//! Exports of the run, a frame and a probe's CSV: what the dialog proposes, what an export would replace, and the
//! worker's job. Threads: [`run`] blocks; the rest reads the state on the UI thread.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use wavedude_app_state::ExportKind;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::model::{Layer, Project};
use wavedude_core::run::store::FrameStore;
use wavedude_export::{ExportFormat, ExportLayers, ExportPlan, ExportState, FrameSource, h264, naming};

use crate::App;
use crate::downloads::Download;
use crate::menus::ExportKindChoice;

impl ExportKindChoice {
	/// The layers it writes.
	pub fn layers(self) -> ExportLayers {
		match self {
			Self::Field | Self::FieldFrame => ExportLayers::Field,
			Self::Intensity | Self::IntensityFrame => ExportLayers::Intensity,
			Self::Both => ExportLayers::Both,
		}
	}

	/// One frame, not the run.
	pub fn is_still(self) -> bool {
		matches!(self, Self::FieldFrame | Self::IntensityFrame)
	}

	/// The dialog's title.
	pub fn title_key(self) -> &'static str {
		match self {
			Self::Field => "ExportDialogTitleField",
			Self::Intensity => "ExportDialogTitleIntensity",
			Self::Both => "ExportDialogTitleBoth",
			Self::FieldFrame | Self::IntensityFrame => "ExportDialogTitleFrame",
		}
	}

	/// The formats the dialog offers.
	pub fn formats(self) -> &'static [ExportFormat] {
		const STILL: [ExportFormat; 4] =
			[ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Exr, ExportFormat::ExrRoot];
		if self.is_still() { &STILL } else { &ExportFormat::ALL }
	}

	/// The save dialog's filter list.
	pub fn filter_key(self) -> &'static str {
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

/// An export to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
	Frames {
		choice: ExportKindChoice,
		format: ExportFormat,
		path: PathBuf,
		frame: usize,
		repeat: bool,
	},
	/// The probe by name, so a reordered list still finds it.
	Csv {
		probe: String,
		path: PathBuf,
	},
}

/// How a worker's job ended, with its log line or error.
pub enum Outcome {
	Completed(String),
	Cancelled(String),
	Failed(String),
}

/// The export dialog's start for a command: the proposed path and the format preselected.
pub fn request(app: &App, choice: ExportKindChoice) -> ExportRequest {
	let kind = if choice.is_still() { ExportKind::Still } else { ExportKind::Run };
	let project_name = app.document.display_name("").trim_end_matches(".wdproj").to_owned();
	let path = app.export_proposal(kind);
	let remembered = app.settings.export_filter_index as usize;
	let frame = app.live.playback.position() as usize;
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
	let layer = match choice.layers() {
		ExportLayers::Field => Some(Layer::Field),
		ExportLayers::Intensity => Some(Layer::Intensity),
		ExportLayers::Both => None,
	};
	// A still keeps the last one's folder and is named for its layer and frame; a run keeps the last name, its layer
	// suffix swapped.
	let path = if choice.is_still() {
		path.with_file_name(naming::still_file_name(&project_name, layer, frame, ext))
	} else {
		naming::with_layer_suffix(&path, layer).with_extension(ext)
	};
	ExportRequest { choice, path, format }
}

/// The probe CSV export's proposed path and the probe's name, which finds it in a reordered list.
pub fn probe_csv_request(app: &App, probe: usize) -> (PathBuf, String) {
	let project = app.document.display_name("").trim_end_matches(".wdproj").to_owned();
	let probe = app.document.project.probes[probe].name.clone();
	let proposal = app.export_proposal(ExportKind::Probe);
	let start = if proposal.extension().is_some_and(|e| e == "csv") {
		proposal
	} else {
		proposal.with_file_name(naming::probe_csv_file_name(&project, &probe))
	};
	(start, probe)
}

/// What an export would replace, one question each: a layer's sequence folder holding files, or its existing video or
/// still; empty when nothing is in the way or the export repeats the last one (which replaces on purpose).
pub fn would_replace(plan: &Plan) -> Vec<(&'static str, PathBuf)> {
	let Plan::Frames { format, path, repeat: false, choice, .. } = plan else { return Vec::new() };
	let both = choice == &ExportKindChoice::Both;
	let sequence = !choice.is_still() && !format.is_video();
	let outputs = choice.layers().layers().iter().map(move |&layer| naming::layer_path(path, layer, both));
	if sequence {
		outputs
			.map(|p| naming::sequence_folder(&p))
			.filter(|f| std::fs::read_dir(f).is_ok_and(|mut d| d.next().is_some()))
			.map(|f| ("ExportFolderExistsFormat", f))
			.collect()
	} else {
		outputs.filter(|p| p.is_file()).map(|p| ("ExportFileExistsFormat", p)).collect()
	}
}

/// The plan writes the Intensity layer, which the run must record.
pub fn needs_intensity(plan: &Plan) -> bool {
	match plan {
		Plan::Frames { choice, .. } => choice.layers().layers().contains(&Layer::Intensity),
		Plan::Csv { .. } => false,
	}
}

/// The plan's kind of export and its path, for the export hint.
pub fn plan_kind(plan: &Plan) -> (ExportKind, PathBuf) {
	match plan {
		Plan::Frames { choice, path, .. } => {
			(if choice.is_still() { ExportKind::Still } else { ExportKind::Run }, path.clone())
		}
		Plan::Csv { path, .. } => (ExportKind::Probe, path.clone()),
	}
}

fn traces(app: &App) -> Vec<ProbeTrace> {
	let mut out = Vec::new();
	app.live.with_traces(&mut |t| out = t.to_vec());
	out
}

/// Everything the worker needs, taken on the UI thread.
pub struct Work {
	pub store: Arc<FrameStore>,
	pub project: Project,
	pub traces: Vec<ProbeTrace>,
	pub total_steps: u64,
	pub schemes: SchemeStore,
	pub use_openh264: bool,
	pub codec_dir: Option<PathBuf>,
	/// The status bar's entry for the codec's download, taken by it.
	pub codec_download: Cell<Option<Download>>,
}

impl Work {
	/// What the worker needs of the live run; `None` without a run to export from.
	pub fn of(app: &App) -> Option<Self> {
		app.live.run_store().map(|store| Self {
			store,
			project: app.document.project.clone(),
			traces: traces(app),
			total_steps: u64::from(app.live.run_request().expect("a run with a store has a request").schedule().steps),
			schemes: app.schemes.clone(),
			use_openh264: app.settings.use_openh264 == Some(true),
			codec_dir: app.codec_dir.clone(),
			codec_download: Cell::new(None),
		})
	}
}

/// The export downloads Cisco's `OpenH264` before it starts.
pub fn fetches_codec(plan: &Plan, work: &Work, app: &App) -> bool {
	matches!(plan, Plan::Frames { format: ExportFormat::Mp4, .. })
		&& work.use_openh264
		&& !h264::BUILT_IN
		&& !crate::codec::downloaded(app)
}

/// Runs `plan` on the calling thread, reporting `(done, total)` frames; `cancel` stops it.
pub fn run(work: &Work, plan: Plan, progress: impl FnMut(usize, usize), cancel: &AtomicBool) -> Outcome {
	match plan {
		Plan::Csv { probe, path } => write_csv(work, &probe, &path).map_or_else(Outcome::Failed, Outcome::Completed),
		Plan::Frames { choice, format, path, frame, .. } => {
			export_frames(work, choice, format, &path, frame, progress, cancel)
		}
	}
}

fn write_csv(work: &Work, probe: &str, path: &Path) -> Result<String, String> {
	let p = work.project.probes.iter().find(|p| p.name == probe).ok_or("no such probe")?;
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
	mut progress: impl FnMut(usize, usize),
	cancel: &AtomicBool,
) -> Outcome {
	let store = &*work.store;
	let count = FrameSource::frame_count(store);
	let h264 = match format {
		ExportFormat::Mp4 => match codec(work) {
			Ok(h264) => h264,
			Err(e) => return Outcome::Failed(e),
		},
		_ => None,
	};
	let plan = ExportPlan {
		project: &work.project,
		format,
		layers: choice.layers(),
		fs: Arc::new(wavedude_export::fs::StdFs),
		path: path.to_path_buf(),
		still: choice.is_still().then(|| frame.min(count.saturating_sub(1))),
		frame_count: count,
		traces: &work.traces,
		total_steps: work.total_steps,
		keep_partial: false,
		h264,
	};
	let job = match plan.build_job(work.store.clone(), &work.schemes) {
		Ok(job) => job,
		Err(e) => return Outcome::Failed(e.to_string()),
	};
	let report = job.run(&mut progress, cancel);
	match report.state {
		ExportState::Completed => {
			Outcome::Completed(format!("{} frames exported to {}", report.frames_done, path.display()))
		}
		ExportState::Cancelled => Outcome::Cancelled(format!("export cancelled after {} frames", report.frames_done)),
		ExportState::Failed(e) => Outcome::Failed(e.to_string()),
	}
}

/// The `OpenH264` library for an MP4 export, downloaded at its first use.
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::dialogs::filters;
	use wavedude_app_state::strings::{Language, Strings};

	#[test]
	fn both_layers_ask_about_each_output_in_the_way() {
		let dir = tempfile::tempdir().unwrap();
		let plan = |format| Plan::Frames {
			choice: ExportKindChoice::Both,
			format,
			path: dir.path().join("run.mp4"),
			frame: 0,
			repeat: false,
		};
		assert!(would_replace(&plan(ExportFormat::Mp4)).is_empty());
		for name in ["run_field.mp4", "run_intensity.mp4"] {
			std::fs::write(dir.path().join(name), b"").unwrap();
		}
		let asked: Vec<_> = would_replace(&plan(ExportFormat::Mp4)).into_iter().map(|(_, p)| p).collect();
		assert_eq!(asked, [dir.path().join("run_field.mp4"), dir.path().join("run_intensity.mp4")]);
		for name in ["run_field", "run_intensity"] {
			std::fs::create_dir(dir.path().join(name)).unwrap();
			std::fs::write(dir.path().join(name).join("f.png"), b"").unwrap();
		}
		let asked: Vec<_> = would_replace(&plan(ExportFormat::Png)).into_iter().map(|(_, p)| p).collect();
		assert_eq!(asked, [dir.path().join("run_field"), dir.path().join("run_intensity")]);
	}

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
