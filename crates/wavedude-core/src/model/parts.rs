use serde::{Deserialize, Serialize};

use super::Point;

use super::ranges;

/// Largest coating depth in pixels.
pub const MAX_COATING: f64 = ranges::COATING.max;
/// Refractive index bounds of a part.
pub const INDEX_MIN: f64 = ranges::INDEX.min;
pub const INDEX_MAX: f64 = ranges::INDEX.max;
/// Largest absorption ramp in pixels.
pub const MAX_ABSORPTION_RAMP: u32 = ranges::ABSORPTION_RAMP.max as u32;

/// What every part has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct PartBase {
	pub name: String,
	pub enabled: bool,
	/// The centre is the top-left corner of pixel (x, y).
	pub x: i32,
	pub y: i32,
	/// Degrees, counter-clockwise on screen.
	pub angle: f64,
}

impl Default for PartBase {
	fn default() -> Self {
		Self { name: String::new(), enabled: true, x: 0, y: 0, angle: 0.0 }
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CoatingProfile {
	/// Quarter-wave layer at sqrt(n).
	#[default]
	Layer,
	/// Raised cosine from the outside index to the body.
	Graded,
}

/// Anti-reflection band on chosen faces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Coating {
	/// Pixels, 0..MAX_COATING.
	pub depth: f64,
	pub profile: CoatingProfile,
	pub front: bool,
	pub back: bool,
	pub sides: bool,
}

impl Default for Coating {
	fn default() -> Self {
		Self { depth: 0.0, profile: CoatingProfile::Layer, front: true, back: true, sides: true }
	}
}

/// Glass or mirror body shared by the lens and the shapes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Material {
	pub mirror: bool,
	pub refractive_index: f64,
	/// Blue level at full share, 0..255.
	pub absorption: u32,
	/// Pixels from the outline over which absorption rises, 1..400.
	pub absorption_ramp: u32,
}

impl Default for Material {
	fn default() -> Self {
		Self { mirror: false, refractive_index: 1.5, absorption: 0, absorption_ramp: 40 }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Lens {
	pub material: Material,
	pub coating: Coating,
	pub diameter: u32,
	/// Front radius; 0 = flat; > 0 convex.
	pub r1: f64,
	/// Back radius; 0 = flat; < 0 convex.
	pub r2: f64,
	pub thickness: u32,
}

impl Default for Lens {
	fn default() -> Self {
		Self {
			material: Material::default(),
			coating: Coating::default(),
			diameter: 200,
			r1: 300.0,
			r2: -300.0,
			thickness: 40,
		}
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Polygon {
	pub material: Material,
	pub coating: Coating,
	pub sides: u32,
	/// Side length in pixels.
	pub side: u32,
}

impl Default for Polygon {
	fn default() -> Self {
		Self { material: Material::default(), coating: Coating::default(), sides: 6, side: 60 }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Rectangle {
	pub material: Material,
	pub coating: Coating,
	/// Along x'.
	pub length: u32,
	/// Along y'.
	pub width: u32,
	/// Length and width edited together.
	pub linked: bool,
}

impl Default for Rectangle {
	fn default() -> Self {
		Self { material: Material::default(), coating: Coating::default(), length: 60, width: 60, linked: false }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Ellipse {
	pub material: Material,
	pub coating: Coating,
	pub length: u32,
	pub width: u32,
	pub linked: bool,
}

impl Default for Ellipse {
	fn default() -> Self {
		Self { material: Material::default(), coating: Coating::default(), length: 60, width: 60, linked: false }
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum TriangleType {
	#[default]
	Equilateral,
	RightIsosceles,
	ThirtySixtyNinety,
	Isosceles,
	Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Triangle {
	pub material: Material,
	pub coating: Coating,
	pub base: u32,
	pub kind: TriangleType,
	/// Degrees at the base's ends.
	pub angle_a: f64,
	pub angle_b: f64,
}

impl Default for Triangle {
	fn default() -> Self {
		Self {
			material: Material::default(),
			coating: Coating::default(),
			base: 60,
			kind: TriangleType::Equilateral,
			angle_a: 60.0,
			angle_b: 60.0,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Outline {
	pub material: Material,
	pub coating: Coating,
	/// Part-frame vertices; fewer than 3 paint nothing.
	pub points: Vec<Point>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FiberEnds {
	#[default]
	Flat,
	Round,
}

/// A Bezier node in the part frame with handles relative to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct FiberNode {
	pub x: i32,
	pub y: i32,
	pub in_x: i32,
	pub in_y: i32,
	pub out_x: i32,
	pub out_y: i32,
	/// Dragging one handle mirrors the other (editor only).
	pub smooth: bool,
}

impl Default for FiberNode {
	fn default() -> Self {
		Self { x: 0, y: 0, in_x: 0, in_y: 0, out_x: 0, out_y: 0, smooth: true }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Fiber {
	pub coating: Coating,
	pub core_index: f64,
	pub core_diameter: u32,
	pub cladding_index: f64,
	pub cladding_thickness: u32,
	/// Blue level of the cladding's mode stripper, 0..255.
	pub absorption: u32,
	pub absorption_ramp: u32,
	pub ends: FiberEnds,
	pub nodes: Vec<FiberNode>,
}

impl Default for Fiber {
	fn default() -> Self {
		Self {
			coating: Coating::default(),
			core_index: 1.5,
			core_diameter: 20,
			cladding_index: 1.45,
			cladding_thickness: 10,
			absorption: 0,
			absorption_ramp: 10,
			ends: FiberEnds::Flat,
			nodes: vec![
				FiberNode { x: -100, out_x: 67, ..FiberNode::default() },
				FiberNode { x: 100, in_x: -67, ..FiberNode::default() },
			],
		}
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct BraggReflector {
	pub pairs: u32,
	pub thickness1: u32,
	pub thickness2: u32,
	pub index1: f64,
	pub index2: f64,
	pub width: u32,
}

impl Default for BraggReflector {
	fn default() -> Self {
		Self { pairs: 10, thickness1: 10, thickness2: 10, index1: 1.5, index2: 2.0, width: 200 }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct GrinLens {
	pub coating: Coating,
	pub length: u32,
	pub diameter: u32,
	pub centre_index: f64,
	pub edge_index: f64,
}

impl Default for GrinLens {
	fn default() -> Self {
		Self { coating: Coating::default(), length: 100, diameter: 100, centre_index: 1.8, edge_index: 1.5 }
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum GateKind {
	Xor,
	And,
	Or,
}

/// A waveguide logic gate; the layout is computed from the design frequency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct LogicGate {
	pub kind: GateKind,
	/// Level 1..255.
	pub design_frequency: f64,
	/// Pixels, 0..MAX_COATING.
	pub coating: f64,
	pub input_coated: bool,
	pub output_coated: bool,
	pub upper_output_active: bool,
	pub lower_output_active: bool,
	pub core_index: f64,
	pub cladding_index: f64,
	/// Pixels, raised to the design's minimum.
	pub port_spacing: u32,
}

impl Default for LogicGate {
	fn default() -> Self {
		Self {
			kind: GateKind::Xor,
			design_frequency: 50.0,
			coating: 6.0,
			input_coated: true,
			output_coated: true,
			upper_output_active: true,
			lower_output_active: true,
			core_index: 2.55,
			cladding_index: 1.45,
			port_spacing: 2,
		}
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct ZonePlate {
	pub mount: bool,
	pub mount_width: u32,
	pub diameter: u32,
	pub thickness: u32,
	/// Pixels over which the block absorption rises, 0..400.
	pub coating: u32,
	/// Level 1..255.
	pub design_frequency: f64,
	pub focal_length: u32,
	pub inverse: bool,
}

impl Default for ZonePlate {
	fn default() -> Self {
		Self {
			mount: false,
			mount_width: 200,
			diameter: 800,
			thickness: 32,
			coating: 0,
			design_frequency: 50.0,
			focal_length: 600,
			inverse: false,
		}
	}
}

/// The kind-specific part of a part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PartKind {
	Lens(Lens),
	Polygon(Polygon),
	Rectangle(Rectangle),
	Ellipse(Ellipse),
	Triangle(Triangle),
	Outline(Outline),
	Fiber(Fiber),
	BraggReflector(BraggReflector),
	GrinLens(GrinLens),
	LogicGate(LogicGate),
	ZonePlate(ZonePlate),
}

/// An optical part painted into the scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Part {
	#[serde(flatten)]
	pub base: PartBase,
	pub kind: PartKind,
}

impl PartKind {
	/// Name stem for new parts of this kind.
	pub fn name_stem(&self) -> &'static str {
		match self {
			PartKind::Lens(l) if l.material.mirror => "Mirror",
			PartKind::Lens(_) => "Lens",
			PartKind::Polygon(_) => "Polygon",
			PartKind::Rectangle(_) => "Rectangle",
			PartKind::Ellipse(_) => "Ellipse",
			PartKind::Triangle(_) => "Triangle",
			PartKind::Outline(_) => "Outline",
			PartKind::Fiber(_) => "Fiber",
			PartKind::BraggReflector(_) => "Bragg reflector",
			PartKind::GrinLens(_) => "GRIN lens",
			PartKind::LogicGate(g) => match g.kind {
				GateKind::Xor => "XOR gate",
				GateKind::And => "AND gate",
				GateKind::Or => "OR gate",
			},
			PartKind::ZonePlate(_) => "Zone plate",
		}
	}

	pub fn material(&self) -> Option<&Material> {
		match self {
			PartKind::Lens(p) => Some(&p.material),
			PartKind::Polygon(p) => Some(&p.material),
			PartKind::Rectangle(p) => Some(&p.material),
			PartKind::Ellipse(p) => Some(&p.material),
			PartKind::Triangle(p) => Some(&p.material),
			PartKind::Outline(p) => Some(&p.material),
			PartKind::Fiber(_)
			| PartKind::BraggReflector(_)
			| PartKind::GrinLens(_)
			| PartKind::LogicGate(_)
			| PartKind::ZonePlate(_) => None,
		}
	}

	pub fn coating(&self) -> Option<&Coating> {
		match self {
			PartKind::Lens(p) => Some(&p.coating),
			PartKind::Polygon(p) => Some(&p.coating),
			PartKind::Rectangle(p) => Some(&p.coating),
			PartKind::Ellipse(p) => Some(&p.coating),
			PartKind::Triangle(p) => Some(&p.coating),
			PartKind::Outline(p) => Some(&p.coating),
			PartKind::Fiber(p) => Some(&p.coating),
			PartKind::GrinLens(p) => Some(&p.coating),
			PartKind::BraggReflector(_) | PartKind::LogicGate(_) | PartKind::ZonePlate(_) => None,
		}
	}
}

impl Part {
	pub fn new(kind: PartKind, x: i32, y: i32) -> Self {
		Self { base: PartBase { x, y, ..PartBase::default() }, kind }
	}
}
