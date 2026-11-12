//! A project opened for the browser: the run spec the application builds for a tier, its engine, the frames
//! stepped to so far, the probe traces, the pictures of their layers and the shapes over them. Nothing here
//! touches JavaScript.
//!
//! The traces only grow: a frame behind the engine is run again from step 0, which queues no reading until it
//! passes the steps recorded, as it reads the same values. A picture's graphs are cut at its frame.

use std::future::Future;

use wavedude_core::engine::probes::{ProbeCollector, ProbeTrace, record};
use wavedude_core::engine::{self, EngineError, GpuHandle, Snapshot, WaveEngine};
use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::io::{self, FormatError, wdproj};
use wavedude_core::model::{Layer, Project, SceneMode, Word};
use wavedude_core::quality::{PreviewDefaults, PreviewTier};
use wavedude_core::render::ScreenRenderer;
use wavedude_core::render::layer::{LayerColours, View, paint};
use wavedude_core::scene::glowing_edges::{self, EdgeImage};
use wavedude_core::spec::{RunRequest, RunSpec, RunSpecError, SceneCache};

use crate::cache::FrameCache;
use crate::overlay::{self, Fonts, Group, Screen};

/// Bytes the frame cache keeps unless asked otherwise.
pub(crate) const DEFAULT_BUDGET: u64 = 512 << 20;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionError {
	#[error(transparent)]
	Format(#[from] FormatError),
	#[error("the scene image cannot be decoded: {0}")]
	Image(String),
	#[error(
		"the scene uses the image file '{0}', which the project does not embed; open a project with its image embedded"
	)]
	ExternalImage(String),
	#[error(transparent)]
	Spec(#[from] RunSpecError),
	#[error(transparent)]
	Engine(#[from] EngineError),
	#[error("the frame budget {0} is not a byte count")]
	Budget(f64),
	#[error("'{0}' is not a tier; expected Low, Middle or High")]
	Tier(String),
	#[error("'{0}' is not a layer; expected field or intensity")]
	Layer(String),
	#[error("the {} layer is not shown by this project", layer_name(*.0))]
	LayerHidden(Layer),
	#[error("frame {frame} is outside the timeline of {count} frames")]
	Frame { frame: u32, count: u32 },
	#[error("a picture of {width} x {height} pixels at {dpr} device pixels per pixel cannot be drawn")]
	Screen { width: f32, height: f32, dpr: f32 },
}

pub(crate) fn tier_of(name: &str) -> Result<PreviewTier, SessionError> {
	PreviewTier::parse(name).ok_or_else(|| SessionError::Tier(name.to_owned()))
}

pub(crate) fn layer_of(name: &str) -> Result<Layer, SessionError> {
	Layer::MEMBERS.iter().copied().find(|l| layer_name(*l) == name).ok_or_else(|| SessionError::Layer(name.to_owned()))
}

pub(crate) fn layer_name(layer: Layer) -> &'static str {
	match layer {
		Layer::Field => "field",
		Layer::Intensity => "intensity",
	}
}

/// An RGBA8 picture, unpremultiplied, alpha 255.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Picture {
	pub(crate) width: u32,
	pub(crate) height: u32,
	pub(crate) rgba: Vec<u8>,
}

/// How one shown layer is coloured, and its outline scaled to the size last drawn.
struct Painter {
	layer: Layer,
	colours: LayerColours,
	outline_gain: Option<f64>,
	screen: ScreenRenderer,
	scaled: Option<((u32, u32), Vec<u8>)>,
}

pub(crate) struct Session {
	project: Project,
	spec: RunSpec,
	engine: Box<dyn WaveEngine>,
	/// None without active probes.
	probes: Option<ProbeCollector>,
	/// Steps whose probe readings are in `probes`.
	recorded: u32,
	frames: FrameCache,
	painters: Vec<Painter>,
	edges: Option<EdgeImage>,
	gpu_fallback: bool,
	/// The device the engine runs on, for the readback wait.
	gpu: Option<GpuHandle>,
	dither_bits: u32,
	fps: u32,
}

impl Session {
	/// Opens the project `text` at `tier` on `gpu`, else on the CPU, keeping at most `budget` bytes of frames.
	pub(crate) async fn open(
		text: &str,
		tier: PreviewTier,
		gpu: Option<GpuHandle>,
		budget: u64,
	) -> Result<Self, SessionError> {
		let mut project = wdproj::read_str(text)?;
		let scene = &project.scene;
		if scene.mode == SceneMode::Image && scene.image_data.is_none() && !scene.image_path.is_empty() {
			return Err(SessionError::ExternalImage(project.scene.image_path));
		}
		io::read_embedded_image_size(&mut project.scene).map_err(SessionError::Image)?;
		let request = RunRequest::new(&project, PreviewDefaults::quality_of(tier), false);
		let mut scenes = SceneCache::default();
		let spec = request.spec(&mut scenes)?;
		let gpu_requested = gpu.is_some();
		let device = gpu.clone();
		let mut engine = engine::build_on(spec.scene.clone(), spec.options.clone(), gpu).await;
		let gpu_fallback = gpu_requested && !engine.is_gpu();
		let gpu = device.filter(|_| engine.is_gpu());
		let probes = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene, spec.seed));
		engine.set_emitters(&spec.emitters);
		engine.set_probes(probes.as_ref().map(ProbeCollector::cells).unwrap_or_default());
		engine.reset();
		let schemes = SchemeStore::default();
		let painters: Vec<Painter> = [Layer::Field, Layer::Intensity]
			.into_iter()
			.filter(|&l| project.display.layer(l).preview)
			.map(|layer| Painter {
				layer,
				colours: LayerColours::of(&project, layer, &schemes),
				outline_gain: LayerColours::overlay_gain(&project, layer),
				screen: ScreenRenderer::new(1, 1, 1, 1),
				scaled: None,
			})
			.collect();
		let edges = painters
			.iter()
			.any(|p| p.outline_gain.is_some())
			.then(|| scenes.pixels(&project))
			.flatten()
			.map(|p| EdgeImage::of(&p));
		Ok(Self {
			spec,
			engine,
			probes,
			recorded: 0,
			frames: FrameCache::new(budget),
			painters,
			edges,
			gpu_fallback,
			gpu,
			dither_bits: PreviewDefaults::dither_bits_of(tier),
			fps: project.display.video.fps,
			project,
		})
	}

	/// "gpu" or "cpu".
	pub(crate) fn engine_name(&self) -> &'static str {
		if self.engine.is_gpu() { "gpu" } else { "cpu" }
	}

	/// The GPU was asked for and the CPU runs instead.
	pub(crate) fn gpu_fell_back(&self) -> bool {
		self.gpu_fallback
	}

	/// Frames per second of the project's video.
	pub(crate) fn fps(&self) -> u32 {
		self.fps
	}

	pub(crate) fn spec(&self) -> &RunSpec {
		&self.spec
	}

	/// The layers the project shows, in drawing order.
	pub(crate) fn layers(&self) -> Vec<Layer> {
		self.painters.iter().map(|p| p.layer).collect()
	}

	/// The probe traces in the spec's probe order, the steps recorded so far.
	pub(crate) fn traces(&self) -> &[ProbeTrace] {
		self.probes.as_ref().map_or(&[], |p| &p.traces)
	}

	/// Stored frames now in the cache.
	#[cfg(test)]
	fn cached_frames(&self) -> usize {
		self.frames.len()
	}

	#[cfg(test)]
	fn cached_bytes(&self) -> u64 {
		self.frames.bytes()
	}

	/// The planes stored for timeline frame `frame`.
	#[cfg(test)]
	fn stored(&self, frame: u32) -> Option<&Snapshot> {
		self.frames.get(self.spec.stored_index_of(frame))
	}

	/// The timeline frame of the nearest stored frame at or before the one `frame` shows.
	pub(crate) fn latest_before(&self, frame: u32) -> Option<u32> {
		self.frames.latest_at_or_before(self.spec.stored_index_of(frame)).map(|i| i * self.spec.frame_divisor())
	}

	/// Steps at most `max_steps` toward the stored frame timeline frame `frame` shows and stores it on
	/// arriving, with the probe readings up to it recorded. A frame behind the engine's step is reached by running
	/// again from step 0. True once the frame is stored.
	pub(crate) async fn step_toward(&mut self, frame: u32, max_steps: u32) -> Result<bool, SessionError> {
		let count = self.spec.frame_count();
		if frame >= count {
			return Err(SessionError::Frame { frame, count });
		}
		let stored = self.spec.stored_index_of(frame);
		if self.frames.get(stored).is_some() {
			return Ok(true);
		}
		let steps = self.spec.capture_every() * stored + 1;
		if self.engine.time() > steps {
			self.engine.reset();
		}
		let behind = steps - self.engine.time();
		for _ in 0..behind.min(max_steps) {
			self.engine.advance_one_step()?;
			if self.probes.is_some() && self.engine.time() > self.recorded {
				self.engine.queue_probes()?;
			}
		}
		if self.engine.time() < steps {
			return Ok(false);
		}
		let pending = self.engine.begin_capture(&self.spec.shape, Snapshot::new(self.spec.shape))?;
		if let Some(collector) = &mut self.probes {
			let readings = self.engine.pending_probes()?;
			settle(readings.ready(), self.gpu.as_ref()).await;
			record(self.engine.as_mut(), collector, &mut self.recorded, self.spec.visualize_every)?;
		}
		settle(pending.ready(), self.gpu.as_ref()).await;
		let snapshot = self.engine.finish_capture(pending)?;
		self.frames.insert(stored, snapshot);
		Ok(true)
	}

	/// The shapes over the picture of `layer` at timeline frame `frame` drawn at `screen`: what the desktop
	/// preview draws over it with nothing selected, the graphs cut at the stored frame the picture shows.
	pub(crate) fn overlay(
		&self,
		frame: u32,
		layer: Layer,
		screen: Screen,
		fonts: Fonts<'_>,
	) -> Result<Vec<Group>, SessionError> {
		let count = self.spec.frame_count();
		if frame >= count {
			return Err(SessionError::Frame { frame, count });
		}
		if !self.painters.iter().any(|p| p.layer == layer) {
			return Err(SessionError::LayerHidden(layer));
		}
		let Screen { width, height, dpr } = screen;
		if ![width, height, dpr].iter().all(|v| v.is_finite() && *v > 0.) {
			return Err(SessionError::Screen { width, height, dpr });
		}
		let shown = self.spec.stored_index_of(frame) * self.spec.frame_divisor();
		Ok(overlay::groups(&self.project, &self.spec, self.traces(), shown, layer, screen, fonts))
	}

	/// The picture of `layer` at timeline frame `frame`; `None` when its stored frame is not in the cache. An
	/// 8-bit frame is coloured at its stored size, a 16-bit one at `device` pixels.
	pub(crate) fn render(
		&mut self,
		frame: u32,
		layer: Layer,
		device: (u32, u32),
	) -> Result<Option<Picture>, SessionError> {
		let count = self.spec.frame_count();
		if frame >= count {
			return Err(SessionError::Frame { frame, count });
		}
		let painter = self.painters.iter_mut().find(|p| p.layer == layer).ok_or(SessionError::LayerHidden(layer))?;
		let Some(snapshot) = self.frames.get(self.spec.stored_index_of(frame)) else {
			return Ok(None);
		};
		let shape = &self.spec.shape;
		let view = View::of(shape, device, None, self.dither_bits);
		let mut rgba = vec![0; view.bytes()];
		paint(snapshot.plane(layer), shape, layer, &painter.colours, &view, &mut painter.screen, &mut rgba);
		if let (Some(gain), Some(edges)) = (painter.outline_gain, &self.edges) {
			let size = (view.width, view.height);
			if painter.scaled.as_ref().is_none_or(|(scaled_to, _)| *scaled_to != size) {
				painter.scaled = Some((size, edges.scaled(None, size)));
			}
			let (_, scaled) = painter.scaled.as_ref().expect("the outline was just scaled");
			glowing_edges::blend_additive(&mut rgba, scaled, gain);
		}
		Ok(Some(Picture { width: view.width, height: view.height, rgba }))
	}
}

/// Waits for a readback in a browser, on the GPU's queue when there is one; natively the engine's finishing call
/// waits instead.
async fn settle(ready: impl Future<Output = ()>, gpu: Option<&GpuHandle>) {
	#[cfg(target_arch = "wasm32")]
	match gpu {
		Some(gpu) => crate::wait::readback(ready, gpu).await,
		None => ready.await,
	}
	#[cfg(not(target_arch = "wasm32"))]
	let _ = (ready, gpu);
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use pollster::block_on;
	use wavedude_core::engine::cpu::CpuEngine;
	use wavedude_core::model::{Edges, Measurement, Probe, ProbeDetector, ProbeKind, Project};
	use wavedude_core::run::{FrameStore, RunState, SimulationRun, StoreLayout};
	use wavedude_view::draw::TextMetrics;
	use wavedude_view::graph::GraphData;

	use super::*;

	/// An 8 x 8 greyscale PNG.
	const PNG: [u8; 99] = [
		0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
		0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x00, 0x00, 0x00, 0x00, 0xe1, 0x64, 0xe1, 0x57, 0x00, 0x00, 0x00,
		0x2a, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x60, 0x10, 0x50, 0x30, 0x70, 0x08, 0x48, 0x28, 0x80, 0xd1,
		0x0d, 0x0c, 0x50, 0x7a, 0x02, 0x03, 0x94, 0x5e, 0xc0, 0x00, 0xa5, 0x37, 0x30, 0x40, 0xe9, 0x03, 0x0c, 0x50,
		0xfa, 0x02, 0x03, 0x94, 0x7e, 0x00, 0x00, 0x1e, 0x75, 0x1c, 0x01, 0x9f, 0x44, 0x8f, 0x6d, 0x00, 0x00, 0x00,
		0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
	];

	/// Example 1 with a short timeline of 10 frames of 4 steps, changed by `edit`, as project text.
	fn text_with(edit: impl FnOnce(&mut Project)) -> String {
		let mut project = wavedude_examples::all()[0].load().unwrap();
		project.simulation.steps = 40;
		project.simulation.visualize_every = 4;
		edit(&mut project);
		wdproj::write_string(&project, None)
	}

	fn open(text: &str, tier: PreviewTier) -> Session {
		block_on(Session::open(text, tier, None, DEFAULT_BUDGET)).unwrap()
	}

	/// Steps in small bites, as a page does, until `frame` is stored.
	fn seek(session: &mut Session, frame: u32) {
		while !block_on(session.step_toward(frame, 7)).unwrap() {}
	}

	#[test]
	fn a_timeline_frame_is_the_engine_stepped_by_hand() {
		for tier in [PreviewTier::Middle, PreviewTier::Low] {
			let mut session = open(&text_with(|_| {}), tier);
			seek(&mut session, 7);
			let spec = session.spec().clone();
			let first_shown = spec.stored_index_of(7) * spec.frame_divisor();
			let mut by_hand = CpuEngine::new(spec.scene.clone(), spec.options.clone());
			by_hand.set_emitters(&spec.emitters);
			by_hand.reset();
			for _ in 0..=spec.step_of(first_shown) {
				by_hand.advance_one_step();
			}
			let expected = by_hand.capture_frame(&spec.shape);
			let stored = session.stored(7).unwrap();
			assert_eq!(stored.field, expected.field, "{tier:?}");
			assert_eq!(stored.intensity, expected.intensity, "{tier:?}");
			assert!(stored.field.iter().any(|&b| b != stored.field[0]), "the frame is not blank");
		}
	}

	#[test]
	fn a_frame_behind_the_engine_is_run_again_from_the_start() {
		let text = text_with(|_| {});
		let mut probe = open(&text, PreviewTier::Middle);
		seek(&mut probe, 2);
		let frame_bytes = probe.cached_bytes();
		let mut session = block_on(Session::open(&text, PreviewTier::Middle, None, 2 * frame_bytes)).unwrap();
		seek(&mut session, 2);
		let first = session.stored(2).unwrap().field.clone();
		seek(&mut session, 5);
		seek(&mut session, 8);
		assert!(session.stored(2).is_none(), "the farthest frame is evicted");
		assert!(session.stored(8).is_some() && session.stored(5).is_some());
		assert_eq!(session.latest_before(3), None);
		assert_eq!(session.latest_before(6), Some(5));
		seek(&mut session, 2);
		assert_eq!(session.stored(2).unwrap().field, first);
		assert!(session.cached_bytes() <= 2 * frame_bytes && session.cached_frames() == 2);
	}

	#[test]
	fn a_stored_frame_is_a_lookup_and_a_far_one_takes_many_calls() {
		let mut session = open(&text_with(|_| {}), PreviewTier::Middle);
		assert!(!block_on(session.step_toward(9, 10)).unwrap());
		assert!(!block_on(session.step_toward(9, 10)).unwrap());
		seek(&mut session, 9);
		assert!(block_on(session.step_toward(9, 0)).unwrap());
		assert!(matches!(block_on(session.step_toward(10, 1)), Err(SessionError::Frame { frame: 10, count: 10 })));
	}

	#[test]
	fn the_frame_divisor_stores_every_other_timeline_frame() {
		let mut session = open(&text_with(|_| {}), PreviewTier::Low);
		assert_eq!((session.spec().frame_count(), session.spec().stored_frames()), (10, 5));
		seek(&mut session, 5);
		assert_eq!(session.cached_frames(), 1);
		assert!(session.stored(4).is_some() && session.stored(5).is_some() && session.stored(6).is_none());
		assert_eq!(session.latest_before(5), Some(4));
	}

	#[test]
	fn an_embedded_image_gives_the_canvas_its_size() {
		let text = text_with(|p| {
			p.scene.mode = SceneMode::Image;
			p.scene.image_path = "tiny.png".into();
			p.scene.image_data = Some(Arc::from(PNG.as_slice()));
		});
		let mut session = open(&text, PreviewTier::Middle);
		assert_eq!((session.spec().scene.canvas_width, session.spec().scene.canvas_height), (8, 8));
		seek(&mut session, 1);
		assert!(session.render(1, Layer::Field, (0, 0)).unwrap().is_some());
	}

	#[test]
	fn an_external_image_is_refused() {
		let text = text_with(|p| {
			p.scene.mode = SceneMode::Image;
			p.scene.image_path = "elsewhere/tiny.png".into();
		});
		let error = block_on(Session::open(&text, PreviewTier::Middle, None, DEFAULT_BUDGET)).err().unwrap();
		assert!(matches!(&error, SessionError::ExternalImage(path) if path == "elsewhere/tiny.png"), "{error}");
		assert!(error.to_string().contains("elsewhere/tiny.png"));
	}

	#[test]
	fn an_image_scene_without_an_image_is_refused_by_the_spec() {
		let text = text_with(|p| {
			p.scene.mode = SceneMode::Image;
			p.scene.image_path.clear();
			p.parts.clear();
		});
		let error = block_on(Session::open(&text, PreviewTier::Middle, None, DEFAULT_BUDGET)).err().unwrap();
		assert!(matches!(error, SessionError::Spec(RunSpecError::NoImage)), "{error}");
	}

	#[test]
	fn a_file_that_is_not_a_project_is_refused() {
		let error = block_on(Session::open("not xml", PreviewTier::Middle, None, DEFAULT_BUDGET)).err().unwrap();
		assert!(matches!(error, SessionError::Format(_)));
	}

	#[test]
	fn a_project_without_a_seed_opens() {
		let text = text_with(|_| {});
		let (before, rest) = text.split_once(" Seed=\"").expect("the project names its seed");
		let without = format!("{before}{}", rest.split_once('"').unwrap().1);
		assert!(!without.contains("Seed="));
		let mut session = open(&without, PreviewTier::Middle);
		seek(&mut session, 1);
		assert!(session.stored(1).is_some());
	}

	#[test]
	fn the_picture_is_sized_by_the_tier() {
		let text = text_with(|_| {});
		let canvas = {
			let session = open(&text, PreviewTier::Middle);
			(session.spec().scene.canvas_width, session.spec().scene.canvas_height)
		};
		let mut middle = open(&text, PreviewTier::Middle);
		assert!(middle.render(3, Layer::Field, (50, 40)).unwrap().is_none(), "frame 3 is not stored yet");
		seek(&mut middle, 3);
		let picture = middle.render(3, Layer::Field, (50, 40)).unwrap().unwrap();
		assert_eq!((picture.width, picture.height), canvas, "8-bit frames ignore the device size");
		assert_eq!(picture.rgba.len(), (canvas.0 * canvas.1 * 4) as usize);
		let mut low = open(&text, PreviewTier::Low);
		seek(&mut low, 3);
		let picture = low.render(3, Layer::Field, (50, 40)).unwrap().unwrap();
		assert_eq!((picture.width, picture.height), (canvas.0 / 2, canvas.1 / 2));
		let mut high = open(&text, PreviewTier::High);
		seek(&mut high, 3);
		let picture = high.render(3, Layer::Field, (50, 40)).unwrap().unwrap();
		assert_eq!((picture.width, picture.height, picture.rgba.len()), (50, 40, 50 * 40 * 4));
		let stored_size = high.render(3, Layer::Field, (0, 0)).unwrap().unwrap();
		assert_eq!((stored_size.width, stored_size.height), canvas);
	}

	#[test]
	fn the_outline_follows_the_layers_overlay_setting() {
		let drawn = |overlay: bool| {
			let mut session = open(&text_with(|p| p.display.field.overlay = overlay), PreviewTier::Middle);
			seek(&mut session, 1);
			session.render(1, Layer::Field, (0, 0)).unwrap().unwrap().rgba
		};
		assert_ne!(drawn(true), drawn(false));
	}

	#[test]
	fn the_layers_follow_the_display_preview_flags() {
		let layers = |field: bool, intensity: bool| {
			open(
				&text_with(|p| {
					p.display.field.preview = field;
					p.display.intensity.preview = intensity;
				}),
				PreviewTier::Middle,
			)
			.layers()
		};
		assert_eq!(layers(true, true), [Layer::Field, Layer::Intensity]);
		assert_eq!(layers(true, false), [Layer::Field]);
		assert_eq!(layers(false, true), [Layer::Intensity]);
		let mut session = open(&text_with(|p| p.display.intensity.preview = false), PreviewTier::Middle);
		seek(&mut session, 1);
		assert!(matches!(
			session.render(1, Layer::Intensity, (0, 0)),
			Err(SessionError::LayerHidden(Layer::Intensity))
		));
	}

	/// Example 1 with point, integrated, line and detector probes near its source, on both layers.
	fn with_probes(p: &mut Project) {
		let (x, y) = (p.sources[0].x as i32, p.sources[0].y as i32);
		let probe = |name: &str, layer| Probe { name: name.into(), layer, ax: x + 6, ay: y, ..Probe::default() };
		let line = |name: &str| Probe {
			kind: ProbeKind::Line,
			ax: x - 8,
			ay: y + 3,
			bx: x + 8,
			by: y + 5,
			..probe(name, Layer::Intensity)
		};
		p.probes = vec![
			probe("amplitude", Layer::Field),
			Probe { measurement: Measurement::IntegratedValue, ..probe("integrated", Layer::Intensity) },
			Probe { measurement: Measurement::Value, ..line("line") },
			Probe {
				measurement: Measurement::IntegratedValue,
				detector: ProbeDetector::Imaging,
				sensitivity: 1e3,
				..line("imaging")
			},
			Probe {
				measurement: Measurement::IntegratedValue,
				detector: ProbeDetector::Counting,
				sensitivity: 1e3,
				..line("counting")
			},
			Probe { active: false, ..probe("off", Layer::Field) },
		];
	}

	/// The traces of a native run of `spec` on the CPU, to its end.
	fn native_traces(spec: &RunSpec) -> Vec<ProbeTrace> {
		let store = Arc::new(FrameStore::new(StoreLayout::of(spec), 1 << 30, None).unwrap());
		let engine = Box::new(CpuEngine::new(spec.scene.clone(), spec.options.clone()));
		let mut run = SimulationRun::start(spec.clone(), store, engine, None).unwrap();
		assert_eq!(run.wait(), RunState::Completed);
		run.traces()
	}

	/// What the graphs of `traces` show at timeline frame `k`.
	fn graphs_at(traces: &[ProbeTrace], spec: &RunSpec, k: u32) -> Vec<(Vec<f32>, Option<Vec<f32>>, u64)> {
		let step = u64::from(spec.step_of(k));
		traces
			.iter()
			.map(|t| {
				let d = GraphData::at(t, step, k as usize);
				(d.series.to_vec(), d.profile.map(<[f32]>::to_vec), d.steps_sampled)
			})
			.collect()
	}

	#[test]
	fn the_graphs_at_a_frame_are_a_native_runs_after_seeking_back_and_forth() {
		let text = text_with(with_probes);
		let mut session = block_on(Session::open(&text, PreviewTier::Middle, None, 0)).unwrap();
		let spec = session.spec().clone();
		assert_eq!((spec.probes.len(), spec.frame_divisor()), (5, 1));
		let native = native_traces(&spec);
		let check = |session: &mut Session, k: u32| {
			seek(session, k);
			assert_eq!(graphs_at(session.traces(), &spec, k), graphs_at(&native, &spec, k), "frame {k}");
			assert!(session.traces()[1].series.iter().any(|&v| v != 0.), "the integrated probe reads the wave");
		};
		check(&mut session, 6);
		assert!(!block_on(session.step_toward(9, 7)).unwrap(), "readings queued, not drained");
		for k in [2, 9, 4] {
			check(&mut session, k);
		}
		let recorded = spec.step_of(9) as usize + 1;
		assert_eq!(session.traces()[0].series.len(), recorded, "a rerun records nothing twice");
		assert_eq!(session.traces()[0].series, native[0].series[..recorded]);
		assert_eq!(session.traces()[3].profiles.len(), 10);
	}

	fn kinds(group: &serde_json::Value) -> Vec<&str> {
		group["shapes"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect()
	}

	struct Fixed;

	impl TextMetrics for Fixed {
		fn line_height(&self) -> f32 {
			13.
		}

		fn width(&self, text: &str) -> f32 {
			text.len() as f32 * 6.
		}
	}

	const FONTS: Fonts<'static> = Fonts { labels: &Fixed, boxes: &Fixed };

	#[test]
	fn the_overlay_is_the_previews_shapes_as_json() {
		let text = text_with(|p| {
			with_probes(p);
			p.scene.edges = Edges::Reflective;
			p.scene.damping_edge = 0;
		});
		let mut session = open(&text, PreviewTier::Middle);
		seek(&mut session, 5);
		let screen = Screen { width: 300., height: 200., dpr: 2. };
		let groups = session.overlay(5, Layer::Intensity, screen, FONTS).unwrap();
		let text = serde_json::to_string(&groups).unwrap();
		assert!(!text.contains("antialias"), "the desktop's flags stay out");
		let json: serde_json::Value = serde_json::from_str(&text).unwrap();
		let groups = json.as_array().unwrap();
		assert_eq!(groups.len(), 2 + 4, "the edge line, the overlay, the four active intensity probes' boxes");
		assert_eq!(kinds(&groups[0]), ["Rect"]);
		assert_eq!(groups[0]["shapes"][0]["ink"]["color"], "#ff0000ff", "a reflective wall's red");
		assert_eq!((groups[0]["scale"].as_f64(), groups[1]["scale"].as_f64()), (Some(1.), Some(2.)));
		let overlay = kinds(&groups[1]);
		assert_eq!((overlay[0], overlay[overlay.len() - 1]), ("Clip", "Unclip"));
		for kind in ["Circle", "Line", "Rect", "Text"] {
			assert!(overlay.contains(&kind), "{kind} in {overlay:?}");
		}
		let curves = groups[2..].iter().filter(|g| kinds(g).contains(&"Path")).count();
		assert!(curves >= 2, "the integrated probe's and the line's curves");
		let title = &groups[2]["shapes"].as_array().unwrap().iter().find(|s| s["text"] == "integrated").unwrap();
		assert_eq!((title["baseline"].as_str(), title["size"].as_f64()), (Some("Top"), Some(22.)));
		assert!(matches!(
			session.overlay(5, Layer::Field, Screen { dpr: 0., ..screen }, FONTS),
			Err(SessionError::Screen { .. })
		));
	}

	#[test]
	fn the_graphs_over_a_picture_are_cut_at_the_stored_frame_it_shows() {
		let mut session = open(&text_with(with_probes), PreviewTier::Low);
		assert_eq!(session.spec().frame_divisor(), 2);
		seek(&mut session, 9);
		let screen = Screen { width: 300., height: 200., dpr: 1. };
		let json =
			|frame| serde_json::to_string(&session.overlay(frame, Layer::Intensity, screen, FONTS).unwrap()).unwrap();
		assert_eq!(json(5), json(4), "frame 5 shows the stored frame 4");
		assert_ne!(json(6), json(4));
	}

	#[test]
	fn tiers_and_layers_parse_by_their_names() {
		assert_eq!(tier_of("Middle").unwrap(), PreviewTier::Middle);
		assert!(tier_of("Standard").is_err());
		assert_eq!(layer_of("intensity").unwrap(), Layer::Intensity);
		assert!(layer_of("Field").is_err());
	}
}
