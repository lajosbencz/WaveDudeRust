//! The platform's file dialogs, each waited for on a thread of its own.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use eframe::egui::{self, DroppedFileHandle};
use wavedude_app_state::document::{Source, Target};
use wavedude_app_state::strings::Strings;

use super::{Files, Picked, is_image};
use crate::App;
use crate::dialogs::{file_dialog, scene_image_filter, scene_image_save_name};
use crate::ui::properties::Dialog;

/// Waits for `dialog` on a thread of its own and gives `then` the file picked, or none; the window repaints after.
pub(in crate::ui) fn on_thread(
	ctx: &egui::Context,
	dialog: impl Future<Output = Option<rfd::FileHandle>> + Send + 'static,
	then: impl FnOnce(Option<PathBuf>) + Send + 'static,
) {
	let repaint = Repaint(ctx.clone());
	std::thread::spawn(move || {
		let _repaint = repaint;
		then(pollster::block_on(dialog).map(|f| f.path().to_owned()));
	});
}

/// Repaints the window when dropped, so a thread that unwinds still wakes it.
struct Repaint(egui::Context);

impl Drop for Repaint {
	fn drop(&mut self) {
		self.0.request_repaint();
	}
}

/// The scene image at `path`, read; its failure's message line.
fn read_scene_image(s: Strings, path: PathBuf) -> Picked {
	match std::fs::read(&path) {
		Ok(bytes) => Picked::SceneImage {
			name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
			path: path.display().to_string(),
			bytes,
		},
		Err(e) => {
			let line = s.format("OpenError", &[&path.display()]);
			tracing::warn!("{line} {e}");
			Picked::Failed(line)
		}
	}
}

impl Files {
	/// Waits for `dialog`'s answer on a thread of its own; `then` makes the answer of the file picked, or of none.
	fn spawn(
		&self,
		ctx: &egui::Context,
		app: &App,
		dialog: impl Future<Output = Option<rfd::FileHandle>> + Send + 'static,
		then: impl FnOnce(Option<PathBuf>) -> Option<Picked> + Send + 'static,
	) {
		let reply = self.reply(ctx, app);
		on_thread(ctx, dialog, move |picked| {
			let answer = catch_unwind(AssertUnwindSafe(|| then(picked)))
				.unwrap_or_else(|_| Some(Picked::Failed("the file thread panicked".into())));
			if let Some(answer) = answer {
				reply(answer);
			}
		});
	}

	/// A dropped file: opened from its path, or, an image, read on a thread of its own.
	pub(super) fn read_dropped(&self, ctx: &egui::Context, app: &App, file: DroppedFileHandle) {
		let (reply, s, path) = (self.reply(ctx, app), app.strings, file.path().to_owned());
		if is_image(&path) {
			std::thread::spawn(move || reply(read_scene_image(s, path)));
		} else {
			reply(Picked::Dropped(Source::Path(path)));
		}
	}

	/// File > Open, in the project's folder.
	pub(in crate::ui) fn open_project(&self, ctx: &egui::Context, app: &App) {
		let d = file_dialog(app.strings.get("OpenFileDialogFilter"), app.document.folder());
		self.spawn(ctx, app, d.pick_file(), |p| p.map(|p| Picked::Project(Source::Path(p))));
	}

	/// Save As, proposing `proposed` or the document's own; a cancel aborts the action that waits for the save.
	pub(in crate::ui) fn save_as(&self, ctx: &egui::Context, app: &App, proposed: Option<PathBuf>) {
		let proposed = proposed.or_else(|| app.document.proposed_save_path());
		let d = file_dialog(app.strings.get("SaveFileDialogFilter"), proposed.as_deref());
		self.spawn(ctx, app, d.save_file(), |p| Some(Picked::SaveAs(p.map(Target::Path))));
	}

	/// Shows a pane's `dialog`; the scene image is read or written on the dialog's thread.
	pub(in crate::ui) fn ask(&self, ctx: &egui::Context, app: &App, dialog: Dialog) {
		let s = app.strings;
		let doc = &app.document;
		match dialog {
			Dialog::PickSceneImage => {
				let d = file_dialog(&scene_image_filter(&s), doc.folder());
				self.spawn(ctx, app, d.pick_file(), move |p| Some(read_scene_image(s, p?)));
			}
			Dialog::PickScheme => {
				let d = file_dialog(s.get("ColorSchemeFilter"), None);
				self.spawn(ctx, app, d.pick_file(), |p| p.map(|p| Picked::Scheme(Source::Path(p))));
			}
			Dialog::SaveSceneImage => {
				let scene = &doc.project.scene;
				let Some(bytes) = scene.image_data.clone() else { return };
				let name = scene_image_save_name(&s, &scene.image);
				let ext = format!(".{}", name.extension().unwrap_or_default().to_string_lossy());
				let dir = Path::new(&scene.image_path).parent().filter(|p| p.is_dir()).or(doc.folder());
				let proposed = dir.map(|d| d.join(&name));
				let d = file_dialog(&s.format("SceneImageSaveFilterFormat", &[&ext]), proposed.as_deref());
				self.spawn(ctx, app, d.save_file(), move |target| {
					let target = target?;
					let e = std::fs::write(&target, &bytes).err()?;
					let line = s.format("SceneImageSaveError", &[&target.display()]);
					tracing::warn!("{line}: {e}");
					Some(Picked::Failed(line))
				});
			}
		}
	}
}
