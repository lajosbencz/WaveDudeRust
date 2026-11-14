use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, bail};
use clap::{Args as ClapArgs, ValueEnum};
use wavedude_core::engine::{
	self, Emitter, EngineOptions, FrameShape, PendingCapture, SampleFormat, SceneData, Snapshot, WaveEngine,
};
use wavedude_core::model::{PulseShape, Scene, Stencil};

use crate::run::EngineArgs;

const WARMUP_STEPS: u32 = 20;

#[derive(Clone, Copy, ValueEnum)]
enum StencilChoice {
	Nine,
	Fourth,
	Both,
}

#[derive(ClapArgs)]
pub(crate) struct Args {
	/// Canvas width in cells.
	#[arg(long, default_value_t = 1920)]
	width: u32,
	/// Canvas height in cells.
	#[arg(long, default_value_t = 1080)]
	height: u32,
	/// Damping rim around the canvas in cells.
	#[arg(long, default_value_t = 32)]
	rim: u32,
	/// Timed steps after a warm-up of 20.
	#[arg(long, default_value_t = 200)]
	steps: u32,
	/// Stencil to time.
	#[arg(long, value_enum, default_value = "both")]
	stencil: StencilChoice,
	/// Capture both planes as f16 every N steps in the second row of each stencil, as a run does (each
	/// capture collected after the next has begun, the buffers reused); 0 skips it.
	#[arg(long, default_value_t = 2)]
	captures_every: u32,
	/// Time the GPU engine after the CPU's.
	#[command(flatten)]
	engine: EngineArgs,
}

fn source(x: i32, y: i32) -> Emitter {
	Emitter {
		x: f64::from(x),
		y: f64::from(y),
		level: 100.0,
		amplitude: 1.0,
		phase_radians: 0.0,
		pulsed: false,
		shape: PulseShape::Gaussian,
		on_width_cycles: 10,
		off_width_cycles: 10,
		off_width_base: 10,
		randomize_off_width: 0,
		detach_after_step: None,
		rng_seed: std::num::NonZeroU32::MIN,
	}
}

struct Row {
	engine: &'static str,
	stencil: Stencil,
	sub_steps: u32,
	captures: u32,
	ms_per_step: f64,
}

/// Captures as a run makes them: one in flight while the engine steps on, two buffers reused.
struct Captures {
	shape: FrameShape,
	pending: Option<PendingCapture>,
	spare: Vec<Snapshot>,
}

impl Captures {
	fn take(&mut self, engine: &mut dyn WaveEngine) -> Result<()> {
		let buffer = self.spare.pop().unwrap_or_else(|| Snapshot::new(self.shape));
		let next = engine.begin_capture(&self.shape, buffer)?;
		if let Some(previous) = self.pending.replace(next) {
			self.spare.push(std::hint::black_box(engine.finish_capture(previous)?));
		}
		Ok(())
	}

	fn finish(&mut self, engine: &mut dyn WaveEngine) -> Result<()> {
		if let Some(last) = self.pending.take() {
			self.spare.push(std::hint::black_box(engine.finish_capture(last)?));
		}
		Ok(())
	}
}

fn time_engine(engine: &mut dyn WaveEngine, shape: &FrameShape, steps: u32, captures_every: u32) -> Result<f64> {
	let mut captures = Captures { shape: *shape, pending: None, spare: Vec::new() };
	let mut run = |engine: &mut dyn WaveEngine, count: u32| -> Result<()> {
		for i in 1..=count {
			engine.advance_one_step()?;
			if captures_every > 0 && i % captures_every == 0 {
				captures.take(engine)?;
			}
		}
		captures.finish(engine)
	};
	run(engine, WARMUP_STEPS)?;
	let started = Instant::now();
	run(engine, steps)?;
	// Waits for the card.
	std::hint::black_box(engine.capture_frame(shape)?);
	Ok(started.elapsed().as_secs_f64() * 1000.0 / steps as f64)
}

pub(crate) fn run(args: &Args) -> Result<()> {
	if args.steps == 0 || args.width == 0 || args.height == 0 {
		bail!("width, height and steps must be positive");
	}
	let stencils: &[Stencil] = match args.stencil {
		StencilChoice::Nine => &[Stencil::NinePoint],
		StencilChoice::Fourth => &[Stencil::FourthOrder],
		StencilChoice::Both => &[Stencil::NinePoint, Stencil::FourthOrder],
	};
	let scene = Arc::new(SceneData::blank(
		args.width,
		args.height,
		&Scene { damping_edge: -(args.rim as i32), ..Scene::default() },
	));
	let shape = FrameShape {
		canvas_width: args.width,
		canvas_height: args.height,
		divisor: 1,
		format: SampleFormat::Half,
		field_brightness: 30.0,
		intensity_brightness: 30.0,
		intensity_root: true,
	};
	let emitters = [source(args.width as i32 / 2, args.height as i32 / 2)];
	let mut rows = Vec::new();
	for &stencil in stencils {
		let options = EngineOptions {
			stencil,
			source_frequency_scale: 2.0,
			image_source_scale: 1.0,
			time_decay: 10.0,
			intensity_scale: 1.0,
			track_intensity: true,
		};
		let mut captures = vec![0];
		if args.captures_every > 0 {
			captures.push(args.captures_every);
		}
		for &every in &captures {
			for gpu in [false, true].into_iter().filter(|&gpu| !gpu || args.engine.gpu) {
				let mut engine = engine::build(scene.clone(), options.clone(), gpu).engine;
				if gpu && engine.name() != "GPU" {
					bail!("no GPU engine to time");
				}
				engine.set_emitters(&emitters);
				let ms_per_step = time_engine(engine.as_mut(), &shape, args.steps, every)?;
				let (name, sub_steps) = (if gpu { "gpu" } else { "cpu" }, engine.sub_steps());
				rows.push(Row { engine: name, stencil, sub_steps, captures: every, ms_per_step });
			}
		}
	}
	print_table(args, &rows);
	Ok(())
}

fn print_table(args: &Args, rows: &[Row]) {
	println!(
		"{}x{} canvas, rim {}, {} steps after {} warm-up, intensity tracked, captures of both planes as f16",
		args.width, args.height, args.rim, args.steps, WARMUP_STEPS
	);
	println!();
	println!("| engine | stencil | sub-steps | capture every | ms/step | capture ms |");
	println!("|---|---|---|---|---|---|");
	for row in rows {
		let name = match row.stencil {
			Stencil::NinePoint => "9-point",
			Stencil::FourthOrder => "4th order",
		};
		let capture = if row.captures == 0 {
			("none".to_owned(), "-".to_owned())
		} else {
			let base = rows
				.iter()
				.find(|r| r.engine == row.engine && r.stencil == row.stencil && r.captures == 0)
				.expect("a row without captures per engine and stencil")
				.ms_per_step;
			let per_capture = (row.ms_per_step - base) * row.captures as f64;
			(row.captures.to_string(), format!("{per_capture:.2}"))
		};
		println!(
			"| {} | {name} | {} | {} | {:.3} | {} |",
			row.engine, row.sub_steps, capture.0, row.ms_per_step, capture.1,
		);
	}
}
