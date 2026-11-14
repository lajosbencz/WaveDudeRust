//! A ZIP archive as an [`OutputFs`]: each file an entry, stored uncompressed. A file is held in memory, as its
//! writer may seek, and added whole when closed, so files written at once on several threads do not interleave;
//! [`ZipFs::write_entry`] streams an entry from chunks that come later instead. Sizes go in data descriptors, so
//! the archive's file is only appended to. ZIP64 records are written where the sizes or the count of entries need them. The archive is
//! dropped whole on failure, so removing a file or a folder does nothing.

use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Cursor, Seek, SeekFrom, Write};
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};
use std::task::Poll;

use rawzip::ZipArchiveWriter;
use rawzip::path::ZipFilePath;
pub use rawzip::time::UtcDateTime;
use wavedude_core::sync::lock;

use super::{OutputFile, OutputFs};

struct Archive {
	/// None once finished or failed.
	writer: Option<Writer>,
	names: BTreeSet<String>,
}

type Writer = ZipArchiveWriter<Box<dyn OutputFile>>;

impl Archive {
	/// Adds the entry `name`, filled by `write`; a failure leaves the archive failed, as the entry is partly written.
	fn add<T>(
		&mut self,
		name: String,
		time: UtcDateTime,
		write: impl FnOnce(&mut dyn Write) -> io::Result<T>,
	) -> io::Result<T> {
		// out of the archive while the entry is written, so a failed or panicking entry leaves none to finish
		let mut writer = self.writer.take().ok_or_else(finished)?;
		let value = add_stored(&mut writer, &name, time, write)?;
		self.writer = Some(writer);
		self.names.insert(name);
		Ok(value)
	}
}

fn add_stored<T>(
	writer: &mut Writer,
	name: &str,
	time: UtcDateTime,
	write: impl FnOnce(&mut dyn Write) -> io::Result<T>,
) -> io::Result<T> {
	let (mut entry, config) = writer.new_file(name).last_modified(time).start().map_err(io::Error::other)?;
	let mut data = config.wrap(&mut entry);
	let value = write(&mut data)?;
	let (_, descriptor) = data.finish().map_err(io::Error::other)?;
	entry.finish(descriptor).map_err(io::Error::other)?;
	Ok(value)
}

/// An [`OutputFs`] writing a ZIP archive to one file; [`finish`](Self::finish) completes it.
pub struct ZipFs {
	archive: Arc<Mutex<Archive>>,
	time: UtcDateTime,
	/// The store of the archive's file, whose readiness and flushing are the archive's.
	store: Option<Arc<dyn OutputFs>>,
}

impl fmt::Debug for ZipFs {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ZipFs").field("time", &self.time).finish_non_exhaustive()
	}
}

impl ZipFs {
	/// An empty archive written to `file`, every entry modified at `time`.
	#[cfg_attr(
		target_arch = "wasm32",
		allow(clippy::arc_with_non_send_sync, reason = "the browser's files are not Send there")
	)]
	pub fn new(file: Box<dyn OutputFile>, time: UtcDateTime) -> Self {
		let archive = Archive { writer: Some(ZipArchiveWriter::new(file)), names: BTreeSet::new() };
		Self { archive: Arc::new(Mutex::new(archive)), time, store: None }
	}

	/// An empty archive written to the file at `path` of `store`, every entry modified at `time`; the archive takes
	/// writes and is flushed as `store` is.
	pub fn in_store(store: Arc<dyn OutputFs>, path: &Path, time: UtcDateTime) -> io::Result<Self> {
		let file = store.create(path)?;
		Ok(Self { store: Some(store), ..Self::new(file, time) })
	}

	/// Adds the file at `path`, its bytes the chunks `next` gives until `None`, each written through to the archive.
	/// The archive is out of reach until the entry ends: files closed meanwhile fail. A failure, or dropping the
	/// future, leaves the archive failed.
	pub async fn write_entry(
		&mut self,
		path: &Path,
		mut next: impl AsyncFnMut() -> io::Result<Option<Vec<u8>>>,
	) -> io::Result<()> {
		let name = entry_name(path)?;
		let mut writer = lock(&self.archive).writer.take().ok_or_else(finished)?;
		let (mut entry, config) = writer.new_file(&name).last_modified(self.time).start().map_err(io::Error::other)?;
		let mut data = config.wrap(&mut entry);
		while let Some(chunk) = next().await? {
			data.write_all(&chunk)?;
		}
		let (_, descriptor) = data.finish().map_err(io::Error::other)?;
		entry.finish(descriptor).map_err(io::Error::other)?;
		let mut archive = lock(&self.archive);
		archive.writer = Some(writer);
		archive.names.insert(name);
		Ok(())
	}

	/// Writes the central directory and closes the archive's file; files closed later fail.
	pub fn finish(&self) -> io::Result<()> {
		let writer = lock(&self.archive).writer.take().ok_or_else(finished)?;
		writer.finish().map_err(io::Error::other)?.close()
	}
}

fn finished() -> io::Error {
	io::Error::other("the archive is finished or failed")
}

/// The entry name of `path`: its components joined by `/`, kept as they are by the archive.
fn entry_name(path: &Path) -> io::Result<String> {
	let parts = path
		.components()
		.filter(|c| *c != Component::CurDir)
		.map(|c| match c {
			Component::Normal(part) => part.to_str(),
			_ => None,
		})
		.collect::<Option<Vec<_>>>()
		.filter(|parts| !parts.is_empty());
	parts.map(|parts| parts.join("/")).filter(|name| ZipFilePath::from_str(name).as_str() == name).ok_or_else(|| {
		io::Error::new(
			io::ErrorKind::InvalidInput,
			format!("{} is no relative UTF-8 path without `\\` or `:`", path.display()),
		)
	})
}

struct Entry {
	archive: Arc<Mutex<Archive>>,
	name: String,
	time: UtcDateTime,
	data: Cursor<Vec<u8>>,
}

impl Write for Entry {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.data.write(buf)
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

impl Seek for Entry {
	fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
		self.data.seek(to)
	}
}

impl OutputFile for Entry {
	fn close(self: Box<Self>) -> io::Result<()> {
		let data = self.data.into_inner();
		lock(&self.archive).add(self.name, self.time, |out| out.write_all(&data))
	}
}

impl OutputFs for ZipFs {
	fn create(&self, path: &Path) -> io::Result<Box<dyn OutputFile>> {
		let name = entry_name(path)?;
		Ok(Box::new(Entry { archive: self.archive.clone(), name, time: self.time, data: Cursor::default() }))
	}

	/// Folders are implied by the entries' names: none is created.
	fn create_dir_all(&self, _path: &Path) -> io::Result<bool> {
		Ok(false)
	}

	fn exists(&self, path: &Path) -> bool {
		let Ok(name) = entry_name(path) else { return false };
		let folder = format!("{name}/");
		let names = &lock(&self.archive).names;
		names.contains(&name) || names.range(folder.clone()..).next().is_some_and(|n| n.starts_with(&folder))
	}

	fn remove_file(&self, _path: &Path) -> io::Result<()> {
		Ok(())
	}

	fn remove_dir(&self, _path: &Path) -> io::Result<()> {
		Ok(())
	}

	fn poll_ready(&self) -> io::Result<Poll<()>> {
		self.store.as_ref().map_or(Ok(Poll::Ready(())), |s| s.poll_ready())
	}

	fn poll_flushed(&self) -> io::Result<Poll<()>> {
		self.store.as_ref().map_or(Ok(Poll::Ready(())), |s| s.poll_flushed())
	}
}

#[cfg(test)]
mod tests {
	use std::io::Read;
	use std::sync::atomic::{AtomicU64, Ordering};
	use std::task::{Context, Waker};

	use ::zip::CompressionMethod;
	use half::f16;
	use wavedude_core::engine::types::{FrameShape, SampleFormat};

	use super::*;
	use crate::fs::MemFs;
	use crate::{ExportFormat, ExportLayers, ExportPlan, ExportState, MemoryFrames};

	const W: u32 = 8;
	const H: u32 = 6;

	fn time() -> UtcDateTime {
		UtcDateTime::from_components(2026, 11, 14, 12, 30, 10, 0).unwrap()
	}

	/// An archive written to `out` of a [`MemFs`].
	fn archive(mem: &MemFs, out: &Path) -> Arc<ZipFs> {
		Arc::new(ZipFs::new(mem.create(out).unwrap(), time()))
	}

	/// Every entry of the archive in `bytes`: name, CRC-32, contents, in archive order.
	fn entries(bytes: Vec<u8>) -> Vec<(String, u32, Vec<u8>)> {
		let mut zip = ::zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
		(0..zip.len())
			.map(|i| {
				let mut entry = zip.by_index(i).unwrap();
				assert_eq!(entry.compression(), CompressionMethod::Stored);
				assert_eq!(entry.last_modified(), ::zip::DateTime::from_date_and_time(2026, 11, 14, 12, 30, 10).ok());
				let mut data = Vec::new();
				entry.read_to_end(&mut data).unwrap();
				(entry.name().unwrap().into_owned(), entry.crc32(), data)
			})
			.collect()
	}

	fn source(count: usize) -> MemoryFrames {
		let shape = FrameShape {
			canvas_width: W,
			canvas_height: H,
			divisor: 1,
			format: SampleFormat::Half,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: true,
		};
		let frames = (0..count)
			.map(|k| {
				let field: Vec<f16> =
					(0..W * H).map(|i| f16::from_f32(((i + k as u32) as f32 * 0.3).sin() * 0.04)).collect();
				let intensity = field.iter().map(|v| f16::from_f32(v.to_f32().abs().sqrt())).collect();
				(field, intensity)
			})
			.collect();
		MemoryFrames { shape, frames }
	}

	/// Exports both layers of five frames as PNG sequences under `run/` of `fs`.
	fn export_sequences(fs: Arc<dyn OutputFs>) {
		let mut project = wavedude_core::model::Project::default();
		project.scene.width = W;
		project.scene.height = H;
		let plan = ExportPlan {
			project: &project,
			format: ExportFormat::Png,
			layers: ExportLayers::Both,
			fs,
			path: "run/run.png".into(),
			still: None,
			frame_count: 5,
			traces: &[],
			total_steps: 10,
			keep_partial: false,
			h264: None,
			h264_profile: Default::default(),
		};
		let job = plan.build_job(Arc::new(source(5)), &wavedude_core::io::schemes::SchemeStore::default()).unwrap();
		let report = job.run(|_, _| {}, &std::sync::atomic::AtomicBool::new(false));
		assert!(matches!(report.state, ExportState::Completed), "{:?}", report.state);
	}

	#[test]
	fn both_layers_as_png_sequences_go_into_one_archive() {
		let mem = MemFs::new();
		let zip = archive(&mem, Path::new("run.zip"));
		export_sequences(zip.clone());
		zip.finish().unwrap();
		let loose = Arc::new(MemFs::new());
		export_sequences(loose.clone());
		let mut entries = entries(mem.take(Path::new("run.zip")).unwrap());
		entries.sort();
		let expected: Vec<_> = ["field", "intensity"]
			.iter()
			.flat_map(|l| (0..5).map(move |f| format!("run/run_{l}/run_{l}_{f:04}.png")))
			.collect();
		assert_eq!(entries.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), expected);
		for (name, crc, data) in entries {
			assert_eq!(crc, crc32(&data), "{name}");
			assert_eq!(Some(data), loose.take(Path::new(&name)), "{name} holds the file's bytes");
		}
	}

	/// The CRC-32 (IEEE) of `data`.
	fn crc32(data: &[u8]) -> u32 {
		!data.iter().fold(!0u32, |crc, &b| {
			(0..8).fold(crc ^ u32::from(b), |c, _| if c & 1 == 1 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 })
		})
	}

	#[test]
	fn files_closed_on_many_threads_become_whole_entries() {
		let mem = MemFs::new();
		let zip = archive(&mem, Path::new("a.zip"));
		std::thread::scope(|scope| {
			for t in 0..8u8 {
				let zip = zip.clone();
				scope.spawn(move || {
					for k in 0..16u8 {
						let mut file = zip.create(Path::new(&format!("t{t}/f{k}.bin"))).unwrap();
						for _ in 0..64 {
							file.write_all(&[t, k, 0, 0]).unwrap();
						}
						file.seek(SeekFrom::Start(2)).unwrap();
						file.write_all(&[0xAA]).unwrap();
						file.close().unwrap();
					}
				});
			}
		});
		assert!(zip.exists(Path::new("t3/f7.bin")) && zip.exists(Path::new("t3")) && !zip.exists(Path::new("t")));
		zip.finish().unwrap();
		assert!(zip.create(Path::new("late")).unwrap().close().is_err(), "a finished archive takes no more");
		let entries = entries(mem.take(Path::new("a.zip")).unwrap());
		assert_eq!(entries.len(), 8 * 16);
		for (name, _, data) in entries {
			let (t, k) = name.trim_start_matches('t').trim_end_matches(".bin").split_once("/f").unwrap();
			let (t, k): (u8, u8) = (t.parse().unwrap(), k.parse().unwrap());
			let mut expected: Vec<u8> = (0..64).flat_map(|_| [t, k, 0, 0]).collect();
			expected[2] = 0xAA;
			assert_eq!(data, expected, "{name}");
		}
	}

	#[test]
	fn paths_out_of_the_archive_are_refused() {
		let zip = archive(&MemFs::new(), Path::new("a.zip"));
		for path in ["/abs", "../up", "", ".", "a:b", "a\\b"] {
			assert!(zip.create(Path::new(path)).is_err(), "{path}");
		}
		assert!(zip.create(Path::new("./a/b")).is_ok());
	}

	#[test]
	#[ignore = "writes 70000 entries; cargo test -p wavedude-export --lib -- --ignored zip64"]
	fn more_entries_than_a_plain_archive_holds_take_zip64() {
		let mem = MemFs::new();
		let zip = archive(&mem, Path::new("many.zip"));
		let count = 70_000u32;
		for k in 0..count {
			let mut file = zip.create(Path::new(&format!("f/{k}"))).unwrap();
			file.write_all(&k.to_le_bytes()).unwrap();
			file.close().unwrap();
		}
		zip.finish().unwrap();
		let mut read = ::zip::ZipArchive::new(Cursor::new(mem.take(Path::new("many.zip")).unwrap())).unwrap();
		assert_eq!(read.len(), count as usize);
		let mut last = Vec::new();
		read.by_name(&format!("f/{}", count - 1)).unwrap().read_to_end(&mut last).unwrap();
		assert_eq!(last, (count - 1).to_le_bytes());
	}

	/// An archive's file counting the bytes it was given.
	struct Tap {
		file: Box<dyn OutputFile>,
		written: Arc<AtomicU64>,
	}

	impl Write for Tap {
		fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
			let n = self.file.write(buf)?;
			self.written.fetch_add(n as u64, Ordering::Relaxed);
			Ok(n)
		}

		fn flush(&mut self) -> io::Result<()> {
			self.file.flush()
		}
	}

	impl Seek for Tap {
		fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
			self.file.seek(to)
		}
	}

	impl OutputFile for Tap {
		fn close(self: Box<Self>) -> io::Result<()> {
			self.file.close()
		}
	}

	/// The outcome of `future`, which must not wait.
	fn now<T>(future: impl Future<Output = T>) -> T {
		match std::pin::pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
			Poll::Ready(value) => value,
			Poll::Pending => panic!("the future waited"),
		}
	}

	#[test]
	fn a_streamed_entry_reaches_the_archive_as_it_is_written() {
		let mem = MemFs::new();
		let written = Arc::new(AtomicU64::default());
		let tap = Tap { file: mem.create(Path::new("big.zip")).unwrap(), written: written.clone() };
		let mut zip = ZipFs::new(Box::new(tap), time());
		zip.create(Path::new("small")).unwrap().close().unwrap();
		let data: Vec<u8> = (0..4u32 << 20).map(|i| (i % 251) as u8).collect();
		let mut chunks = data.chunks(1 << 16);
		let mut given = 0u64;
		now(zip.write_entry(Path::new("v/big.bin"), async || {
			assert!(written.load(Ordering::Relaxed) >= given, "written through, not held");
			let chunk = chunks.next().map(<[u8]>::to_vec);
			given += chunk.as_ref().map_or(0, |c| c.len() as u64);
			Ok(chunk)
		}))
		.unwrap();
		assert!(zip.exists(Path::new("v/big.bin")));
		assert!(now(zip.write_entry(Path::new("/abs"), async || Ok(None))).is_err());
		zip.finish().unwrap();
		let entries = entries(mem.take(Path::new("big.zip")).unwrap());
		assert_eq!(entries.len(), 2);
		let (name, crc, read) = &entries[1];
		assert_eq!((name.as_str(), *crc), ("v/big.bin", crc32(&data)));
		assert!(*read == data, "the entry holds the streamed bytes");
	}

	#[test]
	fn a_failed_entry_fails_the_archive() {
		let mem = MemFs::new();
		let mut zip = ZipFs::new(mem.create(Path::new("a.zip")).unwrap(), time());
		let mut chunks = [Ok(b"part".to_vec()), Err(io::Error::other("source gone"))].into_iter();
		assert!(now(zip.write_entry(Path::new("half"), async || chunks.next().transpose())).is_err());
		assert!(zip.create(Path::new("next")).unwrap().close().is_err() && zip.finish().is_err());
	}

	/// A [`MemFs`] that takes writes while `ready` is set.
	#[derive(Debug, Default)]
	struct Gated {
		mem: MemFs,
		ready: std::sync::atomic::AtomicBool,
	}

	impl OutputFs for Gated {
		fn create(&self, path: &Path) -> io::Result<Box<dyn OutputFile>> {
			self.mem.create(path)
		}

		fn create_dir_all(&self, path: &Path) -> io::Result<bool> {
			self.mem.create_dir_all(path)
		}

		fn exists(&self, path: &Path) -> bool {
			self.mem.exists(path)
		}

		fn remove_file(&self, path: &Path) -> io::Result<()> {
			self.mem.remove_file(path)
		}

		fn remove_dir(&self, path: &Path) -> io::Result<()> {
			self.mem.remove_dir(path)
		}

		fn poll_ready(&self) -> io::Result<Poll<()>> {
			Ok(if self.ready.load(Ordering::Relaxed) { Poll::Ready(()) } else { Poll::Pending })
		}
	}

	#[test]
	fn an_archive_in_a_store_takes_writes_as_the_store_does() {
		let store = Arc::new(Gated::default());
		let zip = ZipFs::in_store(store.clone(), Path::new("a.zip"), time()).unwrap();
		assert_eq!(zip.poll_ready().unwrap(), Poll::Pending);
		store.ready.store(true, Ordering::Relaxed);
		assert_eq!(zip.poll_ready().unwrap(), Poll::Ready(()));
		zip.finish().unwrap();
		assert!(store.mem.take(Path::new("a.zip")).is_some(), "the archive is a file of the store");
	}
}
