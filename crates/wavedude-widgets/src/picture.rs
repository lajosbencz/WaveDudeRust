//! `CanvasImage`: an RGBA8 frame sized by its canvas (never by the bitmap), fitted uniformly into its cell or
//! zoomed in on a [`View`], with the edge line, an overlay drawing hook, a handle hit-test hook and a busy dim
//! with a spinner.
//!
//! Hooks and callbacks compare by identity: a parent that makes them once (in `use_hook`) and sets
//! [`CanvasImage::redraw_key`] when its overlay's inputs change keeps the picture from re-rendering with it.

use std::cell::RefCell;
use std::rc::Rc;

use freya::elements::image::ImageHandle;
use freya::engine::prelude::{Canvas, ClipOp, Paint, SkRect};
use freya::prelude::*;
use wavedude_view::overlay::edge_line;

use crate::paint::{DeviceGrid, DeviceGridExt, ElementFonts, Painter, fill, rgba_of, sk_rect};
use crate::spinner::Spinner;
use crate::theme::use_wave_theme;
use crate::util::{TestIdExt, focus_border, use_modifiers};

/// A rectangle: x, y, width, height.
pub type Rect4 = (f32, f32, f32, f32);

/// Zoom over the fit: 1 is the fit, more shows the part of the canvas around `centre` (canvas pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
	pub zoom: f32,
	pub centre: (f32, f32),
}

impl View {
	pub const MAX_ZOOM: f32 = 64.;
	/// One wheel notch.
	pub const STEP: f32 = 1.25;

	/// The fit of `canvas`.
	pub fn fit(canvas: (u32, u32)) -> Self {
		Self { zoom: 1., centre: (canvas.0 as f32 / 2., canvas.1 as f32 / 2.) }
	}

	/// Zoomed by `factor` about the canvas point `at`, which stays under the pointer; never out past the fit.
	pub fn zoomed(self, factor: f32, at: (f32, f32), canvas: (u32, u32)) -> Self {
		let zoom = (self.zoom * factor).clamp(1., Self::MAX_ZOOM);
		if zoom <= 1. {
			return Self::fit(canvas);
		}
		let k = zoom / self.zoom;
		Self { zoom, centre: (at.0 - (at.0 - self.centre.0) / k, at.1 - (at.1 - self.centre.1) / k) }
	}

	/// The fit of `canvas` while not zoomed in, so the centre is the canvas's whatever it was.
	pub fn normalised(self, canvas: (u32, u32)) -> Self {
		if self.zoom <= 1. { Self::fit(canvas) } else { self }
	}

	/// Moved by a pointer drag of `dx`, `dy` logical pixels at `scale` logical pixels per canvas pixel.
	pub fn panned(self, dx: f32, dy: f32, scale: f32) -> Self {
		if self.zoom <= 1. || scale <= 0. {
			return self;
		}
		Self { centre: (self.centre.0 - dx / scale, self.centre.1 - dy / scale), ..self }
	}
}

impl Default for View {
	fn default() -> Self {
		Self { zoom: 1., centre: (0., 0.) }
	}
}

/// A canvas (in canvas pixels) in a cell (logical pixels): fitted uniformly, centred, with a margin, or zoomed in
/// on a [`View`], when the picture is larger than the cell's area and shows the part around the view's centre,
/// held so that no area is left empty.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTransform {
	/// Logical pixels per canvas pixel.
	pub scale: f32,
	/// Top-left of the picture in the cell, logical pixels.
	pub offset_x: f32,
	pub offset_y: f32,
	pub canvas_width: u32,
	pub canvas_height: u32,
	/// The cell inside its margin, where the picture shows.
	pub area: Rect4,
}

impl CanvasTransform {
	pub fn fit(cell_width: f32, cell_height: f32, canvas: (u32, u32), margin: f32) -> Self {
		Self::view(cell_width, cell_height, canvas, margin, View::default())
	}

	pub fn view(cell_width: f32, cell_height: f32, canvas: (u32, u32), margin: f32, view: View) -> Self {
		let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
		let aw = (cell_width - 2. * margin).max(0.);
		let ah = (cell_height - 2. * margin).max(0.);
		let scale = (aw / cw).min(ah / ch) * view.zoom.max(1.);
		let (pw, ph) = (cw * scale, ch * scale);
		// Centred where the picture fits the axis; where it is larger, around the view's centre, held so the
		// picture covers the area.
		let place = |a: f32, p: f32, centre: f32| {
			if p <= a { (a - p) / 2. } else { (a / 2. - centre * scale).clamp(a - p, 0.) }
		};
		Self {
			scale,
			offset_x: margin + place(aw, pw, view.centre.0),
			offset_y: margin + place(ah, ph, view.centre.1),
			canvas_width: canvas.0,
			canvas_height: canvas.1,
			area: (margin, margin, aw, ah),
		}
	}

	/// The picture rectangle in the cell: x, y, width, height, logical pixels; larger than the area when zoomed.
	pub fn picture(&self) -> Rect4 {
		(self.offset_x, self.offset_y, self.canvas_width as f32 * self.scale, self.canvas_height as f32 * self.scale)
	}

	/// The part of the picture inside the area, logical pixels.
	pub fn visible(&self) -> Rect4 {
		let (px, py, pw, ph) = self.picture();
		let (ax, ay, aw, ah) = self.area;
		let (x0, y0) = (px.max(ax), py.max(ay));
		let (x1, y1) = ((px + pw).min(ax + aw), (py + ph).min(ay + ah));
		(x0, y0, (x1 - x0).max(0.), (y1 - y0).max(0.))
	}

	/// The part of the canvas the visible picture shows, canvas pixels; None when all of it.
	pub fn window(&self) -> Option<Rect4> {
		let (vx, vy, vw, vh) = self.visible();
		let (px, py, pw, ph) = self.picture();
		if (vx, vy, vw, vh) == (px, py, pw, ph) || self.scale <= 0. {
			return None;
		}
		let (x, y) = self.to_canvas(vx, vy);
		Some((x, y, vw / self.scale, vh / self.scale))
	}

	/// Canvas pixel coordinates (pixel centres at +0.5) to cell logical pixels.
	pub fn to_screen(&self, x: f32, y: f32) -> (f32, f32) {
		(self.offset_x + x * self.scale, self.offset_y + y * self.scale)
	}

	pub fn to_canvas(&self, x: f32, y: f32) -> (f32, f32) {
		if self.scale <= 0. {
			return (0., 0.);
		}
		((x - self.offset_x) / self.scale, (y - self.offset_y) / self.scale)
	}

	pub fn contains(&self, x: f32, y: f32) -> bool {
		let (px, py, w, h) = self.visible();
		x >= px && y >= py && x < px + w && y < py + h
	}

	/// The visible picture's size in whole device pixels at `device_scale` (device pixels per logical pixel).
	pub fn device_size(&self, device_scale: f32) -> (u32, u32) {
		let (_, _, w, h) = self.visible();
		((w * device_scale).round().max(0.) as u32, (h * device_scale).round().max(0.) as u32)
	}
}

/// What a picture reports after layout: its visible size in device pixels and the canvas part it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PictureExtent {
	pub device: (u32, u32),
	pub window: Option<Rect4>,
}

/// Draws over the picture: the canvas is in the cell's logical pixels, clipped to the picture, with the picture's
/// painter and the element's fonts; the pointer is in cell logical pixels when over the cell.
pub type OverlayFn = dyn Fn(&Canvas, &mut Painter, ElementFonts<'_>, &CanvasTransform, Option<(f32, f32)>);

#[derive(Clone)]
pub struct OverlayHook(pub Rc<OverlayFn>);

impl PartialEq for OverlayHook {
	fn eq(&self, other: &Self) -> bool {
		Rc::ptr_eq(&self.0, &other.0)
	}
}

impl<F: Fn(&Canvas, &mut Painter, ElementFonts<'_>, &CanvasTransform, Option<(f32, f32)>) + 'static> From<F>
	for OverlayHook
{
	fn from(f: F) -> Self {
		Self(Rc::new(f))
	}
}

/// Finds the handle under a cell point (logical pixels); called on presses only, never on hover moves.
pub type HitTestFn = dyn Fn(&CanvasTransform, (f32, f32)) -> Option<u64>;

#[derive(Clone)]
pub struct HitTestHook(pub Rc<HitTestFn>);

impl PartialEq for HitTestHook {
	fn eq(&self, other: &Self) -> bool {
		Rc::ptr_eq(&self.0, &other.0)
	}
}

impl<F: Fn(&CanvasTransform, (f32, f32)) -> Option<u64> + 'static> From<F> for HitTestHook {
	fn from(f: F) -> Self {
		Self(Rc::new(f))
	}
}

/// A callback compared by identity.
pub struct Callback<A>(Rc<RefCell<dyn FnMut(A)>>);

impl<A> Callback<A> {
	pub fn call(&self, arg: A) {
		(self.0.borrow_mut())(arg);
	}
}

impl<A> Clone for Callback<A> {
	fn clone(&self) -> Self {
		Self(self.0.clone())
	}
}

impl<A> PartialEq for Callback<A> {
	fn eq(&self, other: &Self) -> bool {
		Rc::ptr_eq(&self.0, &other.0)
	}
}

impl<A, F: FnMut(A) + 'static> From<F> for Callback<A> {
	fn from(f: F) -> Self {
		Self(Rc::new(RefCell::new(f)))
	}
}

/// Pointer phases reported by a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
	Down,
	Move,
	Up,
	Secondary,
}

/// A pointer event on a picture, in canvas pixels, with the handle hit at the press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PictureEvent {
	pub phase: PointerPhase,
	pub canvas: (f32, f32),
	/// The handle the press hit; carried on through the drag's moves and its release.
	pub handle: Option<u64>,
	pub modifiers: Modifiers,
	/// Window logical pixels, for context menus and pans.
	pub global: (f64, f64),
	/// The button of a press, carried through its drag.
	pub button: Option<MouseButton>,
}

/// A wheel notch over a picture: the wheel's vertical delta and the canvas point under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelAt {
	pub delta: f64,
	pub canvas: (f32, f32),
}

/// The preview picture.
#[derive(Clone, PartialEq)]
pub struct CanvasImage {
	image: Option<ImageHandle>,
	/// The canvas part the image covers; None is all of it.
	image_window: Option<Rect4>,
	canvas: (u32, u32),
	view: View,
	margin: f32,
	edge: Option<Color>,
	busy: bool,
	smooth: bool,
	overlay: Option<OverlayHook>,
	#[allow(dead_code, reason = "read by the derived PartialEq alone")]
	redraw_key: u64,
	hit_test: Option<HitTestHook>,
	on_event: Option<Callback<PictureEvent>>,
	on_extent: Option<Callback<PictureExtent>>,
	on_key: Option<Callback<Event<KeyboardEventData>>>,
	on_wheel: Option<Callback<WheelAt>>,
	track_pointer: bool,
	test_id: Option<String>,
}

impl CanvasImage {
	/// `canvas` is the scene's size in canvas pixels; the image may be any size and is fitted to it.
	pub fn new(image: Option<ImageHandle>, canvas: (u32, u32)) -> Self {
		Self {
			image,
			image_window: None,
			canvas,
			view: View::default(),
			margin: 4.,
			edge: None,
			busy: false,
			smooth: true,
			overlay: None,
			redraw_key: 0,
			hit_test: None,
			on_event: None,
			on_extent: None,
			on_key: None,
			on_wheel: None,
			track_pointer: false,
			test_id: None,
		}
	}

	/// The part of the canvas the image covers (a zoomed 16-bit picture); None is all of it.
	pub fn image_window(mut self, window: Option<Rect4>) -> Self {
		self.image_window = window;
		self
	}

	pub fn view(mut self, view: View) -> Self {
		self.view = view;
		self
	}

	/// Wheel notches over the picture, with the canvas point under the pointer.
	pub fn on_wheel(mut self, handler: impl Into<Callback<WheelAt>>) -> Self {
		self.on_wheel = Some(handler.into());
		self
	}

	pub fn margin(mut self, margin: f32) -> Self {
		self.margin = margin;
		self
	}

	/// The 1 px edge line around the picture.
	pub fn edge(mut self, color: Option<Color>) -> Self {
		self.edge = color;
		self
	}

	/// Dims the picture under a spinner; not hit-testable.
	pub fn busy(mut self, busy: bool) -> Self {
		self.busy = busy;
		self
	}

	/// Bilinear scaling (default) or nearest-neighbour.
	pub fn smooth(mut self, smooth: bool) -> Self {
		self.smooth = smooth;
		self
	}

	pub fn overlay(mut self, hook: impl Into<OverlayHook>) -> Self {
		self.overlay = Some(hook.into());
		self
	}

	/// Changes whenever what a stable overlay hook draws changes, so the picture repaints.
	pub fn redraw_key(mut self, key: u64) -> Self {
		self.redraw_key = key;
		self
	}

	pub fn hit_test(mut self, hook: impl Into<HitTestHook>) -> Self {
		self.hit_test = Some(hook.into());
		self
	}

	pub fn on_event(mut self, handler: impl Into<Callback<PictureEvent>>) -> Self {
		self.on_event = Some(handler.into());
		self
	}

	/// Called with the picture's visible device-pixel size and canvas window whenever they change (after layout,
	/// never during a render).
	pub fn on_extent(mut self, handler: impl Into<Callback<PictureExtent>>) -> Self {
		self.on_extent = Some(handler.into());
		self
	}

	/// Keys pressed while the picture has focus (arrows on handles, Space, Del).
	pub fn on_key(mut self, handler: impl Into<Callback<Event<KeyboardEventData>>>) -> Self {
		self.on_key = Some(handler.into());
		self
	}

	/// Passes the hover position to the overlay; each move then redraws. Off by default, so a resting or
	/// moving pointer costs nothing.
	pub fn track_pointer(mut self, track: bool) -> Self {
		self.track_pointer = track;
		self
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = Some(id.into());
		self
	}
}

impl Component for CanvasImage {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let modifiers = use_modifiers();
		let mut cell = use_state(|| (0f32, 0f32));
		let mut pointer = use_state(|| None::<(f32, f32)>);
		let mut pressed = use_state(|| None::<Option<u64>>);
		let (cw, ch) = *cell.read();
		let transform = CanvasTransform::view(cw, ch, self.canvas, self.margin, self.view);

		let fit = use_reactive(&(self.canvas, self.margin, self.view));
		let on_extent = use_reactive(&self.on_extent);
		let mut reported = use_state(|| None::<PictureExtent>);
		use_side_effect(move || {
			let (cw, ch) = *cell.read();
			let (canvas, margin, view) = *fit.read();
			let scale = *Platform::get().current_window().scale_factor.read() as f32;
			let t = CanvasTransform::view(cw, ch, canvas, margin, view);
			let extent = PictureExtent { device: t.device_size(scale), window: t.window() };
			if cw > 0. && *reported.peek() != Some(extent) {
				reported.set(Some(extent));
				if let Some(h) = &*on_extent.peek() {
					h.call(extent);
				}
			}
		});

		let image = self.image.clone();
		let image_window = self.image_window;
		let overlay = self.overlay.clone();
		let edge = self.edge;
		let smooth = self.smooth;
		let hover = *pointer.read();
		let painter = use_hook(|| Rc::new(RefCell::new(Painter::default())));
		let render = RenderCallback::new(move |ctx: &mut FillRenderContext<'_>| {
			let grid = DeviceGrid::from_ctx(ctx);
			let RenderTarget::Background { canvas, font_collection } = &mut ctx.target else { return };
			let fonts = ElementFonts::new(font_collection, &ctx.text_style_state.font_families);
			let mut painter = painter.borrow_mut();
			let (px, py, pw, ph) = transform.picture();
			let (vx, vy, vw, vh) = transform.visible();
			grid.enter(canvas);
			let hl = grid.hairline();
			// The visible part plus the edge line's room; a zoomed picture ends beyond the area.
			let clip = SkRect::new(
				grid.x(vx).round() - hl,
				grid.y(vy).round() - hl,
				grid.x(vx + vw).round() + hl,
				grid.y(vy + vh).round() + hl,
			);
			canvas.save();
			canvas.clip_rect(clip, ClipOp::Intersect, false);
			let full = sk_rect(&grid.rect((px, py), (px + pw, py + ph)));
			// A windowed image covers its canvas part alone, the rest of the picture stays black.
			let dst = match image_window {
				Some((wx, wy, ww, wh)) => {
					sk_rect(&grid.rect(transform.to_screen(wx, wy), transform.to_screen(wx + ww, wy + wh)))
				}
				None => full,
			};
			canvas.draw_rect(full, &fill(Color::BLACK));
			if let Some(handle) = &image {
				let one_to_one =
					handle.image.width() as f32 == dst.width() && handle.image.height() as f32 == dst.height();
				let sampling = if smooth && !one_to_one { SamplingMode::Bilinear } else { SamplingMode::Nearest };
				let _ = canvas.draw_image_rect_with_sampling_options(
					&handle.image,
					None,
					dst,
					sampling.sampling_options(),
					&Paint::default(),
				);
			}
			if let Some(color) = edge {
				painter.paint(canvas, fonts, &[edge_line(grid, (px, py, pw, ph), rgba_of(color))]);
			}
			canvas.restore();
			canvas.restore();
			if let Some(hook) = &overlay {
				canvas.save();
				canvas.clip_rect(SkRect::new(vx, vy, vx + vw, vy + vh), ClipOp::Intersect, true);
				(hook.0)(canvas, &mut painter, fonts, &transform, hover);
				canvas.restore();
			}
		});

		let mut button = use_state(|| None::<MouseButton>);
		let event = move |phase: PointerPhase, e: &Event<PointerEventData>, handle: Option<u64>| {
			let l = e.element_location();
			let g = e.global_location();
			PictureEvent {
				phase,
				canvas: transform.to_canvas(l.x as f32, l.y as f32),
				handle,
				modifiers: *modifiers.peek(),
				global: (g.x, g.y),
				button: *button.peek(),
			}
		};
		let on_down = {
			let on_event = self.on_event.clone();
			let hit = self.hit_test.clone();
			move |e: Event<PointerEventData>| {
				id.request_focus();
				let l = e.element_location();
				let handle = hit.as_ref().and_then(|h| (h.0)(&transform, (l.x as f32, l.y as f32)));
				let phase =
					if e.button() == Some(MouseButton::Right) { PointerPhase::Secondary } else { PointerPhase::Down };
				button.set(e.button());
				if phase == PointerPhase::Down {
					pressed.set(Some(handle));
				}
				if let Some(h) = &on_event {
					h.call(event(phase, &e, handle));
				}
			}
		};
		let on_wheel = {
			let on_wheel = self.on_wheel.clone();
			move |e: Event<WheelEventData>| {
				let l = e.element_location;
				if let Some(h) = &on_wheel
					&& e.delta_y != 0.
					&& transform.contains(l.x as f32, l.y as f32)
				{
					e.stop_propagation();
					h.call(WheelAt { delta: e.delta_y, canvas: transform.to_canvas(l.x as f32, l.y as f32) });
				}
			}
		};
		let track_pointer = self.track_pointer;
		let on_move = {
			let on_event = self.on_event.clone();
			move |e: Event<PointerEventData>| {
				let l = e.element_location();
				if track_pointer {
					pointer.set_if_modified(Some((l.x as f32, l.y as f32)));
				}
				if let (Some(handle), Some(h)) = (*pressed.peek(), &on_event) {
					h.call(event(PointerPhase::Move, &e, handle));
				}
			}
		};
		let on_up = {
			let on_event = self.on_event.clone();
			move |e: Event<PointerEventData>| {
				let held = *pressed.peek();
				if let Some(handle) = held {
					pressed.set(None);
					if let Some(h) = &on_event {
						h.call(event(PointerPhase::Up, &e, handle));
					}
				}
			}
		};
		let on_key = self.on_key.clone();
		let busy = self.busy;
		let (px, py, pw, ph) = transform.visible();

		let body = rect()
			.a11y_id(id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Image)
			.maybe_test_id(self.test_id.clone())
			.expanded()
			.background(render)
			.border(focus_border(focus(), theme.accent))
			.on_sized(move |e: Event<SizedEventData>| cell.set_if_modified((e.area.width(), e.area.height())))
			.on_pointer_down(on_down)
			.on_pointer_move(on_move)
			.on_pointer_leave(move |_| pointer.set_if_modified(None))
			.on_global_pointer_up(on_up)
			.on_wheel(on_wheel)
			.maybe(on_key.is_some(), |r| {
				r.on_key_down(move |e: Event<KeyboardEventData>| {
					if let Some(h) = &on_key {
						h.call(e);
					}
				})
			})
			.maybe_child(busy.then(|| {
				rect()
					.position(Position::new_absolute().left(px).top(py))
					.width(Size::px(pw))
					.height(Size::px(ph))
					.interactive(Interactive::No)
					.background(Color::from_af32rgb(0.45, 0, 0, 0))
					.center()
					.child(Spinner::new().size(32.).color(Color::WHITE))
			}));
		crate::util::modifier_tracker(modifiers, body).expanded()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fit_is_uniform_and_centred() {
		let t = CanvasTransform::fit(208., 108., (400, 100), 4.);
		assert_eq!(t.scale, 0.5);
		assert_eq!(t.picture(), (4., 29., 200., 50.));
		assert_eq!(t.to_screen(200., 50.), (104., 54.));
		assert_eq!(t.to_canvas(104., 54.), (200., 50.));
		assert!(t.contains(5., 30.));
		assert!(!t.contains(5., 20.));
		assert_eq!(t.device_size(1.5), (300, 75));
		let tall = CanvasTransform::fit(108., 208., (100, 100), 4.);
		assert_eq!(tall.picture(), (4., 54., 100., 100.));
	}

	#[test]
	fn a_zoomed_view_keeps_the_point_under_the_pointer_and_covers_the_area() {
		let canvas = (400, 200);
		let fit = View::fit(canvas);
		assert_eq!(CanvasTransform::view(208., 108., canvas, 4., fit), CanvasTransform::fit(208., 108., canvas, 4.));
		// Zoom in twice about the canvas point (200, 100): it stays where it was on screen.
		let before = CanvasTransform::view(208., 108., canvas, 4., fit);
		let screen = before.to_screen(200., 100.);
		let zoomed = fit.zoomed(2., (200., 100.), canvas);
		let after = CanvasTransform::view(208., 108., canvas, 4., zoomed);
		assert_eq!(after.scale, 1.);
		assert_eq!(after.to_screen(200., 100.), screen);
		assert_eq!(after.visible(), (4., 4., 200., 100.), "the picture fills the area");
		assert_eq!(after.window(), Some((100., 50., 200., 100.)));
		assert_eq!(after.device_size(1.), (200, 100));
		assert!(after.contains(5., 5.) && !after.contains(2., 2.));
		// Zooming out below the fit is the fit; a centre near the edge is held so the picture covers the area.
		assert_eq!(zoomed.zoomed(0.25, (0., 0.), canvas), fit);
		let edge = CanvasTransform::view(208., 108., canvas, 4., View { zoom: 2., centre: (390., 190.) });
		assert_eq!(edge.window(), Some((200., 100., 200., 100.)));
		let panned = View { zoom: 2., centre: (200., 100.) }.panned(-50., 0., 1.);
		assert_eq!(panned.centre, (250., 100.));
		assert_eq!(fit.panned(-50., 0., 1.), fit);
	}

	#[test]
	fn degenerate_cells_do_not_divide_by_zero() {
		let t = CanvasTransform::fit(0., 0., (0, 0), 4.);
		assert_eq!(t.scale, 0.);
		assert_eq!(t.to_canvas(3., 3.), (0., 0.));
		assert_eq!(t.device_size(2.), (0, 0));
	}
}
