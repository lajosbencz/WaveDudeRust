//! A picture's texture on the renderer's device: written in place from the picture's RGBA8 bytes, made anew only
//! when the picture's size changes, under one egui texture id for its lifetime.

use eframe::egui::TextureId;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;

pub(super) struct Texture {
	id: TextureId,
	texture: wgpu::Texture,
}

impl Texture {
	pub(super) fn id(&self) -> TextureId {
		self.id
	}

	/// Writes `rgba` (`size` pixels, at least one, RGBA8, opaque) into `slot`'s texture, making it when there is
	/// none or its size differs.
	pub(super) fn upload(slot: &mut Option<Self>, gpu: &RenderState, size: (u32, u32), rgba: &[u8]) {
		debug_assert_eq!(rgba.len() as u64, u64::from(size.0) * u64::from(size.1) * 4, "an RGBA8 picture");
		let extent = wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 };
		let t = match slot.take() {
			Some(t) if t.texture.size() == extent => slot.insert(t),
			old => {
				let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
					label: Some("preview picture"),
					size: extent,
					mip_level_count: 1,
					sample_count: 1,
					dimension: wgpu::TextureDimension::D2,
					format: wgpu::TextureFormat::Rgba8Unorm,
					usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
					view_formats: &[],
				});
				let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
				let mut renderer = gpu.renderer.write();
				let filter = wgpu::FilterMode::Linear;
				let id = match old {
					Some(old) => {
						renderer.update_egui_texture_from_wgpu_texture(&gpu.device, &view, filter, old.id);
						old.id
					}
					None => renderer.register_native_texture(&gpu.device, &view, filter),
				};
				slot.insert(Self { id, texture })
			}
		};
		gpu.queue.write_texture(
			t.texture.as_image_copy(),
			rgba,
			wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * size.0), rows_per_image: None },
			extent,
		);
	}
}
