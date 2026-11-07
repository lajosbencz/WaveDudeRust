use super::{Band, Body, Face, PartShape, Vec2, segment_distance};
use crate::model::{Coating, Material};

/// Signed distance of `p` inside a simple polygon: the distance to the nearest edge, negative
/// outside (even-odd rule).
pub fn polygon_depth(vertices: &[Vec2], p: Vec2) -> f64 {
	let mut inside = false;
	let mut nearest = f64::INFINITY;
	let mut j = vertices.len() - 1;
	for (i, &b) in vertices.iter().enumerate() {
		let a = vertices[j];
		if (a.y > p.y) != (b.y > p.y) {
			let cross = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
			if p.x < cross {
				inside = !inside;
			}
		}
		let d = b - a;
		nearest = nearest.min(segment_distance(a, d, d.x * d.x + d.y * d.y, p));
		j = i;
	}
	if inside { nearest } else { -nearest }
}

/// A polygon, rectangle, ellipse, triangle or outline: its vertices as a simple polygon.
pub struct VertexShape {
	vertices: Vec<Vec2>,
	/// Face of the edge ending at vertex i (the edge from i-1 to i).
	faces: Vec<Face>,
	body: Body,
	extent: f64,
}

impl VertexShape {
	/// None for fewer than three vertices.
	pub fn new(vertices: Vec<Vec2>, material: &Material, coating: &Coating) -> Option<Self> {
		if vertices.len() < 3 {
			return None;
		}
		let extent = vertices.iter().map(|v| v.length()).fold(f64::NEG_INFINITY, f64::max);
		let n = vertices.len();
		let faces = (0..n)
			.map(|i| {
				let a = vertices[(i + n - 1) % n];
				let b = vertices[i];
				let (dx, dy) = (b.x - a.x, b.y - a.y);
				let length = (dx * dx + dy * dy).sqrt();
				if length == 0.0 {
					return Face::Sides;
				}
				let (mut nx, ny) = (dy / length, -dx / length);
				let probe = Vec2::new((a.x + b.x) / 2.0 + nx * 0.01, (a.y + b.y) / 2.0 + ny * 0.01);
				if polygon_depth(&vertices, probe) > 0.0 {
					nx = -nx;
				}
				Face::of_normal(nx)
			})
			.collect();
		Some(Self { vertices, faces, body: Body::of(material, coating), extent })
	}

	pub fn vertices(&self) -> &[Vec2] {
		&self.vertices
	}
}

impl PartShape for VertexShape {
	fn depth(&self, p: Vec2) -> f64 {
		polygon_depth(&self.vertices, p)
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

	/// The face of the nearest edge (the first of equals).
	fn face_at(&self, p: Vec2) -> Face {
		let n = self.vertices.len();
		let mut best = f64::INFINITY;
		let mut nearest = 0;
		for i in 0..n {
			let a = self.vertices[(i + n - 1) % n];
			let d = self.vertices[i] - a;
			let distance = segment_distance(a, d, d.x * d.x + d.y * d.y, p);
			if distance < best {
				best = distance;
				nearest = i;
			}
		}
		self.faces[nearest]
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
