//! Declarative attribute tables: one list of fields per model type drives the `.wdproj` reader, the
//! writer, the DTD and the range keys (`"<owner>.<attribute>"`, see `model::ranges`).
//!
//! Reading starts from a base value and sets each attribute present; writing compares with the same base,
//! so a type's defaults are stated once, in its base.

use super::FormatError;
use super::xml::{Element, XmlEnum, fmt_f64, members};

/// How a field's value is read and written.
pub(crate) enum Codec<T> {
	Text {
		get: fn(&T) -> &str,
		set: fn(&mut T, &str),
	},
	/// Read as an `i32`.
	Int {
		get: fn(&T) -> i64,
		set: fn(&mut T, i32),
	},
	F64 {
		get: fn(&T) -> f64,
		set: fn(&mut T, f64),
	},
	Bool {
		get: fn(&T) -> bool,
		set: fn(&mut T, bool),
	},
	/// `set` is false for a name that is no member.
	Enum {
		get: fn(&T) -> &'static str,
		set: fn(&mut T, &str) -> bool,
		members: fn() -> Vec<&'static str>,
	},
}

/// When a field is written.
pub(crate) enum Presence<T> {
	/// Unless equal to the base's value.
	Sparse,
	/// Always; `#REQUIRED` in the DTD unless text.
	Always,
	/// When the predicate holds; `#IMPLIED` in the DTD.
	When(fn(&T) -> bool),
}

/// One attribute of an element.
pub(crate) struct Field<T> {
	/// The model type a range key names, e.g. `Material` in `Material.RefractiveIndex`.
	pub owner: &'static str,
	pub name: &'static str,
	pub codec: Codec<T>,
	pub presence: Presence<T>,
}

/// A model type with its attribute table and the base it is read onto and written against.
pub(crate) trait Table: Sized + 'static {
	const FIELDS: &'static [Field<Self>];

	fn base() -> Self;
}

/// The type of an attribute in the DTD.
pub(crate) enum AttrType {
	Cdata,
	Bool,
	Enum(Vec<&'static str>),
}

/// An attribute declaration; `owner` is None for text and other attributes without a range key.
pub(crate) struct Attr {
	#[cfg_attr(not(test), expect(dead_code, reason = "the range test reads it"))]
	pub owner: Option<&'static str>,
	pub name: &'static str,
	pub ty: AttrType,
	pub required: bool,
}

impl<T> Field<T> {
	fn read(&self, el: &Element, into: &mut T) -> Result<(), FormatError> {
		let Some(text) = el.get(self.name) else {
			return Ok(());
		};
		match &self.codec {
			Codec::Text { set, .. } => set(into, text),
			Codec::Int { set, .. } => set(into, el.parse_i32(self.name, text)?),
			Codec::F64 { set, .. } => set(into, el.parse_f64(self.name, text)?),
			Codec::Bool { set, .. } => set(into, el.parse_bool(self.name, text)?),
			Codec::Enum { set, members, .. } => {
				if !set(into, text) {
					return Err(el.bad(self.name, text, &format!("one of {}", members().join(", "))));
				}
			}
		}
		Ok(())
	}

	fn same(&self, a: &T, b: &T) -> bool {
		match &self.codec {
			Codec::Text { get, .. } => get(a) == get(b),
			Codec::Int { get, .. } => get(a) == get(b),
			Codec::F64 { get, .. } => get(a).to_bits() == get(b).to_bits(),
			Codec::Bool { get, .. } => get(a) == get(b),
			Codec::Enum { get, .. } => get(a) == get(b),
		}
	}

	fn text(&self, value: &T) -> String {
		match &self.codec {
			Codec::Text { get, .. } => get(value).to_owned(),
			Codec::Int { get, .. } => get(value).to_string(),
			Codec::F64 { get, .. } => fmt_f64(get(value)),
			Codec::Bool { get, .. } => if get(value) { "true" } else { "false" }.to_owned(),
			Codec::Enum { get, .. } => get(value).to_owned(),
		}
	}

	fn attr(&self) -> Attr {
		let ty = match &self.codec {
			Codec::Bool { .. } => AttrType::Bool,
			Codec::Enum { members, .. } => AttrType::Enum(members()),
			Codec::Text { .. } | Codec::Int { .. } | Codec::F64 { .. } => AttrType::Cdata,
		};
		let text = matches!(self.codec, Codec::Text { .. });
		let required = matches!(self.presence, Presence::Always) && !text;
		Attr { owner: (!text).then_some(self.owner), name: self.name, ty, required }
	}
}

/// Sets the fields of `into` whose attributes `el` has.
pub(crate) fn read_into<T>(fields: &[Field<T>], el: &Element, into: &mut T) -> Result<(), FormatError> {
	fields.iter().try_for_each(|f| f.read(el, into))
}

/// A `T` read from `el` onto its base.
pub(crate) fn read<T: Table>(el: &Element) -> Result<T, FormatError> {
	read_onto(el, T::base())
}

/// A `T` read from `el` onto `base`: what the element leaves out stays as `base` has it.
pub(crate) fn read_onto<T: Table>(el: &Element, mut base: T) -> Result<T, FormatError> {
	read_into(T::FIELDS, el, &mut base)?;
	Ok(base)
}

/// Appends the attributes of `value` to `el`, a sparse one only when it differs from `base`.
pub(crate) fn write_against<T>(fields: &[Field<T>], value: &T, base: &T, el: &mut Element) {
	for f in fields {
		let written = match f.presence {
			Presence::Sparse => !f.same(value, base),
			Presence::Always => true,
			Presence::When(when) => when(value),
		};
		if written {
			el.attr(f.name, f.text(value));
		}
	}
}

/// Appends the attributes of `value` to `el` against its base.
pub(crate) fn write<T: Table>(value: &T, el: &mut Element) {
	write_against(T::FIELDS, value, &T::base(), el);
}

/// The DTD declarations of a table's attributes.
pub(crate) fn attrs<T>(fields: &[Field<T>]) -> Vec<Attr> {
	fields.iter().map(Field::attr).collect()
}

/// The member names of the enum a getter returns, for the DTD.
pub(crate) fn members_of<T, E: XmlEnum>(_: fn(&T) -> E) -> Vec<&'static str> {
	members::<E>()
}

/// Builds a table: `fields!(Type, "Owner"; kind "Attribute" path [=> presence], [field], ...)` with the kinds
/// `text`, `u32`, `i32`, `f64`, `bool` and `enumv`; `path` is the field (dotted for a nested one); a bracketed
/// item is a [`Field`] written out, for a value with its own conversion.
macro_rules! fields {
	($t:ty, $owner:expr; $($items:tt)*) => {
		$crate::io::fields::fields!(@munch $t, $owner; []; $($items)*)
	};
	(@munch $t:ty, $owner:expr; [$($done:expr,)*]; $(,)?) => {
		&[$($done),*]
	};
	(@munch $t:ty, $owner:expr; [$($done:expr,)*]; [$field:expr] $(, $($rest:tt)*)?) => {
		$crate::io::fields::fields!(@munch $t, $owner; [$($done,)* $field,]; $($($rest)*)?)
	};
	(@munch $t:ty, $owner:expr; [$($done:expr,)*];
		$kind:ident $name:literal $($f:ident).+ $(=> $presence:expr)? $(, $($rest:tt)*)?) => {
		$crate::io::fields::fields!(
			@munch $t, $owner;
			[$($done,)* $crate::io::fields::field!($t, $owner, $kind, $name, $($f).+ $(=> $presence)?),];
			$($($rest)*)?
		)
	};
}

macro_rules! field {
	($t:ty, $owner:expr, $kind:ident, $name:literal, $($f:ident).+) => {
		$crate::io::fields::field!($t, $owner, $kind, $name, $($f).+ => $crate::io::fields::Presence::Sparse)
	};
	($t:ty, $owner:expr, $kind:ident, $name:literal, $($f:ident).+ => $presence:expr) => {
		$crate::io::fields::Field {
			owner: $owner,
			name: $name,
			codec: $crate::io::fields::codec!($t, $kind, $($f).+),
			presence: $presence,
		}
	};
}

macro_rules! codec {
	($t:ty, text, $($f:ident).+) => {
		$crate::io::fields::Codec::Text {
			get: |v: &$t| v.$($f).+.as_str(),
			set: |v: &mut $t, x: &str| v.$($f).+ = x.to_owned(),
		}
	};
	($t:ty, u32, $($f:ident).+) => {
		// a negative value becomes 0 for `normalize` to raise
		$crate::io::fields::Codec::Int { get: |v: &$t| v.$($f).+ as i64, set: |v: &mut $t, x: i32| v.$($f).+ = x.max(0) as u32 }
	};
	($t:ty, i32, $($f:ident).+) => {
		$crate::io::fields::Codec::Int { get: |v: &$t| v.$($f).+ as i64, set: |v: &mut $t, x: i32| v.$($f).+ = x }
	};
	($t:ty, f64, $($f:ident).+) => {
		$crate::io::fields::Codec::F64 { get: |v: &$t| v.$($f).+, set: |v: &mut $t, x: f64| v.$($f).+ = x }
	};
	($t:ty, bool, $($f:ident).+) => {
		$crate::io::fields::Codec::Bool { get: |v: &$t| v.$($f).+, set: |v: &mut $t, x: bool| v.$($f).+ = x }
	};
	($t:ty, enumv, $($f:ident).+) => {
		$crate::io::fields::Codec::Enum {
			get: |v: &$t| $crate::io::xml::XmlEnum::xml_name(v.$($f).+),
			set: |v: &mut $t, x: &str| {
				$crate::io::xml::XmlEnum::from_xml(x).map(|e| v.$($f).+ = e).is_some()
			},
			members: || $crate::io::fields::members_of(|v: &$t| v.$($f).+),
		}
	};
}

pub(crate) use {codec, field, fields};
