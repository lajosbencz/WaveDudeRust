//! The CPML on the rim (Roden and Gedney 2000) for `u_tt = c^2 lap u`, in the second-order form with memory
//! variables for the stretched derivatives (Komatitsch and Martin 2007; Pasalic and McGarry 2010).
//!
//! Along x the PML replaces `d/dx` by `(1/s) d/dx`, `s = kappa + sigma / (alpha + i omega)`; `1/s` is `1/kappa` plus
//! a causal kernel whose convolution is advanced recursively per sub-step dt:
//!
//! ```text
//! b = exp(-(sigma/kappa + alpha) dt),   a = sigma (b - 1) / (sigma kappa + kappa^2 alpha)
//! psi <- b psi + a D1 f                         psi ~ the kernel's convolution with f_x
//! Q    = (1/kappa) D2 f + (1/kappa)' D1 f + D1 psi    = d/dx (f_x / kappa + psi)
//! xi  <- b xi + a Q
//! O_x f = f_x~x~ = Q / kappa + xi
//! ```
//!
//! D2 is the stencil's second derivative along the axis, D1 the 4th-order central first derivative
//! `(8 (f1 - f-1) - (f2 - f-2)) / 12`, (1/kappa)' the profile's analytic slope. Inside the canvas sigma = 0 and
//! kappa = 1, so `O = D2`. The y axis is the same.
//!
//! Every Laplacian is `D2x z + D2y u` with `z = u + chi D2y u` (chi the cross weight: 1/6 for the 9-point, whose D2
//! is the 3-point difference; 0 for the axis stencils). Both factors are stretched, `lap~ u = O_x z~ + O_y u`,
//! `z~ = u + chi O_y u`; an unstretched mixed term makes the layer unstable. With `delta_y = O_y u - D2y u` the rim's
//! correction to the plain stencil is
//!
//! ```text
//! (O_x z~ - D2x z~) + chi D2x delta_y + delta_y
//! ```
//!
//! which the engine adds times c2 to what each pass over the grid writes, in four stages over the rim: psi of y on u,
//! `delta_y` (stored) and xi of y, psi of x on z~, then the x part and the sum. The first two read only the field and
//! run over the y strips before the pass ([`CpuPml::begin`]); the last two need nothing beyond their row, so the pass
//! runs them on each row right after writing it ([`XShared::row`]), and the result is that of a pass over the rim
//! after the pass over the grid. A scheme applying the operator twice (the modified equation) keeps a second set of
//! memory variables.
//!
//! Storage covers the rim only: `psi` and `xi` of x over the left and right strips (full height, 2 rim columns a
//! row), of y over the top and bottom strips (2 rim rows of full width), x first; `delta_y` follows xi over the y
//! strips.

use std::ops::Range;

use rayon::prelude::*;

use super::damage::Damage;
use super::isa;
use super::stencil::PAD;
use super::types::{PmlLayer, SceneData};

/// Cells, along either axis, from which one application ([`CpuPml::begin`] and [`XShared::row`]) reads the field into
/// a correction: psi takes D1 (2 cells), `delta_y` and `O_x` take D1 of psi (2 more), and the stencil's taps of z~
/// reach at most 3.
pub const REACH: usize = 4;

/// Bits of a cell's cut byte: a mirror at -1, -2, +1, +2 along x, then along y.
pub mod cut {
	pub const X_SHIFT: u32 = 0;
	pub const Y_SHIFT: u32 = 4;
}

/// Which of the 9-point's diagonal taps (-x-y, +x-y, -x+y, +x+y) the cut byte drops: those passing
/// an axis neighbour that is a mirror.
#[inline]
pub fn diagonal_cuts(cut: u8) -> [bool; 4] {
	let (w, e, n, s) = (cut & 1 != 0, cut & 4 != 0, cut & 0x10 != 0, cut & 0x40 != 0);
	[w || n, e || n, w || s, e || s]
}

/// The rim's strips on a `gw` x `gh` grid with `rim` cells a side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strips {
	pub gw: usize,
	pub gh: usize,
	pub rim: usize,
}

impl Strips {
	pub fn of(scene: &SceneData) -> Self {
		Self { gw: scene.grid_width() as usize, gh: scene.grid_height() as usize, rim: scene.rim as usize }
	}

	/// Entries of the x strips.
	pub fn x_len(&self) -> usize {
		self.gh * 2 * self.rim
	}

	/// Entries of the y strips.
	pub fn y_len(&self) -> usize {
		2 * self.rim * self.gw
	}

	/// Entries of both strips.
	pub fn len(&self) -> usize {
		self.x_len() + self.y_len()
	}

	pub fn is_empty(&self) -> bool {
		self.rim == 0
	}

	pub fn x_index(&self, gx: usize, gy: usize) -> Option<usize> {
		let r = self.rim;
		if gx >= self.gw || gy >= self.gh {
			return None;
		}
		let col = if gx < r {
			gx
		} else if gx >= self.gw - r {
			gx + 2 * r - self.gw
		} else {
			return None;
		};
		Some(gy * 2 * r + col)
	}

	pub fn y_index(&self, gx: usize, gy: usize) -> Option<usize> {
		let r = self.rim;
		if gx >= self.gw || gy >= self.gh {
			return None;
		}
		let row = if gy < r {
			gy
		} else if gy >= self.gh - r {
			gy + 2 * r - self.gh
		} else {
			return None;
		};
		Some(self.x_len() + row * self.gw + gx)
	}

	/// Rim cells: the top and bottom rows, then the left and right columns of the rows between.
	pub fn rim_cells(&self) -> usize {
		2 * self.rim * (self.gw + self.gh - 2 * self.rim)
	}
}

/// Per-coordinate coefficients of one axis at sub-step `dt`: `[b, a, 1/kappa, d(1/kappa)/dcoordinate]`,
/// the identity `[1, 0, 1, 0]` outside the strips.
pub fn axis_table(profile: &[PmlLayer], len: usize, dt: f64) -> Vec<[f32; 4]> {
	let rim = profile.len();
	(0..len)
		.map(|g| {
			let (depth, outward) = if g < rim {
				(rim - g, -1.0)
			} else if g + rim >= len {
				(g + rim + 1 - len, 1.0)
			} else {
				return [1.0, 0.0, 1.0, 0.0];
			};
			let PmlLayer { sigma, kappa, alpha, inv_kappa_slope } = profile[depth - 1];
			let b = (-(sigma / kappa + alpha) * dt).exp();
			let denominator = sigma * kappa + kappa * kappa * alpha;
			let a = if denominator > 0.0 { sigma * (b - 1.0) / denominator } else { 0.0 };
			[b as f32, a as f32, (1.0 / kappa) as f32, (outward * inv_kappa_slope) as f32]
		})
		.collect()
}

/// The cut byte of every padded cell: which of the cells 1 and 2 away along each axis are mirrors.
/// A stencil tap beyond a mirror reads 0, as the mirror itself does (its c2 is 0), so a wave does not
/// reach across a mirror thinner than the stencil; the condition is symmetric between the two cells
/// of a tap, so the operator stays symmetric. Each region a mirror closes off then runs a principal
/// submatrix of the Laplacian, negative semi-definite as the whole is (docs/physics.md, Stability).
/// The 9-point's diagonals read the axis bits ([`diagonal_cuts`]).
pub fn cuts(scene: &SceneData) -> Vec<u8> {
	let (gw, gh, stride) = (scene.grid_width() as usize, scene.grid_height() as usize, scene.stride());
	let mut out = vec![0u8; scene.padded_cells()];
	let mirror = |gx: isize, gy: isize| {
		gx >= 0
			&& gy >= 0
			&& (gx as usize) < gw
			&& (gy as usize) < gh
			&& scene.c2[gy as usize * gw + gx as usize] <= super::types::MIRROR_C2
	};
	if !scene.c2.iter().any(|&c| c <= super::types::MIRROR_C2) {
		return out;
	}
	for gy in 0..gh as isize {
		for gx in 0..gw as isize {
			let offsets = [(-1, 0), (-2, 0), (1, 0), (2, 0), (0, -1), (0, -2), (0, 1), (0, 2)];
			let byte =
				offsets.iter().enumerate().fold(0u8, |b, (bit, &(dx, dy))| b | (mirror(gx + dx, gy + dy) as u8) << bit);
			out[(gy as usize + PAD) * stride + gx as usize + PAD] = byte;
		}
	}
	out
}

/// The seven values `u[i + k step]`, k = -3..=3, with the taps beyond a mirror dropped; `bits` the
/// cut byte shifted to the axis.
#[inline]
pub fn taps(u: &[f32], i: usize, step: usize, bits: u8) -> [f32; 7] {
	cut_taps([u[i - 3 * step], u[i - 2 * step], u[i - step], u[i], u[i + step], u[i + 2 * step], u[i + 3 * step]], bits)
}

/// `t` with the taps beyond a mirror dropped (`bits` as for [`taps`]).
#[inline]
pub fn cut_taps(t: [f32; 7], bits: u8) -> [f32; 7] {
	let keep = |cut: bool, v: f32| if cut { 0.0 } else { v };
	let (m1, m2, p1, p2) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
	[keep(m1 || m2, t[0]), keep(m1, t[1]), t[2], t[3], t[4], keep(p1, t[5]), keep(p1 || p2, t[6])]
}

/// Second derivative from `taps` with the weights a1..a3 (a0 = -2 (a1 + a2 + a3)), as differences
/// from the centre (engine/cpu.rs).
#[inline]
pub fn second(t: &[f32; 7], a: &[f32; 4]) -> f32 {
	let pair = |m: usize| (t[3 - m] - t[3]) + (t[3 + m] - t[3]);
	a[1] * pair(1) + a[2] * pair(2) + a[3] * pair(3)
}

/// 4th-order central first derivative from `taps`.
#[inline]
pub fn first(t: &[f32; 7]) -> f32 {
	(8.0 * (t[4] - t[2]) - (t[5] - t[1])) / 12.0
}

/// The CPU engine's CPML state: coefficient tables, and the memory variables of up to two
/// applications of the operator per sub-step.
pub struct CpuPml {
	strips: Strips,
	stride: usize,
	x: Columns,
	y: Vec<[f32; 4]>,
	axis: [f32; 4],
	cross: f32,
	psi: [Vec<f32>; 2],
	/// xi of x and y, then `delta_y`.
	xi: [Vec<f32>; 2],
	/// z~ of the current application per grid row, over the x strips' columns and 3 beyond each
	/// side: `2 (rim + 6)` values a row.
	z: Vec<f32>,
	zeros: Vec<f32>,
}

/// An axis's coefficients `[b, a, 1/kappa, (1/kappa)']` per coordinate ([`axis_table`]) as one column each, so the
/// x strips' loops read them as row slices and vectorise.
struct Columns([Vec<f32>; 4]);

impl Columns {
	fn of(table: &[[f32; 4]]) -> Self {
		Self(std::array::from_fn(|j| table.iter().map(|c| c[j]).collect()))
	}

	/// The `n` coordinates from `g0`.
	fn span(&self, g0: usize, n: usize) -> [&[f32]; 4] {
		self.0.each_ref().map(|c| &c[g0..][..n])
	}
}

/// The medium a pass runs over: the cut bytes and `c2` per padded cell, `c2` at the sub-step scale.
#[derive(Clone, Copy)]
pub struct Medium<'a> {
	pub cuts: &'a [u8],
	pub c2: &'a [f32],
}

/// Whether the columns `xs` meet the `rim` columns from `g0`.
fn meets(xs: &Range<usize>, g0: usize, rim: usize) -> bool {
	xs.start < g0 + rim && g0 < xs.end
}

/// The seven rows of `field` from 3 above to 3 below the row whose grid cells start at padded
/// index `at`, `gw` cells each.
fn column_rows(field: &[f32], at: usize, s: usize, gw: usize) -> [&[f32]; 7] {
	std::array::from_fn(|k| &field[(at as isize + (k as isize - 3) * s as isize) as usize..][..gw])
}

impl CpuPml {
	/// The state for `scene` at `n` sub-steps a step with the stencil's axis weights and cross
	/// weight; None without a rim.
	pub fn new(scene: &SceneData, n: u32, axis: [f32; 4], cross: f32) -> Option<Self> {
		let strips = Strips::of(scene);
		if strips.is_empty() {
			return None;
		}
		let dt = 1.0 / n as f64;
		let len = strips.len();
		let xi = len + strips.y_len();
		Some(Self {
			strips,
			stride: scene.stride(),
			x: Columns::of(&axis_table(&scene.pml, strips.gw, dt)),
			y: axis_table(&scene.pml, strips.gh, dt),
			axis,
			cross,
			psi: [vec![0.0; len], vec![0.0; len]],
			xi: [vec![0.0; xi], vec![0.0; xi]],
			z: vec![0.0; strips.gh * Self::z_width(strips.rim)],
			zeros: vec![0.0; strips.gw],
		})
	}

	/// The grid rows and columns from each edge whose field values one application ([`CpuPml::begin`] and
	/// [`XShared::row`]) reads: the rim and the 3 cells of the stencil's taps beyond it. The cells it writes lie within
	/// the rim.
	pub fn band(&self) -> usize {
		self.strips.rim + 3
	}

	pub fn reset(&mut self) {
		for b in self.psi.iter_mut().chain(&mut self.xi) {
			b.fill(0.0);
		}
	}

	/// Starts application `set` of the operator to `field` on `area`: advances psi of y, `delta_y` and xi of y over the
	/// y strips' rows of `area`, which read only `field`, and returns the rest, psi of x and the correction of row after
	/// row, which a pass runs on each row it writes ([`XPart`]); None when `area` is empty. `area` must hold every cell
	/// within [`REACH`] of a `field` value that is not +0 and every cell set `set` was applied on before; the cells
	/// outside would compute to the +0 they hold.
	pub fn begin<'a>(
		&'a mut self,
		set: usize,
		field: &'a [f32],
		medium: Medium<'a>,
		scale: f32,
		area: &Damage,
	) -> Option<XPart<'a>> {
		if area.is_empty() {
			return None;
		}
		self.psi_y(set, field, medium.cuts, area);
		self.delta_y(set, field, medium.cuts, area);
		let (tables, memory) = self.row_wise(set);
		let (rim, ys, zw) = (tables.strips.rim, area.y.clone(), tables.z_width());
		let shared = XShared { tables, delta: memory.delta, field, pass: XPass { xs: area.x.clone(), medium, scale } };
		Some(XPart {
			shared,
			psi: &mut memory.psi_x[ys.start * 2 * rim..ys.end * 2 * rim],
			xi: &mut memory.xi_x[ys.start * 2 * rim..ys.end * 2 * rim],
			z: &mut memory.z[ys.start * zw..ys.end * zw],
		})
	}

	/// Application `set`'s tables and memory variables, for an engine that runs all of its stages row by row: per y-strip
	/// row [`RimTables::psi_y_line`] then [`RimTables::delta_y_line`], per grid row [`RimTables::x_line`], in an order
	/// that gives each the inputs [`CpuPml::begin`] and [`XShared::row`] would (engine/rev2, Blocked steps).
	pub fn row_wise(&mut self, set: usize) -> (RimTables<'_>, RimMemory<'_>) {
		let Self { strips, stride, x, y, axis, cross, psi, xi, z, zeros } = self;
		let (x_len, len) = (strips.x_len(), strips.len());
		let (psi_x, psi_y) = psi[set].split_at_mut(x_len);
		let (xi_x, rest) = xi[set].split_at_mut(x_len);
		let (xi_y, delta) = rest.split_at_mut(len - x_len);
		let tables = RimTables { strips: *strips, stride: *stride, x, y, axis: *axis, cross: *cross, zeros };
		(tables, RimMemory { psi_y, xi_y, delta, psi_x, xi_x, z })
	}

	/// Both applications' tables and memory variables at once ([`row_wise`](Self::row_wise)), for an engine that
	/// interleaves their rows. z~ is filled and read within one call of [`RimTables::x_line`], so the two share it:
	/// the second's `z` is empty and its rows take the first's.
	pub fn row_wise_pair(&mut self) -> (RimTables<'_>, [RimMemory<'_>; 2]) {
		let Self { strips, stride, x, y, axis, cross, psi, xi, z, zeros } = self;
		let (x_len, len) = (strips.x_len(), strips.len());
		let tables = RimTables { strips: *strips, stride: *stride, x, y, axis: *axis, cross: *cross, zeros };
		let [p0, p1] = psi.each_mut();
		let [x0, x1] = xi.each_mut();
		let mut z = Some(z.as_mut_slice());
		let memory = [(p0, x0), (p1, x1)].map(|(psi, xi)| {
			let (psi_x, psi_y) = psi.split_at_mut(x_len);
			let (xi_x, rest) = xi.split_at_mut(x_len);
			let (xi_y, delta) = rest.split_at_mut(len - x_len);
			RimMemory { psi_y, xi_y, delta, psi_x, xi_x, z: z.take().unwrap_or_default() }
		});
		(tables, memory)
	}

	fn z_width(rim: usize) -> usize {
		2 * (rim + 6)
	}

	/// The grid row of y-strip row `r` (0..2 rim).
	fn y_row(strips: Strips, r: usize) -> usize {
		if r < strips.rim { r } else { strips.gh - 2 * strips.rim + r }
	}

	/// The y-strip row of grid row `gy`, if any.
	fn strip_row(strips: Strips, gy: usize) -> Option<usize> {
		let r = strips.rim;
		if gy < r {
			Some(gy)
		} else if gy >= strips.gh - r {
			Some(gy + 2 * r - strips.gh)
		} else {
			None
		}
	}

	/// The y-strip row `r + k` when it lies in the same strip (top or bottom) as y-strip row `r`.
	fn neighbour_row(rim: usize, r: usize, k: isize) -> Option<usize> {
		let q = r as isize + k;
		let (lo, hi) = if r < rim { (0, rim as isize) } else { (rim as isize, 2 * rim as isize) };
		(q >= lo && q < hi).then_some(q as usize)
	}

	/// Row `r + k` of `rows` (`width` values each, 2 rim of them) when it lies in the same strip
	/// (top or bottom, left or right) as `r`, else zeros.
	fn neighbour<'a>(rows: &'a [f32], zeros: &'a [f32], rim: usize, width: usize, r: usize, k: isize) -> &'a [f32] {
		match Self::neighbour_row(rim, r, k) {
			Some(q) => &rows[q * width..][..width],
			None => &zeros[..width],
		}
	}

	fn psi_y(&mut self, set: usize, field: &[f32], cuts: &[u8], area: &Damage) {
		let (tables, memory) = self.row_wise(set);
		let (strips, s, xs) = (tables.strips, tables.stride, &area.x);
		let rows = memory.psi_y.par_chunks_mut(strips.gw).enumerate();
		rows.filter(|(r, _)| area.y.contains(&Self::y_row(strips, *r))).for_each(|(r, line)| {
			let gy = Self::y_row(strips, r);
			tables.psi_y_line(gy, &field[(gy + PAD - 3) * s..][..7 * s], cuts, xs, line);
		});
	}

	fn delta_y(&mut self, set: usize, field: &[f32], cuts: &[u8], area: &Damage) {
		let (tables, memory) = self.row_wise(set);
		let (strips, s, xs) = (tables.strips, tables.stride, &area.x);
		let (gw, rim, psi) = (strips.gw, strips.rim, &*memory.psi_y);
		let rows = memory.xi_y.par_chunks_mut(gw).zip(memory.delta.par_chunks_mut(gw)).enumerate();
		rows.filter(|(r, _)| area.y.contains(&Self::y_row(strips, *r))).for_each(|(r, (xi, delta))| {
			let gy = Self::y_row(strips, r);
			let p = |k: isize| Self::neighbour(psi, tables.zeros, rim, gw, r, k);
			let field = &field[(gy + PAD - 3) * s..][..7 * s];
			tables.delta_y_line(gy, field, cuts, xs, [p(1), p(-1), p(2), p(-2)], xi, delta);
		});
	}
}

/// The tables and sizes every row of one application reads ([`CpuPml::row_wise`]).
#[derive(Clone, Copy)]
pub struct RimTables<'a> {
	strips: Strips,
	stride: usize,
	x: &'a Columns,
	y: &'a [[f32; 4]],
	axis: [f32; 4],
	cross: f32,
	zeros: &'a [f32],
}

/// The memory variables of one application ([`CpuPml::row_wise`]).
pub struct RimMemory<'a> {
	/// psi of y, xi of y and `delta_y`: `gw` values per y-strip row ([`RimTables::strip_row`]).
	pub psi_y: &'a mut [f32],
	pub xi_y: &'a mut [f32],
	pub delta: &'a mut [f32],
	/// psi and xi of x, `2 rim` values per grid row, and z~, [`RimTables::z_width`] values per grid row.
	pub psi_x: &'a mut [f32],
	pub xi_x: &'a mut [f32],
	pub z: &'a mut [f32],
}

/// The memory of one grid row for [`RimTables::x_line`]: its psi and xi of x (`2 rim` values) and z~
/// ([`RimTables::z_width`] values).
pub struct XLine<'a> {
	pub psi: &'a mut [f32],
	pub xi: &'a mut [f32],
	pub z: &'a mut [f32],
}

/// What a pass hands the rim's correction of its rows: the columns it runs on, the medium and the correction's scale.
#[derive(Clone)]
pub struct XPass<'a> {
	pub xs: Range<usize>,
	pub medium: Medium<'a>,
	pub scale: f32,
}

impl RimTables<'_> {
	/// The rim's width.
	pub fn rim(&self) -> usize {
		self.strips.rim
	}

	/// The values of z~ per grid row.
	pub fn z_width(&self) -> usize {
		CpuPml::z_width(self.strips.rim)
	}

	/// The y-strip row of grid row `gy`, if any.
	pub fn strip_row(&self, gy: usize) -> Option<usize> {
		CpuPml::strip_row(self.strips, gy)
	}

	/// The y-strip row `r + k` when it lies in the same strip as y-strip row `r`: the rows of psi of y whose D1
	/// [`delta_y_line`](Self::delta_y_line) takes, zeros elsewhere.
	pub fn neighbour_row(&self, r: usize, k: isize) -> Option<usize> {
		CpuPml::neighbour_row(self.strips.rim, r, k)
	}

	/// A row of zeros as long as a grid row.
	pub fn zeros(&self) -> &[f32] {
		self.zeros
	}

	pub fn scratch(&self) -> XScratch {
		XScratch { padded: vec![0.0; self.strips.rim + 4], plain: vec![0.0; self.strips.gw] }
	}

	/// Advances psi of y of y-strip grid row `gy` (`line`, its `gw` values) on the columns `xs`: `field` holds the 7
	/// padded rows from 3 above the row to 3 below it, `cuts` the whole grid's cut bytes.
	pub fn psi_y_line(&self, gy: usize, field: &[f32], cuts: &[u8], xs: &Range<usize>, line: &mut [f32]) {
		let (gw, s) = (self.strips.gw, self.stride);
		let [b, a, ..] = self.y[gy];
		let at = (gy + PAD) * s + PAD;
		let t = column_rows(field, 3 * s + PAD, s, gw).map(|row| &row[xs.clone()]);
		let (cut, line) = (&cuts[at..][xs.clone()], &mut line[xs.clone()]);
		if any_cut(cut, cut::Y_SHIFT) {
			psi_y_row::<true>(line, t, cut, b, a);
		} else {
			psi_y_row::<false>(line, t, cut, b, a);
		}
	}

	/// Advances xi of y and sets `delta_y` of y-strip grid row `gy` (`xi`, `delta`, `gw` values each) on the columns
	/// `xs`: `field` as for [`psi_y_line`](Self::psi_y_line), `psi` psi of y of the strip rows 1 after, 1 before, 2
	/// after and 2 before it, zeros beyond its strip ([`neighbour_row`](Self::neighbour_row)), each `gw` values.
	#[allow(clippy::too_many_arguments)]
	pub fn delta_y_line(
		&self,
		gy: usize,
		field: &[f32],
		cuts: &[u8],
		xs: &Range<usize>,
		psi: [&[f32]; 4],
		xi: &mut [f32],
		delta: &mut [f32],
	) {
		let (gw, s) = (self.strips.gw, self.stride);
		let at = (gy + PAD) * s + PAD;
		let row = YRow {
			t: column_rows(field, 3 * s + PAD, s, gw).map(|row| &row[xs.clone()]),
			psi: psi.map(|p| &p[xs.clone()]),
			cut: &cuts[at..][xs.clone()],
		};
		let (xi, delta) = (&mut xi[xs.clone()], &mut delta[xs.clone()]);
		if any_cut(row.cut, cut::Y_SHIFT) {
			delta_y_row::<true>(&row, &self.axis, self.y[gy], xi, delta);
		} else {
			delta_y_row::<false>(&row, &self.axis, self.y[gy], xi, delta);
		}
	}

	/// Fills z~ `= u + chi (D3y u + delta_y)` of the x strips' columns and 3 beyond of grid row `gy` and advances psi
	/// of x on it, for each strip the pass's columns meet; then adds the correction times `scale c2` to the row's grid
	/// cells `line` and advances their xi: the x strips' cells, and on a y-strip row the cells of the pass's columns
	/// between them. `field` holds the 3 padded rows from the one above the row, `strip` the row's `delta_y` on a
	/// y-strip row, `memory` the row's own.
	#[cfg_attr(target_arch = "x86_64", inline(always))]
	#[allow(clippy::too_many_arguments)]
	pub fn x_line(
		&self,
		gy: usize,
		field: &[f32],
		strip: Option<&[f32]>,
		memory: XLine<'_>,
		line: &mut [f32],
		scratch: &mut XScratch,
		pass: &XPass<'_>,
	) {
		let Strips { gw, rim, .. } = self.strips;
		let XPass { xs, medium: Medium { cuts, c2 }, scale } = pass;
		let (s, scale, XLine { psi, xi: xi_x, z }) = (self.stride, *scale, memory);
		let (right_start, base, fbase) = (gw - rim, (gy + PAD) * s + PAD, s + PAD);
		let sides = [meets(xs, 0, rim), meets(xs, right_start, rim)];
		let (left, right) = z.split_at_mut(rim + 6);
		let (pl, pr) = psi.split_at_mut(rim);
		for (on, span, zs, g0) in [(sides[0], pl, left, 0), (sides[1], pr, right, right_start)] {
			if !on {
				continue;
			}
			let d = strip.unwrap_or(&self.zeros[..gw]);
			z_span(zs, field, fbase, s, self.cross, d, g0 as isize - 3);
			let (cut, x, zs) = (&cuts[base + g0..][..rim], self.x.span(g0, rim), &*zs);
			if any_cut(cut, cut::X_SHIFT) {
				psi_x_span::<true>(span, zs, cut, x);
			} else {
				psi_x_span::<false>(span, zs, cut, x);
			}
		}
		let XScratch { padded, plain } = scratch;
		let c2 = &c2[base..][..gw];
		let ((zl, zr), (pl, pr)) = (z.split_at(rim + 6), psi.split_at(rim));
		let (xl, xr) = xi_x.split_at_mut(rim);
		let [left, right] = [(0, zl, pl), (right_start, zr, pr)].map(|(g0, z, psi)| XSpan {
			z,
			psi,
			cut: &cuts[base + g0..][..rim],
			x: self.x.span(g0, rim),
			c2: &c2[g0..][..rim],
		});
		let (line_l, rest) = line.split_at_mut(rim);
		let (line_mid, line_r) = rest.split_at_mut(right_start - rim);
		// The columns of `xs` between the x strips, empty when it lies within one.
		let mid_start = xs.start.max(rim);
		let mid = mid_start..xs.end.min(right_start).max(mid_start);
		match strip {
			Some(d) => {
				let (lc, rc) = (0..rim, right_start..gw);
				for (on, cols) in [(sides[0], lc.clone()), (true, mid.clone()), (sides[1], rc.clone())] {
					if on {
						plain_cells(d, self.cross, cols, plain);
					}
				}
				if sides[0] {
					self.correct_span(&left, xl, line_l, padded, scale, &plain[lc]);
				}
				if sides[1] {
					self.correct_span(&right, xr, line_r, padded, scale, &plain[rc]);
				}
				let cells = &mut line_mid[mid.start - rim..mid.end - rim];
				for ((v, &c), &p) in cells.iter_mut().zip(&c2[mid.clone()]).zip(&plain[mid.clone()]) {
					*v += scale * c * p;
				}
			}
			None => {
				let zeros = &self.zeros[..rim];
				if sides[0] {
					self.correct_span(&left, xl, line_l, padded, scale, zeros);
				}
				if sides[1] {
					self.correct_span(&right, xr, line_r, padded, scale, zeros);
				}
			}
		}
	}

	/// Adds `scale c2 (init + O_x z~ - D2x z~)` to the cells `line` of one x strip of a row and advances their
	/// xi; `padded` holds at least the strip's psi and two zeros each side.
	#[cfg_attr(target_arch = "x86_64", inline(always))]
	fn correct_span(
		&self,
		span: &XSpan<'_>,
		xi: &mut [f32],
		line: &mut [f32],
		padded: &mut [f32],
		scale: f32,
		init: &[f32],
	) {
		if any_cut(span.cut, cut::X_SHIFT) {
			self.correct_cells::<true>(span, xi, line, padded, scale, init);
		} else {
			self.correct_cells::<false>(span, xi, line, padded, scale, init);
		}
	}

	/// [`correct_span`](Self::correct_span) on row slices: the taps of z~ and of psi are shifted slices, so the
	/// loop has no bounds checks and vectorises without mirrors.
	#[inline(always)]
	fn correct_cells<const CUT: bool>(
		&self,
		span: &XSpan<'_>,
		xi: &mut [f32],
		line: &mut [f32],
		padded: &mut [f32],
		scale: f32,
		init: &[f32],
	) {
		isa::wide(
			#[cfg_attr(target_arch = "x86_64", inline(always))]
			|| {
				let n = line.len();
				let z: [&[f32]; 7] = std::array::from_fn(|j| &span.z[j..][..n]);
				let (cut, [b, a, ik, dik], c2) = (&span.cut[..n], span.x.map(|c| &c[..n]), &span.c2[..n]);
				let (xi, init) = (&mut xi[..n], &init[..n]);
				let padded = &mut padded[..n + 4];
				padded[2..n + 2].copy_from_slice(&span.psi[..n]);
				let [p0, p1, p3, p4] = [0, 1, 3, 4].map(|j| &padded[j..][..n]);
				for k in 0..n {
					let dpsi = (8.0 * (p3[k] - p1[k]) - (p4[k] - p0[k])) / 12.0;
					let taps = maybe_cut::<CUT>(z.map(|r| r[k]), cut[k] >> cut::X_SHIFT);
					let sum = init[k] + stretched(&taps, &self.axis, [b[k], a[k], ik[k], dik[k]], dpsi, &mut xi[k]);
					line[k] += scale * c2[k] * sum;
				}
			},
		);
	}
}

/// What [`CpuPml::begin`] leaves of an application: psi of x and the correction, row by row. They read the field,
/// `delta_y` and the row's own memory, so a pass runs them on each row right after writing it ([`XShared::row`]).
pub struct XPart<'a> {
	shared: XShared<'a>,
	/// psi and xi of x of the area's rows, `2 rim` values a row, and their z~.
	psi: &'a mut [f32],
	xi: &'a mut [f32],
	z: &'a mut [f32],
}

impl<'a> XPart<'a> {
	/// The state the rows share, and the memory of `rows` grid rows at a time from the area's first row.
	pub fn chunks(&mut self, rows: usize) -> (&XShared<'a>, impl IndexedParallelIterator<Item = XRows<'_>>) {
		let (w, zw) = (2 * self.shared.tables.strips.rim, self.shared.tables.z_width());
		let chunks = self.psi.par_chunks_mut(rows * w).zip(self.xi.par_chunks_mut(rows * w));
		let chunks =
			chunks.zip(self.z.par_chunks_mut(rows * zw)).map(move |((psi, xi), z)| XRows { psi, xi, z, w, zw });
		(&self.shared, chunks)
	}
}

/// The memory of a block of rows of an [`XPart`].
pub struct XRows<'b> {
	psi: &'b mut [f32],
	xi: &'b mut [f32],
	z: &'b mut [f32],
	w: usize,
	zw: usize,
}

/// A thread's scratch rows for [`RimTables::x_line`]: a strip's psi with two zeros each side, and a row of
/// `delta_y + chi D2x delta_y`.
pub struct XScratch {
	padded: Vec<f32>,
	plain: Vec<f32>,
}

/// The state every row of an [`XPart`] reads.
pub struct XShared<'a> {
	tables: RimTables<'a>,
	/// `delta_y` of the y strips' rows.
	delta: &'a [f32],
	field: &'a [f32],
	pass: XPass<'a>,
}

impl XShared<'_> {
	pub fn scratch(&self) -> XScratch {
		self.tables.scratch()
	}

	/// [`RimTables::x_line`] on grid row `gy`, row `j` of `rows`.
	#[cfg_attr(target_arch = "x86_64", inline(always))]
	pub fn row(&self, gy: usize, rows: &mut XRows<'_>, j: usize, line: &mut [f32], scratch: &mut XScratch) {
		let (s, gw, w, zw) = (self.tables.stride, self.tables.strips.gw, rows.w, rows.zw);
		let strip = self.tables.strip_row(gy).map(|r| &self.delta[r * gw..][..gw]);
		let memory =
			XLine { psi: &mut rows.psi[j * w..][..w], xi: &mut rows.xi[j * w..][..w], z: &mut rows.z[j * zw..][..zw] };
		let field = &self.field[(gy + PAD - 1) * s..][..3 * s];
		self.tables.x_line(gy, field, strip, memory, line, scratch, &self.pass);
	}
}

/// One x strip of a grid row: z~ from 3 before its first cell, its psi, cut bytes, x coefficients and c2.
struct XSpan<'a> {
	z: &'a [f32],
	psi: &'a [f32],
	cut: &'a [u8],
	x: [&'a [f32]; 4],
	c2: &'a [f32],
}

/// `delta_y + chi D2x delta_y` of the columns `cols` of a y-strip row into `out` (indexed by column), `d` its
/// `delta_y`, 0 beyond the grid. The cells between the grid's edges run on row slices and vectorise; each cell is
/// the same expression in the same order either way.
fn plain_cells(d: &[f32], cross: f32, cols: Range<usize>, out: &mut [f32]) {
	isa::wide(
		#[cfg_attr(target_arch = "x86_64", inline(always))]
		|| {
			if cross == 0.0 {
				out[cols.clone()].copy_from_slice(&d[cols]);
				return;
			}
			let gw = d.len();
			let edge = |gx: usize| {
				let left = if gx > 0 { d[gx - 1] } else { 0.0 };
				let right = if gx + 1 < gw { d[gx + 1] } else { 0.0 };
				d[gx] + cross * (left + right - 2.0 * d[gx])
			};
			let inner_start = cols.start.max(1).min(cols.end);
			let inner = inner_start..cols.end.min(gw - 1).max(inner_start);
			for gx in (cols.start..inner.start).chain(inner.end..cols.end) {
				out[gx] = edge(gx);
			}
			let n = inner.len();
			let (l, c, r) = (&d[inner.start - 1..][..n], &d[inner.clone()], &d[inner.start + 1..][..n]);
			for (o, ((&l, &c), &r)) in out[inner].iter_mut().zip(l.iter().zip(c).zip(r)) {
				*o = c + cross * (l + r - 2.0 * c);
			}
		},
	);
}

/// One row of the y strips: the field's seven rows around it, psi of the rows 1 and 2 after and before
/// (`[+1, -1, +2, -2]`), and the cut bytes.
struct YRow<'a> {
	t: [&'a [f32]; 7],
	psi: [&'a [f32]; 4],
	cut: &'a [u8],
}

/// Whether any of `cuts` drops a tap along the axis at `shift`.
fn any_cut(cuts: &[u8], shift: u32) -> bool {
	cuts.iter().any(|&c| (c >> shift) & 0xf != 0)
}

/// `t` with the taps beyond a mirror dropped when `CUT`; a span without mirrors skips the cut bytes.
#[inline(always)]
fn maybe_cut<const CUT: bool>(t: [f32; 7], bits: u8) -> [f32; 7] {
	if CUT { cut_taps(t, bits) } else { t }
}

#[inline(always)]
fn psi_y_row<const CUT: bool>(line: &mut [f32], t: [&[f32]; 7], cut: &[u8], b: f32, a: f32) {
	isa::wide(
		#[cfg_attr(target_arch = "x86_64", inline(always))]
		|| {
			let gw = line.len();
			let (t, cut) = (t.map(|row| &row[..gw]), &cut[..gw]);
			for x in 0..gw {
				let taps = maybe_cut::<CUT>(t.map(|row| row[x]), cut[x] >> cut::Y_SHIFT);
				line[x] = b * line[x] + a * first(&taps);
			}
		},
	);
}

#[inline(always)]
fn delta_y_row<const CUT: bool>(row: &YRow<'_>, axis: &[f32; 4], coef: [f32; 4], xi: &mut [f32], delta: &mut [f32]) {
	isa::wide(
		#[cfg_attr(target_arch = "x86_64", inline(always))]
		|| {
			let gw = xi.len();
			let (t, [p1, m1, p2, m2]) = (row.t.map(|r| &r[..gw]), row.psi.map(|r| &r[..gw]));
			let (cut, delta) = (&row.cut[..gw], &mut delta[..gw]);
			for x in 0..gw {
				let taps = maybe_cut::<CUT>(t.map(|r| r[x]), cut[x] >> cut::Y_SHIFT);
				let dpsi = (8.0 * (p1[x] - m1[x]) - (p2[x] - m2[x])) / 12.0;
				delta[x] = stretched(&taps, axis, coef, dpsi, &mut xi[x]);
			}
		},
	);
}

/// z~ of the `out.len()` cells from grid column `g0` of the row whose grid cells start at padded index `base`;
/// `d` is the row's `delta_y` (zeros outside the y strips), 0 beyond the grid.
fn z_span(out: &mut [f32], field: &[f32], base: usize, s: usize, cross: f32, d: &[f32], g0: isize) {
	isa::wide(
		#[cfg_attr(target_arch = "x86_64", inline(always))]
		|| {
			let (n, gw) = (out.len(), d.len());
			let j0 = base.wrapping_add_signed(g0);
			let f = &field[j0..][..n];
			if cross == 0.0 {
				out.copy_from_slice(f);
				return;
			}
			let (up, down) = (&field[j0 - s..][..n], &field[j0 + s..][..n]);
			let cell = |k: usize, dk: f32| {
				let v = f[k];
				v + cross * ((up[k] + down[k] - 2.0 * v) + dk)
			};
			// The cells within the grid, `lo..hi`, take `d` on row slices, so that loop vectorises; the at most 3
			// beyond each side take 0.
			let lo = (-g0).clamp(0, n as isize) as usize;
			let hi = (gw as isize - g0).clamp(lo as isize, n as isize) as usize;
			for k in (0..lo).chain(hi..n) {
				out[k] = cell(k, 0.0);
			}
			let d = &d[(g0 + lo as isize) as usize..][..hi - lo];
			let (f, up, down) = (&f[lo..hi], &up[lo..hi], &down[lo..hi]);
			for (o, (((&v, &u), &w), &dk)) in out[lo..hi].iter_mut().zip(f.iter().zip(up).zip(down).zip(d)) {
				*o = v + cross * ((u + w - 2.0 * v) + dk);
			}
		},
	);
}

/// Advances psi of x of one strip's cells on row slices: the taps of `z` (z~ from 3 before the first cell) are shifted
/// slices and `x` the coefficient columns, so the loop vectorises without mirrors.
#[inline(always)]
fn psi_x_span<const CUT: bool>(psi: &mut [f32], z: &[f32], cut: &[u8], x: [&[f32]; 4]) {
	isa::wide(
		#[cfg_attr(target_arch = "x86_64", inline(always))]
		|| {
			let n = psi.len();
			let z: [&[f32]; 7] = std::array::from_fn(|j| &z[j..][..n]);
			let (cut, [b, a, ..]) = (&cut[..n], x.map(|c| &c[..n]));
			for k in 0..n {
				let taps = maybe_cut::<CUT>(z.map(|r| r[k]), cut[k] >> cut::X_SHIFT);
				psi[k] = b[k] * psi[k] + a[k] * first(&taps);
			}
		},
	);
}

/// `u_x~x~ - D2 u` at one cell from its taps, the axis's coefficients `[b, a, 1/kappa, (1/kappa)']`,
/// `D1 psi`, and its `xi`, which it advances.
#[inline]
pub fn stretched(t: &[f32; 7], axis: &[f32; 4], [b, a, ik, dik]: [f32; 4], dpsi: f32, xi: &mut f32) -> f32 {
	let d2 = second(t, axis);
	let q = ik * d2 + dik * first(t) + dpsi;
	*xi = b * *xi + a * q;
	ik * q + *xi - d2
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Scene;

	#[test]
	fn strips_index_every_rim_cell_once() {
		let scene = SceneData::blank(9, 5, &Scene { damping_edge: -3, ..Scene::default() });
		let st = Strips::of(&scene);
		let mut seen = vec![0; st.len()];
		let mut rim = 0;
		for gy in 0..st.gh {
			for gx in 0..st.gw {
				let (x, y) = (st.x_index(gx, gy), st.y_index(gx, gy));
				assert_eq!(x.is_some() || y.is_some(), gx < 3 || gy < 3 || gx >= 12 || gy >= 8);
				rim += (x.is_some() || y.is_some()) as usize;
				for k in x.into_iter().chain(y) {
					seen[k] += 1;
				}
			}
		}
		assert!(seen.iter().all(|&n| n == 1));
		assert_eq!(rim, st.rim_cells());
	}

	#[test]
	fn tables_are_the_identity_inside_and_mirror_outside() {
		let profile = crate::scene::scene_data::PmlParams::default().profile(4, 1.0);
		let t = axis_table(&profile, 12, 0.5);
		assert_eq!(t[4], [1.0, 0.0, 1.0, 0.0]);
		assert_eq!(t[7], [1.0, 0.0, 1.0, 0.0]);
		assert_eq!(t[0][..3], t[11][..3]);
		assert_eq!(t[0][3], -t[11][3]);
		assert!(t[0][0] < t[3][0] && t[0][1] < 0.0);
	}
}
