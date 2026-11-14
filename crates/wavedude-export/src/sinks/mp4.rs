//! H.264 in MP4: an [`H264Encoder`]'s access units muxed by the `mp4` crate as one `avc1` track (timescale = fps,
//! one tick per sample, the SPS and PPS of the first access unit in `avcC`).
//!
//! Input: 4:2:0 studio-range BT.601 from `yuv`; an odd size is padded to even by repeating the last column
//! and row, and the video has the padded size.

use std::io::{Seek, Write};
use std::task::Poll;

use mp4::{AvcConfig, MediaConfig, Mp4Config, Mp4Sample, TrackConfig, TrackType};
use wavedude_core::sync::MaybeSend;

use super::{FrameSink, SinkSpec, check_size};
use crate::h264::{EncoderSpec, H264Encoder, H264Factory, bitrate};
use crate::yuv::Yuv420;
use crate::{ExportError, Result};

fn mux_error(e: impl std::fmt::Display) -> ExportError {
	ExportError::H264(e.to_string())
}

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

/// The `avc1` track of an MP4, one sample per access unit.
struct Muxer<W: Write + Seek> {
	muxer: Option<mp4::Mp4Writer<W>>,
	/// The output after [`Muxer::finish`].
	done: Option<W>,
	fps: u32,
	/// The coded picture size.
	width: u32,
	height: u32,
	track: bool,
	samples: usize,
}

impl<W: Write + Seek> Muxer<W> {
	fn new(out: W, fps: u32, width: u32, height: u32) -> Result<Self> {
		let brand = |s: &str| s.parse().map_err(mux_error);
		let muxer = mp4::Mp4Writer::write_start(
			out,
			&Mp4Config {
				major_brand: brand("isom")?,
				minor_version: 512,
				compatible_brands: vec![brand("isom")?, brand("iso2")?, brand("avc1")?, brand("mp41")?],
				timescale: fps,
			},
		)
		.map_err(mux_error)?;
		Ok(Self { muxer: Some(muxer), done: None, fps, width, height, track: false, samples: 0 })
	}

	/// Adds the access unit `stream` (Annex B) as the next sample; the first one's SPS and PPS open the track.
	fn write(&mut self, stream: &[u8], sync: bool) -> Result<()> {
		let (mut sps, mut pps, mut sample) = (None, None, Vec::with_capacity(stream.len() + 16));
		for nal in nal_units(stream) {
			match nal[0] & 0x1F {
				7 | 8 if self.track => {}
				7 => sps = Some(nal),
				8 => pps = Some(nal),
				_ => {
					sample.extend_from_slice(&(nal.len() as u32).to_be_bytes());
					sample.extend_from_slice(nal);
				}
			}
		}
		let muxer = self.muxer.as_mut().ok_or(ExportError::Finished)?;
		if !self.track {
			let (Some(sps), Some(pps)) = (sps, pps) else {
				return Err(ExportError::H264("the first frame carries no SPS and PPS".into()));
			};
			muxer
				.add_track(&TrackConfig {
					track_type: TrackType::Video,
					timescale: self.fps,
					language: "und".into(),
					media_conf: MediaConfig::AvcConfig(AvcConfig {
						width: self.width as u16,
						height: self.height as u16,
						seq_param_set: sps.to_vec(),
						pic_param_set: pps.to_vec(),
					}),
				})
				.map_err(mux_error)?;
			self.track = true;
		}
		let frame = Mp4Sample {
			start_time: self.samples as u64,
			duration: 1,
			rendering_offset: 0,
			is_sync: sync,
			bytes: sample.into(),
		};
		muxer.write_sample(1, &frame).map_err(mux_error)?;
		self.samples += 1;
		Ok(())
	}

	/// Writes the index and flushes the output; later calls do nothing.
	fn finish(&mut self) -> Result<()> {
		if let Some(mut muxer) = self.muxer.take() {
			muxer.write_end().map_err(mux_error)?;
			let mut out = muxer.into_writer();
			out.flush()?;
			self.done = Some(out);
		}
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
		let muxer = Muxer::new(out, spec.fps, width, height)?;
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

	/// The writer back after [`FrameSink::finish`].
	pub fn into_inner(self) -> Option<W> {
		self.muxer.done
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

#[cfg(all(test, feature = "openh264-source"))]
mod tests {
	use std::io::Cursor;

	use sha2::{Digest, Sha256};

	use super::*;
	use crate::h264::{OpenH264Factory, Profile};

	/// Size and SHA-256 of [`movie`]'s file.
	const PINNED: (usize, &str) = (2520, "e4463e29852a7fd73f74a78b078b2699519485c9b7df3374a7b60f958181e9b0");

	/// 12 frames of a moving gradient at 33 x 19 (padded to 34 x 20), 5 fps, quality 60: three IDRs.
	fn movie() -> Vec<u8> {
		let (w, h) = (33u32, 19u32);
		let spec = SinkSpec {
			width: w,
			height: h,
			fps: 5,
			quality: 60,
			profile: Profile::Baseline,
			first_frame: 0,
			frame_count: 12,
		};
		let mut sink = Mp4Sink::new(Cursor::new(Vec::new()), spec, &OpenH264Factory::new(None)).unwrap();
		for k in 0..12u32 {
			let rgba: Vec<u8> = (0..w * h)
				.flat_map(|i| {
					let (x, y) = (i % w, i / w);
					[(x * 7 + k * 11) as u8, (y * 13 + k * 3) as u8, ((x + y) * 5) as u8, 255]
				})
				.collect();
			sink.write_frame(&rgba, w, h).unwrap();
		}
		sink.finish().unwrap();
		sink.into_inner().unwrap().into_inner()
	}

	#[test]
	fn the_mp4_of_a_fixed_movie_keeps_its_bytes() {
		let bytes = movie();
		let digest: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
		assert_eq!((bytes.len(), digest.as_str()), PINNED);
	}
}
