//! `Normalize`: clamps every persisted value into its range (bounds in `ranges`),
//! replaces non-finite numbers by their defaults, wraps angles into (-180, 180],
//! applies the presets and fits, and makes names unique.
//!
//! Runs after load, after import, before a run and after every edit; idempotent.

use super::names;
use super::ranges as r;
use super::{
	BraggReflector, Coating, ColorScheme, Diffuser, Display, DisplayLayer, Ellipse, FacettedGrating, Fiber,
	FresnelLens, GrinLens, Layer, Lens, LogicGate, Material, Measurement, Outline, Part, PartKind, Polygon, Probe,
	Project, Rectangle, Scene, Simulation, Source, SourceArray, Stop, Triangle, TriangleType, Video, YScale, ZonePlate,
	normalize_angle,
};

/// Brings a value into its valid state.
pub trait Normalize {
	fn normalize(&mut self);
}

/// A refractive index clamped to 1..2.55 and rounded to 0.01 (ties to even); non-finite becomes 1.
pub fn normalize_index(n: f64) -> f64 {
	round2(r::INDEX.clamp_or(n, r::INDEX.min))
}

/// Rounds to two decimals, ties to even.
fn round2(v: f64) -> f64 {
	(v * 100.0).round_ties_even() / 100.0
}

impl Normalize for Project {
	fn normalize(&mut self) {
		self.scene.normalize();
		self.simulation.normalize();
		self.display.normalize();
		self.sources.iter_mut().for_each(Normalize::normalize);
		self.parts.iter_mut().for_each(Normalize::normalize);
		self.probes.iter_mut().for_each(Normalize::normalize);
		self.color_schemes.iter_mut().for_each(Normalize::normalize);
		names::make_unique(&mut self.sources, |_| Source::NAME_STEM.into());
		names::make_unique(&mut self.parts, |p| p.kind.name_stem().into());
		names::make_unique(&mut self.probes, |_| Probe::NAME_STEM.into());
		names::make_unique(&mut self.color_schemes, |_| names::SCHEME_STEM.into());
	}
}

impl Normalize for Scene {
	fn normalize(&mut self) {
		let d = Scene::default();
		self.width = r::SCENE_SIZE.clamp_u32(self.width);
		self.height = r::SCENE_SIZE.clamp_u32(self.height);
		self.damping_edge = r::SCENE_DAMPING_EDGE.clamp_i32(self.damping_edge);
		self.refractive_index = r::INDEX.clamp_or(self.refractive_index, d.refractive_index);
		self.image_source_scale = r::SCENE_IMAGE_SOURCE_SCALE.clamp_or(self.image_source_scale, d.image_source_scale);
	}
}

impl Normalize for Simulation {
	fn normalize(&mut self) {
		let d = Simulation::default();
		self.steps = r::SIM_STEPS.clamp_u32(self.steps);
		self.visualize_every = r::SIM_VISUALIZE_EVERY.clamp_u32(self.visualize_every);
		self.wave_speed = r::SIM_WAVE_SPEED.clamp_or(self.wave_speed, d.wave_speed);
		self.time_decay = r::SIM_TIME_DECAY.clamp_or(self.time_decay, d.time_decay);
		self.source_frequency_scale =
			r::SIM_SOURCE_FREQUENCY_SCALE.clamp_or(self.source_frequency_scale, d.source_frequency_scale);
		self.intensity_scale = r::SIM_INTENSITY_SCALE.clamp_or(self.intensity_scale, d.intensity_scale);
	}
}

impl DisplayLayer {
	/// Clamps the layer; an empty scheme name becomes the layer's default.
	pub fn normalize_as(&mut self, layer: Layer) {
		let d = match layer {
			Layer::Field => DisplayLayer::field_default(),
			Layer::Intensity => DisplayLayer::intensity_default(),
		};
		if self.color_scheme.trim().is_empty() {
			self.color_scheme = d.color_scheme;
		}
		self.brightness = r::LAYER_BRIGHTNESS.clamp_or(self.brightness, d.brightness);
		self.black_level = r::LAYER_BLACK_LEVEL.clamp_or(self.black_level, d.black_level);
		self.overlay_gain = r::LAYER_OVERLAY_GAIN.clamp_or(self.overlay_gain, d.overlay_gain);
	}
}

impl Normalize for Video {
	fn normalize(&mut self) {
		self.fps = r::VIDEO_FPS.clamp_u32(self.fps);
		self.quality = r::VIDEO_QUALITY.clamp_u32(self.quality);
	}
}

impl Normalize for Display {
	fn normalize(&mut self) {
		self.field.normalize_as(Layer::Field);
		self.intensity.normalize_as(Layer::Intensity);
		self.video.normalize();
		self.export_from = r::STEP_POINT.clamp_u32(self.export_from);
		self.export_to = r::STEP_POINT.clamp_u32(self.export_to);
		if self.export_to != 0 && self.export_to < self.export_from {
			self.export_to = 0;
		}
	}
}

impl Normalize for Source {
	fn normalize(&mut self) {
		let d = Source::default();
		self.x = r::POSITION.clamp_or(self.x, d.x);
		self.y = r::POSITION.clamp_or(self.y, d.y);
		self.frequency = r::LEVEL.clamp_or(self.frequency, d.frequency);
		self.amplitude = r::PERCENT.clamp_or(self.amplitude, d.amplitude);
		self.phase = r::PHASE.clamp_or(self.phase, d.phase);
		self.pulse_width = r::AT_LEAST_1.clamp_u32(self.pulse_width);
		self.off_width = r::AT_LEAST_1.clamp_u32(self.off_width);
		self.detach_after = self.detach_after.map(|s| r::DETACH_AFTER.clamp_u32(s));
		if let Some(a) = &mut self.array {
			a.normalize();
		}
	}
}

impl Normalize for SourceArray {
	fn normalize(&mut self) {
		self.size_x = r::AT_LEAST_1.clamp_u32(self.size_x);
		self.size_y = r::AT_LEAST_1.clamp_u32(self.size_y);
		self.angle = normalize_angle(self.angle);
		self.random_frequency = r::AT_LEAST_0.clamp_u32(self.random_frequency);
		self.random_phase = r::AT_LEAST_0.clamp_u32(self.random_phase);
		self.random_off_width = r::AT_LEAST_0.clamp_u32(self.random_off_width);
		self.random_position = r::AT_LEAST_0.clamp_u32(self.random_position);
	}
}

impl Normalize for Probe {
	fn normalize(&mut self) {
		match self.layer {
			Layer::Field => {
				self.measurement = Measurement::Amplitude;
				self.y_scale = YScale::Linear;
			}
			Layer::Intensity if self.measurement == Measurement::Amplitude => self.measurement = Measurement::Value,
			Layer::Intensity => {}
		}
		if !self.y_min.is_finite() || !self.y_max.is_finite() || self.y_max <= self.y_min {
			self.y_min = -1.0;
			self.y_max = 1.0;
		}
		self.window = r::PROBE_WINDOW.clamp_u32(self.window);
		self.box_width = r::PROBE_BOX_WIDTH.clamp_u32(self.box_width);
		self.box_height = r::PROBE_BOX_HEIGHT.clamp_u32(self.box_height);
		self.start_after = r::AT_LEAST_0.clamp_u32(self.start_after);
		self.decades = r::PROBE_DECADES.clamp_u32(self.decades);
	}
}

impl Normalize for ColorScheme {
	/// No stops become the grey ramp; positions clamp to 0..1 (non-finite 0); unsorted stops are sorted (stable).
	fn normalize(&mut self) {
		if self.stops.is_empty() {
			self.stops = ColorScheme::grey().stops;
			return;
		}
		for s in &mut self.stops {
			s.at = r::STOP_AT.clamp_or(s.at, 0.0);
		}
		if !self.stops.is_sorted_by(|a, b| a.at <= b.at) {
			self.stops.sort_by(|a: &Stop, b: &Stop| a.at.total_cmp(&b.at));
		}
	}
}

impl Normalize for Part {
	fn normalize(&mut self) {
		let b = &mut self.base;
		b.x = r::AT_LEAST_0.clamp_i32(b.x);
		b.y = r::AT_LEAST_0.clamp_i32(b.y);
		b.angle = normalize_angle(b.angle);
		match &mut self.kind {
			PartKind::Lens(p) => p.normalize(),
			PartKind::FresnelLens(p) => p.normalize(),
			PartKind::Polygon(p) => p.normalize(),
			PartKind::Rectangle(p) => p.normalize(),
			PartKind::Ellipse(p) => p.normalize(),
			PartKind::Triangle(p) => p.normalize(),
			PartKind::Outline(p) => p.normalize(),
			PartKind::Diffuser(p) => p.normalize(),
			PartKind::FacettedGrating(p) => p.normalize(),
			PartKind::Fiber(p) => p.normalize(),
			PartKind::BraggReflector(p) => p.normalize(),
			PartKind::GrinLens(p) => p.normalize(),
			PartKind::LogicGate(p) => p.normalize(),
			PartKind::ZonePlate(p) => p.normalize(),
		}
	}
}

impl Normalize for Diffuser {
	fn normalize(&mut self) {
		self.material.normalize();
		self.coating.normalize();
		self.length = r::AT_LEAST_1.clamp_u32(self.length);
		self.width = r::AT_LEAST_2.clamp_u32(self.width);
		let d = Diffuser::default();
		self.center_wavelength = r::RIPPLE_WAVELENGTH.clamp_or(self.center_wavelength, d.center_wavelength);
		self.amplitude = r::PLATE_PIXELS.clamp_or(self.amplitude, d.amplitude);
		self.sigma = r::PLATE_PIXELS.clamp_or(self.sigma, d.sigma);
	}
}

impl Normalize for FacettedGrating {
	fn normalize(&mut self) {
		self.material.normalize();
		self.coating.normalize();
		self.length = r::AT_LEAST_1.clamp_u32(self.length);
		self.width = r::AT_LEAST_2.clamp_u32(self.width);
		let d = FacettedGrating::default();
		self.pitch = r::RIPPLE_WAVELENGTH.clamp_or(self.pitch, d.pitch);
		self.amplitude = r::PLATE_PIXELS.clamp_or(self.amplitude, d.amplitude);
	}
}

impl Normalize for Coating {
	fn normalize(&mut self) {
		self.depth = r::COATING.clamp_or(self.depth, Coating::default().depth);
	}
}

impl Normalize for Material {
	fn normalize(&mut self) {
		self.refractive_index = normalize_index(self.refractive_index);
		self.absorption = r::ABSORPTION.clamp_u32(self.absorption);
		self.absorption_ramp = r::ABSORPTION_RAMP.clamp_u32(self.absorption_ramp);
	}
}

impl Normalize for Lens {
	fn normalize(&mut self) {
		self.material.normalize();
		self.coating.normalize();
		self.diameter = r::AT_LEAST_2.clamp_u32(self.diameter);
		self.thickness = r::AT_LEAST_1.clamp_u32(self.thickness);
		(self.r1, self.r2) = fit_radii(self.diameter, self.thickness, self.r1, self.r2);
	}
}

impl Normalize for FresnelLens {
	fn normalize(&mut self) {
		self.zones = r::FRESNEL_ZONES.clamp_u32(self.zones);
		let lens = &mut self.lens;
		lens.material.normalize();
		lens.coating.normalize();
		lens.diameter = r::AT_LEAST_2.clamp_u32(lens.diameter);
		lens.thickness = r::AT_LEAST_1.clamp_u32(lens.thickness);
		(lens.r1, lens.r2) = super::lens_geometry::fit_radii_zoned(
			lens.diameter,
			lens.thickness,
			lens.r1,
			lens.r2,
			self.zones,
			self.division,
		);
	}
}

impl Normalize for Polygon {
	fn normalize(&mut self) {
		self.material.normalize();
		self.coating.normalize();
		self.sides = r::POLYGON_SIDES.clamp_u32(self.sides);
		self.side = r::AT_LEAST_2.clamp_u32(self.side);
	}
}

impl Normalize for Rectangle {
	fn normalize(&mut self) {
		self.material.normalize();
		self.coating.normalize();
		self.length = r::AT_LEAST_1.clamp_u32(self.length);
		self.width = if self.linked { self.length } else { r::AT_LEAST_1.clamp_u32(self.width) };
	}
}

impl Normalize for Ellipse {
	fn normalize(&mut self) {
		self.material.normalize();
		self.coating.normalize();
		self.length = r::AT_LEAST_1.clamp_u32(self.length);
		self.width = if self.linked { self.length } else { r::AT_LEAST_1.clamp_u32(self.width) };
	}
}

impl Triangle {
	/// The angles at the base's ends the type stands for: the presets, Isosceles with `a <= 89`
	/// and `b = a`, Custom with `b <= 179 - a`. Non-finite angles count as 60.
	pub fn effective_angles(&self) -> (f64, f64) {
		let a = r::TRIANGLE_ANGLE.clamp_or(self.angle_a, 60.0);
		let b = r::TRIANGLE_ANGLE.clamp_or(self.angle_b, 60.0);
		match self.kind {
			TriangleType::Equilateral => (60.0, 60.0),
			TriangleType::RightIsosceles => (45.0, 45.0),
			TriangleType::ThirtySixtyNinety => (90.0, 30.0),
			TriangleType::Isosceles => {
				let a = a.min(89.0);
				(a, a)
			}
			TriangleType::Custom => (a, b.min(r::TRIANGLE_ANGLE.max + 1.0 - a)),
		}
	}
}

impl Normalize for Triangle {
	fn normalize(&mut self) {
		self.material.normalize();
		self.coating.normalize();
		self.base = r::AT_LEAST_2.clamp_u32(self.base);
		(self.angle_a, self.angle_b) = self.effective_angles();
	}
}

impl Normalize for Outline {
	fn normalize(&mut self) {
		self.material.normalize();
		self.coating.normalize();
	}
}

impl Normalize for Fiber {
	fn normalize(&mut self) {
		self.coating.normalize();
		self.core_index = normalize_index(self.core_index);
		self.cladding_index = normalize_index(self.cladding_index);
		self.core_diameter = r::AT_LEAST_2.clamp_u32(self.core_diameter);
		self.cladding_thickness = r::AT_LEAST_0.clamp_u32(self.cladding_thickness);
		self.absorption = r::ABSORPTION.clamp_u32(self.absorption);
		self.absorption_ramp = r::ABSORPTION_RAMP.clamp_u32(self.absorption_ramp);
	}
}

impl Normalize for BraggReflector {
	fn normalize(&mut self) {
		self.pairs = r::BRAGG_PAIRS.clamp_u32(self.pairs);
		self.thickness1 = r::AT_LEAST_1.clamp_u32(self.thickness1);
		self.thickness2 = r::AT_LEAST_1.clamp_u32(self.thickness2);
		self.index1 = normalize_index(self.index1);
		self.index2 = normalize_index(self.index2);
		self.width = r::AT_LEAST_2.clamp_u32(self.width);
	}
}

impl Normalize for GrinLens {
	fn normalize(&mut self) {
		self.coating.normalize();
		self.length = r::AT_LEAST_1.clamp_u32(self.length);
		self.diameter = r::AT_LEAST_2.clamp_u32(self.diameter);
		self.centre_index = normalize_index(self.centre_index);
		self.edge_index = normalize_index(self.edge_index);
	}
}

/// A design frequency clamped to 1..255 and rounded to 0.01 (ties to even); non-finite becomes `fallback`.
fn design_level(v: f64, fallback: f64) -> f64 {
	round2(r::DESIGN_LEVEL.clamp_or(v, fallback))
}

impl Normalize for LogicGate {
	fn normalize(&mut self) {
		let d = LogicGate::default();
		self.design_frequency = design_level(self.design_frequency, d.design_frequency);
		self.core_index = normalize_index(self.core_index);
		self.cladding_index = normalize_index(self.cladding_index);
		self.port_spacing = r::GATE_PORT_SPACING.clamp_u32(self.port_spacing);
		self.coating = r::COATING.clamp_or(self.coating, d.coating);
	}
}

impl Normalize for ZonePlate {
	fn normalize(&mut self) {
		self.mount_width = r::AT_LEAST_1.clamp_u32(self.mount_width);
		self.diameter = r::AT_LEAST_2.clamp_u32(self.diameter);
		self.thickness = r::AT_LEAST_1.clamp_u32(self.thickness);
		self.coating = r::ZONE_PLATE_COATING.clamp_u32(self.coating);
		self.design_frequency = design_level(self.design_frequency, ZonePlate::default().design_frequency);
		self.focal_length = r::AT_LEAST_1.clamp_u32(self.focal_length);
	}
}

pub use super::lens_geometry::{fit_radii, sag};

// ---------------------------------------------------------------- probe box

/// A graph box in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxRect {
	pub x: i32,
	pub y: i32,
	pub width: u32,
	pub height: u32,
}

/// The graph box of a normalised probe on a `w` x `h` canvas: shrunk with its aspect kept (rounded, ties to even)
/// only when larger than the canvas, then placed at the box position clamped inside. At least 1 x 1.
pub fn box_in_canvas(probe: &Probe, w: u32, h: u32) -> BoxRect {
	let (bw, bh) = probe.box_pixels();
	let mut scale: f64 = 1.0;
	if bw > w {
		scale = scale.min(w as f64 / bw as f64);
	}
	if bh > h {
		scale = scale.min(h as f64 / bh as f64);
	}
	let width = ((bw as f64 * scale).round_ties_even() as u32).max(1);
	let height = ((bh as f64 * scale).round_ties_even() as u32).max(1);
	let x = probe.box_x.min(w as i32 - width as i32).max(0);
	let y = probe.box_y.min(h as i32 - height as i32).max(0);
	BoxRect { x, y, width, height }
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{BoxSize, GateKind, Rgb};

	#[test]
	fn angles_wrap() {
		for (a, want) in [(540.0, 180.0), (-180.0, 180.0), (-190.0, 170.0), (190.0, -170.0), (180.0, 180.0), (0.0, 0.0)]
		{
			assert_eq!(normalize_angle(a), want, "{a}");
		}
		assert_eq!(normalize_angle(f64::NAN), 0.0);
		let mut p = Part::new(PartKind::Rectangle(Rectangle::default()), 1, 1);
		p.base.angle = -190.0;
		p.normalize();
		assert_eq!(p.base.angle, 170.0);
	}

	#[test]
	fn lens_radii_are_fitted() {
		let mut lens = Lens { diameter: 100, thickness: 20, r1: 50.0, r2: -50.0, ..Lens::default() };
		lens.normalize();
		assert_eq!((lens.r1, lens.r2), (130.0, -130.0));
		let mut fresnel = FresnelLens { lens: lens.clone(), zones: 200, ..FresnelLens::default() };
		fresnel.normalize();
		assert_eq!((fresnel.zones, fresnel.lens.r1), (100, 130.0), "100 zones of 0.5 px fit the 130 px radius");
	}

	#[test]
	fn probe_rules() {
		let mut p = Probe {
			measurement: Measurement::Value,
			y_scale: YScale::Logarithmic,
			y_min: 2.0,
			y_max: 2.0,
			..Probe::default()
		};
		p.normalize();
		assert_eq!((p.measurement, p.y_scale, p.y_min, p.y_max), (Measurement::Amplitude, YScale::Linear, -1.0, 1.0));
		let mut q = Probe { layer: Layer::Intensity, y_scale: YScale::Logarithmic, ..Probe::default() };
		q.normalize();
		assert_eq!((q.measurement, q.y_scale), (Measurement::Value, YScale::Logarithmic));
	}

	#[test]
	fn linked_and_triangle_presets() {
		let mut rect = Rectangle { length: 80, width: 20, linked: true, ..Rectangle::default() };
		rect.normalize();
		assert_eq!(rect.width, 80);
		let mut t = Triangle { kind: TriangleType::Custom, angle_a: 100.0, angle_b: 100.0, ..Triangle::default() };
		t.normalize();
		assert_eq!((t.angle_a, t.angle_b), (100.0, 79.0));
		let mut t = Triangle { kind: TriangleType::Isosceles, angle_a: 120.0, ..Triangle::default() };
		t.normalize();
		assert_eq!((t.angle_a, t.angle_b), (89.0, 89.0));
		let mut t = Triangle { kind: TriangleType::ThirtySixtyNinety, ..Triangle::default() };
		t.normalize();
		assert_eq!((t.angle_a, t.angle_b), (90.0, 30.0));
	}

	#[test]
	fn index_and_design_rounding() {
		assert_eq!(normalize_index(1.234), 1.23);
		assert_eq!(normalize_index(3.0), 2.55);
		assert_eq!(normalize_index(f64::NAN), 1.0);
		let mut g = LogicGate { kind: GateKind::Or, design_frequency: 50.125, ..LogicGate::default() };
		g.normalize();
		assert_eq!(g.design_frequency, 50.12);
	}

	#[test]
	fn export_to_rule() {
		let mut d = Display { export_from: 100, export_to: 50, ..Display::default() };
		d.normalize();
		assert_eq!(d.export_to, 0);
		let mut d = Display { export_from: 100, export_to: 150, ..Display::default() };
		d.normalize();
		assert_eq!(d.export_to, 150);
		let mut d = Display::default();
		d.field.color_scheme = " ".into();
		d.normalize();
		assert_eq!(d.field.color_scheme, "Default field");
	}

	#[test]
	fn non_finite_becomes_default() {
		let mut s = Source { frequency: f64::NAN, amplitude: f64::INFINITY, phase: f64::NAN, ..Source::default() };
		s.normalize();
		assert_eq!((s.frequency, s.amplitude, s.phase), (100.0, 100.0, 0.0));
		let mut sim = Simulation { wave_speed: f64::NAN, ..Simulation::default() };
		sim.normalize();
		assert_eq!(sim.wave_speed, 1.0);
	}

	#[test]
	fn scheme_rules() {
		let mut s = ColorScheme { name: "a".into(), ..ColorScheme::default() };
		s.normalize();
		assert_eq!(s.stops, ColorScheme::grey().stops);
		let c = Rgb::new(1, 2, 3);
		let mut s = ColorScheme {
			stops: vec![Stop { at: 2.0, color: c }, Stop { at: 0.5, color: c }, Stop { at: -1.0, color: c }],
			..ColorScheme::default()
		};
		s.normalize();
		let at: Vec<f64> = s.stops.iter().map(|s| s.at).collect();
		assert_eq!(at, [0.0, 0.5, 1.0]);
	}

	#[test]
	fn box_fits_canvas() {
		let p = Probe { box_x: 700, box_y: -5, ..Probe::default() };
		assert_eq!(box_in_canvas(&p, 800, 600), BoxRect { x: 400, y: 0, width: 400, height: 200 });
		assert_eq!(box_in_canvas(&p, 300, 600), BoxRect { x: 0, y: 0, width: 300, height: 150 });
		let mut c = Probe { box_size: BoxSize::Custom, box_width: 10, box_height: 10_000, ..Probe::default() };
		c.normalize();
		assert_eq!(c.box_pixels(), (60, 4096));
	}

	#[test]
	fn project_names_unique_after_normalize() {
		let mut p = Project::default();
		p.sources.push(Source { name: "S".into(), ..Source::default() });
		p.sources.push(Source { name: "s".into(), ..Source::default() });
		p.color_schemes.push(ColorScheme::default());
		p.normalize();
		assert_eq!(p.sources[1].name, "s copy");
		assert_eq!(p.color_schemes[0].name, "Scheme 1");
	}
}
