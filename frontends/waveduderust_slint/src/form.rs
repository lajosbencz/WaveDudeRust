//! The properties pane's form of any model type, from its reflected fields (docs/ui/30, Forms).

use std::collections::HashMap;
use waveduderust::model::fields::{Edit, EditError, Field, Fields, Value, ValueKind};

use waveduderust::model::edited::Edited;
use waveduderust::model::lens_geometry::{edge_thickness, fit_radii_zoned};
use waveduderust::model::{
	Lens, PROJECT_VERSION, Part, PartKind, Point, Project, Stencil, View, ZoneDivision, wavelength,
};

use crate::tree::Node;
use crate::ui::{PropKind, PropRow};

/// The fields a form edits: scalars and enums; lists and composites have their own rows in the tree.
/// Nested objects with a tree node of their own, so a form never shows their fields.
const OWN_NODES: [&str; 2] = ["Scene", "Simulation"];

fn editable<T: Fields>() -> Vec<T::Field> {
	T::Field::all()
		.into_iter()
		.filter(|f| !f.dependent() && !matches!(f.kind(), ValueKind::List | ValueKind::Nested))
		.filter(|f| !f.path().split_once('.').is_some_and(|(top, _)| OWN_NODES.contains(&top)))
		.collect()
}

/// `PascalCase` or `snake_case` as words, a path's dots as spaces: `Field.PulseWidth` is `Field pulse width`.
fn label(name: &str) -> String {
	let mut out = String::new();
	for (i, c) in name.chars().enumerate() {
		if c == '_' || c == '.' {
			out.push(' ');
		} else if c.is_uppercase() && i > 0 {
			if !out.ends_with(' ') {
				out.push(' ');
			}
			out.extend(c.to_lowercase());
		} else {
			out.push(c);
		}
	}
	out
}

/// The rows of `of`'s form.
pub(crate) fn rows<T: Fields>(of: &T) -> Vec<PropRow> {
	editable::<T>()
		.into_iter()
		.map(|f| {
			let (kind, choices): (PropKind, &[&str]) = match f.kind() {
				ValueKind::Bool => (PropKind::Bool, &[]),
				ValueKind::Int | ValueKind::UInt => (PropKind::Int, &[]),
				ValueKind::Float => (PropKind::Float, &[]),
				ValueKind::Word(names) => (PropKind::Choice, names),
				_ => (PropKind::Text, &[]),
			};
			let value = f.get(of).map_or_else(String::new, |v| v.to_string());
			let range = f.range().map_or((f32::MIN, f32::MAX), |r| (r.min as f32, r.max as f32));
			let spec = match kind {
				PropKind::Int => crate::number::whole(range.0, range.1),
				PropKind::Float => crate::number::NumberSpec {
					min: range.0,
					max: range.1,
					increment: 1.0,
					decimals: -1,
					..Default::default()
				},
				_ => Default::default(),
			};
			let current = choices.iter().position(|c| *c == value).map_or(-1, |i| i as i32);
			PropRow {
				label: label(&f.path()).into(),
				kind,
				value: value.into(),
				choices: slint::ModelRc::from(
					choices.iter().map(|c| slint::SharedString::from(*c)).collect::<Vec<_>>().as_slice(),
				),
				current,
				spec,
				form: 0,
			}
		})
		.collect()
}

/// Sets row `row` of `of`'s form from the text the user entered.
pub(crate) fn set<T: Fields>(of: &mut T, row: usize, text: &str) -> Result<(), String> {
	let field = *editable::<T>().get(row).ok_or("no such row")?;
	let value = match field.kind() {
		ValueKind::Bool => Value::Bool(text == "true"),
		ValueKind::Int | ValueKind::UInt => {
			Value::Int(text.trim().parse().map_err(|_| format!("{text} is no whole number"))?)
		}
		ValueKind::Float => Value::Float(crate::number::parse(text).ok_or_else(|| format!("{text} is no number"))?),
		ValueKind::Word(names) => Value::Word(names.iter().find(|n| **n == text).ok_or("unknown choice")?),
		_ => Value::Text(text.into()),
	};
	field.edit(value).and_then(|e| e.apply(of)).map(drop).map_err(|e: EditError| e.to_string())
}

/// Runs `$body` with `$of` bound to the object `$node` stands for in `$p` (the project) or `$v` (its view).
macro_rules! with_object {
	($p:expr, $v:expr, $node:expr, |$of:ident| $body:expr, $none:expr $(, $m:tt)?) => {
		match $node {
			Node::Project => { let $of = &$($m)? *$p; $body }
			Node::Scene => { let $of = &$($m)? $p.scene; $body }
			Node::Simulation => { let $of = &$($m)? $p.simulation; $body }
			Node::Display => { let $of = &$($m)? *$v; $body }
			Node::Source(i) => { let $of = &$($m)? $p.sources[i]; $body }
			Node::Part(i) => { let $of = &$($m)? $p.parts[i].base; $body }
			Node::Probe(i) => { let $of = &$($m)? $p.probes[i]; $body }
			Node::Scheme(i) => { let $of = &$($m)? $p.color_schemes[i]; $body }
			Node::Sources | Node::Parts | Node::Probes | Node::Schemes => $none,
		}
	};
}

/// The Slint form `node` shows (ui/forms.slint); empty for the generic rows.
pub(crate) fn kind(project: &Project, node: Node) -> &'static str {
	match node {
		Node::Part(i) => match project.parts.get(i).map(|p| &p.kind) {
			Some(PartKind::Rectangle(_)) => "rectangle",
			Some(PartKind::Ellipse(_)) => "ellipse",
			Some(PartKind::Triangle(_)) => "triangle",
			Some(PartKind::Polygon(_)) => "polygon",
			Some(PartKind::Lens(_)) => "lens",
			Some(PartKind::FresnelLens(_)) => "fresnel-lens",
			Some(PartKind::Outline(_)) => "outline",
			_ => "",
		},
		Node::Project => "project",
		Node::Scene => "scene",
		Node::Simulation => "simulation",
		Node::Display => "display",
		Node::Source(_) => "source",
		Node::Probe(_) => "probe",
		_ => "",
	}
}

/// Every field of `of` as text by its path.
fn paths<T: Fields>(of: &T) -> HashMap<String, String> {
	let mut values = HashMap::new();
	for f in T::Field::all() {
		let Some(value) = f.get(of) else { continue };
		// an absent optional step reads as Next's -1
		let value = if value == Value::Absent { "-1".to_owned() } else { value.to_string() };
		// a word's index among its kind's, for a combo box
		if let ValueKind::Word(names) = f.kind() {
			let index = names.iter().position(|n| *n == value).map_or(-1, |i| i as i32);
			values.insert(format!("{}#", f.path()), index.to_string());
		}
		values.insert(f.path(), value);
	}
	values
}

/// Where a part's own fields sit below `Kind.`: its variant's name and a dot.
fn kind_prefix(part: &Part) -> String {
	let path =
		<Part as Fields>::Field::all().into_iter().find(|f| f.path().starts_with("Kind.") && f.get(part).is_some());
	path.and_then(|f| f.path().split('.').nth(1).map(|k| format!("Kind.{k}."))).unwrap_or_default()
}

/// The values a form of `node` shows by path: its fields and the read-outs Next derives from them. A part's own
/// fields are also by their path below its kind, so one form fits every kind that has them.
pub(crate) fn values(project: &Project, view: &View, node: Node) -> HashMap<String, String> {
	let mut values = match node {
		Node::Part(i) => {
			let part = &project.parts[i];
			let mut values = paths(part);
			let prefix = kind_prefix(part);
			let own: Vec<_> =
				values.iter().filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.to_owned(), v.clone()))).collect();
			values.extend(own);
			values
		}
		_ => with_object!(project, view, node, |of| paths(of), HashMap::new()),
	};
	match node {
		Node::Project => {
			values.insert("Version".into(), PROJECT_VERSION.to_string());
		}
		Node::Scene => {
			let scene = &project.scene;
			let info = scene.image_data.as_ref().map(|bytes| match scene.image_size {
				(0, 0) => "(unreadable)".to_owned(),
				(w, h) => format!("{w} \u{d7} {h}, {} KB", (bytes.len() / 1024).max(1)),
			});
			values.insert("EmbeddedImage".into(), info.unwrap_or_default());
		}
		Node::Source(i) => {
			let source = &project.sources[i];
			values.insert("IsComposite".into(), source.array.is_some().to_string());
			let sim = &project.simulation;
			let lambda = wavelength(source.frequency, sim.wave_speed, sim.source_frequency_scale);
			values.insert("Wavelength".into(), if lambda.is_finite() { format!("{lambda:.1}") } else { String::new() });
			values.insert("NinePoint".into(), (sim.stencil == Stencil::NinePoint).to_string());
		}
		Node::Part(i) => match &project.parts[i].kind {
			PartKind::Triangle(t) => {
				// Next's TriangleApexConverter
				let (a, b) = t.effective_angles();
				values.insert("ApexAngle".into(), format!("{}\u{b0}", one_decimal(180.0 - a - b)));
			}
			PartKind::Lens(l) => {
				values.insert("IsConcave".into(), l.is_concave().to_string());
				values.insert("EdgeThickness".into(), edge_text(l, 1, ZoneDivision::EqualWidth));
			}
			PartKind::FresnelLens(f) => {
				values.insert("EdgeThickness".into(), edge_text(&f.lens, f.zones, f.division));
			}
			PartKind::Outline(o) => {
				values.insert("Points#".into(), o.points.len().to_string());
				for (k, p) in o.points.iter().enumerate() {
					values.insert(format!("Points.{k}.X"), p.x.to_string());
					values.insert(format!("Points.{k}.Y"), p.y.to_string());
				}
			}
			_ => {}
		},
		_ => {}
	}
	values
}

/// A lens's thickness at the rim with its radii fitted as a run fits them (Next's LensEdgeConverter).
fn edge_text(l: &Lens, zones: u32, division: ZoneDivision) -> String {
	let (diameter, thickness) = (l.diameter.max(2), l.thickness.max(1));
	let (r1, r2) = fit_radii_zoned(diameter, thickness, l.r1, l.r2, zones, division);
	format!("{} px", one_decimal(edge_thickness(diameter, thickness, r1, r2, zones, division)))
}

/// `v` as .NET's `0.#`: one decimal, none when it is 0.
fn one_decimal(v: f64) -> String {
	let text = format!("{v:.1}");
	text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}

/// Edits an outline's points: `Add` appends one a step right of the last (Next's PartPointsView), `Remove` takes the
/// one at index `text`, `<index>.X` and `<index>.Y` set a coordinate.
fn set_point(points: &mut Vec<Point>, path: &str, text: &str) -> Result<(), String> {
	const STEP: i32 = 20;
	match path {
		"Add" => {
			let last = points.last().copied();
			points.push(last.map_or(Point::new(0, 0), |p| Point::new(p.x + STEP, p.y)));
		}
		"Remove" => {
			let k = text.parse::<usize>().ok().filter(|&k| k < points.len()).ok_or("no such point")?;
			points.remove(k);
		}
		_ => {
			let (k, axis) = path.split_once('.').ok_or("no such field")?;
			let point = k.parse::<usize>().ok().and_then(|k| points.get_mut(k)).ok_or("no such point")?;
			set_path(point, axis, text)?;
		}
	}
	Ok(())
}

/// Sets the field at `path` of `of` from the text the user entered.
fn set_path<T: Fields>(of: &mut T, path: &str, text: &str) -> Result<(), String> {
	let field = T::Field::all().into_iter().find(|f| f.path() == path).ok_or("no such field")?;
	let value = match field.kind() {
		ValueKind::Bool => Value::Bool(text == "true"),
		ValueKind::Int | ValueKind::UInt => {
			let v = crate::number::parse(text).filter(|v| v.fract() == 0.0);
			let v = v.ok_or_else(|| format!("{text} is no whole number"))?;
			// a box's f32 bounds round i32::MAX up a step
			let (min, max) = if field.kind() == ValueKind::UInt {
				(0.0, f64::from(u32::MAX))
			} else {
				(f64::from(i32::MIN), f64::from(i32::MAX))
			};
			let (min, max) = field.range().map_or((min, max), |r| (r.min, r.max));
			match v.clamp(min, max) {
				// -1 of an optional step (its range's minimum) is no step
				v if v < 0.0 && field.kind() == ValueKind::UInt => Value::Absent,
				v => Value::Int(v as i64),
			}
		}
		ValueKind::Float => Value::Float(crate::number::parse(text).ok_or_else(|| format!("{text} is no number"))?),
		ValueKind::Word(names) => Value::Word(names.iter().find(|n| **n == text).ok_or("unknown choice")?),
		_ => Value::Text(text.into()),
	};
	field.edit(value).and_then(|e| e.apply(of)).map(drop).map_err(|e: EditError| e.to_string())
}

/// Sets the field at `path` of the object `node` stands for.
pub(crate) fn set_at(project: &mut Project, view: &mut View, node: Node, path: &str, text: &str) -> Result<(), String> {
	if let (Node::Source(i), "IsComposite") = (node, path) {
		let array = &mut project.sources.get_mut(i).ok_or("no such source")?.array;
		*array = (text == "true").then(|| array.take().unwrap_or_default());
		return Ok(());
	}
	if let Node::Probe(i) = node {
		let probe = project.probes.get_mut(i).ok_or("no such probe")?;
		let before = probe.clone();
		set_path(probe, path, text)?;
		probe.edited(&before);
		return Ok(());
	}
	if let Node::Part(i) = node {
		let part = project.parts.get_mut(i).ok_or("no such part")?;
		let before = part.clone();
		match (&mut part.kind, path) {
			(_, _) if path.starts_with("Base.") => set_path(part, path, text)?,
			(PartKind::Outline(o), _) if path.starts_with("Points.") => set_point(&mut o.points, &path[7..], text)?,
			(PartKind::Lens(l), "IsConcave") => l.set_concave(text == "true"),
			_ => set_path(part, &format!("{}{path}", kind_prefix(part)), text)?,
		}
		part.edited(&before);
		return Ok(());
	}
	with_object!(project, view, node, |of| set_path(of, path, text), Err("nothing to edit".into()), mut)
}

/// The heading and rows of the selected object's form.
pub(crate) fn of_selected(project: &Project, view: &View, node: Node) -> (String, Vec<PropRow>) {
	let rows = with_object!(project, view, node, |of| rows(of), Vec::new());
	(format!("{node:?}"), rows)
}

/// Sets row `row` of the selected object's form.
pub(crate) fn set_selected(
	project: &mut Project,
	view: &mut View,
	node: Node,
	row: usize,
	text: &str,
) -> Result<(), String> {
	with_object!(project, view, node, |of| set(of, row, text), Err("nothing to edit".into()), mut)
}

#[cfg(test)]
mod tests {
	use waveduderust::model::Source;

	use super::*;

	#[test]
	fn a_source_form_edits_its_name_and_rejects_a_bad_number() {
		let mut s = Source::default();
		let rows = rows(&s);
		let name = rows.iter().position(|r| r.label == "Name").unwrap();
		set(&mut s, name, "Laser").unwrap();
		assert_eq!(s.name, "Laser");
		let int = rows.iter().position(|r| r.kind == PropKind::Int).unwrap();
		assert!(set(&mut s, int, "many").is_err());
	}

	#[test]
	fn the_project_form_leaves_the_scene_and_simulation_to_their_nodes() {
		let rows = rows(&Project::default());
		assert!(rows.iter().all(|r| !r.label.starts_with("Scene") && !r.label.starts_with("Simulation")));
		assert!(rows.iter().any(|r| r.label == "Seed"));
	}

	#[test]
	fn words_read_as_spaced_lower_case() {
		assert_eq!(label("PulseWidth"), "Pulse width");
		assert_eq!(label("x"), "x");
		assert_eq!(label("Field.PulseWidth"), "Field pulse width");
	}
}

#[cfg(test)]
mod path_tests {
	use super::*;

	#[test]
	fn fields_read_and_set_by_path_with_next_s_read_outs() {
		let (mut p, mut v) = (Project::default(), View::default());
		let values = values(&p, &v, Node::Simulation);
		assert_eq!(values["Steps"], p.simulation.steps.to_string());
		assert_eq!((values["Stencil"].as_str(), values["Stencil#"].as_str()), ("FourthOrder", "1"));
		set_at(&mut p, &mut v, Node::Simulation, "Steps", "1200").unwrap();
		assert_eq!(p.simulation.steps, 1200);
		set_at(&mut p, &mut v, Node::Display, "Video.Fps", "30").unwrap();
		assert_eq!(v.video.fps, 30);
		assert!(set_at(&mut p, &mut v, Node::Scene, "Width", "1.5").is_err());
		assert_eq!(super::values(&p, &v, Node::Project)["Version"], PROJECT_VERSION.to_string());
		// -1 is no detach step, both ways
		p.sources.push(Default::default());
		set_at(&mut p, &mut v, Node::Source(0), "DetachAfter", "30").unwrap();
		assert_eq!(p.sources[0].detach_after, Some(30));
		set_at(&mut p, &mut v, Node::Source(0), "DetachAfter", "-1").unwrap();
		assert_eq!(p.sources[0].detach_after, None);
		assert_eq!(super::values(&p, &v, Node::Source(0))["DetachAfter"], "-1");
		// a probe takes Next's coupling: a detector is an integrated intensity line; the box on A follows it
		p.probes.push(Default::default());
		let probe = Node::Probe(0);
		(p.probes[0].box_x, p.probes[0].box_y) = (p.probes[0].ax, p.probes[0].ay);
		set_at(&mut p, &mut v, probe, "Ax", "40").unwrap();
		assert_eq!(p.probes[0].box_x, 40);
		set_at(&mut p, &mut v, probe, "Detector", "Counting").unwrap();
		assert_eq!(super::values(&p, &v, probe)["Measurement"], "IntegratedValue");
		set_at(&mut p, &mut v, probe, "Detector", "Standard").unwrap();
		set_at(&mut p, &mut v, probe, "Layer", "Field").unwrap();
		assert_eq!(super::values(&p, &v, probe)["Measurement"], "Amplitude");
		// a part's own fields by their path below its kind; Next's setters carry an edit along
		let mut rectangle = waveduderust::model::Rectangle::default();
		rectangle.linked = true;
		p.parts.push(Part::new(PartKind::Rectangle(rectangle), 0, 0));
		let part = Node::Part(0);
		set_at(&mut p, &mut v, part, "Width", "90").unwrap();
		assert_eq!(super::values(&p, &v, part)["Kind.Rectangle.Length"], "90");
		set_at(&mut p, &mut v, part, "Base.Name", "Slab").unwrap();
		assert_eq!(super::values(&p, &v, part)["Base.Name"], "Slab");
		p.parts[0].kind = PartKind::Triangle(waveduderust::model::Triangle::default());
		set_at(&mut p, &mut v, part, "Kind", "RightIsosceles").unwrap();
		assert_eq!(super::values(&p, &v, part)["AngleA"], "45");
		assert_eq!(super::values(&p, &v, part)["ApexAngle"], "90\u{b0}");
		// a lens: the mirror box renames it, the concave box flips both radii
		p.parts[0] = Part::new(PartKind::Lens(Lens::default()), 0, 0);
		p.parts[0].base.name = "Lens 1".into();
		set_at(&mut p, &mut v, part, "Material.Mirror", "true").unwrap();
		assert_eq!(p.parts[0].base.name, "Mirror 1");
		let concave = super::values(&p, &v, part)["IsConcave"] == "true";
		set_at(&mut p, &mut v, part, "IsConcave", &(!concave).to_string()).unwrap();
		assert_eq!(super::values(&p, &v, part)["IsConcave"], (!concave).to_string());
		// an outline's points: added a step right of the last, set, removed
		p.parts[0] = Part::new(PartKind::Outline(waveduderust::model::Outline::default()), 0, 0);
		set_at(&mut p, &mut v, part, "Points.Add", "").unwrap();
		set_at(&mut p, &mut v, part, "Points.Add", "").unwrap();
		set_at(&mut p, &mut v, part, "Points.0.Y", "5").unwrap();
		let values = super::values(&p, &v, part);
		assert_eq!(
			(values["Points#"].as_str(), values["Points.1.X"].as_str(), values["Points.0.Y"].as_str()),
			("2", "20", "5")
		);
		set_at(&mut p, &mut v, part, "Points.Remove", "0").unwrap();
		assert_eq!(super::values(&p, &v, part)["Points#"], "1");
	}
}
