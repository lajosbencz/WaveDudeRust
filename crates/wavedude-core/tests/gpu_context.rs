#![cfg(feature = "gpu")]
//! The process's GPU context, apart from the engine tests: setting it changes the device every engine of the
//! process runs on.

use std::sync::Arc;

use wavedude_core::engine::gpu::{GpuContext, context, set_context};

#[test]
fn a_set_context_is_the_process_context() {
	if let Err(e) = context() {
		eprintln!("skipped: {e}");
		return;
	}
	let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
	let adapter =
		pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
	let (device, queue) =
		pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
	let ctx = GpuContext::from_device(&adapter, device, queue).expect("GPU context");
	assert!(!Arc::ptr_eq(&context().unwrap(), &ctx));
	set_context(ctx.clone());
	assert!(Arc::ptr_eq(&context().unwrap(), &ctx));
}
