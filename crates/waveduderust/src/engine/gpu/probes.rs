//! The probes on the device and their readings on the way to the host (the batching is in the docs of `gpu`).

use std::collections::VecDeque;

use super::batch::{BATCH_STEPS, Batch};
use super::buffers::{bind, storage, storage_init};
use super::grid::Grid;
use super::kernels::{Kernel, LINEAR_GROUP};
use super::readback::Readback;
use super::{GpuContext, GpuError};
use crate::engine::types::{PendingProbes, ProbeCells, ProbeQueue, ProbeReadings, SceneData};
use crate::model::Layer;

/// Bit of a probe cell word selecting the blended intensity.
pub(super) const PROBE_INTENSITY: u32 = 1 << 31;

/// What reads the probe cells, while there are any.
struct Gather {
	/// Cells read, at least one.
	count: u32,
	out: wgpu::Buffer,
	/// [`BATCH_STEPS`] readings of `out`, queued on the device.
	batch: wgpu::Buffer,
	/// Per parity of [`Grid::cur`]; they keep the cells' buffer.
	groups: [wgpu::BindGroup; 2],
}

impl Gather {
	/// Bytes of one reading.
	fn bytes(&self) -> u64 {
		u64::from(self.count) * 4
	}

	/// Records the kernel on the field of parity `cur`.
	fn record(&self, encoder: &mut wgpu::CommandEncoder, kernel: &Kernel, cur: usize) {
		let mut pass = encoder.begin_compute_pass(&Default::default());
		pass.set_pipeline(kernel.pipeline());
		pass.set_bind_group(0, &self.groups[cur], &[]);
		pass.dispatch_workgroups(self.count.div_ceil(LINEAR_GROUP), 1, 1);
	}

	fn readback(&self, device: &wgpu::Device) -> Readback {
		Readback::new(device, "probe readback", self.bytes() * u64::from(BATCH_STEPS))
	}
}

#[derive(Default)]
pub(super) struct Probes {
	gather: Option<Gather>,
	/// Readings in the batch buffer since it was last full.
	slots: u32,
	/// Of `slots`, those whose copy to a readback is recorded.
	copied: u32,
	/// Free readback buffers of a full batch, for the batch buffer or `out` alone.
	pool: Vec<Readback>,
	/// Batches copied for readback, oldest first, with their reading counts; the last, while its copies are
	/// recorded, takes the copies of later readings it has room for.
	reads: VecDeque<(Readback, u32)>,
	queue: ProbeQueue,
}

impl Probes {
	/// Cells read a step.
	pub(super) fn count(&self) -> u32 {
		self.gather.as_ref().map_or(0, |g| g.count)
	}

	/// Reads `probes` on `scene`'s canvas from now on; the readings queued are dropped.
	pub(super) fn set(
		&mut self,
		device: &wgpu::Device,
		kernel: &Kernel,
		grid: &Grid,
		scene: &SceneData,
		probes: &[ProbeCells],
	) {
		// Below PROBE_INTENSITY: the grid size is checked at build.
		let words: Vec<u32> = probes
			.iter()
			.flat_map(|p| {
				let flag = if p.layer == Layer::Intensity { PROBE_INTENSITY } else { 0 };
				p.cells.iter().map(move |&(x, y)| scene.canvas_index(x as usize, y as usize) as u32 | flag)
			})
			.collect();
		self.drop_readings();
		self.queue.set_lengths(probes.iter().map(|p| p.cells.len()).collect());
		self.pool.clear();
		self.gather = (!words.is_empty()).then(|| {
			let bytes = (words.len() * 4) as u64;
			let cells = storage_init(device, "probe cells", bytemuck::cast_slice(&words));
			let out = storage(device, "probe out", bytes);
			let groups = [0, 1].map(|c| {
				let buffers = [&grid.params, &grid.field[c], &grid.s1, &grid.s2, &cells, &out];
				bind(device, &kernel.layout, &buffers)
			});
			let batch = storage(device, "probe batch", bytes * u64::from(BATCH_STEPS));
			Gather { count: words.len() as u32, out, batch, groups }
		});
		self.pool.extend(self.gather.as_ref().map(|g| g.readback(device)));
	}

	/// Drops the queued readings, those on the device included.
	pub(super) fn drop_readings(&mut self) {
		let in_flight: u64 = self.reads.drain(..).map(|(_, steps)| u64::from(steps)).sum();
		self.queue.skip(in_flight + u64::from(self.slots - self.copied));
		(self.slots, self.copied) = (0, 0);
	}

	/// Queues a reading of the field of parity `cur`: records the kernel and the copy of its output into the
	/// batch's next slot, behind the commands recorded. Whether the batch is full with it.
	pub(super) fn queue_reading(
		&mut self,
		device: &wgpu::Device,
		batch: &mut Batch,
		kernel: &Kernel,
		cur: usize,
	) -> bool {
		let Some(gather) = &self.gather else {
			self.queue.push(&[]);
			return false;
		};
		let encoder = batch.encoder(device);
		gather.record(encoder, kernel, cur);
		let bytes = gather.bytes();
		encoder.copy_buffer_to_buffer(&gather.out, 0, &gather.batch, u64::from(self.slots) * bytes, bytes);
		self.slots += 1;
		self.slots == BATCH_STEPS
	}

	/// Records the copy of the readings in the batch buffer not copied yet behind those of the last batch,
	/// while it is recorded and has room, else into a new one; the submission carrying it requests the mapping.
	/// The commands recorded later, which reuse the batch buffer once it is full, run after the copy.
	pub(super) fn record_copy(&mut self, device: &wgpu::Device, batch: &mut Batch) {
		let new = self.slots - self.copied;
		let Some(gather) = self.gather.as_ref().filter(|_| new > 0) else { return };
		let open = self.reads.back().is_some_and(|(r, steps)| r.is_recorded() && steps + new <= BATCH_STEPS);
		if !open {
			let mut readback = self.pool.pop().unwrap_or_else(|| gather.readback(device));
			readback.record();
			self.reads.push_back((readback, 0));
		}
		let bytes = gather.bytes();
		let (readback, steps) = self.reads.back_mut().expect("a batch to copy into");
		batch.encoder(device).copy_buffer_to_buffer(
			&gather.batch,
			u64::from(self.copied) * bytes,
			&readback.buffer,
			u64::from(*steps) * bytes,
			u64::from(new) * bytes,
		);
		*steps += new;
		self.copied = self.slots;
		if self.slots == BATCH_STEPS {
			(self.slots, self.copied) = (0, 0);
		}
	}

	/// Requests the mappings of the batches whose copies `submission` carries.
	pub(super) fn submitted(&mut self, ctx: &GpuContext, submission: &wgpu::SubmissionIndex) {
		for (readback, _) in self.reads.iter_mut().filter(|(r, _)| r.is_recorded()) {
			readback.submitted(ctx, submission.clone());
		}
	}

	/// The token of the readings queued so far: those on the host and each batch on its way.
	pub(super) fn pending(&self, ctx: &GpuContext) -> PendingProbes {
		let on_host = self.queue.end();
		let batches = self
			.reads
			.iter()
			.scan(on_host, |end, (r, steps)| {
				*end += u64::from(*steps);
				Some((r.readiness(ctx), *end))
			})
			.collect();
		PendingProbes { batches, on_host }
	}

	/// Whether the oldest batch read back is to be collected for readings below `until`: one mapped already,
	/// or one holding such readings; `Some(true)` when its copies are still to be submitted.
	pub(super) fn next_batch(&self, until: u64) -> Option<bool> {
		let (readback, _) = self.reads.front()?;
		(self.queue.end() < until || readback.is_mapped()).then(|| readback.is_recorded())
	}

	/// Moves the oldest batch read back, submitted, into the host queue, waiting for it.
	pub(super) fn collect(&mut self, device: &wgpu::Device) -> Result<(), GpuError> {
		let (readback, steps) = self.reads.front_mut().expect("the batch to read");
		readback.wait(device)?;
		{
			let bytes = u64::from(*steps) * u64::from(self.gather.as_ref().map_or(0, |g| g.count)) * 4;
			let view = readback.buffer.slice(..bytes).get_mapped_range().map_err(GpuError::MapRange)?;
			let values: &[f32] = bytemuck::cast_slice(&view);
			for step in values.chunks_exact(self.queue.width()) {
				self.queue.push(step);
			}
		}
		readback.buffer.unmap();
		let (readback, _) = self.reads.pop_front().expect("the batch just read");
		self.pool.push(readback);
		Ok(())
	}

	/// Records a reading of the field of parity `cur` and its copy to a readback, which it returns for the
	/// submission to fill; `None` without probes.
	pub(super) fn record_sample(
		&mut self,
		device: &wgpu::Device,
		batch: &mut Batch,
		kernel: &Kernel,
		cur: usize,
	) -> Option<Readback> {
		let gather = self.gather.as_ref()?;
		let readback = self.pool.pop().unwrap_or_else(|| gather.readback(device));
		let encoder = batch.encoder(device);
		gather.record(encoder, kernel, cur);
		encoder.copy_buffer_to_buffer(&gather.out, 0, &readback.buffer, 0, gather.bytes());
		Some(readback)
	}

	/// The reading in `readback`, mapped, as `out`; the buffer goes back to the pool. Without probes `out` is
	/// the empty reading.
	pub(super) fn sample(&mut self, readback: Option<Readback>, out: &mut ProbeReadings) -> Result<(), GpuError> {
		let Some(readback) = readback else {
			out.fill(self.queue.lengths(), &[]);
			return Ok(());
		};
		{
			let bytes = u64::from(self.count()) * 4;
			let view = readback.buffer.slice(..bytes).get_mapped_range().map_err(GpuError::MapRange)?;
			out.fill(self.queue.lengths(), bytemuck::cast_slice(&view));
		}
		readback.buffer.unmap();
		self.pool.push(readback);
		Ok(())
	}

	/// Hands out the readings on the host numbered below `end`.
	pub(super) fn drain(&mut self, end: u64, f: &mut dyn FnMut(&ProbeReadings)) {
		self.queue.drain(end, f);
	}
}
