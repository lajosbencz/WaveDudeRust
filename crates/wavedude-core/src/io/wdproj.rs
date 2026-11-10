//! The WaveDude Next `.wdproj` XML: read (versions 1 to 3, DOCTYPE ignored, unknown elements and
//! attributes ignored) and written sparse with a DOCTYPE that declares every element and attribute.
//!
//! Sources of a 9-point project are stored in the original's frequency units: reading multiplies
//! the frequency scale is divided by [`super::nine_point_speed`], writing multiplies it back. A file whose `Simulation` carries
//! `Rescaled="true"` already holds this program's units and is read unscaled.
//!
//! The root's `Generator` names the program that wrote the file. The display brightness of a file this program
//! wrote is in its own units; one of WaveDude Next's is in the original's (spec/05-deviations.md).

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::fields::{self, Attr, AttrType, Codec, Field, Presence::*, Table, fields};
use super::xml::{self, DECLARATION, Element, XmlEnum, members};
use super::{FormatError, Result, UnreadableFile, wdscheme};
use crate::model::normalize::Normalize;
use crate::model::*;

/// File extension of a project.
pub const EXTENSION: &str = "wdproj";
/// The root's `Generator` in the files this program writes; the version follows it.
pub const GENERATOR: &str = "WaveDude Rust";

/// A read project and who wrote it.
#[derive(Debug)]
pub struct Read {
	pub project: Project,
	/// Written by this program (`Generator`): the brightness is in its units.
	pub own: bool,
}

macro_rules! xml_enum {
	($t:ty: $($m:ident),+) => {
		impl XmlEnum for $t {
			const MEMBERS: &'static [(Self, &'static str)] = &[$((<$t>::$m, stringify!($m))),+];
		}
	};
}

xml_enum!(SceneMode: Canvas, Image);
xml_enum!(Edges: Open, Reflective);
xml_enum!(SourceMode: Continuous, Pulsed);
xml_enum!(PulseShape: Gaussian, SmoothedSquare);
xml_enum!(CoatingProfile: Layer, Graded);
xml_enum!(TriangleType: Equilateral, RightIsosceles, ThirtySixtyNinety, Isosceles, Custom);
xml_enum!(FiberEnds: Flat, Round);
xml_enum!(Layer: Field, Intensity);
xml_enum!(Measurement: Amplitude, Value, IntegratedValue);
xml_enum!(ProbeKind: Point, Line);
xml_enum!(BoxSize: Tiny, Small, Medium, Large, ExtraLarge, Custom);
xml_enum!(ScaleMode: Running, Full);
xml_enum!(YScale: Linear, Logarithmic);

/// The part elements in declaration order.
const PART_ELEMENTS: &[&str] = &[
	"Lens",
	"Polygon",
	"Rectangle",
	"Ellipse",
	"Triangle",
	"Outline",
	"Fiber",
	"BraggReflector",
	"GrinLens",
	"XorGate",
	"AndGate",
	"OrGate",
	"ZonePlate",
];

type R<T> = std::result::Result<T, FormatError>;

// ---------------------------------------------------------------- reading

/// Parses a `.wdproj` text into a normalised project (migrated, 9-point frequencies scaled).
/// The scene's `image_size` is left at (0, 0); [`load`] decodes it.
pub fn read_str(text: &str) -> R<Project> {
	read(text).map(|r| r.project)
}

/// [`read_str`] with who wrote the file.
pub fn read(text: &str) -> R<Read> {
	let doc = xml::parse(text)?;
	let root = &doc.root;
	if root.name != "Project" {
		return Err(FormatError { message: "the document is not a WaveDude Next project".into(), at: Some(root.at) });
	}
	let version = root.i32("Version", PROJECT_VERSION as i32)?;
	let own = root.get("Generator").is_some_and(|g| g.starts_with(GENERATOR));
	let seed = match root.get("Seed") {
		Some(v) => root.parse_i32("Seed", v)?,
		None => Project::new_random_seed().seed,
	};
	let display = Display {
		field: original_layer(Layer::Field),
		intensity: original_layer(Layer::Intensity),
		..Display::default()
	};
	let mut project = Project { seed, display, ..Project::default() };
	let mut damping_read = false;
	let mut rescaled = false;
	for el in &root.children {
		match el.name.as_str() {
			"Description" => project.description = el.text.clone(),
			"Scene" => {
				damping_read = el.has("DampingEdge");
				project.scene = read_scene(el)?;
			}
			"Simulation" => {
				rescaled = el.bool("Rescaled", false)?;
				project.simulation = fields::read(el)?;
			}
			"Display" => project.display = read_display(el)?,
			"Source" => project.sources.push(read_source(el)?),
			"Probe" => project.probes.push(read_probe(el)?),
			"ColorScheme" => project.color_schemes.push(wdscheme::from_element(el)?),
			name if PART_ELEMENTS.contains(&name) => project.parts.push(read_part(el)?),
			_ => {}
		}
	}
	if version > PROJECT_VERSION as i32 {
		tracing::warn!(
			"project file version {version} is newer than the supported {PROJECT_VERSION}; unknown settings are ignored"
		);
	}
	if version < 2 {
		project.scene.edges = Edges::Reflective;
	}
	if version < 3 && !damping_read {
		project.scene.damping_edge = 0;
	}
	if !rescaled && super::scale_nine_point_frequencies(&mut project, super::nine_point_speed()) {
		tracing::info!(
			"9-point project: the frequency scale is divided by the original kernel's speed {:.4} so the wavelengths stay",
			super::nine_point_speed()
		);
	}
	project.normalize();
	Ok(Read { project, own })
}

// ---------------------------------------------------------------- attribute tables

impl Table for Scene {
	const FIELDS: &'static [Field<Self>] = fields!(Scene, "Scene";
		enumv "Mode" mode,
		text "Image" image,
		text "ImagePath" image_path,
		u32 "Width" width,
		u32 "Height" height,
		i32 "DampingEdge" damping_edge,
		enumv "Edges" edges,
		f64 "RefractiveIndex" refractive_index,
		bool "ImageSources" image_sources,
		f64 "ImageSourceScale" image_source_scale,
		bool "ObjectSources" object_sources,
	);

	fn base() -> Self {
		Self::default()
	}
}

impl Table for Simulation {
	const FIELDS: &'static [Field<Self>] = fields!(Simulation, "Simulation";
		u32 "Steps" steps,
		u32 "VisualizeEvery" visualize_every,
		[Field {
			owner: "Simulation",
			name: "FourthOrderLaplacian",
			codec: Codec::Bool {
				get: |s| s.stencil == Stencil::FourthOrder,
				set: |s, fourth| s.stencil = if fourth { Stencil::FourthOrder } else { Stencil::NinePoint },
			},
			presence: Sparse,
		}],
		f64 "WaveSpeed" wave_speed,
		f64 "TimeDecay" time_decay,
		f64 "SourceFrequencyScale" source_frequency_scale,
		f64 "IntensityScale" intensity_scale,
	);

	fn base() -> Self {
		Self::default()
	}
}

/// Brightness a WaveDude Next file omits: its default, made for its blended source, not this program's.
const ORIGINAL_BRIGHTNESS: f64 = 30.0;

/// A layer's defaults as WaveDude Next has them.
fn original_layer(layer: Layer) -> DisplayLayer {
	let d = match layer {
		Layer::Field => DisplayLayer::field_default(),
		Layer::Intensity => DisplayLayer::intensity_default(),
	};
	DisplayLayer { brightness: ORIGINAL_BRIGHTNESS, ..d }
}

/// A `Field` or `Intensity` element: a missing `ColorScheme` is empty (the layer's default after
/// normalising), a missing `BlackLevel` is 0 and a missing `Brightness` 30, as WaveDude Next reads them.
fn layer_base() -> DisplayLayer {
	DisplayLayer {
		color_scheme: String::new(),
		brightness: ORIGINAL_BRIGHTNESS,
		black_level: 0.0,
		..DisplayLayer::field_default()
	}
}

/// The `Intensity` element; `Field` writes its `BlackLevel` always (WaveDude Next's default 0 is not its initial
/// -0.05).
impl Table for DisplayLayer {
	const FIELDS: &'static [Field<Self>] = fields!(DisplayLayer, "DisplayLayer";
		text "ColorScheme" color_scheme => Always,
		f64 "Brightness" brightness,
		f64 "BlackLevel" black_level,
		bool "Preview" preview,
		bool "Overlay" overlay,
		f64 "OverlayGain" overlay_gain,
		bool "OverlayInExport" overlay_in_export,
	);

	fn base() -> Self {
		layer_base()
	}
}

const FIELD_LAYER: &[Field<DisplayLayer>] = fields!(DisplayLayer, "DisplayLayer";
	text "ColorScheme" color_scheme => Always,
	f64 "Brightness" brightness,
	f64 "BlackLevel" black_level => When(|_| true),
	bool "Preview" preview,
	bool "Overlay" overlay,
	f64 "OverlayGain" overlay_gain,
	bool "OverlayInExport" overlay_in_export,
);

impl Table for Video {
	const FIELDS: &'static [Field<Self>] = fields!(Video, "Video"; u32 "Fps" fps, u32 "Quality" quality);

	fn base() -> Self {
		Self::default()
	}
}

impl Table for Display {
	const FIELDS: &'static [Field<Self>] = fields!(Display, "Display";
		u32 "ExportFrom" export_from,
		u32 "ExportTo" export_to,
		bool "ProbeGraphsInExport" probe_graphs_in_export,
		bool "ProbeMarkersInExport" probe_markers_in_export,
		bool "SourceMarkersInExport" source_markers_in_export,
		bool "HandlesInPreview" handles_in_preview,
		bool "LabelsInPreview" labels_in_preview,
	);

	fn base() -> Self {
		Self::default()
	}
}

impl Table for Source {
	const FIELDS: &'static [Field<Self>] = fields!(Source, "Source";
		text "Name" name => Always,
		bool "Enabled" enabled,
		f64 "X" x => Always,
		f64 "Y" y => Always,
		f64 "Frequency" frequency,
		f64 "Amplitude" amplitude,
		f64 "Phase" phase,
		enumv "Mode" mode,
		enumv "PulseShape" pulse_shape,
		u32 "PulseWidth" pulse_width,
		u32 "OffWidth" off_width,
		[Field {
			owner: "Source",
			name: "DetachAfter",
			codec: Codec::Int {
				get: |s| s.detach_after.map_or(-1, i64::from),
				set: |s, step| s.detach_after = u32::try_from(step).ok(),
			},
			presence: Sparse,
		}],
	);

	fn base() -> Self {
		Self::default()
	}
}

impl Table for SourceArray {
	const FIELDS: &'static [Field<Self>] = fields!(SourceArray, "SourceArray";
		u32 "SizeX" size_x,
		u32 "SizeY" size_y,
		i32 "SpacingX" spacing_x,
		i32 "SpacingY" spacing_y,
		f64 "Angle" angle,
		u32 "RandomFrequency" random_frequency,
		u32 "RandomPhase" random_phase,
		u32 "RandomOffWidth" random_off_width,
		u32 "RandomPosition" random_position,
	);

	fn base() -> Self {
		Self::default()
	}
}

/// A missing `BoxX`/`BoxY` reads as A's coordinate (WaveDude Next's box follows A while attached, and its reader
/// set A before the box), so 0 is written whenever A's is not 0.
impl Table for Probe {
	const FIELDS: &'static [Field<Self>] = fields!(Probe, "Probe";
		text "Name" name => Always,
		bool "Active" active,
		enumv "Layer" layer,
		enumv "Measurement" measurement,
		enumv "Kind" kind,
		i32 "AX" ax => Always,
		i32 "AY" ay => Always,
		i32 "BX" bx => When(|p| p.kind == ProbeKind::Line),
		i32 "BY" by => When(|p| p.kind == ProbeKind::Line),
		enumv "BoxSize" box_size,
		i32 "BoxX" box_x => When(|p| p.box_x != 0 || p.ax != 0),
		i32 "BoxY" box_y => When(|p| p.box_y != 0 || p.ay != 0),
		u32 "BoxWidth" box_width => When(|p| p.box_size == BoxSize::Custom),
		u32 "BoxHeight" box_height => When(|p| p.box_size == BoxSize::Custom),
		enumv "ScaleMode" scale_mode,
		bool "AutoScaleX" auto_scale_x,
		bool "AutoScaleY" auto_scale_y,
		f64 "YMin" y_min,
		f64 "YMax" y_max,
		enumv "YScale" y_scale,
		u32 "Decades" decades,
		u32 "Window" window,
		u32 "StartAfter" start_after,
	);

	fn base() -> Self {
		Self::default()
	}
}

impl Table for PartBase {
	const FIELDS: &'static [Field<Self>] = fields!(PartBase, "Part";
		text "Name" name => Always,
		bool "Enabled" enabled,
		i32 "X" x => Always,
		i32 "Y" y => Always,
		f64 "Angle" angle,
	);

	fn base() -> Self {
		Self::default()
	}
}

impl Table for Coating {
	// The original keeps the thickness as an integer: written rounded, so its reader takes the file.
	const FIELDS: &'static [Field<Self>] = fields!(Coating, "Coating";
		[Field {
			owner: "Coating",
			name: "Coating",
			codec: Codec::Int { get: |c| c.depth.round() as i64, set: |c, v| c.depth = f64::from(v) },
			presence: Sparse,
		}],
		enumv "CoatingProfile" profile,
		bool "FrontCoated" front,
		bool "BackCoated" back,
		bool "SidesCoated" sides,
	);

	fn base() -> Self {
		Self::default()
	}
}

impl Table for Material {
	const FIELDS: &'static [Field<Self>] = fields!(Material, "Material";
		bool "Mirror" mirror,
		f64 "RefractiveIndex" refractive_index,
		u32 "Absorption" absorption,
		u32 "AbsorptionRamp" absorption_ramp,
	);

	fn base() -> Self {
		Self::default()
	}
}

/// A part kind's own attributes (after its coating's and material's, which are read separately).
macro_rules! kind_table {
	($t:ident; $($items:tt)*) => {
		impl Table for $t {
			const FIELDS: &'static [Field<Self>] = fields!($t, stringify!($t); $($items)*);

			fn base() -> Self {
				Self::default()
			}
		}
	};
}

kind_table!(Lens; u32 "Diameter" diameter, f64 "R1" r1, f64 "R2" r2, u32 "Thickness" thickness);
kind_table!(Polygon; u32 "Sides" sides, u32 "Side" side);
kind_table!(Rectangle; u32 "Length" length, u32 "Width" width, bool "Linked" linked);
kind_table!(Ellipse; u32 "Length" length, u32 "Width" width, bool "Linked" linked);
kind_table!(Triangle; u32 "Base" base, enumv "Type" kind, f64 "AngleA" angle_a, f64 "AngleB" angle_b);
kind_table!(Outline;);
kind_table!(Fiber;
	f64 "CoreIndex" core_index,
	u32 "CoreDiameter" core_diameter,
	f64 "CladdingIndex" cladding_index,
	u32 "CladdingThickness" cladding_thickness,
	u32 "Absorption" absorption,
	u32 "AbsorptionRamp" absorption_ramp,
	enumv "Ends" ends,
);
kind_table!(BraggReflector;
	u32 "Pairs" pairs,
	u32 "Thickness1" thickness1,
	u32 "Thickness2" thickness2,
	f64 "Index1" index1,
	f64 "Index2" index2,
	u32 "Width" width,
);
kind_table!(GrinLens;
	u32 "Length" length,
	u32 "Diameter" diameter,
	f64 "CentreIndex" centre_index,
	f64 "EdgeIndex" edge_index,
);
kind_table!(LogicGate;
	f64 "DesignFrequency" design_frequency,
	[Field {
		owner: "LogicGate",
		name: "Coating",
		codec: Codec::Int { get: |g| g.coating.round() as i64, set: |g, v| g.coating = f64::from(v) },
		presence: Sparse,
	}],
	bool "InputCoated" input_coated,
	bool "OutputCoated" output_coated,
	bool "UpperOutputActive" upper_output_active,
	bool "LowerOutputActive" lower_output_active,
	f64 "CoreIndex" core_index,
	f64 "CladdingIndex" cladding_index,
	u32 "PortSpacing" port_spacing,
);
kind_table!(ZonePlate;
	bool "Mount" mount,
	u32 "MountWidth" mount_width,
	u32 "Diameter" diameter,
	u32 "Thickness" thickness,
	u32 "Coating" coating,
	f64 "DesignFrequency" design_frequency,
	u32 "FocalLength" focal_length,
	bool "Inverse" inverse,
);

/// An outline's corner.
impl Table for Point {
	const FIELDS: &'static [Field<Self>] = fields!(Point, "Point"; i32 "X" x => Always, i32 "Y" y => Always);

	fn base() -> Self {
		Self::default()
	}
}

impl Table for FiberNode {
	const FIELDS: &'static [Field<Self>] = fields!(FiberNode, "FiberNode";
		i32 "X" x => Always,
		i32 "Y" y => Always,
		i32 "InX" in_x,
		i32 "InY" in_y,
		i32 "OutX" out_x,
		i32 "OutY" out_y,
		bool "Smooth" smooth,
	);

	fn base() -> Self {
		Self::default()
	}
}

// ---------------------------------------------------------------- reading

fn read_scene(el: &Element) -> R<Scene> {
	let mut scene: Scene = fields::read(el)?;
	if let Some(data) = el.child("ImageData") {
		let compact: String = data.text.split_ascii_whitespace().collect();
		let bytes = BASE64.decode(compact).map_err(|e| FormatError {
			message: format!("element 'ImageData' is not valid base64: {e}"),
			at: Some(data.at),
		})?;
		scene.image_data = Some(bytes.into());
	}
	Ok(scene)
}

fn read_display(el: &Element) -> R<Display> {
	let mut display: Display = fields::read(el)?;
	display.field = el.child("Field").map(fields::read).transpose()?.unwrap_or_else(|| original_layer(Layer::Field));
	display.intensity =
		el.child("Intensity").map(fields::read).transpose()?.unwrap_or_else(|| original_layer(Layer::Intensity));
	if let Some(v) = el.child("Video") {
		display.video = fields::read(v)?;
	}
	Ok(display)
}

fn read_source(el: &Element) -> R<Source> {
	let mut source: Source = fields::read(el)?;
	source.array = el.child("Array").map(fields::read).transpose()?;
	Ok(source)
}

fn read_probe(el: &Element) -> R<Probe> {
	let mut probe: Probe = fields::read(el)?;
	if !el.has("BoxX") {
		probe.box_x = probe.ax;
	}
	if !el.has("BoxY") {
		probe.box_y = probe.ay;
	}
	Ok(probe)
}

fn read_part(el: &Element) -> R<Part> {
	let (coating, material) = (fields::read(el), fields::read(el));
	let kind = match el.name.as_str() {
		"Lens" => PartKind::Lens(Lens { coating: coating?, material: material?, ..fields::read(el)? }),
		"Polygon" => PartKind::Polygon(Polygon { coating: coating?, material: material?, ..fields::read(el)? }),
		"Rectangle" => PartKind::Rectangle(Rectangle { coating: coating?, material: material?, ..fields::read(el)? }),
		"Ellipse" => PartKind::Ellipse(Ellipse { coating: coating?, material: material?, ..fields::read(el)? }),
		"Triangle" => PartKind::Triangle(Triangle { coating: coating?, material: material?, ..fields::read(el)? }),
		"Outline" => PartKind::Outline(Outline {
			coating: coating?,
			material: material?,
			points: el.children("Point").map(fields::read).collect::<R<_>>()?,
		}),
		"Fiber" => PartKind::Fiber(Fiber {
			coating: coating?,
			nodes: el.children("Node").map(fields::read).collect::<R<_>>()?,
			..fields::read(el)?
		}),
		"BraggReflector" => PartKind::BraggReflector(fields::read(el)?),
		"GrinLens" => PartKind::GrinLens(GrinLens { coating: coating?, ..fields::read(el)? }),
		"XorGate" => PartKind::LogicGate(LogicGate { kind: GateKind::Xor, ..fields::read(el)? }),
		"AndGate" => PartKind::LogicGate(LogicGate { kind: GateKind::And, ..fields::read(el)? }),
		"OrGate" => PartKind::LogicGate(LogicGate { kind: GateKind::Or, ..fields::read(el)? }),
		"ZonePlate" => PartKind::ZonePlate(fields::read(el)?),
		other => unreachable!("not a part element: {other}"),
	};
	Ok(Part { base: fields::read(el)?, kind })
}

/// Loads a `.wdproj` file and decodes the embedded image's size.
pub fn load(path: &Path) -> Result<Project> {
	load_read(path).map(|r| r.project)
}

/// [`load`] with who wrote the file.
pub fn load_read(path: &Path) -> Result<Read> {
	let text = super::read_text(path)?;
	tracing::info!("loading project from '{}'", path.display());
	let mut read = read(&text).map_err(|e| UnreadableFile::new(path, &text, e))?;
	super::decode_image_size(&mut read.project, path)?;
	Ok(read)
}

// ---------------------------------------------------------------- writing

fn scene_is_default(s: &Scene) -> bool {
	Scene { image_size: (0, 0), ..s.without_image() } == Scene::default() && s.image_data.is_none()
}

/// An element of `value`'s attributes.
fn element<T: Table>(name: &str, value: &T) -> Element {
	let mut el = Element::new(name);
	fields::write(value, &mut el);
	el
}

fn write_scene(s: &Scene, folder: Option<&Path>) -> Element {
	let stored = Scene { image_path: super::stored_image_path(folder, &s.image_path), ..s.without_image() };
	let mut el = element("Scene", &stored);
	if let Some(bytes) = &s.image_data {
		let mut data = Element::new("ImageData");
		data.text = BASE64.encode(bytes);
		el.child_el(data);
	}
	el
}

fn write_display(s: &Display) -> Element {
	let mut el = element("Display", s);
	let mut field = Element::new("Field");
	fields::write_against(FIELD_LAYER, &s.field, &layer_base(), &mut field);
	el.child_el(field).child_el(element("Intensity", &s.intensity));
	if s.video != Video::default() {
		el.child_el(element("Video", &s.video));
	}
	el
}

fn write_source(s: &Source) -> Element {
	if s.x.fract() != 0.0 || s.y.fract() != 0.0 {
		tracing::warn!(
			"the source '{}' sits between pixels ({}, {}): WaveDude Next reads whole numbers only",
			s.name,
			s.x,
			s.y
		);
	}
	let mut el = element("Source", s);
	if let Some(a) = &s.array {
		el.child_el(element("Array", a));
	}
	el
}

fn part_element_name(kind: &PartKind) -> &'static str {
	match kind {
		PartKind::Lens(_) => "Lens",
		PartKind::Polygon(_) => "Polygon",
		PartKind::Rectangle(_) => "Rectangle",
		PartKind::Ellipse(_) => "Ellipse",
		PartKind::Triangle(_) => "Triangle",
		PartKind::Outline(_) => "Outline",
		PartKind::Fiber(_) => "Fiber",
		PartKind::BraggReflector(_) => "BraggReflector",
		PartKind::GrinLens(_) => "GrinLens",
		PartKind::LogicGate(g) => match g.kind {
			GateKind::Xor => "XorGate",
			GateKind::And => "AndGate",
			GateKind::Or => "OrGate",
		},
		PartKind::ZonePlate(_) => "ZonePlate",
	}
}

/// Evaluates `$e` with `$k` bound to the kind's own value, whatever the kind.
macro_rules! with_kind {
	($kind:expr, $k:ident => $e:expr) => {
		match $kind {
			PartKind::Lens($k) => $e,
			PartKind::Polygon($k) => $e,
			PartKind::Rectangle($k) => $e,
			PartKind::Ellipse($k) => $e,
			PartKind::Triangle($k) => $e,
			PartKind::Outline($k) => $e,
			PartKind::Fiber($k) => $e,
			PartKind::BraggReflector($k) => $e,
			PartKind::GrinLens($k) => $e,
			PartKind::LogicGate($k) => $e,
			PartKind::ZonePlate($k) => $e,
		}
	};
}

fn write_part(p: &Part) -> Element {
	let mut el = element(part_element_name(&p.kind), &p.base);
	if let Some(c) = p.kind.coating() {
		fields::write(c, &mut el);
	}
	if let Some(m) = p.kind.material() {
		fields::write(m, &mut el);
	}
	with_kind!(&p.kind, k => fields::write(k, &mut el));
	match &p.kind {
		PartKind::Outline(k) => el.children.extend(k.points.iter().map(|pt| element("Point", pt))),
		PartKind::Fiber(k) => el.children.extend(k.nodes.iter().map(|n| element("Node", n))),
		PartKind::Lens(_)
		| PartKind::Polygon(_)
		| PartKind::Rectangle(_)
		| PartKind::Ellipse(_)
		| PartKind::Triangle(_)
		| PartKind::BraggReflector(_)
		| PartKind::GrinLens(_)
		| PartKind::LogicGate(_)
		| PartKind::ZonePlate(_) => {}
	}
	el
}

/// The `Project` element of a project; `folder` makes an image path inside it relative.
fn to_element(project: &Project, folder: Option<&Path>) -> Element {
	let mut root = Element::new("Project");
	root.attr("Version", PROJECT_VERSION.to_string())
		.attr("Seed", project.seed.to_string())
		.attr("Generator", format!("{GENERATOR} {}", env!("CARGO_PKG_VERSION")));
	if !project.description.is_empty() {
		let mut d = Element::new("Description");
		d.text = project.description.clone();
		root.child_el(d);
	}
	if !scene_is_default(&project.scene) {
		root.child_el(write_scene(&project.scene, folder));
	}
	let mut simulation = project.simulation.clone();
	if simulation.stencil == Stencil::NinePoint {
		simulation.source_frequency_scale *= super::nine_point_speed();
	}
	if simulation != Simulation::default() {
		root.child_el(element("Simulation", &simulation));
	}
	root.child_el(write_display(&project.display));
	root.children.extend(project.sources.iter().map(write_source));
	root.children.extend(project.parts.iter().map(write_part));
	root.children.extend(project.probes.iter().map(|p| element("Probe", p)));
	root.children.extend(project.color_schemes.iter().map(wdscheme::to_element));
	root
}

/// The `.wdproj` text of a project with its DOCTYPE (no trailing newline). 9-point source frequencies
/// are written in the original's units.
pub fn write_string(project: &Project, folder: Option<&Path>) -> String {
	let mut out = format!("{DECLARATION}\n<!DOCTYPE Project [{}]>\n", dtd());
	to_element(project, folder).write(&mut out, 0);
	out
}

pub fn save(project: &Project, path: &Path) -> Result<()> {
	tracing::info!(
		"saving project to '{}' ({} sources, {} parts, {} probes)",
		path.display(),
		project.sources.len(),
		project.parts.len(),
		project.probes.len()
	);
	super::write_file(path, write_string(project, path.parent()).as_bytes())
}

// ---------------------------------------------------------------- DOCTYPE

struct Decl {
	name: &'static str,
	/// `EMPTY`, `(#PCDATA)` or a children model.
	content: String,
	attrs: Vec<Attr>,
}

impl Decl {
	fn new(name: &'static str, content: &str, attrs: Vec<Attr>) -> Self {
		Self { name, content: content.into(), attrs }
	}

	fn of<T: Table>(name: &'static str, content: &str) -> Self {
		Self::new(name, content, fields::attrs(T::FIELDS))
	}
}

/// A part element's attributes: the part's, the coating's and the material's when the kind has them, its own.
fn part_attrs(kind: &PartKind) -> Vec<Attr> {
	let mut attrs = fields::attrs(PartBase::FIELDS);
	if kind.coating().is_some() {
		attrs.extend(fields::attrs(Coating::FIELDS));
	}
	if kind.material().is_some() {
		attrs.extend(fields::attrs(Material::FIELDS));
	}
	fn own<T: Table>(_: &T) -> Vec<Attr> {
		fields::attrs(T::FIELDS)
	}
	attrs.extend(with_kind!(kind, k => own(k)));
	attrs
}

fn decls() -> Vec<Decl> {
	let text = |owner, name, required| Attr { owner, name, ty: AttrType::Cdata, required };
	let mut decls = vec![
		Decl::new(
			"Project",
			&format!(
				"(Description?, Scene?, Simulation?, Display?, Source*, ({})*, Probe*, ColorScheme*)",
				PART_ELEMENTS.join("|")
			),
			vec![text(None, "Version", true), text(Some("Project"), "Seed", true), text(None, "Generator", false)],
		),
		Decl::new("Description", "(#PCDATA)", vec![]),
		Decl::of::<Scene>("Scene", "(ImageData?)"),
		Decl::new("ImageData", "(#PCDATA)", vec![]),
		Decl::new("Simulation", "EMPTY", {
			let mut attrs = fields::attrs(Simulation::FIELDS);
			attrs.push(Attr { owner: None, name: "Rescaled", ty: AttrType::Bool, required: false });
			attrs
		}),
		Decl::of::<Display>("Display", "(Field?, Intensity?, Video?)"),
		Decl::new("Field", "EMPTY", fields::attrs(FIELD_LAYER)),
		Decl::of::<DisplayLayer>("Intensity", "EMPTY"),
		Decl::of::<Video>("Video", "EMPTY"),
		Decl::of::<Source>("Source", "(Array?)"),
		Decl::of::<SourceArray>("Array", "EMPTY"),
	];
	for &name in PART_ELEMENTS {
		let part = read_part(&Element::new(name)).expect("an element without attributes reads");
		let content = match name {
			"Outline" => "(Point*)",
			"Fiber" => "(Node*)",
			_ => "EMPTY",
		};
		decls.push(Decl::new(name, content, part_attrs(&part.kind)));
		match name {
			"Outline" => decls.push(Decl::of::<Point>("Point", "EMPTY")),
			"Fiber" => decls.push(Decl::of::<FiberNode>("Node", "EMPTY")),
			_ => {}
		}
	}
	decls.extend([
		Decl::of::<Probe>("Probe", "EMPTY"),
		Decl::new(
			"ColorScheme",
			"(Stop*)",
			vec![
				text(None, "Name", false),
				Attr { owner: None, name: "Kind", ty: AttrType::Enum(members::<SchemeKind>()), required: false },
				text(None, "Description", false),
			],
		),
		Decl::new("Stop", "EMPTY", vec![text(Some("Stop"), "At", true), text(None, "Color", false)]),
	]);
	decls
}

/// The DOCTYPE's internal subset: one `ELEMENT` per element name in written order, an `ATTLIST`
/// line per attribute (bools and enums enumerated, the rest `CDATA`, `#REQUIRED` where always written).
pub fn dtd() -> String {
	let mut out = String::from("\n");
	for d in decls() {
		out.push_str(&format!("\t<!ELEMENT {} {}>\n", d.name, d.content));
		for a in &d.attrs {
			let ty = match &a.ty {
				AttrType::Cdata => "CDATA".to_string(),
				AttrType::Bool => "(true|false)".to_string(),
				AttrType::Enum(names) => format!("({})", names.join("|")),
			};
			let presence = if a.required { "#REQUIRED" } else { "#IMPLIED" };
			out.push_str(&format!("\t<!ATTLIST {} {} {ty} {presence}>\n", d.name, a.name));
		}
	}
	out
}

// ---------------------------------------------------------------- validation

/// A child element model: `name?`, `name*` or `(a|b)*`, in sequence.
struct Particle {
	names: Vec<String>,
	many: bool,
}

struct DtdElement {
	content: Option<Vec<Particle>>,
	text: bool,
	attrs: Vec<(String, Option<Vec<String>>, bool)>,
}

fn parse_dtd(subset: &str) -> std::result::Result<std::collections::HashMap<String, DtdElement>, String> {
	let mut out = std::collections::HashMap::new();
	for decl in subset.split('<').filter_map(|d| d.trim().strip_prefix('!')) {
		let decl = decl.trim_end().trim_end_matches('>').trim();
		if let Some(rest) = decl.strip_prefix("ELEMENT ") {
			let (name, model) = rest.split_once(' ').ok_or_else(|| format!("bad ELEMENT: {decl}"))?;
			let model = model.trim();
			let (content, text) = match model {
				"EMPTY" => (Some(vec![]), false),
				"(#PCDATA)" => (Some(vec![]), true),
				"ANY" => (None, true),
				m => {
					let inner = m.strip_prefix('(').and_then(|m| m.strip_suffix(')')).ok_or("bad model")?;
					let mut particles = Vec::new();
					let mut depth = 0;
					let mut item = String::new();
					for c in inner.chars().chain(std::iter::once(',')) {
						match c {
							'(' => depth += 1,
							')' => depth -= 1,
							',' if depth == 0 => {
								let t = item.trim();
								let many = t.ends_with('*');
								let t = t.trim_end_matches(['*', '?']).trim_start_matches('(').trim_end_matches(')');
								particles
									.push(Particle { names: t.split('|').map(|s| s.trim().into()).collect(), many });
								item.clear();
								continue;
							}
							_ => {}
						}
						item.push(c);
					}
					(Some(particles), false)
				}
			};
			out.insert(name.to_string(), DtdElement { content, text, attrs: vec![] });
		} else if let Some(rest) = decl.strip_prefix("ATTLIST ") {
			let parts: Vec<&str> = rest.split_whitespace().collect();
			let [element, attr, ty, presence] = parts[..] else {
				return Err(format!("bad ATTLIST: {decl}"));
			};
			let values =
				ty.strip_prefix('(').and_then(|t| t.strip_suffix(')')).map(|t| t.split('|').map(Into::into).collect());
			let el = out.get_mut(element).ok_or_else(|| format!("ATTLIST before ELEMENT: {element}"))?;
			el.attrs.push((attr.into(), values, presence == "#REQUIRED"));
		}
	}
	Ok(out)
}

fn validate_element(el: &Element, dtd: &std::collections::HashMap<String, DtdElement>, problems: &mut Vec<String>) {
	let at = format!("line {}", el.at.0);
	let Some(decl) = dtd.get(&el.name) else {
		problems.push(format!("{at}: element '{}' is not declared", el.name));
		return;
	};
	for (k, v) in &el.attrs {
		match decl.attrs.iter().find(|(a, ..)| a == k) {
			None => problems.push(format!("{at}: attribute '{k}' is not declared on '{}'", el.name)),
			Some((_, Some(values), _)) if !values.iter().any(|x| x == v) => {
				problems.push(format!("{at}: '{v}' is not a value of '{}/@{k}'", el.name))
			}
			_ => {}
		}
	}
	for (a, _, required) in &decl.attrs {
		if *required && !el.has(a) {
			problems.push(format!("{at}: required attribute '{a}' missing on '{}'", el.name));
		}
	}
	if !decl.text && !el.text.trim().is_empty() && decl.content.is_some() {
		problems.push(format!("{at}: element '{}' holds text", el.name));
	}
	if let Some(model) = &decl.content {
		let mut i = 0;
		let mut used = false;
		for c in &el.children {
			while i < model.len() && (!model[i].names.contains(&c.name) || (used && !model[i].many)) {
				i += 1;
				used = false;
			}
			if i == model.len() {
				problems.push(format!("line {}: element '{}' is not allowed here in '{}'", c.at.0, c.name, el.name));
				break;
			}
			used = true;
		}
	}
	for c in &el.children {
		validate_element(c, dtd, problems);
	}
}

/// Validates a `.wdproj` text against the DTD in its own DOCTYPE: declared elements and attributes,
/// enumerated values, required attributes and child order. Returns the problems found.
pub fn validate(text: &str) -> Vec<String> {
	let doc = match xml::parse(text) {
		Ok(d) => d,
		Err(e) => return vec![e.to_string()],
	};
	let Some(doctype) = doc.doctype else {
		return vec!["no DOCTYPE".into()];
	};
	let subset = doctype.split_once('[').and_then(|(_, s)| s.rsplit_once(']')).map_or("", |(s, _)| s);
	let dtd = match parse_dtd(subset) {
		Ok(d) => d,
		Err(e) => return vec![e],
	};
	let mut problems = Vec::new();
	if !doctype.trim_start().starts_with(&doc.root.name) {
		problems.push(format!("the DOCTYPE does not name the root '{}'", doc.root.name));
	}
	validate_element(&doc.root, &dtd, &mut problems);
	problems
}

#[cfg(test)]
mod tests {
	use super::*;

	/// One of every kind with values off their defaults where it matters.
	fn every_kind() -> Project {
		let mut p = Project {
			seed: 1810971758,
			description: "One <of> every & kind\nsecond line".into(),
			..Project::default()
		};
		p.scene.width = 800;
		p.scene.height = 500;
		p.simulation.steps = 4000;
		p.display.export_from = 400;
		p.display.export_to = 2000;
		p.display.video.fps = 30;
		p.add_source_at(120, 160);
		let s = p.add_source_at(120, 340);
		p.sources[s].mode = SourceMode::Pulsed;
		p.sources[s].detach_after = Some(500);
		p.sources[s].array = Some(SourceArray { size_y: 7, spacing_y: 16, angle: 30.0, ..SourceArray::default() });
		for kind in [
			PartKind::Lens(Lens::default()),
			PartKind::Polygon(Polygon::default()),
			PartKind::Rectangle(Rectangle {
				coating: Coating { depth: 4.0, profile: CoatingProfile::Graded, ..Coating::default() },
				..Rectangle::default()
			}),
			PartKind::Ellipse(Ellipse { linked: true, ..Ellipse::default() }),
			PartKind::Triangle(Triangle {
				kind: TriangleType::Custom,
				angle_a: 50.0,
				angle_b: 70.0,
				..Triangle::default()
			}),
			PartKind::Outline(Outline {
				points: vec![Point::new(-40, -30), Point::new(40, -25), Point::new(30, 35)],
				..Outline::default()
			}),
			PartKind::Fiber(Fiber {
				nodes: vec![
					FiberNode { smooth: false, ..FiberNode::default() },
					FiberNode { x: 100, in_y: 5, ..FiberNode::default() },
				],
				..Fiber::default()
			}),
			PartKind::BraggReflector(BraggReflector::default()),
			PartKind::GrinLens(GrinLens::default()),
			PartKind::LogicGate(LogicGate { kind: GateKind::Xor, ..LogicGate::default() }),
			PartKind::LogicGate(LogicGate { kind: GateKind::And, input_coated: false, ..LogicGate::default() }),
			PartKind::LogicGate(LogicGate { kind: GateKind::Or, ..LogicGate::default() }),
			PartKind::ZonePlate(ZonePlate { mount: true, ..ZonePlate::default() }),
		] {
			p.add_part_at(kind, 300, 120);
		}
		p.parts[2].base.angle = 30.0;
		let a = p.add_probe_at(500, 160);
		p.probes[a].box_x = 0;
		// a point probe's line end is not stored
		(p.probes[a].bx, p.probes[a].by) = (0, 0);
		let b = p.add_probe_at(200, 60);
		let q = &mut p.probes[b];
		q.layer = Layer::Intensity;
		q.measurement = Measurement::IntegratedValue;
		q.kind = ProbeKind::Line;
		q.box_size = BoxSize::Custom;
		q.box_width = 320;
		q.box_height = 120;
		q.y_scale = YScale::Logarithmic;
		p.add_new_color_scheme();
		p.normalize();
		p
	}

	#[test]
	fn every_kind_round_trips_and_validates() {
		let p = every_kind();
		let text = write_string(&p, None);
		assert_eq!(validate(&text), Vec::<String>::new(), "{text}");
		let back = read_str(&text).unwrap();
		for (a, b) in format!("{back:#?}").lines().zip(format!("{p:#?}").lines()) {
			assert_eq!(a, b);
		}
		assert_eq!(back, p);
		assert_eq!(write_string(&back, None), text);
	}

	#[test]
	fn sparse_and_always_written() {
		let mut p = Project { seed: 7, ..Project::default() };
		p.add_source_at(0, 0);
		p.add_part_at(PartKind::Lens(Lens::default()), 0, 0);
		p.add_probe_at(0, 0);
		let text = write_string(&p, None);
		let body = text.split_once("]>\n").unwrap().1;
		let generator = format!("{GENERATOR} {}", env!("CARGO_PKG_VERSION"));
		assert_eq!(
			body,
			format!(
				"<Project Version=\"3\" Seed=\"7\" Generator=\"{generator}\">\n\t<Display>\n\t\t<Field ColorScheme=\"Default field\" Brightness=\"1\" BlackLevel=\"-0.05\" />\n\t\t<Intensity ColorScheme=\"Default intensity\" Brightness=\"1.5\" />\n\t</Display>\n\t<Source Name=\"Source 1\" X=\"0\" Y=\"0\" />\n\t<Lens Name=\"Lens 1\" X=\"0\" Y=\"0\" />\n\t<Probe Name=\"Probe 1\" AX=\"0\" AY=\"0\" />\n</Project>"
			)
		);
		assert!(read(&text).unwrap().own, "a file this program wrote is its own");
		assert!(!read("<Project Version=\"3\" Seed=\"7\" />").unwrap().own);
	}

	#[test]
	fn brightness_omitted_is_the_originals() {
		let p = read_str("<Project Version=\"3\"><Display><Field /></Display></Project>").unwrap();
		assert_eq!((p.display.field.brightness, p.display.intensity.brightness), (30.0, 30.0));
		let p = read_str("<Project Version=\"3\" />").unwrap();
		assert_eq!((p.display.field.brightness, p.display.intensity.brightness), (30.0, 30.0));
		let text = write_string(&p, None);
		assert!(!text.split_once("]>\n").unwrap().1.contains("Brightness"), "{text}");
	}

	#[test]
	fn validator_reports_mistakes() {
		let good = write_string(&every_kind(), None);
		assert!(validate(&good.replace("<Lens Name", "<Lens Nmae")).iter().any(|p| p.contains("Nmae")));
		assert!(
			validate(&good.replace("CoatingProfile=\"Graded\"", "CoatingProfile=\"Soft\""))
				.iter()
				.any(|p| p.contains("Soft"))
		);
		let moved = good.replacen("\t<Display ", "\t<Video />\n\t<Display ", 1);
		assert!(validate(&moved).iter().any(|p| p.contains("'Video'")));
	}

	#[test]
	fn migration_rules() {
		let v1 = read_str("<Project Version=\"1\" Seed=\"3\" />").unwrap();
		assert_eq!((v1.scene.edges, v1.scene.damping_edge), (Edges::Reflective, 0));
		let v2 = read_str("<Project Version=\"2\" Seed=\"3\"><Scene DampingEdge=\"-8\" /></Project>").unwrap();
		assert_eq!((v2.scene.edges, v2.scene.damping_edge), (Edges::Open, -8));
		let v3 = read_str("<Project Version=\"3\" Seed=\"3\" />").unwrap();
		assert_eq!((v3.scene.edges, v3.scene.damping_edge), (Edges::Open, -32));
		let v9 =
			read_str("<Project Version=\"9\" Seed=\"3\"><Unknown A=\"1\" /><Scene Foo=\"bar\" /></Project>").unwrap();
		assert_eq!(v9.scene.damping_edge, -32);
	}

	#[test]
	fn reading_rules() {
		let text = "<Project Seed=\"3\"><Display><Field ColorScheme=\"Rainbow\" /></Display>\
			<Rectangle Name=\"r\" X=\"1\" Y=\"2\" Linked=\"true\" Width=\"90\" Length=\"30\" />\
			<Probe Name=\"p\" AX=\"5\" AY=\"6\" BoxY=\"0\" /></Project>";
		let p = read_str(text).unwrap();
		assert_eq!(p.display.field.black_level, 0.0);
		assert_eq!(p.display.intensity.black_level, 0.0);
		let PartKind::Rectangle(r) = &p.parts[0].kind else { panic!() };
		assert_eq!((r.length, r.width), (30, 30));
		assert_eq!((p.probes[0].box_x, p.probes[0].box_y), (5, 0));
	}

	#[test]
	fn nine_point_frequencies_scale_and_unscale() {
		let text = "<Project Seed=\"3\"><Simulation FourthOrderLaplacian=\"false\" /><Source Name=\"s\" X=\"0\" Y=\"0\" Frequency=\"30\" /></Project>";
		let p = read_str(text).unwrap();
		assert_eq!(p.sources[0].frequency, 30.0);
		assert!((p.simulation.source_frequency_scale - 2.0 / 0.316f64.sqrt()).abs() < 1e-12);
		let out = write_string(&p, None);
		assert!(
			out.contains("Frequency=\"30\"") && out.contains("<Simulation FourthOrderLaplacian=\"false\" />"),
			"{out}"
		);
		let marked = text.replace("FourthOrderLaplacian=\"false\"", "FourthOrderLaplacian=\"false\" Rescaled=\"true\"");
		assert_eq!(read_str(&marked).unwrap().simulation.source_frequency_scale, 2.0);
	}

	#[test]
	fn malformed_names_the_problem() {
		let err = read_str("<Project Seed=\"1\">\n<Source X=\"1\" X=\"2\" />\n</Project>").unwrap_err();
		assert_eq!(err.at.map(|a| a.0), Some(2));
		let err = read_str("<Project Seed=\"x\" />").unwrap_err();
		assert!(err.message.contains("Seed"), "{}", err.message);
		let err = read_str("<Project Seed=\"1\"><Source Mode=\"Loud\" X=\"0\" Y=\"0\" /></Project>").unwrap_err();
		assert!(err.message.contains("Loud"));
		assert!(read_str("<ColorScheme />").is_err());
	}

	#[test]
	fn every_persisted_number_has_its_bounds_named() {
		use crate::model::ranges;
		for d in decls() {
			for a in &d.attrs {
				let (Some(owner), AttrType::Cdata) = (a.owner, &a.ty) else {
					continue;
				};
				let key = format!("{owner}.{}", a.name);
				let known = ranges::range(&key).is_some()
					|| ranges::DEPENDENT.iter().any(|(d, _)| *d == key)
					|| ranges::UNBOUNDED.contains(&key.as_str());
				assert!(known, "{}/@{} ({key}) has no declared range, dependent bound or exemption", d.name, a.name);
			}
		}
	}
}
