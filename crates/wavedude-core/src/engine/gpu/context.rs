//! The wgpu device shared by the GPU engines.
//!
//! [`GpuContext::create`] requests it (adapter request plus device creation take 50 to 200 ms, which every engine
//! rebuilt for an edit would pay again); [`GpuContext::from_device`] adopts a device requested elsewhere. Natively
//! [`context`] keeps one per process, or the one given to [`set_context`], until the device is lost; the next engine
//! built after a loss gets a new device. On wasm the caller owns it and requests a new one after a loss. An engine on a
//! lost device keeps failing; none is built on one.
//! Natively `wgpu::Device` and `wgpu::Queue` are `Send + Sync`; engines on different threads submit to the same
//! queue, which wgpu serialises. Software adapters, and adapters without compute shaders, are refused: the CPU
//! engine is faster or the only one.

use crate::sync::Notify;
#[cfg(not(target_arch = "wasm32"))]
use crate::sync::lock;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

/// Why no GPU engine can be built, or why a GPU engine failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GpuError {
	#[error("no usable GPU adapter: {0}")]
	Unavailable(String),
	#[error("GPU pipeline build failed: {0}")]
	Pipeline(String),
	#[error("the grid of {0} cells is too large for the GPU engine's 31-bit cell indices")]
	GridTooLarge(usize),
	#[error("the GPU device was lost ({reason:?}): {message}")]
	Lost { reason: wgpu::DeviceLostReason, message: String },
	#[error("waiting for the GPU failed")]
	Poll(#[source] Arc<wgpu::PollError>),
	#[error(
		"a GPU readback was waited for before it was mapped (await `PendingCapture::ready` or `PendingProbes::ready` \
		 first); the engine is failed"
	)]
	NotReady,
	#[error("mapping a GPU readback failed")]
	Map(#[source] wgpu::BufferAsyncError),
	#[error("reading a mapped GPU buffer failed")]
	MapRange(#[source] wgpu::MapRangeError),
}

impl From<wgpu::PollError> for GpuError {
	fn from(e: wgpu::PollError) -> Self {
		Self::Poll(Arc::new(e))
	}
}

/// The shared adapter facts, device and queue.
pub struct GpuContext {
	pub info: wgpu::AdapterInfo,
	pub limits: wgpu::Limits,
	pub device: wgpu::Device,
	pub queue: wgpu::Queue,
	/// Set by the device-lost callback.
	lost: Arc<Notify<GpuError>>,
}

impl GpuContext {
	/// The loss of the device, once it happened.
	pub fn lost(&self) -> Option<GpuError> {
		self.lost.get().cloned()
	}

	/// Set by the device-lost callback, which wakes its waiters.
	pub(crate) fn lost_notify(&self) -> Arc<Notify<GpuError>> {
		self.lost.clone()
	}

	/// Requests the adapter and the device.
	pub async fn create() -> Result<Arc<Self>, GpuError> {
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
		let adapter = instance
			.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::HighPerformance,
				..Default::default()
			})
			.await
			.map_err(|e| GpuError::Unavailable(e.to_string()))?;
		usable(&adapter)?;
		let (device, queue) = adapter
			.request_device(&wgpu::DeviceDescriptor {
				label: Some("wavedude-core"),
				required_limits: adapter.limits(),
				..Default::default()
			})
			.await
			.map_err(|e| GpuError::Unavailable(e.to_string()))?;
		Self::from_device(&adapter, device, queue)
	}

	/// Runs on a device requested elsewhere (a UI renderer's) with the limits it was requested with; the device's
	/// lost callback is replaced.
	pub fn from_device(
		adapter: &wgpu::Adapter,
		device: wgpu::Device,
		queue: wgpu::Queue,
	) -> Result<Arc<Self>, GpuError> {
		let info = usable(adapter)?;
		let limits = device.limits();
		let lost = Arc::<Notify<GpuError>>::default();
		{
			let lost = lost.clone();
			device.set_device_lost_callback(move |reason, message| lost.set(GpuError::Lost { reason, message }));
		}
		tracing::info!(
			adapter = %info.name,
			backend = ?info.backend,
			device_type = ?info.device_type,
			driver = %info.driver,
			driver_info = %info.driver_info,
			max_buffer_size = limits.max_buffer_size,
			max_storage_binding = limits.max_storage_buffer_binding_size,
			max_workgroups_per_dimension = limits.max_compute_workgroups_per_dimension,
			"GPU device"
		);
		#[cfg_attr(
			target_arch = "wasm32",
			allow(clippy::arc_with_non_send_sync, reason = "wgpu's types are not Send there")
		)]
		let context = Arc::new(Self { info, limits, device, queue, lost });
		Ok(context)
	}
}

/// The adapter's facts, unless it is a software adapter or runs no compute shaders.
fn usable(adapter: &wgpu::Adapter) -> Result<wgpu::AdapterInfo, GpuError> {
	let info = adapter.get_info();
	if info.device_type == wgpu::DeviceType::Cpu {
		return Err(GpuError::Unavailable(format!("{} is a software adapter", info.name)));
	}
	if !adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
		return Err(GpuError::Unavailable(format!("{} ({:?}) runs no compute shaders", info.name, info.backend)));
	}
	Ok(info)
}

#[cfg(not(target_arch = "wasm32"))]
static CONTEXT: Mutex<Option<Result<Arc<GpuContext>, GpuError>>> = Mutex::new(None);

/// The process's context, created on the first call and again after a device loss; a failure to create one
/// is kept, so a missing adapter is looked for once per process.
#[cfg(not(target_arch = "wasm32"))]
pub fn context() -> Result<Arc<GpuContext>, GpuError> {
	let mut slot = lock(&CONTEXT);
	if let Some(Ok(c)) = slot.as_ref()
		&& let Some(e) = c.lost()
	{
		tracing::warn!("{e}; a new GPU device is requested");
		*slot = None;
	}
	slot.get_or_insert_with(|| pollster::block_on(GpuContext::create())).clone()
}

/// Makes `ctx` the process's context for the engines built from now on; after its loss [`context`] requests a
/// device of its own.
#[cfg(not(target_arch = "wasm32"))]
pub fn set_context(ctx: Arc<GpuContext>) {
	*lock(&CONTEXT) = Some(Ok(ctx));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
	use super::*;
	use crate::sync::Readiness;
	use std::sync::atomic::{AtomicBool, Ordering};
	use std::task::{Context, Wake, Waker};

	struct Woken(AtomicBool);

	impl Wake for Woken {
		fn wake(self: Arc<Self>) {
			self.0.store(true, Ordering::SeqCst);
		}
	}

	#[test]
	fn a_lost_device_wakes_its_waiters() {
		let ctx = match pollster::block_on(GpuContext::create()) {
			Ok(ctx) => ctx,
			Err(e) => {
				eprintln!("skipped: {e}");
				return;
			}
		};
		let woken = Arc::new(Woken(AtomicBool::new(false)));
		let waker = Waker::from(woken.clone());
		assert!(ctx.lost_notify().poll_ready(&mut Context::from_waker(&waker)).is_pending());
		ctx.device.destroy();
		ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
		assert!(woken.0.load(Ordering::SeqCst));
		assert!(matches!(ctx.lost(), Some(GpuError::Lost { reason: wgpu::DeviceLostReason::Destroyed, .. })));
	}
}
