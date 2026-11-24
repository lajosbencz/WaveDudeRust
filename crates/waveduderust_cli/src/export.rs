use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::Poll;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use waveduderust::model::Layer;
use waveduderust::quality::PreviewQuality;
use waveduderust::run::store::StoreError;
use waveduderust::run::{FrameStore, Run, RunError};
use waveduderust::unstable::engine::types::FrameShape;
use waveduderust::unstable::export::h264::Profile;
use waveduderust::unstable::export::{ExportFormat, ExportLayers, ExportPlan, ExportState, FrameSource, naming};
use waveduderust_platform::settings::AppDirs;

use crate::project::{load, user_schemes};
use crate::run::{EngineArgs, outcome, parse_steps, start};

#[derive(ClapArgs)]
pub(crate) struct Args {
	/// Project file (.wdproj or Wave Dude 2D .json).
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
	#[arg(long, value_parser = parse_steps)]
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

/// The frames of a run still in progress: a frame is waited for until the run stores it, ends or is interrupted.
struct LiveFrames<'a> {
	run: &'a Run,
	store: Arc<FrameStore>,
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

	fn poll_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> Result<Poll<()>, StoreError> {
		self.store.poll_layer(frame, layer, out)
	}

	fn wait_for(&self, frame: usize) -> Result<bool, StoreError> {
		let started = Instant::now();
		self.run.wait_until(|_| frame < self.store.count() || self.cancel.load(Ordering::Relaxed));
		self.waited.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
		Ok(frame < self.store.count())
	}
}

pub(crate) fn run(args: &Args, dirs: Option<&AppDirs>) -> Result<()> {
	let format = format_of(args)?;
	#[cfg(feature = "mp4")]
	let h264: Option<Arc<dyn waveduderust::unstable::export::h264::H264Factory>> = if format == ExportFormat::Mp4 {
		Some(Arc::new(waveduderust::unstable::export::h264::OpenH264Factory::new(crate::openh264::library(dirs)?)))
	} else {
		None
	};
	#[cfg(not(feature = "mp4"))]
	let h264 = None;
	if args.still.is_some() && !format.supports_still() {
		bail!("a still needs an image format, not a video");
	}
	let (mut project, mut view) = load(&args.project)?;
	let display = &mut view;
	if args.markers || args.graphs || args.source_markers {
		display.probe_markers_in_export = args.markers;
		display.probe_graphs_in_export = args.graphs;
		display.source_markers_in_export = args.source_markers;
	}
	if args.edges {
		for layer in [&mut display.field, &mut display.intensity] {
			layer.overlay_in_export = true;
			if layer.overlay_gain <= 0.0 {
				layer.overlay_gain = waveduderust::model::DisplayLayer::field_default().overlay_gain;
			}
		}
	}
	display.export_from = args.from.unwrap_or(display.export_from);
	display.export_to = args.to.unwrap_or(display.export_to);
	let cancel = Arc::new(AtomicBool::new(false));
	let wake = Arc::new(OnceLock::<Box<dyn Fn() + Send + Sync>>::new());
	{
		let (cancel, wake) = (cancel.clone(), wake.clone());
		ctrlc::set_handler(move || {
			cancel.store(true, Ordering::Relaxed);
			if let Some(wake) = wake.get() {
				wake();
			}
		})?;
	}
	let quality = PreviewQuality { intensity_root: format.intensity_root(), ..PreviewQuality::FULL };
	let track_intensity = args.layers != ExportLayers::Field;
	let run = start(&mut project, &view, args.steps, quality, track_intensity, args.engine)?;
	let canceller = run.canceller();
	let _ = wake.set(Box::new(move || canceller.cancel()));
	// a Ctrl-C before the handler could reach the run
	if cancel.load(Ordering::Relaxed) {
		run.cancel();
	}
	let graphs = view.probe_graphs_in_export && project.probes.iter().any(|p| p.active);
	if graphs {
		eprintln!("running the simulation first: the probe graphs need its recordings");
		outcome(&run.wait_until(|_| false))?;
	}
	let traces = if graphs { run.traces() } else { Vec::new() };
	let store = run.wait_for_store().map_err(|e| anyhow::Error::new(RunError::Failed(e)))?;
	let schedule = run.schedule();
	let out = with_extension(&args.out, format);
	let plan = ExportPlan {
		project: &project,
		view: &view,
		format,
		fs: Arc::new(waveduderust::unstable::export::fs::StdFs),
		outputs: naming::layer_outputs(&out, args.layers),
		still: args.still,
		frame_count: schedule.frame_count() as usize,
		traces: &traces,
		total_steps: schedule.steps as u64,
		keep_partial: args.keep_partial,
		h264,
		h264_profile: Profile::default(),
	};
	#[allow(clippy::arc_with_non_send_sync, reason = "the job shares the source on this thread")]
	let source = Arc::new(LiveFrames { run: &run, store: store.clone(), cancel: &cancel, waited: AtomicU64::new(0) });
	let job = plan.build_job(source.clone(), &user_schemes(dirs))?;
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
			outcome(&run.progress())?;
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
