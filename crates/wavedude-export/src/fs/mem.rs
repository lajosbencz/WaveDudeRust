//! Files held in memory: a still or a CSV handed on as bytes. Folders are implied by the files' paths.

use std::collections::BTreeMap;
use std::io::{self, Cursor, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wavedude_core::sync::lock;

use super::{OutputFile, OutputFs};

type Files = Arc<Mutex<BTreeMap<PathBuf, Vec<u8>>>>;

/// An [`OutputFs`] in memory; a file is there once closed.
#[derive(Debug, Default)]
pub struct MemFs {
	files: Files,
}

impl MemFs {
	pub fn new() -> Self {
		Self::default()
	}

	/// Takes the bytes of the file at `path` out of the store.
	pub fn take(&self, path: &Path) -> Option<Vec<u8>> {
		lock(&self.files).remove(path)
	}

	/// The paths of the files held, in order.
	pub fn paths(&self) -> Vec<PathBuf> {
		lock(&self.files).keys().cloned().collect()
	}
}

struct MemFile {
	files: Files,
	path: PathBuf,
	data: Cursor<Vec<u8>>,
}

impl Write for MemFile {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.data.write(buf)
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

impl Seek for MemFile {
	fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
		self.data.seek(to)
	}
}

impl OutputFile for MemFile {
	fn close(self: Box<Self>) -> io::Result<()> {
		lock(&self.files).insert(self.path, self.data.into_inner());
		Ok(())
	}
}

impl OutputFs for MemFs {
	fn create(&self, path: &Path) -> io::Result<Box<dyn OutputFile>> {
		Ok(Box::new(MemFile { files: self.files.clone(), path: path.to_path_buf(), data: Cursor::default() }))
	}

	fn create_dir_all(&self, path: &Path) -> io::Result<bool> {
		Ok(!self.exists(path))
	}

	fn exists(&self, path: &Path) -> bool {
		lock(&self.files).keys().any(|p| p.starts_with(path))
	}

	fn remove_file(&self, path: &Path) -> io::Result<()> {
		lock(&self.files).remove(path);
		Ok(())
	}

	fn remove_dir(&self, path: &Path) -> io::Result<()> {
		if lock(&self.files).keys().any(|p| p != path && p.starts_with(path)) {
			return Err(io::Error::new(io::ErrorKind::DirectoryNotEmpty, format!("{} is not empty", path.display())));
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ExportFormat;
	use crate::sinks::{self, EncoderPool, SinkSpec};

	#[test]
	fn a_still_written_to_memory_decodes() {
		let fs = Arc::new(MemFs::new());
		let path = Path::new("out/still.png");
		let spec = SinkSpec { width: 3, height: 2, fps: 30, quality: 90, first_frame: 0, frame_count: 1 };
		let dyn_fs: Arc<dyn OutputFs> = fs.clone();
		let mut sink =
			sinks::open(ExportFormat::Png, &dyn_fs, path, spec, true, &EncoderPool::default(), None).unwrap();
		let rgba: Vec<u8> = (0..6u8).flat_map(|i| [i * 40, 255 - i * 40, i, 255]).collect();
		sink.write_frame(&rgba, 3, 2).unwrap();
		sink.finish().unwrap();
		assert_eq!(fs.paths(), [path]);
		let png = fs.take(path).unwrap();
		let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
		let mut rgb = vec![0; reader.output_buffer_size().unwrap()];
		let info = reader.next_frame(&mut rgb).unwrap();
		assert_eq!((info.width, info.height, info.color_type), (3, 2, png::ColorType::Rgb));
		assert_eq!(rgb, rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect::<Vec<_>>());
		assert!(fs.take(path).is_none(), "taken once");
	}
}
