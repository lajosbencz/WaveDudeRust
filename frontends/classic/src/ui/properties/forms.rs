//! The rows of [`waveduderust_app::form`] on the editors of [`super`].

use eframe::egui::{Id, Ui, vec2};
use waveduderust_app::Event;
use waveduderust_app::form::{self, Glyph, RowKind};
use waveduderust_egui_preview::gradient_editor::{GradientColors, gradient_editor};

use super::super::color_picker::color_button;
use super::super::edit;
use super::super::icons::{self, Icon};
use super::super::text_box::{Kind, text_box};
use super::{Choices, Cx, Form, Pane, button, caption, check_box, combo, glyph_button};
use crate::kit::form::{self as kit_form, Look};
use crate::kit::{INPUT_HEIGHT, Palette};

/// Width of a compact glyph button.
const ROW_BUTTON_WIDTH: f32 = 28.;
const ROW_SPACING: f32 = 4.;
/// Width of a layer row's three buttons: the heading row leaves it free so the headings sit over the boxes.
const ROW_BUTTONS_WIDTH: f32 = 3. * ROW_BUTTON_WIDTH + 2. * ROW_SPACING;
/// Width of a layer row's ordinal.
const ORDINAL_WIDTH: f32 = 20.;
/// Width of a row's drag grip.
const GRIP_WIDTH: f32 = 14.;
/// A layer row's grip, ordinal and buttons.
const LAYER_FIXED: f32 = GRIP_WIDTH + ORDINAL_WIDTH + ROW_BUTTONS_WIDTH;
/// A fibre node row's label column.
const NODE_LABEL_WIDTH: f32 = 36.;
/// A fibre node row's tail: the Smooth box and the remove button, or the same width left empty.
const NODE_TAIL_WIDTH: f32 = 116.;
/// A stop row's ordinal and position box.
const STOP_ORDINAL: f32 = 16.;
const STOP_POSITION: f32 = 64.;

/// The icon of a glyph.
fn icon_of(glyph: Glyph) -> Icon {
	match glyph {
		Glyph::Save => icons::SAVE,
		Glyph::Close => icons::CLOSE,
		Glyph::Minus => icons::MINUS,
		Glyph::Up => icons::CHEVRON_UP,
		Glyph::Down => icons::CHEVRON_DOWN,
		Glyph::Linked => icons::CHAIN_LINKED,
		Glyph::Parted => icons::CHAIN_PARTED,
	}
}

/// Every row of `model`.
pub(super) fn draw(p: &mut Pane<'_>, model: &form::Form<'_>) {
	if let Some(d) = &model.description {
		p.description(d.title, d.tip, d.text, d.open);
	}
	for section in &model.sections {
		p.section(section.title, |f| {
			for row in &section.rows {
				draw_row(f, row);
			}
		});
	}
}

/// The width a button with `text` takes.
fn button_width(ui: &Ui, text: &str) -> f32 {
	let font = eframe::egui::FontId::proportional(crate::kit::FONT_SIZE);
	let shown = waveduderust_app::strings::strip_mnemonic(text);
	ui.painter().layout_no_wrap(shown, font, Palette::of(ui.ctx().theme()).text).size().x + 2. * super::BUTTON_PADDING.x
}

/// The number box of `n` in `ui`, `width` wide (the rest of the row when none).
fn number(ui: &mut Ui, cx: &mut Cx<'_>, n: &form::Number<'_>, width: Option<f32>) -> eframe::egui::Response {
	let id = cx.id(n.key);
	cx.number(ui, id, n.value, n.spec.clone(), n.blank, width, n.label, n.first, |v| (n.apply)(v))
}

/// A list's box: `n` `width` wide with its hint as tooltip.
fn list_value(ui: &mut Ui, cx: &mut Cx<'_>, n: &form::Number<'_>, width: f32) {
	number(ui, cx, n, Some(width)).on_hover_text(n.hint);
}

/// The text box of `t` filling the row.
fn text(ui: &mut Ui, cx: &mut Cx<'_>, t: &form::Text<'_>) {
	let id = cx.id(t.key);
	let kind = Kind { multiline: t.multiline, live: t.live };
	cx.text(ui, id, &t.value, kind, t.label, t.first, |v| (t.apply)(v));
}

/// A button with its text; a press applies.
fn text_button(ui: &mut Ui, cx: &mut Cx<'_>, b: &form::Button<'_>, tip: bool) {
	let response = button(ui, b.text, b.enabled);
	let response = if tip && !b.tip.is_empty() { response.on_hover_text(b.tip) } else { response };
	if response.clicked() {
		cx.apply((b.press)());
	}
}

/// A glyph button; a press applies.
fn glyph(ui: &mut Ui, cx: &mut Cx<'_>, b: &form::Button<'_>) {
	let g = b.glyph.unwrap_or(Glyph::Minus);
	let clicked = ui
		.add_enabled_ui(b.enabled, |ui| glyph_button(ui, cx.id(b.key), icon_of(g), None, false, b.tip).clicked())
		.inner;
	if clicked {
		cx.apply((b.press)());
	}
}

/// The kit's row of `row`, its label naming `names`.
fn line(row: &form::Row<'_>, names: Option<Id>) -> kit_form::Row<'static> {
	kit_form::Row { names, ..kit_form::Row::new(row.label, row.hint) }
}

fn draw_row(f: &mut Form<'_, '_>, row: &form::Row<'_>) {
	f.on(row.enabled);
	match &row.kind {
		RowKind::Number(n) => {
			let line = line(row, Some(f.cx.id(n.key)));
			f.row_of(line, |ui, cx| number(ui, cx, n, None));
		}
		RowKind::NumberButton(n, b) => {
			let line = line(row, Some(f.cx.id(n.key)));
			f.row_of(line, |ui, cx| {
				let width = ui.available_width() - button_width(ui, b.text) - 6.;
				number(ui, cx, n, Some(width));
				ui.add_space(6.);
				text_button(ui, cx, b, false);
			});
		}
		RowKind::Check(c) => {
			let id = f.cx.id(c.key);
			let set = f.row_of(line(row, None), |ui, _| check_box(ui, id, c.text, c.on));
			if let Some(on) = set {
				f.cx.out.extend((c.apply)(on));
			}
		}
		RowKind::Choice(c) => {
			let word = |i: usize| c.words[i].clone();
			let swatch = |i: usize| c.swatches.get(i).copied().flatten();
			let choices = Choices {
				len: c.words.len(),
				current: c.current,
				shown: &c.shown,
				word: &word,
				swatch: (!c.swatches.is_empty()).then_some(&swatch as &dyn Fn(usize) -> _),
				disabled: None,
				actions: &c.actions,
			};
			let line = line(row, Some(f.cx.id(c.key)));
			let picked = f.row_of(line, |ui, cx| combo(ui, cx.id(c.key), c.label, &choices));
			if let Some(i) = picked {
				f.cx.apply((c.pick)(i));
			}
		}
		RowKind::Text(t) => {
			// Next's ProjectView.xaml:46: the multiline box's label at its top
			let line = kit_form::Row { top: t.multiline, ..line(row, Some(f.cx.id(t.key))) };
			f.row_of(line, |ui, cx| text(ui, cx, t));
		}
		RowKind::Image { name, browse, save, remove } => {
			let line = line(row, Some(f.cx.id(name.key)));
			f.row_of(line, |ui, cx| {
				let buttons = button_width(ui, browse.text) + 2. * ROW_BUTTON_WIDTH + 3. * ROW_SPACING;
				let size = vec2(ui.available_width() - buttons, INPUT_HEIGHT);
				ui.allocate_ui(size, |ui| text(ui, cx, name));
				ui.add_space(ROW_SPACING);
				text_button(ui, cx, browse, true);
				ui.add_space(ROW_SPACING);
				glyph(ui, cx, save);
				ui.add_space(ROW_SPACING);
				glyph(ui, cx, remove);
			});
		}
		RowKind::Readout(text) => f.readout(row.label, text, row.hint, Look::Readout),
		RowKind::Value(text) => f.readout(row.label, text, row.hint, Look::Value),
		RowKind::Paragraph(text) => f.readout(None, text, "", Look::Readout),
		RowKind::Buttons(buttons) => {
			f.row_of(line(row, None), |ui, cx| {
				for (i, b) in buttons.iter().enumerate() {
					if i > 0 {
						ui.add_space(6.);
					}
					text_button(ui, cx, b, b.tip != row.hint);
				}
			});
		}
		RowKind::Group(rows) => rows.iter().for_each(|r| draw_row(f, r)),
		RowKind::Linked(rows, chain) => linked(f, rows, chain),
		RowKind::Points(p) => points(f, p),
		RowKind::Layers(l) => layers(f, l),
		RowKind::Nodes(n) => nodes(f, n),
		RowKind::Gradient(bar) => {
			let stops = f.row_of(line(row, None), |ui, cx| {
				let palette = Palette::of(ui.ctx().theme());
				let colors = GradientColors { hairline: palette.hairline, accent: palette.accent };
				gradient_editor(ui, cx.id(bar.key), &bar.gradient, bar.tip, colors)
			});
			if let Some(stops) = stops {
				f.cx.send((bar.apply)(stops));
			}
		}
		RowKind::Stop(stop) => {
			f.row_of(line(row, None), |ui, cx| stop_row(ui, cx, stop));
		}
	}
}

/// Two number rows with the chain toggle that links them right of both, in the middle of their rows.
fn linked(f: &mut Form<'_, '_>, rows: &[form::Row<'_>; 2], chain: &form::Button<'_>) {
	let room = f.beside(ROW_BUTTON_WIDTH + ROW_SPACING, |f| {
		for row in rows {
			draw_row(f, row);
		}
	});
	let at = room.with_min_x(room.max.x - ROW_BUTTON_WIDTH);
	let palette = Palette::of(f.ui().ctx().theme());
	let on = chain.glyph == Some(Glyph::Linked);
	let ink = if on { palette.accent } else { palette.text_dim };
	let id = f.cx.id(chain.key);
	let layout = eframe::egui::Layout::left_to_right(eframe::egui::Align::Center);
	let mut ui = f.ui().new_child(eframe::egui::UiBuilder::new().max_rect(at).layout(layout));
	let g = chain.glyph.unwrap_or(Glyph::Parted);
	if glyph_button(&mut ui, id, icon_of(g), Some(ink), true, chain.tip).clicked() {
		f.cx.apply((chain.press)());
	}
}

/// The width each of `n` boxes gets in `ui`'s row after `fixed` and the gaps.
fn share(ui: &Ui, n: usize, fixed: f32, gaps: usize) -> f32 {
	((ui.available_width() - fixed - gaps as f32 * ROW_SPACING) / n as f32).max(20.)
}

/// A gap between a list row's parts.
fn gap(ui: &mut Ui) {
	ui.add_space(ROW_SPACING);
}

/// An outline's points: an X / Y heading, a full-width row per point with X, Y and remove, Add point below.
fn points(f: &mut Form<'_, '_>, p: &form::Points<'_>) {
	let s = f.cx.s;
	if !p.items.is_empty() {
		f.full("", |ui, _| {
			let w = share(ui, 2, ROW_BUTTON_WIDTH, 2);
			caption(ui, p.heading[0], Some(w), true);
			gap(ui);
			caption(ui, p.heading[1], Some(w), true);
		});
	}
	for item in &p.items {
		f.full("", |ui, cx| {
			let w = share(ui, 2, ROW_BUTTON_WIDTH, 2);
			list_value(ui, cx, &item.x, w);
			gap(ui);
			list_value(ui, cx, &item.y, w);
			gap(ui);
			glyph(ui, cx, &item.remove);
		});
	}
	f.full(s.get("TipAddPoint"), |ui, cx| text_button(ui, cx, &p.add, false));
}

/// A layer list's drag by a grip: the pointer's y and the grip's top at the press, the layer's index now.
#[derive(Clone, Copy)]
struct LayerDrag {
	y0: f32,
	top0: f32,
	current: usize,
}

/// An interference filter's layers, the outermost first: a row each with a grip, its thickness and index, up, down and
/// remove. A drag by a row's grip moves the layer to the row whose top lies nearest the grip's top plus the drag; each
/// crossing is one edit, and the mouse held keeps them one undo step.
fn layers(f: &mut Form<'_, '_>, l: &form::Layers<'_>) {
	let drag_id = f.cx.id(l.key);
	let mut drag = f.ui().data(|d| d.get_temp::<LayerDrag>(drag_id));
	let mut tops = Vec::new();
	if !l.items.is_empty() {
		f.full("", |ui, _| {
			let w = share(ui, 2, LAYER_FIXED, 4);
			ui.add_space(GRIP_WIDTH + ROW_SPACING + ORDINAL_WIDTH + ROW_SPACING);
			caption(ui, l.heading[0], Some(w), true);
			gap(ui);
			caption(ui, l.heading[1], Some(w), true);
		});
	}
	for (i, item) in l.items.iter().enumerate() {
		f.full("", |ui, cx| {
			let palette = Palette::of(ui.ctx().theme());
			let w = share(ui, 2, LAYER_FIXED, 4);
			let (grip, _) = ui.allocate_exact_size(vec2(GRIP_WIDTH, 22.), eframe::egui::Sense::hover());
			tops.push(grip.top());
			let response = ui
				.interact(grip, cx.id((l.key, i, "Grip")), eframe::egui::Sense::drag())
				.on_hover_text(l.grip_tip)
				.on_hover_cursor(eframe::egui::CursorIcon::ResizeRow);
			if response.drag_started()
				&& let Some(p) = response.interact_pointer_pos()
			{
				drag = Some(LayerDrag { y0: p.y, top0: grip.top(), current: i });
				cx.send(Event::MouseDown);
			}
			let glyph_rect = eframe::egui::Rect::from_center_size(grip.center(), vec2(16., 16.));
			icons::paint(ui.painter(), glyph_rect, icons::GRIP, palette.text_dim);
			gap(ui);
			caption(ui, &(i + 1).to_string(), Some(ORDINAL_WIDTH), true);
			gap(ui);
			list_value(ui, cx, &item.thickness, w);
			gap(ui);
			list_value(ui, cx, &item.index, w);
			gap(ui);
			glyph(ui, cx, &item.up);
			gap(ui);
			glyph(ui, cx, &item.down);
			gap(ui);
			glyph(ui, cx, &item.remove);
		});
	}
	f.full("", |ui, cx| {
		text_button(ui, cx, &l.add, true);
		ui.add_space(6.);
		text_button(ui, cx, &l.clear, true);
	});
	let (down, y) = f.ui().input(|i| (i.pointer.primary_down(), i.pointer.latest_pos().map(|p| p.y)));
	if let Some(d) = &mut drag {
		if !down {
			drag = None;
			f.cx.send(Event::MouseUp);
		} else if let Some(y) = y {
			let wanted = d.top0 + (y - d.y0);
			let nearest = tops
				.iter()
				.enumerate()
				.min_by(|a, b| (a.1 - wanted).abs().total_cmp(&(b.1 - wanted).abs()))
				.map(|(i, _)| i);
			if let Some(target) = nearest.filter(|&t| t != d.current && t < l.items.len()) {
				f.cx.send((l.reorder)(d.current, target));
				d.current = target;
			}
		}
	}
	match drag {
		Some(d) => f.ui().data_mut(|data| {
			data.insert_temp(drag_id, d);
		}),
		None => f.ui().data_mut(|data| data.remove::<LayerDrag>(drag_id)),
	}
}

/// A fibre's nodes: three rows each, "Node", "In" and "Out", each a label and its X and Y; the first carries Smooth
/// and remove.
fn nodes(f: &mut Form<'_, '_>, n: &form::Nodes<'_>) {
	let s = f.cx.s;
	if !n.items.is_empty() {
		f.full("", |ui, _| {
			let w = share(ui, 2, NODE_LABEL_WIDTH + NODE_TAIL_WIDTH, 3);
			ui.add_space(NODE_LABEL_WIDTH + ROW_SPACING);
			caption(ui, n.heading[0], Some(w), true);
			gap(ui);
			caption(ui, n.heading[1], Some(w), true);
		});
	}
	for item in &n.items {
		for (k, (label, x, y)) in item.lines.iter().enumerate() {
			f.full("", |ui, cx| {
				let w = share(ui, 2, NODE_LABEL_WIDTH + NODE_TAIL_WIDTH, 3);
				caption(ui, label, Some(NODE_LABEL_WIDTH), true);
				gap(ui);
				list_value(ui, cx, x, w);
				gap(ui);
				list_value(ui, cx, y, w);
				gap(ui);
				if k == 0 {
					let box_width = NODE_TAIL_WIDTH - ROW_BUTTON_WIDTH - ROW_SPACING;
					ui.allocate_ui(vec2(box_width, 23.), |ui| {
						let id = cx.id(item.smooth.key);
						if let Some(on) = check_box(ui, id, item.smooth.text, item.smooth.on) {
							cx.out.extend((item.smooth.apply)(on));
						}
					});
					let used = ui.min_rect().right() - ui.max_rect().left();
					let pad = (ui.max_rect().width() - used - ROW_BUTTON_WIDTH).max(0.);
					ui.add_space(pad);
					glyph(ui, cx, &item.remove);
				}
			});
		}
	}
	f.full(s.get("TipAddNode"), |ui, cx| text_button(ui, cx, &n.add, false));
}

/// A colour stop's row: its ordinal, position, swatch, hex text and remove button.
fn stop_row(ui: &mut Ui, cx: &mut Cx<'_>, stop: &form::StopRow<'_>) {
	let s = cx.s;
	caption(ui, &stop.ordinal.to_string(), Some(STOP_ORDINAL), false);
	gap(ui);
	number(ui, cx, &stop.position, Some(STOP_POSITION)).on_hover_text(stop.position.hint);
	gap(ui);
	let swatch = cx.id(stop.color.key);
	let recolor = |c| (stop.color.apply)(c);
	if let Some(c) = color_button(ui, swatch, stop.color.value, stop.color.tip, &s, &recolor) {
		cx.send(recolor(c));
	}
	gap(ui);
	let hex_width = ui.available_width() - ROW_BUTTON_WIDTH - ROW_SPACING;
	let id = cx.id(stop.hex.key);
	let tip = stop.hex.label;
	let edited = text_box(ui, id, &stop.hex.value, hex_width, Kind::default(), tip);
	if let Some(e) = edited.applied.and_then(|t| (stop.hex.apply)(t)) {
		cx.send(e);
	}
	if edited.response.has_focus() || edited.response.lost_focus() {
		edit::offer(ui.ctx(), id, edited.typed.and_then(|t| (stop.hex.apply)(t)));
	}
	edited.response.on_hover_text(tip);
	gap(ui);
	glyph(ui, cx, &stop.remove);
}
