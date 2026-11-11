//! Derives of the model's typed fields: `Fields` for structs and newtype enums, `Word` for unit enums.
//!
//! The generated code names the traits as `::wavedude_core::model::fields::*`; `wavedude-core` makes that path
//! resolve inside itself with `extern crate self as wavedude_core;`.

mod fields;
mod naming;
mod shape;
mod word;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

/// The module of the traits and shared types the generated code refers to.
fn base() -> TokenStream2 {
	quote!(::wavedude_core::model::fields)
}

/// Derives the `Field` and `Edit` enums, the `Fields`, `Field`, `Edit` and `Feed` impls and one accessor const
/// per field.
///
/// Type attribute: `#[fields(name = "...")]`. Field attributes: `#[field(skip)]`, `#[field(nested)]`,
/// `#[field(nested, whole)]`, `#[field(range = <expr>)]`, `#[field(dependent)]`.
#[proc_macro_derive(Fields, attributes(fields, field))]
pub fn derive_fields(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	fields::expand(&input).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Derives `Word` and `Feed` for an enum of unit variants.
#[proc_macro_derive(Word, attributes(fields))]
pub fn derive_word(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	word::expand(&input).unwrap_or_else(syn::Error::into_compile_error).into()
}
