//! Colouring a stored plane on the GPU, into a render pass: the picture [`paint`](super::layer::paint) and the
//! outline blend make on the CPU, from the plane where it lives (a GPU engine's [`DeviceFrame`] or a plane the
//! caller uploaded), so a picture on the device is never read back to be coloured (shaders/colour.wgsl).
//!
//! The tables are the CPU's: the half table, the deep table with the Bayer thresholds, the palette, the resampling
//! axes' weights and the outline's additions, uploaded when they change. Where the two differ: the card may fuse
//! the resampling's multiply-adds, which moves a sum by an f32 ulp and so, at an f16 rounding boundary, picks the
//! neighbouring table entry; an index picture is scaled bilinearly in f32 where a sampler weighs in fixed point.
//!
//! [`DeviceFrame`]: crate::engine::gpu::DeviceFrame

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use super::layer::{Colouring, LayerColours, View};
use super::screen::{ResampleAxis, Window, axes};
use crate::engine::gpu::HALF;
use crate::engine::types::SampleKind;
use crate::engine::{FrameShape, SampleFormat};
use crate::model::Layer;
use crate::scene::glowing_edges::{EdgeImage, additions};

const SHADER: &str = include_str!("shaders/colour.wgsl");

/// Mirrors `Colour` in colour.wgsl (96 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct Params {
	origin: [f32; 2],
	size: [f32; 2],
	mode: u32,
	first_word: u32,
	stored_w: u32,
	stored_h: u32,
	grid_w: u32,
	grid_h: u32,
	energy: u32,
	outline: u32,
	side: u32,
	thresholds: u32,
	x_first: u32,
	x_offsets: u32,
	y_first: u32,
	y_offsets: u32,
	x_weights: u32,
	y_weights: u32,
	srgb: u32,
	_pad: [u32; 3],
}

/// `Colour.mode`.
const PLAIN: u32 = 0;
const DITHERED: u32 = 1;
const INDEX: u32 = 2;

/// The pipeline for one render target format, shared by the pictures drawn into it.
pub struct PicturePipeline {
	layout: wgpu::BindGroupLayout,
	pipeline: wgpu::RenderPipeline,
	/// Bound where a picture has no axes or outline.
	empty: wgpu::Buffer,
	srgb: bool,
}

impl PicturePipeline {
	/// The pipeline drawing opaque pictures into `target`; an sRGB target gets linear colours, so it stores the
	/// pictures' bytes.
	pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
		let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
			binding,
			visibility: wgpu::ShaderStages::FRAGMENT,
			ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
			count: None,
		};
		let storage = wgpu::BufferBindingType::Storage { read_only: true };
		let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("picture"),
			entries: &[
				entry(0, wgpu::BufferBindingType::Uniform),
				entry(1, storage),
				entry(2, storage),
				entry(3, storage),
				entry(4, storage),
			],
		});
		let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("picture"),
			source: wgpu::ShaderSource::Wgsl(format!("{HALF}\n{SHADER}").into()),
		});
		let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("picture"),
			bind_group_layouts: &[Some(&layout)],
			immediate_size: 0,
		});
		let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("picture"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &module,
				entry_point: Some("vs"),
				compilation_options: Default::default(),
				buffers: &[],
			},
			fragment: Some(wgpu::FragmentState {
				module: &module,
				entry_point: Some("fs"),
				compilation_options: Default::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format: target,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			primitive: Default::default(),
			depth_stencil: None,
			multisample: Default::default(),
			multiview_mask: None,
			cache: None,
		});
		let empty = storage_buffer(device, "picture empty", 16);
		Self { layout, pipeline, empty, srgb: target.is_srgb() }
	}
}

/// A plane on the device: packed words as [`DeviceFrame`](crate::engine::gpu::DeviceFrame) describes, the
/// plane from `first_word` on.
#[derive(Clone, Copy, Debug)]
pub struct DevicePlane<'a> {
	pub buffer: &'a wgpu::Buffer,
	pub first_word: u32,
}

/// What a picture shows: the plane, how it is coloured and sized and the outline over it.
pub struct PictureInput<'a> {
	pub plane: DevicePlane<'a>,
	pub shape: &'a FrameShape,
	pub layer: Layer,
	pub colours: &'a LayerColours,
	pub view: &'a View,
	/// The scene's edges and their gain.
	pub outline: Option<(&'a Arc<EdgeImage>, f64)>,
}

/// Whether `a` and `b` are the same cached tables (held, so the addresses name them).
fn same_tables(a: &Colouring, b: &Colouring) -> bool {
	match (a, b) {
		(Colouring::Palette(a), Colouring::Palette(b)) => Arc::ptr_eq(a, b),
		(Colouring::Plain(a), Colouring::Plain(b)) => Arc::ptr_eq(a, b),
		(Colouring::Dithered(a, x), Colouring::Dithered(b, y)) => Arc::ptr_eq(a, b) && x == y,
		_ => false,
	}
}

/// What the axes were made for: the stored size, the window in samples and the picture's size.
type AxesKey = (u32, u32, Option<Window>, u32, u32);

/// What the outline was made for: the edges (held, so the address names them), the gain, the window and the size.
#[derive(Clone)]
struct OutlineKey {
	edges: Arc<EdgeImage>,
	gain: f64,
	window: Option<Window>,
	size: (u32, u32),
}

impl PartialEq for OutlineKey {
	fn eq(&self, other: &Self) -> bool {
		Arc::ptr_eq(&self.edges, &other.edges)
			&& self.gain == other.gain
			&& self.window == other.window
			&& self.size == other.size
	}
}

/// One picture's buffers and bind group, rebuilt as their inputs change; [`draw`](Self::draw) records it.
pub struct GpuPicture {
	uniform: wgpu::Buffer,
	params: Params,
	/// `params` is in the uniform.
	written: bool,
	lut: Option<(Colouring, wgpu::Buffer)>,
	axes: Option<(AxesKey, wgpu::Buffer)>,
	outline: Option<(OutlineKey, wgpu::Buffer)>,
	plane: Option<wgpu::Buffer>,
	group: Option<wgpu::BindGroup>,
	/// The offsets into the axes buffer.
	axes_at: [u32; 6],
}

impl GpuPicture {
	pub fn new(device: &wgpu::Device) -> Self {
		let uniform = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("picture params"),
			size: size_of::<Params>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		Self {
			uniform,
			params: Params::default(),
			written: false,
			lut: None,
			axes: None,
			outline: None,
			plane: None,
			group: None,
			axes_at: [0; 6],
		}
	}

	/// Brings the buffers to `input`; [`place`](Self::place) positions the picture. A table, the axes or the outline
	/// is uploaded when its inputs change, the bind group remade when a buffer does, the parameters written on change.
	pub fn set(
		&mut self,
		pipeline: &PicturePipeline,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		input: &PictureInput<'_>,
	) {
		let PictureInput { plane, shape, layer, colours, view, outline } = *input;
		let mut rebind = self.plane.as_ref() != Some(plane.buffer);
		if rebind {
			self.plane = Some(plane.buffer.clone());
		}
		let colouring = Colouring::of(shape, layer, colours, view);
		let (mode, side, thresholds) = match &colouring {
			Colouring::Palette(_) => (INDEX, 0, 0),
			Colouring::Plain(_) => (PLAIN, 0, 0),
			Colouring::Dithered(table, depth) => (DITHERED, depth.side() as u32, table.entries().len() as u32 * 2),
		};
		if self.lut.as_ref().is_none_or(|(held, _)| !same_tables(held, &colouring)) {
			let words = lut_words(&colouring);
			let buffer = upload(device, queue, self.lut.take().map(|(_, b)| b), "picture lut", &words, &mut rebind);
			self.lut = Some((colouring, buffer));
		}
		let index = shape.format == SampleFormat::Index;
		let (sw, sh) = (shape.width(), shape.height());
		let window = view.sample_window(shape);
		if !index {
			let key = (sw, sh, window, view.width, view.height);
			if self.axes.as_ref().is_none_or(|(k, _)| *k != key) {
				let (x, y) = axes(sw as usize, sh as usize, window, view.width as usize, view.height as usize);
				let (words, at) = axes_words(&x, &y);
				self.axes_at = at;
				let buffer =
					upload(device, queue, self.axes.take().map(|(_, b)| b), "picture axes", &words, &mut rebind);
				self.axes = Some((key, buffer));
			}
		}
		let outline = outline.and_then(|(edges, gain)| additions(gain).map(|added| (edges, gain, added)));
		if let Some((edges, gain, added)) = outline {
			let key = OutlineKey { edges: edges.clone(), gain, window: view.window, size: (view.width, view.height) };
			if self.outline.as_ref().is_none_or(|(k, _)| *k != key) {
				let words: Vec<u32> = edges
					.scaled(view.window, (view.width, view.height))
					.as_chunks::<4>()
					.0
					.iter()
					.map(|e| (0..3).map(|c| added[e[c] as usize].min(255) << (8 * c)).sum())
					.collect();
				let buffer =
					upload(device, queue, self.outline.take().map(|(_, b)| b), "picture outline", &words, &mut rebind);
				self.outline = Some((key, buffer));
			}
		}
		let [x_first, x_offsets, y_first, y_offsets, x_weights, y_weights] = self.axes_at;
		let params = Params {
			origin: self.params.origin,
			size: self.params.size,
			mode,
			first_word: plane.first_word,
			stored_w: sw,
			stored_h: sh,
			grid_w: view.width,
			grid_h: view.height,
			energy: u32::from(shape.sample_kind(layer) == SampleKind::IntensityRoot),
			outline: u32::from(outline.is_some()),
			side,
			thresholds,
			x_first,
			x_offsets,
			y_first,
			y_offsets,
			x_weights,
			y_weights,
			srgb: u32::from(pipeline.srgb),
			_pad: [0; 3],
		};
		self.write(queue, params);
		if rebind || self.group.is_none() {
			let buffers = [
				&self.uniform,
				plane.buffer,
				&self.lut.as_ref().expect("set above").1,
				self.axes.as_ref().map_or(&pipeline.empty, |(_, b)| b),
				self.outline.as_ref().map_or(&pipeline.empty, |(_, b)| b),
			];
			let entries: Vec<_> = buffers
				.iter()
				.enumerate()
				.map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
				.collect();
			self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
				label: Some("picture"),
				layout: &pipeline.layout,
				entries: &entries,
			}));
		}
	}

	/// Places the picture at `origin` with `size`, in framebuffer pixels.
	pub fn place(&mut self, queue: &wgpu::Queue, origin: [f32; 2], size: [f32; 2]) {
		self.write(queue, Params { origin, size, ..self.params });
	}

	fn write(&mut self, queue: &wgpu::Queue, params: Params) {
		if !self.written || self.params != params {
			queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&params));
			self.params = params;
			self.written = true;
		}
	}

	/// Draws the picture over its rectangle, clipped by the pass's viewport and scissor; nothing before a `set`.
	pub fn draw(&self, pipeline: &PicturePipeline, pass: &mut wgpu::RenderPass<'_>) {
		let Some(group) = &self.group else { return };
		pass.set_pipeline(&pipeline.pipeline);
		pass.set_bind_group(0, group, &[]);
		pass.draw(0..3, 0..1);
	}
}

/// The LUT buffer's words: the half table's RGBA8 entries; the deep table's entries, low word first, then the
/// Bayer thresholds; or the palette's entries.
fn lut_words(colouring: &Colouring) -> Vec<u32> {
	match colouring {
		Colouring::Palette(p) => p.entries().iter().map(|&e| u32::from_le_bytes(e)).collect(),
		Colouring::Plain(t) => t.entries().iter().map(|&e| u32::from_le_bytes(e)).collect(),
		Colouring::Dithered(t, depth) => t
			.entries()
			.iter()
			.flat_map(|&e| [e as u32, (e >> 32) as u32])
			.chain(depth.thresholds().into_iter().map(u32::from))
			.collect(),
	}
}

/// The axes' words, each axis's first samples, offsets and weights, and where each part starts: x first, x
/// offsets, y first, y offsets, x weights, y weights.
fn axes_words(x: &ResampleAxis, y: &ResampleAxis) -> (Vec<u32>, [u32; 6]) {
	let parts = |axis: &ResampleAxis| {
		let mut first = Vec::with_capacity(axis.target());
		let mut offsets = Vec::with_capacity(axis.target() + 1);
		let mut weights = Vec::new();
		for t in 0..axis.target() {
			let (f, w) = axis.weights_of(t);
			first.push(f as u32);
			offsets.push(weights.len() as u32);
			weights.extend(w.iter().map(|w| w.to_bits()));
		}
		offsets.push(weights.len() as u32);
		(first, offsets, weights)
	};
	let (xf, xo, xw) = parts(x);
	let (yf, yo, yw) = parts(y);
	let mut at = [0; 6];
	let mut words = Vec::with_capacity(xf.len() + xo.len() + yf.len() + yo.len() + xw.len() + yw.len());
	for (i, part) in [xf, xo, yf, yo, xw, yw].into_iter().enumerate() {
		at[i] = words.len() as u32;
		words.extend(part);
	}
	(words, at)
}

fn storage_buffer(device: &wgpu::Device, label: &str, bytes: u64) -> wgpu::Buffer {
	device.create_buffer(&wgpu::BufferDescriptor {
		label: Some(label),
		size: bytes.next_multiple_of(4).max(16),
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	})
}

/// `words` written into `old` when it holds them, else into a new buffer, which sets `rebind`.
fn upload(
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	old: Option<wgpu::Buffer>,
	label: &str,
	words: &[u32],
	rebind: &mut bool,
) -> wgpu::Buffer {
	let bytes = (words.len() * 4) as u64;
	let buffer = match old {
		Some(b) if b.size() >= bytes => b,
		_ => {
			*rebind = true;
			storage_buffer(device, label, bytes)
		}
	};
	queue.write_buffer(&buffer, 0, bytemuck::cast_slice(words));
	buffer
}
