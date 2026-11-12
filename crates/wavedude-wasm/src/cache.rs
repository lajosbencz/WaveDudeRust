//! The stored frames of a session under a byte budget.

use std::collections::BTreeMap;

use wavedude_core::engine::Snapshot;

/// Frames by stored index. A frame inserted past the budget evicts the frames farthest from it, so the one
/// asked for always stays, even when it alone exceeds the budget.
#[derive(Debug)]
pub(crate) struct FrameCache {
	frames: BTreeMap<u32, Snapshot>,
	bytes: u64,
	budget: u64,
}

fn size_of(snapshot: &Snapshot) -> u64 {
	(snapshot.field.len() + snapshot.intensity.len()) as u64
}

impl FrameCache {
	pub(crate) fn new(budget: u64) -> Self {
		Self { frames: BTreeMap::new(), bytes: 0, budget }
	}

	#[cfg(test)]
	pub(crate) fn len(&self) -> usize {
		self.frames.len()
	}

	#[cfg(test)]
	pub(crate) fn bytes(&self) -> u64 {
		self.bytes
	}

	pub(crate) fn get(&self, index: u32) -> Option<&Snapshot> {
		self.frames.get(&index)
	}

	/// The stored index nearest to `index` from below, `index` itself included.
	pub(crate) fn latest_at_or_before(&self, index: u32) -> Option<u32> {
		self.frames.range(..=index).next_back().map(|(&i, _)| i)
	}

	/// Stores the frame `index` and evicts the farthest frames until the cache fits its budget.
	pub(crate) fn insert(&mut self, index: u32, snapshot: Snapshot) {
		self.bytes += size_of(&snapshot);
		if let Some(old) = self.frames.insert(index, snapshot) {
			self.bytes -= size_of(&old);
		}
		while self.bytes > self.budget {
			let Some(&farthest) = self.frames.keys().filter(|&&i| i != index).max_by_key(|&&i| i.abs_diff(index))
			else {
				break;
			};
			let evicted = self.frames.remove(&farthest).expect("the key was listed");
			self.bytes -= size_of(&evicted);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_core::engine::{FrameShape, SampleFormat};

	fn frame(bytes: usize) -> Snapshot {
		let shape = FrameShape {
			canvas_width: 1,
			canvas_height: 1,
			divisor: 1,
			format: SampleFormat::Index,
			field_brightness: 1.,
			intensity_brightness: 1.,
			intensity_root: false,
		};
		Snapshot { field: vec![0; bytes], ..Snapshot::new(shape) }
	}

	#[test]
	fn the_farthest_frame_goes_first_and_the_requested_one_stays() {
		let mut cache = FrameCache::new(30);
		for i in [0, 1, 2] {
			cache.insert(i, frame(10));
		}
		assert_eq!((cache.len(), cache.bytes()), (3, 30));
		cache.insert(7, frame(10));
		assert!(cache.get(0).is_none() && cache.get(7).is_some() && cache.get(2).is_some());
		cache.insert(0, frame(10));
		assert!(cache.get(7).is_none() && cache.get(0).is_some());
		assert_eq!(cache.bytes(), 30);
	}

	#[test]
	fn a_frame_over_the_budget_stays_alone() {
		let mut cache = FrameCache::new(5);
		cache.insert(3, frame(10));
		cache.insert(4, frame(10));
		assert_eq!((cache.len(), cache.bytes()), (1, 10));
		assert!(cache.get(4).is_some());
	}

	#[test]
	fn the_latest_frame_at_or_before_an_index() {
		let mut cache = FrameCache::new(100);
		cache.insert(2, frame(1));
		cache.insert(5, frame(1));
		assert_eq!(cache.latest_at_or_before(1), None);
		assert_eq!(cache.latest_at_or_before(2), Some(2));
		assert_eq!(cache.latest_at_or_before(4), Some(2));
		assert_eq!(cache.latest_at_or_before(9), Some(5));
	}
}
