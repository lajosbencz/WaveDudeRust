use super::{Band, Body, Face, PartShape, Vec2, segment_distance};
use crate::model::profiled::ProfiledPlate;

/// A plate with a profiled front face (a diffuser, a facetted grating): the polygon of the face, a vertex per
/// pixel, and its three straight edges. The depth is the exact signed distance to that polygon, found without a
/// pass over every edge: the face is a graph over y', so only the segments whose span in y' lies nearer than
/// the best distance so far can beat it, a handful about the point. The face's segments are the front, the
/// flat back the back, the two short ends the sides, whatever the ripple's slope.
pub(super) struct ProfiledShape {
	front: Vec<f64>,
	half_length: f64,
	half_width: f64,
	body: Body,
	extent: f64,
}

impl ProfiledShape {
	/// `body`'s band carries the faces it coats; the caller leaves a face bare by clearing its flag.
	pub fn new(plate: ProfiledPlate, body: Body) -> Self {
		let half_length = plate.half_length();
		let half_width = plate.half_width();
		let reach = plate.front.iter().fold(half_length, |m, x| m.max(x.abs()));
		Self {
			front: plate.front,
			half_length,
			half_width,
			body,
			extent: (reach * reach + half_width * half_width).sqrt(),
		}
	}

	/// The front face's x' at `y`, the profile read between its pixels.
	fn front_at(&self, y: f64) -> f64 {
		let position = y + self.half_width;
		let i = (position.floor() as i64).clamp(0, self.front.len() as i64 - 2) as usize;
		self.front[i] + (position - i as f64) * (self.front[i + 1] - self.front[i])
	}

	fn segment(&self, ax: f64, ay: f64, bx: f64, by: f64, p: Vec2) -> f64 {
		let a = Vec2::new(ax, ay);
		let d = Vec2::new(bx - ax, by - ay);
		segment_distance(a, d, d.x * d.x + d.y * d.y, p)
	}

	/// The distance to the nearest edge and which face that edge is: the three straight edges, then the front's
	/// segments outward from the one under the point while their span in y' can still come nearer.
	fn nearest(&self, p: Vec2) -> (f64, Face) {
		let (hl, hw) = (self.half_length, self.half_width);
		let last = self.front.len() - 1;
		let mut best = self.segment(hl, -hw, hl, hw, p);
		let mut face = Face::Back;
		for (x, y) in [(self.front[0], -hw), (self.front[last], hw)] {
			let d = self.segment(x, y, hl, y, p);
			if d < best {
				best = d;
				face = Face::Sides;
			}
		}
		// segment k runs from y' = k - w/2 to k + 1 - w/2
		let front = |k: usize, p| self.segment(self.front[k], k as f64 - hw, self.front[k + 1], k as f64 + 1.0 - hw, p);
		let start = ((p.y + hw).floor() as i64).clamp(0, last as i64 - 1) as usize;
		for k in (0..=start).rev() {
			if p.y - (k as f64 + 1.0 - hw) >= best {
				break;
			}
			let d = front(k, p);
			if d < best {
				best = d;
				face = Face::Front;
			}
		}
		for k in start + 1..last {
			if k as f64 - hw - p.y >= best {
				break;
			}
			let d = front(k, p);
			if d < best {
				best = d;
				face = Face::Front;
			}
		}
		(best, face)
	}
}

impl PartShape for ProfiledShape {
	/// Inside when the point lies between the front face and the back within the width: the face is a graph
	/// over y', so the test needs no crossing count.
	fn depth(&self, p: Vec2) -> f64 {
		let distance = self.nearest(p).0;
		let inside = p.y.abs() < self.half_width && p.x < self.half_length && p.x > self.front_at(p.y);
		if inside { distance } else { -distance }
	}

	fn body_index_at(&self, _p: Vec2) -> f64 {
		if self.body.mirror { 0.0 } else { self.body.index }
	}

	fn extent(&self) -> f64 {
		self.extent
	}

	fn depth_is_distance(&self) -> bool {
		true
	}

	fn face_at(&self, p: Vec2) -> Face {
		self.nearest(p).1
	}

	fn is_mirror(&self) -> bool {
		self.body.mirror
	}

	fn band(&self) -> Band {
		self.body.band
	}

	fn absorption(&self) -> u32 {
		self.body.absorption
	}

	fn absorption_ramp(&self) -> f64 {
		self.body.ramp
	}
}

#[cfg(test)]
mod tests {
	use super::super::polygon_depth;
	use super::*;
	use crate::model::{Coating, Diffuser, Material};

	/// The searched distance equals the plain polygon distance at every pixel centre around the plate.
	#[test]
	fn the_search_finds_the_polygon_distance() {
		let d = Diffuser { length: 10, width: 40, amplitude: 3.0, ..Diffuser::default() };
		let plate = d.plate("d", 3);
		let polygon: Vec<Vec2> = plate.vertices().into_iter().map(|(x, y)| Vec2::new(x, y)).collect();
		let shape = ProfiledShape::new(plate, Body::of(&Material::default(), &Coating::default()));
		for y in -25..25 {
			for x in -12..12 {
				let p = Vec2::new(x as f64 + 0.5, y as f64 + 0.5);
				let want = polygon_depth(&polygon, p);
				assert!((shape.depth(p) - want).abs() < 1e-9, "at {p:?}: {} vs {want}", shape.depth(p));
			}
		}
		assert_eq!(shape.face_at(Vec2::new(4.9, 0.0)), Face::Back);
		assert_eq!(shape.face_at(Vec2::new(0.0, 19.9)), Face::Sides);
		assert_eq!(shape.face_at(Vec2::new(shape.front_at(3.0) + 0.1, 3.0)), Face::Front);
	}
}
