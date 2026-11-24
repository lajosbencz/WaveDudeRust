//! The properties pane's form of any model type, from its reflected fields (docs/ui/30, Forms).

use std::collections::HashMap;
use waveduderust::model::fields::{Edit, EditError, Field, Fields, Value, ValueKind};

use waveduderust::model::{PROJECT_VERSION, Project, View};

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
pub(crate) fn kind(node: Node) -> &'static str {
	match node {
		Node::Project => "project",
		Node::Scene => "scene",
		Node::Simulation => "simulation",
		Node::Display => "display",
		_ => "",
	}
}

/// Every field of `of` as text by its path.
fn paths<T: Fields>(of: &T) -> HashMap<String, String> {
	let mut values = HashMap::new();
	for f in T::Field::all() {
		let Some(value) = f.get(of).map(|v| v.to_string()) else { continue };
		// a word's index among its kind's, for a combo box
		if let ValueKind::Word(names) = f.kind() {
			let index = names.iter().position(|n| *n == value).map_or(-1, |i| i as i32);
			values.insert(format!("{}#", f.path()), index.to_string());
		}
		values.insert(f.path(), value);
	}
	values
}

/// The values a form of `node` shows by path: its fields and the read-outs Next derives from them.
pub(crate) fn values(project: &Project, view: &View, node: Node) -> HashMap<String, String> {
	let mut values = with_object!(project, view, node, |of| paths(of), HashMap::new());
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
		_ => {}
	}
	values
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
			let (min, max) = field.range().map_or((min, max), |r| (r.min.max(min), r.max.min(max)));
			Value::Int(v.clamp(min, max) as i64)
		}
		ValueKind::Float => Value::Float(crate::number::parse(text).ok_or_else(|| format!("{text} is no number"))?),
		ValueKind::Word(names) => Value::Word(names.iter().find(|n| **n == text).ok_or("unknown choice")?),
		_ => Value::Text(text.into()),
	};
	field.edit(value).and_then(|e| e.apply(of)).map(drop).map_err(|e: EditError| e.to_string())
}

/// Sets the field at `path` of the object `node` stands for.
pub(crate) fn set_at(project: &mut Project, view: &mut View, node: Node, path: &str, text: &str) -> Result<(), String> {
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
	}
}
