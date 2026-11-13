#![cfg(feature = "gpu")]
//! The GPU's colouring of stored planes (render/gpu.rs) against the CPU's (render/layer.rs `paint` and the outline
//! blend), for every kind of plane and colouring.
//!
//! Tolerances: a half picture's pixel may differ where the card fused a multiply-add of the resampling, which moves
//! the sum by an f32 ulp and, at an f16 rounding boundary, picks the neighbouring table entry; the gradients here
//! move at most one level between neighbouring entries, and a dithered channel may also land on the other side
//! of its threshold: at most 2 levels, on at most 1 % of the pixels. A scaled index picture is blended in f32 where
//! the reference blends in f64: 1 level. Everything else is exact. Every test returns early, with the reason
//! printed, when no adapter or device is available; a software one (Mesa's lavapipe in CI) is fine.

use std::sync::Arc;

use wavedude_core::engine::{FrameShape, SampleFormat};
use wavedude_core::f16;
use wavedude_core::model::{ColorScheme, Layer, Rgb, Stop};
use wavedude_core::render::ScreenRenderer;
use wavedude_core::render::gpu::{DevicePlane, GpuPicture, PictureInput, PicturePipeline};
use wavedude_core::render::layer::{LayerColours, View, paint};
use wavedude_core::render::screen::Window;
use wavedude_core::scene::glowing_edges::{EdgeImage, blend_additive};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct Gpu {
	device: wgpu::Device,
	queue: wgpu::Queue,
	pipeline: PicturePipeline,
}

fn gpu() -> Option<Gpu> {
	let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
	let adapter = [false, true].into_iter().find_map(|force_fallback_adapter| {
		pollster::block_on(
			instance.request_adapter(&wgpu::RequestAdapterOptions { force_fallback_adapter, ..Default::default() }),
		)
		.ok()
	});
	let Some(adapter) = adapter else {
		eprintln!("skipped: no adapter");
		return None;
	};
	eprintln!("adapter: {} ({:?})", adapter.get_info().name, adapter.get_info().backend);
	let (device, queue) = match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())) {
		Ok(d) => d,
		Err(e) => {
			eprintln!("skipped: no device: {e}");
			return None;
		}
	};
	let pipeline = PicturePipeline::new(&device, FORMAT);
	Some(Gpu { device, queue, pipeline })
}

/// xorshift32 in [0, 1).
fn noise(state: &mut u32) -> f32 {
	*state ^= *state << 13;
	*state ^= *state >> 17;
	*state ^= *state << 5;
	(*state >> 8) as f32 / (1 << 24) as f32
}

fn scheme() -> ColorScheme {
	let stop = |at, r, g, b| Stop { at, color: Rgb::new(r, g, b) };
	ColorScheme {
		stops: vec![stop(0.0, 16, 32, 96), stop(0.5, 240, 224, 160), stop(1.0, 255, 32, 0)],
		..ColorScheme::default()
	}
}

fn colours(brightness: f64) -> LayerColours {
	LayerColours { scheme: scheme(), black_level: 0.1, brightness }
}

fn shape(w: u32, h: u32, divisor: u32, format: SampleFormat, intensity_root: bool) -> FrameShape {
	FrameShape {
		canvas_width: w,
		canvas_height: h,
		divisor,
		format,
		field_brightness: 2.0,
		intensity_brightness: 3.0,
		intensity_root,
	}
}

/// A plane of `shape`'s `layer`: smooth waves with noise, and for the field a few NaN, infinite and subnormal
/// samples; an index plane covers every index.
fn plane(shape: &FrameShape, layer: Layer) -> Vec<u8> {
	let (w, h) = (shape.width() as usize, shape.height() as usize);
	let mut state = 0x2545_f491;
	if shape.format == SampleFormat::Index {
		return (0..w * h).map(|i| ((i * 7 + (noise(&mut state) * 9.0) as usize) % 256) as u8).collect();
	}
	let halves: Vec<f16> = (0..w * h)
		.map(|i| {
			let (x, y) = ((i % w) as f32, (i / w) as f32);
			let wave = (x * 0.31).sin() * (y * 0.23).cos() * 0.4 + (noise(&mut state) - 0.5) * 0.15;
			let v = match layer {
				Layer::Field => match i % 97 {
					3 => f32::NAN,
					5 => f32::INFINITY,
					7 => f32::NEG_INFINITY,
					11 => 1e-6,
					_ => wave,
				},
				Layer::Intensity => wave * wave * 2.0,
			};
			f16::from_f32(v)
		})
		.collect();
	bytemuck::cast_slice(&halves).to_vec()
}

fn edges(shape: &FrameShape) -> Arc<EdgeImage> {
	let (w, h) = (shape.canvas_width, shape.canvas_height);
	let mut state = 0x9e37_79b9;
	let rgba = (0..w * h * 4).map(|i| if i % 4 == 3 { 255 } else { (noise(&mut state) * 256.0) as u8 }).collect();
	Arc::new(EdgeImage { width: w, height: h, rgba })
}

/// What the presenter makes on the CPU: the picture of `view`, the outline blended.
fn cpu(
	bytes: &[u8],
	shape: &FrameShape,
	layer: Layer,
	colours: &LayerColours,
	view: &View,
	outline: Option<(&Arc<EdgeImage>, f64)>,
) -> Vec<u8> {
	let mut out = vec![0; view.bytes()];
	paint(bytes, shape, layer, colours, view, &mut ScreenRenderer::new(1, 1, 1, 1), &mut out);
	if let Some((e, gain)) = outline {
		blend_additive(&mut out, &e.scaled(view.window, (view.width, view.height)), gain);
	}
	out
}

/// A picture's plane, layer, colours, view and outline.
type Case = (FrameShape, Layer, LayerColours, View, Option<(Arc<EdgeImage>, f64)>);

/// The GPU's picture at `size` pixels, the plane after a few words of something else in its buffer.
fn render(g: &Gpu, picture: &mut GpuPicture, bytes: &[u8], case: &Case, size: (u32, u32)) -> Vec<u8> {
	const BEFORE: usize = 5;
	let mut words = vec![0xdead_beef_u32; BEFORE + bytes.len().div_ceil(4)];
	bytemuck::cast_slice_mut::<u32, u8>(&mut words[BEFORE..])[..bytes.len()].copy_from_slice(bytes);
	let buffer = wgpu::util::DeviceExt::create_buffer_init(
		&g.device,
		&wgpu::util::BufferInitDescriptor {
			label: None,
			contents: bytemuck::cast_slice(&words),
			usage: wgpu::BufferUsages::STORAGE,
		},
	);
	let plane = DevicePlane { buffer: &buffer, first_word: BEFORE as u32 };
	let (shape, layer, colours, view, outline) = case;
	let input = PictureInput {
		plane,
		shape,
		layer: *layer,
		colours,
		view,
		outline: outline.as_ref().map(|(e, gain)| (e, *gain)),
	};
	picture.set(&g.pipeline, &g.device, &g.queue, &input);
	picture.place(&g.queue, [0.0, 0.0], [size.0 as f32, size.1 as f32]);
	let extent = wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 };
	let target = g.device.create_texture(&wgpu::TextureDescriptor {
		label: None,
		size: extent,
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: FORMAT,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
		view_formats: &[],
	});
	let row = (size.0 * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
	let read = g.device.create_buffer(&wgpu::BufferDescriptor {
		label: None,
		size: u64::from(row * size.1),
		usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	});
	let mut encoder = g.device.create_command_encoder(&Default::default());
	{
		let view = target.create_view(&Default::default());
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: &view,
				depth_slice: None,
				resolve_target: None,
				ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
			})],
			..Default::default()
		});
		picture.draw(&g.pipeline, &mut pass);
	}
	encoder.copy_texture_to_buffer(
		target.as_image_copy(),
		wgpu::TexelCopyBufferInfo {
			buffer: &read,
			layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
		},
		extent,
	);
	g.queue.submit([encoder.finish()]);
	read.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
	g.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
	let mapped = read.slice(..).get_mapped_range().unwrap();
	mapped.chunks(row as usize).flat_map(|r| &r[..size.0 as usize * 4]).copied().collect()
}

/// Asserts every channel within `levels` and at most `share` of the pixels differing.
fn assert_close(what: &str, gpu: &[u8], cpu: &[u8], levels: u8, share: f64) {
	assert_eq!(gpu.len(), cpu.len(), "{what}: sizes");
	let (mut differing, mut worst) = (0, 0);
	for (g, c) in gpu.chunks(4).zip(cpu.chunks(4)) {
		let d = g.iter().zip(c).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
		differing += usize::from(d > 0);
		worst = worst.max(d);
	}
	let pixels = gpu.len() / 4;
	eprintln!("{what}: {differing} of {pixels} pixels differ, by at most {worst}");
	assert!(worst <= levels, "{what}: a channel differs by {worst}");
	assert!(differing as f64 <= share * pixels as f64, "{what}: {differing} of {pixels} pixels differ");
}

/// A half plane's case: the GPU's picture against the CPU's.
#[allow(clippy::too_many_arguments, reason = "one case's inputs")]
fn half_case(
	g: &Gpu,
	what: &str,
	shape: FrameShape,
	layer: Layer,
	device: (u32, u32),
	window: Option<Window>,
	dither_bits: u32,
	outline: bool,
) {
	let bytes = plane(&shape, layer);
	let colours = colours(if layer == Layer::Field { 2.0 } else { 3.0 });
	let view = View::of(&shape, device, window, dither_bits);
	let edges = outline.then(|| (edges(&shape), 1.5));
	let expected = cpu(&bytes, &shape, layer, &colours, &view, edges.as_ref().map(|(e, gain)| (e, *gain)));
	let mut picture = GpuPicture::new(&g.device);
	let got = render(g, &mut picture, &bytes, &(shape, layer, colours.clone(), view, edges.clone()), device);
	assert_close(what, &got, &expected, 2, 0.01);
}

#[test]
fn half_planes_shrunk_enlarged_and_zoomed_colour_as_on_the_cpu() {
	let Some(g) = gpu() else { return };
	let half = SampleFormat::Half;
	half_case(&g, "field, shrunk, outline", shape(97, 61, 1, half, true), Layer::Field, (40, 25), None, 0, true);
	half_case(
		&g,
		"intensity, enlarged, zoomed, outline",
		shape(31, 17, 1, half, false),
		Layer::Intensity,
		(80, 50),
		Some((3.5, 2.0, 20.0, 11.0)),
		0,
		true,
	);
	half_case(
		&g,
		"intensity root at divisor 2, zoomed",
		shape(120, 70, 2, half, true),
		Layer::Intensity,
		(45, 30),
		Some((10.0, 6.0, 90.0, 50.0)),
		0,
		false,
	);
	half_case(&g, "field, same size", shape(64, 48, 1, half, true), Layer::Field, (64, 48), None, 0, false);
}

#[test]
fn dithered_half_planes_colour_as_on_the_cpu() {
	let Some(g) = gpu() else { return };
	for bits in [10, 12, 14, 16] {
		let shape = shape(64, 48, 1, SampleFormat::Half, true);
		half_case(&g, &format!("field dithered at {bits} bits"), shape, Layer::Field, (100, 70), None, bits, true);
		half_case(
			&g,
			&format!("intensity root dithered at {bits} bits"),
			shape,
			Layer::Intensity,
			(50, 31),
			None,
			bits,
			false,
		);
	}
}

/// `rgba` of `from` pixels scaled to `to` bilinearly between pixel centres, the edges held, as a sampler does.
fn bilinear(rgba: &[u8], from: (u32, u32), to: (u32, u32)) -> Vec<u8> {
	let (fw, fh) = (from.0 as i64, from.1 as i64);
	let at =
		|x: i64, y: i64, c: usize| f64::from(rgba[((y.clamp(0, fh - 1) * fw + x.clamp(0, fw - 1)) * 4) as usize + c]);
	let mut out = Vec::with_capacity((to.0 * to.1 * 4) as usize);
	for y in 0..to.1 {
		for x in 0..to.0 {
			let u = (f64::from(x) + 0.5) * from.0 as f64 / f64::from(to.0) - 0.5;
			let v = (f64::from(y) + 0.5) * from.1 as f64 / f64::from(to.1) - 0.5;
			let (x0, y0) = (u.floor() as i64, v.floor() as i64);
			let (a, b) = (u - u.floor(), v - v.floor());
			for c in 0..3 {
				let top = at(x0, y0, c) * (1.0 - a) + at(x0 + 1, y0, c) * a;
				let bottom = at(x0, y0 + 1, c) * (1.0 - a) + at(x0 + 1, y0 + 1, c) * a;
				out.push((top * (1.0 - b) + bottom * b).round() as u8);
			}
			out.push(255);
		}
	}
	out
}

#[test]
fn index_planes_colour_through_the_palette_as_on_the_cpu() {
	let Some(g) = gpu() else { return };
	for (layer, root, divisor) in [(Layer::Field, true, 1), (Layer::Intensity, true, 2), (Layer::Intensity, false, 1)] {
		let shape = shape(50, 30, divisor, SampleFormat::Index, root);
		let bytes = plane(&shape, layer);
		let colours = colours(1.0);
		let view = View::of(&shape, (130, 77), Some((5.0, 5.0, 10.0, 10.0)), 16);
		let stored = (shape.width(), shape.height());
		assert_eq!((view.width, view.height, view.window), (stored.0, stored.1, None), "the stored size, whole");
		let e = edges(&shape);
		let expected = cpu(&bytes, &shape, layer, &colours, &view, Some((&e, 2.0)));
		let mut picture = GpuPicture::new(&g.device);
		let case = (shape, layer, colours.clone(), view, Some((e.clone(), 2.0)));
		let got = render(&g, &mut picture, &bytes, &case, stored);
		assert_close(&format!("{layer:?} /{divisor} index at the stored size"), &got, &expected, 0, 0.0);
		let got = render(&g, &mut picture, &bytes, &case, (130, 77));
		let scaled = bilinear(&expected, stored, (130, 77));
		assert_close(&format!("{layer:?} /{divisor} index scaled"), &got, &scaled, 1, 0.1);
	}
}

#[test]
fn a_picture_follows_its_inputs_as_they_change() {
	let Some(g) = gpu() else { return };
	let shape = shape(40, 30, 1, SampleFormat::Half, true);
	let bytes = plane(&shape, Layer::Field);
	let mut picture = GpuPicture::new(&g.device);
	for (brightness, device, bits, outline) in
		[(2.0, (40, 30), 0, false), (5.0, (25, 18), 12, true), (2.0, (40, 30), 0, false)]
	{
		let colours = colours(brightness);
		let view = View::of(&shape, device, None, bits);
		let edges = outline.then(|| (edges(&shape), 1.0));
		let expected = cpu(&bytes, &shape, Layer::Field, &colours, &view, edges.as_ref().map(|(e, gain)| (e, *gain)));
		let got =
			render(&g, &mut picture, &bytes, &(shape, Layer::Field, colours.clone(), view, edges.clone()), device);
		assert_close(&format!("brightness {brightness}, {device:?}, {bits} bits"), &got, &expected, 2, 0.01);
	}
}

/// docs/measurements.md, preview colouring: a new frame's pictures on the CPU (the presenter's paint and outline
/// blend) against the GPU (a picture set to the next of the device's frames, drawn and waited for).
#[test]
#[ignore = "a timing, run in release by hand"]
fn preview_colouring_time() {
	use std::time::Instant;
	const FRAMES: u32 = 60;
	let Some(g) = gpu() else { return };
	let shape = shape(2880, 1080, 1, SampleFormat::Half, true);
	let device = (1600, 600);
	let view = View::of(&shape, device, None, 0);
	let e = edges(&shape);
	for layer in [Layer::Field, Layer::Intensity] {
		let bytes = plane(&shape, layer);
		let colours = colours(2.0);
		let scaled = e.scaled(view.window, (view.width, view.height));
		let mut screen = ScreenRenderer::new(1, 1, 1, 1);
		let mut out = vec![0; view.bytes()];
		let started = Instant::now();
		for _ in 0..FRAMES {
			paint(&bytes, &shape, layer, &colours, &view, &mut screen, &mut out);
			blend_additive(&mut out, &scaled, 1.5);
		}
		let cpu = started.elapsed() / FRAMES;
		let planes: Vec<wgpu::Buffer> = (0..4)
			.map(|_| {
				wgpu::util::DeviceExt::create_buffer_init(
					&g.device,
					&wgpu::util::BufferInitDescriptor {
						label: None,
						contents: &bytes,
						usage: wgpu::BufferUsages::STORAGE,
					},
				)
			})
			.collect();
		let target = g.device.create_texture(&wgpu::TextureDescriptor {
			label: None,
			size: wgpu::Extent3d { width: device.0, height: device.1, depth_or_array_layers: 1 },
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: FORMAT,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			view_formats: &[],
		});
		let target = target.create_view(&Default::default());
		let mut picture = GpuPicture::new(&g.device);
		let mut frame = |i: u32| {
			let input = PictureInput {
				plane: DevicePlane { buffer: &planes[i as usize % planes.len()], first_word: 0 },
				shape: &shape,
				layer,
				colours: &colours,
				view: &view,
				outline: Some((&e, 1.5)),
			};
			picture.set(&g.pipeline, &g.device, &g.queue, &input);
			picture.place(&g.queue, [0.0, 0.0], [device.0 as f32, device.1 as f32]);
			let mut encoder = g.device.create_command_encoder(&Default::default());
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
				picture.draw(&g.pipeline, &mut pass);
			}
			g.queue.submit([encoder.finish()]);
			g.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
		};
		frame(FRAMES);
		let started = Instant::now();
		(0..FRAMES).for_each(&mut frame);
		let gpu = started.elapsed() / FRAMES;
		eprintln!(
			"{layer:?} {}x{} at {device:?}: CPU {cpu:?}, GPU {gpu:?} a picture",
			shape.canvas_width, shape.canvas_height
		);
	}
}
