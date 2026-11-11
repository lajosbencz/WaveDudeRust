#![cfg(feature = "gpu")]
//! The GPU engine against the CPU engine (docs/physics.md, Precision; spec/05-deviations.md).
//!
//! Every test returns early, with the reason printed, when no GPU adapter is available.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use wavedude_core::engine::capture;
use wavedude_core::engine::cpu::CpuEngine;
use wavedude_core::engine::gpu::{BATCH_STEPS, GpuEngine, MediumLayout, context};
use wavedude_core::engine::{
	Emitter, EngineOptions, FrameShape, ProbeCells, ProbeReadings, SampleFormat, SceneData, Snapshot, WaveEngine,
};
use wavedude_core::f16;
use wavedude_core::model::{Edges, Layer, PulseShape, Scene, Stencil};

const STENCILS: [Stencil; 2] = [Stencil::NinePoint, Stencil::FourthOrder];
const EDGES: [Edges; 2] = [Edges::Open, Edges::Reflective];
const W: u32 = 128;
const H: u32 = 96;
const RIM: i32 = 16;

fn gpu_available() -> bool {
	match context() {
		Ok(c) => {
			eprintln!("GPU: {} ({:?})", c.info.name, c.info.backend);
			true
		}
		Err(e) => {
			eprintln!("skipped: {e}");
			false
		}
	}
}

fn options(stencil: Stencil) -> EngineOptions {
	EngineOptions {
		stencil,
		source_frequency_scale: 1.0,
		image_source_scale: 1.0,
		time_decay: 10.0,
		intensity_scale: 1.0,
		track_intensity: true,
	}
}

fn scene_settings(edges: Edges) -> Scene {
	Scene { damping_edge: -RIM, edges, image_sources: true, ..Scene::default() }
}

/// A painted-style canvas: index 1, a block of index 2.55, a mirror bar, an absorbing patch and green
/// image sources at two levels. `red` is the background's red level.
fn pixels(red: u8) -> Vec<u8> {
	let mut px = Vec::with_capacity((W * H * 4) as usize);
	for y in 0..H {
		for x in 0..W {
			let mut p = [red, 0, 0, 255];
			if (70..100).contains(&x) && (30..70).contains(&y) {
				p[0] = 255;
			}
			if (20..24).contains(&x) && (55..90).contains(&y) {
				p[0] = 0;
			}
			if (100..120).contains(&x) && (5..15).contains(&y) {
				p[2] = 80;
			}
			if (x == 10 && (10..20).contains(&y)) || (x == 50 && y == 80) {
				p[1] = if y < 15 { 60 } else { 90 };
			}
			px.extend(p);
		}
	}
	px
}

fn scene(edges: Edges, red: u8) -> SceneData {
	SceneData::from_pixels(&pixels(red), W, H, &scene_settings(edges))
}

fn emitter(x: i32, y: i32, level: f64) -> Emitter {
	Emitter {
		x: f64::from(x),
		y: f64::from(y),
		level,
		amplitude: 1.0,
		phase_radians: 0.3,
		pulsed: false,
		shape: PulseShape::Gaussian,
		on_width_cycles: 3,
		off_width_cycles: 4,
		off_width_base: 4,
		randomize_off_width: 0,
		detach_after_step: None,
		rng_seed: NonZeroU32::new(7).unwrap(),
	}
}

/// Continuous, random-gap pulsed, smoothed square, detaching, one on an image source's cell, two
/// sharing a cell, and one outside the canvas.
fn emitters() -> Vec<Emitter> {
	vec![
		emitter(30, 40, 100.0),
		Emitter {
			pulsed: true,
			randomize_off_width: 3,
			rng_seed: NonZeroU32::new(12345).unwrap(),
			..emitter(60, 20, 80.0)
		},
		Emitter { pulsed: true, shape: PulseShape::SmoothedSquare, ..emitter(40, 70, 120.0) },
		Emitter { detach_after_step: Some(300), ..emitter(110, 80, 70.0) },
		Emitter { amplitude: 0.5, ..emitter(50, 80, 90.0) },
		emitter(90, 85, 60.0),
		Emitter { phase_radians: 1.7, ..emitter(90, 85, 65.0) },
		emitter(-3, 10, 50.0),
	]
}

fn engines(stencil: Stencil, edges: Edges, layout: MediumLayout) -> (CpuEngine, GpuEngine) {
	let mut cpu = CpuEngine::new(scene(edges, 100), options(stencil));
	let mut gpu = GpuEngine::with_layout(scene(edges, 100), options(stencil), layout).expect("GPU engine");
	cpu.set_emitters(&emitters());
	gpu.set_emitters(&emitters());
	(cpu, gpu)
}

fn run(engine: &mut impl WaveEngine, steps: usize) {
	for _ in 0..steps {
		engine.advance_one_step().unwrap();
	}
}

fn peak(values: &[f32]) -> f64 {
	values.iter().fold(0f64, |m, &v| m.max((v as f64).abs()))
}

/// Largest difference relative to the reference's peak.
fn relative(a: &[f32], reference: &[f32]) -> f64 {
	let diff = a.iter().zip(reference).fold(0f64, |m, (&x, &y)| m.max((x as f64 - y as f64).abs()));
	diff / peak(reference).max(f64::MIN_POSITIVE)
}

fn roots(values: &[f32]) -> Vec<f32> {
	values.iter().map(|v| v.max(0.0).sqrt()).collect()
}

#[test]
fn parity_with_the_cpu_engine() {
	if !gpu_available() {
		return;
	}
	let mut cases: Vec<_> = STENCILS.iter().flat_map(|&s| EDGES.map(|e| (s, e, MediumLayout::Compact))).collect();
	cases.push((Stencil::FourthOrder, Edges::Open, MediumLayout::Plain));
	for (stencil, edges, layout) in cases {
		let (mut cpu, mut gpu) = engines(stencil, edges, layout);
		assert_eq!(gpu.sub_steps(), cpu.sub_steps());
		assert_eq!(gpu.medium_layout(), layout);
		run(&mut cpu, 1000);
		run(&mut gpu, 1000);
		let (cf, ci) = cpu.prepare_frame();
		let (gf, gi) = gpu.prepare_frame().unwrap();
		assert!(peak(&cf) > 1e-3, "the scene is excited");
		let field = relative(&gf, &cf);
		let intensity = relative(&roots(&gi), &roots(&ci));
		let identical = cf.iter().zip(&gf).filter(|(a, b)| a.to_bits() == b.to_bits()).count();
		eprintln!(
			"{stencil:?} {edges:?} {layout:?}: field {field:.2e}, sqrt(I) {intensity:.2e}, {identical} of {} cells bit-identical",
			cf.len()
		);
		assert!(field <= 1e-4, "{stencil:?} {edges:?}: field differs by {field:e} of its peak");
		assert!(intensity <= 1e-3, "{stencil:?} {edges:?}: sqrt(I) differs by {intensity:e} of its peak");
	}
}

fn shape(divisor: u32, format: SampleFormat) -> FrameShape {
	FrameShape {
		canvas_width: W,
		canvas_height: H,
		divisor,
		format,
		field_brightness: 4.0,
		intensity_brightness: 6.0,
		intensity_root: true,
	}
}

fn mismatches(a: &[u8], b: &[u8], format: SampleFormat) -> (usize, i32) {
	assert_eq!(a.len(), b.len());
	match format {
		SampleFormat::Index => a.iter().zip(b).fold((0, 0), |(n, m), (&x, &y)| {
			let d = (x as i32 - y as i32).abs();
			(n + (d > 0) as usize, m.max(d))
		}),
		SampleFormat::Half => {
			let (a, b): (&[u16], &[u16]) = (bytemuck::cast_slice(a), bytemuck::cast_slice(b));
			a.iter().zip(b).fold((0, 0), |(n, m), (&x, &y)| {
				let d = (x as i32 - y as i32).abs();
				(n + (d > 0) as usize, m.max(d))
			})
		}
	}
}

/// Open edges without a rim (Mur), behind an inside damping band of 6 (its outer ring d = 0) and
/// without one, at one sub-step (background index 1.25, Courant 0.8) and two (index 1): the GPU stays
/// bounded over 3000 steps and within the parity tolerance of the CPU after 1000.
#[test]
fn open_edges_without_a_rim_match_the_cpu() {
	if !gpu_available() {
		return;
	}
	for band in [6, 0] {
		for red in [125, 100] {
			for stencil in STENCILS {
				let settings =
					Scene { damping_edge: band, edges: Edges::Open, image_sources: true, ..Scene::default() };
				let data = SceneData::from_pixels(&pixels(red), W, H, &settings);
				let mut cpu = CpuEngine::new(data.clone(), options(stencil));
				let mut gpu = GpuEngine::new(data, options(stencil)).expect("GPU engine");
				cpu.set_emitters(&emitters());
				gpu.set_emitters(&emitters());
				run(&mut cpu, 1000);
				run(&mut gpu, 1000);
				let (cf, _) = cpu.prepare_frame();
				let (gf, _) = gpu.prepare_frame().unwrap();
				let field = relative(&gf, &cf);
				run(&mut gpu, 2000);
				let late = gpu.prepare_frame().unwrap().0;
				let top = if late.iter().all(|v| v.is_finite()) { peak(&late) } else { f64::INFINITY };
				eprintln!(
					"band {band}, red {red}, {stencil:?}, {} sub-steps: field {field:.2e} of the CPU's peak at 1000, \
					 peak {top:.3} at 3000",
					gpu.sub_steps()
				);
				assert!(field <= 1e-4, "band {band}, red {red}, {stencil:?}: field differs by {field:e}");
				assert!(top < 10.0, "band {band}, red {red}, {stencil:?}: peak {top}");
			}
		}
	}
}

/// Example18 (open edges, a damping band of 32, no rim) stays bounded on the GPU over 3000 steps,
/// as on the CPU (tests/physics.rs).
#[test]
fn example18_stays_bounded() {
	if !gpu_available() {
		return;
	}
	let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/examples/Example18_XOR_AND_OR_logic.wdproj");
	let project = wavedude_core::io::load_project(&path).unwrap();
	let px = wavedude_core::scene::compose::compose_scene(&project).expect("painted parts");
	let data = SceneData::from_pixels(&px.rgba, px.width, px.height, &project.scene);
	let sim = &project.simulation;
	let opts = EngineOptions {
		stencil: sim.stencil,
		source_frequency_scale: sim.source_frequency_scale,
		..options(sim.stencil)
	};
	let mut gpu = GpuEngine::new(data, opts).expect("GPU engine");
	gpu.set_emitters(&wavedude_core::engine::emitters::emitters_of(&project));
	let mut top = 0f64;
	for step in (100..=3000).step_by(100) {
		run(&mut gpu, 100);
		let f = gpu.prepare_frame().unwrap().0;
		top = top.max(if f.iter().all(|v| v.is_finite()) { peak(&f) } else { f64::INFINITY });
		assert!(top < 10.0, "step {step}: peak {top}");
	}
	eprintln!("Example18 on the GPU, {} sub-steps: peak {top:.3} over 3000 steps", gpu.sub_steps());
}

/// Mirrors on the GPU as on the CPU (tests/physics.rs, 8b and 8c): a 32 x 32 reflective box with a
/// mirror ring two cells thick stays bounded over 30,000 steps after a 3-cycle burst on both stencils,
/// and a one-pixel diagonal mirror line splitting a 64 x 64 box lets under 1e-6 of a bump's square
/// sum across in 600 steps.
#[test]
fn mirrors_hold() {
	if !gpu_available() {
		return;
	}
	let reflective = Scene { damping_edge: 0, edges: Edges::Reflective, ..Scene::default() };
	let canvas = |n: usize, mirror: &dyn Fn(usize, usize) -> bool| -> Vec<u8> {
		(0..n * n).flat_map(|i| if mirror(i % n, i / n) { [0, 0, 0, 255] } else { [100, 0, 0, 255] }).collect()
	};
	let n = 32usize;
	let (a, b) = (n / 8, n - n / 8 - 2);
	let wall = |v: usize| (a..a + 2).contains(&v) || (b..b + 2).contains(&v);
	let span = a..b + 2;
	let ring = canvas(n, &|x, y| (wall(x) && span.contains(&y)) || (wall(y) && span.contains(&x)));
	let line = canvas(64, &|x, y| x == y);
	let start: Vec<f32> = (0..64 * 64)
		.map(|i| {
			let (x, y) = ((i % 64) as f64, (i / 64) as f64);
			if x > y { (-((x - 40.0).powi(2) + (y - 20.0).powi(2)) / 18.0).exp() as f32 } else { 0.0 }
		})
		.collect();
	for stencil in STENCILS {
		let opts = EngineOptions { track_intensity: false, ..options(stencil) };
		let data = SceneData::from_pixels(&ring, n as u32, n as u32, &reflective);
		let mut gpu = GpuEngine::new(data, opts.clone()).expect("GPU engine");
		gpu.set_emitters(&[Emitter {
			pulsed: true,
			off_width_cycles: 100_000,
			off_width_base: 100_000,
			detach_after_step: Some(200),
			phase_radians: 0.0,
			..emitter(15, 18, 100.0)
		}]);
		let (mut early, mut late) = (0f64, 0f64);
		for step in 1..=30_000 {
			gpu.advance_one_step().unwrap();
			if step == 1000 || step == 500 {
				early = early.max(peak(&gpu.prepare_frame().unwrap().0));
			} else if step >= 25_000 && step % 1000 == 0 {
				let f = gpu.prepare_frame().unwrap().0;
				late = late.max(if f.iter().all(|v| v.is_finite()) { peak(&f) } else { f64::INFINITY });
			}
		}
		eprintln!("{stencil:?}: mirror cavity |u| {early:.3} early, {late:.3} from step 25,000");
		assert!(early > 0.05 && late < 4.0 * early, "{stencil:?}: {early} then {late}");

		let data = SceneData::from_pixels(&line, 64, 64, &reflective);
		let mut gpu = GpuEngine::new(data, opts).expect("GPU engine");
		gpu.set_field(&start, &start);
		run(&mut gpu, 600);
		let f = gpu.prepare_frame().unwrap().0;
		let (across, total) = f.iter().enumerate().fold((0f64, 0f64), |(a, t), (i, &v)| {
			let v2 = (v as f64).powi(2);
			(a + if i % 64 < i / 64 { v2 } else { 0.0 }, t + v2)
		});
		eprintln!("{stencil:?}: {:.2e} of the square sum across the diagonal line", across / total);
		assert!(total > 0.0 && across < 1e-6 * total, "{stencil:?}: {across} of {total}");
	}
}

#[test]
fn captured_planes_match() {
	if !gpu_available() {
		return;
	}
	for stencil in STENCILS {
		let (mut cpu, mut gpu) = engines(stencil, Edges::Open, MediumLayout::Compact);
		run(&mut cpu, 400);
		run(&mut gpu, 400);
		let (field, intensity) = gpu.prepare_frame().unwrap();
		let at = |values: &[f32], x: usize, y: usize| values[y * W as usize + x];
		for format in [SampleFormat::Index, SampleFormat::Half] {
			for divisor in [1, 2, 4] {
				let shape = shape(divisor, format);
				let g = gpu.capture_frame(&shape).unwrap();
				let c = cpu.capture_frame(&shape);
				assert_eq!(g.step, c.step);
				for layer in [Layer::Field, Layer::Intensity] {
					let values = match layer {
						Layer::Field => &field,
						Layer::Intensity => &intensity,
					};
					let mut own = Vec::new();
					capture::plane_rows(&shape, layer, &mut own, |y, row| {
						row.iter_mut().enumerate().for_each(|(x, v)| *v = at(values, x, y));
					});
					let (conversion, _) = mismatches(g.plane(layer), &own, format);
					let (n, max) = mismatches(g.plane(layer), c.plane(layer), format);
					eprintln!(
						"{stencil:?} {format:?} /{divisor} {layer:?}: conversion of the GPU's own state {conversion} \
						 differing, against the CPU {n} of {} differing by at most {max}",
						shape.plane_samples()
					);
					assert_eq!(conversion, 0, "the capture converts the GPU's state as engine/capture.rs does");
					if format == SampleFormat::Index {
						assert!(max <= 1, "{format:?} /{divisor} {layer:?}: index differs by {max}");
					}
				}
			}
		}
	}
}

#[test]
fn half_rounding_is_the_cpus() {
	if !gpu_available() {
		return;
	}
	let (w, h) = (64u32, 32u32);
	let settings = Scene { damping_edge: 0, edges: Edges::Reflective, ..Scene::default() };
	let mut gpu = GpuEngine::new(SceneData::blank(w, h, &settings), options(Stencil::NinePoint)).expect("GPU engine");
	let special = [
		0.0f32,
		-0.0,
		65504.0,
		65519.99,
		65520.0,
		-70000.0,
		f32::INFINITY,
		f32::NAN,
		6.1035156e-5,
		6.1e-5,
		5.9604645e-8,
		2.9802322e-8,
		2.9802326e-8,
		1.0 + 1.0 / 2048.0,
		1.0 + 3.0 / 2048.0,
		-0.25,
	];
	let values: Vec<f32> = (0..(w * h) as usize)
		.map(|i| match special.get(i) {
			Some(&v) => v,
			None => {
				f32::from_bits(0x3300_0000u32.wrapping_add((i as u32).wrapping_mul(2_654_435_761) % 0x1500_0000))
					* if i % 2 == 0 { 1.0 } else { -1.0 }
			}
		})
		.collect();
	gpu.set_field(&values, &values);
	let mut shape = FrameShape { canvas_width: w, canvas_height: h, ..shape(1, SampleFormat::Half) };
	let snap = gpu.capture_frame(&shape).unwrap();
	let halves = snap.halves(Layer::Field);
	for (k, (&v, &h)) in values.iter().zip(halves).enumerate() {
		assert_eq!(h.to_bits(), f16::from_f32(v).to_bits(), "sample {k}: {v:e}");
	}
	shape.format = SampleFormat::Index;
	let snap = gpu.capture_frame(&shape).unwrap();
	for (k, (&v, &i)) in values.iter().zip(&snap.field).enumerate() {
		assert_eq!(i, capture::field_index(f16::from_f32(v), 4.0), "sample {k}: {v:e}");
	}
}

#[test]
fn runs_are_deterministic() {
	if !gpu_available() {
		return;
	}
	let shape = shape(2, SampleFormat::Index);
	let trace = |gpu: &mut GpuEngine| {
		run(gpu, 500);
		let (f, i) = gpu.prepare_frame().unwrap();
		let bits: Vec<u32> = f.iter().chain(&i).map(|v| v.to_bits()).collect();
		(bits, gpu.capture_frame(&shape).unwrap().field)
	};
	let (_, mut a) = engines(Stencil::FourthOrder, Edges::Open, MediumLayout::Compact);
	let (_, mut b) = engines(Stencil::FourthOrder, Edges::Open, MediumLayout::Compact);
	let first = trace(&mut a);
	assert!(first == trace(&mut b), "two engines");
	a.reset();
	assert!(first == trace(&mut a), "a reset and replayed engine");
}

#[test]
fn probes_match() {
	if !gpu_available() {
		return;
	}
	let (mut cpu, mut gpu) = engines(Stencil::NinePoint, Edges::Open, MediumLayout::Compact);
	let cell = |x: u32, y: u32| (x, y);
	let probes = [
		ProbeCells { layer: Layer::Field, cells: vec![cell(64, 48)] },
		ProbeCells { layer: Layer::Intensity, cells: (10..90).map(|x| cell(x, 60)).collect() },
		ProbeCells { layer: Layer::Field, cells: (20..40).map(|y| cell(80, y)).collect() },
	];
	cpu.set_probes(&probes);
	gpu.set_probes(&probes);
	let (mut rc, mut rg) = (ProbeReadings::default(), ProbeReadings::default());
	let mut worst = [0f64; 3];
	for _ in 0..600 {
		cpu.advance_one_step();
		gpu.advance_one_step().unwrap();
		cpu.sample_probes(&mut rc);
		gpu.sample_probes(&mut rg).unwrap();
		assert_eq!(rg.iter().map(<[f32]>::len).collect::<Vec<_>>(), [1, 80, 20]);
		for (p, w) in worst.iter_mut().enumerate() {
			let (c, g) = (rc.probe(p), rg.probe(p));
			let diff = c.iter().zip(g).fold(0f64, |m, (&a, &b)| m.max((a as f64 - b as f64).abs()));
			*w = w.max(diff);
		}
	}
	let (cf, ci) = cpu.prepare_frame();
	let (field_peak, intensity_peak) = (peak(&cf), peak(&ci));
	eprintln!(
		"probes: worst field {:.2e}, intensity {:.2e}, field {:.2e} (of peaks {field_peak:.3}, {intensity_peak:.3})",
		worst[0], worst[1], worst[2]
	);
	assert!(worst[0] <= 1e-4 * field_peak && worst[2] <= 1e-4 * field_peak);
	assert!(worst[1] <= 1e-3 * intensity_peak);
}

#[test]
fn queued_probes_read_like_sampled_ones() {
	if !gpu_available() {
		return;
	}
	let (_, mut a) = engines(Stencil::NinePoint, Edges::Open, MediumLayout::Compact);
	let (_, mut b) = engines(Stencil::NinePoint, Edges::Open, MediumLayout::Compact);
	let probes = [
		ProbeCells { layer: Layer::Field, cells: vec![(64, 48)] },
		ProbeCells { layer: Layer::Intensity, cells: (10..90).map(|x| (x, 60)).collect() },
	];
	a.set_probes(&probes);
	b.set_probes(&probes);
	let mut sampled = Vec::new();
	let mut r = ProbeReadings::default();
	let mut queued = Vec::new();
	// Past a full batch, drained mid-batch once.
	for step in 1..=(BATCH_STEPS * 2 + 5) {
		a.advance_one_step().unwrap();
		a.sample_probes(&mut r).unwrap();
		sampled.push(r.clone());
		b.advance_one_step().unwrap();
		b.queue_probes().unwrap();
		if step == BATCH_STEPS + 3 {
			b.drain_probes(&mut |r| queued.push(r.clone())).unwrap();
		}
	}
	b.drain_probes(&mut |r| queued.push(r.clone())).unwrap();
	assert_eq!(queued, sampled);
	b.drain_probes(&mut |_| panic!("drained twice")).unwrap();
}

#[test]
fn replaced_scene_steps_like_a_fresh_engine() {
	if !gpu_available() {
		return;
	}
	let stencil = Stencil::FourthOrder;
	let (_, mut kept) = engines(stencil, Edges::Open, MediumLayout::Compact);
	run(&mut kept, 50);
	let slower = Arc::new(scene(Edges::Open, 200));
	assert!(!kept.try_replace_scene(&Arc::new(SceneData::blank(W + 1, H, &scene_settings(Edges::Open)))));
	assert!(kept.try_replace_scene(&slower));
	assert_eq!(kept.sub_steps(), 1);
	kept.reset();
	let mut fresh = GpuEngine::new(slower, options(stencil)).expect("GPU engine");
	fresh.set_emitters(&emitters());
	run(&mut kept, 300);
	run(&mut fresh, 300);
	let (a, b) = (kept.prepare_frame().unwrap(), fresh.prepare_frame().unwrap());
	assert!(peak(&a.0) > 1e-3);
	assert!(a == b, "the replaced engine is bit-identical to a fresh one");
}

/// Step and capture times at full HD with a 32 px rim.
#[test]
#[ignore = "timing; run with --release -- --ignored --nocapture"]
fn timing_full_hd() {
	if !gpu_available() {
		return;
	}
	let settings = Scene { damping_edge: -32, ..Scene::default() };
	for layout in [MediumLayout::Compact, MediumLayout::Plain] {
		for stencil in STENCILS {
			let mut gpu = GpuEngine::with_layout(SceneData::blank(1920, 1080, &settings), options(stencil), layout)
				.expect("GPU engine");
			gpu.set_emitters(&[emitter(960, 540, 100.0)]);
			run(&mut gpu, 200);
			gpu.synchronize().unwrap();
			let steps = 2000;
			let t = Instant::now();
			run(&mut gpu, steps);
			gpu.synchronize().unwrap();
			let step = t.elapsed().as_secs_f64() * 1e3 / steps as f64;
			let mut capture = |format| {
				let shape = FrameShape { canvas_width: 1920, canvas_height: 1080, ..shape(1, format) };
				drop(gpu.capture_frame(&shape).unwrap());
				let n = 50;
				let t = Instant::now();
				for _ in 0..n {
					drop(gpu.capture_frame(&shape).unwrap());
				}
				t.elapsed().as_secs_f64() * 1e3 / n as f64
			};
			let (half, index) = (capture(SampleFormat::Half), capture(SampleFormat::Index));
			let full = FrameShape { canvas_width: 1920, canvas_height: 1080, ..shape(1, SampleFormat::Half) };
			let t = Instant::now();
			let mut previous = None;
			for _ in 0..100 {
				run(&mut gpu, 2);
				let pending = gpu.begin_capture(&full, Snapshot::new(full)).unwrap();
				if let Some(p) = previous.replace(pending) {
					drop(gpu.finish_capture(p).unwrap());
				}
			}
			drop(gpu.finish_capture(previous.expect("a capture")).unwrap());
			let cycle = t.elapsed().as_secs_f64() * 1e3 / 100.0;
			eprintln!(
				"{layout:?} {stencil:?}, {} sub-steps: {step:.3} ms a step; capture of both planes {half:.2} ms as halves, \
				 {index:.2} ms as indices; 2 steps + overlapped half capture {cycle:.2} ms",
				gpu.sub_steps()
			);
		}
	}
}
