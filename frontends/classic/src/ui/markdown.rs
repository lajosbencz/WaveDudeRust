//! Markdown in the app's look. `egui_commonmark` draws the text; the code blocks (whose copy button it draws in
//! glyphs the bundled font lacks) are cut out and drawn here.

use eframe::egui::{self, FontId, Frame, RichText, Stroke, TextStyle, Ui};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::kit::{FONT_SIZE, Palette};

/// A run of Markdown or a code block's text.
pub(super) enum Piece {
	Text(String),
	Code(String),
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
