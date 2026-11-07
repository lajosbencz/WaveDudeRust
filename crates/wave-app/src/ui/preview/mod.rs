//! The preview pane: the pictures with their overlays, the probe graph boxes, the timeline and the transport bar.
//!
//! [`preview_pane`] composes the pane. A task of the pane polls every few milliseconds: it hands the frame the
//! live controller shows to the [`presenter::Presenter`] (only when what is to be drawn changed), turns the
//! presenter's pictures into images, keeps the busy dim, finishes an Auto brightness and rebuilds the scene's
//! outline when the scene changes. The shell may provide [`PreviewActions`] to receive the context menus'
//! exports; without it they are logged and dropped.
//!
//! Threads: everything here runs on the UI thread; the presenter and the outline builder use threads of their own.

pub mod graph_boxes;
pub mod handles;
pub mod overlay;
pub mod pictures;
pub mod presenter;
pub mod transport;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use bytes::Bytes;
use freya::elements::image::ImageHandle;
use freya::engine::prelude::AlphaType;
use freya::prelude::*;
use serde_json::json;
use wave_core::engine::SampleFormat;
use wave_core::model::{Layer, SceneMode, Stencil};
use wave_core::render::fit_brightness;
use wave_core::scene::compose::{compose_scene, decode_image};
use wave_core::scene::glowing_edges;
use wave_widgets::menu::ContextMenuRequest;
use wave_widgets::{ContextMenuHost, MenuItem, MenuModel, TestIdExt, use_wave_theme};

use self::handles::HandleKind;
use self::overlay::Grab;
use self::pictures::PictureCell;
use self::presenter::{
	EdgeImage, FrameSource, LayerRequest, Outline, Presenter, PresenterStats, Request, SourceKey, as_halves,
};
use crate::ui::app::TICK;
use crate::ui::shared::{App, Shared, Topics};
use wave_app_state::Event as AppEvent;
use wave_app_state::commands::{Command, Target};
use wave_app_state::live::Shown;
use wave_app_state::selection::ItemRef;
use wave_app_state::settings::Orientation;
use wave_app_state::strings::Strings;
use wave_widgets::picture::{PictureExtent, Rect4, View};

/// How often the pane's task polls the state and the presenter while a picture is due; at [`TICK`] otherwise.
pub const POLL: Duration = Duration::from_millis(4);
/// A frame computed for longer than this dims the pictures under a spinner.
pub const BUSY_DELAY: Duration = Duration::from_millis(150);

/// "Field" or "Intensity", for test ids.
pub fn layer_name(layer: Layer) -> &'static str {
	match layer {
		Layer::Field => "Field",
		Layer::Intensity => "Intensity",
	}
}

/// A rendered picture with the canvas part it covers; None is all of it.
pub type PictureShown = (ImageHandle, Option<Rect4>);

/// The pane's view state shared by its parts; plain Freya states, so the struct is `Copy`.
#[derive(Clone, Copy, PartialEq)]
pub struct PaneStates {
	/// The newest picture of each layer (index = `Layer as usize`).
	pub pictures: State<[Option<PictureShown>; 2]>,
	/// An image scene's own picture, shown before the first frame.
	pub scene_image: State<Option<ImageHandle>>,
	/// Each picture's visible size in device pixels and the canvas part it shows.
	pub extents: State<[PictureExtent; 2]>,
	/// The zoom over the fit, shared by both pictures.
	pub view: State<View>,
	pub busy: State<bool>,
	pub menu: State<Option<ContextMenuRequest<PictureMenu>>>,
	pub grab: State<Option<Grab>>,
	/// The keyboard-focused handle of each picture.
	pub focus: State<[Option<(ItemRef, HandleKind)>; 2]>,
	pub box_grab: State<Option<graph_boxes::BoxGrab>>,
	/// Auto brightness waiting for a 16-bit frame of this layer.
	pub fit: State<Option<Layer>>,
	/// The first picture's share of the split, session state.
	pub split: State<f32>,
}

impl PaneStates {
	fn use_new() -> Self {
		Self {
			pictures: use_state(|| [None, None]),
			scene_image: use_state(|| None),
			extents: use_state(|| [PictureExtent { device: (0, 0), window: None }; 2]),
			view: use_state(View::default),
			busy: use_state(|| false),
			menu: use_state(|| None),
			grab: use_state(|| None),
			focus: use_state(|| [None; 2]),
			box_grab: use_state(|| None),
			fit: use_state(|| None),
			split: use_state(|| 0.5),
		}
	}
}

/// What the preview's context menus ask the shell for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewAction {
	/// Focus or edit the layer's brightness.
	Brightness(Layer),
	/// Export the frame shown of this layer.
	ExportFrame(Layer),
	ExportField,
	ExportIntensity,
	ExportBoth,
	/// Export probe `index`'s data as CSV.
	ExportProbeCsv(usize),
}

/// What an entry of the preview's context menus does: the pane's own auto brightness, else the shell's action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureMenu {
	AutoBrightness(Layer),
	Shell(PreviewAction),
}

/// The shell's handler of [`PreviewAction`]s, provided as a context above the pane.
#[derive(Clone, PartialEq)]
pub struct PreviewActions(pub EventHandler<PreviewAction>);

/// "field" or "intensity", the layer's key in the display settings' paths and in menu ids.
pub fn layer_key(layer: Layer) -> &'static str {
	match layer {
		Layer::Field => "field",
		Layer::Intensity => "intensity",
	}
}

/// The picture's context menu (and the header strip's).
pub fn picture_context_menu(strings: &Strings, layer: Layer) -> MenuModel<PictureMenu> {
	let key = layer_key(layer);
	let item = |id: String, text: &str, action| MenuItem::with_action(id, strings.get(text), action).into();
	let shell = |id: &str, text: &str, action| item(id.to_owned(), text, PictureMenu::Shell(action));
	MenuModel::new(vec![
		shell(&format!("pic:brightness:{key}"), "MenuBrightness", PreviewAction::Brightness(layer)),
		item(format!("pic:auto:{key}"), "MenuAutoBrightness", PictureMenu::AutoBrightness(layer)),
		wave_widgets::MenuEntry::Separator,
		shell(&format!("pic:frame:{key}"), "MenuExportThisFrame", PreviewAction::ExportFrame(layer)),
		shell("pic:export:field", "MenuFileExportField", PreviewAction::ExportField),
		shell("pic:export:intensity", "MenuFileExportIntensity", PreviewAction::ExportIntensity),
		shell("pic:export:both", "MenuFileExportBoth", PreviewAction::ExportBoth),
	])
}

/// Opens the picture's context menu at `at` (window logical pixels).
pub fn open_picture_menu(shared: &Shared, states: PaneStates, layer: Layer, at: (f64, f64)) {
	let model = shared.peek(|app| picture_context_menu(&app.strings, layer));
	let mut menu = states.menu;
	menu.set(Some(ContextMenuRequest { at, model }));
}

/// The frame the pictures show, as a presenter source.
fn shown_source(app: &App) -> Option<FrameSource> {
	let live = &app.live;
	match live.shown()? {
		Shown::Run(k) => {
			let store = live.run_store()?;
			let stored = live.run_spec()?.stored_index_of(k) as usize;
			(stored < store.count()).then_some(FrameSource::Run { store, stored })
		}
		Shown::Scratch(_) => live.latest_scratch().cloned().map(FrameSource::Scratch),
	}
}

fn request_of(app: &App, extents: [PictureExtent; 2], edges: Option<&Arc<EdgeImage>>) -> Option<Request> {
	let source = shown_source(app)?;
	let project = &app.document.project;
	let dither_bits = if app.settings.preview_sample_bytes >= 2 { app.settings.preview_dither_bits } else { 0 };
	let layers = [Layer::Field, Layer::Intensity]
		.into_iter()
		.filter(|&l| project.display.layer(l).preview)
		.map(|layer| {
			let d = project.display.layer(layer);
			LayerRequest {
				layer,
				scheme: app.schemes.resolve(&project.color_schemes, &d.color_scheme),
				black_level: d.black_level,
				brightness: d.brightness,
				device: extents[layer as usize].device,
				window: extents[layer as usize].window.map(|(x, y, w, h)| (x.into(), y.into(), w.into(), h.into())),
				dither_bits,
				outline: d.overlay.then(|| edges.map(|e| Outline { edges: e.clone(), gain: d.overlay_gain })).flatten(),
			}
		})
		.collect();
	let motion = app.live.playback.is_playing() || app.live.is_live();
	Some(Request { source, layers, motion, at: Instant::now() })
}

/// Auto brightness: fits the layer's brightness once to the frame shown; with 8-bit samples the frame is computed
/// again in 16-bit first and fitted when it arrives.
pub fn auto_brightness(shared: &Shared, states: PaneStates, layer: Layer) {
	let source = shared.peek(shown_source);
	match source {
		Some(s) if s.shape().format == SampleFormat::Half => fit_now(shared, &s, layer),
		_ => {
			let requested = shared.update(|app| app.live.request_half_frame(&app.document.project));
			if requested {
				let mut fit = states.fit;
				fit.set(Some(layer));
			}
		}
	}
}

fn fit_now(shared: &Shared, source: &FrameSource, layer: Layer) {
	let shape = source.shape();
	let fitted =
		source.with_plane(layer, |bytes| fit_brightness(&as_halves(bytes), shape.sample_kind(layer))).flatten();
	match fitted {
		Some(value) => {
			tracing::debug!(layer = layer_name(layer), value, "auto brightness");
			shared.send(AppEvent::Command(Command::Set {
				target: Target::Display,
				path: format!("{}.brightness", layer_key(layer)),
				value: json!(value),
			}));
		}
		None => tracing::info!("Auto brightness: the frame shown has nothing to fit."),
	}
}

/// What the scene outline and the scene picture are built from: the scene and the parts by their revision.
#[derive(Clone, Copy, PartialEq)]
struct SceneInputs {
	scene_revision: u64,
	image_scene: bool,
	stencil_scale: (bool, u64),
	edges: bool,
}

impl SceneInputs {
	fn of(app: &App) -> Self {
		let project = &app.document.project;
		let sim = &project.simulation;
		Self {
			scene_revision: app.revisions().scene,
			image_scene: project.scene.mode == SceneMode::Image,
			stencil_scale: (sim.stencil == Stencil::FourthOrder, sim.source_frequency_scale.to_bits()),
			edges: project.display.field.overlay || project.display.intensity.overlay,
		}
	}
}

struct SceneArt {
	generation: u64,
	image: Option<(u32, u32, Bytes)>,
	edges: Option<Arc<EdgeImage>>,
}

/// Rebuilds the scene's own picture (an image scene) and its glowing edges (when a layer shows the outline) on a
/// thread when the scene changes; the newest build wins.
#[derive(Default)]
struct SceneWatch {
	inputs: Option<SceneInputs>,
	generation: u64,
	rx: Option<Receiver<SceneArt>>,
	edges: Option<Arc<EdgeImage>>,
}

impl SceneWatch {
	/// A build is in flight.
	fn building(&self) -> bool {
		self.rx.is_some()
	}

	/// Polls; returns the scene picture when a build finished.
	fn poll(&mut self, app: &App) -> Option<Option<(u32, u32, Bytes)>> {
		let inputs = SceneInputs::of(app);
		let wanted = inputs.image_scene || inputs.edges;
		if self.inputs != Some(inputs) {
			self.inputs = Some(inputs);
			self.generation += 1;
			if wanted {
				let (tx, rx) = channel();
				self.rx = Some(rx);
				let mut project = app.document.project.clone();
				project.probes.clear();
				project.sources.clear();
				let generation = self.generation;
				let spawned = std::thread::Builder::new().name("preview-scene".into()).spawn(move || {
					let image = decode_image(&project).map(|p| (p.width, p.height, Bytes::from(p.rgba)));
					let edges = inputs.edges.then(|| compose_scene(&project)).flatten().map(|p| {
						let rgba = glowing_edges::compute(&p.rgba, p.width as usize, p.height as usize);
						Arc::new(EdgeImage { width: p.width, height: p.height, rgba })
					});
					let _ = tx.send(SceneArt { generation, image, edges });
				});
				if let Err(e) = spawned {
					tracing::warn!("the scene outline could not be built: {e}");
				}
			} else {
				self.rx = None;
				self.edges = None;
				return Some(None);
			}
		}
		let art = self.rx.as_ref()?.try_recv().ok()?;
		if art.generation != self.generation {
			return None;
		}
		self.rx = None;
		self.edges = art.edges;
		Some(art.image)
	}
}

/// The poll loop's memory between two polls.
struct Poller {
	presenter: Presenter,
	sent: Option<(SourceKey, Vec<LayerRequest>)>,
	/// A request whose pictures have not come back yet.
	awaiting: bool,
	stats: PresenterStats,
	scene: SceneWatch,
	computing: Option<(u32, Instant)>,
}

impl Poller {
	/// The wait before the next poll: short while a picture is due, the tick's otherwise.
	fn next_wait(&self, shared: &Shared, states: PaneStates) -> Duration {
		let due = self.awaiting
			|| self.scene.building()
			|| self.computing.is_some()
			|| states.fit.peek().is_some()
			|| shared.peek(|app| {
				let live = &app.live;
				live.playback.is_playing() || live.run_is_active() || live.scratch_pending() || live.is_live()
			});
		if due { POLL } else { TICK }
	}

	fn poll(&mut self, shared: &Shared, mut states: PaneStates) {
		let now = Instant::now();
		if let Some(rendered) = self.presenter.latest() {
			if self.sent.as_ref().is_some_and(|(key, _)| *key == rendered.source) {
				self.awaiting = false;
			}
			let mut pictures = states.pictures.peek().clone();
			for p in &rendered.pictures {
				if let Some(image) = ImageHandle::from_rgba(p.width, p.height, p.rgba.clone(), AlphaType::Unpremul) {
					let window = p.window.map(|(x, y, w, h)| (x as f32, y as f32, w as f32, h as f32));
					pictures[p.layer as usize] = Some((image, window));
				}
			}
			states.pictures.set(pictures);
			if let Some(line) = self.stats.record(&rendered, now) {
				tracing::debug!("{line}");
			}
		}
		if let Some(image) = shared.peek(|app| self.scene.poll(app)) {
			states.scene_image.set(image.and_then(|(w, h, b)| ImageHandle::from_rgba(w, h, b, AlphaType::Unpremul)));
		}
		let extents = *states.extents.peek();
		let request = shared.peek(|app| request_of(app, extents, self.scene.edges.as_ref()));
		match request {
			Some(request) => {
				let key = (request.source.key(), request.layers.clone());
				if self.sent.as_ref() != Some(&key) {
					self.sent = Some(key);
					self.awaiting = true;
					self.presenter.request(request);
				}
			}
			None => {
				if shared.peek(|app| app.live.shown().is_none()) && self.sent.take().is_some() {
					self.awaiting = false;
					states.pictures.set([None, None]);
				}
			}
		}
		let (computing, position) = shared.peek(|app| (app.live.computing_frame(), app.live.playback.position()));
		self.computing = match (computing, self.computing) {
			(Some(f), Some((g, since))) if f == g => Some((g, since)),
			(Some(f), _) => Some((f, now)),
			(None, _) => None,
		};
		let busy = self.computing.is_some_and(|(f, since)| f == position && now - since >= BUSY_DELAY);
		states.busy.set_if_modified(busy);
		let fit = *states.fit.peek();
		if let Some(layer) = fit {
			let ready = shared.peek(|app| match (app.live.shown(), app.live.latest_scratch()) {
				(Some(Shown::Scratch(k)), Some(r))
					if k == position && r.snapshot.shape.format == SampleFormat::Half =>
				{
					Some(FrameSource::Scratch(r.clone()))
				}
				_ => None,
			});
			if let Some(source) = ready {
				states.fit.set(None);
				fit_now(shared, &source, layer);
			} else if !shared.peek(|app| app.live.scratch_pending()) {
				states.fit.set(None);
				tracing::debug!("auto brightness dropped: the 16-bit frame did not arrive");
			}
		}
	}
}

/// Turns a context menu entry into its effect.
fn activate(shared: &Shared, states: PaneStates, entry: PictureMenu) {
	match entry {
		PictureMenu::AutoBrightness(layer) => auto_brightness(shared, states, layer),
		PictureMenu::Shell(action) => match try_consume_context::<PreviewActions>() {
			Some(PreviewActions(handler)) => handler.call(action),
			None => tracing::debug!(?action, "no handler for the preview's menu"),
		},
	}
}

/// The splitter between the pictures: drag or arrows (10 px a key) resize, double-click or Enter resets to half.
#[derive(Clone, PartialEq)]
struct Splitter {
	split: State<f32>,
	/// The pictures' container along the split, logical pixels.
	length: f32,
	vertical: bool,
	tooltip: String,
}

impl Component for Splitter {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let mut drag = use_state(|| None::<(f64, f32)>);
		let mut last_down = use_state(|| None::<Instant>);
		let mut split = self.split;
		let length = self.length.max(1.);
		let vertical = self.vertical;
		let along = move |p: CursorPoint| if vertical { p.y } else { p.x };
		let color = if focus() == Focus::Keyboard || drag.read().is_some() { theme.accent } else { theme.splitter };
		let bar = rect()
			.a11y_id(id)
			.a11y_focusable(true)
			.test_id("PreviewSplitter")
			.background(color)
			.maybe(vertical, |r| r.width(Size::fill()).height(Size::px(4.)))
			.maybe(!vertical, |r| r.height(Size::fill()).width(Size::px(4.)))
			.on_pointer_down(move |e: Event<PointerEventData>| {
				id.request_focus();
				let now = Instant::now();
				if last_down.peek().is_some_and(|t| now - t < Duration::from_millis(400)) {
					split.set(0.5);
					last_down.set(None);
					return;
				}
				last_down.set(Some(now));
				drag.set(Some((along(e.global_location()), *split.peek())));
			})
			.on_global_pointer_move(move |e: Event<PointerEventData>| {
				if let Some((start, from)) = *drag.peek() {
					let d = (along(e.global_location()) - start) as f32 / length;
					split.set((from + d).clamp(0.05, 0.95));
				}
			})
			.on_global_pointer_up(move |_| {
				if drag.peek().is_some() {
					drag.set(None);
				}
			})
			.on_key_down(move |e: Event<KeyboardEventData>| {
				let step = 10. / length;
				let delta = match (&e.key, vertical) {
					(Key::Named(NamedKey::ArrowUp), true) | (Key::Named(NamedKey::ArrowLeft), false) => -step,
					(Key::Named(NamedKey::ArrowDown), true) | (Key::Named(NamedKey::ArrowRight), false) => step,
					(Key::Named(NamedKey::Enter), _) => {
						split.set(0.5);
						e.prevent_default();
						e.stop_propagation();
						return;
					}
					_ => return,
				};
				e.prevent_default();
				e.stop_propagation();
				let v = (*split.peek() + delta).clamp(0.05, 0.95);
				split.set(v);
			});
		TooltipContainer::new(Tooltip::new_text(self.tooltip.clone())).child(bar)
	}
}

/// The pane's keys, bubbled from whatever inside the pane has focus: Space plays or pauses, `,` and `.` step,
/// I, O and X set and clear the range, L loops, F follows the run.
fn on_pane_key(shared: &Shared, e: &Event<KeyboardEventData>) {
	if !e.modifiers.is_empty() && e.modifiers != Modifiers::SHIFT {
		return;
	}
	let Key::Character(c) = &e.key else { return };
	let event = match c.to_lowercase().as_str() {
		" " => {
			transport::toggle_playback(shared);
			e.prevent_default();
			return;
		}
		"," => AppEvent::StepFrames(-1),
		"." => AppEvent::StepFrames(1),
		"i" => AppEvent::SetInPoint,
		"o" => AppEvent::SetOutPoint,
		"x" => AppEvent::ClearRange,
		"l" => AppEvent::SetLoop(!shared.peek(|app| app.live.playback.looping())),
		"f" => AppEvent::SetLive(true),
		_ => return,
	};
	e.prevent_default();
	shared.send(event);
}

/// The preview pane: the pictures, the timeline and the transport bar.
pub fn preview_pane() -> impl IntoElement {
	PreviewPane
}

#[derive(Clone, PartialEq)]
struct PreviewPane;

impl Component for PreviewPane {
	fn render(&self) -> impl IntoElement {
		let shared = Shared::get();
		let theme = use_wave_theme();
		let states = PaneStates::use_new();
		let mut size = use_state(|| (0f32, 0f32));
		use_hook({
			let shared = shared.clone();
			move || {
				spawn(async move {
					let presenter = match Presenter::start() {
						Ok(p) => p,
						Err(e) => {
							tracing::error!("the preview presenter could not start: {e}");
							return;
						}
					};
					let mut poller = Poller {
						presenter,
						sent: None,
						awaiting: false,
						stats: PresenterStats::default(),
						scene: SceneWatch::default(),
						computing: None,
					};
					loop {
						timer(poller.next_wait(&shared, states)).await;
						poller.poll(&shared, states);
					}
				});
			}
		});
		let (show_field, show_intensity, vertical, tooltip) = shared.read(Topics::DOCUMENT | Topics::SETTINGS, |app| {
			let d = &app.document.project.display;
			(
				d.field.preview,
				d.intensity.preview,
				app.settings.preview_orientation == Orientation::Vertical,
				app.strings.get("TipPreviewSplitter").to_owned(),
			)
		});
		let split = *states.split.read();
		let (w, h) = *size.read();
		let length = if vertical { h } else { w };
		let cell = |layer: Layer, share: f32| {
			rect()
				.maybe(vertical, |r| r.width(Size::fill()).height(Size::flex(share)))
				.maybe(!vertical, |r| r.height(Size::fill()).width(Size::flex(share)))
				.child(PictureCell { layer, states })
		};
		let pictures = rect()
			.width(Size::fill())
			.height(Size::flex(1.))
			.content(Content::flex())
			.maybe(!vertical, |r| r.horizontal())
			.on_sized(move |e: Event<SizedEventData>| size.set_if_modified((e.area.width(), e.area.height())))
			.maybe_child(show_field.then(|| cell(Layer::Field, if show_intensity { split } else { 1. })))
			.maybe_child((show_field && show_intensity).then(|| Splitter {
				split: states.split,
				length: length - 4.,
				vertical,
				tooltip: tooltip.clone(),
			}))
			.maybe_child(show_intensity.then(|| cell(Layer::Intensity, if show_field { 1. - split } else { 1. })));
		let key_shared = shared.clone();
		let menu_shared = shared.clone();
		rect()
			.width(Size::fill())
			.height(Size::fill())
			.content(Content::flex())
			.background(theme.window_background)
			.color(theme.text)
			.font_size(theme.font_size)
			.test_id("PreviewPane")
			.on_key_down(move |e: Event<KeyboardEventData>| on_pane_key(&key_shared, &e))
			.child(pictures)
			.child(transport::Transport)
			.child(ContextMenuHost::new(states.menu, move |entry: PictureMenu| activate(&menu_shared, states, entry)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn picture_menu_entries_carry_their_layer() {
		use PictureMenu::{AutoBrightness, Shell};
		let s = Strings::new(wave_app_state::strings::Language::English);
		let entries: Vec<(String, PictureMenu)> = picture_context_menu(&s, Layer::Intensity)
			.entries
			.into_iter()
			.filter_map(|e| match e {
				wave_widgets::MenuEntry::Item(i) => Some((i.id, i.action)),
				_ => None,
			})
			.collect();
		let expected = [
			("pic:brightness:intensity", Shell(PreviewAction::Brightness(Layer::Intensity))),
			("pic:auto:intensity", AutoBrightness(Layer::Intensity)),
			("pic:frame:intensity", Shell(PreviewAction::ExportFrame(Layer::Intensity))),
			("pic:export:field", Shell(PreviewAction::ExportField)),
			("pic:export:intensity", Shell(PreviewAction::ExportIntensity)),
			("pic:export:both", Shell(PreviewAction::ExportBoth)),
		]
		.map(|(id, action)| (id.to_owned(), action));
		assert_eq!(entries, expected);
	}
}
