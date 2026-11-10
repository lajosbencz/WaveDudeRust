//! Analytic tests of the CPU engine against docs/physics.md.
//!
//! Grids are small (64 to 256 cells a side) so most tests run in a few seconds in a debug build; the
//! CPML's reflection and the slab guide, which need room and many steps, take about 17 s each, the
//! source amplitude on its 512 x 512 canvas about 40 s.
//! Units: cells and steps (dx = dt = 1); fields are f32, the measures are taken in f64.

use std::f64::consts::PI;
use std::num::NonZeroU32;

use wavedude_core::engine::cpu::CpuEngine;
use wavedude_core::engine::envelope::level_omega;
use wavedude_core::engine::stencil::{StencilCoefficients, TimeScheme};
use wavedude_core::engine::substeps::{courant_limit, sub_steps};
use wavedude_core::engine::{Emitter, EngineOptions, SceneData, WaveEngine};
use wavedude_core::model::{Edges, PulseShape, Scene, Stencil, wavelength};
use wavedude_core::scene::modes::ModeSolver;

const STENCILS: [Stencil; 2] = [Stencil::NinePoint, Stencil::FourthOrder];

fn options(stencil: Stencil, scale: f64) -> EngineOptions {
	EngineOptions {
		stencil,
		source_frequency_scale: scale,
		image_source_scale: 1.0,
		time_decay: 10.0,
		intensity_scale: 1.0,
		track_intensity: false,
	}
}

fn scene(damping_edge: i32, edges: Edges, index: f64) -> Scene {
	Scene { damping_edge, edges, refractive_index: index, ..Scene::default() }
}

fn emitter(x: i32, y: i32, level: f64) -> Emitter {
	Emitter {
		x: f64::from(x),
		y: f64::from(y),
		level,
		amplitude: 1.0,
		phase_radians: 0.0,
		pulsed: false,
		shape: PulseShape::Gaussian,
		on_width_cycles: 10,
		off_width_cycles: 10,
		off_width_base: 10,
		randomize_off_width: 0,
		detach_after_step: None,
		rng_seed: NonZeroU32::MIN,
	}
}

/// A single Gaussian burst of `cycles` cycles that never repeats.
fn pulse(x: i32, y: i32, level: f64, cycles: u32) -> Emitter {
	Emitter {
		pulsed: true,
		on_width_cycles: cycles,
		off_width_cycles: 100_000,
		off_width_base: 100_000,
		..emitter(x, y, level)
	}
}

/// A Gaussian bump of width `sigma` centred at (cx, cy) on a `w` x `h` grid.
fn bump(w: usize, h: usize, cx: f64, cy: f64, sigma: f64) -> Vec<f32> {
	(0..w * h)
		.map(|i| {
			let (x, y) = ((i % w) as f64 - cx, (i / w) as f64 - cy);
			(-(x * x + y * y) / (2.0 * sigma * sigma)).exp() as f32
		})
		.collect()
}

fn field(engine: &mut CpuEngine) -> Vec<f32> {
	engine.prepare_frame().0
}

fn peak(values: &[f32]) -> f64 {
	values.iter().fold(0f64, |m, &v| m.max((v as f64).abs()))
}

/// The engine's Laplacian of a grid-sized field with zeros outside (the wall), in f64 with the
/// engine's f32 weights (the 9-point's 1/6 and 2/3, the DRP's a1..a3).
fn laplacian(stencil: Stencil, u: &[f64], w: usize, h: usize) -> Vec<f64> {
	let at = |x: isize, y: isize| -> f64 {
		if x < 0 || y < 0 || x >= w as isize || y >= h as isize { 0.0 } else { u[y as usize * w + x as usize] }
	};
	let f = |v: f64| v as f32 as f64;
	let a = StencilCoefficients::of(stencil).axis.map(f);
	let mut out = vec![0.0; w * h];
	for y in 0..h as isize {
		for x in 0..w as isize {
			let ring = |m: isize| at(x - m, y) + at(x + m, y) + at(x, y - m) + at(x, y + m);
			let c4 = 4.0 * at(x, y);
			out[y as usize * w + x as usize] = match stencil {
				Stencil::NinePoint => {
					let diagonals = at(x - 1, y - 1) + at(x + 1, y - 1) + at(x - 1, y + 1) + at(x + 1, y + 1);
					f(1.0 / 6.0) * (diagonals - c4) + f(2.0 / 3.0) * (ring(1) - c4)
				}
				Stencil::FourthOrder => a[1] * (ring(1) - c4) + a[2] * (ring(2) - c4) + a[3] * (ring(3) - c4),
			};
		}
	}
	out
}

/// The sub-step operator `B u` of the stencil's scheme at a uniform `c2` (per sub-step): `A u` or
/// `A u + A A u / 12`, `A = c2 L` (engine/stencil.rs).
fn scheme_operator(stencil: Stencil, c2: f64, u: &[f64], w: usize, h: usize) -> Vec<f64> {
	let a: Vec<f64> = laplacian(stencil, u, w, h).iter().map(|v| c2 * v).collect();
	match TimeScheme::of(stencil) {
		TimeScheme::Leapfrog => a,
		TimeScheme::ModifiedEquation => {
			a.iter().zip(laplacian(stencil, &a, w, h)).map(|(&x, l)| x + c2 * l / 12.0).collect()
		}
	}
}

fn widen(v: &[f32]) -> Vec<f64> {
	v.iter().map(|&x| x as f64).collect()
}

/// (1) The discrete energy of a free pulse in a reflective box without damping, over 10,000 steps.
///
/// Both schemes are `u+ - 2u + u- = B u` with `B = c2 L` or `c2 L + (c2 L)^2 / 12`; at a uniform
/// c2, `B / c2` is symmetric because the stencil runs on every cell with zeros beyond the wall, so
///
/// ```text
/// E(n + 1/2) = ( |u(n+1) - u(n)|^2 - <u(n+1), B u(n)> ) / (2 c2)
/// ```
///
/// is conserved exactly (`E(n+1/2) - E(n-1/2) = <B u, u+ - u-> - <u+, B u> + <u, B u-> = 0`) and is
/// positive while the scheme is stable. Index 2 (c2 = 1/4) keeps one sub-step per step on both
/// stencils, so consecutive steps are consecutive levels. In f32 the measured drift is round-off.
#[test]
fn energy_of_a_free_pulse_is_conserved() {
	let (w, h) = (72usize, 64usize);
	let steps = 10_000;
	let mut drifts = Vec::new();
	for stencil in STENCILS {
		let data = SceneData::blank(w as u32, h as u32, &scene(0, Edges::Reflective, 2.0));
		let mut e = CpuEngine::new(data, options(stencil, 2.0));
		assert_eq!(e.sub_steps(), 1);
		let start = bump(w, h, 30.0, 34.0, 3.0);
		e.set_field(&start, &start);
		let c2 = 0.25;
		let energy = |u0: &[f64], u1: &[f64]| {
			let b = scheme_operator(stencil, c2, u0, w, h);
			let kinetic: f64 = u0.iter().zip(u1).map(|(a, b)| (b - a).powi(2)).sum();
			let potential: f64 = u1.iter().zip(&b).map(|(x, y)| x * y).sum();
			(kinetic - potential) / (2.0 * c2)
		};
		let mut first = f64::NAN;
		let mut drift = 0f64;
		let mut before = widen(&field(&mut e));
		for step in 1..=steps {
			e.advance_one_step();
			if step % 20 == 0 || step == 1 {
				let now = widen(&field(&mut e));
				let en = energy(&before, &now);
				if step == 1 {
					first = en;
				}
				drift = drift.max((en - first).abs() / first);
			}
			if step % 20 == 19 || step == steps {
				before = widen(&field(&mut e));
			}
		}
		println!("{stencil:?}: energy drift {drift:.3e} relative over {steps} steps");
		drifts.push((stencil, first, drift));
	}
	for (stencil, first, drift) in drifts {
		assert!(first > 0.0 && drift < 1e-6, "{stencil:?}: drift {drift}");
	}
}

/// Step time on a 1920 x 1080 canvas with a 32 px rim, open edges, two sources, intensity tracked.
/// Run with `cargo test --release -p wavedude-core --no-default-features --test physics -- --ignored --nocapture`.
#[test]
#[ignore]
fn step_time_full_hd() {
	for stencil in STENCILS {
		let data = SceneData::blank(1920, 1080, &scene(-32, Edges::Open, 1.0));
		let mut e = CpuEngine::new(data, EngineOptions { track_intensity: true, ..options(stencil, 2.0) });
		e.set_emitters(&[emitter(600, 540, 100.0), emitter(1300, 400, 80.0)]);
		for _ in 0..20 {
			e.advance_one_step();
		}
		let steps = 200;
		let start = std::time::Instant::now();
		for _ in 0..steps {
			e.advance_one_step();
		}
		let per = start.elapsed().as_secs_f64() * 1000.0 / steps as f64;
		println!(
			"{stencil:?}: {per:.2} ms a step ({} sub-steps), {} threads",
			e.sub_steps(),
			rayon::current_num_threads()
		);
	}
}

/// Canvas pixels for a scene image: a background red level with optional overrides.
fn pixels(w: usize, h: usize, paint: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
	(0..w * h)
		.flat_map(|i| {
			let [r, g, b] = paint(i % w, i / w);
			[r, g, b, 255]
		})
		.collect()
}

/// A scene with every feature the examples use: glass up to index 2.55, a mirror block, an absorber,
/// vacuum (index 1) elsewhere.
fn mixed_pixels(w: usize, h: usize) -> Vec<u8> {
	pixels(w, h, |x, y| match (x, y) {
		(10..=20, 10..=30) => [255, 0, 0],
		(30..=34, 5..=40) => [0, 0, 0],
		(40..=50, 20..=30) => [150, 0, 200],
		_ => [100, 0, 0],
	})
}

/// (2) Stability: the derived sub-steps keep every configuration bounded for 600 steps; one
/// sub-step fewer at c = 1 diverges on both stencils (so the check can fail); and at one sub-step a
/// Courant number 1.5 percent under the derived limit stays bounded while 1.5 percent over diverges,
/// which pins the limit itself (0.8660 for the 9-point's leapfrog, 0.9747 for the DRP's modified
/// equation, engine/substeps.rs).
#[test]
fn sub_steps_keep_every_configuration_stable() {
	let (w, h) = (64u32, 48u32);
	let mixed = mixed_pixels(w as usize, h as usize);
	for stencil in STENCILS {
		for (edges, edge) in [(Edges::Open, -32), (Edges::Open, 0), (Edges::Reflective, 0), (Edges::Reflective, 6)] {
			for image in [false, true] {
				let s = scene(edge, edges, 1.0);
				let data = if image { SceneData::from_pixels(&mixed, w, h, &s) } else { SceneData::blank(w, h, &s) };
				let mut e = CpuEngine::new(data, options(stencil, 2.0));
				e.set_emitters(&[emitter(32, 24, 255.0), emitter(5, 5, 1.0), emitter(60, 44, 100.0)]);
				for _ in 0..600 {
					e.advance_one_step();
				}
				let f = field(&mut e);
				let p = peak(&f);
				assert!(
					f.iter().all(|v| v.is_finite()) && p < 20.0,
					"{stencil:?} {edges:?} {edge} image {image}: peak {p}"
				);
			}
		}
	}
	for stencil in STENCILS {
		let data = SceneData::blank(w, h, &scene(0, Edges::Reflective, 1.0));
		let n = sub_steps(stencil, 1.0);
		let start = bump(w as usize, h as usize, 32.0, 24.0, 2.0);
		let run = |n: u32| {
			let mut e = CpuEngine::with_sub_steps(data.clone(), options(stencil, 2.0), n);
			e.set_field(&start, &start);
			for _ in 0..300 {
				e.advance_one_step();
			}
			let f = field(&mut e);
			if f.iter().all(|v| v.is_finite()) { peak(&f) } else { f64::INFINITY }
		};
		let (stable, under) = (run(n), run(n - 1));
		println!("{stencil:?}: {n} sub-steps peak {stable:.3e}, {} sub-steps peak {under:.3e}", n - 1);
		assert!(stable <= 1.0, "{stencil:?} at {n} sub-steps: {stable}");
		assert!(under > 1e6, "{stencil:?} at {} sub-steps stays bounded: {under}", n - 1);
		let limit = courant_limit(stencil);
		let at = |courant: f64| {
			let index = 1.0 / courant;
			let data = SceneData::blank(w, h, &scene(0, Edges::Reflective, index));
			let mut e = CpuEngine::with_sub_steps(data, options(stencil, 2.0), 1);
			e.set_field(&start, &start);
			for _ in 0..600 {
				e.advance_one_step();
			}
			let f = field(&mut e);
			if f.iter().all(|v| v.is_finite()) { peak(&f) } else { f64::INFINITY }
		};
		let (below, above) = (at(limit * 0.985), at(limit * 1.015));
		println!("{stencil:?}: Courant limit {limit:.4}: peak {below:.3e} at 0.985 of it, {above:.3e} at 1.015");
		assert!(below <= 1.0 && above > 1e6, "{stencil:?}: {below} under the limit, {above} over it");
	}
}

/// The engine's symbol (engine/stencil.rs) for wave numbers (kx, ky).
fn symbol(stencil: Stencil, kx: f64, ky: f64) -> f64 {
	StencilCoefficients::of(stencil).symbol(kx, ky)
}

/// Angular frequency per step of a mode of `symbol` at speed `c` with `n` sub-steps of the
/// stencil's scheme: `n theta(s)`, `s = (c/n)^2 symbol`.
fn discrete_omega(stencil: Stencil, symbol: f64, c: f64, n: u32) -> f64 {
	let s = (c / n as f64).powi(2) * symbol;
	n as f64 * TimeScheme::of(stencil).phase(s)
}

/// The mode of an axis operator with the wall (weights `a`, zeros beyond `n` cells) whose
/// eigenvalue lies nearest `-p(k)`, by inverse iteration on the dense matrix: (eigenvalue, mode).
/// For the 3-point weights it is the sine `sin(k (i + 1))`; for wider ones the wall bends it.
fn axis_mode(a: [f64; 4], n: usize, k: f64, p: f64) -> (f64, Vec<f64>) {
	let at = |i: usize, j: usize| if i.abs_diff(j) < 4 { a[i.abs_diff(j)] } else { 0.0 };
	let mut m: Vec<Vec<f64>> =
		(0..n).map(|i| (0..n).map(|j| at(i, j) + if i == j { p } else { 0.0 }).collect()).collect();
	let mut order: Vec<usize> = (0..n).collect();
	for c in 0..n {
		let r = (c..n).max_by(|&x, &y| m[x][c].abs().total_cmp(&m[y][c].abs())).unwrap();
		m.swap(c, r);
		order.swap(c, r);
		let (pivot, rest) = m.split_at_mut(c + 1);
		let pivot = &pivot[c];
		for row in rest {
			let f = row[c] / pivot[c];
			row[c] = f;
			row[c + 1..].iter_mut().zip(&pivot[c + 1..]).for_each(|(v, p)| *v -= f * p);
		}
	}
	let solve = |b: &[f64]| {
		let mut x: Vec<f64> = order.iter().map(|&i| b[i]).collect();
		for i in 0..n {
			x[i] -= (0..i).map(|j| m[i][j] * x[j]).sum::<f64>();
		}
		for i in (0..n).rev() {
			x[i] = (x[i] - (i + 1..n).map(|j| m[i][j] * x[j]).sum::<f64>()) / m[i][i];
		}
		x
	};
	let mut v: Vec<f64> = (0..n).map(|i| (k * (i + 1) as f64).sin()).collect();
	for _ in 0..20 {
		v = solve(&v);
		let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
		v.iter_mut().for_each(|x| *x /= norm);
	}
	let av: f64 = (0..n).map(|i| v[i] * (0..n).map(|j| at(i, j) * v[j]).sum::<f64>()).sum();
	(av, v)
}

/// (3) Dispersion, temporal and total: a standing wave of 10 and 20 cells per wavelength along x
/// (the lowest mode along y) in a reflective box. The wall's zeros are not a sine's odd extension
/// for taps reaching 2 or 3 cells, so the initial field is the exact mode of the operator with its
/// wall: the product of the axis operators' modes (`axis_mode`, f64 with the engine's f32 weights),
/// whose symbol is `-(ex + ey) - cross ex ey` from their eigenvalues. Its amplitude a(m) is taken by
/// projection each step; any sampled sinusoid obeys `a(m+1) + a(m-1) = 2 cos(w) a(m)`, so `cos w`
/// is the least-squares ratio over 400 steps.
///
/// Measured against that symbol: the full discrete relation (space and time), which must hold to
/// f32 round-off, and the semi-discrete one (`w = c sqrt(symbol)`, exact in time), whose difference
/// is the temporal dispersion: under 1e-4 at 10 cells per wavelength for the modified equation
/// (leapfrog's is about `s / 24`, 4e-3 there at 2 sub-steps). The free plane wave's phase velocity
/// from the analytic symbol, space and time together, is within 3e-4 of 1 for the DRP stencil.
#[test]
fn plane_wave_phase_velocity_matches_the_stencil() {
	let (w, h) = (199usize, 31usize);
	let data = SceneData::blank(w as u32, h as u32, &scene(0, Edges::Reflective, 1.0));
	for stencil in STENCILS {
		let coefficients = StencilCoefficients::of(stencil);
		let weights = coefficients.axis.map(|v| v as f32 as f64);
		let cross = coefficients.cross as f32 as f64;
		for cells_per_wave in [10usize, 20] {
			let m = (w + 1) / cells_per_wave * 2;
			let kx = m as f64 * PI / (w + 1) as f64;
			let ky = PI / (h + 1) as f64;
			let (ex, vx) = axis_mode(weights, w, kx, coefficients.axis_symbol(kx));
			let (ey, vy) = axis_mode(weights, h, ky, coefficients.axis_symbol(ky));
			let exact = -(ex + ey) - cross * ex * ey;
			let mode: Vec<f64> = (0..w * h).map(|i| vx[i % w] * vy[i / w]).collect();
			let start: Vec<f32> = mode.iter().map(|&v| v as f32).collect();
			let mut e = CpuEngine::new(data.clone(), options(stencil, 2.0));
			e.set_field(&start, &start);
			let mut a = Vec::with_capacity(401);
			for _ in 0..401 {
				let f = field(&mut e);
				a.push(f.iter().zip(&mode).map(|(&u, &p)| u as f64 * p).sum::<f64>());
				e.advance_one_step();
			}
			let (num, den) = (1..a.len() - 1)
				.fold((0.0, 0.0), |(n, d), i| (n + a[i] * (a[i + 1] + a[i - 1]), d + 2.0 * a[i] * a[i]));
			let measured = (num / den).acos();
			let n = e.sub_steps();
			let expected = discrete_omega(stencil, exact, 1.0, n);
			let semi = exact.sqrt();
			let k = kx.hypot(ky);
			let plane = discrete_omega(stencil, symbol(stencil, kx, ky), 1.0, n) / k;
			let off = (measured - expected).abs() / expected;
			let temporal = (expected - semi).abs() / semi;
			println!(
				"{stencil:?} {cells_per_wave} cells/wave: phase velocity {:.6} (relation {:.6}, off {off:.2e}; \
				 temporal dispersion {temporal:.2e}); free plane wave {plane:.6}",
				measured / k,
				expected / k
			);
			assert!(off < 1e-5, "{stencil:?} {cells_per_wave}: relation off by {off}");
			if stencil == Stencil::FourthOrder {
				assert!(temporal < 1e-4, "{cells_per_wave}: temporal dispersion {temporal}");
				assert!((plane - 1.0).abs() < 3e-4, "{cells_per_wave}: plane-wave phase velocity {plane}");
			}
		}
	}
}

/// (3b) The DRP stencil's phase error, from its relation at 2 sub-steps (index 1): along an axis
/// and on the diagonal, the stencil alone (exact in time) and with the modified equation. Under
/// 0.3 percent at 6 cells per wavelength; the Taylor 4th order, for comparison, is -0.6 percent.
#[test]
fn drp_phase_error_table() {
	let c = StencilCoefficients::DRP;
	let n = sub_steps(Stencil::FourthOrder, 1.0);
	println!("| cells | axis, space | axis, space and time | diagonal, space and time | Taylor 4th, axis, space |");
	println!("|---|---|---|---|---|");
	for cells in [4.0, 5.0, 6.0, 8.0, 10.0, 16.0, 20.0] {
		let k = 2.0 * PI / cells;
		let space = c.axis_symbol(k).sqrt() / k - 1.0;
		let full = |kx: f64, ky: f64| discrete_omega(Stencil::FourthOrder, c.symbol(kx, ky), 1.0, n) / k - 1.0;
		let (axis, diagonal) = (full(k, 0.0), full(k / 2f64.sqrt(), k / 2f64.sqrt()));
		let taylor = StencilCoefficients::TAYLOR_FOURTH.axis_symbol(k).sqrt() / k - 1.0;
		println!(
			"| {cells} | {:+.4} % | {:+.4} % | {:+.4} % | {:+.4} % |",
			space * 100.0,
			axis * 100.0,
			diagonal * 100.0,
			taylor * 100.0
		);
		if cells == 6.0 {
			assert!(space.abs() < 3e-3 && axis.abs() < 3e-3 && diagonal.abs() < 3e-3, "{space} {axis} {diagonal}");
		}
	}
}

/// Wave number along an axis whose discrete frequency is `omega` (bisection on the monotonic branch).
fn axis_wavenumber(stencil: Stencil, omega: f64, c: f64, n: u32) -> f64 {
	let (mut lo, mut hi) = (1e-9, 2.0);
	for _ in 0..100 {
		let mid = 0.5 * (lo + hi);
		if discrete_omega(stencil, symbol(stencil, mid, 0.0), c, n) < omega { lo = mid } else { hi = mid }
	}
	0.5 * (lo + hi)
}

/// (4) A point source at level 100 (scale 2: 16 px at c = 1) radiates a circular wave; the
/// wavelength from the zero crossings 20 to 80 cells out along a ray matches the stencil's discrete
/// relation within 1 percent (measured 0.5 to 0.6 percent short on both stencils, the source's
/// start-up transient and the near field) and the nominal `wavelength(level, 1, scale)` within that
/// plus the relation's own departure from it.
#[test]
fn point_source_radiates_its_wavelength() {
	let n = 256usize;
	let (level, scale) = (100.0, 2.0);
	let nominal = wavelength(level, 1.0, scale);
	for stencil in STENCILS {
		let data = SceneData::blank(n as u32, n as u32, &scene(0, Edges::Open, 1.0));
		let mut e = CpuEngine::new(data, options(stencil, scale));
		let c = n as i32 / 2;
		e.set_emitters(&[emitter(c, c, level)]);
		for _ in 0..100 {
			e.advance_one_step();
		}
		let f = field(&mut e);
		let row = &f[c as usize * n..(c as usize + 1) * n];
		let crossings: Vec<f64> = (c as usize + 20..c as usize + 80)
			.filter(|&x| (row[x] > 0.0) != (row[x + 1] > 0.0))
			.map(|x| x as f64 + row[x] as f64 / (row[x] as f64 - row[x + 1] as f64))
			.collect();
		assert!(crossings.len() >= 6, "{stencil:?}: {} crossings", crossings.len());
		let measured = 2.0 * (crossings[crossings.len() - 1] - crossings[0]) / (crossings.len() - 1) as f64;
		let k = axis_wavenumber(stencil, level_omega(level, scale), 1.0, e.sub_steps());
		let relation = 2.0 * PI / k;
		let off = (measured - relation).abs() / relation;
		println!(
			"{stencil:?}: wavelength {measured:.3} px, relation {relation:.3}, nominal {nominal:.3}, off {off:.2e}"
		);
		assert!(off < 0.01, "{stencil:?}: {measured} against {relation}");
		assert!((measured - nominal).abs() / nominal < (relation - nominal).abs() / nominal + 0.01);
	}
}

/// Canvas region `(ox, oy, w, h)` of a canvas field `f` that is `fw` wide.
fn region(f: &[f32], fw: usize, (ox, oy, w, h): (usize, usize, usize, usize)) -> Vec<f32> {
	(0..h).flat_map(|y| f[(y + oy) * fw + ox..(y + oy) * fw + ox + w].iter().copied()).collect()
}

/// (5a) Reflection of a long-wave pulse (level 120 at scale 1, 27 cells a wave, a 3-cycle Gaussian
/// burst from the centre of a 64 x 48 canvas), the original's measure for it: the largest |u| in the
/// canvas from step 300 to 700, once the pulse has left, against the largest up to step 150.
/// Measured: open edges alone 0.014 (9-point) and 0.013 percent (4th order), a 32 px CPML rim
/// 0.0095 and 0.0090 percent (the old quadratic damping rim: 0.24 and 0.25), the reflective box
/// keeps it all.
#[test]
fn long_wave_pulse_leaves_through_open_edges() {
	for stencil in STENCILS {
		for (edges, edge, name) in [
			(Edges::Open, 0, "open edges alone"),
			(Edges::Open, -32, "open, 32 px rim"),
			(Edges::Reflective, 0, "reflective box"),
		] {
			let mut e = CpuEngine::new(SceneData::blank(64, 48, &scene(edge, edges, 1.0)), options(stencil, 1.0));
			e.set_emitters(&[pulse(32, 24, 120.0, 3)]);
			let (mut outgoing, mut back) = (0f64, 0f64);
			for step in 1..=700 {
				e.advance_one_step();
				if step % 5 == 0 {
					let p = peak(&field(&mut e));
					if step <= 150 {
						outgoing = outgoing.max(p);
					} else if step >= 300 {
						back = back.max(p);
					}
				}
			}
			let share = back / outgoing;
			println!("{stencil:?}, {name}: {:.4} % comes back", share * 100.0);
			match (edges, edge) {
				(Edges::Open, 0) => assert!(share < 0.002, "{stencil:?} {name}: {share}"),
				(Edges::Open, _) => assert!(share < 0.005, "{stencil:?} {name}: {share}"),
				_ => assert!(share > 0.5, "{stencil:?} {name}: {share}"),
			}
		}
	}
}

/// A Gaussian wave packet of carrier wavelength `lambda` moving at `angle` (radians from +x, y
/// down) at speed 1, centred at (cx, cy) at time `t` (steps), with envelope widths `along` and
/// `across` its direction, on a `w` x `h` grid.
#[allow(clippy::too_many_arguments)]
fn packet(w: usize, h: usize, cx: f64, cy: f64, angle: f64, lambda: f64, along: f64, across: f64, t: f64) -> Vec<f32> {
	let (sin, cos) = angle.sin_cos();
	let k = 2.0 * PI / lambda;
	(0..w * h)
		.map(|i| {
			let (dx, dy) = ((i % w) as f64 - cx, (i / w) as f64 - cy);
			let (p, q) = (dx * cos + dy * sin - t, -dx * sin + dy * cos);
			((-p * p / (2.0 * along * along) - q * q / (2.0 * across * across)).exp() * (k * p).cos()) as f32
		})
		.collect()
}

/// A packet's spectrum: carrier wavelength and envelope widths, cells.
#[derive(Clone, Copy)]
struct Spectrum {
	name: &'static str,
	lambda: f64,
	along: f64,
	across: f64,
}

const LONG: Spectrum = Spectrum { name: "long (27 cells a wave)", lambda: 27.0, along: 14.0, across: 16.0 };
const SHORT: Spectrum = Spectrum { name: "short (8 cells a wave)", lambda: 8.0, along: 5.0, across: 12.0 };

/// The largest field a boundary sends back into a 240 x 240 canvas, relative to the packet's peak,
/// in dB: the packet starts 90 cells from the left edge moving at it, normally or at 45 degrees
/// (from (100, 165), hitting the edge 65 cells below the top), and the canvas is compared every
/// 4 steps with a reference run whose walls lie far enough out that nothing returns from them within
/// the window, which ends after the packet's tail has crossed the deepest rim twice. `edges` and
/// `rim` give the boundary under test (a rim is a CPML backed by the wall).
fn reflection_db(
	spectrum: Spectrum,
	oblique: bool,
	edges: Edges,
	rim: u32,
	reference: &mut Option<Vec<Vec<f32>>>,
) -> f64 {
	let n = 240usize;
	let (cx, cy, angle, distance) =
		if oblique { (100.0, 165.0, 1.25 * PI, 100.0 * 2f64.sqrt()) } else { (90.0, 120.0, PI, 90.0) };
	let steps = (distance + 3.0 * spectrum.along + 2.0 * 32.0 * 1.5 + 30.0) as usize;
	let opts = options(Stencil::FourthOrder, 2.0);
	let start = |engine: &mut CpuEngine, offset: usize, grid: usize| {
		let dt = 1.0 / engine.sub_steps() as f64;
		let at = |t| {
			packet(
				grid,
				grid,
				cx + offset as f64,
				cy + offset as f64,
				angle,
				spectrum.lambda,
				spectrum.along,
				spectrum.across,
				t,
			)
		};
		engine.set_field(&at(0.0), &at(-dt));
	};
	let frames = reference.get_or_insert_with(|| {
		let margin = steps / 2 + 10;
		let grid = n + 2 * margin;
		let mut e =
			CpuEngine::new(SceneData::blank(grid as u32, grid as u32, &scene(0, Edges::Reflective, 1.0)), opts.clone());
		start(&mut e, margin, grid);
		(1..=steps)
			.filter_map(|step| {
				e.advance_one_step();
				(step % 4 == 0).then(|| region(&field(&mut e), grid, (margin, margin, n, n)))
			})
			.collect()
	});
	let mut e = CpuEngine::new(SceneData::blank(n as u32, n as u32, &scene(-(rim as i32), edges, 1.0)), opts);
	start(&mut e, rim as usize, n + 2 * rim as usize);
	let mut back = 0f64;
	for step in 1..=steps {
		e.advance_one_step();
		if step % 4 == 0 {
			let f = field(&mut e);
			back = f.iter().zip(&frames[step / 4 - 1]).fold(back, |m, (&a, &b)| m.max((a as f64 - b as f64).abs()));
		}
	}
	20.0 * back.log10()
}

/// (5b) The CPML's reflection (decisions/0010, item 1): a long and a short packet against rims of 16,
/// 20 and 32 cells at normal and 45 degree incidence, against the wide reference (see
/// [`reflection_db`]). Required: under -60 dB at 20 cells for the long packet, under -40 dB at both
/// angles for the short one; Mur's ring alone, for comparison, reflects much of the short wave.
#[test]
fn cpml_reflection() {
	for spectrum in [LONG, SHORT] {
		for oblique in [false, true] {
			let mut reference = None;
			let angle = if oblique { "45 degrees" } else { "normal" };
			for rim in [16, 20, 32] {
				let db = reflection_db(spectrum, oblique, Edges::Reflective, rim, &mut reference);
				println!("{}, {angle}, CPML of {rim} cells: {db:.1} dB", spectrum.name);
				if rim == 20 && spectrum.lambda > 20.0 && !oblique {
					assert!(db < -60.0, "long packet, 20 cells: {db} dB");
				}
				if rim >= 20 && spectrum.lambda < 20.0 {
					assert!(db < -40.0, "short packet, {angle}, {rim} cells: {db} dB");
				}
			}
			let mur = reflection_db(spectrum, oblique, Edges::Open, 0, &mut reference);
			println!("{}, {angle}, Mur's ring alone: {mur:.1} dB", spectrum.name);
		}
	}
}

/// (6) The same inputs twice give bit-identical fields and intensities (rayon's row split included).
#[test]
fn runs_are_deterministic() {
	let (w, h) = (96u32, 64u32);
	let px = pixels(w as usize, h as usize, |x, y| match (x, y) {
		(5, 5) => [100, 80, 0],
		(20..=30, 10..=50) => [180, 0, 40],
		_ => [100, 0, 0],
	});
	let data = SceneData::from_pixels(&px, w, h, &Scene { image_sources: true, ..scene(-16, Edges::Open, 1.0) });
	let mut random = pulse(60, 30, 90.0, 2);
	random.randomize_off_width = 3;
	random.off_width_cycles = 4;
	random.off_width_base = 4;
	random.rng_seed = NonZeroU32::new(99).unwrap();
	let run = || {
		let mut e =
			CpuEngine::new(data.clone(), EngineOptions { track_intensity: true, ..options(Stencil::FourthOrder, 2.0) });
		e.set_emitters(&[random.clone(), emitter(40, 40, 140.0)]);
		for _ in 0..200 {
			e.advance_one_step();
		}
		let (f, i) = e.prepare_frame();
		(f.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), i.iter().map(|v| v.to_bits()).collect::<Vec<_>>())
	};
	let (a, b) = (run(), run());
	assert!(a.0.iter().any(|&v| v != 0) && a.1.iter().any(|&v| v != 0));
	assert_eq!(a, b);
}

/// (7) An engine given another scene of its grid by `try_replace_scene`, then reset, steps bit for
/// bit like a fresh engine on that scene, also when the scene changes the sub-step count.
#[test]
fn replaced_scene_steps_like_a_fresh_engine() {
	let (w, h) = (80u32, 60u32);
	let with_source = |gx: usize, gy: usize, level: u8, background: u8| {
		pixels(w as usize, h as usize, move |x, y| match (x, y) {
			_ if (x, y) == (gx, gy) => [background, level, 0],
			(30..=40, 20..=40) => [220, 0, 0],
			_ => [background, 0, 0],
		})
	};
	let s = Scene { image_sources: true, ..scene(-8, Edges::Open, 1.0) };
	let first = SceneData::from_pixels(&with_source(5, 5, 80, 100), w, h, &s);
	let second = SceneData::from_pixels(&with_source(60, 40, 120, 160), w, h, &s);
	for stencil in STENCILS {
		let opts = EngineOptions { track_intensity: true, ..options(stencil, 2.0) };
		let emitters = [emitter(20, 30, 70.0)];
		let mut kept = CpuEngine::new(first.clone(), opts.clone());
		kept.set_emitters(&emitters);
		for _ in 0..40 {
			kept.advance_one_step();
		}
		let before = kept.sub_steps();
		assert!(kept.try_replace_scene(&std::sync::Arc::new(second.clone())));
		kept.reset();
		let mut fresh = CpuEngine::new(second.clone(), opts);
		fresh.set_emitters(&emitters);
		assert_eq!(kept.sub_steps(), fresh.sub_steps());
		println!("{stencil:?}: sub-steps {before} -> {}", fresh.sub_steps());
		for _ in 0..120 {
			kept.advance_one_step();
			fresh.advance_one_step();
		}
		assert_eq!(kept.prepare_frame(), fresh.prepare_frame());
	}
}

/// (8) A square cavity walled by mirror pixels (red 0, two cells thick) holds a source's wave for
/// 300 steps: the largest field outside stays under 1e-3 of the largest inside (measured 0 on both
/// stencils: a mirror cell's c2 is 0, and the taps across a mirror are cut, engine/pml.rs `cuts`).
#[test]
fn mirror_cavity_keeps_the_wave_inside() {
	let n = 96usize;
	let wall = |v: usize| (28..30).contains(&v) || (66..68).contains(&v);
	let inside = |v: usize| (30..66).contains(&v);
	let px = pixels(n, n, |x, y| {
		let on_ring = (wall(x) && (28..68).contains(&y)) || (wall(y) && (28..68).contains(&x));
		if on_ring { [0, 0, 0] } else { [100, 0, 0] }
	});
	for stencil in STENCILS {
		let data = SceneData::from_pixels(&px, n as u32, n as u32, &scene(-16, Edges::Open, 1.0));
		let mut e = CpuEngine::new(data, options(stencil, 2.0));
		e.set_emitters(&[emitter(45, 50, 100.0)]);
		for _ in 0..300 {
			e.advance_one_step();
		}
		let f = field(&mut e);
		let (mut inner, mut outer) = (0f64, 0f64);
		for (i, &v) in f.iter().enumerate() {
			let (x, y) = (i % n, i / n);
			if inside(x) && inside(y) {
				inner = inner.max((v as f64).abs());
			} else if !(28..68).contains(&x) || !(28..68).contains(&y) {
				outer = outer.max((v as f64).abs());
			}
		}
		println!("{stencil:?}: inside {inner:.3e}, outside {outer:.3e}");
		assert!(inner > 0.05 && outer < 1e-3 * inner, "{stencil:?}: inside {inner}, outside {outer}");
	}
}

/// The pixels of a `n` x `n` canvas with a square ring of mirror pixels two cells thick, n/8 in from
/// the edges, index 1 elsewhere.
fn mirror_ring(n: usize) -> Vec<u8> {
	let (a, b) = (n / 8, n - n / 8 - 2);
	let wall = |v: usize| (a..a + 2).contains(&v) || (b..b + 2).contains(&v);
	let span = a..b + 2;
	pixels(
		n,
		n,
		|x, y| if (wall(x) && span.contains(&y)) || (wall(y) && span.contains(&x)) { [0, 0, 0] } else { [100, 0, 0] },
	)
}

/// (8b) A mirror cavity stays bounded over 30,000 steps: a 32 x 32 reflective box with a mirror ring
/// inside, a 3-cycle burst in the ring, no damping. A mirror cell is a fixed zero (c2 = 0) and the taps
/// across it are cut, so each closed region runs a principal submatrix of the Laplacian, negative
/// semi-definite; when mirror cells moved (c2 = 1e-6) with their taps cut, the DRP's operator had a
/// growing mode and the field reached 78 by step 30,000 (from 0.45). The largest |u| from step 25,000
/// on must stay under twice the largest of the first 1000 steps.
#[test]
fn mirror_cavity_stays_bounded() {
	let n = 32usize;
	let data = SceneData::from_pixels(&mirror_ring(n), n as u32, n as u32, &scene(0, Edges::Reflective, 1.0));
	for stencil in STENCILS {
		let mut e = CpuEngine::new(data.clone(), options(stencil, 2.0));
		e.set_emitters(&[Emitter { detach_after_step: Some(200), ..pulse(15, 18, 100.0, 3) }]);
		let (mut early, mut late) = (0f64, 0f64);
		for step in 1..=30_000 {
			e.advance_one_step();
			if step <= 1000 && step % 10 == 0 {
				early = early.max(bound(&mut e));
			} else if step >= 25_000 && step % 500 == 0 {
				late = late.max(bound(&mut e));
			}
		}
		println!("{stencil:?}: largest |u| {early:.3} in the first 1000 steps, {late:.3} from step 25,000");
		assert!(early > 0.05 && late < 2.0 * early, "{stencil:?}: {early} then {late}");
	}
}

/// (8c) A mirror line one pixel wide on the diagonal, 8-connected (the painter's centre test gives it
/// for any mirror thinner than about 1.41 px at 45 degrees), splits a reflective 64 x 64 box: a
/// Gaussian bump on one side, at rest, puts under 1e-6 of the field's square sum on the other side
/// over 600 steps. The 9-point's diagonal taps would couple the two cells beside each mirror cell
/// straight across; they are cut (engine/pml.rs, `diagonal_cuts`). Without the cut 10 % crossed.
#[test]
fn diagonal_mirror_line_holds() {
	let n = 64usize;
	let px = pixels(n, n, |x, y| if x == y { [0, 0, 0] } else { [100, 0, 0] });
	let data = SceneData::from_pixels(&px, n as u32, n as u32, &scene(0, Edges::Reflective, 1.0));
	let start: Vec<f32> =
		bump(n, n, 40.0, 20.0, 3.0).iter().enumerate().map(|(i, &v)| if i % n > i / n { v } else { 0.0 }).collect();
	for stencil in STENCILS {
		let mut e = CpuEngine::new(data.clone(), options(stencil, 2.0));
		e.set_field(&start, &start);
		let mut worst = 0f64;
		for step in 1..=600 {
			e.advance_one_step();
			if step % 20 == 0 {
				let f = field(&mut e);
				let (mut across, mut total) = (0f64, 0f64);
				for (i, &v) in f.iter().enumerate() {
					let v2 = (v as f64).powi(2);
					total += v2;
					if i % n < i / n {
						across += v2;
					}
				}
				worst = worst.max(across / total);
			}
		}
		println!("{stencil:?}: at most {worst:.2e} of the field's square sum across the line");
		assert!(worst < 1e-6, "{stencil:?}: {worst}");
	}
}

/// (8d) The intensity's two-stage filter (docs/physics.md, Intensity) settles on the mean square of a
/// steady sinusoid: a continuous source of amplitude 0.5 at level 100 (scale 2, 16 steps a period) on
/// a 192 x 192 canvas with a 32 px CPML rim, the time constant 40 steps (TimeDecay 40). From 6 time
/// constants on, sqrt(I) averaged over a period at the source cell is within 1 % of the cell's fitted
/// swing over sqrt(2).
#[test]
fn intensity_settles_on_the_mean_square() {
	let (w, level, scale, time_decay) = (192usize, 100.0, 2.0, 40.0);
	let tau = time_decay * 100.0 / level;
	let omega = level_omega(level, scale);
	let period = (2.0 * PI / omega).round() as usize;
	let data = SceneData::blank(w as u32, w as u32, &scene(-32, Edges::Open, 1.0));
	for stencil in STENCILS {
		let opts = EngineOptions { track_intensity: true, time_decay, ..options(stencil, scale) };
		let mut e = CpuEngine::new(data.clone(), opts);
		let c = w as i32 / 2;
		e.set_emitters(&[Emitter { amplitude: 0.5, ..emitter(c, c, level) }]);
		let cell = c as usize * w + c as usize;
		let (mut swing, mut roots) = (Vec::new(), Vec::new());
		let steps = (10.0 * tau) as usize;
		for step in 1..=steps {
			e.advance_one_step();
			let (f, i) = e.prepare_frame();
			if step + 4 * period > steps {
				swing.push(f[cell] as f64);
			}
			roots.push((i[cell] as f64).sqrt());
		}
		let expected = fitted_amplitude(&swing, steps + 1 - swing.len(), omega) / 2f64.sqrt();
		let mean_at = |t: usize| roots[t - period..t].iter().sum::<f64>() / period as f64 / expected - 1.0;
		for k in [5usize, 6, 8, 10] {
			println!("{stencil:?}: sqrt(I) at {k} tau {:+.4} off A/sqrt(2) = {expected:.4}", mean_at(k * tau as usize));
		}
		for t in (6 * tau as usize..=steps).step_by(period) {
			assert!(mean_at(t).abs() < 0.01, "{stencil:?} step {t}: {}", mean_at(t));
		}
	}
}

/// (8e) A normally incident wave packet on a slab of index 1.5 whose face lies inside a pixel (at
/// x = 200.3: the pixel takes the effective medium of its 0.7 glass share, as the painter paints it)
/// reflects the Fresnel share (n - 1)/(n + 1) = 0.2 of its amplitude within 2 %: the reflected
/// packet's square sum against the incident one's, both in index 1. 400 x 200 canvas with a 32 px
/// CPML rim, the slab running into the rim; packet of 40 cells a wavelength, envelope 24 along and
/// 40 across. At 24 cells a wavelength the reflection came out 1.6 % (9-point) and 2.6 % (DRP) low:
/// the boundary pixel is a thin layer of intermediate index, whose effect grows as k^2.
#[test]
fn slab_reflects_the_fresnel_share() {
	let (w, h, face) = (400usize, 200usize, 200usize);
	let share = 0.7f64;
	let edge = ((share * 2.25 + (1.0 - share)).sqrt() * 100.0).round() as u8;
	let px = pixels(w, h, |x, _| match x {
		x if x < face => [100, 0, 0],
		x if x == face => [edge, 0, 0],
		_ => [150, 0, 0],
	});
	let rim = 32usize;
	let (gw, gh) = (w + 2 * rim, h + 2 * rim);
	let start = |t: f64| packet(gw, gh, 100.0 + rim as f64, (h / 2 + rim) as f64, 0.0, 40.0, 24.0, 40.0, t);
	let steps = 220;
	for stencil in STENCILS {
		let run = |px: &[u8]| {
			let data = SceneData::from_pixels(px, w as u32, h as u32, &scene(-(rim as i32), Edges::Open, 1.0));
			let mut e = CpuEngine::new(data, options(stencil, 2.0));
			let dt = 1.0 / e.sub_steps() as f64;
			e.set_field(&start(0.0), &start(-dt));
			let first = field(&mut e);
			for _ in 0..steps {
				e.advance_one_step();
			}
			(first, field(&mut e))
		};
		let (incident, with_slab) = run(&px);
		let (_, without) = run(&pixels(w, h, |_, _| [100, 0, 0]));
		let square = |f: &dyn Fn(usize) -> f64| (0..w * h).filter(|i| i % w < face).map(|i| f(i).powi(2)).sum::<f64>();
		let reflected = square(&|i| with_slab[i] as f64 - without[i] as f64).sqrt();
		let r = reflected / square(&|i| incident[i] as f64).sqrt();
		println!("{stencil:?}: reflection {r:.5}, Fresnel 0.2, off {:+.4}", r / 0.2 - 1.0);
		assert!((r / 0.2 - 1.0).abs() < 0.02, "{stencil:?}: {r}");
	}
}

/// (9) A slab guide (core 11 rows of index 1.6, cladding 1.45, open edges with a 32 px PML) fed by a
/// point source at its axis carries its fundamental mode with the mode solver's beta within 1e-3.
/// A 24-cell vacuum wavelength makes the guide single-mode (V = 0.97). The beta is the slope of the
/// phase along the guide, 360 to 560 cells from the source, of the field summed over the core rows
/// and demodulated at the source's frequency over the last 900 of 3000 steps (Hann window), where
/// the radiated part has left the core. Measured 5.8e-5 (9-point) and 1.0e-4 (DRP); 160 to 300
/// cells out the radiation still in the core pulled both to 6.5e-4.
#[test]
fn slab_guide_carries_the_mode_solvers_beta() {
	let (w, h, rim) = (700usize, 141usize, 32u32);
	let core = 65..=75usize;
	let index = |y: usize| if core.contains(&y) { 1.6f64 } else { 1.45 };
	let px = pixels(w, h, |_, y| [(index(y) * 100.0).round() as u8, 0, 0]);
	let (level, scale) = (66.76, 2.0);
	let omega = level_omega(level, scale);
	let lam = 2.0 * PI / omega;
	let (steps, window) = (3000usize, 900usize);
	for stencil in STENCILS {
		let data = SceneData::from_pixels(&px, w as u32, h as u32, &scene(-(rim as i32), Edges::Open, 1.0));
		let mut e = CpuEngine::new(data, options(stencil, scale));
		e.set_emitters(&[emitter(40, 70, level)]);
		let xs: Vec<usize> = (400..=600).collect();
		let (mut re, mut im) = (vec![0f64; xs.len()], vec![0f64; xs.len()]);
		for step in 1..=steps {
			e.advance_one_step();
			if step > steps - window {
				let t = (step - (steps - window)) as f64;
				let hann = 0.5 - 0.5 * (2.0 * PI * t / window as f64).cos();
				let f = field(&mut e);
				for (j, &x) in xs.iter().enumerate() {
					let u: f64 = core.clone().map(|y| f[y * w + x] as f64).sum();
					re[j] += hann * u * (omega * step as f64).cos();
					im[j] += hann * u * (omega * step as f64).sin();
				}
			}
		}
		let mut phase: Vec<f64> = re.iter().zip(&im).map(|(r, i)| i.atan2(*r)).collect();
		for j in 1..phase.len() {
			while phase[j] - phase[j - 1] > PI {
				phase[j] -= 2.0 * PI;
			}
			while phase[j] - phase[j - 1] < -PI {
				phase[j] += 2.0 * PI;
			}
		}
		let n = xs.len() as f64;
		let (mx, mp) = (xs.iter().map(|&x| x as f64).sum::<f64>() / n, phase.iter().sum::<f64>() / n);
		let (sxy, sxx) = xs
			.iter()
			.zip(&phase)
			.fold((0.0, 0.0), |(a, b), (&x, &p)| (a + (x as f64 - mx) * (p - mp), b + (x as f64 - mx).powi(2)));
		let measured = (sxy / sxx).abs();
		let solver = ModeSolver::new(stencil, e.sub_steps());
		let profile: Vec<f64> = (0..h).map(index).collect();
		let mode = solver.beta(&profile, solver.omega_term(lam), false).expect("guided");
		let off = (measured - mode.beta).abs() / mode.beta;
		println!(
			"{stencil:?}: beta {measured:.6} rad/cell simulated, {:.6} from the mode solver ({} sub-steps), off {off:.2e}",
			mode.beta,
			e.sub_steps()
		);
		assert!(off < 1e-3, "{stencil:?}: {measured} against {}", mode.beta);
	}
}

/// Largest |u| of the canvas, infinite once a value is not finite.
fn bound(engine: &mut CpuEngine) -> f64 {
	let f = field(engine);
	if f.iter().all(|v| v.is_finite()) { peak(&f) } else { f64::INFINITY }
}

/// (10a) Open edges stay bounded for every sub-step count and both stencils, behind an inside damping
/// band (whose outermost ring has d = 0) and without one: a 64 x 48 canvas of index 1.25 (Courant 0.8
/// at one sub-step, within the CFL bound), a band of 6 or none and a continuous source, 5000 steps.
/// Before the damped update was centred the band diverged where `d^(1/n)` is small, and before the
/// outer rings were extrapolated linearly the DRP stencil diverged at the corners at one sub-step.
#[test]
fn open_edges_stay_bounded_at_every_sub_step_count() {
	for band in [6, 0] {
		let data = SceneData::blank(64, 48, &scene(band, Edges::Open, 1.25));
		for stencil in STENCILS {
			for n in 1..=4 {
				let mut e = CpuEngine::with_sub_steps(data.clone(), options(stencil, 2.0), n);
				e.set_emitters(&[emitter(32, 24, 100.0)]);
				let mut top = 0f64;
				for step in 1..=5000 {
					e.advance_one_step();
					if step % 100 == 0 {
						top = top.max(bound(&mut e));
					}
				}
				println!("band {band}, {stencil:?}, {n} sub-steps: peak {top:.3}");
				assert!(top < 10.0, "band {band}, {stencil:?} at {n} sub-steps: peak {top}");
			}
		}
	}
}

/// (10b) Example18 (open edges, a damping band of 32 inside the canvas, no rim) stays bounded over
/// 3000 steps; it diverged from step 160 at the top edge before the damped update was centred.
#[test]
fn example18_stays_bounded() {
	use wavedude_core::engine::emitters::emitters_of;
	let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/examples/Example18_XOR_AND_OR_logic.wdproj");
	let project = wavedude_core::io::load_project(&path).unwrap();
	let px = wavedude_core::scene::compose::compose_scene(&project).expect("painted parts");
	let data = SceneData::from_pixels(&px.rgba, px.width, px.height, &project.scene);
	let sim = &project.simulation;
	let opts = EngineOptions {
		stencil: sim.stencil,
		source_frequency_scale: sim.source_frequency_scale,
		..options(sim.stencil, 1.0)
	};
	let mut e = CpuEngine::new(data, opts);
	let emitters = emitters_of(&project);
	let amplitude = emitters.iter().map(|e| e.amplitude).fold(0.0, f64::max);
	e.set_emitters(&emitters);
	let mut top = 0f64;
	for step in 1..=3000 {
		e.advance_one_step();
		if step % 50 == 0 {
			top = top.max(bound(&mut e));
			assert!(top < 10.0 * amplitude, "step {step}: peak {top}, source amplitude {amplitude}");
		}
	}
	println!("Example18, {} sub-steps: peak {top:.3} over 3000 steps, source amplitude {amplitude}", e.sub_steps());
}

/// (11) A source is a forcing term `f = omega^2 A sin(omega t + phi)` added to `u_tt` at every sub-step
/// (engine/forcing.rs), so its radiation does not depend on the sub-step count: a continuous level-100
/// source of amplitude 1 on a 256 x 256 open canvas of index 1.25 (within the CFL bound at one
/// sub-step) gives the same RMS field over the canvas after 600 steps at 1 to 4 sub-steps (measured
/// 0.5 % apart on the 9-point stencil, 0.2 % on the DRP, the two stencils 4 % apart).
#[test]
fn source_strength_is_independent_of_the_sub_steps() {
	let n = 256usize;
	let data = SceneData::blank(n as u32, n as u32, &scene(0, Edges::Open, 1.25));
	let c = n as i32 / 2;
	let cell = c as usize * n + c as usize;
	for stencil in STENCILS {
		let rms: Vec<f64> = (1..=4)
			.map(|subs| {
				let mut e = CpuEngine::with_sub_steps(data.clone(), options(stencil, 2.0), subs);
				e.set_emitters(&[emitter(c, c, 100.0)]);
				let mut swing = 0f64;
				for step in 1..=600 {
					e.advance_one_step();
					if step > 500 {
						swing = swing.max((field(&mut e)[cell] as f64).abs());
					}
				}
				let f = field(&mut e);
				let rms = (f.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / f.len() as f64).sqrt();
				println!("{stencil:?}, {subs} sub-steps: source cell amplitude {swing:.4}, RMS {rms:.5e}");
				rms
			})
			.collect();
		let (lo, hi) = rms.iter().fold((f64::MAX, 0f64), |(l, h), &r| (l.min(r), h.max(r)));
		assert!(hi / lo < 1.03, "{stencil:?}: RMS {rms:?}");
	}
}

/// (12) Sources are transparent: a wave packet crossing the cell of a source of amplitude 0, or of a
/// pulsed source that is silent until it detaches, is the same bit for bit as without it; crossing a
/// weak continuous source, the field is the packet's plus the source's own (superposition) within f32
/// round-off. 160 x 80 open canvas, packet of 12 cells a wavelength from x = 40 towards the source at
/// x = 80, 120 steps, both stencils.
#[test]
fn sources_are_transparent_to_passing_waves() {
	let (w, h) = (160usize, 80usize);
	let data = SceneData::blank(w as u32, h as u32, &scene(0, Edges::Open, 1.0));
	let silent = Emitter {
		pulsed: true,
		randomize_off_width: 1,
		off_width_cycles: 1000,
		off_width_base: 1000,
		detach_after_step: Some(0),
		..emitter(80, 40, 100.0)
	};
	for stencil in STENCILS {
		let run = |emitters: &[Emitter], with_packet: bool| {
			let mut e = CpuEngine::new(data.clone(), options(stencil, 2.0));
			if with_packet {
				let dt = 1.0 / e.sub_steps() as f64;
				let now = packet(w, h, 40.0, 40.0, 0.0, 12.0, 6.0, 8.0, 0.0);
				let before = packet(w, h, 40.0, 40.0, 0.0, 12.0, 6.0, 8.0, -dt);
				e.set_field(&now, &before);
			}
			e.set_emitters(emitters);
			for _ in 0..120 {
				e.advance_one_step();
			}
			field(&mut e)
		};
		let alone = run(&[], true);
		let bits = |f: &[f32]| f.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
		assert_eq!(bits(&run(&[Emitter { amplitude: 0.0, ..emitter(80, 40, 100.0) }], true)), bits(&alone));
		assert_eq!(bits(&run(std::slice::from_ref(&silent), true)), bits(&alone));
		let weak = [Emitter { amplitude: 0.01, ..emitter(80, 40, 100.0) }];
		let (both, source) = (run(&weak, true), run(&weak, false));
		let off = both.iter().zip(&source).zip(&alone).fold(0f64, |m, ((&b, &s), &a)| m.max((b - s - a).abs() as f64));
		let rel = off / peak(&alone);
		println!("{stencil:?}: packet peak {:.3}, superposition off by {rel:.2e} of it", peak(&alone));
		assert!(rel < 1e-5, "{stencil:?}: {rel}");
	}
}

/// Amplitude of the best fit `a sin(omega t) + b cos(omega t)` to `samples` taken at steps `t0..`.
fn fitted_amplitude(samples: &[f64], t0: usize, omega: f64) -> f64 {
	let (mut ss, mut cc, mut sc, mut ys, mut yc) = (0.0, 0.0, 0.0, 0.0, 0.0);
	for (i, &y) in samples.iter().enumerate() {
		let (s, c) = (omega * (t0 + i) as f64).sin_cos();
		(ss, cc, sc, ys, yc) = (ss + s * s, cc + c * c, sc + s * c, ys + y * s, yc + y * c);
	}
	let det = ss * cc - sc * sc;
	let (a, b) = ((ys * cc - yc * sc) / det, (yc * ss - ys * sc) / det);
	a.hypot(b)
}

/// (13) Amplitude is the swing a continuous source gives its own cell in free space: levels 20, 50,
/// 100 and 200 (scale 2) of amplitude 0.5 on a 320 x 320 canvas with a 32 px CPML rim (the return it lets
/// through is below the fit's resolution: the same offsets to 0.001 as on 512 x 512), both stencils,
/// at two sub-steps (index 1, the definition) and at one (index 1.25, which one sub-step needs: there
/// the cell swings by the ratio of the two media's |G0|, engine/green.rs). The amplitude is fitted over
/// the last 240 of 520 steps, three periods at level 20 (measured within 0.2 % of the expectation).
#[test]
fn source_amplitude_is_the_free_space_swing_of_its_cell() {
	use wavedude_core::engine::green::source_response;
	let (w, amplitude, scale) = (320usize, 0.5, 2.0);
	let c = w as i32 / 2;
	for stencil in STENCILS {
		let coefficients = StencilCoefficients::of(stencil);
		let scheme = TimeScheme::of(stencil);
		for (n, index) in [(2u32, 1.0), (1, 1.25)] {
			let data = SceneData::blank(w as u32, w as u32, &scene(-32, Edges::Open, index));
			for level in [20.0, 50.0, 100.0, 200.0] {
				let omega = level_omega(level, scale);
				let c2 = 1.0 / (index * index);
				let expected = amplitude * source_response(&coefficients, scheme, n, omega, c2)
					/ source_response(&coefficients, scheme, n, omega, 1.0);
				let mut e = CpuEngine::with_sub_steps(data.clone(), options(stencil, scale), n);
				e.set_emitters(&[Emitter { amplitude, ..emitter(c, c, level) }]);
				let cell = data.padded_index(32 + c as usize, 32 + c as usize);
				let mut samples = Vec::new();
				for step in 1..=520 {
					e.advance_one_step();
					if step > 280 {
						samples.push(e.field_padded()[cell] as f64);
					}
				}
				let swing = fitted_amplitude(&samples, 281, omega);
				let off = swing / expected - 1.0;
				println!(
					"{stencil:?}, {n} sub-steps, index {index}, level {level}: swing {swing:.4}, expected {expected:.4}, off {off:+.3}"
				);
				assert!(off.abs() < 0.05, "{stencil:?} n {n} level {level}: {swing} against {expected}");
			}
		}
	}
}

/// A source at a fractional position: its Amplitude is the swing of the field sampled at the position
/// with the bilinear shares of its taps (engine/green.rs, `spread_response`), at a half-cell offset
/// along x and at the centre of four cells, level 50 and 100, both stencils, two sub-steps, as in
/// `source_amplitude_is_the_free_space_swing_of_its_cell`.
#[test]
fn spread_source_amplitude_is_the_swing_at_its_position() {
	let (w, amplitude, scale) = (320usize, 0.5, 2.0);
	let c = w / 2;
	for stencil in STENCILS {
		let data = SceneData::blank(w as u32, w as u32, &scene(-32, Edges::Open, 1.0));
		for (fx, fy) in [(0.5, 0.0), (0.5, 0.5), (0.25, 0.75)] {
			for level in [50.0, 100.0] {
				let omega = level_omega(level, scale);
				let mut e = CpuEngine::with_sub_steps(data.clone(), options(stencil, scale), 2);
				e.set_emitters(&[Emitter { amplitude, x: c as f64 + fx, y: c as f64 + fy, ..emitter(0, 0, level) }]);
				let shares = wavedude_core::engine::green::bilinear(fx, fy);
				let cells =
					[(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| data.padded_index(32 + c + dx, 32 + c + dy));
				let mut samples = Vec::new();
				for step in 1..=520 {
					e.advance_one_step();
					if step > 280 {
						let u = e.field_padded();
						samples.push(cells.iter().zip(shares).map(|(&i, s)| u[i] as f64 * s).sum());
					}
				}
				let swing = fitted_amplitude(&samples, 281, omega);
				let off = swing / amplitude - 1.0;
				println!("{stencil:?}, fraction ({fx}, {fy}), level {level}: swing {swing:.4}, off {off:+.3}");
				assert!(off.abs() < 0.05, "{stencil:?} ({fx}, {fy}) level {level}: {swing} against {amplitude}");
			}
		}
	}
}

/// A source on a cell boundary radiates a field mirror-symmetric about that boundary: the two shares
/// are equal, so the rows above and below agree (f32 round-off apart), where a one-cell source half a
/// cell off the boundary does not.
#[test]
fn a_source_on_a_cell_boundary_radiates_symmetrically() {
	let (w, scale) = (256usize, 2.0);
	let c = w / 2;
	for stencil in STENCILS {
		let data = SceneData::blank(w as u32, w as u32, &scene(-32, Edges::Open, 1.0));
		let asymmetry = |y: f64| {
			let mut e = CpuEngine::with_sub_steps(data.clone(), options(stencil, scale), 2);
			e.set_emitters(&[Emitter { x: c as f64, y, ..emitter(0, 0, 100.0) }]);
			for _ in 0..300 {
				e.advance_one_step();
			}
			let u = e.field_padded();
			let mut worst = 0f64;
			let mut peak = 0f64;
			for k in 1..60 {
				for x in c - 60..c + 60 {
					let above = u[data.padded_index(32 + x, 32 + c - k)] as f64;
					let below = u[data.padded_index(32 + x, 32 + c - 1 + k)] as f64;
					worst = worst.max((above - below).abs());
					peak = peak.max(above.abs());
				}
			}
			worst / peak
		};
		let on_boundary = asymmetry(c as f64 - 0.5);
		let off_boundary = asymmetry(c as f64);
		println!("{stencil:?}: asymmetry {on_boundary:.2e} on the boundary, {off_boundary:.2e} half a cell off");
		assert!(on_boundary < 1e-5, "{stencil:?}: {on_boundary}");
		assert!(off_boundary > 1e-2, "{stencil:?}: {off_boundary}");
	}
}
