//! A picture's header strip (docs/ui/40-preview-pane.md, Header strip): the title at the start, the brightness box
//! and its Auto button at the far end, and a caption just before them that gives way first, then the title
//! (trimmed with an ellipsis). Above a picture the strip lies flat; beside one it is turned a quarter clockwise,
//! reading top to bottom, its box and button drawn turned and the box editing upright over its place while focused.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use eframe::egui::epaint::TextShape;
use eframe::egui::text::{LayoutJob, TextWrapping};
use eframe::egui::{
	self, Button, Color32, FontId, Galley, Id, Label, Order, Rect, RichText, Role, Sense, Stroke, StrokeKind, Ui,
	WidgetInfo, pos2, vec2,
};
use wavedude_app_state::Event;

use crate::ui::number_box::{self, NumberSpec, SPIN_WIDTH};
use crate::ui::theme::{FONT_SIZE, INPUT_HEIGHT, Palette, ROW_HEIGHT};

/// Which parts fit along a strip's length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StripLayout {
	pub(super) caption: bool,
	/// Room for the title; it is trimmed with an ellipsis below its natural width.
	pub(super) title_width: f32,
}

/// Lengths along the strip; `caption` includes its trailing margin, `spacing` is the least space after the title.
pub(super) fn strip_layout(length: f32, title: f32, caption: f32, tools: f32, spacing: f32) -> StripLayout {
	let caption_shown = title + spacing + caption + tools <= length;
	let others = tools + if caption_shown { caption } else { 0. };
	StripLayout { caption: caption_shown, title_width: title.min((length - spacing - others).max(0.)) }
}

/// Whether the strip goes beside the picture: the frame (the cell inside its margins) is wider than the picture's
/// aspect, so the spare room is at the sides.
pub(super) fn strip_beside(frame: (f32, f32), canvas: (u32, u32)) -> bool {
	let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
	frame.1 > 0. && frame.0 * ch > frame.1 * cw
}

/// The brightness box's range.
pub(super) const BRIGHTNESS: (f64, f64) = (0.0001, 100_000.);
const SPACING: f32 = 12.;
const CAPTION_MARGIN: f32 = 4.;
const AUTO_GAP: f32 = 4.;
const AUTO_PADDING: f32 = 7.;
const BOX_WIDTH: f32 = 64.;
/// The cell's margin around the strip.
pub(super) const CELL_MARGIN: f32 = 4.;
/// Margin before the strip's start when it is on its side.
const SIDE_START: f32 = 10.;
/// Margin after the strip: below it when flat, before its end when on its side.
const END_MARGIN: f32 = 4.;

/// The strip of `cell` beside or above its picture, and the room left for the picture.
pub(super) fn strip_and_area(cell: Rect, beside: bool) -> (Rect, Rect) {
	let inner = cell.shrink(CELL_MARGIN);
	if beside {
		let strip = Rect::from_min_max(
			pos2(inner.left(), inner.top() + SIDE_START),
			pos2(inner.left() + ROW_HEIGHT, inner.bottom() - END_MARGIN),
		);
		(strip, Rect::from_min_max(pos2(strip.right(), cell.top()), cell.max))
	} else {
		let strip = Rect::from_min_size(inner.min, vec2(inner.width(), ROW_HEIGHT));
		(strip, Rect::from_min_max(pos2(cell.left(), strip.bottom() + END_MARGIN), cell.max))
	}
}

/// What a strip shows.
pub(super) struct Strip<'a> {
	pub(super) title: &'a str,
	pub(super) caption: &'a str,
	/// The brightness box's tooltip.
	pub(super) hint: &'a str,
	pub(super) auto: &'a str,
	pub(super) auto_tip: &'a str,
	pub(super) brightness: f64,
	/// The brightness box's text.
	pub(super) id: Id,
	/// The event of a brightness, for the box's pending edit.
	pub(super) event: &'a dyn Fn(f64) -> Event,
}

/// What the user did on a strip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum StripAction {
	Brightness(f64),
	Auto,
}

/// Lengths along a strip `length` long: the title's and caption's room, where the box and the button start.
struct Along {
	layout: StripLayout,
	box_start: f32,
	auto_start: f32,
	auto_width: f32,
}

impl Along {
	fn new(length: f32, title: &Galley, caption: &Galley, auto: &Galley) -> Self {
		let auto_width = auto.size().x + 2. * AUTO_PADDING;
		let tools = BOX_WIDTH + AUTO_GAP + auto_width;
		let layout = strip_layout(length, title.size().x, caption.size().x + CAPTION_MARGIN, tools, SPACING);
		let auto_start = length - auto_width;
		Self { layout, box_start: auto_start - AUTO_GAP - BOX_WIDTH, auto_start, auto_width }
	}
}

/// `text` in `font` on one line, trimmed with an ellipsis to `width`.
fn galley(ui: &Ui, text: &str, font: FontId, width: f32) -> Arc<Galley> {
	let mut job = LayoutJob::simple_singleline(text.to_owned(), font, Color32::PLACEHOLDER);
	job.wrap = TextWrapping::truncate_at_width(width);
	ui.painter().layout_job(job)
}

/// The strip in `rect`; `editing` keeps whether its turned box edits upright.
pub(super) fn show(
	ui: &mut Ui,
	rect: Rect,
	beside: bool,
	strip: &Strip<'_>,
	editing: &mut bool,
) -> Option<StripAction> {
	let bold = FontId::new(FONT_SIZE, wavedude_view::fonts::bold());
	let regular = FontId::proportional(FONT_SIZE);
	let title = galley(ui, strip.title, bold.clone(), f32::INFINITY);
	let caption = galley(ui, strip.caption, regular.clone(), f32::INFINITY);
	let auto = galley(ui, strip.auto, regular, f32::INFINITY);
	let length = if beside { rect.height() } else { rect.width() };
	let along = Along::new(length, &title, &caption, &auto);
	let spec = NumberSpec::logarithmic(BRIGHTNESS.0, BRIGHTNESS.1);
	if !beside {
		return flat(ui, rect, strip, &along, &spec, &bold, &caption);
	}
	turned(ui, rect, strip, &along, &spec, editing, (&bold, &caption, &auto))
}

fn flat(
	ui: &mut Ui,
	rect: Rect,
	strip: &Strip<'_>,
	along: &Along,
	spec: &NumberSpec,
	bold: &FontId,
	caption: &Galley,
) -> Option<StripAction> {
	let text = ui.visuals().text_color();
	let x = |a: f32| rect.left() + a;
	let row = |from: f32, width: f32, height: f32| {
		Rect::from_center_size(pos2(x(from) + width / 2., rect.center().y), vec2(width, height))
	};
	let title = Label::new(RichText::new(strip.title).font(bold.clone())).truncate().selectable(false);
	ui.put(row(0., along.layout.title_width, ROW_HEIGHT), title);
	if along.layout.caption {
		let at = pos2(x(along.box_start - CAPTION_MARGIN), rect.center().y);
		ui.painter().galley(at - vec2(caption.size().x, caption.size().y / 2.), caption.clone().into(), text);
	}
	let at = row(along.box_start, BOX_WIDTH, INPUT_HEIGHT);
	let edited = number_box::number_box(ui, at, strip.id, strip.brightness, spec, false, Some(strip.caption));
	edited.offer(ui.ctx(), strip.id, strip.event);
	let value = edited.applied;
	edited.response.on_hover_text(strip.hint);
	let auto = ui
		.scope(|ui| {
			ui.spacing_mut().button_padding.x = AUTO_PADDING;
			ui.put(row(along.auto_start, along.auto_width, INPUT_HEIGHT), Button::new(strip.auto))
		})
		.inner;
	let auto = auto.on_hover_text(strip.auto_tip);
	super::take_activation_keys(&auto);
	if auto.clicked() {
		return Some(StripAction::Auto);
	}
	value.map(StripAction::Brightness)
}

/// Turned a quarter clockwise: a length `a` along the strip lies `a` below its top, a row's top edge on the
/// strip's right.
fn turned(
	ui: &mut Ui,
	rect: Rect,
	strip: &Strip<'_>,
	along: &Along,
	spec: &NumberSpec,
	editing: &mut bool,
	(bold, caption, auto): (&FontId, &Arc<Galley>, &Arc<Galley>),
) -> Option<StripAction> {
	let palette = Palette::of(ui.ctx().theme());
	let visuals = ui.visuals().clone();
	let text = visuals.text_color();
	let painter = ui.painter_at(rect);
	let span = |from: f32, length: f32| {
		Rect::from_min_max(pos2(rect.left(), rect.top() + from), pos2(rect.right(), rect.top() + from + length))
	};
	// a text at `from` along, centred across the strip
	let write = |galley: Arc<Galley>, from: f32, color: Color32| {
		let across = (ROW_HEIGHT - galley.size().y) / 2.;
		let at = pos2(rect.right() - across, rect.top() + from);
		painter.add(TextShape::new(at, galley, color).with_angle(FRAC_PI_2));
	};
	write(galley(ui, strip.title, bold.clone(), along.layout.title_width), 0., text);
	if along.layout.caption {
		write(caption.clone(), along.box_start - CAPTION_MARGIN - caption.size().x, text);
	}

	let face = span(along.box_start, BOX_WIDTH).shrink2(vec2((ROW_HEIGHT - INPUT_HEIGHT) / 2., 0.));
	let (text_rect, spin_rect) = face.split_top_bottom_at_y(face.bottom() - SPIN_WIDTH);
	let (down_rect, up_rect) = spin_rect.split_left_right_at_fraction(0.5);
	// it stands for the number box it opens, so it does not hold the pane's keys
	let box_response = ui.interact(text_rect, strip.id.with("turned"), Sense::click()).on_hover_text(strip.hint);
	let up = ui.interact(up_rect, strip.id.with("turned up"), Sense::CLICK);
	let down = ui.interact(down_rect, strip.id.with("turned down"), Sense::CLICK);
	painter.rect(face, 0., palette.input_background, Stroke::new(1., palette.border), StrokeKind::Inside);
	if !*editing {
		let value = galley(ui, &spec.format(strip.brightness), FontId::proportional(FONT_SIZE), f32::INFINITY);
		write(value, along.box_start + 3., text);
	}
	for (r, glyph_up) in [(&up, true), (&down, false)] {
		let color = if r.hovered() { text } else { palette.text_disabled };
		// a step up points to the turned row's top, the strip's right
		let c = r.rect.center();
		let s = if glyph_up { 1. } else { -1. };
		let points = vec![c + vec2(s * 2., 0.), c + vec2(-s * 2., -3.5), c + vec2(-s * 2., 3.5)];
		painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
	}
	let mut action = None;
	let step = |n: i32| Some(StripAction::Brightness(spec.step(strip.brightness, n).value));
	if up.clicked() {
		action = step(1);
	} else if down.clicked() {
		action = step(-1);
	}
	box_response.widget_info(|| WidgetInfo::labeled(Role::SpinButton, true, strip.caption));
	if box_response.clicked() || up.clicked() || down.clicked() {
		*editing = true;
		ui.memory_mut(|m| m.request_focus(strip.id));
	}

	let auto_rect = span(along.auto_start, along.auto_width);
	let auto_response = ui.interact(auto_rect, strip.id.with("auto"), Sense::click()).on_hover_text(strip.auto_tip);
	let look = visuals.widgets.style(&auto_response);
	painter.rect(auto_rect, 0., look.weak_bg_fill, look.bg_stroke, StrokeKind::Inside);
	write(auto.clone(), along.auto_start + AUTO_PADDING, look.text_color());
	auto_response.widget_info(|| WidgetInfo::labeled(Role::Button, true, strip.auto));
	super::take_activation_keys(&auto_response);
	if auto_response.clicked() {
		action = Some(StripAction::Auto);
	}

	if *editing {
		let at = Rect::from_center_size(face.center(), vec2(BOX_WIDTH, INPUT_HEIGHT));
		let area = egui::Area::new(strip.id.with("upright")).order(Order::Foreground).fixed_pos(at.min);
		let edited = area
			.show(ui.ctx(), |ui| {
				number_box::number_box(ui, at, strip.id, strip.brightness, spec, false, Some(strip.caption))
			})
			.inner;
		edited.offer(ui.ctx(), strip.id, strip.event);
		let response = edited.response;
		action = action.or(edited.applied.map(StripAction::Brightness));
		if !response.has_focus() && !ui.memory(|m| m.has_focus(strip.id)) {
			*editing = false;
		}
	}
	action
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn caption_gives_way_before_the_title() {
		assert_eq!(strip_layout(300., 40., 64., 110., 12.), StripLayout { caption: true, title_width: 40. });
		assert_eq!(strip_layout(200., 40., 64., 110., 12.), StripLayout { caption: false, title_width: 40. });
		assert_eq!(strip_layout(140., 40., 64., 110., 12.), StripLayout { caption: false, title_width: 18. });
		assert_eq!(strip_layout(50., 40., 64., 110., 12.), StripLayout { caption: false, title_width: 0. });
	}

	#[test]
	fn strip_goes_beside_a_wide_frame() {
		assert!(strip_beside((800., 200.), (100, 100)));
		assert!(!strip_beside((200., 800.), (100, 100)));
		assert!(!strip_beside((400., 200.), (200, 100)));
		assert!(!strip_beside((400., 0.), (200, 100)));
	}
}
