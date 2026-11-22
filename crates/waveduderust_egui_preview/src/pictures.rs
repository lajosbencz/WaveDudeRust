//! The pictures of the frame shown (docs/ui/40-preview-pane.md, What a picture shows): the presenter's requests and
//! textures, the pictures coloured on the renderer's device, the scene's own picture and outline, the busy state and
//! Auto brightness.
//!
//! Each frame the window polls: it hands the frame the live controller shows to the presenter when what is drawn
//! changed, writes the presenter's pictures into their textures and asks for the next frame after
//! [`POLL`](waveduderust_app::preview::POLL) while a picture is due ([`Pictures::due`]). Where the renderer's device
//! draws pictures, the presenter is bypassed: runs on the GPU engine, and every frame in the browser.
//!
//! Threads: the UI thread; natively the presenter and the scene builder use threads of their own.

mod device;
mod texture;

use eframe::egui::{Color32, InputState, Painter, PointerButton, Pos2, Rect, Response, Ui, pos2, vec2};
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use waveduderust::model::{DisplayLayerEdit, Layer};
use waveduderust::unstable::engine::SampleFormat;
use waveduderust::unstable::render::gpu::{PicturePipeline, PlaneTooLarge, plane_fits};
use waveduderust::unstable::view::DeviceGrid;
use waveduderust::unstable::view::overlay::{edge_color, edge_line};
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::commands::Command;
use waveduderust_app::platform::Instant;
use waveduderust_app::preview::presenter::{self, Delivery, FrameSource, Presenter, PresenterStats};
use waveduderust_app::preview::view::{CanvasTransform, View};
use waveduderust_app::preview::{
	BUSY_DELAY, PictureExtent, RequestInputs, SceneWatch, display_edit, layer_name, request_of, shown_source,
};

use self::device::DevicePictures;
use self::texture::Texture;
use waveduderust_egui_platform::input::{arrow_steps, lock_arrows, pinched_press, wheel_notches};

/// `r` with its edges on the nearest whole device pixels of `grid`.
pub fn snapped(grid: &DeviceGrid, r: Rect) -> Rect {
	let [left, top, right, bottom] = grid.snapped((r.min.x, r.min.y), (r.max.x, r.max.y));
	grid.rect((left, top), (right, bottom))
}

/// `view` after the pointer on the picture `response` at `t`: the wheel zooms about the point under the pointer, a
/// middle-button drag pans a zoomed view, and a pinch zooms about and a two-finger drag pans with the fingers' centre; arrows pan
/// a zoomed view while the picture has the focus (Shift: ten steps).
pub fn navigate(view: View, ui: &Ui, response: &Response, t: &CanvasTransform) -> View {
	let mut view = view;
	pinched_press(ui.ctx());
	// not `zoom_delta`, which counts the wheel's Ctrl-zoom
	match ui.input(InputState::multi_touch) {
		Some(m) => {
			// a gesture that began over the picture itself, not over a popup or beside it
			let began_here =
				t.visible().contains(m.start_pos) && ui.ctx().layer_id_at(m.start_pos) == Some(ui.layer_id());
			if began_here {
				let before = m.center_pos - m.translation_delta;
				view = view.pinched(m.zoom_delta, t.to_canvas(before), m.translation_delta, t.scale, t.canvas);
			}
		}
		None => {
			let notches = wheel_notches(ui, t.visible());
			if let Some(p) = response.hover_pos().filter(|_| notches != 0) {
				let factor = View::STEP.powi(notches);
				view = view.normalised(t.canvas).zoomed(factor, t.to_canvas(p), t.canvas);
			}
		}
	}
	if response.dragged_by(PointerButton::Middle) {
		view = view.panned(response.drag_delta(), t.scale);
	}
	if response.has_focus() {
		lock_arrows(ui, response.id);
		let (dx, dy) = arrow_steps(ui);
		view = view.panned(-PAN_KEY * vec2(dx as f32, dy as f32), t.scale);
	}
	view
}

/// Points a focused picture pans per arrow key.
const PAN_KEY: f32 = 16.;

/// See the module docs.
pub struct Pictures {
	/// The renderer the pictures' textures live on.
	gpu: Option<RenderState>,
	presenter: Option<Presenter>,
	/// What the request was built from last; the same inputs build the same request.
	inputs: Option<RequestInputs>,
	delivery: Delivery,
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
}

impl Pictures {
	pub fn new(gpu: Option<RenderState>) -> Self {
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
			delivery: Delivery::default(),
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
		}
	}

	/// The position's frame has been computing for longer than [`BUSY_DELAY`].
	pub fn busy(&self) -> bool {
		self.busy
	}

	/// The pictures' extents the next request asks for.
	pub fn extents(&self) -> [PictureExtent; 2] {
		self.extents
	}

	/// Sets the extent `layer`'s picture is drawn at, read by the next [`poll`](Self::poll).
	pub fn set_extent(&mut self, layer: Layer, extent: PictureExtent) {
		self.extents[layer as usize] = extent;
	}

	/// Whether a picture is due: the window then polls after [`POLL`](waveduderust_app::preview::POLL).
	pub fn due(&self, app: &App) -> bool {
		let live = &app.live;
		self.delivery.awaiting()
			|| self.device.pending()
			|| self.scene.building()
			|| self.computing.is_some()
			|| self.fit.is_some()
			|| live.playback.is_playing()
			|| live.run_is_progressing()
			|| live.is_working()
			|| live.fit_pending()
	}

	/// Takes the presenter's pictures and the scene's, asks for the frame shown when what is drawn changed and
	/// fits a waiting Auto brightness; the events it asks the state for go to `out`.
	pub fn poll(&mut self, app: &mut App, out: &mut Vec<Event>) {
		let now = Instant::now();
		// A frame vanished or being read back is asked for again at the next poll, the pictures shown meanwhile kept.
		if let Some(rendered) = self.presenter.as_mut().and_then(Presenter::latest)
			&& self.delivery.take(&rendered)
		{
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
			self.delivery.forget();
			self.shown = [None, None];
			self.device.clear();
		}
		let extents = self.extents;
		let inputs = RequestInputs::of(app, extents, self.scene.edges());
		if self.inputs.as_ref() != Some(&inputs)
			|| (self.delivery.is_idle() && !self.on_device)
			|| self.device.pending()
		{
			self.inputs = Some(inputs);
			match request_of(app, extents, self.scene.edges()) {
				Some(request) if self.on_device => {
					if let Some(gpu) = &self.gpu {
						self.device.show(gpu, frames, &request.source, &request.layers);
					}
				}
				Some(request) => {
					if let Some(presenter) = &mut self.presenter {
						self.delivery.send(presenter, request);
					}
				}
				None => {
					if app.live.shown().is_none() {
						self.device.clear();
						if self.delivery.forget() {
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
	pub fn auto_brightness(&mut self, app: &mut App, layer: Layer, out: &mut Vec<Event>) {
		match shown_source(app) {
			Some(s) if s.shape().format == SampleFormat::Half => fit_now(&s, layer, out),
			_ => {
				if app.live.request_half_frame() {
					self.fit = Some(layer);
				}
			}
		}
	}

	/// Paints `layer`'s picture at `t` on whole device pixels: black, then the frame shown (a windowed picture
	/// covers its canvas part alone) or, before the first frame, an image scene's own picture, then the edge line.
	/// Returns the painter clipped to the visible picture and the edge line's room, and the visible picture.
	pub fn paint(&mut self, ui: &Ui, layer: Layer, t: &CanvasTransform, app: &App) -> (Painter, Rect) {
		let grid = DeviceGrid::new(ui.ctx().pixels_per_point(), 0., 0.);
		let visible = snapped(&grid, t.visible());
		let painter = ui.painter_at(visible.expand(grid.hairline() / grid.scale));
		let picture = t.picture();
		let full = snapped(&grid, picture);
		painter.rect_filled(full, 0., Color32::BLACK);
		let uv = Rect::from_min_max(Pos2::ZERO, pos2(1., 1.));
		let dst = |window: Option<Rect>| {
			window.map_or(full, |w| {
				snapped(&grid, Rect::from_min_max(t.to_screen(w.min.x, w.min.y), t.to_screen(w.max.x, w.max.y)))
			})
		};
		match (self.shown[layer as usize], &self.textures[layer as usize], &self.gpu) {
			(_, _, Some(gpu)) if self.on_device && self.device.has(layer) => {
				self.device.paint(gpu, &painter, layer, dst(self.device.window(layer)));
			}
			(Some(window), Some(texture), _) if !self.on_device => {
				painter.image(texture.id(), dst(window), uv, Color32::WHITE);
			}
			_ => {
				if let Some(scene) = self.scene_image.as_ref().filter(|_| self.scene_shown) {
					painter.image(scene.id(), full, uv, Color32::WHITE);
				}
			}
		}
		if let Some(color) = edge_color(&app.document.project.scene) {
			painter.add(edge_line(grid, (picture.min.x, picture.min.y, picture.width(), picture.height()), color));
		}
		(painter, visible)
	}
}

/// Fits `layer`'s brightness to the 16-bit frame `source`.
fn fit_now(source: &FrameSource, layer: Layer, out: &mut Vec<Event>) {
	match waveduderust_app::preview::fit(source, layer) {
		Some(value) => {
			tracing::debug!(layer = layer_name(layer), value, "auto brightness");
			out.extend(
				display_edit(layer, DisplayLayerEdit::Brightness(value)).map(|e| Event::Command(Command::Edit(e))),
			);
		}
		None => tracing::info!("Auto brightness: the frame shown has nothing to fit."),
	}
}
