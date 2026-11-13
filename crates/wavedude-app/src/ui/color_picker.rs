//! The colour swatch button and its picker popup of docs/ui/70 (Colour picker): a saturation/value plane, a hue
//! strip, R/G/B number boxes and a hex box. Every change writes through; Enter keeps the colour and closes, Esc
//! puts the colour back and closes, a click outside closes, and the focus goes back to the swatch.

use eframe::egui::{
	self, Align2, Color32, Id, Key, Mesh, Modifiers, Order, Pos2, Rect, Role, Sense, Shape, Stroke, StrokeKind, Ui,
	Vec2, WidgetInfo, pos2, vec2,
};
use wavedude_app_state::Event;
use wavedude_app_state::strings::Strings;
use wavedude_core::model::Rgb;

use super::edit;
use super::number_box::{NumberSpec, number_box};
use super::text_box::{self, Kind};
use super::theme::{INPUT_HEIGHT, Palette};

/// A colour with one channel replaced.
type Channel = fn(Rgb, u8) -> Rgb;

/// Parses `#RRGGBB`, `RRGGBB` or `#RGB`, any case.
pub(super) fn parse_hex(text: &str) -> Option<Rgb> {
	let t = text.trim().trim_start_matches('#');
	if t.len() == 3 && t.chars().all(|c| c.is_ascii_hexdigit()) {
		let d = |i: usize| u8::from_str_radix(&t[i..=i], 16).ok().map(|v| v * 17);
		return Some(Rgb::new(d(0)?, d(1)?, d(2)?));
	}
	Rgb::parse_hex(t)
}

/// `c` as egui's colour.
pub(super) fn color32(c: Rgb) -> Color32 {
	Color32::from_rgb(c.r, c.g, c.b)
}

/// Straight interpolation in sRGB, `t` 0..1.
pub(super) fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
	let t = t.clamp(0., 1.);
	let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
	Rgb::new(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
}

/// Hue in degrees 0..360, saturation and value 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct Hsv {
	h: f64,
	s: f64,
	v: f64,
}

impl Hsv {
	fn from_rgb(c: Rgb) -> Self {
		let (r, g, b) = (f64::from(c.r) / 255., f64::from(c.g) / 255., f64::from(c.b) / 255.);
		let max = r.max(g).max(b);
		let d = max - r.min(g).min(b);
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

	fn to_rgb(self) -> Rgb {
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

	/// Keeps the hue and saturation of `self` where `c` carries none (greys), so the markers do not jump.
	fn follow(self, c: Rgb) -> Self {
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
const SWATCH: Vec2 = Vec2::new(36., 18.);
const POPUP_WIDTH: f32 = 330.;
const PADDING: f32 = 8.;
const GAP: f32 = 8.;
const COLUMN: f32 = 84.;
const CHANNEL_LABEL: f32 = 12.;
const CHANNEL_BOX: f32 = 64.;
/// Cells per side of the plane's mesh.
const CELLS: usize = 16;

/// The picker's state while open: the colour it opened with, the markers' hue and saturation, a marker dragged.
#[derive(Clone, Copy)]
struct Open {
	start: Rgb,
	hsv: Hsv,
	/// The plane (true) or the strip being dragged.
	dragging: Option<bool>,
}

/// The swatch button `id` of colour `value`, `tip` its tooltip, and its picker while open; the colour it sets.
/// `event` makes the event of a colour, for the pending edit of the picker's typed text.
pub(super) fn color_button(
	ui: &mut Ui,
	id: Id,
	value: Rgb,
	tip: &str,
	s: &Strings,
	event: &dyn Fn(Rgb) -> Event,
) -> Option<Rgb> {
	let palette = Palette::of(ui.ctx().theme());
	let (rect, _) = ui.allocate_exact_size(SWATCH, Sense::hover());
	let swatch = ui.interact(rect, id, Sense::click()).on_hover_text(tip);
	let border = if swatch.has_focus() { palette.accent } else { palette.border };
	ui.painter().rect(rect, 0., color32(value), Stroke::new(1., border), StrokeKind::Inside);
	swatch.widget_info(|| WidgetInfo::labeled(Role::ColorWell, ui.is_enabled(), tip));
	let open_id = id.with("open");
	let mut open = ui.data(|d| d.get_temp::<Open>(open_id));
	let opening = Open { start: value, hsv: Hsv::from_rgb(value), dragging: None };
	if swatch.clicked() {
		swatch.request_focus();
		open = if open.is_some() { None } else { Some(opening) };
	} else if open.is_none() && swatch.has_focus() {
		let keys = ui.input_mut(|i| {
			i.consume_key(Modifiers::NONE, Key::Enter)
				|| i.consume_key(Modifiers::NONE, Key::ArrowDown)
				|| i.consume_key(Modifiers::NONE, Key::Space)
		});
		if keys {
			open = Some(opening);
		}
	}
	let mut set = None;
	if let Some(state) = &mut open {
		let at = rect.left_bottom() + vec2(0., 2.);
		let area = egui::Area::new(id.with("popup")).order(Order::Foreground).fixed_pos(at);
		let shown = area.show(ui.ctx(), |ui| {
			egui::Frame::new()
				.fill(palette.menu_background)
				.stroke(Stroke::new(1., palette.menu_border))
				.shadow(egui::Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(90) })
				.inner_margin(PADDING)
				.show(ui, |ui| {
					ui.set_width(POPUP_WIDTH - 2. * PADDING);
					picker(ui, id, value, state, s, event)
				})
				.inner
		});
		set = shown.inner;
		let popup = shown.response.rect;
		let (enter, esc) =
			ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
		let outside = ui.input(|i| {
			i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !popup.contains(p) && !rect.contains(p))
		});
		if esc {
			set = (value != state.start).then_some(state.start);
		}
		if enter || esc || outside {
			ui.data_mut(|d| d.remove::<Open>(open_id));
			swatch.request_focus();
			return set;
		}
	}
	match open {
		Some(state) => ui.data_mut(|d| {
			d.insert_temp(open_id, state);
		}),
		None => ui.data_mut(|d| d.remove::<Open>(open_id)),
	}
	set
}

/// The picker's body for colour `value`; the colour it sets.
fn picker(ui: &mut Ui, id: Id, value: Rgb, open: &mut Open, s: &Strings, event: &dyn Fn(Rgb) -> Event) -> Option<Rgb> {
	let palette = Palette::of(ui.ctx().theme());
	if open.hsv.to_rgb() != value {
		open.hsv = open.hsv.follow(value);
	}
	let mut next = None;
	ui.horizontal_top(|ui| {
		ui.spacing_mut().item_spacing = vec2(GAP, 4.);
		let (plane_rect, _) = ui.allocate_exact_size(Vec2::splat(PLANE), Sense::hover());
		let (strip_rect, _) = ui.allocate_exact_size(vec2(STRIP, PLANE), Sense::hover());
		let plane =
			ui.interact(plane_rect, id.with("plane"), Sense::click_and_drag()).on_hover_text(s.get("TipColorSquare"));
		let strip =
			ui.interact(strip_rect, id.with("hue"), Sense::click_and_drag()).on_hover_text(s.get("TipColorHue"));
		plane.widget_info(|| WidgetInfo::labeled(Role::Slider, true, s.get("TipColorSquare")));
		strip.widget_info(|| WidgetInfo::labeled(Role::Slider, true, s.get("TipColorHue")));
		for (r, is_plane) in [(&plane, true), (&strip, false)] {
			if r.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
				r.request_focus();
				open.dragging = Some(is_plane);
			}
		}
		if !ui.input(|i| i.pointer.primary_down()) {
			open.dragging = None;
		}
		let mut hsv = open.hsv;
		if let (Some(on_plane), Some(p)) = (open.dragging, ui.input(|i| i.pointer.latest_pos())) {
			let fx = f64::from(((p.x - plane_rect.left()) / PLANE).clamp(0., 1.));
			let fy = f64::from(((p.y - plane_rect.top()) / PLANE).clamp(0., 1.));
			if on_plane {
				(hsv.s, hsv.v) = (fx, 1. - fy);
			} else {
				hsv.h = fy * 359.999;
			}
		}
		if plane.has_focus() {
			let (d, keys) = arrows(ui, 0.01);
			hsv.s = (hsv.s + d * f64::from(keys.x)).clamp(0., 1.);
			hsv.v = (hsv.v - d * f64::from(keys.y)).clamp(0., 1.);
		}
		if strip.has_focus() {
			let (d, keys) = arrows(ui, 1.);
			hsv.h = (hsv.h + d * f64::from(keys.y)).rem_euclid(360.);
		}
		if hsv != open.hsv {
			open.hsv = hsv;
			next = Some(hsv.to_rgb());
		}
		paint_plane(ui, plane_rect, open.hsv, plane.has_focus().then_some(palette.accent));
		paint_strip(ui, strip_rect, open.hsv.h, strip.has_focus().then_some(palette.accent));
		ui.vertical(|ui| {
			ui.spacing_mut().item_spacing = vec2(4., 4.);
			let shown = next.unwrap_or(value);
			let (swatch, _) = ui.allocate_exact_size(vec2(COLUMN, 24.), Sense::hover());
			ui.painter().rect(swatch, 0., color32(shown), Stroke::new(1., palette.border), StrokeKind::Inside);
			let channels: [(&str, u8, Channel); 3] = [
				("R", shown.r, |c, v| Rgb { r: v, ..c }),
				("G", shown.g, |c, v| Rgb { g: v, ..c }),
				("B", shown.b, |c, v| Rgb { b: v, ..c }),
			];
			for (name, channel, put) in channels {
				ui.horizontal(|ui| {
					ui.spacing_mut().item_spacing.x = 4.;
					let (label, _) = ui.allocate_exact_size(vec2(CHANNEL_LABEL, INPUT_HEIGHT), Sense::hover());
					ui.painter().text(
						label.left_center(),
						Align2::LEFT_CENTER,
						name,
						egui::FontId::proportional(super::theme::FONT_SIZE),
						palette.text,
					);
					let (at, _) = ui.allocate_exact_size(vec2(CHANNEL_BOX, INPUT_HEIGHT), Sense::hover());
					let spec = NumberSpec::integer(0., 255.);
					let box_id = id.with(name);
					let edited = number_box(ui, at, box_id, f64::from(channel), &spec, false, Some(name));
					let channel = |v: f64| put(shown, v.round().clamp(0., 255.) as u8);
					edited.offer(ui.ctx(), box_id, |v| event(channel(v)));
					if let Some(v) = edited.applied {
						next = Some(channel(v));
					}
					edited.response.on_hover_text(s.get("TipColorChannel"));
				});
			}
			let hex_id = id.with("hex");
			let hex = text_box::text_box(ui, hex_id, &shown.to_hex(), COLUMN, Kind::default(), "#");
			if let Some(c) = hex.applied.as_deref().and_then(parse_hex) {
				next = Some(c);
			}
			if hex.response.has_focus() || hex.response.lost_focus() {
				edit::offer(ui.ctx(), hex_id, hex.typed.as_deref().and_then(parse_hex).map(event));
			}
		});
	});
	next.filter(|c| *c != value)
}

/// The arrows pressed on a focused marker, taken: the step (ten times with Shift) and the directions.
fn arrows(ui: &Ui, step: f64) -> (f64, Vec2) {
	ui.input_mut(|i| {
		let mut d = Vec2::ZERO;
		for m in [Modifiers::NONE, Modifiers::SHIFT] {
			let n = |i: &mut egui::InputState, key| i.count_and_consume_key(m, key) as f32;
			let v = vec2(n(i, Key::ArrowRight) - n(i, Key::ArrowLeft), n(i, Key::ArrowDown) - n(i, Key::ArrowUp));
			d += if m.shift { v * 10. } else { v };
		}
		(step, d)
	})
}

/// The plane at `rect` for hue `hsv.h`: white to the hue across, to black down; the marker at `hsv`.
fn paint_plane(ui: &Ui, rect: Rect, hsv: Hsv, focus: Option<Color32>) {
	let mut mesh = Mesh::default();
	let at = |i: usize, j: usize| (i as f32 / CELLS as f32, j as f32 / CELLS as f32);
	for j in 0..=CELLS {
		for i in 0..=CELLS {
			let (s, v) = at(i, j);
			let c = Hsv { h: hsv.h, s: f64::from(s), v: 1. - f64::from(v) }.to_rgb();
			mesh.colored_vertex(pos2(rect.left() + s * rect.width(), rect.top() + v * rect.height()), color32(c));
		}
	}
	let row = CELLS as u32 + 1;
	for j in 0..CELLS as u32 {
		for i in 0..CELLS as u32 {
			let a = j * row + i;
			mesh.add_triangle(a, a + 1, a + row);
			mesh.add_triangle(a + 1, a + row, a + row + 1);
		}
	}
	let painter = ui.painter();
	painter.add(Shape::mesh(mesh));
	let c = pos2(
		rect.left() + (hsv.s as f32 * PLANE).round() + 0.5,
		rect.top() + ((1. - hsv.v as f32) * PLANE).round() + 0.5,
	);
	painter.circle_stroke(c, 5., Stroke::new(3., Color32::BLACK));
	painter.circle_stroke(c, 5., Stroke::new(1.5, Color32::WHITE));
	if let Some(accent) = focus {
		painter.rect_stroke(rect, 0., Stroke::new(1., accent), StrokeKind::Outside);
	}
}

/// The hue strip at `rect`, red at the top through the hues back to red; the marker at `hue`.
fn paint_strip(ui: &Ui, rect: Rect, hue: f64, focus: Option<Color32>) {
	let mut mesh = Mesh::default();
	for k in 0..=6u32 {
		let y = rect.top() + rect.height() * k as f32 / 6.;
		let c = color32(Hsv { h: f64::from(k) * 60., s: 1., v: 1. }.to_rgb());
		mesh.colored_vertex(pos2(rect.left(), y), c);
		mesh.colored_vertex(pos2(rect.right(), y), c);
		if k > 0 {
			let b = 2 * k;
			mesh.add_triangle(b - 2, b - 1, b);
			mesh.add_triangle(b - 1, b, b + 1);
		}
	}
	let painter = ui.painter();
	painter.add(Shape::mesh(mesh));
	let y = rect.top() + (hue as f32 / 360. * PLANE).round() + 0.5;
	let marker = Rect::from_min_max(Pos2::new(rect.left() + 0.5, y - 2.), Pos2::new(rect.right() - 0.5, y + 2.));
	painter.rect_stroke(marker, 0., Stroke::new(1., Color32::WHITE), StrokeKind::Middle);
	if let Some(accent) = focus {
		painter.rect_stroke(rect, 0., Stroke::new(1., accent), StrokeKind::Outside);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn hex_reads_both_lengths() {
		assert_eq!(parse_hex("#007acc"), Some(Rgb::new(0, 0x7A, 0xCC)));
		assert_eq!(parse_hex("FFF"), Some(Rgb::new(255, 255, 255)));
		assert_eq!(parse_hex("#12345"), None);
		assert_eq!(parse_hex("#GG0000"), None);
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
		assert!((Hsv::from_rgb(Rgb::new(0, 0, 255)).h - 240.).abs() < 1e-9);
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
		assert_eq!(lerp(Rgb::new(0, 0, 0), Rgb::new(255, 100, 10), 0.5), Rgb::new(128, 50, 5));
		assert_eq!(lerp(Rgb::new(0, 0, 0), Rgb::new(255, 100, 10), 2.), Rgb::new(255, 100, 10));
	}
}
