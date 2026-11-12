//! The memory warning's arithmetic: a preview run whose store exceeds the budget asks first, offering the RAM
//! cache at Auto and three quality levers (spec/40-frames-store-render.md, Memory warning). The UI renders it.

use crate::quality::PreviewQuality;
use crate::spec::RunRequest;

/// The warning's four checkboxes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Levers {
	/// The RAM cache size to Auto.
	pub cache_auto: bool,
	/// Half the resolution.
	pub resolution: bool,
	/// Half the stored frames.
	pub frames: bool,
	/// 8-bit samples.
	pub samples: bool,
}

impl Levers {
	pub fn ticks(&self) -> u32 {
		[self.cache_auto, self.resolution, self.frames, self.samples].into_iter().filter(|&b| b).count() as u32
	}

	/// The fifteen non-empty combinations, in the order the proposal breaks ties by.
	fn combinations() -> impl Iterator<Item = Self> {
		(1u8..16).map(|m| Self {
			samples: m & 1 != 0,
			frames: m & 2 != 0,
			resolution: m & 4 != 0,
			cache_auto: m & 8 != 0,
		})
	}

	fn within(&self, enabled: &Self) -> bool {
		(!self.cache_auto || enabled.cache_auto)
			&& (!self.resolution || enabled.resolution)
			&& (!self.frames || enabled.frames)
			&& (!self.samples || enabled.samples)
	}
}

/// A run that does not fit, its budget now and its budget with the RAM cache at Auto (bytes).
#[derive(Clone, Copy, Debug)]
pub struct MemoryWarning<'a> {
	pub request: &'a RunRequest,
	pub budget: u64,
	pub auto_budget: u64,
}

impl<'a> MemoryWarning<'a> {
	/// The warning for `request` when its store exceeds `budget`; `None` when it fits.
	pub fn check(request: &'a RunRequest, budget: u64, auto_budget: u64) -> Option<Self> {
		(request.store_bytes() > budget).then_some(Self { request, budget, auto_budget })
	}

	/// The levers with something to offer: the cache when Auto gains at least a frame, a quality lever above
	/// its lowest notch.
	pub fn enabled(&self) -> Levers {
		let q = &self.request.quality;
		Levers {
			cache_auto: self.auto_budget.saturating_sub(self.budget) >= self.request.frame_bytes(),
			resolution: !q.resolution_at_lowest(),
			frames: !q.frames_at_lowest(),
			samples: !q.samples_at_lowest(),
		}
	}

	pub fn quality_of(&self, levers: Levers) -> PreviewQuality {
		let mut q = self.request.quality;
		if levers.resolution {
			q = q.halve_resolution();
		}
		if levers.frames {
			q = q.halve_frames();
		}
		if levers.samples {
			q = q.to_index();
		}
		q
	}

	pub fn budget_of(&self, levers: Levers) -> u64 {
		if levers.cache_auto { self.auto_budget } else { self.budget }
	}

	/// The store's bytes at the levers' quality.
	pub fn bytes_of(&self, levers: Levers) -> u64 {
		self.request.store_bytes_at(self.quality_of(levers))
	}

	pub fn fits(&self, levers: Levers) -> bool {
		self.bytes_of(levers) <= self.budget_of(levers)
	}

	/// Bytes beyond the budget at the levers' choice (0 when it fits).
	pub fn spill_of(&self, levers: Levers) -> u64 {
		self.bytes_of(levers).saturating_sub(self.budget_of(levers))
	}

	/// The pre-ticked levers: of the enabled combinations that fit, the fewest ticks, among as many the largest
	/// store (the mildest loss, so the cache comes before any quality); ties go to the first in the order
	/// samples, frames, resolution, cache as the low bits up. When nothing fits, every enabled lever.
	pub fn proposal(&self) -> Levers {
		let enabled = self.enabled();
		let mut best: Option<(Levers, u32, u64)> = None;
		for levers in Levers::combinations().filter(|l| l.within(&enabled)) {
			let bytes = self.bytes_of(levers);
			if bytes > self.budget_of(levers) {
				continue;
			}
			let ticks = levers.ticks();
			if best.is_none_or(|(_, t, b)| ticks < t || (ticks == t && bytes > b)) {
				best = Some((levers, ticks, bytes));
			}
		}
		best.map_or(enabled, |(levers, _, _)| levers)
	}
}
