//! The probe graph box: a translucent box with the title and a readout across its top, value labels left of the
//! plot and step (or position) labels under it, a light grid, the curve clipped to the plot and a resize grip. The
//! caller lays the graph out for the box's size and font; the text keeps its size. Fixed author colours, not themed.

use std::fmt;
use std::rc::Rc;

use freya::engine::prelude::{ClipOp, PaintCap, PaintJoin, PathBuilder, SkRect};
use freya::prelude::*;

use crate::paint::{DeviceGrid, fill, stroke, text_line, text_width};
use crate::picture::Callback;
use crate::theme::use_wave_theme;
use crate::tooltip::Tip;
use crate::util::{TestIdExt, focus_border, shift};

/// One tick: its position along its axis (box pixels), its label, whether it is the zero line.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphTick {
	pub position: f32,
	pub label: String,
	pub zero: bool,
}

/// A graph laid out in its box, box-local logical pixels.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GraphLayout {
	/// False when only the box and its title fit.
	pub has_plot: bool,
	/// x, y, width, height.
	pub plot: (f32, f32, f32, f32),
	pub line_width: f32,
	/// Horizontal grid lines with their labels left of the plot.
	pub y_ticks: Vec<GraphTick>,
	/// Vertical grid lines with their labels under the plot.
	pub x_ticks: Vec<GraphTick>,
	/// One point is a dot.
	pub curve: Vec<(f32, f32)>,
	pub title: String,
	pub readout: String,
}

/// The box font's metrics, logical pixels.
pub struct TextMeasure<'a> {
	pub line_height: f32,
	pub width: &'a dyn Fn(&str) -> f32,
}

type Layouter = dyn Fn(&TextMeasure<'_>, f32, f32) -> GraphLayout;

/// What a graph box shows: its title and its layout for a box size.
#[derive(Clone)]
pub struct GraphData {
	pub title: String,
	layout: Rc<Layouter>,
}

impl GraphData {
	pub fn new(title: impl Into<String>, layout: impl Fn(&TextMeasure<'_>, f32, f32) -> GraphLayout + 'static) -> Self {
		Self { title: title.into(), layout: Rc::new(layout) }
	}

	/// The graph in a box `width` x `height`.
	pub fn layout(&self, text: &TextMeasure<'_>, width: f32, height: f32) -> GraphLayout {
		(self.layout)(text, width, height)
	}
}

impl PartialEq for GraphData {
	fn eq(&self, other: &Self) -> bool {
		self.title == other.title && Rc::ptr_eq(&self.layout, &other.layout)
	}
}

impl fmt::Debug for GraphData {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("GraphData").field("title", &self.title).finish_non_exhaustive()
	}
}

/// The text size, logical pixels.
pub const FONT_SIZE: f32 = 11.;
/// Space between the box edge and its text.
const EDGE_PAD: f32 = 3.;
/// Space between a value label and the plot.
const LABEL_GAP: f32 = 4.;
const BACKGROUND: Color = Color::from_argb(235, 14, 16, 22);
const BORDER: Color = Color::from_argb(170, 200, 200, 210);
const GRID: Color = Color::from_argb(46, 255, 255, 255);
const ZERO: Color = Color::from_argb(120, 255, 255, 255);
const FRAME: Color = Color::from_argb(130, 200, 200, 210);
const LABEL: Color = Color::from_argb(225, 225, 225, 230);
const TITLE: Color = Color::from_argb(240, 240, 240, 245);
const CURVE: Color = Color::from_rgb(255, 208, 64);
const SELECTED: Color = Color::from_rgb(0xFF, 0xE0, 0x00);

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
	on_event: Callback<GraphBoxEvent>,
	test_id: Option<String>,
	grip_tooltip: Option<String>,
}

impl GraphBox {
	pub fn new(data: GraphData, width: f32, height: f32, on_event: impl Into<Callback<GraphBoxEvent>>) -> Self {
		Self { data, width, height, selected: false, on_event: on_event.into(), test_id: None, grip_tooltip: None }
	}

	/// The tooltip of the corner grip.
	pub fn grip_tooltip(mut self, text: impl Into<String>) -> Self {
		self.grip_tooltip = Some(text.into());
		self
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
		let mut drag = use_state(|| None::<BoxDrag>);
		let on_event = self.on_event.clone();
		let (w, h) = (self.width, self.height);
		let data = self.data.clone();
		let border = if self.selected { SELECTED } else { BORDER };
		let focus_ring = (focus() == Focus::Keyboard).then_some(theme.accent);

		let painter = RenderCallback::new(move |ctx: &mut FillRenderContext<'_>| {
			let grid = DeviceGrid::from_ctx(ctx);
			let families = ctx.text_style_state.font_families.clone();
			let RenderTarget::Background { canvas, font_collection } = &mut ctx.target else { return };
			let s = grid.scale;
			let font = FONT_SIZE * s;
			let fonts = font_collection.clone();
			let line = |text: &str, color: Color| text_line(&mut fonts.clone(), &families, text, font, color);
			let width = |text: &str| text_width(&line(text, LABEL)) / s;
			let line_height = line("0", LABEL).height() / s;
			let g = data.layout(&TextMeasure { line_height, width: &width }, w, h);
			grid.enter(canvas);
			let hl = grid.hairline();
			let half = hl / 2.;
			let (bx0, by0, bx1, by1) = (grid.x(0.).floor(), grid.y(0.).floor(), grid.x(w).floor(), grid.y(h).floor());
			canvas.draw_rect(SkRect::new(bx0, by0, bx1, by1), &fill(BACKGROUND));
			let edge = focus_ring.unwrap_or(border);
			canvas.draw_rect(SkRect::new(bx0 + half, by0 + half, bx1 - half, by1 - half), &stroke(edge, hl));

			if g.has_plot {
				let (px, py, pw, ph) = g.plot;
				let (left, top, right, bottom) = (grid.x(px), grid.y(py), grid.x(px + pw), grid.y(py + ph));
				for t in &g.y_ticks {
					let y = grid.snap(grid.y(t.position));
					canvas.draw_line((left, y), (right, y), &stroke(if t.zero { ZERO } else { GRID }, hl));
					if !t.label.is_empty() {
						let p = line(&t.label, LABEL);
						let lw = text_width(&p) / s;
						let lx = EDGE_PAD.max(px - LABEL_GAP - lw);
						let ly =
							(t.position - line_height / 2.).clamp(py - line_height / 2., py + ph - line_height / 2.);
						p.paint(canvas, (grid.x(lx).round(), grid.y(ly).round()));
					}
				}
				for t in &g.x_ticks {
					let x = grid.snap(grid.x(t.position));
					canvas.draw_line((x, top), (x, bottom), &stroke(GRID, hl));
					if !t.label.is_empty() {
						let p = line(&t.label, LABEL);
						let lw = text_width(&p) / s;
						let lx = (t.position - lw / 2.).clamp(EDGE_PAD, (w - lw - EDGE_PAD).max(EDGE_PAD));
						p.paint(canvas, (grid.x(lx).round(), grid.y(py + ph + 2.).round()));
					}
				}
				canvas.draw_rect(
					SkRect::new(grid.snap(left), grid.snap(top), grid.snap(right), grid.snap(bottom)),
					&stroke(FRAME, hl),
				);
				canvas.save();
				canvas.clip_rect(SkRect::new(left, top, right, bottom), ClipOp::Intersect, true);
				match g.curve.as_slice() {
					[] => {}
					[(x, y)] => {
						let mut dot = fill(CURVE);
						dot.set_anti_alias(true);
						canvas.draw_circle((grid.x(*x), grid.y(*y)), 2. * s, &dot);
					}
					[(x0, y0), rest @ ..] => {
						let mut path = PathBuilder::new();
						path.move_to((grid.x(*x0), grid.y(*y0)));
						for (x, y) in rest {
							path.line_to((grid.x(*x), grid.y(*y)));
						}
						let mut pen = stroke(CURVE, g.line_width * s);
						pen.set_anti_alias(true);
						pen.set_stroke_join(PaintJoin::Round);
						pen.set_stroke_cap(PaintCap::Round);
						canvas.draw_path(&path.detach(), &pen);
					}
				}
				canvas.restore();
			}

			// the title strip: the readout right-aligned in the curve's colour, the title in what remains
			let mut readout_width = 0.;
			if !g.readout.is_empty() {
				let p = line(&g.readout, CURVE);
				readout_width = text_width(&p) / s;
				p.paint(canvas, (grid.x(w - readout_width - 8.).round(), grid.y(EDGE_PAD).round()));
			}
			canvas.save();
			let title_end = grid.x(EDGE_PAD + 3. + (w - readout_width - 16.).max(10.));
			canvas.clip_rect(SkRect::new(bx0, by0, title_end, by1), ClipOp::Intersect, false);
			line(&g.title, TITLE).paint(canvas, (grid.x(EDGE_PAD + 3.).round(), grid.y(EDGE_PAD).round()));
			canvas.restore();

			let dot = fill(Color::from_rgb(0x90, 0x90, 0x90));
			for k in 0..3 {
				let o = grid.width(3.) * k as f32;
				canvas.draw_rect(SkRect::new(bx1 - hl * 3. - o, by1 - hl * 3., bx1 - hl * 2. - o, by1 - hl * 2.), &dot);
				canvas.draw_rect(SkRect::new(bx1 - hl * 3., by1 - hl * 3. - o, bx1 - hl * 2., by1 - hl * 2. - o), &dot);
			}
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
						on_event.call(GraphBoxEvent::Move { dx: (p.x - x) as f32, dy: (p.y - y) as f32, done: false });
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
						on_event.call(GraphBoxEvent::Move { dx: (p.x - x) as f32, dy: (p.y - y) as f32, done: true });
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

		// The corner grip: Tab reaches it, the arrow keys resize by a pixel, ten with Shift.
		let grip_id = use_a11y();
		let grip_focus = use_focus(grip_id);
		let on_grip_key = {
			let on_event = on_event.clone();
			move |e: Event<KeyboardEventData>| {
				let n = if shift(&e) { 10. } else { 1. };
				let (dw, dh) = match e.key {
					Key::Named(NamedKey::ArrowLeft) => (-n, 0.),
					Key::Named(NamedKey::ArrowRight) => (n, 0.),
					Key::Named(NamedKey::ArrowUp) => (0., -n),
					Key::Named(NamedKey::ArrowDown) => (0., n),
					_ => return,
				};
				e.stop_propagation();
				on_event.call(GraphBoxEvent::Resize {
					width: (w + dw).max(60.),
					height: (h + dh).max(30.),
					done: true,
				});
			}
		};
		let grip = rect()
			.a11y_id(grip_id)
			.a11y_focusable(true)
			.maybe_test_id(self.test_id.as_ref().map(|t| format!("{t}:Grip")))
			.position(Position::new_absolute().right(0.).bottom(0.))
			.width(Size::px(GRIP))
			.height(Size::px(GRIP))
			.border(focus_border(grip_focus(), theme.accent))
			.cursor(CursorIcon::NwseResize)
			.on_key_down(on_grip_key);
		let grip: Element = match self.grip_tooltip.clone() {
			Some(t) => Tip::new(t).child(grip).into_element(),
			None => grip.into_element(),
		};
		rect()
			.a11y_id(id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Figure)
			.maybe_test_id(self.test_id.clone())
			.width(Size::px(w))
			.height(Size::px(h))
			.background(painter)
			.on_pointer_down(on_down)
			.on_global_pointer_move(on_global_move)
			.on_global_pointer_up(on_global_up)
			.on_key_down(on_key)
			.child(grip)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn data_compares_by_its_layout() {
		let a = GraphData::new("Probe 1", |_, _, _| GraphLayout::default());
		let b = GraphData::new("Probe 1", |_, _, _| GraphLayout::default());
		assert_eq!(a, a.clone());
		assert_ne!(a, b);
		let width = |t: &str| t.len() as f32;
		let laid = a.layout(&TextMeasure { line_height: 12., width: &width }, 200., 100.);
		assert!(!laid.has_plot);
	}
}
