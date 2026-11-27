#![cfg(target_arch = "x86_64")]
//! The AVX2 engine against the CPU engine, stepped side by side (engine/avx). On its reference kernels it must compute
//! the CPU engine's field and intensity stages bit for bit, over the whole padded grid; on its default kernels, which
//! may round differently, within a tolerance. Probes and captures are compared too.
//!
//! Every test returns early, with the reason printed, on a CPU without AVX2 and FMA.

use std::num::NonZeroU32;
use std::sync::Arc;

use waveduderust::f16;
use waveduderust::model::{Edges, Layer, PulseShape, Scene, Stencil};
use waveduderust::unstable::engine::avx::{AvxEngine, available};
use waveduderust::unstable::engine::cpu::CpuEngine;
use waveduderust::unstable::engine::{
	Emitter, EngineOptions, FrameShape, ProbeCells, ProbeReadings, SampleFormat, SceneData, WaveEngine,
};

const W: u32 = 128;
const H: u32 = 96;
const RIM: i32 = 16;
/// The largest difference the default kernels may make, relative to the CPU engine's peak of the plane.
const TOLERANCE: f64 = 1e-4;
const STENCILS: [Stencil; 2] = [Stencil::NinePoint, Stencil::FourthOrder];
/// Reflective, open with Mur's condition, a CPML rim, and an inside damping band (a positive damping edge).
const EDGES: [(Edges, i32); 4] = [(Edges::Reflective, 0), (Edges::Open, 0), (Edges::Open, -RIM), (Edges::Open, 6)];

fn avx_available() -> bool {
	let yes = available();
	if !yes {
		eprintln!("skipped: the CPU has no AVX2 and FMA");
	}
	yes
}

/// Runs `f` on a pool of 8 threads, so the blocked steps' band seams cross the rim's strips on any machine.
fn on_seams(f: impl FnOnce() + Send) {
	rayon::ThreadPoolBuilder::new().num_threads(8).build().expect("a pool of 8 threads").install(f);
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

/// A painted-style canvas on a background of red level `red`: a block of index 2.55, a mirror bar, single mirror cells,
/// mirror lines one cell thin that run into the top and the left edge (and through a rim), an absorbing patch and
/// green image sources at two levels.
fn pixels(red: u8) -> Vec<u8> {
	let mut px = Vec::with_capacity((W * H * 4) as usize);
	for y in 0..H {
		for x in 0..W {
			let mut p = [red, 0, 0, 255];
			if (70..100).contains(&x) && (30..70).contains(&y) {
				p[0] = 255;
			}
			let bar = (20..24).contains(&x) && (55..90).contains(&y);
			let dots = (60..64).contains(&y) && x % 9 == 0;
			let lines = (x == 45 && y < 40) || (y == 50 && x < 12);
			if bar || dots || lines {
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

fn scene(edges: Edges, damping_edge: i32, red: u8) -> SceneData {
	let settings = Scene::default().with_damping_edge(damping_edge).with_edges(edges).with_image_sources(true);
	SceneData::from_pixels(&pixels(red), W, H, &settings)
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

/// Continuous, pulsed, one next to a mirror line, two sharing a cell, and one near a corner, so the rim and the edges
/// are reached early.
fn emitters() -> Vec<Emitter> {
	vec![
		emitter(30, 40, 100.0),
		Emitter { pulsed: true, ..emitter(60, 20, 80.0) },
		emitter(46, 20, 120.0),
		emitter(90, 85, 60.0),
		Emitter { phase_radians: 1.7, ..emitter(90, 85, 65.0) },
		emitter(3, 3, 90.0),
	]
}

/// The AVX2 engine on its reference kernels, on its default kernels, and the CPU engine, on one scene.
struct Trio {
	reference: AvxEngine,
	native: AvxEngine,
	cpu: CpuEngine,
	what: String,
}

impl Trio {
	fn new(stencil: Stencil, edges: Edges, damping_edge: i32, sub_steps: Option<u32>) -> Self {
		let what = format!("{stencil:?} {edges:?} damping edge {damping_edge}");
		Self::on(|| scene(edges, damping_edge, 100), stencil, sub_steps, what, &emitters())
	}

	/// The three engines on the scene `build` makes, with `emitters`.
	fn on(
		build: impl Fn() -> SceneData,
		stencil: Stencil,
		sub_steps: Option<u32>,
		what: String,
		emitters: &[Emitter],
	) -> Self {
		let (reference, native, cpu) = match sub_steps {
			Some(n) => (
				AvxEngine::with_sub_steps(build(), options(stencil), n),
				AvxEngine::with_sub_steps(build(), options(stencil), n),
				CpuEngine::with_sub_steps(build(), options(stencil), n),
			),
			None => (AvxEngine::new(build(), options(stencil)), AvxEngine::new(build(), options(stencil)), {
				CpuEngine::new(build(), options(stencil))
			}),
		};
		let mut trio = Self { reference: reference.with_reference_kernels(), native, cpu, what };
		trio.each(|e| e.set_emitters(emitters));
		trio
	}

	/// Calls `f` on each of the three engines.
	fn each(&mut self, mut f: impl FnMut(&mut dyn WaveEngine)) {
		f(&mut self.reference);
		f(&mut self.native);
		f(&mut self.cpu);
	}

	fn run(&mut self, steps: usize) {
		for _ in 0..steps {
			self.reference.advance_one_step();
			self.native.advance_one_step();
			self.cpu.advance_one_step();
		}
	}

	/// Asserts the reference engine's padded field and intensity stages are the CPU engine's bit for bit, and the
	/// default engine's within [`TOLERANCE`] of its peak; `when` names the moment.
	fn check(&mut self, when: &str) {
		let what = format!("{}, {when}", self.what);
		assert_eq!(self.reference.time(), self.cpu.time(), "{what}");
		let (cs1, cs2) = self.cpu.intensity_stages();
		let planes = |e: &AvxEngine| {
			let (s1, s2) = e.intensity_stages();
			[e.field_padded().to_vec(), s1.to_vec(), s2.to_vec()]
		};
		let cpu = [self.cpu.field_padded(), cs1, cs2];
		assert!(peak(cpu[0]) > 0.0 && peak(cpu[1]) > 0.0, "{what}: the scene is excited");
		for (plane, (r, c)) in ["field", "s1", "s2"].iter().zip(planes(&self.reference).iter().zip(cpu)) {
			let nonzero = |p: &[f32]| p.iter().map(|v| v.to_bits() != 0).collect::<Vec<_>>();
			assert!(nonzero(r) == nonzero(c), "{what}: the reference kernels' {plane} is non-zero elsewhere");
			if let Some(i) = r.iter().zip(c).position(|(a, b)| a.to_bits() != b.to_bits()) {
				panic!("{what}: the reference kernels' {plane} differs at padded cell {i}: {} against {}", r[i], c[i]);
			}
		}
		for (plane, (n, c)) in ["field", "s1", "s2"].iter().zip(planes(&self.native).iter().zip(cpu)) {
			let d = relative(n, c);
			assert!(d <= TOLERANCE, "{what}: the default kernels' {plane} differs by {d:e} of its peak");
		}
		let ((rf, ri), (nf, ni), (cf, ci)) =
			(self.reference.prepare_frame(), self.native.prepare_frame(), self.cpu.prepare_frame());
		assert!(rf == cf && ri == ci, "{what}: the reference kernels' frame differs");
		assert!(relative(&nf, &cf) <= TOLERANCE && relative(&ni, &ci) <= TOLERANCE, "{what}: the frame differs");
	}
}

fn peak(values: &[f32]) -> f64 {
	values.iter().fold(0f64, |m, &v| m.max((v as f64).abs()))
}

/// Largest difference relative to the reference's peak.
fn relative(a: &[f32], reference: &[f32]) -> f64 {
	assert_eq!(a.len(), reference.len());
	let diff = a.iter().zip(reference).fold(0f64, |m, (&x, &y)| m.max((x as f64 - y as f64).abs()));
	diff / peak(reference).max(f64::MIN_POSITIVE)
}

#[test]
fn the_avx_engine_steps_as_the_cpu_engine() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		for stencil in STENCILS {
			for (edges, damping_edge) in EDGES {
				let mut trio = Trio::new(stencil, edges, damping_edge, None);
				assert_eq!(trio.reference.sub_steps(), trio.cpu.sub_steps());
				assert!(trio.cpu.sub_steps() > 1, "{}: the steps are blocked", trio.what);
				for step in 1..=300 {
					trio.run(1);
					if step % 20 == 1 || step == 300 {
						trio.check(&format!("step {step}"));
					}
				}
			}
		}
	});
}

#[test]
fn sub_steps_reset_and_replay_agree() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		for stencil in STENCILS {
			let n = CpuEngine::new(scene(Edges::Open, -RIM, 100), options(stencil)).sub_steps() + 2;
			let mut trio = Trio::new(stencil, Edges::Open, -RIM, Some(n));
			assert_eq!(trio.reference.sub_steps(), n);
			trio.run(150);
			trio.check(&format!("{n} sub-steps"));
			let first = trio.native.prepare_frame();
			trio.each(|e| e.reset());
			assert_eq!(trio.native.time(), 0);
			assert!(trio.native.field_padded().iter().all(|&v| v == 0.0), "{stencil:?}: the reset clears the field");
			trio.run(150);
			trio.check(&format!("{n} sub-steps, replayed"));
			assert_eq!(trio.native.prepare_frame(), first, "{stencil:?}: the replay repeats the run");
		}
	});
}

#[test]
fn intensity_toggled_and_emitters_replaced_agree() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		for stencil in STENCILS {
			let mut trio = Trio::new(stencil, Edges::Open, 0, None);
			trio.run(80);
			trio.each(|e| e.set_track_intensity(false));
			trio.run(20);
			assert!(trio.native.prepare_frame().1.iter().all(|&v| v == 0.0), "{stencil:?}: no intensity untracked");
			trio.each(|e| e.set_track_intensity(true));
			let fewer = [emitter(64, 48, 70.0), Emitter { pulsed: true, ..emitter(20, 70, 110.0) }];
			trio.each(|e| e.set_emitters(&fewer));
			trio.run(100);
			trio.check("tracked again");
		}
	});
}

#[test]
fn probes_and_captures_agree() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		let shape = FrameShape {
			canvas_width: W,
			canvas_height: H,
			divisor: 1,
			format: SampleFormat::Half,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: false,
		};
		let probes = [
			ProbeCells { layer: Layer::Field, cells: vec![(40, 40), (47, 20), (5, 5)] },
			ProbeCells { layer: Layer::Intensity, cells: vec![(40, 40), (95, 80)] },
		];
		for stencil in STENCILS {
			let mut trio = Trio::new(stencil, Edges::Open, -RIM, None);
			trio.each(|e| e.set_probes(&probes));
			for _ in 0..120 {
				trio.run(1);
				trio.each(|e| e.queue_probes().unwrap());
			}
			let readings = |e: &mut dyn WaveEngine| {
				let mut now = ProbeReadings::default();
				e.sample_probes(&mut now).unwrap();
				let (pending, mut queued) = (e.pending_probes().unwrap(), Vec::new());
				e.drain_probes(pending, &mut |r| queued.push(r.clone())).unwrap();
				assert_eq!(queued.len(), 120);
				assert_eq!(queued.last(), Some(&now), "the last reading queued is the one sampled");
				queued
			};
			let (r, n, c) = (readings(&mut trio.reference), readings(&mut trio.native), readings(&mut trio.cpu));
			assert!(r == c, "{stencil:?}: the reference kernels' probes differ");
			for k in 0..probes.len() {
				// Each probe's readings of every step, against the CPU engine's peak over them.
				let trace =
					|readings: &[ProbeReadings]| readings.iter().flat_map(|r| r.probe(k).to_vec()).collect::<Vec<_>>();
				let d = relative(&trace(&n), &trace(&c));
				assert!(d <= TOLERANCE, "{stencil:?}: probe {k} differs by {d:e} of its peak");
			}
			let (sr, sn, sc) = (
				trio.reference.capture_frame(&shape),
				trio.native.capture_frame(&shape),
				trio.cpu.capture_frame(&shape),
			);
			assert_eq!((sn.step, sn.shape), (sc.step, sc.shape));
			for layer in [Layer::Field, Layer::Intensity] {
				assert!(
					sr.halves(layer) == sc.halves(layer),
					"{stencil:?}: the reference kernels' {layer:?} capture differs"
				);
				let floats = |halves: &[f16]| halves.iter().map(|h| h.to_f32()).collect::<Vec<_>>();
				let (x, y) = (floats(sn.halves(layer)), floats(sc.halves(layer)));
				// A value on either side of a half's rounding boundary differs by a half's ulp, 2^-11 of it.
				assert!(!y.is_empty() && relative(&x, &y) <= 1e-3, "{stencil:?}: the captured {layer:?} differs");
			}
		}
	});
}

#[test]
fn a_set_field_and_a_replaced_medium_agree() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		for stencil in STENCILS {
			for (edges, damping_edge) in EDGES {
				let mut trio = Trio::new(stencil, edges, damping_edge, None);
				let gw = trio.cpu.scene().grid_width() as usize;
				let cells = gw * trio.cpu.scene().grid_height() as usize;
				let now: Vec<f32> = (0..cells).map(|i| (((i * 7919) % 101) as f32 - 50.0) * 1e-3).collect();
				let before: Vec<f32> = now.iter().map(|v| v * 0.9).collect();
				trio.reference.set_field(&now, &before);
				trio.native.set_field(&now, &before);
				trio.cpu.set_field(&now, &before);
				trio.run(40);
				trio.check("from a set field");
				// A slower background: another medium on the same grid.
				let slower = Arc::new(scene(edges, damping_edge, 160));
				trio.each(|e| assert!(e.try_replace_scene(&slower)));
				trio.run(40);
				trio.check("after a replaced medium");
			}
		}
	});
}

/// Odd and larger sub-step counts, which the AVX2 engine's blocked steps (engine/avx, Blocked steps) fuse into one sweep
/// over the rows, ending on the other plane than an even count does.
#[test]
fn odd_and_many_sub_steps_agree() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		for (stencil, n) in STENCILS.into_iter().flat_map(|stencil| [3, 5].map(|n| (stencil, n))) {
			for (edges, damping_edge) in EDGES {
				let mut trio = Trio::new(stencil, edges, damping_edge, Some(n));
				trio.what = format!("{}, {n} sub-steps", trio.what);
				for step in 1..=60 {
					trio.run(1);
					if step % 20 == 0 {
						trio.check(&format!("step {step}"));
					}
				}
			}
		}
	});
}

/// Grids of a few rows, fewer than the blocked step's bands would need on every thread, with sources on the first and
/// last rows.
#[test]
fn short_grids_agree() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		for h in [3, 7, 12, 25] {
			let sources = [
				emitter(30, 0, 100.0),
				Emitter { pulsed: true, ..emitter(70, h as i32 - 1, 80.0) },
				emitter(3, h as i32 / 2, 90.0),
			];
			for (edges, damping_edge) in EDGES {
				let settings =
					Scene::default().with_damping_edge(damping_edge).with_edges(edges).with_image_sources(true);
				let build = || SceneData::from_pixels(&pixels(100)[..(W * h * 4) as usize], W, h, &settings);
				for (stencil, n) in STENCILS.into_iter().flat_map(|stencil| [None, Some(3)].map(|n| (stencil, n))) {
					let what = format!("{stencil:?} {edges:?} damping edge {damping_edge}, {h} rows, {n:?} sub-steps");
					let mut trio = Trio::on(build, stencil, n, what, &sources);
					assert!(trio.cpu.sub_steps() > 1, "{}: the steps are blocked", trio.what);
					trio.run(80);
					trio.check("step 80");
				}
			}
		}
	});
}

/// Sources on every row of a column, so some lie within 3 rows of each seam between the blocked step's bands, on a grid
/// whose rim's y strips take most of its rows, so seams cross them too (engine/avx, Blocked steps: the 4th order's
/// bands keep `w` in rings and leave the seams' rows of it in the plane).
#[test]
fn sources_and_strips_on_seams_agree() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		let (h, rim) = (48, 40);
		let sources: Vec<Emitter> = (0..h as i32)
			.map(|y| Emitter { pulsed: y % 3 == 0, phase_radians: 0.1 * f64::from(y), ..emitter(64, y, 40.0) })
			.collect();
		for (edges, damping_edge) in [(Edges::Open, -rim), (Edges::Reflective, 0), (Edges::Open, 0)] {
			let settings = Scene::default().with_damping_edge(damping_edge).with_edges(edges).with_image_sources(true);
			let build = || SceneData::from_pixels(&pixels(100)[..(W * h * 4) as usize], W, h, &settings);
			for (stencil, n) in STENCILS.into_iter().flat_map(|stencil| [None, Some(3)].map(|n| (stencil, n))) {
				let what = format!("{stencil:?} {edges:?} damping edge {damping_edge}, {n:?} sub-steps");
				let mut trio = Trio::on(build, stencil, n, what, &sources);
				for step in 1..=60 {
					trio.run(1);
					if step % 10 == 0 {
						trio.check(&format!("step {step}"));
					}
				}
			}
		}
	});
}

/// Grids about as high as one, two and three of the 4th order's bands at their fewest rows, with and without a rim.
#[test]
fn grids_near_the_fewest_rows_of_a_band_agree() {
	if !avx_available() {
		return;
	}
	on_seams(|| {
		for (edges, damping_edge) in [(Edges::Open, -RIM), (Edges::Open, 0)] {
			let settings = Scene::default().with_damping_edge(damping_edge).with_edges(edges).with_image_sources(true);
			for h in [20, 21, 22, 31, 32, 42, 43, 47, 48, 63, 64, 66, 94] {
				let build = || SceneData::from_pixels(&pixels(100)[..(W * h * 4) as usize], W, h, &settings);
				let sources =
					[emitter(30, 0, 100.0), emitter(70, h as i32 / 2, 90.0), emitter(100, h as i32 - 1, 80.0)];
				for n in [2, 3] {
					let what = format!("{edges:?} damping edge {damping_edge}, {h} rows, {n} sub-steps");
					let mut trio = Trio::on(build, Stencil::FourthOrder, Some(n), what, &sources);
					trio.run(60);
					trio.check("step 60");
				}
			}
		}
	});
}
