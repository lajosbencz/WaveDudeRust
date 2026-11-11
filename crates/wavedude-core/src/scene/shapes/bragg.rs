use super::{PartShape, Vec2};
use crate::model::BraggReflector;

/// A stack of `Pairs` layer pairs along x' from -L/2: `[k p, k p + T1)` Index1, then Index2.
pub(super) struct BraggShape {
	half_length: f64,
	half_width: f64,
	period: f64,
	thickness1: f64,
	index1: f64,
	index2: f64,
}

impl BraggShape {
	pub(super) fn new(b: &BraggReflector) -> Self {
		let thickness1 = b.thickness1 as f64;
		let period = thickness1 + b.thickness2 as f64;
		Self {
			half_length: b.pairs as f64 * period / 2.0,
			half_width: b.width as f64 / 2.0,
			period,
			thickness1,
			index1: b.index1,
			index2: b.index2,
		}
	}

	/// Position within the pair, truncated remainder as in the original.
	fn along(&self, x: f64) -> f64 {
		(x + self.half_length) % self.period
	}

	/// The layer boundaries inside the stack, from -L/2: `k p + T1` for every pair, `k p` from the second.
	fn nearest_boundary(&self, x: f64) -> f64 {
		let from_start = x + self.half_length;
		let pairs = (2.0 * self.half_length / self.period).round();
		let k = (from_start / self.period).floor().clamp(0.0, pairs - 1.0);
		[k - 1.0, k, k + 1.0]
			.into_iter()
			.filter(|&k| (0.0..pairs).contains(&k))
			.flat_map(|k| [k * self.period, k * self.period + self.thickness1])
			.filter(|&b| b > 0.0)
			.min_by(|a, b| (from_start - a).abs().total_cmp(&(from_start - b).abs()))
			.map_or(f64::INFINITY, |b| b - self.half_length)
	}
}

impl PartShape for BraggShape {
	fn depth(&self, p: Vec2) -> f64 {
		(self.half_length - p.x.abs()).min(self.half_width - p.y.abs())
	}

	/// Beyond the ends the end layer's index (asked there for a pixel the outline cuts).
	fn body_index_at(&self, p: Vec2) -> f64 {
		if p.x >= self.half_length {
			self.index2
		} else if self.along(p.x.max(-self.half_length)) < self.thickness1 {
			self.index1
		} else {
			self.index2
		}
	}

	fn extent(&self) -> f64 {
		(self.half_length * self.half_length + self.half_width * self.half_width).sqrt()
	}

	fn depth_is_distance(&self) -> bool {
		true
	}

	fn inner_edge_distance(&self, p: Vec2) -> f64 {
		let along = self.along(p.x);
		along.min((along - self.thickness1).abs().min(self.period - along))
	}

	/// The nearest layer boundary inside the stack and the other layer's index.
	fn inner_edge(&self, p: Vec2) -> (f64, f64) {
		let far = if self.body_index_at(p) == self.index1 { self.index2 } else { self.index1 };
		((p.x - self.nearest_boundary(p.x)).abs(), far)
	}
}
