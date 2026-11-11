//! `#[derive(Fields)]`.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Error, Expr, Field, Ident, Result, Type, Variant};

use crate::base;
use crate::naming::{pascal, screaming, type_name};
use crate::shape::{Leaf, Shape, option_inner};

enum Def {
	/// A struct field of a leaf type.
	Leaf { field: Ident, shape: Box<Shape>, range: Option<Expr>, dependent: bool },
	/// A struct field of a `Fields` type, or an `Option` of one; `whole` also offers replacing the option.
	Nested { field: Ident, ty: Type, optional: bool, whole: bool },
	/// A newtype variant of an enum.
	Case { ty: Type },
}

struct Member {
	/// Variant of the field and edit enums.
	variant: Ident,
	/// Variant of the replacement of a whole option.
	set: Ident,
	name: String,
	konst: Ident,
	def: Def,
}

pub(crate) fn expand(input: &DeriveInput) -> Result<TokenStream> {
	if !input.generics.params.is_empty() {
		return Err(Error::new_spanned(&input.generics, "Fields derives on a type without generics"));
	}
	let (members, is_enum) = match &input.data {
		Data::Struct(data) => {
			let mut members = Vec::new();
			for field in &data.fields {
				members.extend(struct_member(field)?);
			}
			(members, false)
		}
		Data::Enum(data) => {
			let members = data.variants.iter().map(enum_member).collect::<Result<Vec<_>>>()?;
			(members, true)
		}
		Data::Union(_) => return Err(Error::new_spanned(&input.ident, "Fields derives on a struct or an enum")),
	};
	if members.is_empty() {
		return Err(Error::new_spanned(&input.ident, "Fields needs at least one field"));
	}
	Ok(Gen { owner: &input.ident, type_name: type_name(input)?, members, is_enum }.all())
}

fn member(ident: &Ident, def: Def) -> Member {
	let text = ident.unraw().to_string();
	let name = pascal(&text);
	Member {
		variant: Ident::new(&name, ident.span()),
		set: Ident::new(&format!("Set{name}"), ident.span()),
		konst: Ident::new(&screaming(&text), ident.span()),
		name,
		def,
	}
}

fn struct_member(field: &Field) -> Result<Option<Member>> {
	let ident = field.ident.clone().ok_or_else(|| Error::new_spanned(field, "Fields derives on named fields"))?;
	let (mut skip, mut nested, mut whole, mut dependent) = (false, false, false, false);
	let mut range = None;
	for attr in field.attrs.iter().filter(|a| a.path().is_ident("field")) {
		attr.parse_nested_meta(|meta| {
			let path = &meta.path;
			if path.is_ident("skip") {
				skip = true;
			} else if path.is_ident("nested") {
				nested = true;
			} else if path.is_ident("whole") {
				whole = true;
			} else if path.is_ident("dependent") {
				dependent = true;
			} else if path.is_ident("range") {
				range = Some(meta.value()?.parse::<Expr>()?);
			} else {
				return Err(meta.error("expected skip, nested, whole, range or dependent"));
			}
			Ok(())
		})?;
	}
	if skip {
		return Ok(None);
	}
	let def = if nested {
		if range.is_some() || dependent {
			return Err(Error::new_spanned(field, "a nested field has no range of its own"));
		}
		let optional = option_inner(&field.ty);
		if whole && optional.is_none() {
			return Err(Error::new_spanned(field, "`whole` needs an `Option` of a nested type"));
		}
		Def::Nested {
			field: ident.clone(),
			ty: optional.unwrap_or(&field.ty).clone(),
			optional: optional.is_some(),
			whole,
		}
	} else {
		if whole {
			return Err(Error::new_spanned(field, "`whole` goes with `nested`"));
		}
		Def::Leaf { field: ident.clone(), shape: Box::new(Shape::of(&field.ty)?), range, dependent }
	};
	Ok(Some(member(&ident, def)))
}

fn enum_member(variant: &Variant) -> Result<Member> {
	let syn::Fields::Unnamed(fields) = &variant.fields else {
		return Err(Error::new_spanned(variant, "Fields derives on newtype variants"));
	};
	let mut types = fields.unnamed.iter();
	let (Some(field), None) = (types.next(), types.next()) else {
		return Err(Error::new_spanned(variant, "Fields derives on newtype variants"));
	};
	Ok(member(&variant.ident, Def::Case { ty: field.ty.clone() }))
}

/// The match arms of `Field`, one list per method.
#[derive(Default)]
struct Arms {
	name: Vec<TokenStream>,
	path: Vec<TokenStream>,
	kind: Vec<TokenStream>,
	bounds: Vec<TokenStream>,
	dependent: Vec<TokenStream>,
	get: Vec<TokenStream>,
	edit: Vec<TokenStream>,
	all: Vec<TokenStream>,
}

impl Arms {
	/// The arms of a member that holds another `Fields` type.
	fn inner(&mut self, m: &Member, ty: &Type, te: &Ident) {
		let p = base();
		let (v, n) = (&m.variant, &m.name);
		self.name.push(quote!(Self::#v(_) => #n));
		self.path.push(quote!(Self::#v(inner) => format!("{}.{}", #n, #p::Field::path(inner))));
		self.kind.push(quote!(Self::#v(inner) => #p::Field::kind(inner)));
		self.bounds.push(quote!(Self::#v(inner) => #p::Field::bounds(inner)));
		self.dependent.push(quote!(Self::#v(inner) => #p::Field::dependent(inner)));
		self.edit.push(quote!(Self::#v(inner) => #p::Field::edit(inner, value).map(#te::#v)));
		let inner_all = quote!(<<#ty as #p::Fields>::Field as #p::Field>::all());
		self.all.push(quote!(.chain(#inner_all.into_iter().map(Self::#v))));
	}
}

struct Gen<'a> {
	owner: &'a Ident,
	type_name: String,
	members: Vec<Member>,
	is_enum: bool,
}

impl Gen<'_> {
	fn all(&self) -> TokenStream {
		let owner = self.owner;
		let p = base();
		let type_name = &self.type_name;
		let tf = format_ident!("{owner}Field");
		let te = format_ident!("{owner}Edit");
		let field_enum = self.field_enum(&tf);
		let edit_enum = self.edit_enum(&te);
		let field_impl = self.field_impl(&tf, &te);
		let edit_impl = self.edit_impl(&tf, &te);
		let feed = self.feed();
		let consts = self.members.iter().map(|m| self.consts(m, &tf, &te));
		let words = self.members.iter().filter_map(|m| match &m.def {
			Def::Leaf { shape, .. } if shape.leaf == Leaf::Word => Some(&shape.inner),
			_ => None,
		});
		quote! {
			#field_enum
			#edit_enum

			impl #p::Fields for #owner {
				const TYPE_NAME: &'static str = #type_name;
				type Field = #tf;
				type Edit = #te;
			}

			#field_impl
			#edit_impl
			#feed

			impl #owner {
				#(#consts)*
			}

			// A leaf of an unknown type must be a `Word`; the error points at the field's type.
			#(const _: fn() = || {
				fn is_word<W: #p::Word>() {}
				is_word::<#words>();
			};)*
		}
	}

	fn field_enum(&self, tf: &Ident) -> TokenStream {
		let p = base();
		let doc = format!("A field of `{}`.", self.owner);
		let variants = self.members.iter().flat_map(|m| {
			let (v, set) = (&m.variant, &m.set);
			match &m.def {
				Def::Leaf { .. } => vec![quote!(#v)],
				Def::Nested { ty, whole, .. } => {
					let mut out = vec![quote!(#v(<#ty as #p::Fields>::Field))];
					if *whole {
						out.push(quote!(#set));
					}
					out
				}
				Def::Case { ty } => vec![quote!(#v(<#ty as #p::Fields>::Field))],
			}
		});
		quote! {
			#[doc = #doc]
			#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
			pub enum #tf { #(#variants,)* }
		}
	}

	fn edit_enum(&self, te: &Ident) -> TokenStream {
		let p = base();
		let doc = format!("An edit of one field of `{}`.", self.owner);
		let variants = self.members.iter().flat_map(|m| {
			let (v, set) = (&m.variant, &m.set);
			match &m.def {
				Def::Leaf { shape, .. } => {
					let ty = &shape.ty;
					vec![quote!(#v(#ty))]
				}
				Def::Nested { ty, whole, .. } => {
					let mut out = vec![quote!(#v(<#ty as #p::Fields>::Edit))];
					if *whole {
						out.push(quote!(#set(Option<#ty>)));
					}
					out
				}
				Def::Case { ty } => vec![quote!(#v(<#ty as #p::Fields>::Edit))],
			}
		});
		quote! {
			#[doc = #doc]
			#[derive(Clone, Debug, PartialEq)]
			pub enum #te { #(#variants,)* }
		}
	}

	/// The arm taken for any other variant of an enum owner; empty when no other variant exists.
	fn otherwise(&self, result: &TokenStream) -> TokenStream {
		if self.members.len() > 1 { quote!(_ => #result,) } else { quote!() }
	}

	fn field_impl(&self, tf: &Ident, te: &Ident) -> TokenStream {
		let p = base();
		let owner = self.owner;
		let none = quote!(None);
		let mut a = Arms::default();
		let mut wrong_used = false;
		for m in &self.members {
			let (v, set, n) = (&m.variant, &m.set, &m.name);
			match &m.def {
				Def::Leaf { field: f, shape, range, dependent } => {
					a.name.push(quote!(Self::#v => #n));
					a.path.push(quote!(Self::#v => String::from(#n)));
					let k = shape.kind();
					a.kind.push(quote!(Self::#v => #k));
					a.bounds.push(match (range, dependent) {
						(Some(r), _) => quote!(Self::#v => #p::Bounds::Range(#r)),
						(None, true) => quote!(Self::#v => #p::Bounds::Dependent),
						(None, false) => quote!(Self::#v => #p::Bounds::Open),
					});
					a.dependent.push(quote!(Self::#v => #dependent));
					let value = shape.value(&quote!(of.#f));
					a.get.push(quote!(Self::#v => Some(#value)));
					wrong_used = true;
					a.edit.push(if shape.leaf == Leaf::List {
						quote!(Self::#v => { let _ = value; Err(wrong()) })
					} else {
						let parsed = shape.parse(&quote!(value));
						quote!(Self::#v => #parsed.map(#te::#v).ok_or_else(wrong))
					});
					a.all.push(quote!(.chain([Self::#v])));
				}
				Def::Nested { field: f, ty, optional, whole } => {
					a.inner(m, ty, te);
					a.get.push(if *optional {
						quote!(Self::#v(inner) => of.#f.as_ref().and_then(|v| #p::Field::get(inner, v)))
					} else {
						quote!(Self::#v(inner) => #p::Field::get(inner, &of.#f))
					});
					if *whole {
						wrong_used = true;
						a.name.push(quote!(Self::#set => #n));
						a.path.push(quote!(Self::#set => String::from(#n)));
						a.kind.push(quote!(Self::#set => #p::ValueKind::Nested));
						a.bounds.push(quote!(Self::#set => #p::Bounds::Open));
						a.dependent.push(quote!(Self::#set => false));
						a.get.push(quote! {
							Self::#set => Some(match &of.#f {
								Some(v) => #p::Value::Other(format!("{v:?}")),
								None => #p::Value::Absent,
							})
						});
						a.edit.push(quote! {
							Self::#set => match value {
								#p::Value::Absent => Ok(#te::#set(None)),
								_ => Err(wrong()),
							}
						});
					}
				}
				Def::Case { ty } => {
					a.inner(m, ty, te);
					let other = self.otherwise(&none);
					a.get.push(
						quote!(Self::#v(inner) => match of { #owner::#v(v) => #p::Field::get(inner, v), #other }),
					);
				}
			}
		}
		let Arms { name, path, kind, bounds, dependent: dependent_arms, get, edit, all } = a;
		let wrong = if wrong_used {
			quote!(let wrong = || #p::EditError::WrongKind { field: #p::Field::path(self), want: #p::Field::kind(self) };)
		} else {
			quote!()
		};
		quote! {
			impl #p::Field for #tf {
				type Owner = #owner;

				fn all() -> Vec<Self> {
					::core::iter::empty() #(#all)* .collect()
				}

				fn name(self) -> &'static str {
					match self { #(#name,)* }
				}

				fn path(self) -> String {
					match self { #(#path,)* }
				}

				fn kind(self) -> #p::ValueKind {
					match self { #(#kind,)* }
				}

				fn bounds(self) -> #p::Bounds {
					match self { #(#bounds,)* }
				}

				fn dependent(self) -> bool {
					match self { #(#dependent_arms,)* }
				}

				fn get(self, of: &#owner) -> Option<#p::Value> {
					match self { #(#get,)* }
				}

				fn edit(self, value: #p::Value) -> Result<#te, #p::EditError> {
					#wrong
					match self { #(#edit,)* }
				}
			}
		}
	}

	fn edit_impl(&self, tf: &Ident, te: &Ident) -> TokenStream {
		let p = base();
		let owner = self.owner;
		let mut field = Vec::new();
		let mut apply = Vec::new();
		let mut current = Vec::new();
		let none = quote!(None);
		let wrong_variant = quote! {
			Err(#p::EditError::WrongKind {
				field: #p::Field::path(#p::Edit::field(self)),
				want: #p::ValueKind::Nested,
			})
		};
		for m in &self.members {
			let (v, set) = (&m.variant, &m.set);
			let inner_field = quote!(Self::#v(e) => #tf::#v(#p::Edit::field(e)));
			match &m.def {
				Def::Leaf { field: f, shape, .. } => {
					field.push(quote!(Self::#v(_) => #tf::#v));
					let old = shape.value(&quote!(to.#f));
					let new = if shape.is_copy() { quote!(*v) } else { quote!(v.clone()) };
					apply.push(quote!(Self::#v(v) => { let old = #old; to.#f = #new; Ok(old) }));
					let now = if shape.is_copy() { quote!(of.#f) } else { quote!(of.#f.clone()) };
					current.push(quote!(Self::#v(_) => Some(#te::#v(#now))));
				}
				Def::Nested { field: f, optional, whole, .. } => {
					field.push(inner_field);
					if *optional {
						apply.push(quote! {
							Self::#v(e) => match to.#f.as_mut() {
								Some(v) => #p::Edit::apply(e, v),
								None => Err(#p::EditError::Absent(#p::Field::path(#p::Edit::field(self)))),
							}
						});
						current.push(quote! {
							Self::#v(e) => of.#f.as_ref().and_then(|v| #p::Edit::current(e, v)).map(#te::#v)
						});
					} else {
						apply.push(quote!(Self::#v(e) => #p::Edit::apply(e, &mut to.#f)));
						current.push(quote!(Self::#v(e) => #p::Edit::current(e, &of.#f).map(#te::#v)));
					}
					if *whole {
						field.push(quote!(Self::#set(_) => #tf::#set));
						apply.push(quote! {
							Self::#set(v) => {
								let old = match &to.#f {
									Some(o) => #p::Value::Other(format!("{o:?}")),
									None => #p::Value::Absent,
								};
								to.#f = v.clone();
								Ok(old)
							}
						});
						current.push(quote!(Self::#set(_) => Some(#te::#set(of.#f.clone()))));
					}
				}
				Def::Case { .. } => {
					field.push(inner_field);
					let apply_other = self.otherwise(&wrong_variant);
					apply.push(quote! {
						Self::#v(e) => match to { #owner::#v(v) => #p::Edit::apply(e, v), #apply_other }
					});
					let current_other = self.otherwise(&none);
					current.push(quote! {
						Self::#v(e) => match of {
							#owner::#v(v) => #p::Edit::current(e, v).map(#te::#v),
							#current_other
						}
					});
				}
			}
		}
		quote! {
			impl #p::Edit for #te {
				type Owner = #owner;

				fn field(&self) -> #tf {
					match self { #(#field,)* }
				}

				fn apply(&self, to: &mut #owner) -> Result<#p::Value, #p::EditError> {
					match self { #(#apply,)* }
				}

				fn current(&self, of: &#owner) -> Option<Self> {
					match self { #(#current,)* }
				}
			}
		}
	}

	fn feed(&self) -> TokenStream {
		let p = base();
		let owner = self.owner;
		let feeds = self.members.iter().enumerate().map(|(i, m)| match &m.def {
			Def::Leaf { field, .. } | Def::Nested { field, .. } => quote!(#p::Feed::feed(&self.#field, h);),
			Def::Case { .. } => {
				let v = &m.variant;
				let index = Literal::u32_suffixed(i as u32);
				quote!(Self::#v(v) => { #p::Feed::feed(&#index, h); #p::Feed::feed(v, h); })
			}
		});
		let body = if self.is_enum { quote!(match self { #(#feeds)* }) } else { quote!(#(#feeds)*) };
		quote! {
			impl #p::Feed for #owner {
				fn feed<H: ::core::hash::Hasher>(&self, h: &mut H) {
					#body
				}
			}
		}
	}

	fn consts(&self, m: &Member, tf: &Ident, te: &Ident) -> TokenStream {
		let p = base();
		let owner = self.owner;
		let (v, set, k) = (&m.variant, &m.set, &m.konst);
		match &m.def {
			Def::Leaf { field: f, shape, .. } => {
				let ty = &shape.ty;
				let get = if shape.is_copy() { quote!(t.#f) } else { quote!(t.#f.clone()) };
				quote! {
					pub const #k: #p::Prop<#owner, #ty> = #p::Prop { field: #tf::#v, get: |t| #get, edit: #te::#v };
				}
			}
			Def::Nested { field: f, ty, optional, whole } => {
				let (kind, get) =
					if *optional { (quote!(Variant), quote!(t.#f.as_ref())) } else { (quote!(Nested), quote!(&t.#f)) };
				let konst = quote! {
					pub const #k: #p::#kind<#owner, #ty> = #p::#kind { get: |t| #get, wrap: #te::#v, field: #tf::#v };
				};
				if *whole {
					let set_k = format_ident!("SET_{k}");
					quote! {
						#konst
						pub const #set_k: #p::Prop<#owner, Option<#ty>> =
							#p::Prop { field: #tf::#set, get: |t| t.#f.clone(), edit: #te::#set };
					}
				} else {
					konst
				}
			}
			Def::Case { ty } => {
				let other = self.otherwise(&quote!(None));
				quote! {
					pub const #k: #p::Variant<#owner, #ty> = #p::Variant {
						get: |t| match t { #owner::#v(v) => Some(v), #other },
						wrap: #te::#v,
						field: #tf::#v,
					};
				}
			}
		}
	}
}
