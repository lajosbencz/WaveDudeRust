//! One preview picture with its header strip (docs/ui/40-preview-pane.md): the picture sized by the canvas and
//! fitted into its cell, the edge line, the handle overlay, the graph boxes and the busy dim (the picture's own).
//! The picture changes only when a frame for its position arrives; until then the last one stays.
//!
//! Coordinates: the cell in logical pixels, mapped to canvas pixels by [`CanvasTransform`].
//! Threads: the UI thread.

use std::cell::RefCell;
use std::rc::Rc;

use freya::engine::prelude::Canvas;
use freya::prelude::*;
use serde_json::json;
use wave_core::model::{Edges, Layer, Scene};
use wave_widgets::header_strip::strip_beside;
use wave_widgets::picture::{CanvasTransform, HitTestHook, OverlayHook, PictureEvent, PictureExtent, View, WheelAt};
use wave_widgets::{Callback, CanvasImage, HeaderStrip, StripOrientation, TestIdExt, use_wave_theme};

use super::handles::OverlayModel;
use super::overlay::{self, DotCache, OverlayFrame};
use super::{PaneStates, auto_brightness, graph_boxes, layer_key, layer_name, open_picture_menu};
use crate::ui::shared::{Shared, Topics};
use wave_app_state::Event as AppEvent;
use wave_app_state::commands::{Command, Target};

/// The margin between a cell's edge and its picture, logical pixels.
pub const MARGIN: f32 = 4.;

/// The edge line's colour: pure blue for a damping rim outside the canvas (it wins over a wall), pure red for a
/// reflective wall, none for open edges or a damping band inside the canvas.
pub fn edge_color(scene: &Scene) -> Option<Color> {
	if scene.has_rim() {
		Some(Color::from_rgb(0, 0, 255))
	} else if scene.edges == Edges::Reflective {
		Some(Color::from_rgb(255, 0, 0))
	} else {
		None
	}
}

/// The automation id of a layer's picture.
pub fn picture_id(layer: Layer) -> &'static str {
	match layer {
		Layer::Field => "PreviewFieldImage",
		Layer::Intensity => "PreviewIntensityImage",
	}
}

/// What one picture's overlay draws and its handlers act on, kept in a state the stable hooks read.
#[derive(Clone)]
struct OverlayInputs {
	model: Rc<OverlayModel>,
	scale: f32,
	focused: Option<usize>,
	marquee: Option<((f32, f32), (f32, f32))>,
	accent: Color,
}

impl PartialEq for OverlayInputs {
	fn eq(&self, other: &Self) -> bool {
		Rc::ptr_eq(&self.model, &other.model)
			&& self.scale == other.scale
			&& self.focused == other.focused
			&& self.marquee == other.marquee
			&& self.accent == other.accent
	}
}

/// The picture's hooks, made once so the picture compares equal while its inputs stay.
#[derive(Clone)]
struct Hooks {
	draw: OverlayHook,
	hit: HitTestHook,
	on_event: Callback<PictureEvent>,
	on_key: Callback<Event<KeyboardEventData>>,
	on_extent: Callback<PictureExtent>,
	on_wheel: Callback<WheelAt>,
}

fn hooks(inputs: State<OverlayInputs>, states: PaneStates, layer: Layer, canvas: State<(u32, u32)>) -> Hooks {
	let shared = Shared::get();
	let dots = Rc::new(DotCache::default());
	let key_shared = shared.clone();
	let mut extents = states.extents;
	let mut view = states.view;
	Hooks {
		draw: OverlayHook::from(move |canvas: &Canvas, t: &CanvasTransform, _: Option<(f32, f32)>| {
			let i = inputs.peek();
			let frame =
				OverlayFrame { model: &i.model, focused: i.focused, marquee: i.marquee, accent: i.accent, dots: &dots };
			overlay::draw(canvas, t, &frame);
		}),
		hit: HitTestHook::from(move |t: &CanvasTransform, p: (f32, f32)| {
			let c = t.to_canvas(p.0, p.1);
			super::handles::hit(&inputs.peek().model, c, t.scale).map(|i| i as u64)
		}),
		on_event: Callback::from(move |e: PictureEvent| {
			let i = inputs.peek().clone();
			overlay::on_pointer(&shared, states, layer, &i.model, i.scale, e);
		}),
		on_key: Callback::from(move |e: Event<KeyboardEventData>| {
			let model = inputs.peek().model.clone();
			overlay::on_key(&key_shared, states, layer, &model, &e);
		}),
		on_extent: Callback::from(move |extent: PictureExtent| extents.write()[layer as usize] = extent),
		// A notch up zooms in, down zooms out, never past the fit; the point under the pointer stays put.
		on_wheel: Callback::from(move |w: WheelAt| {
			let factor = if w.delta < 0. { View::STEP } else { 1. / View::STEP };
			let canvas = *canvas.peek();
			let current = *view.peek();
			let next = current.zoomed(factor, w.canvas, canvas);
			if next != current {
				view.set(next);
			}
		}),
	}
}

/// The picture of one layer.
#[derive(Clone, PartialEq)]
pub struct PictureCell {
	pub layer: Layer,
	pub states: PaneStates,
}

impl Component for PictureCell {
	fn render(&self) -> impl IntoElement {
		let shared = Shared::get();
		let theme = use_wave_theme();
		let mut cell = use_state(|| (0f32, 0f32));
		let mut area = use_state(|| (0f32, 0f32));
		let layer = self.layer;
		let states = self.states;
		let topics = Topics::DOCUMENT | Topics::SELECTION | Topics::PLAYBACK | Topics::RUN | Topics::SETTINGS;
		let (canvas, edge, title, caption, hint, brightness) = shared.read(topics, |app| {
			let p = &app.document.project;
			let s = &app.strings;
			let title = match layer {
				Layer::Field => s.get("SectionField"),
				Layer::Intensity => s.get("SectionIntensity"),
			};
			(
				p.canvas_size(),
				edge_color(&p.scene),
				title,
				s.label("DisplayLayer", "Brightness"),
				s.hint("DisplayLayer", "Brightness"),
				p.display.layer(layer).brightness,
			)
		});
		let beside = strip_beside(*cell.read(), canvas);
		let (aw, ah) = *area.read();
		let mut canvas_state = use_state(|| canvas);
		canvas_state.set_if_modified(canvas);
		let view = states.view.read().normalised(canvas);
		let t = CanvasTransform::view(aw, ah, canvas, MARGIN, view);

		// The handles are rebuilt only when the document, the selection or the scale changed.
		let revisions = shared.revisions();
		let model_key = (revisions.document, revisions.selection, t.scale.to_bits());
		let built = use_hook(|| Rc::new(RefCell::new(None::<((u64, u64, u32), Rc<OverlayModel>)>)));
		let model = {
			let mut built = built.borrow_mut();
			match &*built {
				Some((k, m)) if *k == model_key => m.clone(),
				_ => {
					let m = Rc::new(shared.peek(|app| overlay::model_of(app, layer, t.scale)));
					*built = Some((model_key, m.clone()));
					m
				}
			}
		};

		let (image, image_window) = match states.pictures.read()[layer as usize].clone() {
			Some((image, window)) => (Some(image), window),
			None => (states.scene_image.read().clone(), None),
		};
		let focused = states.focus.read()[layer as usize]
			.and_then(|(item, kind)| model.handles.iter().position(|h| h.item == item && h.kind == kind));
		let marquee = match &*states.grab.read() {
			Some(overlay::Grab::Marquee { layer: l, a, b, .. }) if *l == layer => Some((*a, *b)),
			_ => None,
		};
		let busy = *states.busy.read();

		let next = OverlayInputs { model, scale: t.scale, focused, marquee, accent: theme.accent };
		let mut inputs = use_state(|| next.clone());
		let mut redraw = use_state(|| 0u64);
		if *inputs.peek() != next {
			inputs.set(next.clone());
			*redraw.write() += 1;
		}
		let hooks = use_hook(|| hooks(inputs, states, layer, canvas_state));
		let picture = CanvasImage::new(image, canvas)
			.image_window(image_window)
			.view(view)
			.margin(MARGIN)
			.edge(edge)
			.busy(busy)
			.overlay(hooks.draw)
			.redraw_key(*redraw.peek())
			.hit_test(hooks.hit)
			.on_event(hooks.on_event)
			.on_key(hooks.on_key)
			.on_extent(hooks.on_extent)
			.on_wheel(hooks.on_wheel)
			.test_id(picture_id(layer));

		let boxes = graph_boxes::box_elements(&shared, states, layer, &t);
		let picture_area = rect()
			.width(Size::flex(1.))
			.height(Size::flex(1.))
			.on_sized(move |e: Event<SizedEventData>| area.set_if_modified((e.area.width(), e.area.height())))
			.child(picture)
			.children(boxes);

		let value_shared = shared.clone();
		let path = format!("{}.brightness", layer_key(layer));
		let auto_shared = shared.clone();
		let strip = HeaderStrip::new(
			title,
			WritableSetter::new(brightness, move |v: f64| {
				value_shared.send(AppEvent::Command(Command::Set {
					target: Target::Display,
					path: path.clone(),
					value: json!(v),
				}));
			}),
			move |_| auto_brightness(&auto_shared, states, layer),
		)
		.caption(caption)
		.tooltip(hint)
		.orientation(if beside { StripOrientation::Vertical } else { StripOrientation::Horizontal })
		.test_id(picture_id(layer));
		let menu_shared = shared.clone();
		let strip = rect()
			.maybe(!beside, |r| r.width(Size::fill()))
			.maybe(beside, |r| r.height(Size::fill()))
			.on_pointer_down(move |e: Event<PointerEventData>| {
				if e.button() == Some(MouseButton::Right) {
					let g = e.global_location();
					open_picture_menu(&menu_shared, states, layer, (g.x, g.y));
				}
			})
			.child(strip);

		rect()
			.width(Size::fill())
			.height(Size::fill())
			.content(Content::flex())
			.maybe(beside, |r| r.horizontal())
			.on_sized(move |e: Event<SizedEventData>| cell.set_if_modified((e.area.width(), e.area.height())))
			.child(strip)
			.child(picture_area)
			.test_id(format!("PreviewCell:{}", layer_name(layer)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_edge_line_names_what_a_wave_meets() {
		let mut s = Scene { damping_edge: -32, edges: Edges::Reflective, ..Scene::default() };
		assert_eq!(edge_color(&s), Some(Color::from_rgb(0, 0, 255)), "a rim wins over a wall");
		s.damping_edge = 0;
		assert_eq!(edge_color(&s), Some(Color::from_rgb(255, 0, 0)));
		s.damping_edge = 20;
		assert_eq!(edge_color(&s), Some(Color::from_rgb(255, 0, 0)), "a band inside keeps the wall");
		s.edges = Edges::Open;
		assert_eq!(edge_color(&s), None);
	}

	#[test]
	fn canvas_to_screen_round_trips_through_the_fit() {
		let t = CanvasTransform::fit(808., 408., (1600, 800), MARGIN);
		assert_eq!(t.scale, 0.5);
		let (sx, sy) = t.to_screen(100.5, 20.5);
		assert_eq!((sx, sy), (54.25, 14.25));
		assert_eq!(t.to_canvas(sx, sy), (100.5, 20.5));
		assert!(t.contains(sx, sy));
	}
}
