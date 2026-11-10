//! The forms of docs/ui/30, row by row, on the editors of [`super::properties`].

use std::path::{Path, PathBuf};

use freya::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use wavedude_core::io::schemes;
use wavedude_core::model::{self as m, PartKind, Project, SchemeKind, SourceArray, Stencil};
use wavedude_core::scene::gate_design::{self, GateDesign};
use wavedude_widgets::icon::icons;
use wavedude_widgets::{
	ColorButton, DescriptionHeader, Gradient, GradientEditor, GradientStop, IconView, NumberSpec, PickerTips,
	PropertyRow, Rgb, SectionHeader, TestIdExt,
};

use wavedude_app_state::commands::{Command, NewPart};
use wavedude_app_state::selection::{ItemKind, Row};
use wavedude_app_state::{Event as AppEvent, Guarded};

use super::dialogs;
use super::properties::{Form, Snapshot, bounds, combo_box, readout, tagged, to_json};
use super::shared::App;

/// Values the forms show that the edited object alone does not hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Readouts {
	pub wavelength: Option<f64>,
	pub nominal: bool,
	pub lens_edge: Option<f64>,
	pub apex: Option<f64>,
	/// Core width, coupler gap, coupler length, total length, ports.
	pub gate: Option<([i64; 4], [m::Point; 4], f64)>,
	pub zones: Option<(i64, f64)>,
	/// A diffuser's roughness: the face's height from crest to trough and its number of ripples.
	pub roughness: Option<(f64, usize)>,
	pub image_found: bool,
	pub image_size: Option<(u32, u32)>,
	/// Scheme names for each layer's picker: (name, project's own).
	pub schemes: [Vec<(String, bool)>; 2],
	/// For the scheme form's Based on picker.
	pub based_on: Vec<String>,
	pub can_remove_stop: bool,
}

fn zones(z: &m::ZonePlate, sim: &m::Simulation) -> Option<(i64, f64)> {
	let lambda = m::wavelength(z.design_frequency, sim.wave_speed, sim.source_frequency_scale);
	if !lambda.is_finite() || lambda <= 0. {
		return None;
	}
	let f = f64::from(z.focal_length);
	let radius = |n: f64| (n * lambda * f + (n * lambda / 2.).powi(2)).sqrt();
	let r = f64::from(z.diameter) / 2.;
	let a = lambda * lambda / 4.;
	let b = lambda * f;
	let n = ((-b + (b * b + 4. * a * r * r).sqrt()) / (2. * a)).floor() as i64;
	let outer = if n >= 1 { radius(n as f64) - radius(n as f64 - 1.) } else { r };
	Some((n, outer))
}

fn edge_thickness(l: &m::Lens, zones: u32, division: m::ZoneDivision) -> f64 {
	m::lens_geometry::edge_thickness(l.diameter, l.thickness, l.r1, l.r2, zones, division)
}

/// The schemes a layer's picker lists: the project's, the user's, the built-in ones.
fn scheme_names(project: &Project, user: &[m::ColorScheme], kind: SchemeKind) -> Vec<(String, bool)> {
	let fits = |k: SchemeKind| k == kind || k == SchemeKind::Unspecified;
	let mut v: Vec<(String, bool)> =
		project.color_schemes.iter().filter(|s| fits(s.kind)).map(|s| (s.name.clone(), true)).collect();
	for s in user.iter().chain(schemes::built_in()).filter(|s| fits(s.kind)) {
		if !v.iter().any(|(n, _)| n.eq_ignore_ascii_case(&s.name)) {
			v.push((s.name.clone(), false));
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
			r.image_found = !p.scene.image_path.is_empty() && Path::new(&p.scene.image_path).is_file();
			r.image_size = p.scene.image_data.as_ref().map(|_| (p.scene.width, p.scene.height));
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
						let ports = gate_design::ports_of(&d, &p.parts[i.index].base);
						r.gate = Some(([d.core_width, d.coupled, d.coupler_len, d.length], ports, d.wavelength));
					}
				}
				PartKind::ZonePlate(z) => {
					r.wavelength = Some(m::wavelength(z.design_frequency, sim.wave_speed, sim.source_frequency_scale));
					r.zones = zones(z, sim);
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
				r.can_remove_stop = own.stops.len() > 2;
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

fn px(v: f64) -> String {
	format!("{v:.1} px")
}

/// Every row of the current form.
pub fn build(f: &Form, row: Row, snap: &Snapshot) -> Vec<Element> {
	let r = &snap.readouts;
	match row {
		Row::Project => project(f),
		Row::Scene => scene(f, r),
		Row::Simulation => simulation(f),
		Row::Display => display(f, r),
		Row::Item(i) => match (i.kind, &snap.part) {
			(ItemKind::Source, _) => source(f, r),
			(ItemKind::Part, Some(kind)) => part(f, r, kind, snap.description_open),
			(ItemKind::Part, None) => unreachable!("a part row's snapshot carries its kind"),
			(ItemKind::Probe, _) => probe(f),
			(ItemKind::Scheme, _) => scheme(f, r),
		},
		Row::Folder(_) => Vec::new(),
	}
}

fn project(f: &Form) -> Vec<Element> {
	let s = &f.s;
	let new_seed = {
		let f = f.clone();
		move |_: Event<PressEventData>| {
			let nanos =
				std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
			f.set("seed", Value::from(i64::from(nanos & 0x7fff_ffff)));
		}
	};
	let p = f.scope("Project", "", "Project");
	vec![section(
		s.get("SectionGeneral"),
		vec![
			readout(s.label("Project", "Version"), m::PROJECT_VERSION.to_string(), "ProjectVersion"),
			p.number_with("Seed", NumberSpec::integer(f64::from(i32::MIN), f64::from(i32::MAX))),
			PropertyRow::new("", dialogs::button(s.get("ButtonNewSeed"), true, new_seed))
				.hint(s.hint("Project", "Seed")),
			p.text("Description", true),
		],
	)]
}

fn image_buttons(f: &Form, has_bytes: bool) -> Element {
	let s = f.s;
	let pick = {
		let f = f.clone();
		move |_: Event<PressEventData>| {
			let cx = f.cx.clone();
			let start = cx.shared.peek(|a| a.document.folder().map(Path::to_owned));
			dialogs::pick_file(&dialogs::scene_image_filter(&s), start, move |path: PathBuf| {
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
		.spacing(2.)
		.width(Size::fill())
		.content(Content::flex())
		.cross_align(Alignment::center())
		.child(label().width(Size::flex(1.)).text(f.word("image")).max_lines(1).test_id("SceneImageName"))
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
	TooltipContainer::new(Tooltip::new_text(text.to_owned())).child(child).into_element()
}

fn save_image(f: &Form) {
	let (bytes, name, path, folder) = f.cx.shared.peek(|a| {
		let sc = &a.document.project.scene;
		(sc.image_data.clone(), sc.image.clone(), sc.image_path.clone(), a.document.folder().map(Path::to_owned))
	});
	let Some(bytes) = bytes else { return };
	let ext =
		Path::new(&name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_else(|| ".png".into());
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

fn scene(f: &Form, r: &Readouts) -> Vec<Element> {
	let s = &f.s;
	let sc = f.scope("Scene", "", "Scene");
	let image_mode = sc.word("Mode") == "Image";
	let mut canvas = vec![sc.combo("Mode", "SceneMode", &["Canvas", "Image"])];
	if image_mode {
		canvas.push(
			PropertyRow::new(s.label("Scene", "Image"), image_buttons(f, r.image_size.is_some()))
				.hint(s.hint("Scene", "Image")),
		);
		let path = sc.word("ImagePath");
		let shown =
			if !path.is_empty() && !r.image_found { format!("{path} {}", s.get("SceneImageNotFound")) } else { path };
		canvas.push(readout(s.label("Scene", "ImagePath"), shown, "SceneImagePath"));
		let size = r.image_size.map_or_else(|| s.get("ImageUnreadable").to_owned(), |(w, h)| format!("{w} x {h}"));
		canvas.push(readout(s.get("LabelEmbeddedImage"), size, "SceneEmbeddedImage").hint(s.get("TipEmbeddedImage")));
	}
	let (smin, smax) = bounds("Scene.Width");
	canvas.extend([
		sc.number_with("Width", NumberSpec::integer(smin, smax)).enabled(!image_mode),
		sc.number_with("Height", NumberSpec::integer(smin, smax)).enabled(!image_mode),
		sc.combo("Edges", "EdgeMode", &["Open", "Reflective"]),
		sc.number("DampingEdge", 1.),
		sc.number("RefractiveIndex", 0.01),
	]);
	let image_sources = sc.flag("ImageSources");
	vec![
		section(s.get("SectionCanvas"), canvas),
		section(
			s.get("SectionImageSources"),
			vec![
				sc.check("ImageSources"),
				sc.number("ImageSourceScale", 0.1).enabled(image_sources),
				sc.check("ObjectSources"),
			],
		),
	]
}

fn simulation(f: &Form) -> Vec<Element> {
	let s = &f.s;
	let sim = f.scope("Simulation", "", "Simulation");
	let fourth = f.bind_as::<Stencil>("stencil") == Stencil::FourthOrder;
	let stencil = {
		let f = f.clone();
		move |on: bool| f.set("stencil", Value::from(if on { "FourthOrder" } else { "NinePoint" }))
	};
	let (dmin, dmax) = bounds("Simulation.TimeDecay");
	vec![
		section(s.get("SectionRun"), vec![sim.number("Steps", 100.), sim.number("VisualizeEvery", 1.)]),
		section(
			s.get("SectionSolver"),
			vec![
				PropertyRow::new(
					"",
					super::properties::check_box(s.label("Simulation", "FourthOrderLaplacian").into(), fourth, stencil)
						.test_id("Simulation.FourthOrderLaplacian"),
				)
				.hint(s.hint("Simulation", "FourthOrderLaplacian"))
				.full_width(),
				sim.number("WaveSpeed", 0.01),
				sim.number_with("TimeDecay", NumberSpec::linear(dmin, dmax, 1.).with_decimals(1).with_drag_step(0.1)),
				sim.number("SourceFrequencyScale", 0.1),
				sim.number("IntensityScale", 0.1),
			],
		),
	]
}

fn layer_rows(f: &Form, layer: &str, names: &[(String, bool)]) -> Vec<PropertyRow> {
	let s = &f.s;
	let l = f.scope("DisplayLayer", layer, "DisplayLayer");
	let path = l.path("ColorScheme");
	let current = f.bind_as::<String>(&path);
	let mut items: Vec<(String, String)> = names
		.iter()
		.map(|(n, own)| (n.clone(), if *own { format!("{n} ({})", s.get("PickerProjectTag")) } else { n.clone() }))
		.collect();
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
		combo_box(&f.cx, current, items, format!("{layer}.ColorScheme"), move |name| {
			if name == BROWSE {
				let cx = form.cx.clone();
				dialogs::pick_file(form.s.get("ColorSchemeFilter"), None, move |path| {
					cx.send(AppEvent::Guarded(Guarded::Open(path)))
				});
			} else if name == EDIT_COPY {
				edit_copy(&form, &path, &current_name);
			} else {
				form.set(&path, Value::from(name));
			}
		})
	};
	let (bmin, bmax) = bounds("DisplayLayer.Brightness");
	vec![
		PropertyRow::new(s.label("DisplayLayer", "ColorScheme"), picker).hint(s.hint("DisplayLayer", "ColorScheme")),
		l.number_with("Brightness", NumberSpec::logarithmic(bmin, bmax)),
		l.number("BlackLevel", 0.01),
		l.check("Preview"),
		l.check("Overlay"),
		l.number("OverlayGain", 0.05),
		l.check("OverlayInExport"),
	]
}

/// Makes a project scheme from the layer's current one (a project scheme is copied) and points the layer at it.
fn edit_copy(f: &Form, path: &str, current: &str) {
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
	f.set(path, Value::from(name_after));
}

fn display(f: &Form, r: &Readouts) -> Vec<Element> {
	let s = &f.s;
	let d = f.scope("Display", "", "Display");
	let v = f.scope("Video", "video", "Video");
	vec![
		section(s.get("SectionField"), layer_rows(f, "field", &r.schemes[0])),
		section(s.get("SectionIntensity"), layer_rows(f, "intensity", &r.schemes[1])),
		section(s.get("SectionPreview"), vec![d.check("HandlesInPreview"), d.check("LabelsInPreview")]),
		section(
			s.get("SectionExportRange"),
			vec![
				d.number("ExportFrom", 1.),
				d.number("ExportTo", 1.),
				d.check("ProbeGraphsInExport"),
				d.check("ProbeMarkersInExport"),
				d.check("SourceMarkersInExport"),
			],
		),
		section(s.get("SectionVideo"), vec![v.number("Fps", 1.), v.number("Quality", 1.)]),
	]
}

fn source(f: &Form, r: &Readouts) -> Vec<Element> {
	let s = &f.s;
	let src = f.scope("Source", "", "Source");
	let pulsed = src.word("Mode") == "Pulsed";
	let composite = f.optional("array").is_some();
	// A plain source shows the default array, greyed.
	let array_form = if composite {
		f.clone()
	} else {
		f.with_json(serde_json::json!({ "array": to_json(SourceArray::default()) }))
	};
	let arr = array_form.scope("SourceArray", "array", "SourceArray");
	let wavelength = r.wavelength.map_or_else(String::new, |w| {
		let text = if w.is_finite() { px(w) } else { "-".into() };
		if r.nominal { format!("{text} {}", s.get("WavelengthNominal")) } else { text }
	});
	let detach = {
		let f = f.clone();
		let value = match f.optional("detach_after") {
			None => -1.,
			Some(v) => v.as_f64().expect("detach_after is a step"),
		};
		let (_, max) = bounds("Source.DetachAfter");
		let editor = wavedude_widgets::NumberBox::new(
			WritableSetter::new(value, move |v: f64| {
				f.set("detach_after", if v < 0. { Value::Null } else { Value::from(v.round() as i64) })
			}),
			NumberSpec::integer(-1., max),
		)
		.width(Size::fill())
		.test_id("Source.DetachAfter");
		PropertyRow::new(s.label("Source", "DetachAfter"), editor).hint(s.hint("Source", "DetachAfter"))
	};
	let composite_box = {
		let f = f.clone();
		let toggle = move |on: bool| {
			f.set("array", if on { to_json(SourceArray::default()) } else { Value::Null });
		};
		PropertyRow::new(
			"",
			super::properties::check_box(s.label("Source", "IsComposite").into(), composite, toggle)
				.test_id("Source.IsComposite"),
		)
		.hint(s.hint("Source", "IsComposite"))
		.full_width()
	};
	let (amin, amax) = bounds("SourceArray.Angle");
	vec![
		section(s.get("SectionGeneral"), vec![f.name("Source"), src.check("Enabled")]),
		section(
			s.get("SectionPosition"),
			vec![src.decimals_in("X", "Source.X", 1., 2), src.decimals_in("Y", "Source.Y", 1., 2)],
		),
		section(
			s.get("SectionWave"),
			vec![
				src.decimals("Frequency", 0.1, 2),
				src.decimals("Amplitude", 0.1, 2),
				src.decimals("Phase", 0.5, 2),
				readout(s.get("LabelWavelength"), wavelength, "SourceWavelength").hint(s.get("TipWavelength")),
			],
		),
		section(
			s.get("SectionTiming"),
			vec![
				src.combo("Mode", "SourceMode", &["Continuous", "Pulsed"]),
				src.combo("PulseShape", "PulseShape", &["Gaussian", "SmoothedSquare"]).enabled(pulsed),
				src.number("PulseWidth", 1.).enabled(pulsed),
				src.number("OffWidth", 1.).enabled(pulsed),
				detach,
			],
		),
		section(
			s.get("SectionArray"),
			vec![
				composite_box,
				arr.number("SizeX", 1.).enabled(composite),
				arr.number("SizeY", 1.).enabled(composite),
				arr.number("SpacingX", 1.).enabled(composite),
				arr.number("SpacingY", 1.).enabled(composite),
				arr.number_with("Angle", NumberSpec::linear(amin, amax, 5.).with_decimals(1).with_drag_step(0.5))
					.enabled(composite),
			],
		),
		section(
			s.get("SectionRandom"),
			["RandomFrequency", "RandomPhase", "RandomOffWidth", "RandomPosition"]
				.map(|m| arr.number(m, 1.).enabled(composite))
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
}

/// The coating's rows of `coating` (a path); greyed when `disabled`.
fn coating_rows(f: &Form, coating: &str, disabled: bool) -> Vec<PropertyRow> {
	coating_rows_of(f, coating, disabled, CoatedFaces::All)
}

fn coating_rows_of(f: &Form, coating: &str, disabled: bool, faces: CoatedFaces) -> Vec<PropertyRow> {
	let c = |m: &str| format!("{coating}.{m}");
	let mut rows = vec![
		f.number("ShapePart", "Coating", &c("depth"), "Coating.Coating", 1.),
		f.combo("CoatedPart", "CoatingProfile", &c("profile"), "CoatingProfile", &["Layer", "Graded"]),
	];
	if faces == CoatedFaces::All {
		rows.push(f.check("CoatedPart", "FrontCoated", &c("front")));
	}
	rows.push(f.check("CoatedPart", "BackCoated", &c("back")));
	if faces == CoatedFaces::All {
		rows.push(f.check("CoatedPart", "SidesCoated", &c("sides")));
	}
	rows.into_iter().map(|r| r.enabled(!disabled)).collect()
}

/// The glass or mirror of a lens or shape at `kind` (`kind.Lens`).
fn material(f: &Form, kind: &str) -> Element {
	material_faces(f, kind, CoatedFaces::All)
}

/// [`material`] with the coating offered on `faces` alone.
fn material_faces(f: &Form, kind: &str, faces: CoatedFaces) -> Element {
	let mat = f.scope("ShapePart", &format!("{kind}.material"), "Material");
	let mirror = mat.flag("Mirror");
	let mut rows = vec![mat.check("Mirror"), mat.number("RefractiveIndex", 0.01).enabled(!mirror)];
	rows.extend(coating_rows_of(f, &format!("{kind}.coating"), mirror, faces));
	rows.push(mat.number("Absorption", 5.).enabled(!mirror));
	rows.push(mat.number("AbsorptionRamp", 5.).enabled(!mirror));
	section(f.s.get("SectionMaterial"), rows)
}

/// The rows every part shares: its description, name, enabled flag and position.
fn part_common(f: &Form, ty: &str, open: bool) -> Vec<Element> {
	let s = &f.s;
	let common = f.scope("OpticalPart", "", "Part");
	let expanded = {
		let cx = f.cx.clone();
		WritableSetter::new(open, move |v: bool| cx.shared.update(|a| a.settings.part_description_expanded = v))
	};
	let (amin, amax) = bounds("Part.Angle");
	vec![
		DescriptionHeader::new(s.kind(ty), expanded)
			.title(s.get("SectionDescription"))
			.tooltip(s.get("TipPartDescription"))
			.into_element(),
		section(s.get("SectionGeneral"), vec![f.name("OpticalPart"), common.check("Enabled")]),
		section(
			s.get("SectionPosition"),
			vec![
				common.number("X", 1.),
				common.number("Y", 1.),
				common.number_with("Angle", NumberSpec::linear(amin, amax, 5.).with_decimals(1).with_drag_step(0.5)),
			],
		),
	]
}

fn part(f: &Form, r: &Readouts, kind: &PartKind, open: bool) -> Vec<Element> {
	// The serde name of the kind, the prefix of its fields' paths and its range keys.
	let variant =
		f.get("kind").as_object().and_then(|o| o.keys().next()).expect("a part's kind is one tagged object").clone();
	let at = format!("kind.{variant}");
	let mut out = part_common(f, NewPart::of(kind).type_name(), open);
	let shape = |rows: Vec<PropertyRow>| section(f.s.get("SectionShape"), rows);
	match kind {
		PartKind::Lens(_) => out.extend([material(f, &at), lens(f, r, &at, None)]),
		PartKind::FresnelLens(_) => out.extend([material(f, &at), lens(f, r, &at, Some(&variant))]),
		PartKind::Polygon(_) => {
			let p = f.scope("PolygonPart", &at, &variant);
			out.extend([material(f, &at), shape(vec![p.number("Sides", 1.), p.number("Side", 10.)])]);
		}
		PartKind::Rectangle(_) | PartKind::Ellipse(_) => {
			let ty = NewPart::of(kind).type_name();
			let b = f.scope(ty, &at, &variant);
			out.extend([
				material(f, &at),
				shape(vec![b.number("Length", 10.), b.number("Width", 10.), b.check("Linked")]),
			]);
		}
		PartKind::Triangle(_) => out.extend([material(f, &at), triangle(f, r, &at, &variant)]),
		PartKind::Outline(_) => out.extend([material(f, &at), points(f, &format!("{at}.points"))]),
		PartKind::Diffuser(_) => {
			let d = f.scope("DiffuserPart", &at, &variant);
			let (height, ripples) = r.roughness.map_or((String::new(), String::new()), |(h, n)| (px(h), n.to_string()));
			out.extend([
				material(f, &at),
				shape(vec![d.number("Length", 10.), d.number("Width", 10.)]),
				section(
					f.s.get("SectionRoughness"),
					vec![
						d.decimals("CenterWavelength", 0.1, 2),
						d.decimals("Amplitude", 0.1, 2),
						d.decimals("Sigma", 0.1, 2),
						readout(f.s.get("LabelProfileHeight"), height, "ProfileHeight")
							.hint(f.s.get("TipProfileHeight")),
						readout(f.s.get("LabelComponentCount"), ripples, "ComponentCount")
							.hint(f.s.get("TipComponentCount")),
					],
				),
			]);
		}
		PartKind::FacettedGrating(_) => {
			let g = f.scope("FacettedGratingPart", &at, &variant);
			out.extend([
				material_faces(f, &at, CoatedFaces::Back),
				shape(vec![g.number("Length", 10.), g.number("Width", 10.)]),
				section(
					f.s.get("SectionFacets"),
					vec![
						g.combo("Pattern", "FacetPattern", &["Sawtooth", "SquareWave", "Sinusoidal"]),
						g.decimals("Pitch", 0.1, 2),
						g.decimals("Amplitude", 0.1, 2),
					],
				),
			]);
		}
		PartKind::Fiber(_) => out.extend(fiber(f, &at, &variant)),
		PartKind::BraggReflector(_) => {
			let b = f.scope("BraggReflectorPart", &at, &variant);
			out.push(section(
				f.s.get("SectionLayers"),
				vec![
					b.number("Pairs", 1.),
					b.number("Thickness1", 1.),
					b.number("Index1", 0.01),
					b.number("Thickness2", 1.),
					b.number("Index2", 0.01),
				],
			));
			out.push(shape(vec![b.number("Width", 10.)]));
		}
		PartKind::GrinLens(_) => {
			let g = f.scope("GrinLensPart", &at, &variant);
			let mut rows = vec![g.number("CentreIndex", 0.01), g.number("EdgeIndex", 0.01)];
			rows.extend(coating_rows(f, &g.path("Coating"), false));
			out.push(section(f.s.get("SectionMaterial"), rows));
			out.push(shape(vec![g.number("Length", 10.), g.number("Diameter", 10.)]));
		}
		PartKind::ZonePlate(_) => out.extend(zone_plate(f, r, &at, &variant)),
		PartKind::LogicGate(g) => out.extend(gate(f, r, &at, &variant, g.kind)),
	}
	out
}

/// The lens's shape rows; with `fresnel` (the Fresnel lens's serde variant) the zone rows instead of the Concave
/// box, whose flip would hollow the zones.
fn lens(f: &Form, r: &Readouts, at: &str, fresnel: Option<&str>) -> Element {
	let s = &f.s;
	let l = f.scope("LensPart", at, "Lens");
	let (r1, r2) = (l.num("R1"), l.num("R2"));
	let flip = {
		let f = f.clone();
		let (pr1, pr2) = (l.path("R1"), l.path("R2"));
		move |_: bool| {
			f.set(&pr1, Value::from(-r1));
			f.set(&pr2, Value::from(-r2));
		}
	};
	let mut rows = vec![l.number("Diameter", 10.)];
	if fresnel.is_none() {
		rows.push(
			PropertyRow::new(
				"",
				super::properties::check_box(s.label("LensPart", "IsConcave").into(), r1 < 0., flip)
					.test_id("LensPart.IsConcave"),
			)
			.hint(s.hint("LensPart", "IsConcave"))
			.full_width(),
		);
	}
	rows.extend([l.number_in("R1", "", 10.), l.number_in("R2", "", 10.), l.number("Thickness", 1.)]);
	if let Some(variant) = fresnel {
		let z = f.scope("FresnelLensPart", at, variant);
		rows.push(z.number("Zones", 1.));
		rows.push(z.combo("Division", "ZoneDivision", &["EqualWidth", "EqualDepth"]));
	}
	rows.push(
		readout(s.get("LabelEdgeThickness"), r.lens_edge.map(px).unwrap_or_default(), "LensEdgeThickness")
			.hint(s.get("TipEdgeThickness")),
	);
	section(s.get("SectionShape"), rows)
}

fn triangle(f: &Form, r: &Readouts, at: &str, variant: &str) -> Element {
	let s = &f.s;
	let t = f.scope("TrianglePart", at, variant);
	let path = t.path("Kind");
	let tt = f.word(&path);
	let a_on = tt == "Isosceles" || tt == "Custom";
	section(
		s.get("SectionShape"),
		vec![
			f.combo(
				"TrianglePart",
				"Type",
				&path,
				"TriangleType",
				&["Equilateral", "RightIsosceles", "ThirtySixtyNinety", "Isosceles", "Custom"],
			),
			t.number("Base", 10.),
			t.decimals("AngleA", 0.5, 1).enabled(a_on),
			t.decimals("AngleB", 0.5, 1).enabled(tt == "Custom"),
			readout(s.get("LabelApexAngle"), r.apex.map(|a| format!("{a:.1} deg")).unwrap_or_default(), "TriangleApex")
				.hint(s.get("TipApexAngle")),
		],
	)
}

fn zone_plate(f: &Form, r: &Readouts, at: &str, variant: &str) -> Vec<Element> {
	let s = &f.s;
	let z = f.scope("ZonePlatePart", at, variant);
	let mount = z.flag("Mount");
	let (count, outer) = r.zones.map_or((String::new(), String::new()), |(n, o)| (n.to_string(), px(o)));
	vec![
		section(
			s.get("SectionDesign"),
			vec![
				z.decimals("DesignFrequency", 0.1, 2),
				readout(s.get("LabelWavelength"), r.wavelength.map(px).unwrap_or_default(), "ZoneWavelength")
					.hint(s.get("TipGateWavelength")),
				z.number("FocalLength", 10.),
				z.check("Inverse"),
			],
		),
		section(
			s.get("SectionShape"),
			vec![
				z.number("Diameter", 10.),
				z.number("Thickness", 1.),
				z.number("Coating", 1.),
				z.check("Mount"),
				z.number("MountWidth", 10.).enabled(mount),
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

/// A list edited as a whole: `edit` changes the model's list, which is then set at `path`.
fn set_list<T: Serialize + DeserializeOwned>(f: &Form, path: &str, edit: impl FnOnce(&mut Vec<T>)) {
	let mut list: Vec<T> = serde_json::from_value(f.get(path).clone()).expect("the list reads back");
	edit(&mut list);
	f.set(path, to_json(list));
}

/// A number box for one coordinate of item `index` of the list at `path`.
fn list_number<T: Serialize + DeserializeOwned + 'static>(
	f: &Form,
	path: &str,
	index: usize,
	value: i32,
	set: fn(&mut T, i32),
	test: String,
) -> Element {
	let f2 = f.clone();
	let path = path.to_owned();
	wavedude_widgets::NumberBox::new(
		WritableSetter::new(f64::from(value), move |v: f64| {
			set_list(&f2, &path, |list: &mut Vec<T>| set(&mut list[index], v.round() as i32))
		}),
		NumberSpec::integer(-1e6, 1e6),
	)
	.width(Size::flex(1.))
	.test_id(test)
	.into_element()
}

fn remove_button<T: Serialize + DeserializeOwned>(
	f: &Form,
	path: &str,
	index: usize,
	tip_key: &str,
	test: String,
) -> Element {
	let f2 = f.clone();
	let path = path.to_owned();
	let button = tip(
		f.s.get(tip_key),
		Button::new()
			.compact()
			.on_press(move |_| {
				set_list(&f2, &path, |list: &mut Vec<T>| {
					list.remove(index);
				})
			})
			.child(IconView::new(icons::MINUS)),
	);
	tagged(button, test)
}

fn hint_row(text: &str) -> PropertyRow {
	PropertyRow::new("", paragraph().width(Size::fill()).span(Span::new(text.to_owned()))).full_width()
}

fn points(f: &Form, path: &str) -> Element {
	let s = &f.s;
	let list: Vec<m::Point> = f.bind_as(path);
	let mut rows = vec![hint_row(s.get("HintPolygonPoints"))];
	for (i, p) in list.iter().enumerate() {
		let editors = rect()
			.horizontal()
			.width(Size::fill())
			.content(Content::flex())
			.spacing(4.)
			.child(list_number(f, path, i, p.x, |p: &mut m::Point, v| p.x = v, format!("Point{i}.X")))
			.child(list_number(f, path, i, p.y, |p: &mut m::Point, v| p.y = v, format!("Point{i}.Y")))
			.child(remove_button::<m::Point>(f, path, i, "TipRemovePoint", format!("Point{i}.Remove")));
		rows.push(PropertyRow::new(format!("{}", i + 1), editors));
	}
	let add = {
		let f = f.clone();
		let path = path.to_owned();
		move |_: Event<PressEventData>| {
			set_list(&f, &path, |list: &mut Vec<m::Point>| {
				let last = list.last().copied().unwrap_or_default();
				list.push(m::Point::new(last.x + 10, last.y));
			})
		}
	};
	rows.push(PropertyRow::new("", dialogs::button(s.get("ButtonAddPoint"), true, add)).hint(s.get("TipAddPoint")));
	section(s.get("SectionPoints"), rows)
}

fn fiber(f: &Form, at: &str, variant: &str) -> Vec<Element> {
	let s = &f.s;
	let fb = f.scope("FiberPart", at, variant);
	let mut cladding = vec![
		fb.number("CladdingIndex", 0.01),
		fb.number("CladdingThickness", 1.),
		fb.number("Absorption", 5.),
		fb.number("AbsorptionRamp", 1.),
	];
	cladding.extend(coating_rows(f, &fb.path("Coating"), false));
	let nodes_path = fb.path("Nodes");
	let list: Vec<m::FiberNode> = f.bind_as(&nodes_path);
	let mut nodes = vec![hint_row(s.get("HintFiberNodes"))];
	type Set = fn(&mut m::FiberNode, i32);
	for (i, node) in list.iter().enumerate() {
		let pair = |(a, set_a): (i32, Set), (b, set_b): (i32, Set), tag: &str| {
			rect()
				.horizontal()
				.width(Size::fill())
				.content(Content::flex())
				.spacing(4.)
				.child(list_number(f, &nodes_path, i, a, set_a, format!("Node{i}.{tag}X")))
				.child(list_number(f, &nodes_path, i, b, set_b, format!("Node{i}.{tag}Y")))
		};
		let toggle = {
			let f = f.clone();
			let path = nodes_path.clone();
			move |on: bool| set_list(&f, &path, |l: &mut Vec<m::FiberNode>| l[i].smooth = on)
		};
		let head = pair((node.x, |n, v| n.x = v), (node.y, |n, v| n.y = v), "");
		nodes.push(PropertyRow::new(format!("{} {}", s.get("LabelNodePoint"), i + 1), head));
		nodes.push(PropertyRow::new(
			s.get("LabelNodeIn"),
			pair((node.in_x, |n, v| n.in_x = v), (node.in_y, |n, v| n.in_y = v), "In"),
		));
		nodes.push(PropertyRow::new(
			s.get("LabelNodeOut"),
			pair((node.out_x, |n, v| n.out_x = v), (node.out_y, |n, v| n.out_y = v), "Out"),
		));
		let tail = rect()
			.horizontal()
			.width(Size::fill())
			.content(Content::flex())
			.cross_align(Alignment::center())
			.spacing(4.)
			.child(rect().width(Size::flex(1.)).child(super::properties::check_box(
				s.label("FiberNode", "Smooth").into(),
				node.smooth,
				toggle,
			)))
			.child(remove_button::<m::FiberNode>(f, &nodes_path, i, "TipRemoveNode", format!("Node{i}.Remove")));
		nodes.push(PropertyRow::new("", tail));
	}
	let add = {
		let f = f.clone();
		let path = nodes_path.clone();
		move |_: Event<PressEventData>| {
			set_list(&f, &path, |l: &mut Vec<m::FiberNode>| {
				let (x, y) = l.last().map_or((0, 0), |n| (n.x + 50, n.y));
				l.push(m::FiberNode { x, y, in_x: -20, in_y: 0, out_x: 20, out_y: 0, smooth: true });
			})
		}
	};
	nodes.push(PropertyRow::new("", dialogs::button(s.get("ButtonAddNode"), true, add)).hint(s.get("TipAddNode")));
	vec![
		section(s.get("SectionCore"), vec![fb.number("CoreIndex", 0.01), fb.number("CoreDiameter", 1.)]),
		section(s.get("SectionCladding"), cladding),
		section(s.get("SectionShape"), vec![fb.combo("Ends", "FiberEnds", &["Flat", "Round"])]),
		section(s.get("SectionNodes"), nodes),
	]
}

fn gate(f: &Form, r: &Readouts, at: &str, variant: &str, kind: m::GateKind) -> Vec<Element> {
	let s = &f.s;
	let g = f.scope("LogicGatePart", at, variant);
	let (layout, ports, quarter) = r.gate.unwrap_or(([0; 4], [m::Point::default(); 4], f64::NAN));
	let point = |q: m::Point| format!("{}, {}", q.x, q.y);
	let quarter_text = if quarter.is_finite() { px(quarter / 4.) } else { String::new() };
	let output_word = |upper: bool| {
		let word = match (kind, upper) {
			(m::GateKind::Xor, true) | (m::GateKind::Or, false) => "Xor",
			(m::GateKind::And, true) => "And",
			(m::GateKind::Or, true) => "Or",
			_ => "Remainder",
		};
		s.enum_word("GateOutput", word)
	};
	let output = |member: &str, upper: bool| {
		f.check_labelled(
			format!("{}: {}", s.label("LogicGatePart", member), output_word(upper)),
			s.hint("LogicGatePart", member),
			&g.path(member),
			&format!("LogicGatePart.{member}"),
		)
	};
	vec![
		section(
			s.get("SectionDesign"),
			vec![
				g.decimals("DesignFrequency", 0.1, 2),
				readout(s.get("LabelWavelength"), r.wavelength.map(px).unwrap_or_default(), "GateWavelength")
					.hint(s.get("TipGateWavelength")),
			],
		),
		section(
			s.get("SectionMaterial"),
			vec![
				g.number("CoreIndex", 0.01),
				g.number("CladdingIndex", 0.01),
				g.number("Coating", 1.),
				g.check("InputCoated"),
				g.check("OutputCoated"),
				readout(s.get("LabelQuarterWave"), quarter_text, "GateQuarterWave").hint(s.get("TipQuarterWave")),
			],
		),
		section(s.get("SectionShape"), vec![g.number("PortSpacing", 2.)]),
		section(s.get("SectionOutputs"), vec![output("UpperOutputActive", true), output("LowerOutputActive", false)]),
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
				s.label("LogicGatePart", "UpperOutputActive"),
				s.label("LogicGatePart", "LowerOutputActive"),
			]
			.iter()
			.zip(ports)
			.enumerate()
			.map(|(i, (text, q))| readout(text, point(q), &format!("GatePort{i}")).hint(s.get("TipGatePorts")))
			.collect(),
		),
	]
}

fn probe(f: &Form) -> Vec<Element> {
	let s = &f.s;
	let p = f.scope("Probe", "", "Probe");
	let line = p.word("Kind") == "Line";
	let measurements: &[&str] = if p.word("Layer") == "Field" { &["Amplitude"] } else { &["Value", "IntegratedValue"] };
	let custom = p.word("BoxSize") == "Custom";
	let log = p.word("YScale") == "Logarithmic";
	vec![
		section(s.get("SectionGeneral"), vec![f.name("Probe"), p.check("Active"), p.number("StartAfter", 1.)]),
		section(
			s.get("SectionMeasurement"),
			vec![
				p.combo("Layer", "ProbeLayer", &["Field", "Intensity"]),
				p.combo("Measurement", "ProbeMeasurement", measurements),
				p.combo("Kind", "ProbeKind", &["Point", "Line"]),
			],
		),
		section(
			s.get("SectionPosition"),
			vec![
				p.number_in("AX", "Part.X", 1.),
				p.number_in("AY", "Part.Y", 1.),
				p.number_in("BX", "Part.X", 1.).enabled(line),
				p.number_in("BY", "Part.Y", 1.).enabled(line),
			],
		),
		section(
			s.get("SectionGraph"),
			vec![
				p.combo("BoxSize", "ProbeBoxSize", &["Tiny", "Small", "Medium", "Large", "ExtraLarge", "Custom"]),
				p.number_in("BoxX", "Part.X", 1.),
				p.number_in("BoxY", "Part.Y", 1.),
				p.number("BoxWidth", 10.).enabled(custom),
				p.number("BoxHeight", 10.).enabled(custom),
			],
		),
		section(
			s.get("SectionAxes"),
			vec![
				p.combo("ScaleMode", "ProbeScaleMode", &["Running", "Full"]),
				p.number("Window", 10.),
				p.check("AutoScaleX"),
				p.check("AutoScaleY"),
				p.combo("YScale", "ProbeValueScale", &["Linear", "Logarithmic"]),
				p.number("Decades", 1.).enabled(log),
				p.number_in("YMin", "", 0.1),
				p.number_in("YMax", "", 0.1),
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

fn stops_json(g: &Gradient) -> Value {
	let stops: Vec<m::Stop> = g
		.stops
		.iter()
		.map(|s| m::Stop { at: s.position, color: m::Rgb::new(s.color.r, s.color.g, s.color.b) })
		.collect();
	to_json(stops)
}

/// An edit of stop `i` of `gradient` by `change`, which says whether it changed anything; sets the whole stop list.
fn stop_edit<T: 'static>(
	f: &Form,
	gradient: &Gradient,
	change: fn(&mut Gradient, usize, T) -> bool,
) -> impl Fn(usize, T) + use<T> {
	let (f, gradient) = (f.clone(), gradient.clone());
	move |i, v| {
		let mut g = gradient.clone();
		if change(&mut g, i, v) {
			f.set("stops", stops_json(&g));
		}
	}
}

fn scheme(f: &Form, r: &Readouts) -> Vec<Element> {
	let s = &f.s;
	let sc = f.scope("ColorScheme", "", "ColorScheme");
	let gradient = to_gradient(&f.bind_as::<Vec<m::Stop>>("stops"));
	let set_gradient = {
		let f = f.clone();
		move |g: Gradient| f.set("stops", stops_json(&g))
	};
	let based_on = {
		let form = f.clone();
		let items = r.based_on.iter().map(|n| (n.clone(), n.clone())).collect();
		combo_box(&f.cx, String::new(), items, "ColorScheme.BasedOn".into(), move |name| {
			let f = &form;
			let source = f.cx.shared.peek(|a| {
				schemes::find(&a.document.project.color_schemes, &name)
					.or_else(|| schemes::find(&a.schemes.user, &name))
					.or_else(|| schemes::find(schemes::built_in(), &name))
					.cloned()
			});
			if let Some(src) = source {
				f.set("stops", to_json(&src.stops));
				f.set("kind", to_json(src.kind));
				f.set("description", Value::from(src.description));
			}
		})
	};
	let mut rows = vec![
		PropertyRow::new(
			"",
			tagged(GradientEditor::new(WritableSetter::new(gradient.clone(), set_gradient)), "SchemeGradient"),
		)
		.full_width(),
	];
	for (i, stop) in gradient.stops.iter().enumerate() {
		let set_at = stop_edit(f, &gradient, |g, i, at: f64| {
			g.set_position(i, at);
			true
		});
		let set_color = stop_edit(f, &gradient, |g, i, c: Rgb| {
			g.set_color(i, c);
			true
		});
		let set_hex = stop_edit(f, &gradient, |g, i, hex: String| match Rgb::from_hex(&hex) {
			Some(c) => {
				g.set_color(i, c);
				true
			}
			None => false,
		});
		let remove = stop_edit(f, &gradient, |g, i, ()| g.remove(i));
		let editors = rect()
			.horizontal()
			.width(Size::fill())
			.spacing(4.)
			.cross_align(Alignment::center())
			.child(label().width(Size::px(16.)).text(format!("{}", i + 1)))
			.child(
				wavedude_widgets::NumberBox::new(
					WritableSetter::new(stop.position, move |v: f64| set_at(i, v)),
					NumberSpec::linear(0., 1., 0.01),
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
	let add = stop_edit(f, &gradient, |g, _, ()| {
		g.add_in_widest_gap();
		true
	});
	rows.push(
		PropertyRow::new("", dialogs::button(s.get("ButtonAddStop"), true, move |_| add(0, ())))
			.hint(s.get("TipAddStop")),
	);
	vec![
		section(
			s.get("SectionGeneral"),
			vec![
				f.name("ColorScheme"),
				sc.combo("Kind", "ColorSchemeKind", &["Unspecified", "Field", "Intensity"]),
				sc.text("Description", false),
				PropertyRow::new(s.get("LabelBasedOn"), based_on).hint(s.get("TipBasedOn")),
			],
		),
		section(s.get("SectionStops"), rows),
	]
}
