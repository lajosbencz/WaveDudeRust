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
//! over `prev`. With a rim, the CPML corrects the rim of each pass, row by row as the pass writes them (pml.rs).
//!
//! Fused passes: the 4th-order outer pass computes `w` itself where it can, so the plane `w` makes no round trip
//! through memory there. Each block of rows keeps the 7 rows of `w` its stencil reads in a ring; the row 3 below the
//! one it writes is computed from `u` (`c2 L u`, as the inner pass would) on the fresh area, the damage before the
//! sub-step less the band of rows and columns the CPML reads `w` from. The inner pass writes the plane on the rest of
//! the damage: the band, where the CPML corrects `w`, and the cells the damage grew by, whose extent the outer pass's
//! area needs. A driven cell of the fresh area takes `w` from the plane, where the forcing completes it. Every value
//! of `w` is the same expression in the same order as in two passes; the plane holds stale values on the fresh area.
//!
//! Uniform medium: `c2` and `d` never change during a run, and most of a scene is one medium. Each grid row keeps its
//! runs of at least 16 cells where both hold one value, bit for bit; the leapfrog pass and the fused outer pass, its
//! ring included, take that run's values as constants and read neither plane there. The values are the same, so the
//! result is the same bit for bit.
//!
//! State: the planes, the medium, the forcing, the probes and the damage, with the calls that do not depend on the
//! passes, are host.rs's, which the AVX2 engine (engine/avx) shares; this module owns the passes.
//!
//! Threads: one driver thread; each pass fans out over rows, or blocks of rows, on the rayon pool, reading only its
//! input fields.
//!
//! Damage: one rectangle of grid cells (damage.rs) outside which the field and the intensity stages are +0. A pass
//! runs on the damage grown by its reach (the stencil's, the CPML's with a rim), its rows only and its columns within
//! them, and so does the CPML on the rim (pml.rs, `CpuPml::begin`); a skipped cell would compute to +0, so the result
//! is the same bit for bit. The damage then takes in the cells a source drives and the cells the pass made non-zero
//! (any bit set), found in the band it grew by. It is empty at rest and shrinks only on a reset, so every cell ever
//! written is recomputed while the run lasts.

use std::ops::Range;
use std::sync::Arc;

use rayon::iter::Either;
use rayon::prelude::*;

use super::damage::{Damage, extent};
use super::host::{Host, Stages, host_engine};
use super::isa;
use super::pml::{self, CpuPml, Medium, XPart, XRows, XScratch, XShared, cut, cut_taps};
use super::stencil::{INV12, K9_AXIS, K9_DIAGONAL, PAD, StencilCoefficients, TimeScheme, centred};
use super::substeps::sub_steps;
use super::types::{EngineOptions, SceneData};

/// The reference engine. See the module docs for the grid layout and the scheme.
pub struct CpuEngine {
	/// The planes, the medium, the forcing, the probes and the damage (host.rs).
	host: Host,
	/// The runs of each grid row where `c2` and `d` hold one value (module docs, Uniform medium).
	uniform: UniformRuns,
	/// The padded cells of the sub-step's fresh area (module docs, Fused passes) the forcing drove, ascending.
	driven: Vec<usize>,
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
		let host = Host::new(scene.into(), options, n);
		let mut engine = Self { host, uniform: UniformRuns::default(), driven: Vec::new() };
		engine.scene_loaded();
		engine
	}

	/// Rebuilds the uniform runs from the medium the host loaded.
	fn scene_loaded(&mut self) {
		let h = &self.host;
		self.uniform = UniformRuns::of(&h.c2, &h.d, h.stride, h.gw());
	}

	/// The leapfrog pass of the 9-point stencil: writes the next field over `prev`, and the intensity
	/// stages when `intensity`.
	fn leapfrog_pass(&mut self, intensity: bool) {
		let (s, gw) = (self.host.stride, self.host.shared.scene.grid_width() as usize);
		let (u, c2, d, cuts, mirrors, uniform) =
			(&self.host.u, &self.host.c2, &self.host.d, &self.host.cuts, self.host.mirrors, &self.uniform);
		let (area, xs) = (&self.host.damage, self.host.damage.x.clone());
		let stages = Host::stages(&mut self.host.s1, &mut self.host.s2, self.host.alpha, intensity);
		let medium = Medium { cuts, c2 };
		let mut rim = self.host.pml.as_mut().and_then(|p| p.begin(0, u, medium, 1.0, area));
		grid_rows(
			&mut self.host.prev,
			stages,
			s,
			area,
			rim.as_mut(),
			#[cfg_attr(target_arch = "x86_64", inline(always))]
			|r, next| {
				let (at, runs) = (r * s + PAD, uniform.row(r - PAD));
				let (c2, d) = (&c2[at..][..gw], &d[at..][..gw]);
				let update = |x: usize, next: &mut f32, v: f32, lap: f32| {
					*next = v + ((v - *next) * d[x] + lap * c2[x] * centred(d[x]));
				};
				// The cells of `span` with no cut, on row slices ([`nine_span`]), a uniform run's with its constants.
				let uncut = |span: Range<usize>, next: &mut [f32]| {
					medium_spans(runs, c2, d, span, |piece, medium| {
						isa::wide(
							#[cfg_attr(target_arch = "x86_64", inline(always))]
							|| {
								let next = &mut next[piece.clone()];
								match medium {
									Some((c2, d)) => nine_span(u, at + piece.start, s, next, |_| c2, |_| d),
									None => {
										let (c2, d) = (&c2[piece.clone()], &d[piece.clone()]);
										nine_span(u, at + piece.start, s, next, |x| c2[x], |x| d[x]);
									}
								}
							},
						);
					});
				};
				match row_cuts(cuts, mirrors, at, gw, &xs) {
					None => uncut(xs.clone(), next),
					Some(cuts) => cut_spans(cuts, xs.clone(), |span, cut| {
						if !cut {
							return uncut(span, next);
						}
						for x in span {
							update(x, &mut next[x], u[at + x], nine_point(u, at + x, s, cuts[x]));
						}
					}),
				}
			},
		);
	}

	/// The modified equation's first pass: `w = c2 L u` on the damaged cells but those of `fresh`, which the outer pass
	/// computes in its ring (module docs, Fused passes).
	fn inner_pass(&mut self, fresh: &Damage) {
		let (s, gw) = (self.host.stride, self.host.shared.scene.grid_width() as usize);
		let (u, c2, cuts, axis, mirrors) =
			(&self.host.u, &self.host.c2, &self.host.cuts, &self.host.axis, self.host.mirrors);
		let (area, xs) = (&self.host.damage, &self.host.damage.x);
		let mut rim = self.host.pml.as_mut().and_then(|p| p.begin(0, u, Medium { cuts, c2 }, 1.0, area));
		grid_rows(
			&mut self.host.w,
			None,
			s,
			area,
			rim.as_mut(),
			#[cfg_attr(target_arch = "x86_64", inline(always))]
			|r, w| {
				let (at, rows) = (r * s + PAD, plane_rows(u, r, s));
				let c2 = &c2[at..][..gw];
				let cuts = row_cuts(cuts, mirrors, at, gw, xs);
				let mut span = |span: Range<usize>| {
					isa::wide(
						#[cfg_attr(target_arch = "x86_64", inline(always))]
						|| drp_row(&rows, span, axis, cuts, |x, lap| w[x] = c2[x] * lap),
					);
				};
				if fresh.y.contains(&(r - PAD)) {
					span(xs.start..fresh.x.start);
					span(fresh.x.end..xs.end);
				} else {
					span(xs.clone());
				}
			},
		);
	}

	/// The modified equation's second pass, fused with the first on `fresh`: writes the next field over `prev`, and
	/// the intensity stages when `intensity`. Each block of rows keeps the 7 rows of `w` its stencil reads in a ring,
	/// computing `w = c2 L u` on `fresh` and taking the plane's elsewhere (module docs, Fused passes).
	fn outer_pass(&mut self, intensity: bool, fresh: &Damage) {
		let (s, gw) = (self.host.stride, self.host.shared.scene.grid_width() as usize);
		let (u, w, c2, d, cuts, axis, mirrors, driven) = (
			&self.host.u,
			&self.host.w,
			&self.host.c2,
			&self.host.d,
			&self.host.cuts,
			&self.host.axis,
			self.host.mirrors,
			&self.driven,
		);
		let uniform = &self.uniform;
		let (area, xs) = (&self.host.damage, &self.host.damage.x);
		// The columns of `w` the stencil reads.
		let wide = xs.start.saturating_sub(3)..(xs.end + 3).min(gw);
		// Padded row `r` of `w` into its slot of `ring`.
		let fill = |ring: &mut [f32], r: usize| {
			let cells = &mut ring[r % RING * s..][PAD..][..gw];
			let (at, plane) = (r * s + PAD, &w[r * s + PAD..][..gw]);
			let own = if fresh.y.contains(&r.wrapping_sub(PAD)) { fresh.x.clone() } else { 0..0 };
			if own.is_empty() {
				return cells[wide.clone()].copy_from_slice(&plane[wide.clone()]);
			}
			cells[wide.start..own.start].copy_from_slice(&plane[wide.start..own.start]);
			cells[own.end..wide.end].copy_from_slice(&plane[own.end..wide.end]);
			let (c2, d, rows) = (&c2[at..][..gw], &d[at..][..gw], plane_rows(u, r, s));
			let own_cuts = row_cuts(cuts, mirrors, at, gw, &own);
			medium_spans(uniform.row(r - PAD), c2, d, own, |piece, medium| {
				isa::wide(
					#[cfg_attr(target_arch = "x86_64", inline(always))]
					|| match medium {
						Some((k, _)) => drp_row(&rows, piece, axis, own_cuts, |x, lap| cells[x] = k * lap),
						None => drp_row(&rows, piece, axis, own_cuts, |x, lap| cells[x] = c2[x] * lap),
					},
				);
			});
			// The driven cells, whose `w` the forcing completed in the plane.
			let first = driven.partition_point(|&c| c < at);
			for &c in driven[first..].iter().take_while(|&&c| c < at + gw) {
				cells[c - at] = w[c];
			}
		};
		let stages = Host::stages(&mut self.host.s1, &mut self.host.s2, self.host.alpha, intensity);
		let block = area.y.len().div_ceil(2 * rayon::current_num_threads()).max(MIN_BLOCK);
		let mut rim = self.host.pml.as_mut().and_then(|p| p.begin(1, w, Medium { cuts, c2 }, INV12, area));
		grid_blocks(
			&mut self.host.prev,
			s,
			area,
			block,
			RowEnd { stages, rim: rim.as_mut() },
			|| vec![0.0f32; RING * s],
			#[cfg_attr(target_arch = "x86_64", inline(always))]
			|ring, r0, lines, done| {
				for r in r0 - 3..r0 + 3 {
					fill(ring, r);
				}
				for (j, line) in lines.chunks_mut(s).enumerate() {
					let r = r0 + j;
					fill(ring, r + 3);
					let rows: Rows<'_> = std::array::from_fn(|k| &ring[(r + k - 3) % RING * s..][..s]);
					let at = r * s + PAD;
					let (uc, wc, c2, d) = (&u[at..][..gw], &rows[3][PAD..][..gw], &c2[at..][..gw], &d[at..][..gw]);
					let (next, row_cuts) = (&mut line[PAD..][..gw], row_cuts(cuts, mirrors, at, gw, xs));
					medium_spans(uniform.row(r - PAD), c2, d, xs.clone(), |piece, medium| {
						isa::wide(
							#[cfg_attr(target_arch = "x86_64", inline(always))]
							|| match medium {
								Some((k, dk)) => drp_row(&rows, piece, axis, row_cuts, |x, lw| {
									let c = uc[x];
									next[x] = c + ((c - next[x]) * dk + (wc[x] + k * lw * INV12) * centred(dk));
								}),
								None => drp_row(&rows, piece, axis, row_cuts, |x, lw| {
									let c = uc[x];
									next[x] = c + ((c - next[x]) * d[x] + (wc[x] + c2[x] * lw * INV12) * centred(d[x]));
								}),
							},
						);
					});
					done(j, line);
				}
			},
		);
	}

	/// The cells of `known` whose `w` the outer pass computes itself (module docs, Fused passes): all but the band the
	/// CPML reads `w` from.
	fn fresh(&self, known: &Damage) -> Damage {
		let scene = &self.host.shared.scene;
		let (gw, gh) = (scene.grid_width() as usize, scene.grid_height() as usize);
		let band = self.host.pml.as_ref().map_or(0, CpuPml::band);
		let within = |r: &Range<usize>, len: usize| r.start.max(band)..r.end.min(len.saturating_sub(band));
		let fresh = Damage { x: within(&known.x, gw), y: within(&known.y, gh) };
		if fresh.is_empty() { Damage::NONE } else { fresh }
	}

	/// One sub-step; the next field ends in `u`. Each pass runs on the damage grown by its reach; the damage then
	/// takes in what the pass and the forcing made non-zero (module docs, Damage).
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
				// `w` is written on the inner area, which a later sub-step's covers, as the damage only grows; on
				// `fresh` it is left to the outer pass.
				self.host.damage = self.host.reached(&known);
				let fresh = self.fresh(&known);
				self.inner_pass(&fresh);
				self.host.force_w(&fresh, &mut self.driven);
				let inner = std::mem::replace(&mut self.host.damage, known.clone());
				let excited = extent(&self.host.w, self.host.stride, &inner, known);
				self.host.damage = self.host.reached(&excited);
				self.outer_pass(intensity, &fresh);
				std::mem::replace(&mut self.host.damage, excited)
			}
		};
		self.host.end_sub_step(&area);
	}
}

host_engine!(CpuEngine, "CPU");

/// Runs `row(r, cells)` on the `gw` grid cells of every padded row `r` of `out` (rows of `s`) that `area`
/// covers, in parallel; `row` writes `area`'s columns. Then advances the intensity stages of those cells by
/// the values written, if given, and runs the rim's correction on the row, if given ([`grid_blocks`]).
fn grid_rows(
	out: &mut [f32],
	stages: Option<Stages<'_>>,
	s: usize,
	area: &Damage,
	rim: Option<&mut XPart<'_>>,
	row: impl Fn(usize, &mut [f32]) + Sync,
) {
	if area.is_empty() {
		return;
	}
	let (first, span) = (PAD + area.y.start, (PAD + area.y.start) * s..(PAD + area.y.end) * s);
	let (xs, gw) = (PAD + area.x.start..PAD + area.x.end, s - 2 * PAD);
	type RimRow<'r, 'a, 'b> = Option<(&'r XShared<'a>, &'r mut XRows<'b>, &'r mut XScratch)>;
	let run = |r: usize, line: &mut [f32], stages: Option<(&mut [f32], &mut [f32], f32)>, rim: RimRow<'_, '_, '_>| {
		isa::wide(
			#[cfg_attr(target_arch = "x86_64", inline(always))]
			|| {
				row(r, &mut line[PAD..][..gw]);
				if let Some((s1, s2, alpha)) = stages {
					accumulate(&mut s1[xs.clone()], &mut s2[xs.clone()], &line[xs.clone()], alpha);
				}
				if let Some((shared, rows, scratch)) = rim {
					shared.row(r - PAD, rows, 0, &mut line[PAD..][..gw], scratch);
				}
			},
		);
	};
	let lines = out[span.clone()].par_chunks_mut(s).enumerate();
	match (stages, rim) {
		(None, None) => lines.for_each(|(k, line)| run(first + k, line, None, None)),
		(Some(Stages { s1, s2, alpha }), None) => {
			let stages = s1[span.clone()].par_chunks_mut(s).zip(s2[span].par_chunks_mut(s));
			lines.zip(stages).for_each(|((k, line), (s1, s2))| run(first + k, line, Some((s1, s2, alpha)), None));
		}
		(None, Some(part)) => {
			let (shared, chunks) = part.chunks(1);
			lines.zip(chunks).for_each_init(
				|| shared.scratch(),
				|scratch, ((k, line), mut rows)| run(first + k, line, None, Some((shared, &mut rows, scratch))),
			);
		}
		(Some(Stages { s1, s2, alpha }), Some(part)) => {
			let (shared, chunks) = part.chunks(1);
			let stages = s1[span.clone()].par_chunks_mut(s).zip(s2[span].par_chunks_mut(s));
			lines.zip(stages).zip(chunks).for_each_init(
				|| shared.scratch(),
				|scratch, (((k, line), (s1, s2)), mut rows)| {
					run(first + k, line, Some((s1, s2, alpha)), Some((shared, &mut rows, scratch)));
				},
			);
		}
	}
}

/// Slots of the outer pass's ring of `w` rows: the 7 its stencil reads.
const RING: usize = 7;

/// The fewest rows a block of the fused outer pass takes: each block computes the 6 rows of `w` around it again.
const MIN_BLOCK: usize = 16;

/// What runs on each row after a pass writes it ([`grid_blocks`]): the intensity stages' advance and the rim's
/// psi of x and correction, each if given.
struct RowEnd<'s, 'r, 'x> {
	stages: Option<Stages<'s>>,
	rim: Option<&'r mut XPart<'x>>,
}

/// Runs `block(scratch, r, lines, done)` on blocks of up to `rows` padded rows of `out` (rows of `s`) that `area`
/// covers, in parallel: `r` the block's first row, `scratch` from `init` and reused across blocks. `block` writes
/// `area`'s columns of each row of `lines` in order and then calls `done(j, line)` on its row `j`, which advances the
/// intensity stages of those cells by the values written, if given, and then runs the rim's psi of x and correction
/// on the row, if given (pml.rs, `XShared::row`), as a pass over the grid followed by one over the rim would.
fn grid_blocks<T>(
	out: &mut [f32],
	s: usize,
	area: &Damage,
	rows: usize,
	RowEnd { stages, rim }: RowEnd<'_, '_, '_>,
	init: impl Fn() -> T + Sync + Send,
	block: impl Fn(&mut T, usize, &mut [f32], &mut dyn FnMut(usize, &mut [f32])) + Sync + Send,
) {
	if area.is_empty() {
		return;
	}
	let (first, span, n) = (PAD + area.y.start, (PAD + area.y.start) * s..(PAD + area.y.end) * s, area.y.len());
	let (xs, gw, blocks) = (PAD + area.x.start..PAD + area.x.end, s - 2 * PAD, n.div_ceil(rows));
	let (s1, s2, alpha) = match stages {
		Some(Stages { s1, s2, alpha }) => (Some(&mut s1[span.clone()]), Some(&mut s2[span.clone()]), alpha),
		None => (None, None, 0.0),
	};
	let (shared, rim_rows) = match rim {
		Some(part) => {
			let (shared, chunks) = part.chunks(rows);
			(Some(shared), Either::Left(chunks.map(Some)))
		}
		None => (None, Either::Right((0..blocks).into_par_iter().map(|_| None))),
	};
	out[span]
		.par_chunks_mut(rows * s)
		.zip(chunks_or_none(s1, rows * s, blocks))
		.zip(chunks_or_none(s2, rows * s, blocks))
		.zip(rim_rows)
		.enumerate()
		.for_each_init(
			|| (init(), shared.map(XShared::scratch)),
			|(scratch, rim_scratch), (k, (((lines, mut s1), mut s2), mut rim_rows))| {
				let r0 = first + k * rows;
				isa::wide(
					#[cfg_attr(target_arch = "x86_64", inline(always))]
					|| {
						// Called through `dyn`, so out of line: it takes the instruction set on its own.
						block(scratch, r0, lines, &mut |j, line| {
							isa::wide(
								#[cfg_attr(target_arch = "x86_64", inline(always))]
								|| {
									if let (Some(s1), Some(s2)) = (s1.as_deref_mut(), s2.as_deref_mut()) {
										accumulate(
											&mut s1[j * s..][xs.clone()],
											&mut s2[j * s..][xs.clone()],
											&line[xs.clone()],
											alpha,
										);
									}
									if let (Some(shared), Some(rim_rows), Some(rim_scratch)) =
										(shared, &mut rim_rows, &mut *rim_scratch)
									{
										shared.row(r0 + j - PAD, rim_rows, j, &mut line[PAD..][..gw], rim_scratch);
									}
								},
							);
						});
					},
				);
			},
		);
}

/// `buf` in chunks of `size`, or `n` Nones without it.
pub(super) fn chunks_or_none(
	buf: Option<&mut [f32]>,
	size: usize,
	n: usize,
) -> impl IndexedParallelIterator<Item = Option<&mut [f32]>> {
	match buf {
		Some(buf) => Either::Left(buf.par_chunks_mut(size).map(Some)),
		None => Either::Right((0..n).into_par_iter().map(|_| None)),
	}
}

/// The fewest cells of a uniform run (module docs, Uniform medium); shorter runs read `c2` and `d` per cell.
const MIN_RUN: usize = 16;

/// The runs of at least [`MIN_RUN`] grid cells per grid row where `c2` and `d` hold one value, bit for bit, in order:
/// `runs[rows[gy]..rows[gy + 1]]` for grid row `gy`, as column ranges.
#[derive(Default)]
pub(super) struct UniformRuns {
	runs: Vec<(u32, u32)>,
	rows: Vec<u32>,
}

impl UniformRuns {
	/// The runs of the padded `c2` and `d` (row length `s`) of a grid `gw` cells wide.
	pub(super) fn of(c2: &[f32], d: &[f32], s: usize, gw: usize) -> Self {
		let gh = (c2.len() / s).saturating_sub(2 * PAD);
		let mut out = Self { runs: Vec::new(), rows: vec![0] };
		for gy in 0..gh {
			let at = (gy + PAD) * s + PAD;
			let (c2, d) = (&c2[at..][..gw], &d[at..][..gw]);
			let mut start = 0;
			while start < gw {
				let same = |x: usize| c2[x].to_bits() == c2[start].to_bits() && d[x].to_bits() == d[start].to_bits();
				let end = (start..gw).find(|&x| !same(x)).unwrap_or(gw);
				if end - start >= MIN_RUN {
					out.runs.push((start as u32, end as u32));
				}
				start = end;
			}
			out.rows.push(out.runs.len() as u32);
		}
		out
	}

	/// The runs of grid row `gy`.
	pub(super) fn row(&self, gy: usize) -> &[(u32, u32)] {
		match (self.rows.get(gy), self.rows.get(gy + 1)) {
			(Some(&a), Some(&b)) => &self.runs[a as usize..b as usize],
			_ => &[],
		}
	}
}

/// Hands `f` the pieces of `xs` in order, each with the `(c2, d)` of the uniform run it lies in (from the row's `c2`
/// and `d`), or None between runs.
#[inline(always)]
pub(super) fn medium_spans(
	runs: &[(u32, u32)],
	c2: &[f32],
	d: &[f32],
	xs: Range<usize>,
	mut f: impl FnMut(Range<usize>, Option<(f32, f32)>),
) {
	let mut at = xs.start;
	for &(a, b) in runs {
		let (a, b) = ((a as usize).max(at), (b as usize).min(xs.end));
		if a >= b {
			continue;
		}
		if at < a {
			f(at..a, None);
		}
		f(a..b, Some((c2[a], d[a])));
		at = b;
	}
	if at < xs.end {
		f(at..xs.end, None);
	}
}

/// The 9-point leapfrog update of the `next.len()` cells from padded cell `at`, none of them cut, on row slices, as
/// [`nine_point`] in the same order; `c2(x)` and `d(x)` give the medium of cell `x` of the span. Every slice has the
/// span's length, so the loop has no bounds checks and vectorises.
#[inline(always)]
fn nine_span(u: &[f32], at: usize, s: usize, next: &mut [f32], c2: impl Fn(usize) -> f32, d: impl Fn(usize) -> f32) {
	let len = next.len();
	let tap = |k: isize| &u[at.wrapping_add_signed(k)..][..len];
	let (si, one) = (s as isize, 1isize);
	let (c, w, e, n, so) = (tap(0), tap(-one), tap(one), tap(-si), tap(si));
	let (nw, ne, sw, se) = (tap(-si - 1), tap(-si + 1), tap(si - 1), tap(si + 1));
	for x in 0..len {
		let v = c[x];
		let diagonals = ((nw[x] - v) + (ne[x] - v)) + ((sw[x] - v) + (se[x] - v));
		let axes = ((n[x] - v) + (so[x] - v)) + ((w[x] - v) + (e[x] - v));
		let lap = K9_DIAGONAL * diagonals + K9_AXIS * axes;
		let (c2, d) = (c2(x), d(x));
		next[x] = v + ((v - next[x]) * d + lap * c2 * centred(d));
	}
}

/// The 9-point Laplacian at padded cell `i` (row length `s`) with the cut byte `cut`: a diagonal tap
/// is dropped when either axis neighbour it passes is a mirror, so no wave crosses a mirror line
/// that is only 8-connected; the rule is symmetric between the two cells of a tap. Same order of
/// operations as the uncut rows.
#[inline]
pub(super) fn nine_point(u: &[f32], i: usize, s: usize, cut: u8) -> f32 {
	let v = u[i];
	let [nw, ne, sw, se] = pml::diagonal_cuts(cut);
	let tap = |k: usize, cut: bool| if cut { 0.0 } else { u[k] };
	let diagonals =
		((tap(i - s - 1, nw) - v) + (tap(i - s + 1, ne) - v)) + ((tap(i + s - 1, sw) - v) + (tap(i + s + 1, se) - v));
	let axes = ((u[i - s] - v) + (u[i + s] - v)) + ((u[i - 1] - v) + (u[i + 1] - v));
	K9_DIAGONAL * diagonals + K9_AXIS * axes
}

/// Seven padded rows (`s` values each, grid column x at `PAD + x`) from 3 above to 3 below a centre row: the rows a
/// DRP Laplacian of the centre row reads, from a plane or from the fused outer pass's ring of `w` rows.
pub(super) type Rows<'a> = [&'a [f32]; 7];

/// The rows of the plane `u` (row length `s`) around padded row `r`.
#[inline]
pub(super) fn plane_rows(u: &[f32], r: usize, s: usize) -> Rows<'_> {
	std::array::from_fn(|k| &u[(r + k - 3) * s..][..s])
}

/// The DRP Laplacian at grid column `x` of the centre row of `rows`, the taps beyond a mirror dropped (`cut`):
/// `sum over m of a_m sum over ring m of (u_k - u)`, the stencil since `a0 = -2 (a1 + a2 + a3)`.
#[inline]
pub(super) fn drp(rows: &Rows<'_>, x: usize, a: &[f32; 4], cut: u8) -> f32 {
	let i = PAD + x;
	let c = rows[3][i];
	let x = cut_taps(std::array::from_fn(|k| rows[3][i + k - 3]), cut >> cut::X_SHIFT);
	let y = cut_taps(std::array::from_fn(|k| rows[k][i]), cut >> cut::Y_SHIFT);
	let ring = |m: usize| ((x[3 - m] - c) + (x[3 + m] - c)) + ((y[3 - m] - c) + (y[3 + m] - c));
	a[1] * ring(1) + a[2] * ring(2) + a[3] * ring(3)
}

/// The cut bytes of the `gw` grid cells from padded cell `at`, or None when the cells `xs` of the row have no mirror.
#[inline]
pub(super) fn row_cuts<'a>(cuts: &'a [u8], mirrors: bool, at: usize, gw: usize, xs: &Range<usize>) -> Option<&'a [u8]> {
	if !mirrors {
		return None;
	}
	let row = &cuts[at..][..gw];
	row[xs.clone()].iter().any(|&c| c != 0).then_some(row)
}

/// Hands `f` the maximal runs of `cuts` within `xs` in order, each with whether its cells drop a tap (a non-zero
/// cut byte). A row crossed by a mirror has only a few cut cells near it; the runs between take the row-slice loops.
#[inline(always)]
pub(super) fn cut_spans(cuts: &[u8], xs: Range<usize>, mut f: impl FnMut(Range<usize>, bool)) {
	let mut start = xs.start;
	while start < xs.end {
		let cut = cuts[start] != 0;
		let len = cuts[start..xs.end].iter().position(|&c| (c != 0) != cut).unwrap_or(xs.end - start);
		f(start..start + len, cut);
		start += len;
	}
}

/// The DRP Laplacian of the grid cells `xs` of the centre row of `rows`, handed to `f` with the cell's column. Cells
/// without a cut run on row slices ([`drp_span`]), the cut ones through [`drp`].
#[inline(always)]
fn drp_row(rows: &Rows<'_>, xs: Range<usize>, a: &[f32; 4], cuts: Option<&[u8]>, mut f: impl FnMut(usize, f32)) {
	let Some(cuts) = cuts else { return drp_span(rows, xs, a, &mut f) };
	cut_spans(cuts, xs, |span, cut| {
		if !cut {
			return drp_span(rows, span, a, &mut f);
		}
		for x in span {
			f(x, drp(rows, x, a, cuts[x]));
		}
	});
}

/// The DRP Laplacian of the cells `span` of the centre row of `rows`, none of them cut, on row slices, as [`drp`] in
/// the same order.
#[inline(always)]
fn drp_span(rows: &Rows<'_>, span: Range<usize>, a: &[f32; 4], f: &mut impl FnMut(usize, f32)) {
	let (x0, len) = (span.start, span.len());
	let tap = |k: isize| &rows[3][(PAD + x0).wrapping_add_signed(k)..][..len];
	let column = |k: usize| &rows[k][PAD + x0..][..len];
	let c = tap(0);
	let (w1, e1, n1, s1) = (tap(-1), tap(1), column(2), column(4));
	let (w2, e2, n2, s2) = (tap(-2), tap(2), column(1), column(5));
	let (w3, e3, n3, s3) = (tap(-3), tap(3), column(0), column(6));
	for x in 0..len {
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
	isa::wide(
		#[cfg_attr(target_arch = "x86_64", inline(always))]
		|| {
			let rest = 1.0 - alpha;
			for ((a, b), &v) in s1.iter_mut().zip(s2).zip(v) {
				*a = *a * alpha + v * v * rest;
				*b = *b * alpha + *a * rest;
			}
		},
	);
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::WaveEngine;
	use crate::engine::types::{Emitter, FrameShape, ImageSource, ProbeCells, ProbeReadings, Snapshot};
	use crate::model::{Edges, Layer, PulseShape, Scene, Stencil};

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
		assert_eq!(engine(Edges::Open, 0, Stencil::NinePoint).host.shared.sources.emitters.len(), 1);
	}

	#[test]
	fn reflective_ring_stays_zero_and_intensity_accumulates() {
		let mut e = engine(Edges::Reflective, 0, Stencil::FourthOrder);
		for _ in 0..80 {
			e.advance_one_step();
		}
		let (s, rows) = (e.host.stride, e.host.u.len() / e.host.stride);
		let padding = (0..rows * s).filter(|&i| {
			let (x, y) = (i % s, i / s);
			x < PAD || y < PAD || x >= s - PAD || y >= rows - PAD
		});
		assert!(padding.into_iter().all(|i| e.host.u[i] == 0.0 && e.host.prev[i] == 0.0 && e.host.w[i] == 0.0));
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
		assert_eq!(e.host.alpha, (-1.0f64 / 10.0).exp() as f32);
		let mut low = emitter(1, 1);
		low.level = 50.0;
		e.set_emitters(&[low]);
		assert_eq!(e.host.alpha, (-1.0f64 / 20.0).exp() as f32);
		e.set_emitters(&[]);
		assert_eq!(e.host.alpha, (-1.0f64 / 10.0).exp() as f32);
		e.host.shared.sources.level_used[0] = true;
		e.host.update_alpha();
		assert_eq!(e.host.alpha, (-1.0f64 / 10.0).exp() as f32, "level 0 is not a source level");
	}

	/// Every buffer the step writes but `w`, which holds stale values on the fresh area (module docs, Fused passes), as
	/// bits.
	fn state(e: &CpuEngine) -> Vec<u32> {
		[&e.host.u, &e.host.prev, &e.host.s1, &e.host.s2].into_iter().flatten().map(|v| v.to_bits()).collect()
	}

	#[test]
	fn the_damage_skips_only_cells_that_stay_zero() {
		for stencil in [Stencil::NinePoint, Stencil::FourthOrder] {
			for (edges, rim) in [(Edges::Open, 0), (Edges::Reflective, 0), (Edges::Open, -6)] {
				let mut scene = SceneData::blank(60, 44, &Scene { damping_edge: rim, edges, ..Scene::default() });
				let gw = scene.grid_width() as usize;
				let r = scene.rim as usize;
				for gy in r + 20..r + 26 {
					scene.c2[gy * gw + r + 38] = 0.0;
				}
				scene.image_sources.push(ImageSource { gx: (r + 50) as u32, gy: (r + 40) as u32, level: 40 });
				let mut damaged = CpuEngine::new(scene.clone(), options(stencil));
				let mut whole = CpuEngine::new(scene, options(stencil));
				for e in [&mut damaged, &mut whole] {
					e.set_emitters(&[emitter(30, 22), emitter(30, 41)]);
				}
				let cells = gw * whole.host.shared.scene.grid_height() as usize;
				whole.set_field(&vec![0.0; cells], &vec![0.0; cells]);
				damaged.advance_one_step();
				whole.advance_one_step();
				let all = Damage::all(gw, cells / gw);
				assert!(damaged.host.damage != all, "{stencil:?} {edges:?} {rim}: nothing skipped");
				for step in 1..150 {
					damaged.advance_one_step();
					whole.advance_one_step();
					assert!(state(&damaged) == state(&whole), "{stencil:?} {edges:?} {rim}: step {step}");
				}
			}
		}
	}

	#[test]
	fn an_idle_scene_has_no_damage_and_a_reset_clears_it() {
		let mut e = CpuEngine::new(SceneData::blank(40, 30, &Scene::default()), options(Stencil::FourthOrder));
		for _ in 0..5 {
			e.advance_one_step();
		}
		assert_eq!(e.host.damage, Damage::NONE);
		e.set_emitters(&[emitter(20, 15)]);
		e.advance_one_step();
		assert!(!e.host.damage.is_empty());
		e.reset();
		assert_eq!(e.host.damage, Damage::NONE);
	}

	#[test]
	fn cut_spans_cover_the_row_in_maximal_runs() {
		let mut runs = Vec::new();
		let cuts = [0, 0, 3, 1, 0, 0, 0, 8];
		cut_spans(&cuts, 0..8, |span, cut| runs.push((span, cut)));
		assert_eq!(runs, [(0..2, false), (2..4, true), (4..7, false), (7..8, true)]);
		runs.clear();
		cut_spans(&cuts, 3..6, |span, cut| runs.push((span, cut)));
		assert_eq!(runs, [(3..4, true), (4..6, false)]);
		runs.clear();
		cut_spans(&[0; 5], 0..5, |span, cut| runs.push((span, cut)));
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
		let rows = plane_rows(&u, at / s, s);
		drp_row(&rows, 0..gw, &a, Some(&cuts), |x, v| got[x] = v);
		let want: Vec<f32> = (0..gw).map(|x| drp(&rows, x, &a, cuts[x])).collect();
		assert_eq!(
			got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
			want.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
		);
	}

	#[test]
	fn uniform_runs_cover_the_long_constant_stretches_and_split_a_span() {
		let (gw, s) = (60, 60 + 2 * PAD);
		let mut c2 = vec![0.0f32; s * (2 + 2 * PAD)];
		let mut d = vec![1.0f32; c2.len()];
		let at = |gy: usize| (gy + PAD) * s + PAD;
		// Row 0: 0.5 on 0..20, a ramp on 20..30, 0.25 on 30..60 but d differs at 40; row 1: 15 cells of 0.5, 16 of
		// -0.0, then +0.0.
		for x in 0..gw {
			c2[at(0) + x] = if x < 20 {
				0.5
			} else if x < 30 {
				x as f32
			} else {
				0.25
			};
			c2[at(1) + x] = if x < 15 {
				0.5
			} else if x < 31 {
				-0.0
			} else {
				0.0
			};
		}
		d[at(0) + 40] = 0.9;
		let runs = UniformRuns::of(&c2, &d, s, gw);
		assert_eq!(runs.row(0), [(0, 20), (41, 60)]);
		assert_eq!(runs.row(1), [(15, 31), (31, 60)]);
		assert_eq!(runs.row(2), &[] as &[(u32, u32)]);
		let mut pieces = Vec::new();
		let (c2, d) = (&c2[at(0)..][..gw], &d[at(0)..][..gw]);
		medium_spans(runs.row(0), c2, d, 10..50, |piece, m| pieces.push((piece, m)));
		assert_eq!(pieces, [(10..20, Some((0.5, 1.0))), (20..41, None), (41..50, Some((0.25, 1.0)))]);
	}
}
