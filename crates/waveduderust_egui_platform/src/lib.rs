//! egui start-up and the OS integration the windows share: the renderer's device the engines share, the keys, input
//! helpers, the clipboard, the colour scheme, the window's place, the menu icons, the number boxes' ranges, the file
//! dialogs and the tasks.

pub mod icons;
pub mod input;
pub mod jobs;
pub mod keys;
pub mod number;
#[cfg(not(target_arch = "wasm32"))]
pub mod os_theme;
#[cfg(not(target_arch = "wasm32"))]
pub mod placement;
pub mod shown;
pub mod theme;

#[cfg(test)]
mod fonts_tests;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::sync::Arc;

use eframe::{egui_wgpu, wgpu};
#[cfg(not(target_arch = "wasm32"))]
use waveduderust::unstable::engine::GpuHandle;
#[cfg(not(target_arch = "wasm32"))]
use waveduderust::unstable::engine::gpu::GpuContext;

#[cfg(not(target_arch = "wasm32"))]
pub use self::native::{LeaveGuard, pastable, pasted, read_clipboard, remember_place, rezoom, set_title};
#[cfg(target_arch = "wasm32")]
pub use self::web::{LeaveGuard, pastable, pasted, read_clipboard, remember_place, rezoom, set_title};

/// Asks for the renderer's device with the adapter's own limits where it runs compute shaders: the engines share it
/// natively, and the preview colours a large run's frames on it (their planes need its storage binding size); egui's
/// request otherwise.
pub fn share_device(options: &mut egui_wgpu::WgpuConfiguration) {
	if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
		let fallback = setup.device_descriptor.clone();
		setup.device_descriptor = Arc::new(move |adapter| {
			if adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
				wgpu::DeviceDescriptor {
					label: Some("wavedude"),
					required_limits: adapter.limits(),
					..Default::default()
				}
			} else {
				fallback(adapter)
			}
		});
	}
}

/// The engines' GPU: the renderer's device when it can run them; `None` (logged) otherwise.
#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_gpu(cc: &eframe::CreationContext<'_>) -> Option<GpuHandle> {
	let rs = cc.wgpu_render_state.as_ref()?;
	GpuContext::from_device(&rs.adapter, rs.device.clone(), rs.queue.clone())
		.inspect_err(|e| tracing::warn!("the renderer's GPU device runs no engine: {e}"))
		.ok()
}
