//! The typed forms (ui/forms.slint): an object as the Slint data its form binds (ui/data.slint), and a form's edits
//! applied back field by field.

use slint::{Model, ModelRc, SharedString, VecModel};
use waveduderust::model::edited::Edited;
use waveduderust::model::lens_geometry::{edge_thickness, fit_radii_zoned};
use waveduderust::model::{
	self as m, Coating, Lens, Material, PROJECT_VERSION, Part, PartKind, Point, Project, Stencil, View, wavelength,
};

use waveduderust::io::schemes::SchemeStore;
use waveduderust::unstable::render::gradient::Gradient;
use waveduderust::unstable::scene::gate_design::{GateDesign, ports_of};

use crate::tree::Node;
use crate::ui::{self, Form, FormKind};

/// A model value as a form holds it.
trait Bridge<M> {
	fn show(model: &M) -> Self;
	fn model(&self) -> M;
}

impl Bridge<bool> for bool {
	fn show(model: &bool) -> Self {
		*model
	}
	fn model(&self) -> bool {
		*self
	}
}

impl Bridge<i32> for i32 {
	fn show(model: &i32) -> Self {
		*model
	}
	fn model(&self) -> i32 {
		*self
	}
}

impl Bridge<u32> for i32 {
	fn show(model: &u32) -> Self {
		i32::try_from(*model).unwrap_or(i32::MAX)
	}
	fn model(&self) -> u32 {
		u32::try_from(*self).unwrap_or(0)
	}
}

/// An optional step: below 0 is none, as the box's minimum.
impl Bridge<Option<u32>> for i32 {
	fn show(model: &Option<u32>) -> Self {
		model.map_or(-1, |v| Bridge::show(&v))
	}
	fn model(&self) -> Option<u32> {
		u32::try_from(*self).ok()
	}
}

impl Bridge<f64> for f32 {
	fn show(model: &f64) -> Self {
		*model as f32
	}
	fn model(&self) -> f64 {
		crate::number::wide(*self)
	}
}

impl Bridge<String> for SharedString {
	fn show(model: &String) -> Self {
		model.into()
	}
	fn model(&self) -> String {
		self.into()
	}
}

/// Slint enums and the model's of the same variants. The model's are non-exhaustive: one is shown as the Slint
/// variant that converts back to it, the first for one this table lacks (tested below).
macro_rules! words {
	($($ui:ident = $model:ident { $($v:ident),+ $(,)? })+) => {
		$(impl Bridge<m::$model> for ui::$ui {
			fn show(model: &m::$model) -> Self {
				[$(Self::$v),+].into_iter().find(|u| u.model() == *model).unwrap_or_default()
			}
			fn model(&self) -> m::$model {
				match self {
					$(Self::$v => m::$model::$v,)+
				}
			}
		})+

		#[cfg(test)]
		#[test]
		fn every_model_word_has_its_slint_variant() {
			use waveduderust::model::Word;
			$(for w in m::$model::MEMBERS {
				assert_eq!(<ui::$ui as Bridge<m::$model>>::show(w).model(), *w, "{}", w.name());
			})+
		}
	};
}

words! {
	SceneMode = SceneMode { Canvas, Image }
	Edges = Edges { Open, Reflective }
	Stencil = Stencil { NinePoint, FourthOrder }
	SourceMode = SourceMode { Continuous, Pulsed }
	PulseShape = PulseShape { Gaussian, SmoothedSquare }
	Layer = Layer { Field, Intensity }
	Measurement = Measurement { Amplitude, Value, IntegratedValue }
	ProbeKind = ProbeKind { Point, Line }
	ProbeDetector = ProbeDetector { Standard, Counting, Imaging }
	BoxSize = BoxSize { Tiny, Small, Medium, Large, ExtraLarge, Custom }
	ScaleMode = ScaleMode { Running, Full }
	YScale = YScale { Linear, Logarithmic }
	CoatingProfile = CoatingProfile { Layer, Graded }
	TriangleType = TriangleType { Equilateral, RightIsosceles, ThirtySixtyNinety, Isosceles, Custom }
	ZoneDivision = ZoneDivision { EqualWidth, EqualDepth }
	FacetPattern = FacetPattern { Sawtooth, SquareWave, Sinusoidal }
	FiberEnds = FiberEnds { Flat, Round }
	MetaLensType = MetaLensType { Converging, Diverging }
	GateKind = GateKind { Xor, And, Or }
	SchemeKind = SchemeKind { Unspecified, Field, Intensity }
}

/// A form's data of a model type: its fields of the same names, the rest left to the caller.
trait Data<M>: Sized {
	fn show(model: &M) -> Self;
	/// Writes to `model` the fields that differ from `shown`, the data `model` gives: a field the form did not change
	/// keeps the model's exact value, which the form's may only approximate.
	fn apply(&self, shown: &Self, model: &mut M);
}

macro_rules! data {
	($($ui:ident = $model:ty { $($f:ident),+ $(,)? })+) => {$(
		impl Data<$model> for ui::$ui {
			// the read-outs are the caller's
			#[allow(clippy::needless_update)]
			fn show(model: &$model) -> Self {
				Self { $($f: Bridge::show(&model.$f),)+ ..Default::default() }
			}
			fn apply(&self, shown: &Self, model: &mut $model) {
				$(if self.$f != shown.$f {
					model.$f = self.$f.model();
				})+
			}
		}
	)+};
}

data! {
	ProjectData = Project { seed, description }
	SceneData = m::Scene {
		mode, image, image_path, width, height, damping_edge, edges, refractive_index, image_sources,
		image_source_scale, object_sources,
	}
	SimulationData = m::Simulation {
		steps, visualize_every, stencil, wave_speed, time_decay, source_frequency_scale, intensity_scale,
	}
	DisplayData = View {
		export_from, export_to, probe_graphs_in_export, probe_markers_in_export, source_markers_in_export,
		handles_in_preview, labels_in_preview,
	}
	LayerData = m::DisplayLayer { color_scheme, brightness, black_level, preview, overlay, overlay_gain, overlay_in_export }
	VideoData = m::Video { fps, quality }
	SourceData = m::Source {
		name, enabled, x, y, frequency, amplitude, phase, mode, pulse_shape, pulse_width, off_width, detach_after,
	}
	ArrayData = m::SourceArray {
		size_x, size_y, spacing_x, spacing_y, angle, random_frequency, random_phase, random_off_width, random_position,
	}
	ProbeData = m::Probe {
		name, active, start_after, detector, sensitivity, bin_size, layer, measurement, kind, ax, ay, bx, by, box_size,
		box_x, box_y, box_width, box_height, scale_mode, window, auto_scale_x, auto_scale_y, y_scale, decades, y_min,
		y_max,
	}
	PartData = m::PartBase { name, enabled, x, y, angle }
	MaterialData = Material { mirror, refractive_index, absorption, absorption_ramp }
	CoatingData = Coating { depth, profile, front, back, sides }
	SidesData = m::Rectangle { length, width, linked }
	TriangleData = m::Triangle { base, kind, angle_a, angle_b }
	PolygonData = m::Polygon { sides, side }
	LensData = Lens { diameter, r1, r2, thickness }
	FresnelData = m::FresnelLens { zones, division }
	BraggData = m::BraggReflector { pairs, thickness1, index1, thickness2, index2, width }
	GateData = m::LogicGate {
		kind, design_frequency, core_index, cladding_index, coating, input_coated, output_coated, upper_output_active,
		lower_output_active,
	}
	MetaData = m::MetaSurface { length, width, design_frequency, pattern_index, pitch, height }
	FacetData = m::FacettedGrating { length, width, pattern, pitch, amplitude }
	GratingData = m::SimpleGrating { columns, rows, column_spacing, row_spacing, diameter }
	GrinData = m::GrinLens { centre_index, edge_index, length, diameter }
	DiffuserData = m::Diffuser { length, width, center_wavelength, amplitude, sigma }
	ZonePlateData = m::ZonePlate { mount, mount_width, diameter, thickness, coating, design_frequency, focal_length, inverse }
	PointData = Point { x, y }
	FilterLayerData = m::FilterLayer { thickness, refractive_index }
	FilterData = m::InterferenceFilter { length, width }
	FiberData = m::Fiber { core_index, core_diameter, cladding_index, cladding_thickness, absorption, absorption_ramp, ends }
	FiberNodeData = m::FiberNode { x, y, in_x, in_y, out_x, out_y, smooth }
	SchemeData = m::ColorScheme { name, kind, description }
}

/// An ellipse's sides are a rectangle's.
impl Data<m::Ellipse> for ui::SidesData {
	fn show(model: &m::Ellipse) -> Self {
		Self { length: Bridge::show(&model.length), width: Bridge::show(&model.width), linked: model.linked }
	}
	fn apply(&self, shown: &Self, model: &mut m::Ellipse) {
		let mut sides = m::Rectangle::default();
		(sides.length, sides.width, sides.linked) = (model.length, model.width, model.linked);
		self.apply(shown, &mut sides);
		(model.length, model.width, model.linked) = (sides.length, sides.width, sides.linked);
	}
}

/// The form `node` shows; none for an object without one.
pub(crate) fn kind(project: &Project, node: Node) -> FormKind {
	match node {
		Node::Part(i) => match project.parts.get(i).map(|p| &p.kind) {
			Some(PartKind::Rectangle(_)) => FormKind::Rectangle,
			Some(PartKind::Ellipse(_)) => FormKind::Ellipse,
			Some(PartKind::Triangle(_)) => FormKind::Triangle,
			Some(PartKind::Polygon(_)) => FormKind::Polygon,
			Some(PartKind::Lens(_)) => FormKind::Lens,
			Some(PartKind::FresnelLens(_)) => FormKind::FresnelLens,
			Some(PartKind::Outline(_)) => FormKind::Outline,
			Some(PartKind::Diffuser(_)) => FormKind::Diffuser,
			Some(PartKind::InterferenceFilter(_)) => FormKind::InterferenceFilter,
			Some(PartKind::Fiber(_)) => FormKind::Fiber,
			Some(PartKind::ZonePlate(_)) => FormKind::ZonePlate,
			Some(PartKind::BraggReflector(_)) => FormKind::BraggReflector,
			Some(PartKind::GrinLens(_)) => FormKind::GrinLens,
			Some(PartKind::SimpleGrating(_)) => FormKind::SimpleGrating,
			Some(PartKind::FacettedGrating(_)) => FormKind::FacettedGrating,
			Some(PartKind::LogicGate(_)) => FormKind::LogicGate,
			Some(PartKind::MetaSurface(s)) => match s.design {
				m::MetaDesign::Lens(_) => FormKind::MetaLens,
				m::MetaDesign::Prism(_) => FormKind::MetaPrism,
				m::MetaDesign::Axicon(_) => FormKind::MetaAxicon,
				_ => FormKind::None,
			},
			_ => FormKind::None,
		},
		Node::Project => FormKind::Project,
		Node::Scene => FormKind::Scene,
		Node::Simulation => FormKind::Simulation,
		Node::Display => FormKind::Display,
		Node::Source(_) => FormKind::Source,
		Node::Probe(_) => FormKind::Probe,
		Node::Scheme(_) => FormKind::Scheme,
		Node::Sources | Node::Parts | Node::Probes | Node::Schemes => FormKind::None,
	}
}

/// Sets a form property to `value` when it differs: an equal one is left alone, so an editor bound to it keeps its
/// caret and selection.
fn put<T: PartialEq>(shown: T, value: T, set: impl FnOnce(T)) {
	if shown != value {
		set(value);
	}
}

/// The material and coating of a part kind that has them.
fn shape(kind: &PartKind) -> Option<(&Material, &Coating)> {
	match kind {
		PartKind::Rectangle(p) => Some((&p.material, &p.coating)),
		PartKind::Ellipse(p) => Some((&p.material, &p.coating)),
		PartKind::Triangle(p) => Some((&p.material, &p.coating)),
		PartKind::Polygon(p) => Some((&p.material, &p.coating)),
		PartKind::Lens(p) => Some((&p.material, &p.coating)),
		PartKind::FresnelLens(p) => Some((&p.lens.material, &p.lens.coating)),
		PartKind::Outline(p) => Some((&p.material, &p.coating)),
		PartKind::Diffuser(p) => Some((&p.material, &p.coating)),
		PartKind::InterferenceFilter(p) => Some((&p.material, &p.coating)),
		PartKind::SimpleGrating(p) => Some((&p.material, &p.coating)),
		PartKind::FacettedGrating(p) => Some((&p.material, &p.coating)),
		PartKind::MetaSurface(p) => Some((&p.material, &p.coating)),
		_ => None,
	}
}

fn shape_mut(kind: &mut PartKind) -> Option<(&mut Material, &mut Coating)> {
	match kind {
		PartKind::Rectangle(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::Ellipse(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::Triangle(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::Polygon(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::Lens(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::FresnelLens(p) => Some((&mut p.lens.material, &mut p.lens.coating)),
		PartKind::Outline(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::Diffuser(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::InterferenceFilter(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::SimpleGrating(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::FacettedGrating(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::MetaSurface(p) => Some((&mut p.material, &mut p.coating)),
		_ => None,
	}
}

/// A meta surface's data: its design's fields and the read-outs.
fn meta_data(s: &m::MetaSurface, scale: f64) -> ui::MetaData {
	let read = |v: Option<f64>| v.unwrap_or_default() as f32;
	let mut data = ui::MetaData {
		wavelength: read(s.wavelength(scale)),
		full_turn_height: read(s.full_turn_height(scale)),
		zone_width: read(s.zone_width(scale)),
		line_length: read(s.line_length()),
		zones: Bridge::show(&s.zone_count(scale)),
		..Data::show(s)
	};
	match &s.design {
		m::MetaDesign::Lens(l) => {
			data.focal_length = Bridge::show(&l.focal_length);
			data.lens_type = Bridge::show(&l.lens_type);
		}
		m::MetaDesign::Prism(p) => data.deflection = Bridge::show(&p.deflection),
		m::MetaDesign::Axicon(a) => data.cone_angle = Bridge::show(&a.cone_angle),
		_ => {}
	}
	data
}

/// A logic gate's data: the port spacing its design uses, and the layout read-outs.
fn gate_data(base: &m::PartBase, g: &m::LogicGate, sim: &m::Simulation) -> ui::GateData {
	let design = GateDesign::for_gate(g, sim);
	let lambda = wavelength(g.design_frequency, 1.0, sim.source_frequency_scale);
	let cells = |n: i64| i32::try_from(n).unwrap_or(i32::MAX);
	let mut data = ui::GateData {
		port_spacing: design.as_ref().map_or_else(|| Bridge::show(&g.port_spacing), |d| cells(d.port_spacing)),
		wavelength: if lambda.is_finite() && lambda > 0.0 { lambda as f32 } else { 0.0 },
		designed: design.is_some(),
		..Data::show(g)
	};
	if let Some(d) = &design {
		data.quarter_wave = (d.wavelength / (4.0 * d.core_index.sqrt())) as f32;
		data.core_width = cells(d.core_width);
		data.gap = cells(d.coupled - d.core_width);
		data.coupler_length = cells(d.coupler_len);
		data.length = cells(d.length);
		let [a, b, upper, lower] = ports_of(d, base).map(|p| Data::show(&p));
		(data.input_a, data.input_b, data.upper_out, data.lower_out) = (a, b, upper, lower);
	}
	data
}

/// Writes a logic gate's changed fields; a port spacing typed is the design's raised one replaced.
fn apply_gate(d: &ui::GateData, g: &mut m::LogicGate, sim: &m::Simulation) {
	let used = GateDesign::for_gate(g, sim)
		.map_or_else(|| Bridge::show(&g.port_spacing), |design| i32::try_from(design.port_spacing).unwrap_or(i32::MAX));
	d.apply(&Data::show(&*g), g);
	if d.port_spacing != used {
		g.port_spacing = d.port_spacing.model();
	}
}

/// Writes `ui` to `model` when it differs from the model's shown value.
fn set<M, U: Bridge<M> + PartialEq>(ui: &U, model: &mut M) {
	if *ui != U::show(model) {
		*model = ui.model();
	}
}

/// Writes a meta surface's changed fields, its design's among them.
fn apply_meta(d: &ui::MetaData, s: &mut m::MetaSurface) {
	d.apply(&Data::show(&*s), s);
	match &mut s.design {
		m::MetaDesign::Lens(l) => {
			set(&d.focal_length, &mut l.focal_length);
			set(&d.lens_type, &mut l.lens_type);
		}
		m::MetaDesign::Prism(p) => set(&d.deflection, &mut p.deflection),
		m::MetaDesign::Axicon(a) => set(&d.cone_angle, &mut a.cone_angle),
		_ => {}
	}
}

/// A lens's data with its read-outs: concave, and its thickness at the rim with its radii fitted as a run fits them.
fn lens_data(l: &Lens, zones: u32, division: m::ZoneDivision) -> ui::LensData {
	let (diameter, thickness) = (l.diameter.max(2), l.thickness.max(1));
	let (r1, r2) = fit_radii_zoned(diameter, thickness, l.r1, l.r2, zones, division);
	ui::LensData {
		concave: l.is_concave(),
		edge: edge_thickness(diameter, thickness, r1, r2, zones, division) as f32,
		..Data::show(l)
	}
}

fn scene_data(scene: &m::Scene) -> ui::SceneData {
	let (image_width, image_height) = scene.image_size;
	ui::SceneData {
		embedded: scene.image_data.is_some(),
		readable: scene.image_size != (0, 0),
		image_width: Bridge::show(&image_width),
		image_height: Bridge::show(&image_height),
		image_kb: scene.image_data.as_ref().map_or(0, |b| i32::try_from(b.len() / 1024).unwrap_or(i32::MAX).max(1)),
		..Data::show(scene)
	}
}

fn source_data(source: &m::Source, sim: &m::Simulation) -> ui::SourceData {
	let lambda = wavelength(source.frequency, sim.wave_speed, sim.source_frequency_scale);
	ui::SourceData {
		composite: source.array.is_some(),
		wavelength: lambda as f32,
		wavelength_known: lambda.is_finite(),
		nine_point: sim.stencil == Stencil::NinePoint,
		..Data::show(source)
	}
}

fn probe_data(probe: &m::Probe) -> ui::ProbeData {
	ui::ProbeData { shows_profile: probe.shows_profile(), ..Data::show(probe) }
}

/// Hands the form of `node` its object's data.
pub(crate) fn show(form: &Form<'_>, project: &Project, view: &View, node: Node) {
	match node {
		Node::Project => put(
			form.get_project(),
			ui::ProjectData { version: Bridge::show(&PROJECT_VERSION), ..Data::show(project) },
			|d| form.set_project(d),
		),
		Node::Scene => put(form.get_scene(), scene_data(&project.scene), |d| form.set_scene(d)),
		Node::Simulation => put(form.get_simulation(), Data::show(&project.simulation), |d| form.set_simulation(d)),
		Node::Display => {
			put(form.get_display(), Data::show(view), |d| form.set_display(d));
			put(form.get_field(), Data::show(&view.field), |d| form.set_field(d));
			put(form.get_intensity(), Data::show(&view.intensity), |d| form.set_intensity(d));
			put(form.get_video(), Data::show(&view.video), |d| form.set_video(d));
		}
		Node::Source(i) => {
			let source = &project.sources[i];
			put(form.get_source(), source_data(source, &project.simulation), |d| form.set_source(d));
			let array = source.array.clone().unwrap_or_default();
			put(form.get_array(), Data::show(&array), |d| form.set_array(d));
		}
		Node::Probe(i) => put(form.get_probe(), probe_data(&project.probes[i]), |d| form.set_probe(d)),
		Node::Part(i) => show_part(form, project, &project.parts[i]),
		Node::Scheme(i) => show_scheme(form, project, i),
		Node::Sources | Node::Parts | Node::Probes | Node::Schemes => {}
	}
}

fn show_part(form: &Form<'_>, project: &Project, part: &Part) {
	put(form.get_part(), Data::show(&part.base), |d| form.set_part(d));
	if let Some((material, coating)) = shape(&part.kind) {
		put(form.get_material(), Data::show(material), |d| form.set_material(d));
		put(form.get_coating(), Data::show(coating), |d| form.set_coating(d));
	}
	match &part.kind {
		PartKind::Rectangle(p) => put(form.get_sides(), Data::show(p), |d| form.set_sides(d)),
		PartKind::Ellipse(p) => put(form.get_sides(), Data::show(p), |d| form.set_sides(d)),
		PartKind::Triangle(t) => {
			let (a, b) = t.effective_angles();
			let data = ui::TriangleData { apex: (180.0 - a - b) as f32, ..Data::show(t) };
			put(form.get_triangle(), data, |d| form.set_triangle(d));
		}
		PartKind::Polygon(p) => put(form.get_polygon(), Data::show(p), |d| form.set_polygon(d)),
		PartKind::Lens(l) => put(form.get_lens(), lens_data(l, 1, m::ZoneDivision::EqualWidth), |d| form.set_lens(d)),
		PartKind::FresnelLens(f) => {
			put(form.get_lens(), lens_data(&f.lens, f.zones, f.division), |d| form.set_lens(d));
			put(form.get_fresnel(), Data::show(f), |d| form.set_fresnel(d));
		}
		PartKind::BraggReflector(b) => put(form.get_bragg(), Data::show(b), |d| form.set_bragg(d)),
		PartKind::FacettedGrating(f) => put(form.get_facets(), Data::show(f), |d| form.set_facets(d)),
		PartKind::LogicGate(g) => {
			put(form.get_gate(), gate_data(&part.base, g, &project.simulation), |d| form.set_gate(d));
		}
		PartKind::MetaSurface(s) => {
			put(form.get_meta(), meta_data(s, project.simulation.source_frequency_scale), |d| form.set_meta(d));
		}
		PartKind::SimpleGrating(g) => {
			let data = ui::GratingData { max_diameter: Bridge::show(&g.max_diameter()), ..Data::show(g) };
			put(form.get_grating(), data, |d| form.set_grating(d));
		}
		PartKind::GrinLens(g) => {
			put(form.get_coating(), Data::show(&g.coating), |d| form.set_coating(d));
			put(form.get_grin(), Data::show(g), |d| form.set_grin(d));
		}
		PartKind::Diffuser(d) => {
			let profile = d.profile(&part.base.name, project.seed);
			let (low, high) =
				profile.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &p| (l.min(p), h.max(p)));
			let data = ui::DiffuserData {
				profile_height: (high - low) as f32,
				ripples: i32::try_from(d.components().len()).unwrap_or(i32::MAX),
				..Data::show(d)
			};
			put(form.get_diffuser(), data, |d| form.set_diffuser(d));
		}
		PartKind::ZonePlate(z) => {
			let scale = project.simulation.source_frequency_scale;
			let data = ui::ZonePlateData {
				wavelength: z.wavelength(scale).unwrap_or_default() as f32,
				zones: i32::try_from(z.zone_count(scale)).unwrap_or(i32::MAX),
				outer_zone: z.outer_zone_width(scale).unwrap_or_default() as f32,
				..Data::show(z)
			};
			put(form.get_zone_plate(), data, |d| form.set_zone_plate(d));
		}
		PartKind::Outline(o) => put_rows(&form.get_points(), &o.points, |m| form.set_points(m)),
		PartKind::Fiber(f) => {
			put(form.get_coating(), Data::show(&f.coating), |d| form.set_coating(d));
			put(form.get_fiber(), Data::show(f), |d| form.set_fiber(d));
			put_rows(&form.get_nodes(), &f.nodes, |m| form.set_nodes(m));
		}
		PartKind::InterferenceFilter(f) => {
			let data = ui::FilterData {
				stack_thickness: ((f.stack_thickness() * 10.0).round() / 10.0) as f32,
				..Data::show(f)
			};
			put(form.get_filter(), data, |d| form.set_filter(d));
			put_rows(&form.get_layers(), &f.layers, |m| form.set_layers(m));
		}
		_ => {}
	}
}

/// Applies the edits the form of `node`, a project object, holds.
pub(crate) fn apply(form: &Form<'_>, project: &mut Project, node: Node) {
	match node {
		Node::Project => {
			let shown = ui::ProjectData { version: form.get_project().version, ..Data::show(&*project) };
			form.get_project().apply(&shown, project);
		}
		Node::Scene => {
			let scene = &mut project.scene;
			form.get_scene().apply(&scene_data(scene), scene);
		}
		Node::Simulation => {
			let sim = &mut project.simulation;
			form.get_simulation().apply(&Data::show(&*sim), sim);
		}
		Node::Source(i) => {
			let sim = project.simulation.clone();
			let Some(source) = project.sources.get_mut(i) else { return };
			let data = form.get_source();
			data.apply(&source_data(source, &sim), source);
			if data.composite != source.array.is_some() {
				source.array = data.composite.then(Default::default);
			} else if let Some(array) = &mut source.array {
				form.get_array().apply(&Data::show(&*array), array);
			}
		}
		Node::Probe(i) => {
			let Some(probe) = project.probes.get_mut(i) else { return };
			let before = probe.clone();
			form.get_probe().apply(&probe_data(&before), probe);
			probe.edited(&before);
		}
		Node::Part(i) => {
			let Some(part) = project.parts.get_mut(i) else { return };
			let before = part.clone();
			apply_part(form, part, &project.simulation);
			part.edited(&before);
		}
		Node::Scheme(i) => {
			let Some(scheme) = project.color_schemes.get_mut(i) else { return };
			form.get_scheme().apply(&Data::show(&*scheme), scheme);
			apply_rows(&form.get_stops(), &mut scheme.stops);
		}
		Node::Display | Node::Sources | Node::Parts | Node::Probes | Node::Schemes => {}
	}
}

fn apply_part(form: &Form<'_>, part: &mut Part, sim: &m::Simulation) {
	form.get_part().apply(&Data::show(&part.base), &mut part.base);
	if let Some((material, coating)) = shape_mut(&mut part.kind) {
		form.get_material().apply(&Data::show(&*material), material);
		form.get_coating().apply(&Data::show(&*coating), coating);
	}
	match &mut part.kind {
		PartKind::Rectangle(p) => form.get_sides().apply(&Data::show(&*p), p),
		PartKind::Ellipse(p) => form.get_sides().apply(&Data::show(&*p), p),
		PartKind::Triangle(p) => form.get_triangle().apply(&Data::show(&*p), p),
		PartKind::Polygon(p) => form.get_polygon().apply(&Data::show(&*p), p),
		PartKind::Lens(l) => apply_lens(&form.get_lens(), l, 1, m::ZoneDivision::EqualWidth),
		PartKind::FresnelLens(f) => {
			apply_lens(&form.get_lens(), &mut f.lens, f.zones, f.division);
			form.get_fresnel().apply(&Data::show(&*f), f);
		}
		PartKind::Diffuser(d) => form.get_diffuser().apply(&Data::show(&*d), d),
		PartKind::BraggReflector(b) => form.get_bragg().apply(&Data::show(&*b), b),
		PartKind::SimpleGrating(g) => form.get_grating().apply(&Data::show(&*g), g),
		PartKind::FacettedGrating(f) => form.get_facets().apply(&Data::show(&*f), f),
		PartKind::MetaSurface(s) => apply_meta(&form.get_meta(), s),
		PartKind::LogicGate(g) => apply_gate(&form.get_gate(), g, sim),
		PartKind::GrinLens(g) => {
			form.get_coating().apply(&Data::show(&g.coating), &mut g.coating);
			form.get_grin().apply(&Data::show(&*g), g);
		}
		PartKind::ZonePlate(z) => form.get_zone_plate().apply(&Data::show(&*z), z),
		PartKind::Outline(o) => apply_rows(&form.get_points(), &mut o.points),
		PartKind::Fiber(f) => {
			form.get_coating().apply(&Data::show(&f.coating), &mut f.coating);
			form.get_fiber().apply(&Data::show(&*f), f);
			let before = f.nodes.clone();
			apply_rows(&form.get_nodes(), &mut f.nodes);
			for (i, node) in before.iter().enumerate() {
				if f.nodes.get(i) != Some(node) {
					f.node_edited(i, node);
				}
			}
		}
		PartKind::InterferenceFilter(f) => {
			form.get_filter().apply(&Data::show(&*f), f);
			apply_rows(&form.get_layers(), &mut f.layers);
		}
		_ => {}
	}
}

fn apply_lens(data: &ui::LensData, l: &mut Lens, zones: u32, division: m::ZoneDivision) {
	let shown = lens_data(l, zones, division);
	data.apply(&shown, l);
	if data.concave != shown.concave {
		l.set_concave(data.concave);
	}
}

/// Applies the edits the Display form holds.
pub(crate) fn apply_view(form: &Form<'_>, view: &mut View) {
	form.get_display().apply(&Data::show(&*view), view);
	form.get_field().apply(&Data::show(&view.field), &mut view.field);
	form.get_intensity().apply(&Data::show(&view.intensity), &mut view.intensity);
	form.get_video().apply(&Data::show(&view.video), &mut view.video);
}

impl Bridge<m::Rgb> for slint::Color {
	fn show(model: &m::Rgb) -> Self {
		Self::from_rgb_u8(model.r, model.g, model.b)
	}
	fn model(&self) -> m::Rgb {
		m::Rgb::new(self.red(), self.green(), self.blue())
	}
}

pub(crate) fn rgb(colour: slint::Color) -> m::Rgb {
	Bridge::model(&colour)
}

/// The colour picker's arithmetic.
pub(crate) fn wire_colors(logic: &ui::ColorLogic<'_>) {
	logic.on_to_hsv(|c| {
		let (hue, saturation, value) = rgb(c).to_hsv();
		ui::Hsv { hue: hue as f32, saturation: saturation as f32, value: value as f32 }
	});
	logic.on_from_hsv(|hsv| {
		Bridge::show(&m::Rgb::from_hsv(f64::from(hsv.hue), f64::from(hsv.saturation), f64::from(hsv.value)))
	});
	logic.on_hex(|c| rgb(c).to_hex().into());
	logic.on_parse_hex(|text, otherwise| m::Rgb::parse_hex(&text).map_or(otherwise, |c| Bridge::show(&c)));
	logic.on_channel(|text, otherwise| {
		text.trim()
			.parse::<f64>()
			.ok()
			.filter(|v| v.is_finite())
			.map_or(otherwise, |v| v.round().clamp(0.0, 255.0) as i32)
	});
}

impl Data<m::Stop> for ui::StopData {
	fn show(model: &m::Stop) -> Self {
		Self { at: Bridge::show(&model.at), color: Bridge::show(&model.color), hex: model.color.to_hex().into() }
	}
	fn apply(&self, shown: &Self, model: &mut m::Stop) {
		if self.at != shown.at {
			model.at = self.at.model();
		}
		// text that does not read as #RRGGBB leaves the colour, which the row then shows again
		if self.hex != shown.hex {
			model.color = m::Rgb::parse_hex(&self.hex).unwrap_or(model.color);
		} else if self.color != shown.color {
			model.color = self.color.model();
		}
	}
}

/// `scheme`'s gradient, a strip a pixel tall to stretch.
fn strip(scheme: &m::ColorScheme) -> slint::Image {
	const WIDTH: u32 = 256;
	let colours = Gradient::new(&scheme.sorted_stops()).interpolate(WIDTH as usize);
	let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(WIDTH, 1);
	for (pixel, c) in pixels.make_mut_slice().iter_mut().zip(colours) {
		*pixel = slint::Rgb8Pixel::new(c.r, c.g, c.b);
	}
	slint::Image::from_rgb8(pixels)
}

/// The schemes a scheme can be based on: the project's others, then the user's, then the built-in ones.
pub(crate) fn bases<'a>(project: &'a Project, user: &'a SchemeStore, i: usize) -> Vec<&'a m::ColorScheme> {
	let others = project.color_schemes.iter().enumerate().filter(|&(k, _)| k != i).map(|(_, s)| s);
	others.chain(&user.user).chain(waveduderust::io::schemes::built_in()).collect()
}

fn show_scheme(form: &Form<'_>, project: &Project, i: usize) {
	let scheme = &project.color_schemes[i];
	put(form.get_scheme(), Data::show(scheme), |d| form.set_scheme(d));
	put_rows(&form.get_stops(), &scheme.stops, |m| form.set_stops(m));
	form.set_gradient(strip(scheme));
}

/// Shows the schemes scheme `i` can be based on, rebuilt only when they differ from `shown`, the last ones shown;
/// whether they did.
pub(crate) fn show_bases(
	form: &Form<'_>,
	project: &Project,
	user: &SchemeStore,
	i: usize,
	shown: &mut Vec<m::ColorScheme>,
) -> bool {
	let now = bases(project, user, i);
	if shown.len() == now.len() && shown.iter().zip(&now).all(|(a, b)| a == *b) {
		return false;
	}
	let choices: Vec<ui::SchemeChoice> = now
		.iter()
		.map(|s| ui::SchemeChoice { name: s.name.as_str().into(), strip: strip(s), ..Default::default() })
		.collect();
	form.set_bases(ModelRc::new(VecModel::from(choices)));
	*shown = now.into_iter().cloned().collect();
	true
}

/// The scheme `name` resolves to, the project's, the user's or a built-in one, and whether it is the project's.
pub(crate) fn find_scheme<'a>(
	project: &'a Project,
	user: &'a SchemeStore,
	name: &str,
) -> Option<(&'a m::ColorScheme, bool)> {
	use waveduderust::io::schemes::{built_in, find};
	find(&project.color_schemes, name)
		.map(|s| (s, true))
		.or_else(|| find(&user.user, name).or_else(|| find(built_in(), name)).map(|s| (s, false)))
}

/// A Display layer picker's schemes: the project's, then the user's and the built-in ones of `kind` or of none, each
/// name once, the user's before a built-in one of the same name; whether each is the project's.
pub(crate) fn layer_schemes<'a>(
	project: &'a Project,
	user: &'a SchemeStore,
	kind: m::SchemeKind,
) -> Vec<(&'a m::ColorScheme, bool)> {
	let mut schemes: Vec<(&m::ColorScheme, bool)> = project.color_schemes.iter().map(|s| (s, true)).collect();
	let shared = user.user.iter().chain(waveduderust::io::schemes::built_in());
	for s in shared.filter(|s| s.kind == kind || s.kind == m::SchemeKind::Unspecified) {
		if !schemes.iter().any(|(t, own)| !own && t.name.eq_ignore_ascii_case(&s.name)) {
			schemes.push((s, false));
		}
	}
	schemes
}

/// What a layer picker shows: its schemes and the layer's scheme name.
pub(crate) type LayerPicker = (Vec<(m::ColorScheme, bool)>, String);

/// Shows layer `layer`'s picker when its schemes or its name changed since `shown`: a name no scheme has first, the
/// copy action last.
pub(crate) fn show_layer_schemes(
	form: &Form<'_>,
	project: &Project,
	user: &SchemeStore,
	view: &View,
	layer: ui::Layer,
	shown: &mut Option<LayerPicker>,
) {
	let (settings, kind) = match layer {
		ui::Layer::Field => (&view.field, m::SchemeKind::Field),
		_ => (&view.intensity, m::SchemeKind::Intensity),
	};
	let name = &settings.color_scheme;
	let now = layer_schemes(project, user, kind);
	if shown.as_ref().is_some_and(|(schemes, n)| {
		n == name && schemes.len() == now.len() && schemes.iter().zip(&now).all(|(a, b)| a.0 == *b.0 && a.1 == b.1)
	}) {
		return;
	}
	let resolved = find_scheme(project, user, name).map(|(s, _)| s);
	let mut current = now.iter().position(|(s, _)| resolved.is_some_and(|r| std::ptr::eq(*s, r)));
	let mut choices: Vec<ui::SchemeChoice> = now
		.iter()
		.map(|(s, own)| ui::SchemeChoice {
			name: s.name.as_str().into(),
			strip: strip(s),
			entry: if *own { ui::SchemeEntry::Project } else { ui::SchemeEntry::Scheme },
		})
		.collect();
	if current.is_none() && !name.is_empty() {
		choices.insert(
			0,
			ui::SchemeChoice { name: name.as_str().into(), entry: ui::SchemeEntry::Missing, ..Default::default() },
		);
		current = Some(0);
	}
	if resolved.is_some() {
		choices.push(ui::SchemeChoice { entry: ui::SchemeEntry::EditCopy, ..Default::default() });
	}
	let current = current.map_or(-1, index);
	let model = ModelRc::new(VecModel::from(choices));
	match layer {
		ui::Layer::Field => {
			form.set_field_schemes(model);
			form.set_field_scheme(current);
		}
		_ => {
			form.set_intensity_schemes(model);
			form.set_intensity_scheme(current);
		}
	}
	*shown = Some((now.into_iter().map(|(s, own)| (s.clone(), own)).collect(), name.clone()));
}

/// The stop indices of `stops` in gradient order, stable.
fn gradient_order(stops: &[f64]) -> Vec<usize> {
	let mut order: Vec<usize> = (0..stops.len()).collect();
	order.sort_by(|&a, &b| stops[a].total_cmp(&stops[b]));
	order
}

fn positions(stops: &ModelRc<ui::StopData>) -> Vec<f64> {
	stops.iter().map(|s| f64::from(s.at)).collect()
}

fn index(k: usize) -> i32 {
	i32::try_from(k).unwrap_or(-1)
}

/// Form.marker-at: the stop nearest `at` within `reach`, the selected one when as near; -1 for none.
pub(crate) fn marker_at(stops: &ModelRc<ui::StopData>, at: f32, reach: f32, selected: i32) -> i32 {
	let distance = |s: &ui::StopData| (s.at - at).abs();
	let mut nearest = None;
	let mut best = reach;
	for (k, s) in stops.iter().enumerate() {
		if distance(&s) <= best {
			(nearest, best) = (Some(k), distance(&s));
		}
	}
	let selected_wins =
		usize::try_from(selected).ok().and_then(|k| stops.row_data(k)).is_some_and(|s| distance(&s) <= best);
	if selected_wins { selected } else { nearest.map_or(-1, index) }
}

/// Form.stop-beside: the stop `step` places after `k` along the gradient, clamped; from -1, the first.
pub(crate) fn stop_beside(stops: &ModelRc<ui::StopData>, k: i32, step: i32) -> i32 {
	let order = gradient_order(&positions(stops));
	let Some(last) = order.len().checked_sub(1) else { return -1 };
	let place = usize::try_from(k).ok().and_then(|k| order.iter().position(|&o| o == k));
	let to = match place {
		Some(p) => p.saturating_add_signed(step as isize).min(last),
		None => 0,
	};
	index(order[to])
}

/// Removes stop `k` of `scheme`; the stop then selected: the one taking its place along the gradient, else the last.
pub(crate) fn remove_stop(scheme: &mut m::ColorScheme, k: usize) -> i32 {
	let ats = |s: &m::ColorScheme| s.stops.iter().map(|s| s.at).collect::<Vec<_>>();
	let place = gradient_order(&ats(scheme)).iter().position(|&o| o == k);
	if !scheme.remove_stop(k) {
		return index(k);
	}
	let order = gradient_order(&ats(scheme));
	place.and_then(|p| order.get(p.min(order.len() - 1))).map_or(-1, |&o| index(o))
}

/// A position along the gradient set by the pointer or the keys: thousandths, as Next keeps them.
pub(crate) fn thousandths(at: f32) -> f64 {
	(crate::number::wide(at) * 1000.0).round_ties_even() / 1000.0
}

/// Copies `base`'s stops and kind into `scheme`, its description naming it.
pub(crate) fn base_on(scheme: &mut m::ColorScheme, base: &m::ColorScheme) {
	scheme.stops.clone_from(&base.stops);
	scheme.kind = base.kind;
	scheme.description = format!("copy of scheme '{}'", base.name);
}

/// Shows `rows` in the list model `shown`, in place: a row's box keeps text typed into it while another row is
/// added or removed.
fn put_rows<M, U>(shown: &ModelRc<U>, rows: &[M], set: impl FnOnce(ModelRc<U>))
where
	U: Data<M> + PartialEq + Clone + 'static,
{
	let rows: Vec<U> = rows.iter().map(Data::show).collect();
	let Some(model) = shown.as_any().downcast_ref::<VecModel<U>>() else {
		set(ModelRc::new(VecModel::from(rows)));
		return;
	};
	while model.row_count() > rows.len() {
		model.remove(model.row_count() - 1);
	}
	for (k, row) in rows.into_iter().enumerate() {
		match model.row_data(k) {
			Some(old) if old == row => {}
			Some(_) => model.set_row_data(k, row),
			None => model.push(row),
		}
	}
}

fn apply_rows<M, U: Data<M>>(shown: &ModelRc<U>, rows: &mut [M]) {
	for (row, data) in rows.iter_mut().zip(shown.iter()) {
		data.apply(&Data::show(&*row), row);
	}
}

/// An edit of a part's list, by row index.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RowEdit {
	Add,
	Remove(usize),
	Move { from: usize, to: usize },
	Clear,
}

/// Applies `edit` to the list `node` keeps: an outline's points, a filter's layers, a fibre's nodes, a scheme's stops.
pub(crate) fn edit_rows(project: &mut Project, node: Node, edit: RowEdit) {
	let kind = match node {
		Node::Part(i) => match project.parts.get_mut(i) {
			Some(part) => &mut part.kind,
			None => return,
		},
		Node::Scheme(i) => {
			if let Some(scheme) = project.color_schemes.get_mut(i) {
				match edit {
					RowEdit::Add => {
						scheme.add_stop();
					}
					RowEdit::Remove(k) => {
						scheme.remove_stop(k);
					}
					RowEdit::Move { .. } | RowEdit::Clear => {}
				}
			}
			return;
		}
		_ => return,
	};
	match kind {
		PartKind::Outline(o) => edit.on(&mut o.points, add_point),
		PartKind::InterferenceFilter(f) => edit.on(&mut f.layers, |l| l.push(l.last().cloned().unwrap_or_default())),
		PartKind::Fiber(f) if matches!(edit, RowEdit::Add) => f.add_node(),
		PartKind::Fiber(f) => edit.on(&mut f.nodes, |_| {}),
		_ => {}
	}
}

impl RowEdit {
	fn on<T>(self, rows: &mut Vec<T>, add: impl FnOnce(&mut Vec<T>)) {
		match self {
			Self::Add => add(rows),
			Self::Remove(k) if k < rows.len() => {
				rows.remove(k);
			}
			Self::Move { from, to } if from < rows.len() && to < rows.len() => {
				let row = rows.remove(from);
				rows.insert(to, row);
			}
			Self::Clear => rows.clear(),
			Self::Remove(_) | Self::Move { .. } => {}
		}
	}
}

/// Appends a point to an outline a step right of its last one.
fn add_point(points: &mut Vec<Point>) {
	const STEP: i32 = 20;
	let last = points.last().copied();
	points.push(last.map_or(Point::new(0, 0), |p| Point::new(p.x + STEP, p.y)));
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn applying_writes_only_the_changed_fields() {
		let mut sim = m::Simulation::default();
		sim.wave_speed = 0.123_456_789_123;
		let shown: ui::SimulationData = Data::show(&sim);
		let edited = ui::SimulationData { steps: 42, ..shown.clone() };
		edited.apply(&shown, &mut sim);
		assert_eq!((sim.steps, sim.wave_speed), (42, 0.123_456_789_123));
		// a decimal as typed, not as f32 holds it
		ui::SimulationData { time_decay: 0.1, ..shown.clone() }.apply(&shown, &mut sim);
		assert_eq!(sim.time_decay, 0.1);
	}

	#[test]
	fn row_edits_move_add_and_ignore_rows_out_of_range() {
		let mut rows = vec![1, 2, 3];
		RowEdit::Move { from: 0, to: 2 }.on(&mut rows, |_| {});
		assert_eq!(rows, [2, 3, 1]);
		RowEdit::Move { from: 2, to: 3 }.on(&mut rows, |_| {});
		RowEdit::Remove(3).on(&mut rows, |_| {});
		assert_eq!(rows, [2, 3, 1]);
		let mut filter = m::InterferenceFilter::default();
		let mut project = Project::default();
		project.parts.push(Part::new(PartKind::InterferenceFilter(filter.clone()), 0, 0));
		edit_rows(&mut project, Node::Part(0), RowEdit::Add);
		filter.layers.push(m::FilterLayer::default());
		assert_eq!(project.parts[0].kind, PartKind::InterferenceFilter(filter));
	}

	fn stops(ats: &[f32]) -> ModelRc<ui::StopData> {
		ModelRc::new(VecModel::from(
			ats.iter().map(|&at| ui::StopData { at, ..Default::default() }).collect::<Vec<_>>(),
		))
	}

	#[test]
	fn the_bar_finds_markers_and_neighbours_along_the_gradient() {
		let s = stops(&[0.0, 0.8, 0.5]);
		assert_eq!(marker_at(&s, 0.52, 0.05, -1), 2);
		assert_eq!(marker_at(&s, 0.3, 0.05, -1), -1);
		assert_eq!(stop_beside(&s, 0, 1), 2);
		assert_eq!(stop_beside(&s, 2, 1), 1);
		assert_eq!(stop_beside(&s, 1, 5), 1);
		assert_eq!(stop_beside(&s, -1, 0), 0);
	}

	#[test]
	fn a_layer_picker_lists_the_projects_then_the_shared_schemes_of_its_kind_once() {
		let named = |name: &str, kind| m::ColorScheme::default().with_name(name.into()).with_kind(kind);
		let mut project = Project::default();
		project.color_schemes.push(named("Mine", m::SchemeKind::Intensity));
		let user = SchemeStore {
			user: vec![named("Inferno", m::SchemeKind::Field), named("Copper", m::SchemeKind::Intensity)],
		};
		let names: Vec<(&str, bool)> = layer_schemes(&project, &user, m::SchemeKind::Field)
			.into_iter()
			.map(|(s, own)| (s.name.as_str(), own))
			.collect();
		assert_eq!(names[..2], [("Mine", true), ("Inferno", false)]);
		assert_eq!(names.iter().filter(|(n, _)| *n == "Inferno").count(), 1);
		assert!(!names.iter().any(|(n, _)| *n == "Copper"));
		assert_eq!(find_scheme(&project, &user, "mine").map(|(s, own)| (s.name.as_str(), own)), Some(("Mine", true)));
	}

	#[test]
	fn a_removed_stops_place_goes_to_the_next_along_the_gradient() {
		let stop = |at| m::Stop::new(at, m::Rgb::default());
		let mut scheme = m::ColorScheme::default().with_stops(vec![stop(0.0), stop(1.0), stop(0.5)]);
		assert_eq!(remove_stop(&mut scheme, 2), 1);
		assert_eq!(remove_stop(&mut scheme, 1), 0);
		assert_eq!(remove_stop(&mut scheme, 0), 0);
		assert_eq!(scheme.stops.len(), 1);
	}

	#[test]
	fn an_optional_step_below_zero_is_none() {
		assert_eq!(<i32 as Bridge<Option<u32>>>::model(&-1), None);
		assert_eq!(<i32 as Bridge<Option<u32>>>::show(&None), -1);
		assert_eq!(<i32 as Bridge<Option<u32>>>::model(&5), Some(5));
	}
}
