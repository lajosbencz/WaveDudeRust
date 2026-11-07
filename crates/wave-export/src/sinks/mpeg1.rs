//! MPEG-1 video (ISO 11172-2) in an MPEG-1 program stream (ISO 11172-1): I-frames only.
//!
//! Stream layout: per picture a pack header (the first followed by the system header) and the picture's video
//! elementary stream in PES packets of at most 65000 bytes, the first with a PTS 0.1 s after the pack's SCR; at
//! the end the sequence end code in a PES packet and the program end code. Video: a sequence header and a GOP
//! header (closed) every 12 pictures, one slice per macroblock row, every macroblock intra with the slice's
//! quantiser, the default intra matrix, 4:2:0 studio-range BT.601 from [`crate::yuv`], the picture padded to whole
//! macroblocks by edge repetition.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::OnceLock;

use rayon::prelude::*;

use super::{FrameSink, check_size};
use crate::bits::BitWriter;
use crate::yuv::Yuv420;
use crate::{ExportError, Result};

/// The frame rates of `frame_rate_code` 1..=8.
pub const FRAME_RATES: [f64; 8] = [24000.0 / 1001.0, 24.0, 25.0, 30000.0 / 1001.0, 30.0, 50.0, 60000.0 / 1001.0, 60.0];
/// Pictures per GOP.
pub const GOP_LENGTH: usize = 12;
/// Largest PES packet, bytes.
const PES_PACKET_LIMIT: usize = 65000;
/// PTS ahead of the pack's SCR, 0.1 s of the 90 kHz clock: every packet of a picture arrives before it is shown.
const PTS_DELAY: u64 = 9000;
const MAX_WIDTH: u32 = 4095;
/// 175 slice start codes of one row each.
const MAX_HEIGHT: u32 = 175 * 16;

const INTRA_MATRIX: [u8; 64] = [
	8, 16, 19, 22, 26, 27, 29, 34, //
	16, 16, 22, 24, 27, 29, 34, 37, //
	19, 22, 26, 27, 29, 34, 34, 38, //
	22, 22, 26, 27, 29, 34, 37, 40, //
	22, 26, 27, 29, 32, 35, 40, 48, //
	26, 27, 29, 32, 35, 40, 48, 58, //
	26, 27, 29, 34, 38, 46, 56, 69, //
	27, 29, 35, 38, 46, 56, 69, 83,
];

const ZIGZAG: [u8; 64] = [
	0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21,
	28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
	47, 55, 62, 63,
];

/// `dct_dc_size_luminance` and `dct_dc_size_chrominance` codes for sizes 0..=8 as (code, length).
const DC_LUMA: [(u32, u32); 9] = [
	(0b100, 3),
	(0b00, 2),
	(0b01, 2),
	(0b101, 3),
	(0b110, 3),
	(0b1110, 4),
	(0b11110, 5),
	(0b111110, 6),
	(0b1111110, 7),
];
const DC_CHROMA: [(u32, u32); 9] = [
	(0b00, 2),
	(0b01, 2),
	(0b10, 2),
	(0b110, 3),
	(0b1110, 4),
	(0b11110, 5),
	(0b111110, 6),
	(0b1111110, 7),
	(0b11111110, 8),
];

/// `dct_coeff_next` variable-length codes without the sign bit: (run, level, code).
const AC_CODES: [(u8, u8, &str); 62] = [
	(0, 1, "11"),
	(1, 1, "011"),
	(0, 2, "0100"),
	(2, 1, "0101"),
	(0, 3, "00101"),
	(3, 1, "00111"),
	(4, 1, "00110"),
	(1, 2, "000110"),
	(5, 1, "000111"),
	(6, 1, "000101"),
	(7, 1, "000100"),
	(0, 4, "0000110"),
	(2, 2, "0000100"),
	(8, 1, "0000111"),
	(9, 1, "0000101"),
	(0, 5, "00100110"),
	(0, 6, "00100001"),
	(1, 3, "00100101"),
	(3, 2, "00100100"),
	(10, 1, "00100111"),
	(11, 1, "00100011"),
	(12, 1, "00100010"),
	(13, 1, "00100000"),
	(0, 7, "0000001010"),
	(1, 4, "0000001100"),
	(2, 3, "0000001011"),
	(4, 2, "0000001111"),
	(5, 2, "0000001001"),
	(14, 1, "0000001110"),
	(15, 1, "0000001101"),
	(16, 1, "0000001000"),
	(0, 8, "000000011101"),
	(0, 9, "000000011000"),
	(0, 10, "000000010011"),
	(0, 11, "000000010000"),
	(1, 5, "000000011011"),
	(2, 4, "000000010100"),
	(3, 3, "000000011100"),
	(4, 3, "000000010010"),
	(6, 2, "000000011110"),
	(7, 2, "000000010101"),
	(8, 2, "000000010001"),
	(17, 1, "000000011111"),
	(18, 1, "000000011010"),
	(19, 1, "000000011001"),
	(20, 1, "000000010111"),
	(21, 1, "000000010110"),
	(0, 12, "0000000011010"),
	(0, 13, "0000000011001"),
	(0, 14, "0000000011000"),
	(0, 15, "0000000010111"),
	(1, 6, "0000000010110"),
	(1, 7, "0000000010101"),
	(2, 5, "0000000010100"),
	(3, 4, "0000000010011"),
	(5, 3, "0000000010010"),
	(9, 2, "0000000010001"),
	(10, 2, "0000000010000"),
	(22, 1, "0000000011111"),
	(23, 1, "0000000011110"),
	(24, 1, "0000000011101"),
	(25, 1, "0000000011100"),
];

/// `[run][level]` -> (code, length), length 0 meaning the escape.
fn ac_table() -> &'static [[(u32, u32); 16]; 32] {
	static TABLE: OnceLock<[[(u32, u32); 16]; 32]> = OnceLock::new();
	TABLE.get_or_init(|| {
		let mut t = [[(0, 0); 16]; 32];
		for (run, level, code) in AC_CODES {
			let bits = u32::from_str_radix(code, 2).expect("AC_CODES holds binary strings");
			t[run as usize][level as usize] = (bits, code.len() as u32);
		}
		t
	})
}

/// The orthonormal 8-point DCT-II basis, `[u][x]`.
fn dct_basis() -> &'static [[f32; 8]; 8] {
	static BASIS: OnceLock<[[f32; 8]; 8]> = OnceLock::new();
	BASIS.get_or_init(|| {
		let mut b = [[0f32; 8]; 8];
		for (u, row) in b.iter_mut().enumerate() {
			let c = if u == 0 { std::f64::consts::FRAC_1_SQRT_2 } else { 1.0 };
			for (x, v) in row.iter_mut().enumerate() {
				*v = (c / 2.0 * ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0).cos()) as f32;
			}
		}
		b
	})
}

/// The 2-D DCT of an 8 x 8 block, row-major; the DC term is 8 times the block's mean.
fn forward_dct(block: &[f32; 64]) -> [f32; 64] {
	let b = dct_basis();
	let mut rows = [0f32; 64];
	for y in 0..8 {
		for u in 0..8 {
			rows[y * 8 + u] = (0..8).map(|x| b[u][x] * block[y * 8 + x]).sum();
		}
	}
	let mut out = [0f32; 64];
	for v in 0..8 {
		for u in 0..8 {
			out[v * 8 + u] = (0..8).map(|y| b[v][y] * rows[y * 8 + u]).sum();
		}
	}
	out
}

/// The quantiser scale of a quality 1..100: `1 + round_even(30 * ((100 - q) / 99)^1.5)`, 1..31.
pub fn quantiser_scale(quality: u32) -> u32 {
	let t = (100 - quality) as f64 / 99.0;
	1 + (30.0 * t.powf(1.5)).round_ties_even() as u32
}

/// `frame_rate_code` (1..=8) of the table rate nearest to `fps`, the first on a tie.
pub fn frame_rate_code(fps: f64) -> u32 {
	let mut best = 0;
	for (i, r) in FRAME_RATES.iter().enumerate() {
		if (r - fps).abs() < (FRAME_RATES[best] - fps).abs() {
			best = i;
		}
	}
	best as u32 + 1
}

/// An MPEG-1 program stream writer.
pub struct Mpeg1Writer<W: Write> {
	out: W,
	width: u32,
	height: u32,
	rate_code: u32,
	quantiser: u32,
	/// `8 / (quantiser * matrix[i])`, row-major.
	reciprocals: [f32; 64],
	/// `quantiser * matrix[i]`, row-major.
	steps: [i32; 64],
	planes: Yuv420,
	es: BitWriter,
	frames: usize,
	finished: bool,
}

impl Mpeg1Writer<BufWriter<File>> {
	pub fn create(path: &Path, width: u32, height: u32, fps: f64, quality: u32) -> Result<Self> {
		Self::check(width, height)?;
		Self::new(BufWriter::with_capacity(1 << 20, File::create(path)?), width, height, fps, quality)
	}
}

impl<W: Write> Mpeg1Writer<W> {
	fn check(width: u32, height: u32) -> Result<()> {
		if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
			return Err(ExportError::Size { format: "MPEG-1", width, height, limit: "up to 4095 x 2800" });
		}
		Ok(())
	}

	pub fn new(out: W, width: u32, height: u32, fps: f64, quality: u32) -> Result<Self> {
		Self::check(width, height)?;
		let rate_code = frame_rate_code(fps);
		let rate = FRAME_RATES[rate_code as usize - 1];
		if (rate - fps).abs() > 0.01 {
			tracing::warn!(fps, rate, "MPEG-1 takes the nearest frame rate of its table");
		}
		let quantiser = quantiser_scale(quality);
		let reciprocals = std::array::from_fn(|i| 8.0 / (quantiser as f32 * INTRA_MATRIX[i] as f32));
		let steps = std::array::from_fn(|i| quantiser as i32 * INTRA_MATRIX[i] as i32);
		let planes = Yuv420::new(width.div_ceil(16) as usize * 16, height.div_ceil(16) as usize * 16);
		tracing::debug!(width, height, rate, quantiser, "MPEG-1 writer opened");
		Ok(Self {
			out,
			width,
			height,
			rate_code,
			quantiser,
			reciprocals,
			steps,
			planes,
			es: BitWriter::with_capacity(1 << 16),
			frames: 0,
			finished: false,
		})
	}

	pub fn quantiser(&self) -> u32 {
		self.quantiser
	}

	pub fn frame_rate(&self) -> f64 {
		FRAME_RATES[self.rate_code as usize - 1]
	}

	/// The writer back after [`FrameSink::finish`] (for in-memory outputs).
	pub fn into_inner(self) -> W {
		self.out
	}

	fn sequence_header(&mut self) {
		let es = &mut self.es;
		es.write_bytes(&[0, 0, 1, 0xB3]);
		es.write(self.width, 12);
		es.write(self.height, 12);
		es.write(1, 4);
		es.write(self.rate_code, 4);
		es.write(0x3FFFF, 18);
		es.write(1, 1);
		es.write(0x3FF, 10);
		es.write(0, 3);
		es.align_zero();
	}

	fn gop_header(&mut self) {
		let rate = self.frame_rate();
		let seconds = self.frames as f64 / rate;
		let es = &mut self.es;
		es.write_bytes(&[0, 0, 1, 0xB8]);
		es.write(0, 1);
		es.write((seconds / 3600.0) as u32 % 24, 5);
		es.write((seconds / 60.0) as u32 % 60, 6);
		es.write(1, 1);
		es.write(seconds as u32 % 60, 6);
		es.write(((seconds.fract() * rate).round() as u32) % 60, 6);
		es.write(1, 1);
		es.write(0, 1);
		es.align_zero();
	}

	fn picture(&mut self) {
		let es = &mut self.es;
		es.write_bytes(&[0, 0, 1, 0x00]);
		es.write((self.frames % GOP_LENGTH) as u32, 10);
		es.write(1, 3);
		es.write(0xFFFF, 16);
		es.write(0, 1);
		es.align_zero();
		let planes = &self.planes;
		let (quantiser, reciprocals, steps) = (self.quantiser, &self.reciprocals, &self.steps);
		let slices: Vec<BitWriter> = (0..planes.height / 16)
			.into_par_iter()
			.map(|row| encode_slice(planes, row, quantiser, reciprocals, steps))
			.collect();
		for s in &slices {
			self.es.write_bytes(s.bytes());
		}
	}

	fn pack(&mut self, with_system_header: bool, with_pts: bool) -> std::io::Result<()> {
		let clock = (self.frames as f64 * 90000.0 / self.frame_rate()).round_ties_even() as u64;
		let out = &mut self.out;
		let mut header = BitWriter::with_capacity(32);
		header.write_bytes(&[0, 0, 1, 0xBA]);
		header.write(0b0010, 4);
		write_timestamp(&mut header, clock);
		header.write(1, 1);
		header.write(0x3FFFFF, 22);
		header.write(1, 1);
		if with_system_header {
			header.write_bytes(&[0, 0, 1, 0xBB, 0, 9]);
			header.write(1, 1);
			header.write(0x3FFFFF, 22);
			header.write(1, 1);
			header.write(0, 6);
			header.write(0, 4);
			header.write(1, 1);
			header.write(1, 5);
			header.write(0xFF, 8);
			header.write(0xE0, 8);
			header.write(0b11, 2);
			header.write(1, 1);
			header.write(0x1FFF, 13);
		}
		header.align_zero();
		out.write_all(header.bytes())?;
		let mut payload = self.es.bytes();
		let mut first = true;
		while first || !payload.is_empty() {
			let extra = if first && with_pts { 5 } else { 1 };
			let take = payload.len().min(PES_PACKET_LIMIT - 6 - extra);
			let length = (extra + take) as u16;
			out.write_all(&[0, 0, 1, 0xE0, (length >> 8) as u8, length as u8])?;
			if extra == 5 {
				let mut pts = BitWriter::with_capacity(5);
				pts.write(0b0010, 4);
				write_timestamp(&mut pts, clock + PTS_DELAY);
				pts.align_zero();
				out.write_all(pts.bytes())?;
			} else {
				out.write_all(&[0x0F])?;
			}
			out.write_all(&payload[..take])?;
			payload = &payload[take..];
			first = false;
		}
		Ok(())
	}
}

/// `ts[32..30] 1 ts[29..15] 1 ts[14..0] 1`.
fn write_timestamp(bits: &mut BitWriter, ts: u64) {
	bits.write(((ts >> 30) & 7) as u32, 3);
	bits.write(1, 1);
	bits.write(((ts >> 15) & 0x7FFF) as u32, 15);
	bits.write(1, 1);
	bits.write((ts & 0x7FFF) as u32, 15);
	bits.write(1, 1);
}

fn encode_slice(planes: &Yuv420, row: usize, quantiser: u32, reciprocals: &[f32; 64], steps: &[i32; 64]) -> BitWriter {
	let columns = planes.width / 16;
	let mut bits = BitWriter::with_capacity(columns * 128);
	bits.write_bytes(&[0, 0, 1, row as u8 + 1]);
	bits.write(quantiser, 5);
	bits.write(0, 1);
	let mut predictors = [128i32; 3];
	let mut block = [0f32; 64];
	let chroma_width = planes.width / 2;
	for column in 0..columns {
		bits.write(1, 1);
		bits.write(1, 1);
		for b in 0..6 {
			let (plane, stride, x0, y0) = match b {
				0..4 => (&planes.y, planes.width, column * 16 + (b & 1) * 8, row * 16 + (b >> 1) * 8),
				4 => (&planes.cb, chroma_width, column * 8, row * 8),
				_ => (&planes.cr, chroma_width, column * 8, row * 8),
			};
			for y in 0..8 {
				let start = (y0 + y) * stride + x0;
				for (d, s) in block[y * 8..y * 8 + 8].iter_mut().zip(&plane[start..start + 8]) {
					*d = *s as f32;
				}
			}
			let component = b.saturating_sub(3);
			write_block(&mut bits, &forward_dct(&block), reciprocals, steps, component, &mut predictors[component]);
		}
	}
	bits.align_zero();
	bits
}

/// The decoder's intra AC reconstruction of a level: `(2 * level * quantiser * matrix) / 16` truncated, an even
/// result moved one toward zero (mismatch control), clamped to +-2047.
fn reconstruct(level: i32, step: i32) -> i32 {
	let mut v = 2 * level.abs() * step / 16;
	if v & 1 == 0 && v != 0 {
		v -= 1;
	}
	(v * level.signum()).clamp(-2047, 2047)
}

/// The level whose reconstruction lies nearest the coefficient, of the two around `coefficient * reciprocal`.
fn quantise(coefficient: f32, reciprocal: f32, step: i32) -> i32 {
	let raw = (coefficient * reciprocal).clamp(-255.0, 255.0);
	let (lo, hi) = (raw.floor() as i32, raw.ceil() as i32);
	let error = |l: i32| (reconstruct(l, step) as f32 - coefficient).abs();
	if error(hi) < error(lo) { hi } else { lo }
}

fn write_block(
	bits: &mut BitWriter,
	dct: &[f32; 64],
	reciprocals: &[f32; 64],
	quantiser_matrix: &[i32; 64],
	component: usize,
	predictor: &mut i32,
) {
	let dc = (dct[0] / 8.0).round().clamp(0.0, 255.0) as i32;
	let diff = dc - *predictor;
	*predictor = dc;
	let size = 32 - diff.unsigned_abs().leading_zeros();
	let (code, length) = if component == 0 { DC_LUMA[size as usize] } else { DC_CHROMA[size as usize] };
	bits.write(code, length);
	if size > 0 {
		bits.write((if diff < 0 { diff + (1 << size) - 1 } else { diff }) as u32, size);
	}
	let table = ac_table();
	let mut run = 0usize;
	for &z in &ZIGZAG[1..] {
		let z = z as usize;
		let level = quantise(dct[z], reciprocals[z], quantiser_matrix[z]);
		if level == 0 {
			run += 1;
			continue;
		}
		let magnitude = level.unsigned_abs() as usize;
		match table.get(run).and_then(|r| r.get(magnitude)) {
			Some(&(code, length)) if length > 0 => {
				bits.write(code, length);
				bits.write((level < 0) as u32, 1);
			}
			_ => {
				bits.write(0b000001, 6);
				bits.write(run as u32, 6);
				if (-127..=127).contains(&level) {
					bits.write(level as u32 & 0xFF, 8);
				} else {
					bits.write(if level < 0 { 0x80 } else { 0 }, 8);
					bits.write(level as u32 & 0xFF, 8);
				}
			}
		}
		run = 0;
	}
	bits.write(0b10, 2);
}

impl<W: Write + Send> FrameSink for Mpeg1Writer<W> {
	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		if self.finished {
			return Err(ExportError::Finished);
		}
		check_size(self.width, self.height, width, height, rgba.len(), 4)?;
		self.planes.convert(rgba, width as usize, height as usize);
		self.es.clear();
		if self.frames.is_multiple_of(GOP_LENGTH) {
			self.sequence_header();
			self.gop_header();
		}
		self.picture();
		self.pack(self.frames == 0, true)?;
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
		if self.frames > 0 {
			self.es.clear();
			self.es.write_bytes(&[0, 0, 1, 0xB7]);
			self.pack(false, false)?;
		}
		let out = &mut self.out;
		out.write_all(&[0, 0, 1, 0xB9])?;
		out.flush()?;
		self.finished = true;
		Ok(())
	}
}
