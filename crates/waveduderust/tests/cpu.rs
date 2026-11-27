//! The CPU engine's blocked and fused steps against the same engine stepping the plainest way, side by side
//! (engine/cpu). The reference is the engine on its generic backend set to step unblocked: one step at a time, each
//! sub-step through the passes row by row. On the generic backend the blocked steps must compute its field and
//! intensity stages bit for bit, over the whole padded grid; on every other backend this CPU runs ([`Backend::ALL`]),
//! which may round differently, within a tolerance. Probes and captures are compared too. A CPU that runs no other
//! backend still checks the generic one.

use std::num::NonZeroU32;
use std::sync::Arc;

use waveduderust::f16;
use waveduderust::model::{Edges, Layer, PulseShape, Scene, Stencil};
use waveduderust::unstable::engine::cpu::{Backend, CpuEngine};
use waveduderust::unstable::engine::substeps::courant_limit;
use waveduderust::unstable::engine::{
	Emitter, EngineOptions, FrameShape, ProbeCells, ProbeReadings, SampleFormat, SceneData, WaveEngine,
};

const W: u32 = 128;
const H: u32 = 96;
const RIM: i32 = 16;
/// The largest difference a backend other than the generic one may make, relative to the reference's peak of a plane.
const TOLERANCE: f64 = 1e-4;
const STENCILS: [Stencil; 2] = [Stencil::NinePoint, Stencil::FourthOrder];
/// Reflective, open with Mur's condition, a CPML rim, and an inside damping band (a positive damping edge).
const EDGES: [(Edges, i32); 4] = [(Edges::Reflective, 0), (Edges::Open, 0), (Edges::Open, -RIM), (Edges::Open, 6)];

/// The backends besides the generic one that this CPU runs, the rest named as skipped.
fn others() -> Vec<Backend> {
	let (runs, skipped): (Vec<Backend>, Vec<Backend>) =
		Backend::ALL.iter().copied().filter(|&b| b != Backend::Generic).partition(|b| b.available());
	for backend in skipped {
		eprintln!("skipped: this CPU does not run the {} backend", backend.name());
	}
	runs
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

/// The engine on each backend this CPU runs, and the reference, on one scene.
struct Engines {
	/// On the generic backend: the reference bit for bit.
	generic: CpuEngine,
	/// On each other backend this CPU runs: the reference within [`TOLERANCE`].
	others: Vec<CpuEngine>,
	/// On the generic backend, stepping unblocked ([`CpuEngine::set_unblocked`]).
	reference: CpuEngine,
	what: String,
}

impl Engines {
	fn new(stencil: Stencil, edges: Edges, damping_edge: i32, sub_steps: Option<u32>) -> Self {
		let what = format!("{stencil:?} {edges:?} damping edge {damping_edge}");
		Self::on(|| scene(edges, damping_edge, 100), stencil, sub_steps, what, &emitters())
	}

	/// The engines on the scene `build` makes, with `emitters`.
	fn on(
		build: impl Fn() -> SceneData,
		stencil: Stencil,
		sub_steps: Option<u32>,
		what: String,
		emitters: &[Emitter],
	) -> Self {
		let engine = |backend| match sub_steps {
			Some(n) => CpuEngine::with_backend_and_sub_steps(build(), options(stencil), backend, n),
			None => CpuEngine::with_backend(build(), options(stencil), backend),
		};
		let mut reference = engine(Backend::Generic);
		reference.set_unblocked(true);
		let others = others().into_iter().map(engine).collect();
		let mut engines = Self { generic: engine(Backend::Generic), others, reference, what };
		engines.each(|e| e.set_emitters(emitters));
		engines
	}

	/// The engines under test, the generic backend's first.
	fn tested(&mut self) -> impl Iterator<Item = &mut CpuEngine> {
		std::iter::once(&mut self.generic).chain(&mut self.others)
	}

	/// Calls `f` on each engine.
	fn each(&mut self, mut f: impl FnMut(&mut dyn WaveEngine)) {
		self.tested().for_each(|e| f(e));
		f(&mut self.reference);
	}

	fn run(&mut self, steps: usize) {
		for _ in 0..steps {
			self.tested().for_each(CpuEngine::advance_one_step);
			self.reference.advance_one_step();
		}
	}

	/// Asserts the generic backend's padded field and intensity stages are the reference's bit for bit, and each other
	/// backend's within [`TOLERANCE`] of its peak; `when` names the moment.
	fn check(&mut self, when: &str) {
		let what = format!("{}, {when}", self.what);
		assert_eq!(self.generic.time(), self.reference.time(), "{what}");
		let (cs1, cs2) = self.reference.intensity_stages();
		let planes = |e: &CpuEngine| {
			let (s1, s2) = e.intensity_stages();
			[e.field_padded().to_vec(), s1.to_vec(), s2.to_vec()]
		};
		let reference = [self.reference.field_padded(), cs1, cs2];
		assert!(peak(reference[0]) > 0.0 && peak(reference[1]) > 0.0, "{what}: the scene is excited");
		for (plane, (r, c)) in ["field", "s1", "s2"].iter().zip(planes(&self.generic).iter().zip(reference)) {
			let nonzero = |p: &[f32]| p.iter().map(|v| v.to_bits() != 0).collect::<Vec<_>>();
			assert!(nonzero(r) == nonzero(c), "{what}: the generic backend's {plane} is non-zero elsewhere");
			if let Some(i) = r.iter().zip(c).position(|(a, b)| a.to_bits() != b.to_bits()) {
				panic!("{what}: the generic backend's {plane} differs at padded cell {i}: {} against {}", r[i], c[i]);
			}
		}
		for other in &self.others {
			let backend = other.backend().name();
			assert_eq!(other.time(), self.reference.time(), "{what}, {backend}");
			for (plane, (n, c)) in ["field", "s1", "s2"].iter().zip(planes(other).iter().zip(reference)) {
				let d = relative(n, c);
				assert!(d <= TOLERANCE, "{what}: the {backend} backend's {plane} differs by {d:e} of its peak");
			}
		}
		let ((gf, gi), (cf, ci)) = (self.generic.prepare_frame(), self.reference.prepare_frame());
		assert!(gf == cf && gi == ci, "{what}: the generic backend's frame differs");
		for other in &mut self.others {
			let (backend, (nf, ni)) = (other.backend().name(), other.prepare_frame());
			let close = relative(&nf, &cf) <= TOLERANCE && relative(&ni, &ci) <= TOLERANCE;
			assert!(close, "{what}: the {backend} backend's frame differs");
		}
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
fn blocked_steps_step_as_the_reference() {
	on_seams(|| {
		for stencil in STENCILS {
			for (edges, damping_edge) in EDGES {
				let mut engines = Engines::new(stencil, edges, damping_edge, None);
				assert_eq!(engines.generic.sub_steps(), engines.reference.sub_steps());
				assert!(engines.generic.sub_steps() > 1, "{}: the steps are blocked", engines.what);
				for step in 1..=300 {
					engines.run(1);
					if step % 20 == 1 || step == 300 {
						engines.check(&format!("step {step}"));
					}
				}
			}
		}
	});
}

#[test]
fn sub_steps_reset_and_replay_agree() {
	on_seams(|| {
		for stencil in STENCILS {
			let n = CpuEngine::new(scene(Edges::Open, -RIM, 100), options(stencil)).sub_steps() + 2;
			let mut engines = Engines::new(stencil, Edges::Open, -RIM, Some(n));
			assert_eq!(engines.generic.sub_steps(), n);
			engines.run(150);
			engines.check(&format!("{n} sub-steps"));
			let first: Vec<_> = engines.tested().map(CpuEngine::prepare_frame).collect();
			engines.each(|e| e.reset());
			for e in engines.tested() {
				assert_eq!(e.time(), 0);
				assert!(e.field_padded().iter().all(|&v| v == 0.0), "{stencil:?}: the reset clears the field");
			}
			engines.run(150);
			engines.check(&format!("{n} sub-steps, replayed"));
			let again: Vec<_> = engines.tested().map(CpuEngine::prepare_frame).collect();
			assert!(again == first, "{stencil:?}: the replay repeats the run");
		}
	});
}

#[test]
fn intensity_toggled_and_emitters_replaced_agree() {
	on_seams(|| {
		for stencil in STENCILS {
			let mut engines = Engines::new(stencil, Edges::Open, 0, None);
			engines.run(80);
			engines.each(|e| e.set_track_intensity(false));
			engines.run(20);
			for e in engines.tested() {
				assert!(e.prepare_frame().1.iter().all(|&v| v == 0.0), "{stencil:?}: no intensity untracked");
			}
			engines.each(|e| e.set_track_intensity(true));
			let fewer = [emitter(64, 48, 70.0), Emitter { pulsed: true, ..emitter(20, 70, 110.0) }];
			engines.each(|e| e.set_emitters(&fewer));
			engines.run(100);
			engines.check("tracked again");
		}
	});
}

#[test]
fn probes_and_captures_agree() {
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
			let mut engines = Engines::new(stencil, Edges::Open, -RIM, None);
			engines.each(|e| e.set_probes(&probes));
			for _ in 0..120 {
				engines.run(1);
				engines.each(|e| e.queue_probes().unwrap());
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
			let (g, c) = (readings(&mut engines.generic), readings(&mut engines.reference));
			assert!(g == c, "{stencil:?}: the generic backend's probes differ");
			let (sg, sc) = (engines.generic.capture_frame(&shape), engines.reference.capture_frame(&shape));
			for layer in [Layer::Field, Layer::Intensity] {
				assert!(
					sg.halves(layer) == sc.halves(layer),
					"{stencil:?}: the generic backend's {layer:?} capture differs"
				);
			}
			for other in &mut engines.others {
				let (backend, n) = (other.backend().name(), readings(other));
				for k in 0..probes.len() {
					// Each probe's readings of every step, against the reference's peak over them.
					let trace = |readings: &[ProbeReadings]| {
						readings.iter().flat_map(|r| r.probe(k).to_vec()).collect::<Vec<_>>()
					};
					let d = relative(&trace(&n), &trace(&c));
					assert!(d <= TOLERANCE, "{stencil:?}, {backend}: probe {k} differs by {d:e} of its peak");
				}
				let sn = other.capture_frame(&shape);
				assert_eq!((sn.step, sn.shape), (sc.step, sc.shape));
				for layer in [Layer::Field, Layer::Intensity] {
					let floats = |halves: &[f16]| halves.iter().map(|h| h.to_f32()).collect::<Vec<_>>();
					let (x, y) = (floats(sn.halves(layer)), floats(sc.halves(layer)));
					// A value on either side of a half's rounding boundary differs by a half's ulp, 2^-11 of it.
					let close = !y.is_empty() && relative(&x, &y) <= 1e-3;
					assert!(close, "{stencil:?}, {backend}: the captured {layer:?} differs");
				}
			}
		}
	});
}

#[test]
fn a_set_field_and_a_replaced_medium_agree() {
	on_seams(|| {
		for stencil in STENCILS {
			for (edges, damping_edge) in EDGES {
				let mut engines = Engines::new(stencil, edges, damping_edge, None);
				let gw = engines.reference.scene().grid_width() as usize;
				let cells = gw * engines.reference.scene().grid_height() as usize;
				let now: Vec<f32> = (0..cells).map(|i| (((i * 7919) % 101) as f32 - 50.0) * 1e-3).collect();
				let before: Vec<f32> = now.iter().map(|v| v * 0.9).collect();
				engines.tested().for_each(|e| e.set_field(&now, &before));
				engines.reference.set_field(&now, &before);
				engines.run(40);
				engines.check("from a set field");
				// A slower background: another medium on the same grid.
				let slower = Arc::new(scene(edges, damping_edge, 160));
				engines.each(|e| assert!(e.try_replace_scene(&slower)));
				engines.run(40);
				engines.check("after a replaced medium");
			}
		}
	});
}

/// Odd and larger sub-step counts, which the engine's blocked steps (engine/cpu, Blocked steps) fuse into one
/// sweep over the rows, ending on the other plane than an even count does.
#[test]
fn odd_and_many_sub_steps_agree() {
	on_seams(|| {
		for (stencil, n) in STENCILS.into_iter().flat_map(|stencil| [3, 5].map(|n| (stencil, n))) {
			for (edges, damping_edge) in EDGES {
				let mut engines = Engines::new(stencil, edges, damping_edge, Some(n));
				engines.what = format!("{}, {n} sub-steps", engines.what);
				for step in 1..=60 {
					engines.run(1);
					if step % 20 == 0 {
						engines.check(&format!("step {step}"));
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
					let mut engines = Engines::on(build, stencil, n, what, &sources);
					assert!(engines.reference.sub_steps() > 1, "{}: the steps are blocked", engines.what);
					engines.run(80);
					engines.check("step 80");
				}
			}
		}
	});
}

/// Sources on every row of a column, so some lie within 3 rows of each seam between the blocked step's bands, on a grid
/// whose rim's y strips take most of its rows, so seams cross them too (engine/cpu, Blocked steps: the 4th order's
/// bands keep `w` in rings and leave the seams' rows of it in the plane).
#[test]
fn sources_and_strips_on_seams_agree() {
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
				let mut engines = Engines::on(build, stencil, n, what, &sources);
				for step in 1..=60 {
					engines.run(1);
					if step % 10 == 0 {
						engines.check(&format!("step {step}"));
					}
				}
			}
		}
	});
}

/// Grids about as high as one, two and three of the 4th order's bands at their fewest rows, with and without a rim.
#[test]
fn grids_near_the_fewest_rows_of_a_band_agree() {
	on_seams(|| {
		for (edges, damping_edge) in [(Edges::Open, -RIM), (Edges::Open, 0)] {
			let settings = Scene::default().with_damping_edge(damping_edge).with_edges(edges).with_image_sources(true);
			for h in [20, 21, 22, 31, 32, 42, 43, 47, 48, 63, 64, 66, 94] {
				let build = || SceneData::from_pixels(&pixels(100)[..(W * h * 4) as usize], W, h, &settings);
				let sources =
					[emitter(30, 0, 100.0), emitter(70, h as i32 / 2, 90.0), emitter(100, h as i32 - 1, 80.0)];
				for n in [2, 3] {
					let what = format!("{edges:?} damping edge {damping_edge}, {h} rows, {n} sub-steps");
					let mut engines = Engines::on(build, Stencil::FourthOrder, Some(n), what, &sources);
					engines.run(60);
					engines.check("step 60");
				}
			}
		}
	});
}

/// Batches of steps a caller may ask for, most no multiple of the fused steps.
const BATCHES: [u32; 10] = [1, 2, 3, 5, 7, 4, 8, 11, 6, 13];

/// Field probes and intensity probes: by sources, on a mirror line, in the rim, one cell twice, and far from the
/// sources, where the field arrives late.
fn fused_probes(h: u32) -> [ProbeCells; 2] {
	let y = |y: u32| y.min(h - 1);
	[
		ProbeCells {
			layer: Layer::Field,
			cells: vec![(30, y(40)), (45, y(20)), (1, 1), (127, h - 1), (64, y(48)), (30, y(40))],
		},
		ProbeCells { layer: Layer::Intensity, cells: vec![(30, y(40)), (90, y(85)), (126, 2), (5, y(90))] },
	]
}

/// The engine on each backend stepping in fused sweeps of at most `k` steps (engine/cpu, Fused steps) against the same
/// backend one step at a time: the same field, intensity stages and probe readings after every batch of [`BATCHES`],
/// `rounds` times over, the emitters set from round `silent` on: on the generic backend bit for bit against the
/// reference, stepping unblocked; on another within [`TOLERANCE`] against its own single blocked steps, as a fused
/// sweep's columns, grown by more steps' reach, split the rows into other spans, whose last cells a backend may round
/// otherwise (backend/mod.rs).
fn fused_agree(
	build: impl Fn() -> SceneData,
	stencil: Stencil,
	sub_steps: Option<u32>,
	emitters: &[Emitter],
	what: &str,
	rounds: usize,
	silent: usize,
) {
	let backends = std::iter::once(Backend::Generic).chain(others());
	for (backend, k) in backends.flat_map(|b| [2, 3, 4, 8].map(|k| (b, k))) {
		let what = format!("{what}, {}, fusing {k}", backend.name());
		let engine = || match sub_steps {
			Some(n) => CpuEngine::with_backend_and_sub_steps(build(), options(stencil), backend, n),
			None => CpuEngine::with_backend(build(), options(stencil), backend),
		};
		let (mut fused, mut single) = (engine(), engine());
		fused.set_fused_steps(k);
		if backend == Backend::Generic {
			single.set_unblocked(true);
		} else {
			single.set_fused_steps(1);
		}
		let probes = fused_probes(fused.scene().canvas_height);
		for e in [&mut fused, &mut single] {
			e.set_probes(&probes);
		}
		let mut traces = (Vec::new(), Vec::new());
		assert_eq!(WaveEngine::fused_steps(&fused), k, "{what}");
		for (round, batch) in (0..rounds).flat_map(|r| BATCHES.map(|b| (r, b))) {
			if round == silent && batch == BATCHES[0] {
				fused.set_emitters(emitters);
				single.set_emitters(emitters);
			}
			if round == 1 && batch == BATCHES[0] {
				// The intensity untracked for a round, then tracked from zero again.
				fused.set_track_intensity(false);
				single.set_track_intensity(false);
			} else if round == 2 && batch == BATCHES[0] {
				fused.set_track_intensity(true);
				single.set_track_intensity(true);
			}
			WaveEngine::advance_steps(&mut fused, batch, true).unwrap();
			for _ in 0..batch {
				single.advance_one_step();
				single.queue_probes().unwrap();
			}
			let when = format!("{what}, step {}", single.time());
			assert_eq!(fused.time(), single.time(), "{when}");
			let planes = |e: &CpuEngine| {
				let (s1, s2) = e.intensity_stages();
				[e.field_padded().to_vec(), s1.to_vec(), s2.to_vec()]
			};
			for (plane, (f, s)) in ["field", "s1", "s2"].iter().zip(planes(&fused).iter().zip(planes(&single).iter())) {
				if backend != Backend::Generic {
					let d = relative(f, s);
					assert!(d <= TOLERANCE, "{when}: the fused {plane} differs by {d:e} of its peak");
				} else if let Some(i) = f.iter().zip(s).position(|(a, b)| a.to_bits() != b.to_bits()) {
					panic!("{when}: the fused {plane} differs at padded cell {i}: {} against {}", f[i], s[i]);
				}
			}
			let readings = |e: &mut CpuEngine| {
				let (pending, mut queued) = (e.pending_probes().unwrap(), Vec::new());
				e.drain_probes(pending, &mut |r| queued.push(r.clone())).unwrap();
				queued
			};
			let (f, s) = (readings(&mut fused), readings(&mut single));
			assert_eq!(s.len(), batch as usize, "{when}");
			assert_eq!(f.len(), s.len(), "{when}");
			if backend == Backend::Generic {
				let bits = |r: &[ProbeReadings]| {
					r.iter().flat_map(|r| r.values().iter().map(|v| v.to_bits())).collect::<Vec<_>>()
				};
				assert!(bits(&f) == bits(&s), "{when}: the fused probe readings differ");
			}
			traces.0.extend(f);
			traces.1.extend(s);
		}
		assert!(peak(single.field_padded()) > 0.0, "{what}: the scene is excited");
		// Each probe cell's values over the run, against the single steps' peak over them.
		let (f, s) = traces;
		for cell in 0..s.first().map_or(0, |r| r.values().len()) {
			let trace = |r: &[ProbeReadings]| r.iter().map(|r| r.values()[cell]).collect::<Vec<_>>();
			let d = relative(&trace(&f), &trace(&s));
			assert!(d <= TOLERANCE, "{what}: the fused readings of probe cell {cell} differ by {d:e} of their peak");
		}
	}
}

/// The standard scene with its speeds scaled so one sub-step a step meets the CFL bound.
fn one_sub_step(edges: Edges, damping_edge: i32, stencil: Stencil) -> SceneData {
	let mut scene = scene(edges, damping_edge, 100);
	let scale = 0.8 * courant_limit(stencil) as f32;
	let max = scene.max_c2();
	scene.c2.iter_mut().for_each(|c2| *c2 *= scale * scale / max);
	scene
}

/// Emitters that pulse and detach within the batches, besides the usual ones.
fn detaching() -> Vec<Emitter> {
	let mut emitters = emitters();
	emitters.push(Emitter { detach_after_step: Some(23), ..emitter(64, 48, 110.0) });
	emitters.push(Emitter { pulsed: true, detach_after_step: Some(61), ..emitter(100, 60, 70.0) });
	emitters
}

#[test]
fn fused_steps_agree_with_single_steps() {
	on_seams(|| {
		for stencil in STENCILS {
			for (edges, damping_edge) in EDGES {
				let what = format!("{stencil:?} {edges:?} damping edge {damping_edge}");
				let build = || scene(edges, damping_edge, 100);
				fused_agree(build, stencil, None, &detaching(), &format!("{what}, CFL sub-steps"), 3, 0);
				let build = || one_sub_step(edges, damping_edge, stencil);
				assert_eq!(CpuEngine::new(build(), options(stencil)).sub_steps(), 1);
				fused_agree(build, stencil, None, &detaching(), &format!("{what}, 1 sub-step"), 3, 0);
			}
		}
	});
}

#[test]
fn fused_steps_agree_with_odd_sub_steps_short_grids_and_silence() {
	on_seams(|| {
		let rim = [(Edges::Open, -RIM), (Edges::Open, 0)];
		for (stencil, (edges, damping_edge)) in STENCILS.into_iter().flat_map(|s| rim.map(|r| (s, r))) {
			let what = format!("{stencil:?} {edges:?} damping edge {damping_edge}");
			let build = || scene(edges, damping_edge, 100);
			fused_agree(build, stencil, Some(3), &detaching(), &format!("{what}, 3 sub-steps"), 3, 0);
			// Fewer rows than 8 bands of a fused sweep take, and an uneven split.
			for h in [12, 47, 61] {
				let settings =
					Scene::default().with_damping_edge(damping_edge).with_edges(edges).with_image_sources(true);
				let build = || {
					let mut scene = SceneData::from_pixels(&pixels(100)[..(W * h * 4) as usize], W, h, &settings);
					let max = scene.max_c2();
					scene.c2.iter_mut().for_each(|c2| *c2 *= 0.6 / max);
					scene
				};
				let sources =
					[emitter(30, 0, 100.0), emitter(70, h as i32 / 2, 90.0), emitter(100, h as i32 - 1, 80.0)];
				fused_agree(build, stencil, None, &sources, &format!("{what}, {h} rows"), 3, 0);
			}
			// Nothing to compute for a round: no image sources, and the emitters set after it.
			let settings = Scene::default().with_damping_edge(damping_edge).with_edges(edges);
			let build = || {
				let mut scene = SceneData::from_pixels(&pixels(100), W, H, &settings);
				let max = scene.max_c2();
				scene.c2.iter_mut().for_each(|c2| *c2 *= 0.6 / max);
				scene
			};
			fused_agree(build, stencil, None, &detaching(), &format!("{what}, silent at first"), 4, 1);
		}
	});
}

/// The 4th order on a grid tall enough for several bands, unevenly split, of a fused sweep of 2 to 4 steps (its
/// trapezoids widen by 6 to 8 rows a level, so 8 steps with a rim fit one band), with sources by the seams: with a rim
/// and with Mur's condition, one sub-step and the CFL's.
#[test]
fn fused_steps_of_the_4th_order_agree_across_bands() {
	const TALL: u32 = 470;
	on_seams(|| {
		for (edges, damping_edge) in [(Edges::Open, -RIM), (Edges::Open, 0)] {
			let settings = Scene::default().with_damping_edge(damping_edge).with_edges(edges).with_image_sources(true);
			let tall = || {
				let canvas = pixels(100);
				let px: Vec<u8> =
					canvas.chunks((W * 4) as usize).cycle().take(TALL as usize).flatten().copied().collect();
				SceneData::from_pixels(&px, W, TALL, &settings)
			};
			let sources = [
				emitter(30, 40, 100.0),
				Emitter { pulsed: true, detach_after_step: Some(37), ..emitter(64, 150, 90.0) },
				emitter(100, 320, 80.0),
				emitter(20, 460, 70.0),
			];
			let what = format!("FourthOrder {edges:?} damping edge {damping_edge}, {TALL} rows");
			fused_agree(tall, Stencil::FourthOrder, None, &sources, &format!("{what}, CFL sub-steps"), 3, 0);
			let one = || {
				let mut scene = tall();
				let scale = 0.8 * courant_limit(Stencil::FourthOrder) as f32;
				let max = scene.max_c2();
				scene.c2.iter_mut().for_each(|c2| *c2 *= scale * scale / max);
				scene
			};
			fused_agree(one, Stencil::FourthOrder, None, &sources, &format!("{what}, 1 sub-step"), 3, 0);
		}
	});
}
