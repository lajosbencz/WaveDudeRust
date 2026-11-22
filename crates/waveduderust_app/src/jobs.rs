//! Work a frontend runs off the UI thread as data: the file pickers and the reads and writes of scene images
//! ([`Job`]), the answers they give ([`Answer`]) and what the window does with them ([`Done`]). An answer is dropped
//! when another project replaced the one it was asked for, but for Open's, a drop's, a failure's and an export's.

use std::path::PathBuf;

use crate::commands::Command;
use crate::dialogs::SceneImageFile;
#[cfg(not(target_arch = "wasm32"))]
use crate::dialogs::scene_image_save;
#[cfg(target_arch = "wasm32")]
use crate::dialogs::scene_image_save_name;
use crate::document::{Source, Target};
#[cfg(not(target_arch = "wasm32"))]
use crate::export::session::{Msg, Target as ExportTarget};
use crate::form::Dialog;
use crate::{App, Event, Guarded};

/// Work for a task.
pub enum Job {
	/// File > Open, natively starting in `start`.
	Open { start: Option<PathBuf> },
	/// Save As, proposing `proposed`.
	SaveAs { proposed: Option<PathBuf> },
	/// The scene image picked, natively starting in `start`, and read.
	PickSceneImage { start: Option<PathBuf> },
	/// The embedded scene image saved where the user picks.
	#[cfg(not(target_arch = "wasm32"))]
	SaveSceneImage(crate::dialogs::SceneImageSave),
	/// The embedded scene image downloaded as `name`.
	#[cfg(target_arch = "wasm32")]
	SaveSceneImage { name: String, bytes: std::sync::Arc<[u8]> },
	/// The scene image the project names but lacks, uploaded; the project keeps `path` for it.
	#[cfg(target_arch = "wasm32")]
	LocateSceneImage { path: String },
	/// A colour scheme file picked to add.
	PickScheme,
	/// The save dialog of the probe named `probe`'s CSV, starting at `start`.
	#[cfg(not(target_arch = "wasm32"))]
	ProbeCsv { start: PathBuf, probe: String },
	/// The save dialog of the export dialog's box `target`, with the filter list `spec`, starting at `start`.
	#[cfg(not(target_arch = "wasm32"))]
	Browse { target: ExportTarget, spec: String, start: PathBuf },
}

impl Job {
	/// The job of a form's file dialog; `None` without a scene image to save.
	pub fn of_dialog(app: &App, dialog: Dialog) -> Option<Self> {
		let doc = &app.document;
		Some(match dialog {
			Dialog::PickSceneImage => Self::PickSceneImage { start: doc.folder().map(Into::into) },
			Dialog::PickScheme => Self::PickScheme,
			#[cfg(not(target_arch = "wasm32"))]
			Dialog::SaveSceneImage => Self::SaveSceneImage(scene_image_save(&app.strings, doc)?),
			#[cfg(target_arch = "wasm32")]
			Dialog::SaveSceneImage => {
				let scene = &doc.project.scene;
				let name = scene_image_save_name(&app.strings, &scene.image).display().to_string();
				Self::SaveSceneImage { name, bytes: scene.image_data.clone()? }
			}
		})
	}
}

/// A task's answer.
pub enum Answer {
	Open(Source),
	/// A dropped file that is no image.
	Dropped(Source),
	/// Save As: where to, `None` when cancelled.
	SaveAs(Option<Target>),
	SceneImage(SceneImageFile),
	/// A file that could not be read or written: the message window's line.
	Failed(String),
	#[cfg(not(target_arch = "wasm32"))]
	Export(Msg),
	#[cfg(not(target_arch = "wasm32"))]
	Csv {
		probe: String,
		path: PathBuf,
	},
	#[cfg(not(target_arch = "wasm32"))]
	Browsed(ExportTarget, PathBuf),
}

impl Answer {
	/// Taken whichever project shows.
	fn any_document(&self) -> bool {
		match self {
			Self::Open(_) | Self::Dropped(_) | Self::Failed(_) => true,
			#[cfg(not(target_arch = "wasm32"))]
			Self::Export(_) | Self::Browsed(..) => true,
			_ => false,
		}
	}

	fn done(self) -> Done {
		match self {
			Self::Open(source) => Done::Send(Event::Guarded(Guarded::Open(source))),
			Self::Dropped(source) => Done::Send(Event::DropFile(source)),
			Self::SaveAs(Some(target)) => Done::Send(Event::SaveAs(target)),
			Self::SaveAs(None) => Done::Send(Event::SaveAsCancelled),
			Self::SceneImage(SceneImageFile { name, path, bytes }) => {
				Done::Send(Event::Command(Command::SetSceneImage { name, path, bytes }))
			}
			Self::Failed(line) => Done::Error(line),
			#[cfg(not(target_arch = "wasm32"))]
			Self::Export(msg) => Done::Export(msg),
			#[cfg(not(target_arch = "wasm32"))]
			Self::Csv { probe, path } => Done::Csv { probe, path },
			#[cfg(not(target_arch = "wasm32"))]
			Self::Browsed(target, path) => Done::Browsed(target, path),
		}
	}
}

/// What the window does with an answer.
pub enum Done {
	Send(Event),
	Error(String),
	/// An export job's report.
	#[cfg(not(target_arch = "wasm32"))]
	Export(Msg),
	/// The probe CSV's path, picked for the probe named so.
	#[cfg(not(target_arch = "wasm32"))]
	Csv {
		probe: String,
		path: PathBuf,
	},
	/// An export dialog's box browsed to a path.
	#[cfg(not(target_arch = "wasm32"))]
	Browsed(ExportTarget, PathBuf),
}

/// What to do with `answers`, each with the document it was asked for; those for another document than `document`
/// are dropped.
pub fn answered(document: u64, answers: impl IntoIterator<Item = (u64, Answer)>) -> Vec<Done> {
	answers.into_iter().filter(|(of, a)| *of == document || a.any_document()).map(|(_, a)| a.done()).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_answer_for_another_project_is_dropped_unless_any_takes_it() {
		let answers = [(1, Answer::SaveAs(None)), (2, Answer::SaveAs(None)), (1, Answer::Failed("f".into()))];
		let done = answered(2, answers);
		assert!(matches!(done.as_slice(), [Done::Send(Event::SaveAsCancelled), Done::Error(line)] if line == "f"));
	}
}
