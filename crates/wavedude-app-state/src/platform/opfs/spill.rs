//! The frames past a store's memory: one file per frame at `<session>/<store>-<generation>/<frame>`
//! ([`wavedude_core::run::SpillBackend`]).
//!
//! The start, each write and each read wake the window as they complete ([`crate::wake`]). A store's spill is
//! refused when its frames do not fit the free storage estimated at the start.

use std::collections::BTreeSet;
use std::io;
use std::sync::Mutex;
use std::task::{Context, Poll};

use js_sys::Uint8Array;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use wavedude_core::run::{SpillBackend, SpillFactory, StoreLayout, Token};
use wavedude_core::sync::lock;
use web_sys::{
	Blob, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetFileOptions, FileSystemWritableFileStream,
};

use super::session::{awaited, folder, prepare, remove_entry, session, storage_error};

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
			if let Err(e) = remove_entry(&session_folder(), &folder, true).await {
				tracing::warn!("the spilled frames in {folder} could not be removed: {e}");
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
			let started = crate::platform::Instant::now();
			let read = read_frame(&folder, frame).await;
			tracing::debug!(frame, ms = started.elapsed().as_secs_f64() * 1e3, "spilled frame read back");
			done.complete(read);
			crate::wake::wake();
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

fn session_folder() -> FileSystemDirectoryHandle {
	session().expect("a spill is opened after the start made the session")
}

async fn file(folder_name: &str, frame: usize, create: bool) -> io::Result<FileSystemFileHandle> {
	let parent = folder(&session_folder(), folder_name, create).await?;
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
