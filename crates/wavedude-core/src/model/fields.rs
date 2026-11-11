//! Typed access to the model's fields: every struct and enum derives `Fields` or `Word`, which generates a
//! field enum and an edit enum per type, accessor consts and the hashing of `Feed`.
//!
//! A field is addressed by a `Field` value, an edit is a value of the type's `Edit` enum; nothing goes by string.

use std::fmt;
use std::hash::{Hash, Hasher};

use thiserror::Error;

pub use wavedude_model_derive::{Fields, Word};

pub use super::ranges::Range;

/// A field's value as a form or a file sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
	Text(String),
	/// Every integer type.
	Int(i64),
	Float(f64),
	Bool(bool),
	/// The name of an enum member.
	Word(&'static str),
	/// An absent optional.
	Absent,
	/// A list as its length, a whole composite as its debug text.
	Other(String),
}

impl fmt::Display for Value {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Value::Text(s) | Value::Other(s) => f.write_str(s),
			Value::Int(i) => write!(f, "{i}"),
			Value::Float(x) => write!(f, "{x}"),
			Value::Bool(b) => write!(f, "{b}"),
			Value::Word(w) => f.write_str(w),
			Value::Absent => f.write_str("absent"),
		}
	}
}

/// What a field holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
	Text,
	Int,
	UInt,
	Float,
	Bool,
	/// An enum, with its member names.
	Word(&'static [&'static str]),
	List,
	/// A composite replaced whole.
	Nested,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum EditError {
	#[error("{field} takes a {want:?}")]
	WrongKind { field: String, want: ValueKind },
	#[error("{0} is absent")]
	Absent(String),
}

/// A model type with typed fields; derived.
pub trait Fields: Sized + 'static {
	/// The type's name in tables and keys (`Probe`, `Part` for `PartBase`).
	const TYPE_NAME: &'static str;
	type Field: Field<Owner = Self>;
	type Edit: Edit<Owner = Self>;
	/// Visits this type and everything reachable from it; a type below several parents is visited each time.
	fn visit<V: Visitor>(v: &mut V);
}

/// Receives every `Fields` type and `Word` enum a walk reaches; a type below several parents comes each time,
/// so a visitor that wants each type once keeps its own set.
pub trait Visitor {
	fn fields<T: Fields>(&mut self);
	fn word<W: Word>(&mut self);
}

pub trait Field: Copy + Eq + Hash + fmt::Debug + 'static {
	type Owner: Fields;
	/// Every leaf, nested ones expanded.
	fn all() -> Vec<Self>;
	/// This level's member in PascalCase: `BoxWidth`, `Ax`, `R1`.
	fn name(self) -> &'static str;
	/// The names from the owner down: `Kind.Lens.Material.Mirror`.
	fn path(self) -> String;
	fn kind(self) -> ValueKind;
	/// The declared range of a number.
	fn range(self) -> Option<Range>;
	/// Whether another value also bounds the field, in code (`#[field(dependent)]`), a range or not.
	fn dependent(self) -> bool;
	/// None through an absent option or another variant.
	fn get(self, of: &Self::Owner) -> Option<Value>;
	/// The edit that sets this field to `value`.
	fn edit(self, value: Value) -> Result<<Self::Owner as Fields>::Edit, EditError>;
}

pub trait Edit: Clone + fmt::Debug + PartialEq + 'static {
	type Owner: Fields;
	fn field(&self) -> <Self::Owner as Fields>::Field;
	/// Sets the field and returns the value it held.
	fn apply(&self, to: &mut Self::Owner) -> Result<Value, EditError>;
	/// The edit that restores what `of` holds now.
	fn current(&self, of: &Self::Owner) -> Option<Self>;
}

/// An enum of unit variants; derived.
pub trait Word: Copy + Eq + 'static {
	const TYPE_NAME: &'static str;
	const MEMBERS: &'static [Self];
	/// The names of `MEMBERS`, in order.
	const NAMES: &'static [&'static str];
	/// The variant's ident.
	fn name(self) -> &'static str;
	fn parse(s: &str) -> Option<Self>;
}

/// Feeds a value to a hasher: floats by their bits, an enum by its variant index, an option by a tag, a list by
/// its length then its elements. Skipped fields are not fed (`Project`'s lists).
pub trait Feed {
	fn feed<H: Hasher>(&self, h: &mut H);
}

/// A number field's type.
pub trait Number: Copy {
	const INTEGER: bool;
	fn to_f64(self) -> f64;
	/// Rounds an integer, saturating.
	fn from_f64(v: f64) -> Self;
}

macro_rules! numbers {
	($($ty:ty => $integer:expr),*) => {$(
		impl Number for $ty {
			const INTEGER: bool = $integer;

			fn to_f64(self) -> f64 {
				self as f64
			}

			fn from_f64(v: f64) -> Self {
				if Self::INTEGER { v.round() as Self } else { v as Self }
			}
		}
	)*};
}

numbers!(u32 => true, i32 => true, f64 => false);

macro_rules! feed_by_hash {
	($($ty:ty),*) => {$(
		impl Feed for $ty {
			fn feed<H: Hasher>(&self, h: &mut H) {
				self.hash(h);
			}
		}
	)*};
}

feed_by_hash!(String, bool, u8, u32, i32);

impl Feed for f64 {
	fn feed<H: Hasher>(&self, h: &mut H) {
		self.to_bits().hash(h);
	}
}

impl<T: Feed> Feed for Option<T> {
	fn feed<H: Hasher>(&self, h: &mut H) {
		self.is_some().hash(h);
		if let Some(v) = self {
			v.feed(h);
		}
	}
}

impl<T: Feed> Feed for Vec<T> {
	fn feed<H: Hasher>(&self, h: &mut H) {
		self.len().hash(h);
		for v in self {
			v.feed(h);
		}
	}
}

/// A typed accessor of a leaf field: `Probe::BOX_WIDTH`.
pub struct Prop<T: Fields, V> {
	pub field: T::Field,
	pub get: fn(&T) -> V,
	pub edit: fn(V) -> T::Edit,
}

/// A typed accessor of a nested field: `Lens::MATERIAL`.
pub struct Nested<T: Fields, U: Fields> {
	pub get: fn(&T) -> &U,
	pub wrap: fn(U::Edit) -> T::Edit,
}

/// A typed accessor of an enum variant or an optional composite: `PartKind::LENS`, `Source::ARRAY`.
pub struct Variant<T: Fields, U: Fields> {
	pub get: fn(&T) -> Option<&U>,
	pub wrap: fn(U::Edit) -> T::Edit,
}

// By hand: derives would demand `T: Copy` and `V: Copy`, and the members are fn pointers and a `Copy` field.
impl<T: Fields, V> Clone for Prop<T, V> {
	fn clone(&self) -> Self {
		*self
	}
}

impl<T: Fields, V> Copy for Prop<T, V> {}

impl<T: Fields, U: Fields> Clone for Nested<T, U> {
	fn clone(&self) -> Self {
		*self
	}
}

impl<T: Fields, U: Fields> Copy for Nested<T, U> {}

impl<T: Fields, U: Fields> Clone for Variant<T, U> {
	fn clone(&self) -> Self {
		*self
	}
}

impl<T: Fields, U: Fields> Copy for Variant<T, U> {}
