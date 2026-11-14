//! The native H.264 encoder (decision 0005): Cisco's prebuilt `OpenH264`, which Cisco licenses
//! under the AVC patent pool only when it is downloaded from Cisco to the user's device, the user can
//! enable and disable it, [`NOTICE`] is shown where they do, and [`LICENSE`] where licensing information
//! is. The user's choice lives in the program's settings and the folder is the program's to name; [`library`]
//! downloads the binary there at the first export after it is enabled, [`delete`] removes it, and
//! [`OpenH264Factory`] loads it at run time. A build with the `openh264-source` feature compiles
//! `OpenH264` in instead ([`BUILT_IN`]).
//!
//! The encoder: constrained baseline at level 5.2, no B-frames, an IDR every second, quality rate control under the
//! bitrate ceiling, no frame skipping, no background detection (it held moving waves as static and cost 3 dB);
//! synchronous, so always ready. At most 3840 x 2160, either orientation.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::task::Poll;

use openh264::encoder::{
	BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, Level, RateControlMode, UsageType,
	VuiConfig,
};
use openh264::formats::YUVSlices;

use super::{EncoderSpec, H264Encoder, H264Factory, Profile, Yuv420};
use crate::{ExportError, Result};

/// The `OpenH264` release whose binaries the `openh264` crate accepts (it checks their SHA-256).
pub const VERSION: &str = "2.6.0";
/// The text Cisco requires where the user controls the codec's use.
pub const NOTICE: &str = "OpenH264 Video Codec provided by Cisco Systems, Inc.";
/// Cisco's licence of its binary, to be shown with the program's licensing information.
pub const LICENSE: &str = include_str!("../../assets/OpenH264-BINARY_LICENSE.txt");
/// `OpenH264` is compiled into this build: no download, no Cisco terms.
pub const BUILT_IN: bool = cfg!(feature = "openh264-source");

const URL: &str = "https://ciscobinary.openh264.org/";
/// Cisco's compressed binaries are about 1 MB.
const MAX_DOWNLOAD: u64 = 16 << 20;

/// Cisco's binary for this platform, `None` where Cisco publishes none.
pub fn file_name() -> Option<&'static str> {
	Some(match (std::env::consts::OS, std::env::consts::ARCH) {
		("linux", "x86_64") => "libopenh264-2.6.0-linux64.8.so",
		("linux", "aarch64") => "libopenh264-2.6.0-linux-arm64.8.so",
		("macos", "x86_64") => "libopenh264-2.6.0-mac-x64.dylib",
		("macos", "aarch64") => "libopenh264-2.6.0-mac-arm64.dylib",
		("windows", "x86_64") => "openh264-2.6.0-win64.dll",
		("windows", "aarch64") => "openh264-2.6.0-win-arm64.dll",
		_ => return None,
	})
}

/// The library an MP4 export loads: `None` when `OpenH264` is compiled in, else Cisco's binary in `dir`,
/// downloaded now when it is missing (`on_chunk` as in [`download`]). Fails with [`ExportError::H264Missing`]
/// unless `enabled`, the user's choice.
pub fn library(dir: &Path, enabled: bool, on_chunk: impl FnMut(usize, Option<u64>)) -> Result<Option<PathBuf>> {
	if BUILT_IN {
		return Ok(None);
	}
	if !enabled {
		return Err(ExportError::H264Missing);
	}
	download(dir, on_chunk).map(Some)
}

/// The downloaded binary in `dir`, if there.
pub fn installed(dir: &Path) -> Option<PathBuf> {
	file_name().map(|n| dir.join(n)).filter(|p| p.is_file())
}

/// Downloads Cisco's binary into `dir` (made when missing) unless it is there, and returns its path. The file
/// is checked against the known SHA-256 of Cisco's builds before it takes its name. `on_chunk(bytes, total)`
/// follows the download, `total` the compressed size when the server names it.
pub fn download(dir: &Path, mut on_chunk: impl FnMut(usize, Option<u64>)) -> Result<PathBuf> {
	if let Some(path) = installed(dir) {
		return Ok(path);
	}
	let name = file_name().ok_or_else(|| ExportError::Codec("Cisco publishes no OpenH264 for this platform".into()))?;
	let url = format!("{URL}{name}.bz2");
	tracing::info!(%url, "downloading OpenH264");
	let failed = |e: &dyn std::fmt::Display| ExportError::Codec(format!("downloading {url}: {e}"));
	let mut response = ureq::get(&url).call().map_err(|e| failed(&e))?;
	let total = response.body().content_length();
	let mut reader = response.body_mut().with_config().limit(MAX_DOWNLOAD).reader();
	let mut packed = Vec::new();
	let mut chunk = [0; 64 << 10];
	loop {
		let n = reader.read(&mut chunk).map_err(|e| failed(&e))?;
		if n == 0 {
			break;
		}
		packed.extend_from_slice(&chunk[..n]);
		on_chunk(n, total);
	}
	let mut bytes = Vec::new();
	bzip2::read::BzDecoder::new(packed.as_slice()).read_to_end(&mut bytes).map_err(|e| failed(&e))?;
	std::fs::create_dir_all(dir)?;
	let part = dir.join(format!("{name}.part"));
	std::fs::write(&part, &bytes)?;
	let checked = openh264::OpenH264API::from_blob_path(&part);
	if let Err(e) = checked {
		std::fs::remove_file(&part)?;
		return Err(ExportError::Codec(format!("the downloaded OpenH264 was refused: {e}")));
	}
	drop(checked);
	let path = dir.join(name);
	std::fs::rename(&part, &path)?;
	tracing::info!(path = %path.display(), "OpenH264 installed");
	Ok(path)
}

/// Deletes the downloaded binary from `dir`, if there.
pub fn delete(dir: &Path) -> Result<()> {
	if let Some(path) = installed(dir) {
		std::fs::remove_file(path)?;
	}
	Ok(())
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
		openh264::OpenH264API::from_blob_path(path).map_err(|e| ExportError::Codec(e.to_string()))
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

	/// Encoders of the library at `library`, as [`library`] returns it.
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
		Ok(Box::new(OpenH264Encoder { encoder, stream: Vec::new(), held: None }))
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
		Ok(())
	}

	fn drain(&mut self, out: &mut dyn FnMut(&[u8], bool) -> Result<()>) -> Result<()> {
		match self.held.take() {
			Some(sync) => out(&self.stream, sync),
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
	fn the_platform_names_a_binary_of_the_accepted_release() {
		if let Some(name) = file_name() {
			assert!(name.contains(VERSION));
		}
		assert!(LICENSE.contains(NOTICE));
	}
}
