//! The photonic gate demonstrations of the Huygens Optics video "Creating Logic using Waves:
//! Photonic Gates" as projects, laid out by this project's mode solver.
//!
//! Canvas coordinates throughout: x to the right, y down, a fibre node or part centre at the
//! top-left corner of its pixel. Arm A (input A, output Q) is the upper one (smaller y), arm B
//! (input B, output R) the lower one. Every pixel feature of arm B is the mirror of arm A's about
//! the axis between them: row r of A mirrors to row `y_a + y_b - 1 - r`.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use clap::Args as ClapArgs;
use wavedude_core::engine::substeps::sub_steps;
use wavedude_core::io::{save_project, wdproj};
use wavedude_core::model::normalize::Normalize;
use wavedude_core::model::{
	BoxSize, Edges, Fiber, FiberNode, GateKind, Layer, LogicGate, Material, Measurement, Outline, Part, PartBase,
	PartKind, Point, Probe, ProbeKind, Project, Rectangle, Scene, SceneMode, Simulation, Source, wavelength,
};
use wavedude_core::quality::PreviewQuality;
use wavedude_core::render::fit_brightness;
use wavedude_core::scene::compose::compose_scene;
use wavedude_core::scene::gate_design::{BEND_EXP, GateDesign, ports_of};
use wavedude_core::scene::modes::ModeSolver;
use wavedude_core::spec::RunSpec;

use crate::run::EngineArgs;
use crate::truth::{coupler_tag, run_steps};

/// Simpson intervals over an S-bend.
const BEND_SAMPLES: usize = 24;
/// Graph box of every probe.
const BOX: BoxSize = BoxSize::Tiny;

fn box_px() -> (u32, u32) {
	BOX.nominal().expect("a named box size")
}
/// Space between a graph box and what it sits beside.
const BOX_GAP: i32 = 12;
/// Absorbing rim outside the canvas.
const RIM: i32 = -32;
/// Steps per timeline frame of the generated projects.
const VISUALIZE_EVERY: u32 = 4;

#[derive(ClapArgs)]
pub(crate) struct Args {
	/// Directory for the projects (created if missing).
	#[arg(long)]
	out: PathBuf,
	/// Source and design frequency level.
	#[arg(long, default_value_t = 50.0)]
	frequency: f64,
	/// Core index of the fibres and gates.
	#[arg(long, default_value_t = 1.8)]
	core: f64,
	/// Index of the medium around the fibres, the gates' cladding.
	#[arg(long, default_value_t = 1.4)]
	cladding: f64,
	/// Canvas width of the coupled-fibre projects; their straight run is one coupling length, which sets the gap.
	#[arg(long, default_value_t = 1200)]
	width: u32,
	/// Canvas height of the fibre projects (default: the design's).
	#[arg(long)]
	height: Option<u32>,
	/// Engine of the brightness fits.
	#[command(flatten)]
	engine: EngineArgs,
}

/// A single-mode slab core as the mode solver sees it, and the coupling of two of them.
#[derive(Debug)]
struct Guide {
	solver: ModeSolver,
	/// Leapfrog frequency term of the solver.
	om: f64,
	core_index: f64,
	clad_index: f64,
	/// Core width.
	core: i64,
	/// Cladding band the solver sees around each core.
	band: i64,
}

impl Guide {
	fn spacing(&self, gap: i64) -> i32 {
		(self.core + gap) as i32
	}

	fn kappa_at(&self, separation: f64) -> f64 {
		self.solver.kappa_at(separation, self.om, self.core_index, self.clad_index, self.core, self.band)
	}

	fn kappa(&self, gap: i64) -> f64 {
		self.kappa_at((self.core + gap) as f64)
	}

	fn coupling_length(&self, gap: i64) -> Option<f64> {
		let separation = (self.core + gap) as f64;
		self.solver.coupling_length(separation, self.om, self.core_index, self.clad_index, self.core, self.band)
	}

	/// Coupling phase of a cubic S-bend of `len` cells between centre separations `near` and `far`:
	/// Simpson's rule over the smoothstep `3t^2 - 2t^3` the Bezier with handles of `len / 3` draws.
	fn bend_phase(&self, near: f64, far: f64, len: i32) -> f64 {
		let sum: f64 = (0..=BEND_SAMPLES)
			.map(|i| {
				let t = i as f64 / BEND_SAMPLES as f64;
				let weight = match i {
					0 | BEND_SAMPLES => 1.0,
					i if i % 2 == 1 => 4.0,
					_ => 2.0,
				};
				weight * self.kappa_at(far - (far - near) * (3.0 * t * t - 2.0 * t * t * t))
			})
			.sum();
		sum * len as f64 / (3.0 * BEND_SAMPLES as f64)
	}
}

/// The two-fibre layout shared by the demonstrations, from the mode solver.
#[derive(Debug)]
struct FibreDesign {
	frequency: f64,
	lambda: f64,
	guide: Guide,
	beta: f64,
	gamma: f64,
	/// Straight lead from the cup to a bend, and from a bend to the canvas edge.
	lead: i32,
	/// Edge-to-edge gap of the coupled pair: one coupling length fits the full-transfer run.
	gap: i64,
	coupling_length: f64,
	/// Edge-to-edge gap where the bent pair runs apart.
	far_gap: i64,
	bend_len: i32,
	/// Coupling phase of one S-bend.
	bend_phase: f64,
	/// Rows of the pair at the far gap; every other spacing is centred between them.
	top: i32,
	/// Probe lines reach this far past the outer rows: the core's half and four decay lengths.
	reach: i32,
	height: u32,
}

impl FibreDesign {
	fn new(args: &Args) -> Result<Self> {
		let (n_c, n_cl) = (args.core, args.cladding);
		ensure!(n_c > n_cl, "the core index {n_c} must exceed the cladding index {n_cl}");
		let sim = Simulation::default();
		let lambda = wavelength(args.frequency, 1.0, sim.source_frequency_scale);
		ensure!(lambda.is_finite() && lambda >= 6.0, "frequency level {} gives no usable wavelength", args.frequency);
		let solver = ModeSolver::new(sim.stencil, sub_steps(sim.stencil, (1.0 / (n_cl * n_cl)) as f32));
		let om = solver.omega_term(lambda);
		let na = (n_c * n_c - n_cl * n_cl).sqrt();
		let core = ((0.9 * lambda / (2.0 * na)).floor() as i64).max(2);
		let guess = solver.single(lambda, om, n_c, n_cl, core, 0).context("the fibre guides no mode")?;
		let band = (8.0 / guess.cladding_decay(om, n_cl)).ceil() as i64;
		let mode = solver.single(lambda, om, n_c, n_cl, core, band).context("the fibre guides no mode")?;
		let gamma = mode.cladding_decay(om, n_cl);
		let guide = Guide { solver, om, core_index: n_c, clad_index: n_cl, core, band };
		let lead = lambda.round() as i32;
		let run = args.width as f64 - (Cup::new(core as i32).face + lead) as f64;
		let (gap, coupling_length) = (1..=4 * band)
			.map_while(|g| guide.coupling_length(g).filter(|&l| l <= run).map(|l| (g, l)))
			.last()
			.with_context(|| format!("no gap gives a coupling length within the {run} px run; widen the canvas"))?;
		let shift = (4.0 / gamma).ceil() as i64;
		let far_gap = gap + 2 * shift;
		let radius = BEND_EXP * 1.5 * mode.beta * mode.beta / gamma.powi(3);
		// The cubic's peak curvature 6 shift / len^2 equals 1 / radius; len a multiple of 3 for whole handles.
		let bend_len = 3 * ((6.0 * shift as f64 * radius).sqrt() / 3.0).ceil() as i32;
		let bend_phase = guide.bend_phase(guide.spacing(gap) as f64, guide.spacing(far_gap) as f64, bend_len);
		let spread = guide.spacing(far_gap);
		let reach = (core / 2) as i32 + (4.0 / gamma).ceil() as i32;
		let top = reach.max(Cup::new(core as i32).wall + core as i32) + BOX_GAP;
		let (_, box_h) = box_px();
		let fit = ((top + spread + reach + 2 * BOX_GAP + box_h as i32) as u32).div_ceil(8) * 8;
		let height = match args.height {
			Some(h) if h < fit => bail!("a height of {h} px cannot hold the pair, its probes and graphs ({fit} px)"),
			Some(h) => h,
			None => fit,
		};
		Ok(Self {
			frequency: args.frequency,
			lambda,
			guide,
			beta: mode.beta,
			gamma,
			lead,
			gap,
			coupling_length,
			far_gap,
			bend_len,
			bend_phase,
			top,
			reach,
			height,
		})
	}

	/// The bent pair's layout for a total coupling phase `theta`: straight coupler length and canvas width.
	fn bent(&self, theta: f64) -> Result<BentPair> {
		let tail = self.lead;
		let far_len = (self.lead + tail) as f64;
		let rest = 2.0 * self.bend_phase + self.guide.kappa(self.far_gap) * far_len;
		let straight = ((theta - rest) / self.guide.kappa(self.gap)).round();
		ensure!(straight >= 0.0, "the bends alone couple {rest:.3} rad, more than {theta:.3}");
		let straight = straight as i32;
		let bend_in = self.cup().face + self.lead;
		let coupler = bend_in + self.bend_len;
		let bend_out = coupler + straight;
		let apart = bend_out + self.bend_len;
		let probe = apart + tail / 2;
		let width = (apart + tail) as u32;
		let phase = rest + straight as f64 * self.guide.kappa(self.gap);
		Ok(BentPair { straight, bend_in, coupler, bend_out, apart, probe, width, phase })
	}

	fn rows(&self, gap: i64) -> (i32, i32) {
		let spacing = self.guide.spacing(gap);
		let upper = self.top + (self.guide.spacing(self.far_gap) - spacing) / 2;
		(upper, upper + spacing)
	}

	fn cup(&self) -> Cup {
		Cup::new(self.guide.core as i32)
	}

	/// Graph boxes' top edge: below the far pair's probe lines.
	fn boxes_y(&self) -> i32 {
		self.rows(self.far_gap).1 + self.reach + BOX_GAP
	}

	fn simulation(&self, width: u32) -> Simulation {
		let sim = Simulation { visualize_every: VISUALIZE_EVERY, ..Simulation::default() };
		let steps =
			run_steps(width, self.guide.core_index, sim.time_decay, self.frequency, crate::truth::DEFAULT_SETTLE);
		Simulation { steps: steps.div_ceil(VISUALIZE_EVERY) * VISUALIZE_EVERY, ..sim }
	}

	fn summary(&self) -> String {
		format!(
			"Wavelength {:.2} px in vacuum, {:.2} px in the core (index {}) and {:.2} px in the medium (index {}); \
			 single-mode core {} px wide, beta {:.4} rad/px, evanescent decay length {:.2} px.",
			self.lambda,
			self.lambda / self.guide.core_index,
			self.guide.core_index,
			self.lambda / self.guide.clad_index,
			self.guide.clad_index,
			self.guide.core,
			self.beta,
			1.0 / self.gamma
		)
	}
}

struct BentPair {
	straight: i32,
	bend_in: i32,
	coupler: i32,
	bend_out: i32,
	apart: i32,
	/// The Output line.
	probe: i32,
	width: u32,
	/// Total coupling phase of the layout.
	phase: f64,
}

fn fibre(name: &str, design: &FibreDesign, nodes: Vec<FiberNode>) -> Part {
	let kind = Fiber {
		core_index: design.guide.core_index,
		core_diameter: design.guide.core as u32,
		cladding_index: design.guide.clad_index,
		cladding_thickness: 0,
		nodes,
		..Fiber::default()
	};
	Part { base: PartBase { name: name.into(), ..PartBase::default() }, kind: PartKind::Fiber(kind) }
}

fn node(x: i32, y: i32, in_x: i32, out_x: i32) -> FiberNode {
	FiberNode { x, y, in_x, in_y: 0, out_x, out_y: 0, smooth: true }
}

fn straight_fibre(name: &str, design: &FibreDesign, y: i32, width: u32) -> Part {
	fibre(name, design, vec![node(design.cup().back, y, 0, 0), node(width as i32 + 8, y, 0, 0)])
}

/// A fibre at `far` that bends to `near` over [`bend_in`, coupler], runs straight to `bend_out` and
/// bends back to `far` by `apart`.
fn bent_fibre(name: &str, design: &FibreDesign, pair: &BentPair, far: i32, near: i32) -> Part {
	let h = design.bend_len / 3;
	fibre(
		name,
		design,
		vec![
			node(design.cup().back, far, 0, 0),
			node(pair.bend_in, far, 0, h),
			node(pair.coupler, near, -h, 0),
			node(pair.bend_out, near, 0, h),
			node(pair.apart, far, -h, 0),
			node(pair.width as i32 + 8, far, 0, 0),
		],
	)
}

/// A mirror (red 0) over the canvas's left edge up to x = `face`, beyond the canvas on three sides.
fn mirror_wall(face: i32, height: u32) -> Part {
	let h = height as i32 + 4;
	let points = vec![Point::new(-4, -4), Point::new(face, -4), Point::new(face, h), Point::new(-4, h)];
	let kind = Outline { material: mirror(), points, ..Outline::default() };
	Part { base: PartBase { name: "Mirror".into(), ..PartBase::default() }, kind: PartKind::Outline(kind) }
}

fn mirror() -> Material {
	Material { mirror: true, ..Material::default() }
}

fn medium(index: f64, absorption: u32, ramp: u32) -> Material {
	Material { refractive_index: index, absorption, absorption_ramp: ramp.max(1), ..Material::default() }
}

/// A rectangle over [x0, x1) x [y0, y1), an odd side widened by a pixel so its edges lie on pixel boundaries.
fn block(name: &str, material: Material, (x0, y0): (i32, i32), (x1, y1): (i32, i32)) -> Part {
	let (length, width) = ((x1 - x0 + 1) & !1, (y1 - y0 + 1) & !1);
	let base = PartBase { name: name.into(), x: x0 + length / 2, y: y0 + width / 2, ..PartBase::default() };
	let kind = Rectangle { material, length: length as u32, width: width as u32, ..Rectangle::default() };
	Part { base, kind: PartKind::Rectangle(kind) }
}

/// The mirror block around the fibres' left ends, in core widths: two deep, the fibres entering a
/// quarter of it from its back and the sources at its middle, a wall of one and a half around the slots.
struct Cup {
	core: i32,
	left: i32,
	/// Where the fibres start.
	back: i32,
	face: i32,
	source: i32,
	wall: i32,
}

impl Cup {
	fn new(core: i32) -> Self {
		let left = core;
		Self { core, left, back: left + core / 2, face: left + 2 * core, source: left + core, wall: core + core / 2 }
	}

	/// The block around one fibre at row `y`, which the fibre painted after it passes through.
	fn single(&self, y: i32) -> Part {
		let half = self.core / 2 + self.wall;
		block("Mirror", mirror(), (self.left, y - half), (self.face, y + half))
	}

	/// An E: the block around fibres at rows `upper` and `lower`, then two slots of `index` a little
	/// wider than the cores; the fibres go after these.
	fn pair(&self, upper: i32, lower: i32, index: f64) -> Vec<Part> {
		let half = self.core / 2;
		let between = lower - upper - self.core;
		let margin = (self.core / 4).min((between - self.core / 4) / 2).max(0);
		let reach = half + margin;
		let slot = |name: &str, y: i32| {
			block(name, medium(index, 0, 1), (self.back, y - reach), (self.face, y - half + self.core + margin))
		};
		vec![
			block("Mirror", mirror(), (self.left, upper - reach - self.wall), (self.face, lower + reach + self.wall)),
			slot("Slot A", upper),
			slot("Slot B", lower),
		]
	}
}

/// An input at cell column `x` on the axis at canvas row `axis`, which lies between two cell rows: two sources
/// at half amplitude, one in each, so a core of even width is excited symmetrically.
fn source(name: &str, frequency: f64, x: i32, axis: i32, phase: f64) -> Vec<Source> {
	[(name.to_owned(), axis - 1), (format!("{name} lower"), axis)]
		.into_iter()
		.map(|(name, y)| Source {
			name,
			x: f64::from(x),
			y: f64::from(y),
			frequency,
			phase,
			amplitude: 50.0,
			..Source::default()
		})
		.collect()
}

/// An intensity probe; a point probe when `b` is None.
fn probe(name: &str, a: (i32, i32), b: Option<(i32, i32)>, at: (i32, i32)) -> Probe {
	let (ax, ay) = a;
	let (bx, by) = b.unwrap_or(a);
	Probe {
		name: name.into(),
		layer: Layer::Intensity,
		measurement: Measurement::Value,
		kind: if b.is_some() { ProbeKind::Line } else { ProbeKind::Point },
		ax,
		ay,
		bx,
		by,
		box_size: BOX,
		box_x: at.0,
		box_y: at.1,
		..Probe::default()
	}
}

/// The line probes Input at `x_in` and Output at `x_out` across arms whose upper row is `upper` and
/// whose mirror image of row r is `upper + lower - 1 - r`, each line its own mirror image, so its
/// upper half reads arm A (Q) and its lower half arm B (R). Graphs at `boxes_y`, Input's on the left
/// under its line, Output's on the right ending under its line.
fn line_probes(x_in: i32, x_out: i32, (upper, lower): (i32, i32), reach: i32, boxes_y: i32, width: u32) -> Vec<Probe> {
	let (box_w, _) = box_px();
	let (box_w, width) = (box_w as i32, width as i32);
	let (top, bottom) = (upper - reach, lower - 1 + reach);
	let in_x = (x_in - box_w / 4).max(0);
	let out_x = (x_out + box_w / 4 - box_w).max(in_x + box_w + BOX_GAP).min((width - box_w).max(0));
	vec![
		probe("Input", (x_in, top), Some((x_in, bottom)), (in_x, boxes_y)),
		probe("Output", (x_out, top), Some((x_out, bottom)), (out_x, boxes_y)),
	]
}

fn project(description: String, width: u32, height: u32, background: f64, simulation: Simulation) -> Project {
	let scene = Scene {
		mode: SceneMode::Canvas,
		width,
		height,
		damping_edge: RIM,
		edges: Edges::Open,
		refractive_index: background,
		..Scene::default()
	};
	Project { description, scene, simulation, ..Project::default() }
}

fn single_fibre(d: &FibreDesign, width: u32) -> Project {
	let y = d.rows(d.far_gap).0;
	let mut p = project(
		format!(
			"A single thin single-mode fibre in a medium: virtually all of the wave energy stays in the core. \
			 The probe Inside reads a constant intensity along the fibre; the line probe Across shows the \
			 evanescent tail outside the interfaces. {}",
			d.summary()
		),
		width,
		d.height,
		d.guide.clad_index,
		d.simulation(width),
	);
	let (box_w, _) = box_px();
	let inside_x = width as i32 / 2;
	let across_x = 3 * width as i32 / 4;
	let boxes_y = y + d.reach + BOX_GAP;
	let cup = d.cup();
	p.parts = vec![cup.single(y), straight_fibre("Fibre", d, y, width)];
	p.sources = source("A", d.frequency, cup.source, y, 0.0);
	p.probes = vec![
		probe("Inside", (inside_x, y), None, (inside_x - box_w as i32 - BOX_GAP, boxes_y)),
		probe("Across", (across_x, y - d.reach), Some((across_x, y + d.reach)), (across_x - box_w as i32 / 2, boxes_y)),
	];
	p
}

fn coupled_fibres(d: &FibreDesign, width: u32, gap: i64, text: &str) -> Project {
	let (upper, lower) = d.rows(gap);
	let length = d.guide.coupling_length(gap).unwrap_or(f64::INFINITY);
	let run = d.coupling_length.round() as i32;
	let cup = d.cup();
	let probe_x = cup.face + run;
	let transfers = run as f64 / length;
	let mut p = project(
		format!(
			"{text} Gap {gap} px between the cores ({} px centre to centre); coupling length {length:.1} px \
			 (pi / (2 kappa), kappa {:.6} rad/px), so the {run} px run to the probes holds {transfers:.2} transfers. \
			 Only source A is on. {}\n{}",
			d.guide.spacing(gap),
			d.guide.kappa(gap),
			d.summary(),
			coupler_tag(run as f64 * d.guide.kappa(gap))
		),
		width,
		d.height,
		d.guide.clad_index,
		d.simulation(width),
	);
	p.parts = cup.pair(upper, lower, d.guide.clad_index);
	p.parts.extend([straight_fibre("Fibre A", d, upper, width), straight_fibre("Fibre B", d, lower, width)]);
	p.sources = source("A", d.frequency, cup.source, upper, 0.0);
	p.sources
		.extend(source("B", d.frequency, cup.source, lower, 0.0).into_iter().map(|s| Source { enabled: false, ..s }));
	p.probes = line_probes(cup.face + d.lead / 2, probe_x, (upper, lower), d.reach, d.boxes_y(), width);
	p
}

fn bent_pair(d: &FibreDesign, theta: f64, phase_b: f64, text: &str) -> Result<Project> {
	let pair = d.bent(theta)?;
	let (near_a, near_b) = d.rows(d.gap);
	let (far_a, far_b) = d.rows(d.far_gap);
	let mut p = project(
		format!(
			"{text} The fibres start {} px apart (gap {} px), S-bend over {} px to a gap of {} px, run parallel \
			 for {} px and bend apart again. {} rad of coupling phase (the straight run's kappa {:.6} rad/px \
			 times {} px plus {:.4} rad for each bend); one coupling length at this gap is {:.1} px. Source B \
			 at {phase_b} degrees. {}\n{}",
			d.guide.spacing(d.far_gap),
			d.far_gap,
			d.bend_len,
			d.gap,
			pair.straight,
			format_args!("{:.4}", pair.phase),
			d.guide.kappa(d.gap),
			pair.straight,
			d.bend_phase,
			d.coupling_length,
			d.summary(),
			coupler_tag(pair.phase)
		),
		pair.width,
		d.height,
		d.guide.clad_index,
		d.simulation(pair.width),
	);
	let cup = d.cup();
	p.parts = cup.pair(far_a, far_b, d.guide.clad_index);
	p.parts.extend([bent_fibre("Fibre A", d, &pair, far_a, near_a), bent_fibre("Fibre B", d, &pair, far_b, near_b)]);
	p.sources = source("A", d.frequency, cup.source, far_a, 0.0);
	p.sources.extend(source("B", d.frequency, cup.source, far_b, phase_b));
	p.probes = line_probes(cup.face + d.lead / 2, pair.probe, (far_a, far_b), d.reach, d.boxes_y(), pair.width);
	Ok(p)
}

fn gate_project(d: &FibreDesign, kind: GateKind) -> Result<Project> {
	let mut gate = LogicGate {
		kind,
		design_frequency: d.frequency,
		core_index: d.guide.core_index,
		cladding_index: d.guide.clad_index,
		..LogicGate::default()
	};
	gate.normalize();
	let sim = Simulation { visualize_every: VISUALIZE_EVERY, ..Simulation::default() };
	let design = GateDesign::for_gate(&gate, &sim).context("the gate has no design for these indices")?;
	let half_len = (design.length as i32 + 1) / 2;
	let margin = 8;
	let (_, box_h) = box_px();
	let (spacing, core) = (design.port_spacing as i32, design.core_width as i32);
	let reach = core / 2 + (4.0 / design.gamma).ceil() as i32;
	let extent = spacing.max(spacing / 2 + reach);
	let axis = extent + BOX_GAP;
	let boxes_y = axis + extent + BOX_GAP;
	let height = ((boxes_y + box_h as i32 + BOX_GAP) as u32).div_ceil(8) * 8;
	let base = PartBase {
		name: PartKind::LogicGate(gate.clone()).name_stem().into(),
		x: margin + half_len,
		y: axis,
		..PartBase::default()
	};
	let [in_a, in_b, out_q, out_r] = ports_of(&design, &base);
	let right = base.x + half_len;
	let pad = spacing - 2 * core;
	let width = (right + pad + core) as u32;
	let face = in_a.x - 1;
	let label = match kind {
		GateKind::Xor => "XOR gate: Q is A xor B, R is A and B",
		GateKind::And => "AND gate: Q is A and B, R is A xor B",
		GateKind::Or => "OR gate: Q is A or B, R the remainder",
	};
	let steps = run_steps(width, d.guide.core_index, sim.time_decay, d.frequency, crate::truth::DEFAULT_SETTLE);
	let mut p = project(
		format!(
			"{label}. The project's logic gate part with the phase section built in: arm {} has a core of index \
			 {} over {} px, so wave B leads wave A by {} degrees at the 3 dB coupler; both sources at phase 0. \
			 Core {} px, coupled spacing {} px centre to centre (gap {} px), coupler {} px straight plus two \
			 {} px S-bends (kappa {:.6} rad/px), port spacing {} px, length {} px. Designed for a blank canvas \
			 of index 1.",
			if design.phase_on_a { "A" } else { "B" },
			design.phase_index,
			design.phase_len,
			wavedude_core::scene::gate_design::phase_shift(kind),
			design.core_width,
			design.coupled,
			design.coupled - design.core_width,
			design.coupler_len,
			design.bend_len,
			design.kappa,
			design.port_spacing,
			design.length
		),
		width,
		height,
		1.0,
		Simulation { steps: steps.div_ceil(VISUALIZE_EVERY) * VISUALIZE_EVERY, ..sim },
	);
	let dump = |name: &str, y: i32| {
		block(name, medium(1.0, 255, (pad / 2) as u32), (right, y - pad / 2), (right + pad, y + pad / 2))
	};
	p.parts = vec![
		Part { base, kind: PartKind::LogicGate(gate) },
		mirror_wall(face, height),
		dump("Dump Q", out_q.y),
		dump("Dump R", out_r.y),
	];
	// The ports are the pixels below the arms' centre lines, which the sources sit on.
	p.sources = source("A", d.frequency, in_a.x, in_a.y, 0.0);
	p.sources.extend(source("B", d.frequency, in_b.x, in_b.y, 0.0));
	p.probes = line_probes(in_a.x + d.lead, out_q.x, (in_a.y, in_b.y), reach, boxes_y, width);
	Ok(p)
}

/// The projects in the video's order, with their file stems.
fn projects(args: &Args) -> Result<(FibreDesign, Vec<(&'static str, Project)>)> {
	let d = FibreDesign::new(args)?;
	let close = (d.gap / 2).max(1);
	let full = "Two parallel fibres within about a wavelength: the energy of fibre A crosses over to fibre B \
	            almost completely over one coupling length.";
	let near = "The fibres closer together: the evanescent field reaches further into the other core, the \
	            coupling length shortens and the energy goes back and forth several times.";
	let list = vec![
		("01-single-fiber", single_fibre(&d, args.width)),
		("02-coupled-fibers-full-transfer", coupled_fibres(&d, args.width, d.gap, full)),
		("03-coupled-fibers-close", coupled_fibres(&d, args.width, close, near)),
		(
			"04-crossover",
			bent_pair(
				&d,
				FRAC_PI_2,
				0.0,
				"Crossover: the fibres bend apart where the energy has crossed over completely, so A alone \
				 exits at R and B alone at Q.",
			)?,
		),
		(
			"05-or-plain",
			bent_pair(
				&d,
				FRAC_PI_4,
				0.0,
				"Half-coupler, a plain OR: A alone or B alone gives half at Q and half at R; both on (in \
				 phase) give twice that level at Q.",
			)?,
		),
		(
			"06-or-neat",
			bent_pair(
				&d,
				FRAC_PI_4,
				-30.0,
				"The neat OR: input B delayed by 30 degrees, so a quarter of the total reaches Q with both on, \
				 the same level as for A or B alone.",
			)?,
		),
		(
			"07-xor",
			bent_pair(
				&d,
				FRAC_PI_4,
				-90.0,
				"XOR: input B delayed by 90 degrees; with both on everything goes to the residual R and Q \
				 goes dark.",
			)?,
		),
		("08-xor-gate-part", gate_project(&d, GateKind::Xor)?),
		("09-or-gate-part", gate_project(&d, GateKind::Or)?),
		("10-and-gate-part", gate_project(&d, GateKind::And)?),
	];
	Ok((d, list))
}

/// The app's Auto brightness (field, intensity) of the project's last timeline frame, run at full quality as saved.
fn fitted_brightness(project: &Project, engine: EngineArgs) -> Result<[f64; 2]> {
	let pixels = compose_scene(project);
	let spec = RunSpec::create(project, pixels.as_ref(), PreviewQuality::FULL, true)?;
	let mut engine = engine.build(&spec);
	engine.set_emitters(&spec.emitters);
	let last = project.simulation.frame_count().saturating_sub(1);
	for _ in 0..=spec.step_of(last) {
		engine.advance_one_step()?;
	}
	let snapshot = engine.capture_frame(&spec.shape)?;
	let fit = |layer| {
		fit_brightness(snapshot.halves(layer), spec.shape.sample_kind(layer))
			.with_context(|| format!("the last frame's {layer:?} has nothing to fit"))
	};
	Ok([fit(Layer::Field)?, fit(Layer::Intensity)?])
}

fn write(path: &Path, project: &Project) -> Result<()> {
	save_project(project, path).with_context(|| format!("writing {}", path.display()))
}

pub(crate) fn run(args: &Args) -> Result<()> {
	let (design, mut list) = projects(args)?;
	fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
	println!(
		"design: wavelength {:.3} px, core {} px, decay length {:.2} px, gap {} px, coupling length {:.1} px, \
		 S-bends {} px ({:.4} rad each), far gap {} px, solver {:?} in time with {} sub-steps",
		design.lambda,
		design.guide.core,
		1.0 / design.gamma,
		design.gap,
		design.coupling_length,
		design.bend_len,
		design.bend_phase,
		design.far_gap,
		design.guide.solver.scheme,
		design.guide.solver.sub_steps
	);
	for (stem, project) in &mut list {
		let [field, intensity] = fitted_brightness(project, args.engine)?;
		project.display.field.brightness = field;
		project.display.intensity.brightness = intensity;
		let path = args.out.join(format!("{stem}.{}", wdproj::EXTENSION));
		write(&path, project)?;
		println!(
			"{} ({}x{}, {} steps, brightness field {field}, intensity {intensity})",
			path.display(),
			project.scene.width,
			project.scene.height,
			project.simulation.steps
		);
	}
	Ok(())
}
