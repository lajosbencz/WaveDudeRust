//! Physical comparison against the C# `WaveDudeNext`: reads the dump of `tools/compare-csharp`, runs the same
//! project through the CPU engine on the same step schedule and writes `report.md` with the scene, field and
//! probe statistics and side-by-side pictures; with a `--render` dump also the rendered frames and the probe
//! graphs. The solvers differ by design (docs/spec/05-deviations.md), so the figures are correlations and ratios, not
//! bit equality.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use clap::Args as ClapArgs;
use wavedude_core::engine::ProbeReadings;
use wavedude_core::engine::probes::{ProbeCollector, ProbeTrace};
use wavedude_core::engine::types::SampleKind;
use wavedude_core::f16;
use wavedude_core::io::nine_point_speed;
use wavedude_core::io::schemes::SchemeStore;
use wavedude_core::io::wdproj::BRIGHTNESS_GAIN;
use wavedude_core::model::normalize::box_in_canvas;
use wavedude_core::model::{DisplayLayer, Probe, Project, Stencil};
use wavedude_core::quality::PreviewQuality;
use wavedude_core::render::{HalfTable, fit_brightness, render_half};
use wavedude_core::scene::pixels::ScenePixels;
use wavedude_core::spec::RunSpec;
use wavedude_view::TextMetrics;
use wavedude_view::graph::{self, GraphData};

use crate::png_out::{write_grey, write_rgba};
use crate::project::{load, scene_pixels};
use crate::run::EngineArgs;

#[derive(ClapArgs)]
pub(crate) struct Args {
	/// Project file, the one the C# dump was made from.
	project: PathBuf,
	/// Directory written by tools/compare-csharp (run with --probe-steps, and --render for the rendered frames).
	csharp: PathBuf,
	/// Directory for report.md and its pictures (created if missing).
	#[arg(long)]
	out: PathBuf,
	/// Steps to run; must match the dump.
	#[arg(long, default_value_t = 600)]
	steps: u32,
	/// Steps between dumped fields; must match the dump.
	#[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..))]
	every: u32,
	#[command(flatten)]
	engine: EngineArgs,
}

/// Longest side the side-by-side pictures are shrunk to at most.
const PICTURE_SIDE: usize = 1200;
/// Grey levels of the side-by-side pictures.
const GREY_LEVELS: usize = 64;
/// Share of the series treated as the transient and left out of the probe statistics.
const TRANSIENT: f64 = 0.5;
/// Share of the peak |u| that marks the wave front.
const FRONT_THRESHOLD: f32 = 0.01;
/// Long-wave phase speed of the original's 9-point kernel (0.066 diagonal, 0.184 axis): sqrt((2 x 0.184 +
/// 4 x 0.066) / 2), the constant its Mur condition uses.
/// Pixels left out next to the source and the front when measuring a wavelength.
const WAVELENGTH_MARGIN: usize = 10;
/// Font size of the probe graph layouts, the C# dump's `ProbeGraphStyle.Estimate` (0.6 em a character, 1.3 em a line).
const GRAPH_TEXT_SIZE: f32 = 11.0;
/// Sources whose wave fronts are tabulated.
const MAX_FRONTS: usize = 4;
/// Distance in pixels from an index edge within which a scene difference counts as an edge difference.
const EDGE_REACH: usize = 2;

/// One field and intensity dump of the canvas, rim excluded.
struct Dump {
	step: u32,
	field: Vec<f32>,
	intensity: Vec<f32>,
}

/// The C# side as tools/compare-csharp wrote it.
struct CSharp {
	facts: HashMap<String, String>,
	width: usize,
	height: usize,
	scene_rgba: Vec<u8>,
	dumps: Vec<Dump>,
	probe_names: Vec<String>,
	probes: Series,
	render: Option<CsRender>,
}

/// What the C# dump's `--render` adds: Next's pictures of the planes, the brightness it would fit and its probe graphs.
struct CsRender {
	/// Per dump, the RGBA pictures of the field and the intensity.
	pictures: Vec<(Vec<u8>, Vec<u8>)>,
	/// Per dump, the fitted brightness of the field and the intensity.
	fits: Vec<(Option<f64>, Option<f64>)>,
	/// Per active probe, as the run ended.
	graphs: Vec<CsGraph>,
}

/// One line of the C# dump's graph.txt with the probe's series and profile.
struct CsGraph {
	series: Vec<f32>,
	profile: Vec<f32>,
	profile_length: f64,
	steps_sampled: u64,
	y: (f64, f64),
	y_ticks: String,
	x_ticks: String,
}

/// One Rust run of the project.
struct RustRun {
	label: String,
	/// File name infix of the run's pictures.
	tag: &'static str,
	sub_steps: u32,
	dumps: Vec<Dump>,
	/// Per probe, value after step i + 1.
	probes: Vec<Vec<f64>>,
	/// Per active probe, what its graph shows after the last step.
	traces: Vec<ProbeTrace>,
}

fn read_f32(path: &Path, n: usize) -> Result<Vec<f32>> {
	let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
	ensure!(bytes.len() == 4 * n, "{} holds {} bytes, expected {}", path.display(), bytes.len(), 4 * n);
	Ok(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

fn write_f32(path: &Path, values: &[f32]) -> Result<()> {
	let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
	fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn read_png_rgba(path: &Path) -> Result<(usize, usize, Vec<u8>)> {
	let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
	let decoder = png::Decoder::new(std::io::BufReader::new(file));
	let mut reader = decoder.read_info()?;
	let size = reader.output_buffer_size().with_context(|| format!("{}: too large to decode", path.display()))?;
	let mut buf = vec![0; size];
	let info = reader.next_frame(&mut buf)?;
	ensure!(info.bit_depth == png::BitDepth::Eight, "{}: 8 bits per channel expected", path.display());
	let data = &buf[..info.buffer_size()];
	let rgba = match info.color_type {
		png::ColorType::Rgba => data.to_vec(),
		png::ColorType::Rgb => data.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
		other => bail!("{}: colour type {other:?} not handled", path.display()),
	};
	Ok((info.width as usize, info.height as usize, rgba))
}

fn read_csharp(dir: &Path, steps: u32, every: u32) -> Result<CSharp> {
	let text = fs::read_to_string(dir.join("scene.txt")).context("reading the C# scene.txt")?;
	let facts: HashMap<String, String> =
		text.lines().filter_map(|l| l.split_once(' ')).map(|(k, v)| (k.to_owned(), v.trim().to_owned())).collect();
	let fact = |k: &str| facts.get(k).with_context(|| format!("scene.txt has no {k}"));
	let width: usize = fact("width")?.parse()?;
	let height: usize = fact("height")?.parse()?;
	ensure!(
		fact("steps")?.parse::<u32>()? == steps && fact("every")?.parse::<u32>()? == every,
		"the C# dump ran another schedule: steps {} every {}",
		fact("steps")?,
		fact("every")?
	);
	let (w, h, scene_rgba) = read_png_rgba(&dir.join("scene.png"))?;
	ensure!((w, h) == (width, height), "scene.png is {w}x{h}, scene.txt says {width}x{height}");
	let n = width * height;
	let dumps = (every..=steps)
		.step_by(every as usize)
		.map(|step| {
			Ok(Dump {
				step,
				field: read_f32(&dir.join(format!("field_{step}.f32")), n)?,
				intensity: read_f32(&dir.join(format!("intensity_{step}.f32")), n)?,
			})
		})
		.collect::<Result<Vec<_>>>()?;
	let (probe_names, probes) = match fs::read_to_string(dir.join("probes.csv")) {
		Ok(csv) => parse_probes(&csv)?,
		Err(_) => (Vec::new(), Vec::new()),
	};
	let render = dir.join("fit.txt").exists().then(|| read_render(dir, &dumps, (width, height))).transpose()?;
	Ok(CSharp { facts, width, height, scene_rgba, dumps, probe_names, probes, render })
}

fn read_render(dir: &Path, dumps: &[Dump], (width, height): (usize, usize)) -> Result<CsRender> {
	let picture = |name: String| {
		let (w, h, rgba) = read_png_rgba(&dir.join(&name))?;
		ensure!((w, h) == (width, height), "{name} is {w}x{h}, expected {width}x{height}");
		Ok(rgba)
	};
	let pictures = dumps
		.iter()
		.map(|d| Ok((picture(format!("field_{}.png", d.step))?, picture(format!("intensity_{}.png", d.step))?)))
		.collect::<Result<Vec<_>>>()?;
	let fit = |cell: &str| (cell != "-").then(|| cell.parse::<f64>()).transpose();
	let fits = fs::read_to_string(dir.join("fit.txt"))?
		.lines()
		.map(|line| {
			let cells: Vec<&str> = line.split(' ').collect();
			ensure!(cells.len() == 3, "fit.txt: expected `step field intensity`, got `{line}`");
			Ok((fit(cells[1])?, fit(cells[2])?))
		})
		.collect::<Result<Vec<_>>>()?;
	ensure!(fits.len() == dumps.len(), "fit.txt has {} lines for {} dumps", fits.len(), dumps.len());
	let graphs = fs::read_to_string(dir.join("graph.txt"))?
		.lines()
		.map(|line| {
			let c: Vec<&str> = line.split('\t').collect();
			ensure!(c.len() == 9, "graph.txt: expected 9 columns, got `{line}`");
			let k: usize = c[0].parse()?;
			Ok(CsGraph {
				series: read_f32(&dir.join(format!("series_{k}.f32")), c[1].parse()?)?,
				profile: read_f32(&dir.join(format!("profile_{k}.f32")), c[2].parse()?)?,
				profile_length: c[3].parse()?,
				steps_sampled: c[4].parse()?,
				y: (c[5].parse()?, c[6].parse()?),
				y_ticks: c[7].to_owned(),
				x_ticks: c[8].to_owned(),
			})
		})
		.collect::<Result<Vec<_>>>()?;
	Ok(CsRender { pictures, fits, graphs })
}

/// Per probe, (step, value) pairs.
type Series = Vec<Vec<(u32, f64)>>;

fn parse_probes(csv: &str) -> Result<(Vec<String>, Series)> {
	let mut lines = csv.lines();
	let names: Vec<String> = lines.next().context("empty probes.csv")?.split(';').skip(1).map(str::to_owned).collect();
	let mut series = vec![Vec::new(); names.len()];
	for line in lines.filter(|l| !l.is_empty()) {
		let mut cells = line.split(';');
		let step: u32 = cells.next().context("probes.csv: no step")?.parse()?;
		for (s, cell) in series.iter_mut().zip(cells) {
			s.push((step, cell.parse()?));
		}
	}
	Ok((names, series))
}

fn run_rust(
	project: &Project,
	pixels: &ScenePixels,
	steps: u32,
	every: u32,
	(label, tag): (&str, &'static str),
	engine: EngineArgs,
) -> Result<RustRun> {
	let spec = RunSpec::create(project, Some(pixels), PreviewQuality::FULL, true)?;
	let mut engine = engine.build(&spec);
	let mut collector = ProbeCollector::new(&spec.probes, &spec.scene, spec.seed);
	engine.set_emitters(&spec.emitters);
	engine.set_probes(collector.cells());
	engine.reset();
	let mut readings = ProbeReadings::default();
	let mut probes = vec![Vec::with_capacity(steps as usize); spec.probes.len()];
	let mut dumps = Vec::new();
	for step in 1..=steps {
		engine.advance_one_step()?;
		if !spec.probes.is_empty() {
			engine.sample_probes(&mut readings)?;
			collector.record_step(&readings, step as u64);
			for (series, values) in probes.iter_mut().zip(readings.iter()) {
				series.push(values.iter().map(|&v| v as f64).sum::<f64>() / values.len() as f64);
			}
		}
		if step % every == 0 {
			let (field, intensity) = engine.prepare_frame()?;
			dumps.push(Dump { step, field, intensity });
			eprint!("\r{label}: step {step}/{steps}");
		}
	}
	eprintln!();
	if !spec.probes.is_empty() {
		collector.record_frame(&readings, steps as u64);
	}
	Ok(RustRun { label: label.to_owned(), tag, sub_steps: engine.sub_steps(), dumps, probes, traces: collector.traces })
}

/// Pearson correlation of `a` and `b`; 0 when either is constant.
fn pearson(a: impl Iterator<Item = f64> + Clone, b: impl Iterator<Item = f64> + Clone) -> f64 {
	let (mut n, mut sa, mut sb) = (0.0, 0.0, 0.0);
	for (x, y) in a.clone().zip(b.clone()) {
		n += 1.0;
		sa += x;
		sb += y;
	}
	if n == 0.0 {
		return 0.0;
	}
	let (ma, mb) = (sa / n, sb / n);
	let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
	for (x, y) in a.zip(b) {
		sab += (x - ma) * (y - mb);
		saa += (x - ma) * (x - ma);
		sbb += (y - mb) * (y - mb);
	}
	if saa == 0.0 || sbb == 0.0 { 0.0 } else { sab / (saa * sbb).sqrt() }
}

fn rms(a: &[f32]) -> f64 {
	(a.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / a.len() as f64).sqrt()
}

/// Correlation, RMS difference over the reference's RMS, PSNR in dB against the reference's peak |value|, the
/// least-squares gain g of `other` ~ g `reference` and the RMS of `other / g - reference` over the reference's RMS.
#[derive(Debug)]
struct FieldStats {
	pearson: f64,
	nrms: f64,
	psnr: f64,
	gain: f64,
	nrms_gain: f64,
}

fn field_stats(reference: &[f32], other: &[f32]) -> FieldStats {
	let diff = (reference.iter().zip(other).map(|(&a, &b)| (a as f64 - b as f64).powi(2)).sum::<f64>()
		/ reference.len() as f64)
		.sqrt();
	let peak = reference.iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
	let reference_rms = rms(reference).max(f64::MIN_POSITIVE);
	let gain = gain(reference, other);
	let residual = (reference.iter().zip(other).map(|(&a, &b)| (b as f64 / gain - a as f64).powi(2)).sum::<f64>()
		/ reference.len() as f64)
		.sqrt();
	FieldStats {
		pearson: pearson(reference.iter().map(|&v| v as f64), other.iter().map(|&v| v as f64)),
		nrms: diff / reference_rms,
		psnr: 20.0 * (peak / diff.max(f64::MIN_POSITIVE)).log10(),
		gain,
		nrms_gain: residual / reference_rms,
	}
}

/// Least-squares g of `other` ~ g `reference`; 1 for a zero reference.
fn gain(reference: &[f32], other: &[f32]) -> f64 {
	let (ab, aa) = reference
		.iter()
		.zip(other)
		.fold((0.0, 0.0), |(ab, aa), (&a, &b)| (ab + a as f64 * b as f64, aa + a as f64 * a as f64));
	if aa > 0.0 && ab != 0.0 { ab / aa } else { 1.0 }
}

/// Columns from `sx` along row `sy` to the farthest pixel at or right of the source whose |u| exceeds
/// [`FRONT_THRESHOLD`] of the peak there; None once the front has reached the canvas's right edge.
fn front(u: &[f32], width: usize, sx: i32, sy: i32) -> Option<usize> {
	let (sx, sy) = (usize::try_from(sx).ok()?, usize::try_from(sy).ok()?);
	let row = u.get(sy * width..(sy + 1) * width)?.get(sx..)?;
	let peak = row.iter().fold(0f32, |m, v| m.max(v.abs()));
	let limit = FRONT_THRESHOLD * peak;
	let far = row.iter().rposition(|v| v.abs() > limit)?;
	(far + 1 < row.len()).then_some(far)
}

/// Mean wavelength in pixels along row `sy` between [`WAVELENGTH_MARGIN`] right of the source and as far before
/// the front (or the canvas edge): twice the mean distance between interpolated zero crossings.
fn wavelength(u: &[f32], width: usize, sx: i32, sy: i32) -> Option<f64> {
	let end = front(u, width, sx, sy).unwrap_or(width.checked_sub(usize::try_from(sx).ok()? + 1)?);
	let (sx, sy) = (usize::try_from(sx).ok()?, usize::try_from(sy).ok()?);
	let row = u.get(sy * width + sx..sy * width + sx + end)?;
	let span = row.get(WAVELENGTH_MARGIN..row.len().checked_sub(WAVELENGTH_MARGIN)?)?;
	let crossings: Vec<f64> = span
		.windows(2)
		.enumerate()
		.filter(|(_, p)| (p[0] < 0.0) != (p[1] < 0.0) && p[0] != p[1])
		.map(|(i, p)| i as f64 + (p[0] / (p[0] - p[1])) as f64)
		.collect();
	let (first, last) = (crossings.first()?, crossings.last()?);
	(crossings.len() >= 3).then(|| 2.0 * (last - first) / (crossings.len() - 1) as f64)
}

/// The value at which the side-by-side pictures reach full black or white: the 99.5th percentile of |u|.
fn brightness_scale(u: &[f32]) -> f32 {
	let mut a: Vec<f32> = u.iter().map(|v| v.abs()).collect();
	let k = (a.len() as f64 * 0.995) as usize;
	let (_, v, _) = a.select_nth_unstable_by(k, f32::total_cmp);
	if *v > 0.0 { *v } else { 1.0 }
}

fn grey(v: f32, scale: f32) -> u8 {
	(127.5 + 127.5 * (v / scale).clamp(-1.0, 1.0)).round() as u8
}

/// Box-averages a greyscale picture by the smallest whole factor that brings both sides to at most `max_side`.
fn shrink(grey: &[u8], width: usize, height: usize, max_side: usize) -> (usize, usize, Vec<u8>) {
	let f = width.max(height).div_ceil(max_side);
	let (w, h) = (width / f, height / f);
	let mut out = vec![0u8; w * h];
	for y in 0..h {
		for x in 0..w {
			let sum: u32 = (0..f)
				.flat_map(|dy| (0..f).map(move |dx| (dy, dx)))
				.map(|(dy, dx)| grey[(y * f + dy) * width + x * f + dx] as u32)
				.sum();
			out[y * w + x] = (sum / (f * f) as u32) as u8;
		}
	}
	(w, h, out)
}

/// `panels` of `w` x `h` pixels of `px` bytes, `cols` to a row and 4 pixels apart, on white.
fn tile(panels: &[&[u8]], (w, h): (usize, usize), px: usize, cols: usize) -> (usize, usize, Vec<u8>) {
	let gap = 4;
	let rows = panels.len().div_ceil(cols);
	let (pw, ph) = (cols * w + (cols - 1) * gap, rows * h + (rows - 1) * gap);
	let mut picture = vec![255u8; pw * ph * px];
	for (k, panel) in panels.iter().enumerate() {
		let (ox, oy) = ((k % cols) * (w + gap), (k / cols) * (h + gap));
		for y in 0..h {
			let at = ((oy + y) * pw + ox) * px;
			picture[at..at + w * px].copy_from_slice(&panel[y * w * px..(y + 1) * w * px]);
		}
	}
	(pw, ph, picture)
}

/// C# | Rust | Rust / `gain` - C# (side by side, or stacked for a canvas wider than 1.5 times its height), shrunk and
/// quantised to [`GREY_LEVELS`] levels so a noisy field stays a small file.
fn side_by_side(path: &Path, w: usize, h: usize, csharp: &[f32], rust: &[f32], gain: f64) -> Result<()> {
	let scale = brightness_scale(csharp);
	let diff: Vec<f32> = csharp.iter().zip(rust).map(|(a, b)| (*b as f64 / gain) as f32 - a).collect();
	let panels: Vec<Vec<u8>> =
		[csharp, rust, &diff[..]].iter().map(|p| p.iter().map(|&v| grey(v, scale)).collect()).collect();
	let stacked = w * 2 > h * 3;
	let panels: Vec<&[u8]> = panels.iter().map(Vec::as_slice).collect();
	let (pw, ph, picture) = tile(&panels, (w, h), 1, if stacked { 1 } else { 3 });
	let (sw, sh, mut small) = shrink(&picture, pw, ph, PICTURE_SIDE);
	let step = 256 / GREY_LEVELS;
	small.iter_mut().for_each(|g| *g = (*g as usize / step * step + step / 2).min(255) as u8);
	write_grey(path, sw as u32, sh as u32, &small)
}

/// Pixels within [`EDGE_REACH`] of a change of the red (index) channel of `rgba`.
fn edge_mask(rgba: &[u8], w: usize, h: usize) -> Vec<bool> {
	let red = |x: usize, y: usize| rgba[(y * w + x) * 4];
	let mut edge = vec![false; w * h];
	for y in 0..h {
		for x in 0..w {
			let r = red(x, y);
			if (x + 1 < w && red(x + 1, y) != r) || (y + 1 < h && red(x, y + 1) != r) {
				edge[y * w + x] = true;
			}
		}
	}
	let mut near = vec![false; w * h];
	for y in 0..h {
		for x in 0..w {
			if edge[y * w + x] {
				for ny in y.saturating_sub(EDGE_REACH)..(y + EDGE_REACH + 1).min(h) {
					for nx in x.saturating_sub(EDGE_REACH)..(x + EDGE_REACH + 1).min(w) {
						near[ny * w + nx] = true;
					}
				}
			}
		}
	}
	near
}

fn scene_section(report: &mut String, out: &Path, cs_dir: &Path, cs: &CSharp, rust: &ScenePixels) -> Result<()> {
	let (w, h) = (cs.width, cs.height);
	ensure!(
		(rust.width as usize, rust.height as usize) == (w, h),
		"the Rust canvas is {}x{}, the C# one {w}x{h}",
		rust.width,
		rust.height
	);
	let near = edge_mask(&cs.scene_rgba, w, h);
	let mut channel = [(0usize, 0u8, 0u64); 4];
	let mut buckets = [0usize; 4];
	let (mut differing, mut at_edges) = (0usize, 0usize);
	let mut diff_png = vec![0u8; w * h * 4];
	for i in 0..w * h {
		let mut any = false;
		for c in 0..4 {
			let d = cs.scene_rgba[i * 4 + c].abs_diff(rust.rgba[i * 4 + c]);
			if d > 0 {
				any = true;
				channel[c].0 += 1;
				channel[c].1 = channel[c].1.max(d);
				channel[c].2 += d as u64;
			}
			if c < 3 {
				diff_png[i * 4 + c] = d.saturating_mul(8);
			}
		}
		diff_png[i * 4 + 3] = 255;
		let dr = cs.scene_rgba[i * 4].abs_diff(rust.rgba[i * 4]);
		match dr {
			0 => {}
			1 => buckets[0] += 1,
			2..=5 => buckets[1] += 1,
			6..=20 => buckets[2] += 1,
			_ => buckets[3] += 1,
		}
		if any {
			differing += 1;
			at_edges += near[i] as usize;
		}
	}
	fs::copy(cs_dir.join("scene.png"), out.join("scene_csharp.png")).context("copying the C# scene.png")?;
	write_rgba(&out.join("scene_rust.png"), w as u32, h as u32, &rust.rgba)?;
	write_rgba(&out.join("scene_diff.png"), w as u32, h as u32, &diff_png)?;
	let pct = |n: usize| 100.0 * n as f64 / (w * h) as f64;
	writeln!(report, "## Painted scene\n")?;
	writeln!(
		report,
		"{differing} of {} pixels differ ({:.3} %); {at_edges} of them ({:.1} %) lie within {EDGE_REACH} px of an \
		 index edge of the C# scene.\n",
		w * h,
		pct(differing),
		if differing > 0 { 100.0 * at_edges as f64 / differing as f64 } else { 0.0 }
	)?;
	writeln!(report, "| channel | pixels differing | max difference | mean difference where differing |")?;
	writeln!(report, "|---|---|---|---|")?;
	for (name, (n, max, sum)) in ["red (index)", "green (sources)", "blue (absorption)", "alpha"].iter().zip(channel) {
		let mean = if n > 0 { sum as f64 / n as f64 } else { 0.0 };
		writeln!(report, "| {name} | {n} | {max} | {mean:.2} |")?;
	}
	writeln!(
		report,
		"\nRed differences by size: 1 level {}, 2-5 levels {}, 6-20 levels {}, over 20 levels {}.\n",
		buckets[0], buckets[1], buckets[2], buckets[3]
	)?;
	writeln!(
		report,
		"C#: ![C# scene](scene_csharp.png) Rust: ![Rust scene](scene_rust.png) Difference x8: ![scene difference](scene_diff.png)\n"
	)?;
	Ok(())
}

fn field_section(
	report: &mut String,
	out: &Path,
	cs: &CSharp,
	run: &RustRun,
	sources: &[(String, f64, f64)],
) -> Result<()> {
	let w = cs.width;
	writeln!(report, "### Field, Rust {}\n", run.label)?;
	writeln!(
		report,
		"| step | Pearson | RMS diff / RMS C# | PSNR dB | gain | RMS diff after gain | energy Rust / C# | \
		 intensity Pearson |"
	)?;
	writeln!(report, "|---|---|---|---|---|---|---|---|")?;
	for (c, r) in cs.dumps.iter().zip(&run.dumps) {
		let s = field_stats(&c.field, &r.field);
		let energy = (rms(&r.field) / rms(&c.field).max(f64::MIN_POSITIVE)).powi(2);
		let ip = pearson(c.intensity.iter().map(|&v| v as f64), r.intensity.iter().map(|&v| v as f64));
		writeln!(
			report,
			"| {} | {:.3} | {:.3} | {:.1} | {:.3} | {:.3} | {energy:.3} | {ip:.3} |",
			c.step, s.pearson, s.nrms, s.psnr, s.gain, s.nrms_gain
		)?;
	}
	writeln!(report)?;
	for (side, dumps) in [("C#", &cs.dumps), ("Rust", &run.dumps)] {
		let peak = |d: &Dump| d.field.iter().fold(0f32, |m, v| m.max(v.abs()));
		if let Some(d) = dumps.iter().find(|d| d.field.iter().any(|v| !v.is_finite()) || peak(d) > 1e3) {
			writeln!(report, "**The {side} field is unstable: non-finite or above 1e3 at step {}.**\n", d.step)?;
		}
	}
	if !sources.is_empty() {
		let mut header = String::from("| step |");
		let mut rule = String::from("|---|");
		for (name, x, y) in sources {
			header += &format!(" {name} ({x}, {y}): front C# / Rust | wavelength C# / Rust |");
			rule += "---|---|";
		}
		writeln!(
			report,
			"Wave front and wavelength right of each source along its row (meaningful where one source owns the row):\n\n\
			 {header}\n{rule}"
		)?;
		let show = |f: Option<usize>| f.map_or_else(|| "edge".to_owned(), |f| f.to_string());
		let length = |l: Option<f64>| l.map_or_else(|| "-".to_owned(), |l| format!("{l:.1}"));
		for (c, r) in cs.dumps.iter().zip(&run.dumps) {
			let mut line = format!("| {} |", c.step);
			for &(_, x, y) in sources {
				let (x, y) = (x.floor() as i32, y.floor() as i32);
				line += &format!(
					" {} / {} | {} / {} |",
					show(front(&c.field, w, x, y)),
					show(front(&r.field, w, x, y)),
					length(wavelength(&c.field, w, x, y)),
					length(wavelength(&r.field, w, x, y))
				);
			}
			writeln!(report, "{line}")?;
		}
		writeln!(report)?;
	}
	for (c, r) in cs.dumps.iter().zip(&run.dumps) {
		let name = format!("field{}_{}.png", run.tag, c.step);
		side_by_side(&out.join(&name), w, cs.height, &c.field, &r.field, gain(&c.field, &r.field))?;
		writeln!(report, "Step {}: ![step {}]({name})\n", c.step, c.step)?;
	}
	Ok(())
}

/// A probe's agreement over the second half of the run: Pearson and amplitude ratio (None for a silent C# series).
struct ProbeRow {
	name: String,
	pearson: f64,
	ratio: Option<f64>,
}

/// The probe table of every Rust run; returns the rows of the first.
fn probe_section(report: &mut String, cs: &CSharp, runs: &[RustRun], steps: u32) -> Result<Vec<ProbeRow>> {
	let mut rows = Vec::new();
	if cs.probe_names.is_empty() {
		return Ok(rows);
	}
	let from = (steps as f64 * TRANSIENT) as u32;
	writeln!(report, "## Probes\n")?;
	writeln!(
		report,
		"Each probe's mean over its cells in its layer, every step; statistics over steps {} to {steps} (after the \
		 transient). Amplitude ratio: RMS of the Rust series over the C# one (mean, for an intensity).\n",
		from + 1
	)?;
	writeln!(report, "| probe | Rust run | Pearson | amplitude ratio | C# RMS | Rust RMS |")?;
	writeln!(report, "|---|---|---|---|---|---|")?;
	for (p, name) in cs.probe_names.iter().enumerate() {
		let cs_series: Vec<(u32, f64)> = cs.probes[p].iter().copied().filter(|&(s, _)| s > from).collect();
		for (i, run) in runs.iter().enumerate() {
			let rust: Vec<f64> = cs_series.iter().map(|&(s, _)| run.probes[p][s as usize - 1]).collect();
			let csv: Vec<f64> = cs_series.iter().map(|&(_, v)| v).collect();
			let r = pearson(csv.iter().copied(), rust.iter().copied());
			let q = |a: &[f64]| (a.iter().map(|v| v * v).sum::<f64>() / a.len().max(1) as f64).sqrt();
			let (qc, qr) = (q(&csv), q(&rust));
			let ratio = (qc > 0.0).then(|| qr / qc);
			writeln!(report, "| {name} | {} | {r:.3} | {} | {qc:.3e} | {qr:.3e} |", run.label, show(ratio))?;
			if i == 0 {
				rows.push(ProbeRow { name: name.clone(), pearson: r, ratio });
			}
		}
	}
	writeln!(report)?;
	Ok(rows)
}

/// The text metrics of the C# dump's graph layouts.
struct Estimate;

impl TextMetrics for Estimate {
	fn line_height(&mut self) -> f32 {
		GRAPH_TEXT_SIZE * 1.3
	}

	fn width(&mut self, text: &str) -> f32 {
		text.chars().count() as f32 * GRAPH_TEXT_SIZE * 0.6
	}
}

/// A probe graph's y range and tick lists in the notation of the C# dump's graph.txt (`label@position`).
struct GraphView {
	y: (f64, f64),
	y_ticks: String,
	x_ticks: String,
}

fn graph_view(probe: &Probe, data: &GraphData<'_>, steps: u32, (w, h): (u32, u32)) -> GraphView {
	let b = box_in_canvas(probe, w, h);
	let layout = graph::layout(probe, data, steps as u64, b.width as f32, b.height as f32, &mut Estimate);
	let ticks =
		|t: &[graph::Tick]| t.iter().map(|t| format!("{}@{:.2}", t.label, t.position)).collect::<Vec<_>>().join(" ");
	// a line probe's range is that of what the layout draws: nothing before its start step
	let started = probe.start_after == 0 || data.steps_sampled >= probe.start_after as u64;
	let shown = if probe.shows_profile() && !started { GraphData::default() } else { *data };
	let (_, _, y_min, y_max) = graph::ranges(probe, &shown, steps as u64);
	GraphView { y: (y_min, y_max), y_ticks: ticks(&layout.y_ticks), x_ticks: ticks(&layout.x_ticks) }
}

/// Mean Rec. 709 luma of RGBA pixels, 0 to 1.
fn mean_luma(rgba: &[u8]) -> f64 {
	let sum: f64 = rgba
		.as_chunks::<4>()
		.0
		.iter()
		.map(|p| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64)
		.sum();
	sum / (255.0 * (rgba.len() / 4) as f64)
}

/// Mean absolute difference of the colour channels of two RGBA pictures, in levels.
fn mean_channel_diff(a: &[u8], b: &[u8]) -> f64 {
	let sum: u64 = a
		.as_chunks::<4>()
		.0
		.iter()
		.zip(b.as_chunks::<4>().0)
		.flat_map(|(p, q)| (0..3).map(move |c| p[c].abs_diff(q[c]) as u64))
		.sum();
	sum as f64 / (3 * (a.len() / 4)) as f64
}

/// A plane as a frame stores it for `kind`: the field itself, the intensity as its square root, as halves.
fn halves(plane: &[f32], kind: SampleKind) -> Vec<f16> {
	let root = kind == SampleKind::IntensityRoot;
	plane.iter().map(|&v| f16::from_f32(if root { v.sqrt() } else { v })).collect()
}

/// Where a rendered picture differs from Next's: pixels, largest channel difference, and per differing sample value
/// (half bits) its pixel count, Next's colour and ours.
#[derive(Default)]
struct Parity {
	pixels: usize,
	max: u8,
	samples: BTreeMap<u16, (usize, [u8; 4], [u8; 4])>,
}

/// The `samples` of a plane through `table` against Next's `picture` of it.
fn parity(samples: &[f16], table: &HalfTable, picture: &[u8]) -> Parity {
	let ours = render_half(samples, table);
	let mut p = Parity::default();
	for ((h, a), b) in samples.iter().zip(picture.as_chunks::<4>().0).zip(ours.as_chunks::<4>().0) {
		let max = (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
		if max > 0 {
			p.pixels += 1;
			p.max = p.max.max(max);
			let e = p.samples.entry(h.to_bits()).or_insert((0, [a[0], a[1], a[2], a[3]], [b[0], b[1], b[2], b[3]]));
			e.0 += 1;
		}
	}
	p
}

/// The colours of `layer`: its scheme through its black level at `brightness`.
fn layer_table(project: &Project, layer: &DisplayLayer, brightness: f64, kind: SampleKind) -> HalfTable {
	let scheme = SchemeStore::default().resolve(&project.color_schemes, &layer.color_scheme);
	HalfTable::new(&scheme, layer.black_level, brightness as f32, kind)
}

/// C# field | Rust field over C# intensity | Rust intensity, shrunk like the grey pictures.
fn render_picture(path: &Path, (w, h): (usize, usize), panels: [&[u8]; 4]) -> Result<()> {
	let (pw, ph, picture) = tile(&panels, (w, h), 4, 2);
	let channels: Vec<_> = (0..3)
		.map(|c| shrink(&picture.as_chunks::<4>().0.iter().map(|p| p[c]).collect::<Vec<u8>>(), pw, ph, PICTURE_SIDE))
		.collect();
	let (sw, sh) = (channels[0].0, channels[0].1);
	let small: Vec<u8> =
		(0..sw * sh).flat_map(|i| [channels[0].2[i], channels[1].2[i], channels[2].2[i], 255]).collect();
	write_rgba(path, sw as u32, sh as u32, &small)
}

/// One dump step of the summary, per layer (field, intensity): fit ratio C# / Rust and luma ratio Rust / C#.
struct StepRow {
	step: u32,
	/// Pixels differing in both layers' render parity.
	parity_pixels: usize,
	fit_ratio: [Option<f64>; 2],
	luma_ratio: [Option<f64>; 2],
}

fn ratio(num: Option<f64>, den: Option<f64>) -> Option<f64> {
	Some(num? / den?).filter(|r| r.is_finite())
}

fn show(v: Option<f64>) -> String {
	v.map_or_else(|| "-".to_owned(), |v| format!("{v:.4}"))
}

/// Next's own rendering against ours: the render pipeline on Next's planes, our planes as the app shows them, and the
/// brightness fit.
fn render_section(
	report: &mut String,
	out: &Path,
	cs: &CSharp,
	csr: &CsRender,
	run: &RustRun,
	project: &Project,
) -> Result<Vec<StepRow>> {
	let display = &project.display;
	let size = (cs.width, cs.height);
	let file = |b: f64| b * BRIGHTNESS_GAIN;
	let (field, intensity) = (SampleKind::Field, SampleKind::IntensityRoot);
	writeln!(report, "## Rendered frames\n")?;
	writeln!(
		report,
		"Next's pictures are its viewport's rendering of its own planes (no edge glow, no markers): the field as halves \
		 and the intensity as halves of its square root, through the palette and black level of the Display layers at \
		 the file's brightness. Brightness in the project model is the file's divided by {BRIGHTNESS_GAIN}.\n"
	)?;
	writeln!(report, "### Render parity\n")?;
	writeln!(
		report,
		"C#'s own planes rendered by our renderer at the file brightness (field {}, intensity {}) against C#'s pictures: \
		 byte identity expected. Fit: `fit_brightness` on C#'s Half planes against Next's `BrightnessFit`.\n",
		file(display.field.brightness),
		file(display.intensity.brightness)
	)?;
	writeln!(
		report,
		"| step | field pixels differing | field max diff | intensity pixels differing | intensity max diff | field fit C# / ours | \
		 intensity fit C# / ours |"
	)?;
	writeln!(report, "|---|---|---|---|---|---|---|")?;
	let tables = [
		layer_table(project, &display.field, file(display.field.brightness), field),
		layer_table(project, &display.intensity, file(display.intensity.brightness), intensity),
	];
	let mut parity_pixels = Vec::new();
	let mut details = String::new();
	for (i, c) in cs.dumps.iter().enumerate() {
		let samples = [halves(&c.field, field), halves(&c.intensity, intensity)];
		let pictures = [&csr.pictures[i].0, &csr.pictures[i].1];
		let p: Vec<Parity> = (0..2).map(|k| parity(&samples[k], &tables[k], pictures[k])).collect();
		let fits = [fit_brightness(&samples[0], field), fit_brightness(&samples[1], intensity)];
		let (cf, ci) = csr.fits[i];
		writeln!(
			report,
			"| {} | {} | {} | {} | {} | {} / {} | {} / {} |",
			c.step,
			p[0].pixels,
			p[0].max,
			p[1].pixels,
			p[1].max,
			show(cf),
			show(fits[0]),
			show(ci),
			show(fits[1])
		)?;
		for (name, p) in ["field", "intensity"].iter().zip(&p) {
			for (bits, (n, theirs, ours)) in p.samples.iter().take(5) {
				writeln!(
					details,
					"- step {} {name}: {n} pixels of sample {} (half bits {bits:#06x}): C# {theirs:?}, ours {ours:?}",
					c.step,
					f16::from_bits(*bits)
				)?;
			}
		}
		parity_pixels.push(p[0].pixels + p[1].pixels);
	}
	writeln!(report)?;
	if !details.is_empty() {
		writeln!(report, "Differing samples (first five per plane, RGBA):\n\n{details}")?;
	}
	writeln!(report, "### End to end\n")?;
	writeln!(
		report,
		"Our own planes rendered at the model brightness (field {}, intensity {}) as the app shows the loaded file, \
		 against C#'s pictures. Luma: mean Rec. 709 luma, 0 to 1; luma ratio = Rust / C#. Diff: mean absolute channel \
		 difference in levels. Fit ratio = C# fit / Rust fit of our own plane, the amplitude ratio the brightness would need. \
		 Pictures: C# field | Rust field over C# intensity | Rust intensity.\n",
		display.field.brightness, display.intensity.brightness
	)?;
	writeln!(
		report,
		"| step | field luma C# / Rust / ratio | field diff | field fit ratio | intensity luma C# / Rust / ratio | intensity diff | \
		 intensity fit ratio |"
	)?;
	writeln!(report, "|---|---|---|---|---|---|---|")?;
	let tables = [
		layer_table(project, &display.field, display.field.brightness, field),
		layer_table(project, &display.intensity, display.intensity.brightness, intensity),
	];
	let mut rows = Vec::new();
	for (i, (c, r)) in cs.dumps.iter().zip(&run.dumps).enumerate() {
		let pictures = [&csr.pictures[i].0, &csr.pictures[i].1];
		let samples = [halves(&r.field, field), halves(&r.intensity, intensity)];
		let ours = [render_half(&samples[0], &tables[0]), render_half(&samples[1], &tables[1])];
		let luma: Vec<(f64, f64)> = (0..2).map(|k| (mean_luma(pictures[k]), mean_luma(&ours[k]))).collect();
		let diff: Vec<f64> = (0..2).map(|k| mean_channel_diff(pictures[k], &ours[k])).collect();
		let (cf, ci) = csr.fits[i];
		let fit_ratio =
			[ratio(cf, fit_brightness(&samples[0], field)), ratio(ci, fit_brightness(&samples[1], intensity))];
		let luma_ratio = [ratio(Some(luma[0].1), Some(luma[0].0)), ratio(Some(luma[1].1), Some(luma[1].0))];
		writeln!(
			report,
			"| {} | {:.4} / {:.4} / {} | {:.2} | {} | {:.4} / {:.4} / {} | {:.2} | {} |",
			c.step,
			luma[0].0,
			luma[0].1,
			show(luma_ratio[0]),
			diff[0],
			show(fit_ratio[0]),
			luma[1].0,
			luma[1].1,
			show(luma_ratio[1]),
			diff[1],
			show(fit_ratio[1])
		)?;
		rows.push(StepRow { step: c.step, parity_pixels: parity_pixels[i], fit_ratio, luma_ratio });
		let name = format!("render_{}.png", c.step);
		render_picture(&out.join(&name), size, [pictures[0], &ours[0], pictures[1], &ours[1]])?;
	}
	writeln!(report)?;
	for r in &rows {
		writeln!(report, "Step {}: ![step {}](render_{}.png)\n", r.step, r.step, r.step)?;
	}
	Ok(rows)
}

/// The probe graph layouts of Next's scope against ours, on C#'s series and on our own.
fn scope_section(
	report: &mut String,
	csr: &CsRender,
	run: &RustRun,
	probes: &[Probe],
	steps: u32,
	canvas: (u32, u32),
) -> Result<()> {
	ensure!(
		csr.graphs.len() == probes.len(),
		"graph.txt has {} probes, the project {}",
		csr.graphs.len(),
		probes.len()
	);
	if probes.is_empty() {
		return Ok(());
	}
	writeln!(report, "### Scope\n")?;
	writeln!(
		report,
		"The graph box of each probe after the last step (`ProbeGraph.Layout` against `graph::layout`, a {GRAPH_TEXT_SIZE} px \
		 estimated typeface on both sides): its y range and its tick labels with their pixel positions (`label@position`). \
		 The middle row is our layout on C#'s own series and profile, which must equal the C# row.\n"
	)?;
	writeln!(report, "| probe | data | y range | y ticks | x ticks |")?;
	writeln!(report, "|---|---|---|---|---|")?;
	for (p, (probe, g)) in probes.iter().zip(&csr.graphs).enumerate() {
		let theirs = GraphData {
			series: &g.series,
			profile: (!g.profile.is_empty()).then_some(&g.profile[..]),
			profile_length: g.profile_length,
			floor: 0.0,
			steps_sampled: g.steps_sampled,
		};
		let on_theirs = graph_view(probe, &theirs, steps, canvas);
		let on_ours = graph_view(probe, &GraphData::at(&run.traces[p], steps as u64 - 1, 0), steps, canvas);
		let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs());
		let same = close(g.y.0, on_theirs.y.0)
			&& close(g.y.1, on_theirs.y.1)
			&& g.y_ticks == on_theirs.y_ticks
			&& g.x_ticks == on_theirs.x_ticks;
		let row = |data: &str, y: (f64, f64), yt: &str, xt: &str| {
			format!("| {} | {data} | {:.4e} to {:.4e} | {yt} | {xt} |", probe.name, y.0, y.1)
		};
		writeln!(report, "{}", row("C#", g.y, &g.y_ticks, &g.x_ticks))?;
		writeln!(
			report,
			"{}",
			row(
				if same { "ours on C# (equal)" } else { "ours on C# (DIFFERS)" },
				on_theirs.y,
				&on_theirs.y_ticks,
				&on_theirs.x_ticks
			)
		)?;
		writeln!(report, "{}", row("ours on ours", on_ours.y, &on_ours.y_ticks, &on_ours.x_ticks))?;
	}
	writeln!(report)?;
	Ok(())
}

/// The line layouts of summary.tsv, tab separated, `-` for an undefined ratio. Fit ratio = C# fit / Rust fit, luma ratio
/// = Rust / C#, amplitude ratio = Rust / C#.
const SUMMARY_LAYOUT: &str = "`step example step parity_pixels field_fit_ratio intensity_fit_ratio field_luma_ratio \
	intensity_luma_ratio` (one line per dump step, with a `--render` dump) and `probe example name pearson \
	amplitude_ratio` (one line per probe, first Rust run)";

/// summary.tsv, see [`SUMMARY_LAYOUT`].
fn summary(example: &str, steps: &[StepRow], probes: &[ProbeRow]) -> String {
	let mut text = String::new();
	for r in steps {
		let _ = writeln!(
			text,
			"step\t{example}\t{}\t{}\t{}\t{}\t{}\t{}",
			r.step,
			r.parity_pixels,
			show(r.fit_ratio[0]),
			show(r.fit_ratio[1]),
			show(r.luma_ratio[0]),
			show(r.luma_ratio[1])
		);
	}
	for p in probes {
		let _ = writeln!(text, "probe\t{example}\t{}\t{:.4}\t{}", p.name, p.pearson, show(p.ratio));
	}
	text
}

pub(crate) fn run(args: &Args) -> Result<()> {
	let mut project = load(&args.project)?;
	project.simulation.steps = args.steps;
	let cs = read_csharp(&args.csharp, args.steps, args.every)?;
	fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
	let pixels = scene_pixels(&project)?;
	let mut variants = vec![(project.clone(), "as loaded".to_owned(), "")];
	if project.simulation.stencil == Stencil::NinePoint {
		variants[0].1 = format!(
			"as loaded (the frequency scale divided by the original 9-point kernel's phase speed {:.4}, so the \
			 wavelengths match the original's)",
			nine_point_speed()
		);
		let mut raw = project.clone();
		raw.simulation.source_frequency_scale *= nine_point_speed();
		variants.push((
			raw,
			"unscaled (the original's frequency scale, so the waves are 1/0.562 longer)".to_owned(),
			"_unscaled",
		));
	}
	let runs = variants
		.iter()
		.map(|(p, label, tag)| run_rust(p, &pixels, args.steps, args.every, (label, tag), args.engine))
		.collect::<Result<Vec<_>>>()?;
	// the Rust dumps beside the report, in the C# dump's layout, for measurements the report does not make
	for run in &runs {
		for d in &run.dumps {
			write_f32(&args.out.join(format!("rust{}_field_{}.f32", run.tag, d.step)), &d.field)?;
			write_f32(&args.out.join(format!("rust{}_intensity_{}.f32", run.tag, d.step)), &d.intensity)?;
		}
	}
	let spec = RunSpec::create(&project, Some(&pixels), PreviewQuality::FULL, true)?;
	let sources: Vec<(String, f64, f64)> =
		project.sources.iter().filter(|s| s.enabled).take(MAX_FRONTS).map(|s| (s.name.clone(), s.x, s.y)).collect();

	let mut report = String::new();
	let name = args.project.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
	writeln!(report, "# {name}: C# WaveDudeNext against the Rust engine\n")?;
	writeln!(
		report,
		"Both CPU engines, {} steps, fields dumped every {} steps. Generated by `wavedude-cli compare`.\n",
		args.steps, args.every
	)?;
	writeln!(report, "| | C# | Rust |\n|---|---|---|")?;
	let fact = |k: &str| cs.facts.get(k).cloned().unwrap_or_else(|| "-".into());
	let scene = &spec.scene;
	writeln!(report, "| canvas | {}x{} | {}x{} |", cs.width, cs.height, scene.canvas_width, scene.canvas_height)?;
	writeln!(report, "| rim | {} | {} |", fact("rim"), scene.rim)?;
	writeln!(report, "| edges | {} | {:?} |", fact("edges"), scene.edges)?;
	writeln!(report, "| stencil | {} | {:?} |", fact("stencil"), spec.options.stencil)?;
	writeln!(report, "| sub-steps | {} | {} |", fact("substeps"), runs[0].sub_steps)?;
	writeln!(report, "| emitters | {} | {} |", fact("emitters"), spec.emitters.len())?;
	writeln!(report)?;
	scene_section(&mut report, &args.out, &args.csharp, &cs, &pixels)?;
	writeln!(report, "## Field\n")?;
	writeln!(
		report,
		"Pearson and RMS difference over the canvas; PSNR against the C# field's peak |u|; energy the ratio of the \
		 sums of u squared; front the farthest pixel right of a source on its row above 1 % of that stretch's peak |u| \
		 (\"edge\" once it has left the canvas). Gain: the least-squares g of Rust ~ g C#; the RMS difference after gain compares \
		 Rust / g with C#, the shape alone. Pictures: C# | Rust | Rust / g - C#, grey at the C# field's 99.5th percentile \
		 of |u|.\n"
	)?;
	for run in &runs {
		field_section(&mut report, &args.out, &cs, run, &sources)?;
	}
	let probe_rows = probe_section(&mut report, &cs, &runs, args.steps)?;
	let mut step_rows = Vec::new();
	if let Some(csr) = &cs.render {
		step_rows = render_section(&mut report, &args.out, &cs, csr, &runs[0], &project)?;
		scope_section(&mut report, csr, &runs[0], &spec.probes, args.steps, (cs.width as u32, cs.height as u32))?;
	}
	writeln!(report, "## summary.tsv\n\nAlways written beside this report: tab separated lines {SUMMARY_LAYOUT}.")?;
	fs::write(args.out.join("summary.tsv"), summary(&name, &step_rows, &probe_rows))?;
	fs::write(args.out.join("report.md"), report)?;
	println!("{}", args.out.join("report.md").display());
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn pearson_of_a_series_with_itself_and_its_negative() {
		let a = [1.0, 2.0, 4.0, 3.0];
		assert!((pearson(a.iter().copied(), a.iter().copied()) - 1.0).abs() < 1e-12);
		assert!((pearson(a.iter().copied(), a.iter().map(|v| -2.0 * v)) + 1.0).abs() < 1e-12);
		assert_eq!(pearson(a.iter().copied(), [5.0; 4].iter().copied()), 0.0);
	}

	#[test]
	fn the_front_is_the_farthest_pixel_right_of_the_source_above_the_threshold() {
		let mut u = vec![0f32; 10 * 10];
		u[5 * 10 + 5] = 1.0;
		u[5 * 10 + 8] = 0.5;
		u[5 * 10 + 9] = 0.001;
		assert_eq!(front(&u, 10, 5, 5), Some(3));
		u[5 * 10 + 9] = 0.1;
		assert_eq!(front(&u, 10, 5, 5), None);
	}

	#[test]
	fn the_wavelength_of_a_sampled_sine() {
		let w = 200;
		let u: Vec<f32> = (0..w).map(|x| (2.0 * std::f32::consts::PI * (x as f32 + 0.3) / 16.0).sin()).collect();
		let l = wavelength(&u, w, 0, 0).unwrap();
		assert!((l - 16.0).abs() < 0.05, "{l}");
	}

	#[test]
	fn identical_fields_have_no_difference() {
		let u = [0.5f32, -1.0, 0.25, 0.0];
		let s = field_stats(&u, &u);
		assert!((s.pearson - 1.0).abs() < 1e-12 && s.nrms == 0.0 && s.psnr > 100.0, "{s:?}");
	}

	#[test]
	fn luma_and_channel_difference_of_rgba_pictures() {
		let white = [255u8; 8];
		assert!((mean_luma(&white) - 1.0).abs() < 1e-12);
		assert_eq!(mean_luma(&[0, 0, 0, 255]), 0.0);
		assert_eq!(mean_channel_diff(&[10, 20, 30, 255], &[13, 20, 24, 255]), 3.0);
	}

	#[test]
	fn the_summary_has_a_line_per_step_and_probe() {
		let steps =
			[StepRow { step: 100, parity_pixels: 0, fit_ratio: [Some(2.0), None], luma_ratio: [Some(0.5), Some(1.0)] }];
		let probes = [ProbeRow { name: "P".into(), pearson: 0.99, ratio: None }];
		assert_eq!(
			summary("Ex", &steps, &probes),
			"step\tEx\t100\t0\t2.0000\t-\t0.5000\t1.0000\nprobe\tEx\tP\t0.9900\t-\n"
		);
	}

	#[test]
	fn parity_names_the_differing_sample() {
		let kind = SampleKind::Field;
		let table = HalfTable::new(&wavedude_core::model::ColorScheme::grey(), 0.0, 1.0, kind);
		let plane = [0.0f32, 0.5];
		let samples = halves(&plane, kind);
		let mut picture = render_half(&samples, &table);
		assert_eq!(parity(&samples, &table, &picture).pixels, 0);
		picture[4] = picture[4].wrapping_add(3);
		let p = parity(&samples, &table, &picture);
		assert_eq!((p.pixels, p.max), (1, 3));
		assert_eq!(p.samples.keys().copied().collect::<Vec<_>>(), [f16::from_f32(0.5).to_bits()]);
	}

	#[test]
	fn shrink_averages_whole_boxes() {
		let (w, h, out) = shrink(&[0, 200, 0, 200], 2, 2, 1);
		assert_eq!((w, h, out), (1, 1, vec![100]));
	}
}
