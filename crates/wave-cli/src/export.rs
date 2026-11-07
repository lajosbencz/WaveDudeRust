use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use wave_core::engine::types::FrameShape;
use wave_core::io::schemes::SchemeStore;
use wave_core::model::Layer;
use wave_core::run::{FrameStore, PreviewQuality, RunError, RunEvent, SimulationRun};
use wave_export::{ExportFormat, ExportLayers, ExportPlan, ExportState, FrameSource};

use crate::project::load;
use crate::run::{EngineArgs, simulate_with};

#[derive(ClapArgs)]
pub struct Args {
	/// Project file (.wdproj, .wdr or Wave Dude 2D .json).
	project: PathBuf,
	/// Video file, image file (a sequence goes into a folder of its name) or still.
	#[arg(long)]
	out: PathBuf,
	/// Output format; inferred from the extension of --out when absent.
	#[arg(long, value_enum)]
	format: Option<ExportFormat>,
	/// Layers to export; Both writes one output per layer with a layer suffix.
	#[arg(long, value_enum, default_value = "field")]
	layers: ExportLayers,
	/// Steps to run instead of the project's.
	#[arg(long)]
	steps: Option<u32>,
	/// In point in steps (default: the project's).
	#[arg(long, conflicts_with = "still")]
	from: Option<u32>,
	/// Out point in steps, 0 for the end (default: the project's).
	#[arg(long, conflicts_with = "still")]
	to: Option<u32>,
	/// Export this timeline frame as a still.
	#[arg(long)]
	still: Option<usize>,
	/// Burn in the probe markers.
	#[arg(long)]
	markers: bool,
	/// Burn in the probe graph boxes (the export then waits for the whole run, the graphs need its recordings).
	#[arg(long)]
	graphs: bool,
	/// Burn in the source markers.
	#[arg(long)]
	source_markers: bool,
	/// Burn in the scene's glowing edges.
	#[arg(long)]
	edges: bool,
	/// Keep the outputs of a failed or interrupted export.
	#[arg(long)]
	keep_partial: bool,
	#[command(flatten)]
	engine: EngineArgs,
}

fn format_of(args: &Args) -> Result<ExportFormat> {
	if let Some(format) = args.format {
		return Ok(format);
	}
	ExportFormat::from_path(&args.out)
		.ok_or_else(|| anyhow!("cannot infer the format of {}, pass --format", args.out.display()))
}

fn with_extension(out: &Path, format: ExportFormat) -> PathBuf {
	if out.extension().is_some() { out.to_path_buf() } else { out.with_extension(format.default_extension()) }
}

/// Wakes waiters on every run event.
#[derive(Default)]
struct Signal {
	lock: Mutex<()>,
	cond: Condvar,
}

impl Signal {
	fn notify(&self) {
		let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
		self.cond.notify_all();
	}

	/// Waits until `done` holds; the run's events wake it, a timeout covers a write between two events.
	fn wait_until(&self, mut done: impl FnMut() -> bool) {
		let mut guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
		while !done() {
			guard = self.cond.wait_timeout(guard, Duration::from_millis(50)).unwrap_or_else(|e| e.into_inner()).0;
		}
	}
}

/// The frames of a run still in progress: a frame is waited for until the run stores it, ends or is interrupted.
struct LiveFrames<'a> {
	store: &'a FrameStore,
	run: &'a SimulationRun,
	signal: &'a Signal,
	cancel: &'a AtomicBool,
	/// Time spent waiting for the run, nanoseconds.
	waited: AtomicU64,
}

impl FrameSource for LiveFrames<'_> {
	fn frame_count(&self) -> usize {
		self.store.count()
	}

	fn shape(&self) -> FrameShape {
		*self.store.shape()
	}

	fn copy_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> Result<()> {
		FrameSource::copy_layer(self.store, frame, layer, out)
	}

	fn wait_for(&self, frame: usize) -> Result<bool> {
		let started = Instant::now();
		self.signal
			.wait_until(|| frame < self.store.count() || self.run.is_finished() || self.cancel.load(Ordering::Relaxed));
		self.waited.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
		Ok(frame < self.store.count())
	}
}

fn run_failure(run: &mut SimulationRun) -> Option<anyhow::Error> {
	run.wait_completed().err().map(|e| anyhow::Error::new(e).context("the run did not complete"))
}

pub fn run(args: &Args) -> Result<()> {
	let format = format_of(args)?;
	if args.still.is_some() && !format.supports_still() {
		bail!("a still needs an image format, not a video");
	}
	let mut project = load(&args.project)?;
	let display = &mut project.display;
	if args.markers || args.graphs || args.source_markers {
		display.probe_markers_in_export = args.markers;
		display.probe_graphs_in_export = args.graphs;
		display.source_markers_in_export = args.source_markers;
	}
	if args.edges {
		for layer in [&mut display.field, &mut display.intensity] {
			layer.overlay_in_export = true;
			if layer.overlay_gain <= 0.0 {
				layer.overlay_gain = wave_core::model::DisplayLayer::field_default().overlay_gain;
			}
		}
	}
	display.export_from = args.from.unwrap_or(display.export_from);
	display.export_to = args.to.unwrap_or(display.export_to);
	let cancel = Arc::new(AtomicBool::new(false));
	{
		let cancel = cancel.clone();
		ctrlc::set_handler(move || cancel.store(true, Ordering::Relaxed))?;
	}
	let signal = Arc::new(Signal::default());
	let events = {
		let signal = signal.clone();
		Box::new(move |_: RunEvent| signal.notify())
	};
	let quality = PreviewQuality { intensity_root: format.intensity_root(), ..PreviewQuality::FULL };
	let track_intensity = args.layers != ExportLayers::Field;
	let (mut run, store) = simulate_with(&mut project, args.steps, quality, track_intensity, args.engine, events)?;
	let graphs = project.display.probe_graphs_in_export && project.probes.iter().any(|p| p.active);
	if graphs {
		eprintln!("running the simulation first: the probe graphs need its recordings");
		signal.wait_until(|| run.is_finished() || cancel.load(Ordering::Relaxed));
		if cancel.load(Ordering::Relaxed) {
			bail!("interrupted");
		}
		if let Some(e) = run_failure(&mut run) {
			return Err(e);
		}
	}
	let traces = if graphs { run.traces() } else { Vec::new() };
	let spec = run.spec().clone();
	let out = with_extension(&args.out, format);
	let plan = ExportPlan {
		project: &project,
		format,
		layers: args.layers,
		path: out.clone(),
		still: args.still,
		frame_count: spec.frame_count() as usize,
		traces: &traces,
		total_steps: spec.steps as u64,
		keep_partial: args.keep_partial,
	};
	let source = LiveFrames { store: &store, run: &run, signal: &signal, cancel: &cancel, waited: AtomicU64::new(0) };
	let job = plan.build_job(&source, &SchemeStore::default())?;
	let report = job.run(|done, total| eprint!("\rexport frame {done}/{total}"), &cancel);
	eprintln!();
	let waited = Duration::from_nanos(source.waited.load(Ordering::Relaxed));
	for path in &report.written {
		println!("wrote {}", path.display());
	}
	match report.state {
		ExportState::Completed => {}
		ExportState::Cancelled => bail!("the export was interrupted after {} frames", report.frames_done),
		ExportState::Failed(e) => {
			run.cancel();
			if let Err(e) = run.wait_completed()
				&& !matches!(e, RunError::Cancelled)
			{
				return Err(anyhow::Error::new(e).context("the run did not complete"));
			}
			bail!("the export failed after {} frames: {e}", report.frames_done)
		}
	}
	let (width, height) = (store.shape().width(), store.shape().height());
	let busy = report.elapsed.saturating_sub(waited).as_secs_f64();
	println!(
		"{} frames of {width}x{height} to {} in {:.2} s, {:.2} s of it waiting for the run ({:.1} ms/frame exporting)",
		report.frames_done,
		out.display(),
		report.elapsed.as_secs_f64(),
		waited.as_secs_f64(),
		1000.0 * busy / report.frames_done as f64
	);
	Ok(())
}
