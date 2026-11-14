//! The preview pane (docs/ui/40-preview-pane.md): the pictures of the frame shown in their cells with the header
//! strips and the splitter between them, the timeline and the transport bar below, and the pane's playback keys.
//!
//! Each frame the pane polls: it hands the frame the live controller shows to the presenter when what is to be drawn
//! changed, writes the presenter's pictures into their textures, keeps the busy dim, finishes an Auto brightness and
//! follows the scene's own picture and outline; while a picture is due it asks for the next frame after [`POLL`].
//! Where the renderer's device draws pictures, it colours them ([`device`]), bypassing the presenter: those of a run
//! on the GPU engine, and every frame where the presenter would colour them on the UI thread (the browser).
//!
//! Threads: the UI thread; natively the presenter and the scene builder use threads of their own.

mod device;
mod graph_boxes;
mod header_strip;
mod overlay;
mod picture;
mod texture;
mod timeline;
mod transport;

use eframe::egui::{self, CursorIcon, EventFilter, Id, Key, Modifiers, Rect, Response, Sense, Ui, pos2, vec2};
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use wavedude_app_state::Event;
use wavedude_app_state::commands::Command;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::Orientation;
use wavedude_core::engine::SampleFormat;
use wavedude_core::model::{DisplayLayerEdit, ItemRef, Layer};
use wavedude_core::render::gpu::{PicturePipeline, PlaneTooLarge, plane_fits};
use wavedude_view::overlay::HandleKind;

use wavedude_view::draw::Outlines;

use self::device::DevicePictures;
use self::graph_boxes::{BoxDrag, Boxes};
use self::overlay::Overlay;
use self::texture::Texture;
use self::timeline::Timeline;
use self::transport::{Transport, play_or_pause};
use super::theme::Palette;
use crate::App;
use crate::menus::{MenuAction, On};
use crate::preview::overlay::Grab;
use crate::preview::presenter::{self, FrameSource, LayerRequest, Presenter, PresenterStats, SourceKey};
use crate::preview::view::View;
use crate::preview::{
	BUSY_DELAY, POLL, PictureExtent, RequestInputs, SceneWatch, display_edit, layer_name, request_of, shown_source,
};

/// The margin around the pictures.
const PICTURES_MARGIN: f32 = 4.;
/// The splitter's thickness.
const SPLITTER: f32 = 4.;
/// A splitter key moves it this far.
const SPLITTER_KEY_STEP: f32 = 10.;
/// The splitter's least share either side.
const SPLIT_LIMIT: f32 = 0.05;
/// The timeline's padding: horizontal, vertical.
const TIMELINE_PADDING: (f32, f32) = (4., 2.);
/// The timeline and the transport bar's height below the pictures.
const HEIGHT_BELOW: f32 = timeline::HEIGHT + 2. * TIMELINE_PADDING.1 + transport::HEIGHT;

/// The pane's state between frames.
pub(super) struct Preview {
	/// The renderer the pictures' textures live on.
	gpu: Option<RenderState>,
	presenter: Option<Presenter>,
	/// What the request was built from last; the same inputs build the same request.
	inputs: Option<RequestInputs>,
	sent: Option<(SourceKey, Vec<LayerRequest>)>,
	/// A request whose pictures have not come back yet.
	awaiting: bool,
	stats: PresenterStats,
	scene: SceneWatch,
	computing: Option<(u32, Instant)>,
	busy: bool,
	/// Auto brightness waiting for a 16-bit frame of this layer.
	fit: Option<Layer>,
	/// Each layer's texture, kept for the next picture.
	textures: [Option<Texture>; 2],
	/// Each layer's picture shown, with the part of the canvas it covers (None: all of it).
	shown: [Option<Option<Rect>>; 2],
	/// The renderer's limits, when it draws pictures with [`PicturePipeline`].
	limits: Option<wgpu::Limits>,
	/// Why the renderer draws no pictures with [`PicturePipeline`], until logged as a picture would be.
	unsupported: Option<String>,
	/// The shown frame's planes the renderer cannot hold, warned of once.
	oversize: Option<PlaneTooLarge>,
	/// The pictures are coloured on the renderer's device, not by the presenter.
	on_device: bool,
	device: DevicePictures,
	/// An image scene's own picture, shown before the first frame.
	scene_image: Option<Texture>,
	scene_shown: bool,
	extents: [PictureExtent; 2],
	/// The zoom over the fit, shared by both pictures.
	view: View,
	/// The first picture's share of the split, session state.
	split: f32,
	/// Each turned header strip's box edits upright.
	editing: [bool; 2],
	/// Each picture's overlay model and graph boxes, kept while what they show stays.
	overlays: [Option<Overlay>; 2],
	boxes: [Option<Boxes>; 2],
	/// The labels' halos.
	outlines: Outlines,
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
		let presenter = match &gpu {
			Some(_) => {
				Presenter::start().inspect_err(|e| tracing::error!("the preview presenter could not start: {e}")).ok()
			}
			None => {
				tracing::warn!("the preview shows no pictures: the window has no wgpu renderer");
				None
			}
		};
		let supported = gpu.as_ref().map(|gpu| {
			PicturePipeline::supported(&gpu.adapter, &gpu.device)
				.map(|()| gpu.device.limits())
				.map_err(|e| e.to_string())
		});
		let (limits, unsupported) = match supported {
			Some(Ok(limits)) => (Some(limits), None),
			Some(Err(e)) => (None, Some(e)),
			None => (None, None),
		};
		Self {
			gpu,
			presenter,
			inputs: None,
			sent: None,
			awaiting: false,
			stats: PresenterStats::default(),
			scene: SceneWatch::default(),
			computing: None,
			busy: false,
			fit: None,
			textures: [None, None],
			shown: [None, None],
			limits,
			unsupported,
			oversize: None,
			on_device: false,
			device: DevicePictures::default(),
			scene_image: None,
			scene_shown: false,
			extents: [PictureExtent::default(); 2],
			view: View::default(),
			split: 0.5,
			editing: [false; 2],
			overlays: [None, None],
			boxes: [None, None],
			outlines: Outlines::new(1.),
			grab: None,
			focus: [None; 2],
			box_drag: None,
			pointer_focus: None,
			timeline: Timeline::default(),
			transport: Transport::default(),
		}
	}

	/// Whether a picture is due: the pane then polls after [`POLL`].
	fn due(&self, app: &App) -> bool {
		let live = &app.live;
		self.awaiting
			|| self.device.pending()
			|| self.scene.building()
			|| self.computing.is_some()
			|| self.fit.is_some()
			|| live.playback.is_playing()
			|| live.run_is_progressing()
			|| live.is_working()
			|| live.fit_pending()
	}

	fn poll(&mut self, app: &mut App, out: &mut Vec<Event>) {
		let now = Instant::now();
		// A render of an older request is dropped: the one in flight follows. One whose frame vanished frees the
		// key, so the same frame made anew is asked for again.
		if let Some(rendered) = self.presenter.as_mut().and_then(Presenter::latest)
			&& self.sent.as_ref().is_some_and(|(key, _)| *key == rendered.source)
		{
			// A frame being read back is asked for again at the next poll, the pictures shown meanwhile kept.
			self.awaiting = rendered.pending;
			if rendered.vanished || rendered.pending {
				self.sent = None;
			}
			if let Some(gpu) = &self.gpu {
				for p in &rendered.pictures {
					let layer = p.layer as usize;
					Texture::upload(&mut self.textures[layer], gpu, (p.width, p.height), &p.rgba);
					let window = p
						.window
						.map(|(x, y, w, h)| Rect::from_min_size(pos2(x as f32, y as f32), vec2(w as f32, h as f32)));
					self.shown[layer] = Some(window);
				}
			}
			if !rendered.pending
				&& let Some(line) = self.stats.record(&rendered, now)
			{
				tracing::debug!("{line}");
			}
		}
		if let Some(image) = self.scene.poll(app) {
			self.scene_shown = false;
			if let (Some(gpu), Some(p)) = (&self.gpu, image) {
				Texture::upload(&mut self.scene_image, gpu, (p.width, p.height), &p.rgba);
				self.scene_shown = true;
			}
		}
		let frames = app.live.device_frames();
		if (frames.is_some() || presenter::IN_CALL)
			&& shown_source(app).is_some()
			&& let Some(e) = self.unsupported.take()
		{
			tracing::warn!("the preview colours its pictures on the CPU: {e}");
		}
		let fits =
			self.limits.as_ref().map(|limits| shown_source(app).map_or(Ok(()), |s| plane_fits(&s.shape(), limits)));
		let oversize = fits.and_then(Result::err);
		if let Some(e) = oversize.filter(|e| self.oversize != Some(*e)) {
			tracing::warn!("the preview colours this run's pictures on the CPU: {e}");
		}
		self.oversize = oversize;
		let on_device = fits.is_some_and(|f| f.is_ok()) && (frames.is_some() || presenter::IN_CALL);
		if self.on_device != on_device {
			self.on_device = on_device;
			self.inputs = None;
			self.sent = None;
			self.awaiting = false;
			self.shown = [None, None];
			self.device.clear();
		}
		let extents = self.extents;
		let inputs = RequestInputs::of(app, extents, self.scene.edges());
		if self.inputs.as_ref() != Some(&inputs) || (self.sent.is_none() && !self.on_device) || self.device.pending() {
			self.inputs = Some(inputs);
			match request_of(app, extents, self.scene.edges()) {
				Some(request) if self.on_device => {
					if let Some(gpu) = &self.gpu {
						self.device.show(gpu, frames, &request.source, &request.layers);
					}
				}
				Some(request) => {
					let key = (request.source.key(), request.layers.clone());
					if let Some(presenter) = &mut self.presenter
						&& self.sent.as_ref() != Some(&key)
					{
						self.sent = Some(key);
						self.awaiting = true;
						presenter.request(request);
					}
				}
				None => {
					if app.live.shown().is_none() {
						self.device.clear();
						if self.sent.take().is_some() {
							self.awaiting = false;
							self.shown = [None, None];
						}
					}
				}
			}
		}
		let (computing, position) = (app.live.computing_picture(), app.live.playback.position());
		self.computing = match (computing, self.computing) {
			(Some(f), Some((g, since))) if f == g => Some((g, since)),
			(Some(f), _) => Some((f, now)),
			(None, _) => None,
		};
		self.busy = self.computing.is_some_and(|(f, since)| f == position && now - since >= BUSY_DELAY);
		if let Some(layer) = self.fit {
			if let Some(frame) = app.live.take_half_frame() {
				self.fit = None;
				if frame.frame == position {
					fit_now(&FrameSource::Snapshot(frame.snapshot), layer, out);
				}
			} else if !app.live.fit_pending() {
				self.fit = None;
				tracing::debug!("auto brightness dropped: the 16-bit frame did not arrive");
			}
		}
	}

	/// Auto brightness: fits the layer's brightness once to the frame shown; with 8-bit samples the frame is
	/// computed again in 16-bit first and fitted when it arrives.
	fn auto_brightness(&mut self, app: &mut App, layer: Layer, out: &mut Vec<Event>) {
		match shown_source(app) {
			Some(s) if s.shape().format == SampleFormat::Half => fit_now(&s, layer, out),
			_ => {
				if app.live.request_half_frame(&app.document.project) {
					self.fit = Some(layer);
				}
			}
		}
	}

	/// The pane in `ui`'s room; the events it asks the state for go to `out`, a context menu entry for the window
	/// is returned with what it applies to.
	pub(super) fn ui(&mut self, ui: &mut Ui, app: &mut App, out: &mut Vec<Event>) -> Option<(MenuAction, On)> {
		ui.data_mut(|d| d.insert_temp(holder(), false));
		self.poll(app, out);
		if self.due(app) {
			ui.ctx().request_repaint_after(POLL);
		}
		let pane = ui.max_rect();
		let below = HEIGHT_BELOW.min(pane.height());
		let (pictures, below) = pane.split_top_bottom_at_y(pane.bottom() - below);
		let (timeline, bar) = below.split_top_bottom_at_y(below.bottom() - transport::HEIGHT);
		let room = pictures.shrink(PICTURES_MARGIN);
		let display = &app.document.project.display;
		let shown = [display.field.preview, display.intensity.preview];
		let vertical = app.settings.preview_orientation == Orientation::Vertical;
		let mut picked = None;
		match shown {
			[true, true] => {
				let [first, second] = self.split_cells(ui, room, vertical, app.strings.get("TipPreviewSplitter"));
				self.cell(ui, first, Layer::Field, app, out, &mut picked);
				self.cell(ui, second, Layer::Intensity, app, out, &mut picked);
			}
			[true, false] => self.cell(ui, room, Layer::Field, app, out, &mut picked),
			[false, true] => self.cell(ui, room, Layer::Intensity, app, out, &mut picked),
			[false, false] => {}
		}
		let timeline = timeline.shrink2(vec2(TIMELINE_PADDING.0, TIMELINE_PADDING.1));
		self.timeline.ui(ui, timeline, app, &mut self.pointer_focus, out);
		self.transport.ui(ui, bar, app, out);
		pane_keys(ui, app, out);
		picked
	}

	/// The two cells and the splitter between them, which a drag or the arrow keys (10 px a key) move and a
	/// double-click or Enter puts back to half.
	fn split_cells(&mut self, ui: &mut Ui, room: Rect, vertical: bool, tip: &str) -> [Rect; 2] {
		let length = if vertical { room.height() } else { room.width() } - SPLITTER;
		let at = (length * self.split).round();
		let (first, bar, second) = if vertical {
			let (first, rest) = room.split_top_bottom_at_y(room.top() + at);
			let (bar, second) = rest.split_top_bottom_at_y(rest.top() + SPLITTER);
			(first, bar, second)
		} else {
			let (first, rest) = room.split_left_right_at_x(room.left() + at);
			let (bar, second) = rest.split_left_right_at_x(rest.left() + SPLITTER);
			(first, bar, second)
		};
		let id = ui.scope_id().with("PreviewSplitter");
		let response = ui.interact(bar, id, Sense::click_and_drag()).on_hover_text(tip);
		let response =
			response.on_hover_cursor(if vertical { CursorIcon::ResizeVertical } else { CursorIcon::ResizeHorizontal });
		hold(&response);
		let mut split = self.split;
		if response.dragged() {
			let d = response.drag_delta();
			split += if vertical { d.y } else { d.x } / length.max(1.);
		}
		if response.double_clicked() {
			split = 0.5;
		}
		if response.has_focus() {
			ui.memory_mut(|m| {
				m.set_focus_lock_filter(
					id,
					EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() },
				);
			});
			let (back, forth) =
				if vertical { (Key::ArrowUp, Key::ArrowDown) } else { (Key::ArrowLeft, Key::ArrowRight) };
			ui.input_mut(|i| {
				let n = i.count_and_consume_key(Modifiers::NONE, forth) as f32
					- i.count_and_consume_key(Modifiers::NONE, back) as f32;
				split += n * SPLITTER_KEY_STEP / length.max(1.);
				if i.consume_key(Modifiers::NONE, Key::Enter) {
					split = 0.5;
				}
			});
		}
		self.split = split.clamp(SPLIT_LIMIT, 1. - SPLIT_LIMIT);
		let palette = Palette::of(ui.ctx().theme());
		let color = if response.has_focus() || response.dragged() { palette.accent } else { palette.window_background };
		ui.painter().rect_filled(bar, 0., color);
		[first, second]
	}
}

/// The pane's keys with the focus anywhere in it but in a text box, with no modifier or Shift alone: Space plays or
/// pauses, `,` and `.` step, I, O and X set and clear the range, L loops, F follows the run, Home and End go to the
/// in and out points.
fn pane_keys(ui: &Ui, app: &App, out: &mut Vec<Event>) {
	let ctx = ui.ctx();
	if !ui.data(|d| d.get_temp(holder())).unwrap_or(false) || ctx.text_edit_focused() {
		return;
	}
	let p = &app.live.playback;
	ctx.input_mut(|i| {
		i.events.retain(|e| {
			let egui::Event::Key { key, pressed: true, modifiers, .. } = e else { return true };
			if !(modifiers.is_none() || *modifiers == Modifiers::SHIFT) {
				return true;
			}
			out.push(match key {
				Key::Space => play_or_pause(app),
				Key::Comma => Event::StepFrames(-1),
				Key::Period => Event::StepFrames(1),
				Key::I => Event::SetInPoint,
				Key::O => Event::SetOutPoint,
				Key::X => Event::ClearRange,
				Key::L => Event::SetLoop(!p.looping()),
				Key::F => Event::SetLive(!app.live.is_live()),
				Key::Home => Event::Scrub(p.range_frames().0),
				Key::End => Event::Scrub(p.range_frames().1),
				_ => return true,
			});
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

/// Whether a button went down on `r` this frame; a press and its release can come in one.
fn pressed_on(r: &Response) -> bool {
	r.hovered() && r.ctx.input(|i| i.pointer.any_pressed())
}

/// Fits `layer`'s brightness to the 16-bit frame `source`.
fn fit_now(source: &FrameSource, layer: Layer, out: &mut Vec<Event>) {
	match crate::preview::fit(source, layer) {
		Some(value) => {
			tracing::debug!(layer = layer_name(layer), value, "auto brightness");
			out.push(Event::Command(Command::Edit(display_edit(layer, DisplayLayerEdit::Brightness(value)))));
		}
		None => tracing::info!("Auto brightness: the frame shown has nothing to fit."),
	}
}
