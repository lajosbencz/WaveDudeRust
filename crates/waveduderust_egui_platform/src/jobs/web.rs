//! The browser's tasks: a picker uploads a file through the page's file chooser, a save downloads it.

use std::path::Path;

use waveduderust::io::wdproj;
use waveduderust_app::dialogs::{SceneImageFile, accept, is_scene_image, scene_image_filter, upload};
use waveduderust_app::document::{Source, Target};
use waveduderust_app::jobs::{Answer, Job};
use waveduderust_app::platform::write;
use waveduderust_app::strings::Strings;

use super::Task;

/// Runs `task` with the strings `s`; `reply` gets its answer, once the user picked a file for an upload.
pub(super) fn run(task: Task, s: Strings, reply: impl FnOnce(Option<Answer>) + 'static) {
	let job = match task {
		Task::Job(job) => job,
		Task::Dropped(file) => return dropped(file, s, reply),
	};
	match job {
		Job::Open { .. } | Job::PickScheme => {
			let key = if matches!(job, Job::Open { .. }) { "OpenFileDialogFilter" } else { "ColorSchemeFilter" };
			pick(s, s.get(key), reply, |name, bytes| Answer::Open(Source::Bytes { name, bytes: bytes.into() }));
		}
		Job::PickSceneImage { .. } => pick(s, &scene_image_filter(&s), reply, |name, bytes| {
			Answer::SceneImage(SceneImageFile { name, path: String::new(), bytes })
		}),
		Job::LocateSceneImage { path } => pick(s, &scene_image_filter(&s), reply, move |name, bytes| {
			Answer::SceneImage(SceneImageFile { name, path, bytes })
		}),
		Job::SaveAs { proposed } => {
			let name = proposed.as_deref().and_then(Path::file_name).map_or_else(
				|| Path::new(s.get("Untitled")).with_extension(wdproj::EXTENSION).display().to_string(),
				|n| n.to_string_lossy().into_owned(),
			);
			reply(Some(Answer::SaveAs(Some(Target::Download(name)))));
		}
		Job::SaveSceneImage { name, bytes } => {
			if let Err(e) = write(&Target::Download(name.clone()), &bytes) {
				let line = s.format("SceneImageSaveError", &[&name]);
				tracing::warn!("{line}: {e}");
				reply(Some(Answer::Failed(line)));
			}
		}
	}
}

/// Reads a file dropped on the window: an image becomes the scene's, another file opens.
fn dropped(file: eframe::egui::DroppedFileHandle, s: Strings, reply: impl FnOnce(Option<Answer>) + 'static) {
	wasm_bindgen_futures::spawn_local(async move {
		let name = file.path().display().to_string();
		let bytes = file.bytes_async().await;
		reply(Some(read(s, name, bytes, |name, bytes| {
			if is_scene_image(Path::new(&name)) {
				Answer::SceneImage(SceneImageFile { name, path: String::new(), bytes })
			} else {
				Answer::Dropped(Source::Bytes { name, bytes: bytes.into() })
			}
		})));
	});
}

/// Uploads a file of the filter list `spec`; `reply` gets `then`'s answer of it.
fn pick(
	s: Strings,
	spec: &str,
	reply: impl FnOnce(Option<Answer>) + 'static,
	then: impl FnOnce(String, Vec<u8>) -> Answer + 'static,
) {
	let picked = move |name, bytes| reply(Some(read(s, name, bytes, then)));
	if let Err(e) = upload(&accept(spec), picked) {
		tracing::warn!("the browser offers no upload: {e:?}");
	}
}

/// `then`'s answer of a file's name and bytes; the message window's line when they could not be read.
fn read(
	s: Strings,
	name: String,
	bytes: Result<Vec<u8>, String>,
	then: impl FnOnce(String, Vec<u8>) -> Answer,
) -> Answer {
	match bytes {
		Ok(bytes) => then(name, bytes),
		Err(e) => {
			let line = s.format("OpenError", &[&name]);
			tracing::warn!("{line} {e}");
			Answer::Failed(line)
		}
	}
}
