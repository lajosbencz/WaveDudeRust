//! Exports of the run and of a frame (docs/ui/10, Export; docs/ui/70, Export Save dialog) on a thread of their own.

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{Exporting, start};
use waveduderust::unstable::export::{ExportFormat, ExportLayers};

use crate::ui::Command;
#[cfg(target_arch = "wasm32")]
pub(crate) use web::{Exporting, start};

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
	fn formats(self) -> Vec<ExportFormat> {
		let all = [
			ExportFormat::Mp4,
			ExportFormat::Mpeg1,
			ExportFormat::Avi,
			ExportFormat::Mov,
			ExportFormat::MovPng,
			ExportFormat::Png,
			ExportFormat::Jpeg,
			ExportFormat::Exr,
			ExportFormat::ExrRoot,
		];
		all.into_iter().filter(|f| matches!(self, Self::Run(_)) || f.supports_still()).collect()
	}
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
	use std::path::PathBuf;
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
	use std::thread::JoinHandle;

	use waveduderust::Backend;
	use waveduderust::io::schemes::SchemeStore;
	use waveduderust::model::{Project, View};
	use waveduderust::unstable::export::h264::{H264Factory, OpenH264Factory, Profile};
	use waveduderust::unstable::export::{ExportFormat, ExportState};
	use waveduderust_platform::export::{Request, export};
	use waveduderust_platform::settings::{AppDirs, Settings};

	use super::Kind;
	use crate::ui::ExportProgress;

	/// An export in progress.
	pub(crate) struct Exporting {
		cancel: Arc<AtomicBool>,
		done: Arc<AtomicUsize>,
		total: Arc<AtomicUsize>,
		thread: Option<JoinHandle<Result<String, String>>>,
	}

	impl Exporting {
		pub(crate) fn cancel(&self) {
			self.cancel.store(true, Ordering::Relaxed);
		}

		/// The progress while it runs; its outcome once it ended.
		pub(crate) fn poll(&mut self) -> Result<ExportProgress, Result<String, String>> {
			if self.thread.as_ref().is_some_and(JoinHandle::is_finished) {
				let outcome = self.thread.take().map_or_else(
					|| Err("gone".into()),
					|t| t.join().unwrap_or_else(|_| Err("the export panicked".into())),
				);
				return Err(outcome);
			}
			let count = |n: &AtomicUsize| i32::try_from(n.load(Ordering::Relaxed)).unwrap_or(i32::MAX);
			Ok(ExportProgress { active: true, done: count(&self.done), total: count(&self.total) })
		}
	}

	/// The H.264 encoders where the user enabled OpenH264 or it is built in.
	fn h264(settings: &Settings) -> Option<Arc<dyn H264Factory>> {
		let dirs = AppDirs::platform()?;
		let enabled = waveduderust_platform::codec::enabled(settings.use_openh264);
		let library = waveduderust_platform::codec::library(&dirs.codec(), enabled, |_, _| {}).ok()?;
		Some(Arc::new(OpenH264Factory::new(library)))
	}

	/// Asks where to save, then starts the export; `None` when the user cancelled.
	pub(crate) fn start(
		kind: Kind,
		project: &Project,
		view: &View,
		position: u32,
		settings: &Settings,
	) -> Option<Exporting> {
		let mut dialog = rfd::FileDialog::new();
		for f in kind.formats() {
			dialog = dialog.add_filter(format!("{f:?}"), f.extensions());
		}
		let out: PathBuf =
			dialog.set_file_name(format!("export.{}", kind.formats()[0].default_extension())).save_file()?;
		let format = ExportFormat::from_path(&out).unwrap_or(kind.formats()[0]);
		let (layers, still) = match kind {
			Kind::Run(layers) => (layers, None),
			Kind::Frame(layers) => (layers, Some(position as usize)),
		};
		let request = Request {
			project: project.clone(),
			view: view.clone(),
			format,
			out,
			layers,
			still,
			backend: if settings.use_gpu { Backend::Gpu } else { Backend::Cpu },
			schemes: SchemeStore::default(),
			h264: if format == ExportFormat::Mp4 { h264(settings) } else { None },
			h264_profile: Profile::default(),
			keep_partial: false,
		};
		let (cancel, done, total) =
			(Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
		let thread = {
			let (cancel, done, total) = (cancel.clone(), done.clone(), total.clone());
			std::thread::spawn(move || {
				let report = export(
					request,
					|d, t| {
						done.store(d, Ordering::Relaxed);
						total.store(t, Ordering::Relaxed);
					},
					&cancel,
				)
				.map_err(|e| e.to_string())?
				.report;
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
		Some(Exporting { cancel, done, total, thread: Some(thread) })
	}
}

/// The browser has no exports yet.
#[cfg(target_arch = "wasm32")]
mod web {
	use waveduderust::model::{Project, View};
	use waveduderust_platform::settings::Settings;

	use super::Kind;
	use crate::ui::ExportProgress;

	pub(crate) enum Exporting {}

	impl Exporting {
		pub(crate) fn cancel(&self) {
			match *self {}
		}

		pub(crate) fn poll(&mut self) -> Result<ExportProgress, Result<String, String>> {
			match *self {}
		}
	}

	pub(crate) fn start(_: Kind, _: &Project, _: &View, _: u32, _: &Settings) -> Option<Exporting> {
		tracing::warn!("exports are not in the browser build yet");
		None
	}
}
