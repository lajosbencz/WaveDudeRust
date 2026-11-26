//! The live run (docs/ui/60, The live run): a worker thread steps the project; in the browser a worker pumped on the
//! page with the GPU engine (decision 0035), or the run worker with the CPU engine (decision 0024), whose frames land in
//! the page's mirror store; the newest stored frame is drawn.

use slint::wgpu_30::wgpu;
use waveduderust::model::normalize::Normalize;
use waveduderust::model::{Layer, Project, View};
use waveduderust::quality::PreviewQuality;
use waveduderust::unstable::scene::off_canvas::{OffCanvas, off_canvas};

use waveduderust::run::{FitFrame, FitRequest, FrameStore, Goal, Status, Wanted, WorkState};
use waveduderust::unstable::engine::gpu::{FrameTag, GpuContext};
use waveduderust::unstable::engine::types::SampleFormat;
use waveduderust::unstable::render::fit_plane;
use waveduderust::unstable::render::gpu::{DevicePlane, PicturePipeline};
use waveduderust::unstable::scene::glowing_edges::EdgeImage;
use waveduderust::unstable::spec::{RunRequest, SceneCache};

use waveduderust::io::schemes::SchemeStore;

use crate::gpu::{CpuPicture, Look, Picture};
use crate::ui::{RunState, RunStatus};

/// What an edit asks of the run (docs/ui/60, Edit classes).
pub(crate) use waveduderust::unstable::spec::RunChange as Change;

/// What the live run is driven through, with `RunWorker`'s meaning: natively the core's worker on its thread, in the
/// browser the runs of `web`.
pub(crate) trait Run {
	fn status(&self) -> Status;
	fn set_goal(&mut self, goal: Goal);
	fn with_traces(&self, f: &mut dyn FnMut(&[waveduderust::ProbeTrace]));
	fn take_fit(&mut self) -> Option<FitFrame>;
	fn discard_frames(&mut self);
	/// Takes in what arrived since the last call.
	fn pump(&mut self) {}
}

#[cfg(not(target_arch = "wasm32"))]
impl Run for waveduderust::run::RunWorker {
	fn status(&self) -> Status {
		Self::status(self)
	}

	fn set_goal(&mut self, goal: Goal) {
		Self::set_goal(self, goal);
	}

	fn with_traces(&self, f: &mut dyn FnMut(&[waveduderust::ProbeTrace])) {
		Self::with_traces(self, f);
	}

	fn take_fit(&mut self) -> Option<FitFrame> {
		Self::take_fit(self)
	}

	fn discard_frames(&mut self) {
		Self::discard_frames(self);
	}
}

/// The run and the pictures it feeds.
pub(crate) struct Live {
	worker: Box<dyn Run>,
	/// The run worker's run while the page's GPU runs.
	#[cfg(target_arch = "wasm32")]
	cpu: Option<Box<dyn Run>>,
	revision: u64,
	active: bool,
	/// Every frame (a run) or the position's frame alone.
	wanted: Wanted,
	request: Option<RunRequest>,
	position: u32,
	paused: bool,
	/// The preview quality of the next run (Settings).
	quality: PreviewQuality,
	/// Re-run after edits (Settings).
	pub(crate) auto_rerun: bool,
	/// A 16-bit plane's dither depth (10..16; else plain).
	dither_bits: u32,
	/// Warn when a run's frames exceed the RAM cache.
	pub(crate) warn: bool,
	/// The RAM cache size of a run's store: the browser's pick, natively Auto.
	pub(crate) ram_cache: waveduderust::run::RamCache,
	asked: Option<Ask>,
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
	last_state: Option<std::mem::Discriminant<WorkState>>,
	/// The device the engine and the pictures share; `None` without one.
	context: Option<std::sync::Arc<GpuContext>>,
	painter: Painter,
	/// The scene the edges are made from.
	scene: SceneCache,
	/// The scene's glowing edges, drawn over a picture whose layer shows the outline.
	edges: Option<std::sync::Arc<EdgeImage>>,
	/// Whether engines are built on the GPU, read by the worker's factory.
	gpu: std::sync::Arc<std::sync::atomic::AtomicBool>,
	/// The objects off the canvas last warned of.
	off: Vec<OffCanvas>,
	/// An Auto of an 8-bit frame waiting for the frame in 16-bit: the request and the layer.
	fit: Option<(u64, Layer)>,
	fits: u64,
	/// An export's hold on the run (decision 0012).
	export: Hold,
	/// What the run is made for while an export needs it, over the preview quality.
	export_run: Option<Needs>,
	/// Edits made while an export read the run, taken up once it ends.
	edited_while_reading: bool,
	/// A waiting export's run became ready or stopped.
	news: Option<ExportNews>,
}

/// What an export needs of its run beyond full quality.
#[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "the browser build makes no export plan"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Needs {
	Field,
	/// The intensity tracked, its plane holding sqrt(I) when `root`.
	Intensity {
		root: bool,
	},
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hold {
	Idle,
	Waiting(Needs),
	Reading,
}

/// An export's ask for its run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gate {
	/// The run is complete as needed: read it now; it is held until [`Live::end_export`].
	Ready,
	/// Such a run started; [`Shown::export`] tells when it is ready or stopped.
	Waiting,
	/// Another export holds the run.
	Busy,
}

/// What became of a waiting export's run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportNews {
	Ready,
	Stopped,
}

/// The run an export reads: its store, traces and schedule.
#[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "the browser build makes no export plan"))]
pub(crate) struct ExportFrames {
	pub(crate) store: std::sync::Arc<FrameStore>,
	pub(crate) traces: Vec<waveduderust::ProbeTrace>,
	pub(crate) frame_count: usize,
	pub(crate) total_steps: u64,
}

/// What asked for every frame: Run, Play with frames missing, an edit's automatic re-run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Then {
	Run,
	Play,
	Rerun,
}

/// A memory warning asked (docs/ui/60, Memory warning): what waits on it and the budgets it was asked on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ask {
	pub(crate) then: Then,
	pub(crate) budget: u64,
	pub(crate) auto_budget: u64,
}

/// What Auto brightness finds.
pub(crate) enum Fit {
	/// The brightness, or nothing to fit.
	Done(Option<f64>),
	/// Asked for the frame in 16-bit; a poll brings the fit.
	Pending,
}

/// Where the pictures are coloured.
enum Painter {
	Gpu(Box<GpuPainter>),
	Cpu(Box<[CpuPicture; 2]>),
}

struct GpuPainter {
	device: wgpu::Device,
	queue: wgpu::Queue,
	pipeline: PicturePipeline,
	pictures: [Picture; 2],
	/// The layers of a stored frame the device no longer holds.
	uploads: Uploads,
}

impl Painter {
	fn cpu() -> Self {
		Self::Cpu(Box::new([CpuPicture::new(Layer::Field), CpuPicture::new(Layer::Intensity)]))
	}
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
	pub(crate) export: Option<ExportNews>,
}

/// The browser's run: on the page's GPU with `context`, else in the run worker on the CPU.
#[cfg(target_arch = "wasm32")]
fn run_on(context: Option<&std::sync::Arc<GpuContext>>) -> Box<dyn Run> {
	match context {
		Some(context) => Box::new(crate::web::gpu_run(context.clone())),
		None => Box::new(crate::web::cpu_run()),
	}
}

/// The engine as the status bar names it, on `context`'s GPU or the CPU.
fn engine_of(context: Option<&std::sync::Arc<GpuContext>>) -> String {
	match context.map(|c| c.info.name.as_str()) {
		None => "CPU".to_owned(),
		// a WebGPU adapter may have no name
		Some("") => "GPU: WebGPU".to_owned(),
		Some(name) => format!("GPU: {name}"),
	}
}

/// The off-canvas warning's list (docs/ui/60, Objects off the canvas).
fn off_canvas_list(off: &[OffCanvas]) -> String {
	let item = |o: &OffCanvas| match o {
		OffCanvas::Source { name, outside, of } if outside == of => format!("{name} (left out)"),
		OffCanvas::Source { name, outside, of } => format!("{name} ({outside} of {of} elements left out)"),
		OffCanvas::Part(name) => format!("{name} (left out)"),
		OffCanvas::Probe(name) => format!("{name} (reads at the edge)"),
	};
	off.iter().map(item).collect::<Vec<_>>().join(", ")
}

impl Live {
	/// The run on `context`'s device, or on the CPU without one.
	pub(crate) fn new(context: Option<&std::sync::Arc<GpuContext>>) -> std::io::Result<Self> {
		let gpu = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(context.is_some()));
		#[cfg(not(target_arch = "wasm32"))]
		let worker: Box<dyn Run> = {
			let gpu = gpu.clone();
			let engines: waveduderust::run::EngineFactory = Box::new(move |scene, options| {
				use waveduderust::Backend;
				let backend = if gpu.load(std::sync::atomic::Ordering::Relaxed) { Backend::Gpu } else { Backend::Cpu };
				waveduderust::run::engines(backend)(scene, options)
			});
			let worker = waveduderust::run::RunWorker::new(engines, std::env::temp_dir(), Default::default())?;
			worker.set_notify(Box::new(crate::wake::wake));
			Box::new(worker)
		};
		#[cfg(target_arch = "wasm32")]
		let worker = run_on(context);
		let painter = context.map_or_else(Painter::cpu, |c| {
			let (device, queue) = (c.device.clone(), c.queue.clone());
			Painter::Gpu(Box::new(GpuPainter {
				pipeline: PicturePipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm),
				pictures: [Picture::new(&device, Layer::Field), Picture::new(&device, Layer::Intensity)],
				uploads: Uploads::default(),
				device,
				queue,
			}))
		});
		Ok(Self {
			worker,
			#[cfg(target_arch = "wasm32")]
			cpu: None,
			revision: 0,
			active: false,
			wanted: Wanted::Position,
			request: None,
			position: 0,
			paused: false,
			quality: PreviewQuality::FULL,
			auto_rerun: true,
			warn: true,
			ram_cache: waveduderust::run::RamCache::Auto,
			dither_bits: 0,
			asked: None,
			frames: 0,
			drawn: None,
			fit: None,
			fits: 0,
			export: Hold::Idle,
			export_run: None,
			edited_while_reading: false,
			news: None,
			started: None,
			summarised: false,
			steps: 0,
			engine: engine_of(context),
			last_state: None,
			context: context.cloned(),
			painter,
			scene: SceneCache::default(),
			edges: None,
			off: Vec::new(),
			gpu,
		})
	}

	/// The device of the engines built from now on (Use the GPU): the next run or edit builds on it.
	/// Engines on the GPU when `on` and a device is there. The frames stay: says `false`.
	#[cfg(not(target_arch = "wasm32"))]
	pub(crate) fn set_gpu(&mut self, on: bool) -> bool {
		self.gpu.store(on && self.context.is_some(), std::sync::atomic::Ordering::Relaxed);
		false
	}

	/// The run on the page's GPU when `on` and a device is there, else in the run worker on the CPU (decision 0035).
	/// The run swapped, the old one's frames go with it as Discard frames' do and the new one makes frame 0; the run
	/// worker, started the first time, stays for the page's life. Says whether the run swapped.
	#[cfg(target_arch = "wasm32")]
	pub(crate) fn set_gpu(&mut self, on: bool) -> bool {
		let context = self.context.clone().filter(|_| on);
		if self.gpu.swap(context.is_some(), std::sync::atomic::Ordering::Relaxed) == context.is_some() {
			return false;
		}
		self.engine = engine_of(context.as_ref());
		match context {
			Some(context) => {
				let mut cpu = std::mem::replace(&mut self.worker, Box::new(crate::web::gpu_run(context)));
				// idle and without frames until taken up again
				cpu.set_goal(Goal::default());
				cpu.discard_frames();
				self.cpu = Some(cpu);
			}
			None => self.worker = self.cpu.take().unwrap_or_else(|| Box::new(crate::web::cpu_run())),
		}
		self.last_state = None;
		self.drop_frames();
		true
	}

	/// The preview quality of the next run; another one ends an export's run quality.
	pub(crate) fn set_quality(&mut self, quality: PreviewQuality) {
		if quality != self.quality {
			self.quality = quality;
			if self.export != Hold::Reading {
				self.export_run = None;
			}
		}
	}

	/// Dithering only redraws.
	pub(crate) fn set_dither(&mut self, bits: u32) {
		if bits != self.dither_bits {
			self.dither_bits = bits;
			self.drawn = None;
		}
	}

	/// A GPU device is there.
	pub(crate) fn has_gpu(&self) -> bool {
		self.context.is_some()
	}

	/// The engine as the status bar names it.
	pub(crate) fn engine(&self) -> &str {
		&self.engine
	}

	pub(crate) fn active(&self) -> bool {
		self.active
	}

	pub(crate) fn paused(&self) -> bool {
		self.paused
	}

	/// An Auto brightness fit is on its way.
	pub(crate) fn fit_pending(&self) -> bool {
		self.fit.is_some()
	}

	/// Runs `project` from the start; a running run starts over (docs/ui/60, Automatic re-run).
	/// A project opened: its frame at the position alone (docs/ui/60, The live run).
	pub(crate) fn load(&mut self, project: &Project, view: &View) {
		// nothing carries over from the project before, its tracking neither, nor a warning asked or an export's quality
		self.request = None;
		self.export_run = None;
		self.asked = None;
		self.active = false;
		self.started = None;
		self.wanted = Wanted::Position;
		self.paused = false;
		self.request(project, view);
	}

	/// Every frame (Run); `false` when the memory warning asks first.
	pub(crate) fn run(&mut self, project: &Project, view: &View) -> bool {
		(self.active, self.wanted, self.paused) = (false, Wanted::Position, false);
		self.request(project, view);
		self.want_all(Then::Run)
	}

	/// Every frame of the project as it is, from the position on (Play with frames missing, docs/ui/50); `false`
	/// when the memory warning asks first.
	pub(crate) fn fill(&mut self) -> bool {
		if self.active || self.complete() {
			return true;
		}
		self.paused = false;
		self.want_all(Then::Play)
	}

	/// The worker holds every frame of the newest goal.
	fn complete(&self) -> bool {
		let status = self.worker.status();
		// a probe edit keeps the frames and records the traces anew
		status.adopted == self.revision
			&& status.store.is_some_and(|s| s.is_complete())
			&& matches!(status.state, WorkState::Idle | WorkState::Fitting)
	}

	/// Every frame, after the memory warning when they exceed the RAM cache.
	fn want_all(&mut self, then: Then) -> bool {
		if let Some(ask) = self.ask(then) {
			tracing::debug!(budget = ask.budget, "memory warning asks before the run");
			// the first asker's answer goes for all
			self.asked.get_or_insert(ask);
			(self.active, self.wanted, self.started) = (false, Wanted::Position, None);
			self.publish();
			return false;
		}
		self.go(then);
		true
	}

	/// Every frame, no warning asked; a run under way goes on.
	pub(crate) fn go(&mut self, then: Then) {
		// a run that goes needs no answer
		self.asked = None;
		if self.active {
			return;
		}
		// a paused run waits paused with the edit; a pause with nothing left to do was no pause
		self.paused = then == Then::Rerun && self.paused && !self.summarised;
		self.begin();
		self.active = true;
		self.wanted = Wanted::All;
		// a goal of its own: the worker's echo of it tells when this run is over
		self.revision += 1;
		self.publish();
	}

	/// The warning for the request's frames on the budgets now, when they exceed the RAM cache.
	#[cfg(not(target_arch = "wasm32"))]
	fn ask(&self, then: Then) -> Option<Ask> {
		use waveduderust::run::{Budget, Memory, MemoryWarning, RamCache};
		let request = self.request.as_ref().filter(|_| self.warn)?;
		// the store a run replaces counts as free
		let reclaimed = self.worker.status().store.map_or(0, |s| s.ram_bytes());
		// natively the RAM cache is Auto: the cache lever has nothing to offer
		let budget = Budget::compute(Memory::default(), reclaimed, RamCache::Auto);
		MemoryWarning::check(request, budget, budget).map(|_| Ask { then, budget, auto_budget: budget })
	}

	/// The browser's budget is the worker's estimate, which it learns while the run starts: no warning.
	#[cfg(target_arch = "wasm32")]
	fn ask(&self, _then: Then) -> Option<Ask> {
		None
	}

	/// The warning asked and its arithmetic on the request as it is.
	pub(crate) fn warning(&self) -> Option<(Ask, waveduderust::run::MemoryWarning<'_>)> {
		let (ask, request) = (self.asked?, self.request.as_ref()?);
		Some((ask, waveduderust::run::MemoryWarning { request, budget: ask.budget, auto_budget: ask.auto_budget }))
	}

	/// The warning answered: what it held back.
	pub(crate) fn take_ask(&mut self) -> Option<Ask> {
		self.asked.take()
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
		let (quality, for_export) = match self.export_run {
			None => (self.quality, false),
			Some(Needs::Field) => (PreviewQuality::FULL, false),
			Some(Needs::Intensity { root }) => (PreviewQuality { intensity_root: root, ..PreviewQuality::FULL }, true),
		};
		RunRequest::new(&project, quality, view.into(), view.intensity.preview || tracked || for_export)
	}

	fn submit(&mut self, request: RunRequest) {
		self.revision += 1;
		self.edges = self.scene.pixels(&request.project).map(|p| std::sync::Arc::new(EdgeImage::of(&p)));
		let off = off_canvas(&request.project);
		if off != self.off {
			if !off.is_empty() {
				tracing::warn!("off the canvas: {}", off_canvas_list(&off));
			}
			self.off = off;
		}
		self.drawn = None;
		let schedule = request.schedule();
		(self.frames, self.steps) = (schedule.frame_count(), schedule.steps);
		self.request = Some(request);
		self.publish();
	}

	fn publish(&mut self) {
		crate::wake::wake();
		self.worker.set_goal(Goal {
			revision: self.revision,
			run: self.request.clone(),
			position: self.position,
			paused: self.paused,
			wanted: self.wanted,
			fit: self.fit.map(|(id, _)| FitRequest { id, frame: self.position }),
			ram_cache: self.ram_cache,
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
		Some(self.with_traces(|traces| {
			f(&crate::graphs::Traces { traces, probes, step, frame, total_steps: u64::from(self.steps) })
		}))
	}

	/// The recording of `probe` in the run, as CSV (docs/ui/70, CSV).
	pub(crate) fn probe_csv(&self, probe: &waveduderust::model::Probe) -> Option<Vec<u8>> {
		let request = self.request.as_ref()?;
		let n = request.probes().position(|p| p.id() == probe.id())?;
		let schedule = request.schedule();
		self.with_traces(|traces| {
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
		if self.export == Hold::Reading {
			self.edited_while_reading = true;
			return Change::None;
		}
		// nothing loaded yet: opening a project asks for its frame
		let Some(old) = &self.request else { return Change::None };
		let mut request = self.request_of(project, view);
		let change = old.change_to(&request);
		if change == Change::Frames && self.export_run.take().is_some() {
			self.drop_waiting_export();
			request = self.request_of(project, view);
		}
		let rerun = change == Change::Frames && self.auto_rerun;
		if change == Change::Frames {
			self.fit = None;
			// the new frames are a run of their own, below
			(self.active, self.wanted, self.started) = (false, Wanted::Position, None);
			self.paused &= rerun;
		}
		if change == Change::None {
			// what no run reads may still be drawn: a colour scheme recolours the frame shown
			self.drawn = None;
		} else {
			self.submit(request);
		}
		if rerun {
			self.want_all(Then::Rerun);
		}
		change
	}

	/// The stored frame of the position.
	fn stored(&self) -> usize {
		let position = self.request.as_ref().map_or(self.position, |r| r.schedule().stored_index_of(self.position));
		position as usize
	}

	/// The run has frames stored beyond the one a preview makes for the position.
	pub(crate) fn has_frames(&self) -> bool {
		self.worker.status().store.is_some_and(|s| s.count() > 1)
	}

	/// The frames and traces go, on a new store; the run stops (Discard frames).
	pub(crate) fn discard(&mut self) {
		self.drop_frames();
		self.worker.discard_frames();
	}

	/// The run stops and starts over at frame 0, its frames gone: the goal first, so a run under way does not start
	/// over on the old one.
	fn drop_frames(&mut self) {
		self.fit = None;
		(self.active, self.wanted, self.paused, self.started) = (false, Wanted::Position, false, None);
		self.position = 0;
		self.drawn = None;
		self.revision += 1;
		self.publish();
	}

	/// Calls `f` with the run's probe traces, in its probe order.
	fn with_traces<R>(&self, f: impl FnOnce(&[waveduderust::ProbeTrace]) -> R) -> R {
		let (mut f, mut out) = (Some(f), None);
		self.worker.with_traces(&mut |traces| out = f.take().map(|f| f(traces)));
		out.expect("the run calls a traces reader once")
	}

	/// An export asks for its run: ready when the run is complete, full quality and as `needs` says, else such a
	/// run starts, the memory warning not asked.
	pub(crate) fn request_export(&mut self, project: &Project, view: &View, needs: Needs) -> Gate {
		if self.export != Hold::Idle {
			return Gate::Busy;
		}
		self.asked = None;
		if self.full_for(needs) {
			self.export = Hold::Reading;
			return Gate::Ready;
		}
		self.export_run = Some(needs);
		(self.active, self.wanted, self.paused) = (false, Wanted::Position, false);
		self.request(project, view);
		self.go(Then::Run);
		self.export = Hold::Waiting(needs);
		Gate::Waiting
	}

	/// Every frame of a full-quality run, with the intensity in the form `needs` says, is stored.
	fn full_for(&self, needs: Needs) -> bool {
		self.request.as_ref().is_some_and(|r| {
			r.quality.is_full()
				&& match needs {
					Needs::Field => true,
					Needs::Intensity { root } => {
						r.layers().contains(&Layer::Intensity) && r.quality.intensity_root == root
					}
				}
		}) && self.complete()
	}

	/// The run a ready export reads.
	pub(crate) fn export_frames(&self) -> Option<ExportFrames> {
		let (store, request) = (self.worker.status().store?, self.request.as_ref()?);
		let schedule = request.schedule();
		Some(ExportFrames {
			traces: self.with_traces(<[_]>::to_vec),
			frame_count: store.count(),
			total_steps: u64::from(schedule.steps),
			store,
		})
	}

	/// The export ended or was cancelled: the edits it held back are taken up.
	pub(crate) fn end_export(&mut self, project: &Project, view: &View) -> Change {
		if matches!(self.export, Hold::Waiting(_)) {
			// a waiting export cancelled: said in the status bar, its run stops
			self.drop_waiting_export();
			self.news = None;
			self.stop();
		}
		self.export = Hold::Idle;
		if std::mem::take(&mut self.edited_while_reading) { self.edited(project, view) } else { Change::None }
	}

	fn drop_waiting_export(&mut self) {
		if let Hold::Waiting(_) = self.export {
			self.export = Hold::Idle;
			tracing::warn!("the export was dropped: its run stopped before the last frame");
			self.news = Some(ExportNews::Stopped);
		}
	}

	/// A waiting export's run is ready, or stopped short.
	fn poll_export(&mut self, state: &WorkState) {
		let Hold::Waiting(needs) = self.export else { return };
		if self.full_for(needs) {
			self.export = Hold::Reading;
			self.news = Some(ExportNews::Ready);
		} else if !self.active || matches!(state, WorkState::Failed(_)) {
			self.drop_waiting_export();
		}
	}

	/// No more frames than the position's.
	pub(crate) fn stop(&mut self) {
		self.drop_waiting_export();
		self.revision += 1;
		self.fit = None;
		self.active = false;
		self.wanted = Wanted::Position;
		self.paused = false;
		self.publish();
	}

	/// The newest stored frame drawn, when it changed, and the run's state.
	pub(crate) fn poll(&mut self, project: &Project, view: &View, schemes: &SchemeStore) -> Shown {
		self.worker.pump();
		// the parked run lands or drops what it was sent before it was parked
		#[cfg(target_arch = "wasm32")]
		if let Some(cpu) = &mut self.cpu {
			cpu.pump();
		}
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
		// the worker has taken up the newest goal: its Idle is this run's, not the one before
		let current = status.adopted == self.revision;
		if let (Some(started), RunState::Completed | RunState::Stopped, Some(store)) =
			(self.started, state, status.store.as_ref())
			&& current
			&& !matches!(status.state, WorkState::Stepping { .. } | WorkState::Fitting)
			&& !self.summarised
		{
			self.summarised = true;
			// a run with every frame made is over: nothing to stop or pause
			if state == RunState::Completed {
				(self.active, self.paused) = (false, false);
			}
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
			export: None,
		};
		self.poll_export(&status.state);
		shown.export = self.news.take();
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
				let look = Look { project, view, schemes, edges: self.edges.as_ref(), dither_bits: self.dither_bits };
				self.drawn = Some(tag);
				let [field, intensity] = match &mut self.painter {
					Painter::Gpu(gpu) => {
						let GpuPainter { device, queue, pipeline, pictures, uploads } = &mut **gpu;
						if let Some(frame) = status.device_frames.and_then(|frames| frames.hold(tag)) {
							pictures.each_mut().map(|picture| {
								let plane = DevicePlane {
									buffer: &frame.buffer,
									first_word: frame.first_word(picture.layer())?,
								};
								picture.draw(device, queue, pipeline, plane, &frame.shape, &look)
							})
						} else {
							uploads.stage(device, queue, &store, stored);
							let shape = *store.shape();
							pictures.each_mut().map(|picture| {
								let plane = DevicePlane { buffer: uploads.buffer(picture.layer())?, first_word: 0 };
								picture.draw(device, queue, pipeline, plane, &shape, &look)
							})
						}
					}
					Painter::Cpu(pictures) => {
						let shape = *store.shape();
						pictures.each_mut().map(|picture| {
							let bytes = store.copy_layer(stored, picture.layer()).ok()?;
							Some(picture.draw(&bytes, &shape, &look))
						})
					}
				};
				(shown.field, shown.intensity) = (field, intensity);
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
