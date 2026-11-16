//! H.264 in MP4: an [`H264Encoder`]'s access units as one `avc1` track (ISO/IEC 14496-15 5.4) muxed by
//! `shiguredo_mp4`'s `Mp4FileMuxer`: timescale = fps, one tick per sample, samples in decode order with their
//! composition offsets in `ctts` when the encoder reorders pictures, the SPS and PPS of the first access unit in
//! `avcC`, `stss` for the IDR pictures. Samples are written as they come; `moov` goes into the room reserved for the
//! frame count ahead of `mdat`, after `mdat` when it does not fit.
//!
//! Input: 4:2:0 studio-range BT.601 from `yuv`; an odd size is padded to even by repeating the last column
//! and row, and the video has the padded size.

use std::io::{Seek, SeekFrom, Write};
use std::num::NonZeroU32;
use std::task::Poll;

use shiguredo_mp4::TrackKind;
use shiguredo_mp4::bitstream::h264::{self, H264NalUnitType, H264SampleEntryConfig, LengthSize};
use shiguredo_mp4::boxes::SampleEntry;
use shiguredo_mp4::mux::{Mp4FileMuxer, Mp4FileMuxerOptions, MuxError, Sample, estimate_maximum_moov_box_size};
use wavedude_core::sync::MaybeSend;

use super::{FrameSink, SinkSpec, check_size};
use crate::h264::{AccessUnit, EncoderSpec, H264Encoder, H264Factory, bitrate};
use crate::yuv::Yuv420;
use crate::{ExportError, Result};

/// `e` in the export's words: the crate's reason for a box it cannot encode; the rest break the sink's invariants.
fn mux_error(e: MuxError) -> ExportError {
	ExportError::Mp4(match e {
		MuxError::EncodeError(e) => e.reason,
		MuxError::NoSyncSamples { .. } => "the video has no key frame".into(),
		MuxError::Overflow => "the video has too many frames for MP4".into(),
		other => format!("the muxer refused a sample: {other:?}"),
	})
}

/// The `avc1` track of an MP4 written to `out` from its start, one sample per access unit.
struct Muxer<W: Write + Seek> {
	out: W,
	mux: Mp4FileMuxer,
	timescale: NonZeroU32,
	/// Where the next sample goes.
	position: u64,
	/// The first access unit's SPS and PPS NAL units.
	sets: Option<Vec<Vec<u8>>>,
	/// Samples written.
	samples: u64,
	/// The last sample, its allocation reused.
	sample: Vec<u8>,
}

impl<W: Write + Seek> Muxer<W> {
	/// Writes the header, with room for the `moov` of `frames` samples.
	fn new(mut out: W, fps: u32, frames: usize) -> Result<Self> {
		let timescale = NonZeroU32::new(fps).ok_or_else(|| ExportError::Mp4("the frame rate is 0".into()))?;
		let options = Mp4FileMuxerOptions {
			reserved_moov_box_size: estimate_maximum_moov_box_size(&[frames]),
			..Mp4FileMuxerOptions::default()
		};
		let mux = Mp4FileMuxer::with_options(options).map_err(mux_error)?;
		out.write_all(mux.initial_boxes_bytes())?;
		let position = mux.initial_boxes_bytes().len() as u64;
		Ok(Self { out, mux, timescale, position, sets: None, samples: 0, sample: Vec::new() })
	}

	/// Adds `unit` as the next sample, its NAL units length-prefixed and its composition offset the distance of its
	/// picture from its place in decode order; parameter sets go to `avcC`, and a later one unlike the first fails,
	/// `avc1` holding one set for the whole track.
	fn write(&mut self, unit: AccessUnit<'_>) -> Result<()> {
		let nals = h264::parse_annexb_nal_units(unit.stream)
			.map_err(|e| ExportError::H264(format!("the encoder's access unit is not Annex B: {}", e.reason)))?;
		let (mut sps, mut pps) = (Vec::new(), Vec::new());
		self.sample.clear();
		for nal in nals {
			match nal.nal_unit_type {
				kind @ (H264NalUnitType::Sps | H264NalUnitType::Pps) => {
					if let Some(sets) = &self.sets {
						if !sets.iter().any(|s| s == nal.data) {
							let name = if kind == H264NalUnitType::Sps { "SPS" } else { "PPS" };
							return Err(ExportError::H264(format!(
								"the encoder changed its {name} in sample {}; an avc1 track holds one",
								self.samples + 1
							)));
						}
					} else if kind == H264NalUnitType::Sps {
						sps.push(nal.data.to_vec());
					} else {
						pps.push(nal.data.to_vec());
					}
				}
				_ => {
					self.sample.extend_from_slice(&(nal.data.len() as u32).to_be_bytes());
					self.sample.extend_from_slice(nal.data);
				}
			}
		}
		let entry = match self.sets {
			Some(_) => None,
			None => {
				let config = H264SampleEntryConfig { length_size: LengthSize::FourBytes };
				let avc1 = h264::build_avc1_box(&sps, &pps, &config).map_err(|e| {
					ExportError::H264(format!("the encoder's parameter sets are unusable: {}", e.reason))
				})?;
				self.sets = Some([sps, pps].concat());
				Some(SampleEntry::Avc1(avc1))
			}
		};
		self.out.write_all(&self.sample)?;
		let offset = unit.picture as i64 - self.samples as i64;
		let sample = Sample {
			track_kind: TrackKind::Video,
			sample_entry: entry,
			keyframe: unit.sync,
			timescale: self.timescale,
			duration: 1,
			composition_time_offset: (offset != 0).then_some(offset),
			data_offset: self.position,
			data_size: self.sample.len(),
		};
		self.mux.append_sample(&sample).map_err(mux_error)?;
		self.position += self.sample.len() as u64;
		self.samples += 1;
		Ok(())
	}

	/// Writes the index and the final header, and flushes the output.
	fn finish(&mut self) -> Result<()> {
		let finalized = self.mux.finalize().map_err(mux_error)?;
		for (offset, bytes) in finalized.offset_and_bytes_pairs() {
			self.out.seek(SeekFrom::Start(offset))?;
			self.out.write_all(bytes)?;
		}
		self.out.flush()?;
		Ok(())
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
		let muxer = Muxer::new(out, spec.fps, spec.frame_count)?;
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
		self.encoder.drain(&mut |unit| muxer.write(unit))
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
pub(crate) mod tests {
	use std::io::Cursor;

	use super::*;
	use crate::bits::BitWriter;
	use crate::sinks::bmff::read::{boxes, find, stbl, u32_at};

	/// A Constrained Baseline SPS NAL unit of a `width x height` picture (both even), cropped from whole
	/// macroblocks.
	pub(crate) fn sps(width: u32, height: u32) -> Vec<u8> {
		fn golomb(bits: &mut BitWriter, v: u32) {
			let len = 32 - (v + 1).leading_zeros();
			bits.write(v + 1, 2 * len - 1);
		}
		let mut bits = BitWriter::default();
		bits.write_bytes(&[0x67, 66, 0xC0, 30]);
		// seq_parameter_set_id, log2_max_frame_num_minus4, pic_order_cnt_type, log2_max_pic_order_cnt_lsb_minus4,
		// max_num_ref_frames
		for v in [0, 0, 0, 0, 1] {
			golomb(&mut bits, v);
		}
		bits.write(0, 1);
		let (mbs_w, mbs_h) = (width.div_ceil(16), height.div_ceil(16));
		golomb(&mut bits, mbs_w - 1);
		golomb(&mut bits, mbs_h - 1);
		// frame_mbs_only_flag, direct_8x8_inference_flag, frame_cropping_flag
		bits.write(0b111, 3);
		for v in [0, (mbs_w * 16 - width) / 2, 0, (mbs_h * 16 - height) / 2] {
			golomb(&mut bits, v);
		}
		// no VUI, the stop bit
		bits.write(0b01, 2);
		bits.align_zero();
		bits.bytes().to_vec()
	}

	const PPS: [u8; 4] = [0x68, 0xCE, 0x3C, 0x80];

	/// An access unit of one slice NAL unit of `kind` and `tag`, after `sets`.
	fn unit(sets: &[&[u8]], kind: u8, tag: u8) -> Vec<u8> {
		let mut unit: Vec<u8> = sets.iter().flat_map(|s| [&[0, 0, 0, 1][..], s].concat()).collect();
		unit.extend_from_slice(&[0, 0, 0, 1, kind, tag, 0x80]);
		unit
	}

	/// An MP4 of the access units of `pictures`, in decode order, room reserved for `frames`; the first a sync
	/// sample.
	fn mp4(pictures: &[u64], frames: usize) -> Vec<u8> {
		let mut muxer = Muxer::new(Cursor::new(Vec::new()), 25, frames).unwrap();
		let sps = sps(32, 18);
		for (k, &picture) in pictures.iter().enumerate() {
			let stream = if k == 0 { unit(&[&sps, &PPS], 0x65, 0) } else { unit(&[], 0x41, picture as u8) };
			muxer.write(AccessUnit { stream: &stream, sync: k == 0, picture }).unwrap();
		}
		muxer.finish().unwrap();
		muxer.out.into_inner()
	}

	fn top(data: &[u8]) -> Vec<[u8; 4]> {
		boxes(data, 0, data.len()).into_iter().map(|b| b.0).collect()
	}

	/// `ctts`'s version and its entries expanded to one offset per sample.
	fn ctts(data: &[u8]) -> Option<(u8, Vec<i32>)> {
		let ctts = stbl(data, b"ctts")?;
		let entries = (0..u32_at(ctts, 4) as usize).map(|k| (u32_at(ctts, 8 + 8 * k), u32_at(ctts, 12 + 8 * k)));
		Some((ctts[0], entries.flat_map(|(n, offset)| vec![offset as i32; n as usize]).collect()))
	}

	#[test]
	fn reordered_pictures_get_composition_offsets_and_no_edit_list() {
		let data = mp4(&[0, 4, 2, 1, 3, 8, 6, 5, 7, 9], 10);
		assert_eq!(ctts(&data), Some((1, vec![0, 3, 0, -2, -1, 3, 0, -2, -1, 0])), "display - decode index");
		assert!(find(&data, &[b"moov", b"trak", b"edts"]).is_none(), "picture 0 presents at 0 unedited");
		let stts = stbl(&data, b"stts").unwrap();
		assert_eq!((u32_at(stts, 4), u32_at(stts, 8), u32_at(stts, 12)), (1, 10, 1), "one tick a sample");
	}

	#[test]
	fn pictures_in_order_need_no_ctts() {
		let data = mp4(&[0, 1, 2, 3], 4);
		assert_eq!(ctts(&data), None);
		let entry = &stbl(&data, b"stsd").unwrap()[8..];
		assert_eq!(&entry[4..8], b"avc1");
		let size = (u16::from_be_bytes([entry[32], entry[33]]), u16::from_be_bytes([entry[34], entry[35]]));
		assert_eq!(size, (32, 18), "the SPS's cropped size");
		let avcc = &entry[86..];
		assert_eq!(&avcc[4..8], b"avcC");
		assert_eq!(&avcc[8..12], &[1, 66, 0xC0, 30], "version, profile, compatibility, level");
		assert_eq!(&avcc[12..14], &[0xFF, 0xE1], "4-byte lengths, one SPS");
		let stss = stbl(&data, b"stss").unwrap();
		assert_eq!((u32_at(stss, 4), u32_at(stss, 8)), (1, 1), "the first sample the only sync sample");
	}

	#[test]
	fn moov_goes_ahead_of_mdat_when_its_room_holds_it() {
		let ahead = mp4(&[0, 1, 2], 3);
		assert_eq!(top(&ahead), [*b"ftyp", *b"moov", *b"free", *b"mdat"]);
		let ftyp = find(&ahead, &[b"ftyp"]).unwrap();
		assert_eq!(&ftyp[..4], b"isom");
		assert!(ftyp[8..].chunks(4).any(|b| b == b"avc1"), "compatible with avc1");
		let pictures: Vec<u64> = (0..1000).collect();
		let behind = mp4(&pictures, 0);
		assert_eq!(top(&behind), [*b"ftyp", *b"free", *b"mdat", *b"moov"], "no room: moov last");
	}

	#[test]
	fn a_parameter_set_unlike_the_first_fails_the_track() {
		let mut muxer = Muxer::new(Cursor::new(Vec::new()), 2, 4).unwrap();
		let first = sps(16, 16);
		let write = |muxer: &mut Muxer<_>, sets: &[&[u8]], picture| {
			let stream = unit(sets, 0x65, 0);
			muxer.write(AccessUnit { stream: &stream, sync: true, picture })
		};
		write(&mut muxer, &[&first, &PPS], 0).unwrap();
		write(&mut muxer, &[&first, &PPS], 1).expect("the same parameter sets repeated");
		let error = write(&mut muxer, &[&first, &[0x68, 0xCE, 0x38, 0x80]], 2).unwrap_err();
		assert!(error.to_string().contains("PPS in sample 3"), "{error}");
		let error = write(&mut muxer, &[&sps(32, 16), &PPS], 2).unwrap_err();
		assert!(error.to_string().contains("SPS in sample 3"), "{error}");
		assert_eq!(muxer.samples, 2, "nothing written after the change");
	}

	#[test]
	fn a_first_access_unit_without_parameter_sets_fails() {
		let mut muxer = Muxer::new(Cursor::new(Vec::new()), 2, 1).unwrap();
		let stream = unit(&[], 0x65, 0);
		let error = muxer.write(AccessUnit { stream: &stream, sync: true, picture: 0 }).unwrap_err();
		assert!(error.to_string().contains("parameter sets are unusable"), "{error}");
	}

	#[test]
	fn an_mp4_without_a_sample_has_no_trak() {
		let mut muxer = Muxer::new(Cursor::new(Vec::new()), 25, 3).unwrap();
		muxer.finish().unwrap();
		let data = muxer.out.into_inner();
		let moov = find(&data, &[b"moov"]).unwrap();
		assert_eq!(boxes(moov, 0, moov.len()).into_iter().map(|b| b.0).collect::<Vec<_>>(), [*b"mvhd"]);
	}
}
