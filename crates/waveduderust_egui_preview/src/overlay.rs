//! A picture's handle overlay (docs/ui/40-preview-pane.md, Handles and overlays, Interaction): the overlay model,
//! built again when the document, the selection or the scale changed, drawn as the view's shapes clipped to the
//! picture with the marquee ([`Overlays`]); the presses, drags and keys on the handles that change the selection and
//! edit the model, and the handles' tooltips ([`drive_pointer`], [`handle_keys`]).

use eframe::egui::{Color32, Key, Modifiers, Rect, Response, Shape, Stroke, Ui, Vec2};
use waveduderust::model::{ItemRef, Layer};
use waveduderust::unstable::view::draw::Outlines;
use waveduderust::unstable::view::overlay::{self as view, Handle, OverlayModel};
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::commands::Command;
use waveduderust_app::preview::handles::hit;
use waveduderust_app::preview::overlay::{Grab, Press, handle_tip, model_of, nudge, press};
use waveduderust_app::preview::view::CanvasTransform;
use waveduderust_app::selection::Row;

use waveduderust_egui_platform::input::{arrow_steps, pinching, touch_started};

/// The marquee's fill over the accent.
const MARQUEE_FILL: f32 = 0.15;

/// A picture's overlay model and what it was built from: the document and selection revisions and the scale.
struct Overlay {
	key: (u64, u64, u32),
	model: OverlayModel,
}

/// Each picture's overlay model, kept while what it shows stays, and the labels' halos.
pub struct Overlays {
	layers: [Option<Overlay>; 2],
	outlines: Outlines,
}

impl Default for Overlays {
	fn default() -> Self {
		Self { layers: [None, None], outlines: Outlines::new(1.) }
	}
}

impl Overlays {
	/// The overlay model of `layer` at `t`'s scale, built when what it shows changed.
	pub fn model(&mut self, app: &App, layer: Layer, t: &CanvasTransform) -> &OverlayModel {
		let r = app.revisions();
		let key = (r.document, r.selection, t.scale.to_bits());
		let slot = &mut self.layers[layer as usize];
		if slot.as_ref().is_none_or(|o| o.key != key) {
			*slot = Some(Overlay { key, model: model_of(app, layer, t.scale) });
		}
		&slot.as_ref().expect("built above").model
	}

	/// Draws the overlay of `layer` clipped to `clip`, the handle `focused` and the one under the pointer in
	/// `accent`, and the `marquee` (canvas corners).
	#[expect(clippy::too_many_arguments, reason = "what one picture's overlay is drawn with")]
	pub fn paint(
		&mut self,
		ui: &Ui,
		app: &App,
		layer: Layer,
		t: &CanvasTransform,
		clip: Rect,
		focused: Option<usize>,
		marquee: Option<((f32, f32), (f32, f32))>,
		accent: Color32,
	) {
		let ctx = ui.ctx();
		self.model(app, layer, t);
		let model = &self.layers[layer as usize].as_ref().expect("built above").model;
		self.outlines.set_pixels_per_point(ctx.pixels_per_point());
		let hovered = ui
			.rect_contains_pointer(clip)
			.then(|| ctx.pointer_hover_pos())
			.flatten()
			.and_then(|p| hit(model, t.to_canvas(p), t.scale));
		let emphasis = view::Emphasis { focused, hovered, accent };
		let shapes = ctx.fonts_mut(|f| view::shapes(model, t.scale, t.picture(), emphasis, f, &mut self.outlines));
		let painter = ui.painter().with_clip_rect(clip);
		painter.extend(shapes);
		if let Some((a, b)) = marquee {
			let r = Rect::from_two_pos(t.to_screen(a.0, a.1), t.to_screen(b.0, b.1));
			painter.rect_filled(r, 0., accent.gamma_multiply(MARQUEE_FILL));
			let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
			painter.extend(Shape::dashed_line(&corners, Stroke::new(1., accent), 4., 3.));
		}
	}
}

/// The modifiers of a press: Ctrl or Command toggles, Shift adds.
pub fn press_of(m: Modifiers) -> Press {
	Press { toggle: m.ctrl || m.command, add: m.shift }
}

/// What [`drive_pointer`] saw on a picture.
pub struct PointerOutcome {
	/// A press landed on the picture.
	pub pressed: bool,
	/// Esc cancelled the drags.
	pub escaped: bool,
	/// The handle under a resting pointer.
	pub hovered: Option<usize>,
}

/// The pointer on the picture of `layer` under `response`: a press selects and grabs or starts a marquee, a move
/// drags, a release ends, a second finger cancels the grab, Esc cancels it while it or another drag (`other`) is
/// on; the handle under a resting pointer shows its tooltip.
#[expect(clippy::too_many_arguments, reason = "one picture's input, state and outputs")]
pub fn drive_pointer(
	ui: &Ui,
	response: &Response,
	app: &mut App,
	overlays: &mut Overlays,
	grab: &mut Option<(Layer, Grab)>,
	other: bool,
	layer: Layer,
	t: &CanvasTransform,
	out: &mut Vec<Event>,
) -> PointerOutcome {
	let (pointer, modifiers, pressed, released, moving) = ui.input(|i| {
		let p = &i.pointer;
		(p.interact_pos(), i.modifiers, p.primary_pressed(), p.primary_released(), p.delta() != Vec2::ZERO)
	});
	let mut outcome = PointerOutcome { pressed: false, escaped: false, hovered: None };
	let at = pointer.map(|p| t.to_canvas(p));
	let grabbed = grab.as_ref().is_some_and(|(l, _)| *l == layer);
	if pinching(ui) || (grabbed && touch_started(ui)) {
		if grabbed && let Some((_, g)) = grab.take() {
			g.cancel(app, out);
		}
	} else if let Some(c) = at {
		if pressed && response.hovered() {
			outcome.pressed = true;
			let model = overlays.model(app, layer, t);
			if let Some(g) = press(app, model, t.scale, c, press_of(modifiers), out) {
				*grab = Some((layer, g));
			}
		} else if grabbed
			&& moving
			&& let Some((_, g)) = grab
		{
			g.drag(app, c, t.scale, modifiers.shift, out);
		}
	}
	if released && let Some((_, g)) = grab.take_if(|(l, _)| *l == layer) {
		let model = overlays.model(app, layer, t);
		g.release(app, model, t.scale, out);
	}
	if (grab.is_some() || other) && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
		outcome.escaped = true;
		if let Some((_, g)) = grab.take() {
			g.cancel(app, out);
		}
	}
	if grab.is_none()
		&& let Some(p) = response.hover_pos()
	{
		let model = overlays.model(app, layer, t);
		if let Some(i) = hit(model, t.to_canvas(p), t.scale) {
			response.clone().on_hover_text_at_pointer(handle_tip(app, &model.handles[i]));
			outcome.hovered = Some(i);
		}
	}
	outcome
}

/// Focusing a handle of `item` selects it unless it is in the selection.
pub fn focus_selects(app: &mut App, item: ItemRef) {
	if !app.selection.contains(item) {
		app.selection.select(Row::Item(item));
	}
}

/// Keys on the focused handle `h`: Del removes its item (true), arrows nudge it one or ten (Shift) a key.
pub fn handle_keys(ui: &Ui, app: &App, h: &Handle, out: &mut Vec<Event>) -> bool {
	if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
		out.push(Event::Command(Command::Remove { target: h.item }));
		return true;
	}
	let (dx, dy) = arrow_steps(ui);
	if (dx, dy) != (0, 0) {
		nudge(app, h, dx, dy, out);
	}
	false
}
