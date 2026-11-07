//! Image outputs: PNG, JPEG and OpenEXR sequences (one file per frame in a folder) and stills (one file), encoded on
//! the job's [`EncoderPool`](super::EncoderPool) so a sequence keeps every core busy.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use half::f16;

use super::exr::write_exr_owned;
use super::jpeg::write_jpeg;
use super::png::{PngFilter, write_png};
use super::pool::{Done, EncoderPool, Lanes};
use super::{FrameSink, SinkSpec, check_size, remove_if_present};
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

fn write_file(path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<()>) -> Result<()> {
	let mut out = BufWriter::with_capacity(1 << 20, File::create(path)?);
	write(&mut out)?;
	out.flush()?;
	Ok(())
}

fn encode(kind: ImageKind, path: &Path, plane: &mut Plane, width: u32, height: u32) -> Result<()> {
	match (kind, plane) {
		(ImageKind::Exr, Plane::Halves(h)) => {
			let (written, back) = write_exr_owned(path, std::mem::take(h), width, height);
			*h = back;
			written
		}
		(ImageKind::Png, Plane::Rgba(p)) => write_file(path, |out| write_png(out, p, width, height, PngFilter::Paeth)),
		(ImageKind::Jpeg { quality }, Plane::Rgba(p)) => {
			write_file(path, |out| write_jpeg(out, p, width, height, quality))
		}
		_ => unreachable!("the plane is the kind's"),
	}
}

impl ImageSink {
	/// Creates the sequence's folder (or the still's) and encodes on `pool`.
	pub fn new(kind: ImageKind, placement: Placement, spec: SinkSpec, pool: &EncoderPool) -> Result<Self> {
		let lanes = Lanes::new(pool.get()?);
		let mut created_folder = None;
		let digits = match &placement {
			Placement::Sequence { chosen, first, planned } => {
				let folder = sequence_folder(chosen);
				if !folder.is_dir() {
					std::fs::create_dir_all(&folder)?;
					created_folder = Some(folder);
				}
				sequence_digits(*first, *planned)
			}
			Placement::Still(path) => {
				if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
					std::fs::create_dir_all(dir)?;
				}
				4
			}
		};
		Ok(Self {
			kind,
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
		let (kind, width, height) = (self.kind, self.width, self.height);
		self.files.push(path.clone());
		self.lanes.spawn(plane, move |plane| encode(kind, &path, plane, width, height));
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
			remove_if_present(&file)?;
		}
		if let Some(folder) = self.created_folder.take() {
			std::fs::remove_dir(folder)?;
		}
		Ok(())
	}
}
