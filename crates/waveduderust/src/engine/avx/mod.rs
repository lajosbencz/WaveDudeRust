//! The CPU wave engine for x86-64 machines with AVX2 and FMA: the scheme, the grid and the features of the reference
//! engine (engine/cpu.rs), its hot loops written as row kernels (kernels.rs) that take 8 lanes at a time.
//!
//! The grid is the CPU engine's: padded by [`PAD`] rings, `stride = grid width + 2 PAD`, the rings 0 unless Mur's
//! condition sets them in `u` on open edges without a rim, mirror cells at c2 = 0 with the taps across a mirror dropped
//! (pml.rs, `cuts`). The planes, the medium, the forcing, the probes, the captures, the damage rectangle, Mur's chains
//! and the CPML's state are the CPU engine's too (host.rs); this module owns the passes over the grid and how they
//! split across threads.
//!
//! Passes: the 9-point sub-step is one leapfrog pass, `next = u + ((u - prev) d + c2 L u)`; the 4th-order (DRP)
//! stencil two, `w = c2 L u` into the plane `w`, then `next = u + ((u - prev) d + (w + c2 L w / 12))` from it. `next`
//! is written over `prev`. Each pass runs on the damage grown by its reach, one grid row per task on the rayon pool;
//! a row's cells with no mirror cut go through a kernel in spans, the cut ones through the CPU engine's per-cell
//! stencils. On the last sub-step of a step the leapfrog and the outer kernels advance the intensity stages of the
//! cells they write in the same loop. With a rim, the CPML corrects each row right after the pass writes it (pml.rs,
//! `XShared::row`), after the stages, as in the CPU engine.
//!
//! The 9-point pass takes the CPU engine's uniform runs (engine/cpu.rs, Uniform medium): a run's cells go to the
//! kernel with its `c2` and `d` as constants, the rest with the planes' values. The 4th order runs its two passes over
//! the plane `w`; the CPU engine's fusion of the outer pass with the inner (a ring of 7 `w` rows per block of rows)
//! is not done here yet. The outer kernel reads its 7 rows of `w` as slices, so a ring can hand it its rows in place
//! of the plane's. Nor does it take a uniform medium's runs as constants: the kernels read the `c2` and `d` planes.
//!
//! Instruction set: the engine is built only on a CPU with AVX2 and FMA (its constructor asserts [`available`]), so
//! the Rust around the kernels (the CPML's rows, the cut cells) runs compiled for them through `wide`, with no
//! feature test on the way.
//!
//! Kernels: an engine picks its row kernels once, when built (kernels.rs, `RowKernels`). The reference kernels are
//! safe Rust in the CPU engine's expressions and order, and Rust contracts no multiply-add, so with them the results
//! are the CPU engine's bit for bit ([`AvxEngine::with_reference_kernels`]). The default kernels may round differently
//! (fused multiply-adds); the engines then agree within a tolerance (tests/avx.rs).

#![allow(unsafe_code)]

mod kernels;

use std::ops::Range;
use std::sync::Arc;

use rayon::iter::Either;
use rayon::prelude::*;

use self::kernels::{Intensity, RowKernels, RowMedium, advance};
use super::cpu::{Rows, UniformRuns, chunks_or_none, cut_spans, drp, medium_spans, nine_point, plane_rows, row_cuts};
use super::damage::{Damage, extent};
use super::host::{Host, Stages, host_engine};
use super::pml::{Medium, XPart, XShared};
use super::stencil::{INV12, PAD, TimeScheme, centred};
use super::substeps::sub_steps;
use super::types::{EngineOptions, SceneData};

/// Whether this CPU runs the engine: it has AVX2 and FMA.
pub fn available() -> bool {
	std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
}

/// The CPU engine on AVX2 and FMA. See the module docs; build it only where [`available`] holds.
pub struct AvxEngine {
	/// The planes, the medium, the forcing, the probes and the damage (host.rs).
	host: Host,
	kernels: RowKernels,
	/// The runs of each grid row where `c2` and `d` hold one value (engine/cpu.rs, Uniform medium).
	uniform: UniformRuns,
}

impl AvxEngine {
	/// An engine at rest on `scene` with the sub-steps its CFL bound requires.
	pub fn new(scene: impl Into<Arc<SceneData>>, options: EngineOptions) -> Self {
		let scene = scene.into();
		let n = sub_steps(options.stencil, scene.max_c2());
		Self::with_sub_steps(scene, options, n)
	}

	/// An engine taking `n` sub-steps per step whatever the scene; below the CFL bound it diverges.
	/// For tests of the bound.
	///
	/// # Panics
	///
	/// On a CPU without AVX2 and FMA ([`available`]).
	pub fn with_sub_steps(scene: impl Into<Arc<SceneData>>, options: EngineOptions, n: u32) -> Self {
		assert!(available(), "the AVX2 engine needs a CPU with AVX2 and FMA");
		let host = Host::new(scene.into(), options, n);
		let mut engine = Self { host, kernels: RowKernels::native(), uniform: UniformRuns::default() };
		engine.scene_loaded();
		engine
	}

	/// This engine on the reference kernels (module docs, Kernels), whose results are the CPU engine's bit for bit.
	/// For tests.
	pub fn with_reference_kernels(self) -> Self {
		Self { kernels: RowKernels::REFERENCE, ..self }
	}

	/// Rebuilds the uniform runs from the medium the host loaded.
	fn scene_loaded(&mut self) {
		let h = &self.host;
		self.uniform = UniformRuns::of(&h.c2, &h.d, h.stride, h.gw());
	}

	/// The leapfrog pass of the 9-point stencil: writes the next field over `prev`, and the intensity stages when
	/// `intensity`.
	fn leapfrog_pass(&mut self, intensity: bool) {
		let h = &mut self.host;
		let (s, gw, nine, uniform) = (h.stride, h.gw(), self.kernels.nine, &self.uniform);
		let (u, c2, d, cuts, mirrors) = (&h.u, &h.c2, &h.d, &h.cuts, h.mirrors);
		let (area, xs) = (&h.damage, h.damage.x.clone());
		let stages = Host::stages(&mut h.s1, &mut h.s2, h.alpha, intensity);
		let mut rim = h.pml.as_mut().and_then(|p| p.begin(0, u, Medium { cuts, c2 }, 1.0, area));
		grid_rows(&mut h.prev, stages, s, area, rim.as_mut(), |r, next, mut stages| {
			let (at, runs) = (r * s + PAD, uniform.row(r - PAD));
			let (c2, d) = (&c2[at..][..gw], &d[at..][..gw]);
			// Grid column x of the rows above, of and below is at x + 1 of these.
			let rows: [&[f32]; 3] = std::array::from_fn(|k| &u[at + k * s - s - 1..][..gw + 2]);
			// The cells of `span` with no cut, a uniform run's with its constants.
			let uncut = |span: Range<usize>, next: &mut [f32], stages: &mut Option<Intensity<'_>>| {
				medium_spans(runs, c2, d, span, |piece, medium| {
					let taps = rows.map(|row| &row[piece.start..piece.end + 2]);
					let medium = match medium {
						Some((c2, d)) => RowMedium::Uniform { c2, d },
						None => RowMedium::Planes { c2: &c2[piece.clone()], d: &d[piece.clone()] },
					};
					let stages = stages.as_mut().map(|st| st.span(piece.start, piece.len()));
					nine(taps, medium, &mut next[piece], stages);
				});
			};
			let Some(cuts) = row_cuts(cuts, mirrors, at, gw, &xs) else {
				return uncut(xs.clone(), next, &mut stages);
			};
			cut_spans(cuts, xs.clone(), |span, cut| {
				if !cut {
					return uncut(span, next, &mut stages);
				}
				for x in span {
					let v = u[at + x];
					let lap = nine_point(u, at + x, s, cuts[x]);
					next[x] = v + ((v - next[x]) * d[x] + lap * c2[x] * centred(d[x]));
					if let Some(st) = stages.as_mut() {
						advance(&mut st.s1[x], &mut st.s2[x], next[x], st.alpha, 1.0 - st.alpha);
					}
				}
			});
		});
	}

	/// The modified equation's first pass: `w = c2 L u` on the damaged cells.
	fn inner_pass(&mut self) {
		let h = &mut self.host;
		let (s, gw, lap) = (h.stride, h.gw(), self.kernels.lap);
		let (u, c2, cuts, axis, mirrors) = (&h.u, &h.c2, &h.cuts, &h.axis, h.mirrors);
		let (area, xs) = (&h.damage, &h.damage.x);
		let mut rim = h.pml.as_mut().and_then(|p| p.begin(0, u, Medium { cuts, c2 }, 1.0, area));
		grid_rows(&mut h.w, None, s, area, rim.as_mut(), |r, w, _| {
			let (at, rows) = (r * s + PAD, plane_rows(u, r, s));
			let c2 = &c2[at..][..gw];
			let uncut = |span: Range<usize>, w: &mut [f32]| {
				lap(span_rows(&rows, &span), axis, &c2[span.clone()], &mut w[span]);
			};
			let Some(cuts) = row_cuts(cuts, mirrors, at, gw, xs) else { return uncut(xs.clone(), w) };
			cut_spans(cuts, xs.clone(), |span, cut| {
				if !cut {
					return uncut(span, w);
				}
				for x in span {
					w[x] = c2[x] * drp(&rows, x, axis, cuts[x]);
				}
			});
		});
	}

	/// The modified equation's second pass: writes the next field over `prev` from `u` and the plane `w`, and the
	/// intensity stages when `intensity`.
	fn outer_pass(&mut self, intensity: bool) {
		let h = &mut self.host;
		let (s, gw, outer) = (h.stride, h.gw(), self.kernels.outer);
		let (u, w, c2, d, cuts, axis, mirrors) = (&h.u, &h.w, &h.c2, &h.d, &h.cuts, &h.axis, h.mirrors);
		let (area, xs) = (&h.damage, h.damage.x.clone());
		let stages = Host::stages(&mut h.s1, &mut h.s2, h.alpha, intensity);
		let mut rim = h.pml.as_mut().and_then(|p| p.begin(1, w, Medium { cuts, c2 }, INV12, area));
		grid_rows(&mut h.prev, stages, s, area, rim.as_mut(), |r, next, mut stages| {
			let (at, rows) = (r * s + PAD, plane_rows(w, r, s));
			let (uc, wc, c2, d) = (&u[at..][..gw], &w[at..][..gw], &c2[at..][..gw], &d[at..][..gw]);
			let uncut = |span: Range<usize>, next: &mut [f32], stages: Option<Intensity<'_>>| {
				let (u, c2, d) = (&uc[span.clone()], &c2[span.clone()], &d[span.clone()]);
				outer(u, span_rows(&rows, &span), axis, c2, d, &mut next[span], stages);
			};
			let Some(cuts) = row_cuts(cuts, mirrors, at, gw, &xs) else {
				return uncut(xs.clone(), next, stages.as_mut().map(|st| st.span(xs.start, xs.len())));
			};
			cut_spans(cuts, xs.clone(), |span, cut| {
				if !cut {
					let stages = stages.as_mut().map(|st| st.span(span.start, span.len()));
					return uncut(span, next, stages);
				}
				for x in span {
					let (lw, c) = (drp(&rows, x, axis, cuts[x]), uc[x]);
					next[x] = c + ((c - next[x]) * d[x] + (wc[x] + c2[x] * lw * INV12) * centred(d[x]));
					if let Some(st) = stages.as_mut() {
						advance(&mut st.s1[x], &mut st.s2[x], next[x], st.alpha, 1.0 - st.alpha);
					}
				}
			});
		});
	}

	/// One sub-step; the next field ends in `u`. Each pass runs on the damage grown by its reach; the damage then
	/// takes in what the pass and the forcing made non-zero (engine/cpu.rs, Damage).
	fn sub_step(&mut self, intensity: bool) {
		let known = self.host.damage.clone();
		let area = match self.host.shared.scheme {
			TimeScheme::Leapfrog => {
				self.host.damage = self.host.reached(&known);
				self.leapfrog_pass(intensity);
				self.host.force_next(intensity);
				std::mem::replace(&mut self.host.damage, known)
			}
			TimeScheme::ModifiedEquation => {
				// `w` is written on the inner area, which a later sub-step's covers, as the damage only grows.
				self.host.damage = self.host.reached(&known);
				self.inner_pass();
				self.host.force_w(&Damage::NONE, &mut Vec::new());
				let inner = std::mem::replace(&mut self.host.damage, known.clone());
				let excited = extent(&self.host.w, self.host.stride, &inner, known);
				self.host.damage = self.host.reached(&excited);
				self.outer_pass(intensity);
				std::mem::replace(&mut self.host.damage, excited)
			}
		};
		self.host.end_sub_step(&area);
	}
}

host_engine!(AvxEngine, "CPU (AVX2)");

/// Runs `row(r, cells, stages)` on the `gw` grid cells of every padded row `r` of `out` (rows of `s`) that `area`
/// covers, one row per task on the rayon pool; `row` writes `area`'s columns and, when given the row's intensity stages,
/// advances those of the cells it writes. Then runs the rim's psi of x and correction on the row, if given (pml.rs,
/// `XShared::row`), as a pass over the grid followed by one over the rim would.
fn grid_rows(
	out: &mut [f32],
	stages: Option<Stages<'_>>,
	s: usize,
	area: &Damage,
	rim: Option<&mut XPart<'_>>,
	row: impl Fn(usize, &mut [f32], Option<Intensity<'_>>) + Sync,
) {
	if area.is_empty() {
		return;
	}
	let (first, span, n) = (PAD + area.y.start, (PAD + area.y.start) * s..(PAD + area.y.end) * s, area.y.len());
	let gw = s - 2 * PAD;
	let (s1, s2, alpha) = match stages {
		Some(Stages { s1, s2, alpha }) => (Some(&mut s1[span.clone()]), Some(&mut s2[span.clone()]), alpha),
		None => (None, None, 0.0),
	};
	let (shared, rim_rows) = match rim {
		Some(part) => {
			let (shared, chunks) = part.chunks(1);
			(Some(shared), Either::Left(chunks.map(Some)))
		}
		None => (None, Either::Right((0..n).into_par_iter().map(|_| None))),
	};
	out[span]
		.par_chunks_mut(s)
		.zip(chunks_or_none(s1, s, n))
		.zip(chunks_or_none(s2, s, n))
		.zip(rim_rows)
		.enumerate()
		.for_each_init(
			|| shared.map(XShared::scratch),
			|scratch, (k, (((line, s1), s2), rim_rows))| {
				let (r, cells) = (first + k, &mut line[PAD..][..gw]);
				let stages = match (s1, s2) {
					(Some(s1), Some(s2)) => {
						Some(Intensity { s1: &mut s1[PAD..][..gw], s2: &mut s2[PAD..][..gw], alpha })
					}
					_ => None,
				};
				wide(
					#[inline(always)]
					|| {
						row(r, cells, stages);
						if let (Some(shared), Some(mut rows), Some(scratch)) = (shared, rim_rows, scratch.as_mut()) {
							shared.row(r - PAD, &mut rows, 0, cells, scratch);
						}
					},
				);
			},
		);
}

/// The seven rows a DRP kernel reads for the grid cells `span`: from 3 cells left of it to 3 right of it.
#[inline]
fn span_rows<'a>(rows: &Rows<'a>, span: &Range<usize>) -> Rows<'a> {
	// Grid column x is at PAD + x, so 3 cells left of the span's first is at its own index.
	rows.map(|row| &row[span.start..PAD + span.end + 3])
}

/// Runs `f` compiled for AVX2 and FMA, which an [`AvxEngine`] has (its constructor asserts [`available`]); `f` takes
/// the instruction set where it is inlined, so its closure is marked `#[inline(always)]`.
#[inline(always)]
fn wide<R>(f: impl FnOnce() -> R) -> R {
	// SAFETY: called only from an `AvxEngine`'s passes, and `AvxEngine::with_sub_steps`, its only constructor, asserts
	// the CPU has AVX2 and FMA, the features `with_avx2` is compiled for.
	unsafe { with_avx2(f) }
}

/// [`wide`]'s body.
#[target_feature(enable = "avx2,fma")]
#[inline]
fn with_avx2<R>(f: impl FnOnce() -> R) -> R {
	f()
}
