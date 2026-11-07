use super::{Band, Face, PartShape, Vec2, absorption_profile, segment_distance};
use crate::model::{Fiber, FiberEnds, FiberNode};

/// Largest distance of a chord from the curve, in pixels.
pub const FLATTEN_TOLERANCE: f64 = 0.05;
/// Side of a chord-grid cell, in pixels of the part frame.
const CELL_SIZE: f64 = 32.0;
/// How far beyond the tube a chord is still listed for a cell; an empty cell reports this depth outside.
const MARGIN: f64 = 2.0;

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
	let d = b - a;
	segment_distance(a, d, d.x * d.x + d.y * d.y, p)
}

/// The path through `nodes` as chord end points in the part frame: each cubic segment (node,
/// node + Out, next + In, next) is one chord when both handles lie within the tolerance of the
/// chord, else `clamp(ceil(sqrt(0.75 * second / 0.05)), 1, 512)` equal steps of t (second: the
/// larger second difference of the control polygon). Consecutive equal points are dropped; fewer
/// than two points give an empty path.
pub fn flatten(nodes: &[FiberNode]) -> Vec<Vec2> {
	if nodes.len() < 2 {
		return Vec::new();
	}
	let mut path = vec![Vec2::new(nodes[0].x as f64, nodes[0].y as f64)];
	for pair in nodes.windows(2) {
		let (a, b) = (&pair[0], &pair[1]);
		let p0 = Vec2::new(a.x as f64, a.y as f64);
		let p1 = Vec2::new((a.x + a.out_x) as f64, (a.y + a.out_y) as f64);
		let p2 = Vec2::new((b.x + b.in_x) as f64, (b.y + b.in_y) as f64);
		let p3 = Vec2::new(b.x as f64, b.y as f64);
		let mut steps = 1;
		if distance_to_segment(p1, p0, p3) > FLATTEN_TOLERANCE || distance_to_segment(p2, p0, p3) > FLATTEN_TOLERANCE {
			let second = Vec2::new(p0.x - 2.0 * p1.x + p2.x, p0.y - 2.0 * p1.y + p2.y)
				.length()
				.max(Vec2::new(p1.x - 2.0 * p2.x + p3.x, p1.y - 2.0 * p2.y + p3.y).length());
			steps = ((0.75 * second / FLATTEN_TOLERANCE).sqrt().ceil() as i64).clamp(1, 512);
		}
		for k in 1..=steps {
			let t = k as f64 / steps as f64;
			let u = 1.0 - t;
			let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
			let point =
				Vec2::new(w0 * p0.x + w1 * p1.x + w2 * p2.x + w3 * p3.x, w0 * p0.y + w1 * p1.y + w2 * p2.y + w3 * p3.y);
			if Some(&point) != path.last() {
				path.push(point);
			}
		}
	}
	if path.len() < 2 { Vec::new() } else { path }
}

fn unit(x: f64, y: f64) -> Option<Vec2> {
	let length = (x * x + y * y).sqrt();
	(length > 0.0).then(|| Vec2::new(x / length, y / length))
}

/// One pass over a point's chords: depth, distance to the path, and the raw face depths.
struct Eval {
	depth: f64,
	path: f64,
	start: f64,
	end: f64,
}

/// A fibre: the tube of the outer radius around the flattened path, the core the inner tube, the
/// ends flat (cut by the planes across the end tangents) or round. Depth is the largest over the
/// chords listed for the point's 32 px grid cell, clamped at -2.
pub(super) struct FiberShape {
	path: Vec<Vec2>,
	/// Per chord i (from path[i-1] to path[i]): run, length, squared length.
	chord_d: Vec<Vec2>,
	chord_length: Vec<f64>,
	chord_length2: Vec<f64>,
	flat: bool,
	start_tangent: Vec2,
	end_tangent: Vec2,
	core_radius: f64,
	outer_radius: f64,
	core_index: f64,
	cladding_index: f64,
	has_inner_edges: bool,
	band: Band,
	absorption: u32,
	/// Half-width of the cladding's absorbing hump.
	hump: f64,
	extent: f64,
	grid: Vec2,
	columns: i64,
	rows: i64,
	cell_start: Vec<usize>,
	cell_chords: Vec<usize>,
	start_zone: usize,
	end_zone: usize,
	allowance: f64,
}

impl FiberShape {
	/// None when the path has fewer than two points.
	pub fn new(fiber: &Fiber) -> Option<Self> {
		let path = flatten(&fiber.nodes);
		if path.len() < 2 {
			return None;
		}
		let n = path.len();
		let mut chord_d = vec![Vec2::default(); n];
		let mut chord_length = vec![0.0; n];
		let mut chord_length2 = vec![0.0; n];
		for i in 1..n {
			let d = path[i] - path[i - 1];
			chord_d[i] = d;
			chord_length2[i] = d.x * d.x + d.y * d.y;
			chord_length[i] = (d.x * d.x + d.y * d.y).sqrt();
		}
		let first = &fiber.nodes[0];
		let last = &fiber.nodes[fiber.nodes.len() - 1];
		let start_tangent = unit(first.out_x as f64, first.out_y as f64)
			.or_else(|| unit(chord_d[1].x, chord_d[1].y))
			.unwrap_or(Vec2::new(1.0, 0.0));
		let end_tangent = unit(-last.in_x as f64, -last.in_y as f64)
			.or_else(|| unit(chord_d[n - 1].x, chord_d[n - 1].y))
			.unwrap_or(Vec2::new(1.0, 0.0));
		let core_radius = fiber.core_diameter as f64 / 2.0;
		let cladding = fiber.cladding_thickness as f64;
		let outer_radius = core_radius + cladding;
		let core_index = fiber.core_index;
		let flat = fiber.ends == FiberEnds::Flat;
		let hump = (fiber.absorption_ramp as f64).min(cladding / 2.0);
		let extent = path.iter().map(|v| v.length()).fold(f64::NEG_INFINITY, f64::max) + outer_radius;

		let wedge = |tangent: Vec2, d: Vec2| {
			let length = d.length();
			let cos = if length > 0.0 { (tangent.dot(d) / length).clamp(-1.0, 1.0) } else { 1.0 };
			if cos <= 0.0 {
				return f64::INFINITY;
			}
			(outer_radius + MARGIN) * ((1.0 - cos * cos) / (cos * cos)) / 2.0
		};
		let allowance = if flat { wedge(start_tangent, chord_d[1]) + wedge(end_tangent, chord_d[n - 1]) } else { 0.0 };

		let mut start_zone = 1;
		let mut arc = 0.0;
		let mut i = 1;
		while i < n && arc <= 2.0 * outer_radius {
			start_zone = i;
			arc += chord_length[i];
			i += 1;
		}
		let mut end_zone = n - 1;
		arc = 0.0;
		let mut i = n - 1;
		while i >= 1 && arc <= 2.0 * outer_radius {
			end_zone = i;
			arc += chord_length[i];
			i -= 1;
		}

		let reach = outer_radius + MARGIN;
		let fold = |f: fn(f64, f64) -> f64, init: f64, get: fn(&Vec2) -> f64| path.iter().map(get).fold(init, f);
		let grid =
			Vec2::new(fold(f64::min, f64::INFINITY, |v| v.x) - reach, fold(f64::min, f64::INFINITY, |v| v.y) - reach);
		let columns =
			(((fold(f64::max, f64::NEG_INFINITY, |v| v.x) + reach - grid.x) / CELL_SIZE).ceil() as i64).max(1);
		let rows = (((fold(f64::max, f64::NEG_INFINITY, |v| v.y) + reach - grid.y) / CELL_SIZE).ceil() as i64).max(1);
		let cell_of = |x: f64, y: f64| {
			(
				(((x - grid.x) / CELL_SIZE).floor() as i64).clamp(0, columns - 1),
				(((y - grid.y) / CELL_SIZE).floor() as i64).clamp(0, rows - 1),
			)
		};
		let mut lists: Vec<Vec<usize>> = vec![Vec::new(); (columns * rows) as usize];
		for i in 1..n {
			let (a, b) = (path[i - 1], path[i]);
			let (c0, r0) = cell_of(a.x.min(b.x) - reach, a.y.min(b.y) - reach);
			let (c1, r1) = cell_of(a.x.max(b.x) + reach, a.y.max(b.y) + reach);
			for r in r0..=r1 {
				for c in c0..=c1 {
					lists[(r * columns + c) as usize].push(i);
				}
			}
		}
		let mut cell_start = Vec::with_capacity(lists.len() + 1);
		cell_start.push(0);
		let mut cell_chords = Vec::new();
		for list in &lists {
			cell_chords.extend_from_slice(list);
			cell_start.push(cell_chords.len());
		}

		Some(Self {
			path,
			chord_d,
			chord_length,
			chord_length2,
			flat,
			start_tangent,
			end_tangent,
			core_radius,
			outer_radius,
			core_index,
			cladding_index: if fiber.cladding_thickness > 0 { fiber.cladding_index } else { core_index },
			has_inner_edges: fiber.cladding_thickness > 0,
			band: Band::of(&fiber.coating),
			absorption: if fiber.cladding_thickness > 0 { fiber.absorption } else { 0 },
			hump,
			extent,
			grid,
			columns,
			rows,
			cell_start,
			cell_chords,
			start_zone,
			end_zone,
			allowance,
		})
	}

	fn chords_near(&self, p: Vec2) -> &[usize] {
		let column = ((p.x - self.grid.x) / CELL_SIZE).floor();
		let row = ((p.y - self.grid.y) / CELL_SIZE).floor();
		if column < 0.0 || column >= self.columns as f64 || row < 0.0 || row >= self.rows as f64 {
			return &[];
		}
		let cell = (row as i64 * self.columns + column as i64) as usize;
		&self.cell_chords[self.cell_start[cell]..self.cell_start[cell + 1]]
	}

	fn face_depths(&self, p: Vec2) -> (f64, f64) {
		let last = self.path[self.path.len() - 1];
		(
			(p.x - self.path[0].x) * self.start_tangent.x + (p.y - self.path[0].y) * self.start_tangent.y,
			(last.x - p.x) * self.end_tangent.x + (last.y - p.y) * self.end_tangent.y,
		)
	}

	/// Distance to the nearest chord listed for `p`'s cell; +inf where none is listed.
	fn path_distance(&self, p: Vec2) -> f64 {
		self.chords_near(p).iter().fold(f64::INFINITY, |nearest, &i| {
			nearest.min(segment_distance(self.path[i - 1], self.chord_d[i], self.chord_length2[i], p))
		})
	}

	fn evaluate(&self, p: Vec2) -> Eval {
		let (start_raw, end_raw) = self.face_depths(p);
		let chords = self.chords_near(p);
		if chords.is_empty() {
			return Eval { depth: -MARGIN, path: f64::INFINITY, start: start_raw, end: end_raw };
		}
		let last = self.path.len() - 1;
		let start_depth = if self.flat { start_raw } else { f64::INFINITY };
		let end_depth = if self.flat { end_raw } else { f64::INFINITY };
		let mut best = f64::NEG_INFINITY;
		let mut nearest = f64::INFINITY;
		for &i in chords {
			let a = self.path[i - 1];
			let d = self.chord_d[i];
			let length = self.chord_length[i];
			let along = ((p.x - a.x) * d.x + (p.y - a.y) * d.y) / length;
			let segment = segment_distance(a, d, self.chord_length2[i], p);
			nearest = nearest.min(segment);
			let mut depth =
				if (0.0..=length).contains(&along) || (i == 1 && start_depth < 0.0) || (i == last && end_depth < 0.0) {
					self.outer_radius - ((p.x - a.x) * d.y - (p.y - a.y) * d.x).abs() / length
				} else {
					self.outer_radius - segment
				};
			if i <= self.start_zone {
				depth = depth.min(start_depth);
			}
			if i >= self.end_zone {
				depth = depth.min(end_depth);
			}
			best = best.max(depth);
		}
		Eval { depth: best.max(-MARGIN), path: nearest, start: start_raw, end: end_raw }
	}

	fn face_of(&self, start: f64, end: f64, path: f64) -> Face {
		if !self.flat {
			return if start < 0.0 {
				Face::Front
			} else if end < 0.0 {
				Face::Back
			} else {
				Face::Sides
			};
		}
		let side = self.outer_radius - path;
		if start <= end && start <= side {
			Face::Front
		} else if end <= side {
			Face::Back
		} else {
			Face::Sides
		}
	}

	fn index_of(&self, path: f64) -> f64 {
		if path <= self.core_radius { self.core_index } else { self.cladding_index }
	}
}

impl PartShape for FiberShape {
	fn depth(&self, p: Vec2) -> f64 {
		self.evaluate(p).depth
	}

	fn body_index_at(&self, p: Vec2) -> f64 {
		self.index_of(self.path_distance(p))
	}

	fn extent(&self) -> f64 {
		self.extent
	}

	fn depth_is_distance(&self) -> bool {
		true
	}

	fn skip_allowance(&self) -> f64 {
		self.allowance
	}

	/// The core boundary's distance `|r - core radius|`; +inf without a cladding.
	fn inner_edge_distance(&self, p: Vec2) -> f64 {
		if self.has_inner_edges { (self.path_distance(p) - self.core_radius).abs() } else { f64::INFINITY }
	}

	/// The core boundary at `|r - core radius|`, the cladding beyond it seen from the core and the core from the cladding.
	fn inner_edge(&self, p: Vec2) -> (f64, f64) {
		let r = self.path_distance(p);
		let far = if r <= self.core_radius { self.cladding_index } else { self.core_index };
		((r - self.core_radius).abs(), far)
	}

	fn depths(&self, p: Vec2) -> (f64, f64) {
		let e = self.evaluate(p);
		(e.depth, if self.has_inner_edges { (e.path - self.core_radius).abs() } else { f64::INFINITY })
	}

	fn sample(&self, p: Vec2) -> (f64, f64) {
		let e = self.evaluate(p);
		(e.depth, self.index_of(e.path))
	}

	fn face_at(&self, p: Vec2) -> Face {
		let e = self.evaluate(p);
		self.face_of(e.start, e.end, e.path)
	}

	fn band(&self) -> Band {
		self.band
	}

	fn absorption(&self) -> u32 {
		self.absorption
	}

	fn absorption_ramp(&self) -> f64 {
		self.hump.ceil().max(1.0)
	}

	/// The cladding's hump by the distance r from the path: 0 up to `outer - 2 hump`, rising as
	/// t^2 to the peak at `outer - hump`, falling as (2 - t)^2 to 0 at the outer radius.
	fn absorption_share(&self, p: Vec2, _depth: f32, _ramp: f64) -> f64 {
		if self.hump <= 0.0 {
			return 0.0;
		}
		let r = self.path_distance(p);
		let inner = self.outer_radius - 2.0 * self.hump;
		if r <= inner || r >= self.outer_radius {
			return 0.0;
		}
		let t = (r - inner) / self.hump;
		absorption_profile(if t <= 1.0 { t } else { 2.0 - t })
	}
}
