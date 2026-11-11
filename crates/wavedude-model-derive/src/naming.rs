//! Spellings of a member: `in_x` and `InX` are `InX` as a name and `IN_X` as a const.

use syn::{DeriveInput, LitStr, Result};

/// PascalCase of a snake_case ident: pieces split on `_`, the first letter of each capitalised.
pub(crate) fn pascal(ident: &str) -> String {
	ident
		.split('_')
		.map(|piece| {
			let mut chars = piece.chars();
			chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
		})
		.collect()
}

/// SCREAMING_SNAKE of a snake_case or PascalCase ident.
pub(crate) fn screaming(ident: &str) -> String {
	let mut out = String::new();
	let mut prev: Option<char> = None;
	for c in ident.chars() {
		if c.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
			out.push('_');
		}
		out.push(c.to_ascii_uppercase());
		prev = Some(c);
	}
	out
}

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
