//! Captures of the canvas on the device and their staging buffers on the way to the host (the batching is in
//! the docs of `gpu`).

use std::num::NonZeroU64;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use super::batch::Batch;
use super::buffers::{entry, storage, uniform};
use super::frames::{self, DeviceFrames};
use super::grid::Grid;
use super::kernels::{CAPTURE_GROUP, Kernel};
use super::readback::Readback;
use super::{GpuContext, GpuError};
use crate::engine::types::{FrameShape, SampleFormat, Snapshot};
use crate::sync::Readiness;

/// Captures pending at most. Each holds a staging buffer of its frame, and a device frame once those are asked
/// for; in the browser a readback maps only after the animation frame that began it, so the run steps on while
/// they map.
const MAX_CAPTURES: usize = 16;
/// The staging bytes of the captures pending (as much again in device frames, once asked for): as many captures
/// as fit, from two to [`MAX_CAPTURES`]; 64 MiB is 16 frames at the Low preview of full HD, 8 at High, 2 of 4K.
const CAPTURE_RING_BYTES: u64 = 64 << 20;

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

/// Word layout of a capture (capture.wgsl): its layers, the words of a plane's samples and the words a layer
/// takes.
struct Words {
	layers: u32,
	samples: u32,
	layer: u32,
}

impl Words {
	fn of(shape: &FrameShape, track_intensity: bool) -> Self {
		let samples = frames::plane_words(shape);
		Self { layers: if track_intensity { 2 } else { 1 }, samples, layer: samples.next_multiple_of(64).max(64) }
	}

	fn bytes(&self) -> u64 {
		u64::from(self.layer * self.layers * 4)
	}
}

/// How many captures of `shape` may be pending at once.
pub(super) fn capacity(shape: &FrameShape, track_intensity: bool) -> usize {
	let bytes = Words::of(shape, track_intensity).bytes();
	usize::try_from(CAPTURE_RING_BYTES / bytes).unwrap_or(MAX_CAPTURES).clamp(2, MAX_CAPTURES)
}

pub(super) struct Captures {
	/// [`CaptureParams`] by capture slot, `stride` apart: the captures of one submission read their own.
	params: wgpu::Buffer,
	stride: u64,
	/// The kernel's output and its bind groups per parity of [`Grid::cur`], from the first capture on.
	out: Option<(wgpu::Buffer, [wgpu::BindGroup; 2])>,
	/// Staging buffers of the captures, by slot; a slot is free once its capture was finished.
	ring: Vec<Readback>,
	/// Where captures keep their planes on the device, once asked for.
	frames: Option<Arc<DeviceFrames>>,
}

impl Captures {
	pub(super) fn new(ctx: &GpuContext) -> Self {
		let stride = (size_of::<CaptureParams>() as u64)
			.next_multiple_of(u64::from(ctx.limits.min_uniform_buffer_offset_alignment));
		let params = uniform(&ctx.device, "capture params", stride * MAX_CAPTURES as u64);
		Self { params, stride, out: None, ring: Vec::new(), frames: None }
	}

	/// Whether the output buffer holds a capture of `shape`.
	pub(super) fn fits(&self, shape: &FrameShape, track_intensity: bool) -> bool {
		let bytes = Words::of(shape, track_intensity).bytes();
		self.out.as_ref().is_some_and(|(out, _)| out.size() >= bytes)
	}

	/// A new output buffer for captures of `shape`; nothing recorded may write the one before.
	pub(super) fn grow(
		&mut self,
		device: &wgpu::Device,
		kernel: &Kernel,
		grid: &Grid,
		shape: &FrameShape,
		track_intensity: bool,
	) {
		let out = storage(device, "capture out", Words::of(shape, track_intensity).bytes());
		let groups = [0, 1].map(|c| {
			device.create_bind_group(&wgpu::BindGroupDescriptor {
				label: Some("capture"),
				layout: &kernel.layout,
				entries: &[
					entry(0, &grid.params, None),
					entry(1, &self.params, NonZeroU64::new(size_of::<CaptureParams>() as u64)),
					entry(2, &grid.field[c], None),
					entry(3, &grid.s1, None),
					entry(4, &grid.s2, None),
					entry(5, &out, None),
				],
			})
		});
		self.out = Some((out, groups));
	}

	/// Records the capture of the field of parity `cur` in `shape`, which [`fits`](Self::fits), with its copy
	/// to a free staging buffer (and to a device frame, once asked for): its slot and what tells when the copy
	/// is mapped.
	pub(super) fn record(
		&mut self,
		ctx: &GpuContext,
		batch: &mut Batch,
		kernel: &Kernel,
		cur: usize,
		shape: &FrameShape,
		track_intensity: bool,
	) -> (usize, Arc<dyn Readiness>) {
		let words = Words::of(shape, track_intensity);
		let bytes = words.bytes();
		let (out, groups) = self.out.as_ref().expect("an output buffer that fits");
		let slot = match self.ring.iter().position(Readback::is_free) {
			Some(free) => free,
			None => {
				assert!(self.ring.len() < MAX_CAPTURES, "more than {MAX_CAPTURES} captures pending");
				self.ring.push(Readback::new(&ctx.device, "capture readback", bytes));
				self.ring.len() - 1
			}
		};
		if self.ring[slot].buffer.size() < bytes {
			self.ring[slot] = Readback::new(&ctx.device, "capture readback", bytes);
		}
		let params = CaptureParams {
			out_w: shape.width(),
			divisor: shape.divisor,
			samples: shape.plane_samples() as u32,
			layer_words: words.layer,
			format: match shape.format {
				SampleFormat::Half => 0,
				SampleFormat::Index => 1,
			},
			root: shape.intensity_root as u32,
			field_b: shape.field_brightness,
			intensity_b: shape.intensity_brightness,
		};
		let offset = slot as u64 * self.stride;
		ctx.queue.write_buffer(&self.params, offset, bytemuck::bytes_of(&params));
		let groups_needed = words.samples.div_ceil(CAPTURE_GROUP);
		let max = ctx.limits.max_compute_workgroups_per_dimension;
		let (gx, gy) = (groups_needed.min(max), groups_needed.div_ceil(max));
		let kept = self.frames.as_ref().map(|f| f.begin(slot, *shape, words.layers == 2, words.layer, bytes));
		let encoder = batch.encoder(&ctx.device);
		{
			let mut pass = encoder.begin_compute_pass(&Default::default());
			pass.set_pipeline(kernel.pipeline());
			pass.set_bind_group(0, &groups[cur], &[offset as u32]);
			pass.dispatch_workgroups(gx, gy, words.layers);
		}
		let readback = &mut self.ring[slot];
		encoder.copy_buffer_to_buffer(out, 0, &readback.buffer, 0, bytes);
		if let Some(kept) = &kept {
			encoder.copy_buffer_to_buffer(out, 0, kept, 0, bytes);
		}
		readback.record();
		(slot, readback.readiness(ctx))
	}

	/// Requests the mappings of the captures `submission` carries.
	pub(super) fn submitted(&mut self, ctx: &GpuContext, submission: &wgpu::SubmissionIndex) {
		for readback in self.ring.iter_mut().filter(|r| r.is_recorded()) {
			readback.submitted(ctx, submission.clone());
		}
	}

	/// Whether the capture of `slot` is still to be submitted.
	pub(super) fn is_recorded(&self, slot: usize) -> bool {
		self.ring[slot].is_recorded()
	}

	/// Copies the planes of the capture of `slot`, submitted, into `snapshot`, waiting for them; the slot is
	/// free after it.
	pub(super) fn collect(
		&mut self,
		device: &wgpu::Device,
		slot: usize,
		snapshot: &mut Snapshot,
		track_intensity: bool,
	) -> Result<(), GpuError> {
		let readback = &mut self.ring[slot];
		if let Err(e) = readback.wait(device) {
			readback.abandon();
			return Err(e);
		}
		if let Some(frames) = &self.frames {
			frames.finish(slot);
		}
		let words = Words::of(&snapshot.shape, track_intensity);
		let (plane, layer_bytes) = (snapshot.shape.plane_bytes(), words.layer as usize * 4);
		let copied = readback.buffer.slice(..).get_mapped_range().map(|view| {
			snapshot.field.clear();
			snapshot.field.extend_from_slice(&view[..plane]);
			snapshot.intensity.clear();
			if words.layers == 2 {
				snapshot.intensity.extend_from_slice(&view[layer_bytes..layer_bytes + plane]);
			}
		});
		readback.buffer.unmap();
		copied.map_err(GpuError::MapRange)
	}

	/// Where the captures from now on keep their planes on `ctx`'s device.
	pub(super) fn frames(&mut self, ctx: &Arc<GpuContext>) -> Arc<DeviceFrames> {
		#[cfg_attr(
			target_arch = "wasm32",
			allow(clippy::arc_with_non_send_sync, reason = "wgpu's types are not Send there")
		)]
		let frames = self.frames.get_or_insert_with(|| Arc::new(DeviceFrames::new(ctx.clone())));
		frames.clone()
	}
}
