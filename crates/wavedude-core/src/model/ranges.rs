//! Bounds of every persisted number, one table, named by `"<Type>.<Property>"`.
//!
//! `normalize` clamps with these constants; the table lets tests prove each one is
//! enforced at both ends. Bounds that depend on another value are listed in
//! [`DEPENDENT`] instead and are clamped in code alone.

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

pub const COATING: Range = Range::new(0.0, 50.0);
pub const ABSORPTION: Range = Range::new(0.0, 255.0);
pub const ABSORPTION_RAMP: Range = Range::new(1.0, 400.0);
pub const POLYGON_SIDES: Range = Range::new(3.0, 64.0);
pub const TRIANGLE_ANGLE: Range = Range::new(1.0, 178.0);
pub const BRAGG_PAIRS: Range = Range::new(1.0, 200.0);
pub const GATE_PORT_SPACING: Range = Range::new(2.0, 2000.0);
pub const ZONE_PLATE_COATING: Range = Range::new(0.0, 400.0);
pub const FRESNEL_ZONES: Range = Range::new(1.0, 100.0);
pub const STOP_AT: Range = Range::new(0.0, 1.0);

/// Every persisted number and its bounds. `Material.*` applies to the lens and every shape,
/// `Coating.Coating` to every coated part, `Part.*` to every part.
pub const ALL: &[(&str, Range)] = &[
	("Scene.Width", SCENE_SIZE),
	("Scene.Height", SCENE_SIZE),
	("Scene.DampingEdge", SCENE_DAMPING_EDGE),
	("Scene.RefractiveIndex", INDEX),
	("Scene.ImageSourceScale", SCENE_IMAGE_SOURCE_SCALE),
	("Simulation.Steps", SIM_STEPS),
	("Simulation.VisualizeEvery", SIM_VISUALIZE_EVERY),
	("Simulation.WaveSpeed", SIM_WAVE_SPEED),
	("Simulation.TimeDecay", SIM_TIME_DECAY),
	("Simulation.SourceFrequencyScale", SIM_SOURCE_FREQUENCY_SCALE),
	("Simulation.IntensityScale", SIM_INTENSITY_SCALE),
	("Display.ExportFrom", STEP_POINT),
	("Display.ExportTo", STEP_POINT),
	("DisplayLayer.Brightness", LAYER_BRIGHTNESS),
	("DisplayLayer.BlackLevel", LAYER_BLACK_LEVEL),
	("DisplayLayer.OverlayGain", LAYER_OVERLAY_GAIN),
	("Video.Fps", VIDEO_FPS),
	("Video.Quality", VIDEO_QUALITY),
	("Source.Frequency", LEVEL),
	("Source.Amplitude", PERCENT),
	("Source.Phase", PHASE),
	("Source.PulseWidth", AT_LEAST_1),
	("Source.OffWidth", AT_LEAST_1),
	("Source.DetachAfter", DETACH_AFTER),
	("SourceArray.SizeX", AT_LEAST_1),
	("SourceArray.SizeY", AT_LEAST_1),
	("SourceArray.Angle", ANGLE),
	("SourceArray.RandomFrequency", AT_LEAST_0),
	("SourceArray.RandomPhase", AT_LEAST_0),
	("SourceArray.RandomOffWidth", AT_LEAST_0),
	("SourceArray.RandomPosition", AT_LEAST_0),
	("Probe.BoxWidth", PROBE_BOX_WIDTH),
	("Probe.BoxHeight", PROBE_BOX_HEIGHT),
	("Probe.Decades", PROBE_DECADES),
	("Probe.Window", PROBE_WINDOW),
	("Probe.StartAfter", AT_LEAST_0),
	("Part.X", AT_LEAST_0),
	("Part.Y", AT_LEAST_0),
	("Part.Angle", ANGLE),
	("Coating.Coating", COATING),
	("Material.RefractiveIndex", INDEX),
	("Material.Absorption", ABSORPTION),
	("Material.AbsorptionRamp", ABSORPTION_RAMP),
	("Lens.Diameter", AT_LEAST_2),
	("Lens.Thickness", AT_LEAST_1),
	("FresnelLens.Zones", FRESNEL_ZONES),
	("Polygon.Sides", POLYGON_SIDES),
	("Polygon.Side", AT_LEAST_2),
	("Rectangle.Length", AT_LEAST_1),
	("Rectangle.Width", AT_LEAST_1),
	("Ellipse.Length", AT_LEAST_1),
	("Ellipse.Width", AT_LEAST_1),
	("Triangle.Base", AT_LEAST_2),
	("Triangle.AngleA", TRIANGLE_ANGLE),
	("Triangle.AngleB", TRIANGLE_ANGLE),
	("Fiber.CoreIndex", INDEX),
	("Fiber.CoreDiameter", AT_LEAST_2),
	("Fiber.CladdingIndex", INDEX),
	("Fiber.CladdingThickness", AT_LEAST_0),
	("Fiber.Absorption", ABSORPTION),
	("Fiber.AbsorptionRamp", ABSORPTION_RAMP),
	("BraggReflector.Pairs", BRAGG_PAIRS),
	("BraggReflector.Thickness1", AT_LEAST_1),
	("BraggReflector.Thickness2", AT_LEAST_1),
	("BraggReflector.Index1", INDEX),
	("BraggReflector.Index2", INDEX),
	("BraggReflector.Width", AT_LEAST_2),
	("GrinLens.Length", AT_LEAST_1),
	("GrinLens.Diameter", AT_LEAST_2),
	("GrinLens.CentreIndex", INDEX),
	("GrinLens.EdgeIndex", INDEX),
	("LogicGate.DesignFrequency", DESIGN_LEVEL),
	("LogicGate.Coating", COATING),
	("LogicGate.CoreIndex", INDEX),
	("LogicGate.CladdingIndex", INDEX),
	("LogicGate.PortSpacing", GATE_PORT_SPACING),
	("ZonePlate.MountWidth", AT_LEAST_1),
	("ZonePlate.Diameter", AT_LEAST_2),
	("ZonePlate.Thickness", AT_LEAST_1),
	("ZonePlate.Coating", ZONE_PLATE_COATING),
	("ZonePlate.DesignFrequency", DESIGN_LEVEL),
	("ZonePlate.FocalLength", AT_LEAST_1),
	("Stop.At", STOP_AT),
];

/// Keys of [`ALL`] that are wrapped into (-180, 180] rather than clamped.
pub const WRAPPED: &[&str] = &["SourceArray.Angle", "Part.Angle"];

/// Persisted numbers whose bounds depend on another value, with the rule `normalize` applies.
pub const DEPENDENT: &[(&str, &str)] = &[
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
];

/// Persisted numbers without bounds (positions, spacings, offsets).
pub const UNBOUNDED: &[&str] = &[
	"Project.Seed",
	"Source.X",
	"Source.Y",
	"SourceArray.SpacingX",
	"SourceArray.SpacingY",
	"Probe.AX",
	"Probe.AY",
	"Probe.BX",
	"Probe.BY",
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

/// The bounds of a key of [`ALL`].
pub fn range(key: &str) -> Option<Range> {
	ALL.iter().find(|(k, _)| *k == key).map(|(_, r)| *r)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::normalize::Normalize;
	use crate::model::*;

	type Set = Box<dyn Fn(&mut Project, f64)>;
	type Get = Box<dyn Fn(&Project) -> f64>;

	/// One of every part kind, in this order; the triangle is Custom with AngleA 1 so AngleB may reach 178.
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
			field!("Stop.At", |p| p.color_schemes[0].stops[0].at),
		];
		for i in (0..6).chain([11]) {
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
		for i in [0, 1, 2, 3, 4, 5, 6, 8, 11] {
			v.push((
				"Coating.Coating",
				Box::new(move |p: &mut Project, x: f64| coating(p, i).depth = x),
				Box::new(move |p: &Project| coating(&mut p.clone(), i).depth),
			));
		}
		v
	}

	#[test]
	fn every_bound_is_enforced_by_normalize() {
		let accessors = accessors();
		for (key, _) in ALL {
			assert!(accessors.iter().any(|(k, ..)| k == key), "no accessor for {key}");
		}
		for (key, set, get) in &accessors {
			let range = range(key).unwrap_or_else(|| panic!("{key} has no declared range"));
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
	fn dependent_and_unbounded_are_named_once() {
		for (key, _) in DEPENDENT {
			assert!(!UNBOUNDED.contains(key), "{key}");
		}
		for key in UNBOUNDED {
			assert!(range(key).is_none(), "{key}");
		}
	}
}
