//! The sources on the device: the driven cells and the value tables of a batch's sub-steps.
//!
//! Values are computed on the host in f64 as on the CPU, one table per sub-step (engine/forcing.rs), a step's tables
//! uploaded together; the kernels only add a cell's values in the CPU's order.

use std::num::NonZeroU64;

use super::GpuContext;
use super::batch::BATCH_STEPS;
use super::buffers::{entry, padded, storage, storage_init};
use super::grid::Grid;
use super::kernels::{Kernel, LINEAR_GROUP};
use crate::engine::shared::Shared;
use crate::engine::stencil::{TimeScheme, centred};

pub(super) struct Forcing {
	/// d^(1/n) per padded cell, for the forcing's h.
	d: Vec<f32>,
	pub group_count: u32,
	/// The tables of a batch's sub-steps, `slot_bytes` apart.
	tables: wgpu::Buffer,
	slot_bytes: u64,
	/// Per parity of [`Grid::cur`], into `w` (modified equation) or the next field; they keep the groups' buffer.
	groups: [wgpu::BindGroup; 2],
	/// The tables of the step being recorded.
	values: Vec<f32>,
}

impl Forcing {
	/// The groups of `shared`'s sources (image sources first, then emitters, per distinct cell in order of first
	/// appearance) on a medium of sub-step damping `d`, and room for a batch's tables.
	pub(super) fn new(ctx: &GpuContext, kernel: &Kernel, grid: &Grid, shared: &Shared, d: Vec<f32>) -> Self {
		let device = &ctx.device;
		let groups = shared.groups();
		let mut words = Vec::with_capacity(4 * groups.cells.len() + groups.entries.len());
		for (&cell, &(start, len)) in groups.cells.iter().zip(&groups.runs) {
			words.extend([cell as u32, start, len, centred(d[cell]).to_bits()]);
		}
		words.extend_from_slice(&groups.entries);
		let cells = storage_init(device, "source groups", bytemuck::cast_slice(&padded(words, 0)));
		let align = ctx.limits.min_storage_buffer_offset_alignment as u64;
		let slot_bytes = ((shared.sources.table_len() * 4) as u64).next_multiple_of(align);
		let slots = (BATCH_STEPS * shared.sub_steps) as u64;
		let tables = storage(device, "step values", slot_bytes * slots);
		let modified = shared.scheme == TimeScheme::ModifiedEquation;
		let bound = [0, 1].map(|c| {
			let target = if modified { &grid.w } else { &grid.field[1 - c] };
			device.create_bind_group(&wgpu::BindGroupDescriptor {
				label: Some("force"),
				layout: &kernel.layout,
				entries: &[
					entry(0, &grid.params, None),
					entry(1, &cells, None),
					entry(2, &tables, NonZeroU64::new(slot_bytes)),
					entry(3, target, None),
					entry(4, &grid.s1, None),
					entry(5, &grid.s2, None),
				],
			})
		});
		Self { d, group_count: groups.cells.len() as u32, tables, slot_bytes, groups: bound, values: Vec::new() }
	}

	/// Groups `shared`'s sources again, on the same medium.
	pub(super) fn regroup(&mut self, ctx: &GpuContext, kernel: &Kernel, grid: &Grid, shared: &Shared) {
		*self = Self::new(ctx, kernel, grid, shared, std::mem::take(&mut self.d));
	}

	/// Computes the tables of the step `shared` takes next and uploads them behind those of the `recorded`
	/// steps of the batch; returns where the step's first table starts.
	pub(super) fn upload(&mut self, queue: &wgpu::Queue, shared: &mut Shared, recorded: u32) -> u64 {
		let slot = self.slot_bytes as usize / 4;
		self.values.clear();
		self.values.resize(slot * shared.sub_steps as usize, 0.0);
		for (k, values) in self.values.chunks_exact_mut(slot).enumerate() {
			shared.source_values(values, k as u32);
		}
		let first = (recorded * shared.sub_steps) as u64 * self.slot_bytes;
		queue.write_buffer(&self.tables, first, bytemuck::cast_slice(&self.values));
		first
	}

	/// Records `pipeline` over the driven cells, if any, with the table of sub-step `k` of the step whose
	/// tables start at `first`, for parity `c`.
	pub(super) fn record(
		&self,
		pass: &mut wgpu::ComputePass<'_>,
		pipeline: &wgpu::ComputePipeline,
		c: usize,
		first: u64,
		k: u32,
	) {
		if self.group_count > 0 {
			pass.set_pipeline(pipeline);
			pass.set_bind_group(0, &self.groups[c], &[(first + k as u64 * self.slot_bytes) as u32]);
			pass.dispatch_workgroups(self.group_count.div_ceil(LINEAR_GROUP), 1, 1);
		}
	}
}
