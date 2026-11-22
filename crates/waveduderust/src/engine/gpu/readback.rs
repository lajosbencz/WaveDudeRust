//! Buffers the host maps to read what the card wrote, and the waiting for them.

use std::sync::Arc;
use std::task::{Context, Poll};

use super::{GpuContext, GpuError};
use crate::sync::{Notify, Readiness};

/// The outcome of a mapping request, set by its callback, which also wakes a waiting future.
type MapState = Arc<Notify<Result<(), wgpu::BufferAsyncError>>>;

/// A mapping as a waiter sees it: ready once done or once the device is lost, so a loss, which the engine reports
/// next, leaves no waiter pending.
struct MappedOrLost {
	mapped: MapState,
	lost: Arc<Notify<GpuError>>,
}

impl Readiness for MappedOrLost {
	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
		if self.mapped.poll_ready(cx).is_ready() { Poll::Ready(()) } else { self.lost.poll_ready(cx) }
	}
}

/// What fills a pending readback.
enum Fill {
	/// Copies in the pending encoder; the mapping is requested once it is submitted.
	Recorded,
	Submitted(
		#[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "waited for natively"))] wgpu::SubmissionIndex,
	),
}

/// A host-mappable buffer, its map state, and what fills it while it is pending.
pub(super) struct Readback {
	pub buffer: wgpu::Buffer,
	mapped: MapState,
	pending: Option<Fill>,
}

impl Readback {
	pub(super) fn new(device: &wgpu::Device, label: &str, bytes: u64) -> Self {
		let buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some(label),
			size: bytes.next_multiple_of(4).max(4),
			usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		Self { buffer, mapped: MapState::default(), pending: None }
	}

	/// Nothing fills it: free for the next use.
	pub(super) fn is_free(&self) -> bool {
		self.pending.is_none()
	}

	/// Its copies are in the pending encoder, not submitted yet.
	pub(super) fn is_recorded(&self) -> bool {
		matches!(self.pending, Some(Fill::Recorded))
	}

	/// Its mapping has landed.
	pub(super) fn is_mapped(&self) -> bool {
		self.mapped.get().is_some()
	}

	/// Ready once [`wait`](Self::wait) will not have to wait, or the device is lost.
	pub(super) fn readiness(&self, ctx: &GpuContext) -> Arc<dyn Readiness> {
		Arc::new(MappedOrLost { mapped: self.mapped.clone(), lost: ctx.lost_notify() })
	}

	/// Filled from now on by copies in the pending encoder.
	pub(super) fn record(&mut self) {
		self.mapped = MapState::default();
		self.pending = Some(Fill::Recorded);
	}

	/// Requests the mapping once `submission`, which fills it, is done; awaited on `ctx` until it lands.
	pub(super) fn submitted(&mut self, ctx: &GpuContext, submission: wgpu::SubmissionIndex) {
		let state = self.mapped.clone();
		let awaited = ctx.awaiting();
		self.pending = Some(Fill::Submitted(submission));
		self.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
			state.set(r);
			drop(awaited);
		});
	}

	/// [`submitted`](Self::submitted) for a readback that was not [`record`](Self::record)ed first.
	pub(super) fn map(&mut self, ctx: &GpuContext, submission: wgpu::SubmissionIndex) {
		self.mapped = MapState::default();
		self.submitted(ctx, submission);
	}

	/// Free again, whatever fills it left unread.
	pub(super) fn abandon(&mut self) {
		self.pending = None;
	}

	/// Waits for the mapping requested by [`submitted`](Self::submitted); the readback is free after it.
	#[cfg(not(target_arch = "wasm32"))]
	pub(super) fn wait(&mut self, device: &wgpu::Device) -> Result<(), GpuError> {
		match self.pending.take() {
			Some(Fill::Submitted(index)) => wait(device, index)?,
			Some(Fill::Recorded) => unreachable!("a readback waited for before its submission"),
			None => {}
		}
		loop {
			if let Some(result) = self.mapped.get() {
				return result.clone().map_err(GpuError::Map);
			}
			device.poll(wgpu::PollType::wait_indefinitely())?;
		}
	}

	/// The mapping requested by [`submitted`](Self::submitted); nothing can block here, so one not done yet is
	/// [`GpuError::NotReady`].
	#[cfg(target_arch = "wasm32")]
	pub(super) fn wait(&mut self, _device: &wgpu::Device) -> Result<(), GpuError> {
		let result = self.mapped.get().ok_or(GpuError::NotReady)?;
		self.pending = None;
		result.clone().map_err(GpuError::Map)
	}
}

pub(super) fn wait(device: &wgpu::Device, index: wgpu::SubmissionIndex) -> Result<(), GpuError> {
	device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: None })?;
	Ok(())
}
