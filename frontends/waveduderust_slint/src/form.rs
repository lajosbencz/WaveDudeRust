//! The properties pane's form of any model type, from its reflected fields (docs/ui/30, Forms).

use waveduderust::model::fields::{Edit, EditError, Field, Fields, Value, ValueKind};
use waveduderust::model::{Project, View};

use crate::tree::Node;
use crate::ui::{PropKind, PropRow};

/// The fields a form edits: scalars and enums; lists and composites have their own rows in the tree.
fn editable<T: Fields>() -> Vec<T::Field> {
	T::Field::all()
		.into_iter()
		.filter(|f| !f.dependent() && !matches!(f.kind(), ValueKind::List | ValueKind::Nested))
		.collect()
}

/// `PascalCase` or `snake_case` as words: `PulseWidth` is `Pulse width`.
fn label(name: &str) -> String {
	let mut out = String::new();
	for (i, c) in name.chars().enumerate() {
		if c == '_' {
			out.push(' ');
		} else if c.is_uppercase() && i > 0 {
			out.push(' ');
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
			let current = choices.iter().position(|c| *c == value).map_or(-1, |i| i as i32);
			PropRow {
				label: label(f.name()).into(),
				kind,
				value: value.into(),
				choices: slint::ModelRc::from(
					choices.iter().map(|c| slint::SharedString::from(*c)).collect::<Vec<_>>().as_slice(),
				),
				current,
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
		ValueKind::Float => Value::Float(text.trim().parse().map_err(|_| format!("{text} is no number"))?),
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
	fn words_read_as_spaced_lower_case() {
		assert_eq!(label("PulseWidth"), "Pulse width");
		assert_eq!(label("x"), "x");
	}
}
