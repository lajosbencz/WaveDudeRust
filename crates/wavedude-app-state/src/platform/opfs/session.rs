//! This tab's session folder and the storage calls the spill and the spool share.

use std::cell::OnceCell;
use std::io;

use js_sys::{Promise, Reflect};
use wasm_bindgen_futures::JsFuture;
use wavedude_core::run::Token;
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{
	FileSystemDirectoryHandle, FileSystemGetDirectoryOptions, FileSystemRemoveOptions, LockOptions, StorageEstimate,
};

const ROOT: &str = "wavedude-spill";
const LOCK_PREFIX: &str = "wavedude-spill-";

thread_local! {
	/// This tab's session folder, once the start made it.
	static SESSION: OnceCell<FileSystemDirectoryHandle> = const { OnceCell::new() };
}

/// This tab's session folder; `None` before the start made it or when it failed.
pub(super) fn session() -> Option<FileSystemDirectoryHandle> {
	SESSION.with(|s| s.get().cloned())
}

/// The JS error's name and message.
pub(super) fn describe(e: &JsValue) -> String {
	let field = |key: &str| Reflect::get(e, &key.into()).ok().and_then(|v| v.as_string());
	match (field("name"), field("message")) {
		(Some(name), Some(message)) => format!("{name}: {message}"),
		_ => format!("{e:?}"),
	}
}

/// A failed storage operation; a full quota as [`io::ErrorKind::QuotaExceeded`], a missing entry as
/// [`io::ErrorKind::NotFound`].
pub(super) fn storage_error(e: &JsValue) -> io::Error {
	let kind = match Reflect::get(e, &"name".into()).ok().and_then(|v| v.as_string()).as_deref() {
		Some("QuotaExceededError") => io::ErrorKind::QuotaExceeded,
		Some("NotFoundError") => io::ErrorKind::NotFound,
		_ => io::ErrorKind::Other,
	};
	io::Error::new(kind, format!("browser storage: {}", describe(e)))
}

pub(super) async fn awaited<T: JsCast>(promise: Promise) -> io::Result<T> {
	let value = JsFuture::from(promise).await.map_err(|e| storage_error(&e))?;
	value.dyn_into().map_err(|v| io::Error::other(format!("browser storage answered {v:?}")))
}

pub(super) async fn folder(
	parent: &FileSystemDirectoryHandle,
	name: &str,
	create: bool,
) -> io::Result<FileSystemDirectoryHandle> {
	let options = FileSystemGetDirectoryOptions::new();
	options.set_create(create);
	awaited(parent.get_directory_handle_with_options(name, &options)).await
}

/// Removes entry `name` of `parent`, with what it holds when `recursive`.
pub(super) async fn remove_entry(parent: &FileSystemDirectoryHandle, name: &str, recursive: bool) -> io::Result<()> {
	let options = FileSystemRemoveOptions::new();
	options.set_recursive(recursive);
	JsFuture::from(parent.remove_entry_with_options(name, &options)).await.map_err(|e| storage_error(&e))?;
	Ok(())
}

/// Whether `object` has `key`.
fn has(object: &JsValue, key: &str) -> bool {
	Reflect::has(object, &key.into()).unwrap_or(false)
}

/// Takes the session's lock, removes the sessions of tabs gone and makes this one's folder; the free storage.
pub(super) async fn prepare() -> io::Result<u64> {
	let unsupported =
		|| io::Error::new(io::ErrorKind::Unsupported, "this browser has no origin private file system to write to");
	let window = web_sys::window().ok_or_else(unsupported)?;
	let navigator = window.navigator();
	let writable = Reflect::get(&window, &"FileSystemFileHandle".into())
		.and_then(|c| Reflect::get(&c, &"prototype".into()))
		.is_ok_and(|p| has(&p, "createWritable"));
	if !has(&navigator, "locks") || !has(&navigator.storage(), "getDirectory") || !writable {
		return Err(unsupported());
	}
	let random = (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
	let session = format!("{:x}-{random:08x}", js_sys::Date::now() as u64);
	hold(&format!("{LOCK_PREFIX}{session}")).await;

	let root: FileSystemDirectoryHandle = awaited(navigator.storage().get_directory()).await?;
	let spill = folder(&root, ROOT, true).await?;
	for name in entries(&spill).await? {
		remove_if_free(&spill, &name).await;
	}
	let mine = folder(&spill, &session, true).await?;
	SESSION.with(|s| s.set(mine)).expect("one start per tab");

	let estimate = navigator.storage().estimate().map_err(|e| storage_error(&e))?;
	// A dictionary, not a class: no instance check.
	let estimate: StorageEstimate = JsFuture::from(estimate).await.map_err(|e| storage_error(&e))?.unchecked_into();
	let (quota, usage) = (estimate.get_quota().unwrap_or(0.0), estimate.get_usage().unwrap_or(0.0));
	Ok((quota - usage).max(0.0) as u64)
}

/// Requests the lock `name` for the page's life; returns once it is granted.
async fn hold(name: &str) {
	let granted = Token::<()>::default();
	let grant = granted.clone();
	let callback = Closure::once_into_js(move |_lock: JsValue| -> Promise {
		grant.complete(Ok(()));
		Promise::new(&mut |_, _| {})
	});
	let _held = web_sys::window().expect("checked").navigator().locks().request(name, callback.unchecked_ref());
	granted.done().await;
}

/// The names in `folder`.
async fn entries(folder: &FileSystemDirectoryHandle) -> io::Result<Vec<String>> {
	let keys = folder.keys();
	let mut names = Vec::new();
	loop {
		let next = keys.next().map_err(|e| storage_error(&e))?;
		let step = JsFuture::from(next).await.map_err(|e| storage_error(&e))?;
		if Reflect::get(&step, &"done".into()).is_ok_and(|d| d.is_truthy()) {
			return Ok(names);
		}
		if let Some(name) = Reflect::get(&step, &"value".into()).ok().and_then(|v| v.as_string()) {
			names.push(name);
		}
	}
}

/// Removes session folder `name` of `spill` when no tab holds its lock.
async fn remove_if_free(spill: &FileSystemDirectoryHandle, name: &str) {
	let (spill, folder) = (spill.clone(), name.to_owned());
	let callback = Closure::once_into_js(move |lock: JsValue| -> Promise {
		// Held by a live tab: answered with false, a removal with undefined.
		if lock.is_null() {
			return Promise::resolve(&JsValue::FALSE);
		}
		let options = FileSystemRemoveOptions::new();
		options.set_recursive(true);
		spill.remove_entry_with_options(&folder, &options)
	});
	let options = LockOptions::new();
	options.set_if_available(true);
	let locks = web_sys::window().expect("checked").navigator().locks();
	match JsFuture::from(locks.request_with_options(
		&format!("{LOCK_PREFIX}{name}"),
		&options,
		callback.unchecked_ref(),
	))
	.await
	{
		Ok(answer) if answer.is_undefined() => {
			tracing::info!("removed the browser storage of an earlier session, {name}");
		}
		Ok(_) => {}
		Err(e) => tracing::warn!("the browser storage of session {name} could not be removed: {}", describe(&e)),
	}
}
