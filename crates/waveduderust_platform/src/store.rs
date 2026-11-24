//! The settings' key-value store and the pointer facts of the platform.

#[cfg(not(target_arch = "wasm32"))]
mod imp {
	use std::path::PathBuf;

	use crate::settings::AppDirs;

	/// Keys are paths under the config folder, `/` separating folders; a write replaces its file whole.
	#[derive(Clone, Debug, PartialEq, Eq)]
	pub struct Store {
		dir: PathBuf,
	}

	impl Store {
		/// The platform's config folder ([`AppDirs::platform`]); `None` when the platform names no home.
		pub fn open() -> Option<Self> {
			AppDirs::platform().map(|d| d.store())
		}

		/// The keys under `dir`.
		pub fn in_dir(dir: PathBuf) -> Self {
			Self { dir }
		}

		/// The file of `key`.
		pub fn path(&self, key: &str) -> PathBuf {
			key.split('/').fold(self.dir.clone(), |p, part| p.join(part))
		}

		/// Where `key` is kept, for messages.
		pub fn location(&self, key: &str) -> String {
			self.path(key).display().to_string()
		}

		/// The text of `key`; `None` when there is none.
		pub fn read(&self, key: &str) -> std::io::Result<Option<String>> {
			match std::fs::read_to_string(self.path(key)) {
				Ok(text) => Ok(Some(text)),
				Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
				Err(e) => Err(e),
			}
		}

		pub fn write(&self, key: &str, text: &str) -> std::io::Result<()> {
			let path = self.path(key);
			if let Some(dir) = path.parent() {
				std::fs::create_dir_all(dir)?;
			}
			waveduderust::io::write_atomic(&path, text.as_bytes())
		}

		/// Removes `key`; none is no error.
		pub fn remove(&self, key: &str) -> std::io::Result<()> {
			match std::fs::remove_file(self.path(key)) {
				Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
				_ => Ok(()),
			}
		}

		/// The keys of the files directly in folder `prefix`, sorted; none without the folder.
		pub fn keys(&self, prefix: &str) -> std::io::Result<Vec<String>> {
			let entries = match std::fs::read_dir(self.path(prefix)) {
				Ok(d) => d,
				Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
				Err(e) => return Err(e),
			};
			let mut keys = Vec::new();
			for entry in entries {
				let entry = entry?;
				if entry.file_type()?.is_file() {
					keys.push(format!("{prefix}/{}", entry.file_name().to_string_lossy()));
				}
			}
			keys.sort();
			Ok(keys)
		}
	}

	/// Whether the primary pointer is a finger; never on the desktop.
	pub fn touch_first() -> bool {
		false
	}
}

#[cfg(target_arch = "wasm32")]
mod imp {
	use web_sys::wasm_bindgen::JsValue;

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

	/// Whether the primary pointer is a finger (`(pointer: coarse)`: phones and tablets, not touchscreen laptops).
	pub fn touch_first() -> bool {
		web_sys::window()
			.and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
			.is_some_and(|query| query.matches())
	}
}

pub use imp::{Store, touch_first};
