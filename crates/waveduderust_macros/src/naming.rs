//! The name a type goes by in tables and keys.

use syn::{DeriveInput, LitStr, Result};

/// `TYPE_NAME` of a type: the `#[fields(name = "...")]` attribute, else the ident.
pub(crate) fn type_name(input: &DeriveInput) -> Result<String> {
	let mut name = input.ident.to_string();
	for attr in input.attrs.iter().filter(|a| a.path().is_ident("fields")) {
		attr.parse_nested_meta(|meta| {
			if meta.path.is_ident("name") {
				name = meta.value()?.parse::<LitStr>()?.value();
				Ok(())
			} else {
				Err(meta.error("expected `name = \"...\"`"))
			}
		})?;
	}
	Ok(name)
}
