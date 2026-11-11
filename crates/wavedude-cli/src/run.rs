use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use wavedude_core::engine::{self, SampleFormat, SampleKind, WaveEngine};
use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::model::{DisplayLayer, Layer, Project};
use wavedude_core::render::frame::{render_half, render_indexed};
use wavedude_core::render::lut::{HalfTable, Palette};
use wavedude_core::run::{Budget, EventSink, FrameStore, PreviewQuality, RunEvent, RunSpec, SimulationRun};
use wavedude_core::scene::compose::compose_scene;

use wavedude_export::csv::write_probe_csv;
use wavedude_export::naming;

use crate::Quality;
use crate::png_out::write_rgba;
use crate::project::load;

/// The engine a subcommand runs on.
#[derive(ClapArgs, Clone, Copy)]
pub(crate) struct EngineArgs {
	/// Run on the GPU engine (the CPU engine, with a warning, when no GPU is available).
	#[arg(long)]
	pub gpu: bool,
}

impl EngineArgs {
	/// The engine for `spec`, reset to its start.
	pub(crate) fn build(self, spec: &RunSpec) -> Box<dyn WaveEngine> {
		engine::build(spec.scene.clone(), spec.options.clone(), self.gpu)
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
	#[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
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
	#[arg(long)]
	steps: Option<u32>,
	#[command(flatten)]
	engine: EngineArgs,
}

fn parse_range(text: &str) -> Result<Range<u32>, String> {
	let (a, b) = text.split_once("..").ok_or("expected A..B")?;
	let (a, b) = (a.parse::<u32>().map_err(|e| e.to_string())?, b.parse::<u32>().map_err(|e| e.to_string())?);
	if a >= b { Err("the range is empty".into()) } else { Ok(a..b) }
}

/// Starts a run of `project`, its progress on stderr.
pub(crate) fn simulate(
	project: &mut Project,
	steps: Option<u32>,
	quality: PreviewQuality,
	track_intensity: bool,
	engine: EngineArgs,
) -> Result<(SimulationRun, Arc<FrameStore>)> {
	let total = steps.unwrap_or(project.simulation.steps);
	let events = Box::new(move |event: RunEvent| {
		if let RunEvent::Progress { steps_done, frames } = event {
			eprint!("\rstep {steps_done}/{total}, {frames} frames");
		}
	});
	simulate_with(project, steps, quality, track_intensity, engine, events)
}

/// Starts a run of `project` whose events go to `events`.
pub(crate) fn simulate_with(
	project: &mut Project,
	steps: Option<u32>,
	quality: PreviewQuality,
	track_intensity: bool,
	engine: EngineArgs,
	events: EventSink,
) -> Result<(SimulationRun, Arc<FrameStore>)> {
	if let Some(steps) = steps {
		project.simulation.steps = steps;
	}
	let pixels = compose_scene(project);
	let spec = RunSpec::create(project, pixels.as_ref(), quality, track_intensity)?;
	let store = Arc::new(FrameStore::new(spec.store_layout(), Budget::compute(0, 0), None)?);
	let engine = engine.build(&spec);
	let run = SimulationRun::start(spec, store.clone(), engine, Some(events))?;
	Ok((run, store))
}

/// Waits for `run` to complete, its progress line ended.
pub(crate) fn finish(run: &mut SimulationRun) -> Result<()> {
	let result = run.wait_completed();
	eprintln!();
	result.context("the run did not complete")
}

enum Colouring {
	Half(Arc<HalfTable>),
	Index(Arc<Palette>),
}

impl Colouring {
	fn new(
		format: SampleFormat,
		settings: &DisplayLayer,
		kind: SampleKind,
		store: &SchemeStore,
		project: &Project,
	) -> Self {
		let scheme = store.resolve(&project.color_schemes, &settings.color_scheme);
		match format {
			SampleFormat::Half => {
				Self::Half(HalfTable::cached(&scheme, settings.black_level, settings.brightness, kind))
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

pub(crate) fn frames(args: &RunArgs) -> Result<()> {
	let mut project = load(&args.project)?;
	let (mut run, store) = simulate(&mut project, args.steps, args.quality.preview(), true, args.engine)?;
	let spec = run.spec().clone();
	finish(&mut run)?;
	fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
	let schemes = SchemeStore::default();
	let shape = spec.shape;
	let root = if shape.intensity_root { SampleKind::IntensityRoot } else { SampleKind::Intensity };
	let frames = spec.covered_frames(store.count() as u32);
	let range = args.frames.clone().unwrap_or(0..frames);
	if range.start >= frames {
		bail!("frame {} is beyond the run's {frames} frames", range.start);
	}
	let layers: Vec<_> = spec
		.layers()
		.iter()
		.map(|&layer| {
			let (name, settings, kind) = match layer {
				Layer::Field => ("field", &project.display.field, SampleKind::Field),
				Layer::Intensity => ("intensity", &project.display.intensity, root),
			};
			(layer, name, Colouring::new(shape.format, settings, kind, &schemes, &project))
		})
		.collect();
	let mut written = 0;
	for k in (range.start..range.end.min(frames)).step_by(args.every as usize) {
		for (layer, name, colouring) in &layers {
			let plane = store.copy_layer(spec.stored_index_of(k) as usize, *layer)?;
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
	let (mut run, _store) = simulate(&mut project, args.steps, Quality::Default.preview(), false, args.engine)?;
	let spec = run.spec().clone();
	finish(&mut run)?;
	let traces = run.traces();
	fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
	if spec.probes.is_empty() {
		println!("the project has no active probes");
		return Ok(());
	}
	let name = args.project.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
	for (probe, trace) in spec.probes.iter().zip(&traces) {
		let path = args.out.join(naming::probe_csv_file_name(&name, &probe.name));
		let mut file = BufWriter::new(File::create(&path).with_context(|| format!("creating {}", path.display()))?);
		write_probe_csv(&mut file, probe, trace, spec.visualize_every, spec.frame_divisor())?;
		file.flush()?;
		println!("{}", path.display());
	}
	Ok(())
}
