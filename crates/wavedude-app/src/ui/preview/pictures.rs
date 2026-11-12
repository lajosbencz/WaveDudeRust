//! One preview picture with its header strip (docs/ui/40-preview-pane.md): the picture sized by the canvas and
//! fitted into its cell, the edge line, the handle overlay, the graph boxes and the busy dim (the picture's own).
//! The picture changes only when a frame for its position arrives; until then the last one stays.
//!
//! Coordinates: the cell in logical pixels, mapped to canvas pixels by [`CanvasTransform`].
//! Threads: the UI thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use freya::engine::prelude::Canvas;
use freya::prelude::*;
use wavedude_core::model::{DisplayLayerEdit, Layer};
use wavedude_view::overlay::{Handle, HandleKind, OverlayModel, edge_color};
use wavedude_widgets::delay::use_delayed;
use wavedude_widgets::header_strip::strip_beside;
use wavedude_widgets::picture::{
	CanvasTransform, HitTestHook, OverlayHook, PictureEvent, PictureExtent, View, WheelAt,
};
use wavedude_widgets::{
	Callback, CanvasImage, ElementFonts, HeaderStrip, Painter, StripOrientation, TestIdExt, color_of, use_wave_theme,
};

use super::overlay::{self, OverlayFrame};
use super::{PaneStates, auto_brightness, display_edit, graph_boxes, layer_name, open_picture_menu};
use crate::ui::shared::{Shared, Topics};
use wavedude_app_state::Event as AppEvent;
use wavedude_app_state::commands::Command;

/// The margin between the picture and its area's edges, logical pixels: the cell's margin on the outer sides, the
/// gap to the header strip on the strip's side.
pub const MARGIN: f32 = 4.;
/// The cell's margin around the header strip, logical pixels.
const CELL_MARGIN: f32 = 4.;
/// A pointer resting on a handle this long shows the handle's tooltip.
const HOVER_TIP_DELAY: Duration = Duration::from_millis(600);

/// The tooltip of handle `h`: what it is and how to move it.
pub fn handle_tip(app: &crate::ui::shared::App, h: &Handle) -> String {
	let s = &app.strings;
	let name = h.item.name(&app.document.project).unwrap_or_default().to_owned();
	match h.kind {
		HandleKind::Source | HandleKind::PartCentre | HandleKind::ProbeA | HandleKind::ProbeB => {
			s.format("TipMarkerFormat", &[&name])
		}
		HandleKind::SourceLever | HandleKind::PartLever => s.format("TipAngleMarkerFormat", &[&name]),
		HandleKind::Grip => s.format("TipGripFormat", &[&name]),
		HandleKind::Point(i) => s.format("TipPointFormat", &[&name, &(i + 1).to_string()]),
		HandleKind::Node(i) => s.format("TipNodeFormat", &[&name, &(i + 1).to_string()]),
		HandleKind::InHandle(i) => s.format("TipInHandleFormat", &[&name, &(i + 1).to_string()]),
		HandleKind::OutHandle(i) => s.format("TipOutHandleFormat", &[&name, &(i + 1).to_string()]),
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
	let key_shared = shared.clone();
	let mut extents = states.extents;
	let mut view = states.view;
	Hooks {
		draw: OverlayHook::from(
			move |canvas: &Canvas,
			      painter: &mut Painter,
			      fonts: ElementFonts<'_>,
			      t: &CanvasTransform,
			      _: Option<(f32, f32)>| {
				let i = inputs.peek();
				let frame = OverlayFrame { model: &i.model, focused: i.focused, marquee: i.marquee, accent: i.accent };
				overlay::draw(canvas, painter, fonts, t, &frame);
			},
		),
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
		let mut area_origin = use_state(|| (0f32, 0f32));
		let layer = self.layer;
		let states = self.states;
		let (canvas, edge, title, caption, hint, brightness, graphs, auto, auto_tip) =
			shared.read(Topics::DOCUMENT | Topics::SELECTION | Topics::SETTINGS, |app| {
				let p = &app.document.project;
				let s = &app.strings;
				let title = match layer {
					Layer::Field => s.get("SectionField"),
					Layer::Intensity => s.get("SectionIntensity"),
				};
				(
					p.canvas_size(),
					edge_color(&p.scene).map(color_of),
					title,
					s.label("DisplayLayer", "Brightness"),
					s.hint("DisplayLayer", "Brightness"),
					p.display.layer(layer).brightness,
					graph_boxes::has_boxes(p, layer),
					s.get("ButtonAutoBrightness").to_owned(),
					s.get("TipAutoBrightness").to_owned(),
				)
			});
		let (cw, ch) = *cell.read();
		let beside = strip_beside((cw - 2. * CELL_MARGIN, ch - 2. * CELL_MARGIN), canvas);
		let (aw, ah) = *area.read();
		let mut canvas_state = use_state(|| canvas);
		canvas_state.set_if_modified(canvas);
		let view = states.view.read().normalised(canvas);
		let t = CanvasTransform::view(aw, ah, canvas, MARGIN, view);
		let mut pictures = consume_context::<crate::ui::app::Ui>().pictures;
		let origin = *area_origin.read();
		if pictures.peek()[layer as usize] != Some((origin, t)) {
			pictures.write()[layer as usize] = Some((origin, t));
		}

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

		// A pointer resting on a handle shows its tooltip; a move or a press hides it.
		let mut hover_tip = use_state(|| None::<(String, (f32, f32))>);
		let mut rest = use_delayed();
		let hover_shared = shared.clone();
		let mut on_hover = move |p: (f32, f32)| {
			hover_tip.set_if_modified(None);
			let shared = hover_shared.clone();
			rest.restart(HOVER_TIP_DELAY, move || {
				let model = inputs.peek().model.clone();
				let hit = super::handles::hit(&model, t.to_canvas(p.0, p.1), t.scale);
				if let Some(text) = hit.map(|i| shared.peek(|app| handle_tip(app, &model.handles[i]))) {
					hover_tip.set(Some((text, p)));
				}
			});
		};
		let mut hover_off = move || {
			rest.cancel();
			hover_tip.set_if_modified(None);
		};
		let tip_view = hover_tip.read().clone().map(|(text, (x, y))| {
			rect()
				.position(Position::new_absolute().left(x + 14.).top(y + 18.))
				.layer(freya::prelude::Layer::Overlay)
				.interactive(Interactive::No)
				.max_width(Size::px(320.))
				.padding((3., 6., 3., 6.))
				.background(theme.menu_background)
				.color(theme.text)
				.border(Border::new().fill(theme.menu_border).width(1.).alignment(BorderAlignment::Inner))
				.test_id(format!("HandleTip:{}", layer_name(layer)))
				.child(paragraph().width(Size::fill()).span(Span::new(text)))
		});
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

		let boxes = graphs.then_some(graph_boxes::GraphBoxes { layer, states, transform: t });
		let picture_area = rect()
			.width(Size::flex(1.))
			.height(Size::flex(1.))
			.on_sized(move |e: Event<SizedEventData>| {
				area.set_if_modified((e.area.width(), e.area.height()));
				area_origin.set_if_modified((e.area.min_x(), e.area.min_y()));
			})
			.on_pointer_move(move |e: Event<PointerEventData>| {
				let l = e.element_location();
				on_hover((l.x as f32, l.y as f32));
			})
			.on_pointer_down(move |_| hover_off())
			.on_pointer_leave(move |_| hover_off())
			.child(picture)
			.maybe_child(boxes)
			.maybe_child(tip_view);

		let value_shared = shared.clone();
		let auto_shared = shared.clone();
		let strip = HeaderStrip::new(
			title,
			WritableSetter::new(brightness, move |v: f64| {
				value_shared
					.send(AppEvent::Command(Command::Edit(display_edit(layer, DisplayLayerEdit::Brightness(v)))));
			}),
			move |()| auto_brightness(&auto_shared, states, layer),
		)
		.caption(caption)
		.tooltip(hint)
		.auto(auto, auto_tip)
		.box_id(states.brightness[layer as usize])
		.orientation(if beside { StripOrientation::Vertical } else { StripOrientation::Horizontal })
		.test_id(picture_id(layer));
		let menu_shared = shared.clone();
		let strip = rect()
			.maybe(!beside, |r| r.width(Size::fill()).padding((CELL_MARGIN, CELL_MARGIN, 0., CELL_MARGIN)))
			.maybe(beside, |r| r.height(Size::fill()).padding((CELL_MARGIN, 0., CELL_MARGIN, CELL_MARGIN)))
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
			.maybe(beside, ContainerWithContentExt::horizontal)
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
	fn canvas_to_screen_round_trips_through_the_fit() {
		let t = CanvasTransform::fit(808., 408., (1600, 800), MARGIN);
		assert_eq!(t.scale, 0.5);
		let (sx, sy) = t.to_screen(100.5, 20.5);
		assert_eq!((sx, sy), (54.25, 14.25));
		assert_eq!(t.to_canvas(sx, sy), (100.5, 20.5));
		assert!(t.contains(sx, sy));
	}
}
