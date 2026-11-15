//! The browser: no file paths, no threads, the navigator's facts, the page's local storage.

use std::path::{Path, PathBuf};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wavedude_core::quality::MachineFacts;
use web_sys::wasm_bindgen::JsValue;

use super::Instant;
use crate::document::Target;
use crate::log::{CRASH_KEY, LogRing, RingLayer, SHOWN_CRASH_KEY, max_level};

/// Files have paths: recent files, the log folder, saving in place.
pub const HAS_PATHS: bool = false;

/// The memory the machine's facts assume where the browser does not tell (`navigator.deviceMemory` is not universal).
const UNKNOWN_MEMORY: u64 = 1 << 30;

/// A positive number of the navigator of the page or the worker it is called in.
fn navigator_number(key: &str) -> Option<f64> {
	let navigator = js_sys::Reflect::get(&js_sys::global(), &"navigator".into()).ok()?;
	js_sys::Reflect::get(&navigator, &key.into()).ok()?.as_f64().filter(|v| v.is_finite() && *v > 0.0)
}

/// `navigator.deviceMemory` in bytes (rounded to a power of 2 and clamped by the browser); `None` where the browser
/// does not tell.
pub fn device_memory() -> Option<u64> {
	navigator_number("deviceMemory").map(|gib| (gib * (1u64 << 30) as f64) as u64)
}

/// [`device_memory`], [`UNKNOWN_MEMORY`] where unknown, and `navigator.hardwareConcurrency`; the GPU and the drive
/// unknown.
pub fn machine_facts() -> MachineFacts {
	MachineFacts {
		physical_bytes: device_memory().unwrap_or(UNKNOWN_MEMORY),
		threads: navigator_number("hardwareConcurrency").map_or(1, |t| t as u32).max(1),
		has_gpu: None,
		cache_drive_seek_penalty: None,
	}
}

/// Whether the primary pointer is a finger (`(pointer: coarse)`: phones and tablets, not touchscreen laptops).
pub fn touch_first() -> bool {
	web_sys::window()
		.and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
		.is_some_and(|query| query.matches())
}

/// The JS error's name and message.
pub(super) fn describe(e: &JsValue) -> String {
	let field = |key: &str| js_sys::Reflect::get(e, &key.into()).ok().and_then(|v| v.as_string());
	match (field("name"), field("message")) {
		(Some(name), Some(message)) => format!("{name}: {message}"),
		_ => format!("{e:?}"),
	}
}

/// Never exists: a browser page cannot watch a file.
#[derive(Debug)]
pub enum FileWatch {}

impl FileWatch {
	pub fn start(_path: PathBuf) -> Option<Self> {
		None
	}

	pub fn path(&self) -> &Path {
		match *self {}
	}

	pub fn changed_at(&self) -> Option<Instant> {
		match *self {}
	}

	pub fn set_changed_at(&self, _at: Option<Instant>) {
		match *self {}
	}
}

/// The prefix of the store's keys in the page's local storage.
const PREFIX: &str = "wavedude/";

fn js_error(e: JsValue) -> std::io::Error {
	std::io::Error::other(format!("{e:?}"))
}

fn local_storage() -> std::io::Result<web_sys::Storage> {
	web_sys::window()
		.ok_or_else(|| std::io::Error::other("no window"))?
		.local_storage()
		.map_err(js_error)?
		.ok_or_else(|| std::io::Error::other("no local storage"))
}

/// Keys are `wavedude/<key>` in the page's local storage, `/` separating folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store(());

impl Store {
	/// The page's local storage; `None` where the browser refuses it.
	pub fn open() -> Option<Self> {
		match local_storage() {
			Ok(_) => Some(Self(())),
			Err(e) => {
				tracing::warn!("the settings are not kept: {e}");
				None
			}
		}
	}

	/// Where `key` is kept, for messages.
	pub fn location(&self, key: &str) -> String {
		format!("local storage {PREFIX}{key}")
	}

	/// The text of `key`; `None` when there is none.
	pub fn read(&self, key: &str) -> std::io::Result<Option<String>> {
		local_storage()?.get_item(&format!("{PREFIX}{key}")).map_err(js_error)
	}

	pub fn write(&self, key: &str, text: &str) -> std::io::Result<()> {
		local_storage()?.set_item(&format!("{PREFIX}{key}"), text).map_err(js_error)
	}

	/// The keys directly in folder `prefix`, sorted.
	pub fn keys(&self, prefix: &str) -> std::io::Result<Vec<String>> {
		let storage = local_storage()?;
		let folder = format!("{PREFIX}{prefix}/");
		let mut keys = Vec::new();
		for i in 0..storage.length().map_err(js_error)? {
			if let Some(key) = storage.key(i).map_err(js_error)?
				&& key.strip_prefix(&folder).is_some_and(|name| !name.contains('/'))
			{
				keys.push(key[PREFIX.len()..].to_owned());
			}
		}
		keys.sort();
		Ok(keys)
	}

	/// Removes `key`; none is no error.
	pub fn remove(&self, key: &str) -> std::io::Result<()> {
		local_storage()?.remove_item(&format!("{PREFIX}{key}")).map_err(js_error)
	}
}

/// Hands `bytes` to the browser as a download of the target's name; a page has no file paths. Called while the
/// user's action is in force, as a browser may drop a download a page starts on its own (decision 0025).
pub fn write(target: &Target, bytes: &[u8]) -> std::io::Result<()> {
	let Target::Download(name) = target else {
		return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no file paths in the browser"));
	};
	Download::of_bytes(bytes, name)?.start()
}

/// An object URL, revoked when dropped.
#[derive(Debug)]
struct ObjectUrl(String);

impl Drop for ObjectUrl {
	fn drop(&mut self) {
		if let Err(e) = web_sys::Url::revoke_object_url(&self.0) {
			tracing::warn!("the URL {} could not be revoked: {}", self.0, describe(&e));
		}
	}
}

thread_local! {
	/// The URL of the download started last, kept until the next starts: a browser may fetch it after the click
	/// returns.
	static STARTED: std::cell::RefCell<Option<std::rc::Rc<ObjectUrl>>> = const { std::cell::RefCell::new(None) };
}

/// A file ready for the browser's download: an object URL and the name it is saved as. [`start`](Self::start) is for
/// the user's click (decision 0025).
#[derive(Debug)]
pub struct Download {
	url: std::rc::Rc<ObjectUrl>,
	name: String,
}

impl Download {
	/// `bytes` to be saved as `name`.
	pub fn of_bytes(bytes: &[u8], name: &str) -> std::io::Result<Self> {
		let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
		Self::of_blob(&web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(js_error)?, name)
	}

	/// `blob` to be saved as `name`.
	pub(super) fn of_blob(blob: &web_sys::Blob, name: &str) -> std::io::Result<Self> {
		let url = web_sys::Url::create_object_url_with_blob(blob).map_err(js_error)?;
		Ok(Self { url: std::rc::Rc::new(ObjectUrl(url)), name: name.to_owned() })
	}

	/// The name the file is saved as.
	pub fn name(&self) -> &str {
		&self.name
	}

	/// Starts the browser's download, by a click on an anchor of the URL; the URL lives until the next download
	/// starts, whatever becomes of `self`.
	pub fn start(&self) -> std::io::Result<()> {
		use web_sys::wasm_bindgen::JsCast;
		let anchor = web_sys::window()
			.and_then(|w| w.document())
			.ok_or_else(|| std::io::Error::other("no document"))?
			.create_element("a")
			.map_err(js_error)?
			.dyn_into::<web_sys::HtmlAnchorElement>()
			.map_err(|e| js_error(e.into()))?;
		anchor.set_href(&self.url.0);
		anchor.set_download(&self.name);
		anchor.click();
		STARTED.with_borrow_mut(|started| *started = Some(self.url.clone()));
		Ok(())
	}
}

/// Nothing to keep alive: the console writes at once.
pub struct LogGuard(());

/// Installs the global subscriber: the browser's console and `ring`.
pub fn init_log(_store: &Store, ring: LogRing) -> anyhow::Result<LogGuard> {
	tracing_subscriber::registry()
		.with(max_level())
		.with(
			tracing_subscriber::fmt::layer()
				.with_writer(tracing_web::MakeWebConsoleWriter::new())
				.with_ansi(false)
				.without_time(),
		)
		.with(RingLayer(ring))
		.try_init()?;
	Ok(LogGuard(()))
}

/// Logs every panic, and appends it to the store's crash report, shown at the next start (a panic ends the
/// page's program). No backtrace: the browser has none for wasm.
pub fn install_panic_hook(store: &Store) {
	let store = store.clone();
	let default = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |info| {
		let report = format!("WaveDude Rust {} in the browser\n{info}\n", crate::build_id());
		let before = store.read(CRASH_KEY).ok().flatten().unwrap_or_default();
		if let Err(e) = store.write(CRASH_KEY, &(before + &report)) {
			tracing::error!("the crash report could not be kept: {e}");
		}
		tracing::error!("{report}");
		default(info);
	}));
}

/// The crash report a panic of an earlier session left, moved aside so it is shown once: its text.
pub fn take_crash_report(store: &Store) -> Option<String> {
	let taken = store.read(CRASH_KEY).and_then(|report| {
		let Some(report) = report else { return Ok(None) };
		store.write(SHOWN_CRASH_KEY, &report)?;
		store.remove(CRASH_KEY)?;
		Ok(Some(report))
	});
	taken.unwrap_or_else(|e| {
		tracing::warn!("the crash report could not be taken: {e}");
		None
	})
}
