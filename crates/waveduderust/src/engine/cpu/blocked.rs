//! A step with its sub-steps fused into one sweep over the rows (module docs of engine/cpu, Blocked steps).
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
//! The 4th order splits the update in two: `W(q, j)` writes `w = c2 L u` of level `j - 1` into row `q` of `w`, with
//! a rim xi of y, `delta_y` and the x part of the rim's first application, then adds the forcing of sub-step
//! `j - 1` to the row's driven cells; `D(q, j)` takes, with a rim, xi of y and `delta_y` of the second application
//! (on `w`), the outer update of row `q` from level `j - 1` and `w` (the intensity stages on level `n`), the second
//! application's x part, and Mur's condition as above. With a rim, psi of y is a unit per application: on the field
//! before `W`, on `w` before `D`.
//!
//! Each unit reads and writes the state of its own row and reads its neighbours': `D` reads the field of level `j - 1`
//! within 1 row, or 3 with a rim (`delta_y`'s second derivative), and psi of y within 2 rows; `Y` reads the field
//! within 2 rows (a first derivative; it is handed 7 rows, of which the outer two are unused). Level `j` is written in
//! place over level `j - 2` (the planes alternate), so a unit also waits for the readers of what it overwrites. The 4th
//! order's `W` reads the field within 3 rows (its stencil, `delta_y`), `D` reads `w` within 3 rows and the field of
//! its own row; `w` of level `j` is written over level `j - 1`. [`kinds`] lists these reads, and [`Tiling`] derives
//! from them which units a band of rows can run without its neighbours, and the order.
//!
//! `w` lives in two places. A seam's units write and read the plane `w`, one row per grid row for all levels, which
//! is what [`kinds`] describes. A band's units keep `w` of each level in a ring of rows ([`Rings`]), its slot reused
//! by `W` of the same level some rows on, which [`Tiling`] sizes as one more overwrite ([`RingShape`]); `W` also
//! copies to the plane the rows of `w` the seams' units read, once the rim's correction and the forcing have completed
//! them.

use std::ops::Range;

use rayon::prelude::*;

use super::super::forcing::live;
use super::super::mur::MurChain;
use super::super::pml::{RimMemory, RimTables, XLine, XPass, XScratch};
use super::super::shared::blend;
use super::super::stencil::{PAD, TimeScheme, centred};
use super::backend::{Intensity, RowKernels};
use super::rows::Rows;
use super::{DrpLine, NineLine};

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

/// A kind of unit of a blocked step: what it computes, what it reads and how long what it writes lives. See the module
/// docs.
struct Kind {
	unit: Unit,
	/// The levels its output lives: a unit of level `j` writes over the value of level `j - keeps` of its row.
	keeps: usize,
	/// The rows each side of its own whose slices it takes from the buffers it reads.
	span: usize,
	reads: Vec<Read>,
	/// Rows ahead of the sweep's front of its level at which it runs (derived).
	offset: usize,
}

/// What a unit `X(q, j)` reads: the output of the kind `of` of level `j - back`, on the rows within `reach` of `q`.
#[derive(Clone, Copy)]
struct Read {
	of: usize,
	back: usize,
	reach: usize,
}

/// The units of a blocked step: one kind each, run in this order within a tick of the sweep.
fn kinds(scheme: TimeScheme, rim: bool) -> Vec<Kind> {
	let kind = |unit, keeps, span, reads: &[(usize, usize, usize)]| Kind {
		unit,
		keeps,
		span,
		reads: reads.iter().map(|&(of, back, reach)| Read { of, back, reach }).collect(),
		offset: 0,
	};
	match (scheme, rim) {
		// D: the field within 1 row, the older level of its row.
		(TimeScheme::Leapfrog, false) => vec![kind(Unit::Update, 2, 1, &[(0, 1, 1), (0, 2, 0)])],
		// Y: the field within 2 rows (7 rows handed). D: psi of y within 2 rows, the field within 3.
		(TimeScheme::Leapfrog, true) => {
			vec![kind(Unit::Psi(0), 1, 3, &[(1, 1, 2)]), kind(Unit::Update, 2, 3, &[(0, 0, 2), (1, 1, 3), (1, 2, 0)])]
		}
		// W: the field within 3 rows. D: `w` within 3 rows, the field of its row, the older level of its row.
		(TimeScheme::ModifiedEquation, false) => {
			vec![kind(Unit::Lap, 1, 3, &[(1, 1, 3)]), kind(Unit::Update, 2, 3, &[(0, 0, 3), (1, 1, 0), (1, 2, 0)])]
		}
		// Yu: the field within 2 rows. W: the field within 3 (the stencil, `delta_y`), psi of y on it within 2. Yw: `w`
		// within 2 rows. D: `w` within 3 rows (the stencil, `delta_y`), psi of y on it within 2, the field of its row,
		// the older level of its row.
		(TimeScheme::ModifiedEquation, true) => vec![
			kind(Unit::Psi(0), 1, 3, &[(3, 1, 2)]),
			kind(Unit::Lap, 1, 3, &[(3, 1, 3), (0, 0, 2)]),
			kind(Unit::Psi(1), 1, 3, &[(1, 0, 2)]),
			kind(Unit::Update, 2, 3, &[(1, 0, 3), (2, 0, 2), (3, 1, 0), (3, 2, 0)]),
		],
	}
}

/// What a unit computes (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unit {
	/// psi of y of the rim's application `set` on a y-strip row: of the field of level `j - 1` (set 0) or of `w` of
	/// level `j` (set 1, the 4th order).
	Psi(usize),
	/// The 4th order's `w` of level `j`.
	Lap,
	/// The row's update to level `j`, then Mur's condition.
	Update,
}

/// Which units of a blocked step of `n` levels each part runs, and in what order. See the module docs.
///
/// A band of rows `a..b` runs, in a first parallel phase, the units whose inputs and whose overwritten values' readers
/// all lie in the band: at level `j` the units of kind `k` on `a + s_k(j)..b - s_k(j)`, shrunk only at a side where
/// another band lies. A second parallel phase runs the rest, the units of `b - s_k(j)..b + s_k(j)` around each
/// boundary `b` between bands. Bands of at least [`min_band`](Self::min_band) rows keep any two boundaries' units off
/// each other's rows.
///
/// The shrinks follow from the kinds' reads ([`kinds`]). If `X(q, j)` reads the output of `P` of level `j - back`
/// within `reach` rows, `X` runs in the band only where `P` does: `s_X(j) >= s_P(j - back) + reach`. And `P(q, j)`
/// overwrites the value of level `j - keeps_P` of its row, which that read takes at level `j' = j - keeps_P + back`,
/// so `P` runs in the band only where those readers do: `s_P(j) >= s_X(j') + reach`. Each shrink is the least that
/// meets both, level by level and kind by kind in the tick's order.
///
/// Within a part the units run in one sweep: at tick `t`, for each level `j` in turn and each kind `k` in order, the
/// unit of row `f + o_k` with `f = t - (j - 1) lag`. A read of the same level is of a kind earlier in the tick, so its
/// offset `o_P >= o_X + reach` gives it the rows `X` reads; a read of `back` levels down needs
/// `back lag + o_P >= o_X + reach`, and an overwrite `d = keeps_P - back` levels above its readers
/// `d lag + o_X >= o_P + reach`. `lag` is the least that meets them all: 1 for the 9-point, 4 with a rim; 6 for the
/// 4th order, 8 with a rim. The update's shrink comes out as `sd(j) = j - 1` for the 9-point, `4 j - 2` with a rim;
/// `6 j - 3` for the 4th order, `8 j - 2` with a rim (the tests below).
pub(super) struct Tiling {
	n: usize,
	scheme: TimeScheme,
	rim: bool,
	kinds: Vec<Kind>,
	lag: usize,
	/// The farthest any unit takes rows from its own.
	rho: usize,
	/// `s_k(j)` at `shrink[k][j - 1]`.
	shrink: Vec<Vec<usize>>,
	/// The 4th order's rings of `w` rows: the slots, the rows each side of its own a reader of `w` takes, and by level
	/// the reach of the seams' readers into a band ([`Ring`]); None for the 9-point.
	ring: Option<RingShape>,
}

/// The shape of a band's rings of `w` rows, one ring per level (derived by [`Tiling::new`]; module docs of
/// engine/cpu, Blocked steps).
///
/// Row `q` of level `j` lives in slot `q % slots` of level `j`'s ring. `W(q + slots, j)` reuses the slot, an overwrite
/// of `w` of level `j` by a unit of the same level `slots` rows on: a unit `X` reading `w` within `reach` rows of its
/// own reads row `q` last at the sweep's front `f = q + reach - o_X`, and `W(q + slots, j)` writes at `f = q + slots -
/// o_W`, before `X` within a tick, so `slots >= o_W - o_X + reach + 1` for every such read: 7 slots for the 4th order,
/// 8 with a rim. A reader takes `2 span + 1` consecutive rows as one slice, so each ring holds `2 span` more rows,
/// copies of its first slots' (rows `q` with `q % slots < 2 span` are written twice).
#[derive(Clone, Debug, PartialEq, Eq)]
struct RingShape {
	slots: usize,
	span: usize,
	/// By level, `max(s_X(j) + reach)` over the units `X` reading `w`: a seam's units read `w` of level `j` up to this
	/// far into the band at its side, on the rows the band's `W` writes from `s_W(j)` on, so the band writes those rows
	/// to the plane `w` too. They are disjoint across levels (`s_W(j + 1) >= s_X(j) + reach`, `W`'s overwrite of the
	/// plane), so the plane holds them all for the seams.
	seam: Vec<usize>,
}

impl Tiling {
	pub(super) fn new(n: usize, scheme: TimeScheme, rim: bool) -> Self {
		let mut kinds = kinds(scheme, rim);
		// The offsets, from the tick's last kind back.
		for k in (0..kinds.len()).rev() {
			let offset = (k + 1..kinds.len())
				.flat_map(|x| kinds[x].reads.iter().filter(|r| r.of == k && r.back == 0).map(move |r| (x, r.reach)))
				.map(|(x, reach)| kinds[x].offset + reach)
				.max()
				.unwrap_or(0);
			kinds[k].offset = offset;
		}
		let mut lag = 1;
		let need = |levels: usize, over: usize| over.div_ceil(levels);
		for (x, kind) in kinds.iter().enumerate() {
			for &Read { of, back, reach } in &kind.reads {
				let (o_x, o_p) = (kind.offset, kinds[of].offset);
				if back == 0 {
					assert!(of < x && o_p >= o_x + reach, "a read of the same level comes earlier in the tick");
				} else {
					lag = lag.max(need(back, (o_x + reach).saturating_sub(o_p)));
				}
				match kinds[of].keeps - back {
					0 => assert!(of == x && reach == 0, "only a unit's own row of what it overwrites"),
					d => lag = lag.max(need(d, (o_p + reach).saturating_sub(o_x))),
				}
			}
		}
		let mut shrink = vec![Vec::with_capacity(n); kinds.len()];
		for j in 1..=n {
			for k in 0..kinds.len() {
				let mut s = 0;
				for &Read { of, back, reach } in &kinds[k].reads {
					if back < j && !(of == k && back == 0) {
						s = s.max(shrink[of][j - back - 1] + reach);
					}
				}
				for (x, kind) in kinds.iter().enumerate() {
					for read in kind.reads.iter().filter(|r| r.of == k) {
						let level = (j + read.back).checked_sub(kinds[k].keeps).filter(|&l| l >= 1);
						match level {
							Some(l) if l < j => s = s.max(shrink[x][l - 1] + read.reach),
							Some(_) if x != k => s = s.max(shrink[x][j - 1] + read.reach),
							_ => {}
						}
					}
				}
				shrink[k].push(s);
			}
		}
		let rho = kinds.iter().map(|k| k.span).max().unwrap_or(0);
		let ring = kinds.iter().position(|k| k.unit == Unit::Lap).map(|lap| {
			let readers: Vec<(usize, usize)> = (0..kinds.len())
				.flat_map(|x| kinds[x].reads.iter().filter(|r| r.of == lap && r.back == 0).map(move |r| (x, r.reach)))
				.collect();
			let o_w = kinds[lap].offset;
			let slots = readers.iter().map(|&(x, reach)| o_w + reach - kinds[x].offset + 1).max().unwrap_or(1);
			let span = readers.iter().map(|&(x, _)| kinds[x].span).max().unwrap_or(0);
			assert!(slots > 2 * span, "a reader's rows lie in distinct slots");
			let seam = (0..n).map(|j| readers.iter().map(|&(x, reach)| shrink[x][j] + reach).max().unwrap_or(0));
			RingShape { slots, span, seam: seam.collect() }
		});
		Self { n, scheme, rim, kinds, lag, rho, shrink, ring }
	}

	/// Whether this is the tiling of `n` levels of `scheme` with or without a rim.
	pub(super) fn is(&self, n: usize, scheme: TimeScheme, rim: bool) -> bool {
		(self.n, self.scheme, self.rim) == (n, scheme, rim)
	}

	/// The widest side of a band's trapezoid: the most rows any unit of a band stays from a side facing another band.
	pub(super) fn side(&self) -> usize {
		self.shrink.iter().flatten().copied().max().unwrap_or(0)
	}

	/// The fewest rows of a band.
	pub(super) fn min_band(&self) -> usize {
		2 * self.side() + self.rho
	}

	/// Runs `unit(u, q, j)` on the units `rows(k, j)` of each kind `k` in the sweep's order.
	fn sweep(&self, rows: impl Fn(usize, usize) -> Range<usize>, mut unit: impl FnMut(Unit, usize, usize)) {
		let (mut lo, mut hi) = (usize::MAX, 0);
		for r in (1..=self.n).flat_map(|j| (0..self.kinds.len()).map(move |k| (k, j))).map(|(k, j)| rows(k, j)) {
			if !r.is_empty() {
				(lo, hi) = (lo.min(r.start), hi.max(r.end));
			}
		}
		if lo >= hi {
			return;
		}
		let ahead = self.kinds.iter().map(|k| k.offset).max().unwrap_or(0);
		for t in lo as isize - ahead as isize..(hi + (self.n - 1) * self.lag) as isize {
			for j in 1..=self.n {
				let f = t - ((j - 1) * self.lag) as isize;
				for (k, kind) in self.kinds.iter().enumerate() {
					let q = f + kind.offset as isize;
					if q >= 0 && rows(k, j).contains(&(q as usize)) {
						unit(kind.unit, q as usize, j);
					}
				}
			}
		}
	}

	/// The first phase's units of the band `rows`, `inner` telling whether another band lies above and below it.
	pub(super) fn band(&self, rows: Range<usize>, inner: (bool, bool), unit: impl FnMut(Unit, usize, usize)) {
		let within = |shrink: usize| {
			let (top, bottom) = (if inner.0 { shrink } else { 0 }, if inner.1 { shrink } else { 0 });
			(rows.start + top)..rows.end.saturating_sub(bottom).max(rows.start + top)
		};
		self.sweep(|k, j| within(self.shrink[k][j - 1]), unit);
	}

	/// The second phase's units around the boundary `b` between two bands.
	pub(super) fn seam(&self, b: usize, unit: impl FnMut(Unit, usize, usize)) {
		self.sweep(|k, j| b - self.shrink[k][j - 1]..b + self.shrink[k][j - 1], unit);
	}
}

/// The 4th order's rings of `w` rows of a blocked step's bands, kept from step to step ([`RingShape`]): per band, a
/// ring of `slots + 2 span` rows of `s` values per level, then `2 span + 1` rows for a reader's rows across the area's
/// edge.
///
/// `W` writes the area's columns of a row, and the rim's x part adds +0 to the cells of its strips beyond them, so a
/// ring's other columns hold +0 as the plane `w` does there, once cleared, for as long as the steps' columns cover the
/// columns of the steps before (they do until a reset or another scene).
#[derive(Default)]
pub(super) struct Rings {
	bands: Vec<Vec<f32>>,
	/// The columns the rings may hold values other than +0 in: those of the steps since they were cleared.
	xs: Range<usize>,
}

impl Rings {
	/// The rings of `count` bands of `len` values each, for a step on the columns `xs`: +0 beyond `xs`.
	fn prepare(&mut self, count: usize, len: usize, xs: &Range<usize>) -> &mut [Vec<f32>] {
		let within = self.xs.is_empty() || (xs.start <= self.xs.start && self.xs.end <= xs.end);
		if !within {
			self.bands.iter_mut().for_each(|band| band.fill(0.0));
		}
		self.xs = xs.clone();
		if self.bands.len() < count {
			self.bands.resize_with(count, Vec::new);
		}
		for band in &mut self.bands[..count] {
			if band.len() != len {
				band.clear();
				band.resize(len, 0.0);
			}
		}
		&mut self.bands[..count]
	}
}

/// A band's rings of `w` rows in a blocked step ([`Rings`]).
struct Ring<'r> {
	rows: &'r mut [f32],
	shape: &'r RingShape,
	s: usize,
	/// The band's rows, and whether another band lies above and below it.
	band: Range<usize>,
	inner: (bool, bool),
}

impl Ring<'_> {
	/// Where level `j`'s ring starts.
	fn level(&self, j: usize) -> usize {
		(j - 1) * (self.shape.slots + 2 * self.shape.span) * self.s
	}

	/// The slot of row `q` of level `j`.
	fn slot(&self, q: usize, j: usize) -> Range<usize> {
		let at = self.level(j) + q % self.shape.slots * self.s;
		at..at + self.s
	}

	/// Row `q` of `w` of level `j`, for `W(q, j)` to write; [`written`](Self::written) once written.
	fn row_mut(&mut self, q: usize, j: usize) -> &mut [f32] {
		let slot = self.slot(q, j);
		&mut self.rows[slot]
	}

	/// Copies row `q` of level `j`, just written, to the slot's copy past the ring's end, if it has one; returns whether
	/// a seam's units read the row, which the band then writes to the plane.
	fn written(&mut self, q: usize, j: usize) -> bool {
		let (slot, extra) = (self.slot(q, j), 2 * self.shape.span);
		if q % self.shape.slots < extra {
			let copy = slot.start + self.shape.slots * self.s;
			self.rows.copy_within(slot, copy);
		}
		let reach = self.shape.seam[j - 1];
		(self.inner.0 && q < self.band.start + reach) || (self.inner.1 && q + reach >= self.band.end)
	}

	/// The rows `q - span` to `q + span` of `w` of level `j`, as one slice: the ring's on the rows of `area`, which the
	/// band's `W` has written once a reader of `q` runs, +0 elsewhere, as in the plane.
	fn rows(&mut self, q: usize, j: usize, area: &Range<usize>) -> &[f32] {
		let (span, s) = (self.shape.span, self.s);
		let len = (2 * span + 1) * s;
		if q >= area.start + span && q + span < area.end {
			let at = self.slot(q - span, j).start;
			return &self.rows[at..at + len];
		}
		let window = self.rows.len() - len;
		for k in 0..2 * span + 1 {
			let at = window + k * s;
			match (q + k).checked_sub(span).filter(|r| area.contains(r)) {
				Some(r) => {
					let slot = self.slot(r, j);
					self.rows.copy_within(slot, at);
				}
				None => self.rows[at..at + s].fill(0.0),
			}
		}
		&self.rows[window..]
	}
}

/// What a thread's units of one part of a blocked step hold: the rim's scratch, and in a band of the 4th order the
/// band's rings of `w` rows (the seams' units take `w` from the plane).
struct Lane<'l> {
	scratch: &'l mut Option<XScratch>,
	ring: Option<Ring<'l>>,
}

/// The rim's state for a blocked step: the tables, z~ of each grid row, and the memory variables of each application
/// with its pass's columns and scale (one application for the 9-point, two for the 4th order).
pub(super) struct Rim<'a> {
	pub(super) tables: RimTables<'a>,
	pub(super) z: Shared,
	pub(super) sets: Vec<RimSet<'a>>,
}

/// One application's memory variables in a blocked step, and the pass it corrects.
pub(super) struct RimSet<'a> {
	pub(super) psi_y: Shared,
	pub(super) xi_y: Shared,
	pub(super) delta: Shared,
	pub(super) psi_x: Shared,
	pub(super) xi_x: Shared,
	pub(super) pass: XPass<'a>,
}

impl<'a> RimSet<'a> {
	/// The set of `memory`, correcting `pass`.
	pub(super) fn of(memory: RimMemory<'_>, pass: XPass<'a>) -> Self {
		Self {
			psi_y: Shared::of(memory.psi_y),
			xi_y: Shared::of(memory.xi_y),
			delta: Shared::of(memory.delta),
			psi_x: Shared::of(memory.psi_x),
			xi_x: Shared::of(memory.xi_x),
			pass,
		}
	}
}

/// How a blocked step updates a row: the 9-point leapfrog, or the 4th order's two passes through `w`, the plane given
/// here and the bands' rings ([`Rings`]).
pub(super) enum Line<'a, B> {
	Nine(NineLine<'a, B>),
	Drp(DrpLine<'a, B>, Shared),
}

/// Where a blocked sweep records the probe readings after each of its steps: `width` values a reading, the readings
/// one after another in `out`, the cells by padded cell with their place in a reading and whether they read the
/// intensity (blended with `scale`).
pub(super) struct ProbeLog<'a> {
	pub(super) cells: &'a [(usize, u32, bool)],
	pub(super) out: Shared,
	pub(super) width: usize,
	pub(super) scale: f32,
}

/// A blocked sweep of one or more steps: what its units read and write. Level `j` lives in `planes[j % 2]`.
pub(super) struct Step<'a, B> {
	/// The backend; each unit runs through its row entry.
	pub(super) kernels: B,
	pub(super) s: usize,
	pub(super) gw: usize,
	pub(super) gh: usize,
	/// The levels: the sub-steps of all the steps.
	pub(super) n: usize,
	/// The sub-steps of a step: level `j` ends a step when `j % sub == 0`.
	pub(super) sub: usize,
	/// The planes `u` and `prev` of the step's start.
	pub(super) planes: [Shared; 2],
	/// The intensity stages and their weight, advanced on the last level of each step when given.
	pub(super) stages: Option<(Shared, Shared, f32)>,
	pub(super) alpha: f32,
	pub(super) line: Line<'a, B>,
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
	/// Where the probe readings after each step go, when they are taken.
	pub(super) probes: Option<ProbeLog<'a>>,
}

impl<B: RowKernels> Step<'_, B> {
	/// The grid rows with units: the area's, and every row when Mur's chains run.
	fn rows(&self) -> Range<usize> {
		if self.mur.is_empty() { self.area.clone() } else { 0..self.gh }
	}

	/// Runs the step's units with the threads of the rayon pool: the bands, then the seams between them ([`Tiling`]).
	pub(super) fn run(&self, tiling: &Tiling, bands: usize, rings: &mut Rings) {
		let (rows, area, s) = (self.rows(), &self.area, self.s);
		let count = bands.clamp(1, (area.len() / tiling.min_band()).max(1));
		// Band k starts at row start(k): the area's rows split evenly, the rows beyond it to the outer bands.
		let start = |k: usize| match k {
			0 => rows.start,
			k if k == count => rows.end,
			k => area.start + k * area.len() / count,
		};
		let scratch = || self.rim.as_ref().map(|rim| rim.tables.scratch());
		// The 4th order's bands keep `w` in rings of rows.
		let shape = tiling.ring.as_ref().filter(|_| matches!(self.line, Line::Drp(..)));
		let bufs = match (shape, &self.line) {
			(Some(shape), Line::Drp(line, _)) => {
				let len = (self.n * (shape.slots + 2 * shape.span) + 2 * shape.span + 1) * s;
				rings.prepare(count, len, line.xs)
			}
			_ => &mut [],
		};
		let band = |scratch: &mut Option<XScratch>, k: usize, buf: Option<&mut Vec<f32>>| {
			let (band, inner) = (start(k)..start(k + 1), (k > 0, k + 1 < count));
			let ring = buf.zip(shape).map(|(rows, shape)| Ring { rows, shape, s, band: band.clone(), inner });
			let mut lane = Lane { scratch, ring };
			tiling.band(band, inner, |unit, q, j| self.unit(unit, q, j, &mut lane));
		};
		if count == 1 {
			return band(&mut scratch(), 0, bufs.first_mut());
		}
		if bufs.is_empty() {
			(0..count).into_par_iter().for_each_init(scratch, |scratch, k| band(scratch, k, None));
		} else {
			bufs.par_iter_mut().enumerate().for_each_init(scratch, |scratch, (k, buf)| band(scratch, k, Some(buf)));
		}
		(1..count).into_par_iter().for_each_init(scratch, |scratch, k| {
			let mut lane = Lane { scratch, ring: None };
			tiling.seam(start(k), |unit, q, j| self.unit(unit, q, j, &mut lane));
		});
	}

	/// Unit `unit` of row `q` and level `j`.
	fn unit(&self, unit: Unit, q: usize, j: usize, lane: &mut Lane<'_>) {
		self.kernels.run_row(
			#[cfg_attr(cpu_wide, inline(always))]
			|| match unit {
				Unit::Psi(set) => self.psi(set, q, j, lane),
				Unit::Lap => {
					if self.area.contains(&q) {
						self.lap(q, j, lane);
					}
				}
				Unit::Update => {
					if self.area.contains(&q) {
						match &self.line {
							Line::Nine(line) => self.leapfrog(line, q, j, lane.scratch),
							Line::Drp(line, w) => self.outer(line, *w, q, j, lane),
						}
					}
					self.open_edges(q, j);
				}
			},
		);
	}

	/// The buffer application `set` of the rim takes the derivatives of on level `j`: the field of level `j - 1`, or
	/// the 4th order's `w`.
	fn field(&self, set: usize, j: usize) -> Shared {
		match (&self.line, set) {
			(Line::Drp(_, w), 1) => *w,
			_ => self.planes[(j - 1) % 2],
		}
	}

	/// psi of y of application `set` on y-strip row `q` of the area; nothing on other rows. A band's units of the 4th
	/// order's second application take `w` from the band's ring.
	#[cfg_attr(cpu_wide, inline(always))]
	fn psi(&self, set: usize, q: usize, j: usize, lane: &mut Lane<'_>) {
		let Some(rim) = &self.rim else { return };
		let Some(k) = rim.tables.strip_row(q).filter(|_| self.area.contains(&q)) else { return };
		let (s, gw, r, memory) = (self.s, self.gw, q + PAD, &rim.sets[set]);
		let field = match (set, lane.ring.as_mut()) {
			(1, Some(ring)) => ring.rows(q, j, &self.area),
			// SAFETY: the planes and `w` outlive the step. The tiling runs no unit that writes the rows within 3 of `q`
			// of the buffer `set` reads while this one runs.
			_ => unsafe { self.field(set, j).get((r - 3) * s..(r + 4) * s) },
		};
		// SAFETY: the CPML's memory outlives the step; no other unit touches psi of y of row `q` while this one runs.
		let line = unsafe { memory.psi_y.get_mut(k * gw..(k + 1) * gw) };
		rim.tables.psi_y_line(q, field, self.cuts, &memory.pass.xs, line);
	}

	/// xi of y and `delta_y` of application `set` on row `q` when it is a y-strip row, from the 7 rows `field` around
	/// it; the row's y-strip row.
	#[cfg_attr(cpu_wide, inline(always))]
	fn delta(&self, set: usize, q: usize, field: &[f32]) -> Option<usize> {
		let rim = self.rim.as_ref()?;
		let k = rim.tables.strip_row(q)?;
		let (gw, memory) = (self.gw, &rim.sets[set]);
		let psi = |dk: isize| match rim.tables.neighbour_row(k, dk) {
			// SAFETY: the CPML's memory outlives the step; the tiling runs no unit writing psi of y of application
			// `set` within 2 rows of `q`, or xi of y and `delta_y` of row `q`, while this one runs.
			Some(row) => unsafe { memory.psi_y.get(row * gw..(row + 1) * gw) },
			None => &rim.tables.zeros()[..gw],
		};
		// SAFETY: as above.
		let (xi, delta) =
			unsafe { (memory.xi_y.get_mut(k * gw..(k + 1) * gw), memory.delta.get_mut(k * gw..(k + 1) * gw)) };
		rim.tables.delta_y_line(q, field, self.cuts, &memory.pass.xs, [psi(1), psi(-1), psi(2), psi(-2)], xi, delta);
		Some(k)
	}

	/// The x part of application `set` on row `q`, `line` its grid cells: psi of x and the correction from the 3 rows
	/// `field` from the one above, and from the row's `delta_y` when `strip` gives its y-strip row.
	#[cfg_attr(cpu_wide, inline(always))]
	fn x_part(
		&self,
		set: usize,
		q: usize,
		field: &[f32],
		strip: Option<usize>,
		line: &mut [f32],
		scratch: &mut Option<XScratch>,
	) {
		let (Some(rim), Some(scratch)) = (&self.rim, scratch.as_mut()) else { return };
		let (gw, memory) = (self.gw, &rim.sets[set]);
		let (w, zw) = (2 * rim.tables.rim(), rim.tables.z_width());
		// SAFETY: the CPML's memory outlives the step; `delta_y` of application `set` of row `q`, its x part's memory
		// and z~ of row `q` are this unit's alone while it runs.
		let (delta, memory) = unsafe {
			let delta = strip.map(|k| memory.delta.get(k * gw..(k + 1) * gw));
			let x = XLine {
				psi: memory.psi_x.get_mut(q * w..(q + 1) * w),
				xi: memory.xi_x.get_mut(q * w..(q + 1) * w),
				z: rim.z.get_mut(q * zw..(q + 1) * zw),
			};
			(delta, x)
		};
		rim.tables.x_line(q, field, delta, memory, line, scratch, &rim.sets[set].pass);
	}

	/// The intensity stages of row `q` on the last level of a step.
	///
	/// # Safety
	///
	/// Only the update of row `q` to level `j` may call it, once, and nothing else may hold row `q` of the stages
	/// while the slices live.
	#[inline(always)]
	#[allow(clippy::mut_from_ref)]
	unsafe fn row_stages(&self, q: usize, j: usize) -> Option<(&mut [f32], &mut [f32])> {
		let (s, r) = (self.s, q + PAD);
		self.stages.filter(|_| j.is_multiple_of(self.sub)).map(|(s1, s2, _)| {
			// SAFETY: the stages outlive the step, and the caller is the one update of row `q` to a step's last level,
			// which runs after the update to the step's last level before on the row (it reads that field).
			unsafe { (s1.get_mut(r * s..(r + 1) * s), s2.get_mut(r * s..(r + 1) * s)) }
		})
	}

	/// Records the probe cells of row `q` after the update to level `j` when it ends a step: the field from `next` (the
	/// padded row of level `j`), the intensity from the row's `stages` (0 when they are not tracked).
	#[inline(always)]
	fn record(&self, q: usize, j: usize, next: &[f32], stages: Option<(&[f32], &[f32])>) {
		let Some(log) = self.probes.as_ref().filter(|_| j.is_multiple_of(self.sub)) else { return };
		let at = (q + PAD) * self.s;
		let first = log.cells.partition_point(|&(c, ..)| c < at);
		let reading = (j / self.sub - 1) * log.width;
		for &(cell, slot, intensity) in log.cells[first..].iter().take_while(|&&(c, ..)| c < at + self.s) {
			let x = cell - at;
			let value = match (intensity, stages) {
				(false, _) => next[x],
				(true, Some((s1, s2))) => blend(s1[x], s2[x], log.scale),
				(true, None) => 0.0,
			};
			let slot = reading + slot as usize;
			// SAFETY: the readings outlive the step; each value of a reading is of one cell, which only the update of
			// its row to the step's last level records.
			let out = unsafe { log.out.get_mut(slot..slot + 1) };
			out[0] = value;
		}
	}

	/// The 9-point's update of row `q` of the area to level `j`: `delta_y`, the leapfrog update, the rim's x part and
	/// the forcing.
	#[cfg_attr(cpu_wide, inline(always))]
	fn leapfrog(&self, line: &NineLine<'_, B>, q: usize, j: usize, scratch: &mut Option<XScratch>) {
		let (s, gw, r) = (self.s, self.gw, q + PAD);
		let (src, dst) = (self.planes[(j - 1) % 2], self.planes[j % 2]);
		// The rim's y stages of a y-strip row read the field of level `j - 1` within 3 rows.
		let strip = self.rim.as_ref().and_then(|_| {
			// SAFETY: the planes outlive the step; the tiling runs no unit writing the field of level `j - 1` within 3
			// rows of `q` while this one runs.
			self.delta(0, q, unsafe { src.get((r - 3) * s..(r + 4) * s) })
		});
		// SAFETY: the planes outlive the step. The tiling runs no unit writing the field of level `j - 1` within 1 row
		// of `q` while this one runs, and no other unit touches row `q` of level `j`'s plane.
		let (u, next) = unsafe { (src.get((r - 1) * s..(r + 2) * s), dst.get_mut(r * s..(r + 1) * s)) };
		// SAFETY: this is the update of row `q` to level `j`, which the tiling runs once and alone on the row.
		let mut stages = unsafe { self.row_stages(q, j) };
		let alpha = self.alpha;
		let intensity =
			stages.as_mut().map(|(s1, s2)| Intensity { s1: &mut s1[PAD..][..gw], s2: &mut s2[PAD..][..gw], alpha });
		line.row(r, u, &mut next[PAD..][..gw], intensity);
		self.x_part(0, q, u, strip, &mut next[PAD..][..gw], scratch);
		self.force(q, j, next, stages.as_mut().map(|(s1, s2)| (&mut **s1, &mut **s2)));
		self.record(q, j, next, stages.as_ref().map(|(s1, s2)| (&**s1, &**s2)));
	}

	/// The 4th order's `W(q, j)` on a row of the area: `w = c2 L u` from the field of level `j - 1`, with the rim's
	/// first application (`delta_y` and the x part), then the forcing of sub-step `j - 1` on its driven cells.
	///
	/// A band writes `w` into its ring, and the rows a seam's units read to the plane too; a seam into the plane.
	#[cfg_attr(cpu_wide, inline(always))]
	fn lap(&self, q: usize, j: usize, lane: &mut Lane<'_>) {
		let Line::Drp(line, w) = &self.line else { return };
		let (s, gw, r) = (self.s, self.gw, q + PAD);
		let Lane { scratch, ring } = lane;
		// SAFETY: the planes outlive the step; the tiling runs no unit writing the field of level `j - 1` within 3 rows
		// of `q` while this one runs.
		let u = unsafe { self.planes[(j - 1) % 2].get((r - 3) * s..(r + 4) * s) };
		let out = match ring.as_mut() {
			Some(ring) => ring.row_mut(q, j),
			// SAFETY: `w` outlives the step, and no other unit touches row `q` of `w` while this one runs.
			None => unsafe { w.get_mut(r * s..(r + 1) * s) },
		};
		let strip = self.delta(0, q, u);
		let rows: Rows<'_> = std::array::from_fn(|k| &u[k * s..][..s]);
		line.lap(r, &rows, &mut out[PAD..][..gw]);
		self.x_part(0, q, &u[2 * s..5 * s], strip, &mut out[PAD..][..gw], scratch);
		self.force_w(q, j, out);
		if let Some(ring) = ring.as_mut()
			&& ring.written(q, j)
		{
			let slot = ring.slot(q, j);
			// SAFETY: `w` outlives the step. In the band phase each band's `W` units write only their own rows of
			// the plane, which no other band's touch, and nothing reads it.
			unsafe { w.get_mut(r * s..(r + 1) * s) }.copy_from_slice(&ring.rows[slot]);
		}
	}

	/// The 4th order's update of row `q` of the area to level `j` from `w`: `delta_y` of the rim's second application,
	/// the outer update (the intensity stages on level `n`), the x part.
	///
	/// A band takes `w` from its ring, a seam from the plane.
	#[cfg_attr(cpu_wide, inline(always))]
	fn outer(&self, line: &DrpLine<'_, B>, w: Shared, q: usize, j: usize, lane: &mut Lane<'_>) {
		let (s, gw, r) = (self.s, self.gw, q + PAD);
		let (src, dst) = (self.planes[(j - 1) % 2], self.planes[j % 2]);
		let Lane { scratch, ring } = lane;
		let wr = match ring.as_mut() {
			Some(ring) => ring.rows(q, j, &self.area),
			// SAFETY: `w` outlives the step; the tiling runs no unit writing `w` within 3 rows of `q` while this one
			// runs.
			None => unsafe { w.get((r - 3) * s..(r + 4) * s) },
		};
		// SAFETY: the planes outlive the step. The tiling runs no unit writing row `q` of the field of level `j - 1`
		// while this one runs, and no other unit touches row `q` of level `j`'s plane.
		let (u, next) = unsafe { (src.get(r * s..(r + 1) * s), dst.get_mut(r * s..(r + 1) * s)) };
		let strip = self.delta(1, q, wr);
		// SAFETY: this is the update of row `q` to level `j`, which the tiling runs once and alone on the row.
		let mut stages = unsafe { self.row_stages(q, j) };
		let alpha = self.alpha;
		let intensity =
			stages.as_mut().map(|(s1, s2)| Intensity { s1: &mut s1[PAD..][..gw], s2: &mut s2[PAD..][..gw], alpha });
		let rows: Rows<'_> = std::array::from_fn(|k| &wr[k * s..][..s]);
		line.outer(r, &u[PAD..][..gw], &rows, &mut next[PAD..][..gw], intensity);
		self.x_part(1, q, &wr[2 * s..5 * s], strip, &mut next[PAD..][..gw], scratch);
		self.record(q, j, next, stages.as_ref().map(|(s1, s2)| (&**s1, &**s2)));
	}

	/// The driven cells of row `q`, with their runs of table entries.
	#[inline(always)]
	fn driven_in(&self, q: usize) -> impl Iterator<Item = &(usize, u32, u32)> {
		let at = (q + PAD) * self.s;
		let first = self.driven.partition_point(|&(c, ..)| c < at);
		self.driven[first..].iter().take_while(move |&&(c, ..)| c < at + self.s)
	}

	/// Adds `F` of sub-step `j - 1` to the driven cells of row `q` of `w` (the padded row): host.rs's `force_w` on the
	/// row.
	#[cfg_attr(cpu_wide, inline(always))]
	fn force_w(&self, q: usize, j: usize, w: &mut [f32]) {
		let (at, table) = ((q + PAD) * self.s, &self.tables[j - 1]);
		for &(cell, start, len) in self.driven_in(q) {
			let mut live_values = self.entries[start as usize..(start + len) as usize]
				.iter()
				.map(|&e| table[e as usize])
				.filter(|&f| live(f));
			let Some(first) = live_values.next() else { continue };
			let mut v = w[cell - at] + first;
			for f in live_values {
				v += f;
			}
			w[cell - at] = v;
		}
	}

	/// Adds `h F` of sub-step `j - 1` to the driven cells of row `q` of `next` (the padded row), and moves their
	/// intensity stages, when given, to the forced value: host.rs's `force_next` on the row.
	#[cfg_attr(cpu_wide, inline(always))]
	fn force(&self, q: usize, j: usize, next: &mut [f32], mut stages: Option<(&mut [f32], &mut [f32])>) {
		let (at, table, rest) = ((q + PAD) * self.s, &self.tables[j - 1], 1.0 - self.alpha);
		for &(cell, start, len) in self.driven_in(q) {
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
	#[cfg_attr(cpu_wide, inline(always))]
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

#[cfg(test)]
mod tests {
	use super::*;

	/// The shrinks of kind `k` by level.
	fn shrinks(tiling: &Tiling, k: usize) -> Vec<usize> {
		tiling.shrink[k].clone()
	}

	#[test]
	fn the_tilings_derive_the_documented_lags_and_shrinks() {
		let nine = Tiling::new(4, TimeScheme::Leapfrog, false);
		assert_eq!((nine.lag, shrinks(&nine, 0)), (1, vec![0, 1, 2, 3]), "sd(j) = j - 1");
		let nine = Tiling::new(4, TimeScheme::Leapfrog, true);
		assert_eq!((nine.lag, shrinks(&nine, 1)), (4, vec![2, 6, 10, 14]), "sd(j) = 4 j - 2");
		assert_eq!(shrinks(&nine, 0), vec![0, 4, 8, 12]);
		let drp = Tiling::new(4, TimeScheme::ModifiedEquation, false);
		assert_eq!((drp.lag, shrinks(&drp, 0), shrinks(&drp, 1)), (6, vec![0, 6, 12, 18], vec![3, 9, 15, 21]));
		// `w` lives from `W(q)` at offset 3 to `D(q + 3)` at offset 0: 7 slots. The seams read `w` `s_D + 3` rows into
		// a band.
		assert_eq!(drp.ring, Some(RingShape { slots: 7, span: 3, seam: vec![6, 12, 18, 24] }));
		assert!(nine.ring.is_none());
		let drp = Tiling::new(4, TimeScheme::ModifiedEquation, true);
		assert_eq!(drp.lag, 8);
		let offsets: Vec<usize> = drp.kinds.iter().map(|k| k.offset).collect();
		assert_eq!(offsets, vec![6, 4, 2, 0]);
		assert_eq!(shrinks(&drp, 0), vec![0, 8, 16, 24]);
		assert_eq!(shrinks(&drp, 1), vec![2, 10, 18, 26]);
		assert_eq!(shrinks(&drp, 2), vec![4, 12, 20, 28]);
		assert_eq!(shrinks(&drp, 3), vec![6, 14, 22, 30], "sd(j) = 8 j - 2");
		assert_eq!(drp.min_band(), 2 * 30 + 3);
		// `W` at offset 4, `D` reading 3 rows on at 0: 8 slots. The seams read `w` `s_D + 3` rows into a band, 7 past
		// `s_W`.
		assert_eq!(drp.ring, Some(RingShape { slots: 8, span: 3, seam: vec![9, 17, 25, 33] }));
	}
}
