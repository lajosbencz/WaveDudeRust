//! The file pickers' answers: Open, Save As and the panes' pickers, from the platform's dialogs (`native`) or the
//! browser's uploads and downloads (`web`), and the files dropped on the window. An answer is taken on a later frame
//! and, but for Open's and a failure's, dropped when another project replaced the one it was asked for.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use native::on_thread;
use wavedude_app_state::commands::Command;
use wavedude_app_state::document::{Source, Target};
use wavedude_app_state::{Event, Guarded};
use wavedude_core::io::image_extensions;

use super::modal::{Modal, Modals};
use crate::App;

/// A picker's answer.
enum Picked {
	Project(Source),
	/// Save As: where to, `None` when cancelled.
	SaveAs(Option<Target>),
	/// The scene image, with the path the project keeps for it.
	SceneImage {
		name: String,
		path: String,
		bytes: Vec<u8>,
	},
	Scheme(Source),
	/// A file dropped on the window that is no image: the state opens it or tells its type is none it opens.
	Dropped(Source),
	/// A file that could not be read or written: the message window's line.
	Failed(String),
}

/// Whether a dropped file of `name` is an image for the scene, by its extension.
fn is_image(name: &Path) -> bool {
	let ext = name.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
	image_extensions().contains(&ext.as_str())
}

/// The answers, each with the document it was asked for ([`App::document_id`]).
pub(super) struct Files {
	tx: Sender<(u64, Picked)>,
	rx: Receiver<(u64, Picked)>,
}

impl Default for Files {
	fn default() -> Self {
		let (tx, rx) = channel();
		Self { tx, rx }
	}
}

impl Files {
	/// Sends an answer for the project `app` shows now; the window repaints after.
	fn reply(&self, ctx: &egui::Context, app: &App) -> impl FnOnce(Picked) + Send + 'static {
		let (tx, ctx, project) = (self.tx.clone(), ctx.clone(), app.document_id());
		move |answer| {
			// the window may have closed meanwhile
			let _ = tx.send((project, answer));
			ctx.request_repaint();
		}
	}

	/// Reads the last file dropped on the window this frame: a project or a colour scheme opens, an image becomes the
	/// scene's.
	pub(super) fn drop_in(&self, ctx: &egui::Context, app: &App) {
		if let Some(file) = ctx.input(|i| i.raw.dropped_files.last().cloned()) {
			self.read_dropped(ctx, app, file);
		}
	}

	/// The events of the answers that arrived for the project `app` still shows; a failure's message window.
	pub(super) fn take(&self, app: &App, out: &mut Vec<Event>, modals: &mut Modals) {
		for (project, picked) in self.rx.try_iter() {
			if project != app.document_id()
				&& !matches!(picked, Picked::Project(_) | Picked::Dropped(_) | Picked::Failed(_))
			{
				continue;
			}
			out.push(match picked {
				Picked::Project(source) | Picked::Scheme(source) => Event::Guarded(Guarded::Open(source)),
				Picked::Dropped(source) => Event::DropFile(source),
				Picked::SaveAs(Some(target)) => Event::SaveAs(target),
				Picked::SaveAs(None) => Event::SaveAsCancelled,
				Picked::SceneImage { name, path, bytes } => {
					Event::Command(Command::SetSceneImage { name, path, bytes })
				}
				Picked::Failed(line) => {
					modals.show(Modal::Error(line));
					continue;
				}
			});
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_dropped_file_is_an_image_by_its_extension() {
		for (name, image) in
			[("a.wdproj", false), ("a.WDScheme", false), ("a.txt", false), ("a.png", true), ("a.JPEG", true)]
		{
			assert_eq!(is_image(Path::new(name)), image, "{name}");
		}
	}
}
