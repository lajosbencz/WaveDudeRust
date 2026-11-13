//! Image outputs: PNG, JPEG and `OpenEXR` sequences (one file per frame in a folder) and stills (one file), encoded on
//! the job's [`EncoderPool`] so a sequence keeps every core busy.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use half::f16;

use super::exr::write_exr_owned;
use super::jpeg::write_jpeg;
use super::png::{PngFilter, write_png};
use super::pool::{Done, EncoderPool, Lanes};
use super::{FrameSink, SinkSpec, check_size};
use crate::fs::{OutputFile, OutputFs};
use crate::naming::{sequence_digits, sequence_file, sequence_folder};
use crate::{ExportError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
	Png,
	Jpeg {
		quality: u32,
	},
	/// Takes halves.
	Exr,
}

/// Where the files go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
	/// One file at the path.
	Still(PathBuf),
	/// `dir/name/name_NNNN.ext` for a chosen `dir/name.ext`, numbered from `first`.
	Sequence { chosen: PathBuf, first: usize, planned: usize },
}

enum Plane {
	Rgba(Vec<u8>),
	Halves(Vec<f16>),
}

/// An image sequence or still.
pub struct ImageSink {
	kind: ImageKind,
	fs: Arc<dyn OutputFs>,
	placement: Placement,
	width: u32,
	height: u32,
	digits: usize,
	lanes: Lanes<Plane>,
	files: Vec<PathBuf>,
	/// The sequence folder, when this sink created it.
	created_folder: Option<PathBuf>,
	error: Option<ExportError>,
}

fn encode(kind: ImageKind, out: &mut dyn OutputFile, plane: &mut Plane, width: u32, height: u32) -> Result<()> {
	match (kind, plane) {
		(ImageKind::Exr, Plane::Halves(h)) => {
			let (written, back) = write_exr_owned(out, std::mem::take(h), width, height);
			*h = back;
			written
		}
		(ImageKind::Png, Plane::Rgba(p)) => write_png(out, p, width, height, PngFilter::Paeth),
		(ImageKind::Jpeg { quality }, Plane::Rgba(p)) => write_jpeg(out, p, width, height, quality),
		_ => unreachable!("the plane is the kind's"),
	}
}

/// Writes the image of `plane` to a file at `path` of `fs`, closed whatever the outcome.
fn write_file(
	fs: &dyn OutputFs,
	path: &Path,
	kind: ImageKind,
	plane: &mut Plane,
	width: u32,
	height: u32,
) -> Result<()> {
	let mut out = fs.create(path)?;
	let written = encode(kind, &mut *out, plane, width, height);
	let closed = out.close();
	written.and(closed.map_err(Into::into))
}

impl ImageSink {
	/// Creates the sequence's folder (or the still's) in `fs` and encodes on `pool`.
	pub fn new(
		kind: ImageKind,
		fs: Arc<dyn OutputFs>,
		placement: Placement,
		spec: SinkSpec,
		pool: &EncoderPool,
	) -> Result<Self> {
		let lanes = Lanes::new(pool)?;
		let mut created_folder = None;
		let digits = match &placement {
			Placement::Sequence { chosen, first, planned } => {
				let folder = sequence_folder(chosen);
				if fs.create_dir_all(&folder)? {
					created_folder = Some(folder);
				}
				sequence_digits(*first, *planned)
			}
			Placement::Still(path) => {
				if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
					fs.create_dir_all(dir)?;
				}
				4
			}
		};
		Ok(Self {
			kind,
			fs,
			placement,
			width: spec.width,
			height: spec.height,
			digits,
			lanes,
			files: Vec::new(),
			created_folder,
			error: None,
		})
	}

	/// The file the next frame goes to.
	fn next_path(&self) -> Result<PathBuf> {
		let written = self.files.len();
		match &self.placement {
			Placement::Still(path) if written == 0 => Ok(path.clone()),
			Placement::Still(_) => Err(ExportError::Finished),
			Placement::Sequence { chosen, first, .. } => Ok(sequence_file(chosen, first + written, self.digits)),
		}
	}

	/// Recycles a finished task's plane, keeping the first error.
	fn settle(&mut self, done: Done<Plane>) {
		self.lanes.recycle(done.buf);
		if let Err(e) = done.result {
			match &self.error {
				None => self.error = Some(e),
				Some(_) => tracing::warn!(error = %e, "a further image failed"),
			}
		}
	}

	fn take_error(&mut self) -> Result<()> {
		self.error.take().map_or(Ok(()), Err)
	}

	fn submit(&mut self, fill: impl FnOnce(&mut Plane)) -> Result<()> {
		while let Some(done) = self.lanes.next_done(false) {
			self.settle(done);
		}
		self.take_error()?;
		let path = self.next_path()?;
		let halves = self.kind == ImageKind::Exr;
		let make = || if halves { Plane::Halves(Vec::new()) } else { Plane::Rgba(Vec::new()) };
		let mut plane = loop {
			if let Some(p) = self.lanes.free(make) {
				break p;
			}
			let done = self.lanes.next_done(true).ok_or(ExportError::Finished)?;
			self.settle(done);
			self.take_error()?;
		};
		fill(&mut plane);
		let (kind, width, height, fs) = (self.kind, self.width, self.height, self.fs.clone());
		self.files.push(path.clone());
		self.lanes.spawn(plane, move |plane| write_file(&*fs, &path, kind, plane, width, height));
		Ok(())
	}

	/// The folder of a sequence or the file of a still.
	pub fn output(&self) -> &Path {
		match &self.placement {
			Placement::Still(p) => p,
			Placement::Sequence { chosen, .. } => chosen,
		}
	}
}

impl FrameSink for ImageSink {
	fn wants_halves(&self) -> bool {
		self.kind == ImageKind::Exr
	}

	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		if self.wants_halves() {
			return Err(ExportError::WrongInput("stored halves"));
		}
		check_size(self.width, self.height, width, height, rgba.len(), 4)?;
		self.submit(|plane| {
			let Plane::Rgba(p) = plane else { unreachable!("an RGBA sink recycles RGBA planes") };
			p.clear();
			p.extend_from_slice(rgba);
		})
	}

	fn write_halves(&mut self, halves: &[f16], width: u32, height: u32) -> Result<()> {
		if !self.wants_halves() {
			return Err(ExportError::WrongInput("rendered pixels"));
		}
		check_size(self.width, self.height, width, height, halves.len(), 1)?;
		self.submit(|plane| {
			let Plane::Halves(h) = plane else { unreachable!("a halves sink recycles halves planes") };
			h.clear();
			h.extend_from_slice(halves);
		})
	}

	fn frames_written(&self) -> usize {
		self.files.len()
	}

	fn finish(&mut self) -> Result<()> {
		while let Some(done) = self.lanes.next_done(true) {
			self.settle(done);
		}
		self.take_error()
	}

	fn files(&self) -> Vec<PathBuf> {
		self.files.clone()
	}

	fn discard(&mut self) -> Result<()> {
		for file in self.files.drain(..) {
			self.fs.remove_file(&file)?;
		}
		if let Some(folder) = self.created_folder.take() {
			self.fs.remove_dir(&folder)?;
		}
		Ok(())
	}
}
