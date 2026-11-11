//! A small element tree over quick-xml: what the `.wdproj` and `.wdscheme` adapters read and write.
//!
//! Reading keeps attributes in document order, element text, and the line and column of each
//! element's start; the DOCTYPE's text is kept but never interpreted. Writing follows .NET's
//! `XmlWriter` with tab indentation: one line per element, ` />` for empty ones.

use std::borrow::Cow;
use std::fmt;

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use super::FormatError;
use crate::model::Word;

/// One element: attributes in order, child elements, concatenated text.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Element {
	pub name: String,
	pub attrs: Vec<(String, String)>,
	pub children: Vec<Element>,
	pub text: String,
	/// 1-based line and column of the start tag.
	pub at: (usize, usize),
}

/// A parsed document: the root element and the DOCTYPE's text when present.
pub(crate) struct Document {
	pub root: Element,
	pub doctype: Option<String>,
}

/// 1-based line and column of byte offset `pos` in `text`.
pub(crate) fn line_col(text: &str, pos: usize) -> (usize, usize) {
	let head = &text.as_bytes()[..pos.min(text.len())];
	let line = head.iter().filter(|&&b| b == b'\n').count() + 1;
	let start = head.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
	let column = String::from_utf8_lossy(&head[start..]).chars().count() + 1;
	(line, column)
}

fn error_at(text: &str, pos: usize, message: impl fmt::Display) -> FormatError {
	FormatError { message: message.to_string(), at: Some(line_col(text, pos)) }
}

fn start_element(text: &str, pos: usize, e: &BytesStart<'_>, version: XmlVersion) -> Result<Element, FormatError> {
	let mut el = Element { name: e.name().as_ref().to_owned(), at: line_col(text, pos), ..Element::default() };
	for attr in e.attributes() {
		let attr = attr.map_err(|err| {
			let key = match err {
				quick_xml::events::attributes::AttrError::Duplicated(at, _) => {
					let raw: &str = e;
					let k: String = raw[at.min(raw.len())..]
						.chars()
						.take_while(|c| !c.is_ascii_whitespace() && *c != '=')
						.collect();
					format!("attribute '{k}' is repeated")
				}
				other => other.to_string(),
			};
			error_at(text, pos, format!("element '{}': {key}", el.name))
		})?;
		let key = attr.key.as_ref().to_owned();
		let value = attr.normalized_value(version).map_err(|err| error_at(text, pos, err))?.into_owned();
		el.attrs.push((key, value));
	}
	Ok(el)
}

/// Parses `text` into its root element; comments, processing instructions and the DOCTYPE are skipped.
pub(crate) fn parse(text: &str) -> Result<Document, FormatError> {
	let mut reader = Reader::from_str(text);
	reader.config_mut().trim_text(false);
	let mut version = XmlVersion::Implicit1_0;
	let mut stack: Vec<Element> = Vec::new();
	let mut doctype = None;
	let mut root = None;
	loop {
		let pos = reader.buffer_position() as usize;
		let event = reader.read_event().map_err(|err| error_at(text, reader.error_position() as usize, err))?;
		match event {
			Event::Start(_) | Event::Empty(_) if root.is_some() => {
				return Err(error_at(text, pos, "the document has more than one root element"));
			}
			Event::Start(e) => stack.push(start_element(text, pos, &e, version)?),
			Event::Empty(e) => {
				let el = start_element(text, pos, &e, version)?;
				match stack.last_mut() {
					Some(parent) => parent.children.push(el),
					None => root = Some(el),
				}
			}
			Event::End(_) => {
				let el = stack.pop().expect("quick-xml checks end names");
				match stack.last_mut() {
					Some(parent) => parent.children.push(el),
					None => root = Some(el),
				}
			}
			Event::Text(t) => {
				if let Some(el) = stack.last_mut() {
					el.text.push_str(&t.xml_content(version));
				}
			}
			Event::GeneralRef(r) => {
				if let Some(el) = stack.last_mut() {
					resolve_ref(&r, &mut el.text).map_err(|err| error_at(text, pos, err))?;
				}
			}
			Event::Decl(d) => version = d.xml_version().map_err(|err| error_at(text, pos, err))?,
			Event::CData(t) => {
				if let Some(el) = stack.last_mut() {
					el.text.push_str(&t);
				}
			}
			Event::DocType(t) => doctype = Some(t.to_string()),
			Event::Eof => break,
			_ => {}
		}
	}
	if let Some(open) = stack.last() {
		return Err(FormatError {
			message: format!("unexpected end of file inside element '{}'", open.name),
			at: Some(line_col(text, text.len())),
		});
	}
	let root = root.ok_or_else(|| FormatError { message: "the document has no root element".into(), at: None })?;
	Ok(Document { root, doctype })
}

/// Appends the text a character or predefined entity reference stands for.
fn resolve_ref(r: &BytesRef<'_>, out: &mut String) -> Result<(), String> {
	if let Some(c) = r.resolve_char_ref().map_err(|e| e.to_string())? {
		out.push(c);
		return Ok(());
	}
	let name: &str = r;
	out.push_str(resolve_predefined_entity(name).ok_or_else(|| format!("unknown entity '&{name};'"))?);
	Ok(())
}

/// Formats a double as .NET's `XmlConvert` does: shortest round trip, `E` notation below 1e-4 and from 1e15.
pub(crate) fn fmt_f64(v: f64) -> String {
	if v.is_nan() {
		return "NaN".into();
	}
	if v.is_infinite() {
		return if v > 0.0 { "INF".into() } else { "-INF".into() };
	}
	if v == 0.0 {
		return if v.is_sign_negative() { "-0".into() } else { "0".into() };
	}
	let sci = format!("{v:e}");
	let (mantissa, exp) = sci.split_once('e').expect("LowerExp has an exponent");
	let exp: i32 = exp.parse().expect("integer exponent");
	if (-4..15).contains(&exp) {
		format!("{v}")
	} else {
		format!("{}E{}{:02}", mantissa, if exp < 0 { '-' } else { '+' }, exp.abs())
	}
}

/// Parses an integer as `XmlConvert` does.
pub(crate) fn parse_i32(s: &str) -> Option<i32> {
	s.trim().parse().ok()
}

/// Parses a boolean as `XmlConvert` does.
pub(crate) fn parse_bool(s: &str) -> Option<bool> {
	match s.trim() {
		"true" | "1" => Some(true),
		"false" | "0" => Some(false),
		_ => None,
	}
}

/// Parses a double as `XmlConvert` does (`INF`, `-INF`, `NaN`, invariant culture).
pub(crate) fn parse_f64(s: &str) -> Option<f64> {
	match s.trim() {
		"INF" => Some(f64::INFINITY),
		"-INF" => Some(f64::NEG_INFINITY),
		"NaN" => Some(f64::NAN),
		t if t.chars().any(|c| c.is_alphabetic() && c != 'e' && c != 'E') => None,
		t => t.parse().ok(),
	}
}

impl Element {
	pub(crate) fn new(name: &str) -> Self {
		Self { name: name.into(), ..Self::default() }
	}

	pub(crate) fn get(&self, key: &str) -> Option<&str> {
		self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
	}

	pub(crate) fn has(&self, key: &str) -> bool {
		self.get(key).is_some()
	}

	pub(crate) fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
		self.children.iter().filter(move |c| c.name == name)
	}

	pub(crate) fn child(&self, name: &str) -> Option<&Element> {
		self.children.iter().find(|c| c.name == name)
	}

	pub(crate) fn bad(&self, key: &str, value: &str, what: &str) -> FormatError {
		FormatError {
			message: format!("element '{}': attribute '{key}' has the value '{value}', which is not {what}", self.name),
			at: Some(self.at),
		}
	}

	pub(crate) fn string(&self, key: &str, default: &str) -> String {
		self.get(key).unwrap_or(default).to_owned()
	}

	pub(crate) fn f64(&self, key: &str, default: f64) -> Result<f64, FormatError> {
		self.get(key).map_or(Ok(default), |v| self.parse_f64(key, v))
	}

	pub(crate) fn i32(&self, key: &str, default: i32) -> Result<i32, FormatError> {
		self.get(key).map_or(Ok(default), |v| self.parse_i32(key, v))
	}

	pub(crate) fn bool(&self, key: &str, default: bool) -> Result<bool, FormatError> {
		self.get(key).map_or(Ok(default), |v| self.parse_bool(key, v))
	}

	/// `v`, the value of attribute `key`, as a number.
	pub(crate) fn parse_f64(&self, key: &str, v: &str) -> Result<f64, FormatError> {
		parse_f64(v).ok_or_else(|| self.bad(key, v, "a number"))
	}

	pub(crate) fn parse_i32(&self, key: &str, v: &str) -> Result<i32, FormatError> {
		parse_i32(v).ok_or_else(|| self.bad(key, v, "an integer"))
	}

	pub(crate) fn parse_bool(&self, key: &str, v: &str) -> Result<bool, FormatError> {
		parse_bool(v).ok_or_else(|| self.bad(key, v, "true or false"))
	}

	pub(crate) fn enumv<T: Word>(&self, key: &str, default: T) -> Result<T, FormatError> {
		self.get(key).map_or(Ok(default), |v| {
			T::parse(v.trim()).ok_or_else(|| self.bad(key, v, &format!("one of {}", T::NAMES.join(", "))))
		})
	}

	// ------------------------------------------------------------ writing

	pub(crate) fn attr(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
		self.attrs.push((key.into(), value.into()));
		self
	}

	pub(crate) fn str_attr(&mut self, key: &str, value: &str, default: &str) -> &mut Self {
		if value != default {
			self.attr(key, value);
		}
		self
	}

	pub(crate) fn enum_attr<T: Word>(&mut self, key: &str, value: T, default: T) -> &mut Self {
		if value != default {
			self.attr(key, value.name());
		}
		self
	}

	pub(crate) fn child_el(&mut self, el: Element) -> &mut Self {
		self.children.push(el);
		self
	}

	/// Serialises the element at `depth` tabs, without a trailing newline.
	pub(crate) fn write(&self, out: &mut String, depth: usize) {
		out.push_str(&"\t".repeat(depth));
		out.push('<');
		out.push_str(&self.name);
		for (k, v) in &self.attrs {
			out.push(' ');
			out.push_str(k);
			out.push_str("=\"");
			out.push_str(&escape(v, true));
			out.push('"');
		}
		if self.children.is_empty() && self.text.is_empty() {
			out.push_str(" />");
			return;
		}
		out.push('>');
		if self.children.is_empty() {
			out.push_str(&escape(&self.text, false));
		} else {
			for c in &self.children {
				out.push('\n');
				c.write(out, depth + 1);
			}
			out.push('\n');
			out.push_str(&"\t".repeat(depth));
		}
		out.push_str("</");
		out.push_str(&self.name);
		out.push('>');
	}
}

fn escape(s: &str, attribute: bool) -> Cow<'_, str> {
	if !s.contains(['&', '<', '>', '"', '\t', '\n', '\r']) {
		return Cow::Borrowed(s);
	}
	let mut out = String::with_capacity(s.len() + 8);
	for c in s.chars() {
		match c {
			'&' => out.push_str("&amp;"),
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			'"' if attribute => out.push_str("&quot;"),
			'\t' if attribute => out.push_str("&#x9;"),
			'\n' if attribute => out.push_str("&#xA;"),
			'\r' => out.push_str("&#xD;"),
			c => out.push(c),
		}
	}
	Cow::Owned(out)
}

/// The XML declaration every written file starts with.
pub(crate) const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>";

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn doubles_as_xmlconvert() {
		for (v, s) in [
			(100.0, "100"),
			(0.125, "0.125"),
			(-0.05, "-0.05"),
			(1e-7, "1E-07"),
			(0.0001, "0.0001"),
			(0.00005, "5E-05"),
			(1e15, "1E+15"),
			(123456789012345.0, "123456789012345"),
			(0.05905511811023622, "0.05905511811023622"),
			(1.5e-6, "1.5E-06"),
		] {
			assert_eq!(fmt_f64(v), s);
			assert_eq!(parse_f64(s), Some(v));
		}
	}

	#[test]
	fn tree_round_trip() {
		let text = "<?xml version=\"1.0\"?>\n<!DOCTYPE A [\n<!ELEMENT A ANY>\n]>\n<A x=\"1 &amp; &#xA;2\">\n\t<B>t&lt;</B>\n\t<C />\n</A>";
		let doc = parse(text).unwrap();
		assert!(doc.doctype.unwrap().contains("ELEMENT"));
		let a = doc.root;
		assert_eq!(a.get("x"), Some("1 & \n2"));
		assert_eq!(a.child("B").unwrap().text, "t<");
		assert_eq!(a.child("C").unwrap().at, (7, 2));
		let mut out = String::new();
		a.write(&mut out, 0);
		assert_eq!(out, "<A x=\"1 &amp; &#xA;2\">\n\t<B>t&lt;</B>\n\t<C />\n</A>");
	}

	#[test]
	fn malformed_reports_position() {
		let err = parse("<A>\n<B x=\"1\" x=\"2\" />\n</A>").err().unwrap();
		assert!(err.message.contains('x'), "{}", err.message);
		assert_eq!(err.at.unwrap().0, 2);
		assert!(parse("<A><B></A>").is_err());
		assert!(parse("<A>").is_err());
	}
}
