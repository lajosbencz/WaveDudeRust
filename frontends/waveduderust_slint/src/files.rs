//! Project files: native dialogs over paths; in the browser a picked file's text in and a download out.

use std::cell::RefCell;
use std::rc::Rc;

/// Files picked in the browser, as (name, text), for the window's timer to open.
pub(crate) type Inbox = Rc<RefCell<Vec<(String, String)>>>;

const PROJECTS: (&str, &[&str]) = ("WaveDude project", &["wdproj", "xml"]);
const LEGACY: (&str, &[&str]) = ("Wave Dude 2D", &["json"]);

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn dialog() -> rfd::FileDialog {
	rfd::FileDialog::new().add_filter(PROJECTS.0, PROJECTS.1).add_filter(LEGACY.0, LEGACY.1)
}

/// Asks for a project; its name and text arrive in `inbox`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn pick(inbox: &Inbox) {
	let inbox = inbox.clone();
	wasm_bindgen_futures::spawn_local(async move {
		let dialog = rfd::AsyncFileDialog::new().add_filter(PROJECTS.0, PROJECTS.1).add_filter(LEGACY.0, LEGACY.1);
		if let Some(file) = dialog.pick_file().await {
			let text = String::from_utf8_lossy(&file.read().await).into_owned();
			inbox.borrow_mut().push((file.file_name(), text));
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
