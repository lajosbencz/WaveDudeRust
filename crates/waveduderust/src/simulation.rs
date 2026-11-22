//! A project stepped in the caller's thread: the field, the intensity and the probes' traces after any step.
//! Native only: the browser's GPU engine cannot be waited on.
//!
//! ```no_run
//! use waveduderust::{Backend, Simulation};
//!
//! let (project, view) = waveduderust::io::load_project("lens.wdproj".as_ref())?;
//! let mut sim = Simulation::builder(&project).backend(Backend::Cpu).track_intensity(view.intensity.preview).build()?;
//! sim.run(200)?;
//! let field = sim.field()?;
//! println!("{}x{}, centre {}", field.width, field.height, field.at(field.width / 2, field.height / 2));
//! for (probe, trace) in sim.traces()? {
//!     println!("{}: {} readings", probe.name, trace.series.len());
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The same project run to its end in the background, its frames read as they arrive:
//!
//! ```no_run
//! use waveduderust::Simulation;
//! use waveduderust::run::RunOptions;
//!
//! let (project, view) = waveduderust::io::load_project("lens.wdproj".as_ref())?;
//! let run = Simulation::builder(&project).spawn(RunOptions::default().brightness((&view).into()))?;
//! let frames = run.wait_for_frames()?;
//! let end = run.wait_until(|p| p.frames >= 10);
//! println!("step {}/{}", end.steps_done, end.steps);
//! if let Some(frame) = frames.get(9)? {
//!     println!("frame 9 after {} steps: {}x{}", frames.steps_at(9), frame.width, frame.height);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::engine::probes::{ProbeCollector, ProbeTrace, record};
use crate::engine::{self, Fallback, WaveEngine};
use crate::model::normalize::Normalize;
use crate::model::{Probe, Project};
use crate::quality::{Brightness, PreviewQuality};
use crate::scene::compose::compose_scene;
use crate::spec::{RunSpec, RunSpecError};

/// Steps between two drains of the queued probe readings, which bound what the engine holds.
const PROBE_DRAIN: u32 = 256;

/// The engine a simulation steps on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Backend {
	/// The GPU's when one is available, else the CPU's ([`Simulation::fallback`] says why).
	#[default]
	Auto,
	/// The CPU engine, the reference.
	Cpu,
	/// The GPU engine; building fails without one.
	Gpu,
}

/// Why a simulation could not be built or stepped.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum SimulationError {
	#[error(transparent)]
	Spec(#[from] RunSpecError),
	#[error("the GPU engine is not available: {0}")]
	NoGpu(Fallback),
	#[error(transparent)]
	Engine(#[from] engine::EngineError),
	/// The background run's folder or thread could not be set up.
	#[error("starting the run: {0}")]
	Io(std::sync::Arc<std::io::Error>),
}

/// What [`Simulation::builder`] builds.
#[derive(Debug)]
#[must_use]
pub struct SimulationBuilder {
	project: Project,
	backend: Backend,
	track_intensity: bool,
}

impl SimulationBuilder {
	pub fn backend(mut self, backend: Backend) -> Self {
		self.backend = backend;
		self
	}

	/// Tracks the time-averaged intensity even when no probe needs it (a view that shows it).
	pub fn track_intensity(mut self, on: bool) -> Self {
		self.track_intensity = on;
		self
	}

	/// The simulation at step 0; blocks while a GPU engine compiles its pipelines.
	pub fn build(self) -> Result<Simulation, SimulationError> {
		let pixels = compose_scene(&self.project);
		let spec = RunSpec::create(
			&self.project,
			pixels.as_ref(),
			PreviewQuality::FULL,
			Brightness::default(),
			self.track_intensity,
		)?;
		let built = engine::build(spec.scene.clone(), spec.options.clone(), self.backend != Backend::Cpu);
		if let (Backend::Gpu, Some(why)) = (self.backend, built.fallback.clone()) {
			return Err(SimulationError::NoGpu(why));
		}
		let mut engine = built.engine;
		engine.set_emitters(&spec.emitters);
		engine.reset();
		let collector = (!spec.probes.is_empty()).then(|| ProbeCollector::new(&spec.probes, &spec.scene, spec.seed));
		engine.set_probes(collector.as_ref().map_or_default(ProbeCollector::cells));
		Ok(Simulation { engine, spec, collector, recorded: 0, fallback: built.fallback })
	}
}

#[cfg(feature = "run")]
impl SimulationBuilder {
	/// Runs the project to its end on a thread of its own, its frames stored as `options` say.
	pub fn spawn(self, options: crate::run::RunOptions) -> Result<crate::run::Run, SimulationError> {
		crate::run::Run::start(&self.project, self.backend, self.track_intensity, options)
	}
}

/// A project's wave field, stepped by [`step`](Self::step) or [`run`](Self::run).
pub struct Simulation {
	engine: Box<dyn WaveEngine>,
	spec: RunSpec,
	collector: Option<ProbeCollector>,
	/// Steps whose probe readings the collector holds.
	recorded: u32,
	fallback: Option<Fallback>,
}

impl Simulation {
	/// A simulation of `project` as it is when built: later changes to `project` do not reach it.
	pub fn builder(project: &Project) -> SimulationBuilder {
		let mut project = project.clone();
		project.normalize();
		SimulationBuilder { project, backend: Backend::Auto, track_intensity: false }
	}

	/// Advances one time step.
	pub fn step(&mut self) -> Result<(), SimulationError> {
		self.engine.advance_one_step()?;
		if self.collector.is_some() {
			self.engine.queue_probes()?;
			if self.engine.time().is_multiple_of(PROBE_DRAIN) {
				self.drain()?;
			}
		}
		Ok(())
	}

	/// Moves the readings queued so far into the traces.
	fn drain(&mut self) -> Result<(), SimulationError> {
		let Some(collector) = self.collector.as_mut() else { return Ok(()) };
		let pending = self.engine.pending_probes()?;
		self.engine.submit_pending()?;
		record(self.engine.as_mut(), pending, collector, &mut self.recorded, self.spec.schedule.visualize_every)?;
		Ok(())
	}

	/// Advances `steps` time steps.
	pub fn run(&mut self, steps: u32) -> Result<(), SimulationError> {
		(0..steps).try_for_each(|_| self.step())
	}

	/// Steps done.
	pub fn time(&self) -> u32 {
		self.engine.time()
	}

	/// The project's step count, where its run ends.
	pub fn steps(&self) -> u32 {
		self.spec.schedule.steps
	}

	/// Whether the GPU engine steps.
	pub fn is_gpu(&self) -> bool {
		self.engine.is_gpu()
	}

	/// Why the CPU engine steps where [`Backend::Auto`] asked for the GPU's.
	pub fn fallback(&self) -> Option<&Fallback> {
		self.fallback.as_ref()
	}

	/// The canvas after the last step.
	pub fn field(&mut self) -> Result<FieldView, SimulationError> {
		let (field, intensity) = self.engine.prepare_frame()?;
		let scene = self.engine.scene();
		Ok(FieldView {
			width: scene.canvas_width as usize,
			height: scene.canvas_height as usize,
			field,
			intensity: self.spec.options.track_intensity.then_some(intensity),
		})
	}

	/// The active probes, in project order.
	pub fn probes(&self) -> &[Probe] {
		&self.spec.probes
	}

	/// Each active probe with its readings up to the last step, in project order.
	pub fn traces(&mut self) -> Result<impl ExactSizeIterator<Item = (&Probe, &ProbeTrace)>, SimulationError> {
		self.drain()?;
		let traces = self.collector.as_ref().map_or(&[][..], |c| &c.traces);
		Ok(self.spec.probes.iter().zip(traces))
	}
}

/// The canvas's values, row-major from the top-left pixel.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct FieldView {
	pub width: usize,
	pub height: usize,
	pub field: Vec<f32>,
	/// The time-averaged intensity, when tracked.
	pub intensity: Option<Vec<f32>>,
}

impl FieldView {
	/// The field at pixel (`x`, `y`).
	///
	/// # Panics
	///
	/// When the pixel is outside the canvas.
	pub fn at(&self, x: usize, y: usize) -> f32 {
		assert!(x < self.width && y < self.height, "({x}, {y}) is outside the {}x{} canvas", self.width, self.height);
		self.field[y * self.width + x]
	}
}
