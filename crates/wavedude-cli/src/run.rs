use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use wavedude_app_state::settings::AppDirs;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::engine::{self, SampleFormat, SampleKind, WaveEngine};
use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::model::{Layer, Project, ranges};
use wavedude_core::quality::PreviewQuality;
use wavedude_core::render::frame::{render_half, render_indexed};
use wavedude_core::render::lut::{HalfTable, Palette};
use wavedude_core::run::{
	EngineFactory, FrameStore, Goal, Memory, RunError, RunWorker, Status, Wanted, WorkState, engine_fits,
};
use wavedude_core::spec::{RunRequest, RunSpec, SceneCache};
use wavedude_core::sync::lock;

use wavedude_export::csv::write_probe_csv;
use wavedude_export::naming;

use crate::Quality;
use crate::png_out::write_rgba;
use crate::project::{load, user_schemes};

/// The engine a subcommand runs on.
#[derive(ClapArgs, Clone, Copy)]
pub(crate) struct EngineArgs {
	/// Run on the GPU engine (the CPU engine, with a warning, when no GPU is available).
	#[arg(long)]
	pub gpu: bool,
}

impl EngineArgs {
	/// The engine for `spec`, reset to its start; an error when its grid does not fit the memory.
	pub(crate) fn build(self, spec: &RunSpec) -> Result<Box<dyn WaveEngine>> {
		engine_fits(&spec.scene, &spec.options, Memory::System.read().1)?;
		Ok(engine::build(spec.scene.clone(), spec.options.clone(), self.gpu).engine)
	}

	/// A run worker's engines.
	fn factory(self) -> EngineFactory {
		Box::new(move |scene, options| engine::build(scene, options, self.gpu).engine)
	}
}

#[derive(ClapArgs)]
pub(crate) struct RunArgs {
	/// Project file (.wdproj or Wave Dude 2D .json).
	project: PathBuf,
	/// Directory for `field_NNNN.png` and `intensity_NNNN.png` (created if missing).
	#[arg(long)]
	out: PathBuf,
	/// Steps to run instead of the project's.
	#[arg(long, value_parser = parse_steps)]
	steps: Option<u32>,
	/// Quality the frames are stored and coloured at.
	#[arg(long, value_enum, default_value = "default")]
	quality: Quality,
	/// Timeline frames to write, as a half-open range A..B (default: all).
	#[arg(long, value_parser = parse_range)]
	frames: Option<Range<u32>>,
	/// Write every Nth frame of the range.
	#[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
	every: u32,
	#[command(flatten)]
	engine: EngineArgs,
}

#[derive(ClapArgs)]
pub(crate) struct ProbesArgs {
	/// Project file (.wdproj or Wave Dude 2D .json).
	project: PathBuf,
	/// Directory for the CSV files (created if missing).
	#[arg(long)]
	out: PathBuf,
	/// Steps to run instead of the project's.
	#[arg(long, value_parser = parse_steps)]
	steps: Option<u32>,
	#[command(flatten)]
	engine: EngineArgs,
}

/// A step count the model accepts.
pub(crate) fn parse_steps(text: &str) -> Result<u32, String> {
	let steps = text.parse::<u32>().map_err(|e| e.to_string())?;
	let range = ranges::SIM_STEPS;
	if range.contains(f64::from(steps)) {
		Ok(steps)
	} else {
		Err(format!("{steps} is outside {}..={}", range.min, range.max))
	}
}

fn parse_range(text: &str) -> Result<Range<u32>, String> {
	let (a, b) = text.split_once("..").ok_or("expected A..B")?;
	let (a, b) = (a.parse::<u32>().map_err(|e| e.to_string())?, b.parse::<u32>().map_err(|e| e.to_string())?);
	if a >= b { Err("the range is empty".into()) } else { Ok(a..b) }
}

/// The goal revision of a [`Simulation`], which sets one goal.
const REVISION: u64 = 1;
/// Least time between two progress lines.
const PROGRESS: Duration = Duration::from_millis(33);

/// A run of a project to its end: a live run that wants every frame.
pub(crate) struct Simulation {
	worker: RunWorker,
	pub spec: RunSpec,
}

impl Simulation {
	/// Starts a run of `project`, `steps` in place of its own, with its spill in the default cache folder.
	pub(crate) fn start(
		project: &mut Project,
		steps: Option<u32>,
		quality: PreviewQuality,
		track_intensity: bool,
		engine: EngineArgs,
	) -> Result<Self> {
		if let Some(steps) = steps {
			project.simulation.steps = steps;
		}
		let request = RunRequest::new(project, quality, track_intensity);
		let scene = Arc::<Mutex<SceneCache>>::default();
		let spec = request.spec(&mut lock(&scene))?;
		let worker = RunWorker::new(engine.factory(), FrameStore::default_dir(), scene)?;
		worker.set_goal(Goal { revision: REVISION, run: Some(request), wanted: Wanted::All, ..Goal::default() });
		Ok(Self { worker, spec })
	}

	/// Blocks until the run ended or `stop` holds; the status then.
	pub(crate) fn wait_until(&self, mut stop: impl FnMut(&Status) -> bool) -> Status {
		self.worker.wait_until(|s| ended(s) || stop(s))
	}

	pub(crate) fn status(&self) -> Status {
		self.worker.status()
	}

	/// Has the waits ask their `stop` again.
	pub(crate) fn waker(&self) -> impl Fn() + Send + Sync + 'static {
		self.worker.waker()
	}

	/// The run's store once it has one; an error when the run failed first.
	pub(crate) fn store(&self) -> Result<Arc<FrameStore>> {
		store_of(self.wait_until(|s| s.store.is_some()))
	}

	/// Waits for the run's end, its progress on stderr; its store.
	pub(crate) fn finish(&self) -> Result<Arc<FrameStore>> {
		let total = self.spec.schedule.steps;
		let mut shown: Option<Instant> = None;
		let status = loop {
			let s = self.wait_until(|_| shown.is_none_or(|t| t.elapsed() >= PROGRESS));
			let ended = ended(&s);
			// Past the last frame only the probes are stepped: a completed run reports every step.
			let steps = if ended && s.state == WorkState::Idle { total } else { s.steps_done };
			let frames = s.store.as_ref().map_or(0, |store| self.spec.schedule.covered_frames(store.count() as u32));
			eprint!("\rstep {steps}/{total}, {frames} frames");
			if ended {
				break s;
			}
			shown = Some(Instant::now());
		};
		eprintln!();
		store_of(status)
	}

	/// The probe traces in the spec's probe order.
	pub(crate) fn traces(&self) -> Vec<ProbeTrace> {
		self.worker.with_traces(<[ProbeTrace]>::to_vec)
	}
}

/// The run completed or failed.
fn ended(s: &Status) -> bool {
	match s.state {
		WorkState::Failed(_) => true,
		WorkState::Idle => s.adopted == REVISION,
		_ => false,
	}
}

/// The run's failure, when it failed.
pub(crate) fn outcome(s: &Status) -> Result<()> {
	match &s.state {
		WorkState::Failed(e) => {
			Err(anyhow::Error::new(RunError::Failed(e.clone())).context("the run did not complete"))
		}
		_ => Ok(()),
	}
}

/// The store of a run that did not fail.
fn store_of(s: Status) -> Result<Arc<FrameStore>> {
	outcome(&s)?;
	Ok(s.store.expect("a run that did not fail publishes its store"))
}

enum Colouring {
	Half(Arc<HalfTable>),
	Index(Arc<Palette>),
}

impl Colouring {
	fn new(format: SampleFormat, layer: Layer, kind: SampleKind, store: &SchemeStore, project: &Project) -> Self {
		let settings = project.display.layer(layer);
		let scheme = store.resolve_layer(project, layer);
		match format {
			SampleFormat::Half => {
				Self::Half(HalfTable::cached(&scheme, settings.black_level, settings.brightness as f32, kind))
			}
			SampleFormat::Index => Self::Index(Palette::cached(&scheme, settings.black_level)),
		}
	}

	fn render(&self, plane: &[u8]) -> Vec<u8> {
		match self {
			Self::Half(table) => render_half(&halves(plane), table),
			Self::Index(palette) => render_indexed(plane, palette),
		}
	}
}

fn halves(plane: &[u8]) -> Vec<wavedude_core::f16> {
	plane.as_chunks::<2>().0.iter().map(|b| wavedude_core::f16::from_ne_bytes([b[0], b[1]])).collect()
}

pub(crate) fn frames(args: &RunArgs, dirs: Option<&AppDirs>) -> Result<()> {
	let mut project = load(&args.project)?;
	let run = Simulation::start(&mut project, args.steps, args.quality.preview(), true, args.engine)?;
	let store = run.finish()?;
	let spec = &run.spec;
	fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
	let schemes = user_schemes(dirs);
	let shape = spec.shape;
	let root = if shape.intensity_root { SampleKind::IntensityRoot } else { SampleKind::Intensity };
	let frames = spec.schedule.covered_frames(store.count() as u32);
	let range = args.frames.clone().unwrap_or(0..frames);
	if range.start >= frames {
		bail!("frame {} is beyond the run's {frames} frames", range.start);
	}
	let layers: Vec<_> = spec
		.layers()
		.iter()
		.map(|&layer| {
			let (name, kind) = match layer {
				Layer::Field => ("field", SampleKind::Field),
				Layer::Intensity => ("intensity", root),
			};
			(layer, name, Colouring::new(shape.format, layer, kind, &schemes, &project))
		})
		.collect();
	let mut written = 0;
	for k in (range.start..range.end.min(frames)).step_by(args.every as usize) {
		for (layer, name, colouring) in &layers {
			let plane = store.copy_layer(spec.schedule.stored_index_of(k) as usize, *layer)?;
			let rgba = colouring.render(&plane);
			write_rgba(&args.out.join(format!("{name}_{k:04}.png")), shape.width(), shape.height(), &rgba)?;
			written += 1;
		}
	}
	println!("{written} pictures in {}", args.out.display());
	Ok(())
}

pub(crate) fn probes(args: &ProbesArgs) -> Result<()> {
	let mut project = load(&args.project)?;
	let run = Simulation::start(&mut project, args.steps, Quality::Default.preview(), false, args.engine)?;
	run.finish()?;
	let (spec, traces) = (&run.spec, run.traces());
	fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
	if spec.probes.is_empty() {
		println!("the project has no active probes");
		return Ok(());
	}
	let name = args.project.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
	for (probe, trace) in spec.probes.iter().zip(&traces) {
		let path = args.out.join(naming::probe_csv_file_name(&name, &probe.name));
		let mut file = BufWriter::new(File::create(&path).with_context(|| format!("creating {}", path.display()))?);
		write_probe_csv(&mut file, probe, trace, spec.schedule.visualize_every, spec.schedule.frame_divisor())?;
		file.flush()?;
		println!("{}", path.display());
	}
	Ok(())
}
