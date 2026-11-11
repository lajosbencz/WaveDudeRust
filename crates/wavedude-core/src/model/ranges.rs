//! Bounds of the persisted numbers, as constants the model's fields declare (`#[field(range = ranges::X)]`).
//!
//! `normalize` clamps with these constants; the tests walk the fields and prove each declared range is
//! enforced at both ends. Bounds that depend on another value are marked `#[field(dependent)]` and are clamped
//! in code alone.

/// Inclusive bounds of a number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
	pub min: f64,
	pub max: f64,
}

/// The `.wdproj` integer ceiling (C# `int.MaxValue`), the upper bound of open-ended integers.
pub const INT_MAX: f64 = i32::MAX as f64;

impl Range {
	pub const fn new(min: f64, max: f64) -> Self {
		Self { min, max }
	}

	/// Clamps a float; non-finite becomes `fallback`.
	pub fn clamp_or(self, v: f64, fallback: f64) -> f64 {
		if v.is_finite() { v.clamp(self.min, self.max) } else { fallback }
	}

	pub fn clamp_i32(self, v: i32) -> i32 {
		(v as f64).clamp(self.min, self.max) as i32
	}

	pub fn clamp_u32(self, v: u32) -> u32 {
		(v as f64).clamp(self.min.max(0.0), self.max) as u32
	}

	pub fn contains(self, v: f64) -> bool {
		v >= self.min && v <= self.max
	}
}

pub const SCENE_SIZE: Range = Range::new(8.0, 16384.0);
pub const SCENE_DAMPING_EDGE: Range = Range::new(-8192.0, 8192.0);
pub const INDEX: Range = Range::new(1.0, 2.55);
pub const SCENE_IMAGE_SOURCE_SCALE: Range = Range::new(0.0, 1000.0);

pub const SIM_STEPS: Range = Range::new(1.0, 10_000_000.0);
pub const SIM_VISUALIZE_EVERY: Range = Range::new(1.0, 1000.0);
pub const SIM_WAVE_SPEED: Range = Range::new(1e-6, 1e6);
pub const SIM_TIME_DECAY: Range = Range::new(1.0, 100.0);
pub const SIM_SOURCE_FREQUENCY_SCALE: Range = Range::new(0.0, 1000.0);
pub const SIM_INTENSITY_SCALE: Range = Range::new(1e-7, 1e6);

pub const STEP_POINT: Range = Range::new(0.0, INT_MAX);
/// A source's position in cells; the canvas edge is not a bound, a source outside drives nothing.
pub const POSITION: Range = Range::new(-INT_MAX, INT_MAX);
pub const LAYER_BRIGHTNESS: Range = Range::new(1e-4, 1e5);
pub const LAYER_BLACK_LEVEL: Range = Range::new(-10.0, 1.0);
pub const LAYER_OVERLAY_GAIN: Range = Range::new(0.0, 5.0);
pub const VIDEO_FPS: Range = Range::new(1.0, 240.0);
pub const VIDEO_QUALITY: Range = Range::new(1.0, 100.0);

/// Frequency level of a source, a gate or a zone plate's design.
pub const LEVEL: Range = Range::new(0.01, 255.0);
pub const DESIGN_LEVEL: Range = Range::new(1.0, 255.0);
pub const PERCENT: Range = Range::new(0.0, 100.0);
pub const PHASE: Range = Range::new(-180.0, 180.0);
pub const AT_LEAST_0: Range = Range::new(0.0, INT_MAX);
pub const AT_LEAST_1: Range = Range::new(1.0, INT_MAX);
pub const AT_LEAST_2: Range = Range::new(2.0, INT_MAX);
/// `DetachAfter` in the file: -1 never, else a step.
pub const DETACH_AFTER: Range = Range::new(-1.0, INT_MAX);
/// Angles are wrapped into (-180, 180], not clamped; see [`WRAPPED`].
pub const ANGLE: Range = Range::new(-180.0, 180.0);

pub const PROBE_BOX_WIDTH: Range = Range::new(60.0, 4096.0);
pub const PROBE_BOX_HEIGHT: Range = Range::new(30.0, 4096.0);
pub const PROBE_DECADES: Range = Range::new(1.0, 12.0);
pub const PROBE_WINDOW: Range = Range::new(10.0, 1_000_000.0);
/// A detector's expected detections per unit of intensity per step.
pub const PROBE_SENSITIVITY: Range = Range::new(0.0, 1_000_000.0);
/// An imaging detector's bin in pixels along the line.
pub const PROBE_BIN_SIZE: Range = Range::new(1.0, 20.0);

pub const COATING: Range = Range::new(0.0, 50.0);
pub const ABSORPTION: Range = Range::new(0.0, 255.0);
pub const ABSORPTION_RAMP: Range = Range::new(1.0, 400.0);
pub const POLYGON_SIDES: Range = Range::new(3.0, 64.0);
pub const TRIANGLE_ANGLE: Range = Range::new(1.0, 178.0);
pub const BRAGG_PAIRS: Range = Range::new(1.0, 200.0);
pub const GATE_PORT_SPACING: Range = Range::new(2.0, 2000.0);
pub const ZONE_PLATE_COATING: Range = Range::new(0.0, 400.0);
pub const FRESNEL_ZONES: Range = Range::new(1.0, 100.0);
/// A ripple's or a relief's wavelength on a profiled plate: two pixels at least, what a profile sampled a pixel
/// apart can hold.
pub const RIPPLE_WAVELENGTH: Range = Range::new(2.0, 10_000.0);
/// A profiled plate's amplitude or sigma in pixels.
pub const PLATE_PIXELS: Range = Range::new(0.0, 10_000.0);
/// A meta surface's pitch or height in pixels.
pub const META_PIXELS: Range = Range::new(1.0, 10_000.0);
/// A meta prism's deflection in degrees: short of grazing.
pub const META_DEFLECTION: Range = Range::new(-89.0, 89.0);
/// A meta axicon's cone angle in degrees: short of no cone and of grazing.
pub const META_CONE_ANGLE: Range = Range::new(0.1, 89.0);
/// A simple grating's column or row spacing: room for a dot of a pixel.
pub const GRATING_SPACING: Range = Range::new(2.0, INT_MAX);
/// An interference filter layer's thickness in pixels; 0 leaves it out.
pub const LAYER_THICKNESS: Range = Range::new(0.0, 1000.0);
pub const STOP_AT: Range = Range::new(0.0, 1.0);

/// Fields (`<TYPE_NAME>.<name>`) that are wrapped into (-180, 180] rather than clamped.
pub const WRAPPED: &[&str] = &["SourceArray.Angle", "Part.Angle"];

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::normalize::Normalize;
	use crate::model::*;

	type Set = Box<dyn Fn(&mut Project, f64)>;
	type Get = Box<dyn Fn(&Project) -> f64>;

	/// One of every part kind, in this order; the triangle is Custom with `AngleA` 1 so `AngleB` may reach 178.
	fn fixture() -> Project {
		let mut p = Project::default();
		p.sources.push(Source { name: "S".into(), array: Some(SourceArray::default()), ..Source::default() });
		p.probes.push(Probe { name: "P".into(), ..Probe::default() });
		p.color_schemes.push(ColorScheme { name: "C".into(), ..ColorScheme::grey() });
		let tri = Triangle { kind: TriangleType::Custom, angle_a: 1.0, ..Triangle::default() };
		let kinds = [
			PartKind::Lens(Lens::default()),
			PartKind::Polygon(Polygon::default()),
			PartKind::Rectangle(Rectangle::default()),
			PartKind::Ellipse(Ellipse::default()),
			PartKind::Triangle(tri),
			PartKind::Outline(Outline::default()),
			PartKind::Fiber(Fiber::default()),
			PartKind::BraggReflector(BraggReflector::default()),
			PartKind::GrinLens(GrinLens::default()),
			PartKind::LogicGate(LogicGate::default()),
			PartKind::ZonePlate(ZonePlate::default()),
			PartKind::FresnelLens(FresnelLens::default()),
			PartKind::Diffuser(Diffuser::default()),
			PartKind::FacettedGrating(FacettedGrating::default()),
			PartKind::InterferenceFilter(InterferenceFilter::new_part()),
			PartKind::SimpleGrating(SimpleGrating::default()),
			PartKind::MetaSurface(MetaSurface::default()),
			PartKind::MetaSurface(MetaSurface {
				design: MetaDesign::Prism(MetaPrismDesign::default()),
				..Default::default()
			}),
			PartKind::MetaSurface(MetaSurface {
				design: MetaDesign::Axicon(MetaAxiconDesign::default()),
				..Default::default()
			}),
		];
		for (i, k) in kinds.into_iter().enumerate() {
			let mut part = Part::new(k, 10, 10);
			part.base.name = format!("part {i}");
			p.parts.push(part);
		}
		p
	}

	macro_rules! field {
		($key:expr, |$p:ident| $place:expr) => {
			(
				$key,
				Box::new(|$p: &mut Project, v: f64| $place = num(v)) as Set,
				Box::new(|$p: &Project| $place as f64) as Get,
			)
		};
	}

	macro_rules! array_field {
		($key:expr, $f:ident) => {
			(
				$key,
				Box::new(|p: &mut Project, v: f64| p.sources[0].array.as_mut().unwrap().$f = num(v)) as Set,
				Box::new(|p: &Project| p.sources[0].array.as_ref().unwrap().$f as f64) as Get,
			)
		};
	}

	/// A field of the meta surface design of part `$i`.
	macro_rules! design {
		($key:expr, $i:expr, $variant:ident, |$d:ident| $place:expr) => {
			(
				$key,
				Box::new(|p: &mut Project, v: f64| {
					if let PartKind::MetaSurface(m) = &mut p.parts[$i].kind
						&& let MetaDesign::$variant($d) = &mut m.design
					{
						$place = num(v);
					}
				}) as Set,
				Box::new(|p: &Project| match &p.parts[$i].kind {
					PartKind::MetaSurface(m) => match &m.design {
						MetaDesign::$variant($d) => $place as f64,
						_ => unreachable!(),
					},
					_ => unreachable!(),
				}) as Get,
			)
		};
	}

	macro_rules! kind {
		($key:expr, $i:expr, $variant:ident, |$k:ident| $place:expr) => {
			(
				$key,
				Box::new(|p: &mut Project, v: f64| {
					if let PartKind::$variant($k) = &mut p.parts[$i].kind {
						$place = num(v);
					}
				}) as Set,
				Box::new(|p: &Project| match &p.parts[$i].kind {
					PartKind::$variant($k) => $place as f64,
					_ => unreachable!(),
				}) as Get,
			)
		};
	}

	/// Converts with saturation into the field's type.
	fn num<T: FromF64>(v: f64) -> T {
		T::from_f64(v)
	}

	trait FromF64 {
		fn from_f64(v: f64) -> Self;
	}
	impl FromF64 for f64 {
		fn from_f64(v: f64) -> Self {
			v
		}
	}
	impl FromF64 for u32 {
		fn from_f64(v: f64) -> Self {
			v as u32
		}
	}
	impl FromF64 for i32 {
		fn from_f64(v: f64) -> Self {
			v as i32
		}
	}

	fn material(p: &mut Project, i: usize) -> &mut Material {
		match &mut p.parts[i].kind {
			PartKind::Lens(k) => &mut k.material,
			PartKind::FresnelLens(k) => &mut k.lens.material,
			PartKind::Diffuser(k) => &mut k.material,
			PartKind::InterferenceFilter(k) => &mut k.material,
			PartKind::SimpleGrating(k) => &mut k.material,
			PartKind::MetaSurface(k) => &mut k.material,
			PartKind::FacettedGrating(k) => &mut k.material,
			PartKind::Polygon(k) => &mut k.material,
			PartKind::Rectangle(k) => &mut k.material,
			PartKind::Ellipse(k) => &mut k.material,
			PartKind::Triangle(k) => &mut k.material,
			PartKind::Outline(k) => &mut k.material,
			_ => unreachable!(),
		}
	}

	fn coating(p: &mut Project, i: usize) -> &mut Coating {
		match &mut p.parts[i].kind {
			PartKind::Lens(k) => &mut k.coating,
			PartKind::FresnelLens(k) => &mut k.lens.coating,
			PartKind::Diffuser(k) => &mut k.coating,
			PartKind::InterferenceFilter(k) => &mut k.coating,
			PartKind::FacettedGrating(k) => &mut k.coating,
			PartKind::MetaSurface(k) => &mut k.coating,
			PartKind::Polygon(k) => &mut k.coating,
			PartKind::Rectangle(k) => &mut k.coating,
			PartKind::Ellipse(k) => &mut k.coating,
			PartKind::Triangle(k) => &mut k.coating,
			PartKind::Outline(k) => &mut k.coating,
			PartKind::Fiber(k) => &mut k.coating,
			PartKind::GrinLens(k) => &mut k.coating,
			_ => unreachable!(),
		}
	}

	fn accessors() -> Vec<(&'static str, Set, Get)> {
		let mut v: Vec<(&'static str, Set, Get)> = vec![
			field!("Scene.Width", |p| p.scene.width),
			field!("Scene.Height", |p| p.scene.height),
			field!("Scene.DampingEdge", |p| p.scene.damping_edge),
			field!("Scene.RefractiveIndex", |p| p.scene.refractive_index),
			field!("Scene.ImageSourceScale", |p| p.scene.image_source_scale),
			field!("Simulation.Steps", |p| p.simulation.steps),
			field!("Simulation.VisualizeEvery", |p| p.simulation.visualize_every),
			field!("Simulation.WaveSpeed", |p| p.simulation.wave_speed),
			field!("Simulation.TimeDecay", |p| p.simulation.time_decay),
			field!("Simulation.SourceFrequencyScale", |p| p.simulation.source_frequency_scale),
			field!("Simulation.IntensityScale", |p| p.simulation.intensity_scale),
			field!("Display.ExportFrom", |p| p.display.export_from),
			field!("Display.ExportTo", |p| p.display.export_to),
			field!("DisplayLayer.Brightness", |p| p.display.intensity.brightness),
			field!("DisplayLayer.BlackLevel", |p| p.display.field.black_level),
			field!("DisplayLayer.OverlayGain", |p| p.display.field.overlay_gain),
			field!("Video.Fps", |p| p.display.video.fps),
			field!("Video.Quality", |p| p.display.video.quality),
			field!("Source.Frequency", |p| p.sources[0].frequency),
			field!("Source.Amplitude", |p| p.sources[0].amplitude),
			field!("Source.Phase", |p| p.sources[0].phase),
			field!("Source.PulseWidth", |p| p.sources[0].pulse_width),
			field!("Source.OffWidth", |p| p.sources[0].off_width),
			(
				"Source.DetachAfter",
				Box::new(|p: &mut Project, v: f64| p.sources[0].detach_after = (v >= 0.0).then_some(v as u32)),
				Box::new(|p: &Project| p.sources[0].detach_after.map_or(-1.0, |s| s as f64)),
			),
			array_field!("SourceArray.SizeX", size_x),
			array_field!("SourceArray.SizeY", size_y),
			array_field!("SourceArray.Angle", angle),
			array_field!("SourceArray.RandomFrequency", random_frequency),
			array_field!("SourceArray.RandomPhase", random_phase),
			array_field!("SourceArray.RandomOffWidth", random_off_width),
			array_field!("SourceArray.RandomPosition", random_position),
			field!("Probe.BoxWidth", |p| p.probes[0].box_width),
			field!("Probe.BoxHeight", |p| p.probes[0].box_height),
			field!("Probe.Decades", |p| p.probes[0].decades),
			field!("Probe.Window", |p| p.probes[0].window),
			field!("Probe.StartAfter", |p| p.probes[0].start_after),
			field!("Probe.Sensitivity", |p| p.probes[0].sensitivity),
			field!("Probe.BinSize", |p| p.probes[0].bin_size),
			field!("Part.X", |p| p.parts[3].base.x),
			field!("Part.Y", |p| p.parts[7].base.y),
			field!("Part.Angle", |p| p.parts[9].base.angle),
			kind!("Lens.Diameter", 0, Lens, |k| k.diameter),
			kind!("Lens.Thickness", 0, Lens, |k| k.thickness),
			kind!("Polygon.Sides", 1, Polygon, |k| k.sides),
			kind!("Polygon.Side", 1, Polygon, |k| k.side),
			kind!("Rectangle.Length", 2, Rectangle, |k| k.length),
			kind!("Rectangle.Width", 2, Rectangle, |k| k.width),
			kind!("Ellipse.Length", 3, Ellipse, |k| k.length),
			kind!("Ellipse.Width", 3, Ellipse, |k| k.width),
			kind!("Triangle.Base", 4, Triangle, |k| k.base),
			kind!("Triangle.AngleA", 4, Triangle, |k| k.angle_a),
			kind!("Triangle.AngleB", 4, Triangle, |k| k.angle_b),
			kind!("Fiber.CoreIndex", 6, Fiber, |k| k.core_index),
			kind!("Fiber.CoreDiameter", 6, Fiber, |k| k.core_diameter),
			kind!("Fiber.CladdingIndex", 6, Fiber, |k| k.cladding_index),
			kind!("Fiber.CladdingThickness", 6, Fiber, |k| k.cladding_thickness),
			kind!("Fiber.Absorption", 6, Fiber, |k| k.absorption),
			kind!("Fiber.AbsorptionRamp", 6, Fiber, |k| k.absorption_ramp),
			kind!("BraggReflector.Pairs", 7, BraggReflector, |k| k.pairs),
			kind!("BraggReflector.Thickness1", 7, BraggReflector, |k| k.thickness1),
			kind!("BraggReflector.Thickness2", 7, BraggReflector, |k| k.thickness2),
			kind!("BraggReflector.Index1", 7, BraggReflector, |k| k.index1),
			kind!("BraggReflector.Index2", 7, BraggReflector, |k| k.index2),
			kind!("BraggReflector.Width", 7, BraggReflector, |k| k.width),
			kind!("GrinLens.Length", 8, GrinLens, |k| k.length),
			kind!("GrinLens.Diameter", 8, GrinLens, |k| k.diameter),
			kind!("GrinLens.CentreIndex", 8, GrinLens, |k| k.centre_index),
			kind!("GrinLens.EdgeIndex", 8, GrinLens, |k| k.edge_index),
			kind!("LogicGate.DesignFrequency", 9, LogicGate, |k| k.design_frequency),
			kind!("LogicGate.Coating", 9, LogicGate, |k| k.coating),
			kind!("LogicGate.CoreIndex", 9, LogicGate, |k| k.core_index),
			kind!("LogicGate.CladdingIndex", 9, LogicGate, |k| k.cladding_index),
			kind!("LogicGate.PortSpacing", 9, LogicGate, |k| k.port_spacing),
			kind!("ZonePlate.MountWidth", 10, ZonePlate, |k| k.mount_width),
			kind!("ZonePlate.Diameter", 10, ZonePlate, |k| k.diameter),
			kind!("ZonePlate.Thickness", 10, ZonePlate, |k| k.thickness),
			kind!("ZonePlate.Coating", 10, ZonePlate, |k| k.coating),
			kind!("ZonePlate.DesignFrequency", 10, ZonePlate, |k| k.design_frequency),
			kind!("ZonePlate.FocalLength", 10, ZonePlate, |k| k.focal_length),
			kind!("FresnelLens.Zones", 11, FresnelLens, |k| k.zones),
			kind!("Diffuser.Length", 12, Diffuser, |k| k.length),
			kind!("Diffuser.Width", 12, Diffuser, |k| k.width),
			kind!("Diffuser.CenterWavelength", 12, Diffuser, |k| k.center_wavelength),
			kind!("Diffuser.Amplitude", 12, Diffuser, |k| k.amplitude),
			kind!("Diffuser.Sigma", 12, Diffuser, |k| k.sigma),
			kind!("InterferenceFilter.Length", 14, InterferenceFilter, |k| k.length),
			kind!("InterferenceFilter.Width", 14, InterferenceFilter, |k| k.width),
			kind!("FilterLayer.Thickness", 14, InterferenceFilter, |k| k.layers[0].thickness),
			kind!("FilterLayer.RefractiveIndex", 14, InterferenceFilter, |k| k.layers[0].refractive_index),
			kind!("SimpleGrating.Columns", 15, SimpleGrating, |k| k.columns),
			kind!("SimpleGrating.Rows", 15, SimpleGrating, |k| k.rows),
			kind!("SimpleGrating.ColumnSpacing", 15, SimpleGrating, |k| k.column_spacing),
			kind!("SimpleGrating.RowSpacing", 15, SimpleGrating, |k| k.row_spacing),
			kind!("MetaSurface.Length", 16, MetaSurface, |k| k.length),
			kind!("MetaSurface.Width", 16, MetaSurface, |k| k.width),
			kind!("MetaSurface.DesignFrequency", 16, MetaSurface, |k| k.design_frequency),
			kind!("MetaSurface.Pitch", 16, MetaSurface, |k| k.pitch),
			kind!("MetaSurface.Height", 16, MetaSurface, |k| k.height),
			kind!("MetaSurface.PatternIndex", 16, MetaSurface, |k| k.pattern_index),
			design!("MetaLens.FocalLength", 16, Lens, |d| d.focal_length),
			design!("MetaPrism.Deflection", 17, Prism, |d| d.deflection),
			design!("MetaAxicon.ConeAngle", 18, Axicon, |d| d.cone_angle),
			kind!("FacettedGrating.Length", 13, FacettedGrating, |k| k.length),
			kind!("FacettedGrating.Width", 13, FacettedGrating, |k| k.width),
			kind!("FacettedGrating.Pitch", 13, FacettedGrating, |k| k.pitch),
			kind!("FacettedGrating.Amplitude", 13, FacettedGrating, |k| k.amplitude),
			field!("Stop.At", |p| p.color_schemes[0].stops[0].at),
		];
		for i in (0..6).chain([11, 12, 13, 14, 15, 16]) {
			v.push((
				"Material.RefractiveIndex",
				Box::new(move |p: &mut Project, x: f64| material(p, i).refractive_index = x),
				Box::new(move |p: &Project| material(&mut p.clone(), i).refractive_index),
			));
			v.push((
				"Material.Absorption",
				Box::new(move |p: &mut Project, x: f64| material(p, i).absorption = num(x)),
				Box::new(move |p: &Project| material(&mut p.clone(), i).absorption as f64),
			));
			v.push((
				"Material.AbsorptionRamp",
				Box::new(move |p: &mut Project, x: f64| material(p, i).absorption_ramp = num(x)),
				Box::new(move |p: &Project| material(&mut p.clone(), i).absorption_ramp as f64),
			));
		}
		for i in [0, 1, 2, 3, 4, 5, 6, 8, 11, 12, 13, 14, 16] {
			v.push((
				"Coating.Depth",
				Box::new(move |p: &mut Project, x: f64| coating(p, i).depth = x),
				Box::new(move |p: &Project| coating(&mut p.clone(), i).depth),
			));
		}
		v
	}

	/// Persisted numbers whose bounds depend on another value, with the rule `normalize` applies.
	const DEPENDENT: &[(&str, &str)] = &[
		("Display.ExportTo", "0 = to the end; below ExportFrom it becomes 0"),
		("Probe.YMin", "YMax <= YMin resets both to -1, 1"),
		("Probe.YMax", "YMax <= YMin resets both to -1, 1"),
		("Lens.R1", "fit to diameter and thickness (fit_radii)"),
		("Lens.R2", "fit to diameter and thickness (fit_radii)"),
		("Triangle.AngleA", "the type's preset; Isosceles at most 89"),
		("Triangle.AngleB", "the type's preset; Custom at most 179 - AngleA"),
		("Rectangle.Width", "equals Length while Linked"),
		("Ellipse.Width", "equals Length while Linked"),
		("LogicGate.PortSpacing", "raised to the design's minimum when designed"),
		("SimpleGrating.Diameter", "1 to half the smaller spacing"),
	];

	fn is_number(kind: ValueKind) -> bool {
		matches!(kind, ValueKind::Int | ValueKind::UInt | ValueKind::Float)
	}

	/// The number fields the types declare, keyed `<TYPE_NAME>.<name>`.
	#[derive(Default)]
	struct Walk {
		ranged: Vec<(String, Range)>,
		dependent: Vec<String>,
	}

	/// Records the number fields `T` declares itself (a nested field's are `U`'s).
	fn walk<T: Fields>(w: &mut Walk) {
		for field in T::Field::all().into_iter().filter(|f| !f.path().contains('.') && is_number(f.kind())) {
			let key = format!("{}.{}", T::TYPE_NAME, field.name());
			if field.dependent() {
				w.dependent.push(key.clone());
			}
			match field.bounds() {
				Bounds::Range(r) => w.ranged.push((key, r)),
				Bounds::Dependent => assert!(field.dependent(), "{key}: dependent bounds without the mark"),
				Bounds::Open => assert!(!field.dependent(), "{key} is dependent but open"),
			}
		}
	}

	fn walked() -> Walk {
		let mut w = Walk::default();
		walk::<Project>(&mut w);
		walk::<Scene>(&mut w);
		walk::<Simulation>(&mut w);
		walk::<Display>(&mut w);
		walk::<DisplayLayer>(&mut w);
		walk::<Video>(&mut w);
		walk::<Source>(&mut w);
		walk::<SourceArray>(&mut w);
		walk::<Probe>(&mut w);
		walk::<ColorScheme>(&mut w);
		walk::<Stop>(&mut w);
		walk::<Rgb>(&mut w);
		walk::<Point>(&mut w);
		walk::<PartBase>(&mut w);
		walk::<Coating>(&mut w);
		walk::<Material>(&mut w);
		walk::<Lens>(&mut w);
		walk::<FresnelLens>(&mut w);
		walk::<Polygon>(&mut w);
		walk::<Rectangle>(&mut w);
		walk::<Ellipse>(&mut w);
		walk::<Triangle>(&mut w);
		walk::<Outline>(&mut w);
		walk::<Diffuser>(&mut w);
		walk::<FilterLayer>(&mut w);
		walk::<InterferenceFilter>(&mut w);
		walk::<SimpleGrating>(&mut w);
		walk::<MetaSurface>(&mut w);
		walk::<MetaLensDesign>(&mut w);
		walk::<MetaPrismDesign>(&mut w);
		walk::<MetaAxiconDesign>(&mut w);
		walk::<FacettedGrating>(&mut w);
		walk::<FiberNode>(&mut w);
		walk::<Fiber>(&mut w);
		walk::<BraggReflector>(&mut w);
		walk::<GrinLens>(&mut w);
		walk::<LogicGate>(&mut w);
		walk::<ZonePlate>(&mut w);
		w
	}

	#[test]
	fn every_bound_is_enforced_by_normalize() {
		let accessors = accessors();
		let walk = walked();
		for (key, _) in &walk.ranged {
			assert!(accessors.iter().any(|(k, ..)| k == key), "no accessor for {key}");
		}
		for (key, set, get) in &accessors {
			let (_, range) =
				walk.ranged.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("{key} has no declared range"));
			let range = *range;
			if WRAPPED.contains(key) {
				let mut p = fixture();
				set(&mut p, 190.0);
				p.normalize();
				assert_eq!(get(&p), -170.0, "{key}");
				continue;
			}
			let delta = range.min.abs().max(range.max.abs()).max(1.0) * 0.5;
			for (value, want) in [(range.min - delta, range.min), (range.max + delta, range.max)] {
				let mut p = fixture();
				set(&mut p, value);
				p.normalize();
				assert_eq!(get(&p), want, "{key} set to {value}");
			}
			for value in [range.min, range.max] {
				let mut p = fixture();
				set(&mut p, value);
				p.normalize();
				assert_eq!(get(&p), value, "{key} at its bound {value}");
			}
		}
	}

	#[test]
	fn dependent_fields_are_the_listed_ones() {
		let walk = walked();
		let mut listed: Vec<&str> = DEPENDENT.iter().map(|(k, _)| *k).collect();
		let mut marked: Vec<&str> = walk.dependent.iter().map(String::as_str).collect();
		listed.sort_unstable();
		marked.sort_unstable();
		assert_eq!(marked, listed);
		assert!(DEPENDENT.iter().all(|(_, rule)| !rule.is_empty()));
	}

	#[test]
	fn wrapped_fields_declare_a_range() {
		let walk = walked();
		for key in WRAPPED {
			assert!(walk.ranged.iter().any(|(k, _)| k == key), "{key}");
		}
	}
}
