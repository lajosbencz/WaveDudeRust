//! The live run (docs/ui/60, The live run): a worker thread steps the project; the newest stored frame is drawn.

use slint::wgpu_30::wgpu;
use waveduderust::model::normalize::Normalize;
use waveduderust::model::{Layer, Project, View};
use waveduderust::quality::PreviewQuality;
use waveduderust::run::{Goal, RunWorker, Wanted, WorkState, engines};
use waveduderust::unstable::engine::gpu::FrameTag;
use waveduderust::unstable::render::gpu::PicturePipeline;
use waveduderust::unstable::spec::RunRequest;

use crate::gpu::Picture;

/// The run and the pictures it feeds.
pub(crate) struct Live {
	worker: RunWorker,
	revision: u64,
	active: bool,
	request: Option<RunRequest>,
	position: u32,
	paused: bool,
	/// The preview quality of the next run (Settings).
	pub(crate) quality: PreviewQuality,
	/// Re-run after edits (Settings).
	pub(crate) auto_rerun: bool,
	/// Frames of the planned run.
	pub(crate) frames: u32,
	drawn: Option<(u64, usize)>,
	last_state: Option<std::mem::Discriminant<WorkState>>,
	device: wgpu::Device,
	queue: wgpu::Queue,
	pipeline: PicturePipeline,
	field: Picture,
	intensity: Picture,
}

/// What a poll brings the window.
pub(crate) struct Shown {
	pub(crate) field: Option<slint::Image>,
	pub(crate) intensity: Option<slint::Image>,
	pub(crate) status: String,
	/// The newest frame the run holds.
	pub(crate) made: Option<u32>,
	/// The simulation step of the frame shown.
	pub(crate) step: Option<u64>,
	/// The run is stepping or fitting.
	pub(crate) running: bool,
}

impl Live {
	pub(crate) fn new(device: wgpu::Device, queue: wgpu::Queue) -> std::io::Result<Self> {
		let worker = RunWorker::new(engines(waveduderust::Backend::Gpu), std::env::temp_dir(), Default::default())?;
		let pipeline = PicturePipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
		let (field, intensity) = (Picture::new(&device, Layer::Field), Picture::new(&device, Layer::Intensity));
		Ok(Self {
			worker,
			revision: 0,
			active: false,
			request: None,
			position: 0,
			paused: false,
			quality: PreviewQuality::FULL,
			auto_rerun: true,
			frames: 0,
			drawn: None,
			last_state: None,
			device,
			queue,
			pipeline,
			field,
			intensity,
		})
	}

	pub(crate) fn active(&self) -> bool {
		self.active
	}

	/// Runs `project` from the start; a running run starts over (docs/ui/60, Automatic re-run).
	pub(crate) fn run(&mut self, project: &Project, view: &View) {
		let mut project = project.clone();
		project.normalize();
		self.revision += 1;
		self.active = true;
		self.paused = false;
		let request = RunRequest::new(&project, self.quality, view.into(), false);
		self.frames = request.schedule().frame_count();
		self.request = Some(request);
		self.publish();
	}

	fn publish(&self) {
		self.worker.set_goal(Goal {
			revision: self.revision,
			run: self.request.clone(),
			position: self.position,
			paused: self.paused,
			wanted: Wanted::All,
			..Goal::default()
		});
	}

	/// Pauses or resumes the run (F6).
	pub(crate) fn toggle_pause(&mut self) {
		if self.active {
			self.paused ^= true;
			self.publish();
		}
	}

	/// Asks the run for the frame at `position` first.
	pub(crate) fn seek(&mut self, position: u32) {
		if self.position != position {
			self.position = position;
			if self.active {
				self.publish();
			}
		}
	}

	/// Starts over after an edit when a run is active.
	pub(crate) fn edited(&mut self, project: &Project, view: &View) {
		if self.active && self.auto_rerun {
			self.run(project, view);
		}
	}

	pub(crate) fn stop(&mut self) {
		self.revision += 1;
		self.active = false;
		self.request = None;
		self.publish();
	}

	/// The newest stored frame drawn, when it changed, and the run's state.
	pub(crate) fn poll(&mut self, project: &Project, view: &View) -> Shown {
		let status = self.worker.status();
		let state = std::mem::discriminant(&status.state);
		if self.last_state != Some(state) {
			self.last_state = Some(state);
			tracing::info!("run: {}", state_text(&status.state, status.steps_done));
		}
		let mut shown = Shown {
			field: None,
			intensity: None,
			status: state_text(&status.state, status.steps_done),
			made: None,
			step: None,
			running: matches!(status.state, WorkState::Stepping { .. } | WorkState::Fitting),
		};
		let stored = self.position as usize;
		if let Some(store) = &status.store {
			shown.made = store.count().checked_sub(1).and_then(|m| u32::try_from(m).ok());
			shown.step = store.has(stored).then(|| store.step_of(stored));
		}
		if let (Some(frames), Some(store)) = (status.device_frames, status.store)
			&& store.has(stored)
			&& self.drawn != Some((store.id(), stored))
			&& let Some(frame) = frames.hold(FrameTag { store: store.id(), generation: store.generation(), stored })
		{
			self.drawn = Some((store.id(), stored));
			shown.field = self.field.draw(&self.device, &self.queue, &self.pipeline, &frame, project, view);
			shown.intensity = self.intensity.draw(&self.device, &self.queue, &self.pipeline, &frame, project, view);
		}
		shown
	}
}

/// The run state as the status bar says it (docs/ui/50, Run states).
fn state_text(state: &WorkState, steps: u32) -> String {
	match state {
		WorkState::Idle => format!("Ready, {steps} steps"),
		WorkState::Stepping { target } => format!("Running: step {steps}, frame {target}"),
		WorkState::Fitting => "Fitting brightness".into(),
		WorkState::Paused => format!("Paused at step {steps}"),
		WorkState::Failed(e) => format!("Run failed: {e}"),
	}
}
