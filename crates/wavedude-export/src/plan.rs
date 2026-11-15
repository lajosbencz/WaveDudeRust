//! What an export writes, decided once for the app and the CLI: the frame range, each layer's look and path, the
//! sinks and the overlays, built into an [`ExportJob`].

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::model::{Layer, Project};
use wavedude_core::scene::compose::compose_scene;

use crate::fs::OutputFs;
use crate::h264::{H264Factory, Profile};
use crate::job::{ExportJob, ExportLayerLook, ExportTarget};
use crate::overlays::Overlays;
use crate::range::frame_range;
use crate::sinks::{self, EncoderPool, FrameSink, SinkSpec, first_error};
use crate::{ExportError, ExportFormat, ExportLayers, FrameSource, Result, naming};

/// An export of a project's run.
#[derive(Clone, Debug)]
pub struct ExportPlan<'a> {
	/// Its Display settings give the in and out points, the looks, the video settings and the overlays.
	pub project: &'a Project,
	pub format: ExportFormat,
	pub layers: ExportLayers,
	/// Where the outputs go.
	pub fs: Arc<dyn OutputFs>,
	/// The chosen output in `fs`; [`ExportLayers::Both`] adds each layer's suffix.
	pub path: PathBuf,
	/// A still of this timeline frame instead of the in-out range.
	pub still: Option<usize>,
	/// Timeline frames the source holds once its run is complete.
	pub frame_count: usize,
	/// The run's recordings of the active probes in project order.
	pub traces: &'a [ProbeTrace],
	/// Steps of the run, the x span of a full-scale probe graph.
	pub total_steps: u64,
	/// Keep the outputs of a failed or cancelled job.
	pub keep_partial: bool,
	/// MP4's H.264 encoders.
	pub h264: Option<Arc<dyn H264Factory>>,
	/// MP4's H.264 profile; the best `h264` has when it lacks this one.
	pub h264_profile: Profile,
}

impl ExportPlan<'_> {
	/// The timeline frames written: the still, or the Display in-out range.
	pub fn frames(&self) -> Result<RangeInclusive<usize>> {
		match self.still {
			Some(frame) if frame >= self.frame_count => {
				Err(ExportError::StillBeyond { frame, count: self.frame_count })
			}
			Some(frame) => Ok(frame..=frame),
			None => {
				let d = &self.project.display;
				frame_range(d.export_from, d.export_to, self.project.simulation.visualize_every, self.frame_count)
					.ok_or(ExportError::NoFrames)
			}
		}
	}

	/// Opens every output (creating the folder) and builds the job reading `source`; schemes resolve through `schemes`.
	/// The scene is composed only when a layer burns in its glowing edges. Overlays are dropped for a format that takes the
	/// stored halves, which refuses a source whose intensity is not in its form. When an output cannot be opened, those
	/// already open are finished and removed.
	pub fn build_job<'s>(&self, source: Arc<dyn FrameSource + 's>, schemes: &SchemeStore) -> Result<ExportJob<'s>> {
		if self.still.is_some() && !self.format.supports_still() {
			return Err(ExportError::WrongInput("an image format for a still"));
		}
		let frames = self.frames()?;
		let shape = source.shape();
		let layers = self.layers.layers();
		if self.format.wants_halves()
			&& layers.contains(&Layer::Intensity)
			&& shape.intensity_root != self.format.intensity_root()
		{
			return Err(ExportError::WrongInput("an intensity plane in the format's form (I or its root)"));
		}
		let (width, height) = (shape.width(), shape.height());
		let display = &self.project.display;
		let settings = |layer| match layer {
			Layer::Field => &display.field,
			Layer::Intensity => &display.intensity,
		};
		let scene = layers
			.iter()
			.any(|&l| !self.format.wants_halves() && settings(l).overlay_in_export && settings(l).overlay_gain > 0.0)
			.then(|| compose_scene(self.project))
			.flatten();
		if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
			self.fs.create_dir_all(dir)?;
		}
		let spec = SinkSpec {
			width,
			height,
			fps: display.video.fps,
			quality: display.video.quality,
			profile: self.h264_profile,
			first_frame: *frames.start(),
			frame_count: frames.clone().count(),
		};
		let pool = EncoderPool::default();
		let both = self.layers == ExportLayers::Both;
		let mut targets: Vec<ExportTarget> = Vec::with_capacity(layers.len());
		// Every look before any sink: a failure here leaves no output behind.
		let looks = layers
			.iter()
			.map(|&layer| {
				let scheme = schemes.resolve_layer(self.project, layer);
				let look = ExportLayerLook::new(
					&scheme,
					settings(layer),
					source.sample_kind(layer),
					scene.as_ref(),
					(width, height),
				)?;
				Ok((layer, look))
			})
			.collect::<Result<Vec<_>>>()?;
		for (layer, look) in looks {
			let path = naming::layer_path(&self.path, layer, both);
			let still = self.still.is_some();
			let sink = match sinks::open(self.format, &self.fs, &path, spec, still, &pool, self.h264.as_deref()) {
				Ok(sink) => sink,
				Err(e) => {
					let mut opened: Vec<Box<dyn FrameSink>> = targets.into_iter().map(|t| t.sink).collect();
					let finished = first_error(opened.iter_mut().map(|s| s.finish()));
					first_error(opened.iter_mut().map(|s| s.discard())).and(finished).unwrap_or_else(
						|cleanup| tracing::error!(error = %cleanup, "an export output could not be closed or removed"),
					);
					return Err(e);
				}
			};
			targets.push(ExportTarget { layer, look, sink });
		}
		let overlays = Overlays::new(self.project, width, height, self.traces, self.total_steps);
		let overlays = (!self.format.wants_halves() && !overlays.is_empty()).then_some(overlays);
		Ok(ExportJob::new(source, self.fs.clone(), frames, targets, overlays, self.keep_partial))
	}
}
