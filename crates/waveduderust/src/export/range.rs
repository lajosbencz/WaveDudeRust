//! The frames an export covers.

use std::ops::RangeInclusive;

/// Timeline frames from the in and out points (steps) of `Display`:
/// `first = clamp(from / ve, 0, last_frame)`, `last = to == 0 ? last_frame : clamp(to / ve, first, last_frame)`,
/// integer division. None when there is no frame.
pub fn frame_range(
	export_from: u32,
	export_to: u32,
	visualize_every: u32,
	frame_count: usize,
) -> Option<RangeInclusive<usize>> {
	let last_frame = frame_count.checked_sub(1)?;
	let ve = visualize_every as usize;
	let first = (export_from as usize / ve).min(last_frame);
	let last = if export_to == 0 { last_frame } else { (export_to as usize / ve).clamp(first, last_frame) };
	Some(first..=last)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_range_rule() {
		assert_eq!(frame_range(0, 0, 2, 10), Some(0..=9));
		assert_eq!(frame_range(4, 14, 2, 10), Some(2..=7));
		assert_eq!(frame_range(5, 15, 2, 10), Some(2..=7));
		assert_eq!(frame_range(100, 0, 2, 10), Some(9..=9));
		assert_eq!(frame_range(10, 4, 2, 10), Some(5..=5));
		assert_eq!(frame_range(0, 1000, 2, 10), Some(0..=9));
		assert_eq!(frame_range(0, 0, 2, 0), None);
	}
}
