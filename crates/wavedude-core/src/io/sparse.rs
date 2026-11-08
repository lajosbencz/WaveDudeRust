//! Sparse JSON of the model: every value equal to its type's default is left out, object by object, so a file
//! says what differs; the reader's serde defaults put the rest back (`#[serde(default)]` on every model type).

use serde::Serialize;
use serde_json::{Map, Value};

use crate::model::{
	Coating, ColorScheme, Display, DisplayLayer, Material, PartKind, Probe, Project, Source, SourceArray, Video,
};

fn to_value(v: impl Serialize) -> Value {
	serde_json::to_value(v).expect("a model value serialises")
}

/// Removes from `value` the members equal to `default`'s, descending into objects both hold; an object left
/// empty stays (an enum's variant is its key). Arrays are kept whole: an item's place is its meaning.
pub fn strip(value: &mut Value, default: &Value) {
	let (Value::Object(v), Value::Object(d)) = (value, default) else { return };
	v.retain(|key, item| match d.get(key) {
		Some(default) if item == default => false,
		Some(default @ Value::Object(_)) if item.is_object() => {
			strip(item, default);
			true
		}
		_ => true,
	});
}

/// Strips each item of the list at `key` against `default_of(item)`.
fn strip_list(object: &mut Value, key: &str, default_of: impl Fn(&Value) -> Value) {
	let Some(items) = object.get_mut(key).and_then(Value::as_array_mut) else { return };
	for item in items {
		let default = default_of(item);
		strip(item, &default);
	}
}

/// A nested object's missing members take its own type's default, whatever its parent holds, so the defaults
/// a value is stripped against are built type by type.
fn display_default() -> Value {
	let mut d = to_value(Display::default());
	d["field"] = to_value(DisplayLayer::default());
	d["intensity"] = to_value(DisplayLayer::default());
	d["video"] = to_value(Video::default());
	d
}

/// The default part of the kind a part value names, which has every field at its default.
fn default_part(part: &Value) -> Value {
	let kind = part.get("kind").and_then(Value::as_object).and_then(|k| k.keys().next().cloned());
	let kind = kind.and_then(|name| {
		let probe = Value::Object([(name, Value::Object(Map::new()))].into_iter().collect());
		serde_json::from_value::<PartKind>(probe).ok()
	});
	match kind {
		Some(kind) => {
			let mut d = to_value(crate::model::Part::new(kind, 0, 0));
			if let Some(body) = d["kind"].as_object_mut().and_then(|k| k.values_mut().next()) {
				if body.get("material").is_some() {
					body["material"] = to_value(Material::default());
				}
				if body.get("coating").is_some() {
					body["coating"] = to_value(Coating::default());
				}
			}
			d
		}
		None => Value::Null,
	}
}

/// `project` as sparse JSON.
pub fn project(project: &Project) -> Value {
	let mut v = to_value(project);
	let mut d = to_value(Project::default());
	d["display"] = display_default();
	for key in ["seed", "description", "scene", "simulation", "display"] {
		let (Some(item), Some(default)) = (v.get_mut(key), d.get(key)) else { continue };
		if item == default {
			v.as_object_mut().expect("a project is an object").remove(key);
		} else {
			strip(item, default);
		}
	}
	strip_list(&mut v, "sources", |_| {
		let mut d = to_value(Source::default());
		d["array"] = to_value(SourceArray::default());
		d
	});
	// A part's `kind` names its variant, so it stays whatever its body holds; the body is stripped on its own.
	if let Some(parts) = v.get_mut("parts").and_then(Value::as_array_mut) {
		for part in parts {
			let mut default = default_part(part);
			let default_body = default
				.get_mut("kind")
				.and_then(Value::as_object_mut)
				.and_then(|k| k.values_mut().next())
				.map(Value::take);
			default.as_object_mut().map(|d| d.remove("kind"));
			strip(part, &default);
			let body = part.get_mut("kind").and_then(Value::as_object_mut).and_then(|k| k.values_mut().next());
			if let (Some(body), Some(default_body)) = (body, default_body) {
				strip(body, &default_body);
			}
		}
	}
	strip_list(&mut v, "probes", |_| to_value(Probe::default()));
	strip_list(&mut v, "color_schemes", |_| to_value(ColorScheme::default()));
	for key in ["sources", "parts", "probes", "color_schemes"] {
		if v.get(key).and_then(Value::as_array).is_some_and(Vec::is_empty) {
			v.as_object_mut().expect("a project is an object").remove(key);
		}
	}
	v
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;
	use crate::model::normalize::Normalize;
	use crate::model::{Lens, Part};

	#[test]
	fn equal_members_go_and_nested_objects_are_stripped_in_turn() {
		let mut v = json!({ "a": 1, "b": { "c": 2, "d": 3 }, "e": { "f": 1 }, "list": [1, 2] });
		let d = json!({ "a": 1, "b": { "c": 2, "d": 0 }, "e": { "f": 1 }, "list": [1] });
		strip(&mut v, &d);
		assert_eq!(v, json!({ "b": { "d": 3 }, "list": [1, 2] }));
	}

	#[test]
	fn a_sparse_project_reads_back_whole() {
		let mut p = Project::default();
		p.add_source();
		p.sources[0].frequency = 42.0;
		p.sources[0].array = Some(SourceArray { size_x: 3, ..SourceArray::default() });
		p.add_part(PartKind::Lens(Lens { diameter: 123, ..Lens::default() }));
		p.add_probe();
		p.add_new_color_scheme();
		p.normalize();
		let sparse = project(&p);
		let text = sparse.to_string();
		assert!(text.contains("\"frequency\":42.0") && text.contains("\"diameter\":123"), "{text}");
		assert!(!text.contains("\"amplitude\"") && !text.contains("\"material\""), "defaults are left out: {text}");
		assert!(text.contains("\"size_x\":3") && !text.contains("\"size_y\""), "{text}");
		let back: Project = serde_json::from_value(sparse).unwrap();
		assert_eq!(back, p);
		let plain = Part::new(PartKind::Lens(Lens::default()), 0, 0);
		assert_eq!(default_part(&to_value(&plain)), to_value(&plain));
	}
}
