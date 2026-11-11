//! Uncompressed 24-bit `OpenDML` AVI.
//!
//! Layout (all little-endian):
//! ```text
//! RIFF 'AVI '
//!   LIST 'hdrl'  avih(56)  LIST 'strl' [ strh(56) strf(BITMAPINFOHEADER, 40) indx(24 + 16 * INDEX_SLOTS) ]
//!                LIST 'odml' [ dmlh(248) ]
//!   LIST 'movi'  00db ... ix00
//!   idx1
//! RIFF 'AVIX'  LIST 'movi'  00db ... ix00         (each RIFF at most the segment limit, 1 GiB)
//! ```
//! A `00db` chunk is one frame: rows bottom-up, BGR, each row padded to 4 bytes. `ix00` is the segment's standard
//! index (offsets from the segment's `movi` list start), `indx` the super index over the `ix00` chunks, `idx1` the
//! legacy index of the first segment (offsets from the `movi` fourcc). The headers' counts are rewritten at close.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use super::counted::Counted;
use super::{FrameSink, check_size};
use crate::{ExportError, Result};

/// The `OpenDML` segment size.
pub const SEGMENT_LIMIT: u64 = 1 << 30;
/// Entries reserved in the super index.
pub const INDEX_SLOTS: usize = 1024;
const AVIF_HASINDEX: u32 = 0x10;
const AVIIF_KEYFRAME: u32 = 0x10;

struct Segment {
	riff: u64,
	movi: u64,
	/// (absolute data offset, size).
	chunks: Vec<(u64, u32)>,
}

/// An `OpenDML` AVI writer.
pub struct AviWriter<W: Write + Seek> {
	out: Counted<W>,
	width: u32,
	height: u32,
	fps: u32,
	stride: usize,
	segment_limit: u64,
	segment: Segment,
	first_segment_frames: Option<usize>,
	/// (ix00 offset, ix00 size, frames) per finished segment.
	super_index: Vec<(u64, u32, u32)>,
	frames: usize,
	row: Vec<u8>,
	positions: Positions,
	finished: bool,
}

#[derive(Default)]
struct Positions {
	avih: u64,
	strh: u64,
	indx: u64,
	dmlh: u64,
}

fn fourcc(out: &mut impl Write, f: &[u8; 4]) -> std::io::Result<()> {
	out.write_all(f)
}

fn u32le(out: &mut impl Write, v: u32) -> std::io::Result<()> {
	out.write_all(&v.to_le_bytes())
}

fn u16le(out: &mut impl Write, v: u16) -> std::io::Result<()> {
	out.write_all(&v.to_le_bytes())
}

fn u64le(out: &mut impl Write, v: u64) -> std::io::Result<()> {
	out.write_all(&v.to_le_bytes())
}

/// A frame's chunk, its size field and `dwSuggestedBufferSize` must fit in 32 bits.
fn check(width: u32, height: u32) -> Result<()> {
	if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
		return Err(ExportError::Size { format: "AVI", width, height, limit: "at least 1 x 1" });
	}
	let stride = (width as u64 * 3).div_ceil(4) * 4;
	if stride * height as u64 + 8 > u32::MAX as u64 {
		return Err(ExportError::Size { format: "AVI", width, height, limit: "a frame under 4 GiB" });
	}
	Ok(())
}

impl AviWriter<BufWriter<File>> {
	pub fn create(path: &Path, width: u32, height: u32, fps: u32) -> Result<Self> {
		check(width, height)?;
		Self::new(BufWriter::with_capacity(1 << 20, File::create(path)?), width, height, fps, SEGMENT_LIMIT)
	}
}

impl<W: Write + Seek> AviWriter<W> {
	/// A writer whose RIFF segments stay within `segment_limit` bytes (each holds at least one frame).
	pub fn new(out: W, width: u32, height: u32, fps: u32, segment_limit: u64) -> Result<Self> {
		check(width, height)?;
		let stride = (width as usize * 3).div_ceil(4) * 4;
		let mut out = Counted::new(out)?;
		let mut positions = Positions::default();
		fourcc(&mut out, b"RIFF")?;
		u32le(&mut out, 0)?;
		fourcc(&mut out, b"AVI ")?;
		let indx_size = 24 + 16 * INDEX_SLOTS as u32;
		let strl_size = 4 + (8 + 56) + (8 + 40) + (8 + indx_size);
		let odml_size = 4 + 8 + 248;
		let hdrl_size = 4 + (8 + 56) + (8 + strl_size) + (8 + odml_size);
		fourcc(&mut out, b"LIST")?;
		u32le(&mut out, hdrl_size)?;
		fourcc(&mut out, b"hdrl")?;
		positions.avih = out.stream_position()?;
		out.write_all(&[0; 8 + 56])?;
		fourcc(&mut out, b"LIST")?;
		u32le(&mut out, strl_size)?;
		fourcc(&mut out, b"strl")?;
		positions.strh = out.stream_position()?;
		out.write_all(&[0; 8 + 56])?;
		fourcc(&mut out, b"strf")?;
		u32le(&mut out, 40)?;
		for v in [40, width, height] {
			u32le(&mut out, v)?;
		}
		u16le(&mut out, 1)?;
		u16le(&mut out, 24)?;
		for v in [0, (stride * height as usize) as u32, 0, 0, 0, 0] {
			u32le(&mut out, v)?;
		}
		positions.indx = out.stream_position()?;
		fourcc(&mut out, b"indx")?;
		u32le(&mut out, indx_size)?;
		out.write_all(&vec![0; indx_size as usize])?;
		fourcc(&mut out, b"LIST")?;
		u32le(&mut out, odml_size)?;
		fourcc(&mut out, b"odml")?;
		positions.dmlh = out.stream_position()?;
		fourcc(&mut out, b"dmlh")?;
		u32le(&mut out, 248)?;
		out.write_all(&[0; 248])?;
		let mut writer = Self {
			out,
			width,
			height,
			fps,
			stride,
			segment_limit,
			segment: Segment { riff: 0, movi: 0, chunks: Vec::new() },
			first_segment_frames: None,
			super_index: Vec::new(),
			frames: 0,
			row: vec![0; stride],
			positions,
			finished: false,
		};
		writer.write_headers()?;
		writer.open_movi()?;
		Ok(writer)
	}

	fn frame_bytes(&self) -> u32 {
		(self.stride * self.height as usize) as u32
	}

	fn open_movi(&mut self) -> std::io::Result<()> {
		self.segment.movi = self.out.stream_position()?;
		fourcc(&mut self.out, b"LIST")?;
		u32le(&mut self.out, 0)?;
		fourcc(&mut self.out, b"movi")
	}

	fn open_avix(&mut self) -> std::io::Result<()> {
		let riff = self.out.stream_position()?;
		fourcc(&mut self.out, b"RIFF")?;
		u32le(&mut self.out, 0)?;
		fourcc(&mut self.out, b"AVIX")?;
		self.segment = Segment { riff, movi: 0, chunks: Vec::new() };
		self.open_movi()
	}

	/// Bytes the segment would hold with one more frame and its index entries.
	fn segment_size_with_next(&mut self) -> std::io::Result<u64> {
		let pos = self.out.stream_position()?;
		let n = self.segment.chunks.len() as u64 + 1;
		let legacy = if self.first_segment_frames.is_none() { 8 + 16 * n } else { 0 };
		Ok(pos - self.segment.riff + 8 + self.frame_bytes() as u64 + 8 + 24 + 8 * n + legacy)
	}

	fn patch_u32(&mut self, at: u64, v: u32) -> std::io::Result<()> {
		self.out.seek(SeekFrom::Start(at))?;
		u32le(&mut self.out, v)
	}

	/// Writes `ix00` into the segment's movi, closes the list (and the first segment's `idx1`) and the RIFF.
	fn close_segment(&mut self) -> std::io::Result<()> {
		let base = self.segment.movi;
		let ix = self.out.stream_position()?;
		let entries = self.segment.chunks.len() as u32;
		let ix_size = 24 + 8 * entries;
		fourcc(&mut self.out, b"ix00")?;
		u32le(&mut self.out, ix_size)?;
		u16le(&mut self.out, 2)?;
		self.out.write_all(&[0, 1])?;
		u32le(&mut self.out, entries)?;
		fourcc(&mut self.out, b"00db")?;
		u64le(&mut self.out, base)?;
		u32le(&mut self.out, 0)?;
		for &(offset, size) in &self.segment.chunks {
			u32le(&mut self.out, (offset - base) as u32)?;
			u32le(&mut self.out, size)?;
		}
		let movi_end = self.out.stream_position()?;
		if self.first_segment_frames.is_none() {
			self.first_segment_frames = Some(self.segment.chunks.len());
			fourcc(&mut self.out, b"idx1")?;
			u32le(&mut self.out, 16 * entries)?;
			for &(offset, size) in &self.segment.chunks {
				fourcc(&mut self.out, b"00db")?;
				u32le(&mut self.out, AVIIF_KEYFRAME)?;
				u32le(&mut self.out, (offset - 8 - (base + 8)) as u32)?;
				u32le(&mut self.out, size)?;
			}
		}
		let end = self.out.stream_position()?;
		self.patch_u32(base + 4, (movi_end - base - 8) as u32)?;
		self.patch_u32(self.segment.riff + 4, (end - self.segment.riff - 8) as u32)?;
		self.out.seek(SeekFrom::Start(end))?;
		self.super_index.push((ix, 8 + ix_size, entries));
		Ok(())
	}

	fn write_headers(&mut self) -> std::io::Result<()> {
		let frame = self.frame_bytes();
		let total = self.frames as u32;
		let first = self.first_segment_frames.unwrap_or(self.frames) as u32;
		let (w, h, fps) = (self.width, self.height, self.fps);
		let out = &mut self.out;
		out.seek(SeekFrom::Start(self.positions.avih))?;
		fourcc(out, b"avih")?;
		u32le(out, 56)?;
		let micro = (1e6 / fps as f64).round() as u32;
		for v in [micro, frame.saturating_mul(fps), 0, AVIF_HASINDEX, first, 0, 1, frame + 8, w, h, 0, 0, 0, 0] {
			u32le(out, v)?;
		}
		out.seek(SeekFrom::Start(self.positions.strh))?;
		fourcc(out, b"strh")?;
		u32le(out, 56)?;
		fourcc(out, b"vids")?;
		u32le(out, 0)?;
		u32le(out, 0)?;
		u16le(out, 0)?;
		u16le(out, 0)?;
		for v in [0, 1, fps, 0, total, frame + 8, u32::MAX, 0] {
			u32le(out, v)?;
		}
		for v in [0, 0, w.min(i16::MAX as u32) as u16, h.min(i16::MAX as u32) as u16] {
			u16le(out, v)?;
		}
		out.seek(SeekFrom::Start(self.positions.indx + 8))?;
		u16le(out, 4)?;
		out.write_all(&[0, 0])?;
		u32le(out, self.super_index.len() as u32)?;
		fourcc(out, b"00db")?;
		out.write_all(&[0; 12])?;
		for &(offset, size, duration) in &self.super_index {
			u64le(out, offset)?;
			u32le(out, size)?;
			u32le(out, duration)?;
		}
		out.seek(SeekFrom::Start(self.positions.dmlh + 8))?;
		u32le(out, total)?;
		out.seek(SeekFrom::End(0))?;
		Ok(())
	}

	/// The writer back after [`FrameSink::finish`] (for in-memory outputs).
	pub fn into_inner(self) -> W {
		self.out.into_inner()
	}
}

impl<W: Write + Seek + Send> FrameSink for AviWriter<W> {
	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		if self.finished {
			return Err(ExportError::Finished);
		}
		check_size(self.width, self.height, width, height, rgba.len(), 4)?;
		if !self.segment.chunks.is_empty() && self.segment_size_with_next()? > self.segment_limit {
			if self.super_index.len() + 1 >= INDEX_SLOTS {
				return Err(ExportError::Size { format: "AVI", width, height, limit: "1024 segments" });
			}
			self.close_segment()?;
			self.open_avix()?;
		}
		let size = self.frame_bytes();
		fourcc(&mut self.out, b"00db")?;
		u32le(&mut self.out, size)?;
		let data = self.out.stream_position()?;
		let w = width as usize;
		for y in (0..height as usize).rev() {
			let src = &rgba[y * w * 4..(y + 1) * w * 4];
			for (d, s) in self.row.as_chunks_mut::<3>().0.iter_mut().zip(src.as_chunks::<4>().0) {
				d.copy_from_slice(&[s[2], s[1], s[0]]);
			}
			self.out.write_all(&self.row)?;
		}
		self.segment.chunks.push((data, size));
		self.frames += 1;
		Ok(())
	}

	fn frames_written(&self) -> usize {
		self.frames
	}

	fn finish(&mut self) -> Result<()> {
		if self.finished {
			return Ok(());
		}
		self.finished = true;
		self.close_segment()?;
		self.write_headers()?;
		self.out.flush()?;
		Ok(())
	}
}
