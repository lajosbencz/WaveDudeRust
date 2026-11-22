//! The native H.264 encoder (decision 0005): Cisco's prebuilt `OpenH264`, licensed under the AVC patent pool only when
//! downloaded from Cisco to the user's device, enabled and disabled by the user, with [`NOTICE`] shown where they do
//! and [`LICENSE`] where licensing information is. Downloading it is the program's (no network here):
//! [`OpenH264Factory`] loads the binary at run time and [`check_library`] checks one. A build with the
//! `openh264-source` feature compiles `OpenH264` in instead ([`BUILT_IN`]).
//!
//! The encoder: constrained baseline at level 5.2, no B-frames, an IDR every second, quality rate control under the
//! bitrate ceiling, no frame skipping, no background detection (it held moving waves as static and cost 3 dB);
//! synchronous, so always ready. At most 3840 x 2160, either orientation.

use std::path::{Path, PathBuf};
use std::task::Poll;

use openh264::encoder::{
	BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, Level, RateControlMode, UsageType,
	VuiConfig,
};
use openh264::formats::YUVSlices;

use super::{AccessUnit, EncoderSpec, H264Encoder, H264Factory, Profile, Yuv420};
use crate::export::{ExportError, Result};

/// The `OpenH264` release whose binaries the `openh264` crate accepts (it checks their SHA-256).
pub const VERSION: &str = "2.6.0";
/// The text Cisco requires where the user controls the codec's use.
pub const NOTICE: &str = "OpenH264 Video Codec provided by Cisco Systems, Inc.";
/// Cisco's licence of its binary, to be shown with the program's licensing information.
pub const LICENSE: &str = include_str!("../../../assets/OpenH264-BINARY_LICENSE.txt");
/// `OpenH264` is compiled into this build: no download, no Cisco terms.
pub const BUILT_IN: bool = cfg!(feature = "openh264-source");

/// Checks that `path` is an `OpenH264` binary of the accepted release.
pub fn check_library(path: &Path) -> Result<()> {
	openh264::OpenH264API::from_blob_path(path)
		.map(drop)
		.map_err(|e| ExportError::OpenH264(format!("the OpenH264 at {} was refused: {e}", path.display())))
}

/// The encoder library: the compiled-in one, else Cisco's binary at `library`.
fn api(library: Option<&Path>) -> Result<openh264::OpenH264API> {
	#[cfg(feature = "openh264-source")]
	{
		let _ = library;
		Ok(openh264::OpenH264API::from_source())
	}
	#[cfg(not(feature = "openh264-source"))]
	{
		let path = library.ok_or(ExportError::H264Missing)?;
		openh264::OpenH264API::from_blob_path(path).map_err(|e| ExportError::OpenH264(e.to_string()))
	}
}

/// openh264 refuses larger pictures (level 5.2).
const MAX_SIDES: (u32, u32) = (3840, 2160);

/// Opens `OpenH264` encoders: the compiled-in library, else Cisco's binary at the path given.
#[derive(Clone, Debug)]
pub struct OpenH264Factory {
	library: Option<PathBuf>,
}

impl OpenH264Factory {
	/// The profiles `OpenH264` encodes.
	pub const PROFILES: &[Profile] = &[Profile::Baseline];

	/// Encoders of the binary at `library`, `None` when `OpenH264` is compiled in.
	pub fn new(library: Option<PathBuf>) -> Self {
		Self { library }
	}
}

impl H264Factory for OpenH264Factory {
	fn profiles(&self) -> &[Profile] {
		Self::PROFILES
	}

	fn open(&self, spec: &EncoderSpec) -> Result<Box<dyn H264Encoder>> {
		let (width, height) = (spec.width, spec.height);
		let (long, short) = (width.max(height), width.min(height));
		if width == 0 || height == 0 || long > MAX_SIDES.0 || short > MAX_SIDES.1 {
			return Err(ExportError::Size { format: "MP4", width, height, limit: "up to 3840 x 2160" });
		}
		let profile = match spec.profile {
			Profile::Baseline => openh264::encoder::Profile::Baseline,
			other => return Err(ExportError::H264(format!("OpenH264 encodes Baseline only, not {other:?}"))),
		};
		let api = api(self.library.as_deref())?;
		let config = EncoderConfig::new()
			.bitrate(BitRate::from_bps(spec.bitrate))
			.max_frame_rate(FrameRate::from_hz(spec.fps as f32))
			.rate_control_mode(RateControlMode::Quality)
			.skip_frames(false)
			.background_detection(false)
			.usage_type(UsageType::CameraVideoRealTime)
			.profile(profile)
			.level(Level::Level_5_2)
			.intra_frame_period(IntraFramePeriod::from_num_frames(spec.fps))
			.vui(VuiConfig::bt601());
		let encoder = Encoder::with_api_config(api, config).map_err(h264_error)?;
		Ok(Box::new(OpenH264Encoder { encoder, stream: Vec::new(), held: None, pictures: 0 }))
	}
}

fn h264_error(e: impl std::fmt::Display) -> ExportError {
	ExportError::H264(e.to_string())
}

struct OpenH264Encoder {
	encoder: Encoder,
	/// The Annex B stream of the last picture, its allocation reused.
	stream: Vec<u8>,
	/// `stream` is not drained yet; whether it is a sync sample.
	held: Option<bool>,
	/// Pictures submitted; `OpenH264` never reorders them.
	pictures: u64,
}

impl H264Encoder for OpenH264Encoder {
	fn poll_ready(&mut self) -> Result<Poll<()>> {
		Ok(Poll::Ready(()))
	}

	fn submit(&mut self, p: &Yuv420) -> Result<()> {
		let yuv = YUVSlices::new((&p.y, &p.cb, &p.cr), (p.width, p.height), (p.width, p.width / 2, p.width / 2));
		let encoded = self.encoder.encode(&yuv).map_err(h264_error)?;
		self.held = Some(matches!(encoded.frame_type(), FrameType::IDR | FrameType::I));
		self.stream.clear();
		encoded.write_vec(&mut self.stream);
		self.pictures += 1;
		Ok(())
	}

	fn drain(&mut self, out: &mut dyn FnMut(AccessUnit<'_>) -> Result<()>) -> Result<()> {
		match self.held.take() {
			Some(sync) => out(AccessUnit { stream: &self.stream, sync, picture: self.pictures - 1 }),
			None => Ok(()),
		}
	}

	fn start_flush(&mut self) -> Result<()> {
		Ok(())
	}

	fn poll_flushed(&mut self) -> Result<Poll<()>> {
		Ok(Poll::Ready(()))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_licence_carries_the_notice() {
		assert!(LICENSE.contains(NOTICE));
	}

	#[test]
	fn a_file_that_is_no_library_is_refused() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("lib");
		std::fs::write(&path, b"not a library").unwrap();
		assert!(matches!(check_library(&path), Err(ExportError::OpenH264(_))));
	}
}
