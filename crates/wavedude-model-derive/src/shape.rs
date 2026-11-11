//! What a leaf field's type says: its kind, how it becomes a `Value` and back.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, GenericArgument, PathArguments, Result, Type};

use crate::base;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leaf {
	UInt,
	Int,
	Float,
	Bool,
	Text,
	Word,
	List,
}

/// A leaf type: `u32`, `Option<f64>`, `Vec<Point>`, a `Word` enum.
pub(crate) struct Shape {
	/// As declared.
	pub ty: Type,
	pub leaf: Leaf,
	/// Without the `Option`.
	pub inner: Type,
	pub optional: bool,
}

/// The `T` of `name<T>`.
fn generic_arg<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
	let Type::Path(path) = ty else { return None };
	let segment = path.path.segments.last()?;
	if segment.ident != name {
		return None;
	}
	let PathArguments::AngleBracketed(args) = &segment.arguments else { return None };
	match args.args.first()? {
		GenericArgument::Type(inner) => Some(inner),
		_ => None,
	}
}

/// The `T` of an `Option<T>`.
pub(crate) fn option_inner(ty: &Type) -> Option<&Type> {
	generic_arg(ty, "Option")
}

/// The element type of a list of `Fields` types; none for another type or a list of plain values.
pub(crate) fn list_element(ty: &Type) -> Option<&Type> {
	let element = generic_arg(ty, "Vec")?;
	let Type::Path(path) = element else { return None };
	let plain = ["u8", "u32", "i32", "f64", "bool", "String"];
	(!plain.iter().any(|p| path.path.is_ident(p))).then_some(element)
}

impl Shape {
	pub(crate) fn of(ty: &Type) -> Result<Self> {
		let (inner, optional) = option_inner(ty).map_or((ty, false), |inner| (inner, true));
		let unsupported =
			|| Error::new_spanned(ty, "not a leaf type; mark the field `#[field(skip)]` or `#[field(nested)]`");
		let Type::Path(path) = inner else { return Err(unsupported()) };
		let segment = path.path.segments.last().ok_or_else(unsupported)?;
		let leaf = match segment.ident.to_string().as_str() {
			"u8" | "u32" => Leaf::UInt,
			"i32" => Leaf::Int,
			"u16" | "u64" | "u128" | "usize" | "i8" | "i16" | "i64" | "i128" | "isize" | "f32" => {
				return Err(Error::new_spanned(
					ty,
					"not a supported number type (u8, u32, i32, f64); mark the field `#[field(skip)]`",
				));
			}
			"f64" => Leaf::Float,
			"bool" => Leaf::Bool,
			"String" => Leaf::Text,
			"Vec" => Leaf::List,
			_ if matches!(segment.arguments, PathArguments::None) => Leaf::Word,
			_ => return Err(unsupported()),
		};
		if optional && option_inner(inner).is_some() {
			return Err(unsupported());
		}
		Ok(Self { ty: ty.clone(), leaf, inner: inner.clone(), optional })
	}

	/// Whether the leaf is a number.
	pub(crate) fn is_number(&self) -> bool {
		matches!(self.leaf, Leaf::UInt | Leaf::Int | Leaf::Float)
	}

	/// Whether the declared type is `Copy`.
	pub(crate) fn is_copy(&self) -> bool {
		!matches!(self.leaf, Leaf::Text | Leaf::List)
	}

	pub(crate) fn kind(&self) -> TokenStream {
		let p = base();
		let inner = &self.inner;
		match self.leaf {
			Leaf::UInt => quote!(#p::ValueKind::UInt),
			Leaf::Int => quote!(#p::ValueKind::Int),
			Leaf::Float => quote!(#p::ValueKind::Float),
			Leaf::Bool => quote!(#p::ValueKind::Bool),
			Leaf::Text => quote!(#p::ValueKind::Text),
			Leaf::List => quote!(#p::ValueKind::List),
			Leaf::Word => quote!(#p::ValueKind::Word(<#inner as #p::Word>::NAMES)),
		}
	}

	/// An expression of type `Value` from `place`, a place of the declared type.
	pub(crate) fn value(&self, place: &TokenStream) -> TokenStream {
		let p = base();
		if self.optional {
			let some = self.leaf_value(&quote!(v), true);
			quote!(match &#place { Some(v) => #some, None => #p::Value::Absent })
		} else {
			self.leaf_value(place, false)
		}
	}

	/// `ex` is a place of the leaf type, or a reference to one.
	fn leaf_value(&self, ex: &TokenStream, by_ref: bool) -> TokenStream {
		let p = base();
		let copy = if by_ref { quote!(*#ex) } else { quote!(#ex) };
		match self.leaf {
			Leaf::UInt | Leaf::Int => quote!(#p::Value::Int(#copy as i64)),
			Leaf::Float => quote!(#p::Value::Float(#copy)),
			Leaf::Bool => quote!(#p::Value::Bool(#copy)),
			Leaf::Text => quote!(#p::Value::Text(#ex.clone())),
			Leaf::Word => quote!(#p::Value::Word(#p::Word::name(#copy))),
			Leaf::List => quote!(#p::Value::Other(format!("[{} items]", #ex.len()))),
		}
	}

	/// An expression of type `Option<declared type>` from `value`, a `Value`.
	pub(crate) fn parse(&self, value: &TokenStream) -> TokenStream {
		let p = base();
		let leaf = self.parse_leaf(value);
		if self.optional {
			let some = self.parse_leaf(&quote!(v));
			quote!(match #value { #p::Value::Absent => Some(None), v => #some.map(Some) })
		} else {
			leaf
		}
	}

	fn parse_leaf(&self, value: &TokenStream) -> TokenStream {
		let p = base();
		let inner = &self.inner;
		match self.leaf {
			Leaf::UInt | Leaf::Int => {
				quote!(match #value { #p::Value::Int(i) => <#inner as TryFrom<i64>>::try_from(i).ok(), _ => None })
			}
			Leaf::Float => quote!(match #value { #p::Value::Float(f) => Some(f), _ => None }),
			Leaf::Bool => quote!(match #value { #p::Value::Bool(b) => Some(b), _ => None }),
			Leaf::Text => quote!(match #value { #p::Value::Text(s) => Some(s), _ => None }),
			Leaf::Word => quote!(match #value { #p::Value::Word(s) => <#inner as #p::Word>::parse(s), _ => None }),
			Leaf::List => quote!({ let _ = #value; None }),
		}
	}
}
