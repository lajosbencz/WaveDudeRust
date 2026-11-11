use serde::{Deserialize, Serialize};

use super::{Fields, Point, Word};

use super::ranges;

/// Largest coating depth in pixels.
pub const MAX_COATING: f64 = ranges::COATING.max;
/// Refractive index bounds of a part.
pub const INDEX_MIN: f64 = ranges::INDEX.min;
pub const INDEX_MAX: f64 = ranges::INDEX.max;
/// Largest absorption ramp in pixels.
pub const MAX_ABSORPTION_RAMP: u32 = ranges::ABSORPTION_RAMP.max as u32;

/// What every part has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[fields(name = "Part")]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct PartBase {
	pub name: String,
	pub enabled: bool,
	/// The centre is the top-left corner of pixel (x, y).
	#[field(range = ranges::AT_LEAST_0)]
	pub x: i32,
	#[field(range = ranges::AT_LEAST_0)]
	pub y: i32,
	/// Degrees, counter-clockwise on screen.
	#[field(range = ranges::ANGLE)]
	pub angle: f64,
}

impl Default for PartBase {
	fn default() -> Self {
		Self { name: String::new(), enabled: true, x: 0, y: 0, angle: 0.0 }
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
#[serde(rename_all = "PascalCase")]
pub enum CoatingProfile {
	/// Quarter-wave layer at sqrt(n).
	#[default]
	Layer,
	/// Raised cosine from the outside index to the body.
	Graded,
}

/// Anti-reflection band on chosen faces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Coating {
	/// Pixels, `0..MAX_COATING`.
	#[field(range = ranges::COATING)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Material {
	pub mirror: bool,
	#[field(range = ranges::INDEX)]
	pub refractive_index: f64,
	/// Blue level at full share, 0..255.
	#[field(range = ranges::ABSORPTION)]
	pub absorption: u32,
	/// Pixels from the outline over which absorption rises, 1..400.
	#[field(range = ranges::ABSORPTION_RAMP)]
	pub absorption_ramp: u32,
}

impl Default for Material {
	fn default() -> Self {
		Self { mirror: false, refractive_index: 1.5, absorption: 0, absorption_ramp: 40 }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Lens {
	#[field(nested)]
	pub material: Material,
	#[field(nested)]
	pub coating: Coating,
	#[field(range = ranges::AT_LEAST_2)]
	pub diameter: u32,
	/// Front radius; 0 = flat; > 0 convex.
	#[field(dependent)]
	pub r1: f64,
	/// Back radius; 0 = flat; < 0 convex.
	#[field(dependent)]
	pub r2: f64,
	#[field(range = ranges::AT_LEAST_1)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
#[serde(rename_all = "PascalCase")]
pub enum ZoneDivision {
	/// Every zone the same width; the outer one the deepest.
	#[default]
	EqualWidth,
	/// Every zone the same share of the rim's sag; the zones narrow outward.
	EqualDepth,
}

/// A lens whose curved surfaces are cut into zones, each zone's arc starting again at the vertex plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct FresnelLens {
	#[serde(flatten)]
	#[field(nested)]
	pub lens: Lens,
	/// `1..MAX_ZONES`; 1 is a plain lens.
	#[field(range = ranges::FRESNEL_ZONES)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Polygon {
	#[field(nested)]
	pub material: Material,
	#[field(nested)]
	pub coating: Coating,
	#[field(range = ranges::POLYGON_SIDES)]
	pub sides: u32,
	/// Side length in pixels.
	#[field(range = ranges::AT_LEAST_2)]
	pub side: u32,
}

impl Default for Polygon {
	fn default() -> Self {
		Self { material: Material::default(), coating: Coating::default(), sides: 6, side: 60 }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Rectangle {
	#[field(nested)]
	pub material: Material,
	#[field(nested)]
	pub coating: Coating,
	/// Along x'.
	#[field(range = ranges::AT_LEAST_1)]
	pub length: u32,
	/// Along y'.
	#[field(range = ranges::AT_LEAST_1, dependent)]
	pub width: u32,
	/// Length and width edited together.
	pub linked: bool,
}

impl Default for Rectangle {
	fn default() -> Self {
		Self { material: Material::default(), coating: Coating::default(), length: 60, width: 60, linked: false }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Ellipse {
	#[field(nested)]
	pub material: Material,
	#[field(nested)]
	pub coating: Coating,
	#[field(range = ranges::AT_LEAST_1)]
	pub length: u32,
	#[field(range = ranges::AT_LEAST_1, dependent)]
	pub width: u32,
	pub linked: bool,
}

impl Default for Ellipse {
	fn default() -> Self {
		Self { material: Material::default(), coating: Coating::default(), length: 60, width: 60, linked: false }
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
#[serde(rename_all = "PascalCase")]
pub enum TriangleType {
	#[default]
	Equilateral,
	RightIsosceles,
	ThirtySixtyNinety,
	Isosceles,
	Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Triangle {
	#[field(nested)]
	pub material: Material,
	#[field(nested)]
	pub coating: Coating,
	#[field(range = ranges::AT_LEAST_2)]
	pub base: u32,
	pub kind: TriangleType,
	/// Degrees at the base's ends.
	#[field(range = ranges::TRIANGLE_ANGLE, dependent)]
	pub angle_a: f64,
	#[field(range = ranges::TRIANGLE_ANGLE, dependent)]
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Outline {
	#[field(nested)]
	pub material: Material,
	#[field(nested)]
	pub coating: Coating,
	/// Part-frame vertices; fewer than 3 paint nothing.
	pub points: Vec<Point>,
}

/// A plate whose front face (-x') is rough: a sum of ripples about a centre wavelength, their phases drawn from
/// the project's seed and the part's name (`profiled.rs`); the back is flat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Diffuser {
	#[field(nested)]
	pub material: Material,
	#[field(nested)]
	pub coating: Coating,
	/// The plate's thickness at the mean of its rough face.
	#[field(range = ranges::AT_LEAST_1)]
	pub length: u32,
	#[field(range = ranges::AT_LEAST_2)]
	pub width: u32,
	/// Pixels, `2..MAX_PIXELS`: the ripple with the full amplitude.
	#[field(range = ranges::RIPPLE_WAVELENGTH)]
	pub center_wavelength: f64,
	/// Pixels, `0..MAX_PIXELS`.
	#[field(range = ranges::PLATE_PIXELS)]
	pub amplitude: f64,
	/// Spread of the ripples' wavelengths about the centre in pixels, `0..MAX_PIXELS`; 0 is the centre alone.
	#[field(range = ranges::PLATE_PIXELS)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct FilterLayer {
	/// Pixels along the axis, `0..MAX_LAYER_THICKNESS` at 0.01; 0 leaves the layer out.
	#[field(range = ranges::LAYER_THICKNESS)]
	pub thickness: f64,
	#[field(range = ranges::INDEX)]
	pub refractive_index: f64,
}

impl Default for FilterLayer {
	fn default() -> Self {
		Self { thickness: 2.0, refractive_index: 1.5 }
	}
}

/// A carrier plate with a stack of thin layers on its front face (-x'), outside the plate; the first layer is
/// the outermost. A mirror plate keeps its layers glass in front of it; the band lies on the plate's back alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct InterferenceFilter {
	#[field(nested)]
	pub material: Material,
	/// Only `back` counts.
	#[field(nested)]
	pub coating: Coating,
	/// The plate's, without the stack.
	#[field(range = ranges::AT_LEAST_1)]
	pub length: u32,
	#[field(range = ranges::AT_LEAST_1)]
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

/// A lattice of round dots of one material, nothing between them and no carrier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct SimpleGrating {
	/// Carried for the file (Next's grating is a coated shape) and never painted: the dots have no faces.
	#[field(nested)]
	pub coating: Coating,
	#[field(nested)]
	pub material: Material,
	/// Dots along the axis; an odd count keeps the middle dot on the position.
	#[field(range = ranges::AT_LEAST_1)]
	pub columns: u32,
	/// Dots across the axis.
	#[field(range = ranges::AT_LEAST_1)]
	pub rows: u32,
	/// Pixels, at least `MIN_SPACING`.
	#[field(range = ranges::GRATING_SPACING)]
	pub column_spacing: u32,
	#[field(range = ranges::GRATING_SPACING)]
	pub row_spacing: u32,
	/// Pixels, 1 to half the smaller spacing.
	#[field(dependent)]
	pub diameter: u32,
}

impl Default for SimpleGrating {
	fn default() -> Self {
		Self {
			coating: Coating::default(),
			material: Material::default(),
			columns: 1,
			rows: 31,
			column_spacing: 16,
			row_spacing: 16,
			diameter: 2,
		}
	}
}

impl SimpleGrating {
	pub const MIN_SPACING: u32 = 2;

	/// A new grating is a mirror: dots that reflect everything.
	pub fn new_part() -> Self {
		Self { material: Material { mirror: true, ..Material::default() }, ..Self::default() }
	}

	pub fn spacing(spacing: u32) -> u32 {
		spacing.max(Self::MIN_SPACING)
	}

	/// The largest diameter: half the smaller spacing, 1 at least.
	pub fn max_diameter(&self) -> u32 {
		(Self::spacing(self.column_spacing).min(Self::spacing(self.row_spacing)) / 2).max(1)
	}

	/// The diameter as the painter takes it, 1 to [`Self::max_diameter`].
	pub fn guarded_diameter(&self) -> u32 {
		self.diameter.clamp(1, self.max_diameter())
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
#[serde(rename_all = "PascalCase")]
pub enum MetaLensType {
	/// The delay greatest at the centre, falling outward within each zone, as a convex lens's glass thins.
	#[default]
	Converging,
	Diverging,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[fields(name = "MetaLens")]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct MetaLensDesign {
	/// Pixels along +x' to the focus; a diverging lens spreads as from a focus that far behind it.
	#[field(range = ranges::AT_LEAST_1)]
	pub focal_length: u32,
	pub lens_type: MetaLensType,
}

impl Default for MetaLensDesign {
	fn default() -> Self {
		Self { focal_length: 500, lens_type: MetaLensType::Converging }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[fields(name = "MetaPrism")]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct MetaPrismDesign {
	/// Degrees a wave along the axis is turned by, counter-clockwise on screen when positive; -89..89.
	#[field(range = ranges::META_DEFLECTION)]
	pub deflection: f64,
}

impl Default for MetaPrismDesign {
	fn default() -> Self {
		Self { deflection: 20.0 }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[fields(name = "MetaAxicon")]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct MetaAxiconDesign {
	/// Degrees each half of a wave along the axis is turned toward it by; 0.1..89.
	#[field(range = ranges::META_CONE_ANGLE)]
	pub cone_angle: f64,
}

impl Default for MetaAxiconDesign {
	fn default() -> Self {
		Self { cone_angle: 10.0 }
	}
}

/// What a meta surface does: the phase delay it asks of each cell (`meta.rs`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "PascalCase")]
pub enum MetaDesign {
	Lens(MetaLensDesign),
	Prism(MetaPrismDesign),
	Axicon(MetaAxiconDesign),
}

impl Default for MetaDesign {
	fn default() -> Self {
		Self::Lens(MetaLensDesign::default())
	}
}

/// A carrier plate with a sub-wavelength pattern on its front (-x'): a ridge of `pattern_index` in every cell of
/// `pitch` across the face, `height` deep in front of the plate, its width the cell's fill factor times the pitch;
/// the fill from the delay the design asks. The band lies on the ridges' tops and the plate's back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct MetaSurface {
	#[field(nested)]
	pub material: Material,
	/// `front` is the ridges' tops, `back` the plate's back; `sides` is not offered.
	#[field(nested)]
	pub coating: Coating,
	/// The plate's, without the pattern.
	#[field(range = ranges::AT_LEAST_1)]
	pub length: u32,
	#[field(range = ranges::AT_LEAST_2)]
	pub width: u32,
	/// Level 1..255; the wavelength it gives through the frequency scale sets every cell's fill.
	#[field(range = ranges::DESIGN_LEVEL)]
	pub design_frequency: f64,
	/// The cell's width in pixels, `1..MAX_PIXELS`.
	#[field(range = ranges::META_PIXELS)]
	pub pitch: f64,
	/// The ridges' height in front of the plate in pixels, `1..MAX_PIXELS`.
	#[field(range = ranges::META_PIXELS)]
	pub height: f64,
	/// The ridges' index.
	#[field(range = ranges::INDEX)]
	pub pattern_index: f64,
	#[field(nested)]
	pub design: MetaDesign,
}

impl Default for MetaSurface {
	fn default() -> Self {
		Self {
			material: Material::default(),
			coating: Coating::default(),
			length: 50,
			width: 500,
			design_frequency: 50.0,
			pitch: 4.0,
			height: 32.0,
			pattern_index: 2.0,
			design: MetaDesign::default(),
		}
	}
}

impl MetaSurface {
	/// A new meta surface of `design` coats nothing on its sides.
	pub fn new_part(design: MetaDesign) -> Self {
		Self { design, coating: Coating { sides: false, ..Coating::default() }, ..Self::default() }
	}
}

/// The relief of a facetted grating over each pitch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct FacettedGrating {
	#[field(nested)]
	pub material: Material,
	/// Only `back` counts.
	#[field(nested)]
	pub coating: Coating,
	#[field(range = ranges::AT_LEAST_1)]
	pub length: u32,
	#[field(range = ranges::AT_LEAST_2)]
	pub width: u32,
	pub pattern: FacetPattern,
	/// Period across the face in pixels, `2..MAX_PIXELS`.
	#[field(range = ranges::RIPPLE_WAVELENGTH)]
	pub pitch: f64,
	/// The relief's height either side of the mean face in pixels, `0..MAX_PIXELS`.
	#[field(range = ranges::PLATE_PIXELS)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
#[serde(rename_all = "PascalCase")]
pub enum FiberEnds {
	#[default]
	Flat,
	Round,
}

/// A Bezier node in the part frame with handles relative to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct Fiber {
	#[field(nested)]
	pub coating: Coating,
	#[field(range = ranges::INDEX)]
	pub core_index: f64,
	#[field(range = ranges::AT_LEAST_2)]
	pub core_diameter: u32,
	#[field(range = ranges::INDEX)]
	pub cladding_index: f64,
	#[field(range = ranges::AT_LEAST_0)]
	pub cladding_thickness: u32,
	/// Blue level of the cladding's mode stripper, 0..255.
	#[field(range = ranges::ABSORPTION)]
	pub absorption: u32,
	#[field(range = ranges::ABSORPTION_RAMP)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct BraggReflector {
	#[field(range = ranges::BRAGG_PAIRS)]
	pub pairs: u32,
	#[field(range = ranges::AT_LEAST_1)]
	pub thickness1: u32,
	#[field(range = ranges::AT_LEAST_1)]
	pub thickness2: u32,
	#[field(range = ranges::INDEX)]
	pub index1: f64,
	#[field(range = ranges::INDEX)]
	pub index2: f64,
	#[field(range = ranges::AT_LEAST_2)]
	pub width: u32,
}

impl Default for BraggReflector {
	fn default() -> Self {
		Self { pairs: 10, thickness1: 10, thickness2: 10, index1: 1.5, index2: 2.0, width: 200 }
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct GrinLens {
	#[field(nested)]
	pub coating: Coating,
	#[field(range = ranges::AT_LEAST_1)]
	pub length: u32,
	#[field(range = ranges::AT_LEAST_2)]
	pub diameter: u32,
	#[field(range = ranges::INDEX)]
	pub centre_index: f64,
	#[field(range = ranges::INDEX)]
	pub edge_index: f64,
}

impl Default for GrinLens {
	fn default() -> Self {
		Self { coating: Coating::default(), length: 100, diameter: 100, centre_index: 1.8, edge_index: 1.5 }
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Word)]
#[serde(rename_all = "PascalCase")]
pub enum GateKind {
	Xor,
	And,
	Or,
}

/// A waveguide logic gate; the layout is computed from the design frequency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct LogicGate {
	pub kind: GateKind,
	/// Level 1..255.
	#[field(range = ranges::DESIGN_LEVEL)]
	pub design_frequency: f64,
	/// Pixels, `0..MAX_COATING`.
	#[field(range = ranges::COATING)]
	pub coating: f64,
	pub input_coated: bool,
	pub output_coated: bool,
	pub upper_output_active: bool,
	pub lower_output_active: bool,
	#[field(range = ranges::INDEX)]
	pub core_index: f64,
	#[field(range = ranges::INDEX)]
	pub cladding_index: f64,
	/// Pixels, raised to the design's minimum.
	#[field(range = ranges::GATE_PORT_SPACING, dependent)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
#[serde(default)]
pub struct ZonePlate {
	pub mount: bool,
	#[field(range = ranges::AT_LEAST_1)]
	pub mount_width: u32,
	#[field(range = ranges::AT_LEAST_2)]
	pub diameter: u32,
	#[field(range = ranges::AT_LEAST_1)]
	pub thickness: u32,
	/// Pixels over which the block absorption rises, 0..400.
	#[field(range = ranges::ZONE_PLATE_COATING)]
	pub coating: u32,
	/// Level 1..255.
	#[field(range = ranges::DESIGN_LEVEL)]
	pub design_frequency: f64,
	#[field(range = ranges::AT_LEAST_1)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
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
	SimpleGrating(SimpleGrating),
	FacettedGrating(FacettedGrating),
	MetaSurface(MetaSurface),
	Fiber(Fiber),
	BraggReflector(BraggReflector),
	GrinLens(GrinLens),
	LogicGate(LogicGate),
	ZonePlate(ZonePlate),
}

/// An optical part painted into the scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Fields)]
#[serde(rename_all = "snake_case")]
pub struct Part {
	#[serde(flatten)]
	#[field(nested)]
	pub base: PartBase,
	#[field(nested)]
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
			PartKind::SimpleGrating(_) => "Grating",
			PartKind::MetaSurface(m) => match m.design {
				MetaDesign::Lens(_) => "Meta lens",
				MetaDesign::Prism(_) => "Meta prism",
				MetaDesign::Axicon(_) => "Meta axicon",
			},
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
			PartKind::SimpleGrating(p) => Some(&p.material),
			PartKind::MetaSurface(p) => Some(&p.material),
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
			PartKind::MetaSurface(p) => Some(&p.coating),
			PartKind::Fiber(p) => Some(&p.coating),
			PartKind::GrinLens(p) => Some(&p.coating),
			PartKind::SimpleGrating(p) => Some(&p.coating),
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
