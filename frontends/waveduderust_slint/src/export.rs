//! Exports of the run and of a frame (docs/ui/10, Export; docs/ui/70, Export Save dialog) on a thread of their own.

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{Dialog, Exporting, Plan, delete_codec, remember, start};
use waveduderust::unstable::export::{ExportFormat, ExportLayers};

use crate::ui::{Command, ExportFileFormat};
#[cfg(target_arch = "wasm32")]
pub(crate) use web::{Dialog, Exporting, Plan, delete_codec, remember, start};

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

	#[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "no exports in the browser yet"))]
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
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
	use std::path::{Path, PathBuf};
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
	use std::thread::JoinHandle;

	use slint::{ModelRc, SharedString, VecModel};
	use waveduderust::Backend;
	use waveduderust::io::schemes::SchemeStore;
	use waveduderust::model::{Project, View};
	use waveduderust::unstable::export::h264::{H264Factory, OpenH264Factory, Profile};
	use waveduderust::unstable::export::naming::{layer_outputs, sequence_folder, still_file_name, with_layer_suffix};
	use waveduderust::unstable::export::{ExportFormat, ExportLayers, ExportState};
	use waveduderust_platform::export::{Made, Request, export_frames};
	use waveduderust_platform::settings::{AppDirs, ExportHint, Settings};

	use super::Kind;
	use crate::live::{ExportFrames, Needs};
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
			let layers = match self.kind {
				Kind::Run(l) | Kind::Frame(l) => l,
			};
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

	/// The export dialog while it is open.
	pub(crate) struct Dialog {
		kind: Kind,
		formats: Vec<(ExportFormat, ExportFileFormat)>,
	}

	/// The layers' names of an export, `None` for both.
	fn layer(layers: ExportLayers) -> Option<waveduderust::model::Layer> {
		match layers.layers() {
			[one] => Some(*one),
			_ => None,
		}
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
		/// The dialog of `kind` for the project at `project` named `name`, the shown timeline frame `frame`.
		pub(crate) fn open(
			kind: Kind,
			project: Option<&Path>,
			name: &str,
			frame: u32,
			settings: &Settings,
			words: &Words<'_>,
		) -> Option<(Self, ExportData)> {
			let formats = kind.formats();
			let still = matches!(kind, Kind::Frame(_));
			let remembered = ExportFormat::ALL.get(settings.export_filter_index as usize).copied();
			let remembered = if still && remembered.is_some_and(ExportFormat::is_video) { None } else { remembered };
			let first = if still { ExportFormat::Png } else { formats[0].0 };
			let format = remembered.filter(|f| formats.iter().any(|(g, _)| g == f)).unwrap_or(first);
			let index = formats.iter().position(|(f, _)| *f == format).unwrap_or(0);
			let ext = format.default_extension();
			let hint = project.and_then(|p| settings.export_hint(p));
			let (last, layers) = match kind {
				Kind::Run(l) => (hint.and_then(|h| h.run.clone()), l),
				Kind::Frame(l) => (hint.and_then(|h| h.still.clone()), l),
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
				(Kind::Run(_), Some(last)) => with_layer_suffix(&last, layer(layers)).with_extension(ext),
				(Kind::Run(_), None) => {
					let suffix = layer(layers).map_or("", waveduderust::unstable::export::naming::layer_suffix);
					PathBuf::from(format!("{name}{suffix}.{ext}"))
				}
				(Kind::Frame(_), _) => PathBuf::from(still_file_name(name, layer(layers), frame as usize, ext)),
			};
			let path = folder.join(file.file_name().unwrap_or_default());
			let archive_path = folder.join(format!("{name}.zip"));
			let title = match kind {
				Kind::Frame(_) => words.get_export_frame(),
				Kind::Run(ExportLayers::Field) => words.get_export_field(),
				Kind::Run(ExportLayers::Intensity) => words.get_export_intensity(),
				Kind::Run(ExportLayers::Both) => words.get_export_both(),
			};
			let names: Vec<SharedString> = formats
				.iter()
				.map(|(f, n)| {
					format!("{} (*.{})", words.invoke_export_filter(*n, still), f.extensions().join(", *.")).into()
				})
				.collect();
			let dialog = Self { kind, formats };
			let codec = AppDirs::platform().map(|d| d.codec());
			let mut data = ExportData {
				title,
				formats: ModelRc::new(VecModel::from(names)),
				format: i32::try_from(index).unwrap_or(0),
				path: path.to_string_lossy().as_ref().into(),
				codec_built_in: waveduderust::unstable::export::h264::BUILT_IN,
				codec_installed: codec.is_some_and(|d| waveduderust_platform::codec::installed(&d).is_some()),
				use_openh264: settings.use_openh264 == Some(true),
				archive: settings.export_archive,
				archive_path: archive_path.to_string_lossy().as_ref().into(),
				profile: Profile::ALL.iter().position(|p| *p == settings.h264_profile).map_or(0, |i| i as i32),
				..ExportData::default()
			};
			dialog.follow(&mut data);
			Some((dialog, data))
		}

		fn format_of(&self, data: &ExportData) -> ExportFormat {
			usize::try_from(data.format).ok().and_then(|i| self.formats.get(i)).map_or(ExportFormat::Png, |(f, _)| *f)
		}

		/// The format's flags and the outputs the path names.
		pub(crate) fn follow(&self, data: &mut ExportData) {
			let format = self.format_of(data);
			data.mp4 = format == ExportFormat::Mp4;
			data.several = matches!(self.kind, Kind::Run(l) if l == ExportLayers::Both || !format.is_video());
			data.outputs = match self.kind {
				Kind::Run(ExportLayers::Both) => {
					let outs = layer_outputs(
						&with_format_extension(Path::new(data.path.as_str()), format),
						ExportLayers::Both,
					);
					let names: Vec<String> = outs
						.iter()
						.filter_map(|(_, p)| p.file_name())
						.map(|n| n.to_string_lossy().into_owned())
						.collect();
					names.join(", ").into()
				}
				_ => SharedString::new(),
			};
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
			let (format, name) = self.formats[usize::try_from(data.format).unwrap_or(0).min(self.formats.len() - 1)];
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
			if let Some(i) = ExportFormat::ALL.iter().position(|f| *f == format) {
				settings.export_filter_index = i as u32;
			}
			if data.mp4 {
				if !data.codec_built_in {
					settings.use_openh264 = Some(data.use_openh264);
				}
				settings.h264_profile =
					Profile::ALL.get(usize::try_from(data.profile).unwrap_or(0)).copied().unwrap_or_default();
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
		thread: Option<JoinHandle<Result<String, String>>>,
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
		pub(crate) fn poll(&mut self) -> Result<ExportProgress, Result<String, String>> {
			if self.waiting.is_some() {
				return Ok(ExportProgress { active: true, waiting: true, done: 0, total: 0 });
			}
			if self.thread.as_ref().is_some_and(JoinHandle::is_finished) {
				let outcome = self.thread.take().map_or_else(
					|| Err("gone".into()),
					|t| t.join().unwrap_or_else(|_| Err("the export panicked".into())),
				);
				return Err(outcome);
			}
			let count = |n: &AtomicUsize| i32::try_from(n.load(Ordering::Relaxed)).unwrap_or(i32::MAX);
			Ok(ExportProgress { active: true, waiting: false, done: count(&self.done), total: count(&self.total) })
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
			let layers = match self.kind {
				Kind::Run(layers) | Kind::Frame(layers) => layers,
			};
			if layers == ExportLayers::Field && !view.intensity.preview {
				Needs::Field
			} else {
				Needs::Intensity { root: self.format.intensity_root() }
			}
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
					request.h264 = Some(h264(use_openh264)?);
				}
				let made =
					Made { traces: &frames.traces, frame_count: frames.frame_count, total_steps: frames.total_steps };
				let report = export_frames(
					&request,
					frames.store.clone(),
					&made,
					|d, t| {
						done.store(d, Ordering::Relaxed);
						total.store(t, Ordering::Relaxed);
					},
					&cancel,
				)
				.map_err(|e| e.to_string())?;
				match report.state {
					ExportState::Completed => {
						Ok(report.written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))
					}
					ExportState::Cancelled => {
						Err(format!("the export was stopped after {} frames", report.frames_done))
					}
					ExportState::Failed(e) => {
						Err(format!("the export failed after {} frames: {e}", report.frames_done))
					}
				}
			})
		};
		Exporting { cancel, done, total, thread: Some(thread), waiting: None }
	}
}

/// The browser has no exports yet.
#[cfg(target_arch = "wasm32")]
mod web {
	use std::path::Path;

	use waveduderust::io::schemes::SchemeStore;
	use waveduderust::model::{Project, View};
	use waveduderust_platform::settings::Settings;

	use super::Kind;
	use crate::live::{ExportFrames, Needs};
	use crate::ui::{ExportData, ExportProgress, Words};

	pub(crate) enum Exporting {}

	impl Exporting {
		pub(crate) fn waiting(plan: &Plan, _: u32) -> Self {
			match plan.never {}
		}

		pub(crate) fn waits(&self) -> Option<&(Plan, u32)> {
			match *self {}
		}

		pub(crate) fn stop(&mut self) {
			match *self {}
		}

		pub(crate) fn cancel(&self) {
			match *self {}
		}

		pub(crate) fn poll(&mut self) -> Result<ExportProgress, Result<String, String>> {
			match *self {}
		}
	}

	/// Never made: the browser's dialog does not open.
	#[derive(Clone, Debug)]
	pub(crate) struct Plan {
		pub(crate) kind: Kind,
		never: std::convert::Infallible,
	}

	impl Plan {
		pub(crate) fn needs(&self, _: &View) -> Needs {
			match self.never {}
		}

		pub(crate) fn existing(&self) -> Vec<(std::path::PathBuf, bool)> {
			Vec::new()
		}
	}

	pub(crate) enum Dialog {}

	impl Dialog {
		pub(crate) fn open(
			_: Kind,
			_: Option<&Path>,
			_: &str,
			_: u32,
			_: &Settings,
			_: &Words<'_>,
		) -> Option<(Self, ExportData)> {
			tracing::warn!("exports are not in the browser build yet");
			None
		}

		pub(crate) fn format(&self, _: &mut ExportData, _: i32) {
			match *self {}
		}

		pub(crate) fn browse(&self, _: &mut ExportData, _: &Words<'_>) {
			match *self {}
		}

		pub(crate) fn browse_archive(&self, _: &mut ExportData, _: &Words<'_>) {
			match *self {}
		}

		pub(crate) fn plan(&self, _: &ExportData, _: &mut Settings) -> Result<Plan, String> {
			match *self {}
		}

		pub(crate) fn follow(&self, _: &mut ExportData) {
			match *self {}
		}
	}

	pub(crate) fn remember(_: &Plan, _: Option<&Path>, _: &mut Settings) {}

	pub(crate) fn delete_codec(_: &mut ExportData) {}

	pub(crate) fn start(
		plan: &Plan,
		_: ExportFrames,
		_: &Project,
		_: &View,
		_: u32,
		_: &Settings,
		_: &SchemeStore,
	) -> Exporting {
		match plan.never {}
	}
}
