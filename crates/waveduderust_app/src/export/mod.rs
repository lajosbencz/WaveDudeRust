//! Exports of the run, a frame and a probe's CSV: what the dialog proposes and what the job needs, shared; the job
//! itself per target: on a worker thread natively (`native`), stepped on the page in the browser (`web`), whose
//! outputs are downloaded (`delivery`); an export's course from the dialog to its end for any window (`session`).

#[cfg(any(target_arch = "wasm32", test))]
pub mod delivery;
#[cfg(not(target_arch = "wasm32"))]
mod native;
pub mod session;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{fetches_codec, h264_profiles, run, would_replace};
#[cfg(target_arch = "wasm32")]
pub use web::{Runner, Started, Status, h264_profiles, h264_support, start};

#[cfg(not(target_arch = "wasm32"))]
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;

use crate::ExportKind;
use crate::live::RunNeeds;
use crate::platform::SystemTime;
use waveduderust::io::schemes::SchemeStore;
use waveduderust::model::{Layer, ProbeId, Project, View};
use waveduderust::run::store::FrameStore;
use waveduderust::unstable::engine::probes::ProbeTrace;
use waveduderust::unstable::export::fs::OutputFs;
use waveduderust::unstable::export::fs::zip::UtcDateTime;
use waveduderust::unstable::export::h264::Profile;
use waveduderust::unstable::export::{ExportFormat, ExportLayers, ExportPlan, FrameSource, naming};

use crate::App;
#[cfg(not(target_arch = "wasm32"))]
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

	/// Writes more than one file in `format`: both layers, or an image sequence.
	pub fn writes_many(self, format: ExportFormat) -> bool {
		self.layers() == ExportLayers::Both || (!self.is_still() && !format.is_video())
	}

	/// The save dialog's filter list.
	pub fn filter_key(self) -> &'static str {
		if self.is_still() { "ExportDialogFilterStill" } else { "ExportDialogFilter" }
	}
}

/// The string key naming H.264 profile `p`.
pub fn profile_key(p: Profile) -> &'static str {
	match p {
		Profile::Baseline => "ExportH264ProfileBaseline",
		Profile::Main => "ExportH264ProfileMain",
		Profile::High => "ExportH264ProfileHigh",
	}
}

/// The dialog's start: the command, each layer's proposed output, the proposed archive and the format preselected.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportRequest {
	pub choice: ExportKindChoice,
	pub outputs: Vec<(Layer, PathBuf)>,
	pub archive: PathBuf,
	pub format: usize,
}

/// An export to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
	Frames {
		choice: ExportKindChoice,
		format: ExportFormat,
		/// Each layer's output: a path, or with `archive` its name in the archive.
		outputs: Vec<(Layer, PathBuf)>,
		/// The ZIP archive the outputs go into.
		archive: Option<PathBuf>,
		frame: usize,
		repeat: bool,
	},
	/// The probe by name, so a reordered list still finds it.
	Csv { probe: String, path: PathBuf },
}

/// How an export ended, with its log line or error.
pub enum Outcome {
	Completed(String),
	Cancelled(String),
	Failed(Failure),
}

/// Why an export failed: its error, and whether the storage was full.
#[derive(Debug)]
pub struct Failure {
	pub message: String,
	pub storage_full: bool,
}

impl std::fmt::Display for Failure {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.message)
	}
}

impl From<String> for Failure {
	fn from(message: String) -> Self {
		Self { message, storage_full: false }
	}
}

impl From<&str> for Failure {
	fn from(message: &str) -> Self {
		message.to_owned().into()
	}
}

impl From<&std::io::Error> for Failure {
	fn from(e: &std::io::Error) -> Self {
		Self { message: e.to_string(), storage_full: waveduderust::run::storage_full(e.kind()) }
	}
}

impl From<std::io::Error> for Failure {
	fn from(e: std::io::Error) -> Self {
		Self::from(&e)
	}
}

impl From<waveduderust::unstable::export::ExportError> for Failure {
	fn from(e: waveduderust::unstable::export::ExportError) -> Self {
		match &e {
			waveduderust::unstable::export::ExportError::Io(io) => io.into(),
			_ => e.to_string().into(),
		}
	}
}

impl Outcome {
	/// An export cancelled after `frames` frames.
	pub fn cancelled(frames: usize) -> Self {
		Self::Cancelled(format!("export cancelled after {frames} frames"))
	}

	/// An export of a run without frames.
	pub fn no_frames() -> Self {
		Self::Failed("the run has no frames".into())
	}
}

/// The export dialog's start for a command: the proposed outputs and archive, and the format preselected.
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
	let archive = naming::with_layer_suffix(&path, None).with_extension("zip");
	ExportRequest { choice, outputs: naming::layer_outputs(&path, choice.layers()), archive, format }
}

/// The probe CSV export's proposed path and the probe's name, which finds it in a reordered list.
pub fn probe_csv_request(app: &App, probe: ProbeId) -> (PathBuf, String) {
	let project = app.document.display_name("").trim_end_matches(".wdproj").to_owned();
	let probe = app.document.project.probe(probe).expect("an existing probe").name.clone();
	let proposal = app.export_proposal(ExportKind::Probe);
	let start = if proposal.extension().is_some_and(|e| e == "csv") {
		proposal
	} else {
		proposal.with_file_name(naming::probe_csv_file_name(&project, &probe))
	};
	(start, probe)
}

/// What the plan needs of its run: the Intensity layer recorded when it writes it, in the format's form.
pub fn run_needs(plan: &Plan) -> RunNeeds {
	match plan {
		Plan::Frames { choice, format, .. } => {
			RunNeeds::new(*format, choice.layers().layers().contains(&Layer::Intensity))
		}
		Plan::Csv { .. } => RunNeeds::Field,
	}
}

/// The plan's kind of export and its path, for the export hint: the archive, else the first output.
pub fn plan_kind(plan: &Plan) -> (ExportKind, PathBuf) {
	match plan {
		Plan::Frames { choice, outputs, archive, .. } => {
			let path = archive.as_ref().unwrap_or(&outputs[0].1).clone();
			(if choice.is_still() { ExportKind::Still } else { ExportKind::Run }, path)
		}
		Plan::Csv { path, .. } => (ExportKind::Probe, path.clone()),
	}
}

/// Now, the archive entries' time.
fn now() -> UtcDateTime {
	let since = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
	UtcDateTime::from_unix(since.as_secs() as i64)
}

fn traces(app: &App) -> Vec<ProbeTrace> {
	let mut out = Vec::new();
	app.live.with_traces(&mut |t| out = t.to_vec());
	out
}

/// Everything the job needs, taken on the UI thread.
pub struct Work {
	pub store: Arc<FrameStore>,
	pub project: Project,
	pub view: View,
	pub traces: Vec<ProbeTrace>,
	pub total_steps: u64,
	pub schemes: SchemeStore,
	pub h264_profile: Profile,
	#[cfg(not(target_arch = "wasm32"))]
	pub use_openh264: bool,
	#[cfg(not(target_arch = "wasm32"))]
	pub codec_dir: Option<PathBuf>,
	/// The status bar's entry for the codec's download, taken by it.
	#[cfg(not(target_arch = "wasm32"))]
	pub codec_download: Cell<Option<Download>>,
}

impl Work {
	/// What the worker needs of the live run; `None` without a run to export from.
	pub fn of(app: &App) -> Option<Self> {
		app.live.run_store().map(|store| Self {
			store,
			project: app.document.project.clone(),
			view: app.document.view.clone(),
			traces: traces(app),
			total_steps: u64::from(app.live.run_request().expect("a run with a store has a request").schedule().steps),
			schemes: app.schemes.clone(),
			h264_profile: app.settings.h264_profile,
			#[cfg(not(target_arch = "wasm32"))]
			use_openh264: app.settings.use_openh264 == Some(true),
			#[cfg(not(target_arch = "wasm32"))]
			codec_dir: app.codec_dir.clone(),
			#[cfg(not(target_arch = "wasm32"))]
			codec_download: Cell::new(None),
		})
	}

	/// The frames of `choice` in `format` to `outputs` of `fs`; a still of timeline frame `frame`, or of the last one
	/// held.
	fn export_plan(
		&self,
		choice: ExportKindChoice,
		format: ExportFormat,
		outputs: Vec<(Layer, PathBuf)>,
		frame: usize,
		fs: Arc<dyn OutputFs>,
	) -> ExportPlan<'_> {
		let count = FrameSource::frame_count(&*self.store);
		ExportPlan {
			project: &self.project,
			view: &self.view,
			format,
			fs,
			outputs,
			still: choice.is_still().then(|| frame.min(count.saturating_sub(1))),
			frame_count: count,
			traces: &self.traces,
			total_steps: self.total_steps,
			keep_partial: false,
			h264: None,
			h264_profile: self.h264_profile,
		}
	}

	/// Writes the CSV of the probe named `probe` to `out`; the log line's start, what was written.
	fn probe_csv(&self, probe: &str, out: &mut impl std::io::Write) -> Result<String, String> {
		let p = self.project.probes.iter().find(|p| p.name == probe).ok_or("no such probe")?;
		let index = self
			.project
			.probes
			.iter()
			.filter(|q| q.active)
			.position(|q| q.name == p.name)
			.ok_or("the probe is not active")?;
		let trace = self.traces.get(index).ok_or("the run has no data for the probe")?;
		let ve = self.project.simulation.visualize_every;
		let rows =
			waveduderust::unstable::export::csv::write_probe_csv(out, p, trace, ve, 1).map_err(|e| e.to_string())?;
		out.flush().map_err(|e| e.to_string())?;
		Ok(format!("{rows} rows of {}", p.name))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::dialogs::filters;
	use crate::strings::{Language, Strings};

	#[test]
	fn every_language_names_each_format_once() {
		for l in Language::ALL {
			let s = Strings::new(l);
			for choice in [ExportKindChoice::Field, ExportKindChoice::FieldFrame] {
				assert_eq!(filters(s.get(choice.filter_key())).len(), choice.formats().len(), "{l:?} {choice:?}");
			}
			let zip = filters(s.get("ExportDialogFilterZip"));
			assert!(zip.len() == 1 && zip[0].1 == ["zip"], "{l:?}");
		}
	}

	#[test]
	fn an_export_writes_several_files_of_both_layers_or_as_a_sequence() {
		use ExportFormat::{Avi, Png};
		use ExportKindChoice::{Both, Field, FieldFrame};
		assert!(Both.writes_many(Avi) && Both.writes_many(Png) && Field.writes_many(Png));
		assert!(!Field.writes_many(Avi) && !FieldFrame.writes_many(Png));
	}
}
