//! Project files: native dialogs over paths; in the browser a picked file's text in and a download out.

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
		}
	});
}

/// Hands `text` to the browser as a download named `name`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn download(name: &str, text: &str) -> Result<(), String> {
	use wasm_bindgen::JsCast;
	let fail = |e: wasm_bindgen::JsValue| format!("{e:?}");
	let parts = js_sys::Array::of1(&text.into());
	let options = web_sys::BlobPropertyBag::new();
	options.set_type("application/xml");
	let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options).map_err(fail)?;
	let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(fail)?;
	let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
	let a: web_sys::HtmlAnchorElement = document.create_element("a").map_err(fail)?.unchecked_into();
	a.set_href(&url);
	a.set_download(name);
	a.click();
	web_sys::Url::revoke_object_url(&url).map_err(fail)
}
