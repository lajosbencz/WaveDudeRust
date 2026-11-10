//! Number box: free text, Up/Down steps, PageUp/PageDown pages, wheel, horizontal drag scrub, repeat buttons,
//! linear or logarithmic stepping, clamping with a brief cue.

use std::time::Duration;

use freya::prelude::*;

use crate::edit::{FocusRequest, use_edit_slot, use_focus_on_layout};
use crate::icon::{IconView, icons};
use crate::text_box::field_frame;
use crate::text_field::TextField;
use crate::theme::{INPUT_HEIGHT, use_wave_theme};
use crate::tooltip::Tip;
use crate::util::{TestIdExt, is_named, use_modifiers};

/// How a step changes the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stepping {
	/// Adds the increment per step.
	Linear(f64),
	/// Multiplies by the factor per step (1.02: two percent); steps and drags keep three significant digits.
	Logarithmic(f64),
}

/// Drag speed chosen by the held modifier: Shift finer (x0.1), Ctrl coarser (x10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragSpeed {
	Fine,
	Normal,
	Coarse,
}

impl DragSpeed {
	pub fn from_modifiers(modifiers: Modifiers) -> Self {
		if modifiers.contains(Modifiers::SHIFT) {
			Self::Fine
		} else if modifiers.contains(Modifiers::CONTROL) {
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
pub struct Stepped {
	pub value: f64,
	pub clamped: bool,
}

/// The arithmetic of a number box. Pure; the view only routes input to it.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberSpec {
	pub min: f64,
	pub max: f64,
	/// Fraction digits a linear box's steps and drags round to.
	pub decimals: u32,
	pub stepping: Stepping,
	/// Steps per PageUp/PageDown.
	pub page_steps: u32,
	/// Value change per dragged logical pixel of a linear box.
	pub drag_step: f64,
	/// Logical pixels of drag that double (or halve) a logarithmic box's value.
	pub doubling_pixels: f64,
	pub decimal_separator: char,
}

impl NumberSpec {
	/// A linear box; decimals follow the increment's, a drag moves one increment per 4 px.
	pub fn linear(min: f64, max: f64, step: f64) -> Self {
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

	pub fn integer(min: f64, max: f64) -> Self {
		Self::linear(min, max, 1.)
	}

	/// A logarithmic box: two percent a step, ten steps a page, a doubling per 100 px of drag.
	pub fn logarithmic(min: f64, max: f64) -> Self {
		Self { stepping: Stepping::Logarithmic(1.02), decimals: 0, drag_step: 0., ..Self::linear(min, max, 1.) }
	}

	pub fn with_decimals(mut self, decimals: u32) -> Self {
		self.decimals = decimals;
		self
	}

	pub fn with_drag_step(mut self, per_pixel: f64) -> Self {
		self.drag_step = per_pixel;
		self
	}

	pub fn with_decimal_separator(mut self, separator: char) -> Self {
		self.decimal_separator = separator;
		self
	}

	pub fn clamp(&self, value: f64) -> Stepped {
		let v = if value.is_nan() { self.min.max(0f64.min(self.max)) } else { value };
		let c = v.clamp(self.min, self.max);
		Stepped { value: c, clamped: c != v }
	}

	/// Rounds to the box's precision: `decimals` when linear, three significant digits when logarithmic.
	pub fn round(&self, value: f64) -> f64 {
		match self.stepping {
			Stepping::Linear(_) => round_to(value, self.decimals),
			Stepping::Logarithmic(_) => round_significant(value, 3),
		}
	}

	/// `steps` steps (negative: down) from `value`, rounded and clamped.
	pub fn step(&self, value: f64, steps: i32) -> Stepped {
		let raw = match self.stepping {
			Stepping::Linear(inc) => value + inc * f64::from(steps),
			Stepping::Logarithmic(factor) => {
				let base = if value > 0. { value } else { self.min.max(f64::MIN_POSITIVE) };
				base * factor.powi(steps)
			}
		};
		self.clamp(self.round(raw))
	}

	pub fn page(&self, value: f64, pages: i32) -> Stepped {
		self.step(value, pages * self.page_steps as i32)
	}

	/// The value after a horizontal drag of `dx` logical pixels from `start`.
	pub fn drag(&self, start: f64, dx: f64, speed: DragSpeed) -> Stepped {
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
	pub fn format(&self, value: f64) -> String {
		let text = format!("{}", value + 0.);
		if self.decimal_separator == '.' { text } else { text.replace('.', &self.decimal_separator.to_string()) }
	}

	/// Parses typed text; either decimal separator is accepted, surrounding blanks ignored.
	pub fn parse(&self, text: &str) -> Option<f64> {
		let t = text.trim().replace([self.decimal_separator, ','], ".");
		t.parse::<f64>().ok().filter(|v| v.is_finite())
	}

	/// A step from typed text; text that does not parse counts as 0.
	pub fn step_text(&self, text: &str, steps: i32) -> Stepped {
		self.step(self.parse(text).unwrap_or(0.), steps)
	}

	/// The value typed text commits to, clamped but kept as typed; `None` when it does not parse.
	pub fn commit(&self, text: &str) -> Option<Stepped> {
		self.parse(text).map(|v| self.clamp(v))
	}
}

/// Fraction digits of an increment as written (0.05 has 2, 10 has 0).
pub fn decimals_of(step: f64) -> u32 {
	let text = format!("{:.10}", step.abs());
	let frac = text.split('.').nth(1).unwrap_or("").trim_end_matches('0');
	frac.len() as u32
}

pub fn round_to(value: f64, decimals: u32) -> f64 {
	let p = 10f64.powi(decimals as i32);
	(value * p).round() / p
}

pub fn round_significant(value: f64, digits: u32) -> f64 {
	if value == 0. || !value.is_finite() {
		return value;
	}
	let magnitude = value.abs().log10().floor() as i32;
	let shift = digits as i32 - 1 - magnitude;
	let p = 10f64.powi(shift);
	(value * p).round() / p
}

const DRAG_THRESHOLD: f64 = 4.;
const BUTTON_WIDTH: f32 = 14.;
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_INTERVAL: Duration = Duration::from_millis(60);

/// A number box bound to a value. Free text applies on Enter or focus loss, Esc reverts typed text.
#[derive(Clone, PartialEq)]
pub struct NumberBox {
	value: WritableSetter<f64>,
	spec: NumberSpec,
	enabled: bool,
	width: Size,
	test_id: Option<String>,
	tooltip: Option<String>,
	blank: bool,
	a11y_id: Option<AccessibilityId>,
	focus_request: Option<FocusRequest>,
}

impl NumberBox {
	pub fn new(value: impl Into<WritableSetter<f64>>, spec: NumberSpec) -> Self {
		Self {
			value: value.into(),
			spec,
			enabled: true,
			width: Size::px(90.),
			test_id: None,
			tooltip: None,
			blank: false,
			a11y_id: None,
			focus_request: None,
		}
	}

	/// The text box's id, for focusing it from outside.
	pub fn a11y_id(mut self, id: AccessibilityId) -> Self {
		self.a11y_id = Some(id);
		self
	}

	/// Takes the focus when `request` is raised and the box is laid out.
	pub fn focus_request(mut self, request: Option<FocusRequest>) -> Self {
		self.focus_request = request;
		self
	}

	pub fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = enabled;
		self
	}

	pub fn width(mut self, width: impl Into<Size>) -> Self {
		self.width = width.into();
		self
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = Some(id.into());
		self
	}

	/// Shows an empty box instead of the value (WPF's blank field for a value that does not apply).
	pub fn blank(mut self, blank: bool) -> Self {
		self.blank = blank;
		self
	}

	pub fn tooltip(mut self, text: impl Into<String>) -> Self {
		self.tooltip = Some(text.into());
		self
	}
}

#[derive(Clone, Copy, PartialEq)]
struct DragState {
	start_x: f64,
	start_value: f64,
	active: bool,
}

impl Component for NumberBox {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let spec = self.spec.clone();
		let enabled = self.enabled;
		let own_id = use_a11y();
		let input_id = self.a11y_id.unwrap_or(own_id);
		let focus = use_focus(input_id);
		let modifiers = use_modifiers();
		let mut text = use_state(|| spec.format(*self.value.peek()));
		let mut typed = use_state(|| false);
		let mut flash = use_state(|| false);
		let mut area = use_state(Area::default);
		let mut drag = use_state(|| None::<DragState>);
		let mut repeat = use_state(|| None::<TaskHandle>);
		let mut hovered = use_state(|| false);
		let mut spin_hover = use_state(|| None::<i32>);
		let mut spin_pressed = use_state(|| None::<i32>);

		let current = *self.value.read();
		let shown = if self.blank { String::new() } else { spec.format(current) };
		if !*typed.peek() && *text.peek() != shown {
			text.set(shown.clone());
		}

		let mut value = self.value.clone();
		let apply = {
			let spec = spec.clone();
			move |stepped: Stepped| {
				value.set_if_modified(stepped.value);
				text.set(spec.format(stepped.value));
				typed.set_if_modified(false);
				if stepped.clamped {
					flash.set(true);
					spawn(async move {
						timer(Duration::from_millis(350)).await;
						flash.set_if_modified(false);
					});
				}
			}
		};

		let step_by = {
			let spec = spec.clone();
			let mut apply = apply.clone();
			move |steps: i32| {
				let stepped = spec.step_text(&text.peek(), steps);
				apply(stepped);
			}
		};

		let commit = {
			let spec = spec.clone();
			let shown = shown.clone();
			let mut apply = apply.clone();
			move || {
				if *typed.peek() {
					let committed = spec.commit(&text.peek());
					match committed {
						Some(stepped) => apply(stepped),
						None => {
							text.set(shown.clone());
							typed.set(false);
						}
					}
				}
			}
		};

		let slot = use_edit_slot(input_id, focus);
		let commit = {
			let mut commit = commit;
			let slot = slot.clone();
			move || {
				slot.withdraw();
				commit();
			}
		};
		let on_laid_out = use_focus_on_layout(input_id, self.focus_request.clone());

		let on_pre_key_down = {
			let spec = spec.clone();
			let shown = shown.clone();
			let mut step_by = step_by.clone();
			let mut commit = commit.clone();
			let slot = slot.clone();
			move |e: Event<KeyboardEventData>| -> bool {
				let steps = match &e.key {
					Key::Named(NamedKey::ArrowUp) => Some(1),
					Key::Named(NamedKey::ArrowDown) => Some(-1),
					Key::Named(NamedKey::PageUp) => Some(spec.page_steps as i32),
					Key::Named(NamedKey::PageDown) => Some(-(spec.page_steps as i32)),
					_ => None,
				};
				if let Some(steps) = steps {
					e.stop_propagation();
					e.prevent_default();
					step_by(steps);
					return false;
				}
				if is_named(&e, NamedKey::Enter) {
					e.stop_propagation();
					commit();
					return false;
				}
				if is_named(&e, NamedKey::Escape) {
					if *typed.peek() {
						e.stop_propagation();
						text.set(shown.clone());
						typed.set(false);
						slot.withdraw();
					}
					return false;
				}
				if is_named(&e, NamedKey::Tab) {
					return false;
				}
				e.stop_propagation();
				e.prevent_default();
				true
			}
		};

		let on_wheel = {
			let mut step_by = step_by.clone();
			move |e: Event<WheelEventData>| {
				if !input_id.is_focused() || e.delta_y == 0. {
					return;
				}
				e.stop_propagation();
				let n = if modifiers.peek().contains(Modifiers::SHIFT) { 10 } else { 1 };
				step_by(if e.delta_y > 0. { n } else { -n });
			}
		};

		let on_global_down = move |e: Event<PointerEventData>| {
			let a = area.peek().to_f64();
			let p = e.global_location();
			let inside =
				p.x >= a.min_x() && p.x < a.max_x() - f64::from(BUTTON_WIDTH) && p.y >= a.min_y() && p.y < a.max_y();
			if inside && enabled && !input_id.is_focused() {
				drag.set(Some(DragState { start_x: p.x, start_value: current, active: false }));
			}
		};

		let on_global_move = {
			let spec = spec.clone();
			let mut apply = apply.clone();
			move |e: Event<PointerEventData>| {
				let Some(mut d) = *drag.peek() else { return };
				let dx = e.global_location().x - d.start_x;
				if !d.active && dx.abs() > DRAG_THRESHOLD {
					d.active = true;
					drag.set(Some(d));
					input_id.request_unfocus();
				}
				if d.active {
					let speed = DragSpeed::from_modifiers(*modifiers.peek());
					apply(spec.drag(d.start_value, dx, speed));
				}
			}
		};

		let on_global_up = move |_: Event<PointerEventData>| {
			drag.set_if_modified(None);
			spin_pressed.set_if_modified(None);
			if let Some(task) = repeat.write().take() {
				task.cancel();
			}
		};

		let on_global_key = {
			let mut apply = apply.clone();
			move |e: Event<KeyboardEventData>| {
				let state = *drag.peek();
				if let Some(d) = state
					&& d.active && is_named(&e, NamedKey::Escape)
				{
					e.stop_propagation();
					drag.set(None);
					apply(Stepped { value: d.start_value, clamped: false });
				}
			}
		};

		let repeat_button = |steps: i32, icon| {
			let step_by = step_by.clone();
			let mut first = step_by.clone();
			let background = if *spin_pressed.read() == Some(steps) {
				theme.pressed_background
			} else if *spin_hover.read() == Some(steps) {
				theme.button_hover
			} else {
				Color::TRANSPARENT
			};
			rect()
				.width(Size::px(BUTTON_WIDTH))
				.height(Size::percent(50.))
				.center()
				.background(background)
				.cursor(CursorIcon::Default)
				.on_pointer_enter(move |_| spin_hover.set(Some(steps)))
				.on_pointer_leave(move |_| spin_hover.set_if_modified(None))
				.on_pointer_down(move |e: Event<PointerEventData>| {
					e.stop_propagation();
					input_id.request_focus();
					spin_pressed.set(Some(steps));
					first(steps);
					let mut step_by = step_by.clone();
					let task = spawn(async move {
						timer(REPEAT_DELAY).await;
						loop {
							step_by(steps);
							timer(REPEAT_INTERVAL).await;
						}
					});
					if let Some(old) = repeat.write().replace(task) {
						old.cancel();
					}
				})
				.child(IconView::new(icon).color(theme.text))
		};

		let (slot_for_typing, commit_for_typing) = (slot.clone(), commit.clone());
		let input = TextField::new(WritableSetter::new(text, move |t: String| {
			text.set(t);
			typed.set_if_modified(true);
			slot_for_typing.offer(commit_for_typing.clone());
		}))
		.a11y_id(input_id)
		.enabled(enabled)
		.width(Size::flex(1.))
		.on_pre_key_down(on_pre_key_down);

		let mut frame = field_frame(&theme, enabled, focus().is_focused(), *hovered.read());
		if *flash.read() {
			frame = frame.border(Border::new().fill(theme.clamp_flash).width(1.).alignment(BorderAlignment::Inner));
		}
		let body = frame
			.width(Size::fill())
			.height(Size::px(INPUT_HEIGHT))
			.on_sized({
				let mut on_laid_out = on_laid_out;
				move |e: Event<SizedEventData>| {
					area.set_if_modified(e.area);
					on_laid_out(e);
				}
			})
			.on_pointer_enter(move |_| hovered.set(true))
			.on_pointer_leave(move |_| hovered.set(false))
			.on_wheel(on_wheel)
			.on_global_pointer_down(on_global_down)
			.on_global_pointer_move(on_global_move)
			.on_global_pointer_up(on_global_up)
			.on_global_key_down(on_global_key)
			.maybe_test_id(self.test_id.clone())
			.child(input)
			.maybe_child(enabled.then(|| {
				rect()
					.width(Size::px(BUTTON_WIDTH))
					.height(Size::fill())
					.child(repeat_button(1, icons::SPIN_UP))
					.child(repeat_button(-1, icons::SPIN_DOWN))
			}))
			.maybe_child((!enabled).then(|| rect().width(Size::px(BUTTON_WIDTH))));

		// the outermost wrapper takes the box's width so a flex box shares its row
		let tracked = crate::util::modifier_tracker(modifiers, body).width(if self.tooltip.is_some() {
			Size::fill()
		} else {
			self.width.clone()
		});
		match self.tooltip.clone() {
			// the wrapper takes the box's width so a flex box still shares its row
			Some(tip) => {
				let mut layout = LayoutData::default();
				layout.layout.width = self.width.clone();
				Tip::new(tip).layout(layout).child(tracked).into_element()
			}
			None => tracked.into_element(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn decimals_follow_the_increment() {
		assert_eq!(decimals_of(1.), 0);
		assert_eq!(decimals_of(0.05), 2);
		assert_eq!(decimals_of(0.1), 1);
		assert_eq!(decimals_of(100.), 0);
		assert_eq!(NumberSpec::linear(1., 2.55, 0.01).decimals, 2);
	}

	#[test]
	fn linear_steps_and_pages_clamp() {
		let s = NumberSpec::linear(0., 10., 0.5);
		assert_eq!(s.step(1., 1), Stepped { value: 1.5, clamped: false });
		assert_eq!(s.step(1., -3), Stepped { value: 0., clamped: true });
		assert_eq!(s.page(1., 1).value, 6.);
		assert_eq!(s.page(6., 1), Stepped { value: 10., clamped: true });
		assert_eq!(NumberSpec::linear(0., 1., 0.1).step(0.2, 1).value, 0.3);
	}

	#[test]
	fn logarithmic_steps_two_percent_with_three_digits() {
		let s = NumberSpec::logarithmic(0.0001, 100_000.);
		assert_eq!(s.step(1., 1).value, 1.02);
		assert_eq!(s.step(1., -1).value, 0.98);
		assert_eq!(s.step(100., 1).value, 102.);
		assert_eq!(s.page(1., 1).value, 1.22);
		assert_eq!(s.format(0.000102), "0.000102");
		assert_eq!(s.format(1020.), "1020");
		assert_eq!(s.format(1.5), "1.5");
		let mut v = 1.;
		for _ in 0..200 {
			let next = s.step(v, 1).value;
			assert!(next > v, "stalled at {v}");
			v = next;
		}
	}

	#[test]
	fn logarithmic_box_starts_from_minimum_when_not_positive() {
		let s = NumberSpec::logarithmic(0.5, 10.);
		assert_eq!(s.step(0., 1).value, 0.51);
		assert_eq!(s.step(-3., -1), Stepped { value: 0.5, clamped: true });
	}

	#[test]
	fn text_parses_either_separator_and_bad_text_counts_as_zero() {
		let s = NumberSpec::linear(-10., 10., 1.).with_decimal_separator(',');
		assert_eq!(s.parse(" 2,5 "), Some(2.5));
		assert_eq!(s.parse("2.5"), Some(2.5));
		assert_eq!(s.parse("abc"), None);
		assert_eq!(s.step_text("abc", 1).value, 1.);
		assert_eq!(s.format(2.), "2");
		assert_eq!(NumberSpec::linear(0., 9., 0.25).with_decimal_separator(',').format(2.5), "2,5");
		assert_eq!(s.commit("99"), Some(Stepped { value: 10., clamped: true }));
		assert_eq!(s.commit("x"), None);
	}

	#[test]
	fn values_show_their_shortest_form_and_typing_keeps_its_digits() {
		let s = NumberSpec::linear(0., 1000., 0.01);
		for (v, text) in [(120., "120"), (1.5, "1.5"), (0., "0"), (-0., "0"), (0.125, "0.125"), (30., "30")] {
			assert_eq!(s.format(v), text);
		}
		assert_eq!(s.commit("0.125"), Some(Stepped { value: 0.125, clamped: false }));
		assert_eq!(NumberSpec::integer(0., 9.).commit("2.5").map(|c| c.value), Some(2.5));
		assert_eq!(s.step(0.125, 1).value, 0.14);
	}

	#[test]
	fn drag_scrubs_linear_and_doubles_logarithmic() {
		let s = NumberSpec::linear(0., 100., 1.).with_drag_step(0.5);
		assert_eq!(s.drag(10., 20., DragSpeed::Normal).value, 20.);
		assert_eq!(s.drag(10., 20., DragSpeed::Fine).value, 11.);
		assert_eq!(s.drag(10., 20., DragSpeed::Coarse), Stepped { value: 100., clamped: true });
		let l = NumberSpec::logarithmic(0.0001, 100_000.);
		assert_eq!(l.drag(2., 100., DragSpeed::Normal).value, 4.);
		assert_eq!(l.drag(2., -100., DragSpeed::Normal).value, 1.);
	}

	#[test]
	fn significant_rounding() {
		assert_eq!(round_significant(12345., 3), 12300.);
		assert_eq!(round_significant(0.0012345, 3), 0.00123);
		assert_eq!(round_significant(-9.996, 3), -10.);
	}
}
