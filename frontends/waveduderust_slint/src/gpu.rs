//! One wgpu device for the engine and the window; the layers drawn into textures the window shows, no CPU readback.

use slint::wgpu_30::{WGPUConfiguration, wgpu};
use std::sync::Arc;
use waveduderust::io::schemes::SchemeStore;
use waveduderust::model::{Layer, Project, View};

use waveduderust::unstable::engine::gpu::{DeviceFrame, GpuContext};
use waveduderust::unstable::render::gpu::{DevicePlane, GpuPicture, PictureInput, PicturePipeline};
use waveduderust::unstable::render::layer::{self, LayerColours};

/// Creates the device with the adapter's limits and hands it to the engine and to Slint (whose own choice would take
/// WebGL2's limits).
pub(crate) async fn select() -> Result<Arc<GpuContext>, Box<dyn std::error::Error>> {
	let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
	let adapter = instance
		.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::HighPerformance,
			..Default::default()
		})
		.await?;
	let (device, queue) = adapter
		.request_device(&wgpu::DeviceDescriptor {
			label: Some("waveduderust"),
			required_limits: adapter.limits(),
			..Default::default()
		})
		.await?;
	let context = GpuContext::from_device(&adapter, device.clone(), queue.clone())?;
	#[cfg(not(target_arch = "wasm32"))]
	waveduderust::unstable::engine::gpu::set_context(context.clone());
	slint::BackendSelector::new()
		.require_wgpu_30(WGPUConfiguration::Manual { instance, adapter, device, queue })
		.select()?;
	Ok(context)
}

/// A layer's picture: the pipeline's draw into a texture of the frame's size.
pub(crate) struct Picture {
	layer: Layer,
	picture: GpuPicture,
	texture: Option<wgpu::Texture>,
}

impl Picture {
	pub(crate) fn new(device: &wgpu::Device, layer: Layer) -> Self {
		Self { layer, picture: GpuPicture::new(device), texture: None }
	}

	/// Draws `frame`'s layer in `project`'s colours; the image to show.
	pub(crate) fn draw(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		pipeline: &PicturePipeline,
		frame: &DeviceFrame,
		project: &Project,
		view: &View,
	) -> Option<slint::Image> {
		let (w, h) = (frame.shape.width(), frame.shape.height());
		let texture = match &self.texture {
			Some(t) if t.width() == w && t.height() == h => t.clone(),
			_ => {
				let t = device.create_texture(&wgpu::TextureDescriptor {
					label: Some("picture"),
					size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
					mip_level_count: 1,
					sample_count: 1,
					dimension: wgpu::TextureDimension::D2,
					format: wgpu::TextureFormat::Rgba8Unorm,
					usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
					view_formats: &[],
				});
				self.texture = Some(t.clone());
				t
			}
		};
		let presentation = match self.layer {
			Layer::Field => &view.field,
			_ => &view.intensity,
		};
		let colours = LayerColours::of(presentation, &project.color_schemes, &SchemeStore::default());
		let fit = layer::View::of(&frame.shape, (w, h), None, 0);
		self.picture.set(
			pipeline,
			device,
			queue,
			&PictureInput {
				plane: DevicePlane { buffer: &frame.buffer, first_word: frame.first_word(self.layer)? },
				shape: &frame.shape,
				layer: self.layer,
				colours: &colours,
				view: &fit,
				outline: None,
			},
		);
		self.picture.place(queue, [0.0, 0.0], [w as f32, h as f32]);
		let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
		let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
		{
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &target,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
						store: wgpu::StoreOp::Store,
					},
				})],
				..Default::default()
			});
			self.picture.draw(pipeline, &mut pass);
		}
		queue.submit([encoder.finish()]);
		slint::Image::try_from(texture).ok()
	}
}
