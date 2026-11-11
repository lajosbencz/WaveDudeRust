//! Colour swatch button with an in-house picker in a popup: saturation/value plane, hue strip, R/G/B number
//! boxes and a hex box. Enter keeps the colour, Esc reverts it, a click outside closes.

use freya::engine::prelude::{Paint, SkRect};
use freya::prelude::*;

use crate::edit::use_pending_edit;
use crate::number_box::{NumberBox, NumberSpec};
use crate::paint::{fill, linear_shader, stroke};
use crate::text_box::TextBox;
use crate::theme::{UiFontExt, use_wave_theme};
use crate::tooltip::Tip;
use crate::util::{TestIdExt, focus_border, shift};

/// An 8-bit sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
	pub r: u8,
	pub g: u8,
	pub b: u8,
}

impl Rgb {
	pub const fn new(r: u8, g: u8, b: u8) -> Self {
		Self { r, g, b }
	}

	pub fn to_hex(self) -> String {
		format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
	}

	/// Parses `#RRGGBB`, `RRGGBB` or `#RGB`, any case.
	pub fn from_hex(text: &str) -> Option<Self> {
		let t = text.trim().trim_start_matches('#');
		let digits: Vec<u8> = t.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
		match digits.as_slice() {
			[r, g, b] => Some(Self::new(r * 17, g * 17, b * 17)),
			[r1, r2, g1, g2, b1, b2] => Some(Self::new(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2)),
			_ => None,
		}
	}

	pub fn to_color(self) -> Color {
		Color::from_rgb(self.r, self.g, self.b)
	}

	/// Straight interpolation in sRGB, `t` 0..1.
	pub fn lerp(self, other: Self, t: f64) -> Self {
		let t = t.clamp(0., 1.);
		let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
		Self::new(mix(self.r, other.r), mix(self.g, other.g), mix(self.b, other.b))
	}
}

/// Hue in degrees 0..360, saturation and value 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Hsv {
	pub h: f64,
	pub s: f64,
	pub v: f64,
}

impl Hsv {
	pub fn from_rgb(c: Rgb) -> Self {
		let (r, g, b) = (f64::from(c.r) / 255., f64::from(c.g) / 255., f64::from(c.b) / 255.);
		let max = r.max(g).max(b);
		let min = r.min(g).min(b);
		let d = max - min;
		let h = if d == 0. {
			0.
		} else if max == r {
			60. * ((g - b) / d).rem_euclid(6.)
		} else if max == g {
			60. * ((b - r) / d + 2.)
		} else {
			60. * ((r - g) / d + 4.)
		};
		Self { h, s: if max == 0. { 0. } else { d / max }, v: max }
	}

	pub fn to_rgb(self) -> Rgb {
		let h = self.h.rem_euclid(360.) / 60.;
		let c = self.v * self.s;
		let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
		let (r, g, b) = match h as u32 {
			0 => (c, x, 0.),
			1 => (x, c, 0.),
			2 => (0., c, x),
			3 => (0., x, c),
			4 => (x, 0., c),
			_ => (c, 0., x),
		};
		let m = self.v - c;
		let q = |v: f64| ((v + m) * 255.).round().clamp(0., 255.) as u8;
		Rgb::new(q(r), q(g), q(b))
	}

	/// Keeps hue and saturation of `self` where `c` carries none (greys), so the markers do not jump.
	pub fn follow(self, c: Rgb) -> Self {
		let mut next = Self::from_rgb(c);
		if next.to_rgb() == self.to_rgb() {
			return self;
		}
		if next.s == 0. {
			next.h = self.h;
			if next.v == 0. {
				next.s = self.s;
			}
		}
		next
	}
}

const PLANE: f32 = 160.;
const STRIP: f32 = 18.;

/// The tooltips of the picker's parts, so the app can localise them.
#[derive(Clone, Debug, PartialEq)]
pub struct PickerTips {
	pub plane: String,
	pub hue: String,
	pub channel: String,
}

impl Default for PickerTips {
	fn default() -> Self {
		Self {
			plane: "Saturation (Left, Right) and brightness (Up, Down); Shift moves by ten steps".into(),
			hue: "Hue: Up and Down move by a degree, ten with Shift".into(),
			channel: "Channel 0 to 255; Up and Down step it".into(),
		}
	}
}

/// The picker body, bound to a colour; every change writes through.
#[derive(Clone, PartialEq)]
pub struct ColorPicker {
	value: WritableSetter<Rgb>,
	tips: PickerTips,
}

impl ColorPicker {
	pub fn new(value: impl Into<WritableSetter<Rgb>>) -> Self {
		Self { value: value.into(), tips: PickerTips::default() }
	}

	pub fn tips(mut self, tips: PickerTips) -> Self {
		self.tips = tips;
		self
	}
}

impl Component for ColorPicker {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let rgb = *self.value.read();
		let mut hsv = use_state(|| Hsv::from_rgb(rgb));
		if hsv.peek().to_rgb() != rgb {
			let next = hsv.peek().follow(rgb);
			hsv.set(next);
		}
		let current = *hsv.read();
		let plane_id = use_a11y();
		let strip_id = use_a11y();
		let plane_focus = use_focus(plane_id);
		let strip_focus = use_focus(strip_id);
		let mut dragging = use_state(|| None::<(bool, f64, f64)>);
		let value = self.value.clone();

		let set_hsv = {
			let mut value = value.clone();
			move |next: Hsv| {
				hsv.set(next);
				value.set_if_modified(next.to_rgb());
			}
		};

		let plane_painter = RenderCallback::new(move |ctx: &mut FillRenderContext<'_>| {
			let RenderTarget::Background { canvas, .. } = &mut ctx.target else { return };
			let r = SkRect::new(0., 0., PLANE, PLANE);
			canvas.draw_rect(r, &fill(Hsv { h: current.h, s: 1., v: 1. }.to_rgb().to_color()));
			let mut p = Paint::default();
			p.set_shader(linear_shader((0., 0.), (PLANE, 0.), &[Color::WHITE, Color::WHITE.with_a(0)], None));
			canvas.draw_rect(r, &p);
			p.set_shader(linear_shader((0., 0.), (0., PLANE), &[Color::BLACK.with_a(0), Color::BLACK], None));
			canvas.draw_rect(r, &p);
			let (x, y) = ((current.s as f32 * PLANE).round() + 0.5, ((1. - current.v as f32) * PLANE).round() + 0.5);
			let mut ring = stroke(Color::WHITE, 1.5);
			ring.set_anti_alias(true);
			canvas.draw_circle((x, y), 5., &stroke(Color::BLACK, 3.));
			canvas.draw_circle((x, y), 5., &ring);
		});
		let strip_painter = RenderCallback::new(move |ctx: &mut FillRenderContext<'_>| {
			let RenderTarget::Background { canvas, .. } = &mut ctx.target else { return };
			let hues: Vec<Color> =
				(0..=6).map(|k| Hsv { h: f64::from(k) * 60., s: 1., v: 1. }.to_rgb().to_color()).collect();
			let mut p = Paint::default();
			p.set_shader(linear_shader((0., 0.), (0., PLANE), &hues, None));
			canvas.draw_rect(SkRect::new(0., 0., STRIP, PLANE), &p);
			let y = (current.h as f32 / 360. * PLANE).round() + 0.5;
			canvas.draw_rect(SkRect::new(0.5, y - 2., STRIP - 0.5, y + 2.), &stroke(Color::WHITE, 1.));
		});

		let pick = {
			let mut set_hsv = set_hsv.clone();
			move |plane: bool, x: f64, y: f64| {
				let fx = (x as f32 / PLANE).clamp(0., 1.);
				let fy = (y as f32 / PLANE).clamp(0., 1.);
				let mut h = *hsv.peek();
				if plane {
					h.s = f64::from(fx);
					h.v = 1. - f64::from(fy);
				} else {
					h.h = f64::from(fy) * 359.999;
				}
				set_hsv(h);
			}
		};
		let pending = use_pending_edit();
		let on_plane_down = {
			let mut pick = pick.clone();
			let pending = pending.clone();
			move |e: Event<PointerEventData>| {
				pending.flush();
				plane_id.request_focus();
				let (l, g) = (e.element_location(), e.global_location());
				dragging.set(Some((true, g.x - l.x, g.y - l.y)));
				pick(true, l.x, l.y);
			}
		};
		let on_strip_down = {
			let mut pick = pick.clone();
			move |e: Event<PointerEventData>| {
				pending.flush();
				strip_id.request_focus();
				let (l, g) = (e.element_location(), e.global_location());
				dragging.set(Some((false, g.x - l.x, g.y - l.y)));
				pick(false, l.x, l.y);
			}
		};
		let on_global_move = {
			let mut pick = pick.clone();
			move |e: Event<PointerEventData>| {
				if let Some((plane, ox, oy)) = *dragging.peek() {
					let g = e.global_location();
					pick(plane, g.x - ox, g.y - oy);
				}
			}
		};
		let on_plane_key = {
			let mut set_hsv = set_hsv.clone();
			move |e: Event<KeyboardEventData>| {
				let d = if shift(&e) { 0.1 } else { 0.01 };
				let mut h = *hsv.peek();
				match e.key {
					Key::Named(NamedKey::ArrowLeft) => h.s = (h.s - d).max(0.),
					Key::Named(NamedKey::ArrowRight) => h.s = (h.s + d).min(1.),
					Key::Named(NamedKey::ArrowUp) => h.v = (h.v + d).min(1.),
					Key::Named(NamedKey::ArrowDown) => h.v = (h.v - d).max(0.),
					_ => return,
				}
				e.stop_propagation();
				set_hsv(h);
			}
		};
		let on_strip_key = {
			let mut set_hsv = set_hsv.clone();
			move |e: Event<KeyboardEventData>| {
				let d = if shift(&e) { 10. } else { 1. };
				let mut h = *hsv.peek();
				match e.key {
					Key::Named(NamedKey::ArrowUp) => h.h = (h.h - d).rem_euclid(360.),
					Key::Named(NamedKey::ArrowDown) => h.h = (h.h + d).rem_euclid(360.),
					_ => return,
				}
				e.stop_propagation();
				set_hsv(h);
			}
		};

		let tips = self.tips.clone();
		let channel = |name: &'static str, get: fn(Rgb) -> u8, put: fn(Rgb, u8) -> Rgb| {
			let mut value = value.clone();
			Tip::new(tips.channel.clone()).child(
				rect()
					.horizontal()
					.cross_align(Alignment::center())
					.spacing(4.)
					.child(label().text(name).width(Size::px(12.)))
					.child(
						NumberBox::new(
							WritableSetter::new(f64::from(get(rgb)), move |v: f64| {
								let c = *value.peek();
								value.set_if_modified(put(c, v.round().clamp(0., 255.) as u8));
							}),
							NumberSpec::integer(0., 255.),
						)
						.width(Size::px(64.))
						.test_id(format!("ColorPicker{name}")),
					),
			)
		};
		let mut hex_value = value.clone();

		rect()
			.horizontal()
			.spacing(8.)
			.padding(8.)
			.color(theme.text)
			.ui_font(&theme)
			.on_global_pointer_move(on_global_move)
			.on_global_pointer_up(move |_| dragging.set_if_modified(None))
			.child(
				Tip::new(tips.plane.clone()).child(
					rect()
						.a11y_id(plane_id)
						.a11y_focusable(true)
						.test_id("ColorPickerPlane")
						.width(Size::px(PLANE))
						.height(Size::px(PLANE))
						.background(plane_painter)
						.border(focus_border(plane_focus(), theme.accent))
						.on_pointer_down(on_plane_down)
						.on_key_down(on_plane_key),
				),
			)
			.child(
				Tip::new(tips.hue.clone()).child(
					rect()
						.a11y_id(strip_id)
						.a11y_focusable(true)
						.test_id("ColorPickerHue")
						.width(Size::px(STRIP))
						.height(Size::px(PLANE))
						.background(strip_painter)
						.border(focus_border(strip_focus(), theme.accent))
						.on_pointer_down(on_strip_down)
						.on_key_down(on_strip_key),
				),
			)
			.child(
				rect()
					.spacing(4.)
					.child(
						rect()
							.width(Size::px(84.))
							.height(Size::px(24.))
							.background(rgb.to_color())
							.border(Border::new().fill(theme.border).width(1.).alignment(BorderAlignment::Inner)),
					)
					.child(channel("R", |c| c.r, |c, v| Rgb { r: v, ..c }))
					.child(channel("G", |c| c.g, |c, v| Rgb { g: v, ..c }))
					.child(channel("B", |c| c.b, |c, v| Rgb { b: v, ..c }))
					.child(
						TextBox::new(rgb.to_hex(), move |text: String| {
							if let Some(c) = Rgb::from_hex(&text) {
								hex_value.set_if_modified(c);
							}
						})
						.select_all_on_focus(true)
						.width(Size::px(84.)),
					),
			)
	}
}

/// A swatch that opens the picker in a popup below it (click, Space, Enter or Down).
#[derive(Clone, PartialEq)]
pub struct ColorButton {
	value: WritableSetter<Rgb>,
	test_id: Option<String>,
	tooltip: Option<String>,
	tips: PickerTips,
}

impl ColorButton {
	pub fn new(value: impl Into<WritableSetter<Rgb>>) -> Self {
		Self { value: value.into(), test_id: None, tooltip: None, tips: PickerTips::default() }
	}

	/// The swatch's tooltip.
	pub fn tooltip(mut self, text: impl Into<String>) -> Self {
		self.tooltip = Some(text.into());
		self
	}

	/// The picker's tooltips.
	pub fn tips(mut self, tips: PickerTips) -> Self {
		self.tips = tips;
		self
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = Some(id.into());
		self
	}
}

impl Component for ColorButton {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let mut open = use_state(|| None::<Rgb>);
		let mut swatch_area = use_state(Area::default);
		let mut popup_area = use_state(Area::default);
		let rgb = *self.value.read();
		let mut value = self.value.clone();

		let close = move |revert: bool| {
			let start = *open.peek();
			if let (true, Some(c)) = (revert, start) {
				value.set_if_modified(c);
			}
			open.set(None);
			id.request_focus();
		};

		let on_key = {
			let mut close = close.clone();
			move |e: Event<KeyboardEventData>| {
				let is_open = open.peek().is_some();
				match &e.key {
					Key::Named(NamedKey::Escape) if is_open => {
						e.stop_propagation();
						close(true);
					}
					Key::Named(NamedKey::Enter) if is_open => {
						e.stop_propagation();
						close(false);
					}
					Key::Named(NamedKey::Enter) | Key::Named(NamedKey::ArrowDown) => {
						e.stop_propagation();
						open.set(Some(rgb));
					}
					Key::Character(c) if c == " " && !is_open => {
						e.stop_propagation();
						open.set(Some(rgb));
					}
					_ => {}
				}
			}
		};
		let mut close_outside = close;
		let on_global_down = move |e: Event<PointerEventData>| {
			if open.peek().is_none() {
				return;
			}
			let p = e.global_location();
			let inside = |a: Area| {
				let a = a.to_f64();
				p.x >= a.min_x() && p.x < a.max_x() && p.y >= a.min_y() && p.y < a.max_y()
			};
			if !inside(*popup_area.peek()) && !inside(*swatch_area.peek()) {
				close_outside(false);
			}
		};

		rect()
			.on_key_down(on_key)
			.on_global_pointer_down(on_global_down)
			.child({
				let swatch = rect()
					.a11y_id(id)
					.a11y_focusable(true)
					.a11y_role(AccessibilityRole::ColorWell)
					.maybe_test_id(self.test_id.clone())
					.width(Size::px(36.))
					.height(Size::px(18.))
					.background(rgb.to_color())
					.border(Border::new().fill(theme.input_border).width(1.).alignment(BorderAlignment::Inner))
					.border(focus_border(focus(), theme.accent))
					.on_sized(move |e: Event<SizedEventData>| swatch_area.set_if_modified(e.area))
					.on_pointer_down(move |_| {
						id.request_focus();
						if open.peek().is_some() {
							open.set(None);
						} else {
							open.set(Some(rgb));
						}
					});
				match self.tooltip.clone() {
					Some(t) => Tip::new(t).child(swatch).into_element(),
					None => swatch.into_element(),
				}
			})
			.maybe_child(open.read().is_some().then(|| {
				rect().position(Position::new_absolute().top(20.).left(0.)).width(Size::px(0.)).child(
					rect()
						.layer(Layer::Overlay)
						.width(Size::px(330.))
						.background(theme.menu_background)
						.border(Border::new().fill(theme.menu_border).width(1.).alignment(BorderAlignment::Inner))
						.shadow((0., 2., 6., 0., Color::from_af32rgb(0.35, 0, 0, 0)))
						.on_sized(move |e: Event<SizedEventData>| popup_area.set_if_modified(e.area))
						.child(ColorPicker::new(self.value.clone()).tips(self.tips.clone())),
				)
			}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn hex_round_trip() {
		assert_eq!(Rgb::from_hex("#007acc"), Some(Rgb::new(0, 0x7A, 0xCC)));
		assert_eq!(Rgb::from_hex("FFF"), Some(Rgb::new(255, 255, 255)));
		assert_eq!(Rgb::from_hex("#12345"), None);
		assert_eq!(Rgb::from_hex("#GG0000"), None);
		assert_eq!(Rgb::new(1, 2, 254).to_hex(), "#0102FE");
	}

	#[test]
	fn hsv_round_trips_every_primary_and_a_grey() {
		for c in [
			Rgb::new(255, 0, 0),
			Rgb::new(0, 255, 0),
			Rgb::new(0, 0, 255),
			Rgb::new(12, 200, 99),
			Rgb::new(128, 128, 128),
		] {
			assert_eq!(Hsv::from_rgb(c).to_rgb(), c);
		}
		let h = Hsv::from_rgb(Rgb::new(0, 0, 255));
		assert!((h.h - 240.).abs() < 1e-9);
	}

	#[test]
	fn greys_keep_the_hue() {
		let red = Hsv { h: 0., s: 1., v: 1. };
		let blue = Hsv { h: 240., s: 0.5, v: 1. };
		assert_eq!(blue.follow(Rgb::new(10, 10, 10)).h, 240.);
		assert_eq!(blue.follow(Rgb::new(0, 0, 0)).s, 0.5);
		assert_eq!(blue.follow(blue.to_rgb()), blue);
		assert_eq!(red.follow(Rgb::new(0, 255, 0)).h, 120.);
	}

	#[test]
	fn lerp_in_srgb() {
		assert_eq!(Rgb::new(0, 0, 0).lerp(Rgb::new(255, 100, 10), 0.5), Rgb::new(128, 50, 5));
		assert_eq!(Rgb::new(0, 0, 0).lerp(Rgb::new(255, 100, 10), 2.), Rgb::new(255, 100, 10));
	}
}
