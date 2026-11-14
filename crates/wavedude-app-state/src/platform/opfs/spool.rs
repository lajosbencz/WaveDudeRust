//! An export's files at `<session>/export-<n>/<path>` ([`Spool`]), downloaded or read back from there.
//!
//! A file is written through a `FileSystemWritableFileStream`, whose swap file the browser moves in place at the
//! close: its writes are queued ([`super::queue`]) and one task per file posts them in order, each completion
//! waking the window ([`crate::wake`]). Folders exist in the spool's books at once and in the browser's storage
//! once a file in them is written.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Component, Path};
use std::rc::Rc;
use std::task::Poll;

use js_sys::Uint8Array;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use wavedude_core::run::Token;
use wavedude_export::fs::{OutputFile, OutputFs};
use web_sys::{
	Blob, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetFileOptions, FileSystemWritableFileStream, Url,
	WriteCommandType, WriteParams,
};

use super::queue::{Next, Queue};
use super::session::{awaited, folder, remove_entry, session, storage_error};

/// Bytes queued and not written at which [`OutputFs::poll_ready`] holds the writer back.
const IN_FLIGHT: u64 = 64 << 20;
/// Writes go through a buffer of this many bytes.
const BUFFER: usize = 1 << 20;

thread_local! {
	/// Spools made in this tab.
	static SPOOLS: Cell<u64> = const { Cell::new(0) };
}

/// An [`OutputFs`] of one export's files in this tab's session folder. Dropping it aborts the writes in flight,
/// revokes its downloads' URLs and removes its folder.
pub struct Spool {
	session: FileSystemDirectoryHandle,
	/// `export-<n>`.
	name: String,
	queue: Rc<RefCell<Queue>>,
	/// Each file's task, done once the file is committed or dropped.
	files: RefCell<BTreeMap<String, Token<()>>>,
	dirs: RefCell<BTreeSet<String>>,
	/// The removal queued last; each runs after the one before.
	removal: RefCell<Token<()>>,
	/// Object URLs of the downloads.
	urls: Rc<RefCell<Vec<String>>>,
}

impl fmt::Debug for Spool {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Spool").field("name", &self.name).finish_non_exhaustive()
	}
}

/// The components of relative `path`.
fn parts(path: &Path) -> io::Result<Vec<String>> {
	let parts = path
		.components()
		.filter(|c| *c != Component::CurDir)
		.map(|c| match c {
			Component::Normal(part) => part.to_str().map(str::to_owned),
			_ => None,
		})
		.collect::<Option<Vec<_>>>()
		.filter(|parts| !parts.is_empty());
	parts.ok_or_else(|| {
		io::Error::new(io::ErrorKind::InvalidInput, format!("{} is no relative UTF-8 path", path.display()))
	})
}

fn done() -> Token<()> {
	let token = Token::default();
	token.complete(Ok(()));
	token
}

impl Spool {
	/// An empty spool; fails when the browser has no origin private file system or the session's start failed.
	pub fn new() -> io::Result<Self> {
		let session = session().ok_or_else(|| {
			io::Error::new(io::ErrorKind::Unsupported, "this browser has no origin private file system to write to")
		})?;
		let n = SPOOLS.with(|n| n.replace(n.get() + 1));
		Ok(Self {
			session,
			name: format!("export-{n}"),
			queue: Rc::new(RefCell::new(Queue::new(IN_FLIGHT))),
			files: RefCell::default(),
			dirs: RefCell::default(),
			removal: RefCell::new(done()),
			urls: Rc::default(),
		})
	}

	/// The task of the file at `parts`, done at once when there is none.
	fn task(&self, parts: &[String]) -> Token<()> {
		self.files.borrow().get(&parts.join("/")).cloned().unwrap_or_else(done)
	}

	/// Removes the entry at `parts` after the removal queued before and `after`; a missing entry is no error.
	fn remove_later(&self, parts: Vec<String>, after: Token<()>) {
		let done = Token::default();
		let before = self.removal.replace(done.clone());
		let (session, name) = (self.session.clone(), self.name.clone());
		spawn_local(async move {
			before.done().await;
			after.done().await;
			let removed = async {
				let (entry, folders) = parts.split_last().expect("a path has a name");
				let parent = parent(&session, &name, folders, false).await?;
				remove_entry(&parent, entry, false).await
			};
			match removed.await {
				Err(e) if e.kind() != io::ErrorKind::NotFound => {
					tracing::warn!("{} of an export could not be removed: {e}", parts.join("/"));
				}
				_ => {}
			}
			done.complete(Ok(()));
		});
	}

	/// Hands the file at `path` to the browser as a download named `name`, once its task ended; follows
	/// [`OutputFs::poll_flushed`]. Its URL lives as long as the spool.
	pub fn download(&self, path: &Path, name: &str) -> Token<()> {
		let token = Token::default();
		let parts = match parts(path) {
			Ok(parts) => parts,
			Err(e) => {
				token.complete(Err(e));
				return token;
			}
		};
		let (task, session, folder, urls) =
			(self.task(&parts), self.session.clone(), self.name.clone(), self.urls.clone());
		let (done, name) = (token.clone(), name.to_owned());
		spawn_local(async move {
			task.done().await;
			let downloaded = async {
				let blob = blob(&session, &folder, &parts).await?;
				let url = Url::create_object_url_with_blob(&blob).map_err(|e| storage_error(&e))?;
				urls.borrow_mut().push(url.clone());
				crate::platform::web::click_download(&url, &name)
			};
			done.complete(downloaded.await);
			crate::wake::wake();
		});
		token
	}

	/// Opens the file at `path` for reading, once its task ended; follows [`OutputFs::poll_flushed`].
	pub fn reader(&self, path: &Path) -> Token<SpoolReader> {
		let token = Token::default();
		let parts = match parts(path) {
			Ok(parts) => parts,
			Err(e) => {
				token.complete(Err(e));
				return token;
			}
		};
		let (task, session, folder, done) = (self.task(&parts), self.session.clone(), self.name.clone(), token.clone());
		spawn_local(async move {
			task.done().await;
			done.complete(blob(&session, &folder, &parts).await.map(SpoolReader));
			crate::wake::wake();
		});
		token
	}
}

impl Drop for Spool {
	fn drop(&mut self) {
		self.queue.borrow_mut().fail(&io::Error::other("the export's files were removed"));
		for url in self.urls.borrow_mut().drain(..) {
			if let Err(e) = Url::revoke_object_url(&url) {
				tracing::warn!("a download's URL could not be revoked: {}", super::session::describe(&e));
			}
		}
		let mut tasks: Vec<Token<()>> = std::mem::take(&mut *self.files.borrow_mut()).into_values().collect();
		tasks.push(self.removal.borrow().clone());
		let (session, name) = (self.session.clone(), self.name.clone());
		spawn_local(async move {
			for task in tasks {
				task.done().await;
			}
			match remove_entry(&session, &name, true).await {
				Err(e) if e.kind() != io::ErrorKind::NotFound => {
					tracing::warn!("the export's files in {name} could not be removed: {e}");
				}
				_ => {}
			}
		});
	}
}

impl OutputFs for Spool {
	fn create(&self, path: &Path) -> io::Result<Box<dyn OutputFile>> {
		let parts = parts(path)?;
		let id = self.queue.borrow_mut().open()?;
		let task = Token::default();
		let before = self.files.borrow_mut().insert(parts.join("/"), task.clone()).unwrap_or_else(done);
		let after = [before, self.removal.borrow().clone()];
		let (queue, session, name) = (self.queue.clone(), self.session.clone(), self.name.clone());
		spawn_local(async move {
			for t in after {
				t.done().await;
			}
			let result = drain(&queue, id, &session, &name, &parts).await;
			if let Err(e) = &result {
				tracing::debug!("{} of an export failed: {e}", parts.join("/"));
			}
			queue.borrow_mut().finished(id, result.as_ref().err());
			task.complete(result);
			crate::wake::wake();
		});
		let raw = RawFile { queue: self.queue.clone(), id, at: 0, len: 0, closed: false };
		Ok(Box::new(SpoolFile(BufWriter::with_capacity(BUFFER, raw))))
	}

	fn create_dir_all(&self, path: &Path) -> io::Result<bool> {
		let parts = parts(path)?;
		let mut dirs = self.dirs.borrow_mut();
		let mut created = false;
		for end in 1..=parts.len() {
			created = dirs.insert(parts[..end].join("/"));
		}
		Ok(created)
	}

	fn exists(&self, path: &Path) -> bool {
		parts(path).is_ok_and(|parts| {
			let key = parts.join("/");
			self.files.borrow().contains_key(&key) || self.dirs.borrow().contains(&key)
		})
	}

	fn remove_file(&self, path: &Path) -> io::Result<()> {
		let parts = parts(path)?;
		if let Some(task) = self.files.borrow_mut().remove(&parts.join("/")) {
			self.remove_later(parts, task);
		}
		Ok(())
	}

	fn remove_dir(&self, path: &Path) -> io::Result<()> {
		let parts = parts(path)?;
		let key = parts.join("/");
		let inside = format!("{key}/");
		if self.files.borrow().keys().chain(self.dirs.borrow().iter()).any(|k| k.starts_with(&inside)) {
			return Err(io::Error::new(io::ErrorKind::DirectoryNotEmpty, format!("{key} is not empty")));
		}
		if !self.dirs.borrow_mut().remove(&key) {
			return Err(io::Error::new(io::ErrorKind::NotFound, format!("{key} is no folder")));
		}
		self.remove_later(parts, done());
		Ok(())
	}

	fn poll_ready(&self) -> io::Result<Poll<()>> {
		self.queue.borrow().poll_ready()
	}

	fn poll_flushed(&self) -> io::Result<Poll<()>> {
		self.queue.borrow().poll_flushed()
	}
}

/// A file of the spool, read in slices.
pub struct SpoolReader(Blob);

impl SpoolReader {
	/// The file's length in bytes.
	pub fn size(&self) -> u64 {
		self.0.size() as u64
	}

	/// Reads the bytes in `range`, cut to the file's end.
	pub fn read(&self, range: Range<u64>) -> Token<Vec<u8>> {
		let token = Token::default();
		let done = token.clone();
		match self.0.slice_with_f64_and_f64(range.start as f64, range.end as f64) {
			Err(e) => token.complete(Err(storage_error(&e))),
			Ok(slice) => spawn_local(async move {
				let read = JsFuture::from(slice.array_buffer()).await.map_err(|e| storage_error(&e));
				done.complete(read.map(|buffer| Uint8Array::new(&buffer).to_vec()));
				crate::wake::wake();
			}),
		}
		token
	}
}

/// The folder `folders` of the spool `name` in `session`.
async fn parent(
	session: &FileSystemDirectoryHandle,
	name: &str,
	folders: &[String],
	create: bool,
) -> io::Result<FileSystemDirectoryHandle> {
	let mut parent = folder(session, name, create).await?;
	for part in folders {
		parent = folder(&parent, part, create).await?;
	}
	Ok(parent)
}

async fn handle(
	session: &FileSystemDirectoryHandle,
	name: &str,
	parts: &[String],
	create: bool,
) -> io::Result<FileSystemFileHandle> {
	let (file, folders) = parts.split_last().expect("a path has a name");
	let parent = parent(session, name, folders, create).await?;
	let options = FileSystemGetFileOptions::new();
	options.set_create(create);
	awaited(parent.get_file_handle_with_options(file, &options)).await
}

async fn blob(session: &FileSystemDirectoryHandle, name: &str, parts: &[String]) -> io::Result<Blob> {
	awaited(handle(session, name, parts, false).await?.get_file()).await
}

/// Writes file `id` of `queue` at `parts` as its writes come, until its close or an abort.
async fn drain(
	queue: &RefCell<Queue>,
	id: u64,
	session: &FileSystemDirectoryHandle,
	name: &str,
	parts: &[String],
) -> io::Result<()> {
	let stream: FileSystemWritableFileStream =
		awaited(handle(session, name, parts, true).await?.create_writable()).await?;
	let committed = loop {
		match std::future::poll_fn(|cx| queue.borrow_mut().poll_next(id, cx.waker())).await {
			Next::Write { at, data } => {
				let written = write_at(&stream, at, &data).await;
				queue.borrow_mut().written(data.len());
				crate::wake::wake();
				if let Err(e) = written {
					break Err(e);
				}
			}
			Next::Close => break JsFuture::from(stream.close()).await.map(|_| true).map_err(|e| storage_error(&e)),
			Next::Abort => break Ok(false),
		}
	};
	if !matches!(committed, Ok(true)) {
		// Releases the file's lock and drops its swap file; the stream may have errored already.
		let _ = JsFuture::from(stream.abort()).await;
	}
	committed.map(drop)
}

async fn write_at(stream: &FileSystemWritableFileStream, at: u64, data: &[u8]) -> io::Result<()> {
	let params = WriteParams::new(WriteCommandType::Write);
	params.set_position(Some(at as f64));
	params.set_data_opt_u8_array(Some(&Uint8Array::from(data)));
	let written = stream.write_with_write_params(&params).map_err(|e| storage_error(&e))?;
	JsFuture::from(written).await.map_err(|e| storage_error(&e))?;
	Ok(())
}

/// A spool file's writes, queued at their positions.
struct RawFile {
	queue: Rc<RefCell<Queue>>,
	id: u64,
	at: u64,
	len: u64,
	closed: bool,
}

impl Write for RawFile {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.queue.borrow_mut().write(self.id, self.at, buf)?;
		self.at += buf.len() as u64;
		self.len = self.len.max(self.at);
		Ok(buf.len())
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

impl Seek for RawFile {
	fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
		let at = match to {
			SeekFrom::Start(at) => Some(at),
			SeekFrom::Current(by) => self.at.checked_add_signed(by),
			SeekFrom::End(by) => self.len.checked_add_signed(by),
		};
		self.at = at.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a seek before the file's start"))?;
		Ok(self.at)
	}
}

impl Drop for RawFile {
	fn drop(&mut self) {
		if !self.closed {
			self.queue.borrow_mut().discard(self.id);
		}
	}
}

struct SpoolFile(BufWriter<RawFile>);

impl Write for SpoolFile {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.0.write(buf)
	}

	fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
		self.0.write_all(buf)
	}

	fn flush(&mut self) -> io::Result<()> {
		self.0.flush()
	}
}

impl Seek for SpoolFile {
	fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
		self.0.seek(to)
	}
}

impl OutputFile for SpoolFile {
	fn close(self: Box<Self>) -> io::Result<()> {
		let mut raw = self.0.into_inner().map_err(io::IntoInnerError::into_error)?;
		raw.queue.borrow_mut().close(raw.id)?;
		raw.closed = true;
		Ok(())
	}
}
