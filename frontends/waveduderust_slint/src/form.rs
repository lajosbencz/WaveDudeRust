//! The typed forms (ui/forms.slint): an object as the Slint data its form binds (ui/data.slint), and a form's edits
//! applied back field by field.

use slint::{Model, ModelRc, SharedString, VecModel};
use waveduderust::model::edited::Edited;
use waveduderust::model::lens_geometry::{edge_thickness, fit_radii_zoned};
use waveduderust::model::{
	self as m, Coating, Lens, Material, PROJECT_VERSION, Part, PartKind, Point, Project, Stencil, View, wavelength,
};

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
	FacetData = m::FacettedGrating { length, width, pattern, pitch, amplitude }
	GratingData = m::SimpleGrating { columns, rows, column_spacing, row_spacing, diameter }
	GrinData = m::GrinLens { centre_index, edge_index, length, diameter }
	DiffuserData = m::Diffuser { length, width, center_wavelength, amplitude, sigma }
	ZonePlateData = m::ZonePlate { mount, mount_width, diameter, thickness, coating, design_frequency, focal_length, inverse }
	PointData = Point { x, y }
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
			Some(PartKind::ZonePlate(_)) => FormKind::ZonePlate,
			Some(PartKind::BraggReflector(_)) => FormKind::BraggReflector,
			Some(PartKind::GrinLens(_)) => FormKind::GrinLens,
			Some(PartKind::SimpleGrating(_)) => FormKind::SimpleGrating,
			Some(PartKind::FacettedGrating(_)) => FormKind::FacettedGrating,
			_ => FormKind::None,
		},
		Node::Project => FormKind::Project,
		Node::Scene => FormKind::Scene,
		Node::Simulation => FormKind::Simulation,
		Node::Display => FormKind::Display,
		Node::Source(_) => FormKind::Source,
		Node::Probe(_) => FormKind::Probe,
		Node::Sources | Node::Parts | Node::Probes | Node::Schemes | Node::Scheme(_) => FormKind::None,
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
		PartKind::SimpleGrating(p) => Some((&p.material, &p.coating)),
		PartKind::FacettedGrating(p) => Some((&p.material, &p.coating)),
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
		PartKind::SimpleGrating(p) => Some((&mut p.material, &mut p.coating)),
		PartKind::FacettedGrating(p) => Some((&mut p.material, &mut p.coating)),
		_ => None,
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
		Node::Sources | Node::Parts | Node::Probes | Node::Schemes | Node::Scheme(_) => {}
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
		PartKind::Outline(o) => {
			// in place: a row's box keeps text typed into it while another row is added or removed
			let points: Vec<ui::PointData> = o.points.iter().map(Data::show).collect();
			let shown = form.get_points();
			match shown.as_any().downcast_ref::<VecModel<ui::PointData>>() {
				Some(model) => {
					while model.row_count() > points.len() {
						model.remove(model.row_count() - 1);
					}
					for (k, point) in points.into_iter().enumerate() {
						match model.row_data(k) {
							Some(old) if old == point => {}
							Some(_) => model.set_row_data(k, point),
							None => model.push(point),
						}
					}
				}
				None => form.set_points(ModelRc::new(VecModel::from(points))),
			}
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
			apply_part(form, part);
			part.edited(&before);
		}
		Node::Display | Node::Sources | Node::Parts | Node::Probes | Node::Schemes | Node::Scheme(_) => {}
	}
}

fn apply_part(form: &Form<'_>, part: &mut Part) {
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
		PartKind::GrinLens(g) => {
			form.get_coating().apply(&Data::show(&g.coating), &mut g.coating);
			form.get_grin().apply(&Data::show(&*g), g);
		}
		PartKind::ZonePlate(z) => form.get_zone_plate().apply(&Data::show(&*z), z),
		PartKind::Outline(o) => {
			for (point, data) in o.points.iter_mut().zip(form.get_points().iter()) {
				data.apply(&Data::show(&*point), point);
			}
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

/// Appends a point to an outline a step right of its last one.
pub(crate) fn add_point(points: &mut Vec<Point>) {
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
	fn an_optional_step_below_zero_is_none() {
		assert_eq!(<i32 as Bridge<Option<u32>>>::model(&-1), None);
		assert_eq!(<i32 as Bridge<Option<u32>>>::show(&None), -1);
		assert_eq!(<i32 as Bridge<Option<u32>>>::model(&5), Some(5));
	}
}
