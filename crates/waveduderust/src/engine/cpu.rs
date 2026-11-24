//! The CPU wave engine on a padded f32 grid, rows in parallel (docs/spec/30-engine.md, deviations in
//! docs/spec/05-deviations.md; docs/physics.md for the scheme).
//!
//! Buffers are padded by [`PAD`] rings: `stride = grid width + 2 PAD`, grid cell (gx, gy) at index
//! `(gy + PAD) * stride + gx + PAD`, canvas pixel (x, y) at grid cell (x + rim, y + rim). The rings stay 0 (the
//! wall) unless the edges are open without a rim, where Mur's first-order condition sets them in `u` (`w` stays 0,
//! docs/physics.md, Open edges). A mirror cell has c2 = 0 and stays 0; taps across a mirror are dropped (pml.rs,
//! `cuts`).
//!
//! Stencils are weighted sums of differences from the centre, `K sum (u_k - u)`: each difference is exact in f32
//! for a smooth field (Sterbenz), so round-off scales with the result and the weights sum to 0 exactly. Summing
//! the taps first biases the 9-point's energy (docs/physics.md, Energy).
//!
//! The 9-point sub-step is one leapfrog pass: `next = u + ((u - prev) d + c2 L u)`. The 4th-order (DRP) stencil is
//! two passes, `w = c2 L u`, then `next = u + ((u - prev) d + (w + c2 L w / 12))` (stencil.rs); `next` is written
//! over `prev`. With a rim, the CPML corrects the rim after each pass (pml.rs).
//!
//! Threads: one driver thread; each pass fans out over rows on the rayon pool, reading only its input field.

use std::ops::Range;
use std::sync::Arc;

use rayon::prelude::*;

use super::capture;
use super::forcing::{IMAGE_LEVELS, SourceGroups, live};
use super::mur::{MurChain, mur_chains};
use super::pml::{self, CpuPml, cut, taps};
use super::shared::{Shared, blend};
use super::stencil::{INV12, K9_AXIS, K9_DIAGONAL, PAD, StencilCoefficients, TimeScheme, centred};
use super::substeps::sub_steps;
use super::types::{
	Emitter, EngineOptions, FrameShape, PendingCapture, PendingProbes, ProbeCells, ProbeQueue, ProbeReadings,
	SceneData, Snapshot,
};
use super::{EngineError, WaveEngine};
use crate::model::Layer;

/// The reference engine. See the module docs for the grid layout and the scheme.
pub struct CpuEngine {
	shared: Shared,
	stride: usize,
	/// The axis weights a0..a3 of the stencil.
	axis: [f32; 4],
	/// c2 / n^2 per padded cell.
	c2: Vec<f32>,
	/// d^(1/n) per padded cell.
	d: Vec<f32>,
	/// Cut bytes per padded cell (pml.rs, `cuts`); all 0 without mirrors.
	cuts: Vec<u8>,
	mirrors: bool,
	u: Vec<f32>,
	prev: Vec<f32>,
	/// The modified equation's `c2 L u`, 0 on the padding.
	w: Vec<f32>,
	/// The intensity's EMA stages; empty unless the intensity is tracked.
	s1: Vec<f32>,
	s2: Vec<f32>,
	mur: Vec<MurChain>,
	pml: Option<CpuPml>,
	groups: SourceGroups,
	/// The source table of the current sub-step (forcing.rs).
	values: Vec<f32>,
	alpha: f32,
	/// Per probe its layer and padded cells.
	probes: Vec<(Layer, Vec<usize>)>,
	probe_queue: ProbeQueue,
	probe_reading: ProbeReadings,
}

/// The bytes [`CpuEngine`] allocates by the grid of `scene` with `options`: the field, medium and stage planes, the
/// mirror cuts and the CPML's auxiliary fields.
pub fn grid_bytes(scene: &SceneData, options: &EngineOptions) -> u64 {
	let cells = scene.padded_cells() as u64;
	let modified = TimeScheme::of(options.stencil) == TimeScheme::ModifiedEquation;
	let planes = 4 + u64::from(modified) + 2 * u64::from(options.track_intensity);
	let strips = pml::Strips::of(scene);
	let (psi, xi) = (strips.len() as u64, (strips.len() + strips.y_len()) as u64);
	cells * (4 * planes + 1) + 2 * 4 * (psi + xi)
}

impl CpuEngine {
	/// An engine at rest on `scene` with the sub-steps its CFL bound requires.
	pub fn new(scene: impl Into<Arc<SceneData>>, options: EngineOptions) -> Self {
		let scene = scene.into();
		let n = sub_steps(options.stencil, scene.max_c2());
		Self::with_sub_steps(scene, options, n)
	}

	/// An engine taking `n` sub-steps per step whatever the scene; below the CFL bound it diverges.
	/// For tests of the bound.
	pub fn with_sub_steps(scene: impl Into<Arc<SceneData>>, options: EngineOptions, n: u32) -> Self {
		assert!(n >= 1, "at least one sub-step");
		let scene = scene.into();
		let stride = scene.stride();
		let cells = scene.padded_cells();
		let scheme = TimeScheme::of(options.stencil);
		let stages = if options.track_intensity { cells } else { 0 };
		let mut engine = Self {
			stride,
			axis: StencilCoefficients::of(options.stencil).axis.map(|a| a as f32),
			c2: vec![0.0; cells],
			d: vec![1.0; cells],
			cuts: Vec::new(),
			mirrors: false,
			u: vec![0.0; cells],
			prev: vec![0.0; cells],
			w: if scheme == TimeScheme::ModifiedEquation { vec![0.0; cells] } else { Vec::new() },
			s1: vec![0.0; stages],
			s2: vec![0.0; stages],
			mur: Vec::new(),
			pml: None,
			groups: SourceGroups::default(),
			values: vec![0.0; IMAGE_LEVELS],
			alpha: 0.0,
			probes: Vec::new(),
			probe_queue: ProbeQueue::default(),
			probe_reading: ProbeReadings::default(),
			shared: Shared::new(scene, options, n),
		};
		engine.load_scene();
		engine.update_alpha();
		engine
	}

	/// Sets the field now and one sub-step before, each grid-sized (no padding), row-major.
	/// For analytic tests that start from a given state.
	pub fn set_field(&mut self, now: &[f32], before: &[f32]) {
		let scene = &self.shared.scene;
		let gw = scene.grid_width() as usize;
		assert_eq!(now.len(), gw * scene.grid_height() as usize);
		assert_eq!(before.len(), now.len());
		for (gy, (a, b)) in now.chunks_exact(gw).zip(before.chunks_exact(gw)).enumerate() {
			let at = scene.padded_index(0, gy);
			self.u[at..at + gw].copy_from_slice(a);
			self.prev[at..at + gw].copy_from_slice(b);
		}
	}

	/// The padded field buffer (see [`SceneData::stride`]).
	pub fn field_padded(&self) -> &[f32] {
		&self.u
	}

	// The inherent calls below take `&mut self` like the trait's, so they win method resolution on a
	// `CpuEngine`: callers on the concrete type get no `Result`.

	/// [`WaveEngine::advance_one_step`], which cannot fail here.
	pub fn advance_one_step(&mut self) {
		for k in 0..self.shared.sub_steps {
			self.shared.source_values(&mut self.values, k);
			self.sub_step(self.shared.accumulates(k));
		}
		self.shared.time += 1;
	}

	/// [`WaveEngine::sample_probes`], which cannot fail here.
	pub fn sample_probes(&mut self, out: &mut ProbeReadings) {
		out.clear();
		for (layer, cells) in &self.probes {
			match layer {
				Layer::Field => out.push(cells.iter().map(|&c| self.u[c])),
				Layer::Intensity => out.push(cells.iter().map(|&c| self.blended(c))),
			}
		}
	}

	/// Converts the canvas after the last step into `out`'s planes in `shape`, their allocations reused.
	pub fn capture_into(&self, shape: &FrameShape, out: &mut Snapshot) {
		let (scene, options) = (&self.shared.scene, &self.shared.options);
		assert_eq!((shape.canvas_width, shape.canvas_height), (scene.canvas_width, scene.canvas_height));
		capture::plane_rows(shape, Layer::Field, &mut out.field, |y, row| {
			let at = scene.canvas_index(0, y);
			row.copy_from_slice(&self.u[at..at + row.len()]);
		});
		if options.track_intensity {
			let scale = options.intensity_scale as f32;
			capture::plane_rows(shape, Layer::Intensity, &mut out.intensity, |y, row| {
				let at = scene.canvas_index(0, y);
				let n = row.len();
				for ((r, &a), &b) in row.iter_mut().zip(&self.s1[at..at + n]).zip(&self.s2[at..at + n]) {
					*r = blend(a, b, scale);
				}
			});
		} else {
			out.intensity.clear();
		}
		out.shape = *shape;
		out.step = self.shared.time.saturating_sub(1);
	}

	/// [`WaveEngine::capture_frame`], which cannot fail here.
	pub fn capture_frame(&mut self, shape: &FrameShape) -> Snapshot {
		let mut out = Snapshot::new(*shape);
		self.capture_into(shape, &mut out);
		out
	}

	/// [`WaveEngine::prepare_frame`], which cannot fail here.
	pub fn prepare_frame(&mut self) -> (Vec<f32>, Vec<f32>) {
		let stages = (!self.s1.is_empty()).then_some((&self.s1[..], &self.s2[..]));
		self.shared.canvas_frame(&self.u, stages)
	}

	/// Copies the scene's maps into the padded buffers at the sub-step scale and builds the cuts, the Mur
	/// chains, the PML and the source groups.
	fn load_scene(&mut self) {
		let (scene, n) = (&self.shared.scene, self.shared.sub_steps);
		scene.fill_sub_step_medium(n, &mut self.c2, &mut self.d);
		self.cuts = pml::cuts(scene);
		self.mirrors = self.cuts.iter().any(|&c| c != 0);
		self.mur = mur_chains(scene, &self.c2, &self.d);
		let cross = StencilCoefficients::of(self.shared.options.stencil).cross as f32;
		self.pml = CpuPml::new(scene, n, self.axis, cross);
		self.group_sources();
	}

	fn update_alpha(&mut self) {
		self.alpha = self.shared.alpha();
	}

	fn group_sources(&mut self) {
		self.groups = self.shared.groups();
		self.values.resize(self.shared.sources.table_len(), 0.0);
	}

	/// Adds the sub-step's forcing `F` to `w` at the driven cells, where the modified equation takes
	/// `A u` (forcing.rs).
	fn force_w(&mut self) {
		for (cell, run) in self.groups.iter() {
			let mut v = self.w[cell];
			for &e in run {
				let f = self.values[e as usize];
				if live(f) {
					v += f;
				}
			}
			self.w[cell] = v;
		}
	}

	/// Adds `h F` to the next field at the driven cells, where leapfrog takes `h A u`, and moves the
	/// intensity stages of the last sub-step to the forced value.
	fn force_next(&mut self, intensity: bool) {
		let rest = 1.0 - self.alpha;
		for (cell, run) in self.groups.iter() {
			let (before, h) = (self.prev[cell], centred(self.d[cell]));
			let mut v = before;
			let mut driven = false;
			for &e in run {
				let f = self.values[e as usize];
				if live(f) {
					v += h * f;
					driven = true;
				}
			}
			if !driven {
				continue;
			}
			self.prev[cell] = v;
			if intensity {
				let s1 = self.s1[cell] + (v * v - before * before) * rest;
				self.s2[cell] += (s1 - self.s1[cell]) * rest;
				self.s1[cell] = s1;
			}
		}
	}

	/// The intensity stages and their weight when `intensity` (and tracked), for [`grid_rows`].
	fn stages<'a>(s1: &'a mut [f32], s2: &'a mut [f32], alpha: f32, intensity: bool) -> Option<Stages<'a>> {
		(intensity && !s1.is_empty()).then_some(Stages { s1, s2, alpha })
	}

	/// The leapfrog pass of the 9-point stencil: writes the next field over `prev`, and the intensity
	/// stages when `intensity`.
	fn leapfrog_pass(&mut self, intensity: bool) {
		let (s, gw) = (self.stride, self.shared.scene.grid_width() as usize);
		let (u, c2, d, cuts, mirrors) = (&self.u, &self.c2, &self.d, &self.cuts, self.mirrors);
		let stages = Self::stages(&mut self.s1, &mut self.s2, self.alpha, intensity);
		grid_rows(&mut self.prev, stages, s, gw, |r, next| {
			let at = r * s + PAD;
			let (c2, d) = (&c2[at..][..gw], &d[at..][..gw]);
			let update = |x: usize, next: &mut f32, v: f32, lap: f32| {
				*next = v + ((v - *next) * d[x] + lap * c2[x] * centred(d[x]));
			};
			// The cells of `span` with no cut, on row slices, as `nine_point` and `update` in the same order. Every
			// slice has the span's length, so the loop has no bounds checks and vectorises.
			let uncut = |span: Range<usize>, next: &mut [f32]| {
				let len = span.len();
				let tap = |k: isize| &u[(at + span.start).wrapping_add_signed(k)..][..len];
				let (si, one) = (s as isize, 1isize);
				let (c, w, e, n, so) = (tap(0), tap(-one), tap(one), tap(-si), tap(si));
				let (nw, ne, sw, se) = (tap(-si - 1), tap(-si + 1), tap(si - 1), tap(si + 1));
				let (c2, d, next) = (&c2[span.clone()], &d[span.clone()], &mut next[span]);
				for x in 0..len {
					let v = c[x];
					let diagonals = ((nw[x] - v) + (ne[x] - v)) + ((sw[x] - v) + (se[x] - v));
					let axes = ((n[x] - v) + (so[x] - v)) + ((w[x] - v) + (e[x] - v));
					let lap = K9_DIAGONAL * diagonals + K9_AXIS * axes;
					next[x] = v + ((v - next[x]) * d[x] + lap * c2[x] * centred(d[x]));
				}
			};
			match row_cuts(cuts, mirrors, at, gw) {
				None => uncut(0..gw, next),
				Some(cuts) => cut_spans(cuts, |span, cut| {
					if !cut {
						return uncut(span, next);
					}
					for x in span {
						update(x, &mut next[x], u[at + x], nine_point(u, at + x, s, cuts[x]));
					}
				}),
			}
		});
	}

	/// The modified equation's first pass: `w = c2 L u` on every grid cell.
	fn inner_pass(&mut self) {
		let (s, gw) = (self.stride, self.shared.scene.grid_width() as usize);
		let (u, c2, cuts, axis, mirrors) = (&self.u, &self.c2, &self.cuts, &self.axis, self.mirrors);
		grid_rows(&mut self.w, None, s, gw, |r, w| {
			let at = r * s + PAD;
			let c2 = &c2[at..][..gw];
			drp_row(u, at, s, gw, axis, row_cuts(cuts, mirrors, at, gw), |x, lap| w[x] = c2[x] * lap);
		});
	}

	/// The modified equation's second pass: writes the next field over `prev`, and the intensity
	/// stages when `intensity`.
	fn outer_pass(&mut self, intensity: bool) {
		let (s, gw) = (self.stride, self.shared.scene.grid_width() as usize);
		let (u, w, c2, d, cuts, axis, mirrors) =
			(&self.u, &self.w, &self.c2, &self.d, &self.cuts, &self.axis, self.mirrors);
		let stages = Self::stages(&mut self.s1, &mut self.s2, self.alpha, intensity);
		grid_rows(&mut self.prev, stages, s, gw, |r, next| {
			let at = r * s + PAD;
			let (uc, wc, c2, d) = (&u[at..][..gw], &w[at..][..gw], &c2[at..][..gw], &d[at..][..gw]);
			drp_row(w, at, s, gw, axis, row_cuts(cuts, mirrors, at, gw), |x, lw| {
				let c = uc[x];
				next[x] = c + ((c - next[x]) * d[x] + (wc[x] + c2[x] * lw * INV12) * centred(d[x]));
			});
		});
	}

	/// One sub-step; the next field ends in `u`.
	fn sub_step(&mut self, intensity: bool) {
		match self.shared.scheme {
			TimeScheme::Leapfrog => {
				self.leapfrog_pass(intensity);
				if let Some(p) = &mut self.pml {
					p.apply(0, &self.u, &self.cuts, &self.c2, &mut self.prev, 1.0);
				}
				self.force_next(intensity);
			}
			TimeScheme::ModifiedEquation => {
				self.inner_pass();
				if let Some(p) = &mut self.pml {
					p.apply(0, &self.u, &self.cuts, &self.c2, &mut self.w, 1.0);
				}
				self.force_w();
				self.outer_pass(intensity);
				if let Some(p) = &mut self.pml {
					p.apply(1, &self.w, &self.cuts, &self.c2, &mut self.prev, INV12);
				}
			}
		}
		self.apply_open_edges();
		std::mem::swap(&mut self.u, &mut self.prev);
	}

	/// Mur's condition on the first ring (mur.rs, `mur_weights`); the second and third rings, which only the DRP stencil
	/// reads, continue the field linearly from the edge cell through the first ring. Chaining Mur ring by ring grows a
	/// mode where two open edges meet at Courant numbers above about 0.75 (docs/physics.md, Open edges).
	fn apply_open_edges(&mut self) {
		for m in &self.mur {
			let [a, b, c] = m.weights;
			let [r1, r2, r3] = m.rings;
			let (edge, first) = (self.prev[m.inner], a * self.u[m.inner] + b * self.prev[m.inner] + c * self.u[r1]);
			self.prev[r1] = first;
			self.prev[r2] = 2.0 * first - edge;
			self.prev[r3] = 3.0 * first - 2.0 * edge;
		}
	}

	/// The intensity at padded cell `cell`, 0 when it is not tracked.
	fn blended(&self, cell: usize) -> f32 {
		if self.s1.is_empty() {
			return 0.0;
		}
		blend(self.s1[cell], self.s2[cell], self.shared.options.intensity_scale as f32)
	}
}

/// The intensity stages a pass advances, and the EMA weight.
struct Stages<'a> {
	s1: &'a mut [f32],
	s2: &'a mut [f32],
	alpha: f32,
}

/// Runs `row(r, cells)` on the `gw` grid cells of every grid row `r` of `out` (padded rows of `s`) in
/// parallel, then advances the intensity stages of those cells by the values written, if given.
fn grid_rows(out: &mut [f32], stages: Option<Stages<'_>>, s: usize, gw: usize, row: impl Fn(usize, &mut [f32]) + Sync) {
	let rows = out.len() / s;
	let grid = |r: usize| (PAD..rows - PAD).contains(&r);
	match stages {
		Some(Stages { s1, s2, alpha }) => out
			.par_chunks_mut(s)
			.zip(s1.par_chunks_mut(s))
			.zip(s2.par_chunks_mut(s))
			.enumerate()
			.filter(|(r, _)| grid(*r))
			.for_each(|(r, ((line, s1), s2))| {
				let cells = &mut line[PAD..][..gw];
				row(r, cells);
				accumulate(&mut s1[PAD..][..gw], &mut s2[PAD..][..gw], cells, alpha);
			}),
		None => out
			.par_chunks_mut(s)
			.enumerate()
			.filter(|(r, _)| grid(*r))
			.for_each(|(r, line)| row(r, &mut line[PAD..][..gw])),
	}
}

/// The 9-point Laplacian at padded cell `i` (row length `s`) with the cut byte `cut`: a diagonal tap
/// is dropped when either axis neighbour it passes is a mirror, so no wave crosses a mirror line
/// that is only 8-connected; the rule is symmetric between the two cells of a tap. Same order of
/// operations as the uncut rows.
#[inline]
fn nine_point(u: &[f32], i: usize, s: usize, cut: u8) -> f32 {
	let v = u[i];
	let [nw, ne, sw, se] = pml::diagonal_cuts(cut);
	let tap = |k: usize, cut: bool| if cut { 0.0 } else { u[k] };
	let diagonals =
		((tap(i - s - 1, nw) - v) + (tap(i - s + 1, ne) - v)) + ((tap(i + s - 1, sw) - v) + (tap(i + s + 1, se) - v));
	let axes = ((u[i - s] - v) + (u[i + s] - v)) + ((u[i - 1] - v) + (u[i + 1] - v));
	K9_DIAGONAL * diagonals + K9_AXIS * axes
}

/// The DRP Laplacian at padded cell `i` (row length `s`), the taps beyond a mirror dropped (`cut`):
/// `sum over m of a_m sum over ring m of (u_k - u)`, the stencil since `a0 = -2 (a1 + a2 + a3)`.
#[inline]
fn drp(u: &[f32], i: usize, s: usize, a: &[f32; 4], cut: u8) -> f32 {
	let c = u[i];
	let (x, y) = (taps(u, i, 1, cut >> cut::X_SHIFT), taps(u, i, s, cut >> cut::Y_SHIFT));
	let ring = |m: usize| ((x[3 - m] - c) + (x[3 + m] - c)) + ((y[3 - m] - c) + (y[3 + m] - c));
	a[1] * ring(1) + a[2] * ring(2) + a[3] * ring(3)
}

/// The cut bytes of the `gw` grid cells from padded cell `at`, or None when the row has no mirror.
#[inline]
fn row_cuts(cuts: &[u8], mirrors: bool, at: usize, gw: usize) -> Option<&[u8]> {
	if !mirrors {
		return None;
	}
	let row = &cuts[at..][..gw];
	row.iter().any(|&c| c != 0).then_some(row)
}

/// Hands `f` the maximal runs of `cuts` in order, each with whether its cells drop a tap (a non-zero cut byte).
/// A row crossed by a mirror has only a few cut cells near it; the runs between take the row-slice loops.
#[inline(always)]
fn cut_spans(cuts: &[u8], mut f: impl FnMut(Range<usize>, bool)) {
	let mut start = 0;
	while start < cuts.len() {
		let cut = cuts[start] != 0;
		let len = cuts[start..].iter().position(|&c| (c != 0) != cut).unwrap_or(cuts.len() - start);
		f(start..start + len, cut);
		start += len;
	}
}

/// The DRP Laplacian of the `gw` grid cells from padded cell `at`, handed to `f` with the cell's
/// offset in the row. Cells without a cut run on row slices ([`drp_span`]), the cut ones through [`drp`].
#[inline(always)]
fn drp_row(
	u: &[f32],
	at: usize,
	s: usize,
	gw: usize,
	a: &[f32; 4],
	cuts: Option<&[u8]>,
	mut f: impl FnMut(usize, f32),
) {
	let Some(cuts) = cuts else { return drp_span(u, at, s, 0..gw, a, &mut f) };
	cut_spans(cuts, |span, cut| {
		if !cut {
			return drp_span(u, at, s, span, a, &mut f);
		}
		for x in span {
			f(x, drp(u, at + x, s, a, cuts[x]));
		}
	});
}

/// The DRP Laplacian of the cells `span` of the row from padded cell `at`, none of them cut, on row slices, as
/// [`drp`] in the same order.
#[inline(always)]
fn drp_span(u: &[f32], at: usize, s: usize, span: Range<usize>, a: &[f32; 4], f: &mut impl FnMut(usize, f32)) {
	let x0 = span.start;
	let tap = |k: isize| &u[(at + x0).wrapping_add_signed(k)..][..span.len()];
	let si = s as isize;
	let c = tap(0);
	let (w1, e1, n1, s1) = (tap(-1), tap(1), tap(-si), tap(si));
	let (w2, e2, n2, s2) = (tap(-2), tap(2), tap(-2 * si), tap(2 * si));
	let (w3, e3, n3, s3) = (tap(-3), tap(3), tap(-3 * si), tap(3 * si));
	for x in 0..span.len() {
		let v = c[x];
		let near = ((w1[x] - v) + (e1[x] - v)) + ((n1[x] - v) + (s1[x] - v));
		let mid = ((w2[x] - v) + (e2[x] - v)) + ((n2[x] - v) + (s2[x] - v));
		let far = ((w3[x] - v) + (e3[x] - v)) + ((n3[x] - v) + (s3[x] - v));
		f(x0 + x, a[1] * near + a[2] * mid + a[3] * far);
	}
}

/// Advances the intensity's two EMA stages of one row by the field `v`.
#[inline]
fn accumulate(s1: &mut [f32], s2: &mut [f32], v: &[f32], alpha: f32) {
	let rest = 1.0 - alpha;
	for ((a, b), &v) in s1.iter_mut().zip(s2).zip(v) {
		*a = *a * alpha + v * v * rest;
		*b = *b * alpha + *a * rest;
	}
}

impl WaveEngine for CpuEngine {
	fn name(&self) -> &'static str {
		"CPU"
	}

	fn scene(&self) -> &SceneData {
		&self.shared.scene
	}

	fn options(&self) -> &EngineOptions {
		&self.shared.options
	}

	fn time(&self) -> u32 {
		self.shared.time
	}

	fn sub_steps(&self) -> u32 {
		self.shared.sub_steps
	}

	fn reset(&mut self) {
		for b in [&mut self.u, &mut self.prev, &mut self.w, &mut self.s1, &mut self.s2] {
			b.fill(0.0);
		}
		if let Some(p) = &mut self.pml {
			p.reset();
		}
		self.shared.reset();
		self.probe_queue.clear();
	}

	fn set_track_intensity(&mut self, on: bool) {
		if self.shared.options.track_intensity != on {
			self.shared.options.track_intensity = on;
			let stages = if on { self.u.len() } else { 0 };
			self.s1 = vec![0.0; stages];
			self.s2 = vec![0.0; stages];
		}
	}

	fn set_emitters(&mut self, emitters: &[Emitter]) {
		self.shared.set_emitters(emitters);
		self.group_sources();
		self.update_alpha();
	}

	fn set_probes(&mut self, probes: &[ProbeCells]) {
		let scene = &self.shared.scene;
		self.probes = probes
			.iter()
			.map(|p| (p.layer, p.cells.iter().map(|&(x, y)| scene.canvas_index(x as usize, y as usize)).collect()))
			.collect();
		self.probe_queue.set_lengths(probes.iter().map(|p| p.cells.len()).collect());
	}

	fn try_replace_scene(&mut self, scene: &Arc<SceneData>) -> bool {
		let Some(n) = self.shared.replacement(scene) else { return false };
		self.shared.replace_scene(scene.clone(), n);
		self.load_scene();
		self.update_alpha();
		true
	}

	fn advance_one_step(&mut self) -> Result<(), EngineError> {
		CpuEngine::advance_one_step(self);
		Ok(())
	}

	fn sample_probes(&mut self, out: &mut ProbeReadings) -> Result<(), EngineError> {
		CpuEngine::sample_probes(self, out);
		Ok(())
	}

	fn queue_probes(&mut self) -> Result<(), EngineError> {
		let mut reading = std::mem::take(&mut self.probe_reading);
		CpuEngine::sample_probes(self, &mut reading);
		self.probe_queue.push(reading.values());
		self.probe_reading = reading;
		Ok(())
	}

	fn pending_probes(&mut self) -> Result<PendingProbes, EngineError> {
		Ok(PendingProbes::done(self.probe_queue.end()))
	}

	fn drain_probes(&mut self, pending: PendingProbes, f: &mut dyn FnMut(&ProbeReadings)) -> Result<(), EngineError> {
		self.probe_queue.drain(pending.end(), f);
		Ok(())
	}

	fn begin_capture(&mut self, shape: &FrameShape, mut into: Snapshot) -> Result<PendingCapture, EngineError> {
		self.capture_into(shape, &mut into);
		Ok(PendingCapture::done(into))
	}

	fn finish_capture(&mut self, pending: PendingCapture) -> Result<Snapshot, EngineError> {
		Ok(pending.snapshot)
	}

	fn prepare_frame(&mut self) -> Result<(Vec<f32>, Vec<f32>), EngineError> {
		Ok(CpuEngine::prepare_frame(self))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Edges, PulseShape, Scene, Stencil};

	fn options(stencil: Stencil) -> EngineOptions {
		EngineOptions {
			stencil,
			source_frequency_scale: 2.0,
			image_source_scale: 1.0,
			time_decay: 10.0,
			intensity_scale: 1.0,
			track_intensity: true,
		}
	}

	fn emitter(x: i32, y: i32) -> Emitter {
		Emitter {
			x: f64::from(x),
			y: f64::from(y),
			level: 100.0,
			amplitude: 1.0,
			phase_radians: 0.0,
			pulsed: false,
			shape: PulseShape::Gaussian,
			on_width_cycles: 10,
			off_width_cycles: 10,
			off_width_base: 10,
			randomize_off_width: 0,
			detach_after_step: None,
			rng_seed: std::num::NonZeroU32::MIN,
		}
	}

	fn engine(edges: Edges, rim: i32, stencil: Stencil) -> CpuEngine {
		let scene = SceneData::blank(40, 30, &Scene { damping_edge: rim, edges, ..Scene::default() });
		let mut e = CpuEngine::new(scene, options(stencil));
		e.set_emitters(&[emitter(20, 15), emitter(-1, 3), emitter(40, 0)]);
		e
	}

	#[test]
	fn a_cpu_capture_is_ready_at_once() {
		use std::future::Future;
		use std::task::{Context, Poll, Waker};
		let mut e = engine(Edges::Open, 0, Stencil::NinePoint);
		e.advance_one_step();
		let shape = FrameShape {
			canvas_width: 40,
			canvas_height: 30,
			divisor: 1,
			format: crate::engine::SampleFormat::Half,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: false,
		};
		let pending = e.begin_capture(&shape, Snapshot::new(shape)).unwrap();
		let mut ready = std::pin::pin!(pending.ready());
		assert_eq!(ready.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Ready(()));
		assert_eq!(e.finish_capture(pending).unwrap().shape, shape);
	}

	#[test]
	fn outside_emitters_are_dropped() {
		assert_eq!(engine(Edges::Open, 0, Stencil::NinePoint).shared.sources.emitters.len(), 1);
	}

	#[test]
	fn reflective_ring_stays_zero_and_intensity_accumulates() {
		let mut e = engine(Edges::Reflective, 0, Stencil::FourthOrder);
		for _ in 0..80 {
			e.advance_one_step();
		}
		let (s, rows) = (e.stride, e.u.len() / e.stride);
		let padding = (0..rows * s).filter(|&i| {
			let (x, y) = (i % s, i / s);
			x < PAD || y < PAD || x >= s - PAD || y >= rows - PAD
		});
		assert!(padding.into_iter().all(|i| e.u[i] == 0.0 && e.prev[i] == 0.0 && e.w[i] == 0.0));
		let (field, intensity) = e.prepare_frame();
		assert!(field.iter().any(|&v| v != 0.0));
		assert!(intensity.iter().all(|&v| v >= 0.0) && intensity.iter().any(|&v| v > 0.0));
		assert_eq!(e.time(), 80);
	}

	#[test]
	fn reset_and_replay_are_identical() {
		let mut e = engine(Edges::Open, -8, Stencil::NinePoint);
		let run = |e: &mut CpuEngine| {
			for _ in 0..50 {
				e.advance_one_step();
			}
			e.prepare_frame()
		};
		let a = run(&mut e);
		e.reset();
		assert_eq!(a, run(&mut e));
	}

	#[test]
	fn probes_read_the_field_and_the_intensity() {
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		e.set_probes(&[
			ProbeCells { layer: Layer::Field, cells: vec![(22, 15)] },
			ProbeCells { layer: Layer::Intensity, cells: vec![(22, 15), (22, 15)] },
		]);
		for _ in 0..30 {
			e.advance_one_step();
		}
		let mut r = ProbeReadings::default();
		e.sample_probes(&mut r);
		let (field, intensity) = e.prepare_frame();
		assert_eq!(r.probe(0), [field[15 * 40 + 22]]);
		assert_eq!(r.probe(1), [intensity[15 * 40 + 22]; 2]);
	}

	#[test]
	fn pending_probes_are_ready_at_once_and_drain_in_step_order() {
		use std::future::Future;
		use std::task::{Context, Poll, Waker};
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		e.set_probes(&[
			ProbeCells { layer: Layer::Field, cells: vec![(22, 15)] },
			ProbeCells { layer: Layer::Intensity, cells: vec![(10, 5), (11, 5), (12, 5)] },
		]);
		let (mut sampled, mut drained, mut r) = (Vec::new(), Vec::new(), ProbeReadings::default());
		let mut held = None;
		for step in 1..=40 {
			e.advance_one_step();
			e.sample_probes(&mut r);
			sampled.push(r.clone());
			e.queue_probes().unwrap();
			if step % 15 == 0 || step == 40 {
				if let Some((pending, covered)) = held.take() {
					e.drain_probes(pending, &mut |r| drained.push(r.clone())).unwrap();
					assert_eq!(drained.len(), covered, "the readings queued after the token stay");
				}
				let pending = e.pending_probes().unwrap();
				let mut ready = std::pin::pin!(pending.ready());
				assert_eq!(ready.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Ready(()));
				held = Some((pending, step));
			}
		}
		let (pending, _) = held.unwrap();
		e.drain_probes(pending, &mut |r| drained.push(r.clone())).unwrap();
		assert_eq!(drained, sampled);
	}

	#[test]
	fn a_token_taken_before_a_reset_drains_nothing_after_it() {
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		e.set_probes(&[ProbeCells { layer: Layer::Field, cells: vec![(22, 15)] }]);
		for _ in 0..10 {
			e.advance_one_step();
			e.queue_probes().unwrap();
		}
		let stale = e.pending_probes().unwrap();
		e.reset();
		for _ in 0..5 {
			e.advance_one_step();
			e.queue_probes().unwrap();
		}
		e.drain_probes(stale, &mut |_| panic!("a reading from before the reset")).unwrap();
		let (pending, mut drained) = (e.pending_probes().unwrap(), 0);
		e.drain_probes(pending, &mut |_| drained += 1).unwrap();
		assert_eq!(drained, 5);
	}

	#[test]
	fn capture_holds_the_canvas() {
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		for _ in 0..30 {
			e.advance_one_step();
		}
		let shape = FrameShape {
			canvas_width: 40,
			canvas_height: 30,
			divisor: 1,
			format: crate::engine::SampleFormat::Half,
			field_brightness: 1.0,
			intensity_brightness: 1.0,
			intensity_root: true,
		};
		let snap = e.capture_frame(&shape);
		let (field, intensity) = e.prepare_frame();
		assert_eq!(snap.step, 29);
		let h = snap.halves(Layer::Field);
		assert!(h.iter().zip(&field).all(|(a, &b)| *a == half::f16::from_f32(b)));
		let i = snap.halves(Layer::Intensity);
		assert!(i.iter().zip(&intensity).all(|(a, &b)| *a == half::f16::from_f32(b.sqrt())));
	}

	#[test]
	fn replace_refuses_another_grid() {
		let mut e = engine(Edges::Open, -4, Stencil::FourthOrder);
		let other = SceneData::blank(41, 30, &Scene { damping_edge: -4, ..Scene::default() });
		assert!(!e.try_replace_scene(&Arc::new(other)));
		let reflective =
			SceneData::blank(40, 30, &Scene { damping_edge: -4, edges: Edges::Reflective, ..Scene::default() });
		assert!(!e.try_replace_scene(&Arc::new(reflective)));
		let slower = SceneData::blank(40, 30, &Scene { damping_edge: -4, refractive_index: 2.0, ..Scene::default() });
		assert!(e.try_replace_scene(&Arc::new(slower)));
		assert_eq!(e.sub_steps(), 1);
	}

	#[test]
	fn alpha_follows_the_lowest_level() {
		let mut e = engine(Edges::Open, 0, Stencil::NinePoint);
		assert_eq!(e.alpha, (-1.0f64 / 10.0).exp() as f32);
		let mut low = emitter(1, 1);
		low.level = 50.0;
		e.set_emitters(&[low]);
		assert_eq!(e.alpha, (-1.0f64 / 20.0).exp() as f32);
		e.set_emitters(&[]);
		assert_eq!(e.alpha, (-1.0f64 / 10.0).exp() as f32);
		e.shared.sources.level_used[0] = true;
		e.update_alpha();
		assert_eq!(e.alpha, (-1.0f64 / 10.0).exp() as f32, "level 0 is not a source level");
	}

	#[test]
	fn cut_spans_cover_the_row_in_maximal_runs() {
		let mut runs = Vec::new();
		cut_spans(&[0, 0, 3, 1, 0, 0, 0, 8], |span, cut| runs.push((span, cut)));
		assert_eq!(runs, [(0..2, false), (2..4, true), (4..7, false), (7..8, true)]);
		runs.clear();
		cut_spans(&[0; 5], |span, cut| runs.push((span, cut)));
		assert_eq!(runs, [(0..5, false)]);
	}

	#[test]
	fn a_row_with_cuts_matches_drp_per_cell() {
		let (s, gw) = (20, 14);
		let u: Vec<f32> = (0..s * 10).map(|i| ((i * 7919 % 101) as f32 - 50.0) * 0.013).collect();
		let at = 4 * s + PAD;
		let mut cuts = vec![0u8; gw];
		(cuts[3], cuts[4], cuts[10]) = (0b0001_0010, 0b1000_0000, 0b0000_0101);
		let a = [0.0, 1.3, -0.17, 0.021];
		let mut got = vec![f32::NAN; gw];
		drp_row(&u, at, s, gw, &a, Some(&cuts), |x, v| got[x] = v);
		let want: Vec<f32> = (0..gw).map(|x| drp(&u, at + x, s, &a, cuts[x])).collect();
		assert_eq!(
			got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
			want.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
		);
	}
}
