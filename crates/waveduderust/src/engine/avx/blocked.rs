//! The 9-point step with its sub-steps fused into one sweep over the rows (module docs of engine/avx, Blocked steps).
//!
//! A unit is the work of one grid row at one level `j` (1..=n) of the step, level 0 being the field before it:
//!
//! - `D(q, j)`, on a row of the step's area: with a rim, on a y-strip row, xi of y and `delta_y` of level `j - 1`;
//!   the leapfrog update of row `q` from level `j - 1` (the intensity stages on level `n`); with a rim, the CPML's x
//!   part of the row; the forcing of sub-step `j - 1` on the row's driven cells. On every row with Mur chains (all
//!   rows when the edges are open without a rim), Mur's condition on the chains the row owns: its left and right ring
//!   cells, and on the first and last grid rows the rings above and below.
//! - `Y(q, j)`, with a rim, on a y-strip row of the area: psi of y of level `j - 1`.
//!
//! Each unit reads and writes the state of its own row and reads its neighbours': `D` reads the field of level `j - 1`
//! within 1 row, or 3 with a rim (`delta_y`'s second derivative), and psi of y within 2 rows; `Y` reads the field
//! within 2 rows (a first derivative; it is handed 7 rows, of which the outer two are unused). Level `j` is written in
//! place over level `j - 2` (the planes alternate), so a unit also waits for the readers of what it overwrites.
//! [`Tiling`] derives from these which units a band of rows can run without its neighbours, and the order.

use std::ops::Range;

use rayon::prelude::*;

use super::super::forcing::live;
use super::super::mur::MurChain;
use super::super::pml::{RimTables, XLine, XPass, XScratch};
use super::super::stencil::{PAD, centred};
use super::NineLine;
use super::kernels::Intensity;

/// A buffer the units of a blocked step share across threads: each unit takes the rows it reads and writes as slices
/// for its own duration, and the tiling keeps any two units that run at once off each other's written rows.
#[derive(Clone, Copy)]
pub(super) struct Shared {
	ptr: *mut f32,
	len: usize,
}

// SAFETY: a `Shared` is a pointer into a buffer that outlives the blocked step (an engine's plane or the CPML's
// memory, borrowed mutably for the step); the slices taken from it follow `get` and `get_mut`'s contracts.
unsafe impl Send for Shared {}
// SAFETY: as for `Send`.
unsafe impl Sync for Shared {}

impl Shared {
	pub(super) fn of(buf: &mut [f32]) -> Self {
		Self { ptr: buf.as_mut_ptr(), len: buf.len() }
	}

	/// The values `range` of the buffer.
	///
	/// # Safety
	///
	/// The buffer outlives `'a`, and while the slice lives nothing writes `range`.
	#[inline(always)]
	unsafe fn get<'a>(self, range: Range<usize>) -> &'a [f32] {
		assert!(range.start <= range.end && range.end <= self.len, "a row within the buffer");
		// SAFETY: within the buffer (asserted), which outlives 'a and which nothing writes there meanwhile (the
		// caller's contract).
		unsafe { std::slice::from_raw_parts(self.ptr.add(range.start), range.len()) }
	}

	/// The values `range` of the buffer, to write.
	///
	/// # Safety
	///
	/// The buffer outlives `'a`, and while the slice lives nothing else reads or writes `range`.
	#[inline(always)]
	#[allow(clippy::mut_from_ref)]
	unsafe fn get_mut<'a>(self, range: Range<usize>) -> &'a mut [f32] {
		assert!(range.start <= range.end && range.end <= self.len, "a row within the buffer");
		// SAFETY: within the buffer (asserted), which outlives 'a and which nothing else touches there meanwhile
		// (the caller's contract).
		unsafe { std::slice::from_raw_parts_mut(self.ptr.add(range.start), range.len()) }
	}
}

/// Mur's chains by the grid row that owns them: a row its left and right chains, the first row also the chains of the
/// rings above the grid, the last row those below.
#[derive(Default)]
pub(super) struct MurRows {
	chains: Vec<MurChain>,
	/// `chains[starts[q]..starts[q + 1]]` for grid row `q`.
	starts: Vec<u32>,
}

impl MurRows {
	/// `chains` of a grid `gh` rows high and rows of `s`, by row.
	pub(super) fn of(chains: &[MurChain], s: usize, gh: usize) -> Self {
		let owner = |m: &MurChain| (m.rings[0] / s).clamp(PAD, PAD + gh - 1) - PAD;
		let mut sorted = chains.to_vec();
		sorted.sort_by_key(owner);
		let starts = (0..=gh).map(|q| sorted.partition_point(|m| owner(m) < q) as u32).collect();
		Self { chains: sorted, starts }
	}

	pub(super) fn is_empty(&self) -> bool {
		self.chains.is_empty()
	}

	fn row(&self, q: usize) -> &[MurChain] {
		&self.chains[self.starts[q] as usize..self.starts[q + 1] as usize]
	}
}

/// Which units of a blocked step of `n` levels each part runs, and in what order. See the module docs.
///
/// A band of rows `a..b` runs, in a first parallel phase, the units whose inputs and whose overwritten values'
/// readers all lie in the band: at level `j` the `D` units of `a + sd(j)..b - sd(j)` and the `Y` units of
/// `a + sy(j)..b - sy(j)`, shrunk only at a side where another band lies. A second parallel phase runs the rest, the
/// units of `b - sd(j)..b + sd(j)` (and `sy`) around each boundary `b` between bands. Bands of at least
/// [`min_band`](Self::min_band) rows keep any two boundaries' units off each other's rows.
///
/// The shrinks: `Y(q, j)` reads the field of level `j - 1` within `ry` rows and overwrites psi of y of level `j - 2`,
/// which the `D` units of level `j - 1` within 2 rows read: `sy(j) = sd(j - 1) + max(ry, 2)` (`sy(1) = 0`). `D(q, j)`
/// reads psi of y of level `j - 1` within 2 rows and the field within `rd` rows, and overwrites the field of level
/// `j - 2`, which the `D` units of level `j - 1` within `rd` rows and the `Y` units within `ry` rows read:
/// `sd(j) = max(sy(j) + 2, sd(j - 1) + rd, sy(j - 1) + ry)`, `sd(1) = 2`. With a rim `rd` is 3 and `ry` 2, so
/// `sd(j) = 4 j - 2`; without one there are no `Y` units and `rd` is 1: `sd(j) = j - 1`.
///
/// Within a part the units run in one sweep: at tick `t`, for each level `j` in turn, `Y(f + 2, j)` and `D(f, j)` with
/// `f = t - (j - 1) lag`. A unit's inputs then come from the ticks before or from lower levels of the same tick, and the
/// readers of what it overwrites have run: `lag` 1 without a rim, 4 with one (`Y(f + 2, j)` reads `D(f + 4, j - 1)`,
/// and `D(f, j)` overwrites what `D(f + 3, j - 1)` and `Y(f + 2, j - 1)` read).
pub(super) struct Tiling {
	n: usize,
	rim: bool,
	lag: usize,
	/// The farthest any unit reads the field.
	rho: usize,
	/// `sd(j)` and `sy(j)` at `j - 1`.
	sd: Vec<usize>,
	sy: Vec<usize>,
}

impl Tiling {
	pub(super) fn new(n: usize, rim: bool) -> Self {
		// The field's reach of `D` and of `Y`, psi of y's of `D`, and the sweep's lag (above).
		let (rd, ry, psi, lag) = if rim { (3, 2, 2, 4) } else { (1, 0, 0, 1) };
		let (mut sd, mut sy) = (vec![psi], vec![0]);
		for j in 1..n {
			sy.push(sd[j - 1] + ry.max(psi));
			sd.push((sy[j] + psi).max(sd[j - 1] + rd).max(sy[j - 1] + ry));
		}
		Self { n, rim, lag, rho: rd, sd, sy }
	}

	/// Whether this is the tiling of `n` levels with or without a rim.
	pub(super) fn is(&self, n: usize, rim: bool) -> bool {
		(self.n, self.rim) == (n, rim)
	}

	/// The fewest rows of a band.
	pub(super) fn min_band(&self) -> usize {
		2 * self.sd[self.n - 1].max(self.sy[self.n - 1]) + self.rho
	}

	/// Runs `y(q, j)` and `d(q, j)` on the units `d_rows(j)` and `y_rows(j)` in the sweep's order.
	fn sweep(
		&self,
		d_rows: impl Fn(usize) -> Range<usize>,
		y_rows: impl Fn(usize) -> Range<usize>,
		mut y: impl FnMut(usize, usize),
		mut d: impl FnMut(usize, usize),
	) {
		let (mut lo, mut hi) = (usize::MAX, 0);
		for r in (1..=self.n).flat_map(|j| [d_rows(j), y_rows(j)]).filter(|r| !r.is_empty()) {
			(lo, hi) = (lo.min(r.start), hi.max(r.end));
		}
		if lo >= hi {
			return;
		}
		for t in lo as isize - 2..(hi + (self.n - 1) * self.lag) as isize {
			for j in 1..=self.n {
				let f = t - ((j - 1) * self.lag) as isize;
				if f + 2 >= 0 && y_rows(j).contains(&((f + 2) as usize)) {
					y((f + 2) as usize, j);
				}
				if f >= 0 && d_rows(j).contains(&(f as usize)) {
					d(f as usize, j);
				}
			}
		}
	}

	/// The first phase's units of the band `rows`, `inner` telling whether another band lies above and below it.
	pub(super) fn band(
		&self,
		rows: Range<usize>,
		inner: (bool, bool),
		y: impl FnMut(usize, usize),
		d: impl FnMut(usize, usize),
	) {
		let within = |shrink: usize| {
			let (top, bottom) = (if inner.0 { shrink } else { 0 }, if inner.1 { shrink } else { 0 });
			(rows.start + top)..rows.end.saturating_sub(bottom).max(rows.start + top)
		};
		self.sweep(|j| within(self.sd[j - 1]), |j| within(self.sy[j - 1]), y, d);
	}

	/// The second phase's units around the boundary `b` between two bands.
	pub(super) fn seam(&self, b: usize, y: impl FnMut(usize, usize), d: impl FnMut(usize, usize)) {
		let around = |shrink: usize| b - shrink..b + shrink;
		self.sweep(|j| around(self.sd[j - 1]), |j| around(self.sy[j - 1]), y, d);
	}
}

/// The rim's state for a blocked step: the tables, the memory variables of application 0 and the pass's columns.
pub(super) struct Rim<'a> {
	pub(super) tables: RimTables<'a>,
	pub(super) psi_y: Shared,
	pub(super) xi_y: Shared,
	pub(super) delta: Shared,
	pub(super) psi_x: Shared,
	pub(super) xi_x: Shared,
	pub(super) z: Shared,
	pub(super) pass: XPass<'a>,
}

/// A blocked 9-point step: what its units read and write. Level `j` lives in `planes[j % 2]`.
pub(super) struct Step<'a> {
	pub(super) s: usize,
	pub(super) gw: usize,
	pub(super) gh: usize,
	pub(super) n: usize,
	/// The planes `u` and `prev` of the step's start.
	pub(super) planes: [Shared; 2],
	/// The intensity stages and their weight, advanced on level `n` when given.
	pub(super) stages: Option<(Shared, Shared, f32)>,
	pub(super) alpha: f32,
	pub(super) line: NineLine<'a>,
	pub(super) d: &'a [f32],
	pub(super) cuts: &'a [u8],
	/// The grid rows the passes run on; the columns are `line`'s.
	pub(super) area: Range<usize>,
	/// The source table of each sub-step.
	pub(super) tables: &'a [Vec<f32>],
	/// The driven cells (padded, ascending) with their runs of table entries in `entries`.
	pub(super) driven: &'a [(usize, u32, u32)],
	pub(super) entries: &'a [u32],
	pub(super) mur: &'a MurRows,
	pub(super) rim: Option<Rim<'a>>,
}

impl Step<'_> {
	/// The grid rows with units: the area's, and every row when Mur's chains run.
	fn rows(&self) -> Range<usize> {
		if self.mur.is_empty() { self.area.clone() } else { 0..self.gh }
	}

	/// Runs the step's units with the threads of the rayon pool: the bands, then the seams between them ([`Tiling`]).
	pub(super) fn run(&self, tiling: &Tiling, bands: usize) {
		let (rows, area) = (self.rows(), &self.area);
		let count = bands.clamp(1, (area.len() / tiling.min_band()).max(1));
		// Band k starts at row start(k): the area's rows split evenly, the rows beyond it to the outer bands.
		let start = |k: usize| match k {
			0 => rows.start,
			k if k == count => rows.end,
			k => area.start + k * area.len() / count,
		};
		let scratch = || self.rim.as_ref().map(|rim| rim.tables.scratch());
		let band = |scratch: &mut Option<XScratch>, k: usize| {
			let inner = (k > 0, k + 1 < count);
			tiling.band(start(k)..start(k + 1), inner, |q, j| self.y_unit(q, j), |q, j| self.d_unit(q, j, scratch));
		};
		if count == 1 {
			return band(&mut scratch(), 0);
		}
		(0..count).into_par_iter().for_each_init(scratch, band);
		(1..count).into_par_iter().for_each_init(scratch, |scratch, k| {
			tiling.seam(start(k), |q, j| self.y_unit(q, j), |q, j| self.d_unit(q, j, scratch));
		});
	}

	/// `Y(q, j)`: psi of y of level `j - 1` on y-strip row `q` of the area; nothing on other rows.
	fn y_unit(&self, q: usize, j: usize) {
		let Some(rim) = &self.rim else { return };
		let Some(k) = rim.tables.strip_row(q).filter(|_| self.area.contains(&q)) else { return };
		let (s, gw, r) = (self.s, self.gw, q + PAD);
		// SAFETY: the planes and the CPML's memory outlive the step. The tiling runs no unit that writes the field of
		// level `j - 1` within 3 rows of `q` or psi of y of row `q` while this one runs, and no other unit touches psi
		// of y of row `q`.
		let (field, line) = unsafe {
			(self.planes[(j - 1) % 2].get((r - 3) * s..(r + 4) * s), rim.psi_y.get_mut(k * gw..(k + 1) * gw))
		};
		super::wide(
			#[inline(always)]
			|| rim.tables.psi_y_line(q, field, self.cuts, &rim.pass.xs, line),
		);
	}

	/// `D(q, j)`: row `q` from level `j - 1` to level `j` (module docs).
	fn d_unit(&self, q: usize, j: usize, scratch: &mut Option<XScratch>) {
		super::wide(
			#[inline(always)]
			|| {
				if self.area.contains(&q) {
					self.update(q, j, scratch);
				}
				self.open_edges(q, j);
			},
		);
	}

	/// The passes of `D(q, j)` on a row of the area: `delta_y`, the leapfrog update, the rim's x part and the forcing.
	#[inline(always)]
	fn update(&self, q: usize, j: usize, scratch: &mut Option<XScratch>) {
		let (s, gw, r) = (self.s, self.gw, q + PAD);
		let (src, dst) = (self.planes[(j - 1) % 2], self.planes[j % 2]);
		// The rim's y stages of a y-strip row: psi of y of the rows within 2 in its strip (zeros beyond it), the
		// field of level `j - 1` within 3 rows.
		let strip = self.rim.as_ref().and_then(|rim| Some((rim, rim.tables.strip_row(q)?)));
		if let Some((rim, k)) = strip {
			let psi = |dk: isize| match rim.tables.neighbour_row(k, dk) {
				// SAFETY: the planes and the CPML's memory outlive the step; the tiling runs no unit writing psi of y
				// within 2 rows of `q`, the field of level `j - 1` within 3 rows, or xi of y and `delta_y` of row `q`
				// while this one runs.
				Some(row) => unsafe { rim.psi_y.get(row * gw..(row + 1) * gw) },
				None => &rim.tables.zeros()[..gw],
			};
			// SAFETY: as above.
			let (field, xi, delta) = unsafe {
				(
					src.get((r - 3) * s..(r + 4) * s),
					rim.xi_y.get_mut(k * gw..(k + 1) * gw),
					rim.delta.get_mut(k * gw..(k + 1) * gw),
				)
			};
			rim.tables.delta_y_line(q, field, self.cuts, &rim.pass.xs, [psi(1), psi(-1), psi(2), psi(-2)], xi, delta);
		}
		// SAFETY: the planes and the stages outlive the step. The tiling runs no unit writing the field of level `j - 1`
		// within 1 row of `q` while this one runs, and no other unit touches row `q` of level `j`'s plane or of the
		// stages.
		let (u, next) = unsafe { (src.get((r - 1) * s..(r + 2) * s), dst.get_mut(r * s..(r + 1) * s)) };
		let mut stages = self.stages.filter(|_| j == self.n).map(|(s1, s2, _)| {
			// SAFETY: as above.
			unsafe { (s1.get_mut(r * s..(r + 1) * s), s2.get_mut(r * s..(r + 1) * s)) }
		});

		let alpha = self.alpha;
		let intensity =
			stages.as_mut().map(|(s1, s2)| Intensity { s1: &mut s1[PAD..][..gw], s2: &mut s2[PAD..][..gw], alpha });
		self.line.row(r, u, &mut next[PAD..][..gw], intensity);
		if let (Some(rim), Some(scratch)) = (&self.rim, scratch.as_mut()) {
			let (w, zw) = (2 * rim.tables.rim(), rim.tables.z_width());
			// SAFETY: as above: `delta_y` of row `q` and the x part's memory of row `q` are this unit's alone.
			let (delta, memory) = unsafe {
				let delta = strip.map(|(_, k)| rim.delta.get(k * gw..(k + 1) * gw));
				let memory = XLine {
					psi: rim.psi_x.get_mut(q * w..(q + 1) * w),
					xi: rim.xi_x.get_mut(q * w..(q + 1) * w),
					z: rim.z.get_mut(q * zw..(q + 1) * zw),
				};
				(delta, memory)
			};
			rim.tables.x_line(q, u, delta, memory, &mut next[PAD..][..gw], scratch, &rim.pass);
		}
		self.force(q, j, next, stages);
	}

	/// Adds `h F` of sub-step `j - 1` to the driven cells of row `q` of `next` (the padded row), and moves their
	/// intensity stages, when given, to the forced value: host.rs's `force_next` on the row.
	#[inline(always)]
	fn force(&self, q: usize, j: usize, next: &mut [f32], mut stages: Option<(&mut [f32], &mut [f32])>) {
		let (at, table, rest) = ((q + PAD) * self.s, &self.tables[j - 1], 1.0 - self.alpha);
		let first = self.driven.partition_point(|&(c, ..)| c < at);
		for &(cell, start, len) in self.driven[first..].iter().take_while(|&&(c, ..)| c < at + self.s) {
			let x = cell - at;
			let (before, h) = (next[x], centred(self.d[cell]));
			let mut v = before;
			let mut driven = false;
			for &e in &self.entries[start as usize..(start + len) as usize] {
				let f = table[e as usize];
				if live(f) {
					v += h * f;
					driven = true;
				}
			}
			if !driven {
				continue;
			}
			next[x] = v;
			if let Some((s1, s2)) = stages.as_mut() {
				let s1v = s1[x] + (v * v - before * before) * rest;
				s2[x] += (s1v - s1[x]) * rest;
				s1[x] = s1v;
			}
		}
	}

	/// Mur's condition on the chains row `q` owns, on level `j`: host.rs's `apply_open_edges` on them.
	#[inline(always)]
	fn open_edges(&self, q: usize, j: usize) {
		let chains = self.mur.row(q);
		if chains.is_empty() {
			return;
		}
		let (s, r) = (self.s, q + PAD);
		// The rows from the rings above the first grid row, or below the last, to the row.
		let (lo, hi) = (if q == 0 { 0 } else { r }, if q + 1 == self.gh { r + PAD + 1 } else { r + 1 });
		// SAFETY: the planes outlive the step. Only row `q`'s units write these rows of either plane (the rings above
		// the first grid row and below the last count as theirs), the tiling runs no unit writing the field of level
		// `j - 1` on row `q` while this one runs, and no other unit touches row `q` of level `j`'s plane.
		let (u, prev) =
			unsafe { (self.planes[(j - 1) % 2].get(lo * s..hi * s), self.planes[j % 2].get_mut(lo * s..hi * s)) };
		let base = lo * s;
		for m in chains {
			let [a, b, c] = m.weights;
			let (inner, [r1, r2, r3]) = (m.inner - base, m.rings.map(|cell| cell - base));
			let (edge, first) = (prev[inner], a * u[inner] + b * prev[inner] + c * u[r1]);
			prev[r1] = first;
			prev[r2] = 2.0 * first - edge;
			prev[r3] = 3.0 * first - 2.0 * edge;
		}
	}
}
