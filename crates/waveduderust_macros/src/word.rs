//! `#[derive(Word)]`.

use proc_macro2::{Literal, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields as Shape, Result};

use crate::base;
use crate::naming::type_name;

pub(crate) fn expand(input: &DeriveInput) -> Result<TokenStream> {
	let Data::Enum(data) = &input.data else {
		return Err(Error::new_spanned(&input.ident, "Word derives on an enum"));
	};
	if !input.generics.params.is_empty() {
		return Err(Error::new_spanned(&input.generics, "Word derives on a type without generics"));
	}
	let ty = &input.ident;
	let type_name = type_name(input)?;
	let mut idents = Vec::new();
	for variant in &data.variants {
		if !matches!(variant.fields, Shape::Unit) {
			return Err(Error::new_spanned(variant, "Word derives on unit variants"));
		}
		idents.push(&variant.ident);
	}
	let names: Vec<String> = idents.iter().map(ToString::to_string).collect();
	let indices = (0..idents.len()).map(|i| Literal::u32_suffixed(i as u32));
	let p = base();
	Ok(quote! {
		impl #p::Word for #ty {
			const TYPE_NAME: &'static str = #type_name;
			const NAMES: &'static [&'static str] = &[#(#names),*];
			const MEMBERS: &'static [Self] = &[#(Self::#idents),*];

			fn name(self) -> &'static str {
				match self {
					#(Self::#idents => #names,)*
				}
			}

			fn parse(s: &str) -> Option<Self> {
				match s {
					#(#names => Some(Self::#idents),)*
					_ => None,
				}
			}
		}

		impl #p::Feed for #ty {
			fn feed<H: ::core::hash::Hasher>(&self, h: &mut H) {
				let index: u32 = match self {
					#(Self::#idents => #indices,)*
				};
				#p::Feed::feed(&index, h);
			}
		}
	})
}
