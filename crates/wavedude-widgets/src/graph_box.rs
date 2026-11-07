//! The probe graph box: translucent box, title strip with a readout, axes (symmetric amplitude, linear, or
//! logarithmic decades), a polyline trace clipped to the plot, a resize grip; fixed text size.

use std::rc::Rc;

use freya::engine::prelude::{ClipOp, PathBuilder, SkRect};
use freya::prelude::*;

use crate::paint::{DeviceGrid, fill, stroke, text_line, text_width};
use crate::theme::use_wave_theme;
use crate::util::{TestIdExt, shift};

/// The value axis of a graph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueAxis {
	/// -max..max, symmetric about zero (field amplitude).
	Symmetric {
		max: f64,
	},
	Linear {
		min: f64,
		max: f64,
	},
	/// `decades` powers of ten below `top`; zero and smaller sit on the bottom line.
	Logarithmic {
		top: f64,
		decades: u32,
	},
}

impl ValueAxis {
	pub fn fit_symmetric(values: &[f32]) -> Self {
		let max = values.iter().fold(0f64, |m, v| m.max(f64::from(v.abs())));
		Self::Symmetric { max: if max > 0. { max } else { 1. } }
	}

	pub fn fit_linear(values: &[f32]) -> Self {
		let (lo, hi) = values
			.iter()
			.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(f64::from(*v)), hi.max(f64::from(*v))));
		if !lo.is_finite() || hi <= lo {
			let c = if lo.is_finite() { lo } else { 0. };
			return Self::Linear { min: c - 1., max: c + 1. };
		}
		Self::Linear { min: lo, max: hi }
	}

	/// Top at the next power of ten above the largest value.
	pub fn fit_logarithmic(values: &[f32], decades: u32) -> Self {
		let max = values.iter().fold(0f64, |m, v| m.max(f64::from(*v)));
		let top = if max > 0. { 10f64.powf(max.log10().ceil()) } else { 1. };
		Self::Logarithmic { top, decades: decades.max(1) }
	}

	/// The value as a fraction of the axis height from the bottom, clamped to 0..1.
	pub fn fraction(&self, v: f64) -> f64 {
		let f = match *self {
			Self::Symmetric { max } => 0.5 + v / (2. * max),
			Self::Linear { min, max } => (v - min) / (max - min),
			Self::Logarithmic { top, decades } => {
				if v <= 0. {
					0.
				} else {
					1. + (v / top).log10() / f64::from(decades)
				}
			}
		};
		if f.is_finite() { f.clamp(0., 1.) } else { 0. }
	}

	/// Tick values with their labels.
	pub fn ticks(&self) -> Vec<(f64, String)> {
		match *self {
			Self::Symmetric { max } => {
				let step = nice_step(max, 2);
				let mut out = Vec::new();
				let mut v = 0.;
				while v <= max * (1. + 1e-9) {
					out.push((v, format_tick(v, step)));
					if v > 0. {
						out.push((-v, format_tick(-v, step)));
					}
					v += step;
				}
				out.sort_by(|a, b| a.0.total_cmp(&b.0));
				out
			}
			Self::Linear { min, max } => {
				let step = nice_step(max - min, 4);
				let mut v = (min / step).ceil() * step;
				let mut out = Vec::new();
				while v <= max + step * 1e-9 {
					out.push((v, format_tick(v, step)));
					v += step;
				}
				out
			}
			Self::Logarithmic { top, decades } => (0..=decades)
				.map(|k| {
					let v = top / 10f64.powi(k as i32);
					(v, format!("1e{}", v.log10().round() as i32))
				})
				.collect(),
		}
	}
}

/// A 1, 2 or 5 x 10^k step giving about `count` intervals over `range`.
pub fn nice_step(range: f64, count: u32) -> f64 {
	let raw = range.abs() / f64::from(count);
	if raw <= 0. || !raw.is_finite() {
		return 1.;
	}
	let base = 10f64.powf(raw.log10().floor());
	let m = raw / base;
	base * if m <= 1. {
		1.
	} else if m <= 2. {
		2.
	} else if m <= 5. {
		5.
	} else {
		10.
	}
}

fn format_tick(v: f64, step: f64) -> String {
	let decimals = if step >= 1. { 0 } else { (-step.log10().floor()) as usize };
	let v = if v.abs() < step * 1e-9 { 0. } else { v };
	if v != 0. && (v.abs() >= 1e5 || v.abs() < 1e-3) { format!("{v:.1e}") } else { format!("{v:.decimals$}") }
}

/// Fixed metrics of a graph box in logical pixels; the text does not scale with the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphMetrics {
	pub title_height: f32,
	pub left: f32,
	pub right: f32,
	pub top: f32,
	pub bottom: f32,
	pub font_size: f32,
}

impl Default for GraphMetrics {
	fn default() -> Self {
		Self { title_height: 16., left: 40., right: 6., top: 6., bottom: 6., font_size: 10. }
	}
}

/// Mapping of a graph box: the plot rectangle in box-local logical pixels, steps along x, values along y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphLayout {
	pub width: f32,
	pub height: f32,
	pub plot: (f32, f32, f32, f32),
	pub x_range: (f64, f64),
	pub axis: ValueAxis,
}

impl GraphLayout {
	pub fn new(width: f32, height: f32, x_range: (f64, f64), axis: ValueAxis, m: GraphMetrics) -> Self {
		let x0 = m.left.min(width / 2.);
		let y0 = (m.title_height + m.top).min(height / 2.);
		let x1 = (width - m.right).max(x0 + 1.);
		let y1 = (height - m.bottom).max(y0 + 1.);
		Self { width, height, plot: (x0, y0, x1, y1), x_range, axis }
	}

	pub fn x_of(&self, x: f64) -> f32 {
		let (a, b) = self.x_range;
		let f = if b > a { (x - a) / (b - a) } else { 0. };
		self.plot.0 + (self.plot.2 - self.plot.0) * f as f32
	}

	pub fn y_of(&self, v: f64) -> f32 {
		self.plot.3 - (self.plot.3 - self.plot.1) * self.axis.fraction(v) as f32
	}

	/// The trace's points: `values[i]` at x = `x_range.0 + i * x_per_sample`.
	pub fn points(&self, values: &[f32], x_per_sample: f64) -> Vec<(f32, f32)> {
		values
			.iter()
			.enumerate()
			.map(|(i, v)| (self.x_of(self.x_range.0 + i as f64 * x_per_sample), self.y_of(f64::from(*v))))
			.collect()
	}
}

/// What a graph box shows.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphData {
	pub title: String,
	pub readout: String,
	pub readout_color: Color,
	pub values: Rc<[f32]>,
	pub x_range: (f64, f64),
	pub x_per_sample: f64,
	pub axis: ValueAxis,
}

/// What the user did on a box. Moves and sizes in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GraphBoxEvent {
	/// Total offset since the drag began, or one arrow-key nudge.
	Move {
		dx: f32,
		dy: f32,
		done: bool,
	},
	Resize {
		width: f32,
		height: f32,
		done: bool,
	},
	Delete,
	Focused,
}

const GRIP: f32 = 10.;

#[derive(Clone, Copy, PartialEq)]
enum BoxDrag {
	Move(f64, f64),
	Resize(f64, f64, f32, f32),
}

/// A probe graph box of a given size; fixed author colours, not themed.
#[derive(Clone, PartialEq)]
pub struct GraphBox {
	data: GraphData,
	width: f32,
	height: f32,
	selected: bool,
	metrics: GraphMetrics,
	on_event: EventHandler<GraphBoxEvent>,
	test_id: Option<String>,
}

impl GraphBox {
	pub fn new(data: GraphData, width: f32, height: f32, on_event: impl Into<EventHandler<GraphBoxEvent>>) -> Self {
		Self {
			data,
			width,
			height,
			selected: false,
			metrics: GraphMetrics::default(),
			on_event: on_event.into(),
			test_id: None,
		}
	}

	pub fn selected(mut self, selected: bool) -> Self {
		self.selected = selected;
		self
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = Some(id.into());
		self
	}
}

impl Component for GraphBox {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let mut hover = use_state(|| false);
		let mut drag = use_state(|| None::<BoxDrag>);
		let on_event = self.on_event.clone();
		let (w, h) = (self.width, self.height);
		let layout = GraphLayout::new(w, h, self.data.x_range, self.data.axis, self.metrics);
		let data = self.data.clone();
		let metrics = self.metrics;
		let border = if self.selected {
			Color::from_rgb(0xFF, 0xD7, 0x00)
		} else if hover() || focus().is_focused() {
			Color::from_rgb(0xC0, 0xC0, 0xC0)
		} else {
			Color::from_rgb(0x60, 0x60, 0x60)
		};
		let focus_ring = (focus() == Focus::Keyboard).then_some(theme.accent);

		let painter = RenderCallback::new(move |ctx: &mut FillRenderContext| {
			let grid = DeviceGrid::from_ctx(ctx);
			let families = ctx.text_style_state.font_families.clone();
			let RenderTarget::Background { canvas, font_collection } = &mut ctx.target else { return };
			grid.enter(canvas);
			let hl = grid.hairline();
			let font = metrics.font_size * grid.scale;
			let (bx0, by0, bx1, by1) = (grid.x(0.).floor(), grid.y(0.).floor(), grid.x(w).floor(), grid.y(h).floor());
			canvas.draw_rect(SkRect::new(bx0, by0, bx1, by1), &fill(Color::from_argb(0xC8, 0x10, 0x10, 0x14)));
			let title_bottom = grid.y(metrics.title_height).floor();
			canvas.draw_rect(SkRect::new(bx0, by0, bx1, title_bottom), &fill(Color::from_argb(0xE0, 0x20, 0x20, 0x26)));
			let text = Color::from_rgb(0xE0, 0xE0, 0xE0);
			let dim = Color::from_rgb(0x90, 0x90, 0x90);
			let readout = text_line(font_collection, &families, &data.readout, font, data.readout_color);
			let rw = text_width(&readout);
			readout.paint(canvas, ((bx1 - rw - 4.).round(), by0 + 1.));
			canvas.save();
			canvas.clip_rect(SkRect::new(bx0, by0, bx1 - rw - 8., title_bottom), ClipOp::Intersect, false);
			text_line(font_collection, &families, &data.title, font, text).paint(canvas, (bx0 + 4., by0 + 1.));
			canvas.restore();

			let (px0, py0, px1, py1) = layout.plot;
			let (dx0, dy0, dx1, dy1) =
				(grid.x(px0).floor(), grid.y(py0).floor(), grid.x(px1).floor(), grid.y(py1).floor());
			let axis_pen = stroke(Color::from_rgb(0x70, 0x70, 0x70), hl);
			let grid_pen = stroke(Color::from_argb(0x40, 0xA0, 0xA0, 0xA0), hl);
			for (v, label) in layout.axis.ticks() {
				let y = grid.snap(grid.y(layout.y_of(v)));
				canvas.draw_line((dx0, y), (dx1, y), &grid_pen);
				let p = text_line(font_collection, &families, &label, font, dim);
				let lw = text_width(&p);
				p.paint(canvas, ((dx0 - lw - 3.).round(), (y - font * 0.65).round()));
			}
			let xs = grid.snap(dx0);
			let ys = grid.snap(dy1);
			canvas.draw_line((xs, dy0), (xs, ys), &axis_pen);
			canvas.draw_line((xs, ys), (dx1, ys), &axis_pen);

			let points = layout.points(&data.values, data.x_per_sample);
			if points.len() > 1 {
				canvas.save();
				canvas.clip_rect(SkRect::new(dx0, dy0, dx1 + hl, dy1 + hl), ClipOp::Intersect, false);
				let mut path = PathBuilder::new();
				path.move_to((grid.x(points[0].0), grid.y(points[0].1)));
				for (x, y) in &points[1..] {
					path.line_to((grid.x(*x), grid.y(*y)));
				}
				let mut pen = stroke(Color::from_rgb(0x4F, 0xC3, 0xF7), hl);
				pen.set_anti_alias(true);
				canvas.draw_path(&path.detach(), &pen);
				canvas.restore();
			}

			let dot = fill(Color::from_rgb(0x90, 0x90, 0x90));
			for k in 0..3 {
				let o = grid.width(3.) * k as f32;
				let s = hl;
				canvas.draw_rect(
					SkRect::new(bx1 - hl * 2. - o - s, by1 - hl * 2. - s, bx1 - hl * 2. - o, by1 - hl * 2.),
					&dot,
				);
				canvas.draw_rect(
					SkRect::new(bx1 - hl * 2. - s, by1 - hl * 2. - o - s, bx1 - hl * 2., by1 - hl * 2. - o),
					&dot,
				);
			}
			let edge = focus_ring.unwrap_or(border);
			let half = hl / 2.;
			canvas.draw_rect(SkRect::new(bx0 + half, by0 + half, bx1 - half, by1 - half), &stroke(edge, hl));
			canvas.restore();
		});

		let on_down = move |e: Event<PointerEventData>| {
			if e.button() != Some(MouseButton::Left) {
				return;
			}
			e.stop_propagation();
			id.request_focus();
			let p = e.global_location();
			let l = e.element_location();
			let on_grip = l.x as f32 >= w - GRIP && l.y as f32 >= h - GRIP;
			drag.set(Some(if on_grip { BoxDrag::Resize(p.x, p.y, w, h) } else { BoxDrag::Move(p.x, p.y) }));
		};
		let on_global_move = {
			let on_event = on_event.clone();
			move |e: Event<PointerEventData>| {
				let p = e.global_location();
				match *drag.peek() {
					Some(BoxDrag::Move(x, y)) => {
						on_event.call(GraphBoxEvent::Move { dx: (p.x - x) as f32, dy: (p.y - y) as f32, done: false })
					}
					Some(BoxDrag::Resize(x, y, w0, h0)) => on_event.call(GraphBoxEvent::Resize {
						width: (w0 + (p.x - x) as f32).max(60.),
						height: (h0 + (p.y - y) as f32).max(30.),
						done: false,
					}),
					None => {}
				}
			}
		};
		let on_global_up = {
			let on_event = on_event.clone();
			move |e: Event<PointerEventData>| {
				let p = e.global_location();
				match drag.peek().as_ref().copied() {
					Some(BoxDrag::Move(x, y)) => {
						on_event.call(GraphBoxEvent::Move { dx: (p.x - x) as f32, dy: (p.y - y) as f32, done: true })
					}
					Some(BoxDrag::Resize(x, y, w0, h0)) => on_event.call(GraphBoxEvent::Resize {
						width: (w0 + (p.x - x) as f32).max(60.),
						height: (h0 + (p.y - y) as f32).max(30.),
						done: true,
					}),
					None => return,
				}
				drag.set(None);
			}
		};
		let on_key = {
			let on_event = on_event.clone();
			move |e: Event<KeyboardEventData>| {
				let n = if shift(&e) { 10. } else { 1. };
				let (dx, dy) = match e.key {
					Key::Named(NamedKey::ArrowLeft) => (-n, 0.),
					Key::Named(NamedKey::ArrowRight) => (n, 0.),
					Key::Named(NamedKey::ArrowUp) => (0., -n),
					Key::Named(NamedKey::ArrowDown) => (0., n),
					Key::Named(NamedKey::Delete) => {
						e.stop_propagation();
						on_event.call(GraphBoxEvent::Delete);
						return;
					}
					_ => return,
				};
				e.stop_propagation();
				on_event.call(GraphBoxEvent::Move { dx, dy, done: true });
			}
		};
		let mut focused_before = use_state(|| false);
		let now = focus().is_focused();
		if *focused_before.peek() != now {
			focused_before.set(now);
			if now {
				on_event.call(GraphBoxEvent::Focused);
			}
		}

		rect()
			.a11y_id(id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Figure)
			.maybe_test_id(self.test_id.clone())
			.width(Size::px(w))
			.height(Size::px(h))
			.background(painter)
			.on_pointer_enter(move |_| hover.set(true))
			.on_pointer_leave(move |_| hover.set(false))
			.on_pointer_down(on_down)
			.on_global_pointer_move(on_global_move)
			.on_global_pointer_up(on_global_up)
			.on_key_down(on_key)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn nice_steps() {
		assert_eq!(nice_step(10., 4), 5.);
		assert_eq!(nice_step(1., 4), 0.5);
		assert_eq!(nice_step(0.03, 2), 0.02);
		assert_eq!(nice_step(0., 4), 1.);
	}

	#[test]
	fn symmetric_axis_is_centred_on_zero() {
		let a = ValueAxis::fit_symmetric(&[0.2, -0.8, 0.5]);
		assert_eq!(a, ValueAxis::Symmetric { max: 0.800000011920929 });
		assert_eq!(a.fraction(0.), 0.5);
		let ticks = ValueAxis::Symmetric { max: 1. }.ticks();
		assert_eq!(ticks.iter().map(|t| t.0).collect::<Vec<_>>(), vec![-1., -0.5, 0., 0.5, 1.]);
		assert_eq!(ticks[0].1, "-1.0");
		assert_eq!(ValueAxis::fit_symmetric(&[]), ValueAxis::Symmetric { max: 1. });
	}

	#[test]
	fn logarithmic_axis_by_decades() {
		let a = ValueAxis::fit_logarithmic(&[0., 0.003, 42.], 3);
		assert_eq!(a, ValueAxis::Logarithmic { top: 100., decades: 3 });
		assert_eq!(a.fraction(100.), 1.);
		assert!((a.fraction(1.) - 1. / 3.).abs() < 1e-12);
		assert_eq!(a.fraction(0.), 0.);
		assert_eq!(a.fraction(1e-9), 0.);
		let labels: Vec<String> = a.ticks().into_iter().map(|t| t.1).collect();
		assert_eq!(labels, vec!["1e2", "1e1", "1e0", "1e-1"]);
	}

	#[test]
	fn layout_maps_steps_and_values_into_the_plot() {
		let m = GraphMetrics::default();
		let l = GraphLayout::new(200., 100., (0., 100.), ValueAxis::Linear { min: 0., max: 10. }, m);
		assert_eq!(l.plot, (40., 22., 194., 94.));
		assert_eq!(l.x_of(0.), 40.);
		assert_eq!(l.x_of(100.), 194.);
		assert_eq!(l.y_of(0.), 94.);
		assert_eq!(l.y_of(10.), 22.);
		assert_eq!(l.y_of(20.), 22.);
		assert_eq!(l.points(&[0., 10.], 100.), vec![(40., 94.), (194., 22.)]);
		let fit = ValueAxis::fit_linear(&[3., 3.]);
		assert_eq!(fit, ValueAxis::Linear { min: 2., max: 4. });
		assert_eq!(
			ValueAxis::Linear { min: 0., max: 10. }.ticks().iter().map(|t| t.0).collect::<Vec<_>>(),
			vec![0., 5., 10.]
		);
	}
}
