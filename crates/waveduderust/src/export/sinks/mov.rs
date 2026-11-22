//! `QuickTime` movies with the Animation codec (`rle `, 24-bit, every sample a full-frame key frame) or PNG samples
//! (`png `, filter None).
//!
//! The container is the `QuickTime` writer of `sinks::bmff`.

use std::collections::BTreeMap;
use std::io::{Seek, Write};

use crate::sync::MaybeSend;

use super::bmff::{SampleEntry, Track};
use super::png::{PngFilter, encode_png_into};
use super::pool::{Done, EncoderPool, Lanes};
use super::{FrameSink, check_size};
use crate::export::{ExportError, Result};

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
	track: Track<W>,
	codec: MovCodec,
	/// The last encoded sample, its allocation reused.
	sample: Vec<u8>,
}

impl<W: Write + Seek> MovWriter<W> {
	/// A writer; `frames`, when known, reserves room for `moov` ahead of `mdat`.
	pub fn new(out: W, width: u32, height: u32, fps: u32, codec: MovCodec, frames: Option<usize>) -> Result<Self> {
		let track = Track::new(out, width, height, fps, frames)?;
		Ok(Self { track, codec, sample: Vec::new() })
	}

	/// The writer back after [`FrameSink::finish`] (for in-memory outputs).
	pub fn into_inner(self) -> W {
		self.track.into_inner()
	}

	pub fn codec(&self) -> MovCodec {
		self.codec
	}

	/// Appends one sample encoded by [`MovCodec::encode_into`] for this writer's size.
	pub fn write_sample(&mut self, sample: &[u8]) -> Result<()> {
		self.track.write_sample(sample, true)
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
		if self.track.is_finished() {
			return Err(ExportError::Finished);
		}
		check_size(self.track.width, self.track.height, width, height, rgba.len(), 4)?;
		let mut sample = std::mem::take(&mut self.sample);
		let written =
			self.codec.encode_into(rgba, width, height, &mut sample).and_then(|()| self.write_sample(&sample));
		self.sample = sample;
		written
	}

	fn frames_written(&self) -> usize {
		self.track.samples()
	}

	fn finish(&mut self) -> Result<()> {
		let entry = SampleEntry { format: self.codec.fourcc(), compressor: self.codec.name() };
		self.track.finish(Some(&entry))
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
		check_size(self.writer.track.width, self.writer.track.height, width, height, rgba.len(), 4)?;
		if self.writer.track.is_finished() {
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
		if self.writer.track.is_finished() { self.writer.track.samples() } else { self.lanes.submitted() }
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
