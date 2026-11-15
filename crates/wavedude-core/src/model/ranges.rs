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

/// A source array's columns or rows: up to the largest canvas's side.
pub const ARRAY_SIZE: Range = Range::new(1.0, SCENE_SIZE.max);
/// A source array's most elements, columns times rows; rows give way to columns.
pub const ARRAY_ELEMENTS: u32 = 1 << 16;

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
/// Angles are wrapped into (-180, 180], not clamped.
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

#[cfg(test)]
mod tests {
	use std::any::TypeId;
	use std::collections::HashSet;

	use super::*;
	use crate::model::normalize::Normalize;
	use crate::model::*;

	/// Fields (`<TYPE_NAME>.<name>`) that `normalize` wraps into (-180, 180] rather than clamps.
	const WRAPPED: &[&str] = &["SourceArray.Angle", "Part.Angle"];

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

	/// A ranged number of the fixture: its path from a root, its `<TYPE_NAME>.<name>`, its range, and the value it
	/// holds after being set to a value and normalised.
	struct Ranged {
		path: String,
		key: String,
		range: Range,
		round_trip: Box<dyn Fn(f64) -> f64>,
	}

	/// The ranged numbers below the root `name`, which `at` reaches in the fixture.
	fn below<R: Fields>(name: &str, at: impl Fn(&mut Project) -> &mut R + Copy + 'static) -> Vec<Ranged> {
		let mut p = fixture();
		let root: &R = at(&mut p);
		R::Field::all()
			.into_iter()
			.filter(|f| is_number(f.kind()) && f.get(root).is_some())
			.filter_map(|field| {
				let range = field.range()?;
				let (type_name, leaf) = field.declared();
				let round_trip = Box::new(move |v: f64| {
					let mut p = fixture();
					edit(field, v).apply(at(&mut p)).unwrap();
					p.normalize();
					number(field.get(at(&mut p)).unwrap(), range)
				});
				Some(Ranged {
					path: format!("{name}.{}", field.path()),
					key: format!("{type_name}.{leaf}"),
					range,
					round_trip,
				})
			})
			.collect()
	}

	/// The ranged numbers below the project and every list element of the fixture.
	fn ranged() -> Vec<Ranged> {
		let mut ranged = below("Project", |p| p);
		ranged.extend(below("Source", |p| &mut p.sources[0]));
		ranged.extend(below("Probe", |p| &mut p.probes[0]));
		ranged.extend(below("ColorScheme", |p| &mut p.color_schemes[0]));
		ranged.extend(below("Stop", |p| &mut p.color_schemes[0].stops[0]));
		for i in 0..fixture().parts.len() {
			ranged.extend(below(&format!("Part {i}"), move |p| &mut p.parts[i]));
		}
		ranged.extend(below("FilterLayer", |p| {
			let mut layers = p.parts.iter_mut().filter_map(|part| match &mut part.kind {
				PartKind::InterferenceFilter(f) => f.layers.first_mut(),
				_ => None,
			});
			layers.next().expect("the fixture has a filter layer")
		}));
		ranged
	}

	/// The edit setting `field` to `v`, saturated into its type; a negative value empties an optional unsigned
	/// field (`DetachAfter`, whose -1 is never).
	fn edit<F: Field>(field: F, v: f64) -> <F::Owner as Fields>::Edit {
		match field.kind() {
			ValueKind::UInt if v < 0.0 => field.edit(Value::Absent).or_else(|_| field.edit(Value::Int(0))),
			ValueKind::UInt => field.edit(Value::Int(i64::from(v as u32))),
			ValueKind::Int => field.edit(Value::Int(i64::from(v as i32))),
			_ => field.edit(Value::Float(v)),
		}
		.unwrap()
	}

	/// A number's value; an absent optional is its range's minimum.
	fn number(value: Value, range: Range) -> f64 {
		match value {
			Value::Int(i) => i as f64,
			Value::Float(x) => x,
			Value::Absent => range.min,
			other => panic!("{other:?} is no number"),
		}
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
		("SourceArray.SizeY", "at most ARRAY_ELEMENTS / SizeX"),
	];

	fn is_number(kind: ValueKind) -> bool {
		matches!(kind, ValueKind::Int | ValueKind::UInt | ValueKind::Float)
	}

	/// The number fields the types declare, keyed `<TYPE_NAME>.<name>`.
	#[derive(Default)]
	struct Walk {
		seen: HashSet<TypeId>,
		ranged: Vec<(String, Range)>,
		dependent: Vec<String>,
	}

	impl Visitor for Walk {
		/// Records the number fields `T` declares itself (a nested field's are `U`'s).
		fn fields<T: Fields>(&mut self) {
			if !self.seen.insert(TypeId::of::<T>()) {
				return;
			}
			for field in T::Field::all().into_iter().filter(|f| !f.path().contains('.') && is_number(f.kind())) {
				let key = format!("{}.{}", T::TYPE_NAME, field.name());
				if field.dependent() {
					self.dependent.push(key.clone());
				}
				if let Some(range) = field.range() {
					self.ranged.push((key, range));
				}
			}
		}

		fn word<W: Word>(&mut self) {}
	}

	fn walked() -> Walk {
		let mut w = Walk::default();
		Project::visit(&mut w);
		w
	}

	#[test]
	fn every_bound_is_enforced_by_normalize() {
		let ranged = ranged();
		for (key, _) in &walked().ranged {
			assert!(ranged.iter().any(|r| &r.key == key), "the fixture has no {key}");
		}
		for Ranged { path, key, range, round_trip } in &ranged {
			let range = *range;
			if WRAPPED.contains(&key.as_str()) {
				assert_eq!(round_trip(190.0), -170.0, "{path}");
				continue;
			}
			let delta = range.min.abs().max(range.max.abs()).max(1.0) * 0.5;
			for (value, want) in [(range.min - delta, range.min), (range.max + delta, range.max)] {
				assert_eq!(round_trip(value), want, "{path} ({key}) set to {value}");
			}
			for value in [range.min, range.max] {
				assert_eq!(round_trip(value), value, "{path} ({key}) at its bound {value}");
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
