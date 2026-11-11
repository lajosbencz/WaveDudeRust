//! The forms of docs/ui/30, row by row, on the editors of [`super::properties`].

use std::path::{Path, PathBuf};
use std::rc::Rc;
use wavedude_widgets::Button;

use freya::prelude::*;
use wavedude_core::io::{resolve_image_path, schemes};
use wavedude_core::model::ranges;
use wavedude_core::model::{
	self as m, BoxSize, BraggReflector, Coating, ColorScheme, Diffuser, Display, DisplayLayer, Ellipse,
	FacettedGrating, Fiber, FiberNode, Field as _, Fields, FilterLayer, FresnelLens, GrinLens, InterferenceFilter,
	Layer, Lens, LogicGate, Material, Measurement, MetaAxiconDesign, MetaDesign, MetaLensDesign, MetaPrismDesign,
	MetaSurface, Number, Outline, Part, PartBase, PartKind, Point, Polygon, Probe, ProbeDetector, ProbeKind, Project,
	ProjectEdit, Prop, Rectangle, Scene, SceneMode, SchemeKind, SimpleGrating, Simulation, Source, SourceArray,
	SourceMode, Stencil, Triangle, TriangleType, Video, YScale, ZonePlate,
};
use wavedude_core::scene::gate_design::{self, GateDesign};
use wavedude_widgets::icon::{Icon, icons};
use wavedude_widgets::{
	ColorButton, DescriptionHeader, Gradient, GradientEditor, GradientStop, IconView, NumberSpec, PickerTips,
	PropertyRow, Rgb, SectionHeader, TestIdExt, TextBox, Tip, use_wave_theme,
};

use wavedude_app_state::commands::{Command, Edit, NewPart};
use wavedude_app_state::selection::{ItemKind, Row};
use wavedude_app_state::{Event as AppEvent, Guarded};

use super::dialogs;
use super::properties::{
	Form, List, Object, Scope, Snapshot, bounds_of, check_box, combo_box, readout, swatched_combo_box, tagged,
};
use super::shared::App;

/// A meta surface's read-outs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MetaReadouts {
	pub full_turn: Option<f64>,
	pub zones: u32,
	/// A prism's or an axicon's.
	pub zone_width: Option<f64>,
	/// An axicon's line of focus.
	pub line: Option<f64>,
}

/// A scheme a layer's picker lists.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemeEntry {
	pub name: String,
	/// The project's own.
	pub own: bool,
	pub stops: Vec<m::Stop>,
}

/// A logic gate's design as its form shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateReadouts {
	/// Core width, coupler gap, coupler length, total length.
	pub layout: [i64; 4],
	pub ports: [Point; 4],
	pub quarter_wave: f64,
	/// The port spacing the design uses: the stored one raised to the design's minimum.
	pub spacing: i64,
}

/// Values the forms show that the edited object alone does not hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Readouts {
	pub wavelength: Option<f64>,
	pub nominal: bool,
	pub lens_edge: Option<f64>,
	pub apex: Option<f64>,
	pub gate: Option<GateReadouts>,
	pub zones: Option<(i64, f64)>,
	/// A diffuser's roughness: the face's height from crest to trough and its number of ripples.
	pub roughness: Option<(f64, usize)>,
	/// An interference filter's stack thickness.
	pub stack: Option<f64>,
	/// A simple grating's largest dot diameter.
	pub max_diameter: Option<u32>,
	pub meta: Option<MetaReadouts>,
	pub image_found: bool,
	/// Width, height and size in KB of the embedded image.
	pub image_size: Option<(u32, u32, usize)>,
	/// Scheme names for each layer's picker: (name, project's own).
	pub schemes: [Vec<SchemeEntry>; 2],
	/// For the scheme form's Based on picker.
	pub based_on: Vec<String>,
	pub can_remove_stop: bool,
}

fn zones(z: &ZonePlate, sim: &Simulation) -> Option<(i64, f64)> {
	let lambda = m::wavelength(z.design_frequency, sim.wave_speed, sim.source_frequency_scale);
	if !lambda.is_finite() || lambda <= 0. {
		return None;
	}
	let f = f64::from(z.focal_length);
	let radius = |n: f64| (n * lambda * f + (n * lambda / 2.).powi(2)).sqrt();
	let r = f64::from(z.diameter) / 2.;
	let a = lambda * lambda / 4.;
	let b = lambda * f;
	// the zone index at the rim
	let n = ((-b + (b * b + 4. * a * r * r).sqrt()) / (2. * a)).floor() as i64;
	let outer = if n >= 1 { radius(n as f64) - radius(n as f64 - 1.) } else { r };
	Some((n + 1, outer))
}

fn edge_thickness(l: &Lens, zones: u32, division: m::ZoneDivision) -> f64 {
	m::lens_geometry::edge_thickness(l.diameter, l.thickness, l.r1, l.r2, zones, division)
}

/// The schemes a layer's picker lists: the project's, the user's, the built-in ones.
fn scheme_names(project: &Project, user: &[ColorScheme], kind: SchemeKind) -> Vec<SchemeEntry> {
	let fits = |k: SchemeKind| k == kind || k == SchemeKind::Unspecified;
	let entry = |s: &ColorScheme, own| SchemeEntry { name: s.name.clone(), own, stops: s.stops.clone() };
	let mut v: Vec<SchemeEntry> =
		project.color_schemes.iter().filter(|s| fits(s.kind)).map(|s| entry(s, true)).collect();
	for s in user.iter().chain(schemes::built_in()).filter(|s| fits(s.kind)) {
		if !v.iter().any(|e| e.name.eq_ignore_ascii_case(&s.name)) {
			v.push(entry(s, false));
		}
	}
	v
}

/// The read-outs of a row.
pub fn readouts(app: &App, row: Row) -> Readouts {
	let p = &app.document.project;
	let sim = &p.simulation;
	let mut r = Readouts::default();
	let nominal = sim.stencil == Stencil::NinePoint;
	match row {
		Row::Scene => {
			r.image_found = !p.scene.image_path.is_empty()
				&& resolve_image_path(app.document.folder(), &p.scene.image_path).is_file();
			r.image_size =
				p.scene.image_data.as_ref().map(|b| (p.scene.width, p.scene.height, (b.len() / 1024).max(1)));
		}
		Row::Display => {
			let user = &app.schemes.user;
			r.schemes = [scheme_names(p, user, SchemeKind::Field), scheme_names(p, user, SchemeKind::Intensity)];
		}
		Row::Item(i) => match i.kind {
			ItemKind::Source => {
				let s = &p.sources[i.index];
				r.wavelength = Some(m::wavelength(s.frequency, sim.wave_speed, sim.source_frequency_scale));
				r.nominal = nominal;
			}
			ItemKind::Part => match &p.parts[i.index].kind {
				PartKind::Lens(l) => r.lens_edge = Some(edge_thickness(l, 1, m::ZoneDivision::EqualWidth)),
				PartKind::FresnelLens(f) => r.lens_edge = Some(edge_thickness(&f.lens, f.zones, f.division)),
				PartKind::Diffuser(d) => {
					let profile = d.profile(&p.parts[i.index].base.name, p.seed);
					let (lo, hi) =
						profile.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
					r.roughness = Some((hi - lo, d.components().len()));
				}
				PartKind::Triangle(t) => r.apex = Some(180. - t.angle_a - t.angle_b),
				PartKind::LogicGate(g) => {
					r.wavelength = gate_design::gate_wavelength(g.design_frequency, sim.source_frequency_scale);
					if let Some(d) = GateDesign::for_gate(g, sim) {
						r.gate = Some(GateReadouts {
							layout: [d.core_width, d.coupled, d.coupler_len, d.length],
							ports: gate_design::ports_of(&d, &p.parts[i.index].base),
							quarter_wave: d.wavelength / (4. * d.core_index.sqrt()),
							spacing: d.port_spacing,
						});
					}
				}
				PartKind::ZonePlate(z) => {
					r.wavelength = Some(m::wavelength(z.design_frequency, sim.wave_speed, sim.source_frequency_scale));
					r.zones = zones(z, sim);
				}
				PartKind::InterferenceFilter(f) => r.stack = Some(f.stack_thickness()),
				PartKind::SimpleGrating(g) => r.max_diameter = Some(g.max_diameter()),
				PartKind::MetaSurface(m) => {
					let scale = sim.source_frequency_scale;
					r.wavelength = Some(m::wavelength(m.design_frequency, 1.0, scale));
					r.meta = Some(MetaReadouts {
						full_turn: m.full_turn_height(scale),
						zones: m.zone_count(scale),
						zone_width: m.zone_width(scale),
						line: m.line_length(),
					});
				}
				PartKind::Polygon(_)
				| PartKind::Rectangle(_)
				| PartKind::Ellipse(_)
				| PartKind::Outline(_)
				| PartKind::FacettedGrating(_)
				| PartKind::Fiber(_)
				| PartKind::BraggReflector(_)
				| PartKind::GrinLens(_) => {}
			},
			ItemKind::Scheme => {
				let own = &p.color_schemes[i.index];
				r.based_on = schemes::built_in()
					.iter()
					.chain(&app.schemes.user)
					.chain(p.color_schemes.iter().filter(|s| s.name != own.name))
					.map(|s| s.name.clone())
					.collect();
				r.can_remove_stop = own.stops.len() > 1;
			}
			ItemKind::Probe => {}
		},
		Row::Project | Row::Simulation | Row::Folder(_) => {}
	}
	r
}

fn section(title: &str, rows: Vec<PropertyRow>) -> Element {
	SectionHeader::new(title).children(rows.into_iter().map(IntoElement::into_element)).into_element()
}

/// At most one decimal: "12 px", "12.5 px".
fn px(v: f64) -> String {
	format!("{} px", one_decimal(v))
}

fn one_decimal(v: f64) -> String {
	let text = format!("{v:.1}");
	text.strip_suffix(".0").map_or_else(|| text.clone(), str::to_owned)
}

/// An estimate: "~ 32.0 px".
fn approx_px(v: f64) -> String {
	format!("~ {v:.1} px")
}

/// Every row of the current form.
pub fn build(f: &Form, row: Row, snap: &Snapshot) -> Vec<Element> {
	let r = &snap.readouts;
	let Some(object) = &snap.object else { return Vec::new() };
	match (&**object, row) {
		(Object::Project(o), _) => project(f, o),
		(Object::Scene(o), _) => scene(f, o, r),
		(Object::Simulation(o), _) => simulation(f, o),
		(Object::Display(o), _) => display(f, o, r),
		(Object::Source(o), Row::Item(i)) => source(f, o, i.index, r),
		(Object::Part(o), Row::Item(i)) => part(f, o, i.index, r, snap.description_open),
		(Object::Probe(o), Row::Item(i)) => probe(f, o, i.index),
		(Object::Scheme(o), Row::Item(i)) => scheme(f, o, i.index, r),
		_ => unreachable!("an object belongs to its row"),
	}
}

fn project(f: &Form, obj: &Project) -> Vec<Element> {
	let s = &f.s;
	let p = f.scope(obj, Edit::Project);
	let new_seed = {
		let set = p.setter(Project::SEED);
		move |_: Event<PressEventData>| {
			let nanos =
				std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
			set((nanos & 0x7fff_ffff) as i32);
		}
	};
	let seed = p.number_beside(
		Project::SEED,
		NumberSpec::integer(f64::from(i32::MIN), f64::from(i32::MAX)),
		tagged(dialogs::button(s.get("ButtonNewSeed"), true, new_seed), "ProjectNewSeed"),
	);
	vec![section(
		s.get("SectionGeneral"),
		vec![
			readout(s.get("LabelProjectVersion"), m::PROJECT_VERSION.to_string(), "ProjectVersion"),
			seed,
			p.text(Project::DESCRIPTION, true),
		],
	)]
}

/// The image's name, editable as Next's, and its browse, save and remove buttons.
fn image_buttons(f: &Form, sc: &Scope<'_, Scene>, has_bytes: bool) -> Element {
	let s = f.s;
	let name = {
		let set = sc.setter(Scene::IMAGE);
		let editor = TextBox::new(sc.obj.image.clone(), move |t: String| set(t)).live();
		rect().width(Size::fill()).test_id("SceneImageName").child(editor)
	};
	let pick = {
		let f = f.clone();
		move |_: Event<PressEventData>| {
			let cx = &f.cx;
			let start = cx.shared.peek(|a| a.document.folder().map(Path::to_owned));
			dialogs::pick_file_for_project(cx, &dialogs::scene_image_filter(&s), start, move |cx, path: PathBuf| {
				match std::fs::read(&path) {
					Ok(bytes) => {
						let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
						cx.send(AppEvent::Command(Command::SetSceneImage {
							name,
							path: path.display().to_string(),
							bytes,
						}));
					}
					Err(e) => tracing::warn!("the image '{}' could not be read: {e}", path.display()),
				}
			});
		}
	};
	let save = {
		let f = f.clone();
		move |_: Event<PressEventData>| save_image(&f)
	};
	let remove = {
		let f = f.clone();
		move |_: Event<PressEventData>| f.cx.send(AppEvent::Command(Command::RemoveSceneImage))
	};
	let glyph = |icon| IconView::new(icon).into_element();
	rect()
		.horizontal()
		.spacing(4.)
		.width(Size::fill())
		.content(Content::flex())
		.cross_align(Alignment::center())
		.child(rect().width(Size::flex(1.)).child(name))
		.child(tip(
			s.get("TipBrowseSceneImage"),
			tagged(Button::new().compact().on_press(pick).child(label().text("...")), "SceneImageBrowse"),
		))
		.child(tip(
			s.get("TipSaveSceneImage"),
			tagged(
				Button::new().compact().enabled(has_bytes).on_press(save).child(glyph(super::menus::glyphs::SAVE)),
				"SceneImageSave",
			),
		))
		.child(tip(
			s.get("TipRemoveSceneImage"),
			tagged(
				Button::new().compact().enabled(has_bytes).on_press(remove).child(glyph(icons::CLOSE)),
				"SceneImageRemove",
			),
		))
		.into_element()
}

fn tip(text: &str, child: impl IntoElement) -> Element {
	Tip::new(text).child(child).into_element()
}

fn save_image(f: &Form) {
	let (bytes, name, path, folder) = f.cx.shared.peek(|a| {
		let sc = &a.document.project.scene;
		(sc.image_data.clone(), sc.image.clone(), sc.image_path.clone(), a.document.folder().map(Path::to_owned))
	});
	let Some(bytes) = bytes else { return };
	let ext = Path::new(&name).extension().map_or_else(|| ".png".into(), |e| format!(".{}", e.to_string_lossy()));
	let dir = Path::new(&path).parent().filter(|p| p.is_dir()).map(Path::to_owned).or(folder);
	let proposed = dir.map(|d| d.join(&name));
	let spec = f.s.format("SceneImageSaveFilterFormat", &[&ext]);
	let (cx, s) = (f.cx.clone(), f.s);
	dialogs::save_file(&spec, proposed, move |target| {
		let Some(target) = target else { return };
		if let Err(e) = std::fs::write(&target, &bytes) {
			tracing::warn!("{}: {e}", s.format("SceneImageSaveError", &[&target.display()]));
			cx.ui.show_modal(dialogs::Modal::Error(s.format("SceneImageSaveError", &[&target.display()])));
		}
	});
}

fn scene(f: &Form, o: &Scene, r: &Readouts) -> Vec<Element> {
	let s = &f.s;
	let sc = f.scope(o, |e| Edit::Project(ProjectEdit::Scene(e)));
	let image_mode = o.mode == SceneMode::Image;
	let mut canvas = vec![sc.combo(Scene::MODE)];
	if image_mode {
		canvas.push(
			PropertyRow::new(sc.label(&Scene::IMAGE), image_buttons(f, &sc, r.image_size.is_some()))
				.hint(sc.hint(&Scene::IMAGE)),
		);
		let path = o.image_path.clone();
		let shown =
			if !path.is_empty() && !r.image_found { format!("{path} {}", s.get("SceneImageNotFound")) } else { path };
		canvas.push(readout(sc.label(&Scene::IMAGE_PATH), shown, "SceneImagePath"));
		let size = r.image_size.map_or_else(
			|| s.get("ImageUnreadable").to_owned(),
			|(w, h, kb)| format!("{w} x {h}, {} KB", s.grouped(kb as u64)),
		);
		canvas.push(readout(s.get("LabelEmbeddedImage"), size, "SceneEmbeddedImage").hint(s.get("TipEmbeddedImage")));
	}
	let (smin, smax) = bounds_of(Scene::WIDTH);
	canvas.extend([
		sc.number_with(Scene::WIDTH, NumberSpec::integer(smin, smax)).enabled(!image_mode),
		sc.number_with(Scene::HEIGHT, NumberSpec::integer(smin, smax)).enabled(!image_mode),
		sc.combo(Scene::EDGES),
		sc.number(Scene::DAMPING_EDGE, 1.),
		sc.number(Scene::REFRACTIVE_INDEX, 0.01),
	]);
	vec![
		section(s.get("SectionCanvas"), canvas),
		section(
			s.get("SectionImageSources"),
			vec![
				sc.check(Scene::IMAGE_SOURCES),
				sc.number(Scene::IMAGE_SOURCE_SCALE, 0.1).enabled(o.image_sources),
				sc.check(Scene::OBJECT_SOURCES),
			],
		),
	]
}

fn simulation(f: &Form, o: &Simulation) -> Vec<Element> {
	let s = &f.s;
	let sim = f.scope(o, |e| Edit::Project(ProjectEdit::Simulation(e)));
	let (dmin, dmax) = bounds_of(Simulation::TIME_DECAY);
	vec![
		section(
			s.get("SectionRun"),
			vec![sim.number(Simulation::STEPS, 100.), sim.number(Simulation::VISUALIZE_EVERY, 1.)],
		),
		section(
			s.get("SectionSolver"),
			vec![
				sim.check_as(
					Simulation::STENCIL,
					|s| *s == Stencil::FourthOrder,
					|on| if on { Stencil::FourthOrder } else { Stencil::NinePoint },
				),
				sim.number(Simulation::WAVE_SPEED, 0.01),
				sim.number_with(
					Simulation::TIME_DECAY,
					NumberSpec::linear(dmin, dmax, 1.).with_decimals(1).with_drag_step(0.1),
				),
				sim.number(Simulation::SOURCE_FREQUENCY_SCALE, 0.1),
				sim.number(Simulation::INTENSITY_SCALE, 0.1),
			],
		),
	]
}

/// A display layer's rows; `id` is its colour picker's test id.
fn layer_rows(f: &Form, l: &Scope<'_, DisplayLayer>, id: &str, names: &[SchemeEntry]) -> Vec<PropertyRow> {
	let s = &f.s;
	let current = l.obj.color_scheme.clone();
	let mut items: Vec<(String, String)> = names
		.iter()
		.map(|e| {
			let n = &e.name;
			(n.clone(), if e.own { format!("{n} ({})", s.get("PickerProjectTag")) } else { n.clone() })
		})
		.collect();
	let swatches = names.iter().map(|e| (e.name.clone(), to_gradient(&e.stops))).collect();
	if !current.is_empty() && !items.iter().any(|(n, _)| *n == current) {
		items.insert(0, (current.clone(), s.format("PickerMissingFormat", &[&current])));
	}
	const BROWSE: &str = "\u{1}browse";
	const EDIT_COPY: &str = "\u{1}copy";
	items.push((BROWSE.into(), s.get("PickerBrowse").to_owned()));
	items.push((EDIT_COPY.into(), s.get("PickerEditCopy").to_owned()));
	let picker = {
		let form = f.clone();
		let current_name = current.clone();
		let set = l.setter(DisplayLayer::COLOR_SCHEME);
		swatched_combo_box(current, items, swatches, true, format!("{id}.ColorScheme"), move |name| {
			if name == BROWSE {
				dialogs::pick_file_for_project(&form.cx, form.s.get("ColorSchemeFilter"), None, move |cx, path| {
					cx.send(AppEvent::Guarded(Guarded::Open(path)));
				});
			} else if name == EDIT_COPY {
				edit_copy(&form, &set, &current_name);
			} else {
				set(name);
			}
		})
	};
	let (bmin, bmax) = bounds_of(DisplayLayer::BRIGHTNESS);
	vec![
		PropertyRow::new(l.label(&DisplayLayer::COLOR_SCHEME), picker).hint(l.hint(&DisplayLayer::COLOR_SCHEME)),
		l.number_with(DisplayLayer::BRIGHTNESS, NumberSpec::logarithmic(bmin, bmax)),
		l.number(DisplayLayer::BLACK_LEVEL, 0.01),
		l.check(DisplayLayer::PREVIEW),
		l.check(DisplayLayer::OVERLAY),
		l.number(DisplayLayer::OVERLAY_GAIN, 0.05),
		l.check(DisplayLayer::OVERLAY_IN_EXPORT),
	]
}

/// Makes a project scheme from the layer's current one (a project scheme is copied) and points the layer at it.
fn edit_copy(f: &Form, set: &impl Fn(String), current: &str) {
	let source = f.cx.shared.peek(|a| {
		let p = &a.document.project;
		schemes::find(&p.color_schemes, current)
			.or_else(|| schemes::find(&a.schemes.user, current))
			.or_else(|| schemes::find(schemes::built_in(), current))
			.cloned()
	});
	let Some(scheme) = source else { return };
	let name_after = f.cx.shared.peek(|a| {
		let taken: Vec<String> = a.document.project.color_schemes.iter().map(|s| s.name.clone()).collect();
		wavedude_core::model::names::copy_name(&scheme.name, taken.iter().map(String::as_str))
	});
	f.cx.send(AppEvent::Command(Command::ImportScheme(Box::new(scheme))));
	set(name_after);
}

fn display(f: &Form, o: &Display, r: &Readouts) -> Vec<Element> {
	let s = &f.s;
	let d = f.scope(o, |e| Edit::Project(ProjectEdit::Display(e)));
	let v = d.nested(Display::VIDEO);
	vec![
		section(s.get("SectionField"), layer_rows(f, &d.nested(Display::FIELD), "field", &r.schemes[0])),
		section(s.get("SectionIntensity"), layer_rows(f, &d.nested(Display::INTENSITY), "intensity", &r.schemes[1])),
		section(
			s.get("SectionPreview"),
			vec![d.check(Display::HANDLES_IN_PREVIEW), d.check(Display::LABELS_IN_PREVIEW)],
		),
		section(
			s.get("SectionExportRange"),
			vec![
				d.number(Display::EXPORT_FROM, 1.),
				d.number(Display::EXPORT_TO, 1.),
				d.check(Display::PROBE_GRAPHS_IN_EXPORT),
				d.check(Display::PROBE_MARKERS_IN_EXPORT),
				d.check(Display::SOURCE_MARKERS_IN_EXPORT),
			],
		),
		section(s.get("SectionVideo"), vec![v.number(Video::FPS, 1.), v.number(Video::QUALITY, 1.)]),
	]
}

fn source(f: &Form, o: &Source, index: usize, r: &Readouts) -> Vec<Element> {
	let s = &f.s;
	let src = f.scope(o, move |e| Edit::Source(index, e));
	let pulsed = o.mode == SourceMode::Pulsed;
	let composite = o.array.is_some();
	// a plain source shows the array's rows greyed and empty
	let plain_array = SourceArray::default();
	let arr = src.variant_or(Source::ARRAY, &plain_array);
	let wavelength = r.wavelength.map_or_else(String::new, |w| {
		let text = if w.is_finite() { approx_px(w) } else { "-".into() };
		if r.nominal { format!("{text} {}", s.get("WavelengthNominal")) } else { text }
	});
	let detach = {
		let value = o.detach_after.map_or(-1., f64::from);
		let set = src.setter(Source::DETACH_AFTER);
		let (_, max) = bounds_of(Source::DETACH_AFTER);
		let editor = wavedude_widgets::NumberBox::new(
			WritableSetter::new(value, move |v: f64| set((v >= 0.).then(|| v.round() as u32))),
			NumberSpec::integer(-1., max),
		)
		.width(Size::fill())
		.test_id("Source.DetachAfter");
		PropertyRow::new(src.label(&Source::DETACH_AFTER), editor).hint(src.hint(&Source::DETACH_AFTER))
	};
	let composite_box = src.check_as(Source::SET_ARRAY, Option::is_some, |on| on.then(SourceArray::default));
	let (amin, amax) = bounds_of(SourceArray::ANGLE);
	vec![
		section(s.get("SectionGeneral"), vec![f.name::<Source>(&o.name), src.check(Source::ENABLED)]),
		section(s.get("SectionPosition"), vec![src.decimals(Source::X, 1., 0), src.decimals(Source::Y, 1., 0)]),
		section(
			s.get("SectionWave"),
			vec![
				src.decimals(Source::FREQUENCY, 0.1, 2),
				src.decimals(Source::AMPLITUDE, 0.1, 2),
				src.decimals(Source::PHASE, 0.5, 2),
				readout(s.get("LabelWavelength"), wavelength, "SourceWavelength").hint(s.get("TipWavelength")),
			],
		),
		section(
			s.get("SectionTiming"),
			vec![
				src.combo(Source::MODE),
				src.combo(Source::PULSE_SHAPE).enabled(pulsed),
				src.number(Source::PULSE_WIDTH, 1.).enabled(pulsed),
				src.number(Source::OFF_WIDTH, 1.).enabled(pulsed),
				detach,
			],
		),
		section(
			s.get("SectionArray"),
			vec![
				composite_box,
				arr.number(SourceArray::SIZE_X, 1.).enabled(composite),
				arr.number(SourceArray::SIZE_Y, 1.).enabled(composite),
				arr.number(SourceArray::SPACING_X, 1.).enabled(composite),
				arr.number(SourceArray::SPACING_Y, 1.).enabled(composite),
				arr.number_with(
					SourceArray::ANGLE,
					NumberSpec::linear(amin, amax, 5.).with_decimals(1).with_drag_step(0.5),
				)
				.enabled(composite),
			],
		),
		section(
			s.get("SectionRandom"),
			[
				SourceArray::RANDOM_FREQUENCY,
				SourceArray::RANDOM_PHASE,
				SourceArray::RANDOM_OFF_WIDTH,
				SourceArray::RANDOM_POSITION,
			]
			.map(|p| arr.number(p, 1.).enabled(composite))
			.into(),
		),
	]
}

/// Which faces a kind offers its coating on: the boxes of the others are not shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CoatedFaces {
	All,
	/// A profiled plate's flat back.
	Back,
	/// A meta surface's ridge tops and plate back.
	FrontBack,
	/// No coating at all (a grating's dots).
	None,
}

/// The coating's rows; greyed when `disabled`.
fn coating_rows(c: &Scope<'_, Coating>, disabled: bool) -> Vec<PropertyRow> {
	coating_rows_of(c, disabled, CoatedFaces::All)
}

fn coating_rows_of(c: &Scope<'_, Coating>, disabled: bool, faces: CoatedFaces) -> Vec<PropertyRow> {
	let (min, max) = bounds_of(Coating::DEPTH);
	let mut rows = vec![c.number_with(Coating::DEPTH, NumberSpec::integer(min, max)), c.combo(Coating::PROFILE)];
	if matches!(faces, CoatedFaces::All | CoatedFaces::FrontBack) {
		rows.push(c.check(Coating::FRONT));
	}
	rows.push(c.check(Coating::BACK));
	if faces == CoatedFaces::All {
		rows.push(c.check(Coating::SIDES));
	}
	rows.into_iter().map(|r| r.enabled(!disabled)).collect()
}

/// The glass or mirror of a lens or shape.
fn material(f: &Form, mat: &Scope<'_, Material>, c: &Scope<'_, Coating>) -> Element {
	material_faces(f, mat, c, CoatedFaces::All)
}

/// [`material`] with the coating offered on `faces` alone.
fn material_faces(f: &Form, mat: &Scope<'_, Material>, c: &Scope<'_, Coating>, faces: CoatedFaces) -> Element {
	let mirror = mat.obj.mirror;
	let mut rows = vec![mat.check(Material::MIRROR), mat.number(Material::REFRACTIVE_INDEX, 0.01).enabled(!mirror)];
	if faces != CoatedFaces::None {
		rows.extend(coating_rows_of(c, mirror, faces));
	}
	rows.push(mat.number(Material::ABSORPTION, 5.).enabled(!mirror));
	rows.push(mat.number(Material::ABSORPTION_RAMP, 5.).enabled(!mirror));
	section(f.s.get("SectionMaterial"), rows)
}

/// The rows every part shares: its description, name, enabled flag and position.
fn part_common(f: &Form, base: &Scope<'_, PartBase>, ty: &str, open: bool) -> Vec<Element> {
	let s = &f.s;
	let expanded = {
		let cx = f.cx.clone();
		WritableSetter::new(open, move |v: bool| cx.shared.update(|a| a.settings.part_description_expanded = v))
	};
	let (amin, amax) = bounds_of(PartBase::ANGLE);
	vec![
		DescriptionHeader::new(s.kind(ty), expanded)
			.title(s.get("SectionDescription"))
			.tooltip(s.get("TipPartDescription"))
			.into_element(),
		section(s.get("SectionGeneral"), vec![f.name::<PartBase>(&base.obj.name), base.check(PartBase::ENABLED)]),
		section(
			s.get("SectionPosition"),
			vec![
				base.number(PartBase::X, 1.),
				base.number(PartBase::Y, 1.),
				base.number_with(
					PartBase::ANGLE,
					NumberSpec::linear(amin, amax, 5.).with_decimals(1).with_drag_step(0.5),
				),
			],
		),
	]
}

fn part(f: &Form, o: &Part, index: usize, r: &Readouts, open: bool) -> Vec<Element> {
	let part = f.scope(o, move |e| Edit::Part(index, e));
	let kind = part.nested(Part::KIND);
	let mut out = part_common(f, &part.nested(Part::BASE), NewPart::of(&o.kind).type_name(), open);
	let shape = |rows: Vec<PropertyRow>| section(f.s.get("SectionShape"), rows);
	match &o.kind {
		PartKind::Lens(inner) => {
			let l = kind.variant_over(PartKind::LENS, inner);
			out.extend([material(f, &l.nested(Lens::MATERIAL), &l.nested(Lens::COATING)), lens(f, r, &l, None)]);
		}
		PartKind::FresnelLens(inner) => {
			let z = kind.variant_over(PartKind::FRESNEL_LENS, inner);
			let l = z.nested(FresnelLens::LENS);
			out.extend([material(f, &l.nested(Lens::MATERIAL), &l.nested(Lens::COATING)), lens(f, r, &l, Some(&z))]);
		}
		PartKind::Polygon(inner) => {
			let p = kind.variant_over(PartKind::POLYGON, inner);
			out.extend([
				material(f, &p.nested(Polygon::MATERIAL), &p.nested(Polygon::COATING)),
				shape(vec![p.number(Polygon::SIDES, 1.), p.number(Polygon::SIDE, 10.)]),
			]);
		}
		PartKind::Rectangle(inner) => {
			let p = kind.variant_over(PartKind::RECTANGLE, inner);
			out.extend([
				material(f, &p.nested(Rectangle::MATERIAL), &p.nested(Rectangle::COATING)),
				shape(vec![linked_sizes(&p, Rectangle::LENGTH, Rectangle::WIDTH, Rectangle::LINKED)]),
			]);
		}
		PartKind::Ellipse(inner) => {
			let p = kind.variant_over(PartKind::ELLIPSE, inner);
			out.extend([
				material(f, &p.nested(Ellipse::MATERIAL), &p.nested(Ellipse::COATING)),
				shape(vec![linked_sizes(&p, Ellipse::LENGTH, Ellipse::WIDTH, Ellipse::LINKED)]),
			]);
		}
		PartKind::Triangle(inner) => {
			let t = kind.variant_over(PartKind::TRIANGLE, inner);
			out.extend([material(f, &t.nested(Triangle::MATERIAL), &t.nested(Triangle::COATING)), triangle(f, r, &t)]);
		}
		PartKind::Outline(inner) => {
			let p = kind.variant_over(PartKind::OUTLINE, inner);
			out.extend([
				material(f, &p.nested(Outline::MATERIAL), &p.nested(Outline::COATING)),
				points(f, &p.list(Outline::POINTS)),
			]);
		}
		PartKind::Diffuser(inner) => {
			let d = kind.variant_over(PartKind::DIFFUSER, inner);
			let (height, ripples) = r.roughness.map_or((String::new(), String::new()), |(h, n)| (px(h), n.to_string()));
			out.extend([
				material(f, &d.nested(Diffuser::MATERIAL), &d.nested(Diffuser::COATING)),
				shape(vec![d.number(Diffuser::LENGTH, 10.), d.number(Diffuser::WIDTH, 10.)]),
				section(
					f.s.get("SectionRoughness"),
					vec![
						d.decimals(Diffuser::CENTER_WAVELENGTH, 0.1, 2),
						d.decimals(Diffuser::AMPLITUDE, 0.1, 2),
						d.decimals(Diffuser::SIGMA, 0.1, 2),
						readout(f.s.get("LabelProfileHeight"), height, "ProfileHeight")
							.hint(f.s.get("TipProfileHeight")),
						readout(f.s.get("LabelComponentCount"), ripples, "ComponentCount")
							.hint(f.s.get("TipComponentCount")),
					],
				),
			]);
		}
		PartKind::InterferenceFilter(inner) => {
			let i = kind.variant_over(PartKind::INTERFERENCE_FILTER, inner);
			out.extend([
				material_faces(
					f,
					&i.nested(InterferenceFilter::MATERIAL),
					&i.nested(InterferenceFilter::COATING),
					CoatedFaces::Back,
				),
				shape(vec![
					i.number(InterferenceFilter::LENGTH, 10.),
					i.number(InterferenceFilter::WIDTH, 10.),
					readout(f.s.get("LabelStackThickness"), r.stack.map(px).unwrap_or_default(), "StackThickness")
						.hint(f.s.get("TipStackThickness")),
				]),
				layers(f, i.list(InterferenceFilter::LAYERS)),
			]);
		}
		PartKind::SimpleGrating(inner) => {
			let g = kind.variant_over(PartKind::SIMPLE_GRATING, inner);
			let max = f64::from(r.max_diameter.unwrap_or(1));
			out.extend([
				material_faces(
					f,
					&g.nested(SimpleGrating::MATERIAL),
					&g.nested(SimpleGrating::COATING),
					CoatedFaces::None,
				),
				section(
					f.s.get("SectionLattice"),
					vec![
						g.number(SimpleGrating::COLUMNS, 1.),
						g.number(SimpleGrating::ROWS, 1.),
						g.number(SimpleGrating::COLUMN_SPACING, 1.),
						g.number(SimpleGrating::ROW_SPACING, 1.),
						g.number_with(SimpleGrating::DIAMETER, NumberSpec::integer(1., max)),
					],
				),
			]);
		}
		PartKind::MetaSurface(inner) => {
			let m = kind.variant_over(PartKind::META_SURFACE, inner);
			out.extend(meta_surface(f, r, &m));
		}
		PartKind::FacettedGrating(inner) => {
			let g = kind.variant_over(PartKind::FACETTED_GRATING, inner);
			out.extend([
				material_faces(
					f,
					&g.nested(FacettedGrating::MATERIAL),
					&g.nested(FacettedGrating::COATING),
					CoatedFaces::Back,
				),
				shape(vec![g.number(FacettedGrating::LENGTH, 10.), g.number(FacettedGrating::WIDTH, 10.)]),
				section(
					f.s.get("SectionFacets"),
					vec![
						g.combo(FacettedGrating::PATTERN),
						g.decimals(FacettedGrating::PITCH, 0.1, 2),
						g.decimals(FacettedGrating::AMPLITUDE, 0.1, 2),
					],
				),
			]);
		}
		PartKind::Fiber(inner) => {
			out.extend(fiber(f, &kind.variant_over(PartKind::FIBER, inner)));
		}
		PartKind::BraggReflector(inner) => {
			let b = kind.variant_over(PartKind::BRAGG_REFLECTOR, inner);
			out.push(section(
				f.s.get("SectionLayers"),
				vec![
					b.number(BraggReflector::PAIRS, 1.),
					b.number(BraggReflector::THICKNESS1, 1.),
					b.number(BraggReflector::INDEX1, 0.01),
					b.number(BraggReflector::THICKNESS2, 1.),
					b.number(BraggReflector::INDEX2, 0.01),
				],
			));
			out.push(shape(vec![b.number(BraggReflector::WIDTH, 10.)]));
		}
		PartKind::GrinLens(inner) => {
			let g = kind.variant_over(PartKind::GRIN_LENS, inner);
			let mut rows = vec![g.number(GrinLens::CENTRE_INDEX, 0.01), g.number(GrinLens::EDGE_INDEX, 0.01)];
			rows.extend(coating_rows(&g.nested(GrinLens::COATING), false));
			out.push(section(f.s.get("SectionMaterial"), rows));
			out.push(shape(vec![g.number(GrinLens::LENGTH, 10.), g.number(GrinLens::DIAMETER, 10.)]));
		}
		PartKind::ZonePlate(inner) => {
			out.extend(zone_plate(f, r, &kind.variant_over(PartKind::ZONE_PLATE, inner)));
		}
		PartKind::LogicGate(inner) => {
			out.extend(gate(f, r, &kind.variant_over(PartKind::LOGIC_GATE, inner)));
		}
	}
	out
}

/// The chain of [`linked_sizes`]: two rings, overlapping when linked.
const CHAIN_LINKED: Icon = Icon::path(
	"M6.5 0.5 h2 a3 3 0 0 1 3 3 v2 a3 3 0 0 1 -3 3 h-2 a3 3 0 0 1 -3 -3 v-2 a3 3 0 0 1 3 -3 z \
	 M6.5 6.5 h2 a3 3 0 0 1 3 3 v2 a3 3 0 0 1 -3 3 h-2 a3 3 0 0 1 -3 -3 v-2 a3 3 0 0 1 3 -3 z",
);
const CHAIN_PARTED: Icon = Icon::path(
	"M6.5 0.5 h2 a3 3 0 0 1 3 3 a3 3 0 0 1 -3 3 h-2 a3 3 0 0 1 -3 -3 a3 3 0 0 1 3 -3 z \
	 M6.5 8.5 h2 a3 3 0 0 1 3 3 a3 3 0 0 1 -3 3 h-2 a3 3 0 0 1 -3 -3 a3 3 0 0 1 3 -3 z",
);

/// Length and Width of a rectangle or ellipse with the chain toggle that links them beside both.
fn linked_sizes<T: Fields>(
	b: &Scope<'_, T>,
	length: Prop<T, u32>,
	width: Prop<T, u32>,
	linked: Prop<T, bool>,
) -> PropertyRow {
	let theme = use_wave_theme();
	let on = b.get(linked);
	let toggle = {
		let set = b.setter(linked);
		move |_: Event<PressEventData>| set(!on)
	};
	let (icon, ink) = if on { (CHAIN_LINKED, theme.accent) } else { (CHAIN_PARTED, theme.text_dim) };
	let chain = tip(
		b.hint(&linked),
		tagged(
			Button::new().flat().compact().on_press(toggle).child(IconView::new(icon).color(ink)),
			Scope::<T>::test_id(&linked),
		),
	);
	let sizes = rect().width(Size::flex(1.)).child(b.number(length, 10.)).child(b.number(width, 10.));
	let row = rect()
		.horizontal()
		.width(Size::fill())
		.content(Content::flex())
		.spacing(ROW_SPACING)
		.cross_align(Alignment::center())
		.child(sizes)
		.child(chain);
	PropertyRow::new("", row).full_width()
}

/// The lens's shape rows; with `fresnel` (the Fresnel lens it belongs to) the zone rows instead of the Concave
/// box, whose flip would hollow the zones.
fn lens(f: &Form, r: &Readouts, l: &Scope<'_, Lens>, fresnel: Option<&Scope<'_, FresnelLens>>) -> Element {
	let s = &f.s;
	let (r1, r2) = (l.obj.r1, l.obj.r2);
	let flip = {
		let (set1, set2) = (l.setter(Lens::R1), l.setter(Lens::R2));
		move |_: bool| {
			set1(-r1);
			set2(-r2);
		}
	};
	let mut rows = vec![l.number(Lens::DIAMETER, 10.)];
	if fresnel.is_none() {
		rows.push(
			PropertyRow::new("", check_box(s.get("LabelLensConcave").into(), r1 < 0., flip).test_id("Lens.IsConcave"))
				.hint(s.get("HintLensConcave"))
				.full_width(),
		);
	}
	rows.extend([l.number(Lens::R1, 10.), l.number(Lens::R2, 10.), l.number(Lens::THICKNESS, 1.)]);
	if let Some(z) = fresnel {
		rows.push(z.number(FresnelLens::ZONES, 1.));
		rows.push(z.combo(FresnelLens::DIVISION));
	}
	rows.push(
		readout(s.get("LabelEdgeThickness"), r.lens_edge.map(px).unwrap_or_default(), "LensEdgeThickness")
			.hint(s.get("TipEdgeThickness")),
	);
	section(s.get("SectionShape"), rows)
}

fn triangle(f: &Form, r: &Readouts, t: &Scope<'_, Triangle>) -> Element {
	let s = &f.s;
	let kind = t.obj.kind;
	let a_on = matches!(kind, TriangleType::Isosceles | TriangleType::Custom);
	section(
		s.get("SectionShape"),
		vec![
			t.combo(Triangle::KIND),
			t.number(Triangle::BASE, 10.),
			t.decimals(Triangle::ANGLE_A, 0.5, 1).enabled(a_on),
			t.decimals(Triangle::ANGLE_B, 0.5, 1).enabled(kind == TriangleType::Custom),
			readout(
				s.get("LabelApexAngle"),
				r.apex.map(|a| format!("{} deg", one_decimal(a))).unwrap_or_default(),
				"TriangleApex",
			)
			.hint(s.get("TipApexAngle")),
		],
	)
}

/// A meta surface's form: the plate's material (tops and back coated), its shape, the design's section (a lens's,
/// a prism's or an axicon's) and the pattern.
fn meta_surface(f: &Form, r: &Readouts, m: &Scope<'_, MetaSurface>) -> Vec<Element> {
	let s = &f.s;
	let design = m.nested(MetaSurface::DESIGN);
	let MetaReadouts { full_turn, zones, zone_width, line } = r.meta.unwrap_or_default();
	let wavelength = readout(s.get("LabelWavelength"), r.wavelength.map(px).unwrap_or_default(), "PartWavelength")
		.hint(s.get("TipGateWavelength"));
	let zones = readout(s.get("LabelMetaZones"), zones.to_string(), "PartZoneCount").hint(s.get("TipMetaZones"));
	let zone_width = readout(s.get("LabelZoneWidth"), zone_width.map(px).unwrap_or_default(), "PartZoneWidth")
		.hint(s.get("TipZoneWidth"));
	let (section_key, rows) = match &design.obj {
		MetaDesign::Lens(inner) => {
			let l = design.variant_over(MetaDesign::LENS, inner);
			(
				"SectionLens",
				vec![
					m.decimals(MetaSurface::DESIGN_FREQUENCY, 0.1, 2),
					wavelength,
					l.number(MetaLensDesign::FOCAL_LENGTH, 10.),
					l.combo(MetaLensDesign::LENS_TYPE),
					zones,
				],
			)
		}
		MetaDesign::Prism(inner) => {
			let p = design.variant_over(MetaDesign::PRISM, inner);
			(
				"SectionPrism",
				vec![
					m.decimals(MetaSurface::DESIGN_FREQUENCY, 0.1, 2),
					wavelength,
					p.decimals(MetaPrismDesign::DEFLECTION, 0.5, 2),
					zone_width,
					zones,
				],
			)
		}
		MetaDesign::Axicon(inner) => {
			let a = design.variant_over(MetaDesign::AXICON, inner);
			(
				"SectionAxicon",
				vec![
					m.decimals(MetaSurface::DESIGN_FREQUENCY, 0.1, 2),
					wavelength,
					a.decimals(MetaAxiconDesign::CONE_ANGLE, 0.5, 2),
					readout(s.get("LabelLineLength"), line.map(px).unwrap_or_default(), "PartLineLength")
						.hint(s.get("TipLineLength")),
					zone_width,
					zones,
				],
			)
		}
	};
	vec![
		material_faces(f, &m.nested(MetaSurface::MATERIAL), &m.nested(MetaSurface::COATING), CoatedFaces::FrontBack),
		section(s.get("SectionShape"), vec![m.number(MetaSurface::LENGTH, 10.), m.number(MetaSurface::WIDTH, 10.)]),
		section(s.get(section_key), rows),
		section(
			s.get("SectionPattern"),
			vec![
				m.number(MetaSurface::PATTERN_INDEX, 0.01),
				m.decimals(MetaSurface::PITCH, 0.1, 2),
				m.decimals(MetaSurface::HEIGHT, 0.1, 2),
				readout(s.get("LabelFullTurnHeight"), full_turn.map(px).unwrap_or_default(), "PartFullTurnHeight")
					.hint(s.get("TipFullTurnHeight")),
			],
		),
	]
}

fn zone_plate(f: &Form, r: &Readouts, z: &Scope<'_, ZonePlate>) -> Vec<Element> {
	let s = &f.s;
	let (count, outer) = r.zones.map_or((String::new(), String::new()), |(n, o)| (n.to_string(), approx_px(o)));
	vec![
		section(
			s.get("SectionDesign"),
			vec![
				z.decimals(ZonePlate::DESIGN_FREQUENCY, 0.1, 2),
				readout(s.get("LabelWavelength"), r.wavelength.map(approx_px).unwrap_or_default(), "ZoneWavelength")
					.hint(s.get("TipGateWavelength")),
				z.number(ZonePlate::FOCAL_LENGTH, 10.),
				z.check(ZonePlate::INVERSE),
			],
		),
		section(
			s.get("SectionShape"),
			vec![
				z.number(ZonePlate::DIAMETER, 10.),
				z.number(ZonePlate::THICKNESS, 1.),
				z.number(ZonePlate::COATING, 1.),
				z.check(ZonePlate::MOUNT),
				z.number(ZonePlate::MOUNT_WIDTH, 10.).enabled(z.obj.mount),
			],
		),
		section(
			s.get("SectionZones"),
			vec![
				readout(s.get("LabelZoneCount"), count, "ZoneCount").hint(s.get("TipZones")),
				readout(s.get("LabelOuterZone"), outer, "ZoneOuter").hint(s.get("TipZones")),
			],
		),
	]
}

/// A number box for one number of item `index` of `list`, for the caller to tip.
fn list_value<T: Fields, U: Fields + Clone, V: Number + 'static>(
	list: &List<T, U>,
	index: usize,
	p: Prop<U, V>,
	spec: NumberSpec,
	test: String,
) -> wavedude_widgets::NumberBox {
	let value = (p.get)(&list.items()[index]).to_f64();
	let list = list.clone();
	wavedude_widgets::NumberBox::new(
		WritableSetter::new(value, move |v: f64| list.set_item(index, p, V::from_f64(v))),
		spec,
	)
	.width(Size::flex(1.))
	.test_id(test)
}

fn remove_button<T: Fields, U: Clone + 'static>(
	f: &Form,
	list: &List<T, U>,
	index: usize,
	tip_key: &str,
	test: String,
) -> Element {
	let list = list.clone();
	let button = tip(
		f.s.get(tip_key),
		Button::new()
			.compact()
			.on_press(move |_| {
				list.set(|items| {
					items.remove(index);
				});
			})
			.child(IconView::new(icons::MINUS)),
	);
	tagged(button, test)
}

fn hint_row(text: &str) -> PropertyRow {
	PropertyRow::new("", paragraph().width(Size::fill()).span(Span::new(text.to_owned()))).full_width()
}

/// A full-width list row: its boxes share the width, its buttons keep theirs.
fn list_row() -> Rect {
	rect()
		.horizontal()
		.width(Size::fill())
		.content(Content::flex())
		.spacing(ROW_SPACING)
		.cross_align(Alignment::center())
}

/// A heading over a list's boxes.
fn list_heading(text: &str) -> Label {
	label().width(Size::flex(1.)).text(text.to_owned()).color(use_wave_theme().text_dim).max_lines(1)
}

/// The tooltip of a list item's field.
fn item_hint<U: Fields, V>(f: &Form, p: &Prop<U, V>) -> &'static str {
	f.s.hint(U::TYPE_NAME, p.field.name())
}

fn item_label<U: Fields, V>(f: &Form, p: &Prop<U, V>) -> &'static str {
	f.s.label(U::TYPE_NAME, p.field.name())
}

/// An outline's points: an X / Y heading, a full-width row per point with X, Y and remove, Add point below.
fn points(f: &Form, list: &List<Outline, Point>) -> Element {
	let s = &f.s;
	let mut rows = vec![hint_row(s.get("HintPolygonPoints"))];
	if !list.items().is_empty() {
		let headings = list_row()
			.child(list_heading(item_label(f, &Point::X)))
			.child(list_heading(item_label(f, &Point::Y)))
			.child(rect().width(Size::px(ROW_BUTTON_WIDTH)));
		rows.push(PropertyRow::new("", headings).full_width());
	}
	let spec = NumberSpec::integer(-1e6, 1e6);
	for i in 0..list.items().len() {
		let editors = list_row()
			.child(list_value(list, i, Point::X, spec.clone(), format!("Point{i}.X")).tooltip(item_hint(f, &Point::X)))
			.child(list_value(list, i, Point::Y, spec.clone(), format!("Point{i}.Y")).tooltip(item_hint(f, &Point::Y)))
			.child(remove_button(f, list, i, "TipRemovePoint", format!("Point{i}.Remove")));
		rows.push(PropertyRow::new("", editors).full_width());
	}
	let add = {
		let list = list.clone();
		move |_: Event<PressEventData>| {
			list.set(|items| {
				let last = items.last().copied().unwrap_or_default();
				items.push(Point::new(last.x + 10, last.y));
			});
		}
	};
	rows.push(
		PropertyRow::new("", dialogs::button(s.get("ButtonAddPoint"), true, add))
			.hint(s.get("TipAddPoint"))
			.full_width(),
	);
	section(s.get("SectionPoints"), rows)
}

/// A button that moves item `index` of `list` by `by` places.
fn move_button<T: Fields, U: Clone + 'static>(
	f: &Form,
	list: &List<T, U>,
	index: usize,
	by: isize,
	tip_key: &str,
	test: String,
) -> Element {
	let list = list.clone();
	let button = tip(
		f.s.get(tip_key),
		Button::new()
			.compact()
			.on_press(move |_| {
				list.set(|items| {
					let to = index as isize + by;
					if (0..items.len() as isize).contains(&to) {
						items.swap(index, to as usize);
					}
				});
			})
			.child(IconView::new(if by < 0 { icons::CHEVRON_UP } else { icons::CHEVRON_DOWN })),
	);
	tagged(button, test)
}

/// Width of a compact glyph button: a 16 px icon and 6 px padding each side.
const ROW_BUTTON_WIDTH: f32 = 28.;
const ROW_SPACING: f32 = 4.;
/// Width of a layer row's three buttons: the heading row leaves it free so the headings sit over the boxes.
const ROW_BUTTONS_WIDTH: f32 = 3. * ROW_BUTTON_WIDTH + 2. * ROW_SPACING;
/// Width of a layer row's ordinal.
const ORDINAL_WIDTH: f32 = 20.;

/// Width of a row's drag grip.
const GRIP_WIDTH: f32 = 14.;

/// A fibre node row's label column.
const NODE_LABEL_WIDTH: f32 = 36.;
/// A fibre node row's tail: the Smooth box and the remove button, or the same width left empty.
const NODE_TAIL_WIDTH: f32 = 116.;

/// An interference filter's layers, the outermost first: a row each with a grip, its thickness and index, up,
/// down and remove; Add layer appends a copy of the last, Remove all empties the list in one step. A drag by a
/// row's grip moves the layer through the list: the row whose top lies nearest the grip's top plus the drag is
/// the target, each crossing is one edit, and the mouse held keeps them one undo step.
fn layers(f: &Form, list: List<InterferenceFilter, FilterLayer>) -> Element {
	LayerList { form: f.clone(), list }.into_element()
}

/// The layer list as a component of its own: the form builders run conditionally, so the drag's state cannot
/// be a hook of theirs.
#[derive(Clone)]
struct LayerList {
	form: Form,
	list: List<InterferenceFilter, FilterLayer>,
}

impl PartialEq for LayerList {
	fn eq(&self, other: &Self) -> bool {
		self.form.row == other.form.row && self.form.s == other.form.s && self.list == other.list
	}
}

impl Component for LayerList {
	fn render(&self) -> impl IntoElement {
		self.rows()
	}
}

impl LayerList {
	fn rows(&self) -> Element {
		let f = &self.form;
		let s = &f.s;
		let list = &self.list;
		let (tmin, tmax) = bounds_of(FilterLayer::THICKNESS);
		let (imin, imax) = bounds_of(FilterLayer::REFRACTIVE_INDEX);
		let mut rows = vec![hint_row(s.get("HintFilterLayers"))];
		let theme = use_wave_theme();
		// the rows' tops by index, and the drag: the pointer's y and the grip's top at the press, the layer's index now
		let mut tops = use_state(Vec::<f32>::new);
		if tops.peek().len() > list.items().len() {
			tops.write().truncate(list.items().len());
		}
		let mut drag = use_state(|| None::<(f64, f32, usize)>);
		// each row: the grip, a narrow ordinal, the two boxes sharing the rest, the buttons
		let ordinal =
			|text: String| label().width(Size::px(ORDINAL_WIDTH)).text(text).color(theme.text_dim).max_lines(1);
		if !list.items().is_empty() {
			let on_move = {
				let list = list.clone();
				move |e: Event<PointerEventData>| {
					let Some((y0, top0, current)) = *drag.peek() else { return };
					let wanted = top0 + (e.global_location().y - y0) as f32;
					let nearest = tops
						.peek()
						.iter()
						.enumerate()
						.min_by(|a, b| (a.1 - wanted).abs().total_cmp(&(b.1 - wanted).abs()))
						.map(|(i, _)| i);
					let Some(target) = nearest.filter(|&t| t != current) else { return };
					list.set(|l| {
						if current < l.len() && target < l.len() {
							let layer = l.remove(current);
							l.insert(target, layer);
						}
					});
					drag.set(Some((y0, top0, target)));
				}
			};
			let headings = list_row()
				.on_global_pointer_move(on_move)
				.on_global_pointer_up(move |_| drag.set_if_modified(None))
				.child(rect().width(Size::px(GRIP_WIDTH)))
				.child(ordinal(String::new()))
				.child(list_heading(item_label(f, &FilterLayer::THICKNESS)))
				.child(list_heading(item_label(f, &FilterLayer::REFRACTIVE_INDEX)))
				.child(rect().width(Size::px(ROW_BUTTONS_WIDTH)));
			rows.push(PropertyRow::new("", headings).full_width());
		}
		for i in 0..list.items().len() {
			let thickness = NumberSpec::linear(tmin, tmax, 1.).with_decimals(2).with_drag_step(0.1);
			let index = NumberSpec::linear(imin, imax, 0.01).with_decimals(2).with_drag_step(0.01);
			let grip = rect()
				.width(Size::px(GRIP_WIDTH))
				.height(Size::px(22.))
				.center()
				.cursor(CursorIcon::RowResize)
				.on_sized(move |e: Event<SizedEventData>| {
					let top = e.area.min_y();
					if tops.peek().get(i) != Some(&top) {
						let mut tops = tops.write();
						if tops.len() <= i {
							tops.resize(i + 1, 0.);
						}
						tops[i] = top;
					}
				})
				.on_pointer_down(move |e: Event<PointerEventData>| {
					let top = tops.peek().get(i).copied().unwrap_or(0.);
					drag.set(Some((e.global_location().y, top, i)));
				})
				.child(IconView::new(icons::GRIP).color(theme.text_dim));
			let editors = list_row()
				.child(tagged(tip(s.get("TipDragLayer"), grip), format!("Layer{i}.Grip")))
				.child(ordinal(format!("{}", i + 1)))
				.child(
					list_value(list, i, FilterLayer::THICKNESS, thickness, format!("Layer{i}.Thickness"))
						.tooltip(item_hint(f, &FilterLayer::THICKNESS)),
				)
				.child(
					list_value(list, i, FilterLayer::REFRACTIVE_INDEX, index, format!("Layer{i}.RefractiveIndex"))
						.tooltip(item_hint(f, &FilterLayer::REFRACTIVE_INDEX)),
				)
				.child(move_button(f, list, i, -1, "TipLayerUp", format!("Layer{i}.Up")))
				.child(move_button(f, list, i, 1, "TipLayerDown", format!("Layer{i}.Down")))
				.child(remove_button(f, list, i, "TipRemoveLayer", format!("Layer{i}.Remove")));
			rows.push(PropertyRow::new("", editors).full_width());
		}
		let add = {
			let list = list.clone();
			move |_: Event<PressEventData>| {
				list.set(|items| {
					items.push(items.last().cloned().unwrap_or_default());
				});
			}
		};
		let clear = {
			let list = list.clone();
			move |_: Event<PressEventData>| list.set(Vec::clear)
		};
		let buttons = rect()
			.horizontal()
			.spacing(6.)
			.child(tip(s.get("TipAddLayer"), tagged(dialogs::button(s.get("ButtonAddLayer"), true, add), "AddLayer")))
			.child(tip(
				s.get("TipRemoveAllLayers"),
				tagged(
					dialogs::button(s.get("ButtonRemoveAllLayers"), !list.items().is_empty(), clear),
					"RemoveAllLayers",
				),
			));
		rows.push(PropertyRow::new("", buttons));
		section(s.get("SectionLayers"), rows)
	}
}

fn fiber(f: &Form, fb: &Scope<'_, Fiber>) -> Vec<Element> {
	let s = &f.s;
	let mut cladding = vec![
		fb.number(Fiber::CLADDING_INDEX, 0.01),
		fb.number(Fiber::CLADDING_THICKNESS, 1.),
		fb.number(Fiber::ABSORPTION, 5.),
		fb.number(Fiber::ABSORPTION_RAMP, 1.),
	];
	cladding.extend(coating_rows(&fb.nested(Fiber::COATING), false));
	let list = fb.list(Fiber::NODES);
	let mut nodes = vec![hint_row(s.get("HintFiberNodes"))];
	let theme = use_wave_theme();
	// a node is three rows, "Node", "In" and "Out", each a label and its X and Y; the first carries Smooth and remove
	let node_label =
		|text: &str| label().width(Size::px(NODE_LABEL_WIDTH)).text(text.to_owned()).color(theme.text_dim).max_lines(1);
	if !list.items().is_empty() {
		let headings = list_row()
			.child(node_label(""))
			.child(list_heading(item_label(f, &FiberNode::X)))
			.child(list_heading(item_label(f, &FiberNode::Y)))
			.child(rect().width(Size::px(NODE_TAIL_WIDTH)));
		nodes.push(PropertyRow::new("", headings).full_width());
	}
	let spec = NumberSpec::integer(-1e6, 1e6);
	for (i, node) in list.items().iter().enumerate() {
		let boxes = |row: Rect, x: Prop<FiberNode, i32>, y: Prop<FiberNode, i32>, tag: &str| {
			row.child(list_value(&list, i, x, spec.clone(), format!("Node{i}.{tag}X")).tooltip(item_hint(f, &x)))
				.child(list_value(&list, i, y, spec.clone(), format!("Node{i}.{tag}Y")).tooltip(item_hint(f, &y)))
		};
		let toggle = {
			let list = list.clone();
			move |on: bool| list.set_item(i, FiberNode::SMOOTH, on)
		};
		let tail = rect()
			.horizontal()
			.width(Size::px(NODE_TAIL_WIDTH))
			.content(Content::flex())
			.cross_align(Alignment::center())
			.spacing(ROW_SPACING)
			.child(rect().width(Size::flex(1.)).child(check_box(
				item_label(f, &FiberNode::SMOOTH).into(),
				node.smooth,
				toggle,
			)))
			.child(remove_button(f, &list, i, "TipRemoveNode", format!("Node{i}.Remove")));
		let point =
			boxes(list_row().child(node_label(s.get("LabelNodePoint"))), FiberNode::X, FiberNode::Y, "").child(tail);
		nodes.push(PropertyRow::new("", point).full_width());
		let spacer = || rect().width(Size::px(NODE_TAIL_WIDTH));
		let into = boxes(list_row().child(node_label(s.get("LabelNodeIn"))), FiberNode::IN_X, FiberNode::IN_Y, "In")
			.child(spacer());
		nodes.push(PropertyRow::new("", into).full_width());
		let out = boxes(list_row().child(node_label(s.get("LabelNodeOut"))), FiberNode::OUT_X, FiberNode::OUT_Y, "Out")
			.child(spacer());
		nodes.push(PropertyRow::new("", out).full_width());
	}
	let add = {
		let list = list.clone();
		move |_: Event<PressEventData>| {
			list.set(|l| {
				let (x, y) = l.last().map_or((0, 0), |n| (n.x + 50, n.y));
				l.push(FiberNode { x, y, in_x: -20, in_y: 0, out_x: 20, out_y: 0, smooth: true });
			});
		}
	};
	nodes.push(PropertyRow::new("", dialogs::button(s.get("ButtonAddNode"), true, add)).hint(s.get("TipAddNode")));
	vec![
		section(s.get("SectionCore"), vec![fb.number(Fiber::CORE_INDEX, 0.01), fb.number(Fiber::CORE_DIAMETER, 1.)]),
		section(s.get("SectionCladding"), cladding),
		section(s.get("SectionShape"), vec![fb.combo(Fiber::ENDS)]),
		section(s.get("SectionNodes"), nodes),
	]
}

fn gate(f: &Form, r: &Readouts, g: &Scope<'_, LogicGate>) -> Vec<Element> {
	let s = &f.s;
	let design = r.gate;
	let layout = design.map_or([0; 4], |d| d.layout);
	let ports = design.map_or([Point::default(); 4], |d| d.ports);
	let point = |q: Point| format!("({}, {})", q.x, q.y);
	let quarter_text = design.map(|d| d.quarter_wave).filter(|q| q.is_finite()).map(approx_px).unwrap_or_default();
	// the box shows the spacing the design uses; an edit stores the typed value
	let designed = design
		.map(|d| LogicGate { port_spacing: u32::try_from(d.spacing).unwrap_or(g.obj.port_spacing), ..g.obj.clone() });
	let spacing = g.over(designed.as_ref().unwrap_or(g.obj)).number(LogicGate::PORT_SPACING, 2.);
	let (upper, lower) = match g.obj.kind {
		m::GateKind::Xor => ("Xor", "And"),
		m::GateKind::And => ("And", "Xor"),
		m::GateKind::Or => ("Or", "Remainder"),
	};
	let (min, max) = bounds_of(LogicGate::COATING);
	vec![
		section(
			s.get("SectionDesign"),
			vec![
				g.decimals(LogicGate::DESIGN_FREQUENCY, 0.1, 2),
				readout(s.get("LabelWavelength"), r.wavelength.map(approx_px).unwrap_or_default(), "GateWavelength")
					.hint(s.get("TipGateWavelength")),
			],
		),
		section(
			s.get("SectionMaterial"),
			vec![
				g.number(LogicGate::CORE_INDEX, 0.01),
				g.number(LogicGate::CLADDING_INDEX, 0.01),
				g.number_with(LogicGate::COATING, NumberSpec::integer(min, max)),
				g.check(LogicGate::INPUT_COATED),
				g.check(LogicGate::OUTPUT_COATED),
				readout(s.get("LabelQuarterWave"), quarter_text, "GateQuarterWave").hint(s.get("TipQuarterWave")),
			],
		),
		section(s.get("SectionShape"), vec![spacing]),
		section(
			s.get("SectionOutputs"),
			vec![
				g.check_text(LogicGate::UPPER_OUTPUT_ACTIVE, s.enum_word("GateOutput", upper).to_owned()),
				g.check_text(LogicGate::LOWER_OUTPUT_ACTIVE, s.enum_word("GateOutput", lower).to_owned()),
			],
		),
		section(
			s.get("SectionLayout"),
			["LabelCoreWidth", "LabelCouplerGap", "LabelCouplerLength", "LabelGateLength"]
				.iter()
				.zip(layout)
				.map(|(key, v)| readout(s.get(key), format!("{v} px"), key).hint(s.get("TipGateLayout")))
				.collect(),
		),
		section(
			s.get("SectionPorts"),
			[
				s.get("LabelInputA"),
				s.get("LabelInputB"),
				g.label(&LogicGate::UPPER_OUTPUT_ACTIVE),
				g.label(&LogicGate::LOWER_OUTPUT_ACTIVE),
			]
			.iter()
			.zip(ports)
			.enumerate()
			.map(|(i, (text, q))| readout(text, point(q), &format!("GatePort{i}")).hint(s.get("TipGatePorts")))
			.collect(),
		),
	]
}

fn probe(f: &Form, o: &Probe, index: usize) -> Vec<Element> {
	let s = &f.s;
	let p = f.scope(o, move |e| Edit::Probe(index, e));
	let line = o.kind == ProbeKind::Line;
	let is_detector = o.is_detector();
	// the step axis belongs to a graph over time: a point probe's or a counting detector's
	let profile = o.shows_profile();
	let field = o.layer == Layer::Field;
	let measurements: &[Measurement] =
		if field { &[Measurement::Amplitude] } else { &[Measurement::Value, Measurement::IntegratedValue] };
	let custom = o.box_size == BoxSize::Custom;
	// the field's amplitude changes sign, so its axis stays linear
	let log = !field && o.y_scale == YScale::Logarithmic;
	let at_least_0 = ranges::AT_LEAST_0;
	vec![
		section(
			s.get("SectionGeneral"),
			vec![f.name::<Probe>(&o.name), p.check(Probe::ACTIVE), p.number(Probe::START_AFTER, 1.)],
		),
		// a detector is a line probe of the integrated intensity: the three after its own are shown as it set them
		section(
			s.get("SectionMeasurement"),
			vec![
				p.combo(Probe::DETECTOR),
				p.decimals(Probe::SENSITIVITY, 0.5, 2).enabled(is_detector),
				p.number(Probe::BIN_SIZE, 1.).enabled(o.detector == ProbeDetector::Imaging),
				p.combo(Probe::LAYER).enabled(!is_detector),
				p.combo_of(Probe::MEASUREMENT, measurements).enabled(!is_detector),
				p.combo(Probe::KIND).enabled(!is_detector),
			],
		),
		section(
			s.get("SectionPosition"),
			vec![
				p.number_in(Probe::AX, at_least_0, 1.),
				p.number_in(Probe::AY, at_least_0, 1.),
				p.number_in(Probe::BX, at_least_0, 1.).enabled(line),
				p.number_in(Probe::BY, at_least_0, 1.).enabled(line),
			],
		),
		section(
			s.get("SectionGraph"),
			vec![
				p.combo(Probe::BOX_SIZE),
				p.number_in(Probe::BOX_X, at_least_0, 1.),
				p.number_in(Probe::BOX_Y, at_least_0, 1.),
				p.number(Probe::BOX_WIDTH, 10.).enabled(custom),
				p.number(Probe::BOX_HEIGHT, 10.).enabled(custom),
			],
		),
		section(
			s.get("SectionAxes"),
			vec![
				p.combo(Probe::SCALE_MODE).enabled(!profile),
				p.number(Probe::WINDOW, 10.),
				p.check(Probe::AUTO_SCALE_X).enabled(!profile),
				p.check(Probe::AUTO_SCALE_Y),
				p.combo(Probe::Y_SCALE).enabled(!field),
				p.number(Probe::DECADES, 1.).enabled(log),
				p.number(Probe::Y_MIN, 0.1),
				p.number(Probe::Y_MAX, 0.1),
			],
		),
	]
}

/// The colour picker's tooltips in the UI language.
pub fn picker_tips(s: &wavedude_app_state::strings::Strings) -> PickerTips {
	PickerTips {
		plane: s.get("TipColorSquare").to_owned(),
		hue: s.get("TipColorHue").to_owned(),
		channel: s.get("TipColorChannel").to_owned(),
	}
}

fn to_gradient(stops: &[m::Stop]) -> Gradient {
	Gradient::new(
		stops
			.iter()
			.map(|s| GradientStop { position: s.at, color: Rgb::new(s.color.r, s.color.g, s.color.b) })
			.collect(),
	)
}

fn stops_of(g: &Gradient) -> Vec<m::Stop> {
	g.stops.iter().map(|s| m::Stop { at: s.position, color: m::Rgb::new(s.color.r, s.color.g, s.color.b) }).collect()
}

/// An edit of stop `i` of `gradient` by `change`, which says whether it changed anything; sets the whole stop list.
fn stop_edit<T: 'static>(
	set: &Rc<dyn Fn(Vec<m::Stop>)>,
	gradient: &Gradient,
	change: fn(&mut Gradient, usize, T) -> bool,
) -> impl Fn(usize, T) + use<T> {
	let (set, gradient) = (set.clone(), gradient.clone());
	move |i, v| {
		let mut g = gradient.clone();
		if change(&mut g, i, v) {
			set(stops_of(&g));
		}
	}
}

fn scheme(f: &Form, o: &ColorScheme, index: usize, r: &Readouts) -> Vec<Element> {
	let s = &f.s;
	let sc = f.scope(o, move |e| Edit::Scheme(index, e));
	let gradient = to_gradient(&o.stops);
	let set_stops: Rc<dyn Fn(Vec<m::Stop>)> = Rc::new(sc.setter(ColorScheme::STOPS));
	let set_gradient = {
		let set = set_stops.clone();
		move |g: Gradient| set(stops_of(&g))
	};
	let based_on = {
		let form = f.clone();
		let (set_stops, set_kind, set_description) =
			(set_stops.clone(), sc.setter(ColorScheme::KIND), sc.setter(ColorScheme::DESCRIPTION));
		let items = r.based_on.iter().map(|n| (n.clone(), n.clone())).collect();
		combo_box(String::new(), items, true, "ColorScheme.BasedOn".into(), move |name| {
			let source = form.cx.shared.peek(|a| {
				schemes::find(&a.document.project.color_schemes, &name)
					.or_else(|| schemes::find(&a.schemes.user, &name))
					.or_else(|| schemes::find(schemes::built_in(), &name))
					.cloned()
			});
			if let Some(src) = source {
				set_stops(src.stops);
				set_kind(src.kind);
				set_description(src.description);
			}
		})
	};
	let mut rows = vec![
		PropertyRow::new(s.get("LabelBasedOn"), based_on).hint(s.get("TipBasedOn")),
		PropertyRow::new(
			"",
			tagged(GradientEditor::new(WritableSetter::new(gradient.clone(), set_gradient)), "SchemeGradient"),
		)
		.full_width()
		.hint(s.get("TipSchemeGradient")),
	];
	for (i, stop) in gradient.stops.iter().enumerate() {
		let set_at = stop_edit(&set_stops, &gradient, |g, i, at: f64| {
			g.set_position(i, at);
			true
		});
		let set_color = stop_edit(&set_stops, &gradient, |g, i, c: Rgb| {
			g.set_color(i, c);
			true
		});
		let set_hex = stop_edit(&set_stops, &gradient, |g, i, hex: String| match Rgb::from_hex(&hex) {
			Some(c) => {
				g.set_color(i, c);
				true
			}
			None => false,
		});
		let remove = stop_edit(&set_stops, &gradient, |g, i, ()| g.remove(i));
		let editors = rect()
			.horizontal()
			.width(Size::fill())
			.spacing(4.)
			.cross_align(Alignment::center())
			.child(label().width(Size::px(16.)).text(format!("{}", i + 1)))
			.child(
				wavedude_widgets::NumberBox::new(
					WritableSetter::new(stop.position, move |v: f64| set_at(i, v)),
					NumberSpec::linear(0., 1., 0.01).with_decimals(3),
				)
				.width(Size::px(64.))
				.tooltip(s.get("TipStopPosition"))
				.test_id(format!("Stop{i}.Position")),
			)
			.child(
				ColorButton::new(WritableSetter::new(stop.color, move |c: Rgb| set_color(i, c)))
					.tooltip(s.get("TipStopSwatch"))
					.tips(picker_tips(s))
					.test_id(format!("Stop{i}.Color")),
			)
			.child(tip(
				s.get("TipStopColor"),
				rect().width(Size::flex(1.)).child(super::properties::text_box(
					stop.color.to_hex(),
					false,
					move |t| set_hex(i, t),
					format!("Stop{i}.Hex"),
				)),
			))
			.child(tip(
				s.get("TipRemoveStop"),
				Button::new()
					.compact()
					.enabled(r.can_remove_stop)
					.on_press(move |_| remove(i, ()))
					.child(IconView::new(icons::MINUS)),
			));
		rows.push(PropertyRow::new("", editors).full_width());
	}
	let add = stop_edit(&set_stops, &gradient, |g, _, ()| {
		g.add_in_widest_gap();
		true
	});
	rows.push(
		PropertyRow::new("", dialogs::button(s.get("ButtonAddStop"), true, move |_| add(0, ())))
			.hint(s.get("TipAddStop"))
			.full_width(),
	);
	vec![
		section(
			s.get("SectionGeneral"),
			vec![f.name::<ColorScheme>(&o.name), sc.combo(ColorScheme::KIND), sc.text(ColorScheme::DESCRIPTION, false)],
		),
		section(s.get("SectionStops"), rows),
	]
}
