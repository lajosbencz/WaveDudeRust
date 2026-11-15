//! `QuickTime` movies with the Animation codec (`rle `, 24-bit, every sample a full-frame key frame) or PNG samples
//! (`png `, filter None).
//!
//! Layout (big-endian): `ftyp` (brand `qt  `); a `free` atom of `1024 + 12 * frames` bytes when the frame count is
//! known, into which `moov` is written at close (else `moov` follows `mdat`); `mdat` with a 64-bit size; `moov` with
//! one video track, timescale = fps, each sample one tick, one sample per chunk, offsets in `co64`.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};

use wavedude_core::sync::MaybeSend;

use super::counted::Counted;
use super::png::{PngFilter, encode_png_into};
use super::pool::{Done, EncoderPool, Lanes};
use super::{FrameSink, check_size};
use crate::{ExportError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovCodec {
	/// Apple Animation, 24-bit RLE.
	Animation,
	/// Each sample a PNG.
	Png,
}

impl MovCodec {
	fn fourcc(self) -> &'static [u8; 4] {
		match self {
			MovCodec::Animation => b"rle ",
			MovCodec::Png => b"png ",
		}
	}

	fn name(self) -> &'static str {
		match self {
			MovCodec::Animation => "Animation",
			MovCodec::Png => "PNG",
		}
	}

	/// One RGBA8 frame as this codec's sample, into `out` (cleared first).
	pub fn encode_into(self, rgba: &[u8], width: u32, height: u32, out: &mut Vec<u8>) -> Result<()> {
		match self {
			MovCodec::Animation => {
				encode_animation_into(rgba, width as usize, height as usize, out);
				Ok(())
			}
			MovCodec::Png => encode_png_into(rgba, width, height, PngFilter::None, out),
		}
	}
}

/// A `QuickTime` writer.
pub struct MovWriter<W: Write + Seek> {
	out: Counted<W>,
	width: u32,
	height: u32,
	fps: u32,
	codec: MovCodec,
	/// Position and size of the reserved `free` atom.
	reserve: Option<(u64, u64)>,
	mdat: u64,
	/// (offset, size) per sample.
	samples: Vec<(u64, u32)>,
	/// The last encoded sample, its allocation reused.
	sample: Vec<u8>,
	finished: bool,
}

/// One atom: size, kind, body.
fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
	let mut a = Vec::with_capacity(body.len() + 8);
	a.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
	a.extend_from_slice(kind);
	a.extend_from_slice(body);
	a
}

/// Big-endian field helper.
#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
	fn u8(mut self, v: u8) -> Self {
		self.0.push(v);
		self
	}
	fn u16(mut self, v: u16) -> Self {
		self.0.extend_from_slice(&v.to_be_bytes());
		self
	}
	fn u32(mut self, v: u32) -> Self {
		self.0.extend_from_slice(&v.to_be_bytes());
		self
	}
	fn u64(mut self, v: u64) -> Self {
		self.0.extend_from_slice(&v.to_be_bytes());
		self
	}
	fn bytes(mut self, v: &[u8]) -> Self {
		self.0.extend_from_slice(v);
		self
	}
	fn zeros(mut self, n: usize) -> Self {
		self.0.resize(self.0.len() + n, 0);
		self
	}
	fn matrix(self) -> Self {
		[0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x4000_0000].into_iter().fold(self, Body::u32)
	}
	fn pascal(self, s: &str, field: usize) -> Self {
		let len = s.len();
		debug_assert!(len < field, "a Pascal string fits its field");
		self.u8(len as u8).bytes(s.as_bytes()).zeros(field - 1 - len)
	}
}

fn check(width: u32, height: u32) -> Result<()> {
	if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
		return Err(ExportError::Size { format: "QuickTime", width, height, limit: "up to 65535 x 65535" });
	}
	Ok(())
}

impl<W: Write + Seek> MovWriter<W> {
	/// A writer; `frames`, when known, reserves room for `moov` ahead of `mdat`.
	pub fn new(out: W, width: u32, height: u32, fps: u32, codec: MovCodec, frames: Option<usize>) -> Result<Self> {
		check(width, height)?;
		let mut out = Counted::new(out)?;
		out.write_all(&atom(b"ftyp", &Body::default().bytes(b"qt  ").u32(0x2005_0300).bytes(b"qt  ").0))?;
		let reserve = match frames {
			Some(n) => {
				let pos = out.stream_position()?;
				let size = 1024 + 12 * n as u64;
				out.write_all(&(size as u32).to_be_bytes())?;
				out.write_all(b"free")?;
				std::io::copy(&mut std::io::repeat(0).take(size - 8), &mut out)?;
				Some((pos, size))
			}
			None => None,
		};
		let mdat = out.stream_position()?;
		out.write_all(&Body::default().u32(1).bytes(b"mdat").u64(16).0)?;
		Ok(Self {
			out,
			width,
			height,
			fps,
			codec,
			reserve,
			mdat,
			samples: Vec::new(),
			sample: Vec::new(),
			finished: false,
		})
	}

	fn moov(&self) -> Vec<u8> {
		let n = self.samples.len() as u32;
		let (w, h) = (self.width, self.height);
		let mvhd = Body::default()
			.u32(0)
			.u32(0)
			.u32(0)
			.u32(self.fps)
			.u32(n)
			.u32(0x10000)
			.u16(0x100)
			.zeros(10)
			.matrix()
			.zeros(24)
			.u32(2);
		let tkhd = Body::default()
			.u32(0x0000_000F)
			.u32(0)
			.u32(0)
			.u32(1)
			.u32(0)
			.u32(n)
			.zeros(8)
			.u16(0)
			.u16(0)
			.u16(0)
			.u16(0)
			.matrix()
			.u32(w << 16)
			.u32(h << 16);
		let mdhd = Body::default().u32(0).u32(0).u32(0).u32(self.fps).u32(n).u16(0).u16(0);
		let hdlr = |kind: &[u8; 4], subtype: &[u8; 4], name: &str| {
			atom(
				b"hdlr",
				&Body::default().u32(0).bytes(kind).bytes(subtype).u32(0).u32(0).u32(0).pascal(name, name.len() + 1).0,
			)
		};
		let vmhd = Body::default().u32(1).u16(0x40).u16(0x8000).u16(0x8000).u16(0x8000);
		let dref = Body::default().u32(0).u32(1).bytes(&atom(b"alis", &Body::default().u32(1).0));
		let entry = Body::default()
			.zeros(6)
			.u16(1)
			.u16(0)
			.u16(0)
			.u32(0)
			.u32(0)
			.u32(0x3FF)
			.u16(w as u16)
			.u16(h as u16)
			.u32(0x0048_0000)
			.u32(0x0048_0000)
			.u32(0)
			.u16(1)
			.pascal(self.codec.name(), 32)
			.u16(24)
			.u16(0xFFFF);
		let stsd = Body::default().u32(0).u32(1).bytes(&atom(self.codec.fourcc(), &entry.0));
		let stts = Body::default().u32(0).u32(1).u32(n).u32(1);
		let stsc = Body::default().u32(0).u32(1).u32(1).u32(1).u32(1);
		let stsz = self.samples.iter().fold(Body::default().u32(0).u32(0).u32(n), |b, s| b.u32(s.1));
		let co64 = self.samples.iter().fold(Body::default().u32(0).u32(n), |b, s| b.u64(s.0));
		let stbl = [
			atom(b"stsd", &stsd.0),
			atom(b"stts", &stts.0),
			atom(b"stsc", &stsc.0),
			atom(b"stsz", &stsz.0),
			atom(b"co64", &co64.0),
		]
		.concat();
		let minf = [
			atom(b"vmhd", &vmhd.0),
			hdlr(b"dhlr", b"alis", "DataHandler"),
			atom(b"dinf", &atom(b"dref", &dref.0)),
			atom(b"stbl", &stbl),
		]
		.concat();
		let mdia = [atom(b"mdhd", &mdhd.0), hdlr(b"mhlr", b"vide", "VideoHandler"), atom(b"minf", &minf)].concat();
		let trak = [atom(b"tkhd", &tkhd.0), atom(b"mdia", &mdia)].concat();
		atom(b"moov", &[atom(b"mvhd", &mvhd.0), atom(b"trak", &trak)].concat())
	}

	/// The writer back after [`FrameSink::finish`] (for in-memory outputs).
	pub fn into_inner(self) -> W {
		self.out.into_inner()
	}

	pub fn codec(&self) -> MovCodec {
		self.codec
	}

	/// Appends one sample encoded by [`MovCodec::encode_into`] for this writer's size.
	pub fn write_sample(&mut self, sample: &[u8]) -> Result<()> {
		if self.finished {
			return Err(ExportError::Finished);
		}
		let size = u32::try_from(sample.len()).map_err(|_| ExportError::Size {
			format: "QuickTime",
			width: self.width,
			height: self.height,
			limit: "a sample under 4 GiB",
		})?;
		let offset = self.out.stream_position()?;
		self.out.write_all(sample)?;
		self.samples.push((offset, size));
		Ok(())
	}
}

/// One full frame in Apple Animation's 24-bit RLE: the sample size (u32), header 0 (u16, the whole frame), then per
/// row a skip byte of 1 and opcodes: `n` in 1..=127 copies n pixels that follow, `-n` in -2..=-128 repeats the one
/// pixel that follows n times, -1 ends the row. Pixels are R, G, B.
pub fn encode_animation(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
	let mut out = Vec::with_capacity(rgba.len() / 2);
	encode_animation_into(rgba, width, height, &mut out);
	out
}

/// [`encode_animation`] into `out`, cleared first.
pub fn encode_animation_into(rgba: &[u8], width: usize, height: usize, out: &mut Vec<u8>) {
	out.clear();
	out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
	let pixel = |row: &[u8], x: usize| [row[x * 4], row[x * 4 + 1], row[x * 4 + 2]];
	for y in 0..height {
		let row = &rgba[y * width * 4..(y + 1) * width * 4];
		out.push(1);
		let mut x = 0;
		let mut literal_start = 0;
		let flush = |out: &mut Vec<u8>, from: usize, to: usize| {
			let mut i = from;
			while i < to {
				let n = (to - i).min(127);
				out.push(n as u8);
				for p in i..i + n {
					out.extend_from_slice(&pixel(row, p));
				}
				i += n;
			}
		};
		while x < width {
			let p = pixel(row, x);
			let mut run = 1;
			while x + run < width && run < 128 && pixel(row, x + run) == p {
				run += 1;
			}
			if run >= 2 {
				flush(out, literal_start, x);
				out.push((-(run as i32)) as u8);
				out.extend_from_slice(&p);
				x += run;
				literal_start = x;
			} else {
				x += 1;
			}
		}
		flush(out, literal_start, width);
		out.push(0xFF);
	}
	let size = out.len() as u32;
	out[..4].copy_from_slice(&size.to_be_bytes());
}

impl<W: Write + Seek + MaybeSend> FrameSink for MovWriter<W> {
	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		if self.finished {
			return Err(ExportError::Finished);
		}
		check_size(self.width, self.height, width, height, rgba.len(), 4)?;
		let mut sample = std::mem::take(&mut self.sample);
		let written =
			self.codec.encode_into(rgba, width, height, &mut sample).and_then(|()| self.write_sample(&sample));
		self.sample = sample;
		written
	}

	fn frames_written(&self) -> usize {
		self.samples.len()
	}

	fn finish(&mut self) -> Result<()> {
		if self.finished {
			return Ok(());
		}
		self.finished = true;
		let end = self.out.stream_position()?;
		self.out.seek(SeekFrom::Start(self.mdat + 8))?;
		self.out.write_all(&(end - self.mdat).to_be_bytes())?;
		let moov = self.moov();
		match self.reserve {
			Some((pos, size)) if moov.len() as u64 == size || moov.len() as u64 + 8 <= size => {
				self.out.seek(SeekFrom::Start(pos))?;
				self.out.write_all(&moov)?;
				let rest = size - moov.len() as u64;
				if rest > 0 {
					self.out.write_all(&(rest as u32).to_be_bytes())?;
					self.out.write_all(b"free")?;
				}
				self.out.seek(SeekFrom::Start(end))?;
			}
			_ => {
				self.out.seek(SeekFrom::Start(end))?;
				self.out.write_all(&moov)?;
			}
		}
		self.out.flush()?;
		Ok(())
	}
}

/// A frame and its encoded sample.
type Buffers = (Vec<u8>, Vec<u8>);

/// A [`MovWriter`] whose samples are encoded on the job's encoder pool and written in order.
pub struct ParallelMov<W: Write + Seek> {
	writer: MovWriter<W>,
	lanes: Lanes<Buffers>,
	/// Finished samples waiting for an earlier one, by sequence number.
	pending: BTreeMap<usize, Buffers>,
	next: usize,
	error: Option<ExportError>,
	/// A sample failed: none after it is written.
	failed: bool,
}

impl<W: Write + Seek> ParallelMov<W> {
	pub fn new(writer: MovWriter<W>, pool: &EncoderPool) -> Result<Self> {
		let lanes = Lanes::new(pool)?;
		Ok(Self { writer, lanes, pending: BTreeMap::new(), next: 0, error: None, failed: false })
	}

	/// Takes a finished sample and writes every sample now in order; keeps the first error.
	fn settle(&mut self, done: Done<Buffers>) {
		let mut fail = |e: ExportError| {
			self.failed = true;
			self.error.get_or_insert(e);
		};
		if let Err(e) = done.result {
			fail(e);
		}
		self.pending.insert(done.seq, done.buf);
		while let Some(buf) = self.pending.remove(&self.next) {
			if !self.failed
				&& let Err(e) = self.writer.write_sample(&buf.1)
			{
				self.failed = true;
				self.error.get_or_insert(e);
			}
			self.lanes.recycle(buf);
			self.next += 1;
		}
	}

	fn take_error(&mut self) -> Result<()> {
		self.error.take().map_or(Ok(()), Err)
	}

	pub fn into_inner(self) -> W {
		self.writer.into_inner()
	}
}

impl<W: Write + Seek + MaybeSend> FrameSink for ParallelMov<W> {
	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		check_size(self.writer.width, self.writer.height, width, height, rgba.len(), 4)?;
		if self.writer.finished {
			return Err(ExportError::Finished);
		}
		while let Some(done) = self.lanes.next_done(false) {
			self.settle(done);
		}
		self.take_error()?;
		let mut buf = loop {
			if let Some(b) = self.lanes.free(Default::default) {
				break b;
			}
			let done = self.lanes.next_done(true).ok_or(ExportError::Finished)?;
			self.settle(done);
			self.take_error()?;
		};
		buf.0.clear();
		buf.0.extend_from_slice(rgba);
		let codec = self.writer.codec;
		self.lanes.spawn(buf, move |(frame, sample)| codec.encode_into(frame, width, height, sample));
		Ok(())
	}

	fn frames_written(&self) -> usize {
		if self.writer.finished { self.writer.samples.len() } else { self.lanes.submitted() }
	}

	fn finish(&mut self) -> Result<()> {
		while let Some(done) = self.lanes.next_done(true) {
			self.settle(done);
		}
		let settled = self.take_error();
		let finished = self.writer.finish();
		settled.and(finished)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The RGB pixels of an [`encode_animation`] sample, read by the opcodes it documents.
	fn decode_animation(s: &[u8], width: usize, height: usize) -> Vec<[u8; 3]> {
		assert_eq!(&s[..4], &(s.len() as u32).to_be_bytes(), "the sample size leads");
		assert_eq!(&s[4..6], &[0, 0], "header 0: the whole frame");
		let px = |i: usize| [s[i], s[i + 1], s[i + 2]];
		let (mut pixels, mut i) = (Vec::new(), 6);
		for y in 0..height {
			assert_eq!(s[i], 1, "row {y} skip byte");
			i += 1;
			let start = pixels.len();
			loop {
				let op = s[i] as i8;
				i += 1;
				match op {
					-1 => break,
					1..=127 => {
						pixels.extend((0..op as usize).map(|k| px(i + 3 * k)));
						i += 3 * op as usize;
					}
					-128..=-2 => {
						pixels.extend(std::iter::repeat_n(px(i), op.unsigned_abs() as usize));
						i += 3;
					}
					0 => panic!("row {y}: a skip code in a full frame"),
				}
			}
			assert_eq!(pixels.len() - start, width, "row {y} width");
		}
		assert_eq!(i, s.len(), "nothing follows the last row");
		pixels
	}

	#[test]
	fn animation_samples_decode_to_their_pixels_and_runs_shrink_them() {
		// Past both opcode limits: runs of more than 128, literals of more than 127.
		let width = 300;
		let rows: [fn(usize) -> [u8; 3]; 3] = [
			|_| [9, 8, 7],
			|x| [x as u8, (x >> 8) as u8, 7],
			|x| if x % 7 < 4 { [(x / 7) as u8, 1, 2] } else { [x as u8, 3, 4] },
		];
		let rgb: Vec<[u8; 3]> = rows.iter().flat_map(|row| (0..width).map(row)).collect();
		let rgba: Vec<u8> = rgb.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect();
		assert_eq!(decode_animation(&encode_animation(&rgba, width, rows.len()), width, rows.len()), rgb);

		let uniform = encode_animation(&rgba[..width * 4], width, 1);
		assert!(uniform.len() < width * 3, "a run of one colour takes {} bytes", uniform.len());
	}
}
