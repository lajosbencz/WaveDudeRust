//! Typed attribute tables: one list of attributes per model type drives the `.wdproj` reader, the writer and the
//! DTD. An attribute names a typed field of the model; its value is read and written by the field's kind.
//!
//! Reading starts from a base value and sets each attribute present; writing compares with the same base,
//! so a type's defaults are stated once, in its base.

use super::FormatError;
use super::xml::{Element, fmt_f64};
use crate::model::{Field, Fields, Value, ValueKind};

/// How an attribute's value is read and written.
pub(crate) enum Codec<T> {
	/// By the field's kind.
	Field,
	/// A decimal field, written rounded: the file holds integers.
	Whole,
	/// Text of its own; `set` fails with what the text should have been.
	Custom { get: fn(&T) -> String, set: fn(&mut T, &str) -> Result<(), &'static str>, ty: AttrType },
}

/// When an attribute is written.
pub(crate) enum Presence<T> {
	/// Unless equal to the base's value.
	Sparse,
	/// Always; `#REQUIRED` in the DTD unless text.
	Always,
	/// When the predicate holds; `#IMPLIED` in the DTD.
	When(fn(&T) -> bool),
}

/// One attribute of an element.
pub(crate) struct Attr<T: Fields> {
	pub field: T::Field,
	/// None: the field's own name.
	name: Option<&'static str>,
	codec: Codec<T>,
	presence: Presence<T>,
}

impl<T: Fields> Attr<T> {
	/// A sparse attribute named like the field.
	pub(crate) const fn of(field: T::Field) -> Self {
		Self { field, name: None, codec: Codec::Field, presence: Presence::Sparse }
	}

	/// An attribute with a text of its own for `field`.
	pub(crate) const fn custom(
		field: T::Field,
		get: fn(&T) -> String,
		set: fn(&mut T, &str) -> Result<(), &'static str>,
		ty: AttrType,
	) -> Self {
		Self { field, name: None, codec: Codec::Custom { get, set, ty }, presence: Presence::Sparse }
	}

	pub(crate) const fn named(self, name: &'static str) -> Self {
		Self { name: Some(name), ..self }
	}

	pub(crate) const fn whole(self) -> Self {
		Self { codec: Codec::Whole, ..self }
	}

	pub(crate) const fn always(self) -> Self {
		Self { presence: Presence::Always, ..self }
	}

	pub(crate) const fn when(self, when: fn(&T) -> bool) -> Self {
		Self { presence: Presence::When(when), ..self }
	}

	pub(crate) fn name(&self) -> &'static str {
		self.name.unwrap_or_else(|| self.field.name())
	}

	fn read(&self, el: &Element, into: &mut T) -> Result<(), FormatError> {
		let name = self.name();
		let Some(text) = el.get(name) else {
			return Ok(());
		};
		if let Codec::Custom { set, .. } = &self.codec {
			return set(into, text).map_err(|what| el.bad(name, text, what));
		}
		let field = self.field;
		let value = if matches!(self.codec, Codec::Whole) {
			Value::Float(f64::from(el.parse_i32(name, text)?))
		} else {
			match field.kind() {
				ValueKind::Text => Value::Text(text.to_owned()),
				ValueKind::Int => Value::Int(i64::from(el.parse_i32(name, text)?)),
				// a negative value becomes 0 for `normalize` to raise
				ValueKind::UInt => Value::Int(i64::from(el.parse_i32(name, text)?.max(0))),
				ValueKind::Float => Value::Float(el.parse_f64(name, text)?),
				ValueKind::Bool => Value::Bool(el.parse_bool(name, text)?),
				ValueKind::Word(names) => {
					let member = names.iter().find(|n| **n == text.trim());
					Value::Word(member.ok_or_else(|| el.bad(name, text, &format!("one of {}", names.join(", "))))?)
				}
				ValueKind::List | ValueKind::Nested => return Err(el.bad(name, text, "a scalar")),
			}
		};
		field.edit(value).and_then(|edit| crate::model::Edit::apply(&edit, into)).map(drop).map_err(|e| FormatError {
			message: format!("element '{}': attribute '{name}': {e}", el.name),
			at: Some(el.at),
		})
	}

	fn same(&self, a: &T, b: &T) -> bool {
		match &self.codec {
			Codec::Custom { get, .. } => get(a) == get(b),
			Codec::Whole => self.text(a) == self.text(b),
			Codec::Field => match (self.field.get(a), self.field.get(b)) {
				(Some(Value::Float(x)), Some(Value::Float(y))) => x.to_bits() == y.to_bits(),
				(x, y) => x == y,
			},
		}
	}

	/// The attribute's text; None for an absent or composite value, which is not written.
	fn text(&self, of: &T) -> Option<String> {
		if let Codec::Custom { get, .. } = &self.codec {
			return Some(get(of));
		}
		match (&self.codec, self.field.get(of)?) {
			(Codec::Whole, Value::Float(x)) => Some((x.round() as i64).to_string()),
			(_, Value::Text(s)) => Some(s),
			(_, Value::Int(i)) => Some(i.to_string()),
			(_, Value::Float(x)) => Some(fmt_f64(x)),
			(_, Value::Bool(b)) => Some(b.to_string()),
			(_, Value::Word(w)) => Some(w.to_owned()),
			(_, Value::Absent | Value::Other(_)) => None,
		}
	}

	fn decl(&self) -> AttrDecl {
		let kind = self.field.kind();
		let ty = match (&self.codec, kind) {
			(Codec::Custom { ty, .. }, _) => *ty,
			(_, ValueKind::Bool) => AttrType::Bool,
			(_, ValueKind::Word(names)) => AttrType::Enum(names),
			_ => AttrType::Cdata,
		};
		let required = matches!(self.presence, Presence::Always) && kind != ValueKind::Text;
		AttrDecl { name: self.name(), ty, required, owner: Some(T::TYPE_NAME) }
	}
}

/// The type of an attribute in the DTD.
#[derive(Clone, Copy)]
pub(crate) enum AttrType {
	Cdata,
	Bool,
	Enum(&'static [&'static str]),
}

/// An attribute declaration.
pub(crate) struct AttrDecl {
	pub name: &'static str,
	pub ty: AttrType,
	pub required: bool,
	/// The model type whose table declares it; none for the hand-written ones.
	#[cfg_attr(not(test), allow(dead_code, reason = "read by the tests that check the tables' coverage"))]
	pub owner: Option<&'static str>,
}

impl AttrDecl {
	/// An attribute of no table.
	pub(crate) const fn plain(name: &'static str, ty: AttrType, required: bool) -> Self {
		Self { name, ty, required, owner: None }
	}
}

/// A model type with its attribute table and the base it is read onto and written against.
pub(crate) trait Table: Fields + Default {
	const ATTRS: &'static [Attr<Self>];

	fn base() -> Self {
		Self::default()
	}
}

/// A `T` read from `el` onto its base.
pub(crate) fn read<T: Table>(el: &Element) -> Result<T, FormatError> {
	read_onto(el, T::base())
}

/// A `T` read from `el` onto `base`: what the element leaves out stays as `base` has it.
pub(crate) fn read_onto<T: Table>(el: &Element, mut base: T) -> Result<T, FormatError> {
	T::ATTRS.iter().try_for_each(|a| a.read(el, &mut base))?;
	Ok(base)
}

/// Appends the attributes of `value` to `el`, a sparse one only when it differs from `base`.
pub(crate) fn write_against<T: Fields>(attrs: &[Attr<T>], value: &T, base: &T, el: &mut Element) {
	for a in attrs {
		let written = match a.presence {
			Presence::Sparse => !a.same(value, base),
			Presence::Always => true,
			Presence::When(when) => when(value),
		};
		if let Some(text) = written.then(|| a.text(value)).flatten() {
			el.attr(a.name(), text);
		}
	}
}

/// Appends the attributes of `value` to `el` against its base.
pub(crate) fn write<T: Table>(value: &T, el: &mut Element) {
	write_against(T::ATTRS, value, &T::base(), el);
}

/// The DTD declarations of a table's attributes.
pub(crate) fn attrs<T: Fields>(attrs: &[Attr<T>]) -> Vec<AttrDecl> {
	attrs.iter().map(Attr::decl).collect()
}
