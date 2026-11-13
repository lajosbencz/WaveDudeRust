//! Markdown in the app's look. `egui_commonmark` draws the text; the code blocks (whose copy button it draws in
//! glyphs the bundled font lacks) and the `<figure name="..."/>` tags (which it would show as text) are cut out and
//! drawn here.

use eframe::egui::{self, FontId, Frame, RichText, Stroke, TextStyle, Ui, vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::theme::{FONT_SIZE, Palette};
use crate::manual::figure_name;
use crate::manual::figures::{self, Figure};

/// A run of Markdown, a code block's text or a figure.
pub(super) enum Piece {
	Text(String),
	Code(String),
	Figure(&'static Figure),
}

/// `text` cut into its pieces, in order.
pub(super) fn pieces(text: &str) -> Vec<Piece> {
	let mut out = Vec::new();
	let mut from = 0;
	let mut code: Option<String> = None;
	for (event, range) in Parser::new(text).into_offset_iter() {
		match event {
			Event::Start(Tag::CodeBlock(_)) => {
				cut(&mut out, &text[from..range.start]);
				from = range.end;
				code = Some(String::new());
			}
			Event::Text(t) => {
				if let Some(code) = &mut code {
					code.push_str(&t);
				}
			}
			Event::End(TagEnd::CodeBlock) => {
				out.push(Piece::Code(code.take().unwrap_or_default().trim_end().to_owned()));
			}
			Event::InlineHtml(html) | Event::Html(html) => {
				if let Some(figure) = figure_name(&html).and_then(figures::get) {
					cut(&mut out, &text[from..range.start]);
					from = range.end;
					out.push(Piece::Figure(figure));
				}
			}
			_ => {}
		}
	}
	cut(&mut out, &text[from..]);
	out
}

/// `run` as a Markdown piece, unless blank.
fn cut(out: &mut Vec<Piece>, run: &str) {
	if !run.trim().is_empty() {
		out.push(Piece::Text(run.to_owned()));
	}
}

/// Draws `pieces`.
pub(super) fn show(ui: &mut Ui, cache: &mut CommonMarkCache, pieces: &[Piece]) {
	ui.scope(|ui| {
		ui.style_mut().text_styles.insert(TextStyle::Heading, FontId::proportional(FONT_SIZE * 2.));
		for piece in pieces {
			match piece {
				Piece::Text(text) => {
					CommonMarkViewer::new().show(ui, cache, text);
				}
				Piece::Code(text) => code(ui, text),
				Piece::Figure(figure) => self::figure(ui, figure),
			}
		}
	});
}

fn code(ui: &mut Ui, text: &str) {
	let palette = Palette::of(ui.ctx().theme());
	Frame::new().fill(palette.input_background).stroke(Stroke::new(1., palette.border)).inner_margin(6.).show(
		ui,
		|ui| {
			ui.set_width(ui.available_width());
			ui.add(egui::Label::new(RichText::new(text).monospace()).wrap());
		},
	);
}

/// Drawn 1:1, or scaled down to the column; its place is known before the image loader decoded it.
fn figure(ui: &mut Ui, figure: &'static Figure) {
	let [width, height] = figure.size().map(|v| v as f32);
	let scale = (ui.available_width() / width).min(1.);
	ui.add_space(6.);
	let (rect, _) = ui.allocate_exact_size(vec2(width, height) * scale, egui::Sense::hover());
	egui::Image::from_bytes(figure.uri, figure.png).paint_at(ui, rect);
	ui.add_space(6.);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn code_blocks_and_figure_tags_are_cut_out() {
		let text = "# Title\n\nText. <figure name=\"main-window\"/>\n\n        code\n          more\n\nAfter.\n";
		let got: Vec<String> = pieces(text)
			.iter()
			.map(|p| match p {
				Piece::Text(t) => format!("text {}", t.trim()),
				Piece::Code(c) => format!("code {c}"),
				Piece::Figure(f) => format!("figure {}", f.name),
			})
			.collect();
		assert_eq!(got, ["text # Title\n\nText.", "figure main-window", "code     code\n      more", "text After."]);
	}
}
