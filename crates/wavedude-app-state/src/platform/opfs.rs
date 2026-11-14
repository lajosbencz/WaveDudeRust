//! The frames past a store's memory in the origin private file system: one file per frame at
//! `wavedude-spill/<session>/<store>-<generation>/<frame>`, written and read from the main thread
//! ([`wavedude_core::run::SpillBackend`]).
//!
//! The start and each write wake the window as they complete ([`crate::wake`]).
//!
//! A tab holds the Web Lock of its session while it lives. At start the sessions whose lock is free (their tab
//! is gone) are removed, then the free storage is estimated once; a store's spill is refused when its frames do
//! not fit it.

use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::io;
use std::sync::Mutex;
use std::task::{Context, Poll};

use js_sys::{Promise, Reflect, Uint8Array};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use wavedude_core::run::{SpillBackend, SpillFactory, StoreLayout, Token};
use wavedude_core::sync::lock;
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{
	Blob, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions, FileSystemGetFileOptions,
	FileSystemRemoveOptions, FileSystemWritableFileStream, LockOptions, StorageEstimate,
};

const ROOT: &str = "wavedude-spill";
const LOCK_PREFIX: &str = "wavedude-spill-";

thread_local! {
	/// This tab's session folder, once the start made it.
	static SESSION: OnceCell<FileSystemDirectoryHandle> = const { OnceCell::new() };
}

/// The spills of this tab's stores.
pub struct Opfs {
	/// The free storage at start.
	free: Token<u64>,
	taken: Mutex<Option<io::Result<u64>>>,
}

impl Opfs {
	/// Takes this tab's session and starts removing the sessions of tabs gone, then estimating the free storage.
	pub fn start() -> Self {
		let free = Token::default();
		let done = free.clone();
		spawn_local(async move {
			let result = prepare().await;
			if let Err(e) = &result {
				tracing::warn!("frames past memory cannot be spilled: {e}");
			}
			done.complete(result);
			crate::wake::wake();
		});
		Self { free, taken: Mutex::new(None) }
	}
}

impl SpillFactory for Opfs {
	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
		self.free.poll_done(cx)
	}

	fn open(&self, store: u64, _layout: &StoreLayout, bytes: u64) -> io::Result<Box<dyn SpillBackend>> {
		let mut taken = lock(&self.taken);
		if taken.is_none() {
			*taken = self.free.take();
		}
		match taken.as_ref().expect("open follows ready") {
			Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
			Ok(free) if bytes > *free => {
				let gb = |b: u64| b as f64 / f64::from(1u32 << 30);
				Err(io::Error::new(
					io::ErrorKind::StorageFull,
					format!(
						"the frames past memory need {:.2} GB of browser storage, {:.2} GB are free",
						gb(bytes),
						gb(*free)
					),
				))
			}
			Ok(_) => Ok(Box::new(Spill { store, generations: Mutex::default(), writes: Mutex::default() })),
		}
	}
}

/// One store's frames.
struct Spill {
	store: u64,
	/// Generations with a folder.
	generations: Mutex<BTreeSet<u64>>,
	/// (generation, write) in flight.
	writes: Mutex<Vec<(u64, Token<()>)>>,
}

impl Spill {
	fn folder(&self, generation: u64) -> String {
		format!("{}-{generation}", self.store)
	}

	/// Removes `generation`'s folder once its writes in flight are done.
	fn remove(&self, generation: u64) {
		let writes: Vec<Token<()>> =
			lock(&self.writes).iter().filter(|(g, _)| *g == generation).map(|(_, t)| t.clone()).collect();
		let folder = self.folder(generation);
		spawn_local(async move {
			for write in writes {
				write.done().await;
			}
			let options = FileSystemRemoveOptions::new();
			options.set_recursive(true);
			if let Err(e) = JsFuture::from(session().remove_entry_with_options(&folder, &options)).await {
				tracing::warn!("the spilled frames in {folder} could not be removed: {}", describe(&e));
			}
		});
	}
}

impl SpillBackend for Spill {
	fn write(&self, generation: u64, frame: usize, planes: &[&[u8]]) -> Token<()> {
		let data = Uint8Array::new_with_length(planes.iter().map(|p| p.len() as u32).sum());
		let mut at = 0;
		for plane in planes {
			let end = at + plane.len() as u32;
			data.subarray(at, end).copy_from(plane);
			at = end;
		}
		lock(&self.generations).insert(generation);
		let token = Token::default();
		let mut writes = lock(&self.writes);
		writes.retain(|(_, t)| !t.is_done());
		writes.push((generation, token.clone()));
		let (done, folder) = (token.clone(), self.folder(generation));
		spawn_local(async move {
			done.complete(write_frame(&folder, frame, &data).await);
			crate::wake::wake();
		});
		token
	}

	fn read(&self, generation: u64, frame: usize) -> Token<Vec<u8>> {
		let token = Token::default();
		let (done, folder) = (token.clone(), self.folder(generation));
		spawn_local(async move {
			let started = super::Instant::now();
			let read = read_frame(&folder, frame).await;
			tracing::debug!(frame, ms = started.elapsed().as_secs_f64() * 1e3, "spilled frame read back");
			done.complete(read);
		});
		token
	}

	fn drop_generation(&self, generation: u64) {
		if lock(&self.generations).remove(&generation) {
			self.remove(generation);
		}
	}
}

impl Drop for Spill {
	fn drop(&mut self) {
		for generation in std::mem::take(&mut *lock(&self.generations)) {
			self.remove(generation);
		}
	}
}

fn session() -> FileSystemDirectoryHandle {
	SESSION.with(|s| s.get().cloned()).expect("a spill is opened after the start made the session")
}

/// The JS error's name and message.
fn describe(e: &JsValue) -> String {
	let field = |key: &str| Reflect::get(e, &key.into()).ok().and_then(|v| v.as_string());
	match (field("name"), field("message")) {
		(Some(name), Some(message)) => format!("{name}: {message}"),
		_ => format!("{e:?}"),
	}
}

/// A failed storage operation; a full quota as [`io::ErrorKind::QuotaExceeded`].
fn storage_error(e: &JsValue) -> io::Error {
	let full =
		Reflect::get(e, &"name".into()).ok().and_then(|v| v.as_string()).as_deref() == Some("QuotaExceededError");
	let kind = if full { io::ErrorKind::QuotaExceeded } else { io::ErrorKind::Other };
	io::Error::new(kind, format!("browser storage: {}", describe(e)))
}

async fn awaited<T: JsCast>(promise: Promise) -> io::Result<T> {
	let value = JsFuture::from(promise).await.map_err(|e| storage_error(&e))?;
	value.dyn_into().map_err(|v| io::Error::other(format!("browser storage answered {v:?}")))
}

async fn folder(parent: &FileSystemDirectoryHandle, name: &str, create: bool) -> io::Result<FileSystemDirectoryHandle> {
	let options = FileSystemGetDirectoryOptions::new();
	options.set_create(create);
	awaited(parent.get_directory_handle_with_options(name, &options)).await
}

async fn file(folder_name: &str, frame: usize, create: bool) -> io::Result<FileSystemFileHandle> {
	let parent = folder(&session(), folder_name, create).await?;
	let options = FileSystemGetFileOptions::new();
	options.set_create(create);
	awaited(parent.get_file_handle_with_options(&frame.to_string(), &options)).await
}

async fn write_frame(folder_name: &str, frame: usize, data: &Uint8Array) -> io::Result<()> {
	let handle = file(folder_name, frame, true).await?;
	let stream: FileSystemWritableFileStream = awaited(handle.create_writable()).await?;
	let written = async {
		JsFuture::from(stream.write_with_js_u8_array(data)?).await?;
		JsFuture::from(stream.close()).await
	};
	if let Err(e) = written.await {
		// Releases the file's lock and drops its swap file; the stream may have errored already.
		let _ = JsFuture::from(stream.abort()).await;
		return Err(storage_error(&e));
	}
	Ok(())
}

async fn read_frame(folder_name: &str, frame: usize) -> io::Result<Vec<u8>> {
	let handle = file(folder_name, frame, false).await?;
	let blob: Blob = awaited(handle.get_file()).await?;
	let buffer = JsFuture::from(blob.array_buffer()).await.map_err(|e| storage_error(&e))?;
	Ok(Uint8Array::new(&buffer).to_vec())
}

/// Whether `object` has `key`.
fn has(object: &JsValue, key: &str) -> bool {
	Reflect::has(object, &key.into()).unwrap_or(false)
}

/// Takes the session's lock, removes the sessions of tabs gone and makes this one's folder; the free storage.
async fn prepare() -> io::Result<u64> {
	let unsupported = || {
		io::Error::new(io::ErrorKind::Unsupported, "this browser has no origin private file system to spill frames to")
	};
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
			tracing::info!("removed the spilled frames of an earlier session, {name}");
		}
		Ok(_) => {}
		Err(e) => tracing::warn!("the spilled frames of session {name} could not be removed: {}", describe(&e)),
	}
}
