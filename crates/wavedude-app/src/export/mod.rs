//! Exports of the run, a frame and a probe's CSV: what the dialog proposes and what the job needs, shared; the job
//! itself per target: on a worker thread natively (`native`), stepped on the page in the browser (`web`), whose
//! outputs are downloaded (`delivery`).

#[cfg(any(target_arch = "wasm32", test))]
pub mod delivery;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{fetches_codec, run, would_replace};
#[cfg(target_arch = "wasm32")]
pub use web::{Runner, Started, Status, start};

#[cfg(not(target_arch = "wasm32"))]
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;

use wavedude_app_state::ExportKind;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::model::{Layer, Project};
use wavedude_core::run::store::FrameStore;
use wavedude_export::fs::OutputFs;
use wavedude_export::{ExportFormat, ExportLayers, ExportPlan, FrameSource, naming};

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

/// How an export ended, with its log line or error.
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

/// Everything the job needs, taken on the UI thread.
pub struct Work {
	pub store: Arc<FrameStore>,
	pub project: Project,
	pub traces: Vec<ProbeTrace>,
	pub total_steps: u64,
	pub schemes: SchemeStore,
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
			traces: traces(app),
			total_steps: u64::from(app.live.run_request().expect("a run with a store has a request").schedule().steps),
			schemes: app.schemes.clone(),
			#[cfg(not(target_arch = "wasm32"))]
			use_openh264: app.settings.use_openh264 == Some(true),
			#[cfg(not(target_arch = "wasm32"))]
			codec_dir: app.codec_dir.clone(),
			#[cfg(not(target_arch = "wasm32"))]
			codec_download: Cell::new(None),
		})
	}

	/// The frames of `choice` in `format` at `path` of `fs`; a still of timeline frame `frame`, or of the last one
	/// held.
	fn export_plan(
		&self,
		choice: ExportKindChoice,
		format: ExportFormat,
		path: PathBuf,
		frame: usize,
		fs: Arc<dyn OutputFs>,
	) -> ExportPlan<'_> {
		let count = FrameSource::frame_count(&*self.store);
		ExportPlan {
			project: &self.project,
			format,
			layers: choice.layers(),
			fs,
			path,
			still: choice.is_still().then(|| frame.min(count.saturating_sub(1))),
			frame_count: count,
			traces: &self.traces,
			total_steps: self.total_steps,
			keep_partial: false,
			h264: None,
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
		let rows = wavedude_export::csv::write_probe_csv(out, p, trace, ve, 1).map_err(|e| e.to_string())?;
		out.flush().map_err(|e| e.to_string())?;
		Ok(format!("{rows} rows of {}", p.name))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::dialogs::filters;
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
