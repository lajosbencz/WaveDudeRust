//! H.264 in MP4: openh264 (constrained baseline at level 5.2, no B-frames, an IDR every second, quality rate control under a bitrate ceiling,
//! no frame skipping, no background detection: it held moving waves as static and cost 3 dB) muxed by the
//! `mp4` crate as one `avc1` track (timescale = fps, one tick per sample, the SPS and PPS in `avcC`).
//!
//! Input: 4:2:0 studio-range BT.601 from `yuv`; an odd size is padded to even by repeating the last column
//! and row, and the video has the padded size. At most 3840 x 2160 (openh264's limit, either orientation).

use std::io::{Seek, Write};
use std::path::Path;

use mp4::{AvcConfig, MediaConfig, Mp4Config, Mp4Sample, TrackConfig, TrackType};
use openh264::encoder::{
	BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, Level, Profile, RateControlMode,
	UsageType, VuiConfig,
};
use openh264::formats::YUVSlices;
use wavedude_core::sync::MaybeSend;

use super::{FrameSink, SinkSpec, check_size};
use crate::yuv::Yuv420;
use crate::{ExportError, Result};

/// openh264 refuses larger pictures (level 5.2).
const MAX_SIDES: (u32, u32) = (3840, 2160);

/// The target bitrate of a quality 1..100: `clamp(round_even(W * H * fps * 0.01 * 50^(q / 100)), 300000, 200000000)`
/// bit/s.
pub fn bitrate(width: u32, height: u32, fps: u32, quality: u32) -> u32 {
	let q = quality as f64;
	let b = (width as f64 * height as f64 * fps as f64 * 0.01 * 50f64.powf(q / 100.0)).round_ties_even();
	b.clamp(300_000.0, 200_000_000.0) as u32
}

fn h264_error(e: impl std::fmt::Display) -> ExportError {
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

/// An MP4 writer.
pub struct Mp4Writer<W: Write + Seek> {
	muxer: Option<mp4::Mp4Writer<W>>,
	/// The output after [`FrameSink::finish`].
	done: Option<W>,
	/// The encoder's Annex B stream of the last frame, its allocation reused.
	stream: Vec<u8>,
	encoder: Encoder,
	width: u32,
	height: u32,
	fps: u32,
	planes: Yuv420,
	track: bool,
	frames: usize,
	finished: bool,
}

impl<W: Write + Seek> Mp4Writer<W> {
	fn check(width: u32, height: u32) -> Result<()> {
		let (long, short) = (width.max(height).next_multiple_of(2), width.min(height).next_multiple_of(2));
		if width == 0 || height == 0 || long > MAX_SIDES.0 || short > MAX_SIDES.1 {
			return Err(ExportError::Size { format: "MP4", width, height, limit: "up to 3840 x 2160" });
		}
		Ok(())
	}

	pub fn new(out: W, spec: SinkSpec, library: Option<&Path>) -> Result<Self> {
		Self::check(spec.width, spec.height)?;
		Self::start(out, spec, crate::h264::api(library)?)
	}

	fn start(out: W, spec: SinkSpec, api: openh264::OpenH264API) -> Result<Self> {
		let fps = spec.fps;
		let bps = bitrate(spec.width, spec.height, fps, spec.quality);
		let config = EncoderConfig::new()
			.bitrate(BitRate::from_bps(bps))
			.max_frame_rate(FrameRate::from_hz(fps as f32))
			.rate_control_mode(RateControlMode::Quality)
			.skip_frames(false)
			.background_detection(false)
			.usage_type(UsageType::CameraVideoRealTime)
			.profile(Profile::Baseline)
			.level(Level::Level_5_2)
			.intra_frame_period(IntraFramePeriod::from_num_frames(fps))
			.vui(VuiConfig::bt601());
		let encoder = Encoder::with_api_config(api, config).map_err(h264_error)?;
		let brand = |s: &str| s.parse().map_err(h264_error);
		let muxer = mp4::Mp4Writer::write_start(
			out,
			&Mp4Config {
				major_brand: brand("isom")?,
				minor_version: 512,
				compatible_brands: vec![brand("isom")?, brand("iso2")?, brand("avc1")?, brand("mp41")?],
				timescale: fps,
			},
		)
		.map_err(h264_error)?;
		tracing::debug!(width = spec.width, height = spec.height, fps, bps, "MP4 writer opened");
		Ok(Self {
			muxer: Some(muxer),
			done: None,
			stream: Vec::new(),
			encoder,
			width: spec.width,
			height: spec.height,
			fps,
			planes: Yuv420::new(spec.width.next_multiple_of(2) as usize, spec.height.next_multiple_of(2) as usize),
			track: false,
			frames: 0,
			finished: false,
		})
	}

	/// The writer back after [`FrameSink::finish`].
	pub fn into_inner(self) -> Option<W> {
		self.done
	}
}

impl<W: Write + Seek + MaybeSend> FrameSink for Mp4Writer<W> {
	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		if self.finished {
			return Err(ExportError::Finished);
		}
		check_size(self.width, self.height, width, height, rgba.len(), 4)?;
		self.planes.convert(rgba, width as usize, height as usize);
		let p = &self.planes;
		let yuv = YUVSlices::new((&p.y, &p.cb, &p.cr), (p.width, p.height), (p.width, p.width / 2, p.width / 2));
		let encoded = self.encoder.encode(&yuv).map_err(h264_error)?;
		let sync = matches!(encoded.frame_type(), FrameType::IDR | FrameType::I);
		self.stream.clear();
		encoded.write_vec(&mut self.stream);
		let (mut sps, mut pps, mut sample) = (None, None, Vec::with_capacity(self.stream.len() + 16));
		for nal in nal_units(&self.stream) {
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
						width: self.planes.width as u16,
						height: self.planes.height as u16,
						seq_param_set: sps.to_vec(),
						pic_param_set: pps.to_vec(),
					}),
				})
				.map_err(h264_error)?;
			self.track = true;
		}
		let frame = Mp4Sample {
			start_time: self.frames as u64,
			duration: 1,
			rendering_offset: 0,
			is_sync: sync,
			bytes: sample.into(),
		};
		muxer.write_sample(1, &frame).map_err(h264_error)?;
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
		if let Some(mut muxer) = self.muxer.take() {
			muxer.write_end().map_err(h264_error)?;
			let mut out = muxer.into_writer();
			out.flush()?;
			self.done = Some(out);
		}
		Ok(())
	}
}
