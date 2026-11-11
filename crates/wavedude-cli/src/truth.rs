//! The truth table of a two-input gate project: four runs with the sources A and B switched off
//! and on, the steady power in the arms at the line probes Input (A, B) and Output (Q, R), against
//! the design's expectation.

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use clap::Args as ClapArgs;
use wavedude_core::model::{GateKind, Measurement, PartKind, Probe, ProbeKind, Project, Source};
use wavedude_core::run::PreviewQuality;

use crate::project::load;
use crate::run::{EngineArgs, finish, simulate};

/// Steps averaged at the end of a run: eight periods at level 50 (32 steps a period).
pub(crate) const DEFAULT_SETTLE: u32 = 256;
/// Largest deviation of a normalised output (in units of one input's power) for a row to pass.
///
/// The design rounds every length to whole pixels (a coupler half a pixel off moves the split by
/// about kappa / 2, under 0.01), the point sources launch radiation besides the mode, part of which
/// still crosses the probes, the bends' coupling is integrated from the straight-guide solver, and
/// a half line takes the other arm's evanescent tail beyond the axis. Together these stay well
/// under a tenth of an input, while a wrong phase convention or coupling length errs by 0.5 or more.
pub(crate) const TOLERANCE: f64 = 0.1;
/// Factor over the slowest phase speed for the wave front and the mode's group delay.
const CROSSING_MARGIN: f64 = 1.25;
/// EMA time constants for the intensity to settle.
const EMA_SETTLE: f64 = 5.0;
const COUPLER_TAG: &str = "Coupler:";

#[derive(ClapArgs)]
pub(crate) struct Args {
	/// Project file with sources named A and B (each with an optional `<name> lower` partner) and line probes named Input and Output, A's and Q's arm in the upper half.
	project: PathBuf,
	/// Steps of each run (default: derived from the canvas width, the core index and the intensity EMA).
	#[arg(long)]
	steps: Option<u32>,
	/// Steps averaged at the end of each run.
	#[arg(long, default_value_t = DEFAULT_SETTLE, value_parser = clap::value_parser!(u32).range(1..))]
	settle: u32,
	#[command(flatten)]
	engine: EngineArgs,
}

/// Steps for a wave to cross `width` cells at the speed `1/index` (with a margin for the group
/// delay), the intensity EMA of time constant `time_decay * 100 / level` to settle, and `settle` steps.
pub(crate) fn run_steps(width: u32, index: f64, time_decay: f64, level: f64, settle: u32) -> u32 {
	let tau = (time_decay * 100.0 / level).max(1.0);
	(CROSSING_MARGIN * index * width as f64 + EMA_SETTLE * tau).ceil() as u32 + settle
}

/// The description line that tells [`run`] a fibre coupler's total coupling phase.
pub(crate) fn coupler_tag(theta: f64) -> String {
	format!("{COUPLER_TAG} {theta:.4} rad")
}

fn coupler_phase(description: &str) -> Option<f64> {
	let rest = &description[description.find(COUPLER_TAG)? + COUPLER_TAG.len()..];
	rest.split_whitespace().next()?.parse().ok()
}

/// Expected Q and R, relative to one input's power, for (A, B) = 00, 10, 01, 11.
#[derive(Debug)]
struct Expectation {
	label: String,
	q: [f64; 4],
	r: [f64; 4],
}

impl Expectation {
	/// Spec 21's table: Q is a gate's upper output, R its lower one.
	fn gate(kind: GateKind) -> Self {
		const XOR: [f64; 4] = [0.0, 0.5, 0.5, 0.0];
		const AND: [f64; 4] = [0.0, 0.5, 0.5, 2.0];
		const OR: [f64; 4] = [0.0, 0.5, 0.5, 0.5];
		const REMAINDER: [f64; 4] = [0.0, 0.5, 0.5, 1.5];
		let (q, r) = match kind {
			GateKind::Xor => (XOR, AND),
			GateKind::And => (AND, XOR),
			GateKind::Or => (OR, REMAINDER),
		};
		Self { label: format!("{kind:?} gate part (docs/spec/21)"), q, r }
	}

	/// A lossless directional coupler of total coupling phase `theta`, B's phase `phi` (radians)
	/// relative to A's: the through arm keeps `cos theta a - i sin theta b`.
	fn coupler(theta: f64, phi: f64) -> Self {
		let (c2, s2) = (theta.cos().powi(2), theta.sin().powi(2));
		let mix = (2.0 * theta).sin() * phi.sin();
		Self {
			label: format!("coupler of {theta:.4} rad, B at {:.1} degrees to A", phi.to_degrees()),
			q: [0.0, c2, s2, 1.0 + mix],
			r: [0.0, s2, c2, 1.0 - mix],
		}
	}

	fn of(project: &Project, a: usize, b: usize) -> Option<Self> {
		let gate = project.parts.iter().filter(|p| p.base.enabled).find_map(|p| match &p.kind {
			PartKind::LogicGate(g) => Some(g.kind),
			_ => None,
		});
		if let Some(kind) = gate {
			return Some(Self::gate(kind));
		}
		let theta = coupler_phase(&project.description)?;
		let phi = (project.sources[b].phase - project.sources[a].phase).to_radians();
		Some(Self::coupler(theta, phi))
	}
}

fn named<T>(items: &[T], name: &str, of: impl Fn(&T) -> &str, what: &str) -> Result<usize> {
	items.iter().position(|i| of(i) == name).with_context(|| format!("the project has no {what} named {name}"))
}

/// The sources of the input `name`: the one named so and its `<name> lower` partner, which drives the cell
/// below it.
fn input_sources(sources: &[Source], name: &str) -> Result<Vec<usize>> {
	let partner = format!("{name} lower");
	let found: Vec<usize> =
		sources.iter().enumerate().filter(|(_, s)| s.name == name || s.name == partner).map(|(i, _)| i).collect();
	ensure!(!found.is_empty(), "the project has no source named {name}");
	Ok(found)
}

/// Highest index a wave travels in: the background, a fibre's core or a gate's core.
fn slowest_index(project: &Project) -> f64 {
	project
		.parts
		.iter()
		.filter(|p| p.base.enabled)
		.filter_map(|p| match &p.kind {
			PartKind::Fiber(f) => Some(f.core_index),
			PartKind::LogicGate(g) => Some(g.core_index),
			_ => None,
		})
		.fold(project.scene.refractive_index, f64::max)
}

/// Line probes read by [`measure`]; the upper half of a line is arm A (Q), the lower half arm B (R).
const LINES: [&str; 2] = ["Input", "Output"];
/// Name suffix of the copy of a line that sums the second half of the averaged steps only.
const LATE: &str = " (late)";

/// Sums of the upper and the lower half of `profile`, a middle sample of an odd length left out.
fn halves(profile: &[f32]) -> [f64; 2] {
	let half = profile.len() / 2;
	let sum = |s: &[f32]| s.iter().map(|&v| v as f64).sum::<f64>();
	[sum(&profile[..half]), sum(&profile[profile.len() - half..])]
}

/// Mean power over the last `settle` steps of A and B at Input and of Q and R at Output, each the
/// intensity summed over its half of the line, and the larger change of Q or R between the halves of
/// those steps relative to Q + R.
///
/// Each line becomes an integrated probe summing from `settle` steps before the end, and a copy of it
/// from half as many; the only other timeline frame is the last step's, so the run stores two frames.
fn measure(project: &Project, steps: u32, settle: u32, engine: EngineArgs) -> Result<([f64; 4], f64)> {
	let mut project = project.clone();
	project.simulation.visualize_every = steps - 1;
	let late = settle / 2;
	let mut copies = Vec::new();
	for probe in &mut project.probes {
		if !LINES.contains(&probe.name.as_str()) {
			probe.active = false;
			continue;
		}
		ensure!(probe.kind == ProbeKind::Line, "probe {} is not a line probe", probe.name);
		probe.active = true;
		probe.measurement = Measurement::IntegratedValue;
		probe.start_after = steps - settle + 1;
		copies.push(Probe { name: format!("{}{LATE}", probe.name), start_after: steps - late + 1, ..probe.clone() });
	}
	project.probes.extend(copies);
	let (mut run, _store) = simulate(&mut project, Some(steps), PreviewQuality::DEFAULT, false, engine)?;
	finish(&mut run)?;
	let traces = run.traces();
	let probes = &run.spec().probes;
	let sums = |name: &str| -> Result<[f64; 2]> {
		let trace = &traces[named(probes, name, |p| &p.name, "line probe")?];
		ensure!(trace.steps_sampled == steps as u64, "probe {name} recorded up to step {}", trace.steps_sampled);
		Ok(halves(trace.profiles.last().with_context(|| format!("probe {name} recorded no profile"))?))
	};
	let [a, b] = sums(LINES[0])?;
	let [q, r] = sums(LINES[1])?;
	let mean = |total: f64, n: u32| total / f64::from(n);
	let means = [mean(a, settle), mean(b, settle), mean(q, settle), mean(r, settle)];
	let mut drift: f64 = 0.0;
	if late > 0 {
		let [q2, r2] = sums(&format!("{}{LATE}", LINES[1]))?;
		for (all, second) in [(q, q2), (r, r2)] {
			drift = drift.max((mean(second, late) - mean(all - second, settle - late)).abs());
		}
	}
	let total = means[2] + means[3];
	Ok((means, if total > 0.0 { drift / total } else { 0.0 }))
}

pub(crate) fn run(args: &Args) -> Result<()> {
	let project = load(&args.project)?;
	let a_all = input_sources(&project.sources, "A")?;
	let b_all = input_sources(&project.sources, "B")?;
	let (a, b) = (a_all[0], b_all[0]);
	let index = slowest_index(&project);
	let level = project.sources[a].frequency.min(project.sources[b].frequency);
	let sim = &project.simulation;
	let derived = run_steps(project.scene.width, index, sim.time_decay, level, args.settle);
	let steps = args.steps.unwrap_or(derived);
	ensure!(args.settle < steps, "--settle {} must be shorter than the {steps} steps", args.settle);
	println!(
		"{steps} steps a run ({}): {CROSSING_MARGIN} x index {index} x {} px to cross, {EMA_SETTLE} intensity time \
		 constants of {:.1} steps, {} steps averaged; engine {}",
		if args.steps.is_some() { "given" } else { "derived" },
		project.scene.width,
		(sim.time_decay * 100.0 / level).max(1.0),
		args.settle,
		if args.engine.gpu { "GPU" } else { "CPU" }
	);
	let rows = [(false, false), (true, false), (false, true), (true, true)];
	let mut measured = Vec::with_capacity(rows.len());
	for &(on_a, on_b) in &rows {
		let mut p = project.clone();
		for (all, on) in [(&a_all, on_a), (&b_all, on_b)] {
			all.iter().for_each(|&i| p.sources[i].enabled = on);
		}
		measured.push(measure(&p, steps, args.settle, args.engine)?);
	}
	let total = measured[1].0[2] + measured[1].0[3];
	ensure!(total > 0.0, "A alone reaches neither Q nor R");
	let input = measured[1].0[0] + measured[1].0[1];
	ensure!(input > 0.0, "A alone reaches neither half of the Input line");
	let expected = Expectation::of(&project, a, b);
	match &expected {
		Some(e) => println!("expected: {}; tolerance {TOLERANCE}", e.label),
		None => println!("expected: unknown (no gate part, no coupler line in the description)"),
	}
	println!(
		"in A and in B normalised to their sum with A alone ({input:.6e}), Q and R to theirs ({total:.6e}); \
		 each the intensity summed over its half of the line"
	);
	println!();
	println!("| A | B | in A | in B | Q | R | Q expected | R expected | verdict |");
	println!("|---|---|---|---|---|---|---|---|---|");
	let mut passed = 0;
	for (row, (&(on_a, on_b), ([in_a, in_b, q, r], _))) in rows.iter().zip(&measured).enumerate() {
		let (in_a, in_b, q, r) = (in_a / input, in_b / input, q / total, r / total);
		let (eq, er, verdict) = match &expected {
			Some(e) => {
				let ok = (q - e.q[row]).abs() <= TOLERANCE && (r - e.r[row]).abs() <= TOLERANCE;
				passed += ok as usize;
				(format!("{:.3}", e.q[row]), format!("{:.3}", e.r[row]), if ok { "ok" } else { "off" })
			}
			None => ("-".into(), "-".into(), "-"),
		};
		println!(
			"| {} | {} | {in_a:.3} | {in_b:.3} | {q:.3} | {r:.3} | {eq} | {er} | {verdict} |",
			on_a as u8, on_b as u8
		);
	}
	let drift = measured.iter().map(|m| m.1).fold(0.0, f64::max);
	println!();
	println!("largest change between the halves of the averaged steps: {:.1} % of Q + R", 100.0 * drift);
	if expected.is_some() {
		println!("{passed} of {} rows within the tolerance", rows.len());
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_coupler_tag_reads_back() {
		let text = format!("A coupler.\n{} of coupling phase", coupler_tag(1.23456));
		assert_eq!(coupler_phase(&text), Some(1.2346));
		assert_eq!(coupler_phase("no tag"), None);
	}

	#[test]
	fn a_3db_coupler_with_b_90_degrees_behind_is_an_xor() {
		let e = Expectation::coupler(std::f64::consts::FRAC_PI_4, (-90f64).to_radians());
		for (got, want) in e.q.iter().zip([0.0, 0.5, 0.5, 0.0]) {
			assert!((got - want).abs() < 1e-12, "{e:?}");
		}
		assert!((e.r[3] - 2.0).abs() < 1e-12);
	}
}
