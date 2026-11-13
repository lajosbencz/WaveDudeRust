//! The browser's files: Open, the scene image and the colour scheme pickers upload a file's bytes, a file dropped on
//! the page is read the same; Save As and the scene image's save download them. A run that fails for want of the
//! scene image asks for its upload.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use eframe::egui::{self, DroppedFileHandle};
use eframe::wasm_bindgen::JsCast as _;
use eframe::wasm_bindgen::closure::Closure;
use eframe::web_sys::{self, HtmlInputElement, js_sys};
use wasm_bindgen_futures::JsFuture;
use wavedude_app_state::document::{Source, Target};
use wavedude_app_state::platform::write;
use wavedude_app_state::strings::Strings;
use wavedude_core::io::wdproj;

use super::{Files, Picked, is_image};
use crate::App;
use crate::dialogs::{filters, scene_image_filter, scene_image_save_name};
use crate::ui::modal::{Modal, Modals, Then};
use crate::ui::properties::Dialog;

impl Files {
	/// Asks the browser for a file of the filter list `spec`; `then` makes the answer of its name and bytes.
	fn upload(
		&self,
		ctx: &egui::Context,
		app: &App,
		spec: &str,
		then: impl FnOnce(String, Vec<u8>) -> Picked + 'static,
	) {
		let (reply, s) = (self.reply(ctx, app), app.strings);
		let picked = move |name, bytes| reply(answer(s, name, bytes, then));
		if let Err(e) = upload(&accept(spec), picked) {
			tracing::warn!("the browser offers no upload: {e:?}");
		}
	}

	/// A dropped file, read as an upload.
	pub(super) fn read_dropped(&self, ctx: &egui::Context, app: &App, file: DroppedFileHandle) {
		let (reply, s) = (self.reply(ctx, app), app.strings);
		wasm_bindgen_futures::spawn_local(async move {
			let name = file.path().display().to_string();
			let bytes = file.bytes_async().await;
			reply(answer(s, name, bytes, |name, bytes| {
				if is_image(Path::new(&name)) {
					Picked::SceneImage { path: String::new(), name, bytes }
				} else {
					Picked::Dropped(Source::Bytes { name, bytes: bytes.into() })
				}
			}));
		});
	}

	/// File > Open.
	pub(in crate::ui) fn open_project(&self, ctx: &egui::Context, app: &App) {
		let spec = app.strings.get("OpenFileDialogFilter");
		self.upload(ctx, app, spec, |name, bytes| Picked::Project(Source::Bytes { name, bytes: bytes.into() }));
	}

	/// Save As, and Save of a project without a file: a download named as `proposed` or the document's own.
	pub(in crate::ui) fn save_as(&self, ctx: &egui::Context, app: &App, proposed: Option<PathBuf>) {
		let named = proposed.or_else(|| app.document.proposed_save_path());
		let name = named.as_deref().and_then(Path::file_name).map_or_else(
			|| Path::new(app.strings.get("Untitled")).with_extension(wdproj::EXTENSION).display().to_string(),
			|n| n.to_string_lossy().into_owned(),
		);
		self.reply(ctx, app)(Picked::SaveAs(Some(Target::Download(name))));
	}

	/// Answers a pane's `dialog`: an upload, or the scene image's download.
	pub(in crate::ui) fn ask(&self, ctx: &egui::Context, app: &App, dialog: Dialog) {
		let s = app.strings;
		match dialog {
			Dialog::PickSceneImage => {
				self.upload(ctx, app, &scene_image_filter(&s), |name, bytes| Picked::SceneImage {
					path: String::new(),
					name,
					bytes,
				});
			}
			Dialog::PickScheme => {
				self.upload(ctx, app, s.get("ColorSchemeFilter"), |name, bytes| {
					Picked::Scheme(Source::Bytes { name, bytes: bytes.into() })
				});
			}
			Dialog::SaveSceneImage => {
				let scene = &app.document.project.scene;
				let Some(bytes) = &scene.image_data else { return };
				let name = scene_image_save_name(&s, &scene.image).display().to_string();
				if let Err(e) = write(&Target::Download(name.clone()), bytes) {
					let line = s.format("SceneImageSaveError", &[&name]);
					tracing::warn!("{line}: {e}");
					self.reply(ctx, app)(Picked::Failed(line));
				}
			}
		}
	}

	/// Asks to upload the scene image the project names but lacks; whether it does.
	pub(in crate::ui) fn ask_scene_image(&self, app: &App, modals: &mut Modals) -> bool {
		let Some(name) = app.document.missing_scene_image() else { return false };
		let text = app.strings.format("LocateSceneImageFormat", &[&name]);
		modals.show(Modal::Confirm { text, yes: Then::LocateSceneImage, destroys: false });
		true
	}

	/// The upload of the scene image the project names but lacks; the project keeps its path for it.
	pub(in crate::ui) fn locate_scene_image(&self, ctx: &egui::Context, app: &App) {
		let path = app.document.project.scene.image_path.clone();
		self.upload(ctx, app, &scene_image_filter(&app.strings), move |name, bytes| Picked::SceneImage {
			name,
			path,
			bytes,
		});
	}
}

/// `then`'s answer of a file's name and bytes; the message window's line when they could not be read.
fn answer(
	s: Strings,
	name: String,
	bytes: Result<Vec<u8>, String>,
	then: impl FnOnce(String, Vec<u8>) -> Picked,
) -> Picked {
	match bytes {
		Ok(bytes) => then(name, bytes),
		Err(e) => {
			let line = s.format("OpenError", &[&name]);
			tracing::warn!("{line} {e}");
			Picked::Failed(line)
		}
	}
}

/// The `accept` list of a filter list: its extensions, empty (any file) when it offers all files.
fn accept(spec: &str) -> String {
	let exts: Vec<String> = filters(spec).into_iter().flat_map(|(_, exts)| exts).collect();
	if exts.iter().any(|e| e == "*") {
		String::new()
	} else {
		exts.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(",")
	}
}

thread_local! {
	/// The last chooser's change handler, with its input; the next chooser drops it, so a cancelled pick holds no more.
	static CHOOSER: RefCell<Option<Closure<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// Opens the browser's file chooser for `accept` and gives `then` the chosen file's name and bytes; nothing when the
/// user cancels. The chooser needs the user's recent input, which the frame after a click or key still has.
fn upload(
	accept: &str,
	then: impl FnOnce(String, Result<Vec<u8>, String>) + 'static,
) -> Result<(), eframe::wasm_bindgen::JsValue> {
	let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
	let input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
	input.set_type("file");
	input.set_accept(accept);
	let chooser = input.clone();
	let changed = Closure::<dyn FnMut()>::once(move || {
		let Some(file) = chooser.files().and_then(|f| f.get(0)) else { return };
		wasm_bindgen_futures::spawn_local(async move {
			let name = file.name();
			let read = JsFuture::from(file.array_buffer()).await;
			then(name, read.map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec()).map_err(|e| format!("{e:?}")));
		});
	});
	input.set_onchange(Some(changed.as_ref().unchecked_ref()));
	CHOOSER.with_borrow_mut(|c| *c = Some(changed));
	input.click();
	Ok(())
}
