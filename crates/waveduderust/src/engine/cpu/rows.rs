//! The row helpers of the CPU engine's passes: the rows a stencil reads, the per-cell stencils of the cut cells, the
//! runs of a row's cuts and of its uniform medium, and the per-row chunks of an optional plane.

use std::ops::Range;

use rayon::iter::Either;
use rayon::prelude::*;

use super::super::pml::{self, cut, cut_taps};
use super::super::stencil::{K9_AXIS, K9_DIAGONAL, PAD};

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

/// The fewest cells of a uniform run (engine/cpu, Uniform medium); shorter runs read `c2` and `d` per cell.
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

/// The 9-point Laplacian at padded cell `i` (row length `s`) with the cut byte `cut`: a diagonal tap
/// is dropped when either axis neighbour it passes is a mirror, so no wave crosses a mirror line
/// that is only 8-connected; the rule is symmetric between the two cells of a tap. Same order of
/// operations as the kernels' uncut cells.
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
/// DRP Laplacian of the centre row reads, from a plane or from a ring of `w` rows.
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
/// cut byte). A row crossed by a mirror has only a few cut cells near it; the runs between go to the kernels.
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

#[cfg(test)]
mod tests {
	use super::*;

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
