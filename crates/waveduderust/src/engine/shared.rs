//! What the CPU and the GPU engine keep alike on the host: the scene, the options, the scheme, the sources
//! and the step count, with what they do the same way.

use std::sync::Arc;

use super::forcing::{SourceGroups, Sources};
use super::stencil::{StencilCoefficients, TimeScheme};
use super::substeps::sub_steps;
use super::types::{Emitter, EngineOptions, SceneData};

/// The displayed intensity from the two EMA stages: their mean times `scale` (docs/physics.md,
/// Intensity). Both engines' host code reads it so.
#[inline]
pub(crate) fn blend(s1: f32, s2: f32, scale: f32) -> f32 {
	(s1 * 0.5 + s2 * 0.5) * scale
}

/// See the module docs.
pub(crate) struct Shared {
	pub scene: Arc<SceneData>,
	pub options: EngineOptions,
	pub sub_steps: u32,
	pub scheme: TimeScheme,
	pub sources: Sources,
	/// Steps simulated so far.
	pub time: u32,
}

impl Shared {
	/// At rest on `scene`, `n` sub-steps a step.
	pub(crate) fn new(scene: Arc<SceneData>, options: EngineOptions, n: u32) -> Self {
		let coefficients = StencilCoefficients::of(options.stencil);
		let mut sources = Sources::new(coefficients, options.source_frequency_scale, options.image_source_scale);
		sources.set_image_levels(&scene);
		Self { scheme: TimeScheme::of(options.stencil), sub_steps: n, sources, time: 0, options, scene }
	}

	/// The sub-steps `scene` needs, when it can replace the current one (`WaveEngine::try_replace_scene`).
	pub(crate) fn replacement(&self, scene: &SceneData) -> Option<u32> {
		self.scene.same_grid(scene).then(|| sub_steps(self.options.stencil, scene.max_c2()))
	}

	/// Swaps in `scene`, a [`replacement`](Self::replacement), at its `n` sub-steps.
	pub(crate) fn replace_scene(&mut self, scene: Arc<SceneData>, n: u32) {
		self.sources.set_image_levels(&scene);
		self.scene = scene;
		self.sub_steps = n;
	}

	pub(crate) fn set_emitters(&mut self, emitters: &[Emitter]) {
		self.sources.set_emitters(emitters, &self.scene);
	}

	/// The driven cells of the image sources and the emitters (forcing.rs).
	pub(crate) fn groups(&self) -> SourceGroups {
		self.sources.groups(&self.scene)
	}

	/// EMA weight of the intensity from the lowest frequency in use.
	pub(crate) fn alpha(&self) -> f32 {
		self.sources.intensity_alpha(self.options.time_decay)
	}

	/// The source table of sub-step `k` of the step being taken; the sub-steps come in order (forcing.rs).
	pub(crate) fn source_values(&mut self, out: &mut [f32], k: u32) {
		self.sources.values(out, self.time, k, self.sub_steps, self.scheme);
	}

	/// The source table of sub-step `k` of the step `ahead` steps after the one being taken, for engines that take
	/// several steps at once; the sub-steps of the steps come in order.
	pub(crate) fn source_values_ahead(&mut self, out: &mut [f32], ahead: u32, k: u32) {
		self.sources.values(out, self.time + ahead, k, self.sub_steps, self.scheme);
	}

	/// Whether sub-step `k` advances the intensity stages: a step's last, when the intensity is tracked.
	pub(crate) fn accumulates(&self, k: u32) -> bool {
		self.options.track_intensity && k + 1 == self.sub_steps
	}

	/// Time and pulse states to zero.
	pub(crate) fn reset(&mut self) {
		self.time = 0;
		self.sources.reset_pulses();
	}

	/// Host copies of the canvas field and blended intensity from the padded field `u` and stages `(s1, s2)`;
	/// the intensity is 0 without stages.
	pub(crate) fn canvas_frame(&self, u: &[f32], stages: Option<(&[f32], &[f32])>) -> (Vec<f32>, Vec<f32>) {
		let (w, h) = (self.scene.canvas_width as usize, self.scene.canvas_height as usize);
		let scale = self.options.intensity_scale as f32;
		let mut field = Vec::with_capacity(w * h);
		let mut intensity = Vec::with_capacity(w * h);
		for y in 0..h {
			let at = self.scene.canvas_index(0, y);
			field.extend_from_slice(&u[at..at + w]);
			match stages {
				Some((s1, s2)) => intensity.extend((at..at + w).map(|c| blend(s1[c], s2[c], scale))),
				None => intensity.resize(intensity.len() + w, 0.0),
			}
		}
		(field, intensity)
	}
}
