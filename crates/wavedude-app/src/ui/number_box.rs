//! The number box of docs/ui/30 (Number box): free text applied on Enter or focus loss (Esc reverts typed text
//! and keeps the key), Up/Down steps, PageUp/PageDown pages, the wheel steps while it has focus (ten with Shift),
//! a horizontal drag on an unfocused box scrubs, and spin buttons repeat while held; linear or logarithmic
//! stepping, clamped to its range with a brief cue on the border.

use eframe::egui::{
	self, Align2, Color32, Event, Id, Key, Modifiers, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, Vec2,
	WidgetInfo, vec2,
};
use wavedude_app_state::platform::Duration;

use super::edit;
use super::text_box::{hold_escape, select_all};
use super::theme::Palette;

/// How a step changes the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Stepping {
	/// Adds the increment per step.
	Linear(f64),
	/// Multiplies by the factor per step (1.02: two percent); steps and drags keep three significant digits.
	Logarithmic(f64),
}

/// The drag speed the held modifier chooses: Shift finer (x0.1), Ctrl coarser (x10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DragSpeed {
	Fine,
	Normal,
	Coarse,
}

impl DragSpeed {
	fn of(modifiers: Modifiers) -> Self {
		if modifiers.shift {
			Self::Fine
		} else if modifiers.command {
			Self::Coarse
		} else {
			Self::Normal
		}
	}

	fn factor(self) -> f64 {
		match self {
			Self::Fine => 0.1,
			Self::Normal => 1.,
			Self::Coarse => 10.,
		}
	}
}

/// A value after a step, drag or commit, and whether the bounds cut it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Stepped {
	pub(super) value: f64,
	pub(super) clamped: bool,
}

/// The arithmetic of a number box.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct NumberSpec {
	pub(super) min: f64,
	pub(super) max: f64,
	/// Fraction digits a linear box's steps and drags round to.
	decimals: u32,
	stepping: Stepping,
	/// Steps per PageUp/PageDown.
	page_steps: u32,
	/// Value change per dragged logical pixel of a linear box.
	drag_step: f64,
	/// Logical pixels of drag that double (or halve) a logarithmic box's value.
	doubling_pixels: f64,
	decimal_separator: char,
}

impl NumberSpec {
	/// A linear box; decimals follow the increment's, a drag moves one increment per 4 px.
	pub(super) fn linear(min: f64, max: f64, step: f64) -> Self {
		Self {
			min,
			max,
			decimals: decimals_of(step),
			stepping: Stepping::Linear(step),
			page_steps: 10,
			drag_step: step / 4.,
			doubling_pixels: 100.,
			decimal_separator: '.',
		}
	}

	pub(super) fn integer(min: f64, max: f64) -> Self {
		Self::linear(min, max, 1.)
	}

	/// Two percent a step, ten steps a page, a doubling per 100 px of drag.
	pub(super) fn logarithmic(min: f64, max: f64) -> Self {
		Self { stepping: Stepping::Logarithmic(1.02), decimals: 0, drag_step: 0., ..Self::linear(min, max, 1.) }
	}

	pub(super) fn with_decimals(mut self, decimals: u32) -> Self {
		self.decimals = decimals;
		self
	}

	pub(super) fn with_drag_step(mut self, per_pixel: f64) -> Self {
		self.drag_step = per_pixel;
		self
	}

	pub(super) fn with_decimal_separator(mut self, separator: char) -> Self {
		self.decimal_separator = separator;
		self
	}

	pub(super) fn clamp(&self, value: f64) -> Stepped {
		let v = if value.is_nan() { self.min.max(0f64.min(self.max)) } else { value };
		let c = v.clamp(self.min, self.max);
		Stepped { value: c, clamped: c != v }
	}

	/// Rounds to the box's precision: `decimals` when linear, three significant digits when logarithmic.
	fn round(&self, value: f64) -> f64 {
		match self.stepping {
			Stepping::Linear(_) => round_to(value, self.decimals),
			Stepping::Logarithmic(_) => round_significant(value, 3),
		}
	}

	/// `steps` steps (negative: down) from `value`, rounded and clamped.
	pub(super) fn step(&self, value: f64, steps: i32) -> Stepped {
		let raw = match self.stepping {
			Stepping::Linear(inc) => value + inc * f64::from(steps),
			Stepping::Logarithmic(factor) => {
				let base = if value > 0. { value } else { self.min.max(f64::MIN_POSITIVE) };
				base * factor.powi(steps)
			}
		};
		self.clamp(self.round(raw))
	}

	/// The value after a horizontal drag of `dx` logical pixels from `start`.
	fn drag(&self, start: f64, dx: f64, speed: DragSpeed) -> Stepped {
		let dx = dx * speed.factor();
		let raw = match self.stepping {
			Stepping::Linear(_) => start + dx * self.drag_step,
			Stepping::Logarithmic(_) => {
				let base = if start > 0. { start } else { self.min.max(f64::MIN_POSITIVE) };
				base * (dx / self.doubling_pixels).exp2()
			}
		};
		self.clamp(self.round(raw))
	}

	/// The shortest text that reads back as `value` ("120", "1.5", "0.125"), with the box's separator.
	pub(super) fn format(&self, value: f64) -> String {
		let text = format!("{}", value + 0.);
		if self.decimal_separator == '.' { text } else { text.replace('.', &self.decimal_separator.to_string()) }
	}

	/// Parses typed text; either decimal separator is accepted, surrounding blanks ignored.
	fn parse(&self, text: &str) -> Option<f64> {
		text.trim().replace([self.decimal_separator, ','], ".").parse::<f64>().ok().filter(|v| v.is_finite())
	}

	/// A step from typed text; text that does not parse counts as 0.
	fn step_text(&self, text: &str, steps: i32) -> Stepped {
		self.step(self.parse(text).unwrap_or(0.), steps)
	}

	/// The value typed text commits to, clamped but kept as typed; `None` when it does not parse.
	fn commit(&self, text: &str) -> Option<Stepped> {
		self.parse(text).map(|v| self.clamp(v))
	}
}

/// Fraction digits of an increment as written (0.05 has 2, 10 has 0).
fn decimals_of(step: f64) -> u32 {
	let text = format!("{:.10}", step.abs());
	text.split('.').nth(1).unwrap_or("").trim_end_matches('0').len() as u32
}

fn round_to(value: f64, decimals: u32) -> f64 {
	let p = 10f64.powi(decimals as i32);
	(value * p).round() / p
}

fn round_significant(value: f64, digits: u32) -> f64 {
	if value == 0. || !value.is_finite() {
		return value;
	}
	let magnitude = value.abs().log10().floor() as i32;
	let p = 10f64.powi(digits as i32 - 1 - magnitude);
	(value * p).round() / p
}

/// The spin buttons' column.
pub(super) const SPIN_WIDTH: f32 = 14.;
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_INTERVAL: Duration = Duration::from_millis(60);
const DRAG_THRESHOLD: f32 = 4.;
const FLASH: f64 = 0.35;
/// The text's room inside the border.
const TEXT_MARGIN: egui::Margin = egui::Margin::symmetric(3, 0);

/// Wheel notches over `rect`, up positive.
pub(super) fn wheel_notches(ui: &Ui, rect: Rect) -> i32 {
	if !ui.rect_contains_pointer(rect) {
		return 0;
	}
	ui.input(|i| {
		i.events
			.iter()
			.map(|e| match e {
				Event::MouseWheel { delta, .. } if delta.y != 0. => delta.y.signum() as i32,
				_ => 0,
			})
			.sum()
	})
}

/// The steps of the keys and the wheel for the focused box; taken, so neither the text nor the pane scrolls.
fn key_steps(ui: &Ui, rect: Rect, page: i32) -> i32 {
	let over = ui.rect_contains_pointer(rect);
	ui.input_mut(|i| {
		let mut steps = 0;
		for (key, n) in [(Key::ArrowUp, 1), (Key::ArrowDown, -1), (Key::PageUp, page), (Key::PageDown, -page)] {
			steps += n * i.count_and_consume_key(Modifiers::NONE, key) as i32;
		}
		if over {
			let notch = if i.modifiers.shift { 10 } else { 1 };
			let wheel: i32 = i
				.events
				.iter()
				.map(|e| match e {
					// Shift turns the wheel sideways on some systems
					Event::MouseWheel { delta, .. } if *delta != Vec2::ZERO => {
						let d = if delta.y == 0. { delta.x } else { delta.y };
						notch * d.signum() as i32
					}
					_ => 0,
				})
				.sum();
			if wheel != 0 {
				i.smooth_scroll_delta = Vec2::ZERO;
			}
			steps += wheel;
		}
		steps
	})
}

/// A spin button's steps this frame: one at the press, then one per interval after the delay while held.
fn spin(ui: &Ui, response: &Response) -> i32 {
	let due = response.id.with("due");
	if !response.is_pointer_button_down_on() {
		ui.data_mut(|d| d.remove::<f64>(due));
		return 0;
	}
	let now = ui.input(|i| i.time);
	let next = ui.data(|d| d.get_temp::<f64>(due));
	let (steps, next) = match next {
		None => (1, now + REPEAT_DELAY.as_secs_f64()),
		Some(t) if now >= t => (1, t + REPEAT_INTERVAL.as_secs_f64()),
		Some(t) => (0, t),
	};
	ui.data_mut(|d| d.insert_temp(due, next));
	ui.ctx().request_repaint_after(Duration::from_secs_f64((next - now).max(0.)));
	steps
}

/// A triangle pointing up (`up`) or down, centred in `rect`.
pub(super) fn spin_glyph(ui: &Ui, rect: Rect, up: bool, color: Color32) {
	let c = rect.center();
	let (w, h) = (3.5, 2.);
	let s = if up { -1. } else { 1. };
	let points = vec![c + vec2(-w, -s * h), c + vec2(w, -s * h), c + vec2(0., s * h)];
	ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// A drag that started on the text of an unfocused box: where, from which value, and whether it passed the
/// threshold.
#[derive(Clone, Copy)]
struct Drag {
	start_x: f32,
	start_value: f64,
	active: bool,
}

/// What a number box did in a frame.
pub(super) struct Edited {
	pub(super) response: Response,
	/// The value it applied.
	pub(super) applied: Option<f64>,
	/// The value its typed text would apply, for [`edit::offer`].
	pub(super) typed: Option<f64>,
}

impl Edited {
	/// Offers the typed value's `event` as the pending edit of box `id` while it has or just lost the focus.
	pub(super) fn offer(&self, ctx: &egui::Context, id: Id, event: impl Fn(f64) -> wavedude_app_state::Event) {
		if self.response.has_focus() || self.response.lost_focus() {
			edit::offer(ctx, id, self.typed.map(event));
		}
	}
}

/// The box at `rect` showing `value` (nothing when `blank`), `id` its text's; `label` names it for assistive
/// technology. Disabled with its `Ui`.
pub(super) fn number_box(
	ui: &mut Ui,
	rect: Rect,
	id: Id,
	value: f64,
	spec: &NumberSpec,
	blank: bool,
	label: Option<&str>,
) -> Edited {
	let palette = Palette::of(ui.ctx().theme());
	let enabled = ui.is_enabled();
	let focused = ui.memory(|m| m.has_focus(id));
	let typed_id = edit::typed_id(id);
	let shown = if blank { String::new() } else { spec.format(value) };
	let mut text =
		focused.then(|| ui.data(|d| d.get_temp::<String>(typed_id))).flatten().unwrap_or_else(|| shown.clone());
	let (text_rect, spin_rect) = rect.split_left_right_at_x(rect.right() - SPIN_WIDTH);
	let mut stepped = None;

	let drag_id = id.with("drag");
	let mut drag = ui.data(|d| d.get_temp::<Drag>(drag_id));
	if enabled && !focused && ui.input(|i| i.pointer.primary_pressed()) && ui.rect_contains_pointer(text_rect) {
		let x = ui.input(|i| i.pointer.press_origin()).map_or(0., |p| p.x);
		drag = Some(Drag { start_x: x, start_value: value, active: false });
	}
	if let Some(d) = &mut drag {
		let (down, x, modifiers) =
			ui.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos().map_or(d.start_x, |p| p.x), i.modifiers));
		let dx = x - d.start_x;
		if !down {
			// a press that did not drag is a click: it puts the caret in
			if !d.active {
				ui.memory_mut(|m| m.request_focus(id));
			}
			drag = None;
		} else if d.active && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
			stepped = Some(Stepped { value: d.start_value, clamped: false });
			drag = None;
		} else {
			if !d.active && dx.abs() > DRAG_THRESHOLD {
				d.active = true;
				ui.memory_mut(|m| m.surrender_focus(id));
			}
			if d.active {
				stepped = Some(spec.drag(d.start_value, f64::from(dx), DragSpeed::of(modifiers)));
			}
		}
	}
	match drag {
		Some(d) => ui.data_mut(|data| {
			data.insert_temp(drag_id, d);
		}),
		None => ui.data_mut(|data| data.remove::<Drag>(drag_id)),
	}
	let scrubbing = drag.is_some_and(|d| d.active);

	let mut commit = false;
	if focused && enabled {
		let typed = text != shown;
		ui.input_mut(|i| {
			commit = i.consume_key(Modifiers::NONE, Key::Enter);
			if typed && i.consume_key(Modifiers::NONE, Key::Escape) {
				text.clone_from(&shown);
			}
		});
	}
	let page = spec.page_steps as i32;
	let mut steps = if focused && enabled { key_steps(ui, rect, page) } else { 0 };
	let (up, down) = spin_rect.split_top_bottom_at_fraction(0.5);
	let (up, down) = (ui.interact(up, id.with("up"), Sense::CLICK), ui.interact(down, id.with("down"), Sense::CLICK));
	if enabled {
		steps += spin(ui, &up) - spin(ui, &down);
	}

	let flash_id = id.with("flash");
	let now = ui.input(|i| i.time);
	let flashing = ui.data(|d| d.get_temp::<f64>(flash_id)).is_some_and(|until| now < until);
	let hovered = enabled && ui.rect_contains_pointer(rect);
	let border = if flashing {
		palette.clamp_flash
	} else if focused {
		palette.accent
	} else if hovered {
		palette.input_hover_border
	} else {
		palette.border
	};
	let fill = if enabled { palette.input_background } else { palette.window_background };
	ui.painter().rect(rect, 0., fill, Stroke::new(1., border), StrokeKind::Inside);
	let ink = if enabled { palette.text } else { palette.text_disabled };
	let edit = egui::TextEdit::singleline(&mut text)
		.id(id)
		.frame(egui::Frame::NONE)
		.margin(TEXT_MARGIN)
		.align(Align2::LEFT_CENTER)
		.text_color(ink)
		.return_key(None)
		// a press is the drag's until it ends: the text takes no caret from it
		.interactive(drag.is_none())
		.desired_width(text_rect.width());
	let response = ui.place(text_rect, edit);
	if response.gained_focus() {
		select_all(ui, id, text.chars().count());
	}
	if enabled {
		for (r, is_up) in [(&up, true), (&down, false)] {
			let back = if r.is_pointer_button_down_on() {
				palette.accent
			} else if r.hovered() {
				palette.button_hover
			} else {
				Color32::TRANSPARENT
			};
			ui.painter().rect_filled(r.rect.shrink(1.), 0., back);
			spin_glyph(ui, r.rect, is_up, palette.text);
		}
	}

	if steps != 0 {
		stepped = Some(spec.step_text(&text, steps));
		if !response.has_focus() {
			response.request_focus();
		}
	}
	let typed = text != shown;
	if commit && typed {
		match spec.commit(&text) {
			Some(s) => stepped = Some(s),
			None => text.clone_from(&shown),
		}
	} else if response.lost_focus() && typed && !scrubbing {
		stepped = spec.commit(&text);
	}
	if let Some(s) = stepped {
		text = spec.format(s.value);
		if s.clamped {
			ui.data_mut(|d| d.insert_temp(flash_id, now + FLASH));
			ui.ctx().request_repaint_after(Duration::from_secs_f64(FLASH));
		}
		if response.has_focus() {
			select_all(ui, id, text.chars().count());
		}
	}
	hold_escape(ui, id, text != shown);
	if response.has_focus() {
		ui.data_mut(|d| d.insert_temp(typed_id, text.clone()));
	} else if response.lost_focus() {
		ui.data_mut(|d| d.remove::<String>(typed_id));
	}
	if let Some(label) = label {
		response.widget_info(|| WidgetInfo::labeled(Role::SpinButton, enabled, label));
	}
	// a value this pass applied leaves nothing pending: the box shows it from the next pass
	let typed = (stepped.is_none() && response.has_focus() && text != shown)
		.then(|| spec.commit(&text))
		.flatten()
		.map(|s| s.value)
		.filter(|&v| v != value);
	Edited { response, applied: stepped.map(|s| s.value).filter(|&v| v != value || blank), typed }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_step_applies_once_and_leaves_nothing_pending() {
		use egui_kittest::kittest::Queryable as _;

		/// The value shown, the values applied, and the passes that also left a value pending.
		#[derive(Default)]
		struct Seen {
			value: f64,
			applied: Vec<f64>,
			both: usize,
		}
		let id = Id::unique("Box");
		let spec = NumberSpec::integer(0., 100.);
		let builder = egui_kittest::Harness::builder().with_context_setup(super::super::theme::install);
		let mut harness = builder.build_ui_state(
			|ui, seen: &mut Seen| {
				let rect = Rect::from_min_size(ui.cursor().min, vec2(80., 22.));
				let edited = number_box(ui, rect, id, seen.value, &spec, false, Some("Box"));
				seen.both += usize::from(edited.applied.is_some() && edited.typed.is_some());
				if let Some(v) = edited.applied {
					seen.applied.push(v);
					seen.value = v;
				}
			},
			Seen { value: 5., ..Seen::default() },
		);
		harness.get_by_label("Box").click();
		harness.run();
		for key in [Key::ArrowUp, Key::PageUp] {
			harness.key_press(key);
			harness.run();
		}
		harness.event(Event::Text("42".into()));
		harness.run();
		harness.key_press(Key::Enter);
		harness.run();
		assert_eq!(harness.state().applied, [6., 16., 42.]);
		assert_eq!(harness.state().both, 0, "an applied value was also left pending");
	}

	#[test]
	fn decimals_follow_the_increment() {
		assert_eq!((decimals_of(1.), decimals_of(0.05), decimals_of(0.1), decimals_of(100.)), (0, 2, 1, 0));
		assert_eq!(NumberSpec::linear(1., 2.55, 0.01).decimals, 2);
	}

	#[test]
	fn linear_steps_and_pages_clamp() {
		let s = NumberSpec::linear(0., 10., 0.5);
		assert_eq!(s.step(1., 1), Stepped { value: 1.5, clamped: false });
		assert_eq!(s.step(1., -3), Stepped { value: 0., clamped: true });
		assert_eq!(s.step(1., 10).value, 6.);
		assert_eq!(s.step(6., 10), Stepped { value: 10., clamped: true });
		assert_eq!(NumberSpec::linear(0., 1., 0.1).step(0.2, 1).value, 0.3);
		let speed = NumberSpec::linear(0.01, 100., 0.25);
		assert_eq!(speed.step(0.1, -1).value, 0.01);
	}

	#[test]
	fn logarithmic_steps_two_percent_with_three_digits() {
		let s = NumberSpec::logarithmic(0.0001, 100_000.);
		assert_eq!(s.step(1., 1).value, 1.02);
		assert_eq!(s.step(1., -1).value, 0.98);
		assert_eq!(s.step(1234., -1).value, 1210.);
		assert_eq!(s.step(0., 1).value, 0.000102);
		assert_eq!(s.step(1., 10).value, 1.22);
		assert_eq!(s.clamp(f64::NAN).value, 0.0001);
		let mut v = 1.;
		for _ in 0..200 {
			let next = s.step(v, 1).value;
			assert!(next > v, "stalled at {v}");
			v = next;
		}
	}

	#[test]
	fn drags_scale_by_pixels_and_speed() {
		let s = NumberSpec::linear(0., 100., 1.);
		assert_eq!(s.drag(10., 8., DragSpeed::Normal).value, 12.);
		assert_eq!(s.drag(10., 8., DragSpeed::Coarse).value, 30.);
		assert_eq!(s.drag(10., -100., DragSpeed::Normal), Stepped { value: 0., clamped: true });
		let fine = NumberSpec::linear(0., 100., 1.).with_decimals(1).with_drag_step(0.1);
		assert_eq!(fine.drag(1., 5., DragSpeed::Fine).value, 1.1);
		let log = NumberSpec::logarithmic(0.0001, 100_000.);
		assert_eq!(log.drag(1., 100., DragSpeed::Normal).value, 2.);
		assert_eq!(log.drag(1., -100., DragSpeed::Normal).value, 0.5);
	}

	#[test]
	fn texts_read_back_with_either_separator() {
		let s = NumberSpec::linear(0., 10., 0.1);
		assert_eq!(s.format(1.5), "1.5");
		assert_eq!(s.format(-0.), "0");
		assert_eq!(s.parse(" 2,5 "), Some(2.5));
		assert_eq!(s.parse("x"), None);
		let comma = s.clone().with_decimal_separator(',');
		assert_eq!(comma.format(1.5), "1,5");
		assert_eq!(comma.parse("1.25"), Some(1.25));
		assert_eq!(s.step_text("x", 1).value, 0.1);
		assert_eq!(s.commit("12"), Some(Stepped { value: 10., clamped: true }));
		assert_eq!(s.commit("abc"), None);
	}
}
