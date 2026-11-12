//! Preview quality and the first-start defaults (spec/40-frames-store-render.md, Preview quality defaults).
//!
//! The quality never changes the simulation, only what a run captures and stores.

use crate::engine::SampleFormat;

/// What a run captures: resolution divisor (1, 2, 4), every n-th timeline frame (1, 2, 4), the sample format,
/// and whether an f16 intensity plane holds sqrt(I) (the preview's) or I.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PreviewQuality {
	pub resolution_divisor: u32,
	pub frame_divisor: u32,
	pub format: SampleFormat,
	pub intensity_root: bool,
}

impl PreviewQuality {
	/// Every cell of every frame as a half, the intensity as its root: what an export needs.
	pub const FULL: Self =
		Self { resolution_divisor: 1, frame_divisor: 1, format: SampleFormat::Half, intensity_root: true };

	/// The fixed default when the machine was not asked (and the Middle tier).
	pub const DEFAULT: Self = Self { format: SampleFormat::Index, ..Self::FULL };

	/// The lowest notch of both divisors.
	pub const MAX_DIVISOR: u32 = 4;

	pub const fn new(resolution_divisor: u32, frame_divisor: u32, format: SampleFormat) -> Self {
		Self { resolution_divisor, frame_divisor, format, intensity_root: true }
	}

	/// Full resolution, every frame, halves; the intensity's form does not count.
	pub fn is_full(&self) -> bool {
		self.resolution_divisor == 1 && self.frame_divisor == 1 && self.format == SampleFormat::Half
	}

	/// The nearest legal divisor not above `v` (at least 1).
	pub const fn legal_divisor(v: u32) -> u32 {
		match v {
			0 | 1 => 1,
			2 | 3 => 2,
			_ => Self::MAX_DIVISOR,
		}
	}

	/// Both divisors are 1, 2 or 4.
	pub const fn is_valid(&self) -> bool {
		Self::legal_divisor(self.resolution_divisor) == self.resolution_divisor
			&& Self::legal_divisor(self.frame_divisor) == self.frame_divisor
	}

	/// The divisors brought to 1, 2 or 4; for qualities from outside (the settings file, the menus).
	pub fn validated(self) -> Self {
		Self {
			resolution_divisor: Self::legal_divisor(self.resolution_divisor),
			frame_divisor: Self::legal_divisor(self.frame_divisor),
			..self
		}
	}

	pub fn resolution_at_lowest(&self) -> bool {
		self.resolution_divisor >= Self::MAX_DIVISOR
	}

	pub fn frames_at_lowest(&self) -> bool {
		self.frame_divisor >= Self::MAX_DIVISOR
	}

	pub fn samples_at_lowest(&self) -> bool {
		self.format == SampleFormat::Index
	}

	/// Half the resolution, at most down to the lowest notch.
	pub fn halve_resolution(self) -> Self {
		Self { resolution_divisor: (self.resolution_divisor * 2).min(Self::MAX_DIVISOR), ..self }
	}

	/// Every second stored frame of the current ones, at most down to the lowest notch.
	pub fn halve_frames(self) -> Self {
		Self { frame_divisor: (self.frame_divisor * 2).min(Self::MAX_DIVISOR), ..self }
	}

	/// 8-bit palette index samples.
	pub fn to_index(self) -> Self {
		Self { format: SampleFormat::Index, ..self }
	}
}

impl Default for PreviewQuality {
	fn default() -> Self {
		Self::DEFAULT
	}
}

/// The first start's choice of preview quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PreviewTier {
	Low,
	Middle,
	High,
}

impl PreviewTier {
	/// In the order of the first-start dialog.
	pub const ALL: [Self; 3] = [Self::Low, Self::Middle, Self::High];

	/// The tier's name in files and interfaces: "Low", "Middle" or "High".
	pub const fn name(self) -> &'static str {
		match self {
			Self::Low => "Low",
			Self::Middle => "Middle",
			Self::High => "High",
		}
	}

	/// The tier named `name` by [`name`](Self::name).
	pub fn parse(name: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|t| t.name() == name)
	}

	/// The string table key of the tier's title in the first-start dialog.
	pub const fn title_key(self) -> &'static str {
		match self {
			Self::Low => "StartupTierLow",
			Self::Middle => "StartupTierStandard",
			Self::High => "StartupTierHigh",
		}
	}
}

/// What the first start asks of the machine. `None` is unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineFacts {
	/// Installed physical memory in bytes.
	pub physical_bytes: u64,
	/// Hardware threads.
	pub threads: u32,
	/// A GPU the GPU engine can use; unknown counts as present.
	pub has_gpu: Option<bool>,
	/// The frame cache's drive has a seek penalty (a hard disk); unknown counts as none.
	pub cache_drive_seek_penalty: Option<bool>,
}

/// The table of spec/40 that maps a machine to its first preview quality.
pub struct PreviewDefaults;

impl PreviewDefaults {
	/// At or above: High.
	pub const HIGH_BYTES: u64 = 30 << 30;
	/// Below: Low.
	pub const LOW_BYTES: u64 = 15 << 30;
	/// At or below: Low.
	pub const LOW_THREADS: u32 = 4;
	pub const HIGH_DITHER_BITS: u32 = 16;

	/// First match of: High at 30 GiB or more; Low under 15 GiB, with a seek penalty, without a GPU or with
	/// at most four threads; else Middle.
	pub fn tier(facts: &MachineFacts) -> PreviewTier {
		if facts.physical_bytes >= Self::HIGH_BYTES {
			return PreviewTier::High;
		}
		let weak = facts.physical_bytes < Self::LOW_BYTES
			|| facts.cache_drive_seek_penalty == Some(true)
			|| facts.has_gpu == Some(false)
			|| facts.threads <= Self::LOW_THREADS;
		if weak { PreviewTier::Low } else { PreviewTier::Middle }
	}

	pub fn quality_of(tier: PreviewTier) -> PreviewQuality {
		match tier {
			PreviewTier::High => PreviewQuality::FULL,
			PreviewTier::Low => PreviewQuality::new(2, 2, SampleFormat::Index),
			PreviewTier::Middle => PreviewQuality::DEFAULT,
		}
	}

	/// Preview dither bits; 0 is off.
	pub fn dither_bits_of(tier: PreviewTier) -> u32 {
		if tier == PreviewTier::High { Self::HIGH_DITHER_BITS } else { 0 }
	}

	/// The quality and dither bits for `facts`.
	pub fn tier_of(facts: &MachineFacts) -> (PreviewQuality, u32) {
		let tier = Self::tier(facts);
		(Self::quality_of(tier), Self::dither_bits_of(tier))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const GIB: u64 = 1 << 30;

	fn facts(gib: u64, threads: u32, gpu: Option<bool>, seek: Option<bool>) -> MachineFacts {
		MachineFacts { physical_bytes: gib * GIB, threads, has_gpu: gpu, cache_drive_seek_penalty: seek }
	}

	#[test]
	fn every_row_of_the_table() {
		let high = (PreviewQuality::FULL, 16);
		let low = (PreviewQuality::new(2, 2, SampleFormat::Index), 0);
		let middle = (PreviewQuality::new(1, 1, SampleFormat::Index), 0);
		// High wins over every weakness.
		for gpu in [None, Some(false), Some(true)] {
			for seek in [None, Some(false), Some(true)] {
				for threads in [2, 4, 16] {
					assert_eq!(PreviewDefaults::tier_of(&facts(30, threads, gpu, seek)), high);
					assert_eq!(PreviewDefaults::tier_of(&facts(64, threads, gpu, seek)), high);
				}
			}
		}
		// Windows reports a nominal 32 GB machine at about 31.9 GiB and a 16 GB one at about 15.9.
		assert_eq!(PreviewDefaults::tier_of(&facts(30, 8, Some(true), None)), high);
		let just_under_high = MachineFacts { physical_bytes: 30 * GIB - 1, ..facts(0, 8, Some(true), None) };
		assert_eq!(PreviewDefaults::tier_of(&just_under_high), middle);
		let just_under_low = MachineFacts { physical_bytes: 15 * GIB - 1, ..facts(0, 8, Some(true), None) };
		assert_eq!(PreviewDefaults::tier_of(&just_under_low), low);
		assert_eq!(PreviewDefaults::tier_of(&facts(15, 8, Some(true), Some(false))), middle);
		// Each weakness alone makes Low.
		assert_eq!(PreviewDefaults::tier_of(&facts(8, 8, Some(true), Some(false))), low);
		assert_eq!(PreviewDefaults::tier_of(&facts(16, 8, Some(true), Some(true))), low);
		assert_eq!(PreviewDefaults::tier_of(&facts(16, 8, Some(false), Some(false))), low);
		assert_eq!(PreviewDefaults::tier_of(&facts(16, 4, Some(true), Some(false))), low);
		assert_eq!(PreviewDefaults::tier_of(&facts(16, 5, Some(true), Some(false))), middle);
		// Unknown drive counts as no penalty, unknown GPU as present.
		assert_eq!(PreviewDefaults::tier_of(&facts(16, 8, None, None)), middle);
		assert_eq!(PreviewDefaults::tier_of(&facts(16, 8, Some(true), None)), middle);
	}

	#[test]
	fn levers() {
		let q = PreviewQuality::FULL;
		assert!(q.is_full() && !q.samples_at_lowest());
		assert!(PreviewQuality { intensity_root: false, ..q }.is_full());
		let q = q.halve_resolution().halve_resolution().halve_resolution();
		assert_eq!(q.resolution_divisor, 4);
		assert!(q.resolution_at_lowest() && !q.frames_at_lowest());
		let q = q.halve_frames().halve_frames();
		assert!(q.frames_at_lowest());
		assert!(q.to_index().samples_at_lowest());
		let odd = PreviewQuality { resolution_divisor: 9, frame_divisor: 0, ..q };
		assert!(!odd.is_valid());
		let v = odd.validated();
		assert!(v.is_valid() && (v.resolution_divisor, v.frame_divisor) == (4, 1));
		let v = PreviewQuality { resolution_divisor: 3, frame_divisor: 3, ..q }.validated();
		assert_eq!((v.resolution_divisor, v.frame_divisor), (2, 2));
	}
}
