//! Part geometry as the painter sees it.
//!
//! Part frame: origin at the part's centre (X, Y), the top-left corner of canvas pixel (X, Y);
//! x' along the part's axis turned by its angle (degrees, counter-clockwise on screen), y' across.
//! All lengths are pixels. Each kind is a struct built from its model struct (composition); the
//! painter maps canvas points into the frame with [`Frame`] and asks the shape.

mod bragg;
mod ellipse;
mod fiber;
mod gate;
mod grating;
mod grin;
mod lens;
mod meta;
mod outline;
mod polygon;
mod profiled;
mod rectangle;
mod stack;
mod triangle;
mod vertex;
mod zone_plate;

pub use fiber::flatten;
pub use vertex::{VertexShape, polygon_depth};

use crate::model::{Coating, CoatingProfile, Material, Part, PartBase, PartKind, Simulation};

/// A point or vector in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
	pub x: f64,
	pub y: f64,
}

impl Vec2 {
	pub const fn new(x: f64, y: f64) -> Self {
		Self { x, y }
	}

	pub fn dot(self, o: Self) -> f64 {
		self.x * o.x + self.y * o.y
	}

	pub fn length(self) -> f64 {
		(self.x * self.x + self.y * self.y).sqrt()
	}
}

impl std::ops::Sub for Vec2 {
	type Output = Self;
	fn sub(self, o: Self) -> Self {
		Self::new(self.x - o.x, self.y - o.y)
	}
}

/// The mapping between canvas coordinates and a part's frame.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
	centre: Vec2,
	cos: f64,
	sin: f64,
}

impl Frame {
	pub fn of(base: &PartBase) -> Self {
		let radians = base.angle.to_radians();
		Self { centre: Vec2::new(base.x as f64, base.y as f64), cos: radians.cos(), sin: radians.sin() }
	}

	/// Canvas point to part frame: `x' = ox cos - oy sin, y' = ox sin + oy cos`, `(ox, oy) = p - centre`.
	pub fn to_frame(&self, canvas: Vec2) -> Vec2 {
		let o = canvas - self.centre;
		Vec2::new(o.x * self.cos - o.y * self.sin, o.x * self.sin + o.y * self.cos)
	}

	/// Part frame to canvas: `X + x' cos + y' sin, Y + y' cos - x' sin`.
	pub fn to_canvas(&self, p: Vec2) -> Vec2 {
		Vec2::new(self.centre.x + p.x * self.cos + p.y * self.sin, self.centre.y + p.y * self.cos - p.x * self.sin)
	}

	pub fn centre(&self) -> Vec2 {
		self.centre
	}
}

/// A face of the outline by its outward normal's x' component `nx`: `nx < -cos 45` front,
/// `nx > cos 45` back, else a side. A fibre: start face front, end face back, tube sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
	Front,
	Back,
	Sides,
}

/// cos 45 deg, the bound on a normal's x' component that makes a face a front or a back.
pub const COS45: f64 = std::f64::consts::FRAC_1_SQRT_2;

impl Face {
	/// The face of an outward normal with x' component `nx`.
	pub fn of_normal(nx: f64) -> Self {
		if nx < -COS45 {
			Face::Front
		} else if nx > COS45 {
			Face::Back
		} else {
			Face::Sides
		}
	}
}

/// The anti-reflection band just inside the outline, applied by the painter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
	/// Thickness in pixels; 0 for none.
	pub width: f64,
	pub profile: CoatingProfile,
	pub front: bool,
	pub back: bool,
	pub sides: bool,
}

impl Band {
	pub const NONE: Band = Band { width: 0.0, profile: CoatingProfile::Layer, front: true, back: true, sides: true };

	/// The model's coating.
	pub fn of(coating: &Coating) -> Self {
		Band {
			width: coating.depth,
			profile: coating.profile,
			front: coating.front,
			back: coating.back,
			sides: coating.sides,
		}
	}

	pub fn all_faces(&self) -> bool {
		self.front && self.back && self.sides
	}

	pub fn coats(&self, face: Face) -> bool {
		match face {
			Face::Front => self.front,
			Face::Back => self.back,
			Face::Sides => self.sides,
		}
	}

	/// The index `depth` px inside a coated face over a body of `body`, the pixel's previous index
	/// being `outside`: a layer is `round_even(sqrt(body), 2)`, a graded band rises as a raised cosine.
	pub fn index(&self, depth: f64, body: f64, outside: f64) -> f64 {
		match self.profile {
			CoatingProfile::Graded => outside + (body - outside) * graded_profile(depth / self.width),
			CoatingProfile::Layer => round_digits(body.sqrt(), 2),
		}
	}
}

/// The graded band's share of the index step at `t` = depth / thickness: `(1 - cos(pi t))/2`, t clamped to 0..1.
pub fn graded_profile(t: f64) -> f64 {
	let t = t.clamp(0.0, 1.0);
	(1.0 - (std::f64::consts::PI * t).cos()) / 2.0
}

/// The share of full absorption at `t` = depth / ramp: `t^2`, t clamped to 0..1.
pub fn absorption_profile(t: f64) -> f64 {
	let t = t.clamp(0.0, 1.0);
	t * t
}

/// Round half to even at `digits` decimals.
pub fn round_digits(v: f64, digits: i32) -> f64 {
	let scale = 10f64.powi(digits);
	(v * scale).round_ties_even() / scale
}

/// Distance from `p` to the segment from `a` along `d`, `d` of squared length `length2`.
pub fn segment_distance(a: Vec2, d: Vec2, length2: f64, p: Vec2) -> f64 {
	let t = if length2 > 0.0 { (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / length2).clamp(0.0, 1.0) } else { 0.0 };
	let px = a.x + t * d.x - p.x;
	let py = a.y + t * d.y - p.y;
	(px * px + py * py).sqrt()
}

/// The geometry of one part in its frame. Immutable once built; any thread.
pub trait PartShape: Send + Sync {
	/// How far inside the outline `p` lies: > 0 inside, < 0 outside. A signed Euclidean distance
	/// where [`depth_is_distance`](Self::depth_is_distance) holds.
	fn depth(&self, p: Vec2) -> f64;

	/// The body index at `p`, a point inside, without a coating band; 0 for a mirror.
	fn body_index_at(&self, p: Vec2) -> f64;

	/// Farthest the outline reaches from the centre (a bound).
	fn extent(&self) -> f64;

	/// Whether depth and inner-edge distance are 1-Lipschitz (true distances).
	fn depth_is_distance(&self) -> bool {
		false
	}

	/// Distance to the nearest edge inside the outline where the index steps; +inf without one.
	fn inner_edge_distance(&self, _p: Vec2) -> f64 {
		f64::INFINITY
	}

	/// The distance from `p` to the nearest inner edge, measured on `p`'s side (>= 0), and the body index
	/// on the edge's far side. Asked only of a distance shape where
	/// [`inner_edge_distance`](Self::inner_edge_distance) is finite.
	fn inner_edge(&self, _p: Vec2) -> (f64, f64) {
		unreachable!("inner_edge asked of a kind without inner edges")
	}

	/// Depth and inner-edge distance in one pass.
	fn depths(&self, p: Vec2) -> (f64, f64) {
		(self.depth(p), self.inner_edge_distance(p))
	}

	/// Depth and body index in one pass; the body index is meaningful only at depth >= 0.
	fn sample(&self, p: Vec2) -> (f64, f64) {
		let depth = self.depth(p);
		if depth < 0.0 { (depth, f64::NAN) } else { (depth, self.body_index_at(p)) }
	}

	/// None outside, Some(0) inside a mirror, else the body index (no coating band).
	fn index_at(&self, p: Vec2) -> Option<f64> {
		let (depth, body) = self.sample(p);
		if depth < 0.0 {
			None
		} else if self.is_mirror() {
			Some(0.0)
		} else {
			Some(body)
		}
	}

	/// The face nearest `p`; asked only inside a band that leaves a face bare.
	fn face_at(&self, _p: Vec2) -> Face {
		Face::Front
	}

	fn is_mirror(&self) -> bool {
		false
	}

	/// The coating band; [`Band::NONE`] for kinds without one.
	fn band(&self) -> Band {
		Band::NONE
	}

	/// Blue level at full share, 0..255; 0 paints no absorption.
	fn absorption(&self) -> u32 {
		0
	}

	/// Pixels over which the default absorption rises, at least 1.
	fn absorption_ramp(&self) -> f64 {
		1.0
	}

	/// Share of [`absorption`](Self::absorption) at the pixel centre `p`, `depth` (f32) inside,
	/// `ramp` the painter's ramp (shortened to the deepest centre). Default: `t^2` of depth/ramp.
	fn absorption_share(&self, _p: Vec2, depth: f32, ramp: f64) -> f64 {
		absorption_profile(if ramp > 0.0 { depth as f64 / ramp } else { 1.0 })
	}

	/// Whether the kind paints red; false for a zone plate (blue only).
	fn paints_index(&self) -> bool {
		true
	}
}

/// What the lens and the vertex shapes share: index, mirror, coating, absorption.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Body {
	pub index: f64,
	pub mirror: bool,
	pub band: Band,
	pub absorption: u32,
	pub ramp: f64,
}

impl Body {
	/// A mirror carries no band and no absorption.
	pub(crate) fn of(material: &Material, coating: &Coating) -> Self {
		let mirror = material.mirror;
		Body {
			index: material.refractive_index,
			mirror,
			band: if mirror { Band::NONE } else { Band::of(coating) },
			absorption: if mirror { 0 } else { material.absorption },
			ramp: material.absorption_ramp as f64,
		}
	}
}

/// The shapes the painter paints for a normalised `part`, in order: one for every kind but an interference
/// filter, which is its plate and its stack (a mirror plate is painted last, so its pixels stay red 0; a glass
/// plate first, so the stack's edge pixels average with it). Empty when the part has nothing to paint (an outline
/// of fewer than three points, a fibre path of fewer than two, a gate without a design, a zone plate without a
/// wavelength). `sim` gives the frequency scale and stencil for the frequency-designed kinds, `seed` the project's
/// seed for a diffuser's roughness.
pub fn shapes_for(part: &Part, sim: &Simulation, seed: i32) -> Vec<Box<dyn PartShape>> {
	match &part.kind {
		PartKind::InterferenceFilter(f) => {
			let plate = plate_shape(f.length, f.width, &f.material, &f.coating);
			let stack = stack::StackShape::new(&f.layers, f.length, f.width).map(|s| Box::new(s) as Box<dyn PartShape>);
			let plate = Box::new(plate) as Box<dyn PartShape>;
			match (stack, f.material.mirror) {
				(None, _) => vec![plate],
				(Some(stack), true) => vec![stack, plate],
				(Some(stack), false) => vec![plate, stack],
			}
		}
		PartKind::MetaSurface(m) => {
			// the plate and its pattern, as the filter's plate and stack
			let plate = Box::new(plate_shape(m.length, m.width, &m.material, &m.coating)) as Box<dyn PartShape>;
			let pattern =
				meta::MetaPatternShape::new(m, sim.source_frequency_scale).map(|s| Box::new(s) as Box<dyn PartShape>);
			match (pattern, m.material.mirror) {
				(None, _) => vec![plate],
				(Some(pattern), true) => vec![pattern, plate],
				(Some(pattern), false) => vec![plate, pattern],
			}
		}
		_ => shape_for(part, sim, seed).into_iter().collect(),
	}
}

/// A carrier plate of `length` by `width`, the band on its back alone.
fn plate_shape(length: u32, width: u32, material: &Material, coating: &Coating) -> VertexShape {
	let coating = Coating { front: false, sides: false, ..coating.clone() };
	let rectangle = rectangle::vertices(&crate::model::Rectangle { length, width, ..Default::default() });
	VertexShape::new(rectangle, material, &coating).expect("a rectangle has four vertices")
}

/// The one shape of a normalised `part` whose kind paints as one; see [`shapes_for`].
pub fn shape_for(part: &Part, sim: &Simulation, seed: i32) -> Option<Box<dyn PartShape>> {
	Some(match &part.kind {
		PartKind::Lens(l) => Box::new(lens::LensShape::new(l)),
		PartKind::FresnelLens(f) => Box::new(lens::LensShape::zoned(&f.lens, f.zones, f.division)),
		PartKind::Polygon(p) => Box::new(VertexShape::new(polygon::vertices(p), &p.material, &p.coating)?),
		PartKind::Rectangle(r) => Box::new(VertexShape::new(rectangle::vertices(r), &r.material, &r.coating)?),
		PartKind::Ellipse(e) => Box::new(VertexShape::new(ellipse::vertices(e), &e.material, &e.coating)?),
		PartKind::Triangle(t) => Box::new(VertexShape::new(triangle::vertices(t), &t.material, &t.coating)?),
		PartKind::Outline(o) => Box::new(VertexShape::new(outline::vertices(o), &o.material, &o.coating)?),
		PartKind::Diffuser(d) => {
			let plate = d.plate(&part.base.name, seed);
			Box::new(profiled::ProfiledShape::new(plate, Body::of(&d.material, &d.coating)))
		}
		PartKind::FacettedGrating(g) => {
			// the band on the back alone
			let coating = Coating { front: false, sides: false, ..g.coating.clone() };
			Box::new(profiled::ProfiledShape::new(g.plate(), Body::of(&g.material, &coating)))
		}
		PartKind::InterferenceFilter(f) => Box::new(plate_shape(f.length, f.width, &f.material, &f.coating)),
		PartKind::SimpleGrating(g) => Box::new(grating::GratingShape::new(g)),
		PartKind::MetaSurface(m) => Box::new(plate_shape(m.length, m.width, &m.material, &m.coating)),
		PartKind::Fiber(f) => Box::new(fiber::FiberShape::new(f)?),
		PartKind::BraggReflector(b) => Box::new(bragg::BraggShape::new(b)),
		PartKind::GrinLens(g) => Box::new(grin::GrinShape::new(g)),
		PartKind::LogicGate(g) => Box::new(gate::GateShape::new(g, sim)?),
		PartKind::ZonePlate(z) => Box::new(zone_plate::ZonePlateShape::new(z, sim)?),
	})
}

/// The part-frame vertices of a normalised polygon-type kind (polygon, rectangle, ellipse, triangle, outline); None for other kinds.
pub fn vertices(kind: &PartKind) -> Option<Vec<Vec2>> {
	Some(match kind {
		PartKind::Polygon(p) => polygon::vertices(p),
		PartKind::Rectangle(r) => rectangle::vertices(r),
		PartKind::Ellipse(e) => ellipse::vertices(e),
		PartKind::Triangle(t) => triangle::vertices(t),
		PartKind::Outline(o) => outline::vertices(o),
		PartKind::Lens(_)
		| PartKind::FresnelLens(_)
		| PartKind::Diffuser(_)
		| PartKind::InterferenceFilter(_)
		| PartKind::SimpleGrating(_)
		| PartKind::FacettedGrating(_)
		| PartKind::MetaSurface(_)
		| PartKind::Fiber(_)
		| PartKind::BraggReflector(_)
		| PartKind::GrinLens(_)
		| PartKind::LogicGate(_)
		| PartKind::ZonePlate(_) => return None,
	})
}

/// The part-frame outline the overlay draws for a kind that paints as no polygon of its own: a profiled plate's
/// (a diffuser, a facetted grating) front face pixel by pixel and the back corners, the box around a simple
/// grating's lattice with the dots' radius; None for other kinds.
pub fn outline_of(part: &Part, seed: i32) -> Option<Vec<Vec2>> {
	let plate = match &part.kind {
		PartKind::Diffuser(d) => d.plate(&part.base.name, seed),
		PartKind::FacettedGrating(g) => g.plate(),
		PartKind::MetaSurface(m) => {
			let back = m.length.max(1) as f64 / 2.0;
			let front = -back - m.guarded_height();
			let w = m.width.max(2) as f64 / 2.0;
			return Some(vec![Vec2::new(front, -w), Vec2::new(back, -w), Vec2::new(back, w), Vec2::new(front, w)]);
		}
		PartKind::SimpleGrating(g) => {
			let r = g.guarded_diameter() as f64 / 2.0;
			let l =
				(g.columns.max(1) - 1) as f64 / 2.0 * crate::model::SimpleGrating::spacing(g.column_spacing) as f64 + r;
			let w = (g.rows.max(1) - 1) as f64 / 2.0 * crate::model::SimpleGrating::spacing(g.row_spacing) as f64 + r;
			return Some(vec![Vec2::new(-l, -w), Vec2::new(l, -w), Vec2::new(l, w), Vec2::new(-l, w)]);
		}
		_ => return None,
	};
	Some(plate.vertices().into_iter().map(|(x, y)| Vec2::new(x, y)).collect())
}

pub use ellipse::vertex_count as ellipse_vertex_count;
pub use zone_plate::{BLOCK_ABSORPTION, zone_at};
