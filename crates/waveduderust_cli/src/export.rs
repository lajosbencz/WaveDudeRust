use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use waveduderust::unstable::export::h264::Profile;
use waveduderust::unstable::export::{ExportFormat, ExportLayers, ExportState};
use waveduderust_platform::settings::AppDirs;

use crate::project::{load, user_schemes};
use crate::run::{EngineArgs, parse_steps};

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
	{
		let cancel = cancel.clone();
		ctrlc::set_handler(move || cancel.store(true, Ordering::Relaxed))?;
	}
	if let Some(steps) = args.steps {
		project.simulation.steps = steps;
	}
	let out = with_extension(&args.out, format);
	let done = waveduderust_platform::export::export(
		waveduderust_platform::export::Request {
			project,
			view,
			format,
			out: out.clone(),
			layers: args.layers,
			still: args.still,
			backend: args.engine.backend(),
			schemes: user_schemes(dirs),
			h264,
			h264_profile: Profile::default(),
			keep_partial: args.keep_partial,
		},
		|done, total| eprint!("\rexport frame {done}/{total}"),
		&cancel,
	)?;
	eprintln!();
	let (report, waited) = (done.report, done.waited);
	for path in &report.written {
		println!("wrote {}", path.display());
	}
	match report.state {
		ExportState::Completed => {}
		ExportState::Cancelled => bail!("the export was interrupted after {} frames", report.frames_done),
		ExportState::Failed(e) => bail!("the export failed after {} frames: {e}", report.frames_done),
	}
	let (width, height) = (done.shape.width(), done.shape.height());
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
