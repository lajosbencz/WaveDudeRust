//! How a browser export reaches the user: where its job writes and what is downloaded under which name. The plan's
//! path is a file name alone; the job's paths are relative to the spool, or to memory for a still.

use std::path::{Path, PathBuf};

use wavedude_core::model::Layer;
use wavedude_export::naming;

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
	/// Each layer's video in the spool, then packed into the ZIP archive `archive`.
	Videos { videos: [PathBuf; 2], archive: PathBuf },
}

impl Delivery {
	pub fn of(plan: &Plan) -> Self {
		match plan {
			Plan::Csv { path, .. } => Self::Csv(path.clone()),
			Plan::Frames { choice, path, .. } if choice.is_still() => Self::Still(path.clone()),
			Plan::Frames { choice, format, path, .. } => {
				let both = choice.layers().layers().len() == 2;
				let archive = path.with_extension("zip");
				match (format.is_video(), both) {
					(false, _) => Self::Archive(archive),
					(true, false) => Self::Video(path.clone()),
					(true, true) => Self::Videos {
						videos: [Layer::Field, Layer::Intensity].map(|l| naming::layer_path(path, l, true)),
						archive,
					},
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
	use wavedude_export::ExportFormat;

	fn frames(choice: ExportKindChoice, format: ExportFormat, name: &str) -> Delivery {
		Delivery::of(&Plan::Frames { choice, format, path: name.into(), frame: 0, repeat: false })
	}

	#[test]
	fn each_kind_of_export_is_downloaded_under_its_name() {
		use ExportFormat::{Avi, Exr, Mov, Png};
		use ExportKindChoice::{Both, Field, FieldFrame, Intensity};
		let csv = Delivery::of(&Plan::Csv { probe: "P".into(), path: "run_P.csv".into() });
		let cases = [
			(csv, "run_P.csv"),
			(frames(FieldFrame, Exr, "run_field_0042.exr"), "run_field_0042.exr"),
			(frames(Field, Png, "run_field.png"), "run_field.zip"),
			(frames(Both, Png, "run.png"), "run.zip"),
			(frames(Intensity, Avi, "run_intensity.avi"), "run_intensity.avi"),
			(frames(Both, Mov, "run.mov"), "run.zip"),
		];
		for (delivery, name) in cases {
			assert_eq!(delivery.download(), Path::new(name), "{delivery:?}");
		}
		assert_eq!(frames(FieldFrame, Png, "a.png"), Delivery::Still("a.png".into()));
		assert_eq!(frames(Field, Png, "run_field.png"), Delivery::Archive("run_field.zip".into()));
		assert_eq!(frames(Field, Avi, "run_field.avi"), Delivery::Video("run_field.avi".into()));
		assert_eq!(
			frames(Both, Avi, "run.avi"),
			Delivery::Videos {
				videos: ["run_field.avi".into(), "run_intensity.avi".into()],
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
