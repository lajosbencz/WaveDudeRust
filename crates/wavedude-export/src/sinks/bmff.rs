//! One video track in an ISO base media file (ISO/IEC 14496-12:2015, clauses 8 and 12.1) or a `QuickTime` movie
//! (<https://developer.apple.com/documentation/quicktime-file-format>), the format it derives from.
//!
//! Layout (big-endian): `ftyp`; a `free` box of `1024 + frames *` [`Brand::reserved_per_sample`] bytes when the
//! frame count is known, into which `moov` is written at close (else `moov` follows `mdat`); `mdat` with a 64-bit
//! size; `moov` with timescale = fps, each sample one tick and a chunk of its own, `stss` unless every sample is a
//! sync sample.

use std::io::{Read, Seek, SeekFrom, Write};

use super::counted::Counted;
use crate::{ExportError, Result};

/// The flavour of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Brand {
	/// `QuickTime` (`qt  `): Apple's media and data handlers, `alis` data reference, chunk offsets in `co64`.
	QuickTime,
	/// MP4 with H.264 (`isom`, compatible `isom`, `iso2`, `avc1`, `mp41`): `url ` data reference, chunk offsets in
	/// `stco` while they fit 32 bits.
	#[cfg_attr(not(feature = "mp4-mux"), expect(dead_code, reason = "the MP4 sink's"))]
	Mp4,
}

impl Brand {
	fn name(self) -> &'static str {
		match self {
			Brand::QuickTime => "QuickTime",
			Brand::Mp4 => "MP4",
		}
	}

	fn ftyp(self) -> Vec<u8> {
		let body = match self {
			Brand::QuickTime => Body::default().bytes(b"qt  ").u32(0x2005_0300).bytes(b"qt  "),
			Brand::Mp4 => Body::default().bytes(b"isom").u32(0x200).bytes(b"isomiso2avc1mp41"),
		};
		atom(b"ftyp", &body.0)
	}

	/// `moov` bytes a sample needs at most: `stsz` and `co64`, and `stss` in MP4.
	fn reserved_per_sample(self) -> u64 {
		match self {
			Brand::QuickTime => 12,
			Brand::Mp4 => 16,
		}
	}

	/// `hdlr` of `subtype`: `QuickTime` names the component type and counts the name, ISO zero-terminates it.
	fn hdlr(self, component: &[u8; 4], subtype: &[u8; 4], name: &str) -> Vec<u8> {
		let body = Body::default().u32(0);
		let body = match self {
			Brand::QuickTime => body.bytes(component).bytes(subtype).zeros(12).pascal(name, name.len() + 1),
			Brand::Mp4 => body.u32(0).bytes(subtype).zeros(12).bytes(name.as_bytes()).u8(0),
		};
		atom(b"hdlr", &body.0)
	}
}

/// A sample description: the visual sample entry's format, its compressor name and the codec's boxes after it.
pub(super) struct SampleEntry<'a> {
	pub format: &'a [u8; 4],
	pub compressor: &'a str,
	pub config: &'a [u8],
}

/// One box: size, kind, body.
pub(super) fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
	let mut a = Vec::with_capacity(body.len() + 8);
	a.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
	a.extend_from_slice(kind);
	a.extend_from_slice(body);
	a
}

/// Big-endian field helper.
#[derive(Default)]
pub(super) struct Body(pub(super) Vec<u8>);

impl Body {
	pub(super) fn u8(mut self, v: u8) -> Self {
		self.0.push(v);
		self
	}
	pub(super) fn u16(mut self, v: u16) -> Self {
		self.0.extend_from_slice(&v.to_be_bytes());
		self
	}
	pub(super) fn u32(mut self, v: u32) -> Self {
		self.0.extend_from_slice(&v.to_be_bytes());
		self
	}
	fn u64(mut self, v: u64) -> Self {
		self.0.extend_from_slice(&v.to_be_bytes());
		self
	}
	pub(super) fn bytes(mut self, v: &[u8]) -> Self {
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

/// A file of one video track, its samples written as they come and its index at [`Track::finish`].
pub(super) struct Track<W: Write + Seek> {
	out: Counted<W>,
	brand: Brand,
	pub width: u32,
	pub height: u32,
	fps: u32,
	/// Position and size of the reserved `free` box.
	reserve: Option<(u64, u64)>,
	mdat: u64,
	/// (offset, size) per sample.
	samples: Vec<(u64, u32)>,
	/// Numbers of the sync samples, from 1.
	sync: Vec<u32>,
	finished: bool,
}

impl<W: Write + Seek> Track<W> {
	/// Writes the header; `frames`, when known, reserves room for `moov` ahead of `mdat`.
	pub(super) fn new(out: W, brand: Brand, width: u32, height: u32, fps: u32, frames: Option<usize>) -> Result<Self> {
		if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
			return Err(ExportError::Size { format: brand.name(), width, height, limit: "up to 65535 x 65535" });
		}
		let mut out = Counted::new(out)?;
		out.write_all(&brand.ftyp())?;
		let reserve = match frames {
			Some(n) => {
				let pos = out.stream_position()?;
				let size = 1024 + brand.reserved_per_sample() * n as u64;
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
			brand,
			width,
			height,
			fps,
			reserve,
			mdat,
			samples: Vec::new(),
			sync: Vec::new(),
			finished: false,
		})
	}

	pub(super) fn samples(&self) -> usize {
		self.samples.len()
	}

	pub(super) fn is_finished(&self) -> bool {
		self.finished
	}

	/// Appends a sample; `sync` marks a sync sample.
	pub(super) fn write_sample(&mut self, sample: &[u8], sync: bool) -> Result<()> {
		if self.finished {
			return Err(ExportError::Finished);
		}
		let size = u32::try_from(sample.len()).map_err(|_| ExportError::Size {
			format: self.brand.name(),
			width: self.width,
			height: self.height,
			limit: "a sample under 4 GiB",
		})?;
		let offset = self.out.stream_position()?;
		self.out.write_all(sample)?;
		self.samples.push((offset, size));
		if sync {
			self.sync.push(self.samples.len() as u32);
		}
		Ok(())
	}

	fn moov(&self, entry: Option<&SampleEntry<'_>>) -> Vec<u8> {
		let brand = self.brand;
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
		let mvhd = atom(b"mvhd", &mvhd.0);
		let Some(entry) = entry else {
			return atom(b"moov", &mvhd);
		};
		// QuickTime: enabled, in movie, preview and poster; ISO: enabled, in movie and preview (0x8 is
		// track_size_is_aspect_ratio there)
		let flags = if brand == Brand::QuickTime { 0xF } else { 0x7 };
		let tkhd = Body::default()
			.u32(flags)
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
		// QuickTime's Macintosh language code 0, else ISO 639-2/T "und" packed in 5-bit letters
		let language = if brand == Brand::QuickTime { 0 } else { 0x55C4 };
		let mdhd = Body::default().u32(0).u32(0).u32(0).u32(self.fps).u32(n).u16(language).u16(0);
		let (vmhd, data_handler, dref) = match brand {
			Brand::QuickTime => (
				Body::default().u32(1).u16(0x40).u16(0x8000).u16(0x8000).u16(0x8000),
				brand.hdlr(b"dhlr", b"alis", "DataHandler"),
				atom(b"alis", &Body::default().u32(1).0),
			),
			Brand::Mp4 => (Body::default().u32(1).zeros(8), Vec::new(), atom(b"url ", &Body::default().u32(1).0)),
		};
		let dref = Body::default().u32(0).u32(1).bytes(&dref);
		// QuickTime's spatial quality codecMaxQuality (ImageCompression.h), ISO's pre_defined 0
		let quality = if brand == Brand::QuickTime { 0x3FF } else { 0 };
		let visual = Body::default()
			.zeros(6)
			.u16(1)
			.u16(0)
			.u16(0)
			.u32(0)
			.u32(0)
			.u32(quality)
			.u16(w as u16)
			.u16(h as u16)
			.u32(0x0048_0000)
			.u32(0x0048_0000)
			.u32(0)
			.u16(1)
			.pascal(entry.compressor, 32)
			.u16(24)
			.u16(0xFFFF)
			.bytes(entry.config);
		let stsd = Body::default().u32(0).u32(1).bytes(&atom(entry.format, &visual.0));
		let stts = Body::default().u32(0).u32(1).u32(n).u32(1);
		let stss = if self.sync.len() < self.samples.len() {
			let stss = Body::default().u32(0).u32(self.sync.len() as u32);
			atom(b"stss", &self.sync.iter().fold(stss, |b, &s| b.u32(s)).0)
		} else {
			Vec::new()
		};
		let stsc = Body::default().u32(0).u32(1).u32(1).u32(1).u32(1);
		let stsz = self.samples.iter().fold(Body::default().u32(0).u32(0).u32(n), |b, s| b.u32(s.1));
		let wide = brand == Brand::QuickTime || self.samples.last().is_some_and(|s| s.0 > u32::MAX as u64);
		let offsets = Body::default().u32(0).u32(n);
		let offsets = if wide {
			atom(b"co64", &self.samples.iter().fold(offsets, |b, s| b.u64(s.0)).0)
		} else {
			atom(b"stco", &self.samples.iter().fold(offsets, |b, s| b.u32(s.0 as u32)).0)
		};
		let stbl = [
			atom(b"stsd", &stsd.0),
			atom(b"stts", &stts.0),
			stss,
			atom(b"stsc", &stsc.0),
			atom(b"stsz", &stsz.0),
			offsets,
		]
		.concat();
		let minf = [atom(b"vmhd", &vmhd.0), data_handler, atom(b"dinf", &atom(b"dref", &dref.0)), atom(b"stbl", &stbl)]
			.concat();
		let mdia =
			[atom(b"mdhd", &mdhd.0), brand.hdlr(b"mhlr", b"vide", "VideoHandler"), atom(b"minf", &minf)].concat();
		let trak = [atom(b"tkhd", &tkhd.0), atom(b"mdia", &mdia)].concat();
		atom(b"moov", &[mvhd, atom(b"trak", &trak)].concat())
	}

	/// Writes the index (without `entry`, there being no sample to describe, a `moov` of no track) and flushes the
	/// output; later calls do nothing.
	pub(super) fn finish(&mut self, entry: Option<&SampleEntry<'_>>) -> Result<()> {
		if self.finished {
			return Ok(());
		}
		self.finished = true;
		let end = self.out.stream_position()?;
		self.out.seek(SeekFrom::Start(self.mdat + 8))?;
		self.out.write_all(&(end - self.mdat).to_be_bytes())?;
		let moov = self.moov(entry);
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

	/// The writer back after [`Track::finish`] (for in-memory outputs).
	pub(super) fn into_inner(self) -> W {
		self.out.into_inner()
	}
}

/// Box reading for the tests.
#[cfg(test)]
pub(crate) mod read {
	/// The boxes of `data[from..to]`: (kind, start of body, end).
	pub(crate) fn boxes(data: &[u8], from: usize, to: usize) -> Vec<([u8; 4], usize, usize)> {
		let mut out = Vec::new();
		let mut i = from;
		while i + 8 <= to {
			let size = u32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
			let kind: [u8; 4] = data[i + 4..i + 8].try_into().unwrap();
			let (body, end) = if size == 1 {
				(i + 16, i + u64::from_be_bytes(data[i + 8..i + 16].try_into().unwrap()) as usize)
			} else {
				(i + 8, i + size)
			};
			out.push((kind, body, end));
			i = end;
		}
		out
	}

	/// The body of the box at `path`.
	pub(crate) fn find<'a>(data: &'a [u8], path: &[&[u8; 4]]) -> Option<&'a [u8]> {
		let (mut from, mut to) = (0, data.len());
		for kind in path {
			let (_, b, e) = boxes(data, from, to).into_iter().find(|a| &a.0 == *kind)?;
			(from, to) = (b, e);
		}
		Some(&data[from..to])
	}

	pub(crate) fn u32_at(body: &[u8], at: usize) -> u32 {
		u32::from_be_bytes(body[at..at + 4].try_into().unwrap())
	}

	/// The 32-bit entries of a full box's table (after version, flags and `skip` more fields).
	pub(crate) fn table(body: &[u8], skip: usize) -> Vec<u32> {
		let n = u32_at(body, 4 + 4 * skip) as usize;
		(0..n).map(|k| u32_at(body, 8 + 4 * skip + 4 * k)).collect()
	}

	const STBL: [&[u8; 4]; 5] = [b"moov", b"trak", b"mdia", b"minf", b"stbl"];

	/// The body of `kind` in the sample table.
	pub(crate) fn stbl<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
		find(data, &[STBL.as_slice(), &[kind]].concat())
	}

	/// Each sample's bytes and whether it is a sync sample.
	pub(crate) fn samples(data: &[u8]) -> Vec<(&[u8], bool)> {
		let sizes = table(stbl(data, b"stsz").unwrap(), 1);
		let offsets: Vec<u64> = match stbl(data, b"stco") {
			Some(stco) => table(stco, 0).into_iter().map(u64::from).collect(),
			None => {
				let co64 = stbl(data, b"co64").unwrap();
				(0..u32_at(co64, 4) as usize)
					.map(|k| u64::from_be_bytes(co64[8 + 8 * k..16 + 8 * k].try_into().unwrap()))
					.collect()
			}
		};
		let sync = stbl(data, b"stss").map(|s| table(s, 0));
		assert_eq!(sizes.len(), offsets.len(), "a chunk a sample");
		(0..sizes.len())
			.map(|k| {
				let at = offsets[k] as usize;
				let is_sync = sync.as_ref().is_none_or(|s| s.contains(&(k as u32 + 1)));
				(&data[at..at + sizes[k] as usize], is_sync)
			})
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use std::io::Cursor;

	use super::read::*;
	use super::*;

	fn track(brand: Brand, frames: Option<usize>, sync: &[bool]) -> Vec<u8> {
		let mut t = Track::new(Cursor::new(Vec::new()), brand, 6, 4, 25, frames).unwrap();
		for (k, &s) in sync.iter().enumerate() {
			t.write_sample(&[k as u8; 3], s).unwrap();
		}
		t.finish(Some(&SampleEntry { format: b"test", compressor: "", config: &[] })).unwrap();
		t.into_inner().into_inner()
	}

	fn top(data: &[u8]) -> Vec<[u8; 4]> {
		boxes(data, 0, data.len()).into_iter().map(|b| b.0).collect()
	}

	#[test]
	fn samples_index_in_order_with_their_sync_flags() {
		let sync = [true, false, false, true, false];
		for brand in [Brand::QuickTime, Brand::Mp4] {
			let data = track(brand, Some(sync.len()), &sync);
			assert_eq!(top(&data), [*b"ftyp", *b"moov", *b"free", *b"mdat"], "{brand:?}: moov in the reserved room");
			let samples = samples(&data);
			assert_eq!(samples.len(), sync.len());
			for (k, (bytes, s)) in samples.into_iter().enumerate() {
				assert_eq!((bytes, s), (&[k as u8; 3][..], sync[k]), "{brand:?} sample {k}");
			}
			let stts = stbl(&data, b"stts").unwrap();
			assert_eq!((u32_at(stts, 4), u32_at(stts, 8), u32_at(stts, 12)), (1, 5, 1), "one tick a sample");
		}
	}

	#[test]
	fn brands_differ_in_the_tables_they_choose() {
		let all = track(Brand::Mp4, None, &[true, true]);
		assert_eq!(top(&all), [*b"ftyp", *b"mdat", *b"moov"]);
		assert!(stbl(&all, b"stss").is_none(), "every sample a sync sample: no stss");
		assert!(stbl(&all, b"stco").is_some(), "32-bit offsets in MP4 while they fit");
		let mov = track(Brand::QuickTime, None, &[true, true]);
		assert!(stbl(&mov, b"co64").is_some(), "QuickTime keeps co64");
		let ftyp = find(&all, &[b"ftyp"]).unwrap();
		assert_eq!(ftyp, b"isom\0\0\x02\0isomiso2avc1mp41");
	}

	#[test]
	fn a_track_without_a_sample_entry_has_no_trak() {
		let mut t = Track::new(Cursor::new(Vec::new()), Brand::Mp4, 6, 4, 25, Some(3)).unwrap();
		t.finish(None).unwrap();
		let data = t.into_inner().into_inner();
		let moov = find(&data, &[b"moov"]).unwrap();
		assert_eq!(boxes(moov, 0, moov.len()).into_iter().map(|b| b.0).collect::<Vec<_>>(), [*b"mvhd"]);
	}
}
