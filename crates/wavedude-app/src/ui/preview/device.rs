//! The pictures of a run on the GPU engine, coloured on the renderer's device (docs/ui/40-preview-pane.md, 8-bit vs
//! 16-bit): the plane is the frame the engine keeps on the device when it is the one shown, else the stored plane
//! uploaded; `wavedude_core::render::gpu` colours it in a paint callback inside egui's render pass, so a frame on the
//! device is never read back to be shown.
//!
//! Threads: the UI thread; the engine's thread captures into the device frames, which a held frame is safe from.

use std::sync::Arc;

use eframe::egui::{self, Painter, Rect};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, RenderState};
use eframe::wgpu;
use wavedude_core::engine::gpu::{DeviceFrame, DeviceFrames, FrameTag};
use wavedude_core::engine::{FrameShape, GpuHandle, SampleFormat};
use wavedude_core::model::Layer;
use wavedude_core::render::gpu::{DevicePlane, GpuPicture, PictureInput, PicturePipeline};
use wavedude_core::render::layer::View;

use crate::preview::presenter::{FrameSource, LayerRequest, NoPlane, SourceKey};

/// The pipeline and the two pictures, in the renderer's callback resources.
struct Pictures {
	pipeline: PicturePipeline,
	pictures: [GpuPicture; 2],
}

/// What a picture was set to: its plane and request.
struct Shown {
	buffer: wgpu::Buffer,
	first_word: u32,
	shape: FrameShape,
	request: LayerRequest,
}

impl Shown {
	fn is(&self, plane: &DevicePlane<'_>, shape: &FrameShape, request: &LayerRequest) -> bool {
		self.buffer == *plane.buffer
			&& self.first_word == plane.first_word
			&& self.shape == *shape
			&& self.request == *request
	}
}

/// See the module docs.
pub(super) struct DevicePictures {
	/// The renderer's context, when the engines run on it: their frames are drawn from there.
	renderer: Option<GpuHandle>,
	/// The frames a frame is held from.
	held_from: Option<Arc<DeviceFrames>>,
	/// The engine's frame shown.
	held: Option<DeviceFrame>,
	/// The source of the planes and its shape.
	source: Option<(SourceKey, FrameShape)>,
	/// Each layer's uploaded plane: the buffer and whether it holds the source's plane.
	uploads: [Option<wgpu::Buffer>; 2],
	uploaded: [bool; 2],
	/// A spilled frame's plane, and a plane padded to whole words.
	spill: Vec<u8>,
	padded: Vec<u8>,
	/// The requests of the layers shown.
	requests: Vec<LayerRequest>,
	set: [Option<Shown>; 2],
	/// The source asked for last is being read back from the spill; the frame shown before stays.
	pending: bool,
}

impl DevicePictures {
	pub(super) fn new(renderer: Option<GpuHandle>) -> Self {
		Self {
			renderer,
			held_from: None,
			held: None,
			source: None,
			uploads: [None, None],
			uploaded: [false; 2],
			spill: Vec::new(),
			padded: Vec::new(),
			requests: Vec::new(),
			set: [None, None],
			pending: false,
		}
	}

	/// Shows nothing, holding no frame.
	pub(super) fn clear(&mut self) {
		if let Some(frames) = self.held_from.take() {
			frames.release();
		}
		self.held = None;
		self.source = None;
		self.uploaded = [false; 2];
		self.requests.clear();
		self.set = [None, None];
		self.pending = false;
	}

	/// The source asked for last waits for its read back: [`show`](Self::show) it again.
	pub(super) fn pending(&self) -> bool {
		self.pending
	}

	/// Shows `source` with `requests`: the engine's frame of it held when `frames` live on the renderer's context,
	/// else its planes uploaded. A source shown already only takes the requests.
	pub(super) fn show(
		&mut self,
		gpu: &RenderState,
		frames: &Arc<DeviceFrames>,
		source: &FrameSource,
		requests: &[LayerRequest],
	) {
		self.requests.clear();
		self.requests.extend_from_slice(requests);
		let key = source.key();
		if self.source.as_ref().is_some_and(|(k, _)| *k == key) {
			return;
		}
		let shape = source.shape();
		let held = match source {
			FrameSource::Run { store, stored }
				if self.renderer.as_ref().is_some_and(|r| Arc::ptr_eq(r, frames.context())) =>
			{
				let tag = FrameTag { store: store.id(), generation: store.generation(), stored: *stored };
				frames.hold(tag)
			}
			_ => None,
		};
		if held.is_some() {
			self.pending = false;
			self.source = Some((key, shape));
			if let Some(old) = self.held_from.replace(frames.clone()).filter(|old| !Arc::ptr_eq(old, frames)) {
				old.release();
			}
			self.held = held;
			self.uploaded = [false; 2];
			return;
		}
		let mut uploaded = [false; 2];
		for layer in [Layer::Field, Layer::Intensity] {
			let (padded, slot) = (&mut self.padded, &mut self.uploads[layer as usize]);
			let written = source.with_plane_into(layer, &mut self.spill, |bytes| {
				// the buffer's size is a multiple of 4
				let bytes = if bytes.len() % 4 == 0 {
					bytes
				} else {
					padded.clear();
					padded.extend_from_slice(bytes);
					padded.resize(bytes.len().next_multiple_of(4), 0);
					padded.as_slice()
				};
				let buffer = match slot.take() {
					Some(b) if b.size() >= bytes.len() as u64 => b,
					_ => gpu.device.create_buffer(&wgpu::BufferDescriptor {
						label: Some("preview plane"),
						size: (bytes.len() as u64).max(16),
						usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
						mapped_at_creation: false,
					}),
				};
				gpu.queue.write_buffer(&buffer, 0, bytes);
				*slot = Some(buffer);
			});
			if let Err(NoPlane::Pending) = written {
				self.pending = true;
				return;
			}
			uploaded[layer as usize] = written.is_ok();
		}
		self.pending = false;
		self.source = Some((key, shape));
		if let Some(old) = self.held_from.take() {
			old.release();
		}
		self.held = None;
		self.uploaded = uploaded;
	}

	/// Whether `layer` has a picture.
	pub(super) fn has(&self, layer: Layer) -> bool {
		self.plane(layer).is_some()
	}

	/// The part of the canvas `layer`'s picture covers; `None` is all of it (an 8-bit picture is whole).
	pub(super) fn window(&self, layer: Layer) -> Option<Rect> {
		let (_, shape) = self.source.as_ref()?;
		if shape.format != SampleFormat::Half {
			return None;
		}
		let (x, y, w, h) = self.requests.iter().find(|r| r.layer == layer)?.window?;
		Some(Rect::from_min_size(egui::pos2(x as f32, y as f32), egui::vec2(w as f32, h as f32)))
	}

	fn plane(&self, layer: Layer) -> Option<DevicePlane<'_>> {
		match &self.held {
			Some(frame) => Some(DevicePlane { buffer: &frame.buffer, first_word: frame.first_word(layer)? }),
			None => self.uploaded[layer as usize]
				.then(|| self.uploads[layer as usize].as_ref())
				.flatten()
				.map(|buffer| DevicePlane { buffer, first_word: 0 }),
		}
	}

	/// Draws `layer`'s picture into `rect` with `painter`, its buffers brought to the request first.
	pub(super) fn paint(&mut self, gpu: &RenderState, painter: &Painter, layer: Layer, rect: Rect) {
		let (Some(plane), Some((_, shape))) = (self.plane(layer), &self.source) else { return };
		let Some(request) = self.requests.iter().find(|r| r.layer == layer) else { return };
		let mut renderer = gpu.renderer.write();
		let resources = &mut renderer.callback_resources;
		if resources.get::<Pictures>().is_none() {
			let pipeline = PicturePipeline::new(&gpu.device, gpu.target_format);
			let pictures = [GpuPicture::new(&gpu.device), GpuPicture::new(&gpu.device)];
			resources.insert(Pictures { pipeline, pictures });
		}
		let Pictures { pipeline, pictures } = resources.get_mut::<Pictures>().expect("inserted above");
		let picture = &mut pictures[layer as usize];
		if !self.set[layer as usize].as_ref().is_some_and(|s| s.is(&plane, shape, request)) {
			let view = View::of(shape, request.device, request.window, request.dither_bits);
			let input = PictureInput {
				plane,
				shape,
				layer,
				colours: &request.colours,
				view: &view,
				outline: request.outline.as_ref().map(|o| (&o.edges, o.gain)),
			};
			picture.set(pipeline, &gpu.device, &gpu.queue, &input);
			self.set[layer as usize] = Some(Shown {
				buffer: plane.buffer.clone(),
				first_word: plane.first_word,
				shape: *shape,
				request: request.clone(),
			});
		}
		let ppp = painter.ctx().pixels_per_point();
		picture.place(&gpu.queue, [rect.min.x * ppp, rect.min.y * ppp], [rect.width() * ppp, rect.height() * ppp]);
		painter.add(egui_wgpu::Callback::new_paint_callback(rect, PictureCallback(layer as usize)));
	}
}

/// Draws the picture of the layer it holds (the index into [`Pictures::pictures`]).
struct PictureCallback(usize);

impl CallbackTrait for PictureCallback {
	fn paint(
		&self,
		_info: egui::PaintCallbackInfo,
		pass: &mut wgpu::RenderPass<'static>,
		resources: &CallbackResources,
	) {
		if let Some(Pictures { pipeline, pictures }) = resources.get::<Pictures>() {
			pictures[self.0].draw(pipeline, pass);
		}
	}
}
