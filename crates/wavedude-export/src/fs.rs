//! Where an export's files go: the platform's file system ([`StdFs`]), or a store the caller provides (the
//! browser's). A file's errors surface when it is closed, never in a drop.

use std::fmt::Debug;
use std::fs::File;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wavedude_core::sync::{MaybeSend, MaybeSync, lock};

/// A file being written.
pub trait OutputFile: Write + Seek + MaybeSend {
	/// Completes the file: what is buffered is written; its failure is the write's.
	fn close(self: Box<Self>) -> io::Result<()>;
}

/// A store of files under folders, addressed by relative or absolute paths.
pub trait OutputFs: Debug + MaybeSend + MaybeSync {
	/// Creates the file at `path`, replacing one there.
	fn create(&self, path: &Path) -> io::Result<Box<dyn OutputFile>>;

	/// Creates the folder `path` and its parents; whether `path` was created.
	fn create_dir_all(&self, path: &Path) -> io::Result<bool>;

	fn exists(&self, path: &Path) -> bool;

	/// Removes the file at `path`; none there is no error.
	fn remove_file(&self, path: &Path) -> io::Result<()>;

	/// Removes the empty folder `path`.
	fn remove_dir(&self, path: &Path) -> io::Result<()>;
}

/// The platform's file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFs;

/// Writes go through a buffer of this many bytes.
const BUFFER: usize = 1 << 20;

impl OutputFile for BufWriter<File> {
	fn close(self: Box<Self>) -> io::Result<()> {
		self.into_inner().map_err(io::IntoInnerError::into_error)?;
		Ok(())
	}
}

impl OutputFs for StdFs {
	fn create(&self, path: &Path) -> io::Result<Box<dyn OutputFile>> {
		Ok(Box::new(BufWriter::with_capacity(BUFFER, File::create(path)?)))
	}

	fn create_dir_all(&self, path: &Path) -> io::Result<bool> {
		let existed = path.is_dir();
		std::fs::create_dir_all(path)?;
		Ok(!existed)
	}

	fn exists(&self, path: &Path) -> bool {
		path.exists()
	}

	fn remove_file(&self, path: &Path) -> io::Result<()> {
		match std::fs::remove_file(path) {
			Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
			_ => Ok(()),
		}
	}

	fn remove_dir(&self, path: &Path) -> io::Result<()> {
		std::fs::remove_dir(path)
	}
}

/// The file at `path` of `fs`, created at the first write or seek and shared by the writer that fills it and the
/// output that closes it once the writer finished.
#[derive(Clone)]
pub(crate) struct SharedFile(Arc<Mutex<Slot>>);

struct Slot {
	fs: Arc<dyn OutputFs>,
	path: PathBuf,
	file: Option<Box<dyn OutputFile>>,
	closed: bool,
}

impl SharedFile {
	#[cfg_attr(
		target_arch = "wasm32",
		allow(clippy::arc_with_non_send_sync, reason = "the browser's files are not Send there")
	)]
	pub(crate) fn new(fs: Arc<dyn OutputFs>, path: PathBuf) -> Self {
		Self(Arc::new(Mutex::new(Slot { fs, path, file: None, closed: false })))
	}

	/// Whether the file was created.
	pub(crate) fn opened(&self) -> bool {
		let slot = lock(&self.0);
		slot.file.is_some() || slot.closed
	}

	fn with<T>(&self, f: impl FnOnce(&mut dyn OutputFile) -> io::Result<T>) -> io::Result<T> {
		let mut slot = lock(&self.0);
		if slot.closed {
			return Err(io::Error::other("the file is closed"));
		}
		if slot.file.is_none() {
			slot.file = Some(slot.fs.create(&slot.path)?);
		}
		f(slot.file.as_deref_mut().expect("created above"))
	}

	/// Closes the file when it was created; later calls do nothing.
	pub(crate) fn close(&self) -> io::Result<()> {
		let mut slot = lock(&self.0);
		slot.closed = true;
		slot.file.take().map_or(Ok(()), OutputFile::close)
	}
}

impl Write for SharedFile {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.with(|f| f.write(buf))
	}

	fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
		self.with(|f| f.write_all(buf))
	}

	fn flush(&mut self) -> io::Result<()> {
		self.with(|f| f.flush())
	}
}

impl Seek for SharedFile {
	fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
		self.with(|f| f.seek(to))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_shared_file_is_created_at_its_first_write_and_closed_once() {
		let dir = std::env::temp_dir().join(format!("wavedude-shared-file-{}", std::process::id()));
		let fs = Arc::new(StdFs);
		fs.create_dir_all(&dir).unwrap();
		let path = dir.join("a.bin");
		let mut file = SharedFile::new(fs.clone(), path.clone());
		assert!(!file.opened() && !path.exists(), "nothing written, no file");
		file.write_all(b"abcd").unwrap();
		file.seek(SeekFrom::Start(1)).unwrap();
		file.write_all(b"X").unwrap();
		file.clone().close().unwrap();
		file.close().unwrap();
		assert!(file.write_all(b"late").is_err(), "a closed file takes no more");
		assert_eq!(std::fs::read(&path).unwrap(), b"aXcd");
		std::fs::remove_dir_all(&dir).unwrap();
	}
}
