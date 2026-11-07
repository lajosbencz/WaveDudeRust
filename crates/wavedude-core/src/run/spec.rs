//! The immutable snapshot a run or a scratch frame works from (spec/40-frames-store-render.md, Run schedule).

use std::sync::Arc;

use crate::engine::emitters::emitters_of;
use crate::engine::{Emitter, EngineOptions, FrameShape, SceneData};
use crate::model::{Layer, Measurement, Probe, Project, SceneEdges, SceneMode};
use crate::scene::compose::{SceneKey, compose_over, decode_image, image_key};
use crate::scene::pixels::ScenePixels;

use super::quality::PreviewQuality;
use super::store::StoreLayout;

const FIELD: &[Layer] = &[Layer::Field];
const BOTH: &[Layer] = &[Layer::Field, Layer::Intensity];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RunSpecError {
	#[error("The scene uses an image, but no image is loaded.")]
	NoImage,
}

/// The project, its scene pixels and the preview quality at a run's start. Cheap to clone (shared parts).
///
/// Timeline frame k holds the state after step index `k * visualize_every`, that is after
/// `k * visualize_every + 1` steps; with a frame divisor F only every F-th timeline frame is stored.
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
		let track_intensity = track_intensity
			|| project.display.intensity.preview
			|| probes.iter().any(|p| p.measurement != Measurement::Amplitude);
		let shape = FrameShape {
			canvas_width: scene.canvas_width,
			canvas_height: scene.canvas_height,
			divisor: quality.resolution_divisor,
			format: quality.format,
			field_brightness: project.display.field.brightness as f32,
			intensity_brightness: project.display.intensity.brightness as f32,
			intensity_root: quality.intensity_root,
		};
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

	/// Timeline frames: `ceil(steps / visualize_every)`.
	pub fn frame_count(&self) -> u32 {
		self.steps.div_ceil(self.visualize_every)
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
		k * self.visualize_every
	}

	/// Timeline frames covered by `stored` stored frames.
	pub fn covered_frames(&self, stored: u32) -> u32 {
		(stored as u64 * self.frame_divisor() as u64).min(self.frame_count() as u64) as u32
	}

	/// The planes of every frame, in store order: the field, then the intensity when tracked.
	pub fn layers(&self) -> &'static [Layer] {
		if self.track_intensity() { BOTH } else { FIELD }
	}

	/// The store layout of a run of this spec.
	pub fn store_layout(&self) -> StoreLayout {
		StoreLayout {
			shape: self.shape,
			layers: self.layers(),
			capacity: self.stored_frames() as usize,
			capture_every: self.capture_every(),
		}
	}

	/// Bytes of one stored frame at this quality.
	pub fn frame_bytes(&self) -> u64 {
		self.layers().len() as u64 * self.shape.plane_bytes() as u64
	}

	/// Bytes the store of a run of this spec at `quality` takes in memory (no file header).
	pub fn store_bytes_at(&self, quality: PreviewQuality) -> u64 {
		debug_assert!(quality.is_valid(), "invalid preview quality {quality:?}");
		let shape = FrameShape { divisor: quality.resolution_divisor, format: quality.format, ..self.shape };
		let frames = self.frame_count().div_ceil(quality.frame_divisor);
		self.layers().len() as u64 * shape.plane_bytes() as u64 * frames as u64
	}

	pub fn store_bytes(&self) -> u64 {
		self.store_bytes_at(self.quality)
	}

	/// Same scene, options and so the same engine: only emitters and probes may differ.
	pub fn same_scene_as(&self, other: &Self) -> bool {
		self.options == other.options && (Arc::ptr_eq(&self.scene, &other.scene) || self.scene == other.scene)
	}

	/// Same grid, edges and options: an engine can take the other scene in place.
	pub fn same_grid_as(&self, other: &Self) -> bool {
		let (a, b) = (&self.scene, &other.scene);
		self.options == other.options
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
