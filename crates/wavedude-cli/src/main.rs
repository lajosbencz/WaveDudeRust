//! Headless tool for WaveDude: machine facts, benches, the stencils' dispersion, scene pictures, scripted runs and the gate demonstrations.

mod bench;
mod compare;
mod dispersion;
mod export;
mod gates;
mod info;
#[cfg(feature = "mp4")]
mod openh264;
mod png_out;
mod project;
mod run;
mod truth;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::filter::LevelFilter;
use wavedude_core::run::PreviewQuality;

#[derive(Parser)]
#[command(name = "wavedude-cli", version, about = "Headless checks, benches and scripted exports for WaveDude")]
struct Cli {
	/// Log verbosity on stderr: -v info, -vv debug, -vvv trace.
	#[arg(short, long, action = clap::ArgAction::Count, global = true)]
	verbose: u8,
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Machine facts, the preview tier the first start would pick, and the GPU adapter.
	Info,
	/// Time the engine's step, with and without frame captures.
	Bench(bench::Args),
	/// Numerical phase velocity of each stencil at given cells per wavelength, as a Markdown table.
	Dispersion(dispersion::Args),
	/// Write a project's composed scene pixels (or their glowing edges) as a PNG.
	Paint {
		/// Project file (.wdproj, .wdr or Wave Dude 2D .json).
		project: PathBuf,
		/// PNG file to write.
		out: PathBuf,
		/// Write the glowing edges picture instead of the scene.
		#[arg(long)]
		edges: bool,
	},
	/// Run a project on the CPU engine and write field and intensity frames as PNG pictures.
	Run(run::RunArgs),
	/// Run a project and write each active probe's series as CSV.
	Probes(run::ProbesArgs),
	/// Run a project at full quality and export it as video, an image sequence, OpenEXR or a still.
	Export(export::Args),
	/// Cisco's OpenH264 codec for MP4 export: status, enable (and download), disable, delete the download, license.
	#[cfg(feature = "mp4")]
	Openh264 {
		#[arg(value_enum, default_value = "status")]
		action: openh264::Action,
	},
	/// Write the photonic gate demonstrations of the Huygens Optics video as projects, laid out by the mode solver.
	Gates(gates::Args),
	/// Run a two-input project with sources A and B off and on and print the intensity at probes Q and R.
	TruthTable(truth::Args),
	/// Compare a project's scene, fields and probes against a dump of the C# WaveDudeNext (tools/compare-csharp).
	Compare(compare::Args),
	/// Load a project and save it in another format, chosen by the extension (.wdr or .wdproj).
	Convert {
		/// Project file to read (.wdproj, .wdr or Wave Dude 2D .json).
		input: PathBuf,
		/// Project file to write (.wdr or .wdproj).
		output: PathBuf,
	},
	/// Load a project, report its grid, sub-steps, frame count and store sizes; non-zero exit on error.
	Check {
		/// Project file (.wdproj, .wdr or Wave Dude 2D .json).
		project: PathBuf,
	},
}

/// Preview quality of a run.
#[derive(Clone, Copy, ValueEnum)]
pub enum Quality {
	/// 16-bit samples, every frame, full resolution (what an export uses).
	Full,
	/// 8-bit palette index samples, every frame, full resolution (the preview's default).
	Default,
}

impl Quality {
	pub fn preview(self) -> PreviewQuality {
		match self {
			Self::Full => PreviewQuality::FULL,
			Self::Default => PreviewQuality::DEFAULT,
		}
	}
}

fn main() -> Result<()> {
	let cli = Cli::parse();
	let level = match cli.verbose {
		0 => LevelFilter::WARN,
		1 => LevelFilter::INFO,
		2 => LevelFilter::DEBUG,
		_ => LevelFilter::TRACE,
	};
	tracing_subscriber::fmt().with_max_level(level).with_writer(std::io::stderr).init();
	match cli.command {
		Command::Info => info::run(),
		Command::Bench(args) => bench::run(&args),
		Command::Dispersion(args) => dispersion::run(&args),
		Command::Paint { project, out, edges } => project::paint(&project, &out, edges),
		Command::Run(args) => run::frames(&args),
		Command::Probes(args) => run::probes(&args),
		Command::Export(args) => export::run(&args),
		#[cfg(feature = "mp4")]
		Command::Openh264 { action } => openh264::run(action),
		Command::Gates(args) => gates::run(&args),
		Command::TruthTable(args) => truth::run(&args),
		Command::Compare(args) => compare::run(&args),
		Command::Convert { input, output } => project::convert(&input, &output),
		Command::Check { project } => project::check(&project),
	}
}
