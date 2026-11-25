//! The memory warning's window (docs/ui/60, Memory warning): the core's arithmetic for the levers ticked.

use waveduderust::run::{Levers, MemoryWarning};

use crate::ui;

/// `bytes` as MB below a GB, else GB with a decimal (binary units).
pub(crate) fn size(bytes: u64) -> String {
	const MB: f64 = (1u64 << 20) as f64;
	let mb = bytes as f64 / MB;
	if mb < 1024. { format!("{mb:.0} MB") } else { format!("{:.1} GB", mb / 1024.) }
}

/// The window for `w` with `ticks`.
pub(crate) fn data(w: &MemoryWarning<'_>, ticks: Levers) -> ui::MemoryWarningData {
	let enabled = w.enabled();
	let (need, budget) = (w.bytes_of(ticks), w.budget_of(ticks));
	ui::MemoryWarningData {
		need: size(w.request.store_bytes()).into(),
		budget: size(w.budget).into(),
		auto_budget: size(w.auto_budget).into(),
		// natively the cache is Auto already (decision 0027)
		cache_shown: cfg!(target_arch = "wasm32"),
		cache_enabled: enabled.cache_auto,
		resolution_enabled: enabled.resolution,
		frames_enabled: enabled.frames,
		samples_enabled: enabled.samples,
		cache_ticked: ticks.cache_auto,
		resolution_ticked: ticks.resolution,
		frames_ticked: ticks.frames,
		samples_ticked: ticks.samples,
		fill: if budget == 0 { 1. } else { (need as f64 / budget as f64).min(1.) as f32 },
		over: need > budget,
		figures: format!("{} / {}", size(need), size(budget)).into(),
		then_need: size(need).into(),
		spill: if need > budget { size(w.spill_of(ticks)).into() } else { Default::default() },
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sizes_read_in_mb_then_gb() {
		assert_eq!(size(300 << 20), "300 MB");
		assert_eq!(size(3 << 29), "1.5 GB");
	}
}
