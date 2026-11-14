//! A ZIP archive as an [`OutputFs`]: each file an entry, stored uncompressed, added whole when closed, so files
//! written at once on several threads do not interleave. Written as a stream (sizes in data descriptors), so the
//! archive's file is only appended to. ZIP64 records are written where the sizes or the count of
//! entries need them. The archive is dropped whole on failure, so removing a file or a folder does nothing.

use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Cursor, Seek, SeekFrom, Write};
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};

pub use ::zip::DateTime;
use ::zip::write::{SimpleFileOptions, StreamWriter};
use ::zip::{CompressionMethod, ZipWriter};
use wavedude_core::sync::lock;

use super::{OutputFile, OutputFs};

struct Archive {
	/// None once finished.
	writer: Option<ZipWriter<StreamWriter<Box<dyn OutputFile>>>>,
	names: BTreeSet<String>,
}

/// An [`OutputFs`] writing a ZIP archive to one file; [`finish`](Self::finish) completes it.
pub struct ZipFs {
	archive: Arc<Mutex<Archive>>,
	time: DateTime,
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
	pub fn new(file: Box<dyn OutputFile>, time: DateTime) -> Self {
		let archive = Archive { writer: Some(ZipWriter::new_stream(file)), names: BTreeSet::new() };
		Self { archive: Arc::new(Mutex::new(archive)), time }
	}

	/// Writes the central directory and closes the archive's file; files closed later fail.
	pub fn finish(&self) -> io::Result<()> {
		let writer = lock(&self.archive).writer.take().ok_or_else(finished)?;
		writer.finish()?.into_inner().close()
	}
}

fn finished() -> io::Error {
	io::Error::other("the archive is finished")
}

/// The entry name of `path`: its components joined by `/`.
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
	let parts = parts.ok_or_else(|| {
		io::Error::new(io::ErrorKind::InvalidInput, format!("{} is no relative UTF-8 path", path.display()))
	})?;
	Ok(parts.join("/"))
}

struct Entry {
	archive: Arc<Mutex<Archive>>,
	name: String,
	time: DateTime,
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
		let options = SimpleFileOptions::default()
			.compression_method(CompressionMethod::Stored)
			.last_modified_time(self.time)
			.large_file(data.len() as u64 >= u64::from(u32::MAX));
		let mut archive = lock(&self.archive);
		let writer = archive.writer.as_mut().ok_or_else(finished)?;
		writer.start_file(self.name.as_str(), options)?;
		writer.write_all(&data)?;
		archive.names.insert(self.name);
		Ok(())
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
}

#[cfg(test)]
mod tests {
	use std::io::Read;

	use half::f16;
	use wavedude_core::engine::types::{FrameShape, SampleFormat};

	use super::*;
	use crate::fs::MemFs;
	use crate::{ExportFormat, ExportLayers, ExportPlan, ExportState, MemoryFrames};

	const W: u32 = 8;
	const H: u32 = 6;

	fn time() -> DateTime {
		DateTime::from_date_and_time(2026, 11, 14, 12, 30, 10).unwrap()
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
				assert_eq!(entry.last_modified(), Some(time()));
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
		for path in ["/abs", "../up", "", "."] {
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
}
