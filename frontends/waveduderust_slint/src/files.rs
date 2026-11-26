//! Project files: native dialogs over paths; in the browser a picked file's text in and downloads out (a save's, an
//! export's).

use std::cell::RefCell;
use std::rc::Rc;

/// Picked files for the window's timer to open, as (the layer a colour scheme is for, or `None` for a project; name;
/// text).
pub(crate) type Inbox = Rc<RefCell<Vec<(Option<Layer>, String, String)>>>;

use crate::ui::{Layer, Words};

const PROJECTS: &[&str] = &["wdproj", "xml"];
const LEGACY: &[&str] = &["json"];
const SCHEMES: &[&str] = &["wdscheme", "json"];

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn dialog(words: &Words<'_>) -> rfd::FileDialog {
	rfd::FileDialog::new().add_filter(words.get_projects(), PROJECTS).add_filter(words.get_legacy_projects(), LEGACY)
}

/// Asks for a project; its name and text arrive in `inbox`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn pick(inbox: &Inbox, words: &Words<'_>) {
	let dialog = rfd::AsyncFileDialog::new()
		.add_filter(words.get_projects(), PROJECTS)
		.add_filter(words.get_legacy_projects(), LEGACY);
	pick_into(inbox, dialog, None);
}

/// Asks for a colour scheme for `layer`; it arrives in `inbox`.
pub(crate) fn pick_scheme(inbox: &Inbox, words: &Words<'_>, layer: Layer) {
	#[cfg(not(target_arch = "wasm32"))]
	if let Some(path) = rfd::FileDialog::new().add_filter(words.get_schemes(), SCHEMES).pick_file() {
		match std::fs::read(&path) {
			Ok(bytes) => {
				let text = String::from_utf8_lossy(&bytes).into_owned();
				inbox.borrow_mut().push((Some(layer), path.display().to_string(), text));
				crate::wake::wake();
			}
			Err(e) => tracing::error!("The colour scheme could not be loaded from '{}': {e}", path.display()),
		}
	}
	#[cfg(target_arch = "wasm32")]
	pick_into(inbox, rfd::AsyncFileDialog::new().add_filter(words.get_schemes(), SCHEMES), Some(layer));
}

#[cfg(target_arch = "wasm32")]
fn pick_into(inbox: &Inbox, dialog: rfd::AsyncFileDialog, what: Option<Layer>) {
	let inbox = inbox.clone();
	wasm_bindgen_futures::spawn_local(async move {
		if let Some(file) = dialog.pick_file().await {
			let text = String::from_utf8_lossy(&file.read().await).into_owned();
			inbox.borrow_mut().push((what, file.file_name(), text));
			crate::wake::wake();
		}
	});
}

/// Hands `text` of media type `mime` to the browser as a download named `name`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn download(name: &str, text: &str, mime: &str) -> Result<(), String> {
	let fail = |e: wasm_bindgen::JsValue| format!("{e:?}");
	let parts = js_sys::Array::of1(&text.into());
	let options = web_sys::BlobPropertyBag::new();
	options.set_type(mime);
	let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options).map_err(fail)?;
	Download::of_blob(&blob, name)?.start()
}

/// A file ready for the browser's download: its object URL and the name it is saved as.
#[cfg(target_arch = "wasm32")]
pub(crate) struct Download {
	url: Rc<ObjectUrl>,
	name: String,
}

/// An object URL, revoked when dropped.
#[cfg(target_arch = "wasm32")]
struct ObjectUrl(String);

#[cfg(target_arch = "wasm32")]
impl Drop for ObjectUrl {
	fn drop(&mut self) {
		let _ = web_sys::Url::revoke_object_url(&self.0);
	}
}

#[cfg(target_arch = "wasm32")]
thread_local! {
	/// The URL of the download started last: the browser may fetch it after the click returns.
	static STARTED: RefCell<Option<Rc<ObjectUrl>>> = const { RefCell::new(None) };
}

#[cfg(target_arch = "wasm32")]
impl Download {
	/// `bytes` to be saved as `name`.
	pub(crate) fn of_bytes(bytes: &[u8], name: &str) -> Result<Self, String> {
		let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
		let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(|e| format!("{e:?}"))?;
		Self::of_blob(&blob, name)
	}

	/// `blob` to be saved as `name`.
	pub(crate) fn of_blob(blob: &web_sys::Blob, name: &str) -> Result<Self, String> {
		let url = web_sys::Url::create_object_url_with_blob(blob).map_err(|e| format!("{e:?}"))?;
		Ok(Self { url: Rc::new(ObjectUrl(url)), name: name.to_owned() })
	}

	/// The name the file is saved as.
	pub(crate) fn name(&self) -> &str {
		&self.name
	}

	/// Starts the browser's download by a click on an anchor of the URL; the URL lives until the next download
	/// starts.
	pub(crate) fn start(&self) -> Result<(), String> {
		use wasm_bindgen::JsCast;
		let fail = |e: wasm_bindgen::JsValue| format!("{e:?}");
		let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
		let a: web_sys::HtmlAnchorElement = document.create_element("a").map_err(fail)?.unchecked_into();
		a.set_href(&self.url.0);
		a.set_download(&self.name);
		a.click();
		STARTED.with_borrow_mut(|started| *started = Some(self.url.clone()));
		Ok(())
	}
}
