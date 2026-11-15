//! Storage buffers, bind groups over them, and the scope that catches their errors.

use std::num::NonZeroU64;
use std::pin::Pin;

/// What a closed [`ResourceScope`] caught, once the device has said.
#[cfg(not(target_arch = "wasm32"))]
pub(super) type Caught = Pin<Box<dyn Future<Output = Option<wgpu::Error>> + Send>>;
/// What a closed [`ResourceScope`] caught, once the device has said.
#[cfg(target_arch = "wasm32")]
pub(super) type Caught = Pin<Box<dyn Future<Output = Option<wgpu::Error>>>>;

/// Catches the validation and out-of-memory errors of the buffers and bind groups made while it is open, which
/// would otherwise reach wgpu's uncaptured-error handler (a panic). Opened and closed with no await between.
pub(super) struct ResourceScope {
	out_of_memory: wgpu::ErrorScopeGuard,
	invalid: wgpu::ErrorScopeGuard,
}

impl ResourceScope {
	pub(super) fn open(device: &wgpu::Device) -> Self {
		let out_of_memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
		Self { out_of_memory, invalid: device.push_error_scope(wgpu::ErrorFilter::Validation) }
	}

	pub(super) fn close(self) -> Caught {
		let (invalid, out_of_memory) = (self.invalid.pop(), self.out_of_memory.pop());
		Box::pin(async move {
			let invalid = invalid.await;
			invalid.or(out_of_memory.await)
		})
	}
}

pub(super) fn storage(device: &wgpu::Device, label: &str, bytes: u64) -> wgpu::Buffer {
	device.create_buffer(&wgpu::BufferDescriptor {
		label: Some(label),
		size: bytes.max(16).next_multiple_of(4),
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
		mapped_at_creation: false,
	})
}

pub(super) fn storage_init(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::Buffer {
	let size = (bytes.len() as u64).max(16).next_multiple_of(4);
	let buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some(label),
		size,
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
		mapped_at_creation: true,
	});
	let mut padded = bytes.to_vec();
	padded.resize(size as usize, 0);
	buffer.slice(..).get_mapped_range_mut().expect("mapped at creation").copy_from_slice(&padded);
	buffer.unmap();
	buffer
}

/// `values` with at least one element.
pub(super) fn padded<T: Copy>(mut values: Vec<T>, fill: T) -> Vec<T> {
	if values.is_empty() {
		values.push(fill);
	}
	values
}

pub(super) fn uniform(device: &wgpu::Device, label: &str, bytes: u64) -> wgpu::Buffer {
	device.create_buffer(&wgpu::BufferDescriptor {
		label: Some(label),
		size: bytes,
		usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	})
}

pub(super) fn entry(binding: u32, buffer: &wgpu::Buffer, size: Option<NonZeroU64>) -> wgpu::BindGroupEntry<'_> {
	wgpu::BindGroupEntry {
		binding,
		resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size }),
	}
}

/// A bind group of `buffers` whole, in binding order. It keeps its buffers: one made only to be bound needs no
/// other owner.
pub(super) fn bind(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	buffers: &[&wgpu::Buffer],
) -> wgpu::BindGroup {
	let entries: Vec<_> = buffers.iter().enumerate().map(|(i, b)| entry(i as u32, b, None)).collect();
	device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries })
}
