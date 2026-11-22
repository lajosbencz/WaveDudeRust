//! The preview's logic without its pane: the presenter's requests from the frame shown, the scene's own
//! picture and outline, Auto brightness, the overlay's edits, the probe graph boxes' placement and the timeline.
//!
//! Threads: the UI thread; natively the presenter and the scene builder use threads of their own.

pub mod graph_boxes;
pub mod handles;
pub mod overlay;
pub mod presenter;
pub mod timeline;
pub mod view;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::Event;
use crate::clipboard::{Copyable, PasteTarget, write_fragment};
use crate::commands::Edit;
use crate::live::ShownSource;
use crate::platform::{Duration, Instant};
use waveduderust::model::{DisplayLayerEdit, Layer, Point, Project, SceneMode, ViewEdit};
use waveduderust::unstable::render::fit_brightness;
use waveduderust::unstable::render::layer::LayerColours;
use waveduderust::unstable::scene::compose::SceneKey;
use waveduderust::unstable::scene::glowing_edges::EdgeImage;
use waveduderust::unstable::scene::pixels::ScenePixels;
use waveduderust::unstable::spec::SceneCache;
use waveduderust::unstable::sync::lock;
use waveduderust::unstable::view::epaint::{Pos2, Rect};

use self::presenter::{FrameSource, LayerRequest, Outline, Request, SourceKey, as_halves};
use crate::App;

/// How often the pane polls the state and the presenter while a picture is due; at [`crate::window::TICK`]
/// otherwise.
pub const POLL: Duration = Duration::from_millis(4);
/// A frame computed for longer than this dims the pictures under a spinner.
pub const BUSY_DELAY: Duration = Duration::from_millis(150);

/// A playback key of the pictures (docs/ui/10, Preview pane).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PaneKey {
	TogglePlay,
	StepBack,
	StepForward,
	SetIn,
	SetOut,
	ClearRange,
	ToggleLoop,
	ToggleLive,
	RangeStart,
	RangeEnd,
}

impl PaneKey {
	/// The string key naming its command.
	pub fn label_key(self) -> &'static str {
		match self {
			Self::TogglePlay => "MenuSimulationPlay",
			Self::StepBack => "TipPreviewStepBack",
			Self::StepForward => "TipPreviewStepForward",
			Self::SetIn => "MenuSimulationSetIn",
			Self::SetOut => "MenuSimulationSetOut",
			Self::ClearRange => "MenuSimulationClearRange",
			Self::ToggleLoop => "MenuSimulationLoop",
			Self::ToggleLive => "MenuSimulationLive",
			Self::RangeStart => "ShortcutsRangeStart",
			Self::RangeEnd => "ShortcutsRangeEnd",
		}
	}
}

/// What `key` asks the state for.
pub fn pane_event(app: &App, key: PaneKey) -> Event {
	let p = &app.live.playback;
	match key {
		PaneKey::TogglePlay => Event::TogglePlayback,
		PaneKey::StepBack => Event::StepFrames(-1),
		PaneKey::StepForward => Event::StepFrames(1),
		PaneKey::SetIn => Event::SetInPoint,
		PaneKey::SetOut => Event::SetOutPoint,
		PaneKey::ClearRange => Event::ClearRange,
		PaneKey::ToggleLoop => Event::SetLoop(!p.looping()),
		PaneKey::ToggleLive => Event::SetLive(!app.live.is_live()),
		PaneKey::RangeStart => Event::Scrub(p.range_frames().0),
		PaneKey::RangeEnd => Event::Scrub(p.range_frames().1),
	}
}

/// "Field" or "Intensity", for logs.
/// `objects` dropped on a picture at `p`: one paste of them as a group, placed on the canvas pixel there.
pub fn drop_paste(app: &App, objects: &[Copyable], t: &view::CanvasTransform, p: Pos2) -> Event {
	let text = write_fragment(&app.document.project, &app.document.view, objects);
	let (x, y) = t.to_canvas(p);
	let target = PasteTarget { current: app.selection.current(), before: false };
	Event::Paste { text, target, place_at: Some(Point::new(x.floor() as i32, y.floor() as i32)) }
}

pub fn layer_name(layer: Layer) -> &'static str {
	match layer {
		Layer::Field => "Field",
		Layer::Intensity => "Intensity",
		_ => "Unknown",
	}
}

/// A picture's visible size in device pixels and the canvas part it shows; `None` is all of it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PictureExtent {
	pub device: (u32, u32),
	pub window: Option<Rect>,
}

/// The edit of `layer`'s view settings by `edit`; `None` for a layer the view lacks.
pub fn display_edit(layer: Layer, edit: DisplayLayerEdit) -> Option<Edit> {
	Some(Edit::View(match layer {
		Layer::Field => ViewEdit::Field(edit),
		Layer::Intensity => ViewEdit::Intensity(edit),
		_ => return None,
	}))
}

/// The frame the pictures show, as a presenter source.
pub fn shown_source(app: &App) -> Option<FrameSource> {
	Some(match app.live.shown_source()? {
		ShownSource::Stored(store, stored) => FrameSource::Run { store, stored },
		ShownSource::Snapshot { store, generation, snapshot } => FrameSource::Latest { store, generation, snapshot },
	})
}

/// Everything [`request_of`] reads, cheap to compare: the revisions of the project, the settings and the run
/// (motion), the frame shown, the pictures' extents and the outline's identity.
#[derive(PartialEq)]
pub struct RequestInputs {
	document: u64,
	settings: u64,
	run: u64,
	source: Option<SourceKey>,
	extents: [PictureExtent; 2],
	edges: Option<usize>,
}

impl RequestInputs {
	pub fn of(app: &App, extents: [PictureExtent; 2], edges: Option<&Arc<EdgeImage>>) -> Self {
		let r = app.revisions();
		Self {
			document: r.document,
			settings: r.settings,
			run: r.run,
			source: shown_source(app).map(|s| s.key()),
			extents,
			edges: edges.map(|e| Arc::as_ptr(e) as usize),
		}
	}
}

/// The presenter's request for the frame shown, `None` without one.
pub fn request_of(app: &App, extents: [PictureExtent; 2], edges: Option<&Arc<EdgeImage>>) -> Option<Request> {
	let source = shown_source(app)?;
	let (project, view) = (&app.document.project, &app.document.view);
	let dither_bits = if app.settings.preview_sample_bytes >= 2 { app.settings.preview_dither_bits } else { 0 };
	let layers = [Layer::Field, Layer::Intensity]
		.into_iter()
		.filter(|&l| view.layer(l).preview)
		.map(|layer| {
			let settings = view.layer(layer);
			let colours = LayerColours::of(settings, &project.color_schemes, &app.schemes);
			let extent = &extents[layer as usize];
			LayerRequest {
				layer,
				outline: LayerColours::overlay_gain(settings)
					.zip(edges)
					.map(|(gain, e)| Outline { edges: e.clone(), gain }),
				colours,
				device: extent.device,
				window: extent.window.map(|w| (w.min.x.into(), w.min.y.into(), w.width().into(), w.height().into())),
				dither_bits,
			}
		})
		.collect();
	let motion = app.live.playback.is_playing() || app.live.is_live();
	Some(Request { source, layers, motion, at: Instant::now() })
}

/// The brightness that fits `layer` of a 16-bit frame; `None` when the frame has nothing to fit.
pub fn fit(source: &FrameSource, layer: Layer) -> Option<f64> {
	let shape = source.shape();
	source.with_plane(layer, |bytes| fit_brightness(&as_halves(bytes), shape.sample_kind(layer))).flatten()
}

/// What the scene outline and the scene picture are built from: the scene's key (everything the painter reads,
/// the seed of a seeded part included) and whether a layer shows the outline.
#[derive(Clone, Copy, PartialEq)]
struct SceneInputs {
	key: SceneKey,
	image_scene: bool,
	edges: bool,
}

impl SceneInputs {
	fn of(app: &App, key: SceneKey) -> Self {
		let (project, view) = (&app.document.project, &app.document.view);
		Self {
			key,
			image_scene: project.scene.mode == SceneMode::Image,
			edges: view.field.overlay || view.intensity.overlay,
		}
	}
}

struct SceneArt {
	image: Option<Arc<ScenePixels>>,
	edges: Option<Arc<EdgeImage>>,
}

/// A scene to draw.
struct SceneJob {
	project: Project,
	edges: bool,
}

impl SceneJob {
	fn draw(&self, cache: &Mutex<SceneCache>) -> SceneArt {
		let (image, pixels) = {
			let mut cache = lock(cache);
			(cache.image(&self.project), self.edges.then(|| cache.pixels(&self.project)).flatten())
		};
		SceneArt { image, edges: pixels.map(|p| Arc::new(EdgeImage::of(&p))) }
	}
}

/// Natively the scene thread's inbox and outbox, each job and art with its generation; on wasm32, where scenes are
/// drawn in the call, the last art.
#[cfg(not(target_arch = "wasm32"))]
type SceneWorker = (Sender<(u64, SceneJob)>, Receiver<(u64, SceneArt)>);
#[cfg(target_arch = "wasm32")]
type SceneWorker = Option<SceneArt>;

/// Builds the scene's own picture (an image scene) and its glowing edges (when a layer shows the outline) when the
/// scene changes: natively on one thread, where a newer scene supersedes one not started yet.
#[derive(Default)]
pub struct SceneWatch {
	inputs: Option<SceneInputs>,
	/// The scene key of the document revision it was hashed at; a key costs a pass over the parts.
	keyed: Option<(u64, SceneKey)>,
	generation: u64,
	worker: Option<SceneWorker>,
	building: bool,
	edges: Option<Arc<EdgeImage>>,
}

impl SceneWatch {
	/// A build is in flight.
	pub fn building(&self) -> bool {
		self.building
	}

	/// The glowing edges of the scene, while a layer shows the outline.
	pub fn edges(&self) -> Option<&Arc<EdgeImage>> {
		self.edges.as_ref()
	}

	/// Polls; returns the scene picture when a build finished.
	pub fn poll(&mut self, app: &App) -> Option<Option<Arc<ScenePixels>>> {
		let document = app.revisions().document;
		let key = match self.keyed {
			Some((at, key)) if at == document => key,
			_ => {
				let key = SceneKey::of(&app.document.project);
				self.keyed = Some((document, key));
				key
			}
		};
		let inputs = SceneInputs::of(app, key);
		if self.inputs != Some(inputs) {
			self.inputs = Some(inputs);
			self.generation += 1;
			if !(inputs.image_scene || inputs.edges) {
				self.building = false;
				self.edges = None;
				return Some(None);
			}
			let mut project = app.document.project.clone();
			project.probes.clear();
			project.sources.clear();
			self.building = self.send(app, SceneJob { project, edges: inputs.edges });
		}
		let art = self.receive()?;
		self.building = false;
		self.edges = art.edges;
		Some(art.image)
	}

	/// Hands `job` to the scene thread, started on first use; whether it took it.
	#[cfg(not(target_arch = "wasm32"))]
	fn send(&mut self, app: &App, job: SceneJob) -> bool {
		if self.worker.is_none() {
			let cache = app.live.backend().scene_cache();
			let (jobs, inbox) = channel();
			let (outbox, arts) = channel();
			let spawned = std::thread::Builder::new()
				.name("preview-scene".into())
				.spawn(move || draw_scenes(&cache, &inbox, &outbox));
			match spawned {
				Ok(_) => self.worker = Some((jobs, arts)),
				Err(e) => tracing::warn!("the scene outline could not be built: {e}"),
			}
		}
		let sent = self.worker.as_ref().is_some_and(|(jobs, _)| jobs.send((self.generation, job)).is_ok());
		if !sent {
			self.worker = None;
		}
		sent
	}

	#[cfg(not(target_arch = "wasm32"))]
	fn receive(&mut self) -> Option<SceneArt> {
		let (_, art) = self.worker.as_ref()?.1.try_iter().filter(|(g, _)| *g == self.generation).last()?;
		Some(art)
	}

	/// Draws `job`.
	#[cfg(target_arch = "wasm32")]
	fn send(&mut self, app: &App, job: SceneJob) -> bool {
		self.worker = Some(Some(job.draw(&app.live.backend().scene_cache())));
		true
	}

	#[cfg(target_arch = "wasm32")]
	fn receive(&mut self) -> Option<SceneArt> {
		self.worker.as_mut()?.take()
	}
}

/// The scene worker: draws the newest job waiting, until the watch is gone.
#[cfg(not(target_arch = "wasm32"))]
fn draw_scenes(cache: &Mutex<SceneCache>, inbox: &Receiver<(u64, SceneJob)>, outbox: &Sender<(u64, SceneArt)>) {
	while let Ok(job) = inbox.recv() {
		let (generation, job) = inbox.try_iter().last().unwrap_or(job);
		if outbox.send((generation, job.draw(cache))).is_err() {
			return;
		}
	}
}
