//! H.264 encoding behind MP4 export: the encoder seam ([`H264Encoder`], opened by an [`H264Factory`]), the
//! profiles and the bitrate a quality maps to. The `openh264` module (feature `mp4`) is Cisco's `OpenH264`
//! (decision 0005).

use std::fmt::Debug;
use std::task::Poll;

use crate::sync::{MaybeSend, MaybeSync};

use crate::export::Result;
pub use crate::export::yuv::Yuv420;

#[cfg(feature = "mp4")]
pub mod openh264;

#[cfg(feature = "mp4")]
pub use self::openh264::{BUILT_IN, LICENSE, NOTICE, OpenH264Factory, VERSION, check_library};

/// H.264 profiles, in the order of the coding tools they add.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Profile {
	/// Constrained Baseline: no B-frames, CAVLC only.
	#[default]
	Baseline,
	Main,
	High,
}

impl Profile {
	pub const ALL: [Profile; 3] = [Profile::Baseline, Profile::Main, Profile::High];

	/// `self` when `supported` holds it, else the last of `supported` in [`Profile`] order; `None` when it is empty.
	pub fn or_best_of(self, supported: &[Profile]) -> Option<Profile> {
		if supported.contains(&self) { Some(self) } else { supported.iter().copied().max() }
	}
}

/// What an encoder is opened with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderSpec {
	/// Picture size, both even.
	pub width: u32,
	pub height: u32,
	pub fps: u32,
	/// The bitrate ceiling in bit/s ([`bitrate`]).
	pub bitrate: u32,
	pub profile: Profile,
}

/// One encoded picture.
#[derive(Clone, Copy, Debug)]
pub struct AccessUnit<'a> {
	/// Annex B bytes.
	pub stream: &'a [u8],
	/// A sync sample.
	pub sync: bool,
	/// The picture's index in submission order, which is display order; each submitted picture comes once.
	pub picture: u64,
}

/// An H.264 encoder: pictures in, their access units out in decode order, which differs from display order when
/// it codes B-frames; an IDR every `fps` pictures starting with the first. The first access unit carries the SPS
/// and PPS.
pub trait H264Encoder: MaybeSend {
	/// Ready when it takes a picture.
	fn poll_ready(&mut self) -> Result<Poll<()>>;

	/// Encodes the next picture.
	fn submit(&mut self, picture: &Yuv420) -> Result<()>;

	/// Hands each access unit encoded since the last call to `out`, in decode order.
	fn drain(&mut self, out: &mut dyn FnMut(AccessUnit<'_>) -> Result<()>) -> Result<()>;

	/// Ends the input: what is held is encoded.
	fn start_flush(&mut self) -> Result<()>;

	/// Ready once every picture submitted is encoded, after [`H264Encoder::start_flush`].
	fn poll_flushed(&mut self) -> Result<Poll<()>>;
}

/// Opens [`H264Encoder`]s.
pub trait H264Factory: Debug + MaybeSend + MaybeSync {
	/// The profiles its encoders write.
	fn profiles(&self) -> &[Profile];

	/// An encoder of `spec`, whose profile is one of [`H264Factory::profiles`].
	fn open(&self, spec: &EncoderSpec) -> Result<Box<dyn H264Encoder>>;
}

/// The target bitrate of a quality 1..100: `clamp(round_even(W * H * fps * 0.01 * 50^(q / 100)), 300000, 200000000)`
/// bit/s.
pub fn bitrate(width: u32, height: u32, fps: u32, quality: u32) -> u32 {
	let q = quality as f64;
	let b = (width as f64 * height as f64 * fps as f64 * 0.01 * 50f64.powf(q / 100.0)).round_ties_even();
	b.clamp(300_000.0, 200_000_000.0) as u32
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_profile_falls_back_to_the_best_supported() {
		use Profile::*;
		assert_eq!(High.or_best_of(&[Baseline]), Some(Baseline));
		assert_eq!(High.or_best_of(&[Baseline, Main]), Some(Main));
		assert_eq!(Main.or_best_of(&[Baseline, Main, High]), Some(Main));
		assert_eq!(Baseline.or_best_of(&[Main, High]), Some(High));
		assert_eq!(Baseline.or_best_of(&[]), None);
	}

	#[test]
	fn a_quality_maps_to_its_bitrate_within_the_bounds() {
		assert_eq!(
			bitrate(1920, 1080, 60, 85),
			(1920.0 * 1080.0 * 60.0 * 0.01 * 50f64.powf(0.85)).round_ties_even() as u32
		);
		assert_eq!(bitrate(16, 16, 1, 1), 300_000);
		assert_eq!(bitrate(4096, 2304, 240, 100), 200_000_000);
	}
}
