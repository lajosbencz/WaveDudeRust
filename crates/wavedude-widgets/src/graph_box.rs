//! The probe graph box: the shapes of [`box_shapes`] (a translucent box with the title and a readout across its top,
//! value labels left of the plot and step (or position) labels under it, a light grid, the curve clipped to the plot
//! and a resize grip) and the box's drag, resize and keys. The caller lays the graph out for the box's size and font;
//! the text keeps its size. Fixed author colours, not themed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use freya::engine::prelude::SkParagraph;
use freya::prelude::*;
use wavedude_view::draw::TextMetrics;
use wavedude_view::graph::{FONT_SIZE, GraphLayout, box_shapes};

use crate::paint::{DeviceGrid, DeviceGridExt, ElementFonts, draw_shapes, rgba_of, text_line, text_width};
use crate::picture::Callback;
use crate::theme::use_wave_theme;
use crate::tooltip::Tip;
use crate::util::{TestIdExt, focus_border, shift};

type Layouter = dyn Fn(&dyn TextMetrics, f32, f32) -> GraphLayout;

/// What a graph box shows: its title and its layout for a box size.
#[derive(Clone)]
pub struct GraphData {
	pub title: String,
	layout: Rc<Layouter>,
}

impl GraphData {
	/// `layout` lays the graph out for a box size; the box is drawn at the size the layout returns
	/// ([`GraphLayout::width`], [`GraphLayout::height`]), so it returns the size it was given.
	pub fn new(title: impl Into<String>, layout: impl Fn(&dyn TextMetrics, f32, f32) -> GraphLayout + 'static) -> Self {
		Self { title: title.into(), layout: Rc::new(layout) }
	}

	/// The graph in a box `width` x `height`.
	pub fn layout(&self, text: &dyn TextMetrics, width: f32, height: f32) -> GraphLayout {
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

/// The box text's metrics in logical pixels: paragraphs in the element's fonts at [`FONT_SIZE`] in device pixels,
/// each text measured once a frame.
struct BoxText<'a> {
	fonts: ElementFonts<'a>,
	scale: f32,
	line_height: f32,
	widths: RefCell<HashMap<String, f32>>,
}

impl<'a> BoxText<'a> {
	fn new(fonts: ElementFonts<'a>, scale: f32) -> Self {
		let mut text = Self { fonts, scale, line_height: 0., widths: RefCell::default() };
		text.line_height = text.line("0").height() / scale;
		text
	}

	fn line(&self, text: &str) -> SkParagraph {
		let mut collection = self.fonts.collection.clone();
		text_line(&mut collection, self.fonts.families, text, FONT_SIZE * self.scale, Color::BLACK)
	}
}

impl TextMetrics for BoxText<'_> {
	fn line_height(&self) -> f32 {
		self.line_height
	}

	fn width(&self, text: &str) -> f32 {
		if let Some(&w) = self.widths.borrow().get(text) {
			return w;
		}
		let w = text_width(&self.line(text)) / self.scale;
		self.widths.borrow_mut().insert(text.to_owned(), w);
		w
	}
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
		let selected = self.selected;
		let focus_ring = (focus() == Focus::Keyboard).then_some(theme.accent);

		let painter = RenderCallback::new(move |ctx: &mut FillRenderContext<'_>| {
			let grid = DeviceGrid::from_ctx(ctx);
			let families = ctx.text_style_state.font_families.clone();
			let RenderTarget::Background { canvas, font_collection } = &mut ctx.target else { return };
			let text = BoxText::new(ElementFonts { collection: font_collection, families: &families }, grid.scale);
			let g = data.layout(&text, w, h);
			let shapes = box_shapes(&g, grid, selected, focus_ring.map(rgba_of), &text);
			grid.enter(canvas);
			draw_shapes(canvas, &shapes, Some(&text.fonts), None);
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

	struct Fixed;

	impl TextMetrics for Fixed {
		fn line_height(&self) -> f32 {
			12.
		}

		fn width(&self, text: &str) -> f32 {
			text.len() as f32
		}
	}

	#[test]
	fn data_compares_by_its_layout() {
		let a = GraphData::new("Probe 1", |_, _, _| GraphLayout::default());
		let b = GraphData::new("Probe 1", |_, _, _| GraphLayout::default());
		assert_eq!(a, a.clone());
		assert_ne!(a, b);
		let laid = a.layout(&Fixed, 200., 100.);
		assert!(!laid.has_plot);
	}
}
