use super::{PartShape, Vec2};
use crate::model::FilterLayer;

/// An interference filter's layer stack: the layers side by side along x', the outermost first, from the stack's
/// outer face at `-length/2 - stack` to the plate's front at `-length/2`, the plate's width across; each layer its
/// own index, no band, no absorption. The boundaries between layers are inner edges, so the pixels near them are
/// supersampled and a fractional boundary becomes a mixed index.
pub(super) struct StackShape {
	/// The x' of each boundary: the outer face first, the plate's front last.
	bounds: Vec<f64>,
	indices: Vec<f64>,
	half_width: f64,
}

impl StackShape {
	/// None without a layer of any thickness.
	pub fn new(layers: &[FilterLayer], length: u32, width: u32) -> Option<Self> {
		let layers: Vec<&FilterLayer> = layers.iter().filter(|l| l.thickness > 0.0).collect();
		if layers.is_empty() {
			return None;
		}
		let inner = -(length.max(1) as f64) / 2.0;
		let stack: f64 = layers.iter().map(|l| l.thickness).sum();
		let mut x = inner - stack;
		let mut bounds = Vec::with_capacity(layers.len() + 1);
		for l in &layers {
			bounds.push(x);
			x += l.thickness;
		}
		bounds.push(inner);
		Some(Self {
			bounds,
			indices: layers.iter().map(|l| l.refractive_index).collect(),
			half_width: width.max(1) as f64 / 2.0,
		})
	}

	fn outer(&self) -> f64 {
		self.bounds[0]
	}

	fn inner(&self) -> f64 {
		self.bounds[self.bounds.len() - 1]
	}

	/// The layer `x` lies in, the end layers beyond the faces.
	fn layer_at(&self, x: f64) -> usize {
		self.bounds[1..self.bounds.len() - 1].partition_point(|&b| b <= x)
	}
}

impl PartShape for StackShape {
	fn depth(&self, p: Vec2) -> f64 {
		(p.x - self.outer()).min(self.inner() - p.x).min(self.half_width - p.y.abs())
	}

	fn body_index_at(&self, p: Vec2) -> f64 {
		self.indices[self.layer_at(p.x)]
	}

	fn extent(&self) -> f64 {
		let reach = self.outer().abs().max(self.inner().abs());
		(reach * reach + self.half_width * self.half_width).sqrt()
	}

	fn depth_is_distance(&self) -> bool {
		true
	}

	/// The distance along x' to the nearest boundary between two layers (the faces are the outline).
	fn inner_edge_distance(&self, p: Vec2) -> f64 {
		self.bounds[1..self.bounds.len() - 1].iter().map(|b| (p.x - b).abs()).fold(f64::INFINITY, f64::min)
	}

	fn inner_edge(&self, p: Vec2) -> (f64, f64) {
		let i = self.layer_at(p.x);
		let (distance, far) = [i.checked_sub(1), (i + 1 < self.indices.len()).then_some(i + 1)]
			.into_iter()
			.flatten()
			.map(|j| ((p.x - self.bounds[j.max(i)]).abs(), self.indices[j]))
			.fold((f64::INFINITY, self.indices[i]), |best, c| if c.0 < best.0 { c } else { best });
		(distance, far)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn layers_lie_outside_the_plate_outermost_first() {
		let layers = [
			FilterLayer { thickness: 2.0, refractive_index: 2.0 },
			FilterLayer { thickness: 3.0, refractive_index: 1.5 },
		];
		let s = StackShape::new(&layers, 10, 20).unwrap();
		assert_eq!((s.outer(), s.inner()), (-10.0, -5.0));
		assert_eq!(s.body_index_at(Vec2::new(-9.0, 0.0)), 2.0);
		assert_eq!(s.body_index_at(Vec2::new(-7.0, 0.0)), 1.5);
		assert_eq!(s.depth(Vec2::new(-9.0, 0.0)), 1.0);
		assert_eq!(s.inner_edge_distance(Vec2::new(-7.0, 0.0)), 1.0);
		assert_eq!(s.inner_edge(Vec2::new(-7.0, 0.0)), (1.0, 2.0));
		assert_eq!(s.inner_edge(Vec2::new(-9.5, 0.0)), (1.5, 1.5));
		assert!(StackShape::new(&[FilterLayer { thickness: 0.0, refractive_index: 2.0 }], 10, 20).is_none());
	}
}
