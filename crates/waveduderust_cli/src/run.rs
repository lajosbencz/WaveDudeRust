use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use waveduderust::io::schemes::SchemeStore;
use waveduderust::model::{Layer, Project, View, ranges};
use waveduderust::quality::PreviewQuality;
use waveduderust::run::{FrameStore, Memory, Progress, Run, RunError, RunOptions, Stage, engine_fits};
use waveduderust::unstable::engine::{self, SampleFormat, SampleKind, WaveEngine};
use waveduderust::unstable::render::frame::{render_half, render_indexed};
use waveduderust::unstable::render::lut::{HalfTable, Palette};
use waveduderust::unstable::spec::RunSpec;
use waveduderust::{Backend, Simulation};
use waveduderust_platform::settings::AppDirs;

use waveduderust::unstable::export::csv::write_probe_csv;
use waveduderust::unstable::export::naming;

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

	pub(crate) fn backend(self) -> Backend {
		if self.gpu { Backend::Auto } else { Backend::Cpu }
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

/// Least time between two progress lines.
const PROGRESS: Duration = Duration::from_millis(33);

/// A background run of `project`, `steps` in place of its own, with its spill in the default cache folder.
/// A run of `project` as `view` shows it: its brightness, the intensity tracked when shown or `track_intensity`.
pub(crate) fn start(
	project: &mut Project,
	view: &View,
	steps: Option<u32>,
	quality: PreviewQuality,
	track_intensity: bool,
	engine: EngineArgs,
) -> Result<Run> {
	if let Some(steps) = steps {
		project.simulation.steps = steps;
	}
	let track_intensity = track_intensity || view.intensity.preview;
	let builder = Simulation::builder(project).backend(engine.backend()).track_intensity(track_intensity);
	Ok(builder.spawn(RunOptions::default().quality(quality).brightness(view.into()))?)
}

/// Waits for `run`'s end, its progress on stderr; its store.
pub(crate) fn finish(run: &Run) -> Result<Arc<FrameStore>> {
	let mut shown: Option<Instant> = None;
	let progress = loop {
		let p = run.wait_until(|_| shown.is_none_or(|t| t.elapsed() >= PROGRESS));
		eprint!("\rstep {}/{}, {} frames", p.steps_done, p.steps, p.frames);
		if p.ended() {
			break p;
		}
		shown = Some(Instant::now());
	};
	eprintln!();
	outcome(&progress)?;
	run.wait_for_store().map_err(failed)
}

fn failed(e: Arc<RunError>) -> anyhow::Error {
	anyhow::Error::new(RunError::Failed(e)).context("the run did not complete")
}

/// The run's failure or cancellation, when it did not complete.
pub(crate) fn outcome(p: &Progress) -> Result<()> {
	match &p.stage {
		Stage::Failed(e) => Err(failed(e.clone())),
		Stage::Cancelled => bail!("interrupted"),
		_ => Ok(()),
	}
}

enum Colouring {
	Half(Arc<HalfTable>),
	Index(Arc<Palette>),
}

impl Colouring {
	fn new(
		format: SampleFormat,
		layer: Layer,
		kind: SampleKind,
		store: &SchemeStore,
		project: &Project,
		view: &View,
	) -> Self {
		let settings = view.layer(layer);
		let scheme = store.resolve_layer(&project.color_schemes, layer, settings);
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

fn halves(plane: &[u8]) -> Vec<waveduderust::f16> {
	plane.as_chunks::<2>().0.iter().map(|b| waveduderust::f16::from_ne_bytes([b[0], b[1]])).collect()
}

pub(crate) fn frames(args: &RunArgs, dirs: Option<&AppDirs>) -> Result<()> {
	let (mut project, view) = load(&args.project)?;
	let run = start(&mut project, &view, args.steps, args.quality.preview(), true, args.engine)?;
	let store = finish(&run)?;
	let schedule = run.schedule();
	fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
	let schemes = user_schemes(dirs);
	let shape = *store.shape();
	let root = if shape.intensity_root { SampleKind::IntensityRoot } else { SampleKind::Intensity };
	let frames = schedule.covered_frames(store.count() as u32);
	let range = args.frames.clone().unwrap_or(0..frames);
	if range.start >= frames {
		bail!("frame {} is beyond the run's {frames} frames", range.start);
	}
	let layers: Vec<_> = store
		.layers()
		.iter()
		.filter_map(|&layer| {
			let (name, kind) = match layer {
				Layer::Field => ("field", SampleKind::Field),
				Layer::Intensity => ("intensity", root),
				_ => return None,
			};
			Some((layer, name, Colouring::new(shape.format, layer, kind, &schemes, &project, &view)))
		})
		.collect();
	let mut written = 0;
	for k in (range.start..range.end.min(frames)).step_by(args.every as usize) {
		for (layer, name, colouring) in &layers {
			let plane = store.copy_layer(schedule.stored_index_of(k) as usize, *layer)?;
			let rgba = colouring.render(&plane);
			write_rgba(&args.out.join(format!("{name}_{k:04}.png")), shape.width(), shape.height(), &rgba)?;
			written += 1;
		}
	}
	println!("{written} pictures in {}", args.out.display());
	Ok(())
}

pub(crate) fn probes(args: &ProbesArgs) -> Result<()> {
	let (mut project, view) = load(&args.project)?;
	let run = start(&mut project, &view, args.steps, Quality::Default.preview(), false, args.engine)?;
	finish(&run)?;
	let (schedule, traces) = (run.schedule(), run.traces());
	fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
	if run.probes().is_empty() {
		println!("the project has no active probes");
		return Ok(());
	}
	let name = args.project.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
	for (probe, trace) in run.probes().iter().zip(&traces) {
		let path = args.out.join(naming::probe_csv_file_name(&name, &probe.name));
		let mut file = BufWriter::new(File::create(&path).with_context(|| format!("creating {}", path.display()))?);
		write_probe_csv(&mut file, probe, trace, schedule.visualize_every, schedule.frame_divisor())?;
		file.flush()?;
		println!("{}", path.display());
	}
	Ok(())
}
