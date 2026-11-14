//! The forms of docs/ui/30, row by row, on the editors of [`super`].

use std::borrow::Cow;

use eframe::egui::{Ui, vec2};
use wavedude_app_state::Event;
use wavedude_app_state::commands::{Command, Edit, NewPart};
use wavedude_app_state::platform::{HAS_PATHS, SystemTime};
use wavedude_app_state::selection::Row;
use wavedude_core::io::schemes;
use wavedude_core::model::{
	self as m, BoxSize, BraggReflector, Coating, ColorScheme, Diffuser, Display, DisplayLayer, Ellipse,
	FacettedGrating, Fiber, FiberNode, Field as _, Fields, FilterLayer, FresnelLens, GrinLens, InterferenceFilter,
	ItemKind, Layer, Lens, LogicGate, Material, Measurement, MetaAxiconDesign, MetaDesign, MetaLensDesign,
	MetaPrismDesign, MetaSurface, Number, Outline, Part, PartBase, PartKind, Point, Polygon, Probe, ProbeDetector,
	ProbeKind, Project, ProjectEdit, Prop, Rectangle, Scene, SceneMode, SimpleGrating, Simulation, Source, SourceArray,
	SourceMode, Stencil, Stop, Triangle, TriangleType, Video, YScale, ZonePlate, ranges,
};

use super::super::color_picker::{color_button, parse_hex};
use super::super::edit;
use super::super::gradient_editor::{Gradient, gradient_editor};
use super::super::icons;
use super::super::number_box::NumberSpec;
use super::super::text_box::{Kind, text_box};
use super::super::theme::Palette;
use super::{Choices, Cx, Dialog, Form, List, Request, Scope, button, caption, check_box, combo, glyph_button};
use crate::properties::{MetaReadouts, SchemeEntry, bounds_of};

/// Width of a compact glyph button.
const ROW_BUTTON_WIDTH: f32 = 28.;
const ROW_SPACING: f32 = 4.;
/// Width of a layer row's three buttons: the heading row leaves it free so the headings sit over the boxes.
const ROW_BUTTONS_WIDTH: f32 = 3. * ROW_BUTTON_WIDTH + 2. * ROW_SPACING;
/// Width of a layer row's ordinal.
const ORDINAL_WIDTH: f32 = 20.;
/// Width of a row's drag grip.
const GRIP_WIDTH: f32 = 14.;
/// A layer row's grip, ordinal and buttons.
const LAYER_FIXED: f32 = GRIP_WIDTH + ORDINAL_WIDTH + ROW_BUTTONS_WIDTH;
/// A fibre node row's label column.
const NODE_LABEL_WIDTH: f32 = 36.;
/// A fibre node row's tail: the Smooth box and the remove button, or the same width left empty.
const NODE_TAIL_WIDTH: f32 = 116.;
/// A stop row's ordinal and position box.
const STOP_ORDINAL: f32 = 16.;
const STOP_POSITION: f32 = 64.;

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

/// Every row of the form of `row`.
pub(super) fn build(f: &mut Form<'_>, row: Row) {
	let p = &f.cx.app.document.project;
	match row {
		Row::Project => project(f, p),
		Row::Scene => scene(f, &p.scene),
		Row::Simulation => simulation(f, &p.simulation),
		Row::Display => display(f, &p.display),
		Row::Item(i) => match i.kind {
			ItemKind::Source => source(f, &p.sources[i.index], row, i.index),
			ItemKind::Part => part(f, &p.parts[i.index], row, i.index),
			ItemKind::Probe => probe(f, &p.probes[i.index], row, i.index),
			ItemKind::Scheme => scheme(f, &p.color_schemes[i.index], row, i.index),
		},
		Row::Folder(_) => unreachable!("a folder shows its count"),
	}
}

/// The width a button with `text` takes.
fn button_width(ui: &Ui, text: &str) -> f32 {
	let font = eframe::egui::FontId::proportional(super::super::theme::FONT_SIZE);
	let shown = wavedude_app_state::strings::strip_mnemonic(text);
	ui.painter().layout_no_wrap(shown, font, Palette::of(ui.ctx().theme()).text).size().x + 2. * super::BUTTON_PADDING.x
}

fn project(f: &mut Form<'_>, o: &Project) {
	let s = f.cx.s;
	let p = Scope::new(o, Edit::Project);
	f.section(s.get("SectionGeneral"), |f| {
		f.readout(s.get("LabelProjectVersion"), &m::PROJECT_VERSION.to_string(), "");
		let label = p.label(&s, &Project::SEED);
		f.row(label, p.hint(&s, &Project::SEED), |ui, cx| {
			let new_seed = s.get("ButtonNewSeed");
			let width = ui.available_width() - button_width(ui, new_seed) - 6.;
			let spec = NumberSpec::integer(f64::from(i32::MIN), f64::from(i32::MAX));
			p.number_editor(ui, cx, Project::SEED, spec, label, Some(width));
			ui.add_space(6.);
			if button(ui, new_seed, true).clicked() {
				let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
				cx.send(p.event(Project::SEED, (nanos & 0x7fff_ffff) as i32));
			}
		});
		p.text(f, Project::DESCRIPTION, &o.description, true);
	});
}

/// The image's name, editable as Next's, and its browse, save and remove buttons.
fn image_row<W: Fn(m::SceneEdit) -> Edit>(f: &mut Form<'_>, sc: &Scope<'_, Scene, W>, has_bytes: bool) {
	let s = f.cx.s;
	let label = sc.label(&s, &Scene::IMAGE);
	f.row(label, sc.hint(&s, &Scene::IMAGE), |ui, cx| {
		let id = sc.id(cx, Scene::IMAGE, String::new());
		let browse = "...";
		let buttons = button_width(ui, browse) + 2. * ROW_BUTTON_WIDTH + 3. * ROW_SPACING;
		let size = vec2(ui.available_width() - buttons, super::super::theme::INPUT_HEIGHT);
		ui.allocate_ui(size, |ui| {
			let kind = Kind { live: true, ..Kind::default() };
			cx.text(ui, id, &sc.obj.image, kind, label, false, |t| sc.event(Scene::IMAGE, t));
		});
		ui.add_space(ROW_SPACING);
		if button(ui, browse, true).on_hover_text(s.get("TipBrowseSceneImage")).clicked() {
			cx.ask(Request::Dialog(Dialog::PickSceneImage));
		}
		ui.add_space(ROW_SPACING);
		ui.add_enabled_ui(has_bytes, |ui| {
			if glyph_button(ui, id.with("save"), icons::SAVE, None, false, s.get("TipSaveSceneImage")).clicked() {
				cx.ask(Request::Dialog(Dialog::SaveSceneImage));
			}
			ui.add_space(ROW_SPACING);
			if glyph_button(ui, id.with("remove"), icons::CLOSE, None, false, s.get("TipRemoveSceneImage")).clicked() {
				cx.send(Event::Command(Command::RemoveSceneImage));
			}
		});
	});
}

fn scene(f: &mut Form<'_>, o: &Scene) {
	let s = f.cx.s;
	let r = f.cx.r;
	let sc = Scope::new(o, |e| Edit::Project(ProjectEdit::Scene(e)));
	let image_mode = o.mode == SceneMode::Image;
	f.section(s.get("SectionCanvas"), |f| {
		sc.combo(f, Scene::MODE);
		if image_mode {
			image_row(f, &sc, r.image_size.is_some());
			// the browser has no file paths
			if HAS_PATHS {
				let path = &o.image_path;
				let shown = if !path.is_empty() && !r.image_found {
					Cow::Owned(format!("{path} {}", s.get("SceneImageNotFound")))
				} else {
					Cow::Borrowed(path.as_str())
				};
				f.readout(sc.label(&s, &Scene::IMAGE_PATH), &shown, sc.hint(&s, &Scene::IMAGE_PATH));
			}
			let size = r.image_size.map_or_else(
				|| s.get("ImageUnreadable").to_owned(),
				|(w, h, kb)| format!("{w} x {h}, {} KB", s.grouped(kb as u64)),
			);
			f.readout(s.get("LabelEmbeddedImage"), &size, s.get("TipEmbeddedImage"));
		}
		let (smin, smax) = bounds_of(Scene::WIDTH);
		sc.number_with(f.on(!image_mode), Scene::WIDTH, NumberSpec::integer(smin, smax));
		sc.number_with(f.on(!image_mode), Scene::HEIGHT, NumberSpec::integer(smin, smax));
		sc.combo(f, Scene::EDGES);
		sc.number(f, Scene::DAMPING_EDGE, 1.);
		sc.number(f, Scene::REFRACTIVE_INDEX, 0.01);
	});
	f.section(s.get("SectionImageSources"), |f| {
		sc.check(f, Scene::IMAGE_SOURCES);
		sc.number(f.on(o.image_sources), Scene::IMAGE_SOURCE_SCALE, 0.1);
		sc.check(f, Scene::OBJECT_SOURCES);
	});
}

fn simulation(f: &mut Form<'_>, o: &Simulation) {
	let s = f.cx.s;
	let sim = Scope::new(o, |e| Edit::Project(ProjectEdit::Simulation(e)));
	f.section(s.get("SectionRun"), |f| {
		sim.number(f, Simulation::STEPS, 100.);
		sim.number(f, Simulation::VISUALIZE_EVERY, 1.);
	});
	f.section(s.get("SectionSolver"), |f| {
		sim.check_as(
			f,
			Simulation::STENCIL,
			|s| *s == Stencil::FourthOrder,
			|on| if on { Stencil::FourthOrder } else { Stencil::NinePoint },
		);
		sim.number(f, Simulation::WAVE_SPEED, 0.01);
		let (dmin, dmax) = bounds_of(Simulation::TIME_DECAY);
		sim.number_with(
			f,
			Simulation::TIME_DECAY,
			NumberSpec::linear(dmin, dmax, 1.).with_decimals(1).with_drag_step(0.1),
		);
		sim.number(f, Simulation::SOURCE_FREQUENCY_SCALE, 0.1);
		sim.number(f, Simulation::INTENSITY_SCALE, 0.1);
	});
}

/// A display layer's rows: the scheme picker (with Load from file and Edit a copy) and the layer's settings.
fn layer_rows<W: Fn(m::DisplayLayerEdit) -> Edit>(
	f: &mut Form<'_>,
	l: &Scope<'_, DisplayLayer, W>,
	names: &[SchemeEntry],
	mine: Option<&str>,
) {
	let s = f.cx.s;
	let current = l.obj.color_scheme.as_str();
	let listed = names.iter().position(|e| e.name == current);
	// a name no scheme has is shown first, greyed
	let missing = usize::from(!current.is_empty() && listed.is_none());
	let word = |i: usize| -> Cow<'_, str> {
		if missing == 1 && i == 0 {
			return s.format("PickerMissingFormat", &[&current]).into();
		}
		let e = &names[i - missing];
		if e.own { format!("{} ({})", e.name, s.get("PickerProjectTag")).into() } else { e.name.as_str().into() }
	};
	let swatch = |i: usize| (i >= missing).then(|| names[i - missing].stops.as_slice());
	let shown = word(listed.map_or(0, |i| i + missing));
	let actions = [s.get("PickerBrowse"), s.get("PickerEditCopy"), s.get("PickerRemoveFromMine")];
	let actions = &actions[..if mine.is_some() { 3 } else { 2 }];
	let label = l.label(&s, &DisplayLayer::COLOR_SCHEME);
	let picked = f.row(label, l.hint(&s, &DisplayLayer::COLOR_SCHEME), |ui, cx| {
		let id = l.id(cx, DisplayLayer::COLOR_SCHEME, String::new());
		let choices = Choices {
			len: names.len() + missing,
			current: (!current.is_empty()).then(|| listed.map_or(0, |i| i + missing)),
			shown: &shown,
			word: &word,
			swatch: Some(&swatch),
			disabled: None,
			actions,
		};
		combo(ui, id, label, &choices)
	});
	let len = names.len() + missing;
	match picked {
		Some(i) if i == len => f.cx.ask(Request::Dialog(Dialog::PickScheme)),
		Some(i) if i == len + 1 => edit_copy(&mut f.cx, l, current),
		Some(i) if i == len + 2 => {
			if let Some(name) = mine {
				f.cx.send(Event::RemoveUserScheme(name.to_owned()));
			}
		}
		Some(i) if i >= missing && names[i - missing].name != current => {
			f.cx.send(l.event(DisplayLayer::COLOR_SCHEME, names[i - missing].name.clone()));
		}
		_ => {}
	}
	let (bmin, bmax) = bounds_of(DisplayLayer::BRIGHTNESS);
	l.number_with(f, DisplayLayer::BRIGHTNESS, NumberSpec::logarithmic(bmin, bmax));
	l.number(f, DisplayLayer::BLACK_LEVEL, 0.01);
	l.check(f, DisplayLayer::PREVIEW);
	l.check(f, DisplayLayer::OVERLAY);
	l.number(f, DisplayLayer::OVERLAY_GAIN, 0.05);
	l.check(f, DisplayLayer::OVERLAY_IN_EXPORT);
}

/// The scheme `name` of the project, the user's or the built-in ones.
fn find_scheme<'a>(app: &'a crate::App, name: &str) -> Option<&'a ColorScheme> {
	schemes::find(&app.document.project.color_schemes, name)
		.or_else(|| schemes::find(&app.schemes.user, name))
		.or_else(|| schemes::find(schemes::built_in(), name))
}

/// Makes a project scheme from the layer's current one (a project scheme is copied) and points the layer at it.
fn edit_copy<W: Fn(m::DisplayLayerEdit) -> Edit>(cx: &mut Cx<'_>, l: &Scope<'_, DisplayLayer, W>, current: &str) {
	let Some(scheme) = find_scheme(cx.app, current) else { return };
	let taken = cx.app.document.project.color_schemes.iter().map(|s| s.name.as_str());
	let name = m::names::copy_name(&scheme.name, taken);
	cx.send(Event::Command(Command::ImportScheme(Box::new(scheme.clone()))));
	cx.send(l.event(DisplayLayer::COLOR_SCHEME, name));
}

fn display(f: &mut Form<'_>, o: &Display) {
	let s = f.cx.s;
	let r = f.cx.r;
	let d = Scope::new(o, |e| Edit::Project(ProjectEdit::Display(e)));
	f.section(s.get("SectionField"), |f| layer_rows(f, &d.nested(Display::FIELD), &r.schemes[0], r.mine[0].as_deref()));
	f.section(s.get("SectionIntensity"), |f| {
		layer_rows(f, &d.nested(Display::INTENSITY), &r.schemes[1], r.mine[1].as_deref());
	});
	f.section(s.get("SectionPreview"), |f| {
		d.check(f, Display::HANDLES_IN_PREVIEW);
		d.check(f, Display::LABELS_IN_PREVIEW);
	});
	f.section(s.get("SectionExportRange"), |f| {
		d.number(f, Display::EXPORT_FROM, 1.);
		d.number(f, Display::EXPORT_TO, 1.);
		d.check(f, Display::PROBE_GRAPHS_IN_EXPORT);
		d.check(f, Display::PROBE_MARKERS_IN_EXPORT);
		d.check(f, Display::SOURCE_MARKERS_IN_EXPORT);
	});
	let v = d.nested(Display::VIDEO);
	f.section(s.get("SectionVideo"), |f| {
		v.number(f, Video::FPS, 1.);
		v.number(f, Video::QUALITY, 1.);
	});
}

fn source(f: &mut Form<'_>, o: &Source, row: Row, index: usize) {
	let s = f.cx.s;
	let r = f.cx.r;
	let src = Scope::new(o, move |e| Edit::Source(index, e));
	let pulsed = o.mode == SourceMode::Pulsed;
	let composite = o.array.is_some();
	// a plain source shows the array's rows greyed and empty
	let plain_array = SourceArray::default();
	let arr = src.variant_or(Source::ARRAY, &plain_array);
	f.section(s.get("SectionGeneral"), |f| {
		f.name::<Source>(row, &o.name);
		src.check(f, Source::ENABLED);
	});
	f.section(s.get("SectionPosition"), |f| {
		src.decimals(f, Source::X, 1., 0);
		src.decimals(f, Source::Y, 1., 0);
	});
	f.section(s.get("SectionWave"), |f| {
		src.decimals(f, Source::FREQUENCY, 0.1, 2);
		src.decimals(f, Source::AMPLITUDE, 0.1, 2);
		src.decimals(f, Source::PHASE, 0.5, 2);
		let wavelength = r.wavelength.map_or_else(String::new, |w| {
			let text = if w.is_finite() { approx_px(w) } else { "-".into() };
			if r.nominal { format!("{text} {}", s.get("WavelengthNominal")) } else { text }
		});
		f.readout(s.get("LabelWavelength"), &wavelength, s.get("TipWavelength"));
	});
	f.section(s.get("SectionTiming"), |f| {
		src.combo(f, Source::MODE);
		src.combo(f.on(pulsed), Source::PULSE_SHAPE);
		src.number(f.on(pulsed), Source::PULSE_WIDTH, 1.);
		src.number(f.on(pulsed), Source::OFF_WIDTH, 1.);
		let label = src.label(&s, &Source::DETACH_AFTER);
		f.row(label, src.hint(&s, &Source::DETACH_AFTER), |ui, cx| {
			let value = o.detach_after.map_or(-1., f64::from);
			let (_, max) = bounds_of(Source::DETACH_AFTER);
			let id = src.id(cx, Source::DETACH_AFTER, None);
			let event = |v: f64| src.event(Source::DETACH_AFTER, (v >= 0.).then(|| v.round() as u32));
			cx.number(ui, id, value, NumberSpec::integer(-1., max), false, None, label, true, event);
		});
	});
	f.section(s.get("SectionArray"), |f| {
		src.check_as(f, Source::SET_ARRAY, Option::is_some, |on| on.then(SourceArray::default));
		arr.number(f.on(composite), SourceArray::SIZE_X, 1.);
		arr.number(f.on(composite), SourceArray::SIZE_Y, 1.);
		arr.number(f.on(composite), SourceArray::SPACING_X, 1.);
		arr.number(f.on(composite), SourceArray::SPACING_Y, 1.);
		let (amin, amax) = bounds_of(SourceArray::ANGLE);
		let angle = NumberSpec::linear(amin, amax, 5.).with_decimals(1).with_drag_step(0.5);
		arr.number_with(f.on(composite), SourceArray::ANGLE, angle);
	});
	f.section(s.get("SectionRandom"), |f| {
		for p in [
			SourceArray::RANDOM_FREQUENCY,
			SourceArray::RANDOM_PHASE,
			SourceArray::RANDOM_OFF_WIDTH,
			SourceArray::RANDOM_POSITION,
		] {
			arr.number(f.on(composite), p, 1.);
		}
	});
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

/// The coating's rows on `faces`; greyed when `disabled`.
fn coating_rows<W: Fn(m::CoatingEdit) -> Edit>(
	f: &mut Form<'_>,
	c: &Scope<'_, Coating, W>,
	disabled: bool,
	faces: CoatedFaces,
) {
	let (min, max) = bounds_of(Coating::DEPTH);
	c.number_with(f.on(!disabled), Coating::DEPTH, NumberSpec::integer(min, max));
	c.combo(f.on(!disabled), Coating::PROFILE);
	if matches!(faces, CoatedFaces::All | CoatedFaces::FrontBack) {
		c.check(f.on(!disabled), Coating::FRONT);
	}
	c.check(f.on(!disabled), Coating::BACK);
	if faces == CoatedFaces::All {
		c.check(f.on(!disabled), Coating::SIDES);
	}
}

/// The glass or mirror of a lens or shape, with the coating offered on `faces`.
fn material<WM: Fn(m::MaterialEdit) -> Edit, WC: Fn(m::CoatingEdit) -> Edit>(
	f: &mut Form<'_>,
	mat: &Scope<'_, Material, WM>,
	c: &Scope<'_, Coating, WC>,
	faces: CoatedFaces,
) {
	let mirror = mat.obj.mirror;
	f.section(f.cx.s.get("SectionMaterial"), |f| {
		mat.check(f, Material::MIRROR);
		mat.number(f.on(!mirror), Material::REFRACTIVE_INDEX, 0.01);
		if faces != CoatedFaces::None {
			coating_rows(f, c, mirror, faces);
		}
		mat.number(f.on(!mirror), Material::ABSORPTION, 5.);
		mat.number(f.on(!mirror), Material::ABSORPTION_RAMP, 5.);
	});
}

/// The rows every part shares: its description, name, enabled flag and position.
fn part_common<W: Fn(m::PartBaseEdit) -> Edit>(f: &mut Form<'_>, base: &Scope<'_, PartBase, W>, row: Row, ty: &str) {
	let s = f.cx.s;
	let open = f.cx.app.settings.part_description_expanded;
	f.description(s.get("SectionDescription"), s.get("TipPartDescription"), s.kind(ty), open);
	f.section(s.get("SectionGeneral"), |f| {
		f.name::<PartBase>(row, &base.obj.name);
		base.check(f, PartBase::ENABLED);
	});
	f.section(s.get("SectionPosition"), |f| {
		base.number(f, PartBase::X, 1.);
		base.number(f, PartBase::Y, 1.);
		let (amin, amax) = bounds_of(PartBase::ANGLE);
		base.number_with(f, PartBase::ANGLE, NumberSpec::linear(amin, amax, 5.).with_decimals(1).with_drag_step(0.5));
	});
}

fn part(f: &mut Form<'_>, o: &Part, row: Row, index: usize) {
	let s = f.cx.s;
	let r = f.cx.r;
	let part = Scope::new(o, move |e| Edit::Part(index, e));
	let kind = part.nested(Part::KIND);
	part_common(f, &part.nested(Part::BASE), row, NewPart::of(&o.kind).type_name());
	let shape = s.get("SectionShape");
	match &o.kind {
		PartKind::Lens(inner) => {
			let l = kind.variant_over(PartKind::LENS, inner);
			material(f, &l.nested(Lens::MATERIAL), &l.nested(Lens::COATING), CoatedFaces::All);
			lens::<_, fn(m::FresnelLensEdit) -> Edit>(f, &l, None);
		}
		PartKind::FresnelLens(inner) => {
			let z = kind.variant_over(PartKind::FRESNEL_LENS, inner);
			let l = z.nested(FresnelLens::LENS);
			material(f, &l.nested(Lens::MATERIAL), &l.nested(Lens::COATING), CoatedFaces::All);
			lens(f, &l, Some(&z));
		}
		PartKind::Polygon(inner) => {
			let p = kind.variant_over(PartKind::POLYGON, inner);
			material(f, &p.nested(Polygon::MATERIAL), &p.nested(Polygon::COATING), CoatedFaces::All);
			f.section(shape, |f| {
				p.number(f, Polygon::SIDES, 1.);
				p.number(f, Polygon::SIDE, 10.);
			});
		}
		PartKind::Rectangle(inner) => {
			let p = kind.variant_over(PartKind::RECTANGLE, inner);
			material(f, &p.nested(Rectangle::MATERIAL), &p.nested(Rectangle::COATING), CoatedFaces::All);
			f.section(shape, |f| linked_sizes(f, &p, Rectangle::LENGTH, Rectangle::WIDTH, Rectangle::LINKED));
		}
		PartKind::Ellipse(inner) => {
			let p = kind.variant_over(PartKind::ELLIPSE, inner);
			material(f, &p.nested(Ellipse::MATERIAL), &p.nested(Ellipse::COATING), CoatedFaces::All);
			f.section(shape, |f| linked_sizes(f, &p, Ellipse::LENGTH, Ellipse::WIDTH, Ellipse::LINKED));
		}
		PartKind::Triangle(inner) => {
			let t = kind.variant_over(PartKind::TRIANGLE, inner);
			material(f, &t.nested(Triangle::MATERIAL), &t.nested(Triangle::COATING), CoatedFaces::All);
			triangle(f, &t);
		}
		PartKind::Outline(inner) => {
			let p = kind.variant_over(PartKind::OUTLINE, inner);
			material(f, &p.nested(Outline::MATERIAL), &p.nested(Outline::COATING), CoatedFaces::All);
			points(f, &p.list(Outline::POINTS, &inner.points));
		}
		PartKind::Diffuser(inner) => {
			let d = kind.variant_over(PartKind::DIFFUSER, inner);
			material(f, &d.nested(Diffuser::MATERIAL), &d.nested(Diffuser::COATING), CoatedFaces::All);
			f.section(shape, |f| {
				d.number(f, Diffuser::LENGTH, 10.);
				d.number(f, Diffuser::WIDTH, 10.);
			});
			let (height, ripples) = r.roughness.map_or((String::new(), String::new()), |(h, n)| (px(h), n.to_string()));
			f.section(s.get("SectionRoughness"), |f| {
				d.decimals(f, Diffuser::CENTER_WAVELENGTH, 0.1, 2);
				d.decimals(f, Diffuser::AMPLITUDE, 0.1, 2);
				d.decimals(f, Diffuser::SIGMA, 0.1, 2);
				f.readout(s.get("LabelProfileHeight"), &height, s.get("TipProfileHeight"));
				f.readout(s.get("LabelComponentCount"), &ripples, s.get("TipComponentCount"));
			});
		}
		PartKind::InterferenceFilter(inner) => {
			let i = kind.variant_over(PartKind::INTERFERENCE_FILTER, inner);
			let (mat, coat) = (i.nested(InterferenceFilter::MATERIAL), i.nested(InterferenceFilter::COATING));
			material(f, &mat, &coat, CoatedFaces::Back);
			f.section(shape, |f| {
				i.number(f, InterferenceFilter::LENGTH, 10.);
				i.number(f, InterferenceFilter::WIDTH, 10.);
				let stack = r.stack.map(px).unwrap_or_default();
				f.readout(s.get("LabelStackThickness"), &stack, s.get("TipStackThickness"));
			});
			layers(f, &i.list(InterferenceFilter::LAYERS, &inner.layers));
		}
		PartKind::SimpleGrating(inner) => {
			let g = kind.variant_over(PartKind::SIMPLE_GRATING, inner);
			material(f, &g.nested(SimpleGrating::MATERIAL), &g.nested(SimpleGrating::COATING), CoatedFaces::None);
			let max = f64::from(r.max_diameter.unwrap_or(1));
			f.section(s.get("SectionLattice"), |f| {
				g.number(f, SimpleGrating::COLUMNS, 1.);
				g.number(f, SimpleGrating::ROWS, 1.);
				g.number(f, SimpleGrating::COLUMN_SPACING, 1.);
				g.number(f, SimpleGrating::ROW_SPACING, 1.);
				g.number_with(f, SimpleGrating::DIAMETER, NumberSpec::integer(1., max));
			});
		}
		PartKind::MetaSurface(inner) => meta_surface(f, &kind.variant_over(PartKind::META_SURFACE, inner)),
		PartKind::FacettedGrating(inner) => {
			let g = kind.variant_over(PartKind::FACETTED_GRATING, inner);
			let (mat, coat) = (g.nested(FacettedGrating::MATERIAL), g.nested(FacettedGrating::COATING));
			material(f, &mat, &coat, CoatedFaces::Back);
			f.section(shape, |f| {
				g.number(f, FacettedGrating::LENGTH, 10.);
				g.number(f, FacettedGrating::WIDTH, 10.);
			});
			f.section(s.get("SectionFacets"), |f| {
				g.combo(f, FacettedGrating::PATTERN);
				g.decimals(f, FacettedGrating::PITCH, 0.1, 2);
				g.decimals(f, FacettedGrating::AMPLITUDE, 0.1, 2);
			});
		}
		PartKind::Fiber(inner) => fiber(f, &kind.variant_over(PartKind::FIBER, inner)),
		PartKind::BraggReflector(inner) => {
			let b = kind.variant_over(PartKind::BRAGG_REFLECTOR, inner);
			f.section(s.get("SectionLayers"), |f| {
				b.number(f, BraggReflector::PAIRS, 1.);
				b.number(f, BraggReflector::THICKNESS1, 1.);
				b.number(f, BraggReflector::INDEX1, 0.01);
				b.number(f, BraggReflector::THICKNESS2, 1.);
				b.number(f, BraggReflector::INDEX2, 0.01);
			});
			f.section(shape, |f| b.number(f, BraggReflector::WIDTH, 10.));
		}
		PartKind::GrinLens(inner) => {
			let g = kind.variant_over(PartKind::GRIN_LENS, inner);
			f.section(s.get("SectionMaterial"), |f| {
				g.number(f, GrinLens::CENTRE_INDEX, 0.01);
				g.number(f, GrinLens::EDGE_INDEX, 0.01);
				coating_rows(f, &g.nested(GrinLens::COATING), false, CoatedFaces::All);
			});
			f.section(shape, |f| {
				g.number(f, GrinLens::LENGTH, 10.);
				g.number(f, GrinLens::DIAMETER, 10.);
			});
		}
		PartKind::ZonePlate(inner) => zone_plate(f, &kind.variant_over(PartKind::ZONE_PLATE, inner)),
		PartKind::LogicGate(inner) => gate(f, &kind.variant_over(PartKind::LOGIC_GATE, inner)),
	}
}

/// Length and Width of a rectangle or ellipse with the chain toggle that links them beside both.
fn linked_sizes<T: Fields, W: Fn(T::Edit) -> Edit>(
	f: &mut Form<'_>,
	b: &Scope<'_, T, W>,
	length: Prop<T, u32>,
	width: Prop<T, u32>,
	linked: Prop<T, bool>,
) {
	let s = f.cx.s;
	let on = b.get(linked);
	let room = f.beside(ROW_BUTTON_WIDTH + ROW_SPACING, |f| {
		b.number(f, length, 10.);
		b.number(f, width, 10.);
	});
	// the chain sits right of both boxes, in the middle of their rows
	let at = room.with_min_x(room.max.x - ROW_BUTTON_WIDTH);
	let palette = Palette::of(f.ui.ctx().theme());
	let ink = if on { palette.accent } else { palette.text_dim };
	let id = b.id(&f.cx, linked, false);
	let hint = b.hint(&s, &linked);
	let layout = eframe::egui::Layout::left_to_right(eframe::egui::Align::Center);
	let mut ui = f.ui.new_child(eframe::egui::UiBuilder::new().max_rect(at).layout(layout));
	let icon = if on { icons::CHAIN_LINKED } else { icons::CHAIN_PARTED };
	let clicked = glyph_button(&mut ui, id, icon, Some(ink), true, hint).clicked();
	if clicked {
		f.cx.send(b.event(linked, !on));
	}
}

/// The lens's shape rows; with `fresnel` (the Fresnel lens it belongs to) the zone rows instead of the Concave
/// box, whose flip would hollow the zones.
fn lens<W: Fn(m::LensEdit) -> Edit, Z: Fn(m::FresnelLensEdit) -> Edit>(
	f: &mut Form<'_>,
	l: &Scope<'_, Lens, W>,
	fresnel: Option<&Scope<'_, FresnelLens, Z>>,
) {
	let s = f.cx.s;
	let r = f.cx.r;
	let (r1, r2) = (l.obj.r1, l.obj.r2);
	f.section(s.get("SectionShape"), |f| {
		l.number(f, Lens::DIAMETER, 10.);
		if fresnel.is_none() {
			let id = f.cx.id("Lens.IsConcave");
			let flip = f.full(s.get("HintLensConcave"), |ui, _| check_box(ui, id, s.get("LabelLensConcave"), r1 < 0.));
			if flip.is_some() {
				f.cx.send(l.event(Lens::R1, -r1));
				f.cx.send(l.event(Lens::R2, -r2));
			}
		}
		l.number(f, Lens::R1, 10.);
		l.number(f, Lens::R2, 10.);
		l.number(f, Lens::THICKNESS, 1.);
		if let Some(z) = fresnel {
			z.number(f, FresnelLens::ZONES, 1.);
			z.combo(f, FresnelLens::DIVISION);
		}
		let edge = r.lens_edge.map(px).unwrap_or_default();
		f.readout(s.get("LabelEdgeThickness"), &edge, s.get("TipEdgeThickness"));
	});
}

fn triangle<W: Fn(m::TriangleEdit) -> Edit>(f: &mut Form<'_>, t: &Scope<'_, Triangle, W>) {
	let s = f.cx.s;
	let r = f.cx.r;
	let kind = t.obj.kind;
	let a_on = matches!(kind, TriangleType::Isosceles | TriangleType::Custom);
	f.section(s.get("SectionShape"), |f| {
		t.combo(f, Triangle::KIND);
		t.number(f, Triangle::BASE, 10.);
		t.decimals(f.on(a_on), Triangle::ANGLE_A, 0.5, 1);
		t.decimals(f.on(kind == TriangleType::Custom), Triangle::ANGLE_B, 0.5, 1);
		let apex = r.apex.map(|a| format!("{} deg", one_decimal(a))).unwrap_or_default();
		f.readout(s.get("LabelApexAngle"), &apex, s.get("TipApexAngle"));
	});
}

/// A meta surface's form: the plate's material (tops and back coated), its shape, the design's section (a lens's,
/// a prism's or an axicon's) and the pattern.
fn meta_surface<W: Fn(m::MetaSurfaceEdit) -> Edit>(f: &mut Form<'_>, ms: &Scope<'_, MetaSurface, W>) {
	let s = f.cx.s;
	let r = f.cx.r;
	let design = ms.nested(MetaSurface::DESIGN);
	let MetaReadouts { full_turn, zones, zone_width, line } = r.meta.unwrap_or_default();
	let wavelength = r.wavelength.map(px).unwrap_or_default();
	let zones = zones.to_string();
	let zone_width = zone_width.map(px).unwrap_or_default();
	let (mat, coat) = (ms.nested(MetaSurface::MATERIAL), ms.nested(MetaSurface::COATING));
	material(f, &mat, &coat, CoatedFaces::FrontBack);
	f.section(s.get("SectionShape"), |f| {
		ms.number(f, MetaSurface::LENGTH, 10.);
		ms.number(f, MetaSurface::WIDTH, 10.);
	});
	let head = |f: &mut Form<'_>| {
		ms.decimals(f, MetaSurface::DESIGN_FREQUENCY, 0.1, 2);
		f.readout(s.get("LabelWavelength"), &wavelength, s.get("TipGateWavelength"));
	};
	let zone_rows = |f: &mut Form<'_>, width: bool| {
		if width {
			f.readout(s.get("LabelZoneWidth"), &zone_width, s.get("TipZoneWidth"));
		}
		f.readout(s.get("LabelMetaZones"), &zones, s.get("TipMetaZones"));
	};
	match design.obj {
		MetaDesign::Lens(inner) => {
			let l = design.variant_over(MetaDesign::LENS, inner);
			f.section(s.get("SectionLens"), |f| {
				head(f);
				l.number(f, MetaLensDesign::FOCAL_LENGTH, 10.);
				l.combo(f, MetaLensDesign::LENS_TYPE);
				zone_rows(f, false);
			});
		}
		MetaDesign::Prism(inner) => {
			let p = design.variant_over(MetaDesign::PRISM, inner);
			f.section(s.get("SectionPrism"), |f| {
				head(f);
				p.decimals(f, MetaPrismDesign::DEFLECTION, 0.5, 2);
				zone_rows(f, true);
			});
		}
		MetaDesign::Axicon(inner) => {
			let a = design.variant_over(MetaDesign::AXICON, inner);
			f.section(s.get("SectionAxicon"), |f| {
				head(f);
				a.decimals(f, MetaAxiconDesign::CONE_ANGLE, 0.5, 2);
				let line = line.map(px).unwrap_or_default();
				f.readout(s.get("LabelLineLength"), &line, s.get("TipLineLength"));
				zone_rows(f, true);
			});
		}
	}
	f.section(s.get("SectionPattern"), |f| {
		ms.number(f, MetaSurface::PATTERN_INDEX, 0.01);
		ms.decimals(f, MetaSurface::PITCH, 0.1, 2);
		ms.decimals(f, MetaSurface::HEIGHT, 0.1, 2);
		let full_turn = full_turn.map(px).unwrap_or_default();
		f.readout(s.get("LabelFullTurnHeight"), &full_turn, s.get("TipFullTurnHeight"));
	});
}

fn zone_plate<W: Fn(m::ZonePlateEdit) -> Edit>(f: &mut Form<'_>, z: &Scope<'_, ZonePlate, W>) {
	let s = f.cx.s;
	let r = f.cx.r;
	let (count, outer) = r.zones.map_or((String::new(), String::new()), |(n, o)| (n.to_string(), approx_px(o)));
	f.section(s.get("SectionDesign"), |f| {
		z.decimals(f, ZonePlate::DESIGN_FREQUENCY, 0.1, 2);
		let wavelength = r.wavelength.map(approx_px).unwrap_or_default();
		f.readout(s.get("LabelWavelength"), &wavelength, s.get("TipGateWavelength"));
		z.number(f, ZonePlate::FOCAL_LENGTH, 10.);
		z.check(f, ZonePlate::INVERSE);
	});
	f.section(s.get("SectionShape"), |f| {
		z.number(f, ZonePlate::DIAMETER, 10.);
		z.number(f, ZonePlate::THICKNESS, 1.);
		z.number(f, ZonePlate::COATING, 1.);
		z.check(f, ZonePlate::MOUNT);
		z.number(f.on(z.obj.mount), ZonePlate::MOUNT_WIDTH, 10.);
	});
	f.section(s.get("SectionZones"), |f| {
		f.readout(s.get("LabelZoneCount"), &count, s.get("TipZones"));
		f.readout(s.get("LabelOuterZone"), &outer, s.get("TipZones"));
	});
}

/// A box for one number of item `index` of `list`, `width` wide, with `p`'s hint as tooltip.
fn list_value<T: Fields, U: Fields + Clone, V: Number, W: Fn(T::Edit) -> Edit>(
	ui: &mut Ui,
	cx: &mut Cx<'_>,
	list: &List<'_, T, U, W>,
	index: usize,
	p: Prop<U, V>,
	spec: NumberSpec,
	width: f32,
) {
	let s = cx.s;
	let value = (p.get)(&list.items[index]).to_f64();
	let id = list.id(cx, index, p.field.name());
	let label = s.label(U::TYPE_NAME, p.field.name());
	let event = |v: f64| list.set_item(index, p, V::from_f64(v));
	let response = cx.number(ui, id, value, spec, false, Some(width), label, false, event);
	response.on_hover_text(s.hint(U::TYPE_NAME, p.field.name()));
}

/// The button that removes item `index` of `list`.
fn remove_button<T: Fields, U: Clone, W: Fn(T::Edit) -> Edit>(
	ui: &mut Ui,
	cx: &mut Cx<'_>,
	list: &List<'_, T, U, W>,
	index: usize,
	tip: &str,
) {
	if glyph_button(ui, list.id(cx, index, "Remove"), icons::MINUS, None, false, tip).clicked() {
		cx.send(list.set(|items| {
			items.remove(index);
		}));
	}
}

/// The button that moves item `index` of `list` by `by` places.
fn move_button<T: Fields, U: Clone, W: Fn(T::Edit) -> Edit>(
	ui: &mut Ui,
	cx: &mut Cx<'_>,
	list: &List<'_, T, U, W>,
	index: usize,
	by: isize,
	tip: &str,
) {
	let icon = if by < 0 { icons::CHEVRON_UP } else { icons::CHEVRON_DOWN };
	if glyph_button(ui, list.id(cx, index, if by < 0 { "Up" } else { "Down" }), icon, None, false, tip).clicked() {
		cx.send(list.set(|items| {
			let to = index as isize + by;
			if (0..items.len() as isize).contains(&to) {
				items.swap(index, to as usize);
			}
		}));
	}
}

/// The width each of `n` boxes gets in `ui`'s row after `fixed` and the gaps.
fn share(ui: &Ui, n: usize, fixed: f32, gaps: usize) -> f32 {
	((ui.available_width() - fixed - gaps as f32 * ROW_SPACING) / n as f32).max(20.)
}

/// A gap between a list row's parts.
fn gap(ui: &mut Ui) {
	ui.add_space(ROW_SPACING);
}

fn item_label<U: Fields, V>(s: &wavedude_app_state::strings::Strings, p: &Prop<U, V>) -> &'static str {
	s.label(U::TYPE_NAME, p.field.name())
}

/// An outline's points: an X / Y heading, a full-width row per point with X, Y and remove, Add point below.
fn points<W: Fn(m::OutlineEdit) -> Edit>(f: &mut Form<'_>, list: &List<'_, Outline, Point, W>) {
	let s = f.cx.s;
	f.section(s.get("SectionPoints"), |f| {
		f.paragraph(s.get("HintPolygonPoints"));
		if !list.items.is_empty() {
			f.full("", |ui, _| {
				let w = share(ui, 2, ROW_BUTTON_WIDTH, 2);
				caption(ui, item_label(&s, &Point::X), Some(w), true);
				gap(ui);
				caption(ui, item_label(&s, &Point::Y), Some(w), true);
			});
		}
		let spec = NumberSpec::integer(-1e6, 1e6);
		for i in 0..list.items.len() {
			f.full("", |ui, cx| {
				let w = share(ui, 2, ROW_BUTTON_WIDTH, 2);
				list_value(ui, cx, list, i, Point::X, spec.clone(), w);
				gap(ui);
				list_value(ui, cx, list, i, Point::Y, spec.clone(), w);
				gap(ui);
				remove_button(ui, cx, list, i, s.get("TipRemovePoint"));
			});
		}
		f.full(s.get("TipAddPoint"), |ui, cx| {
			if button(ui, s.get("ButtonAddPoint"), true).clicked() {
				cx.send(list.set(|items| {
					let last = items.last().copied().unwrap_or_default();
					items.push(Point::new(last.x + 10, last.y));
				}));
			}
		});
	});
}

/// A layer list's drag by a grip: the pointer's y and the grip's top at the press, the layer's index now.
#[derive(Clone, Copy)]
struct LayerDrag {
	y0: f32,
	top0: f32,
	current: usize,
}

/// An interference filter's layers, the outermost first: a row each with a grip, its thickness and index, up,
/// down and remove; Add layer appends a copy of the last, Remove all empties the list in one step. A drag by a
/// row's grip moves the layer through the list: the row whose top lies nearest the grip's top plus the drag is
/// the target, each crossing is one edit, and the mouse held keeps them one undo step.
fn layers<W: Fn(m::InterferenceFilterEdit) -> Edit>(
	f: &mut Form<'_>,
	list: &List<'_, InterferenceFilter, FilterLayer, W>,
) {
	let s = f.cx.s;
	let (tmin, tmax) = bounds_of(FilterLayer::THICKNESS);
	let (imin, imax) = bounds_of(FilterLayer::REFRACTIVE_INDEX);
	let drag_id = list.id(&f.cx, 0, "Drag");
	let mut drag = f.ui.data(|d| d.get_temp::<LayerDrag>(drag_id));
	let mut tops = Vec::new();
	f.section(s.get("SectionLayers"), |f| {
		f.paragraph(s.get("HintFilterLayers"));
		if !list.items.is_empty() {
			f.full("", |ui, _| {
				let w = share(ui, 2, LAYER_FIXED, 4);
				ui.add_space(GRIP_WIDTH + ROW_SPACING + ORDINAL_WIDTH + ROW_SPACING);
				caption(ui, item_label(&s, &FilterLayer::THICKNESS), Some(w), true);
				gap(ui);
				caption(ui, item_label(&s, &FilterLayer::REFRACTIVE_INDEX), Some(w), true);
			});
		}
		let thickness = NumberSpec::linear(tmin, tmax, 1.).with_decimals(2).with_drag_step(0.1);
		let index = NumberSpec::linear(imin, imax, 0.01).with_decimals(2).with_drag_step(0.01);
		for i in 0..list.items.len() {
			f.full("", |ui, cx| {
				let palette = Palette::of(ui.ctx().theme());
				let w = share(ui, 2, LAYER_FIXED, 4);
				let (grip, _) = ui.allocate_exact_size(vec2(GRIP_WIDTH, 22.), eframe::egui::Sense::hover());
				tops.push(grip.top());
				let grip_id = list.id(cx, i, "Grip");
				let response = ui
					.interact(grip, grip_id, eframe::egui::Sense::drag())
					.on_hover_text(s.get("TipDragLayer"))
					.on_hover_cursor(eframe::egui::CursorIcon::ResizeRow);
				if response.drag_started()
					&& let Some(p) = response.interact_pointer_pos()
				{
					drag = Some(LayerDrag { y0: p.y, top0: grip.top(), current: i });
					cx.send(Event::MouseDown);
				}
				let center = grip.center();
				let glyph = eframe::egui::Rect::from_center_size(center, vec2(16., 16.));
				icons::paint(ui, glyph, icons::GRIP, palette.text_dim);
				gap(ui);
				caption(ui, &(i + 1).to_string(), Some(ORDINAL_WIDTH), true);
				gap(ui);
				list_value(ui, cx, list, i, FilterLayer::THICKNESS, thickness.clone(), w);
				gap(ui);
				list_value(ui, cx, list, i, FilterLayer::REFRACTIVE_INDEX, index.clone(), w);
				gap(ui);
				move_button(ui, cx, list, i, -1, s.get("TipLayerUp"));
				gap(ui);
				move_button(ui, cx, list, i, 1, s.get("TipLayerDown"));
				gap(ui);
				remove_button(ui, cx, list, i, s.get("TipRemoveLayer"));
			});
		}
		f.full("", |ui, cx| {
			let add = button(ui, s.get("ButtonAddLayer"), true).on_hover_text(s.get("TipAddLayer"));
			if add.clicked() {
				cx.send(list.set(|items| items.push(items.last().cloned().unwrap_or_default())));
			}
			ui.add_space(6.);
			let clear = button(ui, s.get("ButtonRemoveAllLayers"), !list.items.is_empty());
			if clear.on_hover_text(s.get("TipRemoveAllLayers")).clicked() {
				cx.send(list.set(Vec::clear));
			}
		});
	});
	let (down, y) = f.ui.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos().map(|p| p.y)));
	if let Some(d) = &mut drag {
		if !down {
			drag = None;
			f.cx.send(Event::MouseUp);
		} else if let Some(y) = y {
			let wanted = d.top0 + (y - d.y0);
			let nearest = tops
				.iter()
				.enumerate()
				.min_by(|a, b| (a.1 - wanted).abs().total_cmp(&(b.1 - wanted).abs()))
				.map(|(i, _)| i);
			if let Some(target) = nearest.filter(|&t| t != d.current && t < list.items.len()) {
				let current = d.current;
				f.cx.send(list.set(|l| {
					let layer = l.remove(current);
					l.insert(target, layer);
				}));
				d.current = target;
			}
		}
	}
	match drag {
		Some(d) => f.ui.data_mut(|data| {
			data.insert_temp(drag_id, d);
		}),
		None => f.ui.data_mut(|data| data.remove::<LayerDrag>(drag_id)),
	}
}

fn fiber<W: Fn(m::FiberEdit) -> Edit>(f: &mut Form<'_>, fb: &Scope<'_, Fiber, W>) {
	let s = f.cx.s;
	f.section(s.get("SectionCore"), |f| {
		fb.number(f, Fiber::CORE_INDEX, 0.01);
		fb.number(f, Fiber::CORE_DIAMETER, 1.);
	});
	f.section(s.get("SectionCladding"), |f| {
		fb.number(f, Fiber::CLADDING_INDEX, 0.01);
		fb.number(f, Fiber::CLADDING_THICKNESS, 1.);
		fb.number(f, Fiber::ABSORPTION, 5.);
		fb.number(f, Fiber::ABSORPTION_RAMP, 1.);
		coating_rows(f, &fb.nested(Fiber::COATING), false, CoatedFaces::All);
	});
	f.section(s.get("SectionShape"), |f| fb.combo(f, Fiber::ENDS));
	let list = fb.list(Fiber::NODES, &fb.obj.nodes);
	f.section(s.get("SectionNodes"), |f| {
		f.paragraph(s.get("HintFiberNodes"));
		// a node is three rows, "Node", "In" and "Out", each a label and its X and Y; the first carries Smooth and
		// remove
		if !list.items.is_empty() {
			f.full("", |ui, _| {
				let w = share(ui, 2, NODE_LABEL_WIDTH + NODE_TAIL_WIDTH, 3);
				ui.add_space(NODE_LABEL_WIDTH + ROW_SPACING);
				caption(ui, item_label(&s, &FiberNode::X), Some(w), true);
				gap(ui);
				caption(ui, item_label(&s, &FiberNode::Y), Some(w), true);
			});
		}
		let spec = NumberSpec::integer(-1e6, 1e6);
		type Pair = (Prop<FiberNode, i32>, Prop<FiberNode, i32>);
		let rows: [(&str, Pair); 3] = [
			("LabelNodePoint", (FiberNode::X, FiberNode::Y)),
			("LabelNodeIn", (FiberNode::IN_X, FiberNode::IN_Y)),
			("LabelNodeOut", (FiberNode::OUT_X, FiberNode::OUT_Y)),
		];
		for (i, node) in list.items.iter().enumerate() {
			for (k, (key, (x, y))) in rows.into_iter().enumerate() {
				f.full("", |ui, cx| {
					let w = share(ui, 2, NODE_LABEL_WIDTH + NODE_TAIL_WIDTH, 3);
					caption(ui, s.get(key), Some(NODE_LABEL_WIDTH), true);
					gap(ui);
					list_value(ui, cx, &list, i, x, spec.clone(), w);
					gap(ui);
					list_value(ui, cx, &list, i, y, spec.clone(), w);
					gap(ui);
					if k == 0 {
						let box_width = NODE_TAIL_WIDTH - ROW_BUTTON_WIDTH - ROW_SPACING;
						ui.allocate_ui(vec2(box_width, 23.), |ui| {
							let id = list.id(cx, i, "Smooth");
							if let Some(on) = check_box(ui, id, item_label(&s, &FiberNode::SMOOTH), node.smooth) {
								cx.send(list.set_item(i, FiberNode::SMOOTH, on));
							}
						});
						let used = ui.min_rect().right() - ui.max_rect().left();
						let pad = (ui.max_rect().width() - used - ROW_BUTTON_WIDTH).max(0.);
						ui.add_space(pad);
						remove_button(ui, cx, &list, i, s.get("TipRemoveNode"));
					}
				});
			}
		}
		f.full(s.get("TipAddNode"), |ui, cx| {
			if button(ui, s.get("ButtonAddNode"), true).clicked() {
				cx.send(list.set(|l| {
					let (x, y) = l.last().map_or((0, 0), |n| (n.x + 50, n.y));
					l.push(FiberNode { x, y, in_x: -20, in_y: 0, out_x: 20, out_y: 0, smooth: true });
				}));
			}
		});
	});
}

fn gate<W: Fn(m::LogicGateEdit) -> Edit>(f: &mut Form<'_>, g: &Scope<'_, LogicGate, W>) {
	let s = f.cx.s;
	let r = f.cx.r;
	let design = r.gate;
	let layout = design.map_or([0; 4], |d| d.layout);
	let ports = design.map_or([Point::default(); 4], |d| d.ports);
	let quarter = design.map(|d| d.quarter_wave).filter(|q| q.is_finite()).map(approx_px).unwrap_or_default();
	// the box shows the spacing the design uses; an edit stores the typed value
	let designed = design
		.map(|d| LogicGate { port_spacing: u32::try_from(d.spacing).unwrap_or(g.obj.port_spacing), ..g.obj.clone() });
	let (upper, lower) = match g.obj.kind {
		m::GateKind::Xor => ("Xor", "And"),
		m::GateKind::And => ("And", "Xor"),
		m::GateKind::Or => ("Or", "Remainder"),
	};
	f.section(s.get("SectionDesign"), |f| {
		g.decimals(f, LogicGate::DESIGN_FREQUENCY, 0.1, 2);
		let wavelength = r.wavelength.map(approx_px).unwrap_or_default();
		f.readout(s.get("LabelWavelength"), &wavelength, s.get("TipGateWavelength"));
	});
	f.section(s.get("SectionMaterial"), |f| {
		g.number(f, LogicGate::CORE_INDEX, 0.01);
		g.number(f, LogicGate::CLADDING_INDEX, 0.01);
		let (min, max) = bounds_of(LogicGate::COATING);
		g.number_with(f, LogicGate::COATING, NumberSpec::integer(min, max));
		g.check(f, LogicGate::INPUT_COATED);
		g.check(f, LogicGate::OUTPUT_COATED);
		f.readout(s.get("LabelQuarterWave"), &quarter, s.get("TipQuarterWave"));
	});
	f.section(s.get("SectionShape"), |f| {
		g.over(designed.as_ref().unwrap_or(g.obj)).number(f, LogicGate::PORT_SPACING, 2.);
	});
	f.section(s.get("SectionOutputs"), |f| {
		g.check_text(f, LogicGate::UPPER_OUTPUT_ACTIVE, s.enum_word("GateOutput", upper));
		g.check_text(f, LogicGate::LOWER_OUTPUT_ACTIVE, s.enum_word("GateOutput", lower));
	});
	f.section(s.get("SectionLayout"), |f| {
		for (key, v) in
			["LabelCoreWidth", "LabelCouplerGap", "LabelCouplerLength", "LabelGateLength"].iter().zip(layout)
		{
			f.readout(s.get(key), &format!("{v} px"), s.get("TipGateLayout"));
		}
	});
	f.section(s.get("SectionPorts"), |f| {
		let labels = [
			s.get("LabelInputA"),
			s.get("LabelInputB"),
			g.label(&s, &LogicGate::UPPER_OUTPUT_ACTIVE),
			g.label(&s, &LogicGate::LOWER_OUTPUT_ACTIVE),
		];
		for (text, q) in labels.iter().zip(ports) {
			f.readout(text, &format!("({}, {})", q.x, q.y), s.get("TipGatePorts"));
		}
	});
}

fn probe(f: &mut Form<'_>, o: &Probe, row: Row, index: usize) {
	let s = f.cx.s;
	let p = Scope::new(o, move |e| Edit::Probe(index, e));
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
	f.section(s.get("SectionGeneral"), |f| {
		f.name::<Probe>(row, &o.name);
		p.check(f, Probe::ACTIVE);
		p.number(f, Probe::START_AFTER, 1.);
	});
	// a detector is a line probe of the integrated intensity: the three after its own are shown as it set them
	f.section(s.get("SectionMeasurement"), |f| {
		p.combo(f, Probe::DETECTOR);
		p.decimals(f.on(is_detector), Probe::SENSITIVITY, 0.01, 4);
		p.number(f.on(o.detector == ProbeDetector::Imaging), Probe::BIN_SIZE, 1.);
		p.combo(f.on(!is_detector), Probe::LAYER);
		p.combo_of(f.on(!is_detector), Probe::MEASUREMENT, measurements);
		p.combo(f.on(!is_detector), Probe::KIND);
	});
	f.section(s.get("SectionPosition"), |f| {
		p.number_in(f, Probe::AX, at_least_0, 1.);
		p.number_in(f, Probe::AY, at_least_0, 1.);
		p.number_in(f.on(line), Probe::BX, at_least_0, 1.);
		p.number_in(f.on(line), Probe::BY, at_least_0, 1.);
	});
	f.section(s.get("SectionGraph"), |f| {
		p.combo(f, Probe::BOX_SIZE);
		p.number_in(f, Probe::BOX_X, at_least_0, 1.);
		p.number_in(f, Probe::BOX_Y, at_least_0, 1.);
		p.number(f.on(custom), Probe::BOX_WIDTH, 10.);
		p.number(f.on(custom), Probe::BOX_HEIGHT, 10.);
	});
	f.section(s.get("SectionAxes"), |f| {
		p.combo(f.on(!profile), Probe::SCALE_MODE);
		p.number(f, Probe::WINDOW, 10.);
		p.check(f.on(!profile), Probe::AUTO_SCALE_X);
		p.check(f, Probe::AUTO_SCALE_Y);
		p.combo(f.on(!field), Probe::Y_SCALE);
		p.number(f.on(log), Probe::DECADES, 1.);
		p.number(f, Probe::Y_MIN, 0.1);
		p.number(f, Probe::Y_MAX, 0.1);
	});
}

fn scheme(f: &mut Form<'_>, o: &ColorScheme, row: Row, index: usize) {
	let s = f.cx.s;
	let r = f.cx.r;
	let sc = Scope::new(o, move |e| Edit::Scheme(index, e));
	let gradient = Gradient::of(&o.stops);
	let set = |g: Gradient<'_>| sc.event(ColorScheme::STOPS, g.into_stops());
	f.section(s.get("SectionGeneral"), |f| {
		f.name::<ColorScheme>(row, &o.name);
		sc.combo(f, ColorScheme::KIND);
		sc.text(f, ColorScheme::DESCRIPTION, &o.description, false);
		let key = if r.user_scheme { "MenuSchemeRemoveFromMine" } else { "MenuSchemeAddToMine" };
		f.full("", |ui, cx| {
			if button(ui, s.get(key), true).clicked() {
				cx.send(Event::ToggleUserScheme(index));
			}
		});
	});
	f.section(s.get("SectionStops"), |f| {
		let label = s.get("LabelBasedOn");
		let picked = f.row(label, s.get("TipBasedOn"), |ui, cx| {
			let word = |i: usize| -> Cow<'_, str> { r.based_on[i].as_str().into() };
			let choices = Choices {
				len: r.based_on.len(),
				current: None,
				shown: "",
				word: &word,
				swatch: None,
				disabled: None,
				actions: &[],
			};
			combo(ui, cx.id("ColorScheme.BasedOn"), label, &choices)
		});
		if let Some(source) = picked.and_then(|i| find_scheme(f.cx.app, &r.based_on[i])) {
			f.cx.send(sc.event(ColorScheme::STOPS, source.stops.clone()));
			f.cx.send(sc.event(ColorScheme::KIND, source.kind));
			f.cx.send(sc.event(ColorScheme::DESCRIPTION, source.description.clone()));
		}
		let stops = f.full(s.get("TipSchemeGradient"), |ui, cx| {
			gradient_editor(ui, cx.id("SchemeGradient"), &gradient, s.get("TipSchemeGradient"))
		});
		if let Some(stops) = stops {
			f.cx.send(sc.event(ColorScheme::STOPS, stops));
		}
		for (i, stop) in gradient.stops.iter().enumerate() {
			f.full("", |ui, cx| stop_row(ui, cx, (&gradient, i, *stop), r.can_remove_stop, &set));
		}
		f.full(s.get("TipAddStop"), |ui, cx| {
			if button(ui, s.get("ButtonAddStop"), true).clicked() {
				let mut g = gradient.clone();
				g.add_in_widest_gap();
				cx.send(set(g));
			}
		});
	});
}

/// Stop `i`'s row: its ordinal, position, swatch, hex text and remove button; `set` makes the event of the
/// gradient an edit leaves.
fn stop_row(
	ui: &mut Ui,
	cx: &mut Cx<'_>,
	(gradient, i, stop): (&Gradient<'_>, usize, Stop),
	removable: bool,
	set: &dyn Fn(Gradient<'_>) -> Event,
) {
	let s = cx.s;
	let event = |change: &dyn Fn(&mut Gradient<'_>)| {
		let mut g = gradient.clone();
		change(&mut g);
		set(g)
	};
	let recolor = |c: m::Rgb| event(&|g| g.set_color(i, c));
	caption(ui, &(i + 1).to_string(), Some(STOP_ORDINAL), false);
	gap(ui);
	let spec = NumberSpec::linear(0., 1., 0.01).with_decimals(3);
	let id = cx.id(("Stop", i, "Position"));
	let tip = s.get("TipStopPosition");
	let moved = |v: f64| {
		event(&|g| {
			g.set_position(i, v);
		})
	};
	cx.number(ui, id, stop.at, spec, false, Some(STOP_POSITION), tip, false, moved).on_hover_text(tip);
	gap(ui);
	let swatch = cx.id(("Stop", i, "Color"));
	if let Some(c) = color_button(ui, swatch, stop.color, s.get("TipStopSwatch"), &s, &recolor) {
		cx.send(recolor(c));
	}
	gap(ui);
	let hex_width = ui.available_width() - ROW_BUTTON_WIDTH - ROW_SPACING;
	let id = cx.id(("Stop", i, "Hex"));
	let tip = s.get("TipStopColor");
	let edited = text_box(ui, id, &stop.color.to_hex(), hex_width, Kind::default(), tip);
	if let Some(c) = edited.applied.as_deref().and_then(parse_hex) {
		cx.send(recolor(c));
	}
	if edited.response.has_focus() || edited.response.lost_focus() {
		edit::offer(ui.ctx(), id, edited.typed.as_deref().and_then(parse_hex).map(recolor));
	}
	edited.response.on_hover_text(tip);
	gap(ui);
	ui.add_enabled_ui(removable, |ui| {
		if glyph_button(ui, cx.id(("Stop", i, "Remove")), icons::MINUS, None, false, s.get("TipRemoveStop")).clicked() {
			cx.send(event(&|g| {
				g.remove(i);
			}));
		}
	});
}
