//! Second regimes of rules whose first regime is tested beside their code: pulse envelopes under a random gap,
//! the intensity weight's other lowest levels, lens fits decided by a concave sag or by equal zone depths, the
//! inverse zone plate, the squeezed fibre hump and the glowing edges' vertical gradient.

use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};
use std::num::NonZeroU32;

use wavedude_core::engine::cpu::CpuEngine;
use wavedude_core::engine::envelope::Pulse;
use wavedude_core::engine::forcing::Sources;
use wavedude_core::engine::stencil::StencilCoefficients;
use wavedude_core::engine::{Emitter, EngineOptions, ImageSource, SceneData, WaveEngine};
use wavedude_core::model::lens_geometry::{edge_thickness, fit_radii, fit_radii_zoned, sag, zone_sag};
use wavedude_core::model::normalize::Normalize;
use wavedude_core::model::*;
use wavedude_core::rng::xorshift32;
use wavedude_core::scene::glowing_edges;
use wavedude_core::scene::painter::paint_part;
use wavedude_core::scene::pixels::ScenePixels;
use wavedude_core::scene::shapes::{BLOCK_ABSORPTION, zone_at};

/// Half-width in cycles of the smoothed square's edges (engine/envelope.rs `EDGE_WIDTH`).
const EDGE_WIDTH: f64 = 0.35;

fn pulsed(shape: PulseShape, on: u32, off: u32, random: u32) -> Emitter {
	Emitter {
		x: 0.0,
		y: 0.0,
		level: 100.0,
		amplitude: 1.0,
		phase_radians: 0.0,
		pulsed: true,
		shape,
		on_width_cycles: on,
		off_width_cycles: off,
		off_width_base: off,
		randomize_off_width: random,
		detach_after_step: None,
		rng_seed: NonZeroU32::new(12345).unwrap(),
	}
}

/// Start and gap (cycles) of the first `count` periods of a random-gap emitter as engine/envelope.rs draws
/// them: the first gap the emitter's own, each later one `max(1, base + x % (2 spread + 1) - spread)` for the
/// next xorshift32 state x from the seed. A period is its gap followed by its burst.
fn periods(e: &Emitter, count: usize) -> Vec<(f64, f64)> {
	let spread = e.randomize_off_width as i64;
	let (mut anchor, mut off, mut rng) = (0.0, e.off_width_cycles as i64, e.rng_seed.get());
	let mut out = Vec::with_capacity(count);
	for _ in 0..count {
		out.push((anchor, off as f64));
		anchor += (e.on_width_cycles as i64 + off) as f64;
		rng = xorshift32(rng);
		off = (e.off_width_base as i64 + (rng % (2 * spread + 1) as u32) as i64 - spread).max(1);
	}
	out
}

/// The envelope at `cycles` from a fresh pulse state (omega = TAU, so t counts cycles).
fn envelope_at(e: &Emitter, cycles: f64) -> f64 {
	Pulse::new(e).envelope(e, TAU, cycles)
}

/// The smoothed square's rising step about 0, `x` in cycles.
fn rise(x: f64) -> f64 {
	0.5 + (x / EDGE_WIDTH).atan() / PI
}

fn distinct_gaps(periods: &[(f64, f64)]) -> usize {
	periods.iter().map(|&(_, off)| off as u32).collect::<BTreeSet<_>>().len()
}

/// A Gaussian pulse under a random gap (6 cycles on, gap 5 +- 4): silent through the first gap, each burst
/// peaking at its period's start plus that period's drawn gap plus half the burst, and cut at both edges at
/// `exp(-4.5)` (3 sigma, sigma = on / 6) from 0 outside.
#[test]
fn gaussian_under_a_random_gap_peaks_after_each_drawn_gap() {
	let e = pulsed(PulseShape::Gaussian, 6, 5, 4);
	let on = e.on_width_cycles as f64;
	let periods = periods(&e, 40);
	assert!(distinct_gaps(&periods) > 4, "the gap changes between periods: {periods:?}");
	let first = periods[0].1;
	for k in 0..100 {
		let t = first * k as f64 / 100.0;
		assert_eq!(envelope_at(&e, t), 0.0, "silent in the first gap at {t}");
	}
	let edge = (-4.5f64).exp();
	for (k, &(anchor, off)) in periods.iter().enumerate() {
		let start = anchor + off;
		let peak = envelope_at(&e, start + on / 2.0);
		assert!((peak - 1.0).abs() < 1e-12, "period {k}: peak {peak} at {}, want 1 within 1e-12", start + on / 2.0);
		let (left, right) = (envelope_at(&e, start + on / 2.0 - 1.0), envelope_at(&e, start + on / 2.0 + 1.0));
		assert!((left - right).abs() < 1e-12 && left < 1.0, "period {k}: symmetric about the peak within 1e-12");
		assert_eq!(envelope_at(&e, anchor + off / 2.0), 0.0, "period {k}: silent in the gap");
		assert_eq!(envelope_at(&e, start - 1e-9), 0.0, "period {k}: silent just before the burst");
		let rising = envelope_at(&e, start + 1e-9);
		assert!((rising - edge).abs() < 1e-6, "period {k}: {rising} at the burst's start, want exp(-4.5) within 1e-6");
		let falling = envelope_at(&e, start + on - 1e-9);
		assert!((falling - edge).abs() < 1e-6, "period {k}: {falling} at the burst's end, want exp(-4.5) within 1e-6");
	}
}

/// A smoothed-square pulse under a random gap (3 cycles on, gap 4 +- 3), sampled every step of a level-100
/// source at scale 2: no step moves it by more than the steepest edge allows, `2 / (pi EDGE_WIDTH)` per cycle,
/// across period boundaries where the gap changes. At each later gap's middle the burst before's tail and the
/// next burst's rise meet at that gap's half, `rise(on + off/2) (1 - rise(off/2))`, the gap's lowest point.
#[test]
fn smoothed_square_under_a_random_gap_is_continuous_and_shares_each_gap_at_its_middle() {
	let e = pulsed(PulseShape::SmoothedSquare, 3, 4, 3);
	let on = e.on_width_cycles as f64;
	let periods = periods(&e, 30);
	assert!(distinct_gaps(&periods) > 3, "the gap changes between periods: {periods:?}");
	let omega = level_omega(100.0, 2.0);
	let per_step = omega / TAU;
	let bound = 2.0 / (PI * EDGE_WIDTH) * per_step;
	let steps = (periods[periods.len() - 1].0 / per_step).ceil() as u32;
	let mut pulse = Pulse::new(&e);
	let mut before = pulse.envelope(&e, omega, 0.0);
	for t in 1..=steps {
		let now = pulse.envelope(&e, omega, t as f64);
		assert!(
			(now - before).abs() <= bound,
			"step {t} (cycle {:.3}): {before} -> {now}, more than the edge slope's {bound:.4} a step",
			t as f64 * per_step
		);
		before = now;
	}
	for (k, &(anchor, off)) in periods.iter().enumerate().skip(1) {
		let half = off / 2.0;
		let expected = rise(on + half) * (1.0 - rise(half));
		let middle = envelope_at(&e, anchor + half);
		assert!(
			(middle - expected).abs() < 1e-9,
			"period {k} (gap {off}): {middle} at the middle, want {expected} within 1e-9"
		);
		let (left, right) = (envelope_at(&e, anchor + half - 0.05), envelope_at(&e, anchor + half + 0.05));
		assert!(
			left > middle && right > middle,
			"period {k}: the middle {middle} is the gap's lowest ({left}, {right})"
		);
	}
}

fn intensity_options(time_decay: f64) -> EngineOptions {
	EngineOptions {
		stencil: Stencil::NinePoint,
		source_frequency_scale: 2.0,
		image_source_scale: 1.0,
		time_decay,
		intensity_scale: 1.0,
		track_intensity: true,
	}
}

/// `exp(-1 / max(1, time_decay * 100 / lowest))` in f32 (docs/physics.md, Intensity).
fn alpha_of(time_decay: f64, lowest: f64) -> f32 {
	(-1.0 / (time_decay * 100.0 / lowest).max(1.0)).exp() as f32
}

/// A 64 x 64 reflective canvas without a rim.
fn canvas() -> SceneData {
	SceneData::blank(64, 64, &Scene { damping_edge: 0, edges: Edges::Reflective, ..Scene::default() })
}

/// Field and intensity at canvas `cell` after each of `steps` steps.
fn record(engine: &mut CpuEngine, cell: usize, steps: usize) -> Vec<(f64, f64)> {
	(0..steps)
		.map(|_| {
			engine.advance_one_step();
			let (f, i) = engine.prepare_frame();
			(f[cell] as f64, i[cell] as f64)
		})
		.collect()
}

/// Largest gap between the recorded intensity and the two-stage filter of the recorded field with weight
/// `alpha` replayed in f64, relative to the largest intensity.
fn mismatch(history: &[(f64, f64)], alpha: f32) -> f64 {
	let (a, rest) = (alpha as f64, 1.0 - alpha as f64);
	let (mut s1, mut s2, mut worst, mut top) = (0.0, 0.0, 0f64, 0f64);
	for &(u, i) in history {
		s1 = a * s1 + rest * u * u;
		s2 = a * s2 + rest * s1;
		worst = worst.max((i - (s1 + s2) / 2.0).abs());
		top = top.max(i);
	}
	worst / top
}

fn source(x: f64, y: f64, level: f64) -> Emitter {
	Emitter { x, y, level, pulsed: false, ..pulsed(PulseShape::Gaussian, 1, 1, 0) }
}

/// An image source at level 30 below a level-100 emitter sets the lowest level: tau = 10 * 100 / 30 steps.
/// The engine's intensity at an undriven cell is the replayed filter at that weight within 1e-4 of its peak,
/// and more than 1e-2 off the emitter's weight alone.
#[test]
fn an_image_level_below_the_emitters_sets_the_intensity_weight() {
	let mut sources = Sources::new(StencilCoefficients::of(Stencil::NinePoint), 2.0, 1.0);
	sources.level_used[30] = true;
	sources.emitters.push(source(44.0, 32.0, 100.0));
	assert_eq!(sources.intensity_alpha(10.0), alpha_of(10.0, 30.0));

	let mut data = canvas();
	data.image_sources.push(ImageSource { gx: 20, gy: 32, level: 30 });
	let mut engine = CpuEngine::new(data, intensity_options(10.0));
	engine.set_emitters(&[source(44.0, 32.0, 100.0)]);
	let history = record(&mut engine, 32 * 64 + 23, 200);
	let (right, emitter_only) = (mismatch(&history, alpha_of(10.0, 30.0)), mismatch(&history, alpha_of(10.0, 100.0)));
	assert!(right < 1e-4, "replayed at the image level's weight: {right:.2e}, want under 1e-4");
	assert!(emitter_only > 1e-2, "replayed at the emitter's weight: {emitter_only:.2e}, want over 1e-2");
}

/// TimeDecay 0.5 at level 100 gives tau 0.5, clamped to one step: alpha = exp(-1), not exp(-2). The engine's
/// intensity is the replayed filter at exp(-1) within 1e-4 of its peak.
#[test]
fn a_small_time_decay_clamps_tau_to_one_step() {
	let mut sources = Sources::new(StencilCoefficients::of(Stencil::NinePoint), 2.0, 1.0);
	sources.emitters.push(source(32.0, 32.0, 100.0));
	assert_eq!(sources.intensity_alpha(0.5), (-1f64).exp() as f32);

	let mut engine = CpuEngine::new(canvas(), intensity_options(0.5));
	engine.set_emitters(&[source(32.0, 32.0, 100.0)]);
	let history = record(&mut engine, 32 * 64 + 35, 120);
	let (clamped, unclamped) = (mismatch(&history, (-1f64).exp() as f32), mismatch(&history, (-2f64).exp() as f32));
	assert!(clamped < 1e-4, "replayed at exp(-1): {clamped:.2e}, want under 1e-4");
	assert!(unclamped > 1e-2, "replayed at exp(-2): {unclamped:.2e}, want over 1e-2");
}

/// Without sources the lowest level is 100: a free Gaussian bump under TimeDecay 10 filters at tau = 10 steps,
/// within 1e-4 of its peak, and more than 1e-2 off a weight of exp(-1). Level 0 in the table is no source.
#[test]
fn without_sources_the_lowest_level_is_100() {
	let mut sources = Sources::new(StencilCoefficients::of(Stencil::NinePoint), 2.0, 1.0);
	assert_eq!(sources.intensity_alpha(10.0), alpha_of(10.0, 100.0));
	sources.level_used[0] = true;
	assert_eq!(sources.intensity_alpha(10.0), alpha_of(10.0, 100.0), "level 0 is not a source level");

	let mut engine = CpuEngine::new(canvas(), intensity_options(10.0));
	let bump: Vec<f32> = (0..64 * 64)
		.map(|i| {
			let (x, y) = ((i % 64) as f64 - 32.0, (i / 64) as f64 - 32.0);
			(-(x * x + y * y) / 18.0).exp() as f32
		})
		.collect();
	engine.set_field(&bump, &bump);
	let history = record(&mut engine, 32 * 64 + 40, 120);
	let (right, wrong) = (mismatch(&history, alpha_of(10.0, 100.0)), mismatch(&history, (-1f64).exp() as f32));
	assert!(right < 1e-4, "replayed at tau 10: {right:.2e}, want under 1e-4");
	assert!(wrong > 1e-2, "replayed at exp(-1): {wrong:.2e}, want over 1e-2");
}

/// A meniscus, R1 = R2 = 60 on a 100 px lens 20 px thick: the concave back's 26.8 px sag widens the budget to
/// 46.8 px, so the convex front of the same sag keeps its radius and the rim keeps the centre thickness, where
/// the front alone or against a convex back of -60 is refit flatter. A concave front of -80 likewise holds a
/// convex back of -60.
#[test]
fn a_concave_sag_widens_the_convex_budget() {
	let depth = ZoneDivision::EqualWidth;
	assert!(sag(60.0, 50.0) > 20.0, "the convex sag alone exceeds the thickness");
	assert_eq!(fit_radii(100, 20, 60.0, 60.0), (60.0, 60.0));
	assert_eq!(edge_thickness(100, 20, 60.0, 60.0, 1, depth), 20.0);
	assert!(fit_radii(100, 20, 60.0, 0.0).0 > 60.0, "a plano-convex front is refit");
	let (f1, f2) = fit_radii(100, 20, 60.0, -60.0);
	assert!(f1 > 60.0 && f2 < -60.0, "a biconvex lens is refit: {f1} {f2}");

	assert_eq!(fit_radii(100, 20, -80.0, -60.0), (-80.0, -60.0));
	assert!(edge_thickness(100, 20, -80.0, -60.0, 1, depth) >= 0.0);
	assert!(fit_radii(100, 20, 0.0, -60.0).1 < -60.0, "a plano-convex back is refit");
}

/// Four zones of equal depth on a 500 px plate 4 px thick with R1 = 300: a zone's depth, a quarter of the
/// 134 px rim sag, exceeds the thickness, so R1 is refit to the sagitta radius of a 16 px whole sag,
/// `(250^2 + 16^2) / 32 = 1961.125`, rounded up. Its zone depth fits the thickness, a pixel less does not.
#[test]
fn equal_depth_zones_refit_through_the_whole_sag() {
	let equal_depth = ZoneDivision::EqualDepth;
	assert!(zone_sag(300.0, 250.0, 4, equal_depth) > 4.0);
	let (r1, r2) = fit_radii_zoned(500, 4, 300.0, 0.0, 4, equal_depth);
	assert_eq!((r1, r2), (1962.0, 0.0));
	assert!(zone_sag(r1, 250.0, 4, equal_depth) <= 4.0 && zone_sag(r1 - 1.0, 250.0, 4, equal_depth) > 4.0);
	assert!(edge_thickness(500, 4, r1, r2, 4, equal_depth) >= 0.0);
	assert_ne!(
		fit_radii_zoned(500, 4, 300.0, 0.0, 4, ZoneDivision::EqualWidth).0,
		r1,
		"equal widths fit another radius"
	);
}

fn painted(part: Part, w: u32, h: u32) -> ScenePixels {
	let mut part = part;
	part.normalize();
	let mut pixels = ScenePixels::blank(w, h, 1.0);
	paint_part(&mut pixels, &part, &Simulation::default(), 7);
	pixels
}

/// A zone plate of 480 px without a coating, painted with and without Inverse: down the axis column every row
/// within the diameter is a full block in exactly one of the two, the plain plate blocking the odd zones
/// (`zone_at`) and the inverse one the even zones from zone 0 at the axis; beyond the diameter neither blocks.
#[test]
fn inverse_zone_plate_blocks_the_even_zones() {
	let plate = ZonePlate { diameter: 480, focal_length: 360, thickness: 64, ..ZonePlate::default() };
	let lambda = wavelength(plate.design_frequency, 1.0, Simulation::default().source_frequency_scale);
	let (cx, cy) = (240u32, 320u32);
	let paint =
		|inverse| painted(Part::new(PartKind::ZonePlate(ZonePlate { inverse, ..plate.clone() }), 240, 320), 960, 640);
	let (plain, inverse) = (paint(false), paint(true));
	let full = BLOCK_ABSORPTION as u8;
	assert_eq!((plain.blue(cx, cy), inverse.blue(cx, cy)), (0, full), "zone 0 at the axis");
	let mut zones = BTreeSet::new();
	for y in 0..640 {
		let across = (y as f64 + 0.5 - cy as f64).abs();
		let (a, b) = (plain.blue(cx, y), inverse.blue(cx, y));
		if across < 240.0 {
			let zone = zone_at(across, lambda, plate.focal_length);
			zones.insert(zone);
			let odd = if zone % 2 == 1 { full } else { 0 };
			assert_eq!((a, b), (odd, full - odd), "row {y}, zone {zone}");
		} else {
			assert_eq!((a, b), (0, 0), "row {y} beyond the diameter");
		}
	}
	assert!(zones.contains(&1) && zones.len() > 3, "zones crossed: {zones:?}");
}

/// A fibre of core radius 3 and cladding 20 (outer radius 23) with an absorption ramp of 30, wider than half
/// the cladding: the hump is squeezed to half-width 10, so it spans exactly the cladding, 0 at r <= R_c and
/// r >= R_o, peaking at `R_o - cladding / 2 = 13` and symmetric about it; the core stays clear. Within one
/// level of `round(200 t^2)` (t the hump's profile parameter).
#[test]
fn a_wide_ramp_squeezes_the_fibre_hump_into_the_cladding() {
	let (core, outer, hump, absorption) = (3.0, 23.0, 10.0, 200.0);
	let nodes = vec![
		FiberNode { x: -60, out_x: 67, ..FiberNode::default() },
		FiberNode { x: 60, in_x: -67, ..FiberNode::default() },
	];
	let fiber = Fiber {
		core_diameter: 6,
		cladding_thickness: 20,
		absorption: absorption as u32,
		absorption_ramp: 30,
		nodes,
		..Fiber::default()
	};
	let p = painted(Part::new(PartKind::Fiber(fiber), 100, 100), 200, 200);
	// pixel row y has r = y + 0.5 - 100
	let at = |r: f64| p.blue(100, (100.0 + r - 0.5) as u32) as i32;
	let expected = |r: f64| {
		if r <= core || r >= outer {
			return 0;
		}
		let t = (r - core) / hump;
		let t = if t <= 1.0 { t } else { 2.0 - t };
		(absorption * t * t).round_ties_even() as i32
	};
	for k in -30..30 {
		let r = k as f64 + 0.5;
		let (got, want) = (at(r), expected(r.abs()));
		assert!((got - want).abs() <= 1, "r = {r}: blue {got}, want {want} within 1");
		if r.abs() <= core || r.abs() >= outer {
			assert_eq!(got, 0, "r = {r}: nothing outside the cladding");
		}
	}
	let peak = outer - 10.0;
	assert_eq!(at(peak - 0.5), at(peak + 0.5), "the peak sits at R_o - cladding / 2");
	assert!(at(peak - 0.5) > at(peak - 1.5) && at(peak + 0.5) > at(peak + 1.5) && at(peak - 0.5) >= 175);
	for d in 1..10 {
		let d = d as f64 + 0.5;
		assert!((at(peak - d) - at(peak + d)).abs() <= 1, "symmetric about the peak at +-{d} within 1");
	}
}

/// A horizontal step from black to (200, 40, 0) at row 4 of an 8 x 6 image lights rows 3 and 4 with the step's
/// colour (`|gy| = 4 * 200`, times 0.25) inside the black outer ring, as the vertical step lights its columns.
#[test]
fn a_horizontal_step_lights_the_two_rows_beside_it() {
	let (w, h) = (8, 6);
	let mut rgba = vec![0u8; w * h * 4];
	for y in 4..h {
		for x in 0..w {
			rgba[(y * w + x) * 4..][..4].copy_from_slice(&[200, 40, 0, 255]);
		}
	}
	let edges = glowing_edges::compute(&rgba, w, h);
	for y in 0..h {
		for x in 0..w {
			let p = &edges[(y * w + x) * 4..][..4];
			let lit = (1..w - 1).contains(&x) && (y == 3 || y == 4);
			let expected = if lit { [200, 40, 0, 255] } else { [0, 0, 0, 255] };
			assert_eq!(p, expected, "({x}, {y})");
		}
	}
}
