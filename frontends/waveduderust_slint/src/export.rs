//! Exports of the run and of a frame (docs/ui/10, Export; docs/ui/70, Export Save dialog) from the live run's frames:
//! natively on a thread of their own, in the browser stepped on the page (decision 0025).

use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{Dialog, Exporting, Plan, delete_codec, remember, start};
use slint::{ModelRc, SharedString, VecModel};
use waveduderust::model::{Layer, View};
use waveduderust::unstable::export::h264::Profile;
use waveduderust::unstable::export::naming::{layer_outputs, layer_suffix, still_file_name};
use waveduderust::unstable::export::{ExportFormat, ExportLayers, ExportState};
use waveduderust_platform::settings::Settings;

use crate::live::Needs;
use crate::ui::{Command, ExportFileFormat, Words};
#[cfg(target_arch = "wasm32")]
pub(crate) use web::{Dialog, Exporting, Plan, delete_codec, remember, start};

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(any(test, target_arch = "wasm32"))]
mod webcodecs;

/// A completed export: its line for the log, and in the browser its file for the download the user starts.
pub(crate) struct Written {
	pub(crate) line: String,
	#[cfg(target_arch = "wasm32")]
	pub(crate) download: crate::files::Download,
}

/// How an export ended.
pub(crate) enum Outcome<T = Written> {
	Written(T),
	/// Stopped by the user after `done` of `total` frames; nothing it wrote is kept.
	Cancelled {
		done: usize,
		total: usize,
	},
	/// Why it failed.
	Failed(String),
}

impl<T> Outcome<T> {
	/// The outcome of a job that ended in `state` after `done` of `total` frames, `written` once completed.
	fn of(state: ExportState, done: usize, total: usize, written: impl FnOnce() -> T) -> Self {
		match state {
			ExportState::Completed => Self::Written(written()),
			ExportState::Cancelled => Self::Cancelled { done, total },
			ExportState::Failed(e) => Self::Failed(format!("the export failed after {done} frames: {e}")),
		}
	}
}

/// The log line of a cancelled export, worded as WaveDude Next's.
pub(crate) fn cancelled_line(done: usize, total: usize) -> String {
	format!("Export cancelled after {done} of {total} frames")
}

/// What the menu asks to export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
	Run(ExportLayers),
	/// The frame shown, of one layer.
	Frame(ExportLayers),
}

impl Kind {
	pub(crate) fn of(command: Command) -> Option<Self> {
		Some(match command {
			Command::ExportField => Self::Run(ExportLayers::Field),
			Command::ExportIntensity => Self::Run(ExportLayers::Intensity),
			Command::ExportBoth => Self::Run(ExportLayers::Both),
			Command::ExportFieldFrame => Self::Frame(ExportLayers::Field),
			Command::ExportIntensityFrame => Self::Frame(ExportLayers::Intensity),
			_ => return None,
		})
	}

	/// The formats it offers, each with its name in the dialog.
	fn formats(self) -> Vec<(ExportFormat, ExportFileFormat)> {
		let all = [
			(ExportFormat::Mp4, ExportFileFormat::Mp4),
			(ExportFormat::Mpeg1, ExportFileFormat::Mpeg1),
			(ExportFormat::Avi, ExportFileFormat::Avi),
			(ExportFormat::Mov, ExportFileFormat::Mov),
			(ExportFormat::MovPng, ExportFileFormat::MovPng),
			(ExportFormat::Png, ExportFileFormat::Png),
			(ExportFormat::Jpeg, ExportFileFormat::Jpeg),
			(ExportFormat::Exr, ExportFileFormat::Exr),
			(ExportFormat::ExrRoot, ExportFileFormat::ExrRoot),
		];
		all.into_iter().filter(|(f, _)| matches!(self, Self::Run(_)) || f.supports_still()).collect()
	}

	fn layers(self) -> ExportLayers {
		match self {
			Self::Run(layers) | Self::Frame(layers) => layers,
		}
	}

	/// The layer of an export of one, `None` for both.
	fn layer(self) -> Option<Layer> {
		match self.layers().layers() {
			[one] => Some(*one),
			_ => None,
		}
	}

	/// The format the dialog starts at, of `formats`, and its index: the remembered one when offered (a still takes no
	/// video), else the first.
	fn first_format(self, formats: &[(ExportFormat, ExportFileFormat)], settings: &Settings) -> (ExportFormat, i32) {
		let still = matches!(self, Self::Frame(_));
		let remembered =
			usize::try_from(settings.export_filter_index).ok().and_then(|i| ExportFormat::ALL.get(i)).copied();
		let remembered = if still && remembered.is_some_and(ExportFormat::is_video) { None } else { remembered };
		let first = if still { ExportFormat::Png } else { formats[0].0 };
		let format = remembered.filter(|f| formats.iter().any(|(g, _)| g == f)).unwrap_or(first);
		let index = formats.iter().position(|(f, _)| *f == format).unwrap_or(0);
		(format, i32::try_from(index).unwrap_or(0))
	}

	/// The file name first proposed for the project named `name`, a still's of timeline frame `frame`.
	fn proposed(self, name: &str, frame: u32, ext: &str) -> String {
		match self {
			Self::Run(_) => format!("{name}{}.{ext}", self.layer().map_or("", layer_suffix)),
			Self::Frame(_) => still_file_name(name, self.layer(), frame as usize, ext),
		}
	}

	fn title(self, words: &Words<'_>) -> SharedString {
		match self {
			Self::Frame(_) => words.get_export_frame(),
			Self::Run(ExportLayers::Field) => words.get_export_field(),
			Self::Run(ExportLayers::Intensity) => words.get_export_intensity(),
			Self::Run(ExportLayers::Both) => words.get_export_both(),
		}
	}

	/// The dialog's names of `formats`.
	fn format_names(self, formats: &[(ExportFormat, ExportFileFormat)], words: &Words<'_>) -> ModelRc<SharedString> {
		let still = matches!(self, Self::Frame(_));
		let names: Vec<SharedString> = formats
			.iter()
			.map(|(f, n)| {
				format!("{} (*.{})", words.invoke_export_filter(*n, still), f.extensions().join(", *.")).into()
			})
			.collect();
		ModelRc::new(VecModel::from(names))
	}

	/// An export in `format` writes more than one file: both layers, or an image sequence.
	fn several(self, format: ExportFormat) -> bool {
		matches!(self, Self::Run(l) if l == ExportLayers::Both || !format.is_video())
	}

	/// What an export in `format` needs of its run.
	fn needs(self, format: ExportFormat, view: &View) -> Needs {
		if self.layers() == ExportLayers::Field && !view.intensity.preview {
			Needs::Field
		} else {
			Needs::Intensity { root: format.intensity_root() }
		}
	}
}

/// The index of the remembered H.264 profile.
fn profile_index(settings: &Settings) -> i32 {
	Profile::ALL.iter().position(|p| *p == settings.h264_profile).map_or(0, |i| i32::try_from(i).unwrap_or(0))
}

/// `path` with `format`'s extension: an export format's extension replaced, any other suffix kept.
fn with_format_extension(path: &Path, format: ExportFormat) -> PathBuf {
	let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
	if ext.is_some_and(|e| ExportFormat::ALL.iter().any(|f| f.extensions().contains(&e.as_str()))) {
		return path.with_extension(format.default_extension());
	}
	let mut name = path.as_os_str().to_owned();
	name.push(".");
	name.push(format.default_extension());
	PathBuf::from(name)
}

/// The dialog's line of the files an export of both layers named `path` writes; empty for one output.
fn outputs_line(kind: Kind, path: &Path, format: ExportFormat) -> SharedString {
	if kind != Kind::Run(ExportLayers::Both) {
		return SharedString::new();
	}
	let outs = layer_outputs(&with_format_extension(path, format), ExportLayers::Both);
	let names: Vec<String> =
		outs.iter().filter_map(|(_, p)| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
	names.join(", ").into()
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
	use std::path::{Path, PathBuf};
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
	use std::thread::JoinHandle;

	use waveduderust::Backend;
	use waveduderust::io::schemes::SchemeStore;
	use waveduderust::model::{Project, View};
	use waveduderust::unstable::export::ExportFormat;
	use waveduderust::unstable::export::h264::{H264Factory, OpenH264Factory, Profile};
	use waveduderust::unstable::export::naming::{layer_outputs, sequence_folder, with_layer_suffix};
	use waveduderust_platform::export::{Made, Request, export_frames};
	use waveduderust_platform::settings::{AppDirs, ExportHint, Settings};

	use super::{Kind, Outcome, Written, outputs_line, profile_index, with_format_extension};
	use crate::document::Document;
	use crate::live::{ExportFrames, Needs};
	use crate::models;
	use crate::ui::{ExportData, ExportFileFormat, ExportProgress, Words};

	/// What an export writes: the dialog's answer, repeated by Export last.
	#[derive(Clone, Debug)]
	pub(crate) struct Plan {
		pub(crate) kind: Kind,
		format: ExportFormat,
		out: PathBuf,
		/// The ZIP archive the outputs go into.
		archive: Option<PathBuf>,
	}

	impl Plan {
		/// The outputs that exist: each with whether it is a sequence's folder holding files.
		pub(crate) fn existing(&self) -> Vec<(PathBuf, bool)> {
			if let Some(archive) = &self.archive {
				return if archive.is_file() { vec![(archive.clone(), false)] } else { Vec::new() };
			}
			let layers = self.kind.layers();
			let sequence = matches!(self.kind, Kind::Run(_)) && !self.format.is_video();
			layer_outputs(&self.out, layers)
				.into_iter()
				.filter_map(|(_, p)| {
					if sequence {
						let folder = sequence_folder(&p);
						let full = std::fs::read_dir(&folder).is_ok_and(|mut d| d.next().is_some());
						full.then_some((folder, true))
					} else {
						p.is_file().then_some((p, false))
					}
				})
				.collect()
		}
	}

	/// The export dialog while it is open.
	pub(crate) struct Dialog {
		kind: Kind,
		formats: Vec<(ExportFormat, ExportFileFormat)>,
	}

	/// The archive at `typed`, `.zip` appended unless it ends so.
	fn archive_path(typed: &str) -> Result<PathBuf, String> {
		if typed.is_empty() || typed.ends_with(std::path::MAIN_SEPARATOR) {
			return Err("Enter an archive name.".into());
		}
		let mut path = PathBuf::from(typed);
		if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
			path.as_mut_os_string().push(".zip");
		}
		let path = std::path::absolute(&path).unwrap_or(path);
		if path.is_dir() {
			return Err(format!("{} is a folder.", path.display()));
		}
		Ok(path)
	}

	impl Dialog {
		/// The dialog of `kind` for `doc` named `name`, the shown timeline frame `frame`.
		pub(crate) fn open(
			kind: Kind,
			doc: &Document,
			name: &str,
			frame: u32,
			settings: &Settings,
			words: &Words<'_>,
		) -> Option<(Self, ExportData)> {
			let project = doc.path.as_deref();
			let formats = kind.formats();
			let (format, index) = kind.first_format(&formats, settings);
			let ext = format.default_extension();
			let hint = project.and_then(|p| settings.export_hint(p));
			let last = match kind {
				Kind::Run(_) => hint.and_then(|h| h.run.clone()),
				Kind::Frame(_) => hint.and_then(|h| h.still.clone()),
			};
			let folder = last
				.as_deref()
				.and_then(Path::parent)
				.or_else(|| project.and_then(Path::parent))
				.map(Path::to_path_buf)
				.or_else(|| Some(settings.last_export_folder.clone()).filter(|f| !f.as_os_str().is_empty()))
				.or_else(std::env::home_dir)
				.unwrap_or_default();
			let file = match (kind, last) {
				(Kind::Run(_), Some(last)) => with_layer_suffix(&last, kind.layer()).with_extension(ext),
				(Kind::Run(_), None) | (Kind::Frame(_), _) => PathBuf::from(kind.proposed(name, frame, ext)),
			};
			let path = folder.join(file.file_name().unwrap_or_default());
			let archive_path = folder.join(format!("{name}.zip"));
			let codec = AppDirs::platform().map(|d| d.codec());
			let mut data = ExportData {
				title: kind.title(words),
				formats: kind.format_names(&formats, words),
				format: index,
				path: path.to_string_lossy().as_ref().into(),
				codec_built_in: waveduderust::unstable::export::h264::BUILT_IN,
				codec_installed: codec.is_some_and(|d| waveduderust_platform::codec::installed(&d).is_some()),
				use_openh264: settings.use_openh264 == Some(true),
				archive: settings.export_archive,
				archive_path: archive_path.to_string_lossy().as_ref().into(),
				profile: profile_index(settings),
				// OpenH264, the one native encoder, encodes Baseline alone
				profile_supported: slint::ModelRc::new(slint::VecModel::from(vec![true, false, false])),
				..ExportData::default()
			};
			let dialog = Self { kind, formats };
			dialog.follow(&mut data);
			Some((dialog, data))
		}

		fn format_of(&self, data: &ExportData) -> ExportFormat {
			models::index_from_ui(data.format).and_then(|i| self.formats.get(i)).map_or(ExportFormat::Png, |(f, _)| *f)
		}

		/// The format's flags and the outputs the path names.
		pub(crate) fn follow(&self, data: &mut ExportData) {
			let format = self.format_of(data);
			data.mp4 = format == ExportFormat::Mp4;
			data.several = self.kind.several(format);
			data.outputs = outputs_line(self.kind, Path::new(data.path.as_str()), format);
		}

		/// Picks format `index`: the path takes its extension.
		pub(crate) fn format(&self, data: &mut ExportData, index: i32) {
			data.format = index;
			let path = PathBuf::from(data.path.as_str());
			if path.file_name().is_some() {
				data.path = with_format_extension(&path, self.format_of(data)).to_string_lossy().as_ref().into();
			}
			self.follow(data);
		}

		/// The platform's save dialog for the path box.
		pub(crate) fn browse(&self, data: &mut ExportData, words: &Words<'_>) {
			let (format, name) =
				self.formats[models::index_from_ui(data.format).unwrap_or(0).min(self.formats.len() - 1)];
			let path = PathBuf::from(data.path.as_str());
			let mut dialog = rfd::FileDialog::new()
				.add_filter(words.invoke_export_filter(name, matches!(self.kind, Kind::Frame(_))), format.extensions());
			if let Some(folder) = path.parent().filter(|f| f.is_dir()) {
				dialog = dialog.set_directory(folder);
			}
			if let Some(file) = path.file_name() {
				dialog = dialog.set_file_name(file.to_string_lossy());
			}
			if let Some(picked) = dialog.save_file() {
				data.path = with_format_extension(&picked, format).to_string_lossy().as_ref().into();
				self.follow(data);
			}
		}

		/// The platform's save dialog for the archive box.
		pub(crate) fn browse_archive(&self, data: &mut ExportData, words: &Words<'_>) {
			let path = PathBuf::from(data.archive_path.as_str());
			let mut dialog = rfd::FileDialog::new().add_filter(words.get_zip_archive().as_str(), &["zip"]);
			if let Some(folder) = path.parent().filter(|f| f.is_dir()) {
				dialog = dialog.set_directory(folder);
			}
			if let Some(file) = path.file_name() {
				dialog = dialog.set_file_name(file.to_string_lossy());
			}
			if let Some(picked) = dialog.save_file() {
				data.archive_path = picked.to_string_lossy().as_ref().into();
			}
		}

		/// The plan of the dialog's answer, the settings it leaves remembered; the reason when the path names no file.
		pub(crate) fn plan(&self, data: &ExportData, settings: &mut Settings) -> Result<Plan, String> {
			let format = self.format_of(data);
			let path = PathBuf::from(data.path.trim());
			if path.file_stem().is_none() || data.path.ends_with(std::path::MAIN_SEPARATOR) {
				return Err("Enter a file name.".into());
			}
			let out = with_format_extension(&path, format);
			let out = std::path::absolute(&out).unwrap_or(out);
			if let Some(i) = ExportFormat::ALL.iter().position(|f| *f == format).and_then(|i| u32::try_from(i).ok()) {
				settings.export_filter_index = i;
			}
			if data.mp4 {
				if !data.codec_built_in {
					settings.use_openh264 = Some(data.use_openh264);
				}
				settings.h264_profile =
					Profile::ALL.get(models::index_from_ui(data.profile).unwrap_or(0)).copied().unwrap_or_default();
			}
			if data.several {
				settings.export_archive = data.archive;
			}
			let archive = match data.several && data.archive {
				true => Some(archive_path(data.archive_path.trim())?),
				false => None,
			};
			Ok(Plan { kind: self.kind, format, out, archive })
		}
	}

	/// Records where `plan` writes: the last export folder, and the project's hint.
	pub(crate) fn remember(plan: &Plan, project: Option<&Path>, settings: &mut Settings) {
		if let Some(folder) = plan.out.parent() {
			settings.last_export_folder = folder.to_path_buf();
		}
		if let Some(project) = project {
			let mut hint = settings.export_hint(project).cloned().unwrap_or(ExportHint {
				project: project.to_path_buf(),
				run: None,
				still: None,
				probe: None,
			});
			match plan.kind {
				Kind::Run(_) => hint.run = Some(plan.out.clone()),
				Kind::Frame(_) => hint.still = Some(plan.out.clone()),
			}
			settings.set_export_hint(hint);
		}
	}

	/// Deletes the downloaded `OpenH264` binary.
	pub(crate) fn delete_codec(data: &mut ExportData) {
		let Some(dirs) = AppDirs::platform() else { return };
		match waveduderust_platform::codec::remove(&dirs.codec()) {
			Ok(()) => data.codec_installed = false,
			Err(e) => tracing::error!("cannot delete the codec: {e}"),
		}
	}

	/// An export waiting for its run, or writing.
	pub(crate) struct Exporting {
		cancel: Arc<AtomicBool>,
		done: Arc<AtomicUsize>,
		total: Arc<AtomicUsize>,
		thread: Option<JoinHandle<Outcome>>,
		/// The plan waiting for its run and the timeline frame of its still.
		waiting: Option<(Plan, u32)>,
	}

	impl Exporting {
		/// `plan` waits for its run; a still is of the timeline frame `position`.
		pub(crate) fn waiting(plan: &Plan, position: u32) -> Self {
			Self {
				cancel: Arc::default(),
				done: Arc::default(),
				total: Arc::default(),
				thread: None,
				waiting: Some((plan.clone(), position)),
			}
		}

		/// The plan of an export waiting for its run, with its still's timeline frame.
		pub(crate) fn waits(&self) -> Option<&(Plan, u32)> {
			self.waiting.as_ref()
		}

		pub(crate) fn cancel(&self) {
			self.cancel.store(true, Ordering::Relaxed);
		}

		/// Cancels and waits for the writer to end.
		pub(crate) fn stop(&mut self) {
			self.cancel();
			if let Some(thread) = self.thread.take() {
				let _ = thread.join();
			}
		}

		/// The progress while it waits or writes; its outcome once it ended.
		pub(crate) fn poll(&mut self) -> Result<ExportProgress, Outcome> {
			if self.waiting.is_some() {
				return Ok(ExportProgress { active: true, waiting: true, ..ExportProgress::default() });
			}
			if self.thread.as_ref().is_some_and(JoinHandle::is_finished) {
				let outcome = self.thread.take().map_or_else(
					|| Outcome::Failed("gone".into()),
					|t| t.join().unwrap_or_else(|_| Outcome::Failed("the export panicked".into())),
				);
				return Err(outcome);
			}
			let count = |n: &AtomicUsize| i32::try_from(n.load(Ordering::Relaxed)).unwrap_or(i32::MAX);
			Ok(ExportProgress {
				active: true,
				done: count(&self.done),
				total: count(&self.total),
				..Default::default()
			})
		}
	}

	/// The H.264 encoders, `OpenH264` downloaded first when the user enabled it and it is missing.
	fn h264(use_openh264: Option<bool>) -> Result<Arc<dyn H264Factory>, String> {
		let dirs = AppDirs::platform().ok_or("no app folder for the codec")?;
		let enabled = waveduderust_platform::codec::enabled(use_openh264);
		let library =
			waveduderust_platform::codec::library(&dirs.codec(), enabled, |_, _| {}).map_err(|e| e.to_string())?;
		Ok(Arc::new(OpenH264Factory::new(library)))
	}

	impl Plan {
		/// What the export needs of its run.
		pub(crate) fn needs(&self, view: &View) -> Needs {
			self.kind.needs(self.format, view)
		}
	}

	/// Starts writing `plan` from the run's `frames`, a still at the timeline frame `position`.
	pub(crate) fn start(
		plan: &Plan,
		frames: ExportFrames,
		project: &Project,
		view: &View,
		position: u32,
		settings: &Settings,
		schemes: &SchemeStore,
	) -> Exporting {
		let (layers, still) = match plan.kind {
			Kind::Run(layers) => (layers, None),
			Kind::Frame(layers) => (layers, Some(position as usize)),
		};
		let format = plan.format;
		let mut request = Request {
			project: project.clone(),
			view: view.clone(),
			format,
			out: plan.out.clone(),
			layers,
			still,
			// the frames are made: no run of the export's own
			backend: Backend::Cpu,
			schemes: schemes.clone(),
			h264: None,
			h264_profile: settings.h264_profile,
			keep_partial: false,
			archive: plan.archive.clone(),
		};
		let use_openh264 = settings.use_openh264;
		let (cancel, done, total) =
			(Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
		let thread = {
			let (cancel, done, total) = (cancel.clone(), done.clone(), total.clone());
			std::thread::spawn(move || {
				if format == ExportFormat::Mp4 {
					match h264(use_openh264) {
						Ok(h264) => request.h264 = Some(h264),
						Err(e) => return Outcome::Failed(e),
					}
				}
				let made =
					Made { traces: &frames.traces, frame_count: frames.frame_count, total_steps: frames.total_steps };
				let progress = |d: usize, t: usize| {
					done.store(d, Ordering::Relaxed);
					total.store(t, Ordering::Relaxed);
				};
				match export_frames(&request, frames.store.clone(), &made, progress, &cancel) {
					Ok(report) => Outcome::of(report.state, report.frames_done, report.frames_total, || {
						let written = report.written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>();
						Written { line: format!("exported {}", written.join(", ")) }
					}),
					Err(e) => Outcome::Failed(e.to_string()),
				}
			})
		};
		Exporting { cancel, done, total, thread: Some(thread), waiting: None }
	}
}

#[cfg(test)]
mod tests {
	use waveduderust::unstable::export::{ExportError, ExportState};

	use super::Outcome;

	#[test]
	fn a_cancelled_job_is_told_from_a_failed_one() {
		let of = |state| Outcome::of(state, 20, 100, || ());
		assert!(matches!(of(ExportState::Completed), Outcome::Written(())));
		assert!(matches!(of(ExportState::Cancelled), Outcome::Cancelled { done: 20, total: 100 }));
		let failed = of(ExportState::Failed(ExportError::NoEncoder));
		assert!(matches!(failed, Outcome::Failed(e) if e.starts_with("the export failed after 20 frames: ")));
	}
}
