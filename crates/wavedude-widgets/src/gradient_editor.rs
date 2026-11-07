//! Gradient editor: the gradient bar with draggable stop markers (a click on the bar adds a stop, Del
//! removes the selected one) and one row per stop: position, swatch, hex box, remove; Add stop.

use freya::engine::prelude::{Paint, PathBuilder, SkRect};
use freya::prelude::*;

use crate::color_button::{ColorButton, Rgb};
use crate::number_box::{NumberBox, NumberSpec};
use crate::paint::{fill, linear_shader, stroke};
use crate::theme::use_wave_theme;
use crate::util::{TestIdExt, focus_border, shift};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
	/// 0..1 along the gradient.
	pub position: f64,
	pub color: Rgb,
}

/// Colour stops kept sorted by position.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Gradient {
	pub stops: Vec<GradientStop>,
}

impl Gradient {
	pub fn new(mut stops: Vec<GradientStop>) -> Self {
		for s in &mut stops {
			s.position = s.position.clamp(0., 1.);
		}
		stops.sort_by(|a, b| a.position.total_cmp(&b.position));
		Self { stops }
	}

	/// The colour at `t`, straight sRGB interpolation between neighbours; held beyond the ends.
	pub fn color_at(&self, t: f64) -> Rgb {
		let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) else { return Rgb::default() };
		if t <= first.position {
			return first.color;
		}
		for pair in self.stops.windows(2) {
			let (a, b) = (pair[0], pair[1]);
			if t <= b.position {
				let span = b.position - a.position;
				return if span <= 0. { b.color } else { a.color.lerp(b.color, (t - a.position) / span) };
			}
		}
		last.color
	}

	/// The middle of the widest gap, the ends counting as gaps.
	pub fn widest_gap(&self) -> f64 {
		let mut edges = vec![0.];
		edges.extend(self.stops.iter().map(|s| s.position));
		edges.push(1.);
		let mut best = (0., 0.5);
		for w in edges.windows(2) {
			let gap = w[1] - w[0];
			if gap > best.0 {
				best = (gap, (w[0] + w[1]) / 2.);
			}
		}
		best.1
	}

	/// Inserts a stop and returns its index.
	pub fn insert(&mut self, position: f64, color: Rgb) -> usize {
		let position = position.clamp(0., 1.);
		let index = self.stops.iter().position(|s| s.position > position).unwrap_or(self.stops.len());
		self.stops.insert(index, GradientStop { position, color });
		index
	}

	/// Add stop: in the widest gap, with the gradient's colour there.
	pub fn add_in_widest_gap(&mut self) -> usize {
		let t = self.widest_gap();
		let c = self.color_at(t);
		self.insert(t, c)
	}

	/// Removes a stop; the last one stays.
	pub fn remove(&mut self, index: usize) -> bool {
		if self.stops.len() <= 1 || index >= self.stops.len() {
			return false;
		}
		self.stops.remove(index);
		true
	}

	/// Moves a stop and returns its index after re-sorting.
	pub fn set_position(&mut self, index: usize, position: f64) -> usize {
		let Some(mut stop) = self.stops.get(index).copied() else { return index };
		self.stops.remove(index);
		stop.position = position.clamp(0., 1.);
		self.insert(stop.position, stop.color)
	}

	pub fn set_color(&mut self, index: usize, color: Rgb) {
		if let Some(s) = self.stops.get_mut(index) {
			s.color = color;
		}
	}

	/// The stop whose marker is within `tolerance` (as a fraction of the bar) of `t`: the nearest, among equals
	/// the one drawn on top.
	pub fn stop_near(&self, t: f64, tolerance: f64) -> Option<usize> {
		self.stops
			.iter()
			.enumerate()
			.rev()
			.map(|(i, s)| (i, (s.position - t).abs()))
			.filter(|(_, d)| *d <= tolerance)
			.min_by(|a, b| a.1.total_cmp(&b.1))
			.map(|(i, _)| i)
	}
}

const BAR_HEIGHT: f32 = 20.;
const MARKER: f32 = 7.;

/// The editor, bound to a gradient.
#[derive(Clone, PartialEq)]
pub struct GradientEditor {
	value: WritableSetter<Gradient>,
}

impl GradientEditor {
	pub fn new(value: impl Into<WritableSetter<Gradient>>) -> Self {
		Self { value: value.into() }
	}
}

impl Component for GradientEditor {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let mut width = use_state(|| 1f32);
		let mut selected = use_state(|| None::<usize>);
		let mut dragging = use_state(|| None::<f64>);
		let gradient = self.value.read().clone();
		let value = self.value.clone();
		let sel = selected.read().filter(|&i| i < gradient.stops.len());

		let painter = {
			let gradient = gradient.clone();
			let (line, accent) = (theme.hairline, theme.accent);
			RenderCallback::new(move |ctx: &mut FillRenderContext| {
				let RenderTarget::Background { canvas, .. } = &mut ctx.target else { return };
				let w = ctx.size.width;
				let colors: Vec<Color> = gradient.stops.iter().map(|s| s.color.to_color()).collect();
				let positions: Vec<f32> = gradient.stops.iter().map(|s| s.position as f32).collect();
				let bar = SkRect::new(MARKER, 0., w - MARKER, BAR_HEIGHT);
				if colors.len() > 1 {
					let mut p = Paint::default();
					p.set_shader(linear_shader((bar.left, 0.), (bar.right, 0.), &colors, Some(&positions)));
					canvas.draw_rect(bar, &p);
				} else if let Some(c) = colors.first() {
					canvas.draw_rect(bar, &fill(*c));
				}
				canvas
					.draw_rect(SkRect::new(bar.left + 0.5, 0.5, bar.right - 0.5, BAR_HEIGHT - 0.5), &stroke(line, 1.));
				for (i, s) in gradient.stops.iter().enumerate() {
					let x = (bar.left + (bar.right - bar.left) * s.position as f32).round() + 0.5;
					let mut tri = PathBuilder::new();
					tri.move_to((x, BAR_HEIGHT + 1.));
					tri.line_to((x + MARKER - 1., BAR_HEIGHT + MARKER + 2.));
					tri.line_to((x - MARKER + 1., BAR_HEIGHT + MARKER + 2.));
					tri.close();
					let path = tri.detach();
					let mut body = fill(s.color.to_color());
					body.set_anti_alias(true);
					canvas.draw_path(&path, &body);
					let mut edge =
						stroke(if Some(i) == sel { accent } else { line }, if Some(i) == sel { 2. } else { 1. });
					edge.set_anti_alias(true);
					canvas.draw_path(&path, &edge);
				}
			})
		};

		let to_t = move |x: f64| f64::from(((x as f32 - MARKER) / (*width.peek() - 2. * MARKER).max(1.)).clamp(0., 1.));
		let on_down = {
			let mut value = value.clone();
			move |e: Event<PointerEventData>| {
				id.request_focus();
				let l = e.element_location();
				let t = to_t(l.x);
				let mut g = value.peek().clone();
				let tol = f64::from(MARKER / (*width.peek() - 2. * MARKER).max(1.));
				let index = match g.stop_near(t, tol) {
					Some(i) if l.y as f32 > BAR_HEIGHT - 4. => i,
					_ if l.y as f32 <= BAR_HEIGHT => {
						let c = g.color_at(t);
						let i = g.insert(t, c);
						value.set(g);
						i
					}
					Some(i) => i,
					None => return,
				};
				selected.set(Some(index));
				dragging.set(Some(e.global_location().x - l.x));
			}
		};
		let on_global_move = {
			let mut value = value.clone();
			move |e: Event<PointerEventData>| {
				let (Some(origin), Some(i)) = (*dragging.peek(), *selected.peek()) else { return };
				let mut g = value.peek().clone();
				let ni = g.set_position(i, to_t(e.global_location().x - origin));
				selected.set(Some(ni));
				value.set(g);
			}
		};
		let on_key = {
			let mut value = value.clone();
			move |e: Event<KeyboardEventData>| {
				let mut g = value.peek().clone();
				let count = g.stops.len();
				if count == 0 {
					return;
				}
				let i = selected.peek().unwrap_or(0).min(count - 1);
				let d = if shift(&e) { 0.1 } else { 0.01 };
				match e.key {
					Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowRight) => {
						let sign = if e.key == Key::Named(NamedKey::ArrowLeft) { -1. } else { 1. };
						let p = g.stops[i].position + sign * d;
						selected.set(Some(g.set_position(i, p)));
						value.set(g);
					}
					Key::Named(NamedKey::PageUp) => selected.set(Some(i.saturating_sub(1))),
					Key::Named(NamedKey::PageDown) => selected.set(Some((i + 1).min(count - 1))),
					Key::Named(NamedKey::Delete) => {
						if g.remove(i) {
							selected.set(Some(i.min(g.stops.len() - 1)));
							value.set(g);
						}
					}
					_ => return,
				}
				e.stop_propagation();
			}
		};

		let rows = (0..gradient.stops.len()).map(|i| StopRow { value: value.clone(), index: i }.into_element());
		let mut add_value = value.clone();
		rect()
			.width(Size::fill())
			.spacing(4.)
			.color(theme.text)
			.font_size(theme.font_size)
			.child(
				rect()
					.a11y_id(id)
					.a11y_focusable(true)
					.test_id("GradientBar")
					.width(Size::fill())
					.height(Size::px(BAR_HEIGHT + MARKER + 4.))
					.background(painter)
					.border(focus_border(focus(), theme.accent))
					.on_sized(move |e: Event<SizedEventData>| width.set_if_modified(e.area.width().max(1.)))
					.on_pointer_down(on_down)
					.on_global_pointer_move(on_global_move)
					.on_global_pointer_up(move |_| dragging.set_if_modified(None))
					.on_key_down(on_key),
			)
			.children(rows)
			.child(
				Button::new()
					.compact()
					.on_press(move |_| {
						let mut g = add_value.peek().clone();
						let i = g.add_in_widest_gap();
						add_value.set(g);
						selected.set(Some(i));
					})
					.child(label().text("Add stop")),
			)
	}
}

/// One stop's row: position, swatch, hex box, remove.
#[derive(Clone, PartialEq)]
struct StopRow {
	value: WritableSetter<Gradient>,
	index: usize,
}

impl Component for StopRow {
	fn render(&self) -> impl IntoElement {
		let i = self.index;
		let stop = self.value.read().stops.get(i).copied();
		let Some(stop) = stop else { return rect().into_element() };
		let mut hex = use_state(|| stop.color.to_hex());
		let mut shown = use_state(|| stop.color);
		if *shown.peek() != stop.color {
			shown.set(stop.color);
			hex.set(stop.color.to_hex());
		}
		let mut pos_value = self.value.clone();
		let mut color_value = self.value.clone();
		let mut hex_value = self.value.clone();
		let mut remove_value = self.value.clone();
		let several = self.value.read().stops.len() > 1;
		rect()
			.horizontal()
			.cross_align(Alignment::center())
			.spacing(6.)
			.child(
				NumberBox::new(
					WritableSetter::new(stop.position, move |p: f64| {
						let mut g = pos_value.peek().clone();
						g.set_position(i, p);
						pos_value.set(g);
					}),
					NumberSpec::linear(0., 1., 0.01),
				)
				.width(Size::px(64.))
				.test_id(format!("StopPosition:{i}")),
			)
			.child(
				ColorButton::new(WritableSetter::new(stop.color, move |c: Rgb| {
					let mut g = color_value.peek().clone();
					g.set_color(i, c);
					color_value.set(g);
				}))
				.test_id(format!("StopColor:{i}")),
			)
			.child(Input::new(hex).compact().width(Size::px(80.)).on_submit(move |text: String| {
				match Rgb::from_hex(&text) {
					Some(c) => {
						let mut g = hex_value.peek().clone();
						g.set_color(i, c);
						hex_value.set(g);
					}
					None => hex.set(stop.color.to_hex()),
				}
			}))
			.child(
				Button::new()
					.flat()
					.compact()
					.enabled(several)
					.on_press(move |_| {
						let mut g = remove_value.peek().clone();
						if g.remove(i) {
							remove_value.set(g);
						}
					})
					.child(label().text("Remove")),
			)
			.into_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bw() -> Gradient {
		Gradient::new(vec![
			GradientStop { position: 1., color: Rgb::new(255, 255, 255) },
			GradientStop { position: 0., color: Rgb::new(0, 0, 0) },
		])
	}

	#[test]
	fn stops_sort_and_interpolate() {
		let g = bw();
		assert_eq!(g.stops[0].position, 0.);
		assert_eq!(g.color_at(0.5), Rgb::new(128, 128, 128));
		assert_eq!(g.color_at(-1.), Rgb::new(0, 0, 0));
		assert_eq!(g.color_at(2.), Rgb::new(255, 255, 255));
		assert_eq!(Gradient::default().color_at(0.3), Rgb::default());
	}

	#[test]
	fn add_goes_into_the_widest_gap() {
		let mut g = bw();
		assert_eq!(g.widest_gap(), 0.5);
		assert_eq!(g.add_in_widest_gap(), 1);
		assert_eq!(g.stops[1].position, 0.5);
		assert_eq!(g.stops[1].color, Rgb::new(128, 128, 128));
		let i = g.add_in_widest_gap();
		assert_eq!((i, g.stops[i].position), (1, 0.25));
		let one = Gradient::new(vec![GradientStop { position: 0.2, color: Rgb::default() }]);
		assert_eq!(one.widest_gap(), 0.6);
	}

	#[test]
	fn move_remove_and_hit() {
		let mut g = bw();
		g.insert(0.5, Rgb::new(255, 0, 0));
		assert_eq!(g.set_position(1, 0.9), 1);
		assert_eq!(g.set_position(1, 1.5), 2);
		assert_eq!(g.stops[2].color, Rgb::new(255, 0, 0));
		assert_eq!(g.stop_near(0.98, 0.05), Some(2));
		assert_eq!(g.stop_near(0.5, 0.05), None);
		assert!(g.remove(0));
		assert!(g.remove(0));
		assert!(!g.remove(0));
		assert_eq!(g.stops.len(), 1);
	}
}
