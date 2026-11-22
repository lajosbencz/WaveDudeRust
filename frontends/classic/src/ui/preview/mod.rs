//! The preview pane (docs/ui/40-preview-pane.md): the pictures of the frame shown ([`Pictures`]), the timeline, the
//! transport bar and the pane's playback keys.

mod graph_boxes;
mod header_strip;
mod overlay;
mod picture;
mod transport;

use eframe::egui::{self, Id, Key, Modifiers, Response, Ui, vec2};
use eframe::egui_wgpu::RenderState;
use waveduderust::model::{ItemRef, Layer};
use waveduderust::unstable::view::overlay::HandleKind;
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::menus::{MenuAction, On};
use waveduderust_app::preview::graph_boxes::BoxDrag;
use waveduderust_app::preview::overlay::Grab;
use waveduderust_app::preview::view::View;
use waveduderust_app::preview::{POLL, pane_event};
use waveduderust_app::settings::Orientation;
use waveduderust_egui_platform::keys::PANE_KEYS;
use waveduderust_egui_preview::graph_boxes::GraphBoxes;
use waveduderust_egui_preview::overlay::Overlays;
use waveduderust_egui_preview::pictures::Pictures;
use waveduderust_egui_preview::timeline::{self, Timeline, TimelineColors};

use self::transport::Transport;
use crate::kit::split::Split;
use crate::kit::{ClassicUi, Palette};

/// The margin around the pictures.
const PICTURES_MARGIN: f32 = 4.;
/// The splitter's least share either side.
const SPLIT_LIMIT: f32 = 0.05;
/// The timeline's padding: horizontal, vertical.
const TIMELINE_PADDING: (f32, f32) = (4., 2.);
/// The timeline and the transport bar's height below the pictures.
const HEIGHT_BELOW: f32 = timeline::HEIGHT + 2. * TIMELINE_PADDING.1 + transport::HEIGHT;

/// The pane's state between frames.
pub(super) struct Preview {
	pictures: Pictures,
	/// The zoom over the fit, shared by both pictures.
	view: View,
	/// Each header strip's brightness box focus.
	box_focus: [header_strip::BoxFocus; 2],
	overlays: Overlays,
	boxes: GraphBoxes,
	/// A press in progress on a picture, with its layer.
	grab: Option<(Layer, Grab)>,
	/// Each picture's keyboard-focused handle.
	focus: [Option<(ItemRef, HandleKind)>; 2],
	box_drag: Option<BoxDrag>,
	/// A box, grip or the timeline focused by a press, which shows no focus ring.
	pointer_focus: Option<Id>,
	timeline: Timeline,
	transport: Transport,
}

impl Preview {
	pub(super) fn new(gpu: Option<RenderState>) -> Self {
		Self {
			pictures: Pictures::new(gpu),
			view: View::default(),
			box_focus: [header_strip::BoxFocus::Idle; 2],
			overlays: Overlays::default(),
			boxes: GraphBoxes::default(),
			grab: None,
			focus: [None; 2],
			box_drag: None,
			pointer_focus: None,
			timeline: Timeline::default(),
			transport: Transport::default(),
		}
	}

	/// The pane in `ui`'s room; the events it asks the state for go to `out`, a context menu entry for the window
	/// is returned with what it applies to.
	pub(super) fn ui(
		&mut self,
		ui: &mut ClassicUi<'_>,
		app: &mut App,
		out: &mut Vec<Event>,
	) -> Option<(MenuAction, On)> {
		ui.data_mut(|d| d.insert_temp(holder(), false));
		self.pictures.poll(app, out);
		if self.pictures.due(app) {
			ui.ctx().request_repaint_after(POLL);
		}
		let pane = ui.max_rect();
		let below = HEIGHT_BELOW.min(pane.height());
		let (pictures, below) = pane.split_top_bottom_at_y(pane.bottom() - below);
		let (timeline, bar) = below.split_top_bottom_at_y(below.bottom() - transport::HEIGHT);
		let room = pictures.shrink(PICTURES_MARGIN);
		let display = &app.document.view;
		let shown = [display.field.preview, display.intensity.preview];
		let vertical = app.settings.preview_orientation == Orientation::Vertical;
		let mut picked = None;
		let extents = self.pictures.extents();
		match shown {
			[true, true] => {
				let split = Split::new("PreviewSplitter", app.strings.get("TipPreviewSplitter"))
					.stacked(vertical)
					.limit(SPLIT_LIMIT)
					.show(ui, room);
				hold(&split.response);
				let [first, second] = split.cells;
				self.cell(ui, first, Layer::Field, app, out, &mut picked);
				self.cell(ui, second, Layer::Intensity, app, out, &mut picked);
			}
			[true, false] => self.cell(ui, room, Layer::Field, app, out, &mut picked),
			[false, true] => self.cell(ui, room, Layer::Intensity, app, out, &mut picked),
			[false, false] => {}
		}
		// the poll above asked for the old extents
		if self.pictures.extents() != extents {
			ui.ctx().request_repaint();
		}
		let timeline = timeline.shrink2(vec2(TIMELINE_PADDING.0, TIMELINE_PADDING.1));
		let palette = Palette::of(ui.ctx().theme());
		let colors = TimelineColors {
			background: palette.control_background,
			accent: palette.accent,
			border: palette.border,
			tick: palette.timeline_tick,
			label: palette.text_disabled,
			text: palette.text,
		};
		hold(&self.timeline.show(ui, timeline, app, &mut self.pointer_focus, colors, out));
		self.transport.ui(ui, bar, app, out);
		pane_keys(ui, app, out);
		picked
	}
}

/// The pane's keys ([`PANE_KEYS`]) with the focus anywhere in it but in a text box.
fn pane_keys(ui: &Ui, app: &App, out: &mut Vec<Event>) {
	let ctx = ui.ctx();
	if !ui.data(|d| d.get_temp(holder())).unwrap_or(false) || ctx.text_edit_focused() {
		return;
	}
	ctx.input_mut(|i| {
		i.events.retain(|e| {
			let egui::Event::Key { key, pressed: true, modifiers, .. } = e else { return true };
			if !(modifiers.is_none() || *modifiers == Modifiers::SHIFT) {
				return true;
			}
			let Some(&(_, pane)) = PANE_KEYS.iter().find(|(k, _)| k == key) else { return true };
			out.push(pane_event(app, pane));
			false
		});
	});
}

/// The temp-data key of whether one of the pane's widgets has the focus this frame.
fn holder() -> Id {
	Id::unique("PreviewHoldsFocus")
}

/// Notes the pane holding the focus when its focusable widget `r` has it.
fn hold(r: &Response) {
	if r.has_focus() {
		r.ctx.data_mut(|d| d.insert_temp(holder(), true));
	}
}

/// Takes the keys that pressed the focused button `r`, so the pane's keys leave them.
fn take_activation_keys(r: &Response) {
	hold(r);
	if r.has_focus() {
		r.ctx.input_mut(|i| {
			i.consume_key(Modifiers::NONE, Key::Space);
			i.consume_key(Modifiers::NONE, Key::Enter);
		});
	}
}
