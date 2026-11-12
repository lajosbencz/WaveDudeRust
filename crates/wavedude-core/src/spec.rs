//! The immutable snapshot a run works from (spec/40-frames-store-render.md, Run schedule): a [`RunRequest`]
//! names a project state and a quality; the worker builds the [`RunSpec`] from it, composing the scene off
//! the UI thread; the [`Schedule`] is what both share.

use std::sync::Arc;

use crate::engine::emitters::emitters_of;
use crate::engine::{Emitter, EngineOptions, FrameShape, SampleFormat, SceneData};
use crate::model::{Frame, Layer, Measurement, Probe, Project, SceneEdges, SceneMode, Timeline};
use crate::quality::PreviewQuality;
use crate::scene::compose::{SceneKey, compose_over, decode_image, image_key};
use crate::scene::pixels::ScenePixels;

const FIELD: &[Layer] = &[Layer::Field];
const BOTH: &[Layer] = &[Layer::Field, Layer::Intensity];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RunSpecError {
	#[error("The scene uses an image, but no image is loaded.")]
	NoImage,
}

/// Steps, steps per frame and the quality: the frame arithmetic of a run.
///
/// Timeline frame k holds the state after step index `k * visualize_every`, that is after
/// `k * visualize_every + 1` steps; with a frame divisor F only every F-th timeline frame is stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Schedule {
	pub steps: u32,
	/// Steps per timeline frame, at least 1.
	pub visualize_every: u32,
	pub quality: PreviewQuality,
}

impl Schedule {
	pub fn timeline(&self) -> Timeline {
		Timeline { steps: self.steps, visualize_every: self.visualize_every }
	}

	pub fn frame_count(&self) -> u32 {
		self.timeline().frame_count()
	}

	pub fn frame_divisor(&self) -> u32 {
		self.quality.frame_divisor
	}

	/// Steps between two captures: `visualize_every * frame_divisor`.
	pub fn capture_every(&self) -> u32 {
		self.visualize_every * self.frame_divisor()
	}

	/// Frames the store holds: `ceil(frame_count / frame_divisor)`.
	pub fn stored_frames(&self) -> u32 {
		self.frame_count().div_ceil(self.frame_divisor())
	}

	/// The stored frame timeline frame `k` shows.
	pub fn stored_index_of(&self, k: u32) -> u32 {
		k / self.frame_divisor()
	}

	/// The step index timeline frame `k` holds the state after.
	pub fn step_of(&self, k: u32) -> u32 {
		self.timeline().step_of(Frame(k)).0
	}

	/// Timeline frames covered by `stored` stored frames.
	pub fn covered_frames(&self, stored: u32) -> u32 {
		(stored as u64 * self.frame_divisor() as u64).min(self.frame_count() as u64) as u32
	}
}

/// What a run is made from: a project state, the preview quality and whether the intensity is tracked. The
/// worker builds the [`RunSpec`] from it; what the UI needs meanwhile (the schedule, the store's size, the
/// probes) comes from the project itself. Cheap to clone.
#[derive(Clone, Debug)]
pub struct RunRequest {
	pub project: Arc<Project>,
	pub quality: PreviewQuality,
	pub track_intensity: bool,
}

impl RunRequest {
	/// A request for a copy of a normalised `project` at a valid `quality`.
	pub fn new(project: &Project, quality: PreviewQuality, track_intensity: bool) -> Self {
		let sim = &project.simulation;
		debug_assert!(sim.steps >= 1 && sim.visualize_every >= 1, "the project is not normalised");
		debug_assert!(quality.is_valid(), "invalid preview quality {quality:?}");
		Self { project: Arc::new(project.clone()), quality, track_intensity }
	}

	/// The same request at another quality (the memory warning's choice).
	pub fn with_quality(&self, quality: PreviewQuality) -> Self {
		debug_assert!(quality.is_valid(), "invalid preview quality {quality:?}");
		Self { quality, ..self.clone() }
	}

	/// The same project state, quality and tracking: the same spec comes out.
	pub fn same_as(&self, other: &Self) -> bool {
		Arc::ptr_eq(&self.project, &other.project)
			&& self.quality == other.quality
			&& self.track_intensity == other.track_intensity
	}

	pub fn schedule(&self) -> Schedule {
		let sim = &self.project.simulation;
		Schedule { steps: sim.steps, visualize_every: sim.visualize_every, quality: self.quality }
	}

	/// The active probes in project order, the spec's order.
	pub fn probes(&self) -> impl Iterator<Item = &Probe> {
		self.project.probes.iter().filter(|p| p.active)
	}

	pub fn tracks_intensity(&self) -> bool {
		tracks_intensity(&self.project, self.track_intensity)
	}

	/// The planes of every frame, in store order: the field, then the intensity when tracked.
	pub fn layers(&self) -> &'static [Layer] {
		if self.tracks_intensity() { BOTH } else { FIELD }
	}

	pub fn frame_bytes(&self) -> u64 {
		let canvas = self.project.canvas_size();
		self.layers().len() as u64 * shape_of(&self.project, canvas, self.quality).plane_bytes() as u64
	}

	/// Bytes the store of a run of this request at `quality` takes in memory (no file header).
	pub fn store_bytes_at(&self, quality: PreviewQuality) -> u64 {
		debug_assert!(quality.is_valid(), "invalid preview quality {quality:?}");
		let canvas = self.project.canvas_size();
		let frames = Schedule { quality, ..self.schedule() }.stored_frames();
		self.layers().len() as u64 * shape_of(&self.project, canvas, quality).plane_bytes() as u64 * frames as u64
	}

	pub fn store_bytes(&self) -> u64 {
		self.store_bytes_at(self.quality)
	}

	/// The spec, its scene from `cache`.
	pub fn spec(&self, cache: &mut SceneCache) -> Result<RunSpec, RunSpecError> {
		cache.spec(&self.project, self.quality, self.track_intensity)
	}
}

/// The intensity is tracked when asked for, previewed, or measured by an active probe.
fn tracks_intensity(project: &Project, track_intensity: bool) -> bool {
	track_intensity
		|| project.display.intensity.preview
		|| project.probes.iter().any(|p| p.active && p.measurement != Measurement::Amplitude)
}

/// The captured planes' shape for a `canvas` at `quality`, the brightness from the project's Display.
fn shape_of(project: &Project, canvas: (u32, u32), quality: PreviewQuality) -> FrameShape {
	FrameShape {
		canvas_width: canvas.0,
		canvas_height: canvas.1,
		divisor: quality.resolution_divisor,
		format: quality.format,
		field_brightness: project.display.field.brightness as f32,
		intensity_brightness: project.display.intensity.brightness as f32,
		intensity_root: quality.intensity_root,
	}
}

/// The project, its scene pixels and the preview quality at a run's start. Cheap to clone (shared parts).
#[derive(Clone, Debug)]
pub struct RunSpec {
	pub seed: i32,
	pub steps: u32,
	/// Steps per timeline frame, at least 1.
	pub visualize_every: u32,
	pub quality: PreviewQuality,
	/// Captured planes; brightness from the project's Display (baked into index frames).
	pub shape: FrameShape,
	pub options: EngineOptions,
	pub scene: Arc<SceneData>,
	pub emitters: Arc<[Emitter]>,
	/// The active probes in project order.
	pub probes: Arc<[Probe]>,
}

impl RunSpec {
	/// Snapshots a normalised `project` at a valid `quality` ([`PreviewQuality::is_valid`]) with its scene
	/// built from `pixels` ([`scene_of`](Self::scene_of)). The intensity is tracked when `track_intensity`, the
	/// intensity layer is previewed, or an active probe measures anything but the amplitude.
	pub fn create(
		project: &Project,
		pixels: Option<&ScenePixels>,
		quality: PreviewQuality,
		track_intensity: bool,
	) -> Result<Self, RunSpecError> {
		let scene = Arc::new(Self::scene_of(project, pixels)?);
		Ok(Self::with_scene(project, scene, quality, track_intensity))
	}

	/// The medium of a normalised `project`. An image scene takes its canvas from `pixels`; a canvas scene uses
	/// `pixels` (painted parts) when they have the canvas size, else a blank canvas. Image sources emit only in
	/// an image scene.
	pub fn scene_of(project: &Project, pixels: Option<&ScenePixels>) -> Result<SceneData, RunSpecError> {
		let image = project.scene.mode == SceneMode::Image;
		let pixels = if image {
			Some(pixels.ok_or(RunSpecError::NoImage)?)
		} else {
			pixels.filter(|p| (p.width, p.height) == (project.scene.width, project.scene.height))
		};
		let edges = scene_edges(project);
		Ok(match pixels {
			Some(p) => SceneData::from_pixels(&p.rgba, p.width, p.height, edges),
			None => SceneData::blank(project.scene.width, project.scene.height, edges),
		})
	}

	/// [`create`](Self::create) with the `scene` [`scene_of`](Self::scene_of) built for `project`.
	pub fn with_scene(
		project: &Project,
		scene: Arc<SceneData>,
		quality: PreviewQuality,
		track_intensity: bool,
	) -> Self {
		let sim = &project.simulation;
		debug_assert!(sim.steps >= 1 && sim.visualize_every >= 1, "the project is not normalised");
		debug_assert!(quality.is_valid(), "invalid preview quality {quality:?}");
		let probes: Arc<[Probe]> = project.probes.iter().filter(|p| p.active).cloned().collect();
		let track_intensity = tracks_intensity(project, track_intensity);
		let shape = shape_of(project, (scene.canvas_width, scene.canvas_height), quality);
		let options = EngineOptions {
			stencil: sim.stencil,
			source_frequency_scale: sim.source_frequency_scale,
			image_source_scale: project.scene.image_source_scale,
			time_decay: sim.time_decay,
			intensity_scale: sim.intensity_scale,
			track_intensity,
		};
		Self {
			seed: project.seed,
			steps: sim.steps,
			visualize_every: sim.visualize_every,
			quality,
			shape,
			options,
			scene,
			emitters: emitters_of(project).into(),
			probes,
		}
	}

	/// The same spec at another quality (the memory warning's choice).
	pub fn with_quality(&self, quality: PreviewQuality) -> Self {
		debug_assert!(quality.is_valid(), "invalid preview quality {quality:?}");
		let shape = FrameShape {
			divisor: quality.resolution_divisor,
			format: quality.format,
			intensity_root: quality.intensity_root,
			..self.shape
		};
		Self { quality, shape, ..self.clone() }
	}

	pub fn track_intensity(&self) -> bool {
		self.options.track_intensity
	}

	pub fn schedule(&self) -> Schedule {
		Schedule { steps: self.steps, visualize_every: self.visualize_every, quality: self.quality }
	}

	pub fn frame_count(&self) -> u32 {
		self.schedule().frame_count()
	}

	pub fn frame_divisor(&self) -> u32 {
		self.quality.frame_divisor
	}

	pub fn capture_every(&self) -> u32 {
		self.schedule().capture_every()
	}

	pub fn stored_frames(&self) -> u32 {
		self.schedule().stored_frames()
	}

	pub fn stored_index_of(&self, k: u32) -> u32 {
		self.schedule().stored_index_of(k)
	}

	pub fn step_of(&self, k: u32) -> u32 {
		self.schedule().step_of(k)
	}

	pub fn covered_frames(&self, stored: u32) -> u32 {
		self.schedule().covered_frames(stored)
	}

	/// The planes of every frame, in store order: the field, then the intensity when tracked.
	pub fn layers(&self) -> &'static [Layer] {
		if self.track_intensity() { BOTH } else { FIELD }
	}

	/// Bytes of one stored frame at this quality.
	pub fn frame_bytes(&self) -> u64 {
		self.layers().len() as u64 * self.shape.plane_bytes() as u64
	}

	/// Same scene and solver, so the same engine (the intensity tracking is switched on it): only emitters,
	/// probes and the tracking may differ.
	pub fn same_scene_as(&self, other: &Self) -> bool {
		self.same_solver_as(other) && (Arc::ptr_eq(&self.scene, &other.scene) || self.scene == other.scene)
	}

	/// The options but the intensity tracking agree.
	fn same_solver_as(&self, other: &Self) -> bool {
		let solver = |o: &EngineOptions| EngineOptions { track_intensity: false, ..o.clone() };
		solver(&self.options) == solver(&other.options)
	}

	/// The same frames come out: scene, options, emitters, seed, schedule and quality agree, and the brightness
	/// too when it is baked into the samples (palette indices).
	pub fn same_frames_as(&self, other: &Self) -> bool {
		let baked = |s: &Self| match s.shape.format {
			SampleFormat::Index => Some((s.shape.field_brightness.to_bits(), s.shape.intensity_brightness.to_bits())),
			SampleFormat::Half => None,
		};
		self.same_scene_as(other)
			&& self.options.track_intensity == other.options.track_intensity
			&& self.emitters == other.emitters
			&& (self.seed, self.steps, self.visualize_every, self.quality)
				== (other.seed, other.steps, other.visualize_every, other.quality)
			&& baked(self) == baked(other)
	}

	/// The same recordings ([`Probe::recording_key`]), so the same traces.
	pub fn same_probes_as(&self, other: &Self) -> bool {
		self.probes.iter().map(Probe::recording_key).eq(other.probes.iter().map(Probe::recording_key))
	}

	/// Same grid, edges and options: an engine can take the other scene in place.
	pub fn same_grid_as(&self, other: &Self) -> bool {
		let (a, b) = (&self.scene, &other.scene);
		self.same_solver_as(other)
			&& (a.canvas_width, a.canvas_height, a.rim, a.edges) == (b.canvas_width, b.canvas_height, b.rim, b.edges)
	}
}

/// The settings of `project`'s scene that shape its medium beside the pixels.
fn scene_edges(project: &Project) -> SceneEdges {
	let image = project.scene.mode == SceneMode::Image;
	SceneEdges { image_sources: image && project.scene.image_sources, ..SceneEdges::from(&project.scene) }
}

/// The newest project's scene pixels and medium, rebuilt only when what they are made from changed; specs from
/// one cached medium share it, so [`RunSpec::same_scene_as`] compares pointers.
#[derive(Default)]
pub struct SceneCache {
	image: Option<(Option<u64>, Option<Arc<ScenePixels>>)>,
	pixels: Option<(SceneKey, Option<Arc<ScenePixels>>)>,
	scene: Option<(SceneKey, SceneEdges, Arc<SceneData>)>,
}

impl SceneCache {
	/// The decoded image of an image scene ([`decode_image`]).
	pub fn image(&mut self, project: &Project) -> Option<Arc<ScenePixels>> {
		let key = image_key(project);
		match &self.image {
			Some((k, image)) if *k == key => image.clone(),
			_ => {
				let image = decode_image(project).map(Arc::new);
				self.image = Some((key, image.clone()));
				image
			}
		}
	}

	/// The composed pixels of `project`'s scene ([`compose_scene`](crate::scene::compose::compose_scene)).
	pub fn pixels(&mut self, project: &Project) -> Option<Arc<ScenePixels>> {
		self.pixels_of(SceneKey::of(project), project)
	}

	fn pixels_of(&mut self, key: SceneKey, project: &Project) -> Option<Arc<ScenePixels>> {
		if let Some((k, pixels)) = &self.pixels
			&& *k == key
		{
			return pixels.clone();
		}
		let image = self.image(project);
		let pixels = if project.parts.iter().any(|p| p.base.enabled) {
			compose_over(image.as_deref().cloned(), project).map(Arc::new)
		} else {
			image
		};
		self.pixels = Some((key, pixels.clone()));
		pixels
	}

	/// [`RunSpec::create`] on the cached medium.
	pub fn spec(
		&mut self,
		project: &Project,
		quality: PreviewQuality,
		track_intensity: bool,
	) -> Result<RunSpec, RunSpecError> {
		let key = SceneKey::of(project);
		let edges = scene_edges(project);
		let scene = match &self.scene {
			Some((k, e, scene)) if *k == key && *e == edges => scene.clone(),
			_ => {
				let pixels = self.pixels_of(key, project);
				let scene = Arc::new(RunSpec::scene_of(project, pixels.as_deref())?);
				self.scene = Some((key, edges, scene.clone()));
				scene
			}
		};
		Ok(RunSpec::with_scene(project, scene, quality, track_intensity))
	}
}
