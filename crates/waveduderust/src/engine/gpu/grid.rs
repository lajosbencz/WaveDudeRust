//! The buffers of the padded grid and the bind groups of the kernels that step it.

use bytemuck::{Pod, Zeroable};

use super::buffers::{bind, padded, storage, storage_init, uniform};
use super::kernels::Kernels;
use super::medium::Medium;
use super::probes::PROBE_INTENSITY;
use super::{GpuError, check_buffer};
use crate::engine::mur::mur_chains;
use crate::engine::pml::{Strips, axis_table};
use crate::engine::shared::Shared;
use crate::engine::stencil::{PAD, TimeScheme};
use crate::engine::types::{EngineOptions, SceneData};

/// Uniform shared by every kernel; mirrors `Params` in common.wgsl (80 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(super) struct Params {
	pub stride: u32,
	pub gw: u32,
	pub gh: u32,
	pub rim: u32,
	pub canvas_w: u32,
	pub canvas_h: u32,
	pub mur_count: u32,
	pub probe_count: u32,
	pub alpha: f32,
	pub rest: f32,
	pub intensity_scale: f32,
	pub d_base: u32,
	pub group_count: u32,
	pub pml_x_len: u32,
	pub pml_len: u32,
	pub rim_cells: u32,
	_pad: [u32; 4],
}

impl Params {
	/// The parameters of `scene`'s grid; what depends on the medium, the sources and the probes stays 0.
	pub(super) fn of(scene: &SceneData, options: &EngineOptions) -> Self {
		let strips = Strips::of(scene);
		Self {
			stride: scene.stride() as u32,
			gw: scene.grid_width(),
			gh: scene.grid_height(),
			rim: scene.rim,
			canvas_w: scene.canvas_width,
			canvas_h: scene.canvas_height,
			intensity_scale: options.intensity_scale as f32,
			pml_x_len: strips.x_len() as u32,
			pml_len: strips.len() as u32,
			rim_cells: strips.rim_cells() as u32,
			..Self::default()
		}
	}
}

/// One Mur chain; mirrors `Mur` in edges.wgsl (28 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MurChain {
	inner: u32,
	rings: [u32; PAD],
	weights: [f32; 3],
}

/// Cell indices are u32 words, probe cells carry the layer in bit 31; the field's and the CPML's buffers bind whole.
pub(super) fn check_grid(scene: &SceneData, limits: &wgpu::Limits) -> Result<(), GpuError> {
	let cells = scene.padded_cells();
	if cells >= PROBE_INTENSITY as usize {
		return Err(GpuError::GridTooLarge(cells));
	}
	let strips = Strips::of(scene);
	let words = cells.max(strips.len() + strips.y_len());
	check_buffer(words as u64 * 4, limits)
}

/// The buffers an engine keeps for life: the parameters, and over the padded grid the two fields, swapped by
/// bind group, the modified equation's `w` and the intensity stages; on a rim the CPML's memory.
pub(super) struct Grid {
	/// The uniform of [`Params`].
	pub params: wgpu::Buffer,
	pub field: [wgpu::Buffer; 2],
	/// Index into `field` of the buffer holding the current field.
	pub cur: usize,
	/// 16 bytes for leapfrog, which never reads it: the grid passes of both schemes are entry points of one
	/// module on one layout, which binds it.
	pub w: wgpu::Buffer,
	/// Allocated whether the intensity is tracked or not.
	pub s1: wgpu::Buffer,
	pub s2: wgpu::Buffer,
	/// `psi` (the strips) and `xi` (the strips, then `delta_y` over the y strips) of each application of the
	/// operator in a sub-step; none without a rim.
	memory: Vec<(wgpu::Buffer, wgpu::Buffer)>,
}

impl Grid {
	pub(super) fn new(device: &wgpu::Device, scene: &SceneData, scheme: TimeScheme) -> Self {
		let cells = scene.padded_cells() as u64;
		let grid = |label| storage(device, label, cells * 4);
		let modified = scheme == TimeScheme::ModifiedEquation;
		let strips = Strips::of(scene);
		let memory = |label, cells: usize| storage(device, label, cells as u64 * 4);
		let sets: &[(&str, &str)] = match (strips.is_empty(), modified) {
			(true, _) => &[],
			(false, false) => &[("psi 0", "xi 0")],
			(false, true) => &[("psi 0", "xi 0"), ("psi 1", "xi 1")],
		};
		Self {
			params: uniform(device, "params", size_of::<Params>() as u64),
			field: [grid("field a"), grid("field b")],
			cur: 0,
			w: if modified { grid("w") } else { storage(device, "w", 16) },
			s1: grid("s1"),
			s2: grid("s2"),
			memory: sets
				.iter()
				.map(|&(psi, xi)| (memory(psi, strips.len()), memory(xi, strips.len() + strips.y_len())))
				.collect(),
		}
	}

	/// The buffer holding the current field.
	pub(super) fn current(&self) -> &wgpu::Buffer {
		&self.field[self.cur]
	}

	/// Records the clearing of the intensity stages.
	pub(super) fn clear_stages(&self, encoder: &mut wgpu::CommandEncoder) {
		for stage in [&self.s1, &self.s2] {
			encoder.clear_buffer(stage, 0, None);
		}
	}

	/// Records the clearing of the fields, `w`, the stages and the CPML's memory.
	pub(super) fn clear(&self, encoder: &mut wgpu::CommandEncoder) {
		let memory = self.memory.iter().flat_map(|(psi, xi)| [psi, xi]);
		for buffer in self.field.iter().chain([&self.w, &self.s1, &self.s2]).chain(memory) {
			encoder.clear_buffer(buffer, 0, None);
		}
	}
}

/// The CPML's bind groups of one application of the operator: of the memory stages (`pml_psi_y`, `pml_delta_y`,
/// `pml_psi_x`) and of the correction.
pub(super) struct PmlPass {
	pub memory: wgpu::BindGroup,
	pub correct: wgpu::BindGroup,
}

/// The bind groups of the kernels that step the grid over one scene, each per parity of [`Grid::cur`]. They
/// keep the scene's buffers: the medium (indices into its tables, medium.rs), the cut bytes (four to a
/// word), the CPML's coefficient tables (x then y) and the Mur chains.
pub(super) struct Passes {
	pub step: [wgpu::BindGroup; 2],
	/// Per application of the operator in a sub-step; none without a rim.
	pub pml: [Vec<PmlPass>; 2],
	/// With Mur chains (open edges without a rim).
	pub edges: Option<[wgpu::BindGroup; 2]>,
	pub mur_count: u32,
}

impl Passes {
	/// Uploads `medium`, the Mur chains and the CPML's coefficients of `shared`'s scene, and binds them with `grid`.
	pub(super) fn new(device: &wgpu::Device, kernels: &Kernels, grid: &Grid, shared: &Shared, medium: &Medium) -> Self {
		let scene = &*shared.scene;
		let cells = storage_init(device, "medium", bytemuck::cast_slice(&medium.cells));
		let tables = storage_init(device, "tables", bytemuck::cast_slice(&medium.tables));
		let cuts = storage_init(device, "cuts", &medium.cuts);
		let (p, field, w) = (&grid.params, &grid.field, &grid.w);
		let step = [0, 1].map(|c| {
			let buffers = [p, &field[c], &field[1 - c], &grid.s1, &grid.s2, &cells, &tables, w, &cuts];
			bind(device, &kernels.step.layout, &buffers)
		});
		let dt = 1.0 / shared.sub_steps as f64;
		let strips = Strips::of(scene);
		let coef: Vec<[f32; 4]> =
			axis_table(&scene.pml, strips.gw, dt).into_iter().chain(axis_table(&scene.pml, strips.gh, dt)).collect();
		let coef = storage_init(device, "pml coefficients", bytemuck::cast_slice(&coef));
		let modified = shared.scheme == TimeScheme::ModifiedEquation;
		let pml = [0, 1].map(|c| {
			let (cur, nxt) = (&field[c], &field[1 - c]);
			let applications = grid.memory.iter().enumerate().map(|(set, (psi, xi))| {
				// the memory stages bind `nxt` unread
				let (src, out) = match (set, modified) {
					(0, true) => (cur, w),
					(0, false) => (cur, nxt),
					_ => (w, nxt),
				};
				let group =
					|out| bind(device, &kernels.pml.layout, &[p, src, out, psi, xi, &cells, &tables, &coef, &cuts]);
				PmlPass { memory: group(nxt), correct: group(out) }
			});
			applications.collect()
		});
		let mur: Vec<MurChain> = mur_chains(scene, &medium.c2, &medium.d)
			.into_iter()
			.map(|m| MurChain { inner: m.inner as u32, rings: m.rings.map(|r| r as u32), weights: m.weights })
			.collect();
		let mur_count = mur.len() as u32;
		let edges = (mur_count > 0).then(|| {
			let chains = storage_init(device, "mur", bytemuck::cast_slice(&padded(mur, MurChain::zeroed())));
			[0, 1].map(|c| bind(device, &kernels.edges.layout, &[p, &field[c], &field[1 - c], &chains]))
		});
		Self { step, pml, edges, mur_count }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Scene;

	#[test]
	fn a_grid_fits_up_to_the_smaller_of_the_binding_and_buffer_limits() {
		let limits = |binding, buffer| wgpu::Limits {
			max_storage_buffer_binding_size: binding,
			max_buffer_size: buffer,
			..Default::default()
		};
		let too_large = |r: Result<(), GpuError>| match r {
			Err(GpuError::BufferTooLarge { bytes, limit }) => Some((bytes, limit)),
			_ => None,
		};
		let open = SceneData::blank(64, 48, &Scene { damping_edge: 0, ..Scene::default() });
		let field = open.padded_cells() as u64 * 4;
		assert!(check_grid(&open, &limits(field, 2 * field)).is_ok());
		assert_eq!(too_large(check_grid(&open, &limits(field - 4, 2 * field))), Some((field, field - 4)));
		assert_eq!(too_large(check_grid(&open, &limits(2 * field, field - 4))), Some((field, field - 4)));
		// a rim wider than the canvas: the CPML's auxiliary buffer outgrows the field
		let rimmed = SceneData::blank(8, 8, &Scene { damping_edge: -64, ..Scene::default() });
		let field = rimmed.padded_cells() as u64 * 4;
		let largest = too_large(check_grid(&rimmed, &limits(field, field))).map(|(bytes, _)| bytes);
		assert!(largest.is_some_and(|b| b > field), "{largest:?} against {field}");
	}
}
