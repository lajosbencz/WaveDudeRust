//! The live run (docs/ui/60, The live run): a worker thread steps the project; the newest stored frame is drawn.

use slint::wgpu_30::wgpu;
use waveduderust::model::normalize::Normalize;
use waveduderust::model::{Layer, Project, View};
use waveduderust::quality::PreviewQuality;

use waveduderust::run::{FitRequest, FrameStore, Goal, RunWorker, Wanted, WorkState};
use waveduderust::unstable::engine::gpu::{FrameTag, GpuContext};
use waveduderust::unstable::engine::types::SampleFormat;
use waveduderust::unstable::render::fit_plane;
use waveduderust::unstable::render::gpu::{DevicePlane, PicturePipeline};
use waveduderust::unstable::scene::glowing_edges::EdgeImage;
use waveduderust::unstable::spec::{RunRequest, SceneCache};

use waveduderust::io::schemes::SchemeStore;

use crate::gpu::{Look, Picture};
use crate::ui::{RunState, RunStatus};

/// What an edit asks of the run (docs/ui/60, Edit classes).
pub(crate) use waveduderust::unstable::spec::RunChange as Change;

/// The run and the pictures it feeds.
pub(crate) struct Live {
	worker: RunWorker,
	revision: u64,
	active: bool,
	/// Every frame (a run) or the position's frame alone.
	wanted: Wanted,
	request: Option<RunRequest>,
	position: u32,
	paused: bool,
	/// The preview quality of the next run (Settings).
	pub(crate) quality: PreviewQuality,
	/// Re-run after edits (Settings).
	pub(crate) auto_rerun: bool,
	/// Frames of the planned run.
	pub(crate) frames: u32,
	/// The frame the pictures show.
	drawn: Option<FrameTag>,
	/// When the run the user asked for (Run, Play) began; `None` with no run.
	started: Option<web_time::Instant>,
	/// Its summary is logged.
	summarised: bool,
	/// Steps of the planned run.
	steps: u32,
	/// The engine as the status bar names it.
	engine: String,
	/// The layers of a stored frame the device no longer holds.
	uploads: Uploads,
	last_state: Option<std::mem::Discriminant<WorkState>>,
	device: wgpu::Device,
	queue: wgpu::Queue,
	pipeline: PicturePipeline,
	field: Picture,
	/// The scene the edges are made from.
	scene: SceneCache,
	/// The scene's glowing edges, drawn over a picture whose layer shows the outline.
	edges: Option<std::sync::Arc<EdgeImage>>,
	intensity: Picture,
	/// An Auto of an 8-bit frame waiting for the frame in 16-bit: the request and the layer.
	fit: Option<(u64, Layer)>,
	fits: u64,
}

/// What Auto brightness finds.
pub(crate) enum Fit {
	/// The brightness, or nothing to fit.
	Done(Option<f64>),
	/// Asked for the frame in 16-bit; a poll brings the fit.
	Pending,
}

/// What a poll brings the window.
pub(crate) struct Shown {
	pub(crate) field: Option<slint::Image>,
	pub(crate) intensity: Option<slint::Image>,
	pub(crate) status: RunStatus,
	/// The newest frame the run holds.
	pub(crate) made: Option<u32>,
	/// The simulation step of the frame shown.
	pub(crate) step: Option<u64>,
	/// The run is stepping or fitting.
	pub(crate) running: bool,
	/// The run's state changed kind since the last poll.
	pub(crate) changed: bool,
	/// The run is computing the frame of the position, which is not stored yet.
	pub(crate) computing: bool,
	/// A pending Auto's layer and brightness (`None`: nothing to fit).
	pub(crate) fitted: Option<(Layer, Option<f64>)>,
}

impl Live {
	pub(crate) fn new(context: &std::sync::Arc<GpuContext>) -> std::io::Result<Self> {
		let (device, queue) = (context.device.clone(), context.queue.clone());
		#[cfg(not(target_arch = "wasm32"))]
		let worker = RunWorker::new(
			waveduderust::run::engines(waveduderust::Backend::Gpu),
			std::env::temp_dir(),
			Default::default(),
		)?;
		#[cfg(target_arch = "wasm32")]
		let worker = crate::web::worker(context.clone());
		let pipeline = PicturePipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
		let (field, intensity) = (Picture::new(&device, Layer::Field), Picture::new(&device, Layer::Intensity));
		Ok(Self {
			worker,
			revision: 0,
			active: false,
			wanted: Wanted::Position,
			request: None,
			position: 0,
			paused: false,
			quality: PreviewQuality::FULL,
			auto_rerun: true,
			frames: 0,
			drawn: None,
			fit: None,
			fits: 0,
			started: None,
			summarised: false,
			steps: 0,
			engine: format!("GPU: {}", context.info.name),
			uploads: Uploads::default(),
			last_state: None,
			device,
			queue,
			pipeline,
			field,
			scene: SceneCache::default(),
			edges: None,
			intensity,
		})
	}

	pub(crate) fn active(&self) -> bool {
		self.active
	}

	pub(crate) fn paused(&self) -> bool {
		self.paused
	}

	/// Runs `project` from the start; a running run starts over (docs/ui/60, Automatic re-run).
	/// A project opened: its frame at the position alone (docs/ui/60, The live run).
	pub(crate) fn load(&mut self, project: &Project, view: &View) {
		// nothing carries over from the project before, its tracking neither
		self.request = None;
		self.active = false;
		self.started = None;
		self.wanted = Wanted::Position;
		self.paused = false;
		self.request(project, view);
	}

	/// Every frame (Run).
	pub(crate) fn run(&mut self, project: &Project, view: &View) {
		self.begin();
		self.active = true;
		self.wanted = Wanted::All;
		self.paused = false;
		self.request(project, view);
	}

	/// Every frame of the project as it is, from the position on (Play with frames missing, docs/ui/50).
	pub(crate) fn fill(&mut self) {
		if !self.active {
			self.begin();
			self.active = true;
			self.wanted = Wanted::All;
			self.paused = false;
			self.publish();
		}
	}

	fn begin(&mut self) {
		self.started = Some(web_time::Instant::now());
		self.summarised = false;
	}

	/// A run of `project` from scratch (open, Run): a pending fit goes with the old frames.
	fn request(&mut self, project: &Project, view: &View) {
		self.fit = None;
		let request = self.request_of(project, view);
		self.submit(request);
	}

	/// The request of `project` as it is now.
	fn request_of(&self, project: &Project, view: &View) -> RunRequest {
		let mut project = project.clone();
		project.normalize();
		// a shown intensity picture is tracked; the run keeps tracking it once hidden, as the worker does
		let tracked = self.request.as_ref().is_some_and(RunRequest::tracks_intensity);
		RunRequest::new(&project, self.quality, view.into(), view.intensity.preview || tracked)
	}

	fn submit(&mut self, request: RunRequest) {
		self.revision += 1;
		self.edges = self.scene.pixels(&request.project).map(|p| std::sync::Arc::new(EdgeImage::of(&p)));
		self.drawn = None;
		let schedule = request.schedule();
		(self.frames, self.steps) = (schedule.frame_count(), schedule.steps);
		self.request = Some(request);
		self.publish();
	}

	fn publish(&self) {
		self.worker.set_goal(Goal {
			revision: self.revision,
			run: self.request.clone(),
			position: self.position,
			paused: self.paused,
			wanted: self.wanted,
			fit: self.fit.map(|(id, _)| FitRequest { id, frame: self.position }),
			..Goal::default()
		});
	}

	/// Calls `f` with the probe traces at the frame of the position, when it is stored.
	pub(crate) fn traces<R>(&self, f: impl FnOnce(&crate::graphs::Traces<'_>) -> R) -> Option<R> {
		let store = self.worker.status().store?;
		let frame = self.stored();
		if !store.has(frame) {
			return None;
		}
		let step = store.step_of(frame);
		let probes = self.request.as_ref()?.probes().map(waveduderust::model::Probe::id).collect();
		Some(self.worker.with_traces(|traces| {
			f(&crate::graphs::Traces { traces, probes, step, frame, total_steps: u64::from(self.steps) })
		}))
	}

	/// The recording of `probe` in the run, as CSV (docs/ui/70, CSV).
	pub(crate) fn probe_csv(&self, probe: &waveduderust::model::Probe) -> Option<Vec<u8>> {
		let request = self.request.as_ref()?;
		let n = request.probes().position(|p| p.id() == probe.id())?;
		let schedule = request.schedule();
		self.worker.with_traces(|traces| {
			let mut out = Vec::new();
			let (every, divisor) = (schedule.visualize_every, schedule.frame_divisor());
			waveduderust::unstable::export::csv::write_probe_csv(&mut out, probe, traces.get(n)?, every, divisor)
				.ok()?;
			Some(out)
		})
	}

	/// Draws the shown frame again for a changed look; frames of baked brightness (8-bit) are made again when it
	/// changed, as for a simulation edit.
	pub(crate) fn restyle(&mut self, project: &Project, view: &View) -> Change {
		self.drawn = None;
		self.edited(project, view)
	}

	/// Auto brightness of `layer` for the frame shown (docs/ui/40, Header strip).
	pub(crate) fn auto(&mut self, layer: Layer) -> Fit {
		let stored = self.stored();
		let Some(store) = self.worker.status().store.filter(|s| s.has(stored) && s.layers().contains(&layer)) else {
			return Fit::Done(None);
		};
		if matches!(store.shape().format, SampleFormat::Half) {
			return Fit::Done(store.copy_layer(stored, layer).ok().and_then(|b| fit_plane(&b, store.shape(), layer)));
		}
		self.fits += 1;
		self.fit = Some((self.fits, layer));
		self.publish();
		Fit::Pending
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
			// a pending fit follows the position
			if self.active || self.fit.is_some() {
				self.publish();
			}
		}
	}

	/// Takes up an edit (docs/ui/60, Edit classes): a simulation edit makes the position's frame anew, and every frame
	/// with Re-run automatically on, else nothing more; a probe edit keeps the frames and what is wanted, its
	/// recordings made anew. Says what the edit asked.
	pub(crate) fn edited(&mut self, project: &Project, view: &View) -> Change {
		// nothing loaded yet: opening a project asks for its frame
		let Some(old) = &self.request else { return Change::None };
		let request = self.request_of(project, view);
		let change = old.change_to(&request);
		if change == Change::Frames {
			self.fit = None;
			if self.auto_rerun {
				// a paused run waits paused with the edit; a pause with nothing left to do was no pause
				self.paused &= !self.summarised;
				self.begin();
				self.active = true;
				self.wanted = Wanted::All;
			} else {
				self.active = false;
				self.wanted = Wanted::Position;
				self.paused = false;
				self.started = None;
			}
		}
		if change == Change::None {
			// what no run reads may still be drawn: a colour scheme recolours the frame shown
			self.drawn = None;
		} else {
			self.submit(request);
		}
		change
	}

	/// The stored frame of the position.
	fn stored(&self) -> usize {
		let position = self.request.as_ref().map_or(self.position, |r| r.schedule().stored_index_of(self.position));
		position as usize
	}

	/// No more frames than the position's.
	pub(crate) fn stop(&mut self) {
		self.revision += 1;
		self.fit = None;
		self.active = false;
		self.wanted = Wanted::Position;
		self.paused = false;
		self.publish();
	}

	/// The newest stored frame drawn, when it changed, and the run's state.
	pub(crate) fn poll(&mut self, project: &Project, view: &View, schemes: &SchemeStore) -> Shown {
		#[cfg(target_arch = "wasm32")]
		self.worker.pump(std::time::Duration::from_millis(10), std::task::Waker::noop());
		let mut fitted = None;
		if let Some((id, layer)) = self.fit
			&& let Some(frame) = self.worker.take_fit()
			&& frame.id == id
		{
			self.fit = None;
			self.publish();
			fitted = Some((layer, fit_plane(frame.snapshot.plane(layer), &frame.snapshot.shape, layer)));
		}
		let status = self.worker.status();
		let state = std::mem::discriminant(&status.state);
		let changed = self.last_state != Some(state);
		if changed {
			self.last_state = Some(state);
			if let WorkState::Failed(e) = &status.state {
				self.fit = None;
				tracing::error!("the run failed: {e}");
			}
		}
		let state = self.started.map_or(RunState::None, |_| match &status.state {
			WorkState::Stepping { .. } | WorkState::Fitting => RunState::Running,
			WorkState::Paused => RunState::Paused,
			WorkState::Failed(_) => RunState::Failed,
			WorkState::Idle if status.store.as_ref().is_some_and(|s| s.is_complete()) => RunState::Completed,
			WorkState::Idle => RunState::Stopped,
		});
		if let (Some(started), RunState::Completed | RunState::Stopped, Some(store)) =
			(self.started, state, status.store.as_ref())
			&& !matches!(status.state, WorkState::Stepping { .. } | WorkState::Fitting)
			&& !self.summarised
		{
			self.summarised = true;
			let gib = |b: u64| b as f64 / f64::from(1u32 << 30);
			tracing::info!(
				"Run {}: {} of {} steps, {} frames, {:.1} s; {} stored, {} in memory ({:.1} GB) and {} on disk ({:.1} GB)",
				if state == RunState::Completed { "Completed" } else { "Cancelled" },
				status.steps_done,
				self.steps,
				self.frames,
				started.elapsed().as_secs_f64(),
				store.present_count(),
				store.frames_in_memory(),
				gib(store.ram_bytes()),
				store.frames_on_disk(),
				gib(store.spill_bytes().unwrap_or(0)),
			);
		}
		let mut shown = Shown {
			field: None,
			intensity: None,
			status: RunStatus {
				state,
				steps_done: i32::try_from(status.steps_done).unwrap_or(i32::MAX),
				steps: i32::try_from(self.steps).unwrap_or(i32::MAX),
				engine: self.engine.as_str().into(),
			},
			made: None,
			step: None,
			running: matches!(status.state, WorkState::Stepping { .. } | WorkState::Fitting),
			computing: false,
			changed,
			fitted,
		};
		let stored = self.stored();
		shown.computing = shown.running && !status.store.as_ref().is_some_and(|s| s.has(stored));
		if let Some(store) = &status.store {
			shown.made = store.count().checked_sub(1).and_then(|m| u32::try_from(m).ok());
			shown.step = store.has(stored).then(|| store.step_of(stored));
		}
		if let Some(store) = status.store
			&& store.has(stored)
		{
			let tag = FrameTag { store: store.id(), generation: store.generation(), stored };
			if self.drawn != Some(tag) {
				let look = Look { project, view, schemes, edges: self.edges.as_ref() };
				let pictures = [(&mut self.field, Layer::Field), (&mut self.intensity, Layer::Intensity)];
				if let Some(frame) = status.device_frames.and_then(|frames| frames.hold(tag)) {
					self.drawn = Some(tag);
					let [field, intensity] = pictures.map(|(picture, layer)| {
						let plane = DevicePlane { buffer: &frame.buffer, first_word: frame.first_word(layer)? };
						picture.draw(&self.device, &self.queue, &self.pipeline, plane, &frame.shape, &look)
					});
					(shown.field, shown.intensity) = (field, intensity);
				} else {
					self.uploads.stage(&self.device, &self.queue, &store, stored);
					self.drawn = Some(tag);
					let shape = *store.shape();
					let [field, intensity] = pictures.map(|(picture, layer)| {
						let plane = DevicePlane { buffer: self.uploads.buffer(layer)?, first_word: 0 };
						picture.draw(&self.device, &self.queue, &self.pipeline, plane, &shape, &look)
					});
					(shown.field, shown.intensity) = (field, intensity);
				}
			}
		}
		shown
	}
}

/// A stored frame's layer planes, read from the store into device buffers for a frame the device no longer holds.
#[derive(Default)]
struct Uploads {
	buffers: [Option<wgpu::Buffer>; 2],
}

impl Uploads {
	/// Uploads frame `stored`'s planes.
	fn stage(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, store: &FrameStore, stored: usize) {
		for layer in [Layer::Field, Layer::Intensity] {
			let slot = &mut self.buffers[layer as usize];
			if !store.layers().contains(&layer) {
				*slot = None;
				continue;
			}
			let mut bytes = match store.copy_layer(stored, layer) {
				Ok(bytes) => bytes,
				Err(e) => {
					tracing::warn!("frame {stored} could not be read: {e}");
					*slot = None;
					continue;
				}
			};
			// a storage buffer's size is a multiple of 4
			bytes.resize(bytes.len().next_multiple_of(4), 0);
			let buffer = match slot.take() {
				Some(b) if b.size() >= bytes.len() as u64 => b,
				_ => device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("stored plane"),
					size: (bytes.len() as u64).max(16),
					usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				}),
			};
			queue.write_buffer(&buffer, 0, &bytes);
			*slot = Some(buffer);
		}
	}

	fn buffer(&self, layer: Layer) -> Option<&wgpu::Buffer> {
		self.buffers[layer as usize].as_ref()
	}
}
