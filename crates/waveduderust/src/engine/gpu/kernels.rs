//! The engine's compute pipelines: the WGSL under `shaders/` assembled into modules, their bind group layouts and
//! entry points.
//!
//! A module is the generated stencil constants, `common.wgsl`, then its parts. WGSL has no overloads and naga passes
//! no storage pointers to functions, so a helper over a buffer (`taps.wgsl`, `drp.wgsl`) is written for buffer `SRC`
//! and instantiated per buffer name.

use std::borrow::Cow;

use crate::engine::stencil::{INV12, K9_AXIS, K9_DIAGONAL, StencilCoefficients, TimeScheme};

/// Work group sizes, as the shaders declare them: of the grid passes, of the kernels over a list, and of the
/// capture.
pub(super) const GRID_GROUP: (u32, u32) = (32, 8);
pub(super) const LINEAR_GROUP: u32 = 64;
pub(super) const CAPTURE_GROUP: u32 = 256;

const COMMON: &str = include_str!("shaders/common.wgsl");
const STEP: &str = include_str!("shaders/step.wgsl");
const TAPS: &str = include_str!("shaders/taps.wgsl");
const DRP: &str = include_str!("shaders/drp.wgsl");
const CUTS: &str = include_str!("shaders/cuts.wgsl");
const PML: &str = include_str!("shaders/pml.wgsl");
const MEDIUM: &str = include_str!("shaders/medium.wgsl");
const EDGES: &str = include_str!("shaders/edges.wgsl");
const FORCE: &str = include_str!("shaders/force.wgsl");
const PROBES: &str = include_str!("shaders/probes.wgsl");
const CAPTURE: &str = include_str!("shaders/capture.wgsl");
/// f16 conversions and the correctly rounded square root, for the capture and the colouring of planes.
pub(crate) const HALF: &str = include_str!("shaders/half.wgsl");

/// Binding kinds of a kernel's bind group, in binding order.
#[derive(Clone, Copy)]
enum Slot {
	Uniform,
	/// Uniform with a dynamic offset.
	UniformDynamic,
	Read,
	ReadWrite,
	/// Read-only with a dynamic offset.
	ReadDynamic,
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
const CAPTURE_SLOTS: &[Slot] =
	&[Slot::Uniform, Slot::UniformDynamic, Slot::Read, Slot::Read, Slot::Read, Slot::ReadWrite];

/// The pipelines of one module's entry points and the layout of the bind group they share.
pub(super) struct Kernel {
	pub layout: wgpu::BindGroupLayout,
	pub pipelines: Vec<wgpu::ComputePipeline>,
}

impl Kernel {
	/// The pipelines of `entries` of `module` on `layout`, each with the override `constants`.
	fn new(
		device: &wgpu::Device,
		layout: wgpu::BindGroupLayout,
		module: &wgpu::ShaderModule,
		entries: &[&str],
		constants: impl Fn(usize) -> Vec<(&'static str, f64)>,
	) -> Self {
		let pipelines = entries
			.iter()
			.enumerate()
			.map(|(i, entry)| pipeline(device, &layout, module, entry, &constants(i)))
			.collect();
		Self { layout, pipelines }
	}

	/// A kernel of one entry point, `entry` of `source`.
	fn single(
		device: &wgpu::Device,
		entry: &str,
		source: &str,
		slots: &[Slot],
		coefficients: &StencilCoefficients,
	) -> Self {
		let module = module(device, entry, coefficients, &[source]);
		Self::new(device, kernel_layout(device, entry, slots), &module, &[entry], |_| Vec::new())
	}

	/// The pipeline of a kernel of one entry point.
	pub(super) fn pipeline(&self) -> &wgpu::ComputePipeline {
		&self.pipelines[0]
	}
}

/// Every kernel of an engine.
pub(super) struct Kernels {
	coefficients: StencilCoefficients,
	scheme: TimeScheme,
	/// The scene has mirrors: the grid and CPML kernels apply the cuts.
	cuts: bool,
	/// step, `step_intensity` (leapfrog) or inner, outer, `outer_intensity` (modified equation).
	pub step: Kernel,
	/// `pml_psi_y`, `pml_delta_y`, `pml_psi_x`, `pml_correct` at scale 1, `pml_correct` at scale 1/12.
	pub pml: Kernel,
	pub edges: Kernel,
	/// `force_w` (modified equation) or force and `force_intensity` (leapfrog).
	pub force: Kernel,
	pub probes: Kernel,
	pub capture: Kernel,
}

impl Kernels {
	pub(super) fn new(
		device: &wgpu::Device,
		coefficients: StencilCoefficients,
		scheme: TimeScheme,
		cuts: bool,
	) -> Self {
		let c = &coefficients;
		Self {
			step: step_kernel(device, kernel_layout(device, "step", STEP_SLOTS), c, scheme, cuts),
			pml: pml_kernel(device, kernel_layout(device, "pml", PML_SLOTS), c, cuts),
			edges: Kernel::single(device, "edges", EDGES, EDGES_SLOTS, c),
			force: force_kernel(device, c, scheme),
			probes: Kernel::single(device, "probes", PROBES, PROBES_SLOTS, c),
			capture: Kernel::single(device, "capture", &format!("{HALF}\n{CAPTURE}"), CAPTURE_SLOTS, c),
			coefficients,
			scheme,
			cuts,
		}
	}

	/// Compiles the cuts in or out when `cuts` changes; the layouts stay, and with them the bind groups made
	/// on them.
	pub(super) fn set_cuts(&mut self, device: &wgpu::Device, cuts: bool) {
		if cuts != self.cuts {
			self.step = step_kernel(device, self.step.layout.clone(), &self.coefficients, self.scheme, cuts);
			self.pml = pml_kernel(device, self.pml.layout.clone(), &self.coefficients, cuts);
			self.cuts = cuts;
		}
	}

	pub(super) fn cuts(&self) -> bool {
		self.cuts
	}
}

/// WGSL constants of the stencils, as the CPU engine's f32 values: the 9-point's weights, the axis
/// weights a1..a3 of `coefficients`, 1/12, and the cross weight (CHI, engine/pml.rs).
fn constants(coefficients: &StencilCoefficients) -> String {
	let a = coefficients.axis.map(|v| v as f32);
	[
		("K9_DIAGONAL", K9_DIAGONAL),
		("K9_AXIS", K9_AXIS),
		("AX1", a[1]),
		("AX2", a[2]),
		("AX3", a[3]),
		("INV12", INV12),
		("CHI", coefficients.cross as f32),
	]
	.iter()
	.map(|(name, v)| format!("const {name}: f32 = {v:?}f;\n"))
	.collect()
}

/// `template`, a helper written for buffer `SRC`, for buffer `name`.
fn instance(template: &str, name: &str) -> String {
	template.replace("SRC", name)
}

fn module(
	device: &wgpu::Device,
	label: &str,
	coefficients: &StencilCoefficients,
	parts: &[&str],
) -> wgpu::ShaderModule {
	let source = std::iter::once(constants(coefficients))
		.chain(std::iter::once(COMMON.into()))
		.chain(parts.iter().map(ToString::to_string));
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
					Slot::Uniform | Slot::UniformDynamic => wgpu::BufferBindingType::Uniform,
					Slot::Read | Slot::ReadDynamic => wgpu::BufferBindingType::Storage { read_only: true },
					Slot::ReadWrite => wgpu::BufferBindingType::Storage { read_only: false },
				},
				has_dynamic_offset: matches!(slot, Slot::ReadDynamic | Slot::UniformDynamic),
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

fn flag(on: bool) -> f64 {
	if on { 1.0 } else { 0.0 }
}

/// The grid passes of `scheme`: `step`, `step_intensity` (leapfrog) or `inner`, `outer`,
/// `outer_intensity` (modified equation).
fn step_kernel(
	device: &wgpu::Device,
	layout: wgpu::BindGroupLayout,
	coefficients: &StencilCoefficients,
	scheme: TimeScheme,
	cuts: bool,
) -> Kernel {
	let helpers = ["cur", "wbuf"].map(|buffer| instance(TAPS, buffer) + &instance(DRP, buffer));
	let module = module(device, "step", coefficients, &[CUTS, &helpers[0], &helpers[1], STEP, MEDIUM]);
	let entries: &[&str] = match scheme {
		TimeScheme::Leapfrog => &["step", "step_intensity"],
		TimeScheme::ModifiedEquation => &["inner", "outer", "outer_intensity"],
	};
	Kernel::new(device, layout, &module, entries, |_| vec![("CUTS", flag(cuts))])
}

/// The forcing kernels of `scheme`: `force_w`, or `force` and `force_intensity`.
fn force_kernel(device: &wgpu::Device, coefficients: &StencilCoefficients, scheme: TimeScheme) -> Kernel {
	let module = module(device, "force", coefficients, &[FORCE]);
	let entries: &[&str] = match scheme {
		TimeScheme::Leapfrog => &["force", "force_intensity"],
		TimeScheme::ModifiedEquation => &["force_w"],
	};
	Kernel::new(device, kernel_layout(device, "force", FORCE_SLOTS), &module, entries, |_| Vec::new())
}

/// The CPML kernels: `pml_psi_y`, `pml_delta_y`, `pml_psi_x`, `pml_correct` at scale 1 and at 1/12.
fn pml_kernel(
	device: &wgpu::Device,
	layout: wgpu::BindGroupLayout,
	coefficients: &StencilCoefficients,
	cuts: bool,
) -> Kernel {
	let module = module(device, "pml", coefficients, &[CUTS, &instance(TAPS, "src"), PML, MEDIUM]);
	let entries = ["pml_psi_y", "pml_delta_y", "pml_psi_x", "pml_correct", "pml_correct"];
	let scales = [1.0, 1.0, 1.0, 1.0, f64::from(INV12)];
	Kernel::new(device, layout, &module, &entries, |i| vec![("CUTS", flag(cuts)), ("SCALE", scales[i])])
}
