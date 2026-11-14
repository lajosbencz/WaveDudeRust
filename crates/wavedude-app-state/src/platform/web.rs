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
/// The program looks for and installs its releases.
pub const HAS_UPDATER: bool = false;
/// Exports of videos, image sequences, stills and probe data to files.
pub const HAS_EXPORTS: bool = true;

/// The memory assumed where the browser does not tell (`navigator.deviceMemory` is Chromium's alone).
const UNKNOWN_MEMORY: u64 = 1 << 30;

/// `navigator.deviceMemory` (rounded, at most 8 GiB) and `navigator.hardwareConcurrency`; the GPU and the drive
/// unknown.
pub fn machine_facts() -> MachineFacts {
	let navigator = web_sys::window().map(|w| w.navigator());
	let threads = navigator.as_ref().map_or(1, |n| n.hardware_concurrency() as u32).max(1);
	let gib = navigator
		.and_then(|n| js_sys::Reflect::get(&n, &js_sys::JsString::from("deviceMemory")).ok())
		.and_then(|v| v.as_f64())
		.filter(|g| g.is_finite() && *g > 0.0);
	MachineFacts {
		physical_bytes: gib.map_or(UNKNOWN_MEMORY, |g| (g * (1u64 << 30) as f64) as u64),
		threads,
		has_gpu: None,
		cache_drive_seek_penalty: None,
	}
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

/// Hands `bytes` to the browser as a download of the target's name; a page has no file paths.
pub fn write(target: &Target, bytes: &[u8]) -> std::io::Result<()> {
	let Target::Download(name) = target else {
		return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no file paths in the browser"));
	};
	let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
	let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(js_error)?;
	let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_error)?;
	click_download(&url, name)?;
	web_sys::Url::revoke_object_url(&url).map_err(js_error)
}

/// Starts the browser's download of object URL `url` as `name`.
pub(super) fn click_download(url: &str, name: &str) -> std::io::Result<()> {
	use web_sys::wasm_bindgen::JsCast;
	let anchor = web_sys::window()
		.and_then(|w| w.document())
		.ok_or_else(|| std::io::Error::other("no document"))?
		.create_element("a")
		.map_err(js_error)?
		.dyn_into::<web_sys::HtmlAnchorElement>()
		.map_err(|e| js_error(e.into()))?;
	anchor.set_href(url);
	anchor.set_download(name);
	anchor.click();
	Ok(())
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
		let report = format!("WaveDude Rust {} in the browser\n{info}\n", env!("CARGO_PKG_VERSION"));
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
