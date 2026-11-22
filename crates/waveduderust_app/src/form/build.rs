//! The forms of docs/ui/30, row by row.

use std::borrow::Cow;

use crate::Event;
use crate::commands::{Command, Edit, NewPart};
use crate::platform::HAS_PATHS;
use crate::selection::Row as TreeRow;
use waveduderust::io::schemes;
use waveduderust::model::{
	self as m, BoxSize, BraggReflector, Coating, ColorScheme, Diffuser, DisplayLayer, Ellipse, FacettedGrating, Fiber,
	FiberNode, Fields, FilterLayer, FresnelLens, GrinLens, InterferenceFilter, ItemRef, Layer, Lens, LogicGate,
	Material, Measurement, MetaAxiconDesign, MetaDesign, MetaLensDesign, MetaPrismDesign, MetaSurface, Outline, Part,
	PartBase, PartKind, Point, Polygon, Probe, ProbeDetector, ProbeKind, Project, ProjectEdit, Prop, Rectangle, Scene,
	SceneMode, SimpleGrating, Simulation, Source, SourceArray, SourceMode, Stencil, Triangle, TriangleType, Video,
	View, YScale, ZonePlate, ranges,
};

use super::{
	Builder, Check, Choice, Color, Cx, Dialog, GradientBar, LayerRow, Layers, List, NodeRow, Nodes, Number, Out,
	PointRow, Points, Request, RowKind, Scope, Setter, StopRow, Text, button, glyph, key,
};
use crate::form::Glyph;
use crate::gradient::{Gradient, parse_hex};
use crate::number::NumberSpec;
use crate::properties::{MetaReadouts, SchemeEntry, bounds_of};

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

/// The form of `row`; none for a folder or an item that is gone.
pub fn build<'a>(cx: Cx<'a>, row: TreeRow) -> Option<super::Form<'a>> {
	let (p, v) = (&cx.app.document.project, &cx.app.document.view);
	let mut f = Builder::new(cx);
	match row {
		TreeRow::Project => project(&mut f, p),
		TreeRow::Scene => scene(&mut f, &p.scene),
		TreeRow::Simulation => simulation(&mut f, &p.simulation),
		TreeRow::Display => view(&mut f, v),
		TreeRow::Item(i) => match i {
			ItemRef::Source(id) => source(&mut f, p.source(id)?, i),
			ItemRef::Part(id) => part(&mut f, p.part(id)?, i),
			ItemRef::Probe(id) => probe(&mut f, p.probe(id)?, i),
			ItemRef::Scheme(id) => scheme(&mut f, p.color_scheme(id)?, i),
			_ => return None,
		},
		TreeRow::Folder(_) => return None,
	}
	Some(f.finish())
}

fn project<'a>(f: &mut Builder<'a>, o: &'a Project) {
	let s = f.cx.s;
	let p = Scope::new(o, Edit::Project);
	f.section(s.get("SectionGeneral"), |f| {
		f.value(s.get("LabelProjectVersion"), m::PROJECT_VERSION.to_string(), "");
		let seed = p.number_box(&s, Project::SEED, NumberSpec::integer(f64::from(i32::MIN), f64::from(i32::MAX)));
		let set = p.setter(Project::SEED);
		let again = button(key("NewSeed"), s.get("ButtonNewSeed"), "", true, move || {
			vec![Out::Send(set(Project::new_random_seed().seed))]
		});
		f.push(Some(seed.label), seed.hint, RowKind::NumberButton(seed, again));
		p.text(f, Project::DESCRIPTION, &o.description, true);
	});
}

/// The image's name, editable as Next's, and its browse, save and remove buttons.
fn image_row<'a, W: Fn(m::SceneEdit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	sc: &Scope<'a, Scene, W>,
	has_bytes: bool,
) {
	let s = f.cx.s;
	let label = sc.label(&s, &Scene::IMAGE);
	let set = sc.setter(Scene::IMAGE);
	let name = Text {
		key: sc.key(Scene::IMAGE, String::new()),
		label,
		value: sc.obj.image.as_str().into(),
		multiline: false,
		live: true,
		first: false,
		apply: Box::new(move |t| Some(set(t))),
	};
	let id = name.key;
	let ask = |d| move || vec![Out::Ask(Request::Dialog(d))];
	let browse = button(key((id, "browse")), "...", s.get("TipBrowseSceneImage"), true, ask(Dialog::PickSceneImage));
	let save =
		glyph(key((id, "save")), Glyph::Save, s.get("TipSaveSceneImage"), has_bytes, ask(Dialog::SaveSceneImage));
	let remove = glyph(key((id, "remove")), Glyph::Close, s.get("TipRemoveSceneImage"), has_bytes, || {
		vec![Out::Send(Event::Command(Command::RemoveSceneImage))]
	});
	f.push(Some(label), sc.hint(&s, &Scene::IMAGE), RowKind::Image { name, browse, save, remove });
}

fn scene<'a>(f: &mut Builder<'a>, o: &'a Scene) {
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
				f.readout(sc.label(&s, &Scene::IMAGE_PATH), shown, sc.hint(&s, &Scene::IMAGE_PATH));
			}
			let size = r.image_size.map_or_else(
				|| s.get("ImageUnreadable").to_owned(),
				|(w, h, kb)| format!("{w} x {h}, {} KB", s.grouped(kb as u64)),
			);
			f.readout(s.get("LabelEmbeddedImage"), size, s.get("TipEmbeddedImage"));
		}
		let (smin, smax) = bounds_of(Scene::WIDTH);
		sc.number_with(f.on(!image_mode), Scene::WIDTH, NumberSpec::integer(smin, smax));
		sc.number_with(f.on(!image_mode), Scene::HEIGHT, NumberSpec::integer(smin, smax));
		// a rim outside the canvas ends in the wall for either kind (physics.md, Absorbing boundaries)
		sc.combo(f.on(o.damping_edge >= 0), Scene::EDGES);
		sc.number(f, Scene::DAMPING_EDGE, 1.);
		sc.number(f, Scene::REFRACTIVE_INDEX, 0.01);
	});
	f.section(s.get("SectionImageSources"), |f| {
		sc.check(f, Scene::IMAGE_SOURCES);
		sc.number(f.on(o.image_sources), Scene::IMAGE_SOURCE_SCALE, 0.1);
		sc.check(f, Scene::OBJECT_SOURCES);
	});
}

fn simulation<'a>(f: &mut Builder<'a>, o: &'a Simulation) {
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
fn layer_rows<'a, W: Fn(m::DisplayLayerEdit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	l: &Scope<'a, DisplayLayer, W>,
	names: &'a [SchemeEntry],
	mine: Option<&'a str>,
) {
	let s = f.cx.s;
	let app = f.cx.app;
	let current = l.obj.color_scheme.as_str();
	let listed = names.iter().position(|e| e.name == current);
	// a name no scheme has is shown first, greyed
	let missing = usize::from(!current.is_empty() && listed.is_none());
	let word = |i: usize| -> Cow<'a, str> {
		if missing == 1 && i == 0 {
			return s.format("PickerMissingFormat", &[&current]).into();
		}
		let e = &names[i - missing];
		if e.own { format!("{} ({})", e.name, s.get("PickerProjectTag")).into() } else { e.name.as_str().into() }
	};
	let len = names.len() + missing;
	let mut actions = vec![s.get("PickerBrowse"), s.get("PickerEditCopy")];
	if mine.is_some() {
		actions.push(s.get("PickerRemoveFromMine"));
	}
	let label = l.label(&s, &DisplayLayer::COLOR_SCHEME);
	let set = l.setter(DisplayLayer::COLOR_SCHEME);
	let choice = Choice {
		key: l.key(DisplayLayer::COLOR_SCHEME, String::new()),
		label,
		words: (0..len).map(word).collect(),
		current: (!current.is_empty()).then(|| listed.map_or(0, |i| i + missing)),
		shown: word(listed.map_or(0, |i| i + missing)),
		swatches: (0..len).map(|i| (i >= missing).then(|| names[i - missing].stops.as_slice())).collect(),
		actions,
		pick: Box::new(move |i| match i {
			i if i == len => vec![Out::Ask(Request::Dialog(Dialog::PickScheme))],
			i if i == len + 1 => edit_copy(app, current, &*set),
			i if i == len + 2 => {
				mine.map(|name| Out::Send(Event::RemoveUserScheme(name.to_owned()))).into_iter().collect()
			}
			i if i >= missing && i < len && names[i - missing].name != current => {
				vec![Out::Send(set(names[i - missing].name.clone()))]
			}
			_ => Vec::new(),
		}),
	};
	f.push(Some(label), l.hint(&s, &DisplayLayer::COLOR_SCHEME), RowKind::Choice(choice));
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
fn edit_copy(app: &crate::App, current: &str, set: &dyn Fn(String) -> Event) -> Vec<Out> {
	let Some(scheme) = find_scheme(app, current) else { return Vec::new() };
	let taken = app.document.project.color_schemes.iter().map(|s| s.name.as_str());
	let name = m::names::copy_name(&scheme.name, taken);
	vec![Out::Send(Event::Command(Command::ImportScheme(Box::new(scheme.clone())))), Out::Send(set(name))]
}

fn view<'a>(f: &mut Builder<'a>, o: &'a View) {
	let s = f.cx.s;
	let r = f.cx.r;
	let d = Scope::new(o, Edit::View);
	f.section(s.get("SectionField"), |f| layer_rows(f, &d.nested(View::FIELD), &r.schemes[0], r.mine[0].as_deref()));
	f.section(s.get("SectionIntensity"), |f| {
		layer_rows(f, &d.nested(View::INTENSITY), &r.schemes[1], r.mine[1].as_deref());
	});
	f.section(s.get("SectionPreview"), |f| {
		d.check(f, View::HANDLES_IN_PREVIEW);
		d.check(f, View::LABELS_IN_PREVIEW);
	});
	f.section(s.get("SectionExportRange"), |f| {
		d.number(f, View::EXPORT_FROM, 1.);
		d.number(f, View::EXPORT_TO, 1.);
		d.check(f, View::PROBE_GRAPHS_IN_EXPORT);
		d.check(f, View::PROBE_MARKERS_IN_EXPORT);
		d.check(f, View::SOURCE_MARKERS_IN_EXPORT);
	});
	let v = d.nested(View::VIDEO);
	f.section(s.get("SectionVideo"), |f| {
		v.number(f, Video::FPS, 1.);
		v.number(f, Video::QUALITY, 1.);
	});
}

/// A plain source's array rows, shown greyed and empty.
static PLAIN_ARRAY: std::sync::LazyLock<SourceArray> = std::sync::LazyLock::new(SourceArray::default);

fn source<'a>(f: &mut Builder<'a>, o: &'a Source, item: ItemRef) {
	let s = f.cx.s;
	let r = f.cx.r;
	let index = o.id();
	let src = Scope::new(o, move |e| Edit::Source(index, e));
	let pulsed = o.mode == SourceMode::Pulsed;
	let composite = o.array.is_some();
	let arr = src.variant_or(Source::ARRAY, &PLAIN_ARRAY);
	f.section(s.get("SectionGeneral"), |f| {
		f.name::<Source>(item, &o.name);
		src.check(f, Source::ENABLED);
	});
	f.section(s.get("SectionPosition"), |f| {
		f.group(s.get("GroupPosition"), &[], |f| {
			src.decimals(f, Source::X, 1., 0);
			src.decimals(f, Source::Y, 1., 0);
		});
	});
	f.section(s.get("SectionWave"), |f| {
		src.decimals(f, Source::FREQUENCY, 0.1, 2);
		src.decimals(f, Source::AMPLITUDE, 0.1, 2);
		src.decimals(f, Source::PHASE, 0.5, 2);
		let wavelength = r.wavelength.map_or_else(String::new, |w| {
			let text = if w.is_finite() { approx_px(w) } else { "-".into() };
			if r.nominal { format!("{text} {}", s.get("WavelengthNominal")) } else { text }
		});
		f.readout(s.get("LabelWavelength"), wavelength, s.get("TipWavelength"));
	});
	f.section(s.get("SectionTiming"), |f| {
		src.combo(f, Source::MODE);
		src.combo(f.on(pulsed), Source::PULSE_SHAPE);
		src.number(f.on(pulsed), Source::PULSE_WIDTH, 1.);
		src.number(f.on(pulsed), Source::OFF_WIDTH, 1.);
		let (_, max) = bounds_of(Source::DETACH_AFTER);
		let set = src.setter(Source::DETACH_AFTER);
		let label = src.label(&s, &Source::DETACH_AFTER);
		let detach = Number {
			key: src.key(Source::DETACH_AFTER, None),
			label,
			hint: src.hint(&s, &Source::DETACH_AFTER),
			value: o.detach_after.map_or(-1., f64::from),
			spec: NumberSpec::integer(-1., max),
			unit: super::unit_text(&s, "Source", "DetachAfter"),
			blank: false,
			first: true,
			apply: Box::new(move |v| set((v >= 0.).then(|| v.round() as u32))),
		};
		f.push(Some(label), detach.hint, RowKind::Number(detach));
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
fn coating_rows<'a, W: Fn(m::CoatingEdit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	c: &Scope<'a, Coating, W>,
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
fn material<'a, WM: Fn(m::MaterialEdit) -> Edit + Clone + 'a, WC: Fn(m::CoatingEdit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	mat: &Scope<'a, Material, WM>,
	c: &Scope<'a, Coating, WC>,
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
fn part_common<'a, W: Fn(m::PartBaseEdit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	base: &Scope<'a, PartBase, W>,
	item: ItemRef,
	ty: &str,
) {
	let s = f.cx.s;
	let open = f.cx.app.settings.part_description_expanded;
	f.description(s.get("SectionDescription"), s.get("TipPartDescription"), s.kind(ty), open);
	f.section(s.get("SectionGeneral"), |f| {
		f.name::<PartBase>(item, &base.obj.name);
		base.check(f, PartBase::ENABLED);
	});
	f.section(s.get("SectionPosition"), |f| {
		f.group(s.get("GroupPosition"), &[], |f| {
			base.number(f, PartBase::X, 1.);
			base.number(f, PartBase::Y, 1.);
			let (amin, amax) = bounds_of(PartBase::ANGLE);
			let angle = NumberSpec::linear(amin, amax, 5.).with_decimals(1).with_drag_step(0.5);
			base.number_with(f, PartBase::ANGLE, angle);
		});
	});
}

fn part<'a>(f: &mut Builder<'a>, o: &'a Part, item: ItemRef) {
	let s = f.cx.s;
	let r = f.cx.r;
	let index = o.id();
	let part = Scope::new(o, move |e| Edit::Part(index, e));
	let kind = part.nested(Part::KIND);
	let Some(new_part) = NewPart::of(&o.kind) else {
		return;
	};
	part_common(f, &part.nested(Part::BASE), item, new_part.type_name());
	let shape = s.get("SectionShape");
	match &o.kind {
		PartKind::Lens(inner) => {
			let l = kind.variant_over(PartKind::LENS, inner);
			material(f, &l.nested(Lens::MATERIAL), &l.nested(Lens::COATING), CoatedFaces::All);
			lens(f, &l, None::<&Scope<'a, FresnelLens, fn(m::FresnelLensEdit) -> Edit>>);
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
			points(f, &p.list(s, Outline::POINTS, &inner.points));
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
				f.readout(s.get("LabelProfileHeight"), height, s.get("TipProfileHeight"));
				f.readout(s.get("LabelComponentCount"), ripples, s.get("TipComponentCount"));
			});
		}
		PartKind::InterferenceFilter(inner) => {
			let i = kind.variant_over(PartKind::INTERFERENCE_FILTER, inner);
			let (mat, coat) = (i.nested(InterferenceFilter::MATERIAL), i.nested(InterferenceFilter::COATING));
			material(f, &mat, &coat, CoatedFaces::Back);
			f.section(shape, |f| {
				i.number(f, InterferenceFilter::LENGTH, 10.);
				i.number(f, InterferenceFilter::WIDTH, 10.);
				let stack = r.stack.map_or_default(px);
				f.readout(s.get("LabelStackThickness"), stack, s.get("TipStackThickness"));
			});
			layers(f, &i.list(s, InterferenceFilter::LAYERS, &inner.layers));
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
		_ => {}
	}
}

/// Length and Width of a rectangle or ellipse with the chain toggle that links them beside both.
fn linked_sizes<'a, T: Fields, W: Fn(T::Edit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	b: &Scope<'a, T, W>,
	length: Prop<T, u32>,
	width: Prop<T, u32>,
	linked: Prop<T, bool>,
) {
	let s = f.cx.s;
	let on = b.get(linked);
	let row = |p| {
		let (min, max) = bounds_of(p);
		let n = b.number_box(&s, p, NumberSpec::linear(min, max, 10.).with_decimals(0));
		super::Row { label: Some(n.label), hint: n.hint, enabled: true, kind: RowKind::Number(n) }
	};
	let rows = Box::new([row(length), row(width)]);
	let toggle = b.setter(linked);
	let g = if on { Glyph::Linked } else { Glyph::Parted };
	let chain = glyph(b.key(linked, false), g, b.hint(&s, &linked), true, move || vec![Out::Send(toggle(!on))]);
	f.push(None, "", RowKind::Linked(rows, chain));
}

/// The lens's shape rows; with `fresnel` (the Fresnel lens it belongs to) the zone rows instead of the Concave
/// box, whose flip would hollow the zones.
fn lens<'a, W: Fn(m::LensEdit) -> Edit + Clone + 'a, Z: Fn(m::FresnelLensEdit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	l: &Scope<'a, Lens, W>,
	fresnel: Option<&Scope<'a, FresnelLens, Z>>,
) {
	let s = f.cx.s;
	let r = f.cx.r;
	let (r1, r2) = (l.obj.r1, l.obj.r2);
	f.section(s.get("SectionShape"), |f| {
		l.number(f, Lens::DIAMETER, 10.);
		if fresnel.is_none() {
			let (set1, set2) = (l.setter(Lens::R1), l.setter(Lens::R2));
			let flip = Check {
				key: key("Lens.IsConcave"),
				text: s.get("LabelLensConcave"),
				on: r1 < 0.,
				apply: Box::new(move |_| vec![set1(-r1), set2(-r2)]),
			};
			f.push(None, s.get("HintLensConcave"), RowKind::Check(flip));
		}
		l.number(f, Lens::R1, 10.);
		l.number(f, Lens::R2, 10.);
		l.number(f, Lens::THICKNESS, 1.);
		if let Some(z) = fresnel {
			z.number(f, FresnelLens::ZONES, 1.);
			z.combo(f, FresnelLens::DIVISION);
		}
		let edge = r.lens_edge.map_or_default(px);
		f.readout(s.get("LabelEdgeThickness"), edge, s.get("TipEdgeThickness"));
	});
}

fn triangle<'a, W: Fn(m::TriangleEdit) -> Edit + Clone + 'a>(f: &mut Builder<'a>, t: &Scope<'a, Triangle, W>) {
	let s = f.cx.s;
	let r = f.cx.r;
	let kind = t.obj.kind;
	let a_on = matches!(kind, TriangleType::Isosceles | TriangleType::Custom);
	f.section(s.get("SectionShape"), |f| {
		t.combo(f, Triangle::KIND);
		t.number(f, Triangle::BASE, 10.);
		t.decimals(f.on(a_on), Triangle::ANGLE_A, 0.5, 1);
		t.decimals(f.on(kind == TriangleType::Custom), Triangle::ANGLE_B, 0.5, 1);
		let apex = r.apex.map_or_default(|a| format!("{} deg", one_decimal(a)));
		f.readout(s.get("LabelApexAngle"), apex, s.get("TipApexAngle"));
	});
}

/// A meta surface's form: the plate's material (tops and back coated), its shape, the design's section (a lens's,
/// a prism's or an axicon's) and the pattern.
fn meta_surface<'a, W: Fn(m::MetaSurfaceEdit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	ms: &Scope<'a, MetaSurface, W>,
) {
	let s = f.cx.s;
	let r = f.cx.r;
	let design = ms.nested(MetaSurface::DESIGN);
	let MetaReadouts { full_turn, zones, zone_width, line } = r.meta.unwrap_or_default();
	let wavelength = r.wavelength.map_or_default(px);
	let zones = zones.to_string();
	let zone_width = zone_width.map_or_default(px);
	let (mat, coat) = (ms.nested(MetaSurface::MATERIAL), ms.nested(MetaSurface::COATING));
	material(f, &mat, &coat, CoatedFaces::FrontBack);
	f.section(s.get("SectionShape"), |f| {
		ms.number(f, MetaSurface::LENGTH, 10.);
		ms.number(f, MetaSurface::WIDTH, 10.);
	});
	let head = |f: &mut Builder<'a>| {
		ms.decimals(f, MetaSurface::DESIGN_FREQUENCY, 0.1, 2);
		f.readout(s.get("LabelWavelength"), wavelength.clone(), s.get("TipGateWavelength"));
	};
	let zone_rows = |f: &mut Builder<'a>, width: bool| {
		if width {
			f.readout(s.get("LabelZoneWidth"), zone_width.clone(), s.get("TipZoneWidth"));
		}
		f.readout(s.get("LabelMetaZones"), zones.clone(), s.get("TipMetaZones"));
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
				let line = line.map_or_default(px);
				f.readout(s.get("LabelLineLength"), line, s.get("TipLineLength"));
				zone_rows(f, true);
			});
		}
		_ => {}
	}
	f.section(s.get("SectionPattern"), |f| {
		ms.number(f, MetaSurface::PATTERN_INDEX, 0.01);
		ms.decimals(f, MetaSurface::PITCH, 0.1, 2);
		ms.decimals(f, MetaSurface::HEIGHT, 0.1, 2);
		let full_turn = full_turn.map_or_default(px);
		f.readout(s.get("LabelFullTurnHeight"), full_turn, s.get("TipFullTurnHeight"));
	});
}

fn zone_plate<'a, W: Fn(m::ZonePlateEdit) -> Edit + Clone + 'a>(f: &mut Builder<'a>, z: &Scope<'a, ZonePlate, W>) {
	let s = f.cx.s;
	let r = f.cx.r;
	let (count, outer) = r.zones.map_or((String::new(), String::new()), |(n, o)| (n.to_string(), approx_px(o)));
	f.section(s.get("SectionDesign"), |f| {
		z.decimals(f, ZonePlate::DESIGN_FREQUENCY, 0.1, 2);
		let wavelength = r.wavelength.map_or_default(approx_px);
		f.readout(s.get("LabelWavelength"), wavelength, s.get("TipGateWavelength"));
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
		f.readout(s.get("LabelZoneCount"), count, s.get("TipZones"));
		f.readout(s.get("LabelOuterZone"), outer, s.get("TipZones"));
	});
}

fn item_label<U: Fields, V>(s: &crate::strings::Strings, p: &Prop<U, V>) -> &'static str {
	use waveduderust::model::Field as _;
	s.label(U::TYPE_NAME, p.field.name())
}

/// An outline's points: an X / Y heading, a row per point with X, Y and remove, Add point below.
fn points<'a, W: Fn(m::OutlineEdit) -> Edit + Clone + 'a>(f: &mut Builder<'a>, list: &List<'a, Outline, Point, W>) {
	let s = f.cx.s;
	f.section(s.get("SectionPoints"), |f| {
		f.paragraph(s.get("HintPolygonPoints"));
		let spec = NumberSpec::integer(-1e6, 1e6);
		let items = (0..list.items.len())
			.map(|i| PointRow {
				x: list.number(i, Point::X, spec.clone()),
				y: list.number(i, Point::Y, spec.clone()),
				remove: list.remove(i, s.get("TipRemovePoint")),
			})
			.collect();
		let add = list.clone();
		let add = button(list.key(0, "Add"), s.get("ButtonAddPoint"), s.get("TipAddPoint"), true, move || {
			vec![Out::Send(add.set(|items| {
				let last = items.last().copied().unwrap_or_default();
				items.push(Point::new(last.x + 10, last.y));
			}))]
		});
		let heading = [item_label(&s, &Point::X), item_label(&s, &Point::Y)];
		f.push(None, "", RowKind::Points(Points { heading, items, add }));
	});
}

/// An interference filter's layers, the outermost first: a row each with its thickness and index, up, down and
/// remove; Add layer and Remove all below.
fn layers<'a, W: Fn(m::InterferenceFilterEdit) -> Edit + Clone + 'a>(
	f: &mut Builder<'a>,
	list: &List<'a, InterferenceFilter, FilterLayer, W>,
) {
	let s = f.cx.s;
	let (tmin, tmax) = bounds_of(FilterLayer::THICKNESS);
	let (imin, imax) = bounds_of(FilterLayer::REFRACTIVE_INDEX);
	f.section(s.get("SectionLayers"), |f| {
		f.paragraph(s.get("HintFilterLayers"));
		let thickness = NumberSpec::linear(tmin, tmax, 1.).with_decimals(2).with_drag_step(0.1);
		let index = NumberSpec::linear(imin, imax, 0.01).with_decimals(2).with_drag_step(0.01);
		let items = (0..list.items.len())
			.map(|i| LayerRow {
				thickness: list.number(i, FilterLayer::THICKNESS, thickness.clone()),
				index: list.number(i, FilterLayer::REFRACTIVE_INDEX, index.clone()),
				up: list.shift(i, -1, s.get("TipLayerUp")),
				down: list.shift(i, 1, s.get("TipLayerDown")),
				remove: list.remove(i, s.get("TipRemoveLayer")),
			})
			.collect();
		let (add, clear, reorder) = (list.clone(), list.clone(), list.clone());
		let add = button(list.key(0, "Add"), s.get("ButtonAddLayer"), s.get("TipAddLayer"), true, move || {
			vec![Out::Send(add.set(|items| items.push(items.last().cloned().unwrap_or_default())))]
		});
		let clear = button(
			list.key(0, "Clear"),
			s.get("ButtonRemoveAllLayers"),
			s.get("TipRemoveAllLayers"),
			!list.items.is_empty(),
			move || vec![Out::Send(clear.set(Vec::clear))],
		);
		let heading = [item_label(&s, &FilterLayer::THICKNESS), item_label(&s, &FilterLayer::REFRACTIVE_INDEX)];
		let layers = Layers {
			key: list.key(0, "Drag"),
			heading,
			items,
			grip_tip: s.get("TipDragLayer"),
			reorder: Box::new(move |from, to| reorder.reorder(from, to)),
			add,
			clear,
		};
		f.push(None, "", RowKind::Layers(layers));
	});
}

fn fiber<'a, W: Fn(m::FiberEdit) -> Edit + Clone + 'a>(f: &mut Builder<'a>, fb: &Scope<'a, Fiber, W>) {
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
	let list = fb.list(s, Fiber::NODES, &fb.obj.nodes);
	f.section(s.get("SectionNodes"), |f| {
		f.paragraph(s.get("HintFiberNodes"));
		let spec = NumberSpec::integer(-1e6, 1e6);
		let items = list
			.items
			.iter()
			.enumerate()
			.map(|(i, node)| {
				let line =
					|key: &str, x, y| (s.get(key), list.number(i, x, spec.clone()), list.number(i, y, spec.clone()));
				let set = list.clone();
				NodeRow {
					lines: [
						line("LabelNodePoint", FiberNode::X, FiberNode::Y),
						line("LabelNodeIn", FiberNode::IN_X, FiberNode::IN_Y),
						line("LabelNodeOut", FiberNode::OUT_X, FiberNode::OUT_Y),
					],
					smooth: Check {
						key: list.key(i, "Smooth"),
						text: item_label(&s, &FiberNode::SMOOTH),
						on: node.smooth,
						apply: Box::new(move |on| vec![set.set_item(i, FiberNode::SMOOTH, on)]),
					},
					remove: list.remove(i, s.get("TipRemoveNode")),
				}
			})
			.collect();
		let add = list.clone();
		let add = button(list.key(0, "Add"), s.get("ButtonAddNode"), s.get("TipAddNode"), true, move || {
			vec![Out::Send(add.set(|l| {
				let (x, y) = l.last().map_or((0, 0), |n| (n.x + 50, n.y));
				l.push(
					FiberNode::default()
						.with_x(x)
						.with_y(y)
						.with_in_x(-20)
						.with_in_y(0)
						.with_out_x(20)
						.with_out_y(0)
						.with_smooth(true),
				);
			}))]
		});
		let heading = [item_label(&s, &FiberNode::X), item_label(&s, &FiberNode::Y)];
		f.push(None, "", RowKind::Nodes(Nodes { heading, items, add }));
	});
}

fn gate<'a, W: Fn(m::LogicGateEdit) -> Edit + Clone + 'a>(f: &mut Builder<'a>, g: &Scope<'a, LogicGate, W>) {
	let s = f.cx.s;
	let r = f.cx.r;
	let design = r.gate;
	let layout = design.map_or([0; 4], |d| d.layout);
	let ports = design.map_or([Point::default(); 4], |d| d.ports);
	let quarter = design.map(|d| d.quarter_wave).filter(|q| q.is_finite()).map_or_default(approx_px);
	let (upper, lower) = match g.obj.kind {
		m::GateKind::Xor => ("Xor", "And"),
		m::GateKind::And => ("And", "Xor"),
		m::GateKind::Or => ("Or", "Remainder"),
		_ => return,
	};
	f.section(s.get("SectionDesign"), |f| {
		g.decimals(f, LogicGate::DESIGN_FREQUENCY, 0.1, 2);
		let wavelength = r.wavelength.map_or_default(approx_px);
		f.readout(s.get("LabelWavelength"), wavelength, s.get("TipGateWavelength"));
	});
	f.section(s.get("SectionMaterial"), |f| {
		g.number(f, LogicGate::CORE_INDEX, 0.01);
		g.number(f, LogicGate::CLADDING_INDEX, 0.01);
		let (min, max) = bounds_of(LogicGate::COATING);
		g.number_with(f, LogicGate::COATING, NumberSpec::integer(min, max));
		g.check(f, LogicGate::INPUT_COATED);
		g.check(f, LogicGate::OUTPUT_COATED);
		f.readout(s.get("LabelQuarterWave"), quarter, s.get("TipQuarterWave"));
	});
	f.section(s.get("SectionShape"), |f| {
		// the box shows the spacing the design uses; an edit stores the typed value
		let (min, max) = bounds_of(LogicGate::PORT_SPACING);
		let mut n = g.number_box(&s, LogicGate::PORT_SPACING, NumberSpec::linear(min, max, 2.).with_decimals(0));
		if let Some(d) = design {
			n.value = d.spacing as f64;
		}
		f.push(Some(n.label), n.hint, RowKind::Number(n));
	});
	f.section(s.get("SectionOutputs"), |f| {
		g.check_text(f, LogicGate::UPPER_OUTPUT_ACTIVE, s.enum_word("GateOutput", upper));
		g.check_text(f, LogicGate::LOWER_OUTPUT_ACTIVE, s.enum_word("GateOutput", lower));
	});
	f.section(s.get("SectionLayout"), |f| {
		for (key, v) in
			["LabelCoreWidth", "LabelCouplerGap", "LabelCouplerLength", "LabelGateLength"].iter().zip(layout)
		{
			f.readout(s.get(key), format!("{v} px"), s.get("TipGateLayout"));
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
			f.readout(text, format!("({}, {})", q.x, q.y), s.get("TipGatePorts"));
		}
	});
}

fn probe<'a>(f: &mut Builder<'a>, o: &'a Probe, item: ItemRef) {
	let s = f.cx.s;
	let index = o.id();
	let p = Scope::new(o, move |e| Edit::Probe(index, e));
	let line = o.kind == ProbeKind::Line;
	let is_detector = o.is_detector();
	// the step axis belongs to a graph over time: a point probe's or a counting detector's
	let profile = o.shows_profile();
	let field = o.layer == Layer::Field;
	let measurements: &'static [Measurement] =
		if field { &[Measurement::Amplitude] } else { &[Measurement::Value, Measurement::IntegratedValue] };
	let custom = o.box_size == BoxSize::Custom;
	// the field's amplitude changes sign, so its axis stays linear
	let log = !field && o.y_scale == YScale::Logarithmic;
	let at_least_0 = ranges::AT_LEAST_0;
	f.section(s.get("SectionGeneral"), |f| {
		f.name::<Probe>(item, &o.name);
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
		let xy = [s.get("LabelX"), s.get("LabelY")];
		f.group(s.get("GroupPointA"), &xy, |f| {
			p.number_in(f, Probe::AX, at_least_0, 1.);
			p.number_in(f, Probe::AY, at_least_0, 1.);
		});
		f.on(line).group(s.get("GroupPointB"), &xy, |f| {
			p.number_in(f, Probe::BX, at_least_0, 1.);
			p.number_in(f, Probe::BY, at_least_0, 1.);
		});
	});
	f.section(s.get("SectionGraph"), |f| {
		p.combo(f, Probe::BOX_SIZE);
		let xy = [s.get("LabelX"), s.get("LabelY")];
		f.group(s.get("GroupBoxPosition"), &xy, |f| {
			p.number_in(f, Probe::BOX_X, at_least_0, 1.);
			p.number_in(f, Probe::BOX_Y, at_least_0, 1.);
		});
		f.on(custom).group(s.get("GroupBoxSize"), &[s.get("LabelWidth"), s.get("LabelHeight")], |f| {
			p.number(f, Probe::BOX_WIDTH, 10.);
			p.number(f, Probe::BOX_HEIGHT, 10.);
		});
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

fn scheme<'a>(f: &mut Builder<'a>, o: &'a ColorScheme, item: ItemRef) {
	let s = f.cx.s;
	let r = f.cx.r;
	let app = f.cx.app;
	let index = o.id();
	let sc = Scope::new(o, move |e| Edit::Scheme(index, e));
	let gradient = Gradient::of(&o.stops);
	let set = sc.setter(ColorScheme::STOPS);
	f.section(s.get("SectionGeneral"), |f| {
		f.name::<ColorScheme>(item, &o.name);
		sc.combo(f, ColorScheme::KIND);
		sc.text(f, ColorScheme::DESCRIPTION, &o.description, false);
		let mine = if r.user_scheme { "MenuSchemeRemoveFromMine" } else { "MenuSchemeAddToMine" };
		let toggle = button(key("ToggleUserScheme"), s.get(mine), "", true, move || {
			vec![Out::Send(Event::ToggleUserScheme(index))]
		});
		f.push(None, "", RowKind::Buttons(vec![toggle]));
	});
	f.section(s.get("SectionStops"), |f| {
		let label = s.get("LabelBasedOn");
		let (stops, kind, description) =
			(sc.setter(ColorScheme::STOPS), sc.setter(ColorScheme::KIND), sc.setter(ColorScheme::DESCRIPTION));
		let based_on = &r.based_on;
		let choice = Choice {
			key: key("ColorScheme.BasedOn"),
			label,
			words: based_on.iter().map(|n| n.as_str().into()).collect(),
			current: None,
			shown: "".into(),
			swatches: Vec::new(),
			actions: Vec::new(),
			pick: Box::new(move |i| {
				let Some(source) = based_on.get(i).and_then(|n| find_scheme(app, n)) else { return Vec::new() };
				vec![
					Out::Send(stops(source.stops.clone())),
					Out::Send(kind(source.kind)),
					Out::Send(description(source.description.clone())),
				]
			}),
		};
		f.push(Some(label), s.get("TipBasedOn"), RowKind::Choice(choice));
		let tip = s.get("TipSchemeGradient");
		let to = set.clone();
		let bar = GradientBar {
			key: key("SchemeGradient"),
			gradient: gradient.clone(),
			tip,
			apply: Box::new(move |stops| to(stops)),
		};
		f.push(None, tip, RowKind::Gradient(bar));
		for (i, stop) in gradient.stops.iter().enumerate() {
			let row = stop_row(f.cx.s, &gradient, i, *stop, r.can_remove_stop, &set);
			f.push(None, "", RowKind::Stop(row));
		}
		let (g, to) = (gradient.clone(), set.clone());
		let add = button(key("AddStop"), s.get("ButtonAddStop"), s.get("TipAddStop"), true, move || {
			let mut g = g.clone();
			g.add_in_widest_gap();
			vec![Out::Send(to(g.into_stops()))]
		});
		f.push(None, s.get("TipAddStop"), RowKind::Buttons(vec![add]));
	});
}

/// Stop `i`'s row: its ordinal, position, swatch, hex text and remove button; `set` makes the event of the stops an
/// edit leaves.
fn stop_row<'a>(
	s: crate::strings::Strings,
	gradient: &Gradient<'a>,
	i: usize,
	stop: m::Stop,
	removable: bool,
	set: &Setter<'a, Vec<m::Stop>>,
) -> StopRow<'a> {
	let edit = |change: fn(&mut Gradient<'_>, usize, f64, m::Rgb)| {
		let (g, set) = (gradient.clone(), set.clone());
		move |at: f64, color: m::Rgb| {
			let mut g = g.clone();
			change(&mut g, i, at, color);
			set(g.into_stops())
		}
	};
	let moved = edit(|g, i, at, _| {
		g.set_position(i, at);
	});
	let recolor = edit(|g, i, _, c| g.set_color(i, c));
	let (recolor_hex, recolor_swatch) = (recolor.clone(), recolor);
	let remove = edit(|g, i, _, _| {
		g.remove(i);
	});
	let tip = s.get("TipStopPosition");
	StopRow {
		ordinal: i + 1,
		position: Number {
			key: key(("Stop", i, "Position")),
			label: tip,
			hint: tip,
			value: stop.at,
			spec: NumberSpec::linear(0., 1., 0.01).with_decimals(3),
			unit: None,
			blank: false,
			first: false,
			apply: Box::new(move |v| moved(v, m::Rgb::default())),
		},
		color: Color {
			key: key(("Stop", i, "Color")),
			value: stop.color,
			tip: s.get("TipStopSwatch"),
			apply: Box::new(move |c| recolor_swatch(0., c)),
		},
		hex: Text {
			key: key(("Stop", i, "Hex")),
			label: s.get("TipStopColor"),
			value: stop.color.to_hex().into(),
			multiline: false,
			live: false,
			first: false,
			apply: Box::new(move |t| parse_hex(&t).map(|c| recolor_hex(0., c))),
		},
		remove: glyph(key(("Stop", i, "Remove")), Glyph::Minus, s.get("TipRemoveStop"), removable, move || {
			vec![Out::Send(remove(0., m::Rgb::default()))]
		}),
	}
}
