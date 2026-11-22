//! Next's number box (docs/ui/30, Number box) on egui-mineral's `NumberEdit`, flat or turned a quarter clockwise.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use eframe::egui::epaint::{RectShape, TextShape};
use eframe::egui::{
	self, Align2, Color32, Context, CursorIcon, Event, EventFilter, FontId, Id, Key, Modifiers, Painter, Rect,
	Response, Role, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, vec2,
};
use egui_mineral::edit::{self, FieldFocus};
use egui_mineral::i18n::{AsciiFormat, LocaleFormat};
use egui_mineral::input::button_steps;
use egui_mineral::kit::state::TempState;
use egui_mineral::number::{self, NumberEdit, NumberPass, NumberRef, NumberSource, ScrubRate};
use egui_mineral::range::{RangeModel, Track};
use egui_mineral::runtime::Core;

use super::tokens::{BORDER, FONT_SIZE, NUMBER, Palette};

/// How long the border shows a value cut to the range.
const FLASH: f64 = 0.35;

/// What a box edits and how its text reads.
pub(crate) struct Number<'a> {
	pub range: RangeModel,
	/// Points of drag per step, per doubling on a logarithmic range.
	pub scrub: f32,
	pub format: &'a dyn Fn(f64) -> String,
	/// Typed text's number; `None` when it is not one.
	pub parse: &'a dyn Fn(&str) -> Option<f64>,
	/// Names the box for assistive technology.
	pub label: &'a str,
}

/// What a flat box did in a pass.
pub(crate) struct Edited {
	pub response: Response,
	/// The value it wrote.
	pub applied: Option<f64>,
	/// The value its typed text would write.
	pub typed: Option<f64>,
}

/// What a turned box did in a pass.
pub(crate) struct Turned {
	pub response: Response,
	pub applied: Option<f64>,
	/// A click asks for the upright editor.
	pub open: bool,
}

#[derive(Clone)]
struct State {
	edit: NumberEdit,
	marks: Marks,
}

/// What a box keeps beside its edit.
#[derive(Clone, Copy)]
struct Marks {
	/// The text edit had the focus at the end of its last pass.
	focused: bool,
	pass: u64,
	flash_until: f64,
	/// Esc ended the scrub; the drag runs on unheeded until the release.
	dropped: bool,
}

impl TempState for State {}

impl State {
	fn load(ctx: &Context, id: Id, value: Option<f64>, n: &Number<'_>) -> Self {
		let now = ctx.cumulative_pass_nr();
		let fresh = || Self {
			edit: NumberEdit::new(n.range, value, n.format),
			marks: Marks { focused: false, pass: now, flash_until: 0., dropped: false },
		};
		// a box missing from the last pass lost its edit with it: the window applied its typed text
		let mut st = Self::try_load(ctx, id).filter(|s| s.marks.pass.saturating_add(1) >= now).unwrap_or_else(fresh);
		st.edit.set_snap(false);
		st.edit.set_scrub_rate(ScrubRate::Pixels(n.scrub));
		st
	}

	fn store_at(mut self, ctx: &Context, id: Id, focused: bool) {
		self.marks.focused = focused;
		self.marks.pass = ctx.cumulative_pass_nr();
		self.store(ctx, id);
	}
}

impl Marks {
	fn flash(&mut self, ctx: &Context, clamped: bool) {
		if clamped {
			self.flash_until = ctx.input(|i| i.time) + FLASH;
			ctx.request_repaint_after_secs(FLASH as f32);
		}
	}

	fn flashing(&self, ctx: &Context) -> bool {
		ctx.input(|i| i.time) < self.flash_until
	}
}

/// The runtime's number format; ASCII without a runtime.
fn locale(core: Option<&Core>) -> Arc<dyn LocaleFormat> {
	core.map_or_else(|| Arc::new(AsciiFormat::default()) as Arc<dyn LocaleFormat>, |c| c.i18n().format())
}

/// `v` into the draft, through the locale's own text.
fn apply_value(pass: &mut NumberPass<'_>, locale: &dyn LocaleFormat, v: f64) -> bool {
	*pass.text_mut() = locale.number(v, None);
	pass.apply_text(locale)
}

/// The typed text into the draft, read by `n.parse`: either separator in any language, where en-US reads "2,5" as
/// 25. Keeps the text.
fn apply_typed(pass: &mut NumberPass<'_>, locale: &dyn LocaleFormat, n: &Number<'_>) -> bool {
	let typed = pass.text_mut().clone();
	let ok = match (n.parse)(&typed) {
		Some(v) => apply_value(pass, locale, v),
		None => typed.trim().is_empty() && pass.apply_text(locale),
	};
	*pass.text_mut() = typed;
	ok
}

/// The bound value: a number, or an optional one while the box is blank.
fn bind<'a>(value: Option<f64>, number: &'a mut f64, blank: &'a mut Option<f64>) -> NumberRef<'a, f64> {
	match value {
		Some(v) => {
			*number = v;
			NumberRef::Value(number)
		}
		None => NumberRef::Optional(blank),
	}
}

/// Blank or non-numeric text steps from 0 (NumberBox.cs:163).
fn from_zero(pass: &mut NumberPass<'_>, locale: &dyn LocaleFormat, numeric: bool) {
	if !numeric || pass.value().is_none() {
		apply_value(pass, locale, 0.);
	}
}

/// Steps from `keys`, each its own edit; whether any stepped.
fn step_keys(pass: &mut NumberPass<'_>, locale: &dyn LocaleFormat, numeric: bool, keys: &[Key]) -> bool {
	if keys.is_empty() {
		return false;
	}
	from_zero(pass, locale, numeric);
	for &k in keys {
		pass.step_key(k);
	}
	pass.commit();
	true
}

/// `steps` button steps, one edit.
fn step_by(pass: &mut NumberPass<'_>, locale: &dyn LocaleFormat, numeric: bool, steps: i64) -> bool {
	if steps == 0 {
		return false;
	}
	from_zero(pass, locale, numeric);
	pass.step(steps, NumberSource::Step);
	pass.commit();
	true
}

/// The step keys pressed, taken, and the wheel's notches over the box as keys: a page per notch with Shift.
fn key_presses(ui: &Ui, over: bool) -> Vec<Key> {
	ui.input_mut(|i| {
		let mut keys = Vec::new();
		for k in NumberEdit::STEP_KEYS {
			keys.extend(std::iter::repeat_n(k, i.count_and_consume_key(Modifiers::NONE, k)));
		}
		if over {
			let (up, down) =
				if i.modifiers.shift { (Key::PageUp, Key::PageDown) } else { (Key::ArrowUp, Key::ArrowDown) };
			let mut wheeled = false;
			for e in &i.events {
				if let Event::MouseWheel { delta, .. } = e
					&& *delta != Vec2::ZERO
				{
					// Shift turns the wheel sideways on some systems
					let d = if delta.y == 0. { delta.x } else { delta.y };
					keys.push(if d > 0. { up } else { down });
					wheeled = true;
				}
			}
			if wheeled {
				i.smooth_scroll_delta = Vec2::ZERO;
			}
		}
		keys
	})
}

/// Steps of a repeat button this pass: one at the press, then repeats while held.
fn presses(core: Option<&Core>, r: &Response) -> i64 {
	core.map_or(i64::from(r.clicked()), |c| i64::from(button_steps(c, r, r.id)))
}

/// The scrub handle over the unfocused text; `None` where drags scroll (touch).
fn scrub_handle(ui: &Ui, core: Option<&Core>, rect: Rect, id: Id) -> Option<Response> {
	let sense = core.map_or(Some(Sense::click_and_drag()), |c| number::scrub_sense(&c.input()))?;
	Some(ui.interact(rect, id.with("scrub"), sense))
}

/// A scrub on `handle` along `track`, Esc dropping it, `cursor` shown while it runs; whether it wrote.
fn scrub(
	ui: &Ui,
	pass: &mut NumberPass<'_>,
	marks: &mut Marks,
	handle: Option<&Response>,
	(track, cursor): (Track, CursorIcon),
) -> bool {
	if marks.dropped {
		marks.dropped = ui.input(|i| i.pointer.primary_down());
		pass.scrub(None, 1., track);
		return false;
	}
	let was = pass.scrubbing();
	// before the handle: egui ends its drag on the Esc
	if was && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
		// back to the value the drag started from (NumberBox.cs:417-423)
		pass.cancel();
		pass.scrub(None, 1., track);
		marks.dropped = true;
		return true;
	}
	let wrote = pass.scrub(handle, 1., track);
	if was && !pass.scrubbing() {
		pass.commit();
	}
	if pass.scrubbing() {
		// NumberBox.cs:265
		ui.ctx().set_cursor_icon(cursor);
	} else if handle.is_some_and(Response::hovered) {
		ui.ctx().set_cursor_icon(CursorIcon::Text);
	}
	wrote
}

/// While `typed`, Esc stays with the text edit `id` (it reverts) instead of taking its focus.
pub(crate) fn hold_escape(ctx: &Context, id: Id, typed: bool) {
	let filter = EventFilter { horizontal_arrows: true, vertical_arrows: true, tab: false, escape: typed };
	ctx.memory_mut(|m| m.set_focus_lock_filter(id, filter));
}

/// The border: cut to the range, focused, hovered or plain.
fn border(palette: &Palette, flashing: bool, focused: bool, hovered: bool) -> Color32 {
	if flashing {
		palette.clamp_flash
	} else if focused {
		palette.accent
	} else if hovered {
		palette.input_hover_border
	} else {
		palette.border
	}
}

/// A spin button's background and chevron; `turn` maps the flat chevron onto the screen.
fn paint_button(painter: &Painter, palette: &Palette, r: &Response, up: bool, turn: impl Fn(Vec2) -> Vec2) {
	let back = if r.is_pointer_button_down_on() {
		palette.accent
	} else if r.hovered() {
		palette.button_hover
	} else {
		Color32::TRANSPARENT
	};
	painter.rect_filled(r.rect, 0., back);
	// on the pixel before the centre along the box, as Next's
	let c = r.rect.center() + turn(vec2(-0.5, 0.));
	let c = egui::pos2(painter.round_to_pixel_center(c.x), painter.round_to_pixel_center(c.y));
	let flip = if up { 1. } else { -1. };
	let points = NUMBER.chevron.iter().map(|p| c + turn(vec2(p.x, flip * p.y))).collect();
	painter.add(Shape::line(points, Stroke::new(NUMBER.chevron_width, palette.text)));
}

/// The box at `rect` showing `value` (blank when `None`), `id` its text edit's. Disabled with its `Ui`.
pub(crate) fn flat(ui: &mut Ui, rect: Rect, id: Id, value: Option<f64>, n: &Number<'_>) -> Edited {
	let ctx = ui.ctx().clone();
	let core = Core::of(&ctx);
	let core = core.as_deref();
	let palette = Palette::of(ctx.theme());
	let enabled = ui.is_enabled();
	let focused = enabled && ctx.memory(|m| m.has_focus(id));
	let locale = locale(core);
	let state_id = id.with("number");
	let State { edit: mut number_edit, mut marks } = State::load(&ctx, state_id, value, n);
	let (mut number, mut blank) = (0., None);
	let mut bound = bind(value, &mut number, &mut blank);
	let shown = value.map_or_else(String::new, n.format);
	let mut pass = number_edit.begin(n.range, &bound, FieldFocus::of(focused), n.format);
	let mut wrote = false;
	let mut parsed = *pass.text_mut() != shown && apply_typed(&mut pass, &*locale, n);
	if !focused && pass.editing() && !pass.scrubbing() {
		// the focus left: typed text writes; the window applied the typed text of a box it did not see
		if marks.focused && parsed {
			pass.commit();
			wrote = true;
		} else if *pass.text_mut() == shown {
			pass.commit();
		} else {
			pass.cancel();
		}
		parsed = false;
	}
	// the text reads as a number: the value's own, or typed and parsed
	let mut numeric = *pass.text_mut() == shown || parsed;
	let scrubbing = pass.scrubbing();

	let inner = rect.shrink(BORDER);
	let (text_rect, spin) = inner.split_left_right_at_x(inner.right() - NUMBER.spin);
	if focused && !scrubbing {
		let typed = *pass.text_mut() != shown;
		let (enter, escape) = ui.input_mut(|i| {
			let enter = i.consume_key(Modifiers::NONE, Key::Enter);
			(enter, typed && !enter && i.consume_key(Modifiers::NONE, Key::Escape))
		});
		if enter && typed {
			if parsed {
				pass.commit();
				wrote = true;
			} else {
				pass.cancel();
			}
			numeric = true;
		} else if escape {
			pass.cancel();
			numeric = true;
		}
		let keys = key_presses(ui, ui.rect_contains_pointer(rect));
		if step_keys(&mut pass, &*locale, numeric, &keys) {
			wrote = true;
			numeric = true;
		}
	}

	let frame = ui.painter().add(Shape::Noop);
	let ink = if enabled { palette.text } else { palette.text_disabled };
	let text_edit = egui::TextEdit::singleline(pass.text_mut())
		.id(id)
		.frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(NUMBER.text_inset as i8, 0)))
		.align(Align2::LEFT_CENTER)
		.text_color(ink)
		.return_key(None)
		.desired_width(text_rect.width());
	let response = ui.place(text_rect, text_edit);
	if response.changed() {
		parsed = apply_typed(&mut pass, &*locale, n);
		numeric = parsed;
	}

	if enabled {
		let handle = if focused && !scrubbing { None } else { scrub_handle(ui, core, text_rect, id) };
		wrote |=
			scrub(ui, &mut pass, &mut marks, handle.as_ref(), (Track::horizontal_ltr(), CursorIcon::ResizeHorizontal));
		if !scrubbing && pass.scrubbing() {
			// NumberBox.cs:266-269
			response.request_focus();
		}
		let (up, down) = spin.split_top_bottom_at_fraction(0.5);
		let (up, down) =
			(ui.interact(up, id.with("up"), Sense::CLICK), ui.interact(down, id.with("down"), Sense::CLICK));
		let steps = presses(core, &up) - presses(core, &down);
		if step_by(&mut pass, &*locale, numeric, steps) {
			wrote = true;
			// the box takes the focus, so the wheel works afterwards (NumberBox.cs:212-220)
			response.request_focus();
		}
		if handle.as_ref().is_some_and(Response::clicked) {
			response.request_focus();
		}
		let painter = ui.painter();
		paint_button(painter, palette, &up, true, |v| v);
		paint_button(painter, palette, &down, false, |v| v);
	}
	if response.gained_focus() || (wrote && response.has_focus()) {
		edit::select_all(&ctx, id, pass.text_mut());
	}
	marks.flash(&ctx, wrote && pass.clamped());
	let after = if wrote { pass.value() } else { value };
	let typed = *pass.text_mut() != after.map_or_else(String::new, n.format);
	let pending =
		(typed && parsed && response.has_focus()).then(|| pass.value()).flatten().filter(|&v| Some(v) != value);
	hold_escape(&ctx, id, typed || pass.scrubbing());

	let hovered = enabled && ui.rect_contains_pointer(rect);
	let fill = if enabled { palette.input_background } else { palette.window_background };
	let stroke = Stroke::new(BORDER, border(palette, marks.flashing(&ctx), response.has_focus(), hovered));
	ui.painter().set(frame, RectShape::new(rect, 0., fill, stroke, StrokeKind::Inside));
	response.widget_info(|| WidgetInfo::labeled(Role::SpinButton, enabled, n.label));

	let changed = number_edit.finish(&mut bound, wrote);
	let applied = changed.then(|| bound.get()).flatten();
	State { edit: number_edit, marks }.store_at(&ctx, state_id, response.has_focus());
	Edited { response, applied, typed: pending }
}

/// The box turned in `rect` (the box's length down the screen), sharing the state of the flat box `id`, which edits
/// it upright; `upright` while that one is shown over it, when this one shows no text and takes no input.
pub(crate) fn turned(ui: &Ui, rect: Rect, id: Id, value: f64, n: &Number<'_>, upright: bool) -> Turned {
	let ctx = ui.ctx().clone();
	let core = Core::of(&ctx);
	let core = core.as_deref();
	let palette = Palette::of(ctx.theme());
	let painter = ui.painter_at(rect);
	let inner = rect.shrink(BORDER);
	let (text_rect, spin) = inner.split_top_bottom_at_y(inner.bottom() - NUMBER.spin);
	// a step up points to the turned box's top, the screen's right
	let (down, up) = spin.split_left_right_at_fraction(0.5);
	let face = ui.interact(rect, id.with("turned"), Sense::hover());
	let up = ui.interact(up, id.with("turned up"), Sense::CLICK);
	let down = ui.interact(down, id.with("turned down"), Sense::CLICK);
	let frame = painter.add(Shape::Noop);
	let mut out = Turned { response: face.clone(), applied: None, open: false };
	let state_id = id.with("number");
	let mut flashing = false;
	if !upright {
		let locale = locale(core);
		let State { edit: mut number_edit, mut marks } = State::load(&ctx, state_id, Some(value), n);
		let (mut number, mut blank) = (0., None);
		let mut bound = bind(Some(value), &mut number, &mut blank);
		let mut pass = number_edit.begin(n.range, &bound, FieldFocus::Unfocused, n.format);
		let handle = scrub_handle(ui, core, text_rect, id.with("turned"));
		let mut wrote =
			scrub(ui, &mut pass, &mut marks, handle.as_ref(), (Track::vertical_down(), CursorIcon::ResizeVertical));
		wrote |= step_by(&mut pass, &*locale, true, presses(core, &up) - presses(core, &down));
		marks.flash(&ctx, wrote && pass.clamped());
		let text = pass.text_mut().clone();
		let galley = painter.layout_no_wrap(text, FontId::proportional(FONT_SIZE), palette.text);
		let across = (rect.width() - galley.size().y) / 2.;
		let at = egui::pos2(rect.right() - across, inner.top() + NUMBER.text_inset);
		painter.add(TextShape::new(at, galley, palette.text).with_angle(FRAC_PI_2));
		out.open = up.clicked() || down.clicked() || handle.as_ref().is_some_and(Response::clicked);
		if let Some(h) = handle {
			h.widget_info(|| WidgetInfo::labeled(Role::SpinButton, true, n.label));
			out.response = h;
		}
		flashing = marks.flashing(&ctx);
		let changed = number_edit.finish(&mut bound, wrote);
		out.applied = changed.then(|| bound.get()).flatten();
		let focused = marks.focused;
		State { edit: number_edit, marks }.store_at(&ctx, state_id, focused);
	}
	let turn = |v: Vec2| vec2(-v.y, v.x);
	paint_button(&painter, palette, &up, true, turn);
	paint_button(&painter, palette, &down, false, turn);
	let stroke = Stroke::new(BORDER, border(palette, flashing, false, face.hovered()));
	painter.set(frame, RectShape::new(rect, 0., palette.input_background, stroke, StrokeKind::Inside));
	out
}
