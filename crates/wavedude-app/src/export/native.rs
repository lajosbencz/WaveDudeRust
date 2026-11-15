//! The desktop's export: the job blocks a worker thread, writing to the file system or into a ZIP archive; what it
//! would replace.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wavedude_core::model::Layer;
use wavedude_export::fs::{OutputFs, StdFs, ZipFs};
use wavedude_export::h264::H264Factory;
use wavedude_export::{ExportFormat, ExportState, h264, naming};

use super::{Outcome, Plan, Work, now};
use crate::App;
use crate::menus::ExportKindChoice;

/// Bytes of a spooled video read and written into the archive at a time.
const CHUNK: u64 = 4 << 20;

/// What an export would replace, one question each: its archive, or a layer's sequence folder holding files, or its
/// existing video or still; empty when nothing is in the way or the export repeats the last one (which replaces on
/// purpose).
pub fn would_replace(plan: &Plan) -> Vec<(&'static str, PathBuf)> {
	let Plan::Frames { format, outputs, archive, repeat: false, choice, .. } = plan else { return Vec::new() };
	if let Some(archive) = archive {
		return archive.is_file().then(|| ("ExportFileExistsFormat", archive.clone())).into_iter().collect();
	}
	let outputs = outputs.iter().map(|(_, path)| path);
	if !choice.is_still() && !format.is_video() {
		outputs
			.map(|p| naming::sequence_folder(p))
			.filter(|f| std::fs::read_dir(f).is_ok_and(|mut d| d.next().is_some()))
			.map(|f| ("ExportFolderExistsFormat", f))
			.collect()
	} else {
		outputs.filter(|p| p.is_file()).map(|p| ("ExportFileExistsFormat", p.clone())).collect()
	}
}

/// The export downloads Cisco's `OpenH264` before it starts.
pub fn fetches_codec(plan: &Plan, work: &Work, app: &App) -> bool {
	matches!(plan, Plan::Frames { format: ExportFormat::Mp4, .. })
		&& work.use_openh264
		&& !h264::BUILT_IN
		&& !crate::codec::downloaded(app)
}

/// Runs `plan` on the calling thread, reporting `(done, total)` frames; `cancel` stops it.
pub fn run(work: &Work, plan: Plan, mut progress: impl FnMut(usize, usize), cancel: &AtomicBool) -> Outcome {
	match plan {
		Plan::Csv { probe, path } => write_csv(work, &probe, &path).map_or_else(Outcome::Failed, Outcome::Completed),
		Plan::Frames { choice, format, outputs, archive, frame, .. } => {
			let frames = Frames { work, choice, format, frame, cancel };
			let written = match &archive {
				None => frames.to_files(outputs, &mut progress),
				Some(archive) => frames.to_archive(outputs, archive, &mut progress),
			};
			written.map_or_else(|e| e, Outcome::Completed)
		}
	}
}

fn write_csv(work: &Work, probe: &str, path: &Path) -> Result<String, String> {
	let mut bytes = Vec::new();
	let written = work.probe_csv(probe, &mut bytes)?;
	wavedude_core::io::write_atomic(path, &bytes).map_err(|e| e.to_string())?;
	Ok(format!("{written} written to {}", path.display()))
}

/// An export of frames, its outputs to come.
struct Frames<'a> {
	work: &'a Work,
	choice: ExportKindChoice,
	format: ExportFormat,
	frame: usize,
	cancel: &'a AtomicBool,
}

impl Frames<'_> {
	/// Exports to `outputs`, paths; the log line once completed, else how it ended.
	fn to_files(&self, outputs: Vec<(Layer, PathBuf)>, progress: impl FnMut(usize, usize)) -> Result<String, Outcome> {
		let shown: Vec<String> = outputs.iter().map(|(_, path)| path.display().to_string()).collect();
		let frames = self.job(outputs, Arc::new(StdFs), progress)?;
		Ok(format!("{frames} frames exported to {}", shown.join(", ")))
	}

	/// Exports into the ZIP archive `archive`, `outputs` the names in it. The archive is written beside it under a
	/// temporary name and takes its place once complete; image files go straight into it, videos into a temporary
	/// folder beside it first, packed afterwards.
	fn to_archive(
		&self,
		outputs: Vec<(Layer, PathBuf)>,
		archive: &Path,
		progress: impl FnMut(usize, usize),
	) -> Result<String, Outcome> {
		let dir = archive.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
		let mut builder = tempfile::Builder::new();
		builder.prefix(".wavedude-").suffix(".zip");
		#[cfg(unix)]
		builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
		let file = builder.tempfile_in(dir).map_err(failed)?;
		let open = || StdFs.create(file.path()).map(|f| ZipFs::new(f, now())).map_err(failed);
		let frames = if self.format.is_video() {
			let spool = tempfile::Builder::new().prefix(".wavedude-").tempdir_in(dir).map_err(failed)?;
			let spooled = outputs.iter().map(|(layer, name)| (*layer, spool.path().join(name))).collect();
			let frames = self.job(spooled, Arc::new(StdFs), progress)?;
			let mut zip = open()?;
			for (_, name) in &outputs {
				pack(&mut zip, &spool.path().join(name), name, self.cancel).map_err(|e| self.ended(frames, &e))?;
			}
			zip.finish().map_err(failed)?;
			frames
		} else {
			let zip = Arc::new(open()?);
			let frames = self.job(outputs, zip.clone(), progress)?;
			zip.finish().map_err(failed)?;
			frames
		};
		file.persist(archive).map_err(|e| failed(e.error))?;
		Ok(format!("{frames} frames exported to {}", archive.display()))
	}

	/// Runs the job writing `outputs` of `fs`; the frames written once completed, else how it ended.
	fn job(
		&self,
		outputs: Vec<(Layer, PathBuf)>,
		fs: Arc<dyn OutputFs>,
		mut progress: impl FnMut(usize, usize),
	) -> Result<usize, Outcome> {
		let work = self.work;
		let mut plan = work.export_plan(self.choice, self.format, outputs, self.frame, fs);
		if self.format == ExportFormat::Mp4 {
			let library = codec(work).map_err(Outcome::Failed)?;
			plan.h264 = Some(Arc::new(h264::OpenH264Factory::new(library)) as Arc<dyn H264Factory>);
		}
		let job = plan.build_job(work.store.clone(), &work.schemes).map_err(failed)?;
		let report = job.run(&mut progress, self.cancel);
		match report.state {
			ExportState::Completed => Ok(report.frames_done),
			ExportState::Cancelled => Err(Outcome::cancelled(report.frames_done)),
			ExportState::Failed(e) => Err(failed(e)),
		}
	}

	/// How an export that wrote `frames` frames ended on `error`: cancelled when it was asked to.
	fn ended(&self, frames: usize, error: &io::Error) -> Outcome {
		if self.cancel.load(Ordering::Relaxed) { Outcome::cancelled(frames) } else { failed(error) }
	}
}

/// Copies the file at `path` into `zip` as `name`, chunk by chunk; `cancel` stops it.
fn pack(zip: &mut ZipFs, path: &Path, name: &Path, cancel: &AtomicBool) -> io::Result<()> {
	let mut file = File::open(path)?;
	zip.write_entry_sync(name, || {
		if cancel.load(Ordering::Relaxed) {
			return Err(io::Error::new(io::ErrorKind::Interrupted, "the export was cancelled"));
		}
		let mut chunk = Vec::new();
		(&mut file).take(CHUNK).read_to_end(&mut chunk)?;
		Ok((!chunk.is_empty()).then_some(chunk))
	})
}

fn failed(error: impl std::fmt::Display) -> Outcome {
	Outcome::Failed(error.to_string())
}

/// The `OpenH264` library for an MP4 export, downloaded at its first use.
fn codec(work: &Work) -> Result<Option<PathBuf>, String> {
	let dir = work.codec_dir.as_deref().ok_or("the app has no codec folder")?;
	let download = work.codec_download.take();
	let on_chunk = |bytes, total| {
		if let Some(d) = &download {
			d.chunk(bytes, total);
		}
	};
	h264::library(dir, work.use_openh264, on_chunk).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The export of `choice` in `format` as `run` in `dir`, or named so in the archive `archive` there.
	fn plan(dir: &Path, choice: ExportKindChoice, format: ExportFormat, archive: Option<&str>) -> Plan {
		let base = if archive.is_some() { Path::new("") } else { dir };
		let outputs =
			naming::layer_outputs(&base.join("run").with_extension(format.default_extension()), choice.layers());
		Plan::Frames { choice, format, outputs, archive: archive.map(|a| dir.join(a)), frame: 0, repeat: false }
	}

	#[test]
	fn both_layers_ask_about_each_output_in_the_way() {
		let dir = tempfile::tempdir().unwrap();
		let both = |format| plan(dir.path(), ExportKindChoice::Both, format, None);
		assert!(would_replace(&both(ExportFormat::Mp4)).is_empty());
		for name in ["run_field.mp4", "run_intensity.mp4"] {
			std::fs::write(dir.path().join(name), b"").unwrap();
		}
		let asked: Vec<_> = would_replace(&both(ExportFormat::Mp4)).into_iter().map(|(_, p)| p).collect();
		assert_eq!(asked, [dir.path().join("run_field.mp4"), dir.path().join("run_intensity.mp4")]);
		for name in ["run_field", "run_intensity"] {
			std::fs::create_dir(dir.path().join(name)).unwrap();
			std::fs::write(dir.path().join(name).join("f.png"), b"").unwrap();
		}
		let asked: Vec<_> = would_replace(&both(ExportFormat::Png)).into_iter().map(|(_, p)| p).collect();
		assert_eq!(asked, [dir.path().join("run_field"), dir.path().join("run_intensity")]);
	}

	#[test]
	fn an_archived_export_asks_about_its_archive_alone() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("run_field.mp4"), b"").unwrap();
		let archived = plan(dir.path(), ExportKindChoice::Both, ExportFormat::Mp4, Some("run.zip"));
		assert!(would_replace(&archived).is_empty(), "the outputs are names in the archive");
		std::fs::write(dir.path().join("run.zip"), b"").unwrap();
		assert_eq!(would_replace(&archived), [("ExportFileExistsFormat", dir.path().join("run.zip"))]);
	}

	#[test]
	fn a_spooled_video_is_packed_whole_and_a_cancel_stops_it() {
		let dir = tempfile::tempdir().unwrap();
		let video: Vec<u8> = (0..(CHUNK as u32 + 5)).map(|i| (i % 251) as u8).collect();
		std::fs::write(dir.path().join("v.avi"), &video).unwrap();
		let archive = dir.path().join("a.zip");
		let mut zip = ZipFs::new(StdFs.create(&archive).unwrap(), now());
		pack(&mut zip, &dir.path().join("v.avi"), Path::new("run_field.avi"), &AtomicBool::new(false)).unwrap();
		zip.finish().unwrap();
		let bytes = std::fs::read(&archive).unwrap();
		assert!(bytes.windows(video.len()).any(|w| w == video), "the entry holds the video, stored");
		let mut zip = ZipFs::new(StdFs.create(&archive).unwrap(), now());
		let error = pack(&mut zip, &dir.path().join("v.avi"), Path::new("x.avi"), &AtomicBool::new(true)).unwrap_err();
		assert_eq!(error.kind(), io::ErrorKind::Interrupted);
	}
}
