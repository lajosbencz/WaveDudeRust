//! The GPU wave engine on wgpu compute (spec/30-engine.md, GPU engine; decisions/0003).
//!
//! Same rules as the CPU engine (engine/cpu.rs), which is the reference:
//!
//! - Buffers cover the padded grid ([`SceneData::stride`], [`PAD`] rings), all f32: two field
//!   buffers swapped by bind group (the next field is written over the previous one), the modified
//!   equation's `w`, the intensity stages `s1` and `s2`, the medium (see [`MediumLayout`]), the cut
//!   bytes (four to a word), the Mur chains, the source groups, and on a rim the CPML's coefficient
//!   tables and two sets of `psi` (the strips) and `xi` (the strips, then delta_y over the y strips;
//!   engine/pml.rs).
//! - Kernels (WGSL under `shaders/`): `force_w` (the sources' forcing into the modified equation's
//!   w between its passes) or `force` / `force_intensity` (into leapfrog's next field); the grid passes `step` /
//!   `step_intensity` (9-point leapfrog) or `inner` and `outer` / `outer_intensity` (DRP, modified
//!   equation); the CPML's `pml_psi_y`, `pml_delta_y`, `pml_psi_x` over the rim's strips and
//!   `pml_correct` over its cells, after each grid pass as on the CPU; `edges` (Mur on the
//!   rings); `capture`; `probes`. Mirror cuts are compiled in through the `CUTS` override only for a
//!   scene with mirrors. Grid kernels run in 32 x 8 work groups over the grid rounded up to whole
//!   groups, with bounds checks; the rim's in 64-wide groups.
//! - Source values are computed on the host in f64 exactly as the CPU engine does (the per-level
//!   image values and every object emitter's value, pulse envelopes and random-gap state included),
//!   one set per sub-step (engine/forcing.rs), the step's sets uploaded together; the kernels only
//!   add a cell's values in the CPU's order.
//! - The intensity keeps `s1` and `s2`; their blend is made where it is read.
//!
//! Steps are recorded into one command encoder and submitted every [`BATCH_STEPS`] steps or when
//! something is read; at most [`MAX_IN_FLIGHT`] batches are queued ahead of the card. Probe
//! readings and `prepare_frame` wait for the card; a capture is submitted with its copy to one of two
//! staging buffers and collected later, so the card converts and copies while the host steps on.
//!
//! Where the GPU may differ from the CPU: the driver may fuse a multiply and an add (the CPU does
//! not), and may flush subnormals. Both change the last bit of some cells per step; the difference
//! stays within the tolerance of docs/physics.md (relative 1e-4 on the field after 1000 steps). The
//! capture's block mean divides by the block's cell count with the card's division, not guaranteed
//! correctly rounded by WGSL; the intensity root is the card's square root corrected to the correctly
//! rounded one in integers (capture.wgsl, `sqrt_rn`), the f16 rounding is bit-exact (round to
//! nearest even), and the source values are the CPU's bits.
//!
//! Threading: one driver thread per engine, no internal locking; the device and queue are shared
//! by every engine of the process (see [`context`]).

mod context;
mod medium;

use std::borrow::Cow;
use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::{Arc, OnceLock};

use bytemuck::{Pod, Zeroable};

pub use context::{GpuContext, GpuError, context};
pub use medium::MediumLayout;

use super::cpu::{blend, centred, mur_chains, sub_step_time};
use super::forcing::{IMAGE_LEVELS, SourceGroups, Sources};
use super::pml::{self, Strips, axis_table};
use super::stencil::{PAD, StencilCoefficients, TimeScheme};
use super::substeps::sub_steps;
use super::types::{
	Emitter, EngineOptions, FrameShape, PendingCapture, ProbeCells, ProbeReadings, SampleFormat, SceneData, Snapshot,
};
use super::{EngineError, WaveEngine};
use crate::model::{Edges, Layer};

/// Steps recorded per submission at most.
pub const BATCH_STEPS: u32 = 32;
/// Submissions queued ahead of the card at most.
pub const MAX_IN_FLIGHT: usize = 2;

const GRID_GROUP: (u32, u32) = (32, 8);
const LINEAR_GROUP: u32 = 64;
const CAPTURE_GROUP: u32 = 256;
/// Bit of a probe cell word selecting the blended intensity.
const PROBE_INTENSITY: u32 = 1 << 31;

const COMMON: &str = include_str!("shaders/common.wgsl");
const STEP: &str = include_str!("shaders/step.wgsl");
const TAPS: &str = include_str!("shaders/taps.wgsl");
const CUTS: &str = include_str!("shaders/cuts.wgsl");
const PML: &str = include_str!("shaders/pml.wgsl");
const MEDIUM_COMPACT: &str = include_str!("shaders/medium_compact.wgsl");
const MEDIUM_PLAIN: &str = include_str!("shaders/medium_plain.wgsl");
const EDGES: &str = include_str!("shaders/edges.wgsl");
const FORCE: &str = include_str!("shaders/force.wgsl");
const PROBES: &str = include_str!("shaders/probes.wgsl");
const CAPTURE: &str = include_str!("shaders/capture.wgsl");

/// Uniform shared by every kernel; mirrors `Params` in common.wgsl (80 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct Params {
	stride: u32,
	gw: u32,
	gh: u32,
	rim: u32,
	canvas_w: u32,
	canvas_h: u32,
	mur_count: u32,
	probe_count: u32,
	alpha: f32,
	rest: f32,
	intensity_scale: f32,
	d_base: u32,
	group_count: u32,
	pml_x_len: u32,
	pml_len: u32,
	rim_cells: u32,
	_pad: [u32; 4],
}

/// Uniform of the capture kernel; mirrors `Capture` in capture.wgsl (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct CaptureParams {
	out_w: u32,
	divisor: u32,
	samples: u32,
	layer_words: u32,
	format: u32,
	root: u32,
	field_b: f32,
	intensity_b: f32,
}

/// One Mur chain; mirrors `Mur` in edges.wgsl (28 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MurChain {
	inner: u32,
	rings: [u32; PAD],
	weights: [f32; 3],
}

const STEP_SLOTS: &[Slot] = &[
	Slot::Uniform,
	Slot::Read,
	Slot::ReadWrite,
	Slot::ReadWrite,
	Slot::ReadWrite,
	Slot::Read,
	Slot::Read,
	Slot::ReadWrite,
	Slot::Read,
];
const PML_SLOTS: &[Slot] = &[
	Slot::Uniform,
	Slot::Read,
	Slot::ReadWrite,
	Slot::ReadWrite,
	Slot::ReadWrite,
	Slot::Read,
	Slot::Read,
	Slot::Read,
	Slot::Read,
];
const EDGES_SLOTS: &[Slot] = &[Slot::Uniform, Slot::Read, Slot::ReadWrite, Slot::Read];
const FORCE_SLOTS: &[Slot] =
	&[Slot::Uniform, Slot::Read, Slot::ReadDynamic, Slot::ReadWrite, Slot::ReadWrite, Slot::ReadWrite];
const PROBES_SLOTS: &[Slot] = &[Slot::Uniform, Slot::Read, Slot::Read, Slot::Read, Slot::Read, Slot::ReadWrite];
const CAPTURE_SLOTS: &[Slot] = &[Slot::Uniform, Slot::Uniform, Slot::Read, Slot::Read, Slot::Read, Slot::ReadWrite];

/// Binding kinds of a kernel's bind group, in binding order.
#[derive(Clone, Copy)]
enum Slot {
	Uniform,
	Read,
	ReadWrite,
	/// Read-only with a dynamic offset.
	ReadDynamic,
}

struct Kernel {
	slots: &'static [Slot],
	layout: wgpu::BindGroupLayout,
	pipeline: wgpu::ComputePipeline,
}

/// The outcome of a mapping request, set by its callback.
type MapState = Arc<OnceLock<Result<(), wgpu::BufferAsyncError>>>;

/// A host-mappable buffer, its map state, and the submission that fills it while it is pending.
struct Readback {
	buffer: wgpu::Buffer,
	mapped: MapState,
	pending: Option<wgpu::SubmissionIndex>,
}

/// Word layout of a capture of `shape` (capture.wgsl): layers, words of samples and words per layer.
fn capture_words(shape: &FrameShape, track_intensity: bool) -> (u32, u32, u32) {
	let layers = if track_intensity { 2 } else { 1 };
	let per_word = match shape.format {
		SampleFormat::Half => 2,
		SampleFormat::Index => 4,
	};
	let words = (shape.plane_samples() as u32).div_ceil(per_word);
	(layers, words, words.next_multiple_of(64).max(64))
}

/// The wgpu engine. See the module docs for layouts and differences from the CPU engine.
pub struct GpuEngine {
	ctx: Arc<GpuContext>,
	scene: Arc<SceneData>,
	options: EngineOptions,
	/// The layout asked for, and the one built.
	preferred: MediumLayout,
	layout: MediumLayout,
	sub_steps: u32,
	scheme: TimeScheme,
	/// The scene has mirrors: the kernels apply the cuts.
	cuts_on: bool,
	stride: usize,
	cells: usize,
	params: Params,
	params_buf: wgpu::Buffer,
	field: [wgpu::Buffer; 2],
	/// Index into `field` of the buffer holding the current field.
	cur: usize,
	/// The modified equation's `w` (a placeholder for leapfrog).
	w: wgpu::Buffer,
	s1: wgpu::Buffer,
	s2: wgpu::Buffer,
	medium: wgpu::Buffer,
	tables: wgpu::Buffer,
	cuts: wgpu::Buffer,
	mur: wgpu::Buffer,
	/// CPML coefficients (x then y), and psi and xi of the two applications of the operator.
	coef: wgpu::Buffer,
	psi: [wgpu::Buffer; 2],
	xi: [wgpu::Buffer; 2],
	groups: wgpu::Buffer,
	step_values: wgpu::Buffer,
	slot_bytes: u64,
	probe_cells: wgpu::Buffer,
	probe_out: wgpu::Buffer,
	probe_readback: Option<Readback>,
	probe_lengths: Vec<usize>,
	capture_params: wgpu::Buffer,
	capture_out: wgpu::Buffer,
	captures: [Option<Readback>; 2],
	next_capture: usize,
	/// step, step_intensity (leapfrog) or inner, outer, outer_intensity (modified equation).
	step_kernels: Vec<Kernel>,
	/// pml_psi_y, pml_delta_y, pml_psi_x, pml_correct at scale 1, pml_correct at scale 1/12.
	pml_kernels: Vec<Kernel>,
	edges_kernel: Kernel,
	/// force_w (modified equation) or force and force_intensity (leapfrog).
	force_kernels: Vec<Kernel>,
	probes_kernel: Kernel,
	capture_kernel: Kernel,
	step_groups: [wgpu::BindGroup; 2],
	/// Per parity, per application of the operator: the memory stages (psi_y, delta_y, psi_x) and the
	/// correction.
	psi_groups: [[wgpu::BindGroup; 2]; 2],
	correct_groups: [[wgpu::BindGroup; 2]; 2],
	edges_groups: [wgpu::BindGroup; 2],
	force_groups: [wgpu::BindGroup; 2],
	probes_groups: [wgpu::BindGroup; 2],
	capture_groups: [wgpu::BindGroup; 2],
	pending: Option<wgpu::CommandEncoder>,
	batch_steps: u32,
	in_flight: VecDeque<wgpu::SubmissionIndex>,
	sources: Sources,
	/// d^(1/n) per padded cell, for the forcing's h.
	d: Vec<f32>,
	values: Vec<f32>,
	time: u32,
	/// The first failure of a call that returns no `Result`, returned by the next one that does.
	failed: Option<GpuError>,
}

impl GpuEngine {
	/// An engine at rest on `scene` on the shared device, with the compact medium.
	pub fn new(scene: impl Into<Arc<SceneData>>, options: EngineOptions) -> Result<Self, GpuError> {
		Self::with_layout(scene, options, MediumLayout::Compact)
	}

	/// An engine with the medium in `layout` (compact falls back to plain when the scene has too
	/// many distinct values). For measurements of the two layouts.
	pub fn with_layout(
		scene: impl Into<Arc<SceneData>>,
		options: EngineOptions,
		layout: MediumLayout,
	) -> Result<Self, GpuError> {
		let scene = scene.into();
		// Cell indices are u32 words, probe cells carry the layer in bit 31.
		if scene.padded_cells() >= PROBE_INTENSITY as usize {
			return Err(GpuError::GridTooLarge(scene.padded_cells()));
		}
		let ctx = context()?;
		let device = ctx.device.clone();
		let device = &device;
		let n = sub_steps(options.stencil, scene.max_c2());
		let medium = medium::build(&scene, n, layout);
		let stride = scene.stride();
		let cells = scene.padded_cells();
		let scheme = TimeScheme::of(options.stencil);
		let coefficients = StencilCoefficients::of(options.stencil);
		let cuts = pml::cuts(&scene);
		let cuts_on = cuts.iter().any(|&c| c != 0);

		let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
		let step_layout = kernel_layout(device, "step", STEP_SLOTS);
		let step_kernels = step_pipelines(device, &step_layout, &coefficients, scheme, medium.layout, cuts_on);
		let pml_layout = kernel_layout(device, "pml", PML_SLOTS);
		let pml_kernels = pml_pipelines(device, &pml_layout, &coefficients, medium.layout, cuts_on);
		let edges_kernel = kernel(device, "edges", EDGES, EDGES_SLOTS, &coefficients);
		let force_kernels = force_pipelines(device, &coefficients, scheme);
		let probes_kernel = kernel(device, "probes", PROBES, PROBES_SLOTS, &coefficients);
		let capture_kernel = kernel(device, "capture", CAPTURE, CAPTURE_SLOTS, &coefficients);
		if let Some(e) = pollster::block_on(scope.pop()) {
			return Err(GpuError::Pipeline(e.to_string()));
		}

		let grid = |label| storage(device, label, (cells * 4) as u64);
		let field = [grid("field a"), grid("field b")];
		let (s1, s2) = (grid("s1"), grid("s2"));
		let w = if scheme == TimeScheme::ModifiedEquation { grid("w") } else { storage(device, "w", 16) };
		let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("params"),
			size: size_of::<Params>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let capture_params = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("capture params"),
			size: size_of::<CaptureParams>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let strips = Strips::of(&scene);
		let memory = |label, cells: usize| storage(device, label, (cells.max(1) * 4) as u64);
		let empty = |label| storage(device, label, 16);
		let mut engine = Self {
			ctx,
			preferred: layout,
			layout: medium.layout,
			sub_steps: n,
			scheme,
			cuts_on,
			stride,
			cells,
			params: Params::default(),
			params_buf,
			cur: 0,
			w,
			medium: empty("medium"),
			tables: empty("tables"),
			cuts: empty("cuts"),
			mur: empty("mur"),
			coef: empty("pml coefficients"),
			psi: [memory("psi 0", strips.len()), memory("psi 1", strips.len())],
			xi: [memory("xi 0", strips.len() + strips.y_len()), memory("xi 1", strips.len() + strips.y_len())],
			groups: empty("groups"),
			step_values: empty("step values"),
			slot_bytes: 16,
			probe_cells: empty("probe cells"),
			probe_out: empty("probe out"),
			probe_readback: None,
			probe_lengths: Vec::new(),
			capture_params,
			capture_out: empty("capture out"),
			captures: [None, None],
			next_capture: 0,
			step_groups: placeholder(device, &step_kernels[0]),
			psi_groups: [placeholder(device, &pml_kernels[0]), placeholder(device, &pml_kernels[0])],
			correct_groups: [placeholder(device, &pml_kernels[3]), placeholder(device, &pml_kernels[3])],
			edges_groups: placeholder(device, &edges_kernel),
			force_groups: placeholder(device, &force_kernels[0]),
			probes_groups: placeholder(device, &probes_kernel),
			capture_groups: placeholder(device, &capture_kernel),
			step_kernels,
			pml_kernels,
			edges_kernel,
			force_kernels,
			probes_kernel,
			capture_kernel,
			field,
			s1,
			s2,
			pending: None,
			batch_steps: 0,
			in_flight: VecDeque::new(),
			sources: Sources::new(coefficients, options.source_frequency_scale, options.image_source_scale),
			d: Vec::new(),
			values: Vec::new(),
			time: 0,
			failed: None,
			options,
			scene,
		};
		engine.load_scene(medium);
		engine.update_alpha();
		engine.upload_groups();
		engine.bind_probes();
		engine.bind_capture();
		engine.reset();
		engine.check()?;
		tracing::info!(
			grid = %format!("{} x {}", engine.scene.grid_width(), engine.scene.grid_height()),
			canvas = %format!("{} x {}", engine.scene.canvas_width, engine.scene.canvas_height),
			rim = engine.scene.rim,
			edges = ?engine.scene.edges,
			stride,
			stencil = ?engine.options.stencil,
			scheme = ?engine.scheme,
			mirror_cuts = engine.cuts_on,
			sub_steps = n,
			medium = ?engine.layout,
			work_groups = %format!(
				"{} x {} of {} x {}",
				engine.scene.grid_width().div_ceil(GRID_GROUP.0),
				engine.scene.grid_height().div_ceil(GRID_GROUP.1),
				GRID_GROUP.0,
				GRID_GROUP.1
			),
			"GPU engine built"
		);
		Ok(engine)
	}

	/// The medium layout in use (compact may have fallen back to plain).
	pub fn medium_layout(&self) -> MediumLayout {
		self.layout
	}

	/// Sets the field now and one sub-step before, each grid-sized (no padding), row-major; the ring
	/// keeps its values. For analytic tests that start from a given state.
	pub fn set_field(&mut self, now: &[f32], before: &[f32]) {
		let gw = self.scene.grid_width() as usize;
		assert_eq!(now.len(), gw * self.scene.grid_height() as usize);
		assert_eq!(before.len(), now.len());
		let flushed = self.flush();
		self.note(flushed);
		let queue = &self.ctx.queue;
		for (gy, (a, b)) in now.chunks_exact(gw).zip(before.chunks_exact(gw)).enumerate() {
			let at = (self.scene.padded_index(0, gy) * 4) as u64;
			queue.write_buffer(&self.field[self.cur], at, bytemuck::cast_slice(a));
			queue.write_buffer(&self.field[1 - self.cur], at, bytemuck::cast_slice(b));
		}
	}

	/// Submits the recorded steps and waits until the card has run them (timing, tests).
	pub fn synchronize(&mut self) -> Result<(), GpuError> {
		self.check()?;
		let index = self.submit(|_| ())?;
		wait(&self.ctx.device, index)?;
		self.in_flight.clear();
		Ok(())
	}

	/// The kept failure or the device's loss.
	fn check(&self) -> Result<(), GpuError> {
		match self.failed.clone().or_else(|| self.ctx.lost()) {
			Some(e) => Err(e),
			None => Ok(()),
		}
	}

	/// Keeps the first failure of a call that returns no `Result`.
	fn note(&mut self, result: Result<(), GpuError>) {
		if let Err(e) = result {
			self.failed.get_or_insert(e);
		}
	}

	fn device(&self) -> &wgpu::Device {
		&self.ctx.device
	}

	fn cell_of_grid(&self, gx: usize, gy: usize) -> usize {
		self.scene.padded_index(gx, gy)
	}

	/// Uploads the medium, the cuts, the Mur chains and the CPML coefficients, and rebuilds what
	/// depends on them.
	fn load_scene(&mut self, medium: medium::Medium) {
		let cuts = pml::cuts(&self.scene);
		let cuts_on = cuts.iter().any(|&c| c != 0);
		if medium.layout != self.layout || cuts_on != self.cuts_on {
			let coefficients = StencilCoefficients::of(self.options.stencil);
			let device = &self.ctx.device;
			self.step_kernels = step_pipelines(
				device,
				&self.step_kernels[0].layout,
				&coefficients,
				self.scheme,
				medium.layout,
				cuts_on,
			);
			self.pml_kernels =
				pml_pipelines(device, &self.pml_kernels[0].layout, &coefficients, medium.layout, cuts_on);
		}
		self.layout = medium.layout;
		self.cuts_on = cuts_on;
		self.medium = storage_init(self.device(), "medium", &medium.cells);
		self.tables = storage_init(self.device(), "tables", bytemuck::cast_slice(&medium.tables));
		self.cuts = storage_init(self.device(), "cuts", &cuts);
		let open = self.scene.edges == Edges::Open && self.scene.rim == 0;
		let mur: Vec<MurChain> = if open {
			mur_chains(&self.scene, &medium.c2, &medium.d)
				.into_iter()
				.map(|m| MurChain { inner: m.inner as u32, rings: m.rings.map(|r| r as u32), weights: m.weights })
				.collect()
		} else {
			Vec::new()
		};
		self.mur = storage_init(self.device(), "mur", bytemuck::cast_slice(&padded(mur.clone(), MurChain::zeroed())));
		let dt = 1.0 / self.sub_steps as f64;
		let strips = Strips::of(&self.scene);
		let coef: Vec<[f32; 4]> = axis_table(&self.scene.pml, strips.gw, dt)
			.into_iter()
			.chain(axis_table(&self.scene.pml, strips.gh, dt))
			.collect();
		self.coef = storage_init(self.device(), "pml coefficients", bytemuck::cast_slice(&coef));
		self.sources.level_used = [false; IMAGE_LEVELS];
		for s in &self.scene.image_sources {
			self.sources.level_used[s.level as usize] = true;
		}
		self.d = medium.d;
		self.params = Params {
			stride: self.stride as u32,
			gw: self.scene.grid_width(),
			gh: self.scene.grid_height(),
			rim: self.scene.rim,
			canvas_w: self.scene.canvas_width,
			canvas_h: self.scene.canvas_height,
			mur_count: mur.len() as u32,
			intensity_scale: self.options.intensity_scale as f32,
			d_base: medium.d_base,
			pml_x_len: strips.x_len() as u32,
			pml_len: strips.len() as u32,
			rim_cells: strips.rim_cells() as u32,
			..self.params
		};
		self.bind_grid();
	}

	/// EMA weight of the intensity from the lowest frequency in use (as the CPU engine).
	fn update_alpha(&mut self) {
		self.params.alpha = self.sources.intensity_alpha(self.options.time_decay);
		self.params.rest = 1.0 - self.params.alpha;
	}

	/// Builds and uploads the source groups (image sources first, then emitters, per distinct cell
	/// in order of first appearance), the value slots (one per sub-step of a batch), and the parameters.
	fn upload_groups(&mut self) {
		let image: Vec<(usize, u8)> = self
			.scene
			.image_sources
			.iter()
			.map(|s| (self.cell_of_grid(s.gx as usize, s.gy as usize), s.level))
			.collect();
		let groups = SourceGroups::new(&self.scene, image, &self.sources.tap_cells());
		let mut words = Vec::with_capacity(4 * groups.cells.len() + groups.entries.len());
		for (&cell, &(start, len)) in groups.cells.iter().zip(&groups.runs) {
			words.extend([cell as u32, start, len, centred(self.d[cell]).to_bits()]);
		}
		words.extend_from_slice(&groups.entries);
		self.params.group_count = groups.cells.len() as u32;
		self.groups = storage_init(self.device(), "source groups", bytemuck::cast_slice(&padded(words, 0)));
		let align = self.ctx.limits.min_storage_buffer_offset_alignment as u64;
		self.slot_bytes = ((self.sources.table_len() * 4) as u64).next_multiple_of(align);
		let slots = (BATCH_STEPS * self.sub_steps) as u64;
		self.step_values = storage(self.device(), "step values", self.slot_bytes * slots);
		self.write_params();
		self.bind_sources();
	}

	fn write_params(&self) {
		self.ctx.queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
	}

	/// Bind groups of the kernels reading the field, the medium or the Mur pairs, per parity.
	fn bind_grid(&mut self) {
		let device = &self.ctx.device;
		let (p, s1, s2) = (&self.params_buf, &self.s1, &self.s2);
		let (medium, tables, cuts, w) = (&self.medium, &self.tables, &self.cuts, &self.w);
		self.step_groups = [0, 1].map(|c| {
			let (cur, nxt) = (&self.field[c], &self.field[1 - c]);
			bind(device, &self.step_kernels[0].layout, &[p, cur, nxt, s1, s2, medium, tables, w, cuts])
		});
		let modified = self.scheme == TimeScheme::ModifiedEquation;
		let pml_layout = &self.pml_kernels[0].layout;
		let (psi, xi, coef) = (&self.psi, &self.xi, &self.coef);
		self.psi_groups = [0, 1].map(|c| {
			[0, 1].map(|set| {
				let src = if set == 0 { &self.field[c] } else { w };
				let unused = &self.field[1 - c];
				bind(device, pml_layout, &[p, src, unused, &psi[set], &xi[set], medium, tables, coef, cuts])
			})
		});
		self.correct_groups = [0, 1].map(|c| {
			[0, 1].map(|set| {
				let (src, out) = match (set, modified) {
					(0, true) => (&self.field[c], w),
					(0, false) => (&self.field[c], &self.field[1 - c]),
					_ => (w, &self.field[1 - c]),
				};
				bind(device, pml_layout, &[p, src, out, &psi[set], &xi[set], medium, tables, coef, cuts])
			})
		});
		self.edges_groups = [0, 1]
			.map(|c| bind(device, &self.edges_kernel.layout, &[p, &self.field[c], &self.field[1 - c], &self.mur]));
		self.bind_sources();
		self.bind_probes();
		self.bind_capture();
	}

	/// The forcing's bind groups per parity: into `w` (modified equation) or the next field.
	fn bind_sources(&mut self) {
		let device = &self.ctx.device;
		let layout = &self.force_kernels[0].layout;
		let modified = self.scheme == TimeScheme::ModifiedEquation;
		self.force_groups = [0, 1].map(|c| {
			let target = if modified { &self.w } else { &self.field[1 - c] };
			device.create_bind_group(&wgpu::BindGroupDescriptor {
				label: Some("force"),
				layout,
				entries: &[
					entry(0, &self.params_buf, None),
					entry(1, &self.groups, None),
					entry(2, &self.step_values, NonZeroU64::new(self.slot_bytes)),
					entry(3, target, None),
					entry(4, &self.s1, None),
					entry(5, &self.s2, None),
				],
			})
		});
	}

	fn bind_probes(&mut self) {
		let device = &self.ctx.device;
		self.probes_groups = [0, 1].map(|c| {
			let buffers = [&self.params_buf, &self.field[c], &self.s1, &self.s2, &self.probe_cells, &self.probe_out];
			bind(device, &self.probes_kernel.layout, &buffers)
		});
	}

	fn bind_capture(&mut self) {
		let device = &self.ctx.device;
		self.capture_groups = [0, 1].map(|c| {
			let buffers =
				[&self.params_buf, &self.capture_params, &self.field[c], &self.s1, &self.s2, &self.capture_out];
			bind(device, &self.capture_kernel.layout, &buffers)
		});
	}

	/// The step's source tables, one slot per sub-step at the time of its current field
	/// ([`sub_step_time`]), computed as the CPU engine does (engine/forcing.rs).
	fn source_values(&mut self) {
		let n = self.sub_steps;
		let slot = self.slot_bytes as usize / 4;
		self.values.clear();
		self.values.resize(slot * n as usize, 0.0);
		for (k, values) in self.values.chunks_exact_mut(slot).enumerate() {
			self.sources.values(values, sub_step_time(self.time, k as u32, n), n, self.scheme);
		}
	}

	/// Runs `record` on the pending encoder (or a new one) and submits everything.
	fn submit(&mut self, record: impl FnOnce(&mut wgpu::CommandEncoder)) -> Result<wgpu::SubmissionIndex, GpuError> {
		let mut encoder =
			self.pending.take().unwrap_or_else(|| self.ctx.device.create_command_encoder(&Default::default()));
		record(&mut encoder);
		self.batch_steps = 0;
		let index = self.ctx.queue.submit([encoder.finish()]);
		self.in_flight.push_back(index.clone());
		while self.in_flight.len() > MAX_IN_FLIGHT {
			let oldest = self.in_flight.pop_front().expect("in flight");
			wait(&self.ctx.device, oldest)?;
		}
		Ok(index)
	}

	/// Submits the recorded steps, if any.
	fn flush(&mut self) -> Result<(), GpuError> {
		if self.pending.is_some() {
			self.submit(|_| ())?;
		}
		Ok(())
	}

	/// Copies `sources` (buffer, bytes) into one mapped buffer and returns its bytes.
	fn read(&mut self, sources: &[(&wgpu::Buffer, u64)]) -> Result<Vec<u8>, GpuError> {
		let total: u64 = sources.iter().map(|s| s.1).sum();
		let readback = readback_buffer(&self.ctx.device, "readback", total);
		let list: Vec<(wgpu::Buffer, u64)> = sources.iter().map(|&(b, n)| (b.clone(), n)).collect();
		let target = readback.buffer.clone();
		let index = self.submit(|encoder| {
			let mut at = 0;
			for (b, n) in &list {
				encoder.copy_buffer_to_buffer(b, 0, &target, at, *n);
				at += n;
			}
		})?;
		let mut readback = readback;
		map(&mut readback, index);
		wait_mapped(&self.ctx.device, &mut readback)?;
		let bytes = readback.buffer.slice(..).get_mapped_range().map_err(GpuError::MapRange)?.to_vec();
		readback.buffer.unmap();
		Ok(bytes)
	}
}

impl WaveEngine for GpuEngine {
	fn name(&self) -> &'static str {
		"GPU"
	}

	fn scene(&self) -> &SceneData {
		&self.scene
	}

	fn options(&self) -> &EngineOptions {
		&self.options
	}

	fn time(&self) -> u32 {
		self.time
	}

	fn sub_steps(&self) -> u32 {
		self.sub_steps
	}

	fn reset(&mut self) {
		let buffers = [
			&self.field[0],
			&self.field[1],
			&self.w,
			&self.s1,
			&self.s2,
			&self.psi[0],
			&self.psi[1],
			&self.xi[0],
			&self.xi[1],
		]
		.map(|b| b.clone());
		let submitted = self.submit(|encoder| {
			for b in &buffers {
				encoder.clear_buffer(b, 0, None);
			}
		});
		self.note(submitted.map(drop));
		self.cur = 0;
		self.time = 0;
		self.sources.reset_pulses();
	}

	fn set_emitters(&mut self, emitters: &[Emitter]) {
		let flushed = self.flush();
		self.note(flushed);
		self.sources.set_emitters(emitters, &self.scene);
		self.update_alpha();
		self.upload_groups();
	}

	fn set_probes(&mut self, probes: &[ProbeCells]) {
		let flushed = self.flush();
		self.note(flushed);
		let (scene, rim) = (&self.scene, self.scene.rim as usize);
		// Below PROBE_INTENSITY: the grid size is checked at build.
		let words: Vec<u32> = probes
			.iter()
			.flat_map(|p| {
				let flag = if p.layer == Layer::Intensity { PROBE_INTENSITY } else { 0 };
				p.cells.iter().map(move |&(x, y)| scene.padded_index(x as usize + rim, y as usize + rim) as u32 | flag)
			})
			.collect();
		self.probe_lengths = probes.iter().map(|p| p.cells.len()).collect();
		self.params.probe_count = words.len() as u32;
		self.probe_cells = storage_init(self.device(), "probe cells", bytemuck::cast_slice(&padded(words.clone(), 0)));
		let bytes = (words.len().max(1) * 4) as u64;
		self.probe_out = storage(self.device(), "probe out", bytes);
		self.probe_readback = Some(readback_buffer(self.device(), "probe readback", bytes));
		self.write_params();
		self.bind_probes();
	}

	fn try_replace_scene(&mut self, scene: &Arc<SceneData>) -> bool {
		if !self.scene.same_grid(scene) {
			return false;
		}
		let flushed = self.flush();
		self.note(flushed);
		self.scene = scene.clone();
		self.sub_steps = sub_steps(self.options.stencil, self.scene.max_c2());
		let medium = medium::build(&self.scene, self.sub_steps, self.preferred);
		self.load_scene(medium);
		self.update_alpha();
		self.upload_groups();
		tracing::debug!(sub_steps = self.sub_steps, medium = ?self.layout, "GPU engine scene replaced");
		true
	}

	fn advance_one_step(&mut self) -> Result<(), EngineError> {
		self.guarded(Self::step)
	}

	fn sample_probes(&mut self, out: &mut ProbeReadings) -> Result<(), EngineError> {
		self.guarded(|e| e.read_probes(out))
	}

	fn begin_capture(&mut self, shape: &FrameShape, into: Snapshot) -> Result<PendingCapture, EngineError> {
		self.guarded(|e| e.begin(shape, into))
	}

	fn finish_capture(&mut self, pending: PendingCapture) -> Result<Snapshot, EngineError> {
		self.guarded(|e| e.finish(pending))
	}

	fn prepare_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), EngineError> {
		self.guarded(Self::read_frame)
	}
}

/// The bodies of the fallible [`WaveEngine`] calls, run through [`GpuEngine::guarded`].
impl GpuEngine {
	/// Runs `f` unless the engine failed before; a failure is kept.
	fn guarded<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, GpuError>) -> Result<T, EngineError> {
		self.check()?;
		f(self).map_err(|e| {
			self.failed.get_or_insert(e.clone());
			e.into()
		})
	}

	fn step(&mut self) -> Result<(), GpuError> {
		self.source_values();
		let first_slot = (self.batch_steps * self.sub_steps) as u64 * self.slot_bytes;
		self.ctx.queue.write_buffer(&self.step_values, first_slot, bytemuck::cast_slice(&self.values));
		let mut encoder =
			self.pending.take().unwrap_or_else(|| self.ctx.device.create_command_encoder(&Default::default()));
		{
			let mut pass = encoder.begin_compute_pass(&Default::default());
			let groups = self.params.group_count;
			let force = |pass: &mut wgpu::ComputePass, kernel: &Kernel, group: &wgpu::BindGroup, k: u32| {
				if groups > 0 {
					pass.set_pipeline(&kernel.pipeline);
					pass.set_bind_group(0, group, &[(first_slot + k as u64 * self.slot_bytes) as u32]);
					pass.dispatch_workgroups(groups.div_ceil(LINEAR_GROUP), 1, 1);
				}
			};
			let grid = (self.params.gw.div_ceil(GRID_GROUP.0), self.params.gh.div_ceil(GRID_GROUP.1));
			let max = self.ctx.limits.max_compute_workgroups_per_dimension;
			let linear = |threads: u32| {
				let groups = threads.div_ceil(LINEAR_GROUP);
				(groups.min(max), groups.div_ceil(max))
			};
			let y_len = self.params.pml_len - self.params.pml_x_len;
			let (x_strips, y_strips) = (linear(self.params.pml_x_len), linear(y_len));
			let rim_cells = linear(self.params.rim_cells);
			let pml = self.params.rim_cells > 0;
			let mur = self.params.mur_count;
			let dispatch =
				|pass: &mut wgpu::ComputePass, kernel: &Kernel, group: &wgpu::BindGroup, (x, y): (u32, u32)| {
					pass.set_pipeline(&kernel.pipeline);
					pass.set_bind_group(0, group, &[]);
					pass.dispatch_workgroups(x, y, 1);
				};
			for k in 0..self.sub_steps {
				let intensity = self.options.track_intensity && k + 1 == self.sub_steps;
				let c = self.cur;
				let (step, psi, correct) = (&self.step_groups[c], &self.psi_groups[c], &self.correct_groups[c]);
				let kernels = &self.pml_kernels;
				let apply = |pass: &mut wgpu::ComputePass, set: usize, correction: &Kernel| {
					if pml {
						dispatch(pass, &kernels[0], &psi[set], y_strips);
						dispatch(pass, &kernels[1], &psi[set], y_strips);
						dispatch(pass, &kernels[2], &psi[set], x_strips);
						dispatch(pass, correction, &correct[set], rim_cells);
					}
				};
				match self.scheme {
					TimeScheme::Leapfrog => {
						dispatch(&mut pass, &self.step_kernels[intensity as usize], step, grid);
						apply(&mut pass, 0, &kernels[3]);
						force(&mut pass, &self.force_kernels[intensity as usize], &self.force_groups[c], k);
					}
					TimeScheme::ModifiedEquation => {
						dispatch(&mut pass, &self.step_kernels[0], step, grid);
						apply(&mut pass, 0, &kernels[3]);
						force(&mut pass, &self.force_kernels[0], &self.force_groups[c], k);
						dispatch(&mut pass, &self.step_kernels[1 + intensity as usize], step, grid);
						apply(&mut pass, 1, &kernels[4]);
					}
				}
				if mur > 0 {
					dispatch(&mut pass, &self.edges_kernel, &self.edges_groups[c], (mur.div_ceil(LINEAR_GROUP), 1));
				}
				self.cur = 1 - self.cur;
			}
		}
		self.pending = Some(encoder);
		self.batch_steps += 1;
		self.time += 1;
		if self.batch_steps == BATCH_STEPS {
			self.flush()?;
		}
		Ok(())
	}

	fn read_probes(&mut self, out: &mut ProbeReadings) -> Result<(), GpuError> {
		let count = self.params.probe_count;
		if count == 0 {
			out.fill(&self.probe_lengths, &[]);
			return Ok(());
		}
		let mut readback = self.probe_readback.take().expect("probe readback");
		let bytes = count as u64 * 4;
		let pipeline = self.probes_kernel.pipeline.clone();
		let group = self.probes_groups[self.cur].clone();
		let (probe_out, target) = (self.probe_out.clone(), readback.buffer.clone());
		let index = self.submit(|encoder| {
			{
				let mut pass = encoder.begin_compute_pass(&Default::default());
				pass.set_pipeline(&pipeline);
				pass.set_bind_group(0, &group, &[]);
				pass.dispatch_workgroups(count.div_ceil(LINEAR_GROUP), 1, 1);
			}
			encoder.copy_buffer_to_buffer(&probe_out, 0, &target, 0, bytes);
		})?;
		map(&mut readback, index);
		wait_mapped(&self.ctx.device, &mut readback)?;
		{
			let view = readback.buffer.slice(..bytes).get_mapped_range().map_err(GpuError::MapRange)?;
			out.fill(&self.probe_lengths, bytemuck::cast_slice(&view));
		}
		readback.buffer.unmap();
		self.probe_readback = Some(readback);
		Ok(())
	}

	fn begin(&mut self, shape: &FrameShape, mut into: Snapshot) -> Result<PendingCapture, GpuError> {
		assert_eq!((shape.canvas_width, shape.canvas_height), (self.scene.canvas_width, self.scene.canvas_height));
		let slot = self.next_capture;
		assert!(self.captures[slot].as_ref().is_none_or(|r| r.pending.is_none()), "more than two captures pending");
		self.next_capture = 1 - slot;
		let (layers, words, layer_words) = capture_words(shape, self.options.track_intensity);
		let bytes = (layer_words * layers * 4) as u64;
		if self.capture_out.size() < bytes {
			self.flush()?;
			self.capture_out = storage(&self.ctx.device, "capture out", bytes);
			self.bind_capture();
		}
		let readback = self.captures[slot].take().filter(|r| r.buffer.size() >= bytes);
		let mut readback = readback.unwrap_or_else(|| readback_buffer(&self.ctx.device, "capture readback", bytes));
		let params = CaptureParams {
			out_w: shape.width(),
			divisor: shape.divisor,
			samples: shape.plane_samples() as u32,
			layer_words,
			format: match shape.format {
				SampleFormat::Half => 0,
				SampleFormat::Index => 1,
			},
			root: shape.intensity_root as u32,
			field_b: shape.field_brightness,
			intensity_b: shape.intensity_brightness,
		};
		self.ctx.queue.write_buffer(&self.capture_params, 0, bytemuck::bytes_of(&params));
		let groups = words.div_ceil(CAPTURE_GROUP);
		let max = self.ctx.limits.max_compute_workgroups_per_dimension;
		let (gx, gy) = (groups.min(max), groups.div_ceil(max));
		let pipeline = self.capture_kernel.pipeline.clone();
		let group = self.capture_groups[self.cur].clone();
		let out = self.capture_out.clone();
		let target = readback.buffer.clone();
		let submitted = self.submit(|encoder| {
			{
				let mut pass = encoder.begin_compute_pass(&Default::default());
				pass.set_pipeline(&pipeline);
				pass.set_bind_group(0, &group, &[]);
				pass.dispatch_workgroups(gx, gy, layers);
			}
			encoder.copy_buffer_to_buffer(&out, 0, &target, 0, bytes);
		});
		let submission = match submitted {
			Ok(s) => s,
			Err(e) => {
				self.captures[slot] = Some(readback);
				return Err(e);
			}
		};
		map(&mut readback, submission);
		self.captures[slot] = Some(readback);
		into.shape = *shape;
		into.step = self.time.saturating_sub(1);
		Ok(PendingCapture { snapshot: into, slot })
	}

	fn finish(&mut self, pending: PendingCapture) -> Result<Snapshot, GpuError> {
		let PendingCapture { mut snapshot, slot } = pending;
		let readback = self.captures[slot].as_mut().expect("capture slot");
		let waited = wait_mapped(&self.ctx.device, readback);
		if let Err(e) = waited {
			readback.pending = None;
			return Err(e);
		}
		let (layers, _, layer_words) = capture_words(&snapshot.shape, self.options.track_intensity);
		let (plane, layer_bytes) = (snapshot.shape.plane_bytes(), layer_words as usize * 4);
		let copied = readback.buffer.slice(..).get_mapped_range().map(|view| {
			snapshot.field.clear();
			snapshot.field.extend_from_slice(&view[..plane]);
			snapshot.intensity.clear();
			if layers == 2 {
				snapshot.intensity.extend_from_slice(&view[layer_bytes..layer_bytes + plane]);
			}
		});
		readback.buffer.unmap();
		copied.map_err(GpuError::MapRange)?;
		Ok(snapshot)
	}

	fn read_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), GpuError> {
		let n = (self.cells * 4) as u64;
		let (u, s1, s2) = (self.field[self.cur].clone(), self.s1.clone(), self.s2.clone());
		let bytes = self.read(&[(&u, n), (&s1, n), (&s2, n)])?;
		let all: &[f32] = bytemuck::cast_slice(&bytes);
		let (u, rest) = all.split_at(self.cells);
		let (s1, s2) = rest.split_at(self.cells);
		let (w, h, rim) =
			(self.scene.canvas_width as usize, self.scene.canvas_height as usize, self.scene.rim as usize);
		let scale = self.options.intensity_scale as f32;
		let mut field = Vec::with_capacity(w * h);
		let mut intensity = Vec::with_capacity(w * h);
		for y in 0..h {
			let at = self.cell_of_grid(rim, y + rim);
			field.extend_from_slice(&u[at..at + w]);
			intensity.extend((at..at + w).map(|c| blend(s1[c], s2[c], scale)));
		}
		Ok((field, intensity))
	}
}

impl Drop for GpuEngine {
	fn drop(&mut self) {
		let _ = self.flush();
	}
}

/// WGSL constants of the stencils, as the CPU engine's f32 values: the 9-point's weights, the axis
/// weights a1..a3 of `coefficients`, 1/12, and the cross weight (CHI, engine/pml.rs).
fn constants(coefficients: &StencilCoefficients) -> String {
	let a = coefficients.axis.map(|v| v as f32);
	[
		("K9_DIAGONAL", 1.0f32 / 6.0),
		("K9_AXIS", 2.0 / 3.0),
		("AX1", a[1]),
		("AX2", a[2]),
		("AX3", a[3]),
		("INV12", 1.0 / 12.0),
		("CHI", coefficients.cross as f32),
	]
	.iter()
	.map(|(name, v)| format!("const {name}: f32 = {v:?}f;\n"))
	.collect()
}

/// The stencil-tap helper of `taps.wgsl` for buffer `name`.
fn taps(name: &str) -> String {
	TAPS.replace("SRC", name)
}

fn module(
	device: &wgpu::Device,
	label: &str,
	coefficients: &StencilCoefficients,
	parts: &[&str],
) -> wgpu::ShaderModule {
	let source = std::iter::once(constants(coefficients))
		.chain(std::iter::once(COMMON.into()))
		.chain(parts.iter().map(|p| p.to_string()));
	device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some(label),
		source: wgpu::ShaderSource::Wgsl(Cow::Owned(source.collect::<Vec<_>>().join("\n"))),
	})
}

fn kernel_layout(device: &wgpu::Device, label: &str, slots: &[Slot]) -> wgpu::BindGroupLayout {
	let entries: Vec<_> = slots
		.iter()
		.enumerate()
		.map(|(i, slot)| wgpu::BindGroupLayoutEntry {
			binding: i as u32,
			visibility: wgpu::ShaderStages::COMPUTE,
			ty: wgpu::BindingType::Buffer {
				ty: match slot {
					Slot::Uniform => wgpu::BufferBindingType::Uniform,
					Slot::Read | Slot::ReadDynamic => wgpu::BufferBindingType::Storage { read_only: true },
					Slot::ReadWrite => wgpu::BufferBindingType::Storage { read_only: false },
				},
				has_dynamic_offset: matches!(slot, Slot::ReadDynamic),
				min_binding_size: None,
			},
			count: None,
		})
		.collect();
	device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries })
}

fn pipeline(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	module: &wgpu::ShaderModule,
	entry: &str,
	constants: &[(&str, f64)],
) -> wgpu::ComputePipeline {
	let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some(entry),
		bind_group_layouts: &[Some(layout)],
		immediate_size: 0,
	});
	device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
		label: Some(entry),
		layout: Some(&pipeline_layout),
		module,
		entry_point: Some(entry),
		compilation_options: wgpu::PipelineCompilationOptions { constants, ..Default::default() },
		cache: None,
	})
}

fn kernel(
	device: &wgpu::Device,
	entry: &str,
	source: &str,
	slots: &'static [Slot],
	coefficients: &StencilCoefficients,
) -> Kernel {
	let layout = kernel_layout(device, entry, slots);
	let module = module(device, entry, coefficients, &[source]);
	let pipeline = pipeline(device, &layout, &module, entry, &[]);
	Kernel { slots, layout, pipeline }
}

fn medium_accessor(medium: MediumLayout) -> &'static str {
	match medium {
		MediumLayout::Compact => MEDIUM_COMPACT,
		MediumLayout::Plain => MEDIUM_PLAIN,
	}
}

/// The grid passes of `scheme` for `medium`: `step`, `step_intensity` (leapfrog) or `inner`, `outer`,
/// `outer_intensity` (modified equation).
fn step_pipelines(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	coefficients: &StencilCoefficients,
	scheme: TimeScheme,
	medium: MediumLayout,
	cuts: bool,
) -> Vec<Kernel> {
	let (cur, w) = (taps("cur"), taps("wbuf"));
	let module = module(device, "step", coefficients, &[CUTS, &cur, &w, STEP, medium_accessor(medium)]);
	let constants = [("CUTS", if cuts { 1.0 } else { 0.0 })];
	let entries: &[&str] = match scheme {
		TimeScheme::Leapfrog => &["step", "step_intensity"],
		TimeScheme::ModifiedEquation => &["inner", "outer", "outer_intensity"],
	};
	entries
		.iter()
		.map(|entry| Kernel {
			slots: STEP_SLOTS,
			layout: layout.clone(),
			pipeline: pipeline(device, layout, &module, entry, &constants),
		})
		.collect()
}

/// The forcing kernels of `scheme` on one layout: `force_w`, or `force` and `force_intensity`.
fn force_pipelines(device: &wgpu::Device, coefficients: &StencilCoefficients, scheme: TimeScheme) -> Vec<Kernel> {
	let layout = kernel_layout(device, "force", FORCE_SLOTS);
	let module = module(device, "force", coefficients, &[FORCE]);
	let entries: &[&str] = match scheme {
		TimeScheme::Leapfrog => &["force", "force_intensity"],
		TimeScheme::ModifiedEquation => &["force_w"],
	};
	entries
		.iter()
		.map(|entry| Kernel {
			slots: FORCE_SLOTS,
			layout: layout.clone(),
			pipeline: pipeline(device, &layout, &module, entry, &[]),
		})
		.collect()
}

/// The CPML kernels for `medium`: `pml_psi_y`, `pml_delta_y`, `pml_psi_x`, `pml_correct` at scale 1
/// and at 1/12.
fn pml_pipelines(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	coefficients: &StencilCoefficients,
	medium: MediumLayout,
	cuts: bool,
) -> Vec<Kernel> {
	let module = module(device, "pml", coefficients, &[CUTS, &taps("src"), PML, medium_accessor(medium)]);
	let cuts = if cuts { 1.0 } else { 0.0 };
	let twelfth = (1.0f32 / 12.0) as f64;
	[("pml_psi_y", 1.0), ("pml_delta_y", 1.0), ("pml_psi_x", 1.0), ("pml_correct", 1.0), ("pml_correct", twelfth)]
		.iter()
		.map(|&(entry, scale)| Kernel {
			slots: PML_SLOTS,
			layout: layout.clone(),
			pipeline: pipeline(device, layout, &module, entry, &[("CUTS", cuts), ("SCALE", scale)]),
		})
		.collect()
}

/// Bind groups of `kernel` over small dummy buffers, replaced before first use.
fn placeholder(device: &wgpu::Device, kernel: &Kernel) -> [wgpu::BindGroup; 2] {
	let buffers: Vec<wgpu::Buffer> = kernel
		.slots
		.iter()
		.map(|slot| match slot {
			Slot::Uniform => device.create_buffer(&wgpu::BufferDescriptor {
				label: Some("placeholder"),
				size: 64,
				usage: wgpu::BufferUsages::UNIFORM,
				mapped_at_creation: false,
			}),
			_ => storage(device, "placeholder", 16),
		})
		.collect();
	let refs: Vec<&wgpu::Buffer> = buffers.iter().collect();
	[0, 1].map(|_| bind(device, &kernel.layout, &refs))
}

fn entry(binding: u32, buffer: &wgpu::Buffer, size: Option<NonZeroU64>) -> wgpu::BindGroupEntry<'_> {
	wgpu::BindGroupEntry {
		binding,
		resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size }),
	}
}

fn bind(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, buffers: &[&wgpu::Buffer]) -> wgpu::BindGroup {
	let entries: Vec<_> = buffers.iter().enumerate().map(|(i, b)| entry(i as u32, b, None)).collect();
	device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries })
}

fn storage(device: &wgpu::Device, label: &str, bytes: u64) -> wgpu::Buffer {
	device.create_buffer(&wgpu::BufferDescriptor {
		label: Some(label),
		size: bytes.max(16).next_multiple_of(4),
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
		mapped_at_creation: false,
	})
}

fn storage_init(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::Buffer {
	let size = (bytes.len() as u64).max(16).next_multiple_of(4);
	let buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some(label),
		size,
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
		mapped_at_creation: true,
	});
	let mut padded = bytes.to_vec();
	padded.resize(size as usize, 0);
	buffer.slice(..).get_mapped_range_mut().expect("mapped at creation").copy_from_slice(&padded);
	buffer.unmap();
	buffer
}

/// `values` with at least one element.
fn padded<T: Copy>(mut values: Vec<T>, fill: T) -> Vec<T> {
	if values.is_empty() {
		values.push(fill);
	}
	values
}

fn readback_buffer(device: &wgpu::Device, label: &str, bytes: u64) -> Readback {
	let buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some(label),
		size: bytes.next_multiple_of(4).max(4),
		usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	});
	Readback { buffer, mapped: MapState::default(), pending: None }
}

/// Requests the mapping of `readback` once `submission`, which fills it, is done.
fn map(readback: &mut Readback, submission: wgpu::SubmissionIndex) {
	let state = MapState::default();
	readback.mapped = state.clone();
	readback.pending = Some(submission);
	readback.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
		let _ = state.set(r);
	});
}

fn wait(device: &wgpu::Device, index: wgpu::SubmissionIndex) -> Result<(), GpuError> {
	device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: None })?;
	Ok(())
}

/// Waits for the mapping requested by [`map`]; the readback is no longer pending after it.
fn wait_mapped(device: &wgpu::Device, readback: &mut Readback) -> Result<(), GpuError> {
	if let Some(index) = readback.pending.take() {
		wait(device, index)?;
	}
	loop {
		if let Some(result) = readback.mapped.get() {
			return result.clone().map_err(GpuError::Map);
		}
		device.poll(wgpu::PollType::wait_indefinitely())?;
	}
}
