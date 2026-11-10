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

/// How a Fresnel lens's zones are divided from the axis to the rim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ZoneDivision {
	/// Every zone the same width; the outer one the deepest.
	#[default]
	EqualWidth,
	/// Every zone the same share of the rim's sag; the zones narrow outward.
	EqualDepth,
}

/// A lens whose curved surfaces are cut into zones, each zone's arc starting again at the vertex plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct FresnelLens {
	#[serde(flatten)]
	pub lens: Lens,
	/// 1..MAX_ZONES; 1 is a plain lens.
	pub zones: u32,
	pub division: ZoneDivision,
}

impl Default for FresnelLens {
	fn default() -> Self {
		Self { lens: Lens::default(), zones: 4, division: ZoneDivision::EqualWidth }
	}
}

impl FresnelLens {
	pub const MAX_ZONES: u32 = 100;

	/// A new Fresnel lens: a plate of 20 px across 500 with radii of 1500, so four zones of equal width fit
	/// (the lens's defaults would be refit to a far flatter lens).
	pub fn new_plate() -> Self {
		Self {
			lens: Lens { diameter: 500, r1: 1500.0, r2: -1500.0, thickness: 20, ..Lens::default() },
			..Self::default()
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

/// A plate whose front face (-x') is rough: a sum of ripples about a centre wavelength, their phases drawn from
/// the project's seed and the part's name (`profiled.rs`); the back is flat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Diffuser {
	pub material: Material,
	pub coating: Coating,
	/// The plate's thickness at the mean of its rough face.
	pub length: u32,
	pub width: u32,
	/// Pixels, 2..MAX_PIXELS: the ripple with the full amplitude.
	pub center_wavelength: f64,
	/// Pixels, 0..MAX_PIXELS.
	pub amplitude: f64,
	/// Spread of the ripples' wavelengths about the centre in pixels, 0..MAX_PIXELS; 0 is the centre alone.
	pub sigma: f64,
}

impl Default for Diffuser {
	fn default() -> Self {
		Self {
			material: Material::default(),
			coating: Coating::default(),
			length: 50,
			width: 500,
			center_wavelength: 16.0,
			amplitude: 1.0,
			sigma: 6.0,
		}
	}
}

impl Diffuser {
	/// A new diffuser coats its flat back alone; a loaded one keeps what its file says.
	pub fn new_part() -> Self {
		Self { coating: Coating { front: false, sides: false, ..Coating::default() }, ..Self::default() }
	}
}

/// One layer of an interference filter's stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct FilterLayer {
	/// Pixels along the axis, 0..MAX_LAYER_THICKNESS at 0.01; 0 leaves the layer out.
	pub thickness: f64,
	pub refractive_index: f64,
}

impl Default for FilterLayer {
	fn default() -> Self {
		Self { thickness: 2.0, refractive_index: 1.5 }
	}
}

/// A carrier plate with a stack of thin layers on its front face (-x'), outside the plate; the first layer is
/// the outermost. A mirror plate keeps its layers glass in front of it; the band lies on the plate's back alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct InterferenceFilter {
	pub material: Material,
	/// Only `back` counts.
	pub coating: Coating,
	/// The plate's, without the stack.
	pub length: u32,
	pub width: u32,
	pub layers: Vec<FilterLayer>,
}

impl Default for InterferenceFilter {
	fn default() -> Self {
		Self { material: Material::default(), coating: Coating::default(), length: 50, width: 500, layers: Vec::new() }
	}
}

impl InterferenceFilter {
	pub const MAX_LAYER_THICKNESS: f64 = 1000.0;
	/// The wavelength a new filter's stack is cut for.
	pub const STACK_WAVELENGTH: f64 = 16.0;

	/// A new filter: a quarter-wave stack for [`Self::STACK_WAVELENGTH`] that begins and ends with the high
	/// index, five pairs of 2.00 then 1.50 from the outside in and one more 2.00 on the plate.
	pub fn new_part() -> Self {
		let quarter = |index: f64| FilterLayer {
			refractive_index: index,
			thickness: (Self::STACK_WAVELENGTH / (4.0 * index) * 100.0).round() / 100.0,
		};
		let mut layers: Vec<FilterLayer> = (0..5).flat_map(|_| [quarter(2.0), quarter(1.5)]).collect();
		layers.push(quarter(2.0));
		Self { layers, ..Self::default() }
	}

	/// The sum of the layers' thicknesses: how far the stack stands out of the plate's front.
	pub fn stack_thickness(&self) -> f64 {
		self.layers.iter().map(|l| l.thickness.clamp(0.0, Self::MAX_LAYER_THICKNESS)).sum()
	}
}

/// The relief of a facetted grating over each pitch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FacetPattern {
	/// A ramp from the crest to the trough, then a step back: a blazed grating.
	#[default]
	Sawtooth,
	/// A crest over half the pitch and a trough over the other half.
	SquareWave,
	Sinusoidal,
}

/// A plate whose front face (-x') carries a periodic relief; the back is flat and alone takes the band.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct FacettedGrating {
	pub material: Material,
	/// Only `back` counts.
	pub coating: Coating,
	pub length: u32,
	pub width: u32,
	pub pattern: FacetPattern,
	/// Period across the face in pixels, 2..MAX_PIXELS.
	pub pitch: f64,
	/// The relief's height either side of the mean face in pixels, 0..MAX_PIXELS.
	pub amplitude: f64,
}

impl Default for FacettedGrating {
	fn default() -> Self {
		Self {
			material: Material::default(),
			coating: Coating::default(),
			length: 50,
			width: 500,
			pattern: FacetPattern::Sawtooth,
			pitch: 32.0,
			amplitude: 4.0,
		}
	}
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
	FresnelLens(FresnelLens),
	Polygon(Polygon),
	Rectangle(Rectangle),
	Ellipse(Ellipse),
	Triangle(Triangle),
	Outline(Outline),
	Diffuser(Diffuser),
	InterferenceFilter(InterferenceFilter),
	FacettedGrating(FacettedGrating),
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
			PartKind::FresnelLens(_) => "Fresnel lens",
			PartKind::Polygon(_) => "Polygon",
			PartKind::Rectangle(_) => "Rectangle",
			PartKind::Ellipse(_) => "Ellipse",
			PartKind::Triangle(_) => "Triangle",
			PartKind::Outline(_) => "Outline",
			PartKind::Diffuser(_) => "Diffuser",
			PartKind::InterferenceFilter(_) => "Interference filter",
			PartKind::FacettedGrating(_) => "Facetted grating",
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
			PartKind::FresnelLens(p) => Some(&p.lens.material),
			PartKind::Polygon(p) => Some(&p.material),
			PartKind::Rectangle(p) => Some(&p.material),
			PartKind::Ellipse(p) => Some(&p.material),
			PartKind::Triangle(p) => Some(&p.material),
			PartKind::Outline(p) => Some(&p.material),
			PartKind::Diffuser(p) => Some(&p.material),
			PartKind::InterferenceFilter(p) => Some(&p.material),
			PartKind::FacettedGrating(p) => Some(&p.material),
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
			PartKind::FresnelLens(p) => Some(&p.lens.coating),
			PartKind::Polygon(p) => Some(&p.coating),
			PartKind::Rectangle(p) => Some(&p.coating),
			PartKind::Ellipse(p) => Some(&p.coating),
			PartKind::Triangle(p) => Some(&p.coating),
			PartKind::Outline(p) => Some(&p.coating),
			PartKind::Diffuser(p) => Some(&p.coating),
			PartKind::InterferenceFilter(p) => Some(&p.coating),
			PartKind::FacettedGrating(p) => Some(&p.coating),
			PartKind::Fiber(p) => Some(&p.coating),
			PartKind::GrinLens(p) => Some(&p.coating),
			PartKind::BraggReflector(_) | PartKind::LogicGate(_) | PartKind::ZonePlate(_) => None,
		}
	}
}

impl PartKind {
	/// Whether the kind draws on the project's seed (a diffuser's phases), so the seed is part of the scene.
	pub fn is_seeded(&self) -> bool {
		matches!(self, PartKind::Diffuser(_))
	}
}

impl Part {
	pub fn new(kind: PartKind, x: i32, y: i32) -> Self {
		Self { base: PartBase { x, y, ..PartBase::default() }, kind }
	}
}
