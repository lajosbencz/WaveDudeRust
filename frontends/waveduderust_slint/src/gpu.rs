//! One wgpu device for the engine and the window; the layers drawn into textures the window shows, no CPU readback.
//! Without a device the window draws in software and the layers are coloured on the CPU.

use slint::wgpu_30::{WGPUConfiguration, wgpu};
use std::sync::Arc;
use waveduderust::io::schemes::SchemeStore;
use waveduderust::model::{Layer, Project, View};

use waveduderust::unstable::engine::gpu::GpuContext;
use waveduderust::unstable::engine::types::FrameShape;
use waveduderust::unstable::render::gpu::{DevicePlane, GpuPicture, PictureInput, PicturePipeline};
use waveduderust::unstable::render::layer::{self, LayerColours};
use waveduderust::unstable::render::screen::ScreenRenderer;
use waveduderust::unstable::scene::glowing_edges::{self, EdgeImage};

/// Creates the device with the adapter's limits and hands it to the engine and to Slint (whose own choice would take
/// WebGL2's limits); natively, without one, Slint's software renderer and `None`.
pub(crate) async fn select() -> Result<Option<Arc<GpuContext>>, Box<dyn std::error::Error>> {
	match device().await {
		Ok(context) => Ok(Some(context)),
		#[cfg(not(target_arch = "wasm32"))]
		Err(why) => {
			tracing::warn!(
				"no usable GPU ({why}): the window and the pictures are drawn on the CPU, the run uses the CPU engine"
			);
			slint::BackendSelector::new().renderer_name("software".into()).select()?;
			Ok(None)
		}
		#[cfg(target_arch = "wasm32")]
		Err(why) => Err(why),
	}
}

async fn device() -> Result<Arc<GpuContext>, Box<dyn std::error::Error>> {
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

/// What a picture is drawn with: the project's colours, the view's layer settings and the scene's glowing edges.
#[derive(Clone, Copy)]
pub(crate) struct Look<'a> {
	pub(crate) project: &'a Project,
	pub(crate) view: &'a View,
	/// The user's schemes.
	pub(crate) schemes: &'a SchemeStore,
	pub(crate) edges: Option<&'a Arc<EdgeImage>>,
	/// A 16-bit plane's dither depth (10..16; else plain).
	pub(crate) dither_bits: u32,
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

	pub(crate) fn layer(&self) -> Layer {
		self.layer
	}

	/// Draws `frame`'s layer in `project`'s colours; the image to show.
	pub(crate) fn draw(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		pipeline: &PicturePipeline,
		plane: DevicePlane<'_>,
		shape: &FrameShape,
		look: &Look<'_>,
	) -> Option<slint::Image> {
		let Look { project, view, schemes, edges, dither_bits } = *look;
		let (w, h) = (shape.width(), shape.height());
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
		let colours = LayerColours::of(presentation, &project.color_schemes, schemes);
		let fit = layer::View::of(shape, (w, h), None, dither_bits);
		self.picture.set(
			pipeline,
			device,
			queue,
			&PictureInput {
				plane,
				shape,
				layer: self.layer,
				colours: &colours,
				view: &fit,
				outline: LayerColours::overlay_gain(presentation).zip(edges).map(|(gain, e)| (e, gain)),
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

/// A layer's picture coloured on the CPU from the stored plane.
pub(crate) struct CpuPicture {
	layer: Layer,
	screen: ScreenRenderer,
	outline: Option<Outline>,
}

/// Glowing edges scaled to a picture's size.
struct Outline {
	of: std::sync::Weak<EdgeImage>,
	size: (u32, u32),
	scaled: Vec<u8>,
}

impl CpuPicture {
	pub(crate) fn new(layer: Layer) -> Self {
		Self { layer, screen: ScreenRenderer::new(1, 1, 1, 1), outline: None }
	}

	pub(crate) fn layer(&self) -> Layer {
		self.layer
	}

	/// Colours the plane `bytes` of a frame of `shape` as `look` says, at the stored size; the image to show.
	pub(crate) fn draw(&mut self, bytes: &[u8], shape: &FrameShape, look: &Look<'_>) -> slint::Image {
		let Look { project, view, schemes, edges, dither_bits } = *look;
		let presentation = match self.layer {
			Layer::Field => &view.field,
			_ => &view.intensity,
		};
		let colours = LayerColours::of(presentation, &project.color_schemes, schemes);
		let fit = layer::View::of(shape, (0, 0), None, dither_bits);
		let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(fit.width, fit.height);
		let rgba = pixels.make_mut_bytes();
		layer::paint(bytes, shape, self.layer, &colours, &fit, &mut self.screen, rgba);
		if let Some((gain, edges)) = LayerColours::overlay_gain(presentation).zip(edges) {
			let size = (fit.width, fit.height);
			let stale = self
				.outline
				.as_ref()
				.is_none_or(|o| !std::ptr::eq(o.of.as_ptr(), Arc::as_ptr(edges)) || o.size != size);
			if stale {
				self.outline = Some(Outline { of: Arc::downgrade(edges), size, scaled: edges.scaled(None, size) });
			}
			if let Some(o) = &self.outline {
				glowing_edges::blend_additive(rgba, &o.scaled, gain);
			}
		}
		slint::Image::from_rgba8(pixels)
	}
}
