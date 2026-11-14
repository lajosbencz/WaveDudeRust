//! The desktop's export: the job blocks a worker thread, writing to the file system; what it would replace.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use wavedude_export::fs::StdFs;
use wavedude_export::{ExportFormat, ExportState, h264, naming};

use super::{Outcome, Plan, Work};
use crate::App;
use crate::menus::ExportKindChoice;

/// What an export would replace, one question each: a layer's sequence folder holding files, or its existing video or
/// still; empty when nothing is in the way or the export repeats the last one (which replaces on purpose).
pub fn would_replace(plan: &Plan) -> Vec<(&'static str, PathBuf)> {
	let Plan::Frames { format, path, repeat: false, choice, .. } = plan else { return Vec::new() };
	let both = choice == &ExportKindChoice::Both;
	let sequence = !choice.is_still() && !format.is_video();
	let outputs = choice.layers().layers().iter().map(move |&layer| naming::layer_path(path, layer, both));
	if sequence {
		outputs
			.map(|p| naming::sequence_folder(&p))
			.filter(|f| std::fs::read_dir(f).is_ok_and(|mut d| d.next().is_some()))
			.map(|f| ("ExportFolderExistsFormat", f))
			.collect()
	} else {
		outputs.filter(|p| p.is_file()).map(|p| ("ExportFileExistsFormat", p)).collect()
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
pub fn run(work: &Work, plan: Plan, progress: impl FnMut(usize, usize), cancel: &AtomicBool) -> Outcome {
	match plan {
		Plan::Csv { probe, path } => write_csv(work, &probe, &path).map_or_else(Outcome::Failed, Outcome::Completed),
		Plan::Frames { choice, format, path, frame, .. } => {
			export_frames(work, choice, format, &path, frame, progress, cancel)
		}
	}
}

fn write_csv(work: &Work, probe: &str, path: &Path) -> Result<String, String> {
	let mut file = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
	let written = work.probe_csv(probe, &mut file)?;
	Ok(format!("{written} written to {}", path.display()))
}

fn export_frames(
	work: &Work,
	choice: ExportKindChoice,
	format: ExportFormat,
	path: &Path,
	frame: usize,
	mut progress: impl FnMut(usize, usize),
	cancel: &AtomicBool,
) -> Outcome {
	let mut plan = work.export_plan(choice, format, path.to_path_buf(), frame, Arc::new(StdFs));
	if format == ExportFormat::Mp4 {
		match codec(work) {
			Ok(library) => plan.h264 = Some(Arc::new(h264::OpenH264Factory::new(library))),
			Err(e) => return Outcome::Failed(e),
		}
	}
	let job = match plan.build_job(work.store.clone(), &work.schemes) {
		Ok(job) => job,
		Err(e) => return Outcome::Failed(e.to_string()),
	};
	let report = job.run(&mut progress, cancel);
	match report.state {
		ExportState::Completed => {
			Outcome::Completed(format!("{} frames exported to {}", report.frames_done, path.display()))
		}
		ExportState::Cancelled => Outcome::Cancelled(format!("export cancelled after {} frames", report.frames_done)),
		ExportState::Failed(e) => Outcome::Failed(e.to_string()),
	}
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

	#[test]
	fn both_layers_ask_about_each_output_in_the_way() {
		let dir = tempfile::tempdir().unwrap();
		let plan = |format| Plan::Frames {
			choice: ExportKindChoice::Both,
			format,
			path: dir.path().join("run.mp4"),
			frame: 0,
			repeat: false,
		};
		assert!(would_replace(&plan(ExportFormat::Mp4)).is_empty());
		for name in ["run_field.mp4", "run_intensity.mp4"] {
			std::fs::write(dir.path().join(name), b"").unwrap();
		}
		let asked: Vec<_> = would_replace(&plan(ExportFormat::Mp4)).into_iter().map(|(_, p)| p).collect();
		assert_eq!(asked, [dir.path().join("run_field.mp4"), dir.path().join("run_intensity.mp4")]);
		for name in ["run_field", "run_intensity"] {
			std::fs::create_dir(dir.path().join(name)).unwrap();
			std::fs::write(dir.path().join(name).join("f.png"), b"").unwrap();
		}
		let asked: Vec<_> = would_replace(&plan(ExportFormat::Png)).into_iter().map(|(_, p)| p).collect();
		assert_eq!(asked, [dir.path().join("run_field"), dir.path().join("run_intensity")]);
	}
}
