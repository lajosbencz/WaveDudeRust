//! H.264 in MP4: an [`H264Encoder`]'s access units as one `avc1` track (ISO/IEC 14496-15 5.4) of the ISO base media
//! writer of `sinks::bmff` (timescale = fps, one tick per sample, the SPS and PPS of the first access unit in `avcC`,
//! `stss` for the IDR pictures).
//!
//! Input: 4:2:0 studio-range BT.601 from `yuv`; an odd size is padded to even by repeating the last column
//! and row, and the video has the padded size.

use std::io::{Seek, Write};
use std::task::Poll;

use wavedude_core::sync::MaybeSend;

use super::bmff::{Body, Brand, SampleEntry, Track, atom};
use super::{FrameSink, SinkSpec, check_size};
use crate::h264::{EncoderSpec, H264Encoder, H264Factory, bitrate};
use crate::yuv::Yuv420;
use crate::{ExportError, Result};

/// The NAL units of an Annex B stream, start codes removed.
fn nal_units(stream: &[u8]) -> Vec<&[u8]> {
	let mut starts = Vec::new();
	let mut i = 0;
	while i + 3 <= stream.len() {
		if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
			starts.push(i + 3);
			i += 3;
		} else {
			i += 1;
		}
	}
	starts
		.iter()
		.enumerate()
		.map(|(k, &s)| {
			let mut end = starts.get(k + 1).map_or(stream.len(), |&n| n - 3);
			while end > s && stream[end - 1] == 0 {
				end -= 1;
			}
			&stream[s..end]
		})
		.filter(|n| !n.is_empty())
		.collect()
}

/// The RBSP of a NAL unit's payload: emulation prevention bytes (`00 00 03`) removed.
fn rbsp(payload: &[u8]) -> Vec<u8> {
	let mut out = Vec::with_capacity(payload.len());
	let mut zeros = 0;
	for &b in payload {
		if zeros >= 2 && b == 3 {
			zeros = 0;
			continue;
		}
		zeros = if b == 0 { zeros + 1 } else { 0 };
		out.push(b);
	}
	out
}

/// Exp-Golomb reading (ITU-T H.264 9.1), most significant bit first.
struct Bits<'a> {
	bytes: &'a [u8],
	at: usize,
}

impl Bits<'_> {
	fn bit(&mut self) -> Option<u32> {
		let byte = *self.bytes.get(self.at / 8)?;
		let bit = (byte >> (7 - self.at % 8)) & 1;
		self.at += 1;
		Some(u32::from(bit))
	}

	fn golomb(&mut self) -> Option<u32> {
		let mut zeros = 0;
		while self.bit()? == 0 {
			zeros += 1;
			if zeros > 31 {
				return None;
			}
		}
		let mut value = 0u64;
		for _ in 0..zeros {
			value = value << 1 | u64::from(self.bit()?);
		}
		u32::try_from((1u64 << zeros) - 1 + value).ok()
	}
}

/// `chroma_format_idc`, `bit_depth_luma_minus8` and `bit_depth_chroma_minus8` of the SPS NAL unit `sps` (ITU-T
/// H.264 7.3.2.1.1, plus 144 of its 2005 edition, which `avcC` still lists; the defaults 1, 0, 0 for profiles that
/// do not code them).
fn sps_format(sps: &[u8]) -> Option<(u8, u8, u8)> {
	let profile = *sps.get(1)?;
	if !matches!(profile, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135 | 144) {
		return Some((1, 0, 0));
	}
	let payload = rbsp(sps.get(4..)?);
	let mut bits = Bits { bytes: &payload, at: 0 };
	bits.golomb()?; // seq_parameter_set_id
	let chroma = bits.golomb()?;
	if chroma == 3 {
		bits.bit()?; // separate_colour_plane_flag
	}
	let (luma, chroma_depth) = (bits.golomb()?, bits.golomb()?);
	Some((u8::try_from(chroma).ok()?, u8::try_from(luma).ok()?, u8::try_from(chroma_depth).ok()?))
}

/// The `avcC` box (`AVCDecoderConfigurationRecord`, ISO/IEC 14496-15 5.3.3.1) of the parameter sets `sps` and
/// `pps` (NAL units), NAL unit lengths in 4 bytes.
fn avc_config(sps: &[&[u8]], pps: &[&[u8]]) -> Result<Vec<u8>> {
	let invalid = |what: &str| ExportError::H264(format!("the encoder's parameter sets are unusable: {what}"));
	let first = match sps.first() {
		Some(first) if first.len() >= 4 && !pps.is_empty() => first,
		_ => return Err(ExportError::H264("the first frame carries no SPS and PPS".into())),
	};
	if sps.len() > 31 || pps.len() > 255 || sps.iter().chain(pps).any(|n| n.len() > u16::MAX as usize) {
		return Err(invalid("too many or too long"));
	}
	let sets = |body: Body, sets: &[&[u8]]| sets.iter().fold(body, |b, s| b.u16(s.len() as u16).bytes(s));
	// configurationVersion, profile, compatibility and level from the SPS, lengthSizeMinusOne 3
	let body = Body::default().u8(1).bytes(&first[1..4]).u8(0xFC | 3).u8(0xE0 | sps.len() as u8);
	let mut body = sets(sets(body, sps).u8(pps.len() as u8), pps);
	// the profiles of 14496-15 5.3.3.1.2 whose record carries the format fields
	if matches!(first[1], 100 | 110 | 122 | 144) {
		let (chroma, luma, chroma_depth) = sps_format(first).ok_or_else(|| invalid("the SPS is truncated"))?;
		// chroma_format, bit depths less 8, under reserved 1 bits; no SPS extensions
		body = body.u8(0xFC | chroma).u8(0xF8 | luma).u8(0xF8 | chroma_depth).u8(0);
	}
	Ok(atom(b"avcC", &body.0))
}

/// The `avc1` track of an MP4, one sample per access unit, the parameter sets of the first in `avcC`.
struct Muxer<W: Write + Seek> {
	track: Track<W>,
	/// The first access unit's SPS and PPS NAL units and the `avcC` box made of them.
	config: Option<(Vec<Vec<u8>>, Vec<u8>)>,
	/// The last sample, its allocation reused.
	sample: Vec<u8>,
}

impl<W: Write + Seek> Muxer<W> {
	/// Adds the access unit `stream` (Annex B) as the next sample, its NAL units length-prefixed; parameter sets go
	/// to `avcC`, and a later one unlike the first fails, `avc1` holding one set for the whole track.
	fn write(&mut self, stream: &[u8], sync: bool) -> Result<()> {
		let (mut sps, mut pps) = (Vec::new(), Vec::new());
		self.sample.clear();
		for nal in nal_units(stream) {
			match nal[0] & 0x1F {
				kind @ (7 | 8) => {
					if let Some((sets, _)) = &self.config {
						if !sets.iter().any(|s| s == nal) {
							let name = if kind == 7 { "SPS" } else { "PPS" };
							return Err(ExportError::H264(format!(
								"the encoder changed its {name} in sample {}; an avc1 track holds one",
								self.track.samples() + 1
							)));
						}
					} else if kind == 7 {
						sps.push(nal);
					} else {
						pps.push(nal);
					}
				}
				_ => {
					self.sample.extend_from_slice(&(nal.len() as u32).to_be_bytes());
					self.sample.extend_from_slice(nal);
				}
			}
		}
		if self.config.is_none() {
			let avcc = avc_config(&sps, &pps)?;
			self.config = Some((sps.iter().chain(&pps).map(|n| n.to_vec()).collect(), avcc));
		}
		self.track.write_sample(&self.sample, sync)
	}

	fn finish(&mut self) -> Result<()> {
		let entry = self.config.as_ref().map(|(_, config)| SampleEntry { format: b"avc1", compressor: "", config });
		self.track.finish(entry.as_ref())
	}
}

/// An MP4 sink: frames converted to 4:2:0, encoded by an [`H264Encoder`] and muxed as they come out of it.
pub struct Mp4Sink<W: Write + Seek> {
	muxer: Muxer<W>,
	encoder: Box<dyn H264Encoder>,
	planes: Yuv420,
	/// Canvas pixels.
	width: u32,
	height: u32,
	frames: usize,
	flushing: bool,
	finished: bool,
}

impl<W: Write + Seek> Mp4Sink<W> {
	/// An MP4 of `spec` written to `out`, encoded by an encoder of `h264` in `spec.profile`, or in the best profile
	/// `h264` has when it lacks that one.
	pub fn new(out: W, spec: SinkSpec, h264: &dyn H264Factory) -> Result<Self> {
		let profile = spec
			.profile
			.or_best_of(h264.profiles())
			.ok_or_else(|| ExportError::H264("the encoder offers no profile".into()))?;
		if profile != spec.profile {
			tracing::warn!(asked = ?spec.profile, ?profile, "the H.264 encoder lacks the profile asked for");
		}
		let planes = Yuv420::new(spec.width.next_multiple_of(2) as usize, spec.height.next_multiple_of(2) as usize);
		let (width, height) = (planes.width as u32, planes.height as u32);
		let bps = bitrate(spec.width, spec.height, spec.fps, spec.quality);
		let encoder = h264.open(&EncoderSpec { width, height, fps: spec.fps, bitrate: bps, profile })?;
		let track = Track::new(out, Brand::Mp4, width, height, spec.fps, Some(spec.frame_count))?;
		let muxer = Muxer { track, config: None, sample: Vec::new() };
		tracing::debug!(width = spec.width, height = spec.height, fps = spec.fps, bps, ?profile, "MP4 writer opened");
		Ok(Self {
			muxer,
			encoder,
			planes,
			width: spec.width,
			height: spec.height,
			frames: 0,
			flushing: false,
			finished: false,
		})
	}

	/// Muxes what the encoder has encoded.
	fn drain(&mut self) -> Result<()> {
		let muxer = &mut self.muxer;
		self.encoder.drain(&mut |stream, sync| muxer.write(stream, sync))
	}
}

impl<W: Write + Seek + MaybeSend> FrameSink for Mp4Sink<W> {
	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		if self.finished || self.flushing {
			return Err(ExportError::Finished);
		}
		check_size(self.width, self.height, width, height, rgba.len(), 4)?;
		self.planes.convert(rgba, width as usize, height as usize);
		self.encoder.submit(&self.planes)?;
		self.frames += 1;
		self.drain()
	}

	fn frames_written(&self) -> usize {
		self.frames
	}

	fn poll_ready(&mut self) -> Result<Poll<()>> {
		let ready = self.encoder.poll_ready()?;
		self.drain()?;
		Ok(ready)
	}

	fn poll_finish(&mut self) -> Result<Poll<()>> {
		if !self.flushing {
			self.encoder.start_flush()?;
			self.flushing = true;
		}
		let flushed = self.encoder.poll_flushed()?;
		self.drain()?;
		Ok(flushed)
	}

	fn finish(&mut self) -> Result<()> {
		if self.finished {
			return Ok(());
		}
		self.finished = true;
		self.drain()?;
		self.muxer.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::bits::BitWriter;

	/// An SPS NAL unit of `profile` whose coded fields are the Exp-Golomb `fields` (from `seq_parameter_set_id`).
	fn sps(profile: u8, fields: &[u32]) -> Vec<u8> {
		let mut bits = BitWriter::default();
		bits.write_bytes(&[0x67, profile, 0, 40]);
		for &v in fields {
			let len = 32 - (v + 1).leading_zeros();
			bits.write(v + 1, 2 * len - 1);
		}
		bits.write(1, 1);
		bits.align_zero();
		bits.bytes().to_vec()
	}

	/// The `avcC` body after the parameter sets.
	fn extension(sps: &[u8], pps: &[u8]) -> Vec<u8> {
		let avcc = avc_config(&[sps], &[pps]).unwrap();
		assert_eq!(&avcc[4..8], b"avcC");
		assert_eq!(&avcc[8..12], &[1, sps[1], sps[2], sps[3]], "version, profile, compatibility, level");
		assert_eq!(&avcc[12..14], &[0xFF, 0xE1], "4-byte lengths, one SPS");
		avcc[14 + 2 + sps.len() + 1 + 2 + pps.len()..].to_vec()
	}

	#[test]
	fn avcc_declares_the_chroma_format_and_bit_depths_of_high_profiles() {
		let pps = [0x68, 0xCE, 0x3C, 0x80];
		assert_eq!(extension(&sps(66, &[0]), &pps), [] as [u8; 0], "Baseline: no extension");
		assert_eq!(extension(&sps(100, &[0, 1, 0, 0]), &pps), [0xFD, 0xF8, 0xF8, 0], "High: 4:2:0, 8-bit");
		assert_eq!(extension(&sps(122, &[3, 2, 2, 2]), &pps), [0xFE, 0xFA, 0xFA, 0], "High 4:2:2: 10-bit");
		assert!(avc_config(&[&sps(100, &[0])[..5]], &[&pps]).is_err(), "a truncated SPS");
		assert!(avc_config(&[], &[&pps]).is_err(), "no SPS");
	}

	#[test]
	fn a_parameter_set_unlike_the_first_fails_the_track() {
		let track = Track::new(std::io::Cursor::new(Vec::new()), Brand::Mp4, 16, 16, 2, None).unwrap();
		let mut muxer = Muxer { track, config: None, sample: Vec::new() };
		let unit = |sps: &[u8], pps: &[u8], slice: u8| {
			[&[0, 0, 0, 1][..], sps, &[0, 0, 0, 1], pps, &[0, 0, 0, 1, slice, 0x80]].concat()
		};
		let (first, pps) = (sps(66, &[0]), [0x68, 0xCE, 0x3C, 0x80]);
		muxer.write(&unit(&first, &pps, 0x65), true).unwrap();
		muxer.write(&unit(&first, &pps, 0x65), true).expect("the same parameter sets repeated");
		let error = muxer.write(&unit(&first, &[0x68, 0xCE, 0x38, 0x80], 0x65), true).unwrap_err();
		assert!(error.to_string().contains("PPS in sample 3"), "{error}");
		let error = muxer.write(&unit(&sps(66, &[1]), &pps, 0x65), true).unwrap_err();
		assert!(error.to_string().contains("SPS in sample 3"), "{error}");
		assert_eq!(muxer.track.samples(), 2, "nothing written after the change");
	}

	#[test]
	fn rbsp_drops_emulation_prevention_bytes() {
		assert_eq!(rbsp(&[0, 0, 3, 1, 0, 0, 3, 0, 0, 3]), [0, 0, 1, 0, 0, 0, 0]);
		assert_eq!(rbsp(&[0, 3, 0, 0, 0, 3]), [0, 3, 0, 0, 0]);
	}
}
