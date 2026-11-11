//! The `WaveDude` Next `.wdproj` XML: read (versions 1 to 4, DOCTYPE ignored, unknown elements and
//! attributes ignored) and written sparse with a DOCTYPE that declares every element and attribute.
//!
//! A sparse file means its version's defaults where it names no value, so an element is read onto the base
//! of the file's version (`FileVersion`) and written against the current one.
//!
//! Sources of a 9-point project are stored in the original's frequency units: reading multiplies
//! the frequency scale is divided by [`super::nine_point_speed`], writing multiplies it back. A file whose `Simulation` carries
//! `Rescaled="true"` already holds this program's units and is read unscaled.
//!
//! `Brightness` is in `WaveDude` Next's units in the file and in this program's in the model ([`BRIGHTNESS_GAIN`]).

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::fields::{self, Attr, AttrDecl, AttrType, Table};
use super::xml::{self, DECLARATION, Element};
use super::{FormatError, Result, UnreadableFile, wdscheme};
use crate::model::normalize::Normalize;
use crate::model::*;

/// File extension of a project.
pub const EXTENSION: &str = "wdproj";

/// The part elements in declaration order.
// Next's declaration order, which its DTD and ours follow.
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
	"Diffuser",
	"InterferenceFilter",
	"SimpleGrating",
	"FacettedGrating",
	"MetaLens",
	"MetaPrism",
	"MetaAxicon",
	"FresnelLens",
];

type R<T> = std::result::Result<T, FormatError>;

// ---------------------------------------------------------------- reading

/// Parses a `.wdproj` text into a normalised project (migrated, 9-point frequencies scaled).
/// The scene's `image_size` is left at (0, 0); [`load`] decodes it.
pub fn read_str(text: &str) -> R<Project> {
	let doc = xml::parse(text)?;
	let root = &doc.root;
	if root.name != "Project" {
		return Err(FormatError { message: "the document is not a WaveDude Next project".into(), at: Some(root.at) });
	}
	let version = FileVersion(root.i32("Version", PROJECT_VERSION as i32)?);
	let seed = match root.get("Seed") {
		Some(v) => root.parse_i32("Seed", v)?,
		None => Project::new_random_seed().seed,
	};
	let display = Display {
		field: original_layer(Layer::Field, version),
		intensity: original_layer(Layer::Intensity, version),
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
			"Display" => project.display = read_display(el, version)?,
			"Source" => project.sources.push(read_source(el)?),
			"Probe" => project.probes.push(read_probe(el)?),
			"ColorScheme" => project.color_schemes.push(wdscheme::from_element(el)?),
			name if PART_ELEMENTS.contains(&name) => project.parts.push(read_part(el, version)?),
			name => tracing::warn!("the element '{name}' is unknown and ignored"),
		}
	}
	if version.0 > PROJECT_VERSION as i32 {
		tracing::warn!(
			"project file version {} is newer than the supported {PROJECT_VERSION}; unknown settings are ignored",
			version.0
		);
	}
	if version.0 < 2 {
		project.scene.edges = Edges::Reflective;
	}
	if version.0 < 3 && !damping_read {
		project.scene.damping_edge = 0;
	}
	if !rescaled && super::scale_nine_point_frequencies(&mut project, super::nine_point_speed()) {
		tracing::info!(
			"9-point project: the frequency scale is divided by the original kernel's speed {:.4} so the wavelengths stay",
			super::nine_point_speed()
		);
	}
	project.normalize();
	Ok(project)
}

/// The `Version` of the file being read: what its omitted attributes mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileVersion(i32);

const CURRENT: FileVersion = FileVersion(PROJECT_VERSION as i32);

impl FileVersion {
	/// Until version 4 (2026-11-09) a layer without `OverlayInExport` meant off, and a zone plate without
	/// `Thickness` or `Coating` meant 64 and 32; the defaults since are on, 32 and 0.
	fn before_4(self) -> bool {
		self.0 < 4
	}

	fn overlay_in_export(self) -> bool {
		!self.before_4()
	}

	fn zone_plate(self) -> ZonePlate {
		if self.before_4() {
			ZonePlate { thickness: 64, coating: 32, ..ZonePlate::default() }
		} else {
			ZonePlate::default()
		}
	}
}

// ---------------------------------------------------------------- attribute tables

impl Table for Scene {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(SceneField::Mode),
		Attr::of(SceneField::Image),
		Attr::of(SceneField::ImagePath),
		Attr::of(SceneField::Width),
		Attr::of(SceneField::Height),
		Attr::of(SceneField::DampingEdge),
		Attr::of(SceneField::Edges),
		Attr::of(SceneField::RefractiveIndex),
		Attr::of(SceneField::ImageSources),
		Attr::of(SceneField::ImageSourceScale),
		Attr::of(SceneField::ObjectSources),
	];
}

impl Table for Simulation {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(SimulationField::Steps),
		Attr::of(SimulationField::VisualizeEvery),
		Attr::custom(
			SimulationField::Stencil,
			"FourthOrderLaplacian",
			|s| (s.stencil == Stencil::FourthOrder).to_string(),
			|s, text| {
				let fourth = xml::parse_bool(text).ok_or("true or false")?;
				s.stencil = if fourth { Stencil::FourthOrder } else { Stencil::NinePoint };
				Ok(())
			},
			AttrType::Bool,
		),
		Attr::of(SimulationField::WaveSpeed),
		Attr::of(SimulationField::TimeDecay),
		Attr::of(SimulationField::SourceFrequencyScale),
		Attr::of(SimulationField::IntensityScale),
	];
}

/// Factor from this program's brightness to `WaveDude` Next's: this program's source swings its cell 4.8 to 9.7 times
/// as far (docs/comparison), so the same picture takes that much less gain there. A power of two, so a value
/// converts both ways exactly.
pub const BRIGHTNESS_GAIN: f64 = 8.0;

/// The brightness `WaveDude` Next omits from a file, in this program's units.
pub(super) const ORIGINAL_BRIGHTNESS: f64 = 30.0 / BRIGHTNESS_GAIN;

/// The `Brightness` attribute: `WaveDude` Next's units in the file.
const BRIGHTNESS: Attr<DisplayLayer> = Attr::custom(
	DisplayLayerField::Brightness,
	"Brightness",
	|l| xml::fmt_f64(l.brightness * BRIGHTNESS_GAIN),
	|l, text| {
		l.brightness = xml::parse_f64(text).ok_or("a number")? / BRIGHTNESS_GAIN;
		Ok(())
	},
	AttrType::Cdata,
);

/// A layer's defaults as `WaveDude` Next has them in a file of `version`.
fn original_layer(layer: Layer, version: FileVersion) -> DisplayLayer {
	let d = match layer {
		Layer::Field => DisplayLayer::field_default(),
		Layer::Intensity => DisplayLayer::intensity_default(),
	};
	DisplayLayer { brightness: ORIGINAL_BRIGHTNESS, overlay_in_export: version.overlay_in_export(), ..d }
}

/// A `Field` or `Intensity` element: a missing `ColorScheme` is empty (the layer's default after
/// normalising), a missing `BlackLevel` is 0 and a missing `Brightness` 30, as `WaveDude` Next reads them.
fn layer_base() -> DisplayLayer {
	DisplayLayer {
		color_scheme: String::new(),
		brightness: ORIGINAL_BRIGHTNESS,
		black_level: 0.0,
		..DisplayLayer::field_default()
	}
}

/// [`layer_base`] as a file of `version` means it.
fn layer_base_of(version: FileVersion) -> DisplayLayer {
	DisplayLayer { overlay_in_export: version.overlay_in_export(), ..layer_base() }
}

/// The `Intensity` element; `Field` writes its `BlackLevel` always (`WaveDude` Next's default 0 is not its initial
/// -0.05).
impl Table for DisplayLayer {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(DisplayLayerField::ColorScheme).always(),
		BRIGHTNESS,
		Attr::of(DisplayLayerField::BlackLevel),
		Attr::of(DisplayLayerField::Preview),
		Attr::of(DisplayLayerField::Overlay),
		Attr::of(DisplayLayerField::OverlayGain),
		Attr::of(DisplayLayerField::OverlayInExport),
	];

	fn base() -> Self {
		layer_base()
	}
}

const FIELD_LAYER: &[Attr<DisplayLayer>] = &[
	Attr::of(DisplayLayerField::ColorScheme).always(),
	BRIGHTNESS,
	Attr::of(DisplayLayerField::BlackLevel).when(|_| true),
	Attr::of(DisplayLayerField::Preview),
	Attr::of(DisplayLayerField::Overlay),
	Attr::of(DisplayLayerField::OverlayGain),
	Attr::of(DisplayLayerField::OverlayInExport),
];

impl Table for Video {
	const ATTRS: &'static [Attr<Self>] = &[Attr::of(VideoField::Fps), Attr::of(VideoField::Quality)];
}

impl Table for Display {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(DisplayField::ExportFrom),
		Attr::of(DisplayField::ExportTo),
		Attr::of(DisplayField::ProbeGraphsInExport),
		Attr::of(DisplayField::ProbeMarkersInExport),
		Attr::of(DisplayField::SourceMarkersInExport),
		Attr::of(DisplayField::HandlesInPreview),
		Attr::of(DisplayField::LabelsInPreview),
	];
}

impl Table for Source {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(SourceField::Name).always(),
		Attr::of(SourceField::Enabled),
		Attr::of(SourceField::X).whole().always(),
		Attr::of(SourceField::Y).whole().always(),
		Attr::of(SourceField::Frequency),
		Attr::of(SourceField::Amplitude),
		Attr::of(SourceField::Phase),
		Attr::of(SourceField::Mode),
		Attr::of(SourceField::PulseShape),
		Attr::of(SourceField::PulseWidth),
		Attr::of(SourceField::OffWidth),
		Attr::custom(
			SourceField::DetachAfter,
			"DetachAfter",
			|s| s.detach_after.map_or(-1, i64::from).to_string(),
			|s, text| {
				s.detach_after = u32::try_from(xml::parse_i32(text).ok_or("an integer")?).ok();
				Ok(())
			},
			AttrType::Cdata,
		),
	];
}

impl Table for SourceArray {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(SourceArrayField::SizeX),
		Attr::of(SourceArrayField::SizeY),
		Attr::of(SourceArrayField::SpacingX),
		Attr::of(SourceArrayField::SpacingY),
		Attr::of(SourceArrayField::Angle),
		Attr::of(SourceArrayField::RandomFrequency),
		Attr::of(SourceArrayField::RandomPhase),
		Attr::of(SourceArrayField::RandomOffWidth),
		Attr::of(SourceArrayField::RandomPosition),
	];
}

/// A missing `BoxX`/`BoxY` reads as A's coordinate (`WaveDude` Next's box follows A while attached, and its reader
/// set A before the box), so 0 is written whenever A's is not 0.
impl Table for Probe {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(ProbeField::Name).always(),
		Attr::of(ProbeField::Active),
		Attr::of(ProbeField::Layer),
		Attr::of(ProbeField::Measurement),
		Attr::of(ProbeField::Kind),
		Attr::of(ProbeField::Detector),
		Attr::of(ProbeField::Sensitivity),
		Attr::of(ProbeField::BinSize),
		Attr::of(ProbeField::Ax).named("AX").always(),
		Attr::of(ProbeField::Ay).named("AY").always(),
		Attr::of(ProbeField::Bx).named("BX").when(|p| p.kind == ProbeKind::Line),
		Attr::of(ProbeField::By).named("BY").when(|p| p.kind == ProbeKind::Line),
		Attr::of(ProbeField::BoxSize),
		Attr::of(ProbeField::BoxX).when(|p| p.box_x != 0 || p.ax != 0),
		Attr::of(ProbeField::BoxY).when(|p| p.box_y != 0 || p.ay != 0),
		Attr::of(ProbeField::BoxWidth).when(|p| p.box_size == BoxSize::Custom),
		Attr::of(ProbeField::BoxHeight).when(|p| p.box_size == BoxSize::Custom),
		Attr::of(ProbeField::ScaleMode),
		Attr::of(ProbeField::AutoScaleX),
		Attr::of(ProbeField::AutoScaleY),
		Attr::of(ProbeField::YMin),
		Attr::of(ProbeField::YMax),
		Attr::of(ProbeField::YScale),
		Attr::of(ProbeField::Decades),
		Attr::of(ProbeField::Window),
		Attr::of(ProbeField::StartAfter),
	];
}

impl Table for PartBase {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(PartBaseField::Name).always(),
		Attr::of(PartBaseField::Enabled),
		Attr::of(PartBaseField::X).always(),
		Attr::of(PartBaseField::Y).always(),
		Attr::of(PartBaseField::Angle),
	];
}

impl Table for Coating {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(CoatingField::Depth).named("Coating").whole(),
		Attr::of(CoatingField::Profile).named("CoatingProfile"),
		Attr::of(CoatingField::Front).named("FrontCoated"),
		Attr::of(CoatingField::Back).named("BackCoated"),
		Attr::of(CoatingField::Sides).named("SidesCoated"),
	];
}

impl Table for Material {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(MaterialField::Mirror),
		Attr::of(MaterialField::RefractiveIndex),
		Attr::of(MaterialField::Absorption),
		Attr::of(MaterialField::AbsorptionRamp),
	];
}

impl Table for Lens {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(LensField::Diameter),
		Attr::of(LensField::R1),
		Attr::of(LensField::R2),
		Attr::of(LensField::Thickness),
	];
}
// its lens's attributes come first (read_part, write_part)
impl Table for FresnelLens {
	const ATTRS: &'static [Attr<Self>] = &[Attr::of(FresnelLensField::Division), Attr::of(FresnelLensField::Zones)];
}
impl Table for Polygon {
	const ATTRS: &'static [Attr<Self>] = &[Attr::of(PolygonField::Sides), Attr::of(PolygonField::Side)];
}
impl Table for Rectangle {
	const ATTRS: &'static [Attr<Self>] =
		&[Attr::of(RectangleField::Length), Attr::of(RectangleField::Width), Attr::of(RectangleField::Linked)];
}
impl Table for Ellipse {
	const ATTRS: &'static [Attr<Self>] =
		&[Attr::of(EllipseField::Length), Attr::of(EllipseField::Width), Attr::of(EllipseField::Linked)];
}
impl Table for Triangle {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(TriangleField::Base),
		Attr::of(TriangleField::Kind).named("Type"),
		Attr::of(TriangleField::AngleA),
		Attr::of(TriangleField::AngleB),
	];
}
impl Table for Outline {
	const ATTRS: &'static [Attr<Self>] = &[];
}
impl Table for Diffuser {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(DiffuserField::Length),
		Attr::of(DiffuserField::Width),
		Attr::of(DiffuserField::CenterWavelength),
		Attr::of(DiffuserField::Amplitude),
		Attr::of(DiffuserField::Sigma),
	];
}
impl Table for InterferenceFilter {
	const ATTRS: &'static [Attr<Self>] =
		&[Attr::of(InterferenceFilterField::Length), Attr::of(InterferenceFilterField::Width)];
}
impl Table for FilterLayer {
	const ATTRS: &'static [Attr<Self>] =
		&[Attr::of(FilterLayerField::Thickness), Attr::of(FilterLayerField::RefractiveIndex)];
}
impl Table for SimpleGrating {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(SimpleGratingField::Columns),
		Attr::of(SimpleGratingField::Rows),
		Attr::of(SimpleGratingField::ColumnSpacing),
		Attr::of(SimpleGratingField::RowSpacing),
		Attr::of(SimpleGratingField::Diameter),
	];
}
// the design's attributes follow (`read_part`, `write_part`, `part_attrs`)
impl Table for MetaSurface {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(MetaSurfaceField::Length),
		Attr::of(MetaSurfaceField::Width),
		Attr::of(MetaSurfaceField::DesignFrequency),
		Attr::of(MetaSurfaceField::Pitch),
		Attr::of(MetaSurfaceField::Height),
		Attr::of(MetaSurfaceField::PatternIndex),
	];
}
impl Table for MetaLensDesign {
	const ATTRS: &'static [Attr<Self>] =
		&[Attr::of(MetaLensDesignField::FocalLength), Attr::of(MetaLensDesignField::LensType)];
}
impl Table for MetaPrismDesign {
	const ATTRS: &'static [Attr<Self>] = &[Attr::of(MetaPrismDesignField::Deflection)];
}
impl Table for MetaAxiconDesign {
	const ATTRS: &'static [Attr<Self>] = &[Attr::of(MetaAxiconDesignField::ConeAngle)];
}
impl Table for FacettedGrating {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(FacettedGratingField::Length),
		Attr::of(FacettedGratingField::Width),
		Attr::of(FacettedGratingField::Pattern),
		Attr::of(FacettedGratingField::Pitch),
		Attr::of(FacettedGratingField::Amplitude),
	];
}
impl Table for Fiber {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(FiberField::CoreIndex),
		Attr::of(FiberField::CoreDiameter),
		Attr::of(FiberField::CladdingIndex),
		Attr::of(FiberField::CladdingThickness),
		Attr::of(FiberField::Absorption),
		Attr::of(FiberField::AbsorptionRamp),
		Attr::of(FiberField::Ends),
	];
}
impl Table for BraggReflector {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(BraggReflectorField::Pairs),
		Attr::of(BraggReflectorField::Thickness1),
		Attr::of(BraggReflectorField::Thickness2),
		Attr::of(BraggReflectorField::Index1),
		Attr::of(BraggReflectorField::Index2),
		Attr::of(BraggReflectorField::Width),
	];
}
impl Table for GrinLens {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(GrinLensField::Length),
		Attr::of(GrinLensField::Diameter),
		Attr::of(GrinLensField::CentreIndex),
		Attr::of(GrinLensField::EdgeIndex),
	];
}
impl Table for LogicGate {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(LogicGateField::DesignFrequency),
		Attr::of(LogicGateField::Coating).whole(),
		Attr::of(LogicGateField::InputCoated),
		Attr::of(LogicGateField::OutputCoated),
		Attr::of(LogicGateField::UpperOutputActive),
		Attr::of(LogicGateField::LowerOutputActive),
		Attr::of(LogicGateField::CoreIndex),
		Attr::of(LogicGateField::CladdingIndex),
		Attr::of(LogicGateField::PortSpacing),
	];
}
impl Table for ZonePlate {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(ZonePlateField::Mount),
		Attr::of(ZonePlateField::MountWidth),
		Attr::of(ZonePlateField::Diameter),
		Attr::of(ZonePlateField::Thickness),
		Attr::of(ZonePlateField::Coating),
		Attr::of(ZonePlateField::DesignFrequency),
		Attr::of(ZonePlateField::FocalLength),
		Attr::of(ZonePlateField::Inverse),
	];
}

/// An outline's corner.
impl Table for Point {
	const ATTRS: &'static [Attr<Self>] = &[Attr::of(PointField::X).always(), Attr::of(PointField::Y).always()];
}

impl Table for FiberNode {
	const ATTRS: &'static [Attr<Self>] = &[
		Attr::of(FiberNodeField::X).always(),
		Attr::of(FiberNodeField::Y).always(),
		Attr::of(FiberNodeField::InX),
		Attr::of(FiberNodeField::InY),
		Attr::of(FiberNodeField::OutX),
		Attr::of(FiberNodeField::OutY),
		Attr::of(FiberNodeField::Smooth),
	];
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

fn read_display(el: &Element, version: FileVersion) -> R<Display> {
	let mut display: Display = fields::read(el)?;
	let layer = |name, layer| match el.child(name) {
		Some(el) => fields::read_onto(el, layer_base_of(version)),
		None => Ok(original_layer(layer, version)),
	};
	display.field = layer("Field", Layer::Field)?;
	display.intensity = layer("Intensity", Layer::Intensity)?;
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

fn read_part(el: &Element, version: FileVersion) -> R<Part> {
	let (coating, material) = (fields::read(el), fields::read(el));
	let kind = match el.name.as_str() {
		"Lens" => PartKind::Lens(Lens { coating: coating?, material: material?, ..fields::read(el)? }),
		"FresnelLens" => PartKind::FresnelLens(FresnelLens {
			lens: Lens { coating: coating?, material: material?, ..fields::read(el)? },
			..fields::read(el)?
		}),
		"Polygon" => PartKind::Polygon(Polygon { coating: coating?, material: material?, ..fields::read(el)? }),
		"Rectangle" => PartKind::Rectangle(Rectangle { coating: coating?, material: material?, ..fields::read(el)? }),
		"Ellipse" => PartKind::Ellipse(Ellipse { coating: coating?, material: material?, ..fields::read(el)? }),
		"Triangle" => PartKind::Triangle(Triangle { coating: coating?, material: material?, ..fields::read(el)? }),
		"Outline" => PartKind::Outline(Outline {
			coating: coating?,
			material: material?,
			points: el.children("Point").map(fields::read).collect::<R<_>>()?,
		}),
		"Diffuser" => PartKind::Diffuser(Diffuser { coating: coating?, material: material?, ..fields::read(el)? }),
		"InterferenceFilter" => PartKind::InterferenceFilter(InterferenceFilter {
			coating: coating?,
			material: material?,
			layers: el.children("Layer").map(fields::read).collect::<R<_>>()?,
			..fields::read(el)?
		}),
		"SimpleGrating" => {
			PartKind::SimpleGrating(SimpleGrating { coating: coating?, material: material?, ..fields::read(el)? })
		}
		"MetaLens" | "MetaPrism" | "MetaAxicon" => {
			let design = match el.name.as_str() {
				"MetaLens" => MetaDesign::Lens(fields::read(el)?),
				"MetaPrism" => MetaDesign::Prism(fields::read(el)?),
				_ => MetaDesign::Axicon(fields::read(el)?),
			};
			PartKind::MetaSurface(MetaSurface { coating: coating?, material: material?, design, ..fields::read(el)? })
		}
		"FacettedGrating" => {
			PartKind::FacettedGrating(FacettedGrating { coating: coating?, material: material?, ..fields::read(el)? })
		}
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
		"ZonePlate" => PartKind::ZonePlate(fields::read_onto(el, version.zone_plate())?),
		other => unreachable!("not a part element: {other}"),
	};
	Ok(Part { base: fields::read(el)?, kind })
}

/// Loads a `.wdproj` file and decodes the embedded image's size.
pub fn load(path: &Path) -> Result<Project> {
	let text = super::read_text(path)?;
	tracing::info!("loading project from '{}'", path.display());
	let mut project = read_str(&text).map_err(|e| UnreadableFile::new(path, &text, e))?;
	super::decode_image_size(&mut project, path)?;
	Ok(project)
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

/// Warns once per attribute of `name` whose value is not whole: it is written rounded, as `WaveDude` Next reads
/// integers only.
fn warn_rounded(name: &str, values: &[(&str, f64)]) {
	for (attr, v) in values {
		if v.fract() != 0.0 {
			tracing::warn!("'{name}': {attr} {v} is written rounded, WaveDude Next reads whole numbers only");
		}
	}
}

fn write_source(s: &Source) -> Element {
	warn_rounded(&s.name, &[("X", s.x), ("Y", s.y)]);
	let mut el = element("Source", s);
	if let Some(a) = &s.array {
		el.child_el(element("Array", a));
	}
	el
}

fn part_element_name(kind: &PartKind) -> &'static str {
	match kind {
		PartKind::Lens(_) => "Lens",
		PartKind::FresnelLens(_) => "FresnelLens",
		PartKind::Polygon(_) => "Polygon",
		PartKind::Rectangle(_) => "Rectangle",
		PartKind::Ellipse(_) => "Ellipse",
		PartKind::Triangle(_) => "Triangle",
		PartKind::Outline(_) => "Outline",
		PartKind::Diffuser(_) => "Diffuser",
		PartKind::InterferenceFilter(_) => "InterferenceFilter",
		PartKind::SimpleGrating(_) => "SimpleGrating",
		PartKind::MetaSurface(m) => match m.design {
			MetaDesign::Lens(_) => "MetaLens",
			MetaDesign::Prism(_) => "MetaPrism",
			MetaDesign::Axicon(_) => "MetaAxicon",
		},
		PartKind::FacettedGrating(_) => "FacettedGrating",
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
			PartKind::FresnelLens($k) => $e,
			PartKind::Polygon($k) => $e,
			PartKind::Rectangle($k) => $e,
			PartKind::Ellipse($k) => $e,
			PartKind::Triangle($k) => $e,
			PartKind::Outline($k) => $e,
			PartKind::Diffuser($k) => $e,
			PartKind::InterferenceFilter($k) => $e,
			PartKind::SimpleGrating($k) => $e,
			PartKind::MetaSurface($k) => $e,
			PartKind::FacettedGrating($k) => $e,
			PartKind::Fiber($k) => $e,
			PartKind::BraggReflector($k) => $e,
			PartKind::GrinLens($k) => $e,
			PartKind::LogicGate($k) => $e,
			PartKind::ZonePlate($k) => $e,
		}
	};
}

/// Evaluates `$e` with `$d` bound to the design's own value, whatever the design.
macro_rules! with_design {
	($design:expr, $d:ident => $e:expr) => {
		match $design {
			MetaDesign::Lens($d) => $e,
			MetaDesign::Prism($d) => $e,
			MetaDesign::Axicon($d) => $e,
		}
	};
}

fn write_part(p: &Part) -> Element {
	let mut el = element(part_element_name(&p.kind), &p.base);
	if let Some(c) = p.kind.coating() {
		warn_rounded(&p.base.name, &[("Coating", c.depth)]);
		fields::write(c, &mut el);
	}
	if let PartKind::LogicGate(g) = &p.kind {
		warn_rounded(&p.base.name, &[("Coating", g.coating)]);
	}
	if let Some(m) = p.kind.material() {
		fields::write(m, &mut el);
	}
	if let PartKind::FresnelLens(f) = &p.kind {
		fields::write(&f.lens, &mut el);
	}
	with_kind!(&p.kind, k => fields::write(k, &mut el));
	if let PartKind::MetaSurface(m) = &p.kind {
		with_design!(&m.design, d => fields::write(d, &mut el));
	}
	match &p.kind {
		PartKind::Outline(k) => el.children.extend(k.points.iter().map(|pt| element("Point", pt))),
		PartKind::Fiber(k) => el.children.extend(k.nodes.iter().map(|n| element("Node", n))),
		PartKind::InterferenceFilter(k) => el.children.extend(k.layers.iter().map(|l| element("Layer", l))),
		PartKind::Lens(_)
		| PartKind::FresnelLens(_)
		| PartKind::Polygon(_)
		| PartKind::Rectangle(_)
		| PartKind::Ellipse(_)
		| PartKind::Triangle(_)
		| PartKind::Diffuser(_)
		| PartKind::SimpleGrating(_)
		| PartKind::FacettedGrating(_)
		| PartKind::MetaSurface(_)
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
	root.attr("Version", PROJECT_VERSION.to_string()).attr("Seed", project.seed.to_string());
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
	attrs: Vec<AttrDecl>,
}

impl Decl {
	fn new(name: &'static str, content: &str, attrs: Vec<AttrDecl>) -> Self {
		Self { name, content: content.into(), attrs }
	}

	fn of<T: Table>(name: &'static str, content: &str) -> Self {
		Self::new(name, content, fields::attrs(T::ATTRS))
	}
}

/// A part element's attributes: the part's, the coating's and the material's when the kind has them, its own.
fn part_attrs(kind: &PartKind) -> Vec<AttrDecl> {
	let mut attrs = fields::attrs(PartBase::ATTRS);
	if kind.coating().is_some() {
		attrs.extend(fields::attrs(Coating::ATTRS));
	}
	if kind.material().is_some() {
		attrs.extend(fields::attrs(Material::ATTRS));
	}
	if let PartKind::FresnelLens(_) = kind {
		attrs.extend(fields::attrs(Lens::ATTRS));
	}
	fn own<T: Table>(_: &T) -> Vec<AttrDecl> {
		fields::attrs(T::ATTRS)
	}
	attrs.extend(with_kind!(kind, k => own(k)));
	if let PartKind::MetaSurface(m) = kind {
		attrs.extend(with_design!(&m.design, d => own(d)));
	}
	attrs
}

fn decls() -> Vec<Decl> {
	let text = |name, required| AttrDecl { name, ty: AttrType::Cdata, required };
	let mut decls = vec![
		Decl::new(
			"Project",
			&format!(
				"(Description?, Scene?, Simulation?, Display?, Source*, ({})*, Probe*, ColorScheme*)",
				PART_ELEMENTS.join("|")
			),
			vec![text("Version", true), text("Seed", true)],
		),
		Decl::new("Description", "(#PCDATA)", vec![]),
		Decl::of::<Scene>("Scene", "(ImageData?)"),
		Decl::new("ImageData", "(#PCDATA)", vec![]),
		Decl::new("Simulation", "EMPTY", {
			let mut attrs = fields::attrs(Simulation::ATTRS);
			attrs.push(AttrDecl { name: "Rescaled", ty: AttrType::Bool, required: false });
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
		let part = read_part(&Element::new(name), CURRENT).expect("an element without attributes reads");
		let content = match name {
			"Outline" => "(Point*)",
			"Fiber" => "(Node*)",
			"InterferenceFilter" => "(Layer*)",
			_ => "EMPTY",
		};
		decls.push(Decl::new(name, content, part_attrs(&part.kind)));
		match name {
			"Outline" => decls.push(Decl::of::<Point>("Point", "EMPTY")),
			"Fiber" => decls.push(Decl::of::<FiberNode>("Node", "EMPTY")),
			"InterferenceFilter" => decls.push(Decl::of::<FilterLayer>("Layer", "EMPTY")),
			_ => {}
		}
	}
	decls.extend([
		Decl::of::<Probe>("Probe", "EMPTY"),
		Decl::new(
			"ColorScheme",
			"(Stop*)",
			vec![
				text("Name", false),
				AttrDecl { name: "Kind", ty: AttrType::Enum(SchemeKind::NAMES), required: false },
				text("Description", false),
			],
		),
		Decl::new("Stop", "EMPTY", vec![text("At", true), text("Color", false)]),
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
				problems.push(format!("{at}: '{v}' is not a value of '{}/@{k}'", el.name));
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
	use crate::model::{Bounds, Field, ValueKind};

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
			PartKind::FresnelLens(FresnelLens { division: ZoneDivision::EqualDepth, ..FresnelLens::new_plate() }),
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
			PartKind::Diffuser(Diffuser { sigma: 0.0, ..Diffuser::new_part() }),
			PartKind::InterferenceFilter(InterferenceFilter::new_part()),
			PartKind::SimpleGrating(SimpleGrating { columns: 2, ..SimpleGrating::new_part() }),
			PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Lens(MetaLensDesign {
				lens_type: MetaLensType::Diverging,
				..MetaLensDesign::default()
			}))),
			PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Prism(MetaPrismDesign { deflection: -15.0 }))),
			PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Axicon(MetaAxiconDesign::default()))),
			PartKind::FacettedGrating(FacettedGrating {
				pattern: FacetPattern::Sinusoidal,
				..FacettedGrating::default()
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
		let d = p.add_probe_at(300, 60);
		p.probes[d].detector = ProbeDetector::Imaging;
		p.probes[d].bin_size = 2;
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
		assert_eq!(
			body,
			"<Project Version=\"4\" Seed=\"7\">\n\t<Display>\n\t\t<Field ColorScheme=\"Default field\" Brightness=\"8\" BlackLevel=\"-0.05\" />\n\t\t<Intensity ColorScheme=\"Default intensity\" Brightness=\"12\" />\n\t</Display>\n\t<Source Name=\"Source 1\" X=\"0\" Y=\"0\" />\n\t<Lens Name=\"Lens 1\" X=\"0\" Y=\"0\" />\n\t<Probe Name=\"Probe 1\" AX=\"0\" AY=\"0\" />\n</Project>"
		);
	}

	#[test]
	fn decimals_are_written_rounded() {
		let mut p = Project { seed: 7, ..Project::default() };
		p.add_source_at(0, 0);
		p.sources[0].x = 41.5;
		p.sources[0].y = 3.2;
		p.add_part_at(PartKind::Lens(Lens::default()), 0, 0);
		if let PartKind::Lens(l) = &mut p.parts[0].kind {
			l.coating.depth = 2.6;
		}
		let text = write_string(&p, None);
		assert!(text.contains("X=\"42\" Y=\"3\""), "{text}");
		assert!(text.contains("Coating=\"3\""), "{text}");
		let back = read_str(&text).unwrap();
		assert_eq!((back.sources[0].x, back.sources[0].y), (42.0, 3.0));
	}

	#[test]
	fn brightness_is_next_units_in_the_file() {
		let p = read_str("<Project Version=\"3\"><Display><Field /></Display></Project>").unwrap();
		assert_eq!((p.display.field.brightness, p.display.intensity.brightness), (3.75, 3.75));
		let p = read_str("<Project Version=\"3\" />").unwrap();
		assert_eq!((p.display.field.brightness, p.display.intensity.brightness), (3.75, 3.75));
		let text = write_string(&p, None);
		assert!(!text.split_once("]>\n").unwrap().1.contains("Brightness"), "{text}");
		let p = read_str("<Project Version=\"4\"><Display><Field Brightness=\"16\" /></Display></Project>").unwrap();
		assert_eq!(p.display.field.brightness, 2.0);
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

	/// Until version 4 a layer without `OverlayInExport` meant off and a zone plate without `Thickness` or
	/// `Coating` meant 64 and 32; a version-4 file means the defaults since, and so does a written file.
	#[test]
	fn version_4_changed_what_an_omitted_attribute_means() {
		let plate = |p: &Project| match &p.parts[0].kind {
			PartKind::ZonePlate(z) => (z.thickness, z.coating),
			_ => panic!(),
		};
		let overlays = |p: &Project| (p.display.field.overlay_in_export, p.display.intensity.overlay_in_export);
		let body = "<Display><Field /></Display><ZonePlate Name=\"z\" X=\"0\" Y=\"0\" Coating=\"5\" />";
		let v3 = read_str(&format!("<Project Version=\"3\">{body}</Project>")).unwrap();
		assert_eq!((overlays(&v3), plate(&v3)), ((false, false), (64, 5)));
		let v4 = read_str(&format!("<Project Version=\"4\">{body}</Project>")).unwrap();
		assert_eq!((overlays(&v4), plate(&v4)), ((true, true), (32, 5)));
		let bare = read_str("<Project Version=\"3\" />").unwrap();
		assert_eq!(overlays(&bare), (false, false));
		let text = write_string(&v3, None);
		assert!(text.contains("OverlayInExport=\"false\"") && text.contains("Thickness=\"64\""), "{text}");
		assert_eq!(read_str(&text).unwrap(), v3);
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

	/// Calls `$f::<T>()` for every type with a table.
	macro_rules! each_table {
		($f:ident) => {
			each_table!(@call $f; Scene, Simulation, DisplayLayer, Video, Display, Source, SourceArray, Probe, PartBase,
				Coating, Material, Lens, FresnelLens, Polygon, Rectangle, Ellipse, Triangle, Outline, Diffuser,
				InterferenceFilter, FilterLayer, SimpleGrating, MetaSurface, MetaLensDesign, MetaPrismDesign,
				MetaAxiconDesign, FacettedGrating, Fiber, BraggReflector, GrinLens, LogicGate, ZonePlate, Point,
				FiberNode)
		};
		(@call $f:ident; $($t:ty),*) => { $($f::<$t>();)* };
	}

	/// Numbers without bounds (positions, spacings, offsets), by `Type.Path`.
	const UNBOUNDED: &[&str] = &[
		"Project.Seed",
		"Source.X",
		"Source.Y",
		"SourceArray.SpacingX",
		"SourceArray.SpacingY",
		"Probe.Ax",
		"Probe.Ay",
		"Probe.Bx",
		"Probe.By",
		"Probe.BoxX",
		"Probe.BoxY",
		"Point.X",
		"Point.Y",
		"FiberNode.X",
		"FiberNode.Y",
		"FiberNode.InX",
		"FiberNode.InY",
		"FiberNode.OutX",
		"FiberNode.OutY",
	];

	fn assert_bounds_named(key: &str, bounds: Bounds) {
		assert!(
			matches!(bounds, Bounds::Range(_) | Bounds::Dependent) || UNBOUNDED.contains(&key),
			"{key} has no range, dependent bound or exemption"
		);
	}

	fn bounds_are_named<T: Table>() {
		for a in T::ATTRS {
			let field = a.field;
			let key = format!("{}.{}", T::TYPE_NAME, field.path());
			match field.kind() {
				ValueKind::Int | ValueKind::UInt | ValueKind::Float => assert_bounds_named(&key, field.bounds()),
				ValueKind::Text | ValueKind::Bool | ValueKind::Word(_) => {}
				ValueKind::List | ValueKind::Nested => panic!("{key} is no scalar, a table cannot carry it"),
			}
		}
	}

	#[test]
	fn every_persisted_number_has_its_bounds_named() {
		each_table!(bounds_are_named);
		// the numbers of the hand-written declarations: the project's `Seed` and a stop's `At`
		assert_bounds_named("Project.Seed", ProjectField::Seed.bounds());
		assert_bounds_named("Stop.At", StopField::At.bounds());
	}

	/// Every leaf of the type's fields is an attribute of its table, or under one of `skips` (a path or its prefix).
	fn covered_but<T: Table>(skips: &[&str]) {
		let carried: Vec<T::Field> = T::ATTRS.iter().map(|a| a.field).collect();
		let all = <T::Field as Field>::all();
		for f in &carried {
			assert!(all.contains(f), "{}.{f:?} is no leaf of the type", T::TYPE_NAME);
		}
		for f in all {
			let path = f.path();
			let skipped = skips.iter().any(|s| path == *s || path.starts_with(&format!("{s}.")));
			assert!(
				carried.contains(&f) != skipped,
				"{}.{path} is {}",
				T::TYPE_NAME,
				if skipped { "skipped but in the table" } else { "in no table and not skipped" }
			);
		}
	}

	/// The sub-tables every part kind but a few reads beside its own: the coating's and the material's.
	const SHAPE_PARTS: &[&str] = &["Coating", "Material"];

	#[test]
	fn every_field_of_a_table_type_is_an_attribute_or_skipped() {
		covered_but::<Scene>(&[]);
		covered_but::<Simulation>(&[]);
		covered_but::<DisplayLayer>(&[]);
		covered_but::<Video>(&[]);
		// the layers and the video are elements of their own, `Display`'s children
		covered_but::<Display>(&["Field", "Intensity", "Video"]);
		// the array is the `Array` child
		covered_but::<Source>(&["Array"]);
		covered_but::<SourceArray>(&[]);
		covered_but::<Probe>(&[]);
		covered_but::<PartBase>(&[]);
		covered_but::<Coating>(&[]);
		covered_but::<Material>(&[]);
		// the coating and the material are read by their own tables
		covered_but::<Lens>(SHAPE_PARTS);
		// its lens is read by the `Lens` table
		covered_but::<FresnelLens>(&["Lens"]);
		covered_but::<Polygon>(SHAPE_PARTS);
		covered_but::<Rectangle>(SHAPE_PARTS);
		covered_but::<Ellipse>(SHAPE_PARTS);
		covered_but::<Triangle>(SHAPE_PARTS);
		// the corners are `Point` children
		covered_but::<Outline>(&["Coating", "Material", "Points"]);
		covered_but::<Diffuser>(SHAPE_PARTS);
		// the layers are `Layer` children
		covered_but::<InterferenceFilter>(&["Coating", "Material", "Layers"]);
		covered_but::<FilterLayer>(&[]);
		covered_but::<SimpleGrating>(SHAPE_PARTS);
		// the design is read by its own table, chosen by the element name
		covered_but::<MetaSurface>(&["Coating", "Material", "Design"]);
		covered_but::<MetaLensDesign>(&[]);
		covered_but::<MetaPrismDesign>(&[]);
		covered_but::<MetaAxiconDesign>(&[]);
		covered_but::<FacettedGrating>(SHAPE_PARTS);
		// the nodes are `Node` children
		covered_but::<Fiber>(&["Coating", "Nodes"]);
		covered_but::<BraggReflector>(&[]);
		covered_but::<GrinLens>(&["Coating"]);
		// the kind is the element name (`XorGate`)
		covered_but::<LogicGate>(&["Kind"]);
		covered_but::<ZonePlate>(&[]);
		covered_but::<Point>(&[]);
		covered_but::<FiberNode>(&[]);
	}

	#[test]
	fn every_table_attribute_is_a_scalar_and_the_names_are_unique() {
		fn scalars<T: Table>() {
			let mut names = Vec::new();
			for a in T::ATTRS {
				let f = a.field;
				assert!(
					!matches!(f.kind(), ValueKind::List | ValueKind::Nested),
					"{}.{} is no scalar",
					T::TYPE_NAME,
					f.path()
				);
				assert!(!names.contains(&a.name()), "{} names {} twice", T::TYPE_NAME, a.name());
				names.push(a.name());
			}
		}
		each_table!(scalars);
	}
}
