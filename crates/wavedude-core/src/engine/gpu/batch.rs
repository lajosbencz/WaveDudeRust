//! The commands recorded since the last submission, and the submissions the card has yet to run.

use std::collections::VecDeque;
#[cfg(target_arch = "wasm32")]
use std::sync::Arc;

use super::{GpuContext, GpuError};
#[cfg(target_arch = "wasm32")]
use crate::sync::Notify;

/// Steps recorded per submission at most.
pub const BATCH_STEPS: u32 = 32;
/// Submissions queued ahead of the card at most.
pub const MAX_IN_FLIGHT: usize = 2;

/// A submission the card may not have run yet: natively its index, waited for; on wasm32 set once the card ran it.
#[cfg(not(target_arch = "wasm32"))]
type Submitted = wgpu::SubmissionIndex;
#[cfg(target_arch = "wasm32")]
type Submitted = Arc<Notify<()>>;

#[derive(Default)]
pub(super) struct Batch {
	encoder: Option<wgpu::CommandEncoder>,
	/// Steps recorded since the last submission.
	pub steps: u32,
	in_flight: VecDeque<Submitted>,
	#[cfg(test)]
	pub submissions: usize,
}

impl Batch {
	/// The encoder of the commands recorded, a new one after a submission.
	pub(super) fn encoder(&mut self, device: &wgpu::Device) -> &mut wgpu::CommandEncoder {
		self.encoder.get_or_insert_with(|| device.create_command_encoder(&Default::default()))
	}

	/// Whether commands are recorded.
	pub(super) fn is_recorded(&self) -> bool {
		self.encoder.is_some()
	}

	/// Submits the commands recorded, even none.
	pub(super) fn submit(&mut self, ctx: &GpuContext) -> wgpu::SubmissionIndex {
		let encoder = self.encoder.take().unwrap_or_else(|| ctx.device.create_command_encoder(&Default::default()));
		self.steps = 0;
		#[cfg(test)]
		{
			self.submissions += 1;
		}
		ctx.submit([encoder.finish()])
	}

	/// Waits for the oldest submissions beyond [`MAX_IN_FLIGHT`].
	#[cfg(not(target_arch = "wasm32"))]
	pub(super) fn pace(&mut self, ctx: &GpuContext, submitted: wgpu::SubmissionIndex) -> Result<(), GpuError> {
		self.in_flight.push_back(submitted);
		while self.in_flight.len() > MAX_IN_FLIGHT {
			let oldest = self.in_flight.pop_front().expect("in flight");
			super::readback::wait(&ctx.device, oldest)?;
		}
		Ok(())
	}

	/// Notes the submission, set once the card ran it, for [`oldest_when_full`](Self::oldest_when_full).
	#[cfg(target_arch = "wasm32")]
	pub(super) fn pace(&mut self, ctx: &GpuContext, _submitted: wgpu::SubmissionIndex) -> Result<(), GpuError> {
		let done = Arc::<Notify<()>>::default();
		let ran = done.clone();
		ctx.queue.on_submitted_work_done(move || ran.set(()));
		self.in_flight.retain(|done| done.get().is_none());
		self.in_flight.push_back(done);
		Ok(())
	}

	/// The oldest submission the card has not run, while [`MAX_IN_FLIGHT`] of them are queued.
	#[cfg(target_arch = "wasm32")]
	pub(super) fn oldest_when_full(&mut self) -> Option<&Arc<Notify<()>>> {
		self.in_flight.retain(|done| done.get().is_none());
		self.in_flight.front().filter(|_| self.in_flight.len() >= MAX_IN_FLIGHT)
	}

	/// The card has run every submission.
	pub(super) fn settled(&mut self) {
		self.in_flight.clear();
	}
}
