//! How a browser export reaches the user: where its job writes and what is downloaded under which name. The plan's
//! outputs and archive are file names alone; the job's paths are relative to the spool, the archive, or to memory for
//! a still.

use std::path::{Path, PathBuf};

use super::Plan;

/// A browser export's outputs; each path is also the download's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
	/// A still written to memory.
	Still(PathBuf),
	/// A probe's CSV, written to memory.
	Csv(PathBuf),
	/// The image sequences in this ZIP archive of the spool.
	Archive(PathBuf),
	/// One video in the spool.
	Video(PathBuf),
	/// The videos in the spool, then packed into the ZIP archive `archive`.
	Videos { videos: Vec<PathBuf>, archive: PathBuf },
}

impl Delivery {
	/// The delivery of `plan`, which archives an export of several files.
	pub fn of(plan: &Plan) -> Self {
		match plan {
			Plan::Csv { path, .. } => Self::Csv(path.clone()),
			Plan::Frames { choice, outputs, .. } if choice.is_still() => Self::Still(outputs[0].1.clone()),
			Plan::Frames { format, outputs, archive: None, .. } if format.is_video() && outputs.len() == 1 => {
				Self::Video(outputs[0].1.clone())
			}
			Plan::Frames { format, outputs, archive, .. } => {
				let archive = archive.clone().expect("a browser export of several files is archived");
				if format.is_video() {
					Self::Videos { videos: outputs.iter().map(|(_, video)| video.clone()).collect(), archive }
				} else {
					Self::Archive(archive)
				}
			}
		}
	}

	/// The file downloaded.
	pub fn download(&self) -> &Path {
		match self {
			Self::Still(path) | Self::Csv(path) | Self::Archive(path) | Self::Video(path) => path,
			Self::Videos { archive, .. } => archive,
		}
	}
}

/// The file name of what is typed in the browser's name box, extension `ext` added when it has none; `None` without
/// a name.
pub fn typed_name(typed: &str, ext: &str) -> Option<PathBuf> {
	let name = PathBuf::from(Path::new(typed.trim()).file_name()?);
	Some(if name.extension().is_none() { name.with_extension(ext) } else { name })
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::menus::ExportKindChoice;
	use waveduderust::unstable::export::{ExportFormat, naming};

	/// The delivery of `choice` in `format` named `name`, archived as `archive`.
	fn frames(choice: ExportKindChoice, format: ExportFormat, name: &str, archive: Option<&str>) -> Delivery {
		let outputs = naming::layer_outputs(Path::new(name), choice.layers());
		let archive = archive.map(PathBuf::from);
		Delivery::of(&Plan::Frames { choice, format, outputs, archive, frame: 0, repeat: false })
	}

	#[test]
	fn each_kind_of_export_is_downloaded_under_its_name() {
		use ExportFormat::{Avi, Exr, Mov, Png};
		use ExportKindChoice::{Both, Field, FieldFrame, Intensity};
		let csv = Delivery::of(&Plan::Csv { probe: "P".into(), path: "run_P.csv".into() });
		let cases = [
			(csv, "run_P.csv"),
			(frames(FieldFrame, Exr, "run_field_0042.exr", None), "run_field_0042.exr"),
			(frames(Field, Png, "run_field.png", Some("seq.zip")), "seq.zip"),
			(frames(Both, Png, "run.png", Some("run.zip")), "run.zip"),
			(frames(Intensity, Avi, "run_intensity.avi", None), "run_intensity.avi"),
			(frames(Both, Mov, "run.mov", Some("movies.zip")), "movies.zip"),
		];
		for (delivery, name) in cases {
			assert_eq!(delivery.download(), Path::new(name), "{delivery:?}");
		}
		assert_eq!(frames(FieldFrame, Png, "a.png", None), Delivery::Still("a.png".into()));
		assert_eq!(frames(Field, Png, "run_field.png", Some("run.zip")), Delivery::Archive("run.zip".into()));
		assert_eq!(frames(Field, Avi, "run_field.avi", None), Delivery::Video("run_field.avi".into()));
		assert_eq!(
			frames(Both, Avi, "run.avi", Some("run.zip")),
			Delivery::Videos {
				videos: vec!["run_field.avi".into(), "run_intensity.avi".into()],
				archive: "run.zip".into()
			}
		);
	}

	#[test]
	fn the_name_box_keeps_the_file_name_alone() {
		assert_eq!(typed_name(" some/dir/run ", "avi"), Some("run.avi".into()));
		assert_eq!(typed_name("run.mov", "avi"), Some("run.mov".into()));
		assert_eq!(typed_name("probe", "csv"), Some("probe.csv".into()));
		assert_eq!(typed_name("  ", "csv"), None);
		assert_eq!(typed_name("dir/..", "csv"), None);
	}
}
